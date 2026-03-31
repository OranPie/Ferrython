//! The main virtual machine — executes bytecode instructions.

use crate::builtins;
use crate::frame::{BlockKind, Frame, ScopeKind};
use compact_str::CompactString;
use ferrython_bytecode::code::{CodeFlags, CodeObject, ConstantValue};
use ferrython_bytecode::opcode::Opcode;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    CompareOp, GeneratorState, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, PyFunction, SharedGlobals};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

/// The Ferrython virtual machine.
pub struct VirtualMachine {
    call_stack: Vec<Frame>,
    builtins: IndexMap<CompactString, PyObjectRef>,
    modules: IndexMap<CompactString, PyObjectRef>,
}

impl VirtualMachine {
    pub fn new() -> Self {
        Self {
            call_stack: Vec::new(),
            builtins: builtins::init_builtins(),
            modules: IndexMap::new(),
        }
    }

    /// Execute a code object (module-level).
    pub fn execute(&mut self, code: CodeObject) -> PyResult<PyObjectRef> {
        let globals = Arc::new(RwLock::new(IndexMap::new()));
        let frame = Frame::new(code, globals, self.builtins.clone());
        self.call_stack.push(frame);
        let result = self.run_frame();
        self.call_stack.pop();
        result
    }

    /// Execute a code object as a function call with arguments.
    fn call_function(
        &mut self,
        code: &CodeObject,
        args: Vec<PyObjectRef>,
        defaults: &[PyObjectRef],
        globals: SharedGlobals,
        closure: &[Arc<RwLock<Option<PyObjectRef>>>],
    ) -> PyResult<PyObjectRef> {
        let mut frame = Frame::new(code.clone(), globals, self.builtins.clone());
        let nparams = code.arg_count as usize;
        let has_varargs = code.flags.contains(CodeFlags::VARARGS);
        let has_varkw = code.flags.contains(CodeFlags::VARKEYWORDS);

        // Assign positional parameters
        let positional_count = args.len().min(nparams);
        for i in 0..positional_count {
            frame.set_local(i, args[i].clone());
        }

        // Fill in defaults for missing positional args
        if args.len() < nparams && !defaults.is_empty() {
            let ndefaults = defaults.len();
            let first_default_param = nparams - ndefaults;
            for i in args.len()..nparams {
                if i >= first_default_param {
                    let default_idx = i - first_default_param;
                    frame.set_local(i, defaults[default_idx].clone());
                }
            }
        }

        // Pack extra positional args into *args tuple
        if has_varargs {
            let extra: Vec<PyObjectRef> = if args.len() > nparams {
                args[nparams..].to_vec()
            } else {
                Vec::new()
            };
            frame.set_local(nparams, PyObject::tuple(extra));
        }

        // Pack **kwargs into a dict
        if has_varkw {
            let kwargs_idx = nparams + if has_varargs { 1 } else { 0 };
            frame.set_local(kwargs_idx, PyObject::dict(IndexMap::new()));
        }

        // Install closure cells as free vars in this frame.
        let n_cell = code.cellvars.len();
        for (i, cell) in closure.iter().enumerate() {
            if n_cell + i < frame.cells.len() {
                frame.cells[n_cell + i] = cell.clone();
            }
        }
        // For cell vars that are also parameters, copy the parameter value into the cell
        for (cell_idx, cell_name) in code.cellvars.iter().enumerate() {
            for (var_idx, var_name) in code.varnames.iter().enumerate() {
                if cell_name == var_name {
                    if let Some(val) = frame.locals[var_idx].take() {
                        *frame.cells[cell_idx].write() = Some(val);
                    }
                    break;
                }
            }
        }
        frame.scope_kind = ScopeKind::Function;

        // If the function is a generator, return a Generator object without executing
        if code.flags.contains(CodeFlags::GENERATOR) {
            let name = CompactString::from(code.name.as_str());
            return Ok(PyObject::generator(name, Box::new(frame)));
        }

        self.call_stack.push(frame);
        let result = self.run_frame();
        self.call_stack.pop();
        result
    }

    fn call_function_kw(
        &mut self,
        code: &CodeObject,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
        defaults: &[PyObjectRef],
        globals: SharedGlobals,
        closure: &[Arc<RwLock<Option<PyObjectRef>>>],
    ) -> PyResult<PyObjectRef> {
        let mut frame = Frame::new(code.clone(), globals, self.builtins.clone());
        let nparams = code.arg_count as usize;
        let has_varargs = code.flags.contains(CodeFlags::VARARGS);
        let has_varkw = code.flags.contains(CodeFlags::VARKEYWORDS);

        // Assign positional parameters
        let positional_count = pos_args.len().min(nparams);
        for i in 0..positional_count {
            frame.set_local(i, pos_args[i].clone());
        }

        // Place keyword args at their correct parameter positions
        let mut extra_kwargs: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
        for (name, val) in &kwargs {
            if let Some(idx) = code.varnames.iter().position(|v| v.as_str() == name.as_str()) {
                if idx < nparams {
                    frame.set_local(idx, val.clone());
                    continue;
                }
            }
            // Not a known parameter — goes into **kwargs
            extra_kwargs.insert(
                HashableKey::Str(name.clone()),
                val.clone(),
            );
        }

        // Fill in defaults for missing positional args
        if !defaults.is_empty() {
            let ndefaults = defaults.len();
            let first_default_param = nparams - ndefaults;
            for i in 0..nparams {
                if frame.locals[i].is_none() && i >= first_default_param {
                    let default_idx = i - first_default_param;
                    frame.set_local(i, defaults[default_idx].clone());
                }
            }
        }

        // Pack extra positional args into *args tuple
        if has_varargs {
            let extra: Vec<PyObjectRef> = if pos_args.len() > nparams {
                pos_args[nparams..].to_vec()
            } else {
                Vec::new()
            };
            frame.set_local(nparams, PyObject::tuple(extra));
        }

        // Pack **kwargs into a dict
        if has_varkw {
            let kwargs_idx = nparams + if has_varargs { 1 } else { 0 };
            frame.set_local(kwargs_idx, PyObject::dict(extra_kwargs));
        }

        // Install closure cells
        let n_cell = code.cellvars.len();
        for (i, cell) in closure.iter().enumerate() {
            if n_cell + i < frame.cells.len() {
                frame.cells[n_cell + i] = cell.clone();
            }
        }
        for (cell_idx, cell_name) in code.cellvars.iter().enumerate() {
            for (var_idx, var_name) in code.varnames.iter().enumerate() {
                if cell_name == var_name {
                    if let Some(val) = frame.locals[var_idx].take() {
                        *frame.cells[cell_idx].write() = Some(val);
                    }
                    break;
                }
            }
        }
        frame.scope_kind = ScopeKind::Function;

        // If the function is a generator, return a Generator object without executing
        if code.flags.contains(CodeFlags::GENERATOR) {
            let name = CompactString::from(code.name.as_str());
            return Ok(PyObject::generator(name, Box::new(frame)));
        }

        self.call_stack.push(frame);
        let result = self.run_frame();
        self.call_stack.pop();
        result
    }

    fn call_object_kw(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        match &func.payload {
            PyObjectPayload::Function(pyfunc) => {
                let code = pyfunc.code.clone();
                let globals = pyfunc.globals.clone();
                let defaults = pyfunc.defaults.clone();
                let closure = pyfunc.closure.clone();
                self.call_function_kw(&code, pos_args, kwargs, &defaults, globals, &closure)
            }
            PyObjectPayload::BoundMethod { receiver, method } => {
                let mut bound_args = vec![receiver.clone()];
                bound_args.extend(pos_args);
                self.call_object_kw(method.clone(), bound_args, kwargs)
            }
            PyObjectPayload::Class(_class) => {
                let instance = PyObject::instance(func.clone());
                if let Some(init) = func.get_attr("__init__") {
                    let init_fn = match &init.payload {
                        PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                        _ => init.clone(),
                    };
                    let mut init_args = vec![instance.clone()];
                    init_args.extend(pos_args);
                    self.call_object_kw(init_fn, init_args, kwargs)?;
                }
                Ok(instance)
            }
            _ => {
                // Fall back to call_object for builtins etc (kwargs ignored)
                let mut all_args = pos_args;
                for (_, v) in kwargs {
                    all_args.push(v);
                }
                self.call_object(func, all_args)
            }
        }
    }
    fn run_frame(&mut self) -> PyResult<PyObjectRef> {
        loop {
            let frame = self.call_stack.last().unwrap();
            if frame.ip >= frame.code.instructions.len() {
                return Ok(PyObject::none());
            }

            let instr = frame.code.instructions[frame.ip];
            let frame = self.call_stack.last_mut().unwrap();
            frame.ip += 1;

            match self.execute_one(instr) {
                Ok(Some(ret)) => return Ok(ret),
                Ok(None) => {}
                Err(exc) => {
                    if let Some(handler_ip) = self.unwind_except() {
                        let frame = self.call_stack.last_mut().unwrap();
                        // CPython pushes (traceback, value, type) — 3 items
                        let exc_value = PyObject::exception_instance(exc.kind.clone(), exc.message.clone());
                        let exc_type = PyObject::exception_type(exc.kind.clone());
                        frame.push(PyObject::none());     // traceback
                        frame.push(exc_value);            // value
                        frame.push(exc_type);             // type
                        frame.ip = handler_ip;
                    } else {
                        return Err(exc);
                    }
                }
            }
        }
    }

    /// Find an exception handler on the block stack. Returns handler IP if found.
    fn unwind_except(&mut self) -> Option<usize> {
        let frame = self.call_stack.last_mut()?;
        while let Some(block) = frame.pop_block() {
            match block.kind {
                BlockKind::Except | BlockKind::Finally => {
                    // Unwind value stack to block level
                    while frame.stack.len() > block.stack_level {
                        frame.pop();
                    }
                    // Push an ExceptHandler block so PopExcept can find it
                    frame.push_block(BlockKind::ExceptHandler, 0);
                    return Some(block.handler);
                }
                BlockKind::ExceptHandler => {
                    // Clean up a previous except handler (exception in except body)
                    while frame.stack.len() > block.stack_level {
                        frame.pop();
                    }
                    continue;
                }
                BlockKind::Loop => {
                    while frame.stack.len() > block.stack_level {
                        frame.pop();
                    }
                    continue;
                }
                BlockKind::With => {
                    // With block exception — jump to cleanup handler which will
                    // call __exit__ with exception info
                    while frame.stack.len() > block.stack_level {
                        frame.pop();
                    }
                    return Some(block.handler);
                }
            }
        }
        None
    }

    fn execute_one(&mut self, instr: ferrython_bytecode::Instruction) -> Result<Option<PyObjectRef>, PyException> {
        let frame = self.call_stack.last_mut().unwrap();
        
        match instr.op {
                Opcode::Nop => {}

                // ── Stack operations ──
                Opcode::PopTop => { frame.pop(); }
                Opcode::RotTwo => {
                    let a = frame.pop();
                    let b = frame.pop();
                    frame.push(a);
                    frame.push(b);
                }
                Opcode::RotThree => {
                    let a = frame.pop();
                    let b = frame.pop();
                    let c = frame.pop();
                    frame.push(a);
                    frame.push(c);
                    frame.push(b);
                }
                Opcode::DupTop => {
                    let v = frame.peek().clone();
                    frame.push(v);
                }

                // ── Load/Store ──
                Opcode::LoadConst => {
                    let idx = instr.arg as usize;
                    let constant = &frame.code.constants[idx];
                    let obj = constant_to_object(constant);
                    frame.push(obj);
                }
                Opcode::LoadName => {
                    let name = &frame.code.names[instr.arg as usize];
                    match frame.load_name(name) {
                        Some(v) => frame.push(v),
                        None => return Err(PyException::name_error(format!(
                            "name '{}' is not defined", name
                        ))),
                    }
                }
                Opcode::StoreName => {
                    let name = frame.code.names[instr.arg as usize].clone();
                    let value = frame.pop();
                    match frame.scope_kind {
                        ScopeKind::Module => {
                            // Module level: locals == globals
                            frame.globals.write().insert(name, value);
                        }
                        ScopeKind::Class => {
                            // Class body: write to local_names (the class namespace)
                            frame.local_names.insert(name, value);
                        }
                        ScopeKind::Function => {
                            // Shouldn't normally get STORE_NAME in function scope
                            // (compiler uses STORE_FAST), but handle it anyway
                            frame.local_names.insert(name, value);
                        }
                    }
                }
                Opcode::DeleteName => {
                    let name = &frame.code.names[instr.arg as usize];
                    frame.local_names.shift_remove(name.as_str());
                    frame.globals.write().shift_remove(name.as_str());
                }
                Opcode::LoadFast => {
                    let idx = instr.arg as usize;
                    match frame.get_local(idx) {
                        Some(v) => {
                            let v = v.clone();
                            frame.push(v);
                        }
                        None => {
                            let name = &frame.code.varnames[idx];
                            return Err(PyException::name_error(format!(
                                "local variable '{}' referenced before assignment", name
                            )));
                        }
                    }
                }
                Opcode::StoreFast => {
                    let value = frame.pop();
                    frame.set_local(instr.arg as usize, value);
                }
                Opcode::LoadDeref => {
                    let idx = instr.arg as usize;
                    let val = frame.cells[idx].read().clone();
                    match val {
                        Some(v) => {
                            frame.push(v);
                        }
                        None => {
                            let n_cell = frame.code.cellvars.len();
                            let name = if idx < n_cell {
                                frame.code.cellvars[idx].clone()
                            } else {
                                frame.code.freevars[idx - n_cell].clone()
                            };
                            return Err(PyException::name_error(format!(
                                "free variable '{}' referenced before assignment in enclosing scope", name
                            )));
                        }
                    }
                }
                Opcode::StoreDeref => {
                    let value = frame.pop();
                    let idx = instr.arg as usize;
                    *frame.cells[idx].write() = Some(value);
                }
                Opcode::LoadClosure => {
                    // Push the cell itself (as a Cell object) onto the stack
                    let idx = instr.arg as usize;
                    let cell = frame.cells[idx].clone();
                    frame.push(PyObject::cell(cell));
                }
                Opcode::LoadGlobal => {
                    let name = &frame.code.names[instr.arg as usize];
                    let from_globals = frame.globals.read().get(name.as_str()).cloned();
                    if let Some(v) = from_globals {
                        frame.push(v);
                    } else if let Some(v) = frame.builtins.get(name.as_str()) {
                        let v = v.clone();
                        frame.push(v);
                    } else {
                        return Err(PyException::name_error(format!(
                            "name '{}' is not defined", name
                        )));
                    }
                }
                Opcode::StoreGlobal => {
                    let name = frame.code.names[instr.arg as usize].clone();
                    let value = frame.pop();
                    frame.globals.write().insert(name, value);
                }
                Opcode::DeleteFast => {
                    let idx = instr.arg as usize;
                    frame.locals[idx] = None;
                }
                Opcode::DeleteGlobal => {
                    let name = &frame.code.names[instr.arg as usize];
                    frame.globals.write().shift_remove(name.as_str());
                }
                Opcode::LoadAttr => {
                    let name = frame.code.names[instr.arg as usize].clone();
                    let obj = frame.pop();
                    match obj.get_attr(&name) {
                        Some(v) => frame.push(v),
                        None => return Err(PyException::attribute_error(format!(
                            "'{}' object has no attribute '{}'", obj.type_name(), name
                        ))),
                    }
                }
                Opcode::StoreAttr => {
                    let name = frame.code.names[instr.arg as usize].clone();
                    let obj = frame.pop();   // TOS: the object
                    let value = frame.pop(); // TOS1: the value
                    match &obj.payload {
                        PyObjectPayload::Instance(inst) => {
                            inst.attrs.write().insert(name, value);
                        }
                        PyObjectPayload::Class(cd) => {
                            cd.namespace.write().insert(name, value);
                        }
                        _ => {
                            return Err(PyException::attribute_error(format!(
                                "'{}' object does not support attribute assignment", obj.type_name()
                            )));
                        }
                    }
                }

                // ── Unary operations ──
                Opcode::UnaryPositive => {
                    let v = frame.pop();
                    frame.push(v.positive()?);
                }
                Opcode::UnaryNegative => {
                    let v = frame.pop();
                    frame.push(v.negate()?);
                }
                Opcode::UnaryNot => {
                    let v = frame.pop();
                    frame.push(PyObject::bool_val(!v.is_truthy()));
                }
                Opcode::UnaryInvert => {
                    let v = frame.pop();
                    frame.push(v.invert()?);
                }

                // ── Binary operations ──
                Opcode::BinaryAdd | Opcode::InplaceAdd => {
                    let b = frame.pop(); let a = frame.pop();
                    frame.push(a.add(&b)?);
                }
                Opcode::BinarySubtract | Opcode::InplaceSubtract => {
                    let b = frame.pop(); let a = frame.pop();
                    frame.push(a.sub(&b)?);
                }
                Opcode::BinaryMultiply | Opcode::InplaceMultiply => {
                    let b = frame.pop(); let a = frame.pop();
                    frame.push(a.mul(&b)?);
                }
                Opcode::BinaryTrueDivide | Opcode::InplaceTrueDivide => {
                    let b = frame.pop(); let a = frame.pop();
                    frame.push(a.true_div(&b)?);
                }
                Opcode::BinaryFloorDivide | Opcode::InplaceFloorDivide => {
                    let b = frame.pop(); let a = frame.pop();
                    frame.push(a.floor_div(&b)?);
                }
                Opcode::BinaryModulo | Opcode::InplaceModulo => {
                    let b = frame.pop(); let a = frame.pop();
                    frame.push(a.modulo(&b)?);
                }
                Opcode::BinaryPower | Opcode::InplacePower => {
                    let b = frame.pop(); let a = frame.pop();
                    frame.push(a.power(&b)?);
                }
                Opcode::BinaryLshift | Opcode::InplaceLshift => {
                    let b = frame.pop(); let a = frame.pop();
                    frame.push(a.lshift(&b)?);
                }
                Opcode::BinaryRshift | Opcode::InplaceRshift => {
                    let b = frame.pop(); let a = frame.pop();
                    frame.push(a.rshift(&b)?);
                }
                Opcode::BinaryAnd | Opcode::InplaceAnd => {
                    let b = frame.pop(); let a = frame.pop();
                    frame.push(a.bit_and(&b)?);
                }
                Opcode::BinaryOr | Opcode::InplaceOr => {
                    let b = frame.pop(); let a = frame.pop();
                    frame.push(a.bit_or(&b)?);
                }
                Opcode::BinaryXor | Opcode::InplaceXor => {
                    let b = frame.pop(); let a = frame.pop();
                    frame.push(a.bit_xor(&b)?);
                }
                Opcode::BinarySubscr => {
                    let key = frame.pop();
                    let obj = frame.pop();
                    frame.push(obj.get_item(&key)?);
                }
                Opcode::StoreSubscr => {
                    // Stack: TOS = key, TOS1 = obj, TOS2 = value
                    let key = frame.pop();
                    let obj = frame.pop();
                    let value = frame.pop();
                    match &obj.payload {
                        PyObjectPayload::List(items) => {
                            let idx = key.to_int()?;
                            let mut w = items.write();
                            let len = w.len() as i64;
                            let actual = if idx < 0 { len + idx } else { idx };
                            if actual < 0 || actual >= len {
                                return Err(PyException::index_error("list assignment index out of range"));
                            }
                            w[actual as usize] = value;
                        }
                        PyObjectPayload::Dict(map) => {
                            let hk = key.to_hashable_key()?;
                            map.write().insert(hk, value);
                        }
                        _ => return Err(PyException::type_error(format!(
                            "'{}' object does not support item assignment", obj.type_name()))),
                    }
                }

                Opcode::DeleteSubscr => {
                    // Stack: TOS = key, TOS1 = obj
                    let key = frame.pop();
                    let obj = frame.pop();
                    match &obj.payload {
                        PyObjectPayload::List(items) => {
                            let idx = key.to_int()?;
                            let mut w = items.write();
                            let len = w.len() as i64;
                            let actual = if idx < 0 { len + idx } else { idx };
                            if actual < 0 || actual >= len {
                                return Err(PyException::index_error("list assignment index out of range"));
                            }
                            w.remove(actual as usize);
                        }
                        PyObjectPayload::Dict(map) => {
                            let hk = key.to_hashable_key()?;
                            if map.write().swap_remove(&hk).is_none() {
                                return Err(PyException::key_error(key.repr()));
                            }
                        }
                        _ => return Err(PyException::type_error(format!(
                            "'{}' object does not support item deletion", obj.type_name()))),
                    }
                }

                // ── Comparison ──
                Opcode::CompareOp => {
                    let b = frame.pop();
                    let a = frame.pop();
                    let result = match instr.arg {
                        0 => a.compare(&b, CompareOp::Lt)?,
                        1 => a.compare(&b, CompareOp::Le)?,
                        2 => a.compare(&b, CompareOp::Eq)?,
                        3 => a.compare(&b, CompareOp::Ne)?,
                        4 => a.compare(&b, CompareOp::Gt)?,
                        5 => a.compare(&b, CompareOp::Ge)?,
                        6 => PyObject::bool_val(b.contains(&a)?),   // in
                        7 => PyObject::bool_val(!b.contains(&a)?),  // not in
                        8 => PyObject::bool_val(a.is_same(&b)),     // is
                        9 => PyObject::bool_val(!a.is_same(&b)),    // is not
                        10 => {
                            // exception match: a is exception type on stack, b is type to match
                            let matched = match (&a.payload, &b.payload) {
                                (PyObjectPayload::ExceptionType(kind_a), PyObjectPayload::ExceptionType(kind_b)) => {
                                    exception_kind_matches(kind_a, kind_b)
                                }
                                _ => false,
                            };
                            PyObject::bool_val(matched)
                        }
                        _ => return Err(PyException::runtime_error("invalid compare op")),
                    };
                    frame.push(result);
                }

                // ── Jump operations ──
                Opcode::JumpForward => {
                    let frame = self.call_stack.last_mut().unwrap();
                    frame.ip = instr.arg as usize;
                }
                Opcode::JumpAbsolute => {
                    let frame = self.call_stack.last_mut().unwrap();
                    frame.ip = instr.arg as usize;
                }
                Opcode::PopJumpIfFalse => {
                    let v = frame.pop();
                    if !v.is_truthy() {
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.ip = instr.arg as usize;
                    }
                }
                Opcode::PopJumpIfTrue => {
                    let v = frame.pop();
                    if v.is_truthy() {
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.ip = instr.arg as usize;
                    }
                }
                Opcode::JumpIfTrueOrPop => {
                    if frame.peek().is_truthy() {
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.ip = instr.arg as usize;
                    } else {
                        frame.pop();
                    }
                }
                Opcode::JumpIfFalseOrPop => {
                    if !frame.peek().is_truthy() {
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.ip = instr.arg as usize;
                    } else {
                        frame.pop();
                    }
                }

                // ── Iterator operations ──
                Opcode::GetIter => {
                    let obj = frame.pop();
                    let iter = obj.get_iter()?;
                    frame.push(iter);
                }
                Opcode::ForIter => {
                    let iter = frame.peek().clone();
                    // Handle generator objects specially — need VM access for resumption
                    if let PyObjectPayload::Generator(ref gen_arc) = iter.payload {
                        let gen_arc = gen_arc.clone();
                        match self.resume_generator(&gen_arc, PyObject::none()) {
                            Ok(value) => {
                                let frame = self.call_stack.last_mut().unwrap();
                                frame.push(value);
                            }
                            Err(e) if e.kind == ExceptionKind::StopIteration => {
                                let frame = self.call_stack.last_mut().unwrap();
                                frame.pop(); // remove exhausted generator
                                frame.ip = instr.arg as usize;
                            }
                            Err(e) => return Err(e),
                        }
                    } else {
                        match builtins::iter_advance(&iter)? {
                            Some((new_iter, value)) => {
                                frame.pop();
                                frame.push(new_iter);
                                frame.push(value);
                            }
                            None => {
                                frame.pop(); // remove exhausted iterator
                                let frame = self.call_stack.last_mut().unwrap();
                                frame.ip = instr.arg as usize;
                            }
                        }
                    }
                }

                // ── Build operations ──
                Opcode::BuildTuple => {
                    let count = instr.arg as usize;
                    let mut items = Vec::with_capacity(count);
                    for _ in 0..count { items.push(frame.pop()); }
                    items.reverse();
                    frame.push(PyObject::tuple(items));
                }
                Opcode::BuildList => {
                    let count = instr.arg as usize;
                    let mut items = Vec::with_capacity(count);
                    for _ in 0..count { items.push(frame.pop()); }
                    items.reverse();
                    frame.push(PyObject::list(items));
                }
                Opcode::BuildSet => {
                    let count = instr.arg as usize;
                    let mut stack_items = Vec::new();
                    for _ in 0..count { stack_items.push(frame.pop()); }
                    stack_items.reverse();
                    let mut set = IndexMap::new();
                    for item in stack_items {
                        if let Ok(key) = item.to_hashable_key() {
                            set.insert(key, item);
                        }
                    }
                    frame.push(PyObject::set(set));
                }
                Opcode::BuildMap => {
                    let count = instr.arg as usize;
                    let mut entries = Vec::new();
                    for _ in 0..count {
                        let value = frame.pop();
                        let key = frame.pop();
                        entries.push((key, value));
                    }
                    entries.reverse();
                    let mut map = IndexMap::new();
                    for (key, value) in entries {
                        let hkey = key.to_hashable_key()?;
                        map.insert(hkey, value);
                    }
                    frame.push(PyObject::dict(map));
                }
                Opcode::BuildConstKeyMap => {
                    let keys_tuple = frame.pop();
                    let keys = keys_tuple.to_list()?;
                    let count = instr.arg as usize;
                    let mut values = Vec::new();
                    for _ in 0..count { values.push(frame.pop()); }
                    values.reverse();
                    let mut map = IndexMap::new();
                    for (key, value) in keys.into_iter().zip(values) {
                        let hkey = key.to_hashable_key()?;
                        map.insert(hkey, value);
                    }
                    frame.push(PyObject::dict(map));
                }
                Opcode::BuildString => {
                    let count = instr.arg as usize;
                    let mut parts = Vec::new();
                    for _ in 0..count { parts.push(frame.pop()); }
                    parts.reverse();
                    let s: String = parts.iter().map(|p| p.py_to_string()).collect();
                    frame.push(PyObject::str_val(CompactString::from(s)));
                }
                Opcode::ListAppend => {
                    let item = frame.pop();
                    let idx = instr.arg as usize;
                    let stack_pos = frame.stack.len() - idx;
                    let list_obj = frame.stack[stack_pos].clone();
                    if let PyObjectPayload::List(items) = &list_obj.payload {
                        items.write().push(item);
                    }
                }
                Opcode::SetAdd => {
                    let item = frame.pop();
                    let idx = instr.arg as usize;
                    let stack_pos = frame.stack.len() - idx;
                    let set_obj = frame.stack[stack_pos].clone();
                    if let PyObjectPayload::Set(s) = &set_obj.payload {
                        let mut new_set = s.clone();
                        if let Ok(key) = item.to_hashable_key() {
                            new_set.insert(key, item);
                        }
                        frame.stack[stack_pos] = PyObject::set(new_set);
                    }
                }
                Opcode::MapAdd => {
                    let value = frame.pop();
                    let key = frame.pop();
                    let idx = instr.arg as usize;
                    let stack_pos = frame.stack.len() - idx;
                    let dict_obj = &frame.stack[stack_pos];
                    if let PyObjectPayload::Dict(m) = &dict_obj.payload {
                        if let Ok(hk) = key.to_hashable_key() {
                            m.write().insert(hk, value);
                        }
                    }
                }
                Opcode::BuildSlice => {
                    let argc = instr.arg as usize;
                    let step = if argc == 3 { Some(frame.pop()) } else { None };
                    let stop = frame.pop();
                    let start = frame.pop();
                    let s_start = if matches!(start.payload, PyObjectPayload::None) { None } else { Some(start) };
                    let s_stop = if matches!(stop.payload, PyObjectPayload::None) { None } else { Some(stop) };
                    frame.push(PyObject::slice(s_start, s_stop, step));
                }

                // ── Unpack ──
                Opcode::UnpackSequence => {
                    let seq = frame.pop();
                    let items = seq.to_list()?;
                    let count = instr.arg as usize;
                    if items.len() != count {
                        return Err(PyException::value_error(format!(
                            "not enough values to unpack (expected {}, got {})",
                            count, items.len()
                        )));
                    }
                    for item in items.into_iter().rev() {
                        frame.push(item);
                    }
                }

                Opcode::UnpackEx => {
                    let seq = frame.pop();
                    let items = seq.to_list()?;
                    let before = (instr.arg & 0xFF) as usize;
                    let after = ((instr.arg >> 8) & 0xFF) as usize;
                    let total_fixed = before + after;
                    if items.len() < total_fixed {
                        return Err(PyException::value_error(format!(
                            "not enough values to unpack (expected at least {}, got {})",
                            total_fixed, items.len()
                        )));
                    }
                    // Push in reverse order: after_items (reversed), starred list, before_items (reversed)
                    // Stack after: before_0, before_1, ..., starred_list, after_0, after_1, ...
                    let star_count = items.len() - total_fixed;
                    // Push after items in reverse
                    for i in (0..after).rev() {
                        let idx = before + star_count + i;
                        frame.push(items[idx].clone());
                    }
                    // Push starred list
                    let starred: Vec<PyObjectRef> = items[before..before + star_count].to_vec();
                    frame.push(PyObject::list(starred));
                    // Push before items in reverse
                    for i in (0..before).rev() {
                        frame.push(items[i].clone());
                    }
                }

                // ── Function call ──
                Opcode::CallFunction => {
                    let arg_count = instr.arg as usize;
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count { args.push(frame.pop()); }
                    args.reverse();
                    let func = frame.pop();
                    let result = self.call_object(func, args)?;
                    let frame = self.call_stack.last_mut().unwrap();
                    frame.push(result);
                }
                Opcode::CallFunctionKw => {
                    let kw_names_obj = frame.pop();
                    let arg_count = instr.arg as usize;
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count { args.push(frame.pop()); }
                    args.reverse();
                    let func = frame.pop();
                    // Extract keyword names from the tuple
                    let kw_names: Vec<CompactString> = match &kw_names_obj.payload {
                        PyObjectPayload::Tuple(items) => {
                            items.iter().map(|item| {
                                match &item.payload {
                                    PyObjectPayload::Str(s) => s.clone(),
                                    _ => CompactString::from(item.py_to_string()),
                                }
                            }).collect()
                        }
                        _ => Vec::new(),
                    };
                    let n_kw = kw_names.len();
                    let n_pos = arg_count - n_kw;
                    let pos_args = args[..n_pos].to_vec();
                    let mut kwargs: Vec<(CompactString, PyObjectRef)> = Vec::new();
                    for (i, name) in kw_names.iter().enumerate() {
                        kwargs.push((name.clone(), args[n_pos + i].clone()));
                    }
                    let result = self.call_object_kw(func, pos_args, kwargs)?;
                    let frame = self.call_stack.last_mut().unwrap();
                    frame.push(result);
                }
                Opcode::CallMethod => {
                    let arg_count = instr.arg as usize;
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count { args.push(frame.pop()); }
                    args.reverse();
                    let method = frame.pop();
                    let result = self.call_object(method, args)?;
                    let frame = self.call_stack.last_mut().unwrap();
                    frame.push(result);
                }
                Opcode::CallFunctionEx => {
                    // arg bit 0: has **kwargs dict
                    let has_kwargs = (instr.arg & 1) != 0;
                    let kwargs_obj = if has_kwargs { Some(frame.pop()) } else { None };
                    let args_obj = frame.pop();
                    let func = frame.pop();

                    // Unpack positional args from tuple/list
                    let pos_args = args_obj.to_list().unwrap_or_default();

                    if let Some(kw_obj) = kwargs_obj {
                        // Unpack kwargs from dict
                        let mut kw_vec: Vec<(CompactString, PyObjectRef)> = Vec::new();
                        if let PyObjectPayload::Dict(map) = &kw_obj.payload {
                            for (k, v) in map.read().iter() {
                                let name = match k {
                                    HashableKey::Str(s) => s.clone(),
                                    _ => CompactString::from(format!("{:?}", k)),
                                };
                                kw_vec.push((name, v.clone()));
                            }
                        }
                        let result = self.call_object_kw(func, pos_args, kw_vec)?;
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.push(result);
                    } else {
                        let result = self.call_object(func, pos_args)?;
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.push(result);
                    }
                }
                Opcode::LoadMethod => {
                    let name = frame.code.names[instr.arg as usize].clone();
                    let obj = frame.pop();
                    match obj.get_attr(&name) {
                        Some(method) => frame.push(method),
                        None => return Err(PyException::attribute_error(format!(
                            "'{}' object has no attribute '{}'", obj.type_name(), name
                        ))),
                    }
                }

                // ── Function construction ──
                Opcode::MakeFunction => {
                    let qualname = frame.pop();
                    let code_obj = frame.pop();
                    let flags = instr.arg;
                    // CPython pops: closure(0x08), annotations(0x04), kwdefaults(0x02), defaults(0x01)
                    let closure_cells = if flags & 0x08 != 0 {
                        let closure_tuple = frame.pop();
                        // The closure tuple contains PyObjectRef wrapping CellRef objects
                        // We stored them as a tuple of cell wrapper objects
                        match &closure_tuple.payload {
                            PyObjectPayload::Tuple(items) => {
                                items.iter().map(|item| {
                                    match &item.payload {
                                        PyObjectPayload::Cell(cell) => cell.clone(),
                                        _ => {
                                            // Shouldn't happen, but wrap as cell
                                            Arc::new(RwLock::new(Some(item.clone())))
                                        }
                                    }
                                }).collect()
                            }
                            _ => Vec::new(),
                        }
                    } else { Vec::new() };
                    if flags & 0x04 != 0 { frame.pop(); } // annotations
                    if flags & 0x02 != 0 { frame.pop(); } // kwdefaults
                    let mut defaults = Vec::new();
                    if flags & 0x01 != 0 {
                        let default_tuple = frame.pop();
                        defaults = default_tuple.to_list().unwrap_or_default();
                    }
                    let code = match &code_obj.payload {
                        PyObjectPayload::Code(c) => *c.clone(),
                        _ => return Err(PyException::type_error(
                            "expected code object for MAKE_FUNCTION",
                        )),
                    };
                    let name_str = qualname.as_str().map(CompactString::from)
                        .unwrap_or_else(|| code.name.clone());
                    let func = PyFunction {
                        name: name_str.clone(),
                        qualname: name_str,
                        code,
                        defaults,
                        kw_defaults: IndexMap::new(),
                        globals: frame.globals.clone(),
                        closure: closure_cells,
                        annotations: IndexMap::new(),
                    };
                    frame.push(PyObject::function(func));
                }

                Opcode::ReturnValue => {
                    let value = frame.pop();
                    return Ok(Some(value));
                }

                // ── Import ──
                Opcode::ImportName => {
                    let _fromlist = frame.pop();
                    let _level = frame.pop();
                    let name = frame.code.names[instr.arg as usize].clone();
                    // Check module cache first
                    if let Some(module) = self.modules.get(&name) {
                        frame.push(module.clone());
                    } else {
                        // Try to load a builtin module
                        let module = self.load_builtin_module(&name)?;
                        self.modules.insert(name, module.clone());
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.push(module);
                    }
                }
                Opcode::ImportFrom => {
                    let name = &frame.code.names[instr.arg as usize];
                    let module = frame.peek().clone();
                    match module.get_attr(name) {
                        Some(v) => frame.push(v),
                        None => return Err(PyException::import_error(format!(
                            "cannot import name '{}' from module", name
                        ))),
                    }
                }
                Opcode::ImportStar => {
                    let _module = frame.pop();
                    // TODO: copy all names from module to local scope
                }

                // ── Exception handling ──
                Opcode::SetupFinally => {
                    frame.push_block(BlockKind::Finally, instr.arg as usize);
                }
                Opcode::PopBlock => { frame.pop_block(); }
                Opcode::PopExcept => { frame.pop_block(); }
                Opcode::EndFinally => {
                    // Check TOS: if it's an exception type, re-raise the exception.
                    // If it's None, the finally block was entered normally — continue.
                    // If the stack is empty or TOS is something else, just continue.
                    if !frame.stack.is_empty() {
                        let tos = frame.peek();
                        match &tos.payload {
                            PyObjectPayload::ExceptionType(kind) => {
                                let kind = kind.clone();
                                frame.pop(); // type
                                let value = if !frame.stack.is_empty() { frame.pop() } else { PyObject::none() };
                                if !frame.stack.is_empty() { frame.pop(); } // traceback
                                let msg = match &value.payload {
                                    PyObjectPayload::ExceptionInstance { message, .. } => message.to_string(),
                                    _ => value.py_to_string(),
                                };
                                return Err(PyException::new(kind, msg));
                            }
                            PyObjectPayload::None => {
                                frame.pop(); // consume the None
                            }
                            _ => {
                                // Integer or other — just continue
                            }
                        }
                    }
                }
                Opcode::BeginFinally => {
                    // Push None to indicate normal (non-exception) entry into finally
                    frame.push(PyObject::none());
                }
                Opcode::RaiseVarargs => {
                    match instr.arg {
                        0 => return Err(PyException::runtime_error(
                            "No active exception to re-raise")),
                        1 => {
                            let exc = frame.pop();
                            match &exc.payload {
                                PyObjectPayload::ExceptionInstance { kind, message, .. } => {
                                    return Err(PyException::new(kind.clone(), message.to_string()));
                                }
                                PyObjectPayload::ExceptionType(kind) => {
                                    return Err(PyException::new(kind.clone(), ""));
                                }
                                _ => return Err(PyException::runtime_error(exc.py_to_string())),
                            }
                        }
                        2 => {
                            let _cause = frame.pop();
                            let exc = frame.pop();
                            match &exc.payload {
                                PyObjectPayload::ExceptionInstance { kind, message, .. } => {
                                    return Err(PyException::new(kind.clone(), message.to_string()));
                                }
                                PyObjectPayload::ExceptionType(kind) => {
                                    return Err(PyException::new(kind.clone(), ""));
                                }
                                _ => return Err(PyException::runtime_error(exc.py_to_string())),
                            }
                        }
                        _ => return Err(PyException::runtime_error(
                            "bad RAISE_VARARGS arg")),
                    }
                }
                Opcode::SetupWith => {
                    // TOS is the context manager
                    let ctx_mgr = frame.pop();
                    // Get __exit__ method and save it on the stack
                    let exit_method = ctx_mgr.get_attr("__exit__").ok_or_else(||
                        PyException::attribute_error("__exit__"))?;
                    frame.push(exit_method);
                    // Call __enter__
                    let enter_method = ctx_mgr.get_attr("__enter__").ok_or_else(||
                        PyException::attribute_error("__enter__"))?;
                    let enter_result = self.call_object(enter_method, vec![])?;
                    let frame = self.call_stack.last_mut().unwrap();
                    // Setup the With block (points to cleanup handler)
                    frame.push_block(BlockKind::With, instr.arg as usize);
                    // Push the __enter__ result (to be stored by `as` or popped)
                    frame.push(enter_result);
                }

                // ── Print (interactive mode) ──
                Opcode::PrintExpr => {
                    let value = frame.pop();
                    if !matches!(value.payload, PyObjectPayload::None) {
                        println!("{}", value.repr());
                    }
                }

                // ── Load/Build class ──
                Opcode::LoadBuildClass => {
                    frame.push(PyObject::builtin_function(
                        CompactString::from("__build_class__")));
                }
                Opcode::SetupAnnotations => {
                    if !frame.local_names.contains_key("__annotations__") {
                        frame.store_name(
                            CompactString::from("__annotations__"),
                            PyObject::dict(IndexMap::new()),
                        );
                    }
                }

                // ── Format ──
                Opcode::FormatValue => {
                    if instr.arg & 0x04 != 0 {
                        let _fmt_spec = frame.pop();
                    }
                    let value = frame.pop();
                    let conversion = (instr.arg & 0x03) as u8;
                    let formatted = match conversion {
                        1 => value.py_to_string(),   // !s
                        2 => value.repr(),            // !r
                        3 => value.py_to_string(),    // !a (ascii)
                        _ => value.py_to_string(),
                    };
                    frame.push(PyObject::str_val(CompactString::from(formatted)));
                }

                // ── Extended arg ──
                Opcode::ExtendedArg => {}

                // ── Yield ──
                Opcode::YieldValue => {
                    let value = frame.pop();
                    frame.yielded = true;
                    return Ok(Some(value));
                }
                Opcode::YieldFrom => {
                    let _value = frame.pop();
                    frame.push(PyObject::none());
                }

                // ── With cleanup ──
                Opcode::WithCleanupStart => {
                    // Stack at this point (from top): exception info or None (from BeginFinally)
                    // Below that: __exit__ method
                    // If TOS is None, it's a normal exit — call __exit__(None, None, None)
                    // If TOS is an exception type, call __exit__(type, value, traceback)
                    let tos = frame.peek().clone();
                    if matches!(tos.payload, PyObjectPayload::None) {
                        // Normal exit: pop None, get __exit__, call it
                        frame.pop(); // pop None
                        let exit_fn = frame.pop();
                        let result = self.call_object(exit_fn, vec![
                            PyObject::none(), PyObject::none(), PyObject::none()
                        ])?;
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.push(PyObject::none()); // indicate normal flow
                        frame.push(result); // __exit__ return value
                    } else if matches!(tos.payload, PyObjectPayload::ExceptionType(_)) {
                        // Exception exit: pop type, value, traceback; get __exit__; call it
                        let exc_type = frame.pop();
                        let exc_val = if !frame.stack.is_empty() { frame.pop() } else { PyObject::none() };
                        let exc_tb = if !frame.stack.is_empty() { frame.pop() } else { PyObject::none() };
                        let exit_fn = frame.pop();
                        let result = self.call_object(exit_fn, vec![
                            exc_type.clone(), exc_val, exc_tb
                        ])?;
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.push(exc_type); // re-push exception type for EndFinally
                        frame.push(result); // __exit__ return value
                    } else {
                        // Fallback: treat as normal
                        frame.pop();
                        let exit_fn = frame.pop();
                        let result = self.call_object(exit_fn, vec![
                            PyObject::none(), PyObject::none(), PyObject::none()
                        ])?;
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.push(PyObject::none());
                        frame.push(result);
                    }
                }
                Opcode::WithCleanupFinish => {
                    let exit_result = frame.pop(); // __exit__ return value
                    let exc_or_none = frame.pop();
                    // If there was an exception and __exit__ returned truthy,
                    // the exception is suppressed — push None for EndFinally
                    if !matches!(exc_or_none.payload, PyObjectPayload::None) && exit_result.is_truthy() {
                        frame.push(PyObject::none()); // suppress exception
                    } else {
                        frame.push(exc_or_none); // re-push for EndFinally
                    }
                }

                _ => {
                    return Err(PyException::runtime_error(format!(
                        "unimplemented opcode: {:?}", instr.op
                    )));
                }
            }
            Ok(None)
    }

    /// Call a Python object (function, builtin, class).
    fn call_object(
        &mut self,
        func: PyObjectRef,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        match &func.payload {
            PyObjectPayload::Function(pyfunc) => {
                let code = pyfunc.code.clone();
                let globals = pyfunc.globals.clone();
                let defaults = pyfunc.defaults.clone();
                let closure = pyfunc.closure.clone();
                self.call_function(&code, args, &defaults, globals, &closure)
            }
            PyObjectPayload::BuiltinFunction(name) => {
                if name.as_str() == "__build_class__" {
                    return self.build_class(args);
                }
                // VM-aware builtins that need to call user-defined methods
                match name.as_str() {
                    "str" => {
                        if args.is_empty() {
                            return Ok(PyObject::str_val(CompactString::from("")));
                        }
                        // Only check __str__ on class instances, not builtins
                        if matches!(&args[0].payload, PyObjectPayload::Instance(_)) {
                            if let Some(str_method) = args[0].get_attr("__str__") {
                                return self.call_object(str_method, vec![]);
                            }
                        }
                        return Ok(PyObject::str_val(CompactString::from(args[0].py_to_string())));
                    }
                    "repr" => {
                        if args.is_empty() {
                            return Ok(PyObject::str_val(CompactString::from("")));
                        }
                        if matches!(&args[0].payload, PyObjectPayload::Instance(_)) {
                            if let Some(repr_method) = args[0].get_attr("__repr__") {
                                return self.call_object(repr_method, vec![]);
                            }
                        }
                        return Ok(PyObject::str_val(CompactString::from(args[0].repr())));
                    }
                    "map" => {
                        if args.len() < 2 {
                            return Err(PyException::type_error("map() requires at least 2 arguments"));
                        }
                        let func_obj = args[0].clone();
                        let iterable = args[1].to_list()?;
                        let mut result = Vec::new();
                        for item in iterable {
                            result.push(self.call_object(func_obj.clone(), vec![item])?);
                        }
                        return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                            ferrython_core::object::IteratorData::List { items: result, index: 0 }
                        )));
                    }
                    "filter" => {
                        if args.len() < 2 {
                            return Err(PyException::type_error("filter() requires at least 2 arguments"));
                        }
                        let func_obj = args[0].clone();
                        let iterable = args[1].to_list()?;
                        let mut result = Vec::new();
                        for item in iterable {
                            let keep = if matches!(func_obj.payload, PyObjectPayload::None) {
                                item.is_truthy()
                            } else {
                                self.call_object(func_obj.clone(), vec![item.clone()])?.is_truthy()
                            };
                            if keep {
                                result.push(item);
                            }
                        }
                        return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                            ferrython_core::object::IteratorData::List { items: result, index: 0 }
                        )));
                    }
                    "next" => {
                        if args.is_empty() {
                            return Err(PyException::type_error("next() requires at least 1 argument"));
                        }
                        if let PyObjectPayload::Generator(ref gen_arc) = args[0].payload {
                            let gen_arc = gen_arc.clone();
                            return match self.resume_generator(&gen_arc, PyObject::none()) {
                                Ok(value) => Ok(value),
                                Err(e) if e.kind == ExceptionKind::StopIteration && args.len() > 1 => {
                                    Ok(args[1].clone()) // default value
                                }
                                Err(e) => Err(e),
                            };
                        }
                        // Fall through to regular next() for iterators
                    }
                    "list" => {
                        if args.is_empty() {
                            return Ok(PyObject::list(vec![]));
                        }
                        // Handle generators by draining them
                        if let PyObjectPayload::Generator(ref gen_arc) = args[0].payload {
                            let gen_arc = gen_arc.clone();
                            let mut items = Vec::new();
                            loop {
                                match self.resume_generator(&gen_arc, PyObject::none()) {
                                    Ok(value) => items.push(value),
                                    Err(e) if e.kind == ExceptionKind::StopIteration => break,
                                    Err(e) => return Err(e),
                                }
                            }
                            return Ok(PyObject::list(items));
                        }
                        // Fall through to regular list() for other iterables
                    }
                    "tuple" => {
                        if args.is_empty() {
                            return Ok(PyObject::tuple(vec![]));
                        }
                        if let PyObjectPayload::Generator(ref gen_arc) = args[0].payload {
                            let gen_arc = gen_arc.clone();
                            let mut items = Vec::new();
                            loop {
                                match self.resume_generator(&gen_arc, PyObject::none()) {
                                    Ok(value) => items.push(value),
                                    Err(e) if e.kind == ExceptionKind::StopIteration => break,
                                    Err(e) => return Err(e),
                                }
                            }
                            return Ok(PyObject::tuple(items));
                        }
                    }
                    "sum" => {
                        if args.is_empty() {
                            return Err(PyException::type_error("sum() requires at least 1 argument"));
                        }
                        if let PyObjectPayload::Generator(ref gen_arc) = args[0].payload {
                            let gen_arc = gen_arc.clone();
                            let start = if args.len() > 1 { args[1].clone() } else { PyObject::int(0) };
                            let mut total = start;
                            loop {
                                match self.resume_generator(&gen_arc, PyObject::none()) {
                                    Ok(value) => total = total.add(&value)?,
                                    Err(e) if e.kind == ExceptionKind::StopIteration => break,
                                    Err(e) => return Err(e),
                                }
                            }
                            return Ok(total);
                        }
                    }
                    _ => {}
                }
                match builtins::get_builtin_fn(name.as_str()) {
                    Some(f) => f(&args),
                    None => Err(PyException::type_error(format!(
                        "'{}' is not callable", name
                    ))),
                }
            }
            PyObjectPayload::Class(_class) => {
                let instance = PyObject::instance(func.clone());
                // Look up __init__ through MRO (supports inheritance)
                if let Some(init) = func.get_attr("__init__") {
                    let init_fn = match &init.payload {
                        PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                        _ => init.clone(),
                    };
                    let mut init_args = vec![instance.clone()];
                    init_args.extend(args);
                    self.call_object(init_fn, init_args)?;
                }
                Ok(instance)
            }
            PyObjectPayload::BoundMethod { receiver, method } => {
                let mut bound_args = vec![receiver.clone()];
                bound_args.extend(args);
                self.call_object(method.clone(), bound_args)
            }
            PyObjectPayload::BuiltinBoundMethod { receiver, method_name } => {
                builtins::call_method(receiver, method_name.as_str(), &args)
            }
            PyObjectPayload::ExceptionType(kind) => {
                // Calling an exception type creates an exception instance
                let msg = if args.is_empty() {
                    String::new()
                } else {
                    args[0].py_to_string()
                };
                Ok(PyObject::exception_instance(kind.clone(), msg))
            }
            PyObjectPayload::NativeFunction { func, .. } => {
                func(&args)
            }
            _ => Err(PyException::type_error(format!(
                "'{}' object is not callable", func.type_name()
            ))),
        }
    }

    /// Handle __build_class__(func, name, *bases).
    fn build_class(&mut self, args: Vec<PyObjectRef>) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "__build_class__ requires at least 2 arguments"));
        }
        let body_func = args[0].clone();
        let class_name = match &args[1].payload {
            PyObjectPayload::Str(s) => s.clone(),
            _ => CompactString::from(args[1].py_to_string()),
        };
        let bases: Vec<PyObjectRef> = args[2..].to_vec();

        let namespace = match &body_func.payload {
            PyObjectPayload::Function(pyfunc) => {
                let code = pyfunc.code.clone();
                let globals = pyfunc.globals.clone();
                let mut frame = Frame::new(code, globals, self.builtins.clone());
                frame.scope_kind = ScopeKind::Class;
                self.call_stack.push(frame);
                let _ = self.run_frame();
                let frame = self.call_stack.pop().unwrap();
                frame.local_names
            }
            _ => IndexMap::new(),
        };

        Ok(PyObject::class(class_name, bases, namespace))
    }

    /// Resume a generator, pushing the given `send_value` onto its stack and running
    /// until the next `YieldValue` or `ReturnValue`.
    /// Returns `Ok(value)` for yielded values, or `Err(StopIteration)` when done.
    fn resume_generator(
        &mut self,
        gen_arc: &Arc<RwLock<GeneratorState>>,
        send_value: PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        let mut gen = gen_arc.write();
        if gen.finished {
            return Err(PyException::new(ExceptionKind::StopIteration, ""));
        }
        let mut frame = match gen.frame.take() {
            Some(f) => *f.downcast::<Frame>().expect("generator frame downcast"),
            None => return Err(PyException::runtime_error("generator already executing")),
        };

        // If generator was already started, push the send value onto the frame's stack
        // (it becomes the result of the `yield` expression)
        if gen.started {
            frame.push(send_value);
        }
        gen.started = true;
        drop(gen); // release lock before executing

        self.call_stack.push(frame);
        let result = self.run_frame();
        let frame = self.call_stack.pop().unwrap();

        let mut gen = gen_arc.write();
        if frame.yielded {
            // Generator yielded — save frame for later resumption
            let mut saved_frame = frame;
            saved_frame.yielded = false;
            gen.frame = Some(Box::new(saved_frame));
            result // Ok(yielded_value)
        } else {
            // Generator returned — mark finished, raise StopIteration
            gen.finished = true;
            gen.frame = None;
            Err(PyException::new(ExceptionKind::StopIteration, ""))
        }
    }

    fn load_builtin_module(&self, name: &str) -> PyResult<PyObjectRef> {
        match name {
            "math" => Ok(builtins::create_math_module()),
            "sys" => Ok(builtins::create_sys_module()),
            "os" => Ok(builtins::create_os_module()),
            "os.path" => Ok(builtins::create_os_path_module()),
            "string" => Ok(builtins::create_string_module()),
            "json" => Ok(builtins::create_json_module()),
            "time" => Ok(builtins::create_time_module()),
            "random" => Ok(builtins::create_random_module()),
            "collections" => Ok(builtins::create_collections_module()),
            "functools" => Ok(builtins::create_functools_module()),
            "itertools" => Ok(builtins::create_itertools_module()),
            "io" => Ok(builtins::create_io_module()),
            "re" => Ok(builtins::create_re_module()),
            "hashlib" => Ok(builtins::create_hashlib_module()),
            _ => {
                // Try to load from filesystem
                self.load_file_module(name)
            }
        }
    }

    fn load_file_module(&self, name: &str) -> PyResult<PyObjectRef> {
        // Convert module name to file path (e.g., "foo.bar" -> "foo/bar.py")
        let path = name.replace('.', "/");
        let candidates = [
            format!("{}.py", path),
            format!("{}/__init__.py", path),
        ];
        for candidate in &candidates {
            if std::path::Path::new(candidate).exists() {
                let source = std::fs::read_to_string(candidate)
                    .map_err(|e| PyException::import_error(format!("cannot read '{}': {}", candidate, e)))?;
                let ast = ferrython_parser::parse(&source, candidate)
                    .map_err(|e| PyException::import_error(format!("syntax error in '{}': {}", candidate, e)))?;
                let code = ferrython_compiler::compile(&ast, candidate)
                    .map_err(|e| PyException::import_error(format!("compilation error in '{}': {}", candidate, e)))?;
                let mut vm = VirtualMachine::new();
                let _result = vm.execute(code)?;
                // Collect module globals
                // This is simplified — real import is more complex
                return Err(PyException::import_error(format!("No module named '{}'", name)));
            }
        }
        Err(PyException::import_error(format!("No module named '{}'", name)))
    }
}

/// Convert a bytecode constant to a runtime PyObject.
fn constant_to_object(constant: &ConstantValue) -> PyObjectRef {
    match constant {
        ConstantValue::None => PyObject::none(),
        ConstantValue::Bool(b) => PyObject::bool_val(*b),
        ConstantValue::Integer(n) => PyObject::int(*n),
        ConstantValue::BigInteger(n) => PyObject::big_int(n.as_ref().clone()),
        ConstantValue::Float(f) => PyObject::float(*f),
        ConstantValue::Complex { real, imag } => PyObject::complex(*real, *imag),
        ConstantValue::Str(s) => PyObject::str_val(s.clone()),
        ConstantValue::Bytes(b) => PyObject::bytes(b.clone()),
        ConstantValue::Ellipsis => PyObject::ellipsis(),
        ConstantValue::Code(code) => PyObject::code(*code.clone()),
        ConstantValue::Tuple(items) => {
            let objs: Vec<PyObjectRef> = items.iter().map(constant_to_object).collect();
            PyObject::tuple(objs)
        }
        ConstantValue::FrozenSet(items) => {
            let mut set = IndexMap::new();
            for item in items {
                let obj = constant_to_object(item);
                if let Ok(key) = obj.to_hashable_key() {
                    set.insert(key, obj);
                }
            }
            PyObject::set(set)
        }
    }
}

impl Default for VirtualMachine {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if `actual` exception kind matches `expected` (including inheritance).
fn exception_kind_matches(actual: &ExceptionKind, expected: &ExceptionKind) -> bool {
    if std::mem::discriminant(actual) == std::mem::discriminant(expected) {
        return true;
    }
    // Walk the exception hierarchy
    match expected {
        ExceptionKind::BaseException => true, // catches everything
        ExceptionKind::Exception => !matches!(actual,
            ExceptionKind::SystemExit | ExceptionKind::KeyboardInterrupt | ExceptionKind::GeneratorExit
        ),
        ExceptionKind::ArithmeticError => matches!(actual,
            ExceptionKind::ArithmeticError | ExceptionKind::FloatingPointError |
            ExceptionKind::OverflowError | ExceptionKind::ZeroDivisionError
        ),
        ExceptionKind::LookupError => matches!(actual,
            ExceptionKind::LookupError | ExceptionKind::IndexError | ExceptionKind::KeyError
        ),
        ExceptionKind::OSError => matches!(actual,
            ExceptionKind::OSError | ExceptionKind::BlockingIOError |
            ExceptionKind::BrokenPipeError | ExceptionKind::FileExistsError |
            ExceptionKind::FileNotFoundError | ExceptionKind::PermissionError
        ),
        ExceptionKind::UnicodeError => matches!(actual,
            ExceptionKind::UnicodeError | ExceptionKind::UnicodeDecodeError |
            ExceptionKind::UnicodeEncodeError
        ),
        ExceptionKind::ValueError => matches!(actual,
            ExceptionKind::ValueError | ExceptionKind::UnicodeError |
            ExceptionKind::UnicodeDecodeError | ExceptionKind::UnicodeEncodeError
        ),
        ExceptionKind::Warning => matches!(actual,
            ExceptionKind::Warning | ExceptionKind::DeprecationWarning |
            ExceptionKind::RuntimeWarning | ExceptionKind::UserWarning
        ),
        ExceptionKind::ImportError => matches!(actual,
            ExceptionKind::ImportError | ExceptionKind::ModuleNotFoundError
        ),
        _ => false,
    }
}
