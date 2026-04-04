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
            // field(default=..., default_factory=..., compare=..., init=..., repr=..., ...)
            // kwargs passed as trailing dict by VM
            let mut compare = true;
            let mut init = true;
            let mut default_val: Option<PyObjectRef> = None;
            let mut factory_val: Option<PyObjectRef> = None;

            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(kw_map) = &last.payload {
                    let r = kw_map.read();
                    if let Some(v) = r.get(&HashableKey::Str(CompactString::from("compare"))) {
                        compare = v.is_truthy();
                    }
                    if let Some(v) = r.get(&HashableKey::Str(CompactString::from("init"))) {
                        init = v.is_truthy();
                    }
                    if let Some(f) = r.get(&HashableKey::Str(CompactString::from("default_factory"))) {
                        factory_val = Some(f.clone());
                    }
                    if let Some(d) = r.get(&HashableKey::Str(CompactString::from("default"))) {
                        default_val = Some(d.clone());
                    }
                }
            }
            // Always return a field sentinel Module
            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("__field_compare__"), PyObject::bool_val(compare));
            attrs.insert(CompactString::from("__field_init__"), PyObject::bool_val(init));
            if let Some(factory) = factory_val {
                attrs.insert(CompactString::from("__field_factory__"), factory);
            } else if let Some(default) = default_val {
                attrs.insert(CompactString::from("__field_default__"), default);
            }
            Ok(PyObject::module_with_attrs(CompactString::from("_field"), attrs))
        })),
        ("asdict", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("asdict requires 1 argument")); }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                // Use __dataclass_fields__ to get fields in order
                if let Some(class) = inst.attrs.read().get("__class__").cloned()
                    .or_else(|| Some(inst.class.clone())) {
                    if let Some(fields) = class.get_attr("__dataclass_fields__") {
                        if let PyObjectPayload::Tuple(field_tuples) = &fields.payload {
                            let attrs = inst.attrs.read();
                            let mut map = IndexMap::new();
                            for ft in field_tuples {
                                if let PyObjectPayload::Tuple(info) = &ft.payload {
                                    let name = info[0].py_to_string();
                                    if let Some(v) = attrs.get(name.as_str()) {
                                        map.insert(HashableKey::Str(CompactString::from(name.as_str())), v.clone());
                                    }
                                }
                            }
                            return Ok(PyObject::dict(map));
                        }
                    }
                }
                // Fallback: all non-_ attrs
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
                if let Some(class) = inst.attrs.read().get("__class__").cloned()
                    .or_else(|| Some(inst.class.clone())) {
                    if let Some(fields) = class.get_attr("__dataclass_fields__") {
                        if let PyObjectPayload::Tuple(field_tuples) = &fields.payload {
                            let attrs = inst.attrs.read();
                            let items: Vec<_> = field_tuples.iter().filter_map(|ft| {
                                if let PyObjectPayload::Tuple(info) = &ft.payload {
                                    let name = info[0].py_to_string();
                                    attrs.get(name.as_str()).cloned()
                                } else { None }
                            }).collect();
                            return Ok(PyObject::tuple(items));
                        }
                    }
                }
                let attrs = inst.attrs.read();
                let items: Vec<_> = attrs.values().cloned().collect();
                Ok(PyObject::tuple(items))
            } else {
                Err(PyException::type_error("astuple() should be called on dataclass instances"))
            }
        })),
        ("fields", make_builtin(|args| {
            // fields(instance_or_class) -> tuple of Field objects
            if args.is_empty() { return Err(PyException::type_error("fields requires 1 argument")); }
            let cls = match &args[0].payload {
                PyObjectPayload::Class(_) => args[0].clone(),
                PyObjectPayload::Instance(inst) => inst.class.clone(),
                _ => return Err(PyException::type_error("fields() argument must be a dataclass or instance")),
            };
            if let Some(fields_tuple) = cls.get_attr("__dataclass_fields__") {
                if let PyObjectPayload::Tuple(field_tuples) = &fields_tuple.payload {
                    let field_objs: Vec<PyObjectRef> = field_tuples.iter().map(|ft| {
                        if let PyObjectPayload::Tuple(info) = &ft.payload {
                            // Create a simple Field-like object with .name, .default, etc.
                            let name = info[0].py_to_string();
                            let mut field_attrs = IndexMap::new();
                            field_attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(name.as_str())));
                            if info.len() > 2 {
                                field_attrs.insert(CompactString::from("default"), info[2].clone());
                            }
                            if info.len() > 3 {
                                field_attrs.insert(CompactString::from("init"), info[3].clone());
                            }
                            if info.len() > 4 {
                                field_attrs.insert(CompactString::from("compare"), info[4].clone());
                            }
                            PyObject::instance_with_attrs(
                                PyObject::builtin_type(CompactString::from("Field")),
                                field_attrs,
                            )
                        } else { ft.clone() }
                    }).collect();
                    return Ok(PyObject::tuple(field_objs));
                }
            }
            Ok(PyObject::tuple(vec![]))
        })),
        ("replace", make_builtin(|args| {
            // replace(instance, **kwargs)
            if args.is_empty() { return Err(PyException::type_error("replace requires at least 1 argument")); }
            let instance = &args[0];
            if let PyObjectPayload::Instance(inst) = &instance.payload {
                let cls = inst.class.clone();
                // Clone all attrs
                let mut new_attrs = inst.attrs.read().clone();
                // Apply kwargs overrides
                if args.len() > 1 {
                    if let PyObjectPayload::Dict(kw_map) = &args[1].payload {
                        for (k, v) in kw_map.read().iter() {
                            if let HashableKey::Str(name) = k {
                                new_attrs.insert(name.clone(), v.clone());
                            }
                        }
                    }
                }
                Ok(PyObject::instance_with_attrs(cls, new_attrs))
            } else {
                Err(PyException::type_error("replace() argument must be a dataclass instance"))
            }
        })),
    ])
}

fn dataclass_decorator(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("dataclass requires 1 argument")); }
    let cls = &args[0];
    
    // If called as @dataclass(eq=True, ...) the first arg is kwargs dict, not a class.
    if !matches!(&cls.payload, PyObjectPayload::Class(_)) {
        let mut order = false;
        let mut frozen = false;
        if let PyObjectPayload::Dict(map) = &cls.payload {
            let m = map.read();
            if let Some(v) = m.get(&HashableKey::Str(CompactString::from("order"))) {
                order = v.is_truthy();
            }
            if let Some(v) = m.get(&HashableKey::Str(CompactString::from("frozen"))) {
                frozen = v.is_truthy();
            }
        }
        let order_flag = order;
        let frozen_flag = frozen;
        return Ok(PyObject::native_closure("dataclass", move |args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("dataclass requires 1 argument")); }
            dataclass_apply(&args[0], order_flag, frozen_flag)
        }));
    }
    
    dataclass_apply(cls, false, false)
}

fn dataclass_apply(cls: &PyObjectRef, order: bool, frozen: bool) -> PyResult<PyObjectRef> {
    
    // Get annotations to discover fields
    let mut field_names: Vec<CompactString> = Vec::new();
    let mut field_defaults: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    let mut compare_fields: Vec<CompactString> = Vec::new();
    let mut init_fields: Vec<CompactString> = Vec::new(); // fields that participate in __init__
    
    if let PyObjectPayload::Class(cd) = &cls.payload {
        let ns = cd.namespace.read();
        if let Some(annotations) = ns.get("__annotations__") {
            if let PyObjectPayload::Dict(ann_map) = &annotations.payload {
                for (k, _v) in ann_map.read().iter() {
                    if let HashableKey::Str(name) = k {
                        field_names.push(name.clone());
                        let mut compare = true;
                        let mut init = true;
                        
                        if let Some(default) = ns.get(name.as_str()) {
                            if let PyObjectPayload::Module(md) = &default.payload {
                                let mod_attrs = md.attrs.read();
                                if let Some(cmp_flag) = mod_attrs.get("__field_compare__") {
                                    compare = cmp_flag.is_truthy();
                                }
                                if let Some(init_flag) = mod_attrs.get("__field_init__") {
                                    init = init_flag.is_truthy();
                                }
                                if let Some(factory) = mod_attrs.get("__field_factory__") {
                                    field_defaults.insert(name.clone(), factory.clone());
                                } else if let Some(default_val) = mod_attrs.get("__field_default__") {
                                    field_defaults.insert(name.clone(), default_val.clone());
                                }
                                // field() with no default/factory: no default entry
                            } else {
                                field_defaults.insert(name.clone(), default.clone());
                            }
                        }
                        if compare { compare_fields.push(name.clone()); }
                        if init { init_fields.push(name.clone()); }
                    }
                }
            }
        }
    }
    
    // Store __dataclass_fields__ as tuple of (name, has_default, default_val, init, compare) tuples
    let fields_info: Vec<PyObjectRef> = field_names.iter().map(|name| {
        let has_default = field_defaults.contains_key(name.as_str());
        let default_val = field_defaults.get(name.as_str()).cloned().unwrap_or_else(PyObject::none);
        let init_flag = init_fields.contains(name);
        let compare_flag = compare_fields.contains(name);
        PyObject::tuple(vec![
            PyObject::str_val(CompactString::from(name.as_str())),
            PyObject::bool_val(has_default),
            default_val,
            PyObject::bool_val(init_flag),
            PyObject::bool_val(compare_flag),
        ])
    }).collect();
    
    if let PyObjectPayload::Class(cd) = &cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(CompactString::from("__dataclass_fields__"), PyObject::tuple(fields_info));
        ns.insert(CompactString::from("__dataclass__"), PyObject::bool_val(true));

        // Generate __setattr__ and __delattr__ for frozen=True
        if frozen {
            ns.insert(CompactString::from("__dataclass_frozen__"), PyObject::bool_val(true));
        }

        // Generate ordering methods if order=True
        if order {
            let fields_for_lt = compare_fields.clone();
            ns.insert(CompactString::from("__lt__"), PyObject::native_closure("__lt__", move |args: &[PyObjectRef]| {
                check_args("__lt__", args, 2)?;
                let (a, b) = (&args[0], &args[1]);
                let tup_a = extract_compare_tuple(a, &fields_for_lt);
                let tup_b = extract_compare_tuple(b, &fields_for_lt);
                tup_a.compare(&tup_b, ferrython_core::object::CompareOp::Lt)
            }));

            let fields_for_le = compare_fields.clone();
            ns.insert(CompactString::from("__le__"), PyObject::native_closure("__le__", move |args: &[PyObjectRef]| {
                check_args("__le__", args, 2)?;
                let (a, b) = (&args[0], &args[1]);
                let tup_a = extract_compare_tuple(a, &fields_for_le);
                let tup_b = extract_compare_tuple(b, &fields_for_le);
                tup_a.compare(&tup_b, ferrython_core::object::CompareOp::Le)
            }));

            let fields_for_gt = compare_fields.clone();
            ns.insert(CompactString::from("__gt__"), PyObject::native_closure("__gt__", move |args: &[PyObjectRef]| {
                check_args("__gt__", args, 2)?;
                let (a, b) = (&args[0], &args[1]);
                let tup_a = extract_compare_tuple(a, &fields_for_gt);
                let tup_b = extract_compare_tuple(b, &fields_for_gt);
                tup_a.compare(&tup_b, ferrython_core::object::CompareOp::Gt)
            }));

            let fields_for_ge = compare_fields.clone();
            ns.insert(CompactString::from("__ge__"), PyObject::native_closure("__ge__", move |args: &[PyObjectRef]| {
                check_args("__ge__", args, 2)?;
                let (a, b) = (&args[0], &args[1]);
                let tup_a = extract_compare_tuple(a, &fields_for_ge);
                let tup_b = extract_compare_tuple(b, &fields_for_ge);
                tup_a.compare(&tup_b, ferrython_core::object::CompareOp::Ge)
            }));
        }
    }
    
    Ok(cls.clone())
}

/// Extract a comparison tuple from a dataclass instance for ordering.
fn extract_compare_tuple(obj: &PyObjectRef, fields: &[CompactString]) -> PyObjectRef {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        let attrs = inst.attrs.read();
        let vals: Vec<PyObjectRef> = fields.iter()
            .map(|f| attrs.get(f.as_str()).cloned().unwrap_or_else(PyObject::none))
            .collect();
        PyObject::tuple(vals)
    } else {
        PyObject::tuple(vec![])
    }
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
                match &method.payload {
                    PyObjectPayload::NativeFunction { func, .. } => func(&extra_args),
                    PyObjectPayload::NativeClosure { func, .. } => func(&extra_args),
                    PyObjectPayload::BuiltinBoundMethod { .. } => {
                        // BuiltinBoundMethod needs VM to dispatch; return as-is
                        // Caller should dispatch through VM
                        Ok(method)
                    }
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

// ── pprint module ──

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

// ── shelve module ──

pub fn create_shelve_module() -> PyObjectRef {
    let open_fn = make_builtin(|args: &[PyObjectRef]| {
        let _filename = if !args.is_empty() { args[0].py_to_string() } else { "shelf.db".to_string() };
        let cls = PyObject::class(CompactString::from("Shelf"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            let data: Arc<RwLock<IndexMap<HashableKey, PyObjectRef>>> = Arc::new(RwLock::new(IndexMap::new()));

            let d1 = data.clone();
            w.insert(CompactString::from("__getitem__"), PyObject::native_closure(
                "Shelf.__getitem__", move |args: &[PyObjectRef]| {
                    check_args_min("Shelf.__getitem__", args, 1)?;
                    let key = HashableKey::Str(CompactString::from(args[0].py_to_string().as_str()));
                    d1.read().get(&key).cloned().ok_or_else(|| PyException::key_error(args[0].py_to_string()))
                }
            ));

            let d2 = data.clone();
            w.insert(CompactString::from("__setitem__"), PyObject::native_closure(
                "Shelf.__setitem__", move |args: &[PyObjectRef]| {
                    check_args_min("Shelf.__setitem__", args, 2)?;
                    let key = HashableKey::Str(CompactString::from(args[0].py_to_string().as_str()));
                    d2.write().insert(key, args[1].clone());
                    Ok(PyObject::none())
                }
            ));

            let d3 = data.clone();
            w.insert(CompactString::from("__contains__"), PyObject::native_closure(
                "Shelf.__contains__", move |args: &[PyObjectRef]| {
                    check_args_min("Shelf.__contains__", args, 1)?;
                    let key = HashableKey::Str(CompactString::from(args[0].py_to_string().as_str()));
                    Ok(PyObject::bool_val(d3.read().contains_key(&key)))
                }
            ));

            let d4 = data.clone();
            w.insert(CompactString::from("keys"), PyObject::native_closure(
                "Shelf.keys", move |_: &[PyObjectRef]| {
                    let keys: Vec<PyObjectRef> = d4.read().keys().map(|k| match k {
                        HashableKey::Str(s) => PyObject::str_val(s.clone()),
                        _ => PyObject::none(),
                    }).collect();
                    Ok(PyObject::list(keys))
                }
            ));

            w.insert(CompactString::from("close"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("sync"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));

            let ir = inst.clone();
            w.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "Shelf.__enter__", move |_: &[PyObjectRef]| Ok(ir.clone())
            ));
            w.insert(CompactString::from("__exit__"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::bool_val(false))));
        }
        Ok(inst)
    });

    make_module("shelve", vec![
        ("open", open_fn),
    ])
}

// ── abc module ──

pub fn create_abc_module() -> PyObjectRef {
    // ABC base class with __abstractmethods__ marker
    let abc_class = PyObject::class(
        CompactString::from("ABC"),
        vec![],
        IndexMap::new(),
    );
    if let PyObjectPayload::Class(ref cd) = abc_class.payload {
        cd.namespace.write().insert(
            CompactString::from("__abstractmethods__"),
            PyObject::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(IndexMap::new())))),
        );
    }

    let abstractmethod_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("abstractmethod requires 1 argument"));
        }
        let func = args[0].clone();
        // Return a marker tuple: ("__abstract__", func)
        let marker = PyObject::tuple(vec![
            PyObject::str_val(CompactString::from("__abstract__")),
            func,
        ]);
        Ok(marker)
    });

    let abcmeta_cls = PyObject::class(
        CompactString::from("ABCMeta"), vec![], IndexMap::new(),
    );

    let abstractclassmethod_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("abstractclassmethod requires 1 argument"));
        }
        Ok(args[0].clone())
    });

    let abstractstaticmethod_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("abstractstaticmethod requires 1 argument"));
        }
        Ok(args[0].clone())
    });

    let abstractproperty_fn = make_builtin(|args: &[PyObjectRef]| {
        if !args.is_empty() {
            Ok(args[0].clone())
        } else {
            Ok(PyObject::none())
        }
    });

    let cache_token: Arc<RwLock<i64>> = Arc::new(RwLock::new(0));
    let get_cache_token_fn = PyObject::native_closure(
        "abc.get_cache_token", move |_args: &[PyObjectRef]| {
            Ok(PyObject::int(*cache_token.read()))
        }
    );

    make_module("abc", vec![
        ("ABC", abc_class),
        ("ABCMeta", abcmeta_cls),
        ("abstractmethod", abstractmethod_fn),
        ("abstractclassmethod", abstractclassmethod_fn),
        ("abstractstaticmethod", abstractstaticmethod_fn),
        ("abstractproperty", abstractproperty_fn),
        ("get_cache_token", get_cache_token_fn),
    ])
}
