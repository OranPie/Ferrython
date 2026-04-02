//! The main virtual machine — executes bytecode instructions.

use crate::builtins;
use crate::frame::{BlockKind, Frame, ScopeKind};
use compact_str::CompactString;
use ferrython_bytecode::code::{CodeFlags, CodeObject, ConstantValue};
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    ClassData, GeneratorState, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, SharedGlobals};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

/// The Ferrython virtual machine.
pub struct VirtualMachine {
    pub(crate) call_stack: Vec<Frame>,
    pub(crate) builtins: IndexMap<CompactString, PyObjectRef>,
    pub(crate) modules: IndexMap<CompactString, PyObjectRef>,
    /// Currently active exception being handled (for bare `raise` re-raise).
    pub(crate) active_exception: Option<PyException>,
}

impl VirtualMachine {
    pub fn new() -> Self {
        Self {
            call_stack: Vec::new(),
            builtins: builtins::init_builtins(),
            modules: IndexMap::new(),
            active_exception: None,
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

    pub(crate) fn call_object_kw(
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
                                let mut indices: Vec<usize> = (0..decorated.len()).collect();
                                for i in 1..indices.len() {
                                    let mut j = i;
                                    while j > 0 {
                                        let is_less = self.vm_lt(&decorated[indices[j]].0, &decorated[indices[j - 1]].0)?;
                                        if is_less {
                                            indices.swap(j, j - 1);
                                            j -= 1;
                                        } else { break; }
                                    }
                                }
                                items_vec = indices.into_iter().map(|i| decorated[i].1.clone()).collect();
                            } else {
                                self.vm_sort(&mut items_vec)?;
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
                    PyObjectPayload::BuiltinFunction(name) | PyObjectPayload::BuiltinType(name) => Some(name.clone()),
                    _ => None,
                };
                if let Some(name) = builtin_name {
                    match name.as_str() {
                        "__build_class__" => {
                            return self.build_class_kw(pos_args, kwargs);
                        }
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
                                    // Sort keys using VM-level comparison
                                    let keys: Vec<PyObjectRef> = decorated.iter().map(|(k, _)| k.clone()).collect();
                                    let mut indices: Vec<usize> = (0..decorated.len()).collect();
                                    // Insertion sort on indices by key
                                    for i in 1..indices.len() {
                                        let mut j = i;
                                        while j > 0 {
                                            let is_less = self.vm_lt(&keys[indices[j]], &keys[indices[j - 1]])?;
                                            if is_less {
                                                indices.swap(j, j - 1);
                                                j -= 1;
                                            } else {
                                                break;
                                            }
                                        }
                                    }
                                    items_vec = indices.into_iter().map(|i| decorated[i].1.clone()).collect();
                                } else {
                                    self.vm_sort(&mut items_vec)?;
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
                                let better = if is_max {
                                    self.vm_lt(&best_key, &item_key)?
                                } else {
                                    self.vm_lt(&item_key, &best_key)?
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
    pub(crate) fn run_frame(&mut self) -> PyResult<PyObjectRef> {
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
                        // Store active exception for bare `raise` re-raise
                        self.active_exception = Some(exc.clone());
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
                        // Attach traceback entry from the current frame
                        let mut exc = exc;
                        if exc.traceback.is_empty() {
                            self.attach_traceback(&mut exc);
                        }
                        return Err(exc);
                    }
                }
            }
        }
    }

    /// Attach traceback entries from the current call stack to an exception.
    fn attach_traceback(&self, exc: &mut PyException) {
        use ferrython_core::error::TracebackEntry;
        for frame in &self.call_stack {
            let lineno = ferrython_debug::resolve_lineno(
                &frame.code,
                frame.ip.saturating_sub(1),
            );
            exc.traceback.push(TracebackEntry {
                filename: frame.code.filename.to_string(),
                function: frame.code.name.to_string(),
                lineno,
            });
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
        use ferrython_bytecode::opcode::Opcode;
        match instr.op {
            Opcode::Nop | Opcode::PopTop | Opcode::RotTwo | Opcode::RotThree
            | Opcode::DupTop | Opcode::DupTopTwo | Opcode::LoadConst
                => self.exec_stack_ops(instr),

            Opcode::LoadName | Opcode::StoreName | Opcode::DeleteName
            | Opcode::LoadFast | Opcode::StoreFast | Opcode::DeleteFast
            | Opcode::LoadDeref | Opcode::StoreDeref | Opcode::LoadClosure
            | Opcode::LoadGlobal | Opcode::StoreGlobal | Opcode::DeleteGlobal
                => self.exec_name_ops(instr),

            Opcode::LoadAttr | Opcode::StoreAttr | Opcode::DeleteAttr
                => self.exec_attr_ops(instr),

            Opcode::UnaryPositive | Opcode::UnaryNegative
            | Opcode::UnaryNot | Opcode::UnaryInvert
                => self.exec_unary_ops(instr),

            Opcode::BinaryAdd | Opcode::InplaceAdd
            | Opcode::BinarySubtract | Opcode::InplaceSubtract
            | Opcode::BinaryMultiply | Opcode::InplaceMultiply
            | Opcode::BinaryTrueDivide | Opcode::InplaceTrueDivide
            | Opcode::BinaryFloorDivide | Opcode::InplaceFloorDivide
            | Opcode::BinaryModulo | Opcode::InplaceModulo
            | Opcode::BinaryPower | Opcode::InplacePower
            | Opcode::BinaryLshift | Opcode::InplaceLshift
            | Opcode::BinaryRshift | Opcode::InplaceRshift
            | Opcode::BinaryAnd | Opcode::InplaceAnd
            | Opcode::BinaryOr | Opcode::InplaceOr
            | Opcode::BinaryXor | Opcode::InplaceXor
            | Opcode::BinaryMatrixMultiply | Opcode::InplaceMatrixMultiply
                => self.exec_binary_ops(instr),

            Opcode::BinarySubscr | Opcode::StoreSubscr | Opcode::DeleteSubscr
                => self.exec_subscript_ops(instr),

            Opcode::CompareOp => self.exec_compare_ops(instr),

            Opcode::JumpForward | Opcode::JumpAbsolute
            | Opcode::PopJumpIfFalse | Opcode::PopJumpIfTrue
            | Opcode::JumpIfTrueOrPop | Opcode::JumpIfFalseOrPop
            | Opcode::GetIter | Opcode::ForIter
                => self.exec_jump_ops(instr),

            Opcode::BuildTuple | Opcode::BuildList | Opcode::BuildSet
            | Opcode::BuildMap | Opcode::BuildConstKeyMap | Opcode::BuildString
            | Opcode::ListAppend | Opcode::SetAdd | Opcode::MapAdd
            | Opcode::DictUpdate | Opcode::DictMerge | Opcode::ListExtend
            | Opcode::SetUpdate | Opcode::ListToTuple | Opcode::BuildSlice
            | Opcode::UnpackSequence | Opcode::UnpackEx
                => self.exec_build_ops(instr),

            Opcode::CallFunction | Opcode::CallFunctionKw | Opcode::CallMethod
            | Opcode::CallFunctionEx | Opcode::LoadMethod | Opcode::MakeFunction
                => self.exec_call_ops(instr),

            Opcode::ReturnValue | Opcode::ImportName | Opcode::ImportFrom
            | Opcode::ImportStar
                => self.exec_return_import(instr),

            Opcode::SetupFinally | Opcode::SetupExcept | Opcode::PopBlock
            | Opcode::PopExcept | Opcode::EndFinally | Opcode::BeginFinally
            | Opcode::RaiseVarargs | Opcode::SetupWith
            | Opcode::WithCleanupStart | Opcode::WithCleanupFinish
                => self.exec_exception_ops(instr),

            Opcode::PrintExpr | Opcode::LoadBuildClass | Opcode::SetupAnnotations
            | Opcode::FormatValue | Opcode::ExtendedArg
            | Opcode::YieldValue | Opcode::YieldFrom
                => self.exec_misc_ops(instr),

            _ => Err(PyException::runtime_error(format!(
                "unimplemented opcode: {:?}", instr.op
            ))),
        }
    }


    /// Truthiness test that dispatches __bool__/__len__ on instances.
    /// Walk a class hierarchy to find if it inherits from an ExceptionType
    pub(crate) fn find_exception_kind(cls: &PyObjectRef) -> ExceptionKind {
        match &cls.payload {
            PyObjectPayload::ExceptionType(kind) => kind.clone(),
            PyObjectPayload::BuiltinType(name) | PyObjectPayload::BuiltinFunction(name) => {
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

    pub(crate) fn vm_is_truthy(&mut self, obj: &PyObjectRef) -> PyResult<bool> {
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

    /// Try to call a dunder method on an instance. Returns None if the object
    /// is not an Instance or doesn't have the named dunder.
    pub(crate) fn try_call_dunder(
        &mut self, obj: &PyObjectRef, dunder: &str, args: Vec<PyObjectRef>,
    ) -> Result<Option<PyObjectRef>, PyException> {
        if let PyObjectPayload::Instance(_) = &obj.payload {
            if let Some(method) = obj.get_attr(dunder) {
                return Ok(Some(self.call_object(method, args)?));
            }
        }
        Ok(None)
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
    pub(crate) fn call_object(
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
            PyObjectPayload::BuiltinFunction(name) | PyObjectPayload::BuiltinType(name) => {
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
                            let mut items = self.collect_iterable(&args[0])?;
                            self.vm_sort(&mut items)?;
                            return Ok(PyObject::list(items));
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
                            if items.is_empty() {
                                return Err(PyException::value_error("min() arg is an empty sequence"));
                            }
                            let mut best = items[0].clone();
                            for item in &items[1..] {
                                if self.vm_lt(item, &best)? {
                                    best = item.clone();
                                }
                            }
                            return Ok(best);
                        }
                    }
                    "max" => {
                        if args.len() == 1 {
                            let items = self.collect_iterable(&args[0])?;
                            if items.is_empty() {
                                return Err(PyException::value_error("max() arg is an empty sequence"));
                            }
                            let mut best = items[0].clone();
                            for item in &items[1..] {
                                if self.vm_lt(&best, item)? {
                                    best = item.clone();
                                }
                            }
                            return Ok(best);
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
                        self.vm_sort(&mut items_vec)?;
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
                // namedtuple methods — delegated to builtins
                if let PyObjectPayload::Instance(inst) = &receiver.payload {
                    if inst.class.get_attr("__namedtuple__").is_some()
                        || inst.attrs.read().contains_key("__deque__")
                    {
                        // deque extend/extendleft need iterable collection via VM
                        if inst.attrs.read().contains_key("__deque__") && matches!(method_name.as_str(), "extend" | "extendleft") {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::call_method(receiver, method_name.as_str(), &[PyObject::list(items)]);
                        }
                        return builtins::call_method(receiver, method_name.as_str(), &args);
                    }
                    // Hashlib methods — delegated to builtins
                    let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload { cd.name.to_string() } else { String::new() };
                    if matches!(class_name.as_str(), "md5" | "sha1" | "sha256" | "sha224" | "sha384" | "sha512") {
                        return builtins::call_method(receiver, method_name.as_str(), &args);
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
            name: class_name, bases: bases.clone(), namespace: Arc::new(RwLock::new(namespace)), mro,
        }));

        // Call __init_subclass__ on each base class (PEP 487)
        for base in &bases {
            if let Some(init_sub) = base.get_attr("__init_subclass__") {
                let bound = if matches!(&init_sub.payload, PyObjectPayload::BoundMethod { .. }) {
                    init_sub
                } else {
                    Arc::new(PyObject {
                        payload: PyObjectPayload::BoundMethod {
                            receiver: base.clone(),
                            method: init_sub,
                        }
                    })
                };
                // __init_subclass__(cls) where cls is the new subclass
                self.call_object(bound, vec![cls.clone()])?;
            }
        }

        Ok(cls)
    }

    /// Handle __build_class__ with keyword args (e.g., metaclass=Meta).
    fn build_class_kw(
        &mut self,
        args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
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

        // Extract metaclass from kwargs
        let metaclass = kwargs.iter()
            .find(|(k, _)| k.as_str() == "metaclass")
            .map(|(_, v)| v.clone());

        // Execute class body to get namespace
        let namespace = match &body_func.payload {
            PyObjectPayload::Function(pyfunc) => {
                let code = pyfunc.code.clone();
                let globals = pyfunc.globals.clone();
                let mut frame = Frame::new(code, globals, self.builtins.clone());
                frame.scope_kind = ScopeKind::Class;
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

        if let Some(meta) = metaclass {
            // Metaclass provided: call meta(name, bases, namespace_dict)
            let ns_dict = {
                let mut map = IndexMap::new();
                for (k, v) in &namespace {
                    if let Ok(hk) = PyObject::str_val(CompactString::from(k.as_str())).to_hashable_key() {
                        map.insert(hk, v.clone());
                    }
                }
                PyObject::dict(map)
            };
            let bases_tuple = PyObject::tuple(bases);
            let name_obj = PyObject::str_val(class_name);
            self.call_object(meta, vec![name_obj, bases_tuple, ns_dict])
        } else {
            // No metaclass: build normally
            let mro = Self::compute_mro(&bases);
            let cls = PyObject::wrap(PyObjectPayload::Class(ClassData {
                name: class_name, bases: bases.clone(),
                namespace: Arc::new(RwLock::new(namespace)), mro,
            }));
            // __init_subclass__
            for base in &bases {
                if let Some(init_sub) = base.get_attr("__init_subclass__") {
                    let bound = if matches!(&init_sub.payload, PyObjectPayload::BoundMethod { .. }) {
                        init_sub
                    } else {
                        Arc::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: base.clone(),
                                method: init_sub,
                            }
                        })
                    };
                    self.call_object(bound, vec![cls.clone()])?;
                }
            }
            Ok(cls)
        }
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
    pub(crate) fn resume_generator(
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

    /// Sort items using VM-level comparison (supports custom __lt__).
    /// Uses insertion sort to allow &mut self access during comparisons.
    pub fn vm_sort(&mut self, items: &mut Vec<PyObjectRef>) -> PyResult<()> {
        let n = items.len();
        if n <= 1 { return Ok(()); }
        let has_instances = items.iter().any(|x| matches!(&x.payload, PyObjectPayload::Instance(_)));
        if !has_instances {
            items.sort_by(|a, b| {
                builtins::partial_cmp_for_sort(a, b).unwrap_or(std::cmp::Ordering::Equal)
            });
            return Ok(());
        }
        // Insertion sort with VM-level __lt__ calls
        for i in 1..n {
            let mut j = i;
            while j > 0 {
                let is_less = self.vm_lt(&items[j], &items[j - 1])?;
                if is_less {
                    items.swap(j, j - 1);
                    j -= 1;
                } else {
                    break;
                }
            }
        }
        Ok(())
    }

    /// Compare two objects using __lt__, falling back to native comparison.
    fn vm_lt(&mut self, a: &PyObjectRef, b: &PyObjectRef) -> PyResult<bool> {
        if let PyObjectPayload::Instance(_) = &a.payload {
            if let Some(method) = a.get_attr("__lt__") {
                let result = self.call_object(method, vec![b.clone()])?;
                return Ok(result.is_truthy());
            }
        }
        Ok(builtins::partial_cmp_for_sort(a, b) == Some(std::cmp::Ordering::Less))
    }
}

/// Convert a bytecode constant to a runtime PyObject.
pub(crate) fn constant_to_object(constant: &ConstantValue) -> PyObjectRef {
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
pub(crate) fn exception_kind_matches(actual: &ExceptionKind, expected: &ExceptionKind) -> bool {
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
