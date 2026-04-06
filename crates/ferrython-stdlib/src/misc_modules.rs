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

            // ExitStack methods are stored as instance attrs. Instance attr lookup
            // does NOT bind self, so we capture inst via closure (NativeClosure).
            let self_ref = inst.clone();
            attrs.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "ExitStack.__enter__", {
                    let self_ref = self_ref.clone();
                    move |_args: &[PyObjectRef]| Ok(self_ref.clone())
                }
            ));

            attrs.insert(CompactString::from("__exit__"), PyObject::native_closure(
                "ExitStack.__exit__", {
                    let self_ref = self_ref.clone();
                    move |_args: &[PyObjectRef]| {
                        if let Some(cbs) = self_ref.get_attr("_callbacks") {
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
                        Ok(PyObject::bool_val(false))
                    }
                }
            ));

            attrs.insert(CompactString::from("push"), PyObject::native_closure(
                "ExitStack.push", {
                    let self_ref = self_ref.clone();
                    move |args: &[PyObjectRef]| {
                        check_args_min("ExitStack.push", args, 1)?;
                        let callback = &args[0];
                        if let Some(cbs) = self_ref.get_attr("_callbacks") {
                            if let PyObjectPayload::List(items) = &cbs.payload {
                                items.write().push(callback.clone());
                            }
                        }
                        Ok(callback.clone())
                    }
                }
            ));

            attrs.insert(CompactString::from("callback"), PyObject::native_closure(
                "ExitStack.callback", {
                    let self_ref = self_ref.clone();
                    move |args: &[PyObjectRef]| {
                        check_args_min("ExitStack.callback", args, 1)?;
                        let func = &args[0];
                        if let Some(cbs) = self_ref.get_attr("_callbacks") {
                            if let PyObjectPayload::List(items) = &cbs.payload {
                                items.write().push(func.clone());
                            }
                        }
                        Ok(func.clone())
                    }
                }
            ));

            attrs.insert(CompactString::from("enter_context"), PyObject::native_closure(
                "ExitStack.enter_context", {
                    let self_ref = self_ref.clone();
                    move |args: &[PyObjectRef]| {
                        check_args_min("ExitStack.enter_context", args, 1)?;
                        let cm = &args[0];
                        // Call __enter__
                        let result = if let Some(enter) = cm.get_attr("__enter__") {
                            match &enter.payload {
                                PyObjectPayload::NativeFunction { func, .. } => func(&[cm.clone()])?,
                                PyObjectPayload::NativeClosure { func, .. } => func(&[cm.clone()])?,
                                // BuiltinBoundMethod: for all builtin types __enter__ returns self
                                PyObjectPayload::BuiltinBoundMethod { .. } => cm.clone(),
                                _ => cm.clone()
                            }
                        } else {
                            PyObject::none()
                        };
                        // Register __exit__ as callback
                        if let Some(exit_fn) = cm.get_attr("__exit__") {
                            if let Some(cbs) = self_ref.get_attr("_callbacks") {
                                if let PyObjectPayload::List(items) = &cbs.payload {
                                    items.write().push(exit_fn);
                                }
                            }
                        }
                        Ok(result)
                    }
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

    // redirect_stdout(new_target) — context manager that swaps sys.stdout
    // Uses the global STDOUT_OVERRIDE stack so print() picks it up.
    let redirect_stdout_fn = make_builtin(|args: &[PyObjectRef]| {
        let target = if !args.is_empty() { args[0].clone() } else { PyObject::none() };
        let cls = PyObject::class(CompactString::from("redirect_stdout"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("__redirect_stdout__"), PyObject::bool_val(true));
        attrs.insert(CompactString::from("_new_target"), target.clone());
        let inst = PyObject::instance_with_attrs(cls, attrs);
        if let PyObjectPayload::Instance(ref idata) = inst.payload {
            let t = target.clone();
            idata.attrs.write().insert(CompactString::from("__enter__"), PyObject::native_closure(
                "redirect_stdout.__enter__", move |_args| {
                    crate::push_stdout_override(t.clone());
                    Ok(t.clone())
                }
            ));
            idata.attrs.write().insert(CompactString::from("__exit__"), PyObject::native_closure(
                "redirect_stdout.__exit__", move |_args| {
                    crate::pop_stdout_override();
                    Ok(PyObject::bool_val(false))
                }
            ));
        }
        Ok(inst)
    });

    // redirect_stderr(new_target) — same pattern for stderr
    let redirect_stderr_fn = make_builtin(|args: &[PyObjectRef]| {
        let target = if !args.is_empty() { args[0].clone() } else { PyObject::none() };
        let cls = PyObject::class(CompactString::from("redirect_stderr"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("__redirect_stderr__"), PyObject::bool_val(true));
        attrs.insert(CompactString::from("_new_target"), target.clone());
        let inst = PyObject::instance_with_attrs(cls, attrs);
        if let PyObjectPayload::Instance(ref idata) = inst.payload {
            let t = target.clone();
            idata.attrs.write().insert(CompactString::from("__enter__"), PyObject::native_closure(
                "redirect_stderr.__enter__", move |_args| {
                    crate::push_stderr_override(t.clone());
                    Ok(t.clone())
                }
            ));
            idata.attrs.write().insert(CompactString::from("__exit__"), PyObject::native_closure(
                "redirect_stderr.__exit__", move |_args| {
                    crate::pop_stderr_override();
                    Ok(PyObject::bool_val(false))
                }
            ));
        }
        Ok(inst)
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
                is_special: true, dict_storage: inst.dict_storage.as_ref().map(|ds| Arc::new(RwLock::new(ds.read().clone()))),
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
        // Additional builtins
        ("breakpoint", PyObject::builtin_function(CompactString::from("breakpoint"))),
        ("compile", PyObject::builtin_function(CompactString::from("compile"))),
        ("delattr", PyObject::builtin_function(CompactString::from("delattr"))),
        ("memoryview", PyObject::builtin_type(CompactString::from("memoryview"))),
        ("slice", PyObject::builtin_type(CompactString::from("slice"))),
        ("NotImplemented", PyObject::none()),
        ("Ellipsis", PyObject::none()),
        ("__debug__", PyObject::bool_val(true)),
    ])
}

// ── contextvars module ──

pub fn create_contextvars_module() -> PyObjectRef {
    make_module("contextvars", vec![
        ("ContextVar", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("ContextVar() requires a name")); }
            let name = args[0].py_to_string();
            // Check for default kwarg in trailing dict
            let default_val = if args.len() > 1 {
                if let PyObjectPayload::Dict(kw) = &args[args.len()-1].payload {
                    kw.read().get(&HashableKey::Str(CompactString::from("default")))
                        .cloned()
                } else {
                    Some(args[1].clone())
                }
            } else { None };

            let cls = PyObject::class(CompactString::from("ContextVar"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(&name)));
                let value: Arc<RwLock<Option<PyObjectRef>>> = Arc::new(RwLock::new(default_val.clone()));

                let v = value.clone();
                attrs.insert(CompactString::from("get"), PyObject::native_closure("ContextVar.get", move |a: &[PyObjectRef]| {
                    if let Some(val) = v.read().as_ref() {
                        Ok(val.clone())
                    } else if !a.is_empty() {
                        Ok(a[0].clone()) // default argument
                    } else {
                        Err(PyException::runtime_error("ContextVar has no value"))
                    }
                }));

                let v = value.clone();
                attrs.insert(CompactString::from("set"), PyObject::native_closure("ContextVar.set", move |a: &[PyObjectRef]| {
                    if a.is_empty() { return Err(PyException::type_error("set() requires a value")); }
                    let old = v.read().clone();
                    *v.write() = Some(a[0].clone());
                    // Return a Token
                    let token_cls = PyObject::class(CompactString::from("Token"), vec![], IndexMap::new());
                    let token = PyObject::instance(token_cls);
                    if let PyObjectPayload::Instance(ref td) = token.payload {
                        let mut ta = td.attrs.write();
                        ta.insert(CompactString::from("old_value"), old.unwrap_or_else(PyObject::none));
                        ta.insert(CompactString::from("var"), PyObject::str_val(CompactString::from(&name)));
                    }
                    Ok(token)
                }));
            }
            Ok(inst)
        })),
        ("Context", make_builtin(|_| {
            let cls = PyObject::class(CompactString::from("Context"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                attrs.insert(CompactString::from("run"), make_builtin(|_| Ok(PyObject::none())));
                attrs.insert(CompactString::from("copy"), make_builtin(|_| Ok(PyObject::none())));
            }
            Ok(inst)
        })),
        ("copy_context", make_builtin(|_| {
            let cls = PyObject::class(CompactString::from("Context"), vec![], IndexMap::new());
            Ok(PyObject::instance(cls))
        })),
        ("Token", PyObject::class(CompactString::from("Token"), vec![], IndexMap::new())),
    ])
}

// ── mimetypes module ──

pub fn create_mimetypes_module() -> PyObjectRef {
    make_module("mimetypes", vec![
        ("guess_type", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("guess_type requires a url")); }
            let url = args[0].py_to_string();
            let ext = url.rsplit('.').next().unwrap_or("");
            let mime = match ext.to_lowercase().as_str() {
                "html" | "htm" => "text/html",
                "css" => "text/css",
                "js" => "application/javascript",
                "json" => "application/json",
                "xml" => "application/xml",
                "txt" => "text/plain",
                "csv" => "text/csv",
                "py" => "text/x-python",
                "jpg" | "jpeg" => "image/jpeg",
                "png" => "image/png",
                "gif" => "image/gif",
                "svg" => "image/svg+xml",
                "ico" => "image/x-icon",
                "webp" => "image/webp",
                "pdf" => "application/pdf",
                "zip" => "application/zip",
                "gz" | "gzip" => "application/gzip",
                "tar" => "application/x-tar",
                "mp3" => "audio/mpeg",
                "mp4" => "video/mp4",
                "wav" => "audio/wav",
                "woff" => "font/woff",
                "woff2" => "font/woff2",
                "ttf" => "font/ttf",
                "otf" => "font/otf",
                "wasm" => "application/wasm",
                "yaml" | "yml" => "application/x-yaml",
                "toml" => "application/toml",
                "md" => "text/markdown",
                "rs" => "text/x-rust",
                _ => return Ok(PyObject::tuple(vec![PyObject::none(), PyObject::none()])),
            };
            Ok(PyObject::tuple(vec![
                PyObject::str_val(CompactString::from(mime)),
                PyObject::none(), // encoding
            ]))
        })),
        ("guess_extension", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("guess_extension requires a type")); }
            let mime = args[0].py_to_string();
            let ext = match mime.as_str() {
                "text/html" => ".html",
                "text/css" => ".css",
                "application/javascript" => ".js",
                "application/json" => ".json",
                "text/plain" => ".txt",
                "image/jpeg" => ".jpg",
                "image/png" => ".png",
                "application/pdf" => ".pdf",
                _ => return Ok(PyObject::none()),
            };
            Ok(PyObject::str_val(CompactString::from(ext)))
        })),
        ("init", make_builtin(|_| Ok(PyObject::none()))),
        ("types_map", PyObject::dict(IndexMap::new())),
    ])
}

// ── readline module ──

pub fn create_readline_module() -> PyObjectRef {
    // Stub readline — used by REPL but mostly no-ops in embedded context
    make_module("readline", vec![
        ("parse_and_bind", make_builtin(|_| Ok(PyObject::none()))),
        ("set_completer", make_builtin(|_| Ok(PyObject::none()))),
        ("get_completer", make_builtin(|_| Ok(PyObject::none()))),
        ("set_completer_delims", make_builtin(|_| Ok(PyObject::none()))),
        ("get_completer_delims", make_builtin(|_| Ok(PyObject::str_val(CompactString::from(" \t\n`~!@#$%^&*()-=+[{]}\\|;:'\",<>/?"))
        ))),
        ("add_history", make_builtin(|_| Ok(PyObject::none()))),
        ("clear_history", make_builtin(|_| Ok(PyObject::none()))),
        ("get_history_length", make_builtin(|_| Ok(PyObject::int(-1)))),
        ("set_history_length", make_builtin(|_| Ok(PyObject::none()))),
        ("get_current_history_length", make_builtin(|_| Ok(PyObject::int(0)))),
        ("read_history_file", make_builtin(|_| Ok(PyObject::none()))),
        ("write_history_file", make_builtin(|_| Ok(PyObject::none()))),
        ("set_startup_hook", make_builtin(|_| Ok(PyObject::none()))),
        ("set_pre_input_hook", make_builtin(|_| Ok(PyObject::none()))),
    ])
}

// ── runpy module ──

pub fn create_runpy_module() -> PyObjectRef {
    make_module("runpy", vec![
        ("run_module", make_builtin(|_| {
            Err(PyException::not_implemented_error("runpy.run_module"))
        })),
        ("run_path", make_builtin(|_| {
            Err(PyException::not_implemented_error("runpy.run_path"))
        })),
    ])
}

// ── cmd module ──

pub fn create_cmd_module() -> PyObjectRef {
    make_module("cmd", vec![
        ("Cmd", make_builtin(|_| {
            let cls = PyObject::class(CompactString::from("Cmd"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                attrs.insert(CompactString::from("prompt"), PyObject::str_val(CompactString::from("(Cmd) ")));
                attrs.insert(CompactString::from("intro"), PyObject::none());
                attrs.insert(CompactString::from("cmdloop"), make_builtin(|_| {
                    Err(PyException::not_implemented_error("Cmd.cmdloop requires interactive I/O"))
                }));
                attrs.insert(CompactString::from("onecmd"), make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() { return Err(PyException::type_error("onecmd requires a string")); }
                    Ok(PyObject::bool_val(false)) // not stop
                }));
                attrs.insert(CompactString::from("emptyline"), make_builtin(|_| Ok(PyObject::none())));
                attrs.insert(CompactString::from("default"), make_builtin(|args: &[PyObjectRef]| {
                    let line = if !args.is_empty() { args[0].py_to_string() } else { String::new() };
                    Err(PyException::runtime_error(&format!("*** Unknown syntax: {}", line)))
                }));
            }
            Ok(inst)
        })),
    ])
}

// ── compileall module ──

pub fn create_compileall_module() -> PyObjectRef {
    make_module("compileall", vec![
        ("compile_dir", make_builtin(|_| Ok(PyObject::bool_val(true)))),
        ("compile_file", make_builtin(|_| Ok(PyObject::bool_val(true)))),
        ("compile_path", make_builtin(|_| Ok(PyObject::bool_val(true)))),
    ])
}

// ── pstats module ──

pub fn create_pstats_module() -> PyObjectRef {
    make_module("pstats", vec![
        ("Stats", make_builtin(|args: &[PyObjectRef]| {
            let cls = PyObject::class(CompactString::from("Stats"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                if !args.is_empty() {
                    attrs.insert(CompactString::from("_data"), args[0].clone());
                }
                attrs.insert(CompactString::from("sort_stats"), make_builtin(|_| Ok(PyObject::none())));
                attrs.insert(CompactString::from("print_stats"), make_builtin(|_| Ok(PyObject::none())));
                attrs.insert(CompactString::from("print_callers"), make_builtin(|_| Ok(PyObject::none())));
                attrs.insert(CompactString::from("print_callees"), make_builtin(|_| Ok(PyObject::none())));
                attrs.insert(CompactString::from("strip_dirs"), make_builtin(|_| Ok(PyObject::none())));
            }
            Ok(inst)
        })),
        ("SortKey", {
            let cls = PyObject::class(CompactString::from("SortKey"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                attrs.insert(CompactString::from("CALLS"), PyObject::str_val(CompactString::from("calls")));
                attrs.insert(CompactString::from("CUMULATIVE"), PyObject::str_val(CompactString::from("cumulative")));
                attrs.insert(CompactString::from("TIME"), PyObject::str_val(CompactString::from("time")));
                attrs.insert(CompactString::from("NAME"), PyObject::str_val(CompactString::from("name")));
            }
            inst
        }),
    ])
}

// ── quopri module ──

pub fn create_quopri_module() -> PyObjectRef {
    make_module("quopri", vec![
        ("encode", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("encode requires input")); }
            let data = args[0].py_to_string();
            let mut encoded = String::new();
            for b in data.bytes() {
                if (b == b'\t' || b == b' ' || (b >= 33 && b <= 126)) && b != b'=' {
                    encoded.push(b as char);
                } else {
                    encoded.push_str(&format!("={:02X}", b));
                }
            }
            Ok(PyObject::str_val(CompactString::from(encoded)))
        })),
        ("decode", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("decode requires input")); }
            let data = args[0].py_to_string();
            let mut decoded = Vec::new();
            let bytes = data.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'=' && i + 2 < bytes.len() {
                    if let Ok(val) = u8::from_str_radix(
                        std::str::from_utf8(&bytes[i+1..i+3]).unwrap_or("00"), 16
                    ) {
                        decoded.push(val);
                        i += 3;
                        continue;
                    }
                }
                decoded.push(bytes[i]);
                i += 1;
            }
            Ok(PyObject::str_val(CompactString::from(
                String::from_utf8_lossy(&decoded).to_string()
            )))
        })),
        ("encodestring", make_builtin(|args: &[PyObjectRef]| {
            // Alias for encode
            if args.is_empty() { return Err(PyException::type_error("encodestring requires input")); }
            let data = args[0].py_to_string();
            let mut encoded = String::new();
            for b in data.bytes() {
                if (b == b'\t' || b == b' ' || (b >= 33 && b <= 126)) && b != b'=' {
                    encoded.push(b as char);
                } else {
                    encoded.push_str(&format!("={:02X}", b));
                }
            }
            Ok(PyObject::str_val(CompactString::from(encoded)))
        })),
        ("decodestring", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("decodestring requires input")); }
            let data = args[0].py_to_string();
            let mut decoded = Vec::new();
            let bytes = data.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'=' && i + 2 < bytes.len() {
                    if let Ok(val) = u8::from_str_radix(
                        std::str::from_utf8(&bytes[i+1..i+3]).unwrap_or("00"), 16
                    ) {
                        decoded.push(val);
                        i += 3;
                        continue;
                    }
                }
                decoded.push(bytes[i]);
                i += 1;
            }
            Ok(PyObject::str_val(CompactString::from(
                String::from_utf8_lossy(&decoded).to_string()
            )))
        })),
    ])
}

// ── stringprep module ──

pub fn create_stringprep_module() -> PyObjectRef {
    // RFC 3454 string preparation — used by SASL, LDAP, etc.
    make_module("stringprep", vec![
        ("in_table_a1", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::bool_val(false)); }
            let c = args[0].py_to_string();
            let ch = c.chars().next().unwrap_or('\0');
            // Unassigned code points (simplified check)
            Ok(PyObject::bool_val(!ch.is_alphanumeric() && !ch.is_ascii() && (ch as u32) > 0xFFFD))
        })),
        ("in_table_b1", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::bool_val(false)); }
            let c = args[0].py_to_string();
            let ch = c.chars().next().unwrap_or('\0');
            // Commonly mapped to nothing: soft hyphen, zero-width joiner, etc.
            Ok(PyObject::bool_val(ch == '\u{00AD}' || ch == '\u{200B}' || ch == '\u{200C}' || ch == '\u{200D}'))
        })),
        ("in_table_c12", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::bool_val(false)); }
            let c = args[0].py_to_string();
            let ch = c.chars().next().unwrap_or('\0');
            // Non-ASCII space
            Ok(PyObject::bool_val(ch.is_whitespace() && !ch.is_ascii()))
        })),
        ("in_table_c21", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::bool_val(false)); }
            let c = args[0].py_to_string();
            let ch = c.chars().next().unwrap_or('\0');
            Ok(PyObject::bool_val(ch.is_control() && ch.is_ascii()))
        })),
        ("in_table_c22", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::bool_val(false)); }
            let c = args[0].py_to_string();
            let ch = c.chars().next().unwrap_or('\0');
            Ok(PyObject::bool_val(ch.is_control() && !ch.is_ascii()))
        })),
        ("in_table_d1", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::bool_val(false)); }
            let c = args[0].py_to_string();
            let ch = c.chars().next().unwrap_or('\0');
            // RTL characters (simplified)
            Ok(PyObject::bool_val((ch as u32) >= 0x0590 && (ch as u32) <= 0x08FF))
        })),
        ("in_table_d2", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::bool_val(false)); }
            let c = args[0].py_to_string();
            let ch = c.chars().next().unwrap_or('\0');
            // LTR characters (simplified: Latin, CJK, etc.)
            Ok(PyObject::bool_val(ch.is_alphanumeric() && (ch as u32) < 0x0590))
        })),
    ])
}

// ── plistlib module ──

pub fn create_plistlib_module() -> PyObjectRef {
    // plistlib.dumps(value, fmt=FMT_XML) — serialize to XML plist bytes
    let dumps_fn = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("plistlib.dumps() missing required argument: 'value'"));
        }
        let xml = plist_serialize_xml(&args[0])?;
        let full = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n{}</plist>\n", xml);
        Ok(PyObject::bytes(full.into_bytes()))
    });

    // plistlib.loads(data) — parse XML plist bytes
    let loads_fn = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("plistlib.loads() missing required argument: 'data'"));
        }
        let data = match &args[0].payload {
            PyObjectPayload::Bytes(b) => String::from_utf8_lossy(b).to_string(),
            PyObjectPayload::Str(s) => s.to_string(),
            _ => return Err(PyException::type_error("plistlib.loads() argument must be bytes or str")),
        };
        plist_parse_xml(&data)
    });

    // plistlib.dump(value, fp) — serialize to file
    let dump_fn = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("plistlib.dump() requires 2 arguments: value and file"));
        }
        let xml = plist_serialize_xml(&args[0])?;
        let full = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n{}</plist>\n", xml);
        // If file arg is a string path, write directly
        if let PyObjectPayload::Str(path) = &args[1].payload {
            std::fs::write(path.as_str(), full.as_bytes())
                .map_err(|e| PyException::runtime_error(format!("plistlib.dump: {}", e)))?;
        }
        Ok(PyObject::none())
    });

    // plistlib.load(fp) — parse from file
    let load_fn = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("plistlib.load() missing required argument: 'fp'"));
        }
        if let PyObjectPayload::Str(path) = &args[0].payload {
            let data = std::fs::read_to_string(path.as_str())
                .map_err(|e| PyException::runtime_error(format!("plistlib.load: {}", e)))?;
            return plist_parse_xml(&data);
        }
        Err(PyException::runtime_error("plistlib.load: expected file path or file-like object"))
    });

    make_module("plistlib", vec![
        ("loads", loads_fn),
        ("dumps", dumps_fn),
        ("load", load_fn),
        ("dump", dump_fn),
        ("FMT_XML", PyObject::int(1)),
        ("FMT_BINARY", PyObject::int(2)),
    ])
}

/// Serialize a Python object to XML plist format string
fn plist_serialize_xml(obj: &PyObjectRef) -> PyResult<String> {
    plist_serialize_xml_indent(obj, 0)
}

fn plist_serialize_xml_indent(obj: &PyObjectRef, indent: usize) -> PyResult<String> {
    let pad = "\t".repeat(indent);
    match &obj.payload {
        PyObjectPayload::None => Ok(format!("{}<false/>\n", pad)),
        PyObjectPayload::Bool(b) => {
            Ok(format!("{}<{}/>\n", pad, if *b { "true" } else { "false" }))
        }
        PyObjectPayload::Int(n) => Ok(format!("{}<integer>{}</integer>\n", pad, n)),
        PyObjectPayload::Float(f) => Ok(format!("{}<real>{}</real>\n", pad, f)),
        PyObjectPayload::Str(s) => {
            let escaped = s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
            Ok(format!("{}<string>{}</string>\n", pad, escaped))
        }
        PyObjectPayload::Bytes(b) => {
            use std::fmt::Write;
            let mut encoded = String::new();
            // Simple base64 encoding
            let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut i = 0;
            while i + 2 < b.len() {
                let n = ((b[i] as u32) << 16) | ((b[i+1] as u32) << 8) | (b[i+2] as u32);
                let _ = write!(encoded, "{}{}{}{}", table[(n >> 18) as usize & 63] as char,
                    table[(n >> 12) as usize & 63] as char,
                    table[(n >> 6) as usize & 63] as char,
                    table[n as usize & 63] as char);
                i += 3;
            }
            if i + 1 == b.len() {
                let n = (b[i] as u32) << 16;
                let _ = write!(encoded, "{}{}==", table[(n >> 18) as usize & 63] as char,
                    table[(n >> 12) as usize & 63] as char);
            } else if i + 2 == b.len() {
                let n = ((b[i] as u32) << 16) | ((b[i+1] as u32) << 8);
                let _ = write!(encoded, "{}{}{}=", table[(n >> 18) as usize & 63] as char,
                    table[(n >> 12) as usize & 63] as char,
                    table[(n >> 6) as usize & 63] as char);
            }
            Ok(format!("{}<data>\n{}{}\n{}</data>\n", pad, pad, encoded, pad))
        }
        PyObjectPayload::List(items) => {
            let items_r = items.read();
            let mut out = format!("{}<array>\n", pad);
            for item in items_r.iter() {
                out.push_str(&plist_serialize_xml_indent(item, indent + 1)?);
            }
            out.push_str(&format!("{}</array>\n", pad));
            Ok(out)
        }
        PyObjectPayload::Tuple(items) => {
            let mut out = format!("{}<array>\n", pad);
            for item in items.iter() {
                out.push_str(&plist_serialize_xml_indent(item, indent + 1)?);
            }
            out.push_str(&format!("{}</array>\n", pad));
            Ok(out)
        }
        PyObjectPayload::Dict(map) => {
            let map_r = map.read();
            let mut out = format!("{}<dict>\n", pad);
            for (k, v) in map_r.iter() {
                let key_str = match k {
                    HashableKey::Str(s) => s.to_string(),
                    HashableKey::Int(i) => i.to_string(),
                    _ => format!("{:?}", k),
                };
                let escaped = key_str.replace('&', "&amp;").replace('<', "&lt;");
                out.push_str(&format!("{}\t<key>{}</key>\n", pad, escaped));
                out.push_str(&plist_serialize_xml_indent(v, indent + 1)?);
            }
            out.push_str(&format!("{}</dict>\n", pad));
            Ok(out)
        }
        _ => Ok(format!("{}<string>{}</string>\n", pad, obj.py_to_string()
            .replace('&', "&amp;").replace('<', "&lt;"))),
    }
}

/// Parse XML plist data into Python objects
fn plist_parse_xml(xml: &str) -> PyResult<PyObjectRef> {
    // Find content inside <plist ...> ... </plist>
    let content = if let Some(start) = xml.find("<plist") {
        if let Some(gt) = xml[start..].find('>') {
            let after = &xml[start + gt + 1..];
            if let Some(end) = after.rfind("</plist>") {
                after[..end].trim()
            } else { after.trim() }
        } else { xml.trim() }
    } else { xml.trim() };

    let (obj, _) = plist_parse_element(content, 0)?;
    Ok(obj)
}

/// Parse a single XML element, return (value, position_after_element)
fn plist_parse_element(xml: &str, pos: usize) -> PyResult<(PyObjectRef, usize)> {
    let s = &xml[pos..];
    let s = s.trim_start();
    let new_pos = xml.len() - s.len();

    if s.is_empty() {
        return Ok((PyObject::none(), xml.len()));
    }

    if !s.starts_with('<') {
        return Err(PyException::value_error("plistlib: expected XML element"));
    }

    // Self-closing tags
    if s.starts_with("<true/>") { return Ok((PyObject::bool_val(true), new_pos + 7)); }
    if s.starts_with("<false/>") { return Ok((PyObject::bool_val(false), new_pos + 8)); }

    // Find tag name
    let gt = s.find('>').ok_or_else(|| PyException::value_error("plistlib: malformed XML"))?;
    let tag = &s[1..gt];

    if tag == "integer" {
        let end = s.find("</integer>").ok_or_else(|| PyException::value_error("plistlib: unclosed <integer>"))?;
        let val: i64 = s[gt+1..end].trim().parse().unwrap_or(0);
        return Ok((PyObject::int(val), new_pos + end + 10));
    }
    if tag == "real" {
        let end = s.find("</real>").ok_or_else(|| PyException::value_error("plistlib: unclosed <real>"))?;
        let val: f64 = s[gt+1..end].trim().parse().unwrap_or(0.0);
        return Ok((PyObject::float(val), new_pos + end + 7));
    }
    if tag == "string" {
        let end = s.find("</string>").ok_or_else(|| PyException::value_error("plistlib: unclosed <string>"))?;
        let val = s[gt+1..end].replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">");
        return Ok((PyObject::str_val(CompactString::from(val)), new_pos + end + 9));
    }
    if tag == "data" {
        let end = s.find("</data>").ok_or_else(|| PyException::value_error("plistlib: unclosed <data>"))?;
        let b64: String = s[gt+1..end].chars().filter(|c| !c.is_whitespace()).collect();
        let bytes = base64_decode(&b64);
        return Ok((PyObject::bytes(bytes), new_pos + end + 7));
    }
    if tag == "date" {
        let end = s.find("</date>").ok_or_else(|| PyException::value_error("plistlib: unclosed <date>"))?;
        let val = &s[gt+1..end];
        return Ok((PyObject::str_val(CompactString::from(val.trim())), new_pos + end + 7));
    }
    if tag == "key" {
        let end = s.find("</key>").ok_or_else(|| PyException::value_error("plistlib: unclosed <key>"))?;
        let val = s[gt+1..end].replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">");
        return Ok((PyObject::str_val(CompactString::from(val)), new_pos + end + 6));
    }
    if tag == "dict" {
        let end_tag = find_closing_tag(s, "dict")?;
        let inner = &s[gt+1..end_tag];
        let mut map = IndexMap::new();
        let mut ipos = 0;
        while ipos < inner.len() {
            let rest = inner[ipos..].trim_start();
            if rest.is_empty() || rest.starts_with("</") { break; }
            ipos = inner.len() - rest.len();
            // Parse key
            let (key_obj, next) = plist_parse_element(inner, ipos)?;
            ipos = next;
            // Parse value
            let (val_obj, next2) = plist_parse_element(inner, ipos)?;
            ipos = next2;
            let key = CompactString::from(key_obj.py_to_string());
            map.insert(HashableKey::Str(key), val_obj);
        }
        return Ok((PyObject::dict(map), new_pos + end_tag + 7));
    }
    if tag == "array" {
        let end_tag = find_closing_tag(s, "array")?;
        let inner = &s[gt+1..end_tag];
        let mut items = Vec::new();
        let mut ipos = 0;
        while ipos < inner.len() {
            let rest = inner[ipos..].trim_start();
            if rest.is_empty() || rest.starts_with("</") { break; }
            ipos = inner.len() - rest.len();
            let (item, next) = plist_parse_element(inner, ipos)?;
            items.push(item);
            ipos = next;
        }
        return Ok((PyObject::list(items), new_pos + end_tag + 8));
    }

    // Unknown tag — skip it
    if let Some(close) = s.find(&format!("</{}>", tag)) {
        let val = &s[gt+1..close];
        return Ok((PyObject::str_val(CompactString::from(val.trim())), new_pos + close + tag.len() + 3));
    }

    Ok((PyObject::none(), new_pos + gt + 1))
}

/// Find closing tag position for nested XML elements
fn find_closing_tag(s: &str, tag: &str) -> PyResult<usize> {
    let open_tag = format!("<{}", tag);
    let close_tag = format!("</{}>", tag);
    let mut depth = 0;
    let mut pos = 0;
    while pos < s.len() {
        if s[pos..].starts_with(&close_tag) {
            if depth == 1 { return Ok(pos); }
            depth -= 1;
            pos += close_tag.len();
        } else if s[pos..].starts_with(&open_tag) {
            depth += 1;
            pos += open_tag.len();
        } else {
            pos += 1;
        }
    }
    Err(PyException::value_error(format!("plistlib: unclosed <{}>", tag)))
}

/// Simple base64 decoder
fn base64_decode(input: &str) -> Vec<u8> {
    let mut result = Vec::new();
    let mut buf: u32 = 0;
    let mut bits = 0;
    for c in input.bytes() {
        let val = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => continue,
        };
        buf = (buf << 6) | val as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            result.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    result
}

