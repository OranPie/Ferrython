//! The main virtual machine — executes bytecode instructions.

use crate::builtins;
use crate::frame::{BlockKind, Frame, ScopeKind};
use compact_str::CompactString;
use ferrython_bytecode::code::{CodeObject, ConstantValue};
use ferrython_bytecode::opcode::Opcode;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    CompareOp, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, PyFunction, SharedGlobals};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

/// The Ferrython virtual machine.
pub struct VirtualMachine {
    call_stack: Vec<Frame>,
    builtins: IndexMap<CompactString, PyObjectRef>,
}

impl VirtualMachine {
    pub fn new() -> Self {
        Self {
            call_stack: Vec::new(),
            builtins: builtins::init_builtins(),
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
    ) -> PyResult<PyObjectRef> {
        let mut frame = Frame::new(code.clone(), globals, self.builtins.clone());
        let nparams = code.arg_count as usize;
        for (i, arg) in args.iter().enumerate() {
            if i < code.varnames.len() {
                frame.set_local(i, arg.clone());
            }
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
        frame.scope_kind = ScopeKind::Function;
        self.call_stack.push(frame);
        let result = self.run_frame();
        self.call_stack.pop();
        result
    }

    /// Main evaluation loop.
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
                        let exc_obj = PyObject::str_val(CompactString::from(exc.message.clone()));
                        frame.push(PyObject::none());     // traceback
                        frame.push(exc_obj);              // value
                        frame.push(PyObject::none());     // type
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
                BlockKind::Loop | BlockKind::With => {
                    while frame.stack.len() > block.stack_level {
                        frame.pop();
                    }
                    continue;
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
                            // Dict uses interior mutability via Arc
                            // For now, we need a mutable dict approach
                            // This is a limitation — we'll handle it when we make Dict mutable too
                            let _ = (map, &key, &value);
                            return Err(PyException::type_error("dict item assignment not yet fully supported"));
                        }
                        _ => return Err(PyException::type_error(format!(
                            "'{}' object does not support item assignment", obj.type_name()))),
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
                    let dict_obj = frame.stack[stack_pos].clone();
                    if let PyObjectPayload::Dict(m) = &dict_obj.payload {
                        let mut new_map = m.clone();
                        if let Ok(hk) = key.to_hashable_key() {
                            new_map.insert(hk, value);
                        }
                        frame.stack[stack_pos] = PyObject::dict(new_map);
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
                    let _kw_names = frame.pop();
                    let arg_count = instr.arg as usize;
                    let mut args = Vec::with_capacity(arg_count);
                    for _ in 0..arg_count { args.push(frame.pop()); }
                    args.reverse();
                    let func = frame.pop();
                    let result = self.call_object(func, args)?;
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
                    if flags & 0x08 != 0 { frame.pop(); } // closure
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
                        closure: Vec::new(),
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
                    let name = &frame.code.names[instr.arg as usize];
                    let module = PyObject::module(name.clone());
                    frame.push(module);
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
                    // In CPython, EndFinally checks TOS for re-raise.
                    // For bare except (no 'as'), we just continue.
                    // The 3 exception values were already popped by PopTop*3 + PopExcept.
                    // Nothing to do here for now.
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
                            return Err(PyException::runtime_error(exc.py_to_string()));
                        }
                        2 => {
                            let _cause = frame.pop();
                            let exc = frame.pop();
                            return Err(PyException::runtime_error(exc.py_to_string()));
                        }
                        _ => return Err(PyException::runtime_error(
                            "bad RAISE_VARARGS arg")),
                    }
                }
                Opcode::SetupWith => {
                    frame.push_block(BlockKind::With, instr.arg as usize);
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
                    return Ok(Some(value));
                }
                Opcode::YieldFrom => {
                    let _value = frame.pop();
                    frame.push(PyObject::none());
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
                self.call_function(&code, args, &defaults, globals)
            }
            PyObjectPayload::BuiltinFunction(name) => {
                if name.as_str() == "__build_class__" {
                    return self.build_class(args);
                }
                match builtins::get_builtin_fn(name.as_str()) {
                    Some(f) => f(&args),
                    None => Err(PyException::type_error(format!(
                        "'{}' is not callable", name
                    ))),
                }
            }
            PyObjectPayload::Class(class) => {
                let instance = PyObject::instance(func.clone());
                if let Some(init) = class.namespace.get("__init__") {
                    let mut init_args = vec![instance.clone()];
                    init_args.extend(args);
                    self.call_object(init.clone(), init_args)?;
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
