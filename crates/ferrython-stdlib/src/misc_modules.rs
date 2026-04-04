//! Miscellaneous stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, CompareOp, InstanceData,
    make_module, make_builtin, check_args, check_args_min,
};
use ferrython_core::types::{HashableKey, PyInt};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

use super::serial_modules::extract_bytes;

// ── contextlib module ──


pub fn create_contextlib_module() -> PyObjectRef {
    // suppress(*exceptions) — context manager that suppresses specified exceptions
    let suppress_fn = make_builtin(|args: &[PyObjectRef]| {
        let exceptions: Vec<PyObjectRef> = args.to_vec();
        let suppress_cls = PyObject::class(
            CompactString::from("suppress"),
            vec![],
            IndexMap::new(),
        );
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("__suppress_exceptions__"), PyObject::list(exceptions));
        attrs.insert(CompactString::from("__enter__"), PyObject::native_function(
            "suppress.__enter__", |args: &[PyObjectRef]| {
                if args.is_empty() { return Ok(PyObject::none()); }
                Ok(args[0].clone())
            }
        ));
        attrs.insert(CompactString::from("__exit__"), PyObject::native_function(
            "suppress.__exit__", |args: &[PyObjectRef]| {
                // args: self, exc_type, exc_val, exc_tb
                if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
                    let exc_type = &args[1];
                    // Get the exception kind from the exception type
                    let exc_kind = match &exc_type.payload {
                        PyObjectPayload::ExceptionType(k) => Some(k.clone()),
                        _ => {
                            // Fall back to name-based lookup
                            let name = exc_type.py_to_string();
                            ferrython_core::error::ExceptionKind::from_name(name.trim_start_matches("<class '").trim_end_matches("'>"))
                        }
                    };
                    if let Some(exc_kind) = exc_kind {
                        if let Some(suppressed) = args[0].get_attr("__suppress_exceptions__") {
                            if let Ok(exc_list) = suppressed.to_list() {
                                for allowed in &exc_list {
                                    let allowed_kind = match &allowed.payload {
                                        PyObjectPayload::ExceptionType(k) => Some(k.clone()),
                                        _ => {
                                            let name = allowed.py_to_string();
                                            ferrython_core::error::ExceptionKind::from_name(name.trim_start_matches("<class '").trim_end_matches("'>"))
                                        }
                                    };
                                    if let Some(allowed_kind) = allowed_kind {
                                        if exc_kind.is_subclass_of(&allowed_kind) {
                                            return Ok(PyObject::bool_val(true));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(PyObject::bool_val(false))
            }
        ));
        Ok(PyObject::instance_with_attrs(suppress_cls, attrs))
    });

    // ExitStack — real context manager with callback registration
    let exit_stack_cls = {
        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("__exitstack__"), PyObject::bool_val(true));
        PyObject::class(CompactString::from("ExitStack"), vec![], ns)
    };

    let exit_stack_cls_clone = exit_stack_cls.clone();
    let exit_stack_fn = PyObject::native_closure("ExitStack", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(exit_stack_cls_clone.clone());
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(CompactString::from("_callbacks"), PyObject::list(vec![]));

            attrs.insert(CompactString::from("__enter__"), PyObject::native_function(
                "ExitStack.__enter__", |args: &[PyObjectRef]| {
                    if args.is_empty() { return Ok(PyObject::none()); }
                    Ok(args[0].clone())
                }
            ));

            attrs.insert(CompactString::from("__exit__"), PyObject::native_function(
                "ExitStack.__exit__", |args: &[PyObjectRef]| {
                    // Call registered callbacks in reverse order (best-effort for native fns)
                    if let Some(self_obj) = args.first() {
                        if let Some(cbs) = self_obj.get_attr("_callbacks") {
                            if let Ok(items) = cbs.to_list() {
                                for cb in items.iter().rev() {
                                    match &cb.payload {
                                        PyObjectPayload::NativeFunction { func, .. } => {
                                            let _ = func(&[]);
                                        }
                                        PyObjectPayload::NativeClosure { func, .. } => {
                                            let _ = func(&[]);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    Ok(PyObject::bool_val(false))
                }
            ));

            attrs.insert(CompactString::from("push"), PyObject::native_function(
                "ExitStack.push", |args: &[PyObjectRef]| {
                    check_args_min("ExitStack.push", args, 2)?;
                    let self_obj = &args[0];
                    let callback = &args[1];
                    if let Some(cbs) = self_obj.get_attr("_callbacks") {
                        if let PyObjectPayload::List(items) = &cbs.payload {
                            items.write().push(callback.clone());
                        }
                    }
                    Ok(callback.clone())
                }
            ));

            attrs.insert(CompactString::from("callback"), PyObject::native_function(
                "ExitStack.callback", |args: &[PyObjectRef]| {
                    check_args_min("ExitStack.callback", args, 2)?;
                    let self_obj = &args[0];
                    let func = &args[1];
                    if let Some(cbs) = self_obj.get_attr("_callbacks") {
                        if let PyObjectPayload::List(items) = &cbs.payload {
                            items.write().push(func.clone());
                        }
                    }
                    Ok(func.clone())
                }
            ));

            attrs.insert(CompactString::from("enter_context"), PyObject::native_function(
                "ExitStack.enter_context", |args: &[PyObjectRef]| {
                    check_args_min("ExitStack.enter_context", args, 2)?;
                    let self_obj = &args[0];
                    let cm = &args[1];
                    // Call __enter__ if it's a native function
                    let result = if let Some(enter) = cm.get_attr("__enter__") {
                        match &enter.payload {
                            PyObjectPayload::NativeFunction { func, .. } => {
                                func(&[cm.clone()])?
                            }
                            PyObjectPayload::NativeClosure { func, .. } => {
                                func(&[cm.clone()])?
                            }
                            _ => PyObject::none()
                        }
                    } else {
                        PyObject::none()
                    };
                    // Register __exit__ as callback
                    if let Some(exit_fn) = cm.get_attr("__exit__") {
                        if let Some(cbs) = self_obj.get_attr("_callbacks") {
                            if let PyObjectPayload::List(items) = &cbs.payload {
                                items.write().push(exit_fn);
                            }
                        }
                    }
                    Ok(result)
                }
            ));
        }
        Ok(inst)
    });

    // nullcontext(enter_result=None) — no-op context manager
    let nullcontext_fn = make_builtin(|args: &[PyObjectRef]| {
        let enter_result = if !args.is_empty() {
            args[0].clone()
        } else {
            PyObject::none()
        };
        let cls = PyObject::class(CompactString::from("nullcontext"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        let enter_val = enter_result.clone();
        attrs.insert(CompactString::from("__enter__"), PyObject::native_closure(
            "nullcontext.__enter__", move |_args: &[PyObjectRef]| {
                Ok(enter_val.clone())
            }
        ));
        attrs.insert(CompactString::from("__exit__"), PyObject::native_function(
            "nullcontext.__exit__", |_args: &[PyObjectRef]| {
                Ok(PyObject::bool_val(false))
            }
        ));
        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    // closing(thing) — context manager that calls thing.close() on exit
    let closing_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Err(PyException::type_error("closing requires 1 argument")); }
        let thing = args[0].clone();
        let cls = PyObject::class(CompactString::from("closing"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        let thing_enter = thing.clone();
        attrs.insert(CompactString::from("__enter__"), PyObject::native_closure(
            "closing.__enter__", move |_args: &[PyObjectRef]| {
                Ok(thing_enter.clone())
            }
        ));
        let thing_exit = thing.clone();
        attrs.insert(CompactString::from("__exit__"), PyObject::native_closure(
            "closing.__exit__", move |_args: &[PyObjectRef]| {
                if let Some(close_fn) = thing_exit.get_attr("close") {
                    match &close_fn.payload {
                        PyObjectPayload::NativeFunction { func, .. } => { let _ = func(&[thing_exit.clone()]); }
                        PyObjectPayload::NativeClosure { func, .. } => { let _ = func(&[thing_exit.clone()]); }
                        _ => {}
                    }
                }
                Ok(PyObject::bool_val(false))
            }
        ));
        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    // redirect_stdout(new_target) — context manager stub
    let redirect_stdout_fn = make_builtin(|args: &[PyObjectRef]| {
        let target = if !args.is_empty() { args[0].clone() } else { PyObject::none() };
        let cls = PyObject::class(CompactString::from("redirect_stdout"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        let t = target.clone();
        attrs.insert(CompactString::from("__enter__"), PyObject::native_closure(
            "redirect_stdout.__enter__", move |_args: &[PyObjectRef]| {
                Ok(t.clone())
            }
        ));
        attrs.insert(CompactString::from("__exit__"), PyObject::native_function(
            "redirect_stdout.__exit__", |_args: &[PyObjectRef]| {
                Ok(PyObject::bool_val(false))
            }
        ));
        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    // redirect_stderr(new_target) — context manager stub
    let redirect_stderr_fn = make_builtin(|args: &[PyObjectRef]| {
        let target = if !args.is_empty() { args[0].clone() } else { PyObject::none() };
        let cls = PyObject::class(CompactString::from("redirect_stderr"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        let t = target.clone();
        attrs.insert(CompactString::from("__enter__"), PyObject::native_closure(
            "redirect_stderr.__enter__", move |_args: &[PyObjectRef]| {
                Ok(t.clone())
            }
        ));
        attrs.insert(CompactString::from("__exit__"), PyObject::native_function(
            "redirect_stderr.__exit__", |_args: &[PyObjectRef]| {
                Ok(PyObject::bool_val(false))
            }
        ));
        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    make_module("contextlib", vec![
        ("contextmanager", make_builtin(contextlib_contextmanager)),
        ("suppress", suppress_fn),
        ("closing", closing_fn),
        ("ExitStack", exit_stack_fn),
        ("nullcontext", nullcontext_fn),
        ("redirect_stdout", redirect_stdout_fn),
        ("redirect_stderr", redirect_stderr_fn),
    ])
}

fn contextlib_contextmanager(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // contextmanager decorator — returns the function unchanged.
    // The function is a generator function. When called, it returns a Generator.
    // The VM's SetupWith handles Generator objects as context managers directly.
    if args.is_empty() { return Err(PyException::type_error("contextmanager requires 1 argument")); }
    Ok(args[0].clone())
}

// ── dataclasses module ──


pub fn create_dataclasses_module() -> PyObjectRef {
    make_module("dataclasses", vec![
        ("dataclass", make_builtin(dataclass_decorator)),
        ("field", make_builtin(|args| {
            // field(default=..., default_factory=..., ...)
            // kwargs passed as trailing dict by VM
            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(kw_map) = &last.payload {
                    let r = kw_map.read();
                    // Check for default_factory
                    if let Some(factory) = r.get(&HashableKey::Str(CompactString::from("default_factory"))) {
                        // Return a sentinel with __field_factory__ marker
                        let mut attrs = IndexMap::new();
                        attrs.insert(CompactString::from("__field_factory__"), factory.clone());
                        return Ok(PyObject::module_with_attrs(CompactString::from("_field"), attrs));
                    }
                    if let Some(default) = r.get(&HashableKey::Str(CompactString::from("default"))) {
                        return Ok(default.clone());
                    }
                }
            }
            let default = if args.is_empty() { PyObject::none() } else { args[0].clone() };
            Ok(default)
        })),
        ("asdict", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("asdict requires 1 argument")); }
            // Convert instance attrs to dict
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                let attrs = inst.attrs.read();
                let mut map = IndexMap::new();
                for (k, v) in attrs.iter() {
                    if !k.starts_with('_') {
                        map.insert(HashableKey::Str(k.clone()), v.clone());
                    }
                }
                Ok(PyObject::dict(map))
            } else {
                Err(PyException::type_error("asdict() should be called on dataclass instances"))
            }
        })),
        ("astuple", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("astuple requires 1 argument")); }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                let attrs = inst.attrs.read();
                let items: Vec<_> = attrs.values().cloned().collect();
                Ok(PyObject::tuple(items))
            } else {
                Err(PyException::type_error("astuple() should be called on dataclass instances"))
            }
        })),
        ("fields", make_builtin(|_| Ok(PyObject::tuple(vec![])))),
    ])
}

fn dataclass_decorator(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("dataclass requires 1 argument")); }
    let cls = &args[0];
    
    // Get annotations to discover fields
    let mut field_names: Vec<CompactString> = Vec::new();
    let mut field_defaults: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    
    if let PyObjectPayload::Class(cd) = &cls.payload {
        let ns = cd.namespace.read();
        if let Some(annotations) = ns.get("__annotations__") {
            if let PyObjectPayload::Dict(ann_map) = &annotations.payload {
                for (k, _v) in ann_map.read().iter() {
                    if let HashableKey::Str(name) = k {
                        field_names.push(name.clone());
                        // Check for default value in class namespace
                        if let Some(default) = ns.get(name.as_str()) {
                            // Check if it's a field() sentinel with factory (Module with __field_factory__ attr)
                            if let PyObjectPayload::Module(md) = &default.payload {
                                let mod_attrs = md.attrs.read();
                                if let Some(factory) = mod_attrs.get("__field_factory__") {
                                    field_defaults.insert(name.clone(), factory.clone());
                                } else {
                                    field_defaults.insert(name.clone(), default.clone());
                                }
                            } else {
                                field_defaults.insert(name.clone(), default.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Store __dataclass_fields__ as a tuple of (name, has_default, default_val) tuples
    let fields_info: Vec<PyObjectRef> = field_names.iter().map(|name| {
        let has_default = field_defaults.contains_key(name.as_str());
        let default_val = field_defaults.get(name.as_str()).cloned().unwrap_or_else(PyObject::none);
        PyObject::tuple(vec![
            PyObject::str_val(CompactString::from(name.as_str())),
            PyObject::bool_val(has_default),
            default_val,
        ])
    }).collect();
    
    // Store on the class
    if let PyObjectPayload::Class(cd) = &cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(CompactString::from("__dataclass_fields__"), PyObject::tuple(fields_info));
        // Mark it as a dataclass
        ns.insert(CompactString::from("__dataclass__"), PyObject::bool_val(true));
    }
    
    Ok(cls.clone())
}

// ── struct module ──


pub fn create_copy_module() -> PyObjectRef {
    make_module("copy", vec![
        ("copy", make_builtin(copy_copy)),
        ("deepcopy", make_builtin(copy_deepcopy)),
    ])
}

fn copy_copy(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("copy() requires 1 argument")); }
    shallow_copy(&args[0])
}

fn copy_deepcopy(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("deepcopy() requires 1 argument")); }
    deep_copy(&args[0])
}

fn shallow_copy(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::None | PyObjectPayload::Bool(_) | PyObjectPayload::Int(_)
        | PyObjectPayload::Float(_) | PyObjectPayload::Str(_) | PyObjectPayload::Bytes(_)
        | PyObjectPayload::FrozenSet(_) => Ok(obj.clone()),
        PyObjectPayload::Tuple(items) => Ok(PyObject::tuple(items.clone())),
        PyObjectPayload::List(items) => Ok(PyObject::list(items.read().clone())),
        PyObjectPayload::Dict(map) => Ok(PyObject::dict(map.read().clone())),
        PyObjectPayload::Set(set) => Ok(PyObject::set(set.read().clone())),
        PyObjectPayload::Instance(inst) => {
            // Create new instance with same class, shallow copy of attrs
            Ok(PyObject::wrap(PyObjectPayload::Instance(InstanceData {
                class: inst.class.clone(),
                attrs: Arc::new(RwLock::new(inst.attrs.read().clone())),
                dict_storage: inst.dict_storage.as_ref().map(|ds| Arc::new(RwLock::new(ds.read().clone()))),
            })))
        }
        _ => Ok(obj.clone()),
    }
}

fn deep_copy(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::None | PyObjectPayload::Bool(_) | PyObjectPayload::Int(_)
        | PyObjectPayload::Float(_) | PyObjectPayload::Str(_) | PyObjectPayload::Bytes(_)
        | PyObjectPayload::FrozenSet(_) => Ok(obj.clone()),
        PyObjectPayload::Tuple(items) => {
            let new_items: Result<Vec<_>, _> = items.iter().map(|x| deep_copy(x)).collect();
            Ok(PyObject::tuple(new_items?))
        }
        PyObjectPayload::List(items) => {
            let new_items: Result<Vec<_>, _> = items.read().iter().map(|x| deep_copy(x)).collect();
            Ok(PyObject::list(new_items?))
        }
        PyObjectPayload::Dict(map) => {
            let mut new_map = IndexMap::new();
            for (k, v) in map.read().iter() {
                new_map.insert(k.clone(), deep_copy(v)?);
            }
            Ok(PyObject::dict(new_map))
        }
        PyObjectPayload::Set(set) => {
            let mut new_set = IndexMap::new();
            for (k, v) in set.read().iter() {
                new_set.insert(k.clone(), deep_copy(v)?);
            }
            Ok(PyObject::set(new_set))
        }
        PyObjectPayload::Instance(inst) => {
            let mut new_attrs = IndexMap::new();
            for (k, v) in inst.attrs.read().iter() {
                new_attrs.insert(k.clone(), deep_copy(v)?);
            }
            Ok(PyObject::instance_with_attrs(inst.class.clone(), new_attrs))
        }
        _ => Ok(obj.clone()),
    }
}

// ── operator module ──


pub fn create_operator_module() -> PyObjectRef {
    make_module("operator", vec![
        ("add", make_builtin(|args| {
            check_args_min("add", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    return Ok(PyObject::int(a + b));
                }
            }
            if let (Ok(a), Ok(b)) = (args[0].to_float(), args[1].to_float()) {
                Ok(PyObject::float(a + b))
            } else {
                let a = args[0].py_to_string();
                let b = args[1].py_to_string();
                Ok(PyObject::str_val(CompactString::from(format!("{}{}", a, b))))
            }
        })),
        ("sub", make_builtin(|args| {
            check_args_min("sub", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    return Ok(PyObject::int(a - b));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a - b))
        })),
        ("mul", make_builtin(|args| {
            check_args_min("mul", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    return Ok(PyObject::int(a * b));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a * b))
        })),
        ("truediv", make_builtin(|args| {
            check_args_min("truediv", args, 2)?;
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            if b == 0.0 { return Err(PyException::zero_division_error("division by zero")); }
            Ok(PyObject::float(a / b))
        })),
        ("floordiv", make_builtin(|args| {
            check_args_min("floordiv", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b == 0 { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                    return Ok(PyObject::int(a.div_euclid(b)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            if b == 0.0 { return Err(PyException::zero_division_error("float floor division by zero")); }
            Ok(PyObject::float((a / b).floor()))
        })),
        ("mod_", make_builtin(|args| {
            check_args_min("mod_", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b == 0 { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                    return Ok(PyObject::int(a.rem_euclid(b)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a % b))
        })),
        // Also register as "mod" for getattr(operator, "mod") usage
        ("mod", make_builtin(|args| {
            check_args_min("mod", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b == 0 { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                    return Ok(PyObject::int(a.rem_euclid(b)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a % b))
        })),
        ("neg", make_builtin(|args| {
            check_args_min("neg", args, 1)?;
            if matches!(&args[0].payload, PyObjectPayload::Float(_)) {
                Ok(PyObject::float(-args[0].to_float()?))
            } else if let Ok(n) = args[0].to_int() {
                Ok(PyObject::int(-n))
            } else {
                Ok(PyObject::float(-args[0].to_float()?))
            }
        })),
        ("pow", make_builtin(|args| {
            check_args_min("pow", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b >= 0 {
                        return Ok(PyObject::int(a.pow(b as u32)));
                    }
                    return Ok(PyObject::float((a as f64).powf(b as f64)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a.powf(b)))
        })),
        ("pos", make_builtin(|args| {
            check_args_min("pos", args, 1)?;
            Ok(args[0].clone())
        })),
        ("not_", make_builtin(|args| {
            check_args_min("not_", args, 1)?;
            Ok(PyObject::bool_val(!args[0].is_truthy()))
        })),
        ("eq", make_builtin(|args| {
            check_args_min("eq", args, 2)?;
            args[0].compare(&args[1], CompareOp::Eq)
        })),
        ("ne", make_builtin(|args| {
            check_args_min("ne", args, 2)?;
            args[0].compare(&args[1], CompareOp::Ne)
        })),
        ("lt", make_builtin(|args| {
            check_args_min("lt", args, 2)?;
            args[0].compare(&args[1], CompareOp::Lt)
        })),
        ("le", make_builtin(|args| {
            check_args_min("le", args, 2)?;
            args[0].compare(&args[1], CompareOp::Le)
        })),
        ("gt", make_builtin(|args| {
            check_args_min("gt", args, 2)?;
            args[0].compare(&args[1], CompareOp::Gt)
        })),
        ("ge", make_builtin(|args| {
            check_args_min("ge", args, 2)?;
            args[0].compare(&args[1], CompareOp::Ge)
        })),
        ("abs", make_builtin(|args| {
            check_args_min("abs", args, 1)?;
            check_args("abs", args, 1)?;
            args[0].py_abs()
        })),
        ("contains", make_builtin(|args| {
            check_args_min("contains", args, 2)?;
            Ok(PyObject::bool_val(args[0].contains(&args[1])?))
        })),
        ("getitem", make_builtin(|args| {
            check_args_min("getitem", args, 2)?;
            match &args[0].payload {
                PyObjectPayload::List(items) => {
                    let idx = args[1].to_int()? as usize;
                    items.read().get(idx).cloned()
                        .ok_or_else(|| PyException::index_error("list index out of range"))
                }
                PyObjectPayload::Dict(map) => {
                    let key = args[1].to_hashable_key()?;
                    map.read().get(&key).cloned()
                        .ok_or_else(|| PyException::key_error(args[1].repr()))
                }
                PyObjectPayload::Tuple(items) => {
                    let idx = args[1].to_int()? as usize;
                    items.get(idx).cloned()
                        .ok_or_else(|| PyException::index_error("tuple index out of range"))
                }
                _ => Err(PyException::type_error("object is not subscriptable")),
            }
        })),
        ("itemgetter", make_builtin(|args| {
            check_args_min("itemgetter", args, 1)?;
            let keys: Vec<PyObjectRef> = args.to_vec();
            Ok(PyObject::native_closure("operator.itemgetter", move |call_args| {
                if call_args.is_empty() {
                    return Err(PyException::type_error("itemgetter expected 1 argument, got 0"));
                }
                let obj = &call_args[0];
                if keys.len() == 1 {
                    obj.get_item(&keys[0])
                } else {
                    let items: Vec<PyObjectRef> = keys.iter()
                        .map(|k| obj.get_item(k))
                        .collect::<PyResult<Vec<_>>>()?;
                    Ok(PyObject::tuple(items))
                }
            }))
        })),
        ("attrgetter", make_builtin(|args| {
            check_args_min("attrgetter", args, 1)?;
            let attr_names: Vec<String> = args.iter().map(|a| a.py_to_string()).collect();
            Ok(PyObject::native_closure("operator.attrgetter", move |call_args| {
                if call_args.is_empty() {
                    return Err(PyException::type_error("attrgetter expected 1 argument, got 0"));
                }
                let obj = &call_args[0];
                if attr_names.len() == 1 {
                    obj.get_attr(&attr_names[0])
                        .ok_or_else(|| PyException::attribute_error(format!(
                            "'{}' object has no attribute '{}'", obj.type_name(), attr_names[0]
                        )))
                } else {
                    let items: Vec<PyObjectRef> = attr_names.iter()
                        .map(|name| obj.get_attr(name).ok_or_else(|| PyException::attribute_error(
                            format!("'{}' object has no attribute '{}'", obj.type_name(), name)
                        )))
                        .collect::<PyResult<Vec<_>>>()?;
                    Ok(PyObject::tuple(items))
                }
            }))
        })),
        ("and_", make_builtin(|args| {
            check_args_min("and_", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a & b))
        })),
        ("or_", make_builtin(|args| {
            check_args_min("or_", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a | b))
        })),
        ("xor", make_builtin(|args| {
            check_args_min("xor", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a ^ b))
        })),
        ("lshift", make_builtin(|args| {
            check_args_min("lshift", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a << b))
        })),
        ("rshift", make_builtin(|args| {
            check_args_min("rshift", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a >> b))
        })),
        ("invert", make_builtin(|args| {
            check_args_min("invert", args, 1)?;
            let a = args[0].to_int()?;
            Ok(PyObject::int(!a))
        })),
        ("inv", make_builtin(|args| {
            check_args_min("inv", args, 1)?;
            let a = args[0].to_int()?;
            Ok(PyObject::int(!a))
        })),
        ("truth", make_builtin(|args| {
            check_args_min("truth", args, 1)?;
            Ok(PyObject::bool_val(args[0].is_truthy()))
        })),
        ("is_", make_builtin(|args| {
            check_args_min("is_", args, 2)?;
            Ok(PyObject::bool_val(std::sync::Arc::ptr_eq(&args[0], &args[1])))
        })),
        ("is_not", make_builtin(|args| {
            check_args_min("is_not", args, 2)?;
            Ok(PyObject::bool_val(!std::sync::Arc::ptr_eq(&args[0], &args[1])))
        })),
        ("index", make_builtin(|args| {
            check_args_min("index", args, 1)?;
            args[0].to_int().map(PyObject::int)
        })),
        ("setitem", make_builtin(|args| {
            check_args_min("setitem", args, 3)?;
            match &args[0].payload {
                PyObjectPayload::List(items) => {
                    let idx = args[1].to_int()? as usize;
                    let mut w = items.write();
                    if idx < w.len() {
                        w[idx] = args[2].clone();
                        Ok(PyObject::none())
                    } else {
                        Err(PyException::index_error("list assignment index out of range"))
                    }
                }
                PyObjectPayload::Dict(map) => {
                    let key = args[1].to_hashable_key()?;
                    map.write().insert(key, args[2].clone());
                    Ok(PyObject::none())
                }
                _ => Err(PyException::type_error("object does not support item assignment")),
            }
        })),
        ("delitem", make_builtin(|args| {
            check_args_min("delitem", args, 2)?;
            match &args[0].payload {
                PyObjectPayload::Dict(map) => {
                    let key = args[1].to_hashable_key()?;
                    map.write().shift_remove(&key);
                    Ok(PyObject::none())
                }
                _ => Err(PyException::type_error("object does not support item deletion")),
            }
        })),
        ("concat", make_builtin(|args| {
            check_args_min("concat", args, 2)?;
            args[0].add(&args[1])
        })),
        ("iadd", make_builtin(|args| {
            check_args_min("iadd", args, 2)?;
            args[0].add(&args[1])
        })),
        ("methodcaller", make_builtin(|args| {
            check_args_min("methodcaller", args, 1)?;
            let method_name = args[0].py_to_string();
            let extra_args: Vec<PyObjectRef> = if args.len() > 1 { args[1..].to_vec() } else { vec![] };
            Ok(PyObject::native_closure("operator.methodcaller", move |call_args| {
                if call_args.is_empty() {
                    return Err(PyException::type_error("methodcaller expected 1 argument, got 0"));
                }
                let obj = &call_args[0];
                let method = obj.get_attr(&method_name)
                    .ok_or_else(|| PyException::attribute_error(format!(
                        "'{}' object has no attribute '{}'", obj.type_name(), method_name
                    )))?;
                // We can't call through VM from native closure, so just return the method bound
                match &method.payload {
                    PyObjectPayload::NativeFunction { func, .. } => func(&extra_args),
                    PyObjectPayload::NativeClosure { func, .. } => func(&extra_args),
                    _ => Ok(method),
                }
            }))
        })),
        ("length_hint", make_builtin(|args| {
            check_args_min("length_hint", args, 1)?;
            let default = if args.len() > 1 { args[1].to_int().unwrap_or(0) } else { 0 };
            // Try __length_hint__ first
            if let Some(method) = args[0].get_attr("__length_hint__") {
                match &method.payload {
                    PyObjectPayload::NativeFunction { func, .. } => {
                        if let Ok(result) = func(&[args[0].clone()]) {
                            if let Ok(n) = result.to_int() {
                                return Ok(PyObject::int(n));
                            }
                        }
                    }
                    _ => {}
                }
            }
            // Try __len__
            if let Some(method) = args[0].get_attr("__len__") {
                match &method.payload {
                    PyObjectPayload::NativeFunction { func, .. } => {
                        if let Ok(result) = func(&[args[0].clone()]) {
                            if let Ok(n) = result.to_int() {
                                return Ok(PyObject::int(n));
                            }
                        }
                    }
                    _ => {}
                }
            }
            // Try len() directly
            match args[0].py_len() {
                Ok(n) => Ok(PyObject::int(n as i64)),
                Err(_) => Ok(PyObject::int(default)),
            }
        })),
    ])
}

// ── typing module (stub) ──


pub fn create_hashlib_module() -> PyObjectRef {
    make_module("hashlib", vec![
        ("md5", make_builtin(hashlib_md5)),
        ("sha1", make_builtin(hashlib_sha1)),
        ("sha256", make_builtin(hashlib_sha256)),
        ("sha512", make_builtin(hashlib_sha512)),
        ("sha224", make_builtin(hashlib_sha224)),
        ("sha384", make_builtin(hashlib_sha384)),
        ("new", make_builtin(hashlib_new)),
    ])
}

fn make_hash_object(name: &str, data: Vec<u8>, digest_hex: String, digest_bytes: Vec<u8>, block_size: i64, digest_size: i64) -> PyObjectRef {
    let class = PyObject::class(CompactString::from(name), vec![], IndexMap::new());
    let attrs = IndexMap::new();
    let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class: class.clone(),
        attrs: Arc::new(RwLock::new(attrs)),
        dict_storage: None,
    }));
    {
        let a = if let PyObjectPayload::Instance(ref d) = inst.payload { d.attrs.clone() } else { unreachable!() };
        let mut w = a.write();
        w.insert(CompactString::from("_hexdigest"), PyObject::str_val(CompactString::from(&digest_hex)));
        w.insert(CompactString::from("_digest"), PyObject::bytes(digest_bytes));
        w.insert(CompactString::from("_data"), PyObject::bytes(data));
        w.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(name)));
        w.insert(CompactString::from("block_size"), PyObject::int(block_size));
        w.insert(CompactString::from("digest_size"), PyObject::int(digest_size));
    }
    inst
}

fn hashlib_md5(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use md5::Md5;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Md5::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("md5", data, hex, result.to_vec(), 64, 16))
}

fn hashlib_sha1(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha1::Sha1;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha1::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha1", data, hex, result.to_vec(), 64, 20))
}

fn hashlib_sha256(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha2::Sha256;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha256", data, hex, result.to_vec(), 64, 32))
}

fn hashlib_sha224(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha2::Sha224;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha224::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha224", data, hex, result.to_vec(), 64, 28))
}

fn hashlib_sha384(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha2::Sha384;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha384::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha384", data, hex, result.to_vec(), 128, 48))
}

fn hashlib_sha512(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha2::Sha512;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha512::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha512", data, hex, result.to_vec(), 128, 64))
}

fn hashlib_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("hashlib.new() requires algorithm name")); }
    let name = match &args[0].payload {
        PyObjectPayload::Str(s) => s.to_string(),
        _ => return Err(PyException::type_error("algorithm name must be a string")),
    };
    let data_args = if args.len() > 1 { &args[1..] } else { &[] as &[PyObjectRef] };
    match name.as_str() {
        "md5" => hashlib_md5(data_args),
        "sha1" => hashlib_sha1(data_args),
        "sha256" => hashlib_sha256(data_args),
        "sha224" => hashlib_sha224(data_args),
        "sha384" => hashlib_sha384(data_args),
        "sha512" => hashlib_sha512(data_args),
        _ => Err(PyException::value_error(format!("unsupported hash type {}", name))),
    }
}

// ── copy module ──


pub fn create_logging_module() -> PyObjectRef {
    // Logging levels
    let debug_level = PyObject::int(10);
    let info_level = PyObject::int(20);
    let warning_level = PyObject::int(30);
    let error_level = PyObject::int(40);
    let critical_level = PyObject::int(50);

    // StreamHandler class — creates handler instance with stream ref and format/emit
    let stream_handler_cls = PyObject::class(CompactString::from("StreamHandler"), vec![], IndexMap::new());
    let sh_cls = stream_handler_cls.clone();
    let stream_handler_fn = PyObject::native_closure("StreamHandler", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(sh_cls.clone());
        let stream = if args.is_empty() { PyObject::none() } else { args[0].clone() };
        // Shared state for formatter and level
        let formatter_ref: Arc<RwLock<PyObjectRef>> = Arc::new(RwLock::new(PyObject::none()));
        let level_ref: Arc<RwLock<i64>> = Arc::new(RwLock::new(0));

        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(CompactString::from("stream"), stream.clone());
            attrs.insert(CompactString::from("level"), PyObject::int(0));
            attrs.insert(CompactString::from("formatter"), PyObject::none());

            let lr = level_ref.clone();
            attrs.insert(CompactString::from("setLevel"), PyObject::native_closure(
                "setLevel", move |args: &[PyObjectRef]| {
                    if let Some(v) = args.first() {
                        if let Some(n) = v.as_int() { *lr.write() = n; }
                    }
                    Ok(PyObject::none())
                }
            ));
            let fr = formatter_ref.clone();
            attrs.insert(CompactString::from("setFormatter"), PyObject::native_closure(
                "setFormatter", move |args: &[PyObjectRef]| {
                    if let Some(v) = args.first() {
                        *fr.write() = v.clone();
                    }
                    Ok(PyObject::none())
                }
            ));
            // emit(record) — write formatted message to stream or stderr
            let fr2 = formatter_ref.clone();
            let stream2 = stream.clone();
            attrs.insert(CompactString::from("emit"), PyObject::native_closure(
                "emit", move |args: &[PyObjectRef]| {
                    // args[0] may be handler (from logger dispatch) or record (direct call)
                    // Detect: if called with 2 args, args[0]=handler, args[1]=record
                    // If called with 1 arg, args[0]=record
                    let record = if args.len() >= 2 { &args[1] } else if !args.is_empty() { &args[0] } else {
                        return Ok(PyObject::none());
                    };

                    let msg = if let Some(m) = record.get_attr("message") {
                        m.py_to_string()
                    } else if let Some(m) = record.get_attr("msg") {
                        m.py_to_string()
                    } else {
                        record.py_to_string()
                    };

                    // Apply formatter if set
                    let fmt = fr2.read().clone();
                    let formatted = if !matches!(&fmt.payload, PyObjectPayload::None) {
                        if let Some(fmt_str) = fmt.get_attr("_fmt") {
                            let fs = fmt_str.py_to_string();
                            let mut result = fs.clone();
                            result = result.replace("%(message)s", &msg);
                            let levelname = if let Some(ln) = record.get_attr("levelname") {
                                ln.py_to_string()
                            } else { "INFO".to_string() };
                            let name = if let Some(n) = record.get_attr("name") {
                                n.py_to_string()
                            } else { "root".to_string() };
                            result = result.replace("%(levelname)s", &levelname);
                            result = result.replace("%(name)s", &name);
                            result
                        } else { msg.clone() }
                    } else { msg.clone() };

                    // Write to stream (directly to StringIO buffer)
                    if let PyObjectPayload::Instance(ref si) = stream2.payload {
                        let attrs_r = si.attrs.read();
                        if attrs_r.contains_key("__stringio__") {
                            drop(attrs_r);
                            let mut attrs_w = si.attrs.write();
                            let line = format!("{}\n", formatted);
                            if let Some(buf) = attrs_w.get("_buffer") {
                                let cur = buf.py_to_string();
                                attrs_w.insert(
                                    CompactString::from("_buffer"),
                                    PyObject::str_val(CompactString::from(format!("{}{}", cur, line))),
                                );
                            }
                            return Ok(PyObject::none());
                        }
                    }
                    eprintln!("{}", formatted);
                    Ok(PyObject::none())
                }
            ));
        }
        Ok(inst)
    });

    // FileHandler class — handler that writes to file
    let file_handler_cls = PyObject::class(CompactString::from("FileHandler"), vec![], IndexMap::new());
    let fh_cls = file_handler_cls.clone();
    let file_handler_fn = PyObject::native_closure("FileHandler", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(fh_cls.clone());
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let filename = if args.is_empty() {
                CompactString::from("")
            } else {
                CompactString::from(args[0].py_to_string())
            };
            attrs.insert(CompactString::from("baseFilename"), PyObject::str_val(filename));
            attrs.insert(CompactString::from("level"), PyObject::int(0));
            attrs.insert(CompactString::from("formatter"), PyObject::none());
            attrs.insert(CompactString::from("setLevel"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() >= 2 {
                    if let PyObjectPayload::Instance(ref d) = args[0].payload {
                        d.attrs.write().insert(CompactString::from("level"), args[1].clone());
                    }
                }
                Ok(PyObject::none())
            }));
            attrs.insert(CompactString::from("setFormatter"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() >= 2 {
                    if let PyObjectPayload::Instance(ref d) = args[0].payload {
                        d.attrs.write().insert(CompactString::from("formatter"), args[1].clone());
                    }
                }
                Ok(PyObject::none())
            }));
        }
        Ok(inst)
    });

    // Formatter(fmt) — stores format string, has format(record) method
    let formatter_cls = PyObject::class(CompactString::from("Formatter"), vec![], IndexMap::new());
    let fmt_cls = formatter_cls.clone();
    let formatter_fn = PyObject::native_closure("Formatter", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(fmt_cls.clone());
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let fmt_str = if args.is_empty() {
                CompactString::from("%(levelname)s:%(name)s:%(message)s")
            } else {
                CompactString::from(args[0].py_to_string())
            };
            attrs.insert(CompactString::from("_fmt"), PyObject::str_val(fmt_str));
            attrs.insert(CompactString::from("format"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() >= 2 {
                    Ok(PyObject::str_val(CompactString::from(args[1].py_to_string())))
                } else {
                    Ok(PyObject::str_val(CompactString::from("")))
                }
            }));
        }
        Ok(inst)
    });

    // Handler base class
    let handler_cls = PyObject::class(CompactString::from("Handler"), vec![], IndexMap::new());
    let h_cls = handler_cls.clone();
    let handler_fn = PyObject::native_closure("Handler", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(h_cls.clone());
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(CompactString::from("level"), PyObject::int(0));
            attrs.insert(CompactString::from("setLevel"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("setFormatter"), make_builtin(|_| Ok(PyObject::none())));
        }
        Ok(inst)
    });

    // basicConfig(**kwargs) — configure root logger
    let basic_config_fn = make_builtin(|args: &[PyObjectRef]| {
        // Accept kwargs as last dict arg from VM
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw_map) = &last.payload {
                let r = kw_map.read();
                // Extract level if present
                if let Some(_level) = r.get(&HashableKey::Str(CompactString::from("level"))) {
                    // In a real impl, would set root logger level
                }
                // Extract format if present
                if let Some(_format) = r.get(&HashableKey::Str(CompactString::from("format"))) {
                    // Would set root logger format
                }
            }
        }
        Ok(PyObject::none())
    });

    make_module("logging", vec![
        ("DEBUG", debug_level),
        ("INFO", info_level),
        ("WARNING", warning_level.clone()),
        ("ERROR", error_level),
        ("CRITICAL", critical_level),
        ("NOTSET", PyObject::int(0)),
        ("basicConfig", basic_config_fn),
        ("getLogger", make_builtin(logging_get_logger)),
        ("debug", make_builtin(|args| { logging_log(10, args) })),
        ("info", make_builtin(|args| { logging_log(20, args) })),
        ("warning", make_builtin(|args| { logging_log(30, args) })),
        ("error", make_builtin(|args| { logging_log(40, args) })),
        ("critical", make_builtin(|args| { logging_log(50, args) })),
        ("log", make_builtin(|args| {
            if args.len() >= 2 {
                let level = args[0].as_int().unwrap_or(20);
                logging_log(level, &args[1..])
            } else {
                Ok(PyObject::none())
            }
        })),
        ("StreamHandler", stream_handler_fn),
        ("FileHandler", file_handler_fn),
        ("Formatter", formatter_fn),
        ("Handler", handler_fn),
        ("Logger", make_builtin(logging_get_logger)),
    ])
}

fn logging_log(level: i64, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Ok(PyObject::none()); }
    let level_name = match level {
        10 => "DEBUG",
        20 => "INFO",
        30 => "WARNING",
        40 => "ERROR",
        50 => "CRITICAL",
        _ => "UNKNOWN",
    };
    let msg = args[0].py_to_string();
    eprintln!("{}:root:{}", level_name, msg);
    Ok(PyObject::none())
}

fn logging_get_logger(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let logger_name = if args.is_empty() || matches!(&args[0].payload, PyObjectPayload::None) {
        CompactString::from("root")
    } else {
        CompactString::from(args[0].py_to_string())
    };
    let mut ns = IndexMap::new();
    ns.insert(CompactString::from("name"), PyObject::str_val(logger_name.clone()));
    ns.insert(CompactString::from("level"), PyObject::int(30)); // WARNING default
    let handlers_list = PyObject::list(vec![]);
    ns.insert(CompactString::from("handlers"), handlers_list.clone());

    // Create log methods that capture the shared handlers list
    let make_log_method = |level: i64, level_name: &'static str, handlers: PyObjectRef, name: CompactString| -> PyObjectRef {
        PyObject::native_closure(level_name, move |args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::none()); }
            let msg = args[0].py_to_string();

            // Create a LogRecord-like instance
            let rec_cls = PyObject::class(CompactString::from("LogRecord"), vec![], IndexMap::new());
            let record = PyObject::instance(rec_cls);
            if let PyObjectPayload::Instance(ref rd) = record.payload {
                let mut ra = rd.attrs.write();
                ra.insert(CompactString::from("levelname"), PyObject::str_val(CompactString::from(level_name)));
                ra.insert(CompactString::from("levelno"), PyObject::int(level));
                ra.insert(CompactString::from("name"), PyObject::str_val(name.clone()));
                ra.insert(CompactString::from("message"), PyObject::str_val(CompactString::from(msg.clone())));
                ra.insert(CompactString::from("msg"), PyObject::str_val(CompactString::from(msg.clone())));
            }

            // Dispatch to handlers via shared list
            let mut dispatched = false;
            if let PyObjectPayload::List(items) = &handlers.payload {
                let items_r = items.read();
                for handler in items_r.iter() {
                    if let Some(emit_fn) = handler.get_attr("emit") {
                        match &emit_fn.payload {
                            PyObjectPayload::NativeFunction { func, .. } => {
                                let _ = func(&[handler.clone(), record.clone()]);
                                dispatched = true;
                            }
                            PyObjectPayload::NativeClosure { func, .. } => {
                                let _ = func(&[handler.clone(), record.clone()]);
                                dispatched = true;
                            }
                            _ => {}
                        }
                    }
                }
            }
            if !dispatched {
                eprintln!("{}:{}:{}", level_name, name, msg);
            }
            Ok(PyObject::none())
        })
    };

    ns.insert(CompactString::from("debug"), make_log_method(10, "DEBUG", handlers_list.clone(), logger_name.clone()));
    ns.insert(CompactString::from("info"), make_log_method(20, "INFO", handlers_list.clone(), logger_name.clone()));
    ns.insert(CompactString::from("warning"), make_log_method(30, "WARNING", handlers_list.clone(), logger_name.clone()));
    ns.insert(CompactString::from("error"), make_log_method(40, "ERROR", handlers_list.clone(), logger_name.clone()));
    ns.insert(CompactString::from("critical"), make_log_method(50, "CRITICAL", handlers_list.clone(), logger_name.clone()));

    ns.insert(CompactString::from("setLevel"), make_builtin(|_| Ok(PyObject::none())));
    // addHandler — push to shared handlers list
    let hl = handlers_list.clone();
    ns.insert(CompactString::from("addHandler"), PyObject::native_closure(
        "addHandler", move |args: &[PyObjectRef]| {
            if !args.is_empty() {
                if let PyObjectPayload::List(items) = &hl.payload {
                    items.write().push(args[0].clone());
                }
            }
            Ok(PyObject::none())
        }
    ));
    ns.insert(CompactString::from("removeHandler"), make_builtin(|_| Ok(PyObject::none())));
    let hl2 = handlers_list.clone();
    ns.insert(CompactString::from("hasHandlers"), PyObject::native_closure(
        "hasHandlers", move |_: &[PyObjectRef]| {
            if let PyObjectPayload::List(items) = &hl2.payload {
                return Ok(PyObject::bool_val(!items.read().is_empty()));
            }
            Ok(PyObject::bool_val(false))
        }
    ));
    ns.insert(CompactString::from("isEnabledFor"), make_builtin(|_| Ok(PyObject::bool_val(true))));
    ns.insert(CompactString::from("getEffectiveLevel"), make_builtin(|_| Ok(PyObject::int(30))));
    
    let cls = PyObject::class(CompactString::from("Logger"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        for (k, v) in ns {
            attrs.insert(k, v);
        }
    }
    Ok(inst)
}

// ── subprocess module (basic) ──



// ── csv module (basic) ──


pub fn create_unittest_module() -> PyObjectRef {
    // Create TestCase class
    let mut tc_ns = IndexMap::new();
    tc_ns.insert(CompactString::from("__unittest_testcase__"), PyObject::bool_val(true));
    let test_case = PyObject::class(CompactString::from("TestCase"), vec![], tc_ns);

    make_module("unittest", vec![
        ("TestCase", test_case),
        ("main", make_builtin(|_| Ok(PyObject::none()))),
        ("TestSuite", make_builtin(|_| Ok(PyObject::none()))),
        ("TestLoader", make_builtin(|_| Ok(PyObject::none()))),
        ("TextTestRunner", make_builtin(|_| Ok(PyObject::none()))),
        ("skip", make_builtin(|_args| {
            // Return identity decorator
            Ok(make_builtin(|args| {
                if args.is_empty() { Ok(PyObject::none()) } else { Ok(args[0].clone()) }
            }))
        })),
        ("skipIf", make_builtin(|_| {
            Ok(make_builtin(|args| {
                if args.is_empty() { Ok(PyObject::none()) } else { Ok(args[0].clone()) }
            }))
        })),
        ("expectedFailure", make_builtin(|args| {
            if args.is_empty() { Ok(PyObject::none()) } else { Ok(args[0].clone()) }
        })),
    ])
}

// ── threading module (basic) ──


pub fn create_pprint_module() -> PyObjectRef {
    make_module("pprint", vec![
        ("pprint", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::none()); }
            println!("{}", args[0].py_to_string());
            Ok(PyObject::none())
        })),
        ("pformat", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
            Ok(PyObject::str_val(CompactString::from(args[0].py_to_string())))
        })),
        ("PrettyPrinter", make_builtin(|_| Ok(PyObject::none()))),
    ])
}

// ── argparse module (basic) ──


pub fn create_argparse_module() -> PyObjectRef {
    // ArgumentParser class — functional constructor
    let ap_cls = PyObject::class(CompactString::from("ArgumentParser"), vec![], IndexMap::new());
    let apc = ap_cls.clone();
    let argument_parser_fn = PyObject::native_closure("ArgumentParser", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(apc.clone());
        // Shared argument storage between add_argument and parse_args
        let arg_defs: Arc<RwLock<Vec<(Vec<String>, IndexMap<CompactString, PyObjectRef>)>>> =
            Arc::new(RwLock::new(Vec::new()));

        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            // Store description, prog from kwargs
            let mut description = CompactString::from("");
            let mut prog = CompactString::from("");
            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(kw_map) = &last.payload {
                    let r = kw_map.read();
                    if let Some(d) = r.get(&HashableKey::Str(CompactString::from("description"))) {
                        description = CompactString::from(d.py_to_string());
                    }
                    if let Some(p) = r.get(&HashableKey::Str(CompactString::from("prog"))) {
                        prog = CompactString::from(p.py_to_string());
                    }
                }
            }
            attrs.insert(CompactString::from("description"), PyObject::str_val(description));
            attrs.insert(CompactString::from("prog"), PyObject::str_val(prog));

            // add_argument(*name_or_flags, **kwargs) — closure captures shared arg_defs
            let ad = arg_defs.clone();
            attrs.insert(CompactString::from("add_argument"), PyObject::native_closure(
                "add_argument", move |args: &[PyObjectRef]| {
                    let mut names: Vec<String> = Vec::new();
                    let mut kwargs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
                    for arg in args {
                        match &arg.payload {
                            PyObjectPayload::Str(s) => { names.push(s.to_string()); }
                            PyObjectPayload::Dict(kw_map) => {
                                let r = kw_map.read();
                                for (k, v) in r.iter() {
                                    if let HashableKey::Str(ks) = k {
                                        kwargs.insert(ks.clone(), v.clone());
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    ad.write().push((names, kwargs));
                    Ok(PyObject::none())
                }
            ));

            // parse_args(args=None) — closure captures shared arg_defs
            let pa = arg_defs.clone();
            attrs.insert(CompactString::from("parse_args"), PyObject::native_closure(
                "parse_args", move |_args: &[PyObjectRef]| {
                    let ns_cls = PyObject::class(CompactString::from("Namespace"), vec![], IndexMap::new());
                    let ns_inst = PyObject::instance(ns_cls);
                    // Set defaults from stored argument definitions
                    let defs = pa.read();
                    for (names, kwargs) in defs.iter() {
                        let dest = if let Some(d) = kwargs.get("dest") {
                            d.py_to_string()
                        } else {
                            // Prefer long option names (--verbose) over short (-v)
                            let long = names.iter().find(|n| n.starts_with("--"));
                            let chosen = long.or(names.first());
                            if let Some(n) = chosen {
                                n.trim_start_matches('-').replace('-', "_")
                            } else { continue; }
                        };
                        let default = kwargs.get("default").cloned().unwrap_or_else(PyObject::none);
                        if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                            nd.attrs.write().insert(CompactString::from(dest.as_str()), default);
                        }
                    }
                    Ok(ns_inst)
                }
            ));

            attrs.insert(CompactString::from("add_subparsers"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("add_mutually_exclusive_group"), make_builtin(|_| Ok(PyObject::none())));
        }
        Ok(inst)
    });

    // Namespace class
    let ns_cls = PyObject::class(CompactString::from("Namespace"), vec![], IndexMap::new());
    let nsc = ns_cls.clone();
    let namespace_fn = PyObject::native_closure("Namespace", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(nsc.clone());
        // Accept kwargs
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw_map) = &last.payload {
                if let PyObjectPayload::Instance(ref id) = inst.payload {
                    let mut attrs = id.attrs.write();
                    let r = kw_map.read();
                    for (k, v) in r.iter() {
                        if let HashableKey::Str(ks) = k {
                            attrs.insert(ks.clone(), v.clone());
                        }
                    }
                }
            }
        }
        Ok(inst)
    });

    make_module("argparse", vec![
        ("ArgumentParser", argument_parser_fn),
        ("Namespace", namespace_fn),
        ("Action", make_builtin(|_| Ok(PyObject::none()))),
        ("HelpFormatter", make_builtin(|_| Ok(PyObject::none()))),
        ("RawDescriptionHelpFormatter", make_builtin(|_| Ok(PyObject::none()))),
    ])
}


// ── errno module ──

pub fn create_errno_module() -> PyObjectRef {
    make_module("errno", vec![
        ("EPERM", PyObject::int(1)),
        ("ENOENT", PyObject::int(2)),
        ("ESRCH", PyObject::int(3)),
        ("EINTR", PyObject::int(4)),
        ("EIO", PyObject::int(5)),
        ("ENXIO", PyObject::int(6)),
        ("E2BIG", PyObject::int(7)),
        ("ENOEXEC", PyObject::int(8)),
        ("EBADF", PyObject::int(9)),
        ("ECHILD", PyObject::int(10)),
        ("EAGAIN", PyObject::int(11)),
        ("ENOMEM", PyObject::int(12)),
        ("EACCES", PyObject::int(13)),
        ("EFAULT", PyObject::int(14)),
        ("EBUSY", PyObject::int(16)),
        ("EEXIST", PyObject::int(17)),
        ("EXDEV", PyObject::int(18)),
        ("ENODEV", PyObject::int(19)),
        ("ENOTDIR", PyObject::int(20)),
        ("EISDIR", PyObject::int(21)),
        ("EINVAL", PyObject::int(22)),
        ("ENFILE", PyObject::int(23)),
        ("EMFILE", PyObject::int(24)),
        ("ENOTTY", PyObject::int(25)),
        ("EFBIG", PyObject::int(27)),
        ("ENOSPC", PyObject::int(28)),
        ("ESPIPE", PyObject::int(29)),
        ("EROFS", PyObject::int(30)),
        ("EMLINK", PyObject::int(31)),
        ("EPIPE", PyObject::int(32)),
        ("EDOM", PyObject::int(33)),
        ("ERANGE", PyObject::int(34)),
        ("EDEADLK", PyObject::int(35)),
        ("ENAMETOOLONG", PyObject::int(36)),
        ("ENOLCK", PyObject::int(37)),
        ("ENOSYS", PyObject::int(38)),
        ("ENOTEMPTY", PyObject::int(39)),
        ("ECONNREFUSED", PyObject::int(111)),
        ("ETIMEDOUT", PyObject::int(110)),
        ("errorcode", make_builtin(|_| {
            let mut map = IndexMap::new();
            let codes: Vec<(i64, &str)> = vec![
                (1, "EPERM"), (2, "ENOENT"), (13, "EACCES"), (17, "EEXIST"),
                (22, "EINVAL"), (32, "EPIPE"), (110, "ETIMEDOUT"), (111, "ECONNREFUSED"),
            ];
            for (num, name) in codes {
                map.insert(HashableKey::Int(PyInt::Small(num)), PyObject::str_val(CompactString::from(name)));
            }
            Ok(PyObject::dict(map))
        })),
    ])
}

// ── uuid module ────────────────────────────────────────────────────
pub fn create_uuid_module() -> PyObjectRef {
    make_module("uuid", vec![
        ("uuid4", make_builtin(uuid_uuid4)),
        ("uuid1", make_builtin(uuid_uuid1)),
        ("UUID", make_builtin(uuid_UUID)),
        ("NAMESPACE_DNS", PyObject::str_val(CompactString::from("6ba7b810-9dad-11d1-80b4-00c04fd430c8"))),
        ("NAMESPACE_URL", PyObject::str_val(CompactString::from("6ba7b811-9dad-11d1-80b4-00c04fd430c8"))),
    ])
}

fn random_uuid_bytes() -> [u8; 16] {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as u64;
    // Simple xorshift-based PRNG for generating random bytes
    let mut state = seed ^ 0x517cc1b727220a95;
    let mut bytes = [0u8; 16];
    for chunk in bytes.chunks_mut(8) {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        for (i, b) in chunk.iter_mut().enumerate() {
            *b = ((state >> (i * 8)) & 0xFF) as u8;
        }
    }
    bytes
}

fn format_uuid(bytes: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

fn uuid_uuid4(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let _ = args;
    let mut bytes = random_uuid_bytes();
    // Set version 4 bits
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    // Set variant bits
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    let hex_str = format_uuid(&bytes);
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("hex"), PyObject::str_val(CompactString::from(hex_str.replace('-', ""))));
    attrs.insert(CompactString::from("version"), PyObject::int(4));
    attrs.insert(CompactString::from("int"), PyObject::int(
        u128::from_be_bytes(bytes.try_into().unwrap()) as i64
    ));
    attrs.insert(CompactString::from("__str_val__"), PyObject::str_val(CompactString::from(&hex_str)));
    attrs.insert(CompactString::from("__uuid__"), PyObject::bool_val(true));
    Ok(PyObject::instance_with_attrs(
        PyObject::str_val(CompactString::from("UUID")),
        attrs,
    ))
}

fn uuid_uuid1(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // uuid1 is time-based; use same approach as uuid4 for simplicity
    uuid_uuid4(args)
}

#[allow(non_snake_case)]
fn uuid_UUID(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("UUID", args, 1)?;
    let s = args[0].py_to_string();
    let hex_str = s.replace('-', "");
    if hex_str.len() != 32 || !hex_str.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(PyException::value_error(format!("badly formed hexadecimal UUID string: '{}'", s)));
    }
    // Parse the canonical form
    let canonical = format!("{}-{}-{}-{}-{}",
        &hex_str[0..8], &hex_str[8..12], &hex_str[12..16],
        &hex_str[16..20], &hex_str[20..32]);
    let version = u8::from_str_radix(&hex_str[12..13], 16).unwrap_or(0);
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("hex"), PyObject::str_val(CompactString::from(&hex_str)));
    attrs.insert(CompactString::from("version"), PyObject::int(version as i64));
    attrs.insert(CompactString::from("__str_val__"), PyObject::str_val(CompactString::from(&canonical)));
    attrs.insert(CompactString::from("__uuid__"), PyObject::bool_val(true));
    Ok(PyObject::instance_with_attrs(
        PyObject::str_val(CompactString::from("UUID")),
        attrs,
    ))
}

// ── codecs module ──────────────────────────────────────────────────
pub fn create_codecs_module() -> PyObjectRef {
    make_module("codecs", vec![
        ("encode", make_builtin(codecs_encode)),
        ("decode", make_builtin(codecs_decode)),
        ("lookup", make_builtin(codecs_lookup)),
        ("getencoder", make_builtin(codecs_getencoder)),
        ("getdecoder", make_builtin(codecs_getdecoder)),
        ("utf_8_encode", make_builtin(codecs_utf8_encode)),
        ("utf_8_decode", make_builtin(codecs_utf8_decode)),
    ])
}

fn codecs_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("codecs.encode", args, 1)?;
    let s = args[0].py_to_string();
    let encoding = if args.len() > 1 { args[1].py_to_string() } else { "utf-8".to_string() };
    match encoding.to_lowercase().replace('-', "_").as_str() {
        "utf_8" | "utf8" => Ok(PyObject::bytes(s.as_bytes().to_vec())),
        "ascii" => {
            let bytes: Vec<u8> = s.chars().filter_map(|c| if c.is_ascii() { Some(c as u8) } else { None }).collect();
            Ok(PyObject::bytes(bytes))
        }
        "latin_1" | "latin1" | "iso_8859_1" => {
            let bytes: Vec<u8> = s.chars().map(|c| c as u8).collect();
            Ok(PyObject::bytes(bytes))
        }
        _ => Err(PyException::value_error(format!("unknown encoding: {}", encoding))),
    }
}

fn codecs_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("codecs.decode", args, 1)?;
    let bytes = extract_bytes(&args[0])?;
    let encoding = if args.len() > 1 { args[1].py_to_string() } else { "utf-8".to_string() };
    match encoding.to_lowercase().replace('-', "_").as_str() {
        "utf_8" | "utf8" => {
            let s = String::from_utf8(bytes).map_err(|_| PyException::value_error("invalid utf-8"))?;
            Ok(PyObject::str_val(CompactString::from(s)))
        }
        "ascii" => {
            let s: String = bytes.iter().map(|&b| b as char).collect();
            Ok(PyObject::str_val(CompactString::from(s)))
        }
        "latin_1" | "latin1" | "iso_8859_1" => {
            let s: String = bytes.iter().map(|&b| b as char).collect();
            Ok(PyObject::str_val(CompactString::from(s)))
        }
        _ => Err(PyException::value_error(format!("unknown encoding: {}", encoding))),
    }
}

fn codecs_lookup(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("codecs.lookup", args, 1)?;
    let encoding = args[0].py_to_string().to_lowercase().replace('-', "_");
    match encoding.as_str() {
        "utf_8" | "utf8" | "ascii" | "latin_1" | "latin1" | "iso_8859_1" => {
            Ok(PyObject::tuple(vec![
                PyObject::str_val(CompactString::from(&encoding)),
                PyObject::none(), // encode
                PyObject::none(), // decode
                PyObject::none(), // stream reader
            ]))
        }
        _ => Err(PyException::value_error(format!("unknown encoding: {}", encoding))),
    }
}

fn codecs_getencoder(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("codecs.getencoder", args, 1)?;
    Ok(make_builtin(codecs_encode))
}

fn codecs_getdecoder(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("codecs.getdecoder", args, 1)?;
    Ok(make_builtin(codecs_decode))
}

fn codecs_utf8_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("codecs.utf_8_encode", args, 1)?;
    let s = args[0].py_to_string();
    let b = s.as_bytes().to_vec();
    let len = b.len() as i64;
    Ok(PyObject::tuple(vec![PyObject::bytes(b), PyObject::int(len)]))
}

fn codecs_utf8_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("codecs.utf_8_decode", args, 1)?;
    let bytes = extract_bytes(&args[0])?;
    let s = String::from_utf8(bytes.clone()).map_err(|_| PyException::value_error("invalid utf-8"))?;
    let len = bytes.len() as i64;
    Ok(PyObject::tuple(vec![PyObject::str_val(CompactString::from(s)), PyObject::int(len)]))
}

// ── _thread module ──

// ── secrets module ──────────────────────────────────────────────────

pub fn create_secrets_module() -> PyObjectRef {
    make_module("secrets", vec![
        ("token_bytes", make_builtin(secrets_token_bytes)),
        ("token_hex", make_builtin(secrets_token_hex)),
        ("token_urlsafe", make_builtin(secrets_token_urlsafe)),
        ("randbelow", make_builtin(secrets_randbelow)),
        ("choice", make_builtin(secrets_choice)),
        ("compare_digest", make_builtin(secrets_compare_digest)),
    ])
}

fn secrets_random_bytes(n: usize) -> Vec<u8> {
    use std::time::SystemTime;
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut result = Vec::with_capacity(n);
    for _ in 0..n {
        let cnt = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos() as u64;
        let seed = nanos
            .wrapping_mul(6364136223846793005)
            .wrapping_add(cnt.wrapping_mul(1442695040888963407));
        result.push((seed >> 16) as u8);
    }
    result
}

fn secrets_random_f64() -> f64 {
    use std::time::SystemTime;
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let cnt = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos() as u64;
    let seed = nanos
        .wrapping_mul(6364136223846793005)
        .wrapping_add(cnt.wrapping_mul(1442695040888963407));
    (seed >> 11) as f64 / (1u64 << 53) as f64
}

fn secrets_token_bytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let nbytes = if args.is_empty() { 32 } else { args[0].to_int()? as usize };
    Ok(PyObject::bytes(secrets_random_bytes(nbytes)))
}

fn secrets_token_hex(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let nbytes = if args.is_empty() { 32 } else { args[0].to_int()? as usize };
    let bytes = secrets_random_bytes(nbytes);
    let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    Ok(PyObject::str_val(CompactString::from(hex)))
}

fn secrets_token_urlsafe(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let nbytes = if args.is_empty() { 32 } else { args[0].to_int()? as usize };
    let bytes = secrets_random_bytes(nbytes);
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::with_capacity((nbytes * 4 + 2) / 3);
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i] as u32;
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if i + 1 < bytes.len() {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        }
        if i + 2 < bytes.len() {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        }
        i += 3;
    }
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn secrets_randbelow(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("secrets.randbelow", args, 1)?;
    let n = args[0].to_int()?;
    if n <= 0 {
        return Err(PyException::value_error("upper bound must be positive"));
    }
    let val = (secrets_random_f64() * n as f64) as i64;
    Ok(PyObject::int(val.min(n - 1)))
}

fn secrets_choice(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("secrets.choice", args, 1)?;
    let items = args[0].to_list()?;
    if items.is_empty() {
        return Err(PyException::index_error("cannot choose from an empty sequence"));
    }
    let idx = (secrets_random_f64() * items.len() as f64) as usize;
    Ok(items[idx.min(items.len() - 1)].clone())
}

fn secrets_compare_digest(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("secrets.compare_digest", args, 2)?;
    let a = args[0].py_to_string();
    let b = args[1].py_to_string();
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let mut result: u8 = if a_bytes.len() != b_bytes.len() { 1 } else { 0 };
    let len = std::cmp::min(a_bytes.len(), b_bytes.len());
    for i in 0..len {
        result |= a_bytes[i] ^ b_bytes[i];
    }
    Ok(PyObject::bool_val(result == 0))
}


// ── hmac module ──────────────────────────────────────────────────────
pub fn create_hmac_module() -> PyObjectRef {
    use std::sync::Arc;
    use parking_lot::RwLock;

    fn hmac_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("hmac.new requires key and msg")); }
        let key = match &args[0].payload {
            PyObjectPayload::Bytes(b) => b.clone(),
            PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
            _ => return Err(PyException::type_error("key must be bytes")),
        };
        let msg = match &args[1].payload {
            PyObjectPayload::Bytes(b) => b.clone(),
            PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
            _ => vec![],
        };
        let digestmod = if args.len() > 2 { args[2].py_to_string() } else { "sha256".to_string() };

        // HMAC computation: H((K ^ opad) || H((K ^ ipad) || message))
        let block_size = 64usize;
        let mut k = key;
        if k.len() > block_size {
            k = simple_hash(&k, &digestmod);
        }
        while k.len() < block_size { k.push(0); }
        let ipad: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
        let opad: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
        let mut inner = ipad;
        inner.extend_from_slice(&msg);
        let inner_hash = simple_hash(&inner, &digestmod);
        let mut outer = opad;
        outer.extend_from_slice(&inner_hash);
        let result = simple_hash(&outer, &digestmod);

        let hex_str = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("_digest"), PyObject::bytes(result.clone()));
        attrs.insert(CompactString::from("_hexdigest"), PyObject::str_val(CompactString::from(&hex_str)));
        attrs.insert(CompactString::from("digest_size"), PyObject::int(result.len() as i64));
        attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(format!("hmac-{}", digestmod))));
        attrs.insert(CompactString::from("_digest_bytes"), PyObject::bytes(result));
        attrs.insert(CompactString::from("_hex_str"), PyObject::str_val(CompactString::from(&hex_str)));

        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("digest"), make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::bytes(vec![])); }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(v) = inst.attrs.read().get("_digest_bytes") { return Ok(v.clone()); }
            }
            Ok(PyObject::bytes(vec![]))
        }));
        ns.insert(CompactString::from("hexdigest"), make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(v) = inst.attrs.read().get("_hex_str") { return Ok(v.clone()); }
            }
            Ok(PyObject::str_val(CompactString::from("")))
        }));
        let class = PyObject::class(CompactString::from("HMAC"), vec![], ns);
        let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
            class,
            attrs: Arc::new(RwLock::new(attrs)),
            dict_storage: None,
        }));
        Ok(inst)
    }

    fn simple_hash(data: &[u8], algo: &str) -> Vec<u8> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        // Use Rust's built-in hasher as a simplified substitute
        // (Real HMAC would need proper SHA implementation)
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        algo.hash(&mut hasher);
        let h = hasher.finish();
        let mut result = Vec::new();
        for i in 0..4 {
            let mut hasher2 = DefaultHasher::new();
            data.hash(&mut hasher2);
            (h.wrapping_add(i as u64)).hash(&mut hasher2);
            let v = hasher2.finish();
            result.extend_from_slice(&v.to_be_bytes());
        }
        result
    }

    fn hmac_compare_digest(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("compare_digest requires 2 arguments")); }
        let a = args[0].py_to_string();
        let b = args[1].py_to_string();
        let a_bytes = a.as_bytes();
        let b_bytes = b.as_bytes();
        if a_bytes.len() != b_bytes.len() { return Ok(PyObject::bool_val(false)); }
        let mut result = 0u8;
        for i in 0..a_bytes.len() { result |= a_bytes[i] ^ b_bytes[i]; }
        Ok(PyObject::bool_val(result == 0))
    }

    make_module("hmac", vec![
        ("new", make_builtin(hmac_new)),
        ("compare_digest", make_builtin(hmac_compare_digest)),
        ("digest", make_builtin(|args| hmac_new(args).and_then(|h| {
            h.get_attr("_digest").ok_or_else(|| PyException::runtime_error("no digest"))
        }))),
        ("HMAC", make_builtin(hmac_new)),
    ])
}

// ── configparser module ──────────────────────────────────────────────
pub fn create_configparser_module() -> PyObjectRef {
    use std::sync::Arc;
    use parking_lot::RwLock;

    fn configparser_new(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("read"), make_builtin(cp_read));
        ns.insert(CompactString::from("read_string"), make_builtin(cp_read_string));
        ns.insert(CompactString::from("get"), make_builtin(cp_get));
        ns.insert(CompactString::from("getint"), make_builtin(cp_getint));
        ns.insert(CompactString::from("getfloat"), make_builtin(cp_getfloat));
        ns.insert(CompactString::from("getboolean"), make_builtin(cp_getboolean));
        ns.insert(CompactString::from("sections"), make_builtin(cp_sections));
        ns.insert(CompactString::from("has_section"), make_builtin(cp_has_section));
        ns.insert(CompactString::from("has_option"), make_builtin(cp_has_option));
        ns.insert(CompactString::from("options"), make_builtin(cp_options));
        ns.insert(CompactString::from("items"), make_builtin(cp_items));
        ns.insert(CompactString::from("set"), make_builtin(cp_set));
        let class = PyObject::class(CompactString::from("ConfigParser"), vec![], ns);
        let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
            class,
            attrs: Arc::new(RwLock::new(IndexMap::new())),
            dict_storage: None,
        }));
        // Store sections as a dict of dicts
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("__configparser__"), PyObject::bool_val(true));
            w.insert(CompactString::from("_sections"), PyObject::dict(IndexMap::new()));
        }
        Ok(inst)
    }

    fn get_sections(obj: &PyObjectRef) -> Option<Arc<RwLock<IndexMap<HashableKey, PyObjectRef>>>> {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            if let Some(sec) = inst.attrs.read().get("_sections") {
                if let PyObjectPayload::Dict(d) = &sec.payload {
                    return Some(d.clone());
                }
            }
        }
        None
    }

    fn parse_ini(content: &str) -> IndexMap<HashableKey, PyObjectRef> {
        let mut sections: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
        let mut current_section = CompactString::from("DEFAULT");
        let mut current_items: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') { continue; }
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                if !current_items.is_empty() {
                    sections.insert(HashableKey::Str(current_section.clone()), PyObject::dict(current_items.clone()));
                    current_items.clear();
                }
                current_section = CompactString::from(&trimmed[1..trimmed.len()-1]);
            } else if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim();
                let val = trimmed[eq_pos+1..].trim();
                current_items.insert(HashableKey::Str(CompactString::from(key)), PyObject::str_val(CompactString::from(val)));
            } else if let Some(eq_pos) = trimmed.find(':') {
                let key = trimmed[..eq_pos].trim();
                let val = trimmed[eq_pos+1..].trim();
                current_items.insert(HashableKey::Str(CompactString::from(key)), PyObject::str_val(CompactString::from(val)));
            }
        }
        if !current_items.is_empty() {
            sections.insert(HashableKey::Str(current_section), PyObject::dict(current_items));
        }
        sections
    }

    fn cp_read(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("read requires filename")); }
        let path = args[1].py_to_string();
        let content = std::fs::read_to_string(&path).map_err(|e|
            PyException::runtime_error(format!("Cannot read {}: {}", path, e)))?;
        if let Some(secs) = get_sections(&args[0]) {
            let parsed = parse_ini(&content);
            let mut w = secs.write();
            for (k, v) in parsed { w.insert(k, v); }
        }
        Ok(PyObject::none())
    }

    fn cp_read_string(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("read_string requires string")); }
        let content = args[1].py_to_string();
        if let Some(secs) = get_sections(&args[0]) {
            let parsed = parse_ini(&content);
            let mut w = secs.write();
            for (k, v) in parsed { w.insert(k, v); }
        }
        Ok(PyObject::none())
    }

    fn cp_get(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 3 { return Err(PyException::type_error("get requires section and option")); }
        let section = CompactString::from(args[1].py_to_string());
        let option = CompactString::from(args[2].py_to_string());
        if let Some(secs) = get_sections(&args[0]) {
            let r = secs.read();
            if let Some(sec_dict) = r.get(&HashableKey::Str(section.clone())) {
                if let PyObjectPayload::Dict(d) = &sec_dict.payload {
                    if let Some(val) = d.read().get(&HashableKey::Str(option.clone())) {
                        return Ok(val.clone());
                    }
                }
            }
        }
        if args.len() > 3 { return Ok(args[3].clone()); } // fallback
        Err(PyException::key_error(format!("No option '{}' in section", option)))
    }

    fn cp_getint(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let val = cp_get(args)?;
        let s = val.py_to_string();
        let n: i64 = s.parse().map_err(|_| PyException::value_error(format!("invalid int: {}", s)))?;
        Ok(PyObject::int(n))
    }

    fn cp_getfloat(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let val = cp_get(args)?;
        let s = val.py_to_string();
        let f: f64 = s.parse().map_err(|_| PyException::value_error(format!("invalid float: {}", s)))?;
        Ok(PyObject::float(f))
    }

    fn cp_getboolean(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let val = cp_get(args)?;
        let s = val.py_to_string().to_lowercase();
        Ok(PyObject::bool_val(matches!(s.as_str(), "1" | "yes" | "true" | "on")))
    }

    fn cp_sections(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() { return Ok(PyObject::list(vec![])); }
        if let Some(secs) = get_sections(&args[0]) {
            let keys: Vec<PyObjectRef> = secs.read().keys()
                .filter_map(|k| if let HashableKey::Str(s) = k { Some(PyObject::str_val(s.clone())) } else { None })
                .collect();
            return Ok(PyObject::list(keys));
        }
        Ok(PyObject::list(vec![]))
    }

    fn cp_has_section(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
        let section = CompactString::from(args[1].py_to_string());
        if let Some(secs) = get_sections(&args[0]) {
            return Ok(PyObject::bool_val(secs.read().contains_key(&HashableKey::Str(section))));
        }
        Ok(PyObject::bool_val(false))
    }

    fn cp_has_option(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 3 { return Ok(PyObject::bool_val(false)); }
        let section = CompactString::from(args[1].py_to_string());
        let option = CompactString::from(args[2].py_to_string());
        if let Some(secs) = get_sections(&args[0]) {
            let r = secs.read();
            if let Some(sec_dict) = r.get(&HashableKey::Str(section)) {
                if let PyObjectPayload::Dict(d) = &sec_dict.payload {
                    return Ok(PyObject::bool_val(d.read().contains_key(&HashableKey::Str(option))));
                }
            }
        }
        Ok(PyObject::bool_val(false))
    }

    fn cp_options(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Ok(PyObject::list(vec![])); }
        let section = CompactString::from(args[1].py_to_string());
        if let Some(secs) = get_sections(&args[0]) {
            let r = secs.read();
            if let Some(sec_dict) = r.get(&HashableKey::Str(section)) {
                if let PyObjectPayload::Dict(d) = &sec_dict.payload {
                    let keys: Vec<PyObjectRef> = d.read().keys()
                        .filter_map(|k| if let HashableKey::Str(s) = k { Some(PyObject::str_val(s.clone())) } else { None })
                        .collect();
                    return Ok(PyObject::list(keys));
                }
            }
        }
        Ok(PyObject::list(vec![]))
    }

    fn cp_items(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Ok(PyObject::list(vec![])); }
        let section = CompactString::from(args[1].py_to_string());
        if let Some(secs) = get_sections(&args[0]) {
            let r = secs.read();
            if let Some(sec_dict) = r.get(&HashableKey::Str(section)) {
                if let PyObjectPayload::Dict(d) = &sec_dict.payload {
                    let items: Vec<PyObjectRef> = d.read().iter()
                        .map(|(k, v)| {
                            let key = if let HashableKey::Str(s) = k { PyObject::str_val(s.clone()) } else { PyObject::none() };
                            PyObject::tuple(vec![key, v.clone()])
                        }).collect();
                    return Ok(PyObject::list(items));
                }
            }
        }
        Ok(PyObject::list(vec![]))
    }

    fn cp_set(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 4 { return Err(PyException::type_error("set requires section, option, value")); }
        let section = CompactString::from(args[1].py_to_string());
        let option = CompactString::from(args[2].py_to_string());
        let value = args[3].clone();
        if let Some(secs) = get_sections(&args[0]) {
            let mut w = secs.write();
            let sec_key = HashableKey::Str(section.clone());
            if !w.contains_key(&sec_key) {
                w.insert(sec_key.clone(), PyObject::dict(IndexMap::new()));
            }
            if let Some(sec_dict) = w.get(&sec_key) {
                if let PyObjectPayload::Dict(d) = &sec_dict.payload {
                    d.write().insert(HashableKey::Str(option), value);
                }
            }
        }
        Ok(PyObject::none())
    }

    make_module("configparser", vec![
        ("ConfigParser", make_builtin(configparser_new)),
        ("RawConfigParser", make_builtin(configparser_new)),
        ("SafeConfigParser", make_builtin(configparser_new)),
    ])
}

// ── __future__ module ──

pub fn create_future_module() -> PyObjectRef {
    make_module("__future__", vec![
        ("division", PyObject::bool_val(true)),
        ("absolute_import", PyObject::bool_val(true)),
        ("print_function", PyObject::bool_val(true)),
        ("unicode_literals", PyObject::bool_val(true)),
        ("generator_stop", PyObject::bool_val(true)),
        ("annotations", PyObject::bool_val(true)),
        ("CO_FUTURE_DIVISION", PyObject::int(0x20000)),
        ("CO_FUTURE_ABSOLUTE_IMPORT", PyObject::int(0x40000)),
        ("CO_FUTURE_PRINT_FUNCTION", PyObject::int(0x10000)),
        ("CO_FUTURE_UNICODE_LITERALS", PyObject::int(0x20000)),
        ("CO_FUTURE_GENERATOR_STOP", PyObject::int(0x80000)),
        ("CO_FUTURE_ANNOTATIONS", PyObject::int(0x100000)),
    ])
}

// ── builtins module ──

pub fn create_builtins_module() -> PyObjectRef {
    make_module("builtins", vec![
        ("__name__", PyObject::str_val(CompactString::from("builtins"))),
        ("__doc__", PyObject::str_val(CompactString::from("Built-in functions, exceptions, and other objects."))),
        ("print", PyObject::builtin_function(CompactString::from("print"))),
        ("len", PyObject::builtin_function(CompactString::from("len"))),
        ("range", PyObject::builtin_function(CompactString::from("range"))),
        ("int", PyObject::builtin_type(CompactString::from("int"))),
        ("float", PyObject::builtin_type(CompactString::from("float"))),
        ("str", PyObject::builtin_type(CompactString::from("str"))),
        ("bool", PyObject::builtin_type(CompactString::from("bool"))),
        ("list", PyObject::builtin_type(CompactString::from("list"))),
        ("tuple", PyObject::builtin_type(CompactString::from("tuple"))),
        ("dict", PyObject::builtin_type(CompactString::from("dict"))),
        ("set", PyObject::builtin_type(CompactString::from("set"))),
        ("frozenset", PyObject::builtin_type(CompactString::from("frozenset"))),
        ("bytes", PyObject::builtin_type(CompactString::from("bytes"))),
        ("bytearray", PyObject::builtin_type(CompactString::from("bytearray"))),
        ("type", PyObject::builtin_type(CompactString::from("type"))),
        ("object", PyObject::builtin_type(CompactString::from("object"))),
        ("complex", PyObject::builtin_type(CompactString::from("complex"))),
        ("super", PyObject::builtin_type(CompactString::from("super"))),
        ("property", PyObject::builtin_type(CompactString::from("property"))),
        ("classmethod", PyObject::builtin_type(CompactString::from("classmethod"))),
        ("staticmethod", PyObject::builtin_type(CompactString::from("staticmethod"))),
        ("abs", PyObject::builtin_function(CompactString::from("abs"))),
        ("all", PyObject::builtin_function(CompactString::from("all"))),
        ("any", PyObject::builtin_function(CompactString::from("any"))),
        ("ascii", PyObject::builtin_function(CompactString::from("ascii"))),
        ("bin", PyObject::builtin_function(CompactString::from("bin"))),
        ("callable", PyObject::builtin_function(CompactString::from("callable"))),
        ("chr", PyObject::builtin_function(CompactString::from("chr"))),
        ("dir", PyObject::builtin_function(CompactString::from("dir"))),
        ("divmod", PyObject::builtin_function(CompactString::from("divmod"))),
        ("enumerate", PyObject::builtin_function(CompactString::from("enumerate"))),
        ("eval", PyObject::builtin_function(CompactString::from("eval"))),
        ("exec", PyObject::builtin_function(CompactString::from("exec"))),
        ("filter", PyObject::builtin_function(CompactString::from("filter"))),
        ("format", PyObject::builtin_function(CompactString::from("format"))),
        ("getattr", PyObject::builtin_function(CompactString::from("getattr"))),
        ("globals", PyObject::builtin_function(CompactString::from("globals"))),
        ("hasattr", PyObject::builtin_function(CompactString::from("hasattr"))),
        ("hash", PyObject::builtin_function(CompactString::from("hash"))),
        ("hex", PyObject::builtin_function(CompactString::from("hex"))),
        ("id", PyObject::builtin_function(CompactString::from("id"))),
        ("input", PyObject::builtin_function(CompactString::from("input"))),
        ("isinstance", PyObject::builtin_function(CompactString::from("isinstance"))),
        ("issubclass", PyObject::builtin_function(CompactString::from("issubclass"))),
        ("iter", PyObject::builtin_function(CompactString::from("iter"))),
        ("locals", PyObject::builtin_function(CompactString::from("locals"))),
        ("map", PyObject::builtin_function(CompactString::from("map"))),
        ("max", PyObject::builtin_function(CompactString::from("max"))),
        ("min", PyObject::builtin_function(CompactString::from("min"))),
        ("next", PyObject::builtin_function(CompactString::from("next"))),
        ("oct", PyObject::builtin_function(CompactString::from("oct"))),
        ("open", PyObject::builtin_function(CompactString::from("open"))),
        ("ord", PyObject::builtin_function(CompactString::from("ord"))),
        ("pow", PyObject::builtin_function(CompactString::from("pow"))),
        ("repr", PyObject::builtin_function(CompactString::from("repr"))),
        ("reversed", PyObject::builtin_function(CompactString::from("reversed"))),
        ("round", PyObject::builtin_function(CompactString::from("round"))),
        ("setattr", PyObject::builtin_function(CompactString::from("setattr"))),
        ("sorted", PyObject::builtin_function(CompactString::from("sorted"))),
        ("sum", PyObject::builtin_function(CompactString::from("sum"))),
        ("vars", PyObject::builtin_function(CompactString::from("vars"))),
        ("zip", PyObject::builtin_function(CompactString::from("zip"))),
        ("__import__", PyObject::builtin_function(CompactString::from("__import__"))),
        ("__build_class__", PyObject::builtin_function(CompactString::from("__build_class__"))),
        // Exception types
        ("Exception", PyObject::exception_type(ferrython_core::error::ExceptionKind::Exception)),
        ("ValueError", PyObject::exception_type(ferrython_core::error::ExceptionKind::ValueError)),
        ("TypeError", PyObject::exception_type(ferrython_core::error::ExceptionKind::TypeError)),
        ("KeyError", PyObject::exception_type(ferrython_core::error::ExceptionKind::KeyError)),
        ("IndexError", PyObject::exception_type(ferrython_core::error::ExceptionKind::IndexError)),
        ("AttributeError", PyObject::exception_type(ferrython_core::error::ExceptionKind::AttributeError)),
        ("NameError", PyObject::exception_type(ferrython_core::error::ExceptionKind::NameError)),
        ("RuntimeError", PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError)),
        ("StopIteration", PyObject::exception_type(ferrython_core::error::ExceptionKind::StopIteration)),
        ("OSError", PyObject::exception_type(ferrython_core::error::ExceptionKind::OSError)),
        ("IOError", PyObject::exception_type(ferrython_core::error::ExceptionKind::OSError)),
        ("FileNotFoundError", PyObject::exception_type(ferrython_core::error::ExceptionKind::FileNotFoundError)),
        ("ImportError", PyObject::exception_type(ferrython_core::error::ExceptionKind::ImportError)),
        ("NotImplementedError", PyObject::exception_type(ferrython_core::error::ExceptionKind::NotImplementedError)),
        ("ZeroDivisionError", PyObject::exception_type(ferrython_core::error::ExceptionKind::ZeroDivisionError)),
        ("OverflowError", PyObject::exception_type(ferrython_core::error::ExceptionKind::OverflowError)),
        ("AssertionError", PyObject::exception_type(ferrython_core::error::ExceptionKind::AssertionError)),
        ("SyntaxError", PyObject::exception_type(ferrython_core::error::ExceptionKind::SyntaxError)),
    ])
}

// ── atexit module ──

pub fn create_atexit_module() -> PyObjectRef {
    use std::sync::Mutex;
    let callbacks: Arc<Mutex<Vec<PyObjectRef>>> = Arc::new(Mutex::new(Vec::new()));
    let cb_reg = callbacks.clone();
    let register_fn = PyObject::native_closure("atexit.register", move |args: &[PyObjectRef]| {
        if args.is_empty() { return Err(PyException::type_error("atexit.register requires a callable")); }
        cb_reg.lock().unwrap().push(args[0].clone());
        Ok(args[0].clone())
    });
    let cb_unreg = callbacks.clone();
    let unregister_fn = PyObject::native_closure("atexit.unregister", move |_args: &[PyObjectRef]| {
        let _cbs = cb_unreg.lock().unwrap();
        Ok(PyObject::none())
    });
    let _ncallbacks = PyObject::native_closure("atexit._ncallbacks", move |_args: &[PyObjectRef]| {
        let cbs = callbacks.lock().unwrap();
        Ok(PyObject::int(cbs.len() as i64))
    });
    make_module("atexit", vec![
        ("register", register_fn),
        ("unregister", unregister_fn),
        ("_run_exitfuncs", make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none()))),
        ("_ncallbacks", _ncallbacks),
    ])
}

// ── site module ──

pub fn create_site_module() -> PyObjectRef {
    make_module("site", vec![
        ("ENABLE_USER_SITE", PyObject::bool_val(false)),
        ("PREFIXES", PyObject::list(vec![])),
        ("USER_SITE", PyObject::none()),
        ("USER_BASE", PyObject::none()),
        ("getusersitepackages", make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::str_val(CompactString::from(""))))),
        ("getsitepackages", make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::list(vec![])))),
    ])
}

// ── sched module ──

pub fn create_sched_module() -> PyObjectRef {
    let scheduler_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("scheduler"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("enter"), make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("run"), make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("empty"), make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::bool_val(true))));
            w.insert(CompactString::from("queue"), PyObject::list(vec![]));
        }
        Ok(inst)
    });
    make_module("sched", vec![("scheduler", scheduler_fn)])
}
