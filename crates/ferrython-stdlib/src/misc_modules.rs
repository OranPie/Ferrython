//! Miscellaneous stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, InstanceData,
    make_module, make_builtin, check_args, check_args_min,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

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
    // Uses __closing_thing__ marker so the VM can call close() through normal dispatch
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
        // __exit__ is a no-op; the VM handles calling close() via __closing_thing__ marker
        attrs.insert(CompactString::from("__exit__"), PyObject::native_function(
            "closing.__exit__", |_args: &[PyObjectRef]| {
                Ok(PyObject::bool_val(false))
            }
        ));
        attrs.insert(CompactString::from("__closing_thing__"), thing);
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
    
    // Get annotations to discover fields — walk MRO for inherited dataclass fields
    let mut field_names: Vec<CompactString> = Vec::new();
    let mut field_defaults: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    let mut compare_fields: Vec<CompactString> = Vec::new();
    let mut init_fields: Vec<CompactString> = Vec::new();
    
    if let PyObjectPayload::Class(cd) = &cls.payload {
        // Collect fields from base classes first (MRO order), then own class
        let mut all_classes: Vec<PyObjectRef> = cd.bases.iter().rev().cloned().collect();
        all_classes.push(cls.clone());
        
        for base_cls in &all_classes {
            if let PyObjectPayload::Class(bcd) = &base_cls.payload {
                let ns = bcd.namespace.read();
                if let Some(annotations) = ns.get("__annotations__") {
                    if let PyObjectPayload::Dict(ann_map) = &annotations.payload {
                        for (k, _v) in ann_map.read().iter() {
                            if let HashableKey::Str(name) = k {
                                if !field_names.contains(name) {
                                    field_names.push(name.clone());
                                }
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

