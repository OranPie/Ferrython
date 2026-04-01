//! The main virtual machine — executes bytecode instructions.

use crate::builtins;
use crate::frame::{BlockKind, Frame, ScopeKind};
use compact_str::CompactString;
use ferrython_bytecode::code::{CodeFlags, CodeObject, ConstantValue};
use ferrython_bytecode::opcode::Opcode;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    lookup_in_class_mro, ClassData, CompareOp, GeneratorState, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
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

    /// Create a new empty shared globals map.
    pub fn new_globals() -> SharedGlobals {
        Arc::new(RwLock::new(IndexMap::new()))
    }

    /// Execute a code object (module-level).
    pub fn execute(&mut self, code: CodeObject) -> PyResult<PyObjectRef> {
        let globals = Arc::new(RwLock::new(IndexMap::new()));
        // Set __name__ = "__main__" for top-level scripts
        globals.write().insert(
            CompactString::from("__name__"),
            PyObject::str_val(CompactString::from("__main__")),
        );
        self.execute_with_globals(code, globals)
    }

    /// Execute a code object with shared globals (for REPL).
    pub fn execute_with_globals(&mut self, code: CodeObject, globals: SharedGlobals) -> PyResult<PyObjectRef> {
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

    /// Unified class instantiation: __new__, dataclass/namedtuple auto-init, __init__, exception attrs.
    fn instantiate_class(
        &mut self,
        cls: &PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        // __new__
        let instance = if let Some(new_method) = cls.get_attr("__new__") {
            let new_fn = match &new_method.payload {
                PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                _ => new_method.clone(),
            };
            let mut new_args = vec![cls.clone()];
            new_args.extend(pos_args.clone());
            self.call_object(new_fn, new_args)?
        } else {
            PyObject::instance(cls.clone())
        };

        let is_dataclass = cls.get_attr("__dataclass__").is_some();
        let has_user_init = cls.get_attr("__init__").is_some();

        if is_dataclass && !has_user_init {
            // Dataclass auto-init: populate fields from args/kwargs
            if let Some(fields) = cls.get_attr("__dataclass_fields__") {
                if let PyObjectPayload::Tuple(field_tuples) = &fields.payload {
                    let mut arg_idx = 0;
                    for ft in field_tuples {
                        if let PyObjectPayload::Tuple(info) = &ft.payload {
                            let name = info[0].py_to_string();
                            let has_default = info[1].is_truthy();
                            let default_val = &info[2];

                            let value = if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == name.as_str()) {
                                v.clone()
                            } else if arg_idx < pos_args.len() {
                                let v = pos_args[arg_idx].clone();
                                arg_idx += 1;
                                v
                            } else if has_default {
                                default_val.clone()
                            } else {
                                return Err(PyException::type_error(format!(
                                    "__init__() missing required argument: '{}'", name
                                )));
                            };

                            if let PyObjectPayload::Instance(inst) = &instance.payload {
                                inst.attrs.write().insert(CompactString::from(name.as_str()), value);
                            }
                        }
                    }
                }
            }
        } else if cls.get_attr("__namedtuple__").is_some() {
            // Namedtuple: populate named fields
            if let Some(fields) = cls.get_attr("_fields") {
                if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                    if let PyObjectPayload::Instance(inst) = &instance.payload {
                        let mut attrs = inst.attrs.write();
                        for (i, field) in field_names.iter().enumerate() {
                            let name = field.py_to_string();
                            let value = if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == name.as_str()) {
                                v.clone()
                            } else if i < pos_args.len() {
                                pos_args[i].clone()
                            } else {
                                PyObject::none()
                            };
                            attrs.insert(CompactString::from(name.as_str()), value);
                        }
                        attrs.insert(CompactString::from("_tuple"), PyObject::tuple(pos_args.clone()));
                    }
                }
            }
        } else if let Some(init) = cls.get_attr("__init__") {
            // Normal __init__
            let init_fn = match &init.payload {
                PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                _ => init.clone(),
            };
            let mut init_args = vec![instance.clone()];
            init_args.extend(pos_args.clone());
            if kwargs.is_empty() {
                self.call_object(init_fn, init_args)?;
            } else {
                self.call_object_kw(init_fn, init_args, kwargs)?;
            }
        }

        // Exception subclass attrs
        if Self::is_exception_class(cls) {
            if let PyObjectPayload::Instance(inst) = &instance.payload {
                let mut attrs = inst.attrs.write();
                if !attrs.contains_key("args") {
                    attrs.insert(CompactString::from("args"), PyObject::tuple(pos_args.clone()));
                }
                if !attrs.contains_key("message") && !pos_args.is_empty() {
                    attrs.insert(CompactString::from("message"), pos_args[0].clone());
                }
            }
        }

        Ok(instance)
    }

    /// Build a super() proxy from current call frame or explicit args.
    fn make_super(&self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            let frame = self.call_stack.last().unwrap();
            if let Some(self_obj) = frame.locals.first().cloned().flatten() {
                let qualname = frame.code.qualname.as_str();
                let defining_class_name = qualname.rsplit_once('.')
                    .map(|(cls_part, _)| {
                        cls_part.rsplit_once('.').map(|(_, c)| c).unwrap_or(cls_part)
                    });

                let (runtime_cls, instance_for_super) = match &self_obj.payload {
                    PyObjectPayload::Instance(inst) => (inst.class.clone(), self_obj.clone()),
                    PyObjectPayload::Class(_) => (self_obj.clone(), self_obj.clone()),
                    _ => return Err(PyException::runtime_error("super(): no current class")),
                };

                let mut cls = runtime_cls.clone();
                if let Some(def_name) = defining_class_name {
                    if let PyObjectPayload::Class(cd) = &runtime_cls.payload {
                        let mro = if cd.mro.is_empty() {
                            vec![runtime_cls.clone()]
                        } else {
                            cd.mro.clone()
                        };
                        for m in &mro {
                            if let PyObjectPayload::Class(mc) = &m.payload {
                                if mc.name.as_str() == def_name {
                                    cls = m.clone();
                                    break;
                                }
                            }
                        }
                    }
                }
                return Ok(Arc::new(PyObject {
                    payload: PyObjectPayload::Super { cls, instance: instance_for_super }
                }));
            }
            Err(PyException::runtime_error("super(): no current class"))
        } else if args.len() == 2 {
            Ok(Arc::new(PyObject {
                payload: PyObjectPayload::Super { cls: args[0].clone(), instance: args[1].clone() }
            }))
        } else {
            Err(PyException::type_error("super() takes 0 or 2 arguments"))
        }
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
                self.instantiate_class(&func, pos_args, kwargs)
            }
            _ => {
                // For BuiltinBoundMethod on str.format, pass kwargs as a dict
                if let PyObjectPayload::BuiltinBoundMethod { receiver, method_name } = &func.payload {
                    // Handle list.sort(key=..., reverse=...)
                    if method_name.as_str() == "sort" {
                        if let PyObjectPayload::List(items_arc) = &receiver.payload {
                            let mut items_vec = items_arc.read().clone();
                            let key_fn = kwargs.iter().find(|(k, _)| k.as_str() == "key").map(|(_, v)| v.clone());
                            let reverse = kwargs.iter().find(|(k, _)| k.as_str() == "reverse")
                                .map(|(_, v)| v.is_truthy()).unwrap_or(false);
                            if let Some(key) = key_fn {
                                let mut decorated: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
                                for item in &items_vec {
                                    let k = self.call_object(key.clone(), vec![item.clone()])?;
                                    decorated.push((k, item.clone()));
                                }
                                decorated.sort_by(|(a, _), (b, _)| {
                                    builtins::partial_cmp_for_sort(a, b).unwrap_or(std::cmp::Ordering::Equal)
                                });
                                items_vec = decorated.into_iter().map(|(_, v)| v).collect();
                            } else {
                                items_vec.sort_by(|a, b| {
                                    builtins::partial_cmp_for_sort(a, b).unwrap_or(std::cmp::Ordering::Equal)
                                });
                            }
                            if reverse { items_vec.reverse(); }
                            *items_arc.write() = items_vec;
                            return Ok(PyObject::none());
                        }
                    }
                    if method_name.as_str() == "format" && !kwargs.is_empty() {
                        if let PyObjectPayload::Str(s) = &receiver.payload {
                            // Handle str.format() with named args
                            let mut result = String::new();
                            let mut chars = s.chars().peekable();
                            let mut arg_idx = 0usize;
                            while let Some(c) = chars.next() {
                                if c == '{' {
                                    if chars.peek() == Some(&'{') {
                                        chars.next();
                                        result.push('{');
                                    } else if chars.peek() == Some(&'}') {
                                        chars.next();
                                        if arg_idx < pos_args.len() {
                                            result.push_str(&pos_args[arg_idx].py_to_string());
                                            arg_idx += 1;
                                        }
                                    } else {
                                        let mut field = String::new();
                                        for c in chars.by_ref() {
                                            if c == '}' { break; }
                                            field.push(c);
                                        }
                                        // Try numeric index first
                                        if let Ok(idx) = field.parse::<usize>() {
                                            if idx < pos_args.len() {
                                                result.push_str(&pos_args[idx].py_to_string());
                                            }
                                        } else {
                                            // Named arg lookup
                                            let found = kwargs.iter().find(|(k, _)| k.as_str() == field);
                                            if let Some((_, v)) = found {
                                                result.push_str(&v.py_to_string());
                                            }
                                        }
                                    }
                                } else if c == '}' && chars.peek() == Some(&'}') {
                                    chars.next();
                                    result.push('}');
                                } else {
                                    result.push(c);
                                }
                            }
                            return Ok(PyObject::str_val(CompactString::from(result)));
                        }
                    }
                }
                // Fall back to call_object for builtins etc
                // Handle builtins with keyword args
                let builtin_name = match &func.payload {
                    PyObjectPayload::BuiltinFunction(name) => Some(name.clone()),
                    PyObjectPayload::BuiltinType(name) => Some(name.clone()),
                    _ => None,
                };
                if let Some(name) = builtin_name {
                    match name.as_str() {
                        "sorted" => {
                            if !pos_args.is_empty() {
                                let items = self.collect_iterable(&pos_args[0])?;
                                let mut items_vec = items;
                                let key_fn = kwargs.iter().find(|(k, _)| k.as_str() == "key").map(|(_, v)| v.clone());
                                let reverse = kwargs.iter().find(|(k, _)| k.as_str() == "reverse")
                                    .map(|(_, v)| v.is_truthy()).unwrap_or(false);
                                if let Some(key) = key_fn {
                                    // Decorate-sort-undecorate (Schwartzian transform)
                                    let mut decorated: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
                                    for item in &items_vec {
                                        let k = self.call_object(key.clone(), vec![item.clone()])?;
                                        decorated.push((k, item.clone()));
                                    }
                                    decorated.sort_by(|(a, _), (b, _)| {
                                        builtins::partial_cmp_for_sort(a, b).unwrap_or(std::cmp::Ordering::Equal)
                                    });
                                    items_vec = decorated.into_iter().map(|(_, v)| v).collect();
                                } else {
                                    items_vec.sort_by(|a, b| {
                                        builtins::partial_cmp_for_sort(a, b).unwrap_or(std::cmp::Ordering::Equal)
                                    });
                                }
                                if reverse {
                                    items_vec.reverse();
                                }
                                return Ok(PyObject::list(items_vec));
                            }
                        }
                        "print" => {
                            let sep = kwargs.iter().find(|(k, _)| k.as_str() == "sep")
                                .map(|(_, v)| v.py_to_string()).unwrap_or_else(|| " ".to_string());
                            let end = kwargs.iter().find(|(k, _)| k.as_str() == "end")
                                .map(|(_, v)| v.py_to_string()).unwrap_or_else(|| "\n".to_string());
                            let mut parts = Vec::new();
                            for a in &pos_args {
                                parts.push(self.vm_str(a)?);
                            }
                            print!("{}{}", parts.join(&sep), end);
                            return Ok(PyObject::none());
                        }
                        "max" | "min" => {
                            let is_max = name.as_str() == "max";
                            let key_fn = kwargs.iter().find(|(k, _)| k.as_str() == "key").map(|(_, v)| v.clone());
                            let default = kwargs.iter().find(|(k, _)| k.as_str() == "default").map(|(_, v)| v.clone());
                            let items = if pos_args.len() == 1 {
                                self.collect_iterable(&pos_args[0])?
                            } else {
                                pos_args.clone()
                            };
                            if items.is_empty() {
                                return if let Some(d) = default {
                                    Ok(d)
                                } else {
                                    Err(PyException::value_error(format!("{}() arg is an empty sequence", name)))
                                };
                            }
                            let mut best = items[0].clone();
                            let mut best_key = if let Some(ref kf) = key_fn {
                                self.call_object(kf.clone(), vec![best.clone()])?
                            } else { best.clone() };
                            for item in &items[1..] {
                                let item_key = if let Some(ref kf) = key_fn {
                                    self.call_object(kf.clone(), vec![item.clone()])?
                                } else { item.clone() };
                                let cmp = builtins::partial_cmp_for_sort(&item_key, &best_key);
                                let better = if is_max {
                                    cmp == Some(std::cmp::Ordering::Greater)
                                } else {
                                    cmp == Some(std::cmp::Ordering::Less)
                                };
                                if better {
                                    best = item.clone();
                                    best_key = item_key;
                                }
                            }
                            return Ok(best);
                        }
                        "super" => {
                            return self.make_super(&pos_args);
                        }
                        _ => {}
                    }
                }
                // Handle other payload types that support kwargs
                match &func.payload {
                    PyObjectPayload::NativeFunction { func: nf, name } => {
                        if name.as_str() == "functools.partial" {
                            // functools.partial(func, *args, **kwargs)
                            if pos_args.is_empty() {
                                return Err(PyException::type_error("partial() requires at least 1 argument"));
                            }
                            let pf = pos_args[0].clone();
                            let pa = if pos_args.len() > 1 { pos_args[1..].to_vec() } else { vec![] };
                            return Ok(PyObject::wrap(PyObjectPayload::Partial {
                                func: pf, args: pa, kwargs,
                            }));
                        }
                        // Other native functions: drop kwargs
                        return nf(&pos_args);
                    }
                    PyObjectPayload::Partial { func: partial_func, args: partial_args, kwargs: partial_kwargs } => {
                        let partial_func = partial_func.clone();
                        let mut combined_args = partial_args.clone();
                        combined_args.extend(pos_args);
                        let mut combined_kw = partial_kwargs.clone();
                        combined_kw.extend(kwargs);
                        if combined_kw.is_empty() {
                            return self.call_object(partial_func, combined_args);
                        } else {
                            return self.call_object_kw(partial_func, combined_args, combined_kw);
                        }
                    }
                    PyObjectPayload::ExceptionType(kind) => {
                        let msg = if pos_args.is_empty() { String::new() } else { pos_args[0].py_to_string() };
                        return Ok(PyObject::exception_instance(kind.clone(), msg));
                    }
                    PyObjectPayload::Instance(_) => {
                        if let Some(method) = func.get_attr("__call__") {
                            return self.call_object_kw(method, pos_args, kwargs);
                        }
                        return Err(PyException::type_error(format!(
                            "'{}' object is not callable", func.type_name()
                        )));
                    }
                    _ => {}
                }
                // Final fallback: merge kwargs into positional (lossy but functional)
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
                        // If the exception has an original Instance, use it as the value
                        // and push its class as the type (for proper class-based matching)
                        let (exc_value, exc_type) = if let Some(orig) = &exc.original {
                            let cls = if let PyObjectPayload::Instance(inst) = &orig.payload {
                                inst.class.clone()
                            } else {
                                PyObject::exception_type(exc.kind.clone())
                            };
                            (orig.clone(), cls)
                        } else {
                            (
                                PyObject::exception_instance(exc.kind.clone(), exc.message.clone()),
                                PyObject::exception_type(exc.kind.clone()),
                            )
                        };
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
        // Helper: pop two values from the current frame
        macro_rules! pop2 {
            () => {{
                let f = self.call_stack.last_mut().unwrap();
                let b = f.pop();
                let a = f.pop();
                (a, b)
            }};
        }
        macro_rules! push {
            ($val:expr) => {{
                self.call_stack.last_mut().unwrap().push($val);
            }};
        }
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
                Opcode::DupTopTwo => {
                    let top = frame.stack[frame.stack.len() - 1].clone();
                    let second = frame.stack[frame.stack.len() - 2].clone();
                    frame.push(second);
                    frame.push(top);
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
                        Some(v) => {
                            // Handle property descriptor — call fget(obj)
                            if let PyObjectPayload::Property { fget, .. } = &v.payload {
                                if let Some(getter) = fget {
                                    let getter = getter.clone();
                                    let result = self.call_object(getter, vec![obj])?;
                                    let frame = self.call_stack.last_mut().unwrap();
                                    frame.push(result);
                                } else {
                                    return Err(PyException::attribute_error(format!(
                                        "unreadable attribute '{}'", name
                                    )));
                                }
                            } else if matches!(&obj.payload, PyObjectPayload::Module(_))
                                && matches!(&v.payload, PyObjectPayload::NativeFunction { .. })
                                && obj.get_attr("_bind_methods").is_some()
                            {
                                // Auto-bind NativeFunction methods on module-like objects
                                frame.push(Arc::new(PyObject {
                                    payload: PyObjectPayload::BoundMethod {
                                        receiver: obj,
                                        method: v,
                                    }
                                }));
                            } else {
                                frame.push(v);
                            }
                        }
                        None => {
                            // Try __getattr__ fallback for instances
                            if let PyObjectPayload::Instance(_) = &obj.payload {
                                if let Some(ga) = obj.get_attr("__getattr__") {
                                    let name_arg = PyObject::str_val(CompactString::from(name.as_str()));
                                    drop(frame);
                                    let result = self.call_object(ga, vec![name_arg])?;
                                    push!(result);
                                    return Ok(None);
                                }
                            }
                            return Err(PyException::attribute_error(format!(
                                "'{}' object has no attribute '{}'", obj.type_name(), name
                            )));
                        }
                    }
                }
                Opcode::StoreAttr => {
                    let name = frame.code.names[instr.arg as usize].clone();
                    let obj = frame.pop();   // TOS: the object
                    let value = frame.pop(); // TOS1: the value
                    // Check for property descriptor in class MRO
                    if let PyObjectPayload::Instance(inst) = &obj.payload {
                        if let Some(desc) = lookup_in_class_mro(&inst.class, &name) {
                            if let PyObjectPayload::Property { fset, .. } = &desc.payload {
                                if let Some(setter) = fset {
                                    let setter = setter.clone();
                                    drop(frame);
                                    self.call_object(setter, vec![obj, value])?;
                                    return Ok(None);
                                } else {
                                    return Err(PyException::attribute_error(format!(
                                        "can't set attribute '{}'", name
                                    )));
                                }
                            }
                        }
                    }
                    // Check for __setattr__ dunder in class MRO (not instance dict)
                    if let PyObjectPayload::Instance(inst) = &obj.payload {
                        if let Some(sa) = lookup_in_class_mro(&inst.class, "__setattr__") {
                            if matches!(&sa.payload, PyObjectPayload::Function(_)) {
                                let method = Arc::new(PyObject {
                                    payload: PyObjectPayload::BoundMethod {
                                        receiver: obj.clone(),
                                        method: sa,
                                    }
                                });
                                let name_arg = PyObject::str_val(name);
                                drop(frame);
                                self.call_object(method, vec![name_arg, value])?;
                                return Ok(None);
                            }
                        }
                    }
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
                    if let PyObjectPayload::Instance(_) = &v.payload {
                        if let Some(method) = v.get_attr("__pos__") {
                            drop(frame);
                            let result = self.call_object(method, vec![])?;
                            push!(result);
                            return Ok(None);
                        }
                    }
                    frame.push(v.positive()?);
                }
                Opcode::UnaryNegative => {
                    let v = frame.pop();
                    if let PyObjectPayload::Instance(_) = &v.payload {
                        if let Some(method) = v.get_attr("__neg__") {
                            drop(frame);
                            let result = self.call_object(method, vec![])?;
                            push!(result);
                            return Ok(None);
                        }
                    }
                    frame.push(v.negate()?);
                }
                Opcode::UnaryNot => {
                    let v = frame.pop();
                    drop(frame);
                    let truthy = self.vm_is_truthy(&v)?;
                    push!(PyObject::bool_val(!truthy));
                }
                Opcode::UnaryInvert => {
                    let v = frame.pop();
                    if let PyObjectPayload::Instance(_) = &v.payload {
                        if let Some(method) = v.get_attr("__invert__") {
                            drop(frame);
                            let result = self.call_object(method, vec![])?;
                            push!(result);
                            return Ok(None);
                        }
                    }
                    frame.push(v.invert()?);
                }

                // ── Binary operations ──
                Opcode::BinaryAdd => {
                    let b = frame.pop(); let a = frame.pop();
                    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = a.get_attr("__add__") {
                            drop(frame);
                            let r = self.call_object(m, vec![b])?;
                            push!(r); return Ok(None);
                        }
                    }
                    if matches!(&b.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = b.get_attr("__radd__") {
                            drop(frame);
                            let r = self.call_object(m, vec![a])?;
                            push!(r); return Ok(None);
                        }
                    }
                    frame.push(a.add(&b)?);
                }
                Opcode::InplaceAdd => {
                    let b = frame.pop(); let a = frame.pop();
                    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = a.get_attr("__iadd__").or_else(|| a.get_attr("__add__")) {
                            drop(frame);
                            let r = self.call_object(m, vec![b])?;
                            push!(r); return Ok(None);
                        }
                    }
                    frame.push(a.add(&b)?);
                }
                Opcode::BinarySubtract => {
                    let b = frame.pop(); let a = frame.pop();
                    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = a.get_attr("__sub__") {
                            drop(frame);
                            let r = self.call_object(m, vec![b])?;
                            push!(r); return Ok(None);
                        }
                    }
                    if matches!(&b.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = b.get_attr("__rsub__") {
                            drop(frame);
                            let r = self.call_object(m, vec![a])?;
                            push!(r); return Ok(None);
                        }
                    }
                    frame.push(a.sub(&b)?);
                }
                Opcode::InplaceSubtract => {
                    let b = frame.pop(); let a = frame.pop();
                    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = a.get_attr("__isub__").or_else(|| a.get_attr("__sub__")) {
                            drop(frame);
                            let r = self.call_object(m, vec![b])?;
                            push!(r); return Ok(None);
                        }
                    }
                    frame.push(a.sub(&b)?);
                }
                Opcode::BinaryMultiply => {
                    let b = frame.pop(); let a = frame.pop();
                    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = a.get_attr("__mul__") {
                            drop(frame);
                            let r = self.call_object(m, vec![b])?;
                            push!(r); return Ok(None);
                        }
                    }
                    if matches!(&b.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = b.get_attr("__rmul__") {
                            drop(frame);
                            let r = self.call_object(m, vec![a])?;
                            push!(r); return Ok(None);
                        }
                    }
                    frame.push(a.mul(&b)?);
                }
                Opcode::InplaceMultiply => {
                    let b = frame.pop(); let a = frame.pop();
                    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = a.get_attr("__imul__").or_else(|| a.get_attr("__mul__")) {
                            drop(frame);
                            let r = self.call_object(m, vec![b])?;
                            push!(r); return Ok(None);
                        }
                    }
                    frame.push(a.mul(&b)?);
                }
                Opcode::BinaryTrueDivide => {
                    let b = frame.pop(); let a = frame.pop();
                    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = a.get_attr("__truediv__") {
                            drop(frame);
                            let r = self.call_object(m, vec![b])?;
                            push!(r); return Ok(None);
                        }
                    }
                    if matches!(&b.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = b.get_attr("__rtruediv__") {
                            drop(frame);
                            let r = self.call_object(m, vec![a])?;
                            push!(r); return Ok(None);
                        }
                    }
                    frame.push(a.true_div(&b)?);
                }
                Opcode::InplaceTrueDivide => {
                    let b = frame.pop(); let a = frame.pop();
                    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = a.get_attr("__itruediv__").or_else(|| a.get_attr("__truediv__")) {
                            drop(frame);
                            let r = self.call_object(m, vec![b])?;
                            push!(r); return Ok(None);
                        }
                    }
                    frame.push(a.true_div(&b)?);
                }
                Opcode::BinaryFloorDivide => {
                    let b = frame.pop(); let a = frame.pop();
                    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = a.get_attr("__floordiv__") {
                            drop(frame);
                            let r = self.call_object(m, vec![b])?;
                            push!(r); return Ok(None);
                        }
                    }
                    if matches!(&b.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = b.get_attr("__rfloordiv__") {
                            drop(frame);
                            let r = self.call_object(m, vec![a])?;
                            push!(r); return Ok(None);
                        }
                    }
                    frame.push(a.floor_div(&b)?);
                }
                Opcode::InplaceFloorDivide => {
                    let b = frame.pop(); let a = frame.pop();
                    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = a.get_attr("__ifloordiv__").or_else(|| a.get_attr("__floordiv__")) {
                            drop(frame);
                            let r = self.call_object(m, vec![b])?;
                            push!(r); return Ok(None);
                        }
                    }
                    frame.push(a.floor_div(&b)?);
                }
                Opcode::BinaryModulo | Opcode::InplaceModulo => {
                    let b = frame.pop(); let a = frame.pop();
                    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = a.get_attr("__mod__") {
                            drop(frame);
                            let r = self.call_object(m, vec![b])?;
                            push!(r); return Ok(None);
                        }
                    }
                    if matches!(&b.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = b.get_attr("__rmod__") {
                            drop(frame);
                            let r = self.call_object(m, vec![a])?;
                            push!(r); return Ok(None);
                        }
                    }
                    frame.push(a.modulo(&b)?);
                }
                Opcode::BinaryPower | Opcode::InplacePower => {
                    let b = frame.pop(); let a = frame.pop();
                    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = a.get_attr("__pow__") {
                            drop(frame);
                            let r = self.call_object(m, vec![b])?;
                            push!(r); return Ok(None);
                        }
                    }
                    if matches!(&b.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = b.get_attr("__rpow__") {
                            drop(frame);
                            let r = self.call_object(m, vec![a])?;
                            push!(r); return Ok(None);
                        }
                    }
                    frame.push(a.power(&b)?);
                }
                Opcode::BinaryLshift | Opcode::InplaceLshift => {
                    let b = frame.pop(); let a = frame.pop();
                    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = a.get_attr("__lshift__") {
                            drop(frame);
                            let r = self.call_object(m, vec![b])?;
                            push!(r); return Ok(None);
                        }
                    }
                    frame.push(a.lshift(&b)?);
                }
                Opcode::BinaryRshift | Opcode::InplaceRshift => {
                    let b = frame.pop(); let a = frame.pop();
                    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = a.get_attr("__rshift__") {
                            drop(frame);
                            let r = self.call_object(m, vec![b])?;
                            push!(r); return Ok(None);
                        }
                    }
                    frame.push(a.rshift(&b)?);
                }
                Opcode::BinaryAnd | Opcode::InplaceAnd => {
                    let b = frame.pop(); let a = frame.pop();
                    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = a.get_attr("__and__") {
                            drop(frame);
                            let r = self.call_object(m, vec![b])?;
                            push!(r); return Ok(None);
                        }
                    }
                    frame.push(a.bit_and(&b)?);
                }
                Opcode::BinaryOr | Opcode::InplaceOr => {
                    let b = frame.pop(); let a = frame.pop();
                    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = a.get_attr("__or__") {
                            drop(frame);
                            let r = self.call_object(m, vec![b])?;
                            push!(r); return Ok(None);
                        }
                    }
                    frame.push(a.bit_or(&b)?);
                }
                Opcode::BinaryXor | Opcode::InplaceXor => {
                    let b = frame.pop(); let a = frame.pop();
                    if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = a.get_attr("__xor__") {
                            drop(frame);
                            let r = self.call_object(m, vec![b])?;
                            push!(r); return Ok(None);
                        }
                    }
                    frame.push(a.bit_xor(&b)?);
                }
                Opcode::BinarySubscr => {
                    let key = frame.pop();
                    let obj = frame.pop();
                    if matches!(&obj.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = obj.get_attr("__getitem__") {
                            drop(frame);
                            let r = self.call_object(m, vec![key])?;
                            push!(r); return Ok(None);
                        }
                        // namedtuple indexing: support int subscript via _tuple
                        if let Some(tup) = obj.get_attr("_tuple") {
                            frame.push(tup.get_item(&key)?);
                            return Ok(None);
                        }
                    }
                    // Handle defaultdict: on KeyError, check for factory
                    if let PyObjectPayload::Dict(map) = &obj.payload {
                        let hk = key.to_hashable_key()?;
                        let existing = map.read().get(&hk).cloned();
                        if let Some(val) = existing {
                            frame.push(val);
                        } else {
                            let factory_key = HashableKey::Str(CompactString::from("__defaultdict_factory__"));
                            let factory = map.read().get(&factory_key).cloned();
                            if let Some(factory) = factory {
                                drop(frame);
                                let default = self.call_object(factory, vec![])?;
                                map.write().insert(hk, default.clone());
                                push!(default);
                                return Ok(None);
                            } else {
                                return Err(PyException::key_error(key.py_to_string()));
                            }
                        }
                    } else {
                        frame.push(obj.get_item(&key)?);
                    }
                }
                Opcode::StoreSubscr => {
                    // Stack: TOS = key, TOS1 = obj, TOS2 = value
                    let key = frame.pop();
                    let obj = frame.pop();
                    let value = frame.pop();
                    match &obj.payload {
                        PyObjectPayload::List(items) => {
                            if let PyObjectPayload::Slice { start, stop, step: _ } = &key.payload {
                                // Slice assignment: lst[start:stop] = iterable
                                let new_items = value.to_list()?;
                                let mut w = items.write();
                                let len = w.len() as i64;
                                let s_val = start.as_ref().map(|v| v.as_int().unwrap_or(0)).unwrap_or(0);
                                let e_val = stop.as_ref().map(|v| v.as_int().unwrap_or(len)).unwrap_or(len);
                                let s = (if s_val < 0 { (len + s_val).max(0) } else { s_val.min(len) }) as usize;
                                let e = (if e_val < 0 { (len + e_val).max(0) } else { e_val.min(len) }) as usize;
                                let e = e.max(s);
                                w.splice(s..e, new_items);
                            } else {
                                let idx = key.to_int()?;
                                let mut w = items.write();
                                let len = w.len() as i64;
                                let actual = if idx < 0 { len + idx } else { idx };
                                if actual < 0 || actual >= len {
                                    return Err(PyException::index_error("list assignment index out of range"));
                                }
                                w[actual as usize] = value;
                            }
                        }
                        PyObjectPayload::Dict(map) => {
                            let hk = key.to_hashable_key()?;
                            map.write().insert(hk, value);
                        }
                        PyObjectPayload::Instance(_) => {
                            if let Some(m) = obj.get_attr("__setitem__") {
                                drop(frame);
                                self.call_object(m, vec![key, value])?;
                                return Ok(None);
                            } else {
                                return Err(PyException::type_error(format!(
                                    "'{}' object does not support item assignment", obj.type_name())));
                            }
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
                        PyObjectPayload::Instance(_) => {
                            if let Some(method) = obj.get_attr("__delitem__") {
                                drop(frame);
                                self.call_object(method, vec![key])?;
                                return Ok(None);
                            }
                            return Err(PyException::type_error(format!(
                                "'{}' object does not support item deletion", obj.type_name())));
                        }
                        _ => return Err(PyException::type_error(format!(
                            "'{}' object does not support item deletion", obj.type_name()))),
                    }
                }
                Opcode::DeleteAttr => {
                    let name = frame.code.names[instr.arg as usize].clone();
                    let obj = frame.pop();
                    match &obj.payload {
                        PyObjectPayload::Instance(inst) => {
                            // Check for __delattr__ dunder
                            if let Some(delattr_method) = lookup_in_class_mro(&inst.class, "__delattr__") {
                                if matches!(&delattr_method.payload, PyObjectPayload::Function(_)) {
                                    let method = Arc::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod { receiver: obj.clone(), method: delattr_method }
                                    });
                                    let name_arg = PyObject::str_val(name);
                                    drop(frame);
                                    self.call_object(method, vec![name_arg])?;
                                } else {
                                    if inst.attrs.write().swap_remove(name.as_str()).is_none() {
                                        return Err(PyException::attribute_error(format!(
                                            "'{}' object has no attribute '{}'", obj.type_name(), name)));
                                    }
                                }
                            } else {
                                if inst.attrs.write().swap_remove(name.as_str()).is_none() {
                                    return Err(PyException::attribute_error(format!(
                                        "'{}' object has no attribute '{}'", obj.type_name(), name)));
                                }
                            }
                        }
                        PyObjectPayload::Class(cd) => {
                            if cd.namespace.write().swap_remove(name.as_str()).is_none() {
                                return Err(PyException::attribute_error(format!(
                                    "type object has no attribute '{}'", name)));
                            }
                        }
                        _ => return Err(PyException::attribute_error(format!(
                            "'{}' object does not support attribute deletion", obj.type_name()))),
                    }
                }

                // ── Comparison ──
                Opcode::CompareOp => {
                    let b = frame.pop();
                    let a = frame.pop();
                    // Check for dunder compare methods on instances — need to drop frame
                    if let op @ 0..=5 = instr.arg {
                        let dunder = match op {
                            0 => "__lt__", 1 => "__le__", 2 => "__eq__",
                            3 => "__ne__", 4 => "__gt__", 5 => "__ge__",
                            _ => unreachable!()
                        };
                        if matches!(&a.payload, PyObjectPayload::Instance(_)) {
                            if let Some(method) = a.get_attr(dunder) {
                                drop(frame);
                                let r = self.call_object(method, vec![b])?;
                                push!(r); return Ok(None);
                            }
                            // Dataclass auto-equality
                            if op == 2 || op == 3 {
                                if let (PyObjectPayload::Instance(inst_a), PyObjectPayload::Instance(inst_b)) = (&a.payload, &b.payload) {
                                    let cls_a = &inst_a.class;
                                    if cls_a.get_attr("__dataclass__").is_some() {
                                        if let Some(fields) = cls_a.get_attr("__dataclass_fields__") {
                                            if let PyObjectPayload::Tuple(field_tuples) = &fields.payload {
                                                let attrs_a = inst_a.attrs.read();
                                                let attrs_b = inst_b.attrs.read();
                                                let mut eq = true;
                                                for ft in field_tuples {
                                                    if let PyObjectPayload::Tuple(info) = &ft.payload {
                                                        let name = info[0].py_to_string();
                                                        let va = attrs_a.get(name.as_str());
                                                        let vb = attrs_b.get(name.as_str());
                                                        match (va, vb) {
                                                            (Some(x), Some(y)) => {
                                                                if let Ok(r) = x.compare(y, CompareOp::Eq) {
                                                                    if !r.is_truthy() { eq = false; break; }
                                                                } else { eq = false; break; }
                                                            }
                                                            _ => { eq = false; break; }
                                                        }
                                                    }
                                                }
                                                let result = if op == 2 { eq } else { !eq };
                                                push!(PyObject::bool_val(result));
                                                return Ok(None);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // 'in' / 'not in' with __contains__
                    if instr.arg == 6 || instr.arg == 7 {
                        if matches!(&b.payload, PyObjectPayload::Instance(_)) {
                            if let Some(method) = b.get_attr("__contains__") {
                                drop(frame);
                                let r = self.call_object(method, vec![a])?;
                                let val = if instr.arg == 6 { r.is_truthy() } else { !r.is_truthy() };
                                push!(PyObject::bool_val(val)); return Ok(None);
                            }
                        }
                    }
                    let result = match instr.arg {
                        0 => a.compare(&b, CompareOp::Lt)?,
                        1 => a.compare(&b, CompareOp::Le)?,
                        2 => a.compare(&b, CompareOp::Eq)?,
                        3 => a.compare(&b, CompareOp::Ne)?,
                        4 => a.compare(&b, CompareOp::Gt)?,
                        5 => a.compare(&b, CompareOp::Ge)?,
                        6 => PyObject::bool_val(b.contains(&a)?),
                        7 => PyObject::bool_val(!b.contains(&a)?),
                        8 => PyObject::bool_val(a.is_same(&b)),     // is
                        9 => PyObject::bool_val(!a.is_same(&b)),    // is not
                        10 => {
                            // exception match: a is exception type on stack, b is type to match
                            // a can be ExceptionType or Class (for custom exceptions)
                            // b can be ExceptionType, Class, or Tuple of types
                            let match_one = |a_item: &PyObjectRef, b_item: &PyObjectRef| -> bool {
                                // Case 1: both are Class — check class identity/inheritance
                                if let PyObjectPayload::Class(cls_a) = &a_item.payload {
                                    if let PyObjectPayload::Class(cls_b) = &b_item.payload {
                                        // Check if a is b or a inherits from b
                                        if cls_a.name == cls_b.name {
                                            return true;
                                        }
                                        // Check MRO of a for b
                                        for base in &cls_a.mro {
                                            if let PyObjectPayload::Class(bc) = &base.payload {
                                                if bc.name == cls_b.name {
                                                    return true;
                                                }
                                            }
                                        }
                                        // Check bases
                                        for base in &cls_a.bases {
                                            if let PyObjectPayload::Class(bc) = &base.payload {
                                                if bc.name == cls_b.name {
                                                    return true;
                                                }
                                            }
                                        }
                                        return false;
                                    }
                                    // a is Class, b is ExceptionType — check if a inherits from that exception
                                    if let PyObjectPayload::ExceptionType(kind_b) = &b_item.payload {
                                        let kind_a = Self::find_exception_kind(a_item);
                                        return exception_kind_matches(&kind_a, kind_b);
                                    }
                                    return false;
                                }
                                // Case 2: a is ExceptionType
                                if let PyObjectPayload::ExceptionType(kind_a) = &a_item.payload {
                                    return match &b_item.payload {
                                        PyObjectPayload::ExceptionType(kind_b) => {
                                            exception_kind_matches(kind_a, kind_b)
                                        }
                                        PyObjectPayload::Class(_) => {
                                            let kind_b = Self::find_exception_kind(b_item);
                                            exception_kind_matches(kind_a, &kind_b)
                                        }
                                        PyObjectPayload::BuiltinType(name) => {
                                            if let Some(kind_b) = ExceptionKind::from_name(name) {
                                                exception_kind_matches(kind_a, &kind_b)
                                            } else {
                                                false
                                            }
                                        }
                                        _ => false,
                                    };
                                }
                                false
                            };
                            let matched = match &b.payload {
                                PyObjectPayload::Tuple(items) => {
                                    items.iter().any(|item| match_one(&a, item))
                                }
                                _ => match_one(&a, &b),
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
                    drop(frame);
                    if !self.vm_is_truthy(&v)? {
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.ip = instr.arg as usize;
                    }
                }
                Opcode::PopJumpIfTrue => {
                    let v = frame.pop();
                    drop(frame);
                    if self.vm_is_truthy(&v)? {
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.ip = instr.arg as usize;
                    }
                }
                Opcode::JumpIfTrueOrPop => {
                    let v = frame.peek().clone();
                    drop(frame);
                    if self.vm_is_truthy(&v)? {
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.ip = instr.arg as usize;
                    } else {
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.pop();
                    }
                }
                Opcode::JumpIfFalseOrPop => {
                    let v = frame.peek().clone();
                    drop(frame);
                    if !self.vm_is_truthy(&v)? {
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.ip = instr.arg as usize;
                    } else {
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.pop();
                    }
                }

                // ── Iterator operations ──
                Opcode::GetIter => {
                    let obj = frame.pop();
                    if matches!(&obj.payload, PyObjectPayload::Instance(_)) {
                        if let Some(m) = obj.get_attr("__iter__") {
                            drop(frame);
                            let r = self.call_object(m, vec![])?;
                            push!(r); return Ok(None);
                        }
                    }
                    frame.push(obj.get_iter()?);
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
                    } else if matches!(&iter.payload, PyObjectPayload::Instance(_)) {
                        // Custom iterator with __next__
                        if let Some(next_method) = iter.get_attr("__next__") {
                            drop(frame);
                            match self.call_object(next_method, vec![]) {
                                Ok(value) => {
                                    push!(value);
                                }
                                Err(e) if e.kind == ExceptionKind::StopIteration => {
                                    let f = self.call_stack.last_mut().unwrap();
                                    f.pop();
                                    f.ip = instr.arg as usize;
                                }
                                Err(e) => return Err(e),
                            }
                            return Ok(None);
                        } else {
                            return Err(PyException::type_error("iterator has no __next__ method"));
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
                        if let Ok(key) = item.to_hashable_key() {
                            s.write().insert(key, item);
                        }
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
                Opcode::DictUpdate | Opcode::DictMerge => {
                    let update_obj = frame.pop();
                    let idx = instr.arg as usize;
                    let stack_pos = frame.stack.len() - idx;
                    let dict_obj = &frame.stack[stack_pos];
                    if let PyObjectPayload::Dict(target) = &dict_obj.payload {
                        if let PyObjectPayload::Dict(source) = &update_obj.payload {
                            let src = source.read();
                            let mut tgt = target.write();
                            for (k, v) in src.iter() {
                                tgt.insert(k.clone(), v.clone());
                            }
                        }
                    }
                }
                Opcode::ListExtend => {
                    let iterable = frame.pop();
                    let idx = instr.arg as usize;
                    let stack_pos = frame.stack.len() - idx;
                    let list_obj = frame.stack[stack_pos].clone();
                    if let PyObjectPayload::List(items) = &list_obj.payload {
                        let new_items = iterable.to_list()?;
                        items.write().extend(new_items);
                    }
                }
                Opcode::SetUpdate => {
                    let iterable = frame.pop();
                    let idx = instr.arg as usize;
                    let stack_pos = frame.stack.len() - idx;
                    let set_obj = frame.stack[stack_pos].clone();
                    if let PyObjectPayload::Set(s) = &set_obj.payload {
                        let new_items = iterable.to_list()?;
                        let mut set = s.write();
                        for item in new_items {
                            if let Ok(key) = item.to_hashable_key() {
                                set.insert(key, item);
                            }
                        }
                    }
                }
                Opcode::ListToTuple => {
                    let list = frame.pop();
                    let items = list.to_list()?;
                    frame.push(PyObject::tuple(items));
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
                        Some(method) => {
                            // For Module objects with a _bind_methods marker,
                            // bind NativeFunction methods to the module (self pattern)
                            if matches!(&obj.payload, PyObjectPayload::Module(_))
                                && matches!(&method.payload, PyObjectPayload::NativeFunction { .. })
                                && obj.get_attr("_bind_methods").is_some()
                            {
                                frame.push(Arc::new(PyObject {
                                    payload: PyObjectPayload::BoundMethod {
                                        receiver: obj,
                                        method,
                                    }
                                }));
                            } else {
                                frame.push(method);
                            }
                        }
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
                    // Unwind block stack looking for Finally blocks
                    while let Some(block) = frame.block_stack.last() {
                        if block.kind == BlockKind::Finally {
                            let handler = block.handler;
                            frame.block_stack.pop();
                            frame.pending_return = Some(value.clone());
                            frame.push(PyObject::none()); // normal entry marker for finally
                            frame.ip = handler;
                            break;
                        } else {
                            frame.block_stack.pop();
                        }
                    }
                    if frame.pending_return.is_none() {
                        return Ok(Some(value));
                    }
                }

                // ── Import ──
                Opcode::ImportName => {
                    let _fromlist = frame.pop();
                    let _level = frame.pop();
                    let name = frame.code.names[instr.arg as usize].clone();
                    let filename = frame.code.filename.clone();
                    // Check module cache first
                    if let Some(module) = self.modules.get(&name) {
                        frame.push(module.clone());
                    } else {
                        // Try builtin module first, then filesystem
                        drop(frame);
                        let module = match self.load_builtin_module(&name) {
                            Ok(m) => m,
                            Err(_) => self.load_file_module(&name, &filename)?
                        };
                        self.modules.insert(name, module.clone());
                        push!(module);
                        return Ok(None);
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
                    let module = frame.pop();
                    // Copy all public names from module to local scope
                    if let PyObjectPayload::Module(mod_data) = &module.payload {
                        let all_names: Option<Vec<String>> = mod_data.attrs.get("__all__").and_then(|v| {
                            v.to_list().ok().map(|items| items.iter().map(|x| x.py_to_string()).collect())
                        });
                        let mut globals = frame.globals.write();
                        for (k, v) in &mod_data.attrs {
                            if k.starts_with('_') && all_names.is_none() { continue; }
                            if let Some(ref names) = all_names {
                                if !names.contains(&k.to_string()) { continue; }
                            }
                            globals.insert(k.clone(), v.clone());
                        }
                    }
                }

                // ── Exception handling ──
                Opcode::SetupFinally => {
                    frame.push_block(BlockKind::Finally, instr.arg as usize);
                }
                Opcode::SetupExcept => {
                    frame.push_block(BlockKind::Except, instr.arg as usize);
                }
                Opcode::PopBlock => { frame.pop_block(); }
                Opcode::PopExcept => { frame.pop_block(); }
                Opcode::EndFinally => {
                    // Check for pending return (from return-in-try-finally)
                    if let Some(ret_val) = frame.pending_return.take() {
                        // Check if there are more Finally blocks to unwind through
                        let mut has_finally = false;
                        while let Some(block) = frame.block_stack.last() {
                            if block.kind == BlockKind::Finally {
                                let handler = block.handler;
                                frame.block_stack.pop();
                                frame.pending_return = Some(ret_val.clone());
                                frame.push(PyObject::none());
                                frame.ip = handler;
                                has_finally = true;
                                break;
                            } else {
                                frame.block_stack.pop();
                            }
                        }
                        if !has_finally {
                            return Ok(Some(ret_val));
                        }
                    } else {
                        // Normal EndFinally: check TOS for exception re-raise
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
                }
                Opcode::BeginFinally => {
                    // Push None to indicate normal (non-exception) entry into finally
                    frame.push(PyObject::none());
                }
                Opcode::RaiseVarargs => {
                    let raise_exc = |exc: &PyObjectRef| -> PyException {
                        match &exc.payload {
                            PyObjectPayload::ExceptionInstance { kind, message, .. } => {
                                PyException::new(kind.clone(), message.to_string())
                            }
                            PyObjectPayload::ExceptionType(kind) => {
                                PyException::new(kind.clone(), "")
                            }
                            PyObjectPayload::Instance(inst) => {
                                // Check if class inherits from Exception
                                let kind = Self::find_exception_kind(&inst.class);
                                let msg = if let Some(m) = exc.get_attr("message") {
                                    m.py_to_string()
                                } else {
                                    exc.py_to_string()
                                };
                                PyException::with_original(kind, msg, exc.clone())
                            }
                            PyObjectPayload::Class(_) => {
                                // Raising a class directly (not an instance)
                                let kind = Self::find_exception_kind(exc);
                                PyException::new(kind, "")
                            }
                            _ => PyException::runtime_error(exc.py_to_string()),
                        }
                    };
                    match instr.arg {
                        0 => return Err(PyException::runtime_error(
                            "No active exception to re-raise")),
                        1 => {
                            let exc = frame.pop();
                            return Err(raise_exc(&exc));
                        }
                        2 => {
                            let _cause = frame.pop();
                            let exc = frame.pop();
                            return Err(raise_exc(&exc));
                        }
                        _ => return Err(PyException::runtime_error(
                            "bad RAISE_VARARGS arg")),
                    }
                }
                Opcode::SetupWith => {
                    // TOS is the context manager
                    let ctx_mgr = frame.pop();
                    
                    // Handle generator-based context managers (@contextmanager)
                    if let PyObjectPayload::Generator(gen_arc) = &ctx_mgr.payload {
                        // __enter__: resume generator to get yielded value
                        drop(frame);
                        let enter_result = match self.resume_generator(gen_arc, PyObject::none()) {
                            Ok(val) => val,
                            Err(e) if e.kind == ExceptionKind::StopIteration => PyObject::none(),
                            Err(e) => return Err(e),
                        };
                        let frame = self.call_stack.last_mut().unwrap();
                        // Save the generator as the "exit" handler
                        frame.push(ctx_mgr.clone());
                        frame.push_block(BlockKind::With, instr.arg as usize);
                        frame.push(enter_result);
                    } else {
                        // Standard context manager protocol
                        let exit_method = ctx_mgr.get_attr("__exit__").ok_or_else(||
                            PyException::attribute_error("__exit__"))?;
                        frame.push(exit_method);
                        // Call __enter__, passing ctx_mgr as self for non-bound methods
                        let enter_method = ctx_mgr.get_attr("__enter__").ok_or_else(||
                            PyException::attribute_error("__enter__"))?;
                        let enter_args = if matches!(&enter_method.payload, PyObjectPayload::BoundMethod { .. }) {
                            vec![]
                        } else {
                            vec![ctx_mgr.clone()]
                        };
                        let enter_result = self.call_object(enter_method, enter_args)?;
                        let frame = self.call_stack.last_mut().unwrap();
                        // Setup the With block (points to cleanup handler)
                        frame.push_block(BlockKind::With, instr.arg as usize);
                        // Push the __enter__ result (to be stored by `as` or popped)
                        frame.push(enter_result);
                    }
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
                    let fmt_spec = if instr.arg & 0x04 != 0 {
                        let spec_obj = frame.pop();
                        spec_obj.as_str().unwrap_or("").to_string()
                    } else {
                        String::new()
                    };
                    let value = frame.pop();
                    let conversion = (instr.arg & 0x03) as u8;
                    let base_str = match conversion {
                        1 => value.py_to_string(),   // !s
                        2 => value.repr(),            // !r
                        3 => value.py_to_string(),    // !a (ascii)
                        _ => {
                            if !fmt_spec.is_empty() {
                                // Check for __format__ on Instance
                                if matches!(&value.payload, PyObjectPayload::Instance(_)) {
                                    if let Some(format_method) = value.get_attr("__format__") {
                                        drop(frame);
                                        let spec = PyObject::str_val(CompactString::from(&fmt_spec));
                                        let r = self.call_object(format_method, vec![spec])?;
                                        push!(PyObject::str_val(CompactString::from(r.py_to_string())));
                                        return Ok(None);
                                    }
                                }
                                // Apply format spec to the value directly
                                match value.format_value(&fmt_spec) {
                                    Ok(s) => s,
                                    Err(_) => value.py_to_string(),
                                }
                            } else {
                                // Check Instance __format__ first, then __str__ via VM
                                if matches!(&value.payload, PyObjectPayload::Instance(_)) {
                                    if let Some(format_method) = value.get_attr("__format__") {
                                        drop(frame);
                                        let spec = PyObject::str_val(CompactString::from(""));
                                        let r = self.call_object(format_method, vec![spec])?;
                                        push!(PyObject::str_val(CompactString::from(r.py_to_string())));
                                        return Ok(None);
                                    }
                                    if let Some(str_method) = value.get_attr("__str__") {
                                        drop(frame);
                                        let r = self.call_object(str_method, vec![])?;
                                        let s = r.py_to_string();
                                        push!(PyObject::str_val(CompactString::from(s)));
                                        return Ok(None);
                                    }
                                }
                                value.py_to_string()
                            }
                        }
                    };
                    let formatted = if !fmt_spec.is_empty() && conversion != 0 {
                        // If there's a format spec AND a conversion, apply spec to converted string
                        use ferrython_core::object::apply_string_format_spec;
                        apply_string_format_spec(&base_str, &fmt_spec)
                    } else {
                        base_str
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
                    // yield from <iterable>: delegate to sub-iterator
                    // TOS = send_value (from .send()), TOS1 = sub-iterator
                    let send_val = frame.pop();
                    let sub_iter = frame.peek().clone();

                    if let PyObjectPayload::Generator(ref gen_arc) = sub_iter.payload {
                        let gen_arc = gen_arc.clone();
                        drop(frame);
                        match self.resume_generator(&gen_arc, send_val) {
                            Ok(yielded) => {
                                let frame = self.call_stack.last_mut().unwrap();
                                frame.yielded = true;
                                frame.ip -= 1; // re-enter YieldFrom next time
                                return Ok(Some(yielded));
                            }
                            Err(e) if e.kind == ExceptionKind::StopIteration => {
                                let frame = self.call_stack.last_mut().unwrap();
                                frame.pop(); // remove exhausted sub-generator
                                frame.push(PyObject::none());
                            }
                            Err(e) => return Err(e),
                        }
                    } else if matches!(&sub_iter.payload, PyObjectPayload::Instance(_)) {
                        if let Some(next_method) = sub_iter.get_attr("__next__") {
                            drop(frame);
                            match self.call_object(next_method, vec![]) {
                                Ok(val) => {
                                    let frame = self.call_stack.last_mut().unwrap();
                                    frame.yielded = true;
                                    frame.ip -= 1;
                                    return Ok(Some(val));
                                }
                                Err(e) if e.kind == ExceptionKind::StopIteration => {
                                    let frame = self.call_stack.last_mut().unwrap();
                                    frame.pop();
                                    frame.push(PyObject::none());
                                }
                                Err(e) => return Err(e),
                            }
                        } else {
                            frame.pop();
                            frame.push(PyObject::none());
                        }
                    } else {
                        // Regular iterator (IteratorData)
                        match builtins::iter_advance(&sub_iter)? {
                            Some((new_iter, value)) => {
                                frame.pop(); // remove old iterator
                                frame.push(new_iter); // push advanced iterator
                                frame.yielded = true;
                                frame.ip -= 1;
                                return Ok(Some(value));
                            }
                            None => {
                                frame.pop(); // exhausted
                                frame.push(PyObject::none());
                            }
                        }
                    }
                }

                // ── With cleanup ──
                Opcode::WithCleanupStart => {
                    let tos = frame.peek().clone();
                    if matches!(tos.payload, PyObjectPayload::None) {
                        frame.pop(); // pop None
                        let exit_fn = frame.pop();
                        // Handle generator-based context manager exit
                        if let PyObjectPayload::Generator(gen_arc) = &exit_fn.payload {
                            drop(frame);
                            // Resume generator to run cleanup code
                            match self.resume_generator(gen_arc, PyObject::none()) {
                                Ok(_) => {} // generator yielded again (shouldn't happen)
                                Err(e) if e.kind == ExceptionKind::StopIteration => {} // normal
                                Err(e) => return Err(e),
                            }
                            let frame = self.call_stack.last_mut().unwrap();
                            frame.push(PyObject::none());
                            frame.push(PyObject::none());
                        } else {
                            let result = self.call_object(exit_fn, vec![
                                PyObject::none(), PyObject::none(), PyObject::none()
                            ])?;
                            let frame = self.call_stack.last_mut().unwrap();
                            frame.push(PyObject::none());
                            frame.push(result);
                        }
                    } else if matches!(tos.payload, PyObjectPayload::ExceptionType(_)) {
                        let exc_type = frame.pop();
                        let exc_val = if !frame.stack.is_empty() { frame.pop() } else { PyObject::none() };
                        let exc_tb = if !frame.stack.is_empty() { frame.pop() } else { PyObject::none() };
                        let exit_fn = frame.pop();
                        if let PyObjectPayload::Generator(gen_arc) = &exit_fn.payload {
                            drop(frame);
                            match self.resume_generator(gen_arc, PyObject::none()) {
                                Ok(_) => {}
                                Err(e) if e.kind == ExceptionKind::StopIteration => {}
                                Err(e) => return Err(e),
                            }
                            let frame = self.call_stack.last_mut().unwrap();
                            frame.push(exc_type.clone());
                            frame.push(PyObject::none());
                        } else {
                            let result = self.call_object(exit_fn, vec![
                                exc_type.clone(), exc_val, exc_tb
                            ])?;
                            let frame = self.call_stack.last_mut().unwrap();
                            frame.push(exc_type);
                            frame.push(result);
                        }
                    } else {
                        frame.pop();
                        let exit_fn = frame.pop();
                        if let PyObjectPayload::Generator(gen_arc) = &exit_fn.payload {
                            drop(frame);
                            match self.resume_generator(gen_arc, PyObject::none()) {
                                Ok(_) => {}
                                Err(e) if e.kind == ExceptionKind::StopIteration => {}
                                Err(e) => return Err(e),
                            }
                            let frame = self.call_stack.last_mut().unwrap();
                            frame.push(PyObject::none());
                            frame.push(PyObject::none());
                        } else {
                            let result = self.call_object(exit_fn, vec![
                                PyObject::none(), PyObject::none(), PyObject::none()
                            ])?;
                            let frame = self.call_stack.last_mut().unwrap();
                            frame.push(PyObject::none());
                            frame.push(result);
                        }
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

    /// Truthiness test that dispatches __bool__/__len__ on instances.
    /// Walk a class hierarchy to find if it inherits from an ExceptionType
    fn find_exception_kind(cls: &PyObjectRef) -> ExceptionKind {
        match &cls.payload {
            PyObjectPayload::ExceptionType(kind) => kind.clone(),
            PyObjectPayload::BuiltinType(name) => {
                ExceptionKind::from_name(name).unwrap_or(ExceptionKind::RuntimeError)
            }
            PyObjectPayload::Class(cd) => {
                for base in &cd.bases {
                    let kind = Self::find_exception_kind(base);
                    if !matches!(kind, ExceptionKind::RuntimeError) {
                        return kind;
                    }
                    // Also check if base IS the exception type
                    if let PyObjectPayload::ExceptionType(k) = &base.payload {
                        return k.clone();
                    }
                }
                // Check MRO
                for base in &cd.mro {
                    if let PyObjectPayload::ExceptionType(k) = &base.payload {
                        return k.clone();
                    }
                }
                ExceptionKind::RuntimeError
            }
            _ => ExceptionKind::RuntimeError,
        }
    }

    fn vm_is_truthy(&mut self, obj: &PyObjectRef) -> PyResult<bool> {
        if let PyObjectPayload::Instance(_) = &obj.payload {
            if let Some(bool_method) = obj.get_attr("__bool__") {
                let result = self.call_object(bool_method, vec![])?;
                return Ok(result.is_truthy());
            }
            if let Some(len_method) = obj.get_attr("__len__") {
                let result = self.call_object(len_method, vec![])?;
                return Ok(result.is_truthy());
            }
        }
        Ok(obj.is_truthy())
    }

    /// Produce a str() string for an object, dispatching __str__ on instances.
    /// For containers, uses vm_repr for elements (like CPython).
    /// Check if a class object inherits from Exception (via MRO or ExceptionType bases)
    fn is_exception_class(cls: &PyObjectRef) -> bool {
        if matches!(&cls.payload, PyObjectPayload::ExceptionType(_)) {
            return true;
        }
        if let PyObjectPayload::Class(cd) = &cls.payload {
            // Check if any base is an ExceptionType or an exception class
            for base in &cd.bases {
                if matches!(&base.payload, PyObjectPayload::ExceptionType(_)) {
                    return true;
                }
                if Self::is_exception_class(base) {
                    return true;
                }
            }
        }
        false
    }

    fn vm_str(&mut self, obj: &PyObjectRef) -> PyResult<String> {
        match &obj.payload {
            PyObjectPayload::Instance(_) => {
                if let Some(str_method) = obj.get_attr("__str__") {
                    let result = self.call_object(str_method, vec![])?;
                    return Ok(result.py_to_string());
                }
                // Fall back to __repr__ if no __str__
                if let Some(repr_method) = obj.get_attr("__repr__") {
                    let result = self.call_object(repr_method, vec![])?;
                    return Ok(result.py_to_string());
                }
                // Exception instances: str(e) returns the message from args
                if let Some(args) = obj.get_attr("args") {
                    if let PyObjectPayload::Tuple(items) = &args.payload {
                        return match items.len() {
                            0 => Ok(String::new()),
                            1 => Ok(items[0].py_to_string()),
                            _ => self.vm_repr(&args),
                        };
                    }
                }
                // Fall back to vm_repr (handles namedtuple, dataclass, etc.)
                self.vm_repr(obj)
            }
            // For containers, str() is same as repr() (elements use repr)
            PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) |
            PyObjectPayload::Dict(_) | PyObjectPayload::Set(_) |
            PyObjectPayload::FrozenSet(_) => self.vm_repr(obj),
            _ => Ok(obj.py_to_string()),
        }
    }

    /// Produce a repr string for an object, dispatching __repr__ on instances.
    fn vm_repr(&mut self, obj: &PyObjectRef) -> PyResult<String> {
        match &obj.payload {
            PyObjectPayload::Instance(inst) => {
                if let Some(repr_method) = obj.get_attr("__repr__") {
                    let result = self.call_object(repr_method, vec![])?;
                    return Ok(result.py_to_string());
                }
                // Dataclass auto-repr
                let class = &inst.class;
                if class.get_attr("__dataclass__").is_some() {
                    if let Some(fields) = class.get_attr("__dataclass_fields__") {
                        if let PyObjectPayload::Tuple(field_tuples) = &fields.payload {
                            let class_name = if let PyObjectPayload::Class(cd) = &class.payload {
                                cd.name.to_string()
                            } else { "?".to_string() };
                            let mut parts = Vec::new();
                            let attrs = inst.attrs.read();
                            for ft in field_tuples {
                                if let PyObjectPayload::Tuple(info) = &ft.payload {
                                    let name = info[0].py_to_string();
                                    if let Some(val) = attrs.get(name.as_str()) {
                                        let val_repr = self.vm_repr(val)?;
                                        parts.push(format!("{}={}", name, val_repr));
                                    }
                                }
                            }
                            return Ok(format!("{}({})", class_name, parts.join(", ")));
                        }
                    }
                }
                // Namedtuple auto-repr
                if class.get_attr("__namedtuple__").is_some() {
                    if let Some(fields) = class.get_attr("_fields") {
                        if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                            let class_name = if let PyObjectPayload::Class(cd) = &class.payload {
                                cd.name.to_string()
                            } else { "?".to_string() };
                            let mut parts = Vec::new();
                            let attrs = inst.attrs.read();
                            for field in field_names {
                                let name = field.py_to_string();
                                if let Some(val) = attrs.get(name.as_str()) {
                                    let val_repr = self.vm_repr(val)?;
                                    parts.push(format!("{}={}", name, val_repr));
                                }
                            }
                            return Ok(format!("{}({})", class_name, parts.join(", ")));
                        }
                    }
                }
                Ok(obj.repr())
            }
            PyObjectPayload::List(items) => {
                let items = items.read().clone();
                let mut parts = Vec::new();
                for item in &items {
                    parts.push(self.vm_repr(item)?);
                }
                Ok(format!("[{}]", parts.join(", ")))
            }
            PyObjectPayload::Tuple(items) => {
                let mut parts = Vec::new();
                for item in items {
                    parts.push(self.vm_repr(item)?);
                }
                if parts.len() == 1 {
                    Ok(format!("({},)", parts[0]))
                } else {
                    Ok(format!("({})", parts.join(", ")))
                }
            }
            PyObjectPayload::Dict(m) => {
                let m = m.read().clone();
                let mut parts = Vec::new();
                for (k, v) in &m {
                    // Hide defaultdict internal factory key
                    if let HashableKey::Str(s) = k {
                        if s.as_str() == "__defaultdict_factory__" { continue; }
                    }
                    let kr = self.vm_repr(&k.to_object())?;
                    let vr = self.vm_repr(v)?;
                    parts.push(format!("{}: {}", kr, vr));
                }
                Ok(format!("{{{}}}", parts.join(", ")))
            }
            PyObjectPayload::Set(m) => {
                let m = m.read().clone();
                if m.is_empty() { return Ok("set()".to_string()); }
                let mut parts = Vec::new();
                for v in m.values() {
                    parts.push(self.vm_repr(v)?);
                }
                Ok(format!("{{{}}}", parts.join(", ")))
            }
            _ => Ok(obj.repr()),
        }
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
                    "print" => {
                        let mut parts = Vec::new();
                        for a in &args {
                            parts.push(self.vm_str(a)?);
                        }
                        println!("{}", parts.join(" "));
                        return Ok(PyObject::none());
                    }
                    "str" => {
                        if args.is_empty() {
                            return Ok(PyObject::str_val(CompactString::from("")));
                        }
                        return self.vm_str(&args[0]).map(|s| PyObject::str_val(CompactString::from(s)));
                    }
                    "repr" => {
                        if args.is_empty() {
                            return Ok(PyObject::str_val(CompactString::from("")));
                        }
                        return self.vm_repr(&args[0]).map(|s| PyObject::str_val(CompactString::from(s)));
                    }
                    "map" => {
                        if args.len() < 2 {
                            return Err(PyException::type_error("map() requires at least 2 arguments"));
                        }
                        let func_obj = args[0].clone();
                        if args.len() == 2 {
                            let iterable = self.collect_iterable(&args[1])?;
                            let mut result = Vec::new();
                            for item in iterable {
                                result.push(self.call_object(func_obj.clone(), vec![item])?);
                            }
                            return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                                Arc::new(std::sync::Mutex::new(ferrython_core::object::IteratorData::List { items: result, index: 0 }))
                            )));
                        } else {
                            let mut iters: Vec<Vec<PyObjectRef>> = Vec::new();
                            for a in &args[1..] { iters.push(self.collect_iterable(a)?); }
                            let min_len = iters.iter().map(|v| v.len()).min().unwrap_or(0);
                            let mut result = Vec::new();
                            for i in 0..min_len {
                                let call_args: Vec<PyObjectRef> = iters.iter().map(|v| v[i].clone()).collect();
                                result.push(self.call_object(func_obj.clone(), call_args)?);
                            }
                            return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                                Arc::new(std::sync::Mutex::new(ferrython_core::object::IteratorData::List { items: result, index: 0 }))
                            )));
                        }
                    }
                    "filter" => {
                        if args.len() < 2 {
                            return Err(PyException::type_error("filter() requires at least 2 arguments"));
                        }
                        let func_obj = args[0].clone();
                        let iterable = self.collect_iterable(&args[1])?;
                        let mut result = Vec::new();
                        for item in iterable {
                            let keep = if matches!(func_obj.payload, PyObjectPayload::None) {
                                self.vm_is_truthy(&item)?
                            } else {
                                { let r = self.call_object(func_obj.clone(), vec![item.clone()])?; self.vm_is_truthy(&r)? }
                            };
                            if keep {
                                result.push(item);
                            }
                        }
                        return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                            Arc::new(std::sync::Mutex::new(ferrython_core::object::IteratorData::List { items: result, index: 0 }))
                        )));
                    }
                    "iter" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(iter_method) = args[0].get_attr("__iter__") {
                                    return self.call_object(iter_method, vec![]);
                                }
                            }
                            // Fall through to builtin dispatch for non-instances
                        }
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
                        // Instance with __next__
                        if let PyObjectPayload::Instance(_) = &args[0].payload {
                            if let Some(next_method) = args[0].get_attr("__next__") {
                                return match self.call_object(next_method, vec![]) {
                                    Ok(value) => Ok(value),
                                    Err(e) if e.kind == ExceptionKind::StopIteration && args.len() > 1 => {
                                        Ok(args[1].clone())
                                    }
                                    Err(e) => Err(e),
                                };
                            }
                        }
                        // Fall through to regular next() for iterators
                    }
                    "list" => {
                        if args.is_empty() {
                            return Ok(PyObject::list(vec![]));
                        }
                        let items = self.collect_iterable(&args[0])?;
                        return Ok(PyObject::list(items));
                    }
                    "tuple" => {
                        if args.is_empty() {
                            return Ok(PyObject::tuple(vec![]));
                        }
                        let items = self.collect_iterable(&args[0])?;
                        return Ok(PyObject::tuple(items));
                    }
                    "sum" => {
                        if args.is_empty() {
                            return Err(PyException::type_error("sum() requires at least 1 argument"));
                        }
                        let items = self.collect_iterable(&args[0])?;
                        let start = if args.len() > 1 { args[1].clone() } else { PyObject::int(0) };
                        let mut total = start;
                        for item in items {
                            total = total.add(&item)?;
                        }
                        return Ok(total);
                    }
                    "sorted" => {
                        if !args.is_empty() {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::dispatch("sorted", &[PyObject::list(items)]);
                        }
                    }
                    "set" => {
                        if args.is_empty() {
                            return builtins::dispatch("set", &[]);
                        }
                        let items = self.collect_iterable(&args[0])?;
                        return builtins::dispatch("set", &[PyObject::list(items)]);
                    }
                    "frozenset" => {
                        if args.is_empty() {
                            return builtins::dispatch("frozenset", &[]);
                        }
                        let items = self.collect_iterable(&args[0])?;
                        return builtins::dispatch("frozenset", &[PyObject::list(items)]);
                    }
                    "any" => {
                        if !args.is_empty() {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::dispatch("any", &[PyObject::list(items)]);
                        }
                    }
                    "all" => {
                        if !args.is_empty() {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::dispatch("all", &[PyObject::list(items)]);
                        }
                    }
                    "min" => {
                        if args.len() == 1 {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::dispatch("min", &items);
                        }
                    }
                    "max" => {
                        if args.len() == 1 {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::dispatch("max", &items);
                        }
                    }
                    "reversed" => {
                        if !args.is_empty() {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::dispatch("reversed", &[PyObject::list(items)]);
                        }
                    }
                    "enumerate" => {
                        if !args.is_empty() {
                            let items = self.collect_iterable(&args[0])?;
                            let mut new_args = vec![PyObject::list(items)];
                            if args.len() > 1 {
                                new_args.push(args[1].clone());
                            }
                            return builtins::dispatch("enumerate", &new_args);
                        }
                    }
                    "zip" => {
                        if !args.is_empty() {
                            let mut collected = Vec::new();
                            for a in args.iter() {
                                collected.push(PyObject::list(self.collect_iterable(a)?));
                            }
                            return builtins::dispatch("zip", &collected);
                        }
                    }
                    "len" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = args[0].get_attr("__len__") {
                                    return self.call_object(method, vec![]);
                                }
                            }
                        }
                    }
                    "abs" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = args[0].get_attr("__abs__") {
                                    return self.call_object(method, vec![]);
                                }
                            }
                        }
                    }
                    "hash" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = args[0].get_attr("__hash__") {
                                    return self.call_object(method, vec![]);
                                }
                            }
                        }
                    }
                    "int" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = args[0].get_attr("__int__") {
                                    return self.call_object(method, vec![]);
                                }
                            }
                        }
                    }
                    "float" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = args[0].get_attr("__float__") {
                                    return self.call_object(method, vec![]);
                                }
                            }
                        }
                    }
                    "bool" => {
                        if args.len() == 1 {
                            return Ok(PyObject::bool_val(self.vm_is_truthy(&args[0])?));
                        }
                    }
                    "super" => {
                        return self.make_super(&args);
                    }
                    "exec" => {
                        if args.is_empty() || args.len() > 3 {
                            return Err(PyException::type_error("exec() takes 1 to 3 arguments"));
                        }
                        let code_str = args[0].as_str().ok_or_else(||
                            PyException::type_error("exec() arg 1 must be a string"))?;
                        match ferrython_parser::parse(code_str, "<string>") {
                            Ok(module) => {
                                let mut compiler = ferrython_compiler::Compiler::new("<string>".to_string());
                                match compiler.compile_module(&module) {
                                    Ok(code) => {
                                        // Execute in same globals
                                        let globals = self.call_stack.last().unwrap().globals.clone();
                                        self.execute_with_globals(code, globals)?;
                                        return Ok(PyObject::none());
                                    }
                                    Err(_) => return Err(PyException::syntax_error("exec: compilation failed")),
                                }
                            }
                            Err(e) => return Err(PyException::syntax_error(format!("exec: {}", e))),
                        }
                    }
                    "eval" => {
                        if args.is_empty() || args.len() > 3 {
                            return Err(PyException::type_error("eval() takes 1 to 3 arguments"));
                        }
                        let code_str = args[0].as_str().ok_or_else(||
                            PyException::type_error("eval() arg 1 must be a string"))?;
                        // Wrap expression in a module that evaluates and returns it
                        let wrapped = format!("__eval_result__ = ({})", code_str);
                        match ferrython_parser::parse(&wrapped, "<string>") {
                            Ok(module) => {
                                let mut compiler = ferrython_compiler::Compiler::new("<string>".to_string());
                                match compiler.compile_module(&module) {
                                    Ok(code) => {
                                        let globals = self.call_stack.last().unwrap().globals.clone();
                                        self.execute_with_globals(code, globals.clone())?;
                                        // Retrieve the result from globals
                                        let result = globals.read().get("__eval_result__").cloned()
                                            .unwrap_or_else(PyObject::none);
                                        return Ok(result);
                                    }
                                    Err(_) => return Err(PyException::syntax_error("eval: compilation failed")),
                                }
                            }
                            Err(e) => return Err(PyException::syntax_error(format!("eval: {}", e))),
                        }
                    }
                    "globals" => {
                        let frame = self.call_stack.last().unwrap();
                        let g = frame.globals.read();
                        let pairs: Vec<(PyObjectRef, PyObjectRef)> = g.iter()
                            .map(|(k, v)| (PyObject::str_val(CompactString::from(k.as_str())), v.clone()))
                            .collect();
                        drop(g);
                        return Ok(PyObject::dict_from_pairs(pairs));
                    }
                    "locals" => {
                        let frame = self.call_stack.last().unwrap();
                        let mut pairs: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
                        for (i, name) in frame.code.varnames.iter().enumerate() {
                            if let Some(Some(val)) = frame.locals.get(i) {
                                pairs.push((PyObject::str_val(name.clone()), val.clone()));
                            }
                        }
                        for (i, name) in frame.code.cellvars.iter().chain(frame.code.freevars.iter()).enumerate() {
                            if let Some(cell) = frame.cells.get(i) {
                                let cell_val = cell.read();
                                if let Some(val) = cell_val.as_ref() {
                                    pairs.push((PyObject::str_val(name.clone()), val.clone()));
                                }
                            }
                        }
                        return Ok(PyObject::dict_from_pairs(pairs));
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
            PyObjectPayload::BuiltinType(name) => {
                // BuiltinType dispatch: same as BuiltinFunction for VM-aware builtins
                match name.as_str() {
                    "print" => {
                        let mut parts = Vec::new();
                        for a in &args {
                            parts.push(self.vm_str(a)?);
                        }
                        println!("{}", parts.join(" "));
                        return Ok(PyObject::none());
                    }
                    "str" => {
                        if args.is_empty() {
                            return Ok(PyObject::str_val(CompactString::from("")));
                        }
                        return self.vm_str(&args[0]).map(|s| PyObject::str_val(CompactString::from(s)));
                    }
                    "repr" => {
                        if args.is_empty() {
                            return Ok(PyObject::str_val(CompactString::from("")));
                        }
                        return self.vm_repr(&args[0]).map(|s| PyObject::str_val(CompactString::from(s)));
                    }
                    "map" => {
                        if args.len() < 2 {
                            return Err(PyException::type_error("map() requires at least 2 arguments"));
                        }
                        let func_obj = args[0].clone();
                        if args.len() == 2 {
                            let iterable = self.collect_iterable(&args[1])?;
                            let mut result = Vec::new();
                            for item in iterable {
                                result.push(self.call_object(func_obj.clone(), vec![item])?);
                            }
                            return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                                Arc::new(std::sync::Mutex::new(ferrython_core::object::IteratorData::List { items: result, index: 0 }))
                            )));
                        } else {
                            let mut iters: Vec<Vec<PyObjectRef>> = Vec::new();
                            for a in &args[1..] { iters.push(self.collect_iterable(a)?); }
                            let min_len = iters.iter().map(|v| v.len()).min().unwrap_or(0);
                            let mut result = Vec::new();
                            for i in 0..min_len {
                                let call_args: Vec<PyObjectRef> = iters.iter().map(|v| v[i].clone()).collect();
                                result.push(self.call_object(func_obj.clone(), call_args)?);
                            }
                            return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                                Arc::new(std::sync::Mutex::new(ferrython_core::object::IteratorData::List { items: result, index: 0 }))
                            )));
                        }
                    }
                    "filter" => {
                        if args.len() < 2 {
                            return Err(PyException::type_error("filter() requires at least 2 arguments"));
                        }
                        let func_obj = args[0].clone();
                        let iterable = self.collect_iterable(&args[1])?;
                        let mut result = Vec::new();
                        for item in iterable {
                            let keep = if matches!(func_obj.payload, PyObjectPayload::None) {
                                self.vm_is_truthy(&item)?
                            } else {
                                { let r = self.call_object(func_obj.clone(), vec![item.clone()])?; self.vm_is_truthy(&r)? }
                            };
                            if keep {
                                result.push(item);
                            }
                        }
                        return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                            Arc::new(std::sync::Mutex::new(ferrython_core::object::IteratorData::List { items: result, index: 0 }))
                        )));
                    }
                    "list" => {
                        if args.is_empty() { return Ok(PyObject::list(vec![])); }
                        let items = self.collect_iterable(&args[0])?;
                        return Ok(PyObject::list(items));
                    }
                    "tuple" => {
                        if args.is_empty() { return Ok(PyObject::tuple(vec![])); }
                        let items = self.collect_iterable(&args[0])?;
                        return Ok(PyObject::tuple(items));
                    }
                    "sum" => {
                        if args.is_empty() {
                            return Err(PyException::type_error("sum() requires at least 1 argument"));
                        }
                        let items = self.collect_iterable(&args[0])?;
                        let start = if args.len() > 1 { args[1].clone() } else { PyObject::int(0) };
                        let mut total = start;
                        for item in items {
                            total = total.add(&item)?;
                        }
                        return Ok(total);
                    }
                    "sorted" => {
                        if !args.is_empty() {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::dispatch("sorted", &[PyObject::list(items)]);
                        }
                    }
                    "set" => {
                        if args.is_empty() { return builtins::dispatch("set", &[]); }
                        let items = self.collect_iterable(&args[0])?;
                        return builtins::dispatch("set", &[PyObject::list(items)]);
                    }
                    "frozenset" => {
                        if args.is_empty() { return builtins::dispatch("frozenset", &[]); }
                        let items = self.collect_iterable(&args[0])?;
                        return builtins::dispatch("frozenset", &[PyObject::list(items)]);
                    }
                    "any" => {
                        if !args.is_empty() {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::dispatch("any", &[PyObject::list(items)]);
                        }
                    }
                    "all" => {
                        if !args.is_empty() {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::dispatch("all", &[PyObject::list(items)]);
                        }
                    }
                    "min" => {
                        if args.len() == 1 {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::dispatch("min", &items);
                        }
                    }
                    "max" => {
                        if args.len() == 1 {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::dispatch("max", &items);
                        }
                    }
                    "reversed" => {
                        if !args.is_empty() {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::dispatch("reversed", &[PyObject::list(items)]);
                        }
                    }
                    "enumerate" => {
                        if !args.is_empty() {
                            let items = self.collect_iterable(&args[0])?;
                            let mut new_args = vec![PyObject::list(items)];
                            if args.len() > 1 { new_args.push(args[1].clone()); }
                            return builtins::dispatch("enumerate", &new_args);
                        }
                    }
                    "super" => {
                        return self.make_super(&args);
                    }
                    "bool" => {
                        if args.len() == 1 {
                            return Ok(PyObject::bool_val(self.vm_is_truthy(&args[0])?));
                        }
                    }
                    "int" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = args[0].get_attr("__int__") {
                                    return self.call_object(method, vec![]);
                                }
                            }
                        }
                    }
                    "float" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = args[0].get_attr("__float__") {
                                    return self.call_object(method, vec![]);
                                }
                            }
                        }
                    }
                    "len" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = args[0].get_attr("__len__") {
                                    return self.call_object(method, vec![]);
                                }
                            }
                        }
                    }
                    "hash" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = args[0].get_attr("__hash__") {
                                    return self.call_object(method, vec![]);
                                }
                            }
                        }
                    }
                    "abs" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = args[0].get_attr("__abs__") {
                                    return self.call_object(method, vec![]);
                                }
                            }
                        }
                    }
                    "iter" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = args[0].get_attr("__iter__") {
                                    return self.call_object(method, vec![]);
                                }
                            }
                        }
                    }
                    "next" => {
                        if !args.is_empty() {
                            if let PyObjectPayload::Generator(ref gen_arc) = args[0].payload {
                                let gen_arc = gen_arc.clone();
                                return match self.resume_generator(&gen_arc, PyObject::none()) {
                                    Ok(value) => Ok(value),
                                    Err(e) if e.kind == ExceptionKind::StopIteration && args.len() > 1 => {
                                        Ok(args[1].clone())
                                    }
                                    Err(e) => Err(e),
                                };
                            }
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(next_method) = args[0].get_attr("__next__") {
                                    return match self.call_object(next_method, vec![]) {
                                        Ok(value) => Ok(value),
                                        Err(e) if e.kind == ExceptionKind::StopIteration && args.len() > 1 => {
                                            Ok(args[1].clone())
                                        }
                                        Err(e) => Err(e),
                                    };
                                }
                            }
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
                self.instantiate_class(&func, args, vec![])
            }
            PyObjectPayload::BoundMethod { receiver, method } => {
                let mut bound_args = vec![receiver.clone()];
                bound_args.extend(args);
                self.call_object(method.clone(), bound_args)
            }
            PyObjectPayload::BuiltinBoundMethod { receiver, method_name } => {
                // Generator methods need VM access
                if let PyObjectPayload::Generator(gen_arc) = &receiver.payload {
                    match method_name.as_str() {
                        "send" => {
                            let val = if args.is_empty() { PyObject::none() } else { args[0].clone() };
                            return self.resume_generator(gen_arc, val);
                        }
                        "throw" => {
                            // throw(type, value=None) — inject exception into generator
                            let msg = if args.len() >= 2 { args[1].py_to_string() } else { String::new() };
                            let kind = if !args.is_empty() {
                                if let PyObjectPayload::ExceptionType(k) = &args[0].payload {
                                    k.clone()
                                } else {
                                    ExceptionKind::RuntimeError
                                }
                            } else {
                                ExceptionKind::RuntimeError
                            };
                            // Throw the exception into the generator
                            let mut gen = gen_arc.write();
                            if gen.finished {
                                return Err(PyException::new(kind, msg));
                            }
                            gen.finished = true;
                            gen.frame = None;
                            return Err(PyException::new(kind, msg));
                        }
                        "close" => {
                            let mut gen = gen_arc.write();
                            gen.finished = true;
                            gen.frame = None;
                            return Ok(PyObject::none());
                        }
                        "__next__" => {
                            return self.resume_generator(gen_arc, PyObject::none());
                        }
                        _ => {}
                    }
                }
                // VM-level methods that need iterable collection
                if method_name.as_str() == "join" {
                    if let PyObjectPayload::Str(sep) = &receiver.payload {
                        if !args.is_empty() {
                            let items = self.collect_iterable(&args[0])?;
                            let strs: Result<Vec<String>, _> = items.iter()
                                .map(|x| x.as_str().map(String::from).ok_or_else(||
                                    ferrython_core::error::PyException::type_error("sequence item: expected str")))
                                .collect();
                            return Ok(PyObject::str_val(CompactString::from(strs?.join(sep.as_str()))));
                        }
                    }
                }
                // VM-level list.sort with key function
                if method_name.as_str() == "sort" {
                    if let PyObjectPayload::List(items_arc) = &receiver.payload {
                        let items_arc = items_arc.clone();
                        let mut items_vec = items_arc.read().clone();
                        items_vec.sort_by(|a, b| {
                            builtins::partial_cmp_for_sort(a, b).unwrap_or(std::cmp::Ordering::Equal)
                        });
                        *items_arc.write() = items_vec;
                        return Ok(PyObject::none());
                    }
                }
                // Property descriptor methods: setter/getter/deleter
                if let PyObjectPayload::Property { fget, fset, fdel } = &receiver.payload {
                    if args.len() == 1 {
                        let func = args[0].clone();
                        let new_prop = match method_name.as_str() {
                            "setter" => PyObjectPayload::Property { fget: fget.clone(), fset: Some(func), fdel: fdel.clone() },
                            "getter" => PyObjectPayload::Property { fget: Some(func), fset: fset.clone(), fdel: fdel.clone() },
                            "deleter" => PyObjectPayload::Property { fget: fget.clone(), fset: fset.clone(), fdel: Some(func) },
                            _ => return Err(PyException::attribute_error(format!("property has no attribute '{}'", method_name))),
                        };
                        return Ok(Arc::new(PyObject { payload: new_prop }));
                    }
                }
                // namedtuple methods
                if let PyObjectPayload::Instance(inst) = &receiver.payload {
                    if inst.class.get_attr("__namedtuple__").is_some() {
                        match method_name.as_str() {
                            "_asdict" => {
                                if let Some(fields) = inst.class.get_attr("_fields") {
                                    if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                                        let mut map = IndexMap::new();
                                        let attrs = inst.attrs.read();
                                        for field in field_names {
                                            let name = field.py_to_string();
                                            let val = attrs.get(name.as_str()).cloned().unwrap_or_else(PyObject::none);
                                            map.insert(HashableKey::Str(CompactString::from(name.as_str())), val);
                                        }
                                        return Ok(PyObject::dict(map));
                                    }
                                }
                            }
                            "__len__" => {
                                if let Some(tup) = inst.attrs.read().get("_tuple") {
                                    if let PyObjectPayload::Tuple(items) = &tup.payload {
                                        return Ok(PyObject::int(items.len() as i64));
                                    }
                                }
                                return Ok(PyObject::int(0));
                            }
                            "__iter__" => {
                                if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                                    if let PyObjectPayload::Tuple(items) = &tup.payload {
                                        return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                                            Arc::new(std::sync::Mutex::new(
                                                ferrython_core::object::IteratorData::Tuple { items: items.clone(), index: 0 }
                                            ))
                                        )));
                                    }
                                }
                                return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                                    Arc::new(std::sync::Mutex::new(
                                        ferrython_core::object::IteratorData::Tuple { items: vec![], index: 0 }
                                    ))
                                )));
                            }
                            _ => {}
                        }
                    }
                    // Deque methods
                    if inst.attrs.read().contains_key("__deque__") {
                        let get_data = || -> PyObjectRef {
                            inst.attrs.read().get("_data").cloned().unwrap_or_else(|| PyObject::list(vec![]))
                        };
                        match method_name.as_str() {
                            "append" => {
                                let data = get_data();
                                if let PyObjectPayload::List(list) = &data.payload {
                                    list.write().push(args[0].clone());
                                }
                                return Ok(PyObject::none());
                            }
                            "appendleft" => {
                                let data = get_data();
                                if let PyObjectPayload::List(list) = &data.payload {
                                    list.write().insert(0, args[0].clone());
                                }
                                return Ok(PyObject::none());
                            }
                            "pop" => {
                                let data = get_data();
                                if let PyObjectPayload::List(list) = &data.payload {
                                    let mut v = list.write();
                                    if v.is_empty() {
                                        return Err(PyException::new(ExceptionKind::IndexError, "pop from an empty deque"));
                                    }
                                    return Ok(v.pop().unwrap());
                                }
                                return Ok(PyObject::none());
                            }
                            "popleft" => {
                                let data = get_data();
                                if let PyObjectPayload::List(list) = &data.payload {
                                    let mut v = list.write();
                                    if v.is_empty() {
                                        return Err(PyException::new(ExceptionKind::IndexError, "pop from an empty deque"));
                                    }
                                    return Ok(v.remove(0));
                                }
                                return Ok(PyObject::none());
                            }
                            "extend" => {
                                let items = self.collect_iterable(&args[0])?;
                                let data = get_data();
                                if let PyObjectPayload::List(list) = &data.payload {
                                    list.write().extend(items);
                                }
                                return Ok(PyObject::none());
                            }
                            "extendleft" => {
                                let items = self.collect_iterable(&args[0])?;
                                let data = get_data();
                                if let PyObjectPayload::List(list) = &data.payload {
                                    let mut v = list.write();
                                    for item in items.into_iter().rev() {
                                        v.insert(0, item);
                                    }
                                }
                                return Ok(PyObject::none());
                            }
                            "rotate" => {
                                let n = if args.is_empty() { 1i64 } else { args[0].as_int().unwrap_or(1) };
                                let data = get_data();
                                if let PyObjectPayload::List(list) = &data.payload {
                                    let mut v = list.write();
                                    let len = v.len() as i64;
                                    if len > 0 {
                                        let n = ((n % len) + len) % len;
                                        let split = v.len() - n as usize;
                                        let tail: Vec<_> = v.drain(split..).collect();
                                        for (i, item) in tail.into_iter().enumerate() {
                                            v.insert(i, item);
                                        }
                                    }
                                }
                                return Ok(PyObject::none());
                            }
                            "clear" => {
                                let data = get_data();
                                if let PyObjectPayload::List(list) = &data.payload {
                                    list.write().clear();
                                }
                                return Ok(PyObject::none());
                            }
                            "copy" => {
                                let data = get_data();
                                let items = data.to_list()?;
                                return builtins::dispatch("deque", &[PyObject::list(items)]);
                            }
                            "count" => {
                                let data = get_data();
                                if let PyObjectPayload::List(list) = &data.payload {
                                    let v = list.read();
                                    let count = v.iter().filter(|x| x.py_to_string() == args[0].py_to_string()).count();
                                    return Ok(PyObject::int(count as i64));
                                }
                                return Ok(PyObject::int(0));
                            }
                            "reverse" => {
                                let data = get_data();
                                if let PyObjectPayload::List(list) = &data.payload {
                                    list.write().reverse();
                                }
                                return Ok(PyObject::none());
                            }
                            "__len__" => {
                                let data = get_data();
                                if let PyObjectPayload::List(list) = &data.payload {
                                    return Ok(PyObject::int(list.read().len() as i64));
                                }
                                return Ok(PyObject::int(0));
                            }
                            "__iter__" => {
                                let data = get_data();
                                return Ok(data);
                            }
                            _ => {}
                        }
                    }
                    // Hash object methods (hashlib)
                    let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload { cd.name.to_string() } else { String::new() };
                    if matches!(class_name.as_str(), "md5" | "sha1" | "sha256" | "sha224" | "sha384" | "sha512") {
                        match method_name.as_str() {
                            "hexdigest" => {
                                let attrs = inst.attrs.read();
                                if let Some(hd) = attrs.get("_hexdigest") {
                                    return Ok(hd.clone());
                                }
                                return Ok(PyObject::str_val(CompactString::from("")));
                            }
                            "digest" => {
                                let attrs = inst.attrs.read();
                                if let Some(d) = attrs.get("_digest") {
                                    return Ok(d.clone());
                                }
                                return Ok(PyObject::bytes(vec![]));
                            }
                            _ => {}
                        }
                    }
                }
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
            PyObjectPayload::NativeFunction { func, name } => {
                // Intercept functions that need VM access to call Python callables
                if name.as_str() == "functools.reduce" {
                    return self.vm_functools_reduce(&args);
                }
                func(&args)
            }
            PyObjectPayload::Partial { func: partial_func, args: partial_args, kwargs: partial_kwargs } => {
                let partial_func = partial_func.clone();
                let mut combined_args = partial_args.clone();
                combined_args.extend(args);
                if !partial_kwargs.is_empty() {
                    let kw: Vec<(CompactString, PyObjectRef)> = partial_kwargs.iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    self.call_object_kw(partial_func, combined_args, kw)
                } else {
                    self.call_object(partial_func, combined_args)
                }
            }
            PyObjectPayload::Instance(_) => {
                // Callable instances: check for __call__
                if let Some(method) = func.get_attr("__call__") {
                    self.call_object(method, args)
                } else {
                    Err(PyException::type_error(format!(
                        "'{}' object is not callable", func.type_name()
                    )))
                }
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
                // Wire up closure cells from the captured function
                let n_cell = frame.code.cellvars.len();
                for (i, cell) in pyfunc.closure.iter().enumerate() {
                    let free_idx = n_cell + i;
                    if free_idx < frame.cells.len() {
                        frame.cells[free_idx] = cell.clone();
                    }
                }
                self.call_stack.push(frame);
                let _ = self.run_frame();
                let frame = self.call_stack.pop().unwrap();
                frame.local_names
            }
            _ => IndexMap::new(),
        };

        // Build MRO: [self_class, ...linearized_parents, object]
        // Simple C3-like: for single inheritance just chain; for multiple use bases order
        let mro = Self::compute_mro(&bases);
        let cls = PyObject::wrap(PyObjectPayload::Class(ClassData {
            name: class_name, bases, namespace: Arc::new(RwLock::new(namespace)), mro,
        }));
        Ok(cls)
    }

    /// Compute a simple MRO from bases (includes bases and their ancestors, NOT self).
    /// C3 linearization for MRO computation (matches CPython).
    fn compute_mro(bases: &[PyObjectRef]) -> Vec<PyObjectRef> {
        if bases.is_empty() {
            return vec![];
        }
        // Build linearizations: L(base) for each base, plus the bases list itself
        let mut linearizations: Vec<Vec<PyObjectRef>> = Vec::new();
        for base in bases {
            let mut l = vec![base.clone()];
            if let PyObjectPayload::Class(cd) = &base.payload {
                l.extend(cd.mro.iter().cloned());
            }
            linearizations.push(l);
        }
        linearizations.push(bases.to_vec());
        Self::c3_merge(&mut linearizations)
    }

    fn c3_merge(linearizations: &mut Vec<Vec<PyObjectRef>>) -> Vec<PyObjectRef> {
        let mut result = Vec::new();
        loop {
            // Remove empty lists
            linearizations.retain(|l| !l.is_empty());
            if linearizations.is_empty() {
                break;
            }
            // Find a good head: first element of some list that doesn't appear in the tail of any list
            let mut found = None;
            for lin in linearizations.iter() {
                let candidate = &lin[0];
                let candidate_ptr = Arc::as_ptr(candidate);
                let in_tail = linearizations.iter().any(|other| {
                    other.iter().skip(1).any(|x| Arc::as_ptr(x) == candidate_ptr)
                });
                if !in_tail {
                    found = Some(candidate.clone());
                    break;
                }
            }
            if let Some(head) = found {
                let head_ptr = Arc::as_ptr(&head);
                result.push(head);
                for lin in linearizations.iter_mut() {
                    if !lin.is_empty() && Arc::as_ptr(&lin[0]) == head_ptr {
                        lin.remove(0);
                    }
                }
            } else {
                // C3 linearization failure — fall back to DFS
                break;
            }
        }
        result
    }

    fn vm_functools_reduce(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("reduce() requires at least 2 arguments")); }
        let func = args[0].clone();
        let items = self.collect_iterable(&args[1])?;
        let has_initial = args.len() > 2;
        let mut acc = if has_initial {
            args[2].clone()
        } else if !items.is_empty() {
            items[0].clone()
        } else {
            return Err(PyException::type_error("reduce() of empty sequence with no initial value"));
        };
        let start_idx = if has_initial { 0 } else { 1 };
        for item in &items[start_idx..] {
            acc = self.call_object(func.clone(), vec![acc, item.clone()])?;
        }
        Ok(acc)
    }

    /// Collect all items from any iterable (list, tuple, generator, instance with __iter__/__next__).
    fn collect_iterable(&mut self, obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
        match &obj.payload {
            PyObjectPayload::Generator(gen_arc) => {
                let gen_arc = gen_arc.clone();
                let mut items = Vec::new();
                loop {
                    match self.resume_generator(&gen_arc, PyObject::none()) {
                        Ok(value) => items.push(value),
                        Err(e) if e.kind == ExceptionKind::StopIteration => break,
                        Err(e) => return Err(e),
                    }
                }
                Ok(items)
            }
            PyObjectPayload::Instance(inst) => {
                // Deque: directly return internal data as list
                if inst.attrs.read().contains_key("__deque__") {
                    if let Some(data) = inst.attrs.read().get("_data").cloned() {
                        return data.to_list();
                    }
                }
                if let Some(iter_method) = obj.get_attr("__iter__") {
                    let iter_obj = self.call_object(iter_method, vec![])?;
                    // If __iter__ returned a builtin Iterator, use iter_advance
                    if matches!(&iter_obj.payload, PyObjectPayload::Iterator(_)) {
                        let mut items = Vec::new();
                        loop {
                            match builtins::iter_advance(&iter_obj)? {
                                Some((_new_iter, value)) => items.push(value),
                                None => break,
                            }
                        }
                        return Ok(items);
                    }
                    // If it returned a generator, collect from it
                    if let PyObjectPayload::Generator(gen_arc) = &iter_obj.payload {
                        let gen_arc = gen_arc.clone();
                        let mut items = Vec::new();
                        loop {
                            match self.resume_generator(&gen_arc, PyObject::none()) {
                                Ok(value) => items.push(value),
                                Err(e) if e.kind == ExceptionKind::StopIteration => break,
                                Err(e) => return Err(e),
                            }
                        }
                        return Ok(items);
                    }
                    // Otherwise, it's an instance with __next__
                    let mut items = Vec::new();
                    loop {
                        if let Some(next_method) = iter_obj.get_attr("__next__") {
                            match self.call_object(next_method.clone(), vec![]) {
                                Ok(value) => items.push(value),
                                Err(e) if e.kind == ExceptionKind::StopIteration => break,
                                Err(e) => return Err(e),
                            }
                        } else { break; }
                    }
                    Ok(items)
                } else {
                    obj.to_list()
                }
            }
            _ => obj.to_list(),
        }
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
            "copy" => Ok(builtins::create_copy_module()),
            "operator" => Ok(builtins::create_operator_module()),
            "typing" => Ok(builtins::create_typing_module()),
            "abc" => Ok(builtins::create_abc_module()),
            "enum" => Ok(builtins::create_enum_module()),
            "contextlib" => Ok(builtins::create_contextlib_module()),
            "dataclasses" => Ok(builtins::create_dataclasses_module()),
            "struct" => Ok(builtins::create_struct_module()),
            "textwrap" => Ok(builtins::create_textwrap_module()),
            "traceback" => Ok(builtins::create_traceback_module()),
            "warnings" => Ok(builtins::create_warnings_module()),
            "decimal" => Ok(builtins::create_decimal_module()),
            "statistics" => Ok(builtins::create_statistics_module()),
            "numbers" => Ok(builtins::create_numbers_module()),
            "platform" => Ok(builtins::create_platform_module()),
            "locale" => Ok(builtins::create_locale_module()),
            "inspect" => Ok(builtins::create_inspect_module()),
            "dis" => Ok(builtins::create_dis_module()),
            "logging" => Ok(builtins::create_logging_module()),
            "subprocess" => Ok(builtins::create_subprocess_module()),
            "pathlib" => Ok(builtins::create_pathlib_module()),
            "unittest" => Ok(builtins::create_unittest_module()),
            "threading" => Ok(builtins::create_threading_module()),
            "csv" => Ok(builtins::create_csv_module()),
            "shutil" => Ok(builtins::create_shutil_module()),
            "glob" => Ok(builtins::create_glob_module()),
            "tempfile" => Ok(builtins::create_tempfile_module()),
            "fnmatch" => Ok(builtins::create_fnmatch_module()),
            "base64" => Ok(builtins::create_base64_module()),
            "pprint" => Ok(builtins::create_pprint_module()),
            "argparse" => Ok(builtins::create_argparse_module()),
            "datetime" => Ok(builtins::create_datetime_module()),
            "weakref" => Ok(builtins::create_weakref_module()),
            "abc" => Ok(builtins::create_abc_module()),
            "numbers" => Ok(builtins::create_numbers_module()),
            "decimal" => Ok(builtins::create_decimal_module()),
            "textwrap" => Ok(builtins::create_textwrap_module()),
            _ => Err(PyException::import_error(format!("No module named '{}'", name))),
        }
    }

    fn load_file_module(&mut self, name: &str, importer_filename: &str) -> PyResult<PyObjectRef> {
        let module_path = name.replace('.', "/");
        // Search relative to importer's directory, then cwd
        let importer_dir = std::path::Path::new(importer_filename)
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let search_dirs = [importer_dir.to_path_buf(), std::path::PathBuf::from(".")];
        for dir in &search_dirs {
            let candidates = [
                dir.join(format!("{}.py", module_path)),
                dir.join(format!("{}/__init__.py", module_path)),
            ];
            for candidate in &candidates {
                if candidate.exists() {
                    let candidate_str = candidate.to_string_lossy().to_string();
                    let source = std::fs::read_to_string(candidate)
                        .map_err(|e| PyException::import_error(format!("cannot read '{}': {}", candidate_str, e)))?;
                    let ast = ferrython_parser::parse(&source, &candidate_str)
                        .map_err(|e| PyException::import_error(format!("syntax error in '{}': {}", candidate_str, e)))?;
                    let code = ferrython_compiler::compile(&ast, &candidate_str)
                        .map_err(|e| PyException::import_error(format!("compile error in '{}': {}", candidate_str, e)))?;
                    // Execute module code and collect its globals as module attributes
                    let mod_globals = Arc::new(RwLock::new(IndexMap::new()));
                    let frame = Frame::new(code, mod_globals.clone(), self.builtins.clone());
                    self.call_stack.push(frame);
                    let _ = self.run_frame();
                    self.call_stack.pop();
                    let attrs = mod_globals.read().clone();
                    return Ok(PyObject::module_with_attrs(CompactString::from(name), attrs));
                }
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
