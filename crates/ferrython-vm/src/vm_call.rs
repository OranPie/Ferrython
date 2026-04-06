//! Function/method call dispatch, class instantiation, super().

use crate::builtins;
use crate::frame::{Frame, ScopeKind};
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::code::{CodeFlags, CodeObject};
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{
    AsyncGenAction, CompareOp, IteratorData, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef, is_data_descriptor, has_descriptor_get, lookup_in_class_mro,
    get_builtin_base_type_name,
};
use ferrython_core::types::{HashableKey, SharedConstantCache, SharedGlobals};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

impl VirtualMachine {
    /// Write text to a file-like object, handling both BoundMethod (e.g. StringIO)
    /// and NativeFunction (e.g. default sys.stdout) cases.
    fn write_to_file_object(&mut self, target: &PyObjectRef, text: &str) -> PyResult<()> {
        if let Some(write_fn) = target.get_attr("write") {
            let text_obj = PyObject::str_val(CompactString::from(text));
            match &write_fn.payload {
                // Bound methods already include self in dispatch
                PyObjectPayload::BoundMethod { .. }
                | PyObjectPayload::BuiltinBoundMethod { .. } => {
                    self.call_object(write_fn, vec![text_obj])?;
                }
                // NativeClosure (e.g. StringIO.write): instance method stored on instance dict
                PyObjectPayload::NativeClosure { .. } => {
                    self.call_object(write_fn, vec![text_obj])?;
                }
                // Raw NativeFunction (e.g. default stdio): prepend self
                _ => {
                    self.call_object(write_fn, vec![target.clone(), text_obj])?;
                }
            }
            Ok(())
        } else {
            print!("{}", text);
            Ok(())
        }
    }

    /// Resolve the output target for print(): file= kwarg > sys.stdout > native stdout.
    fn resolve_print_target(&self, explicit_file: Option<PyObjectRef>) -> Option<PyObjectRef> {
        explicit_file
            .or_else(|| ferrython_stdlib::get_stdout_override())
            .or_else(|| self.modules.get("sys").and_then(|s| s.get_attr("stdout")))
    }

    /// str.format_map() with dict subclass mapping, supporting __missing__ via VM call dispatch.
    fn vm_format_map(
        &mut self,
        template: &str,
        mapping: &PyObjectRef,
        dict_storage: &Arc<RwLock<IndexMap<HashableKey, PyObjectRef>>>,
        mapping_class: &PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        let mut result = String::new();
        let mut chars = template.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '{' {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    result.push('{');
                } else {
                    let mut field = String::new();
                    for c in chars.by_ref() {
                        if c == '}' { break; }
                        field.push(c);
                    }
                    let key = HashableKey::Str(CompactString::from(&field));
                    if let Some(val) = dict_storage.read().get(&key) {
                        result.push_str(&val.py_to_string());
                    } else if let Some(missing_fn) = lookup_in_class_mro(mapping_class, "__missing__") {
                        // Call __missing__(self, key) via VM dispatch
                        let key_obj = PyObject::str_val(CompactString::from(&field));
                        let val = self.call_object(missing_fn, vec![mapping.clone(), key_obj])?;
                        result.push_str(&val.py_to_string());
                    } else {
                        return Err(PyException::key_error(field));
                    }
                }
            } else if c == '}' && chars.peek() == Some(&'}') {
                chars.next();
                result.push('}');
            } else {
                result.push(c);
            }
        }
        Ok(PyObject::str_val(CompactString::from(result)))
    }

    /// Collect the current frame's local variables into a dict.
    /// At module scope, locals() == globals().
    fn collect_locals_dict(&self) -> PyResult<PyObjectRef> {
        let frame = self.call_stack.last().unwrap();
        if matches!(frame.scope_kind, ScopeKind::Module) {
            // At module level, locals() == globals()
            let g = frame.globals.read();
            let pairs: Vec<(PyObjectRef, PyObjectRef)> = g.iter()
                .map(|(k, v)| (PyObject::str_val(CompactString::from(k.as_str())), v.clone()))
                .collect();
            drop(g);
            return Ok(PyObject::dict_from_pairs(pairs));
        }
        let mut pairs: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
        // Fast locals (function parameters and local variables)
        for (i, name) in frame.code.varnames.iter().enumerate() {
            if let Some(Some(val)) = frame.locals.get(i) {
                pairs.push((PyObject::str_val(name.clone()), val.clone()));
            }
        }
        // local_names (class scope, etc.)
        for (k, v) in &frame.local_names {
            pairs.push((PyObject::str_val(k.clone()), v.clone()));
        }
        // Cell and free variables
        for (i, name) in frame.code.cellvars.iter().chain(frame.code.freevars.iter()).enumerate() {
            if let Some(cell) = frame.cells.get(i) {
                let cell_val = cell.read();
                if let Some(val) = cell_val.as_ref() {
                    pairs.push((PyObject::str_val(name.clone()), val.clone()));
                }
            }
        }
        Ok(PyObject::dict_from_pairs(pairs))
    }

    pub(crate) fn call_function(
        &mut self,
        code: &Arc<CodeObject>,
        args: Vec<PyObjectRef>,
        defaults: &[PyObjectRef],
        kw_defaults: &IndexMap<CompactString, PyObjectRef>,
        globals: SharedGlobals,
        closure: &[Arc<RwLock<Option<PyObjectRef>>>],
        constant_cache: &SharedConstantCache,
    ) -> PyResult<PyObjectRef> {
        let mut frame = Frame::new_from_pool(Arc::clone(code), globals, Arc::clone(&self.builtins), Arc::clone(constant_cache), &mut self.frame_pool);
        let nparams = code.arg_count as usize;
        let nkwonly = code.kwonlyarg_count as usize;
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

        // Check for missing required positional args
        if args.len() < nparams {
            let ndefaults = defaults.len();
            let required = nparams - ndefaults;
            if args.len() < required {
                let missing = required - args.len();
                let fname = code.name.as_str();
                let missing_names: Vec<&str> = (args.len()..required)
                    .filter_map(|i| code.varnames.get(i).map(|s| s.as_str()))
                    .collect();
                return Err(PyException::type_error(format!(
                    "{}() missing {} required positional argument{}: {}",
                    fname, missing, if missing == 1 { "" } else { "s" },
                    missing_names.iter().map(|n| format!("'{}'", n)).collect::<Vec<_>>().join(", ")
                )));
            }
        }

        // Pack extra positional args into *args tuple, or raise TypeError
        if has_varargs {
            let extra: Vec<PyObjectRef> = if args.len() > nparams {
                args[nparams..].to_vec()
            } else {
                Vec::new()
            };
            frame.set_local(nparams, PyObject::tuple(extra));
        } else if args.len() > nparams {
            let fname = code.name.as_str();
            return Err(PyException::type_error(format!(
                "{}() takes {} positional argument{} but {} {} given",
                fname, nparams, if nparams == 1 { "" } else { "s" },
                args.len(), if args.len() == 1 { "was" } else { "were" }
            )));
        }

        // Fill in kw_defaults for keyword-only args
        let kwonly_start = if has_varargs { nparams + 1 } else { nparams };
        for i in 0..nkwonly {
            let slot = kwonly_start + i;
            if frame.locals.get(slot).map_or(true, |v| v.is_none()) {
                if let Some(varname) = code.varnames.get(slot) {
                    if let Some(default_val) = kw_defaults.get(varname.as_str()) {
                        frame.set_local(slot, default_val.clone());
                    }
                }
            }
        }

        // Pack **kwargs into a dict
        if has_varkw {
            let kwargs_idx = kwonly_start + nkwonly;
            if frame.locals.get(kwargs_idx).map_or(true, |v| v.is_none()) {
                frame.set_local(kwargs_idx, PyObject::dict(IndexMap::new()));
            }
        }

        self.install_closure_and_run(frame, code, closure)
    }

    pub(crate) fn call_function_kw(
        &mut self,
        code: &Arc<CodeObject>,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
        defaults: &[PyObjectRef],
        kw_defaults: &IndexMap<CompactString, PyObjectRef>,
        globals: SharedGlobals,
        closure: &[Arc<RwLock<Option<PyObjectRef>>>],
        constant_cache: &SharedConstantCache,
    ) -> PyResult<PyObjectRef> {
        let mut frame = Frame::new_from_pool(Arc::clone(code), globals, Arc::clone(&self.builtins), Arc::clone(constant_cache), &mut self.frame_pool);
        let nparams = code.arg_count as usize;
        let nkwonly = code.kwonlyarg_count as usize;
        let has_varargs = code.flags.contains(CodeFlags::VARARGS);
        let has_varkw = code.flags.contains(CodeFlags::VARKEYWORDS);

        // Total named parameters (positional + keyword-only)
        let _total_named = nparams + nkwonly;
        // Varargs slot comes after positional params
        let varargs_slot = nparams;
        // Keyword-only params start after *args slot (if present)
        let kwonly_start = if has_varargs { nparams + 1 } else { nparams };

        // Assign positional parameters
        let positional_count = pos_args.len().min(nparams);
        for i in 0..positional_count {
            frame.set_local(i, pos_args[i].clone());
        }

        // Place keyword args at their correct parameter positions
        // Build a name→index lookup for O(1) kwarg matching
        let posonlyarg_count = code.posonlyarg_count as usize;
        let mut extra_kwargs: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
        // Pre-build varname→index map for fast lookup when kwargs > 2
        let varname_map: Option<std::collections::HashMap<&str, usize>> = if kwargs.len() > 2 {
            Some(code.varnames.iter().enumerate().map(|(i, v)| (v.as_str(), i)).collect())
        } else {
            None
        };
        for (name, val) in &kwargs {
            let found_idx = if let Some(ref map) = varname_map {
                map.get(name.as_str()).copied()
            } else {
                code.varnames.iter().position(|v| v.as_str() == name.as_str())
            };
            if let Some(idx) = found_idx {
                // Reject positional-only parameters passed as keyword arguments
                if idx < posonlyarg_count {
                    return Err(PyException::type_error(format!(
                        "{}() got some positional-only arguments passed as keyword arguments: '{}'",
                        code.name, name
                    )));
                }
                // Accept both positional params (< nparams) and kwonly params
                let is_positional = idx < nparams;
                let is_kwonly = idx >= kwonly_start && idx < kwonly_start + nkwonly;
                if is_positional || is_kwonly {
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

        // Fill in kw_defaults for missing keyword-only args
        for i in 0..nkwonly {
            let slot = kwonly_start + i;
            if frame.locals.get(slot).map_or(true, |v| v.is_none()) {
                if let Some(varname) = code.varnames.get(slot) {
                    if let Some(default_val) = kw_defaults.get(varname.as_str()) {
                        frame.set_local(slot, default_val.clone());
                    }
                }
            }
        }

        // Pack extra positional args into *args tuple, or raise TypeError
        if has_varargs {
            let extra: Vec<PyObjectRef> = if pos_args.len() > nparams {
                pos_args[nparams..].to_vec()
            } else {
                Vec::new()
            };
            frame.set_local(varargs_slot, PyObject::tuple(extra));
        } else if pos_args.len() > nparams {
            let fname = code.name.as_str();
            return Err(PyException::type_error(format!(
                "{}() takes {} positional argument{} but {} {} given",
                fname, nparams, if nparams == 1 { "" } else { "s" },
                pos_args.len(), if pos_args.len() == 1 { "was" } else { "were" }
            )));
        }

        // Pack **kwargs into a dict
        if has_varkw {
            let kwargs_idx = kwonly_start + nkwonly;
            frame.set_local(kwargs_idx, PyObject::dict(extra_kwargs));
        }

        self.install_closure_and_run(frame, code, closure)
    }

    /// Unified class instantiation: __new__, dataclass/namedtuple auto-init, __init__, exception attrs.
    pub(crate) fn instantiate_class(
        &mut self,
        cls: &PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        // Enum lookup: Color(2) returns the member with that value
        if let PyObjectPayload::Class(cd) = &cls.payload {
            if cd.namespace.read().contains_key("__enum__") && pos_args.len() == 1 && kwargs.is_empty() {
                let target_val = &pos_args[0];
                let ns = cd.namespace.read();
                for (_, member) in ns.iter() {
                    if let PyObjectPayload::Instance(inst) = &member.payload {
                        if let Some(val) = inst.attrs.read().get("value") {
                            if val.compare(target_val, CompareOp::Eq)
                                .map(|r| r.is_truthy())
                                .unwrap_or(false)
                            {
                                return Ok(member.clone());
                            }
                        }
                    }
                }
                return Err(PyException::value_error(format!(
                    "{} is not a valid {}", target_val.repr(), cd.name
                )));
            }
        }
        // Check for abstract methods (ABC support)
        // Walk the full MRO to collect abstract markers, then check if
        // the concrete class (or any class earlier in MRO) overrides them.
        if let PyObjectPayload::Class(cd) = &cls.payload {
            let is_abstract_marker = |val: &PyObjectRef| -> bool {
                if let PyObjectPayload::Tuple(items) = &val.payload {
                    items.len() == 2 && items[0].as_str() == Some("__abstract__")
                } else {
                    false
                }
            };
            let mut abstract_names: Vec<String> = Vec::new();
            // Check this class's own namespace for abstract markers
            {
                let ns = cd.namespace.read();
                for (name, val) in ns.iter() {
                    if is_abstract_marker(val) {
                        abstract_names.push(name.to_string());
                    }
                }
            }
            // Walk full MRO (bases + their bases) for inherited abstract methods
            for ancestor in &cd.mro {
                if let PyObjectPayload::Class(ancestor_cd) = &ancestor.payload {
                    let ancestor_ns = ancestor_cd.namespace.read();
                    for (name, val) in ancestor_ns.iter() {
                        if !is_abstract_marker(val) {
                            continue;
                        }
                        // Check if any class from the concrete class up through
                        // the MRO (before this ancestor) provides a concrete override
                        let overridden = cd.namespace.read().get(name.as_str())
                            .map(|v| !is_abstract_marker(v))
                            .unwrap_or(false);
                        if !overridden && !abstract_names.contains(&name.to_string()) {
                            abstract_names.push(name.to_string());
                        }
                    }
                }
            }
            if !abstract_names.is_empty() {
                abstract_names.sort();
                return Err(PyException::type_error(format!(
                    "Can't instantiate abstract class {} with abstract method{}{}",
                    cd.name,
                    if abstract_names.len() > 1 { "s " } else { " " },
                    abstract_names.join(", ")
                )));
            }
        }
        // __new__
        let instance = if let Some(new_method) = cls.get_attr("__new__") {
            // If __new__ is from a BuiltinType base (dict, list, etc.), just create instance
            let is_builtin_new = matches!(&new_method.payload,
                PyObjectPayload::BuiltinBoundMethod { receiver, .. }
                    if matches!(&receiver.payload, PyObjectPayload::BuiltinType(_))
            );
            // Also recognize builtin __new__ NativeFunctions (tuple.__new__, list.__new__, etc.)
            let is_native_builtin_new = matches!(&new_method.payload,
                PyObjectPayload::NativeFunction { name, .. }
                    if name.ends_with(".__new__") && matches!(name.as_str(),
                        "tuple.__new__" | "list.__new__" | "str.__new__" | "int.__new__"
                        | "float.__new__" | "object.__new__")
            );
            if is_builtin_new || is_native_builtin_new {
                let inst = PyObject::instance(cls.clone());
                // For builtin type subclasses (int, str, float), store the constructor
                // argument as __builtin_value__ so arithmetic/methods work correctly.
                if !pos_args.is_empty() {
                    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                        if let Some(base_type) = get_builtin_base_type_name(cls) {
                            let value = match base_type.as_str() {
                                "int" => {
                                    let arg = &pos_args[0];
                                    match &arg.payload {
                                        PyObjectPayload::Int(_) | PyObjectPayload::Bool(_) => Some(arg.clone()),
                                        PyObjectPayload::Float(f) => Some(PyObject::int(*f as i64)),
                                        PyObjectPayload::Str(s) => s.trim().parse::<i64>().ok().map(PyObject::int),
                                        _ => None,
                                    }
                                }
                                "float" => {
                                    let arg = &pos_args[0];
                                    match &arg.payload {
                                        PyObjectPayload::Float(_) => Some(arg.clone()),
                                        PyObjectPayload::Int(n) => Some(PyObject::float(n.to_f64())),
                                        PyObjectPayload::Bool(b) => Some(PyObject::float(if *b { 1.0 } else { 0.0 })),
                                        PyObjectPayload::Str(s) => s.trim().parse::<f64>().ok().map(PyObject::float),
                                        _ => None,
                                    }
                                }
                                "str" => {
                                    // Use vm_str for VM-aware conversion (calls __str__/__repr__)
                                    match self.vm_str(&pos_args[0]) {
                                        Ok(s) => Some(PyObject::str_val(CompactString::from(s))),
                                        Err(_) => {
                                            let s = pos_args[0].py_to_string();
                                            Some(PyObject::str_val(CompactString::from(s)))
                                        }
                                    }
                                }
                                "list" => {
                                    Some(PyObject::list(pos_args[0].to_list().unwrap_or_default()))
                                }
                                "tuple" => {
                                    // Namedtuple: multiple positional args → store as tuple
                                    // Regular tuple subclass: single iterable arg → expand to tuple
                                    if pos_args.len() > 1 {
                                        Some(PyObject::tuple(pos_args.clone()))
                                    } else {
                                        let items = pos_args[0].to_list().unwrap_or_default();
                                        Some(PyObject::tuple(items))
                                    }
                                }
                                "set" => {
                                    Some(pos_args[0].clone())
                                }
                                "bytes" => {
                                    Some(pos_args[0].clone())
                                }
                                "bytearray" => {
                                    Some(pos_args[0].clone())
                                }
                                _ => None,
                            };
                            if let Some(val) = value {
                                inst_data.attrs.write().insert(
                                    intern_or_new("__builtin_value__"), val,
                                );
                            }
                        }
                    }
                }
                inst
            } else {
                let new_fn = match &new_method.payload {
                    PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                    _ => new_method.clone(),
                };
                let mut new_args = vec![cls.clone()];
                new_args.extend(pos_args.clone());
                self.call_object(new_fn, new_args)?
            }
        } else {
            PyObject::instance(cls.clone())
        };

        // Check markers in class namespace directly, not via get_attr,
        // because BuiltinType get_attr can return false positives.
        let class_has_key = |obj: &PyObjectRef, key: &str| -> bool {
            // Check the class itself and its MRO (base classes)
            if let PyObjectPayload::Class(cd) = &obj.payload {
                if cd.namespace.read().contains_key(key) {
                    return true;
                }
                for base in &cd.bases {
                    if let PyObjectPayload::Class(bcd) = &base.payload {
                        if bcd.namespace.read().contains_key(key) {
                            return true;
                        }
                    }
                }
            }
            false
        };
        let is_dataclass = class_has_key(cls, "__dataclass__");
        let has_user_init = cls.get_attr("__init__").is_some();

        if is_dataclass && !has_user_init {
            // Dataclass auto-init: populate fields from args/kwargs
            let is_frozen = class_has_key(cls, "__dataclass_frozen__");
            if let Some(fields) = cls.get_attr("__dataclass_fields__") {
                if let PyObjectPayload::Tuple(field_tuples) = &fields.payload {
                    let mut arg_idx = 0;
                    for ft in field_tuples {
                        if let PyObjectPayload::Tuple(info) = &ft.payload {
                            let name = info[0].py_to_string();
                            let has_default = info[1].is_truthy();
                            let default_val = &info[2];
                            // init flag (index 3): whether field participates in __init__
                            let field_init = if info.len() > 3 { info[3].is_truthy() } else { true };

                            let value = if !field_init {
                                // init=False: use default if available, else skip (post_init sets it)
                                if has_default {
                                    if default_val.is_callable() {
                                        self.call_object(default_val.clone(), vec![])?
                                    } else {
                                        default_val.clone()
                                    }
                                } else {
                                    continue; // Will be set by __post_init__
                                }
                            } else if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == name.as_str()) {
                                v.clone()
                            } else if arg_idx < pos_args.len() {
                                let v = pos_args[arg_idx].clone();
                                arg_idx += 1;
                                v
                            } else if has_default {
                                if default_val.is_callable() {
                                    self.call_object(default_val.clone(), vec![])?
                                } else {
                                    default_val.clone()
                                }
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
            // Call __post_init__ if defined
            if let Some(post_init) = cls.get_attr("__post_init__") {
                let pi_fn = match &post_init.payload {
                    PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                    _ => post_init.clone(),
                };
                self.call_object(pi_fn, vec![instance.clone()])?;
            }
            // For frozen dataclasses, install __setattr__/__delattr__ that raise
            if is_frozen {
                if let PyObjectPayload::Class(cd) = &cls.payload {
                    let ns = cd.namespace.read();
                    if !ns.contains_key("__setattr__") {
                        drop(ns);
                        let mut ns = cd.namespace.write();
                        ns.insert(intern_or_new("__setattr__"), PyObject::native_function("__setattr__", |_args| {
                            Err(PyException::attribute_error(String::from("cannot assign to field of frozen dataclass")))
                        }));
                        ns.insert(intern_or_new("__delattr__"), PyObject::native_function("__delattr__", |_args| {
                            Err(PyException::attribute_error(String::from("cannot delete field of frozen dataclass")))
                        }));
                    }
                }
            }
        } else if class_has_key(cls, "__namedtuple__") {
            // Namedtuple: populate named fields
            if let Some(fields) = cls.get_attr("_fields") {
                if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                    if let PyObjectPayload::Instance(inst) = &instance.payload {
                        // Get defaults dict if available
                        let defaults_map = cls.get_attr("_field_defaults").and_then(|d| {
                            if let PyObjectPayload::Dict(map) = &d.payload {
                                Some(map.read().clone())
                            } else { None }
                        });
                        let mut attrs = inst.attrs.write();
                        let mut tuple_values = Vec::with_capacity(field_names.len());
                        for (i, field) in field_names.iter().enumerate() {
                            let name = field.py_to_string();
                            let value = if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == name.as_str()) {
                                v.clone()
                            } else if i < pos_args.len() {
                                pos_args[i].clone()
                            } else if let Some(ref dmap) = defaults_map {
                                let key = HashableKey::Str(CompactString::from(name.as_str()));
                                dmap.get(&key).cloned().unwrap_or_else(PyObject::none)
                            } else {
                                PyObject::none()
                            };
                            attrs.insert(CompactString::from(name.as_str()), value.clone());
                            tuple_values.push(value);
                        }
                        attrs.insert(CompactString::from("_tuple"), PyObject::tuple(tuple_values));
                    }
                }
            }
        } else if let Some(init) = cls.get_attr("__init__") {
            // Skip builtin __init__ — instance already created, no user code to run
            let is_builtin_init = matches!(&init.payload,
                PyObjectPayload::BuiltinBoundMethod { receiver, .. }
                    if matches!(&receiver.payload, PyObjectPayload::BuiltinType(_)));
            if !is_builtin_init {
                let init_fn = match &init.payload {
                    PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                    _ => init.clone(),
                };
                let mut init_args = vec![instance.clone()];
                init_args.extend(pos_args.clone());
                let init_result = if kwargs.is_empty() {
                    self.call_object(init_fn, init_args)?
                } else {
                    self.call_object_kw(init_fn, init_args, kwargs.clone())?
                };
                // CPython: __init__ must return None
                if !matches!(&init_result.payload, PyObjectPayload::None) {
                    return Err(PyException::type_error(
                        "__init__() should return None, not '".to_string()
                            + init_result.type_name() + "'"
                    ));
                }
            }
            // Dict subclass: populate dict_storage from pos_args/kwargs
            if let PyObjectPayload::Instance(inst) = &instance.payload {
                if let Some(ref ds) = inst.dict_storage {
                    let mut storage = ds.write();
                    // If first positional arg is a dict, copy its entries
                    if !pos_args.is_empty() {
                        if let PyObjectPayload::Dict(src) = &pos_args[0].payload {
                            for (k, v) in src.read().iter() {
                                storage.insert(k.clone(), v.clone());
                            }
                        }
                    }
                    // Populate kwargs into dict_storage
                    for (k, v) in &kwargs {
                        storage.insert(HashableKey::Str(k.clone()), v.clone());
                    }
                }
            }
        }

        // Exception subclass attrs
        if Self::is_exception_class(cls) {
            if let PyObjectPayload::Instance(inst) = &instance.payload {
                let mut attrs = inst.attrs.write();
                if !attrs.contains_key("args") {
                    attrs.insert(CompactString::from("args"), PyObject::tuple(pos_args));
                }
            }
        }

        Ok(instance)
    }

    /// Build a super() proxy from current call frame or explicit args.
    pub(crate) fn make_super(&self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
                    PyObjectPayload::Class(cd) => {
                        // For metaclass methods: if defining_class_name matches the metaclass,
                        // use the metaclass as runtime_cls (so super walks metaclass MRO)
                        if let Some(meta) = &cd.metaclass {
                            (meta.clone(), self_obj.clone())
                        } else {
                            (self_obj.clone(), self_obj.clone())
                        }
                    }
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
                let globals = pyfunc.globals.clone();
                self.call_function_kw(&pyfunc.code, pos_args, kwargs, &pyfunc.defaults, &pyfunc.kw_defaults, globals, &pyfunc.closure, &pyfunc.constant_cache)
            }
            PyObjectPayload::BoundMethod { receiver, method } => {
                let mut bound_args = vec![receiver.clone()];
                bound_args.extend(pos_args);
                self.call_object_kw(method.clone(), bound_args, kwargs)
            }
            PyObjectPayload::Class(cd) => {
                // If class has a metaclass with __call__, dispatch through it
                if let Some(meta) = &cd.metaclass {
                    if let Some(call_method) = meta.get_attr("__call__") {
                        let mut call_args = vec![func.clone()];
                        call_args.extend(pos_args);
                        if kwargs.is_empty() {
                            return self.call_object(call_method, call_args);
                        } else {
                            return self.call_object_kw(call_method, call_args, kwargs);
                        }
                    }
                }
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
                            self.sort_with_key(&mut items_vec, key_fn, reverse)?;
                            *items_arc.write() = items_vec;
                            return Ok(PyObject::none());
                        }
                    }
                    // Handle dict.update(key=val, ...)
                    if method_name.as_str() == "update" && !kwargs.is_empty() {
                        if let PyObjectPayload::Dict(map) = &receiver.payload {
                            // First process positional arg (another dict or iterable)
                            if !pos_args.is_empty() {
                                if let PyObjectPayload::Dict(other) = &pos_args[0].payload {
                                    let other_items = other.read().clone();
                                    let mut w = map.write();
                                    for (k, v) in other_items {
                                        w.insert(k, v);
                                    }
                                }
                            }
                            // Then add kwargs
                            let mut w = map.write();
                            for (k, v) in &kwargs {
                                w.insert(HashableKey::Str(k.clone()), v.clone());
                            }
                            return Ok(PyObject::none());
                        }
                    }
                    if method_name.as_str() == "format" && !kwargs.is_empty() {
                        if let PyObjectPayload::Str(s) = &receiver.payload {
                            // Handle str.format() with named args via VM-aware formatter
                            return self.vm_str_format_kw(s, &pos_args, &kwargs);
                        }
                    }
                }
                // BuiltinBoundMethod kwargs: resolve known kwargs to positional args
                if let PyObjectPayload::BuiltinBoundMethod { method_name, .. } = &func.payload {
                    if !kwargs.is_empty() {
                        match method_name.as_str() {
                            // str.encode(encoding=, errors=) / bytes.decode(encoding=, errors=)
                            "encode" | "decode" => {
                                let mut resolved = pos_args;
                                if resolved.is_empty() {
                                    // encoding kwarg or default
                                    let enc = kwargs.iter().find(|(k, _)| k.as_str() == "encoding")
                                        .map(|(_, v)| v.clone())
                                        .unwrap_or_else(|| PyObject::str_val(CompactString::from("utf-8")));
                                    resolved.push(enc);
                                }
                                if resolved.len() < 2 {
                                    if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "errors") {
                                        resolved.push(v.clone());
                                    }
                                }
                                return self.call_object(func, resolved);
                            }
                            _ => {
                                // Generic fallback: pass kwargs as trailing dict
                                let mut all_args = pos_args;
                                let mut kw_map = IndexMap::new();
                                for (k, v) in kwargs {
                                    kw_map.insert(HashableKey::Str(k), v);
                                }
                                all_args.push(PyObject::dict(kw_map));
                                return self.call_object(func, all_args);
                            }
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
                                self.sort_with_key(&mut items_vec, key_fn, reverse)?;
                                return Ok(PyObject::list(items_vec));
                            }
                        }
                        "print" => {
                            let sep = kwargs.iter().find(|(k, _)| k.as_str() == "sep")
                                .map(|(_, v)| v.py_to_string()).unwrap_or_else(|| " ".to_string());
                            let end = kwargs.iter().find(|(k, _)| k.as_str() == "end")
                                .map(|(_, v)| v.py_to_string()).unwrap_or_else(|| "\n".to_string());
                            let file_obj = kwargs.iter().find(|(k, _)| k.as_str() == "file").map(|(_, v)| v.clone());
                            let flush = kwargs.iter().find(|(k, _)| k.as_str() == "flush")
                                .map(|(_,v)| v.is_truthy()).unwrap_or(false);
                            let mut parts = Vec::new();
                            for a in &pos_args {
                                parts.push(self.vm_str(a)?);
                            }
                            let output = format!("{}{}", parts.join(&sep), end);
                            if let Some(f) = self.resolve_print_target(file_obj) {
                                self.write_to_file_object(&f, &output)?;
                                if flush {
                                    if let Some(flush_fn) = f.get_attr("flush") {
                                        let _ = self.call_object(flush_fn, vec![]);
                                    }
                                }
                            } else {
                                print!("{}", output);
                                if flush {
                                    use std::io::Write;
                                    let _ = std::io::stdout().flush();
                                }
                            }
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
                            return self.compute_min_max(items, is_max, key_fn, default, name.as_str());
                        }
                        "super" => {
                            return self.make_super(&pos_args);
                        }
                        "dict" => {
                            let mut map = IndexMap::new();
                            // dict(mapping_or_iterable, **kwargs) or dict(**kwargs)
                            if !pos_args.is_empty() {
                                let mut handled = false;
                                // Check for Dict payload
                                if let PyObjectPayload::Dict(src) = &pos_args[0].payload {
                                    for (k, v) in src.read().iter() {
                                        map.insert(k.clone(), v.clone());
                                    }
                                    handled = true;
                                }
                                // Check for Instance with dict_storage (e.g., defaultdict, OrderedDict)
                                if !handled {
                                    if let PyObjectPayload::Instance(inst) = &pos_args[0].payload {
                                        if let Some(ref ds) = inst.dict_storage {
                                            for (k, v) in ds.read().iter() {
                                                map.insert(k.clone(), v.clone());
                                            }
                                            handled = true;
                                        }
                                    }
                                }
                                if !handled {
                                    // dict(iterable_of_pairs, **kwargs)
                                    let items = self.collect_iterable(&pos_args[0])?;
                                    for item in &items {
                                        let pair = item.to_list()?;
                                        if pair.len() == 2 {
                                            let hk = pair[0].to_hashable_key()?;
                                            map.insert(hk, pair[1].clone());
                                        }
                                    }
                                }
                            }
                            for (k, v) in &kwargs {
                                map.insert(HashableKey::Str(k.clone()), v.clone());
                            }
                            return Ok(PyObject::dict(map));
                        }
                        "enumerate" => {
                            let start = kwargs.iter().find(|(k, _)| k.as_str() == "start")
                                .map(|(_, v)| v.clone())
                                .unwrap_or_else(|| PyObject::int(0));
                            let mut all_args = pos_args;
                            all_args.push(start);
                            return self.call_object(func, all_args);
                        }
                        "open" => {
                            // open(file, mode='r', buffering=-1, encoding=None, ...)
                            let mut all_args = pos_args;
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "mode") {
                                while all_args.len() < 2 { all_args.push(PyObject::str_val(CompactString::from("r"))); }
                                all_args[1] = v.clone();
                            }
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "encoding") {
                                while all_args.len() < 4 { all_args.push(PyObject::none()); }
                                all_args[3] = v.clone();
                            }
                            return self.call_object(func, all_args);
                        }
                        "type" => {
                            // type(name, bases, dict) — 3-arg form with kwargs
                            if !kwargs.is_empty() && pos_args.len() >= 3 {
                                return self.call_object(func, pos_args);
                            }
                            let mut all_args = pos_args;
                            let mut kw_map = IndexMap::new();
                            for (k, v) in kwargs {
                                kw_map.insert(HashableKey::Str(k), v);
                            }
                            if !kw_map.is_empty() {
                                all_args.push(PyObject::dict(kw_map));
                            }
                            return self.call_object(func, all_args);
                        }
                        _ => {
                            // Generic BuiltinFunction kwargs: pass as trailing dict
                            if !kwargs.is_empty() {
                                let mut all_args = pos_args;
                                let mut kw_map = IndexMap::new();
                                for (k, v) in kwargs {
                                    kw_map.insert(HashableKey::Str(k), v);
                                }
                                all_args.push(PyObject::dict(kw_map));
                                return self.call_object(func, all_args);
                            }
                            return self.call_object(func, pos_args);
                        }
                    }
                }
                // Handle other payload types that support kwargs
                match &func.payload {
                    PyObjectPayload::NativeFunction { func: nf, name } => {
                        // OrderedDict(**kwargs) / Counter(**kwargs) / defaultdict(factory, **kwargs) — dict-like init
                        if name.as_str() == "collections.OrderedDict" || name.as_str() == "collections.Counter" {
                            let mut map = IndexMap::new();
                            if !pos_args.is_empty() {
                                if let PyObjectPayload::Dict(src) = &pos_args[0].payload {
                                    for (k, v) in src.read().iter() { map.insert(k.clone(), v.clone()); }
                                } else {
                                    let items = self.collect_iterable(&pos_args[0])?;
                                    for item in &items {
                                        let pair = item.to_list()?;
                                        if pair.len() == 2 {
                                            let hk = pair[0].to_hashable_key()?;
                                            map.insert(hk, pair[1].clone());
                                        }
                                    }
                                }
                            }
                            for (k, v) in &kwargs {
                                map.insert(HashableKey::Str(k.clone()), v.clone());
                            }
                            if name.as_str() == "collections.Counter" {
                                return nf(&[PyObject::dict(map)]);
                            }
                            return Ok(PyObject::dict(map));
                        }
                        if name.as_str() == "collections.defaultdict" {
                            // defaultdict(factory, mapping_or_iterable, **kwargs) or defaultdict(factory, **kwargs)
                            let mut all = pos_args.clone();
                            if !kwargs.is_empty() {
                                let mut map = IndexMap::new();
                                // If there's a second positional arg (mapping), merge it first
                                if all.len() >= 2 {
                                    if let PyObjectPayload::Dict(src) = &all[1].payload {
                                        for (k, v) in src.read().iter() { map.insert(k.clone(), v.clone()); }
                                    }
                                }
                                for (k, v) in &kwargs {
                                    map.insert(HashableKey::Str(k.clone()), v.clone());
                                }
                                if all.len() >= 2 { all[1] = PyObject::dict(map); } else {
                                    while all.len() < 1 { all.push(PyObject::none()); }
                                    all.push(PyObject::dict(map));
                                }
                            }
                            return nf(&all);
                        }
                        if name.as_str() == "collections.deque" {
                            // deque(iterable, maxlen=N)
                            let mut all = pos_args.clone();
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "maxlen") {
                                while all.len() < 1 { all.push(PyObject::list(vec![])); }
                                if all.len() < 2 { all.push(v.clone()); } else { all[1] = v.clone(); }
                            }
                            return nf(&all);
                        }
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
                        // re.sub / re.subn with callable replacement
                        if (name.as_str() == "re.sub" || name.as_str() == "re.subn") && pos_args.len() >= 3 {
                            let repl = &pos_args[1];
                            let is_callable = matches!(&repl.payload,
                                PyObjectPayload::Function(_) | PyObjectPayload::BuiltinFunction(_)
                                | PyObjectPayload::NativeFunction { .. } | PyObjectPayload::NativeClosure { .. }
                                | PyObjectPayload::Partial { .. });
                            if is_callable {
                                return self.re_sub_with_callable(&pos_args, name.as_str() == "re.subn");
                            }
                        }
                        // itertools.groupby with key function
                        if name.as_str() == "itertools.groupby" {
                            let key_fn = kwargs.iter().find(|(k, _)| k.as_str() == "key").map(|(_, v)| v.clone())
                                .or_else(|| if pos_args.len() >= 2 { Some(pos_args[1].clone()) } else { None });
                            let iterable = vec![pos_args[0].clone()];
                            return self.vm_itertools_groupby(&iterable, key_fn);
                        }
                        // itertools.accumulate with initial kwarg
                        if name.as_str() == "itertools.accumulate" && !kwargs.is_empty() {
                            let initial = kwargs.iter().find(|(k, _)| k.as_str() == "initial").map(|(_, v)| v.clone());
                            let func_arg = if pos_args.len() >= 2 && !matches!(&pos_args[1].payload, PyObjectPayload::None) {
                                Some(pos_args[1].clone())
                            } else {
                                None
                            };
                            let mut all = vec![pos_args[0].clone()];
                            all.push(func_arg.unwrap_or_else(PyObject::none));
                            all.push(initial.unwrap_or_else(PyObject::none));
                            return nf(&all);
                        }
                        // re.split with maxsplit kwarg
                        if name.as_str() == "re.split" && !kwargs.is_empty() {
                            let mut all = pos_args.clone();
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "maxsplit") {
                                while all.len() < 3 { all.push(PyObject::int(0)); }
                                all[2] = v.clone();
                            }
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "flags") {
                                while all.len() < 4 { all.push(PyObject::int(0)); }
                                all[3] = v.clone();
                            }
                            return nf(&all);
                        }
                        // re.sub with count kwarg
                        if name.as_str() == "re.sub" && !kwargs.is_empty() {
                            let mut all = pos_args.clone();
                            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "count") {
                                while all.len() < 4 { all.push(PyObject::int(0)); }
                                all[3] = v.clone();
                            }
                            return nf(&all);
                        }
                        // type.__call__(cls, *args, **kwargs) — standard class instantiation
                        if name.as_str() == "__type_call__" {
                            if pos_args.is_empty() {
                                return Err(PyException::type_error("type.__call__ requires cls"));
                            }
                            let cls = pos_args[0].clone();
                            let rest = pos_args[1..].to_vec();
                            return self.instantiate_class(&cls, rest, kwargs);
                        }
                        // json.dumps / json.dump with `default` kwarg that may be a Python function
                        if (name.as_str() == "json.dumps" || name.as_str() == "json.dump") && !kwargs.is_empty() {
                            let default_fn = kwargs.iter()
                                .find(|(k, _)| k.as_str() == "default")
                                .map(|(_, v)| v.clone());
                            let cls_default = if default_fn.is_none() {
                                kwargs.iter()
                                    .find(|(k, _)| k.as_str() == "cls")
                                    .and_then(|(_, v)| v.get_attr("default"))
                            } else { None };
                            let effective_default = default_fn.or(cls_default);
                            if let Some(ref def) = effective_default {
                                if matches!(&def.payload, PyObjectPayload::Function(_)) {
                                    // Pre-process object tree: call `default` on non-serializable values
                                    let prepared = self.json_prepare_with_default(&pos_args[0], def)?;
                                    // Rebuild kwargs without `default` and `cls`
                                    let filtered_kwargs: Vec<(CompactString, PyObjectRef)> = kwargs.into_iter()
                                        .filter(|(k, _)| k.as_str() != "default" && k.as_str() != "cls")
                                        .collect();
                                    if name.as_str() == "json.dump" {
                                        // json.dump(obj, fp, **kwargs) → dump prepared obj to fp
                                        let mut dump_args = vec![prepared];
                                        if pos_args.len() > 1 { dump_args.push(pos_args[1].clone()); }
                                        if !filtered_kwargs.is_empty() {
                                            let mut kw_map = IndexMap::new();
                                            for (k, v) in filtered_kwargs {
                                                kw_map.insert(HashableKey::Str(k), v);
                                            }
                                            dump_args.push(PyObject::dict(kw_map));
                                        }
                                        return nf(&dump_args);
                                    }
                                    // json.dumps(prepared, **remaining_kwargs)
                                    let mut dump_args = vec![prepared];
                                    if !filtered_kwargs.is_empty() {
                                        let mut kw_map = IndexMap::new();
                                        for (k, v) in filtered_kwargs {
                                            kw_map.insert(HashableKey::Str(k), v);
                                        }
                                        dump_args.push(PyObject::dict(kw_map));
                                    }
                                    return nf(&dump_args);
                                }
                            }
                        }
                        // Pass kwargs as trailing dict if present
                        if !kwargs.is_empty() {
                            let mut all_args = pos_args;
                            let mut kw_map = IndexMap::new();
                            for (k, v) in kwargs {
                                kw_map.insert(HashableKey::Str(k), v);
                            }
                            all_args.push(PyObject::dict(kw_map));
                            return nf(&all_args);
                        }
                        return nf(&pos_args);
                    }
                    PyObjectPayload::NativeClosure { func, .. } => {
                        let result = if !kwargs.is_empty() {
                            let mut all_args = pos_args;
                            let mut kw_map = IndexMap::new();
                            for (k, v) in kwargs {
                                kw_map.insert(HashableKey::Str(k), v);
                            }
                            all_args.push(PyObject::dict(kw_map));
                            func(&all_args)?
                        } else {
                            func(&pos_args)?
                        };
                        // Check if asyncio.run() was invoked
                        if let Some(coro) = ferrython_stdlib::take_asyncio_run_coro() {
                            return self.maybe_await_result(coro);
                        }
                        return Ok(result);
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
                        return Ok(PyObject::exception_instance_with_args(kind.clone(), msg, pos_args));
                    }
                    PyObjectPayload::Instance(_) => {
                        if func.get_attr("__singledispatch__").is_some() {
                            return self.vm_singledispatch_call_instance(&func, &pos_args);
                        }
                        if let Some(method) = func.get_attr("__call__") {
                            return self.call_object_kw(method, pos_args, kwargs);
                        }
                        return Err(PyException::type_error(format!(
                            "'{}' object is not callable", func.type_name()
                        )));
                    }
                    _ => {}
                }
                // Final fallback: pass kwargs as trailing dict to preserve key names
                if !kwargs.is_empty() {
                    let mut all_args = pos_args;
                    let mut kw_map = IndexMap::new();
                    for (k, v) in kwargs {
                        kw_map.insert(HashableKey::Str(k), v);
                    }
                    all_args.push(PyObject::dict(kw_map));
                    self.call_object(func, all_args)
                } else {
                    self.call_object(func, pos_args)
                }
            }
        }
    }

    pub(crate) fn call_object(
        &mut self,
        func: PyObjectRef,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        match &func.payload {
            PyObjectPayload::Function(pyfunc) => {
                // Borrow fields directly from the Arc-backed func instead of cloning
                // expensive Vec/IndexMap payloads. Only globals needs cloning (moved into frame).
                let globals = pyfunc.globals.clone();
                self.call_function(&pyfunc.code, args, &pyfunc.defaults, &pyfunc.kw_defaults, globals, &pyfunc.closure, &pyfunc.constant_cache)
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
                        let output = format!("{}\n", parts.join(" "));
                        if let Some(f) = self.resolve_print_target(None) {
                            self.write_to_file_object(&f, &output)?;
                        } else {
                            print!("{}", output);
                        }
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
                            let source = self.resolve_iterable(&args[1])?;
                            return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                                Arc::new(std::sync::Mutex::new(IteratorData::Map { func: func_obj, source }))
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
                                Arc::new(std::sync::Mutex::new(IteratorData::List { items: result, index: 0 }))
                            )));
                        }
                    }
                    "filter" => {
                        if args.len() < 2 {
                            return Err(PyException::type_error("filter() requires at least 2 arguments"));
                        }
                        let func_obj = args[0].clone();
                        let source = self.resolve_iterable(&args[1])?;
                        return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                            Arc::new(std::sync::Mutex::new(IteratorData::Filter { func: func_obj, source }))
                        )));
                    }
                    "iter" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(iter_method) = Self::resolve_instance_dunder(&args[0], "__iter__") {
                                    return self.call_object(iter_method, vec![]);
                                }
                                // Builtin base type subclass: delegate to __builtin_value__
                                if let Some(bv) = Self::get_builtin_value(&args[0]) {
                                    let resolved = self.resolve_iterable(&bv)?;
                                    return Ok(resolved);
                                }
                            }
                            // Fall through to builtin dispatch for non-instances
                        }
                    }
                    "next" => {
                        if args.is_empty() {
                            return Err(PyException::type_error("next() requires at least 1 argument"));
                        }
                        // For generators, resume directly so StopIteration return value propagates
                        if let PyObjectPayload::Generator(gen_arc) = &args[0].payload {
                            match self.resume_generator(gen_arc, PyObject::none()) {
                                Ok(value) => return Ok(value),
                                Err(e) if e.kind == ExceptionKind::StopIteration && args.len() > 1 => {
                                    return Ok(args[1].clone());
                                }
                                Err(e) => return Err(e),
                            }
                        }
                        // Use vm_iter_next which handles instances and lazy iterators
                        match self.vm_iter_next(&args[0]) {
                            Ok(Some(value)) => return Ok(value),
                            Ok(None) => {
                                if args.len() > 1 {
                                    return Ok(args[1].clone()); // default value
                                }
                                return Err(PyException::new(ExceptionKind::StopIteration, ""));
                            }
                            Err(e) if e.kind == ExceptionKind::StopIteration && args.len() > 1 => {
                                return Ok(args[1].clone());
                            }
                            Err(e) => return Err(e),
                        }
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
                            // Use VM-level add to support __add__/__radd__
                            if let Some(r) = self.try_binary_dunder(&total, &item, "__add__", Some("__radd__"))? {
                                total = r;
                            } else {
                                total = total.add(&item)?;
                            }
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
                    "dict" => {
                        if args.is_empty() {
                            return Ok(PyObject::dict(IndexMap::new()));
                        }
                        // dict(mapping) — handle Dict payload
                        if let PyObjectPayload::Dict(_) = &args[0].payload {
                            return builtins::dispatch("dict", &args);
                        }
                        // dict(instance_with_dict_storage) — e.g., defaultdict, OrderedDict
                        if let PyObjectPayload::Instance(inst) = &args[0].payload {
                            if let Some(ref ds) = inst.dict_storage {
                                let mut map = IndexMap::new();
                                for (k, v) in ds.read().iter() {
                                    map.insert(k.clone(), v.clone());
                                }
                                return Ok(PyObject::dict(map));
                            }
                        }
                        // dict(iterable_of_pairs)
                        let items = self.collect_iterable(&args[0])?;
                        return builtins::dispatch("dict", &[PyObject::list(items)]);
                    }
                    "any" => {
                        if !args.is_empty() {
                            let iter_obj = builtins::get_iter_from_obj_pub(&args[0])?;
                            loop {
                                match self.vm_iter_next(&iter_obj)? {
                                    Some(item) => if item.is_truthy() { return Ok(PyObject::bool_val(true)); },
                                    None => return Ok(PyObject::bool_val(false)),
                                }
                            }
                        }
                    }
                    "all" => {
                        if !args.is_empty() {
                            let iter_obj = builtins::get_iter_from_obj_pub(&args[0])?;
                            loop {
                                match self.vm_iter_next(&iter_obj)? {
                                    Some(item) => if !item.is_truthy() { return Ok(PyObject::bool_val(false)); },
                                    None => return Ok(PyObject::bool_val(true)),
                                }
                            }
                        }
                    }
                    "isinstance" => {
                        if args.len() == 2 {
                            let cls = &args[1];
                            // Check for metaclass __instancecheck__ on user-defined classes
                            if let PyObjectPayload::Class(cd) = &cls.payload {
                                if let Some(ref metaclass) = cd.metaclass {
                                    if let Some(ic) = metaclass.get_attr("__instancecheck__") {
                                        let result = self.call_object(ic, vec![cls.clone(), args[0].clone()])?;
                                        return Ok(PyObject::bool_val(result.is_truthy()));
                                    }
                                }
                                // Check __subclasshook__ on the class (ABC protocol)
                                if let Some(hook) = cls.get_attr("__subclasshook__") {
                                    // Pass the type of the object being checked
                                    let obj = &args[0];
                                    let obj_type = match &obj.payload {
                                        PyObjectPayload::Instance(inst) => inst.class.clone(),
                                        _ => PyObject::builtin_type(CompactString::from(obj.type_name())),
                                    };
                                    let result = self.call_object(hook, vec![obj_type])?;
                                    if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                                        return Ok(PyObject::bool_val(result.is_truthy()));
                                    }
                                }
                                // Check for runtime_checkable Protocol — structural subtyping
                                let ns = cd.namespace.read();
                                if ns.get("_is_runtime_checkable").map_or(false, |v| v.is_truthy()) {
                                    if let Some(protocol_attrs) = ns.get("__protocol_attrs__") {
                                        if let PyObjectPayload::Tuple(required) = &protocol_attrs.payload {
                                            let obj = &args[0];
                                            let has_all = required.iter().all(|attr_name| {
                                                let name = attr_name.py_to_string();
                                                obj.get_attr(&name).is_some()
                                            });
                                            return Ok(PyObject::bool_val(has_all));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "issubclass" => {
                        if args.len() == 2 {
                            let sup = &args[1];
                            if let PyObjectPayload::Class(cd) = &sup.payload {
                                // Check metaclass __subclasscheck__ first
                                if let Some(ref metaclass) = cd.metaclass {
                                    if let Some(sc) = metaclass.get_attr("__subclasscheck__") {
                                        let result = self.call_object(sc, vec![sup.clone(), args[0].clone()])?;
                                        return Ok(PyObject::bool_val(result.is_truthy()));
                                    }
                                }
                                // Check __subclasshook__ on the superclass (ABC protocol)
                                if let Some(hook) = sup.get_attr("__subclasshook__") {
                                    let result = self.call_object(hook, vec![args[0].clone()])?;
                                    // If NotImplemented, fall through to normal check
                                    if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                                        return Ok(PyObject::bool_val(result.is_truthy()));
                                    }
                                }
                            }
                        }
                    }
                    "min" => {
                        if args.len() == 1 {
                            let items = self.collect_iterable(&args[0])?;
                            return self.compute_min_max(items, false, None, None, "min");
                        }
                    }
                    "max" => {
                        if args.len() == 1 {
                            let items = self.collect_iterable(&args[0])?;
                            return self.compute_min_max(items, true, None, None, "max");
                        }
                    }
                    "reversed" => {
                        if !args.is_empty() {
                            // Check for __reversed__ dunder on instances
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(rev_method) = Self::resolve_instance_dunder(&args[0], "__reversed__") {
                                    return self.call_object(rev_method, vec![]);
                                }
                            }
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::dispatch("reversed", &[PyObject::list(items)]);
                        }
                    }
                    "enumerate" => {
                        if !args.is_empty() {
                            let mut resolved = Vec::with_capacity(args.len());
                            resolved.push(self.resolve_iterable(&args[0])?);
                            resolved.extend_from_slice(&args[1..]);
                            return builtins::dispatch("enumerate", &resolved);
                        }
                        return builtins::dispatch("enumerate", &args);
                    }
                    "zip" => {
                        // Pre-resolve custom __iter__ before dispatching to zip
                        let resolved = self.resolve_iterables(&args)?;
                        return builtins::dispatch("zip", &resolved);
                    }
                    "len" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                                // Dict subclass: use dict_storage length
                                if let Some(ref ds) = inst.dict_storage {
                                    return Ok(PyObject::int(ds.read().len() as i64));
                                }
                                // Check for custom __len__ (skip BuiltinBoundMethod from BuiltinType base)
                                if let Some(method) = args[0].get_attr("__len__") {
                                    if !matches!(&method.payload, PyObjectPayload::BuiltinBoundMethod { .. }) {
                                        let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] };
                                        return self.call_object(method, ca);
                                    }
                                }
                                // Builtin base type subclass (list, tuple, etc.)
                                if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                                    if let Ok(n) = bv.py_len() {
                                        return Ok(PyObject::int(n as i64));
                                    }
                                }
                            }
                        }
                    }
                    "abs" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__abs__") {
                                    let call_args = if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) { vec![] } else { vec![args[0].clone()] };
                                    return self.call_object(method, call_args);
                                }
                            }
                        }
                    }
                    "hash" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__hash__") {
                                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] }; return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "format" => {
                        if !args.is_empty() {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__format__") {
                                    let spec = if args.len() > 1 {
                                        args[1].clone()
                                    } else {
                                        PyObject::str_val(CompactString::from(""))
                                    };
                                    let mut ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] }; ca.push(spec); return self.call_object(method, ca);
                                }
                                // No __format__: use __str__ for empty/no spec (CPython default __format__)
                                let has_spec = args.len() > 1 && !args[1].py_to_string().is_empty();
                                if !has_spec {
                                    let s = self.vm_str(&args[0])?;
                                    return Ok(PyObject::str_val(CompactString::from(s)));
                                }
                            }
                            // Fall through to native format
                        }
                    }
                    "int" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                                // Check for __builtin_value__ first (int subclass)
                                if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                                    if matches!(&val.payload, PyObjectPayload::Int(_)) {
                                        return Ok(val);
                                    }
                                }
                                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__int__") {
                                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] }; return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "float" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                                // Check for __builtin_value__ first (float subclass)
                                if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                                    if matches!(&val.payload, PyObjectPayload::Float(_)) {
                                        return Ok(val);
                                    }
                                    // int subclass → convert to float
                                    if let PyObjectPayload::Int(n) = &val.payload {
                                        return Ok(PyObject::float(n.to_f64()));
                                    }
                                }
                                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__float__") {
                                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] }; return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "round" => {
                        if !args.is_empty() {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__round__") {
                                    let mut ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] };
                                    if args.len() >= 2 { ca.push(args[1].clone()); }
                                    return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "bytes" => {
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__bytes__") {
                                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] }; return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "bool" => {
                        if args.len() == 1 {
                            return Ok(PyObject::bool_val(self.vm_is_truthy(&args[0])?));
                        }
                    }
                    "dir" => {
                        if args.is_empty() {
                            // dir() with no args: return sorted local variable names
                            let locals = self.collect_locals_dict()?;
                            if let PyObjectPayload::Dict(map) = &locals.payload {
                                let mut names: Vec<String> = map.read().keys()
                                    .map(|k| k.to_object().py_to_string())
                                    .collect();
                                names.sort();
                                let items = names.into_iter()
                                    .map(|n| PyObject::str_val(CompactString::from(n)))
                                    .collect();
                                return Ok(PyObject::list(items));
                            }
                        }
                        if args.len() == 1 {
                            if let PyObjectPayload::Instance(_) = &args[0].payload {
                                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__dir__") {
                                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] };
                                    return self.call_object(method, ca);
                                }
                            }
                        }
                    }
                    "super" => {
                        return self.make_super(&args);
                    }
                    "exec" => {
                        return self.builtin_exec(&args);
                    }
                    "eval" => {
                        return self.builtin_eval(&args);
                    }
                    "compile" => {
                        return self.builtin_compile(&args);
                    }
                    "__import__" => {
                        if args.is_empty() {
                            return Err(PyException::type_error("__import__() requires at least 1 argument"));
                        }
                        let name = args[0].py_to_string();
                        let level = if args.len() >= 5 {
                            args[4].as_int().unwrap_or(0) as usize
                        } else {
                            0
                        };
                        return self.import_module_simple(&name, level);
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
                        return self.collect_locals_dict();
                    }
                    "vars" => {
                        if args.is_empty() {
                            return self.collect_locals_dict();
                        }
                        // vars(obj) — fall through to static builtin_vars
                    }
                    "getattr" => {
                        if args.len() < 2 || args.len() > 3 {
                            return Err(PyException::type_error("getattr expected 2 or 3 arguments"));
                        }
                        let attr_name = args[1].as_str().ok_or_else(||
                            PyException::type_error("getattr(): attribute name must be string"))?;
                        // Use get_attr which handles MRO + data descriptors
                        match args[0].get_attr(attr_name) {
                            Some(v) => {
                                // Invoke descriptor protocol (Property, custom __get__)
                                if let PyObjectPayload::Property { fget, .. } = &v.payload {
                                    if let Some(getter) = fget {
                                        return self.call_object(getter.clone(), vec![args[0].clone()]);
                                    }
                                    return Err(PyException::attribute_error(
                                        format!("unreadable attribute '{}'", attr_name)));
                                }
                                if has_descriptor_get(&v) {
                                    if let Some(get_method) = v.get_attr("__get__") {
                                        let (inst_arg, owner_arg) = match &args[0].payload {
                                            PyObjectPayload::Instance(inst) =>
                                                (args[0].clone(), inst.class.clone()),
                                            PyObjectPayload::Class(_) =>
                                                (PyObject::none(), args[0].clone()),
                                            _ => (args[0].clone(), PyObject::none()),
                                        };
                                        // get_method is already a BoundMethod if from class MRO
                                        return self.call_object(get_method, vec![inst_arg, owner_arg]);
                                    }
                                }
                                return Ok(v);
                            }
                            None => {
                                // Try __getattr__ fallback
                                if let PyObjectPayload::Instance(_) = &args[0].payload {
                                    if let Some(ga) = args[0].get_attr("__getattr__") {
                                        let name_arg = PyObject::str_val(CompactString::from(attr_name));
                                        return self.call_object(ga, vec![name_arg]);
                                    }
                                }
                                if args.len() > 2 {
                                    return Ok(args[2].clone());
                                }
                                return Err(PyException::attribute_error(format!(
                                    "'{}' object has no attribute '{}'",
                                    args[0].type_name(), attr_name)));
                            }
                        }
                    }
                    "setattr" => {
                        if args.len() != 3 {
                            return Err(PyException::type_error("setattr() takes exactly 3 arguments"));
                        }
                        let attr_name = args[1].py_to_string();
                        let value = args[2].clone();
                        if let PyObjectPayload::Instance(inst) = &args[0].payload {
                            if let Some(desc) = lookup_in_class_mro(&inst.class, &attr_name) {
                                if let PyObjectPayload::Property { fset, .. } = &desc.payload {
                                    if let Some(setter) = fset {
                                        self.call_object(setter.clone(), vec![args[0].clone(), value])?;
                                        return Ok(PyObject::none());
                                    } else {
                                        return Err(PyException::attribute_error(format!(
                                            "can't set attribute '{}'", attr_name
                                        )));
                                    }
                                }
                                if is_data_descriptor(&desc) {
                                    if let Some(set_method) = desc.get_attr("__set__") {
                                        // set_method is already bound to desc
                                        self.call_object(set_method, vec![args[0].clone(), value])?;
                                        return Ok(PyObject::none());
                                    }
                                }
                            }
                            if let Some(sa) = lookup_in_class_mro(&inst.class, "__setattr__") {
                                if matches!(&sa.payload, PyObjectPayload::Function(_)) {
                                    let method = Arc::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod {
                                            receiver: args[0].clone(),
                                            method: sa,
                                        }
                                    });
                                    self.call_object(method, vec![PyObject::str_val(CompactString::from(&attr_name)), value])?;
                                    return Ok(PyObject::none());
                                }
                            }
                        }
                        return builtins::dispatch("setattr", &args);
                    }
                    "delattr" => {
                        if args.len() != 2 {
                            return Err(PyException::type_error("delattr() takes exactly 2 arguments"));
                        }
                        let attr_name = args[1].py_to_string();
                        if let PyObjectPayload::Instance(inst) = &args[0].payload {
                            if let Some(desc) = lookup_in_class_mro(&inst.class, &attr_name) {
                                if let PyObjectPayload::Property { fdel, .. } = &desc.payload {
                                    if let Some(deleter) = fdel {
                                        self.call_object(deleter.clone(), vec![args[0].clone()])?;
                                        return Ok(PyObject::none());
                                    }
                                }
                            }
                        }
                        return builtins::dispatch("delattr", &args);
                    }
                    _ => {}
                }
                match builtins::get_builtin_fn(name.as_str()) {
                    Some(f) => {
                        let result = f(&args);
                        // Check if breakpoint() was called
                        if crate::builtins::core_fns::BREAKPOINT_TRIGGERED
                            .swap(false, std::sync::atomic::Ordering::Relaxed)
                        {
                            self.breakpoints.builtin_breakpoint_pending = true;
                            self.handle_breakpoint_hit();
                        }
                        result
                    }
                    None => Err(PyException::type_error(format!(
                        "'{}' is not callable", name
                    ))),
                }
            }
            PyObjectPayload::Class(cd) => {
                // If class has a metaclass with __call__, dispatch through it
                if let Some(meta) = &cd.metaclass {
                    if let Some(call_method) = meta.get_attr("__call__") {
                        let mut call_args = vec![func.clone()];
                        call_args.extend(args);
                        return self.call_object(call_method, call_args);
                    }
                }
                self.instantiate_class(&func, args, vec![])
            }
            PyObjectPayload::BoundMethod { receiver, method } => {
                let mut bound_args = vec![receiver.clone()];
                bound_args.extend(args);
                self.call_object(method.clone(), bound_args)
            }
            PyObjectPayload::BuiltinBoundMethod { receiver, method_name } => {
                // ── Generator / Coroutine / AsyncGenerator dispatch ──
                // Extract gen_arc and discriminate the receiver kind for proper protocol.
                let gen_kind = match &receiver.payload {
                    PyObjectPayload::Generator(g) => Some(("generator", g.clone())),
                    PyObjectPayload::Coroutine(g) => Some(("coroutine", g.clone())),
                    PyObjectPayload::AsyncGenerator(g) => Some(("async_generator", g.clone())),
                    _ => None,
                };
                if let Some((kind, ref gen_arc)) = gen_kind {
                    match method_name.as_str() {
                        "send" => {
                            let val = if args.is_empty() { PyObject::none() } else { args[0].clone() };
                            return self.resume_generator(gen_arc, val);
                        }
                        "throw" => {
                            let (exc_kind, msg) = Self::parse_throw_args(&args);
                            return self.gen_throw(gen_arc, exc_kind, msg);
                        }
                        "close" => {
                            // CPython: throw GeneratorExit into the frame so finally blocks run.
                            // If generator yields during cleanup → RuntimeError.
                            let gen = gen_arc.read();
                            if gen.finished || gen.frame.is_none() {
                                // Already finished — nothing to clean up
                                drop(gen);
                                return Ok(PyObject::none());
                            }
                            drop(gen);
                            match self.gen_throw(gen_arc, ExceptionKind::GeneratorExit, String::new()) {
                                Ok(_yielded) => {
                                    // Generator yielded during close → RuntimeError
                                    return Err(PyException::runtime_error(
                                        "generator ignored GeneratorExit"
                                    ));
                                }
                                Err(e) if e.kind == ExceptionKind::GeneratorExit
                                       || e.kind == ExceptionKind::StopIteration => {
                                    // Expected: GeneratorExit propagated out or StopIteration
                                    let mut gen = gen_arc.write();
                                    gen.finished = true;
                                    gen.frame = None;
                                    return Ok(PyObject::none());
                                }
                                Err(e) => {
                                    // Other exception from finally block — propagate
                                    let mut gen = gen_arc.write();
                                    gen.finished = true;
                                    gen.frame = None;
                                    return Err(e);
                                }
                            }
                        }
                        "__next__" if kind != "async_generator" => {
                            return self.resume_generator(gen_arc, PyObject::none());
                        }
                        // ── Async generator protocol methods ──
                        // __aiter__ returns self (async generator is its own async iterator)
                        "__aiter__" if kind == "async_generator" => {
                            return Ok(receiver.clone());
                        }
                        // These return AsyncGenAwaitable objects, not direct results.
                        "__anext__" if kind == "async_generator" => {
                            return Ok(Arc::new(PyObject {
                                payload: PyObjectPayload::AsyncGenAwaitable {
                                    gen: gen_arc.clone(),
                                    action: AsyncGenAction::Next,
                                }
                            }));
                        }
                        "asend" if kind == "async_generator" => {
                            let val = if args.is_empty() { PyObject::none() } else { args[0].clone() };
                            return Ok(Arc::new(PyObject {
                                payload: PyObjectPayload::AsyncGenAwaitable {
                                    gen: gen_arc.clone(),
                                    action: AsyncGenAction::Send(val),
                                }
                            }));
                        }
                        "athrow" if kind == "async_generator" => {
                            let (exc_kind, msg) = Self::parse_throw_args(&args);
                            return Ok(Arc::new(PyObject {
                                payload: PyObjectPayload::AsyncGenAwaitable {
                                    gen: gen_arc.clone(),
                                    action: AsyncGenAction::Throw(exc_kind, CompactString::from(msg)),
                                }
                            }));
                        }
                        "aclose" if kind == "async_generator" => {
                            return Ok(Arc::new(PyObject {
                                payload: PyObjectPayload::AsyncGenAwaitable {
                                    gen: gen_arc.clone(),
                                    action: AsyncGenAction::Close,
                                }
                            }));
                        }
                        _ => {}
                    }
                }

                // ── Iterator protocol dispatch ──
                if let PyObjectPayload::Iterator(_) = &receiver.payload {
                    match method_name.as_str() {
                        "__next__" => {
                            match crate::builtins::iter_advance(&receiver)? {
                                Some((_new_iter, value)) => return Ok(value),
                                None => return Err(ferrython_core::error::PyException::stop_iteration()),
                            }
                        }
                        "__iter__" => {
                            return Ok(receiver.clone());
                        }
                        _ => {}
                    }
                }

                // ── AsyncGenAwaitable dispatch (driving the awaitable) ──
                if let PyObjectPayload::AsyncGenAwaitable { gen, action } = &receiver.payload {
                    match method_name.as_str() {
                        "send" => {
                            let send_val = if args.is_empty() { PyObject::none() } else { args[0].clone() };
                            return self.drive_async_gen_awaitable(gen, action, send_val);
                        }
                        "throw" => {
                            let (exc_kind, msg) = Self::parse_throw_args(&args);
                            return self.gen_throw(gen, exc_kind, msg);
                        }
                        "close" => {
                            return Ok(PyObject::none());
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
                // Class introspection methods
                if let PyObjectPayload::Class(cd) = &receiver.payload {
                    match method_name.as_str() {
                        "__subclasses__" => {
                            let subs = cd.subclasses.read();
                            let alive: Vec<PyObjectRef> = subs.iter()
                                .filter_map(|w| w.upgrade())
                                .collect();
                            drop(subs);
                            // Prune dead weak refs periodically
                            cd.subclasses.write().retain(|w| w.strong_count() > 0);
                            return Ok(PyObject::list(alive));
                        }
                        "mro" => {
                            let mut mro_list = vec![receiver.clone()];
                            mro_list.extend(cd.mro.iter().cloned());
                            return Ok(PyObject::list(mro_list));
                        }
                        _ => {}
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
                    if matches!(&inst.class.payload, PyObjectPayload::Class(cd) if cd.namespace.read().contains_key("__namedtuple__"))
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
                // Unbound method call: str.upper("hello") → call_method("hello", "upper", [])
                if let PyObjectPayload::BuiltinType(tn) = &receiver.payload {
                    // Class methods (e.g., int.from_bytes, dict.fromkeys)
                    if let Some(class_method) = builtins::resolve_type_class_method(tn, method_name) {
                        if let PyObjectPayload::NativeFunction { func, .. } = &class_method.payload {
                            return func(&args);
                        }
                    }
                    if !args.is_empty() {
                        let instance = args[0].clone();
                        let rest_args = if args.len() > 1 { args[1..].to_vec() } else { vec![] };
                        return builtins::call_method(&instance, method_name.as_str(), &rest_args);
                    }
                }
                // list.extend with generator/lazy iterator needs VM-level collection
                if method_name.as_str() == "extend" && !args.is_empty() {
                    if matches!(receiver.payload, PyObjectPayload::List(_)) {
                        if matches!(args[0].payload, PyObjectPayload::Generator(_)) ||
                           (matches!(&args[0].payload, PyObjectPayload::Iterator(ref d) if {
                               let data = d.lock().unwrap();
                               matches!(&*data, IteratorData::Enumerate { .. } | IteratorData::Zip { .. }
                                   | IteratorData::Map { .. } | IteratorData::Filter { .. }
                                   | IteratorData::Sentinel { .. })
                           }))
                        {
                            let items = self.collect_iterable(&args[0])?;
                            return builtins::call_method(receiver, "extend", &[PyObject::list(items)]);
                        }
                    }
                }
                // list.sort(key=, reverse=) needs VM for key function calls
                if method_name.as_str() == "sort" {
                    if let PyObjectPayload::List(items) = &receiver.payload {
                        // Extract key and reverse from trailing kwargs dict
                        let mut key_fn: Option<PyObjectRef> = None;
                        let mut reverse = false;
                        for arg in &args {
                            if let PyObjectPayload::Dict(d) = &arg.payload {
                                let rd = d.read();
                                if let Some(v) = rd.get(&HashableKey::Str(CompactString::from("reverse"))) {
                                    reverse = v.is_truthy();
                                }
                                if let Some(v) = rd.get(&HashableKey::Str(CompactString::from("key"))) {
                                    if !matches!(v.payload, PyObjectPayload::None) {
                                        key_fn = Some(v.clone());
                                    }
                                }
                            }
                        }
                        if let Some(key) = key_fn {
                            // Decorate-sort-undecorate (Schwartzian transform)
                            let mut w = items.write();
                            let mut decorated: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
                            for item in w.iter() {
                                let k = self.call_object(key.clone(), vec![item.clone()])?;
                                decorated.push((k, item.clone()));
                            }
                            let keys: Vec<PyObjectRef> = decorated.iter().map(|(k, _)| k.clone()).collect();
                            let mut indices: Vec<usize> = (0..decorated.len()).collect();
                            for i in 1..indices.len() {
                                let mut j = i;
                                while j > 0 {
                                    if self.vm_lt(&keys[indices[j]], &keys[indices[j - 1]])? {
                                        indices.swap(j, j - 1);
                                        j -= 1;
                                    } else {
                                        break;
                                    }
                                }
                            }
                            w.clear();
                            for i in indices {
                                w.push(decorated[i].1.clone());
                            }
                            if reverse {
                                w.reverse();
                            }
                            return Ok(PyObject::none());
                        } else if reverse {
                            let mut w = items.write();
                            let mut v: Vec<_> = w.drain(..).collect();
                            self.vm_sort(&mut v)?;
                            v.reverse();
                            w.extend(v);
                            return Ok(PyObject::none());
                        }
                        // No key or reverse — fall through to basic sort
                    }
                }
                // str.format with positional args: needs VM for __str__ on instances
                if method_name.as_str() == "format" {
                    if let PyObjectPayload::Str(s) = &receiver.payload {
                        return self.vm_str_format(s, &args);
                    }
                }
                // str.format_map with dict subclass: needs VM for __missing__ calls
                if method_name.as_str() == "format_map" && !args.is_empty() {
                    if let PyObjectPayload::Str(s) = &receiver.payload {
                        if let PyObjectPayload::Instance(inst) = &args[0].payload {
                            if let Some(ref ds) = inst.dict_storage {
                                return self.vm_format_map(s, &args[0], ds, &inst.class);
                            }
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
                Ok(PyObject::exception_instance_with_args(kind.clone(), msg, args))
            }
            PyObjectPayload::NativeFunction { func, name } => {
                // Intercept functions that need VM access to call Python callables
                if name.as_str() == "functools.reduce" {
                    return self.vm_functools_reduce(&args);
                }
                if name.as_str() == "itertools.islice" {
                    return self.vm_itertools_islice(&args);
                }
                // singledispatch.register: register(type) → decorator
                if name.as_str() == "singledispatch.register" {
                    return self.vm_singledispatch_register(&args);
                }
                // type.__call__(cls, *args) — standard class instantiation protocol
                if name.as_str() == "__type_call__" {
                    if args.is_empty() {
                        return Err(PyException::type_error("type.__call__ requires cls"));
                    }
                    let cls = args[0].clone();
                    let rest = args[1..].to_vec();
                    return self.instantiate_class(&cls, rest, vec![]);
                }
                // re.sub / re.subn with callable replacement
                if (name.as_str() == "re.sub" || name.as_str() == "re.subn") && args.len() >= 3 {
                    let repl = &args[1];
                    let is_callable = matches!(&repl.payload,
                        PyObjectPayload::Function(_) | PyObjectPayload::BuiltinFunction(_)
                        | PyObjectPayload::NativeFunction { .. } | PyObjectPayload::NativeClosure { .. }
                        | PyObjectPayload::Partial { .. });
                    if is_callable {
                        return self.re_sub_with_callable(&args, name.as_str() == "re.subn");
                    }
                }
                if name.as_str() == "itertools.groupby" {
                    let mut key_fn = None;
                    let mut iterable_end = args.len();
                    // Check last arg for kwargs dict with "key"
                    if let Some(last) = args.last() {
                        if let PyObjectPayload::Dict(map) = &last.payload {
                            let map_r = map.read();
                            key_fn = map_r.get(&HashableKey::Str(CompactString::from("key"))).cloned();
                            if key_fn.is_some() {
                                iterable_end = args.len() - 1;
                            }
                        }
                    }
                    // Check for positional key arg (2nd arg, not a dict)
                    if key_fn.is_none() && iterable_end >= 2 {
                        key_fn = Some(args[1].clone());
                        iterable_end = 1;
                    }
                    return self.vm_itertools_groupby(&args[..iterable_end], key_fn);
                }
                if name.as_str() == "itertools.filterfalse" && args.len() >= 2 {
                    return self.vm_itertools_filterfalse(&args);
                }
                if name.as_str() == "itertools.starmap" && args.len() >= 2 {
                    return self.vm_itertools_starmap(&args);
                }
                if name.as_str() == "itertools.accumulate" && args.len() >= 2 {
                    return self.vm_itertools_accumulate(&args);
                }
                // math.trunc / math.floor / math.ceil — dispatch to __trunc__ / __floor__ / __ceil__
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        let dunder = match name.as_str() {
                            "math.trunc" => Some("__trunc__"),
                            "math.floor" => Some("__floor__"),
                            "math.ceil" => Some("__ceil__"),
                            _ => None,
                        };
                        if let Some(dunder_name) = dunder {
                            if let Some(method) = args[0].get_attr(dunder_name) {
                                let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] };
                                return self.call_object(method, ca);
                            }
                        }
                    }
                }
                // os.fspath — dispatch to __fspath__
                if name.as_str() == "os.fspath" && args.len() == 1 {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(method) = args[0].get_attr("__fspath__") {
                            let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod{..}) { vec![] } else { vec![args[0].clone()] };
                            return self.call_object(method, ca);
                        }
                    }
                }
                // Resolve generators to lists for unnamed stdlib NativeFunctions
                // that expect iterables (e.g. Counter, deque, OrderedDict)
                if name.is_empty() && !args.is_empty()
                    && matches!(&args[0].payload, PyObjectPayload::Generator(_))
                {
                    let mut resolved = Vec::with_capacity(args.len());
                    resolved.push(PyObject::list(self.collect_iterable(&args[0])?));
                    resolved.extend_from_slice(&args[1..]);
                    return func(&resolved);
                }
                let result = func(&args)?;
                // Check if native function requested VM method calls
                let mut last_result = None;
                while let Some((method, margs)) = ferrython_core::error::take_pending_vm_call() {
                    last_result = Some(self.call_object(method, margs)?);
                }
                if let Some(r) = last_result {
                    return Ok(r);
                }
                // Execute any deferred calls (e.g., HTMLParser.feed() callbacks)
                let deferred = ferrython_stdlib::drain_deferred_calls();
                for (dfunc, dargs) in deferred {
                    self.call_object(dfunc, dargs)?;
                }
                Ok(result)
            }
            PyObjectPayload::NativeClosure { func, .. } => {
                let result = func(&args)?;
                // Check if stdlib requested VM method calls (loop for multiple)
                let mut last_result = None;
                while let Some((method, margs)) = ferrython_core::error::take_pending_vm_call() {
                    last_result = Some(self.call_object(method, margs)?);
                }
                if let Some(r) = last_result {
                    return Ok(r);
                }
                // Execute any deferred calls (e.g., Thread.start() calling Python functions)
                let deferred = ferrython_stdlib::drain_deferred_calls();
                for (dfunc, dargs) in deferred {
                    self.call_object(dfunc, dargs)?;
                }
                // Check if asyncio.run() was invoked — drive the coroutine to completion
                if let Some(coro) = ferrython_stdlib::take_asyncio_run_coro() {
                    return self.maybe_await_result(coro);
                }
                Ok(result)
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
            PyObjectPayload::Instance(_inst) => {
                // lru_cache wrapper: check _cache + __wrapped__
                if let Some(cache_obj) = func.get_attr("_cache") {
                    if let Some(wrapped) = func.get_attr("__wrapped__") {
                        if let PyObjectPayload::Dict(cache_map) = &cache_obj.payload {
                            // Build cache key from stringified args
                            let key_str = args.iter().map(|a| a.repr()).collect::<Vec<_>>().join(",");
                            let cache_key = HashableKey::Str(CompactString::from(&key_str));
                            // Check cache
                            if let Some(cached) = cache_map.read().get(&cache_key) {
                                // Cache hit: increment _hits counter
                                if let PyObjectPayload::Instance(ref d) = func.payload {
                                    let mut w = d.attrs.write();
                                    let hits = w.get(&intern_or_new("_hits"))
                                        .and_then(|v| v.as_int()).unwrap_or(0);
                                    w.insert(intern_or_new("_hits"), PyObject::int(hits + 1));
                                }
                                return Ok(cached.clone());
                            }
                            // Cache miss: call the wrapped function, increment _misses
                            if let PyObjectPayload::Instance(ref d) = func.payload {
                                let mut w = d.attrs.write();
                                let misses = w.get(&intern_or_new("_misses"))
                                    .and_then(|v| v.as_int()).unwrap_or(0);
                                w.insert(intern_or_new("_misses"), PyObject::int(misses + 1));
                            }
                            let result = self.call_object(wrapped, args)?;
                            // Enforce maxsize: evict oldest entry (FIFO) when cache is full
                            {
                                let mut cache_w = cache_map.write();
                                if let PyObjectPayload::Instance(ref d) = func.payload {
                                    let maxsize = d.attrs.read()
                                        .get(&intern_or_new("_maxsize"))
                                        .and_then(|v| v.as_int());
                                    if let Some(max) = maxsize {
                                        if max >= 0 {
                                            while cache_w.len() >= max as usize {
                                                cache_w.shift_remove_index(0);
                                            }
                                        }
                                    }
                                }
                                cache_w.insert(cache_key, result.clone());
                            }
                            return Ok(result);
                        }
                    }
                }
                // Callable instances: check for __call__
                if func.get_attr("__singledispatch__").is_some() {
                    // singledispatch: dispatch based on first arg type
                    return self.vm_singledispatch_call_instance(&func, &args);
                }
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

    /// Install closure cells, set scope, and either return generator/coroutine or execute frame.
    fn install_closure_and_run(
        &mut self,
        mut frame: Frame,
        code: &CodeObject,
        closure: &[Arc<RwLock<Option<PyObjectRef>>>],
    ) -> PyResult<PyObjectRef> {
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

        if code.flags.contains(CodeFlags::GENERATOR) && code.flags.contains(CodeFlags::COROUTINE) {
            return Ok(PyObject::async_generator(CompactString::from(code.name.as_str()), Box::new(frame)));
        }
        if code.flags.contains(CodeFlags::COROUTINE) {
            return Ok(PyObject::coroutine(CompactString::from(code.name.as_str()), Box::new(frame)));
        }
        if code.flags.contains(CodeFlags::GENERATOR) {
            return Ok(PyObject::generator(CompactString::from(code.name.as_str()), Box::new(frame)));
        }

        self.call_stack.push(frame);
        // Check recursion limit before running
        let limit = ferrython_stdlib::get_recursion_limit() as usize;
        if self.call_stack.len() > limit {
            if let Some(frame) = self.call_stack.pop() {
                frame.recycle(&mut self.frame_pool);
            }
            return Err(PyException::recursion_error(
                "maximum recursion depth exceeded"
            ));
        }
        let result = self.run_frame();
        if let Some(frame) = self.call_stack.pop() {
            frame.recycle(&mut self.frame_pool);
        }
        result
    }

    /// Schwartzian transform: sort items by key function, optionally reversed.
    fn sort_with_key(
        &mut self,
        items: &mut Vec<PyObjectRef>,
        key_fn: Option<PyObjectRef>,
        reverse: bool,
    ) -> PyResult<()> {
        if let Some(key) = key_fn {
            // Check if key is a cmp_to_key class — use comparison function directly
            if let PyObjectPayload::Class(cd) = &key.payload {
                if let Some(cmp_func) = cd.namespace.read().get("__cmp_to_key_func__").cloned() {
                    // Sort using comparison function: cmp(a, b) < 0 means a < b
                    let mut indices: Vec<usize> = (0..items.len()).collect();
                    for i in 1..indices.len() {
                        let mut j = i;
                        while j > 0 {
                            let a = &items[indices[j]];
                            let b = &items[indices[j - 1]];
                            let result = self.call_object(cmp_func.clone(), vec![a.clone(), b.clone()])?;
                            let cmp_val = result.to_int().unwrap_or(0);
                            if cmp_val < 0 {
                                indices.swap(j, j - 1);
                                j -= 1;
                            } else {
                                break;
                            }
                        }
                    }
                    *items = indices.into_iter().map(|i| items[i].clone()).collect();
                    if reverse { items.reverse(); }
                    return Ok(());
                }
            }
            // Normal key function sort
            let mut decorated: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
            for item in items.iter() {
                let k = self.call_object(key.clone(), vec![item.clone()])?;
                decorated.push((k, item.clone()));
            }
            let mut indices: Vec<usize> = (0..decorated.len()).collect();
            for i in 1..indices.len() {
                let mut j = i;
                while j > 0 {
                    let cmp = if reverse {
                        // Sort descending directly for stable reverse
                        self.vm_lt(&decorated[indices[j - 1]].0, &decorated[indices[j]].0)?
                    } else {
                        self.vm_lt(&decorated[indices[j]].0, &decorated[indices[j - 1]].0)?
                    };
                    if cmp {
                        indices.swap(j, j - 1);
                        j -= 1;
                    } else {
                        break;
                    }
                }
            }
            *items = indices.into_iter().map(|i| decorated[i].1.clone()).collect();
        } else {
            self.vm_sort(items)?;
            if reverse {
                items.reverse();
            }
        }
        Ok(())
    }

    /// Compute min or max from a collection, with optional key function and default value.
    fn compute_min_max(
        &mut self,
        items: Vec<PyObjectRef>,
        is_max: bool,
        key_fn: Option<PyObjectRef>,
        default: Option<PyObjectRef>,
        func_name: &str,
    ) -> PyResult<PyObjectRef> {
        if items.is_empty() {
            return if let Some(d) = default {
                Ok(d)
            } else {
                Err(PyException::value_error(format!("{}() arg is an empty sequence", func_name)))
            };
        }
        let mut best = items[0].clone();
        let mut best_key = if let Some(ref kf) = key_fn {
            self.call_object(kf.clone(), vec![best.clone()])?
        } else {
            best.clone()
        };
        for item in &items[1..] {
            let item_key = if let Some(ref kf) = key_fn {
                self.call_object(kf.clone(), vec![item.clone()])?
            } else {
                item.clone()
            };
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
        Ok(best)
    }

    /// Pre-process an object tree for json.dumps: replace non-JSON-serializable
    /// values by calling `default(obj)` (a user Python function). Basic types
    /// (dict, list, tuple, str, int, float, bool, None) are passed through.
    fn json_prepare_with_default(
        &mut self,
        obj: &PyObjectRef,
        default_fn: &PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        match &obj.payload {
            PyObjectPayload::Dict(map) => {
                let entries: Vec<_> = map.read().iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let mut new_map = IndexMap::new();
                for (k, v) in entries {
                    new_map.insert(k, self.json_prepare_with_default(&v, default_fn)?);
                }
                Ok(PyObject::dict(new_map))
            }
            PyObjectPayload::List(items) => {
                let items: Vec<_> = items.read().clone();
                let mut prepared = Vec::with_capacity(items.len());
                for item in &items {
                    prepared.push(self.json_prepare_with_default(item, default_fn)?);
                }
                Ok(PyObject::list(prepared))
            }
            PyObjectPayload::Tuple(items) => {
                let mut prepared = Vec::with_capacity(items.len());
                for item in items.iter() {
                    prepared.push(self.json_prepare_with_default(item, default_fn)?);
                }
                Ok(PyObject::tuple(prepared))
            }
            PyObjectPayload::Str(_)
            | PyObjectPayload::Int(_)
            | PyObjectPayload::Float(_)
            | PyObjectPayload::Bool(_)
            | PyObjectPayload::None => Ok(obj.clone()),
            _ => {
                // Call default(obj) and recursively prepare the result
                let result = self.call_object(default_fn.clone(), vec![obj.clone()])?;
                self.json_prepare_with_default(&result, default_fn)
            }
        }
    }
}
