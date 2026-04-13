//! Miscellaneous stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    FxHashKeyMap, new_fx_hashkey_map,PyCell, 
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, InstanceData,
    make_module, make_builtin, check_args, check_args_min,
    FxAttrMap,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

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
                                        PyObjectPayload::NativeFunction(nf) => {
                                            let _ = (nf.func)(&[]);
                                        }
                                        PyObjectPayload::NativeClosure(nc) => {
                                            let _ = (nc.func)(&[]);
                                        }
                                        PyObjectPayload::Function(_) => {
                                            // Python function callback — use request_vm_call
                                            ferrython_core::error::request_vm_call(cb.clone(), vec![]);
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
                        let func = args[0].clone();
                        let extra_args: Vec<PyObjectRef> = args[1..].to_vec();
                        // Wrap callback+args into a NativeClosure so __exit__ can call it
                        let wrapper = PyObject::native_closure("_callback_wrapper", move |_: &[PyObjectRef]| {
                            match &func.payload {
                                PyObjectPayload::NativeFunction(nf) => (nf.func)(&extra_args),
                                PyObjectPayload::NativeClosure(nc) => (nc.func)(&extra_args),
                                PyObjectPayload::BoundMethod { method, receiver, .. } => {
                                    let mut call_args = vec![(*receiver).clone()];
                                    call_args.extend(extra_args.iter().cloned());
                                    match &method.payload {
                                        PyObjectPayload::NativeFunction(nf) => (nf.func)(&call_args),
                                        PyObjectPayload::NativeClosure(nc) => (nc.func)(&call_args),
                                        _ => {
                                            ferrython_core::error::request_vm_call((*method).clone(), call_args);
                                            Ok(PyObject::none())
                                        }
                                    }
                                }
                                _ => {
                                    let call_args = extra_args.clone();
                                    ferrython_core::error::request_vm_call(func.clone(), call_args);
                                    Ok(PyObject::none())
                                }
                            }
                        });
                        if let Some(cbs) = self_ref.get_attr("_callbacks") {
                            if let PyObjectPayload::List(items) = &cbs.payload {
                                items.write().push(wrapper);
                            }
                        }
                        Ok(PyObject::none())
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
                                PyObjectPayload::NativeFunction(nf) => (nf.func)(&[cm.clone()])?,
                                PyObjectPayload::NativeClosure(nc) => (nc.func)(&[cm.clone()])?,
                                PyObjectPayload::BuiltinBoundMethod(_) => {
                                    // Generator __enter__/__exit__ — needs VM dispatch
                                    ferrython_core::error::request_vm_call(enter, vec![cm.clone()]);
                                    PyObject::none() // placeholder; VM will execute
                                },
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

            // close() — immediately unwinds the callback stack
            attrs.insert(CompactString::from("close"), PyObject::native_closure(
                "ExitStack.close", {
                    let self_ref = self_ref.clone();
                    move |_args: &[PyObjectRef]| {
                        // Invoke __exit__ with (None, None, None)
                        if let Some(exit_fn) = self_ref.get_attr("__exit__") {
                            match &exit_fn.payload {
                                PyObjectPayload::NativeClosure(nc) => {
                                    let none = PyObject::none();
                                    (nc.func)(&[none.clone(), none.clone(), none])?;
                                }
                                _ => {}
                            }
                        }
                        Ok(PyObject::none())
                    }
                }
            ));

            // pop_all() — transfer callbacks to a new ExitStack, clearing this one
            attrs.insert(CompactString::from("pop_all"), PyObject::native_closure(
                "ExitStack.pop_all", {
                    let self_ref = self_ref.clone();
                    let cls_for_pop = exit_stack_cls_clone.clone();
                    move |_args: &[PyObjectRef]| {
                        // Get current callbacks
                        let callbacks = if let Some(cbs) = self_ref.get_attr("_callbacks") {
                            if let Ok(items) = cbs.to_list() {
                                items
                            } else {
                                vec![]
                            }
                        } else {
                            vec![]
                        };
                        // Clear our callbacks
                        if let Some(cbs) = self_ref.get_attr("_callbacks") {
                            if let PyObjectPayload::List(items) = &cbs.payload {
                                items.write().clear();
                            }
                        }
                        // Create new ExitStack instance with the transferred callbacks
                        let new_inst = PyObject::instance(cls_for_pop.clone());
                        if let PyObjectPayload::Instance(ref inst_data) = new_inst.payload {
                            let mut new_attrs = inst_data.attrs.write();
                            new_attrs.insert(CompactString::from("_callbacks"), PyObject::list(callbacks));
                        }
                        Ok(new_inst)
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

    // asynccontextmanager — same as contextmanager but for async generators
    let asynccontextmanager_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Err(PyException::type_error("asynccontextmanager requires 1 argument")); }
        Ok(args[0].clone())
    });

    // AbstractContextManager — base class with __enter__ returning self
    let acm_cls = {
        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("__enter__"), PyObject::native_function(
            "AbstractContextManager.__enter__", |args: &[PyObjectRef]| {
                if args.is_empty() { return Ok(PyObject::none()); }
                Ok(args[0].clone())
            }
        ));
        ns.insert(CompactString::from("__exit__"), PyObject::native_function(
            "AbstractContextManager.__exit__", |_args: &[PyObjectRef]| {
                Ok(PyObject::none())
            }
        ));
        PyObject::class(CompactString::from("AbstractContextManager"), vec![], ns)
    };

    // AbstractAsyncContextManager
    let aacm_cls = {
        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("__aenter__"), PyObject::native_function(
            "AbstractAsyncContextManager.__aenter__", |args: &[PyObjectRef]| {
                if args.is_empty() { return Ok(PyObject::none()); }
                Ok(args[0].clone())
            }
        ));
        ns.insert(CompactString::from("__aexit__"), PyObject::native_function(
            "AbstractAsyncContextManager.__aexit__", |_args: &[PyObjectRef]| {
                Ok(PyObject::none())
            }
        ));
        PyObject::class(CompactString::from("AbstractAsyncContextManager"), vec![], ns)
    };

    // AsyncExitStack — async version of ExitStack
    let async_exit_stack_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("AsyncExitStack"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        let callbacks: PyObjectRef = PyObject::list(vec![]);
        let cb_ref = callbacks.clone();
        attrs.insert(CompactString::from("_callbacks"), callbacks.clone());
        attrs.insert(CompactString::from("__aenter__"), PyObject::native_closure(
            "AsyncExitStack.__aenter__", {
                let inst_placeholder = PyObject::none();
                move |_args| { Ok(inst_placeholder.clone()) }
            }
        ));
        let cb_exit = cb_ref.clone();
        attrs.insert(CompactString::from("__aexit__"), PyObject::native_closure(
            "AsyncExitStack.__aexit__", move |_args| {
                // Pop and call all callbacks (simplified — sync-only for now)
                if let PyObjectPayload::List(list) = &cb_exit.payload {
                    let mut w = list.write();
                    while let Some(_cb) = w.pop() {
                        // Would need to await async callbacks
                    }
                }
                Ok(PyObject::bool_val(false))
            }
        ));
        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    // ContextDecorator: mixin class for context managers that can also be used as decorators
    let context_decorator_cls = PyObject::class(
        CompactString::from("ContextDecorator"),
        vec![],
        IndexMap::new(),
    );

    make_module("contextlib", vec![
        ("contextmanager", make_builtin(contextlib_contextmanager)),
        ("asynccontextmanager", asynccontextmanager_fn),
        ("suppress", suppress_fn),
        ("closing", closing_fn),
        ("ExitStack", exit_stack_fn),
        ("AsyncExitStack", async_exit_stack_fn),
        ("nullcontext", nullcontext_fn),
        ("redirect_stdout", redirect_stdout_fn),
        ("redirect_stderr", redirect_stderr_fn),
        ("AbstractContextManager", acm_cls.clone()),
        ("AbstractAsyncContextManager", aacm_cls),
        ("ContextDecorator", context_decorator_cls),
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
            let mut repr_flag = true;
            let mut hash_flag: Option<bool> = None;
            let mut default_val: Option<PyObjectRef> = None;
            let mut factory_val: Option<PyObjectRef> = None;

            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(kw_map) = &last.payload {
                    let r = kw_map.read();
                    if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("compare"))) {
                        compare = v.is_truthy();
                    }
                    if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("init"))) {
                        init = v.is_truthy();
                    }
                    if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("repr"))) {
                        repr_flag = v.is_truthy();
                    }
                    if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("hash"))) {
                        if !matches!(&v.payload, PyObjectPayload::None) {
                            hash_flag = Some(v.is_truthy());
                        }
                    }
                    if let Some(f) = r.get(&HashableKey::str_key(CompactString::from("default_factory"))) {
                        factory_val = Some(f.clone());
                    }
                    if let Some(d) = r.get(&HashableKey::str_key(CompactString::from("default"))) {
                        default_val = Some(d.clone());
                    }
                }
            }
            // Return a field sentinel Module with all metadata
            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("__field_compare__"), PyObject::bool_val(compare));
            attrs.insert(CompactString::from("__field_init__"), PyObject::bool_val(init));
            attrs.insert(CompactString::from("__field_repr__"), PyObject::bool_val(repr_flag));
            attrs.insert(CompactString::from("repr"), PyObject::bool_val(repr_flag));
            attrs.insert(CompactString::from("init"), PyObject::bool_val(init));
            attrs.insert(CompactString::from("compare"), PyObject::bool_val(compare));
            attrs.insert(CompactString::from("hash"), match hash_flag {
                Some(v) => PyObject::bool_val(v),
                None => PyObject::none(),
            });
            attrs.insert(CompactString::from("metadata"), PyObject::dict(IndexMap::new()));
            attrs.insert(CompactString::from("kw_only"), PyObject::bool_val(false));
            if let Some(factory) = factory_val {
                attrs.insert(CompactString::from("__field_factory__"), factory.clone());
                attrs.insert(CompactString::from("default_factory"), factory);
                attrs.insert(CompactString::from("default"), PyObject::none());
            } else if let Some(default) = default_val {
                attrs.insert(CompactString::from("__field_default__"), default.clone());
                attrs.insert(CompactString::from("default"), default);
                attrs.insert(CompactString::from("default_factory"), PyObject::none());
            } else {
                attrs.insert(CompactString::from("default"), PyObject::none());
                attrs.insert(CompactString::from("default_factory"), PyObject::none());
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
                        if let PyObjectPayload::Dict(field_dict) = &fields.payload {
                            let dict = field_dict.read();
                            let attrs = inst.attrs.read();
                            let mut map = IndexMap::new();
                            for (k, _v) in dict.iter() {
                                if let HashableKey::Str(name) = k {
                                    if let Some(v) = attrs.get(name.as_str()) {
                                        map.insert(HashableKey::str_key(name.as_ref().clone()), v.clone());
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
                        map.insert(HashableKey::str_key(k.clone()), v.clone());
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
                        if let PyObjectPayload::Dict(field_dict) = &fields.payload {
                            let dict = field_dict.read();
                            let attrs = inst.attrs.read();
                            let items: Vec<_> = dict.keys().filter_map(|k| {
                                if let HashableKey::Str(name) = k {
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
            if let Some(fields_data) = cls.get_attr("__dataclass_fields__") {
                if let PyObjectPayload::Dict(field_dict) = &fields_data.payload {
                    let dict = field_dict.read();
                    let field_objs: Vec<PyObjectRef> = dict.values().cloned().collect();
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
                let mut new_attrs: IndexMap<CompactString, PyObjectRef> = inst.attrs.read().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                // Apply kwargs overrides
                if args.len() > 1 {
                    if let PyObjectPayload::Dict(kw_map) = &args[1].payload {
                        for (k, v) in kw_map.read().iter() {
                            if let HashableKey::Str(name) = k {
                                new_attrs.insert(name.as_ref().clone(), v.clone());
                            }
                        }
                    }
                }
                Ok(PyObject::instance_with_attrs(cls, new_attrs))
            } else {
                Err(PyException::type_error("replace() argument must be a dataclass instance"))
            }
        })),
        ("is_dataclass", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::bool_val(false)); }
            let obj = &args[0];
            match &obj.payload {
                PyObjectPayload::Class(_) => {
                    Ok(PyObject::bool_val(obj.get_attr("__dataclass_fields__").is_some()))
                }
                PyObjectPayload::Instance(inst) => {
                    Ok(PyObject::bool_val(inst.class.get_attr("__dataclass_fields__").is_some()))
                }
                _ => Ok(PyObject::bool_val(false)),
            }
        })),
        ("make_dataclass", make_builtin(|args| {
            // make_dataclass(cls_name, fields, *, bases=()) -> class
            if args.is_empty() { return Err(PyException::type_error("make_dataclass requires cls_name")); }
            let cls_name = args[0].py_to_string();
            let field_list = if args.len() > 1 { args[1].to_list()? } else { vec![] };
            let mut ns = IndexMap::new();
            let mut annotations = IndexMap::new();
            // Parse field specs: can be "name", ("name", type), or ("name", type, field(...))
            for f in &field_list {
                let items = f.to_list().unwrap_or_else(|_| vec![f.clone()]);
                let name = items.first().map(|v| v.py_to_string()).unwrap_or_default();
                if name.is_empty() { continue; }
                annotations.insert(
                    HashableKey::str_key(CompactString::from(name.as_str())),
                    if items.len() > 1 { items[1].clone() } else { PyObject::none() },
                );
                // If a field(...) default is provided as 3rd element, set as class attr
                if items.len() > 2 {
                    ns.insert(CompactString::from(name.as_str()), items[2].clone());
                }
            }
            ns.insert(CompactString::from("__annotations__"), PyObject::dict(annotations));
            let cls = PyObject::class(CompactString::from(cls_name.as_str()), vec![], ns);
            // Apply the dataclass transform to generate __init__, __repr__, __eq__
            dataclass_apply(&cls, true, false, false, true, false, false)
        })),
        ("FrozenInstanceError", PyObject::exception_type(ferrython_core::error::ExceptionKind::AttributeError)),
        ("InitVar", make_builtin(|args: &[PyObjectRef]| {
            // InitVar acts as a type marker for dataclass fields
            let cls = PyObject::class(CompactString::from("InitVar"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut attrs = d.attrs.write();
                attrs.insert(CompactString::from("__initvar__"), PyObject::bool_val(true));
                if !args.is_empty() {
                    attrs.insert(CompactString::from("type"), args[0].clone());
                }
            }
            Ok(inst)
        })),
    ])
}

fn dataclass_decorator(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // @dataclass() — called with empty parens, return decorator with defaults
    if args.is_empty() {
        return Ok(PyObject::native_closure("dataclass", move |args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("dataclass requires 1 argument")); }
            dataclass_apply(&args[0], true, false, false, true, false, false)
        }));
    }
    let cls = &args[0];
    
    // If called as @dataclass(eq=True, ...) the first arg is kwargs dict, not a class.
    if !matches!(&cls.payload, PyObjectPayload::Class(_)) {
        let mut eq = true;
        let mut order = false;
        let mut frozen = false;
        let mut repr = true;
        let mut unsafe_hash = false;
        let mut slots = false;
        if let PyObjectPayload::Dict(map) = &cls.payload {
            let m = map.read();
            if let Some(v) = m.get(&HashableKey::str_key(CompactString::from("eq"))) {
                eq = v.is_truthy();
            }
            if let Some(v) = m.get(&HashableKey::str_key(CompactString::from("order"))) {
                order = v.is_truthy();
            }
            if let Some(v) = m.get(&HashableKey::str_key(CompactString::from("frozen"))) {
                frozen = v.is_truthy();
            }
            if let Some(v) = m.get(&HashableKey::str_key(CompactString::from("repr"))) {
                repr = v.is_truthy();
            }
            if let Some(v) = m.get(&HashableKey::str_key(CompactString::from("unsafe_hash"))) {
                unsafe_hash = v.is_truthy();
            }
            if let Some(v) = m.get(&HashableKey::str_key(CompactString::from("slots"))) {
                slots = v.is_truthy();
            }
        }
        return Ok(PyObject::native_closure("dataclass", move |args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("dataclass requires 1 argument")); }
            dataclass_apply(&args[0], eq, order, frozen, repr, unsafe_hash, slots)
        }));
    }
    
    dataclass_apply(cls, true, false, false, true, false, false)
}

/// Call a default_factory callable or clone a static default value.
/// Handles NativeFunction, NativeClosure, BuiltinType (dict/list/set/tuple/frozenset),
/// Function (Python lambda/def), and Class (user-defined types).
fn call_factory_or_clone(default: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &default.payload {
        PyObjectPayload::NativeFunction(nf) => (nf.func)(&[]),
        PyObjectPayload::NativeClosure(nc) => (nc.func)(&[]),
        PyObjectPayload::BuiltinType(name) => {
            // Common builtin types: dict() → {}, list() → [], set() → set(), etc.
            match name.as_str() {
                "dict" => Ok(PyObject::dict(IndexMap::new())),
                "list" => Ok(PyObject::list(vec![])),
                "set" => Ok(PyObject::set(IndexMap::new())),
                "tuple" => Ok(PyObject::tuple(vec![])),
                "frozenset" => Ok(PyObject::frozenset(IndexMap::new())),
                "str" => Ok(PyObject::str_val(CompactString::new(""))),
                "int" => Ok(PyObject::int(0)),
                "float" => Ok(PyObject::float(0.0)),
                "bool" => Ok(PyObject::bool_val(false)),
                "bytes" => Ok(PyObject::bytes(vec![])),
                "bytearray" => Ok(PyObject::bytearray(vec![])),
                _ => Ok(default.clone()),
            }
        }
        // BuiltinFunction holds a name string, not callable — skip
        _ => Ok(default.clone()),
    }
}

fn dataclass_apply(cls: &PyObjectRef, eq: bool, order: bool, frozen: bool, repr: bool, unsafe_hash: bool, slots: bool) -> PyResult<PyObjectRef> {
    
    // Get annotations to discover fields — walk MRO for inherited dataclass fields
    let mut field_names: Vec<CompactString> = Vec::new();
    let mut field_defaults: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    let mut field_types: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    let mut compare_fields: Vec<CompactString> = Vec::new();
    let mut init_fields: Vec<CompactString> = Vec::new();
    let mut repr_fields: Vec<CompactString> = Vec::new();
    
    if let PyObjectPayload::Class(cd) = &cls.payload {
        // Collect fields from base classes first (MRO order), then own class
        let mut all_classes: Vec<PyObjectRef> = cd.bases.iter().rev().cloned().collect();
        all_classes.push(cls.clone());
        
        for base_cls in &all_classes {
            if let PyObjectPayload::Class(bcd) = &base_cls.payload {
                let ns = bcd.namespace.read();
                if let Some(annotations) = ns.get("__annotations__") {
                    if let PyObjectPayload::Dict(ann_map) = &annotations.payload {
                        for (k, v) in ann_map.read().iter() {
                            if let HashableKey::Str(name) = k {
                                if !field_names.contains(name) {
                                    field_names.push(name.as_ref().clone());
                                }
                                field_types.insert(name.as_ref().clone(), v.clone());
                                let mut compare = true;
                                let mut init = true;
                                let mut field_repr = true;
                                
                                if let Some(default) = ns.get(name.as_str()) {
                                    if let PyObjectPayload::Module(md) = &default.payload {
                                        let mod_attrs = md.attrs.read();
                                        if let Some(cmp_flag) = mod_attrs.get("__field_compare__") {
                                            compare = cmp_flag.is_truthy();
                                        }
                                        if let Some(init_flag) = mod_attrs.get("__field_init__") {
                                            init = init_flag.is_truthy();
                                        }
                                        if let Some(repr_flag) = mod_attrs.get("__field_repr__") {
                                            field_repr = repr_flag.is_truthy();
                                        }
                                        if let Some(factory) = mod_attrs.get("__field_factory__") {
                                            field_defaults.insert(name.as_ref().clone(), factory.clone());
                                        } else if let Some(default_val) = mod_attrs.get("__field_default__") {
                                            field_defaults.insert(name.as_ref().clone(), default_val.clone());
                                        }
                                    } else {
                                        field_defaults.insert(name.as_ref().clone(), default.clone());
                                    }
                                }
                                if compare { compare_fields.push(name.as_ref().clone()); }
                                if init { init_fields.push(name.as_ref().clone()); }
                                if field_repr { repr_fields.push(name.as_ref().clone()); }
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Store __dataclass_fields__ as dict mapping field name → Field-like object
    // CPython stores Field objects; we use Module objects with the same key attributes.
    let mut fields_dict: FxHashKeyMap = new_fx_hashkey_map();
    for name in &field_names {
        let has_default = field_defaults.contains_key(name.as_str());
        let default_val = field_defaults.get(name.as_str()).cloned().unwrap_or_else(PyObject::none);
        let init_flag = init_fields.contains(name);
        let compare_flag = compare_fields.contains(name);
        let repr_flag = repr_fields.contains(name);
        let type_val = field_types.get(name).cloned().unwrap_or_else(PyObject::none);
        // Create a Field-like object with standard dataclass Field attributes
        let mut field_attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
        field_attrs.insert(CompactString::from("name"), PyObject::str_val(name.clone()));
        field_attrs.insert(CompactString::from("type"), type_val);
        field_attrs.insert(CompactString::from("default"), default_val.clone());
        field_attrs.insert(CompactString::from("default_factory"), PyObject::none());
        field_attrs.insert(CompactString::from("__has_default__"), PyObject::bool_val(has_default));
        field_attrs.insert(CompactString::from("init"), PyObject::bool_val(init_flag));
        field_attrs.insert(CompactString::from("repr"), PyObject::bool_val(repr_flag));
        field_attrs.insert(CompactString::from("compare"), PyObject::bool_val(compare_flag));
        field_attrs.insert(CompactString::from("hash"), PyObject::none());
        field_attrs.insert(CompactString::from("metadata"), PyObject::dict(IndexMap::new()));
        field_attrs.insert(CompactString::from("kw_only"), PyObject::bool_val(false));
        field_attrs.insert(CompactString::from("_field_type"), PyObject::str_val(CompactString::from("_FIELD")));
        let field_obj = PyObject::module_with_attrs(CompactString::from("Field"), field_attrs);
        fields_dict.insert(
            HashableKey::str_key(name.clone()),
            field_obj,
        );
    }
    
    if let PyObjectPayload::Class(cd) = &cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(CompactString::from("__dataclass_fields__"), PyObject::dict(fields_dict));
        ns.insert(CompactString::from("__dataclass__"), PyObject::bool_val(true));

        // slots=True: add __slots__ tuple and restrict attribute assignment
        if slots {
            let slot_names: Vec<PyObjectRef> = field_names.iter()
                .map(|n| PyObject::str_val(n.clone()))
                .collect();
            ns.insert(CompactString::from("__slots__"), PyObject::tuple(slot_names));
            // Add __setattr__ that restricts to declared slots + dataclass internals
            let allowed: Vec<CompactString> = field_names.clone();
            ns.insert(CompactString::from("__setattr__"), PyObject::native_closure("__setattr__", move |args: &[PyObjectRef]| {
                if args.len() < 3 {
                    return Err(PyException::type_error("__setattr__ requires 3 arguments"));
                }
                let attr_name = args[1].py_to_string();
                if !allowed.iter().any(|f| f.as_str() == attr_name) && !attr_name.starts_with("__") {
                    return Err(PyException::attribute_error(
                        format!("'{}' object has no attribute '{}'", "object", attr_name)
                    ));
                }
                if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    inst.attrs.write().insert(CompactString::from(attr_name), args[2].clone());
                }
                Ok(PyObject::none())
            }));
        }

        // Generate __init__ for all dataclasses (frozen and non-frozen),
        // but only if the class doesn't already define __init__ (CPython _set_new_attribute behavior)
        if !ns.contains_key("__init__") {
            let init_field_names = init_fields.clone();
            let init_field_defaults = field_defaults.clone();
            let cls_for_init = cls.clone();
            ns.insert(CompactString::from("__init__"), PyObject::native_closure("__init__", move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Err(PyException::type_error("__init__ requires self"));
                }
                let self_obj = &args[0];
                // Detect trailing kwargs dict (VM packs kwargs as last arg for NativeClosure).
                // Only check when arg count doesn't match field count exactly —
                // if we have exactly the right number of positional args, they ARE positional
                // (avoids treating a user dict arg like {"a":1} as kwargs).
                let n_args_excl_self = args.len() - 1;
                let trailing_kwargs: Option<FxHashKeyMap> =
                    if n_args_excl_self != init_field_names.len() && args.len() >= 2 {
                        if let PyObjectPayload::Dict(map) = &args[args.len() - 1].payload {
                            Some(map.read().clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                let pos_end = if trailing_kwargs.is_some() { args.len() - 1 } else { args.len() };
                if let PyObjectPayload::Instance(inst) = &self_obj.payload {
                    let mut attrs = inst.attrs.write();
                    let mut pos = 1; // skip self
                    for fname in &init_field_names {
                        // Try positional arg first, then kwargs, then defaults
                        let value = if pos < pos_end {
                            args[pos].clone()
                        } else if let Some(ref kw) = trailing_kwargs {
                            if let Some(v) = kw.get(&HashableKey::str_key(fname.clone())) {
                                v.clone()
                            } else if let Some(default) = init_field_defaults.get(fname.as_str()) {
                                call_factory_or_clone(default)?
                            } else {
                                return Err(PyException::type_error(format!(
                                    "__init__() missing required argument: '{}'", fname
                                )));
                            }
                        } else if let Some(default) = init_field_defaults.get(fname.as_str()) {
                            call_factory_or_clone(default)?
                        } else {
                            return Err(PyException::type_error(format!(
                                "__init__() missing required argument: '{}'", fname
                            )));
                        };
                        attrs.insert(fname.clone(), value);
                        pos += 1;
                    }
                }
                // Call __post_init__ if defined (CPython does this in generated __init__)
                if let PyObjectPayload::Class(cd) = &cls_for_init.payload {
                    if let Some(post_init) = cd.namespace.read().get("__post_init__") {
                        ferrython_core::error::request_vm_call(
                            post_init.clone(),
                            vec![self_obj.clone()],
                        );
                    }
                }
                Ok(PyObject::none())
            }));
        }

        // Generate __setattr__ and __delattr__ for frozen=True
        if frozen {
            ns.insert(CompactString::from("__dataclass_frozen__"), PyObject::bool_val(true));

            // Raise FrozenInstanceError on frozen field assignment, allow other attrs
            let frozen_field_names: Vec<CompactString> = field_names.clone();
            ns.insert(CompactString::from("__setattr__"), PyObject::native_closure("__setattr__", move |args: &[PyObjectRef]| {
                // args: self, name, value
                if args.len() < 3 {
                    return Err(PyException::type_error("__setattr__ requires 3 arguments"));
                }
                let attr_name = args[1].py_to_string();
                if frozen_field_names.iter().any(|f| f.as_str() == attr_name) {
                    return Err(PyException::attribute_error(
                        format!("cannot assign to field '{}'", attr_name)
                    ));
                }
                // Allow non-field attributes (e.g., subclass __init__ setting new attrs)
                if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    inst.attrs.write().insert(CompactString::from(attr_name), args[2].clone());
                }
                Ok(PyObject::none())
            }));
            let frozen_del_names: Vec<CompactString> = field_names.clone();
            ns.insert(CompactString::from("__delattr__"), PyObject::native_closure("__delattr__", move |args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Err(PyException::type_error("__delattr__ requires 2 arguments"));
                }
                let attr_name = args[1].py_to_string();
                if frozen_del_names.iter().any(|f| f.as_str() == attr_name) {
                    return Err(PyException::attribute_error(
                        format!("cannot delete field '{}'", attr_name)
                    ));
                }
                if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    inst.attrs.write().swap_remove(attr_name.as_str());
                }
                Ok(PyObject::none())
            }));
        }

        // Generate __repr__ if repr=True (default)
        if repr {
            let fields_for_repr = repr_fields.clone();
            let cls_ref = cls.clone();
            ns.insert(CompactString::from("__repr__"), PyObject::native_closure("__repr__", move |args: &[PyObjectRef]| {
                check_args("__repr__", args, 1)?;
                let cls_name = if let PyObjectPayload::Class(cd) = &cls_ref.payload {
                    cd.name.clone()
                } else {
                    CompactString::from("???")
                };
                let mut parts = Vec::new();
                if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    let attrs = inst.attrs.read();
                    for f in &fields_for_repr {
                        let val = attrs.get(f.as_str()).cloned().unwrap_or_else(PyObject::none);
                        let val_repr = val.repr();
                        parts.push(format!("{}={}", f, val_repr));
                    }
                }
                Ok(PyObject::str_val(CompactString::from(
                    format!("{}({})", cls_name, parts.join(", "))
                )))
            }));
        }

        // Generate __eq__ if eq=True (default)
        if eq {
            let fields_for_eq = compare_fields.clone();
            ns.insert(CompactString::from("__eq__"), PyObject::native_closure("__eq__", move |args: &[PyObjectRef]| {
                check_args("__eq__", args, 2)?;
                let (a, b) = (&args[0], &args[1]);
                // Must be same type
                if !same_class(a, b) {
                    return Ok(PyObject::not_implemented());
                }
                let tup_a = extract_compare_tuple(a, &fields_for_eq);
                let tup_b = extract_compare_tuple(b, &fields_for_eq);
                tup_a.compare(&tup_b, ferrython_core::object::CompareOp::Eq)
            }));

            let fields_for_ne = compare_fields.clone();
            ns.insert(CompactString::from("__ne__"), PyObject::native_closure("__ne__", move |args: &[PyObjectRef]| {
                check_args("__ne__", args, 2)?;
                let (a, b) = (&args[0], &args[1]);
                if !same_class(a, b) {
                    return Ok(PyObject::not_implemented());
                }
                let tup_a = extract_compare_tuple(a, &fields_for_ne);
                let tup_b = extract_compare_tuple(b, &fields_for_ne);
                tup_a.compare(&tup_b, ferrython_core::object::CompareOp::Ne)
            }));
        }

        // Generate __hash__
        // CPython: if eq=True and frozen=True, generate __hash__
        //          if eq=True and frozen=False, set __hash__ = None (unhashable)
        //          if unsafe_hash=True, always generate __hash__
        if unsafe_hash || (eq && frozen) {
            let fields_for_hash = compare_fields.clone();
            ns.insert(CompactString::from("__hash__"), PyObject::native_closure("__hash__", move |args: &[PyObjectRef]| {
                check_args("__hash__", args, 1)?;
                use std::hash::{Hash, Hasher};
                use std::collections::hash_map::DefaultHasher;
                if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    let attrs = inst.attrs.read();
                    let vals: Vec<PyObjectRef> = fields_for_hash.iter()
                        .map(|f| attrs.get(f.as_str()).cloned().unwrap_or_else(PyObject::none))
                        .collect();
                    let tup = PyObject::tuple(vals);
                    let hk = tup.to_hashable_key()?;
                    let mut hasher = DefaultHasher::new();
                    hk.hash(&mut hasher);
                    Ok(PyObject::int(hasher.finish() as i64))
                } else {
                    Ok(PyObject::int(0))
                }
            }));
        } else if eq {
            // eq=True, frozen=False → unhashable (like CPython)
            ns.insert(CompactString::from("__hash__"), PyObject::none());
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

    // Invalidate vtable so the inline class instantiation uses namespace lookup.
    // The decorator added __init__ (and possibly __eq__/__repr__/etc.) AFTER class creation,
    // so the vtable is stale and must be cleared.
    if let PyObjectPayload::Class(cd) = &cls.payload {
        cd.method_vtable.write().clear();
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

/// Check if two instances share the same class (by Arc pointer identity).
fn same_class(a: &PyObjectRef, b: &PyObjectRef) -> bool {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Instance(ia), PyObjectPayload::Instance(ib)) => {
            PyObjectRef::ptr_eq(&ia.class, &ib.class)
        }
        _ => false,
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
    let mut memo = std::collections::HashMap::new();
    deep_copy_with_memo(&args[0], &mut memo)
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
            Ok(PyObject::wrap(PyObjectPayload::Instance(Box::new(InstanceData {
                class: inst.class.clone(),
                attrs: Rc::new(PyCell::new(inst.attrs.read().clone())),
                is_special: true, dict_storage: inst.dict_storage.as_ref().map(|ds| Rc::new(PyCell::new(ds.read().clone()))),
                class_flags: InstanceData::compute_flags(&inst.class),
            }))))
        }
        _ => Ok(obj.clone()),
    }
}

fn deep_copy(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    let mut memo = std::collections::HashMap::new();
    deep_copy_with_memo(obj, &mut memo)
}

fn deep_copy_with_memo(obj: &PyObjectRef, memo: &mut std::collections::HashMap<usize, PyObjectRef>) -> PyResult<PyObjectRef> {
    // Check memo for already-copied objects (handles circular references)
    let ptr = PyObjectRef::as_ptr(obj) as usize;
    if let Some(existing) = memo.get(&ptr) {
        return Ok(existing.clone());
    }

    match &obj.payload {
        PyObjectPayload::None | PyObjectPayload::Bool(_) | PyObjectPayload::Int(_)
        | PyObjectPayload::Float(_) | PyObjectPayload::Str(_) | PyObjectPayload::Bytes(_)
        | PyObjectPayload::FrozenSet(_) => Ok(obj.clone()),
        PyObjectPayload::Tuple(items) => {
            let new_items: Result<Vec<_>, _> = items.iter().map(|x| deep_copy_with_memo(x, memo)).collect();
            let result = PyObject::tuple(new_items?);
            memo.insert(ptr, result.clone());
            Ok(result)
        }
        PyObjectPayload::List(items) => {
            // Pre-insert empty list to handle circular refs
            let result = PyObject::list(vec![]);
            memo.insert(ptr, result.clone());
            let new_items: Result<Vec<_>, _> = items.read().iter().map(|x| deep_copy_with_memo(x, memo)).collect();
            if let PyObjectPayload::List(new_list) = &result.payload {
                *new_list.write() = new_items?;
            }
            Ok(result)
        }
        PyObjectPayload::Dict(map) => {
            let result = PyObject::dict(IndexMap::new());
            memo.insert(ptr, result.clone());
            let mut new_map = new_fx_hashkey_map();
            for (k, v) in map.read().iter() {
                new_map.insert(k.clone(), deep_copy_with_memo(v, memo)?);
            }
            if let PyObjectPayload::Dict(new_dict) = &result.payload {
                *new_dict.write() = new_map;
            }
            Ok(result)
        }
        PyObjectPayload::Set(set) => {
            let mut new_set = new_fx_hashkey_map();
            for (k, v) in set.read().iter() {
                new_set.insert(k.clone(), deep_copy_with_memo(v, memo)?);
            }
            let result = PyObject::set(new_set);
            memo.insert(ptr, result.clone());
            Ok(result)
        }
        PyObjectPayload::Instance(inst) => {
            // Pre-insert placeholder instance to handle circular refs
            let result = PyObject::instance_with_attrs(inst.class.clone(), IndexMap::new());
            memo.insert(ptr, result.clone());
            let mut new_attrs = FxAttrMap::default();
            for (k, v) in inst.attrs.read().iter() {
                new_attrs.insert(k.clone(), deep_copy_with_memo(v, memo)?);
            }
            if let PyObjectPayload::Instance(new_inst) = &result.payload {
                *new_inst.attrs.write() = new_attrs;
            }
            Ok(result)
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
    // Create a shared ContextVar class so isinstance() works
    let context_var_class = PyObject::class(CompactString::from("ContextVar"), vec![], IndexMap::new());
    let cv_cls = context_var_class.clone();

    // __new__ receives (cls, name, ...) — create a properly-typed instance
    let cv_new = PyObject::native_closure("ContextVar.__new__", move |args: &[PyObjectRef]| {
        // args[0] = cls, args[1..] = user args
        let user_args = if args.len() > 1 { &args[1..] } else { &[] };
        if user_args.is_empty() { return Err(PyException::type_error("ContextVar() requires a name")); }
        let name = user_args[0].py_to_string();
        let default_val = if user_args.len() > 1 {
            if let PyObjectPayload::Dict(kw) = &user_args[user_args.len()-1].payload {
                kw.read().get(&HashableKey::str_key(CompactString::from("default")))
                    .cloned()
            } else {
                Some(user_args[1].clone())
            }
        } else { None };

        let inst = PyObject::instance(cv_cls.clone());
        if let PyObjectPayload::Instance(ref data) = inst.payload {
            let mut attrs = data.attrs.write();
            attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(&name)));
            let value: Rc<PyCell<Option<PyObjectRef>>> = Rc::new(PyCell::new(default_val.clone()));

            let v = value.clone();
            attrs.insert(CompactString::from("get"), PyObject::native_closure("ContextVar.get", move |a: &[PyObjectRef]| {
                if let Some(val) = v.read().as_ref() {
                    Ok(val.clone())
                } else if !a.is_empty() {
                    Ok(a[0].clone())
                } else {
                    Err(PyException::runtime_error("ContextVar has no value"))
                }
            }));

            let v = value.clone();
            let name_clone = name.clone();
            attrs.insert(CompactString::from("set"), PyObject::native_closure("ContextVar.set", move |a: &[PyObjectRef]| {
                if a.is_empty() { return Err(PyException::type_error("set() requires a value")); }
                let old = v.read().clone();
                *v.write() = Some(a[0].clone());
                let v_restore = v.clone();
                let token_cls = PyObject::class(CompactString::from("Token"), vec![], IndexMap::new());
                let token = PyObject::instance(token_cls);
                if let PyObjectPayload::Instance(ref td) = token.payload {
                    let mut ta = td.attrs.write();
                    ta.insert(CompactString::from("old_value"), old.clone().unwrap_or_else(PyObject::none));
                    ta.insert(CompactString::from("var"), PyObject::str_val(CompactString::from(name_clone.as_str())));
                    let old_clone = old;
                    ta.insert(CompactString::from("_restore"), PyObject::native_closure("Token._restore", move |_| {
                        *v_restore.write() = old_clone.clone();
                        Ok(PyObject::none())
                    }));
                }
                Ok(token)
            }));

            let v = value.clone();
            let default_clone = default_val.clone();
            attrs.insert(CompactString::from("reset"), PyObject::native_closure("ContextVar.reset", move |a: &[PyObjectRef]| {
                if a.is_empty() { return Err(PyException::type_error("reset() requires a token")); }
                let token = &a[0];
                if let Some(restore_fn) = token.get_attr("_restore") {
                    if let PyObjectPayload::NativeClosure(nc) = &restore_fn.payload {
                        return (nc.func)(&[]);
                    }
                }
                if let Some(old) = token.get_attr("old_value") {
                    if matches!(&old.payload, PyObjectPayload::None) {
                        *v.write() = default_clone.clone();
                    } else {
                        *v.write() = Some(old);
                    }
                }
                Ok(PyObject::none())
            }));
        }
        Ok(inst)
    });

    if let PyObjectPayload::Class(ref cd) = context_var_class.payload {
        cd.namespace.write().insert(CompactString::from("__new__"), cv_new);
    }

    make_module("contextvars", vec![
        ("ContextVar", context_var_class),
        ("Context", make_builtin(|_| {
            let cls = PyObject::class(CompactString::from("Context"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                attrs.insert(CompactString::from("run"), make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() { return Err(PyException::type_error("Context.run() requires a callable")); }
                    let callable = &args[0];
                    let call_args: Vec<PyObjectRef> = args[1..].to_vec();
                    match &callable.payload {
                        PyObjectPayload::NativeFunction(nf) => (nf.func)(&call_args),
                        PyObjectPayload::NativeClosure(nc) => (nc.func)(&call_args),
                        _ => {
                            ferrython_core::error::request_vm_call(callable.clone(), call_args);
                            Ok(PyObject::none())
                        }
                    }
                }));
                attrs.insert(CompactString::from("copy"), make_builtin(|_| {
                    let cls = PyObject::class(CompactString::from("Context"), vec![], IndexMap::new());
                    let copy_inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref d) = copy_inst.payload {
                        let mut a = d.attrs.write();
                        a.insert(CompactString::from("__len__"), make_builtin(|_| Ok(PyObject::int(0))));
                    }
                    Ok(copy_inst)
                }));
            }
            Ok(inst)
        })),
        ("copy_context", make_builtin(|_| {
            let cls = PyObject::class(CompactString::from("Context"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                attrs.insert(CompactString::from("run"), make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() { return Err(PyException::type_error("Context.run() requires a callable")); }
                    let callable = &args[0];
                    let call_args: Vec<PyObjectRef> = args[1..].to_vec();
                    match &callable.payload {
                        PyObjectPayload::NativeFunction(nf) => (nf.func)(&call_args),
                        PyObjectPayload::NativeClosure(nc) => (nc.func)(&call_args),
                        _ => {
                            ferrython_core::error::request_vm_call(callable.clone(), call_args);
                            Ok(PyObject::none())
                        }
                    }
                }));
                attrs.insert(CompactString::from("copy"), make_builtin(|_| {
                    let cls = PyObject::class(CompactString::from("Context"), vec![], IndexMap::new());
                    let copy_inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref d) = copy_inst.payload {
                        let mut a = d.attrs.write();
                        a.insert(CompactString::from("__len__"), make_builtin(|_| Ok(PyObject::int(0))));
                    }
                    Ok(copy_inst)
                }));
                attrs.insert(CompactString::from("__len__"), make_builtin(|_| Ok(PyObject::int(0))));
            }
            Ok(inst)
        })),
        ("Token", PyObject::class(CompactString::from("Token"), vec![], IndexMap::new())),
    ])
}

// ── mimetypes module ──

fn ext_to_mime(ext: &str) -> Option<&'static str> {
    match ext.to_lowercase().as_str() {
        // Text
        "html" | "htm" => Some("text/html"), "xhtml" => Some("application/xhtml+xml"),
        "css" => Some("text/css"), "csv" => Some("text/csv"),
        "txt" | "text" | "log" => Some("text/plain"), "rtf" => Some("application/rtf"),
        "md" | "markdown" => Some("text/markdown"), "rst" => Some("text/x-rst"),
        "ics" => Some("text/calendar"), "vcf" => Some("text/vcard"),
        "tsv" => Some("text/tab-separated-values"),
        // Programming
        "js" | "mjs" => Some("application/javascript"), "ts" => Some("application/typescript"),
        "json" => Some("application/json"), "jsonld" => Some("application/ld+json"),
        "xml" => Some("application/xml"), "xsl" | "xslt" => Some("application/xslt+xml"),
        "dtd" => Some("application/xml-dtd"),
        "py" => Some("text/x-python"), "rb" => Some("text/x-ruby"),
        "java" => Some("text/x-java-source"), "c" | "h" => Some("text/x-c"),
        "cpp" | "cxx" | "cc" | "hpp" => Some("text/x-c++src"),
        "rs" => Some("text/x-rust"), "go" => Some("text/x-go"),
        "sh" | "bash" => Some("application/x-sh"), "bat" | "cmd" => Some("application/x-msdos-program"),
        "sql" => Some("application/sql"), "php" => Some("application/x-httpd-php"),
        "pl" | "pm" => Some("text/x-perl"), "lua" => Some("text/x-lua"),
        // Images
        "jpg" | "jpeg" | "jpe" => Some("image/jpeg"), "png" => Some("image/png"),
        "gif" => Some("image/gif"), "bmp" => Some("image/bmp"),
        "svg" | "svgz" => Some("image/svg+xml"), "ico" => Some("image/x-icon"),
        "webp" => Some("image/webp"), "tiff" | "tif" => Some("image/tiff"),
        "avif" => Some("image/avif"), "heic" | "heif" => Some("image/heif"),
        "psd" => Some("image/vnd.adobe.photoshop"),
        // Audio
        "mp3" => Some("audio/mpeg"), "wav" => Some("audio/wav"),
        "ogg" | "oga" => Some("audio/ogg"), "flac" => Some("audio/flac"),
        "aac" => Some("audio/aac"), "m4a" => Some("audio/mp4"),
        "wma" => Some("audio/x-ms-wma"), "mid" | "midi" => Some("audio/midi"),
        "opus" => Some("audio/opus"), "aiff" | "aif" => Some("audio/aiff"),
        // Video
        "mp4" | "m4v" => Some("video/mp4"), "webm" => Some("video/webm"),
        "ogv" => Some("video/ogg"), "avi" => Some("video/x-msvideo"),
        "mov" => Some("video/quicktime"), "wmv" => Some("video/x-ms-wmv"),
        "flv" => Some("video/x-flv"), "mkv" => Some("video/x-matroska"),
        "mpeg" | "mpg" => Some("video/mpeg"), "3gp" => Some("video/3gpp"),
        // Archives
        "zip" => Some("application/zip"), "gz" | "gzip" => Some("application/gzip"),
        "tar" => Some("application/x-tar"), "bz2" => Some("application/x-bzip2"),
        "xz" => Some("application/x-xz"), "7z" => Some("application/x-7z-compressed"),
        "rar" => Some("application/vnd.rar"), "zst" => Some("application/zstd"),
        "lz" => Some("application/x-lzip"), "lz4" => Some("application/x-lz4"),
        // Documents
        "pdf" => Some("application/pdf"),
        "doc" => Some("application/msword"),
        "docx" => Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        "xls" => Some("application/vnd.ms-excel"),
        "xlsx" => Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        "ppt" => Some("application/vnd.ms-powerpoint"),
        "pptx" => Some("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
        "odt" => Some("application/vnd.oasis.opendocument.text"),
        "ods" => Some("application/vnd.oasis.opendocument.spreadsheet"),
        "odp" => Some("application/vnd.oasis.opendocument.presentation"),
        "epub" => Some("application/epub+zip"),
        // Fonts
        "woff" => Some("font/woff"), "woff2" => Some("font/woff2"),
        "ttf" => Some("font/ttf"), "otf" => Some("font/otf"), "eot" => Some("application/vnd.ms-fontobject"),
        // Data formats
        "yaml" | "yml" => Some("application/x-yaml"), "toml" => Some("application/toml"),
        "ini" | "cfg" => Some("text/plain"), "env" => Some("text/plain"),
        "wasm" => Some("application/wasm"), "bin" => Some("application/octet-stream"),
        "exe" => Some("application/x-msdownload"), "dll" | "so" | "dylib" => Some("application/octet-stream"),
        // Package formats
        "deb" => Some("application/x-debian-package"), "rpm" => Some("application/x-rpm"),
        "dmg" => Some("application/x-apple-diskimage"), "iso" => Some("application/x-iso9660-image"),
        "whl" => Some("application/zip"), "egg" => Some("application/zip"),
        _ => None,
    }
}

fn mime_to_ext(mime: &str) -> Option<&'static str> {
    match mime {
        "text/html" => Some(".html"), "text/css" => Some(".css"),
        "text/csv" => Some(".csv"), "text/plain" => Some(".txt"),
        "text/markdown" => Some(".md"), "text/calendar" => Some(".ics"),
        "text/x-python" => Some(".py"), "text/x-rust" => Some(".rs"),
        "application/javascript" => Some(".js"), "application/json" => Some(".json"),
        "application/xml" => Some(".xml"), "application/pdf" => Some(".pdf"),
        "application/zip" => Some(".zip"), "application/gzip" => Some(".gz"),
        "application/x-tar" => Some(".tar"), "application/x-bzip2" => Some(".bz2"),
        "application/x-xz" => Some(".xz"), "application/x-7z-compressed" => Some(".7z"),
        "application/rtf" => Some(".rtf"), "application/sql" => Some(".sql"),
        "application/wasm" => Some(".wasm"), "application/x-sh" => Some(".sh"),
        "application/octet-stream" => Some(".bin"),
        "application/msword" => Some(".doc"),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => Some(".docx"),
        "application/vnd.ms-excel" => Some(".xls"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => Some(".xlsx"),
        "application/epub+zip" => Some(".epub"),
        "image/jpeg" => Some(".jpg"), "image/png" => Some(".png"),
        "image/gif" => Some(".gif"), "image/bmp" => Some(".bmp"),
        "image/svg+xml" => Some(".svg"), "image/webp" => Some(".webp"),
        "image/x-icon" => Some(".ico"), "image/tiff" => Some(".tiff"),
        "image/avif" => Some(".avif"),
        "audio/mpeg" => Some(".mp3"), "audio/wav" => Some(".wav"),
        "audio/ogg" => Some(".ogg"), "audio/flac" => Some(".flac"),
        "audio/aac" => Some(".aac"), "audio/mp4" => Some(".m4a"),
        "audio/opus" => Some(".opus"),
        "video/mp4" => Some(".mp4"), "video/webm" => Some(".webm"),
        "video/ogg" => Some(".ogv"), "video/quicktime" => Some(".mov"),
        "video/x-msvideo" => Some(".avi"), "video/x-matroska" => Some(".mkv"),
        "font/woff" => Some(".woff"), "font/woff2" => Some(".woff2"),
        "font/ttf" => Some(".ttf"), "font/otf" => Some(".otf"),
        _ => None,
    }
}

pub fn create_mimetypes_module() -> PyObjectRef {
    make_module("mimetypes", vec![
        ("guess_type", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("guess_type requires a url")); }
            let url = args[0].py_to_string();
            let ext = url.rsplit('.').next().unwrap_or("");
            let mime = ext_to_mime(ext);
            match mime {
                Some(m) => Ok(PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from(m)),
                    PyObject::none(),
                ])),
                None => Ok(PyObject::tuple(vec![PyObject::none(), PyObject::none()])),
            }
        })),
        ("guess_extension", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("guess_extension requires a type")); }
            let mime = args[0].py_to_string();
            let ext = mime_to_ext(&mime);
            match ext {
                Some(e) => Ok(PyObject::str_val(CompactString::from(e))),
                None => Ok(PyObject::none()),
            }
        })),
        ("guess_all_extensions", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("guess_all_extensions requires a type")); }
            let mime = args[0].py_to_string();
            let ext = mime_to_ext(&mime);
            match ext {
                Some(e) => Ok(PyObject::list(vec![PyObject::str_val(CompactString::from(e))])),
                None => Ok(PyObject::list(vec![])),
            }
        })),
        ("init", make_builtin(|_| Ok(PyObject::none()))),
        ("types_map", PyObject::dict(IndexMap::new())),
    ])
}

// ── readline module ──

pub fn create_readline_module() -> PyObjectRef {
    // Shared readline state
    let history: Rc<PyCell<Vec<String>>> = Rc::new(PyCell::new(Vec::new()));
    let history_max_len: Rc<PyCell<i64>> = Rc::new(PyCell::new(-1)); // -1 = unlimited
    let completer: Rc<PyCell<Option<PyObjectRef>>> = Rc::new(PyCell::new(None));
    let completer_delims: Rc<PyCell<String>> = Rc::new(PyCell::new(
        " \t\n`~!@#$%^&*()-=+[{]}\\|;:'\",<>/?".to_string()
    ));
    let startup_hook: Rc<PyCell<Option<PyObjectRef>>> = Rc::new(PyCell::new(None));
    let pre_input_hook: Rc<PyCell<Option<PyObjectRef>>> = Rc::new(PyCell::new(None));

    let h = history.clone();
    let add_history_fn = PyObject::native_closure("add_history", move |args: &[PyObjectRef]| {
        if !args.is_empty() {
            let line = args[0].py_to_string();
            h.write().push(line);
        }
        Ok(PyObject::none())
    });

    let h = history.clone();
    let clear_history_fn = PyObject::native_closure("clear_history", move |_: &[PyObjectRef]| {
        h.write().clear();
        Ok(PyObject::none())
    });

    let h = history.clone();
    let get_current_history_length_fn = PyObject::native_closure("get_current_history_length", move |_: &[PyObjectRef]| {
        Ok(PyObject::int(h.read().len() as i64))
    });

    let ml = history_max_len.clone();
    let get_history_length_fn = PyObject::native_closure("get_history_length", move |_: &[PyObjectRef]| {
        Ok(PyObject::int(*ml.read()))
    });

    let ml = history_max_len.clone();
    let set_history_length_fn = PyObject::native_closure("set_history_length", move |args: &[PyObjectRef]| {
        if !args.is_empty() {
            let n = args[0].as_int().unwrap_or(-1);
            *ml.write() = n;
        }
        Ok(PyObject::none())
    });

    let h = history.clone();
    let read_history_file_fn = PyObject::native_closure("read_history_file", move |args: &[PyObjectRef]| {
        let path = if !args.is_empty() {
            args[0].py_to_string()
        } else {
            "~/.history".to_string()
        };
        let expanded = if path.starts_with('~') {
            if let Ok(home) = std::env::var("HOME") {
                path.replacen('~', &home, 1)
            } else { path }
        } else { path };
        if let Ok(contents) = std::fs::read_to_string(&expanded) {
            let mut hist = h.write();
            for line in contents.lines() {
                if !line.is_empty() {
                    hist.push(line.to_string());
                }
            }
        }
        Ok(PyObject::none())
    });

    let h = history.clone();
    let write_history_file_fn = PyObject::native_closure("write_history_file", move |args: &[PyObjectRef]| {
        let path = if !args.is_empty() {
            args[0].py_to_string()
        } else {
            "~/.history".to_string()
        };
        let expanded = if path.starts_with('~') {
            if let Ok(home) = std::env::var("HOME") {
                path.replacen('~', &home, 1)
            } else { path }
        } else { path };
        let hist = h.read();
        let contents = hist.join("\n");
        let _ = std::fs::write(&expanded, contents);
        Ok(PyObject::none())
    });

    let c = completer.clone();
    let set_completer_fn = PyObject::native_closure("set_completer", move |args: &[PyObjectRef]| {
        if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None) {
            *c.write() = Some(args[0].clone());
        } else {
            *c.write() = None;
        }
        Ok(PyObject::none())
    });

    let c = completer.clone();
    let get_completer_fn = PyObject::native_closure("get_completer", move |_: &[PyObjectRef]| {
        Ok(c.read().clone().unwrap_or_else(PyObject::none))
    });

    let d = completer_delims.clone();
    let set_completer_delims_fn = PyObject::native_closure("set_completer_delims", move |args: &[PyObjectRef]| {
        if !args.is_empty() {
            *d.write() = args[0].py_to_string();
        }
        Ok(PyObject::none())
    });

    let d = completer_delims.clone();
    let get_completer_delims_fn = PyObject::native_closure("get_completer_delims", move |_: &[PyObjectRef]| {
        Ok(PyObject::str_val(CompactString::from(d.read().as_str())))
    });

    let sh = startup_hook.clone();
    let set_startup_hook_fn = PyObject::native_closure("set_startup_hook", move |args: &[PyObjectRef]| {
        if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None) {
            *sh.write() = Some(args[0].clone());
        } else {
            *sh.write() = None;
        }
        Ok(PyObject::none())
    });

    let pih = pre_input_hook.clone();
    let set_pre_input_hook_fn = PyObject::native_closure("set_pre_input_hook", move |args: &[PyObjectRef]| {
        if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None) {
            *pih.write() = Some(args[0].clone());
        } else {
            *pih.write() = None;
        }
        Ok(PyObject::none())
    });

    let h = history.clone();
    let get_history_item_fn = PyObject::native_closure("get_history_item", move |args: &[PyObjectRef]| {
        if args.is_empty() { return Err(PyException::type_error("get_history_item requires an index")); }
        let idx = args[0].as_int().unwrap_or(0) as usize;
        let hist = h.read();
        // readline uses 1-based indexing
        if idx >= 1 && idx <= hist.len() {
            Ok(PyObject::str_val(CompactString::from(hist[idx - 1].as_str())))
        } else {
            Ok(PyObject::none())
        }
    });

    let h = history.clone();
    let remove_history_item_fn = PyObject::native_closure("remove_history_item", move |args: &[PyObjectRef]| {
        if args.is_empty() { return Err(PyException::type_error("remove_history_item requires an index")); }
        let idx = args[0].as_int().unwrap_or(0) as usize;
        let mut hist = h.write();
        if idx < hist.len() {
            hist.remove(idx);
        }
        Ok(PyObject::none())
    });

    make_module("readline", vec![
        ("parse_and_bind", make_builtin(|_| Ok(PyObject::none()))),
        ("set_completer", set_completer_fn),
        ("get_completer", get_completer_fn),
        ("set_completer_delims", set_completer_delims_fn),
        ("get_completer_delims", get_completer_delims_fn),
        ("add_history", add_history_fn),
        ("clear_history", clear_history_fn),
        ("get_history_length", get_history_length_fn),
        ("set_history_length", set_history_length_fn),
        ("get_current_history_length", get_current_history_length_fn),
        ("get_history_item", get_history_item_fn),
        ("remove_history_item", remove_history_item_fn),
        ("read_history_file", read_history_file_fn),
        ("write_history_file", write_history_file_fn),
        ("set_startup_hook", set_startup_hook_fn),
        ("set_pre_input_hook", set_pre_input_hook_fn),
    ])
}

// ── runpy module ──

pub fn create_runpy_module() -> PyObjectRef {
    // run_path(path, init_globals=None, run_name=None) -> dict
    // Reads file, compiles it, stores code + globals to be executed by VM deferred mechanism
    let run_path = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("runpy.run_path", args, 1)?;
        let path = args[0].py_to_string();
        let source = std::fs::read_to_string(&*path)
            .map_err(|e| PyException::os_error(format!("Cannot read {}: {}", path, e)))?;
        // Build a globals dict for the executed module
        let run_name = if args.len() >= 3 && !matches!(&args[2].payload, PyObjectPayload::None) {
            args[2].py_to_string().to_string()
        } else {
            "<run_path>".to_string()
        };
        let mut ns = IndexMap::new();
        ns.insert(
            HashableKey::str_key(CompactString::from("__name__")),
            PyObject::str_val(CompactString::from(run_name.as_str())),
        );
        ns.insert(
            HashableKey::str_key(CompactString::from("__file__")),
            PyObject::str_val(CompactString::from(&*path)),
        );
        // Merge init_globals if provided
        if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
            if let PyObjectPayload::Dict(ref d) = args[1].payload {
                for (k, v) in d.read().iter() {
                    ns.insert(k.clone(), v.clone());
                }
            }
        }
        let result_dict = PyObject::dict(ns);

        // Use the compile + exec mechanism via deferred call
        // Since we can't directly invoke the VM here, we compile to code object
        // and attach it along with the namespace for the VM to pick up
        let _code_src = PyObject::str_val(CompactString::from(source.as_str()));
        let _filename = PyObject::str_val(CompactString::from(&*path));

        // Store (source, filename, globals_dict) for deferred execution
        // The caller should use exec() in Python land instead.
        // For simplicity, compile and push as deferred call to builtins.exec
        // Actually, we need to push compile+exec as a deferred pair.
        // Simpler approach: use DEFERRED_CALLS with a special marker.
        
        // For now: compile the source using the compiler and return the namespace.
        // The file gets parsed and compiled to bytecode.
        match ferrython_parser::parse(&source, &*path) {
            Ok(module) => match ferrython_compiler::compile(&module, &*path) {
                Ok(code) => {
                    let code_obj = PyObject::code(code);
                    crate::concurrency_modules::push_deferred_call(
                        PyObject::str_val(CompactString::from("__runpy_exec__")),
                        vec![code_obj, result_dict.clone()],
                    );
                    Ok(result_dict)
                }
                Err(e) => Err(PyException::syntax_error(format!("Failed to compile {}: {:?}", path, e))),
            },
            Err(e) => Err(PyException::syntax_error(format!("Failed to parse {}: {:?}", path, e))),
        }
    });

    // run_module(mod_name, run_name=None, alter_sys=False) -> dict
    let run_module = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("runpy.run_module", args, 1)?;
        let mod_name = args[0].py_to_string();
        // Try to find module file by checking current directory and common paths
        let search_paths: Vec<String> = {
            let mut paths = vec![".".to_string()];
            // Add PYTHONPATH entries if available
            if let Ok(pp) = std::env::var("PYTHONPATH") {
                for p in pp.split(':') {
                    if !p.is_empty() { paths.push(p.to_string()); }
                }
            }
            paths
        };
        for dir in &search_paths {
            // Check module_name.py
            let file_path = format!("{}/{}.py", dir, mod_name);
            if std::path::Path::new(&file_path).exists() {
                let source = std::fs::read_to_string(&file_path)
                    .map_err(|e| PyException::os_error(format!("Cannot read {}: {}", file_path, e)))?;
                let run_name = if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
                    args[1].py_to_string().to_string()
                } else {
                    mod_name.to_string()
                };
                let mut ns = IndexMap::new();
                ns.insert(
                    HashableKey::str_key(CompactString::from("__name__")),
                    PyObject::str_val(CompactString::from(run_name.as_str())),
                );
                ns.insert(
                    HashableKey::str_key(CompactString::from("__file__")),
                    PyObject::str_val(CompactString::from(file_path.as_str())),
                );
                let result_dict = PyObject::dict(ns);
                match ferrython_parser::parse(&source, &file_path) {
                    Ok(module) => match ferrython_compiler::compile(&module, &file_path) {
                        Ok(code) => {
                            let code_obj = PyObject::code(code);
                            crate::concurrency_modules::push_deferred_call(
                                PyObject::str_val(CompactString::from("__runpy_exec__")),
                                vec![code_obj, result_dict.clone()],
                            );
                            return Ok(result_dict);
                        }
                        Err(e) => return Err(PyException::syntax_error(
                            format!("Failed to compile {}: {:?}", file_path, e)
                        )),
                    },
                    Err(e) => return Err(PyException::syntax_error(
                        format!("Failed to parse {}: {:?}", file_path, e)
                    )),
                }
            }
            // Check module_name/__main__.py
            let pkg_main = format!("{}/{}/__main__.py", dir, mod_name);
            if std::path::Path::new(&pkg_main).exists() {
                let source = std::fs::read_to_string(&pkg_main)
                    .map_err(|e| PyException::os_error(format!("Cannot read {}: {}", pkg_main, e)))?;
                let mut ns = IndexMap::new();
                ns.insert(
                    HashableKey::str_key(CompactString::from("__name__")),
                    PyObject::str_val(CompactString::from("__main__")),
                );
                ns.insert(
                    HashableKey::str_key(CompactString::from("__file__")),
                    PyObject::str_val(CompactString::from(pkg_main.as_str())),
                );
                let result_dict = PyObject::dict(ns);
                match ferrython_parser::parse(&source, &pkg_main) {
                    Ok(module) => match ferrython_compiler::compile(&module, &pkg_main) {
                        Ok(code) => {
                            let code_obj = PyObject::code(code);
                            crate::concurrency_modules::push_deferred_call(
                                PyObject::str_val(CompactString::from("__runpy_exec__")),
                                vec![code_obj, result_dict.clone()],
                            );
                            return Ok(result_dict);
                        }
                        Err(e) => return Err(PyException::syntax_error(
                            format!("Failed to compile {}: {:?}", pkg_main, e)
                        )),
                    },
                    Err(e) => return Err(PyException::syntax_error(
                        format!("Failed to parse {}: {:?}", pkg_main, e)
                    )),
                }
            }
        }
        Err(PyException::import_error(format!("No module named '{}'", mod_name)))
    });

    make_module("runpy", vec![
        ("run_module", run_module),
        ("run_path", run_path),
    ])
}

// ── cmd module ──

pub fn create_cmd_module() -> PyObjectRef {
    // Create Cmd as a proper class so it can be subclassed
    let cmd_cls = PyObject::class(CompactString::from("Cmd"), vec![], IndexMap::new());
    if let PyObjectPayload::Class(ref cd) = cmd_cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(CompactString::from("prompt"), PyObject::str_val(CompactString::from("(Cmd) ")));
        ns.insert(CompactString::from("intro"), PyObject::none());
        ns.insert(CompactString::from("identchars"), PyObject::str_val(CompactString::from(
            "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_"
        )));
        ns.insert(CompactString::from("ruler"), PyObject::str_val(CompactString::from("=")));
        ns.insert(CompactString::from("lastcmd"), PyObject::str_val(CompactString::from("")));
        ns.insert(CompactString::from("doc_leader"), PyObject::str_val(CompactString::from("")));
        ns.insert(CompactString::from("doc_header"), PyObject::str_val(CompactString::from("Documented commands (type help <topic>):")));
        ns.insert(CompactString::from("undoc_header"), PyObject::str_val(CompactString::from("Undocumented commands:")));
        ns.insert(CompactString::from("misc_header"), PyObject::str_val(CompactString::from("Miscellaneous help topics:")));
        ns.insert(CompactString::from("nohelp"), PyObject::str_val(CompactString::from("*** No help on %s")));
        ns.insert(CompactString::from("use_rawinput"), PyObject::bool_val(true));

        ns.insert(CompactString::from("cmdloop"), make_builtin(|args: &[PyObjectRef]| {
            // Basic cmdloop: read lines from stdin and dispatch
            let inst = if !args.is_empty() { args[0].clone() } else { return Ok(PyObject::none()); };
            let prompt_attr = inst.get_attr("prompt")
                .map(|p| p.py_to_string())
                .unwrap_or_else(|| "(Cmd) ".to_string());
            let intro = inst.get_attr("intro");

            if let Some(ref intro_obj) = intro {
                if !matches!(&intro_obj.payload, PyObjectPayload::None) {
                    println!("{}", intro_obj.py_to_string());
                }
            }

            loop {
                eprint!("{}", prompt_attr);
                let mut line = String::new();
                match std::io::stdin().read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    _ => {}
                }
                let line = line.trim_end_matches('\n').trim_end_matches('\r');
                if line == "EOF" || line.is_empty() && std::io::stdin().read_line(&mut String::new()).unwrap_or(0) == 0 {
                    break;
                }

                // Dispatch via onecmd
                if let Some(onecmd_fn) = inst.get_attr("onecmd") {
                    match &onecmd_fn.payload {
                        PyObjectPayload::NativeFunction(nf) => {
                            match (nf.func)(&[PyObject::str_val(CompactString::from(line))]) {
                                Ok(result) => {
                                    if result.is_truthy() { break; }
                                }
                                Err(e) => { eprintln!("{}", e.message); }
                            }
                        }
                        PyObjectPayload::NativeClosure(nc) => {
                            match (nc.func)(&[PyObject::str_val(CompactString::from(line))]) {
                                Ok(result) => {
                                    if result.is_truthy() { break; }
                                }
                                Err(e) => { eprintln!("{}", e.message); }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(PyObject::none())
        }));

        ns.insert(CompactString::from("parseline"), make_builtin(|args: &[PyObjectRef]| {
            let line = if args.len() > 1 { args[1].py_to_string() }
                       else if !args.is_empty() { args[0].py_to_string() }
                       else { return Ok(PyObject::tuple(vec![PyObject::none(), PyObject::none(), PyObject::str_val(CompactString::from(""))])); };
            let line = line.trim().to_string();
            if line.is_empty() {
                return Ok(PyObject::tuple(vec![
                    PyObject::none(), PyObject::none(),
                    PyObject::str_val(CompactString::from(line)),
                ]));
            }
            // Find the command word
            let identchars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";
            let cmd_end = line.find(|c: char| !identchars.contains(c)).unwrap_or(line.len());
            let cmd = &line[..cmd_end];
            let rest = line[cmd_end..].trim_start();
            Ok(PyObject::tuple(vec![
                PyObject::str_val(CompactString::from(cmd)),
                PyObject::str_val(CompactString::from(rest)),
                PyObject::str_val(CompactString::from(line)),
            ]))
        }));

        ns.insert(CompactString::from("onecmd"), make_builtin(|args: &[PyObjectRef]| {
            let line = if args.len() > 1 { args[1].py_to_string() }
                       else if !args.is_empty() { args[0].py_to_string() }
                       else { return Ok(PyObject::bool_val(false)); };
            if line.trim().is_empty() {
                return Ok(PyObject::bool_val(false));
            }
            // Parse cmd + args
            let trimmed = line.trim();
            let identchars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";
            let cmd_end = trimmed.find(|c: char| !identchars.contains(c)).unwrap_or(trimmed.len());
            let cmd = &trimmed[..cmd_end];
            let rest = trimmed[cmd_end..].trim_start();
            // Look for do_<cmd> method
            let self_obj = if args.len() > 1 { &args[0] } else { return Ok(PyObject::bool_val(false)); };
            let method_name = format!("do_{}", cmd);
            if let Some(method) = self_obj.get_attr(&method_name) {
                match &method.payload {
                    PyObjectPayload::NativeFunction(nf) => {
                        return (nf.func)(&[PyObject::str_val(CompactString::from(rest))]);
                    }
                    PyObjectPayload::NativeClosure(nc) => {
                        return (nc.func)(&[PyObject::str_val(CompactString::from(rest))]);
                    }
                    _ => {
                        ferrython_core::error::request_vm_call(method.clone(),
                            vec![PyObject::str_val(CompactString::from(rest))]);
                        return Ok(PyObject::bool_val(false));
                    }
                }
            }
            eprintln!("*** Unknown syntax: {}", trimmed);
            Ok(PyObject::bool_val(false))
        }));

        ns.insert(CompactString::from("precmd"), make_builtin(|args: &[PyObjectRef]| {
            // Default: return line unchanged
            if args.len() > 1 { Ok(args[1].clone()) }
            else if !args.is_empty() { Ok(args[0].clone()) }
            else { Ok(PyObject::str_val(CompactString::from(""))) }
        }));

        ns.insert(CompactString::from("postcmd"), make_builtin(|args: &[PyObjectRef]| {
            // Default: return stop flag unchanged
            if args.len() > 1 { Ok(args[1].clone()) }
            else if !args.is_empty() { Ok(args[0].clone()) }
            else { Ok(PyObject::bool_val(false)) }
        }));

        ns.insert(CompactString::from("preloop"), make_builtin(|_| Ok(PyObject::none())));
        ns.insert(CompactString::from("postloop"), make_builtin(|_| Ok(PyObject::none())));
        ns.insert(CompactString::from("emptyline"), make_builtin(|_| Ok(PyObject::none())));

        ns.insert(CompactString::from("default"), make_builtin(|args: &[PyObjectRef]| {
            let line = if args.len() > 1 { args[1].py_to_string() }
                       else if !args.is_empty() { args[0].py_to_string() }
                       else { String::new() };
            eprintln!("*** Unknown syntax: {}", line);
            Ok(PyObject::none())
        }));

        ns.insert(CompactString::from("columnize"), make_builtin(|args: &[PyObjectRef]| {
            let list = if args.len() > 1 { &args[1] } else if !args.is_empty() { &args[0] } else {
                return Ok(PyObject::none());
            };
            if let PyObjectPayload::List(ref items) = list.payload {
                let items_r = items.read();
                let strs: Vec<String> = items_r.iter().map(|i| i.py_to_string()).collect();
                println!("{}", strs.join("  "));
            }
            Ok(PyObject::none())
        }));
    }

    make_module("cmd", vec![("Cmd", cmd_cls)])
}

// ── compileall module ──

pub fn create_compileall_module() -> PyObjectRef {
    // compile_file(fullname, ddir=None, force=False, rx=None, quiet=0)
    let compile_file = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("compileall.compile_file", args, 1)?;
        let path = args[0].py_to_string();
        if !path.ends_with(".py") {
            return Ok(PyObject::bool_val(true));
        }
        let source = match std::fs::read_to_string(&*path) {
            Ok(s) => s,
            Err(_) => return Ok(PyObject::bool_val(false)),
        };
        match ferrython_parser::parse(&source, &*path) {
            Ok(module) => match ferrython_compiler::compile(&module, &*path) {
                Ok(_) => Ok(PyObject::bool_val(true)),
                Err(_) => Ok(PyObject::bool_val(false)),
            },
            Err(_) => Ok(PyObject::bool_val(false)),
        }
    });

    // compile_dir(dir, maxlevels=10, ddir=None, force=False, rx=None, quiet=0)
    let compile_dir = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("compileall.compile_dir", args, 1)?;
        let dir = args[0].py_to_string();
        let max_levels = if args.len() >= 2 { args[1].to_int().unwrap_or(10) } else { 10 };
        fn compile_dir_recursive(dir: &str, levels: i64) -> bool {
            if levels < 0 { return true; }
            let entries = match std::fs::read_dir(dir) {
                Ok(e) => e,
                Err(_) => return false,
            };
            let mut ok = true;
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext == "py" {
                            let p = path.to_string_lossy().to_string();
                            let source = match std::fs::read_to_string(&p) {
                                Ok(s) => s,
                                Err(_) => { ok = false; continue; }
                            };
                            if ferrython_parser::parse(&source, &p)
                                .map_err(|_| ())
                                .and_then(|m| ferrython_compiler::compile(&m, &p).map_err(|_| ()))
                                .is_err() {
                                ok = false;
                            }
                        }
                    }
                } else if path.is_dir() && levels > 0 {
                    if !compile_dir_recursive(&path.to_string_lossy(), levels - 1) {
                        ok = false;
                    }
                }
            }
            ok
        }
        Ok(PyObject::bool_val(compile_dir_recursive(&dir, max_levels)))
    });

    make_module("compileall", vec![
        ("compile_dir", compile_dir),
        ("compile_file", compile_file),
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
                // Stats methods return self for chaining
                let self_ref = inst.clone();
                let s = self_ref.clone();
                attrs.insert(CompactString::from("sort_stats"), PyObject::native_closure(
                    "Stats.sort_stats", move |args: &[PyObjectRef]| {
                        // Store sort key for reference
                        if let PyObjectPayload::Instance(ref d) = s.payload {
                            if !args.is_empty() {
                                d.attrs.write().insert(
                                    CompactString::from("_sort_key"),
                                    args[0].clone(),
                                );
                            }
                        }
                        Ok(s.clone())
                    }
                ));
                let s = self_ref.clone();
                attrs.insert(CompactString::from("print_stats"), PyObject::native_closure(
                    "Stats.print_stats", move |_: &[PyObjectRef]| {
                        Ok(s.clone())
                    }
                ));
                let s = self_ref.clone();
                attrs.insert(CompactString::from("print_callers"), PyObject::native_closure(
                    "Stats.print_callers", move |_: &[PyObjectRef]| {
                        Ok(s.clone())
                    }
                ));
                let s = self_ref.clone();
                attrs.insert(CompactString::from("print_callees"), PyObject::native_closure(
                    "Stats.print_callees", move |_: &[PyObjectRef]| {
                        Ok(s.clone())
                    }
                ));
                let s = self_ref.clone();
                attrs.insert(CompactString::from("strip_dirs"), PyObject::native_closure(
                    "Stats.strip_dirs", move |_: &[PyObjectRef]| {
                        Ok(s.clone())
                    }
                ));
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
            map.insert(HashableKey::str_key(key), val_obj);
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


// ── curses module (stub) ──

/// Helper to create a curses window object with standard methods
fn make_curses_window(nlines: i64, ncols: i64, begin_y: i64, begin_x: i64) -> PyObjectRef {
    let cls = PyObject::class(CompactString::from("Window"), vec![], IndexMap::new());
    let win = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref d) = win.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("_nlines"), PyObject::int(nlines));
        w.insert(CompactString::from("_ncols"), PyObject::int(ncols));
        w.insert(CompactString::from("_begin_y"), PyObject::int(begin_y));
        w.insert(CompactString::from("_begin_x"), PyObject::int(begin_x));
        w.insert(CompactString::from("_cur_y"), PyObject::int(0));
        w.insert(CompactString::from("_cur_x"), PyObject::int(0));

        let self_ref = win.clone();
        let s = self_ref.clone();
        w.insert(CompactString::from("addstr"), PyObject::native_closure("addstr", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("addnstr"), PyObject::native_closure("addnstr", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("refresh"), PyObject::native_closure("refresh", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("clear"), PyObject::native_closure("clear", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("erase"), PyObject::native_closure("erase", move |_| Ok(s.clone())));
        w.insert(CompactString::from("getch"), make_builtin(|_| Ok(PyObject::int(-1))));
        w.insert(CompactString::from("getkey"), make_builtin(|_| Ok(PyObject::str_val(CompactString::from("")))));
        let nl = nlines;
        let nc = ncols;
        w.insert(CompactString::from("getmaxyx"), PyObject::native_closure("getmaxyx", move |_| {
            Ok(PyObject::tuple(vec![PyObject::int(nl), PyObject::int(nc)]))
        }));
        w.insert(CompactString::from("getyx"), make_builtin(|_| Ok(PyObject::tuple(vec![PyObject::int(0), PyObject::int(0)]))));
        w.insert(CompactString::from("getbegyx"), {
            let by = begin_y;
            let bx = begin_x;
            PyObject::native_closure("getbegyx", move |_| {
                Ok(PyObject::tuple(vec![PyObject::int(by), PyObject::int(bx)]))
            })
        });
        let s = self_ref.clone();
        w.insert(CompactString::from("move"), PyObject::native_closure("move", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("clrtoeol"), PyObject::native_closure("clrtoeol", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("clrtobot"), PyObject::native_closure("clrtobot", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("keypad"), PyObject::native_closure("keypad", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("nodelay"), PyObject::native_closure("nodelay", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("timeout"), PyObject::native_closure("timeout", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("scrollok"), PyObject::native_closure("scrollok", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("idlok"), PyObject::native_closure("idlok", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("border"), PyObject::native_closure("border", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("box"), PyObject::native_closure("box", move |_| Ok(s.clone())));
        w.insert(CompactString::from("subwin"), make_builtin(|args: &[PyObjectRef]| {
            let (nl, nc, by, bx) = match args.len() {
                4 => (
                    args[0].as_int().unwrap_or(24),
                    args[1].as_int().unwrap_or(80),
                    args[2].as_int().unwrap_or(0),
                    args[3].as_int().unwrap_or(0),
                ),
                2 => (args[0].as_int().unwrap_or(24), args[1].as_int().unwrap_or(80), 0, 0),
                _ => (24, 80, 0, 0),
            };
            Ok(make_curses_window(nl, nc, by, bx))
        }));
        w.insert(CompactString::from("derwin"), make_builtin(|args: &[PyObjectRef]| {
            let (nl, nc, by, bx) = match args.len() {
                4 => (
                    args[0].as_int().unwrap_or(24),
                    args[1].as_int().unwrap_or(80),
                    args[2].as_int().unwrap_or(0),
                    args[3].as_int().unwrap_or(0),
                ),
                2 => (args[0].as_int().unwrap_or(24), args[1].as_int().unwrap_or(80), 0, 0),
                _ => (24, 80, 0, 0),
            };
            Ok(make_curses_window(nl, nc, by, bx))
        }));
        let s = self_ref.clone();
        w.insert(CompactString::from("mvaddstr"), PyObject::native_closure("mvaddstr", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("attron"), PyObject::native_closure("attron", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("attroff"), PyObject::native_closure("attroff", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("attrset"), PyObject::native_closure("attrset", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("bkgd"), PyObject::native_closure("bkgd", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("noutrefresh"), PyObject::native_closure("noutrefresh", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("insstr"), PyObject::native_closure("insstr", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("deleteln"), PyObject::native_closure("deleteln", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("insertln"), PyObject::native_closure("insertln", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("scroll"), PyObject::native_closure("scroll", move |_| Ok(s.clone())));
        let s = self_ref.clone();
        w.insert(CompactString::from("setscrreg"), PyObject::native_closure("setscrreg", move |_| Ok(s.clone())));
        w.insert(CompactString::from("inch"), make_builtin(|_| Ok(PyObject::int(32)))); // space char
        w.insert(CompactString::from("instr"), make_builtin(|_| Ok(PyObject::str_val(CompactString::from("")))));
    }
    win
}

pub fn create_curses_module() -> PyObjectRef {
    // Stub curses module — provides constants and no-op functions
    // so that programs that conditionally import curses don't crash.

    let initscr_fn = make_builtin(|_: &[PyObjectRef]| {
        Ok(make_curses_window(24, 80, 0, 0))
    });

    let wrapper_fn = make_builtin(|args: &[PyObjectRef]| {
        // curses.wrapper(func) — calls func(stdscr)
        if args.is_empty() {
            return Err(PyException::type_error("wrapper() requires a callable"));
        }
        let stdscr = make_curses_window(24, 80, 0, 0);
        ferrython_core::error::request_vm_call(
            args[0].clone(),
            vec![stdscr],
        );
        Ok(PyObject::none())
    });

    make_module("curses", vec![
        ("initscr", initscr_fn.clone()),
        ("endwin", make_builtin(|_| Ok(PyObject::none()))),
        ("wrapper", wrapper_fn),
        ("start_color", make_builtin(|_| Ok(PyObject::none()))),
        ("init_pair", make_builtin(|_| Ok(PyObject::none()))),
        ("color_pair", make_builtin(|args| {
            Ok(PyObject::int(args.first().and_then(|a| a.as_int()).unwrap_or(0)))
        })),
        ("cbreak", make_builtin(|_| Ok(PyObject::none()))),
        ("nocbreak", make_builtin(|_| Ok(PyObject::none()))),
        ("echo", make_builtin(|_| Ok(PyObject::none()))),
        ("noecho", make_builtin(|_| Ok(PyObject::none()))),
        ("raw", make_builtin(|_| Ok(PyObject::none()))),
        ("noraw", make_builtin(|_| Ok(PyObject::none()))),
        ("curs_set", make_builtin(|args| {
            // Returns previous cursor visibility (0, 1, or 2)
            let _new = args.first().and_then(|a| a.as_int()).unwrap_or(1);
            Ok(PyObject::int(1))
        })),
        ("newwin", make_builtin(|args: &[PyObjectRef]| {
            let (nl, nc, by, bx) = match args.len() {
                4 => (
                    args[0].as_int().unwrap_or(24),
                    args[1].as_int().unwrap_or(80),
                    args[2].as_int().unwrap_or(0),
                    args[3].as_int().unwrap_or(0),
                ),
                2 => (args[0].as_int().unwrap_or(24), args[1].as_int().unwrap_or(80), 0, 0),
                _ => (24, 80, 0, 0),
            };
            Ok(make_curses_window(nl, nc, by, bx))
        })),
        ("newpad", make_builtin(|args: &[PyObjectRef]| {
            let nl = args.first().and_then(|a| a.as_int()).unwrap_or(100);
            let nc = args.get(1).and_then(|a| a.as_int()).unwrap_or(100);
            Ok(make_curses_window(nl, nc, 0, 0))
        })),
        ("napms", make_builtin(|args| {
            let ms = args.first().and_then(|a| a.as_int()).unwrap_or(0);
            if ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(ms as u64));
            }
            Ok(PyObject::none())
        })),
        ("beep", make_builtin(|_| Ok(PyObject::none()))),
        ("flash", make_builtin(|_| Ok(PyObject::none()))),
        ("doupdate", make_builtin(|_| Ok(PyObject::none()))),
        ("has_colors", make_builtin(|_| Ok(PyObject::bool_val(false)))),
        ("can_change_color", make_builtin(|_| Ok(PyObject::bool_val(false)))),
        ("use_default_colors", make_builtin(|_| Ok(PyObject::none()))),
        ("use_env", make_builtin(|_| Ok(PyObject::none()))),
        ("isendwin", make_builtin(|_| Ok(PyObject::bool_val(false)))),
        ("erasechar", make_builtin(|_| Ok(PyObject::str_val(CompactString::from("\x08"))))),
        ("killchar", make_builtin(|_| Ok(PyObject::str_val(CompactString::from("\x15"))))),
        ("LINES", PyObject::int(24)),
        ("COLS", PyObject::int(80)),
        ("COLORS", PyObject::int(256)),
        ("COLOR_PAIRS", PyObject::int(256)),
        // Color constants
        ("COLOR_BLACK", PyObject::int(0)),
        ("COLOR_RED", PyObject::int(1)),
        ("COLOR_GREEN", PyObject::int(2)),
        ("COLOR_YELLOW", PyObject::int(3)),
        ("COLOR_BLUE", PyObject::int(4)),
        ("COLOR_MAGENTA", PyObject::int(5)),
        ("COLOR_CYAN", PyObject::int(6)),
        ("COLOR_WHITE", PyObject::int(7)),
        // Attribute constants
        ("A_NORMAL", PyObject::int(0)),
        ("A_STANDOUT", PyObject::int(1 << 16)),
        ("A_UNDERLINE", PyObject::int(1 << 17)),
        ("A_REVERSE", PyObject::int(1 << 18)),
        ("A_BLINK", PyObject::int(1 << 19)),
        ("A_DIM", PyObject::int(1 << 20)),
        ("A_BOLD", PyObject::int(1 << 21)),
        ("A_PROTECT", PyObject::int(1 << 24)),
        ("A_INVIS", PyObject::int(1 << 23)),
        ("A_ALTCHARSET", PyObject::int(1 << 22)),
        // Key constants
        ("KEY_UP", PyObject::int(259)),
        ("KEY_DOWN", PyObject::int(258)),
        ("KEY_LEFT", PyObject::int(260)),
        ("KEY_RIGHT", PyObject::int(261)),
        ("KEY_HOME", PyObject::int(262)),
        ("KEY_BACKSPACE", PyObject::int(263)),
        ("KEY_F0", PyObject::int(264)),
        ("KEY_DC", PyObject::int(330)),
        ("KEY_IC", PyObject::int(331)),
        ("KEY_NPAGE", PyObject::int(338)),
        ("KEY_PPAGE", PyObject::int(339)),
        ("KEY_ENTER", PyObject::int(343)),
        ("KEY_RESIZE", PyObject::int(410)),
        // Error class
        ("error", PyObject::class(CompactString::from("error"), vec![], IndexMap::new())),
    ])
}

// ── ctypes module ──

/// Call a C function at `sym_addr` with the given Python arguments.
/// Arguments are converted: int→i64, float→f64, bytes/str→pointer, None→NULL.
/// Returns i64 result as Python int (caller can set .restype to change).
fn ctypes_call_function(sym_addr: usize, fn_name: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Convert Python args to C values  
    let mut c_args: Vec<u64> = Vec::with_capacity(args.len());
    // Keep CString alive for the duration of the call
    let mut _string_keepalive: Vec<std::ffi::CString> = Vec::new();
    
    for (i, arg) in args.iter().enumerate() {
        match &arg.payload {
            PyObjectPayload::Int(n) => c_args.push(n.to_i64().unwrap_or(0) as u64),
            PyObjectPayload::Float(f) => c_args.push(f.to_bits()),
            PyObjectPayload::Bool(b) => c_args.push(if *b { 1 } else { 0 }),
            PyObjectPayload::Bytes(b) => {
                let cs = std::ffi::CString::new(b.as_slice()).unwrap_or_else(|_| {
                    std::ffi::CString::new("").unwrap()
                });
                c_args.push(cs.as_ptr() as u64);
                _string_keepalive.push(cs);
            }
            PyObjectPayload::Str(s) => {
                let cs = std::ffi::CString::new(s.as_str()).unwrap_or_else(|_| {
                    std::ffi::CString::new("").unwrap()
                });
                c_args.push(cs.as_ptr() as u64);
                _string_keepalive.push(cs);
            }
            PyObjectPayload::None => c_args.push(0),
            // ctypes type instances: extract .value
            PyObjectPayload::Instance(_) => {
                if let Some(val) = arg.get_attr("value") {
                    match &val.payload {
                        PyObjectPayload::Int(n) => c_args.push(n.to_i64().unwrap_or(0) as u64),
                        PyObjectPayload::Float(f) => c_args.push(f.to_bits()),
                        PyObjectPayload::Bool(b) => c_args.push(if *b { 1 } else { 0 }),
                        PyObjectPayload::Bytes(b) => {
                            let cs = std::ffi::CString::new(b.as_slice()).unwrap_or_default();
                            c_args.push(cs.as_ptr() as u64);
                            _string_keepalive.push(cs);
                        }
                        PyObjectPayload::Str(s) => {
                            let cs = std::ffi::CString::new(s.as_str()).unwrap_or_default();
                            c_args.push(cs.as_ptr() as u64);
                            _string_keepalive.push(cs);
                        }
                        _ => c_args.push(0),
                    }
                } else {
                    c_args.push(0);
                }
            }
            _ => return Err(PyException::type_error(&format!(
                "ctypes: cannot convert argument {} of type {} for {}", i, arg.type_name(), fn_name
            ))),
        }
    }
    
    // Call the function using the system ABI (x86_64 SysV: first 6 args in registers)
    let result: i64 = unsafe {
        let fn_ptr = sym_addr as *const ();
        match c_args.len() {
            0 => {
                let f: extern "C" fn() -> i64 = std::mem::transmute(fn_ptr);
                f()
            }
            1 => {
                let f: extern "C" fn(u64) -> i64 = std::mem::transmute(fn_ptr);
                f(c_args[0])
            }
            2 => {
                let f: extern "C" fn(u64, u64) -> i64 = std::mem::transmute(fn_ptr);
                f(c_args[0], c_args[1])
            }
            3 => {
                let f: extern "C" fn(u64, u64, u64) -> i64 = std::mem::transmute(fn_ptr);
                f(c_args[0], c_args[1], c_args[2])
            }
            4 => {
                let f: extern "C" fn(u64, u64, u64, u64) -> i64 = std::mem::transmute(fn_ptr);
                f(c_args[0], c_args[1], c_args[2], c_args[3])
            }
            5 => {
                let f: extern "C" fn(u64, u64, u64, u64, u64) -> i64 = std::mem::transmute(fn_ptr);
                f(c_args[0], c_args[1], c_args[2], c_args[3], c_args[4])
            }
            6 => {
                let f: extern "C" fn(u64, u64, u64, u64, u64, u64) -> i64 = std::mem::transmute(fn_ptr);
                f(c_args[0], c_args[1], c_args[2], c_args[3], c_args[4], c_args[5])
            }
            _ => return Err(PyException::type_error(&format!(
                "ctypes: too many arguments ({}) for {}", c_args.len(), fn_name
            ))),
        }
    };
    
    Ok(PyObject::int(result))
}

fn make_ctype(name: &str) -> PyObjectRef {
    // Create a callable ctypes type: c_int(42) → instance with .value attribute
    let type_name = CompactString::from(name);
    let cls = PyObject::class(type_name.clone(), vec![], IndexMap::new());
    let cls_clone = cls.clone();
    let name_owned = name.to_string();
    PyObject::native_closure(name, move |args: &[PyObjectRef]| {
        let value = if args.is_empty() {
            // Default values depend on type
            if name_owned.contains("char_p") || name_owned.contains("wchar_p") {
                PyObject::none()
            } else if name_owned.contains("double") || name_owned.contains("float") {
                PyObject::float(0.0)
            } else if name_owned.contains("bool") {
                PyObject::bool_val(false)
            } else {
                PyObject::int(0)
            }
        } else {
            args[0].clone()
        };
        let inst = PyObject::instance(cls_clone.clone());
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut attrs = d.attrs.write();
            attrs.insert(CompactString::from("value"), value);
            attrs.insert(CompactString::from("_type_"), PyObject::str_val(CompactString::from(name_owned.as_str())));
        }
        Ok(inst)
    })
}

/// Return the standard byte size for a ctypes type name
fn ctype_sizeof(name: &str) -> i64 {
    match name {
        n if n.contains("int8") || n.contains("byte") || n.contains("char") && !n.contains("char_p") || n.contains("bool") => 1,
        n if n.contains("int16") || n.contains("short") => 2,
        n if n.contains("int32") || n == "c_int" || n == "c_uint" || n.contains("float") && !n.contains("double") => 4,
        n if n.contains("int64") || n.contains("long") || n.contains("double") || n.contains("size_t") || n.contains("ssize_t") => 8,
        n if n.contains("_p") || n.contains("void_p") => std::mem::size_of::<usize>() as i64,
        _ => 8,
    }
}

pub fn create_ctypes_module() -> PyObjectRef {
    // ctypes stub — provides type definitions with .value support
    // so that programs that import ctypes get basic functionality.

    let c_int = make_ctype("c_int");
    let c_long = make_ctype("c_long");
    let c_char = make_ctype("c_char");
    let c_char_p = make_ctype("c_char_p");
    let c_wchar_p = make_ctype("c_wchar_p");
    let c_void_p = make_ctype("c_void_p");
    let c_double = make_ctype("c_double");
    let c_float = make_ctype("c_float");
    let c_uint = make_ctype("c_uint");
    let c_ulong = make_ctype("c_ulong");
    let c_short = make_ctype("c_short");
    let c_ushort = make_ctype("c_ushort");
    let c_byte = make_ctype("c_byte");
    let c_ubyte = make_ctype("c_ubyte");
    let c_bool = make_ctype("c_bool");
    let c_longlong = make_ctype("c_longlong");
    let c_ulonglong = make_ctype("c_ulonglong");
    let c_size_t = make_ctype("c_size_t");
    let c_ssize_t = make_ctype("c_ssize_t");

    let structure_cls = {
        let mut ns = IndexMap::new();
        // _fields_ is typically set by subclasses, but provide a default empty list
        ns.insert(CompactString::from("_fields_"), PyObject::list(vec![]));
        ns.insert(CompactString::from("_pack_"), PyObject::int(0));
        PyObject::class(CompactString::from("Structure"), vec![], ns)
    };
    let union_cls = {
        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("_fields_"), PyObject::list(vec![]));
        PyObject::class(CompactString::from("Union"), vec![], ns)
    };
    let array_cls = PyObject::class(CompactString::from("Array"), vec![], IndexMap::new());

    // CDLL — real dlopen/dlsym based foreign function interface
    let cdll_fn = make_builtin(|args: &[PyObjectRef]| {
        let name = args.first().map(|a| a.py_to_string()).unwrap_or_default();
        
        // dlopen the library
        let c_name = std::ffi::CString::new(name.as_str()).map_err(|_| {
            PyException::os_error(&format!("invalid library name: {}", name))
        })?;
        let handle = unsafe { libc::dlopen(c_name.as_ptr(), libc::RTLD_LAZY | libc::RTLD_GLOBAL) };
        if handle.is_null() {
            let err = unsafe { libc::dlerror() };
            let msg = if err.is_null() {
                format!("cannot load library '{}'", name)
            } else {
                unsafe { std::ffi::CStr::from_ptr(err).to_string_lossy().into_owned() }
            };
            return Err(PyException::os_error(&msg));
        }
        let handle_val = handle as usize;
        
        let cls = PyObject::class(CompactString::from("CDLL"), vec![], IndexMap::new());
        let mut cls_ns = IndexMap::new();
        
        // __getattr__ for function lookup via dlsym
        let lib_name = name.clone();
        cls_ns.insert(CompactString::from("__getattr__"), PyObject::native_closure("__getattr__", move |args: &[PyObjectRef]| {
            let attr_name = if args.len() > 1 { args[1].py_to_string() } else if !args.is_empty() { args[0].py_to_string() } else {
                return Err(PyException::type_error("__getattr__ requires a name"));
            };
            // Look up symbol via dlsym
            let c_sym = std::ffi::CString::new(attr_name.as_str()).map_err(|_| {
                PyException::attribute_error(&format!("invalid symbol name: {}", attr_name))
            })?;
            let sym = unsafe { libc::dlsym(handle_val as *mut libc::c_void, c_sym.as_ptr()) };
            if sym.is_null() {
                return Err(PyException::attribute_error(&format!(
                    "undefined symbol: {}", attr_name
                )));
            }
            let sym_addr = sym as usize;
            let fn_name = attr_name.clone();
            
            // Return a callable that invokes the C function
            // Supports calling conventions: integers, pointers (bytes/str → c_char_p)
            Ok(PyObject::native_closure(&format!("{}.{}", lib_name, attr_name), move |call_args: &[PyObjectRef]| {
                ctypes_call_function(sym_addr, &fn_name, call_args)
            }))
        }));
        
        let inst = PyObject::instance_with_attrs(cls, cls_ns);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut attrs = d.attrs.write();
            attrs.insert(CompactString::from("_name"), PyObject::str_val(CompactString::from(name.as_str())));
            attrs.insert(CompactString::from("_handle"), PyObject::int(handle_val as i64));
        }
        Ok(inst)
    });

    // POINTER(type) → returns a new pointer type callable
    let pointer_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Err(PyException::type_error("POINTER requires a type")); }
        let base_type = args[0].clone();
        let type_name = format!("LP_{}", base_type.py_to_string());
        let ptr_cls = PyObject::class(CompactString::from(type_name.as_str()), vec![], IndexMap::new());
        let ptr_cls_clone = ptr_cls.clone();
        Ok(PyObject::native_closure(&type_name, move |args: &[PyObjectRef]| {
            let inst = PyObject::instance(ptr_cls_clone.clone());
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut attrs = d.attrs.write();
                attrs.insert(CompactString::from("_type_"), base_type.clone());
                attrs.insert(CompactString::from("contents"), if args.is_empty() {
                    PyObject::none()
                } else {
                    args[0].clone()
                });
            }
            Ok(inst)
        }))
    });

    // byref(obj, offset=0) → reference wrapper with ._obj
    let byref_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Err(PyException::type_error("byref requires an argument")); }
        let cls = PyObject::class(CompactString::from("CArgObject"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut attrs = d.attrs.write();
            attrs.insert(CompactString::from("_obj"), args[0].clone());
            attrs.insert(CompactString::from("value"), args[0].clone());
            let offset = if args.len() > 1 { args[1].as_int().unwrap_or(0) } else { 0 };
            attrs.insert(CompactString::from("_offset"), PyObject::int(offset));
        }
        Ok(inst)
    });

    // sizeof(type_or_instance)
    let sizeof_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Err(PyException::type_error("sizeof requires an argument")); }
        // Try to get type name from _type_ attr or class name
        let type_name = if let Some(t) = args[0].get_attr("_type_") {
            t.py_to_string()
        } else {
            args[0].py_to_string()
        };
        Ok(PyObject::int(ctype_sizeof(&type_name)))
    });

    make_module("ctypes", vec![
        ("c_int", c_int),
        ("c_long", c_long),
        ("c_char", c_char),
        ("c_char_p", c_char_p),
        ("c_wchar_p", c_wchar_p),
        ("c_void_p", c_void_p),
        ("c_double", c_double),
        ("c_float", c_float),
        ("c_uint", c_uint),
        ("c_ulong", c_ulong),
        ("c_short", c_short),
        ("c_ushort", c_ushort),
        ("c_byte", c_byte),
        ("c_ubyte", c_ubyte),
        ("c_bool", c_bool),
        ("c_longlong", c_longlong),
        ("c_ulonglong", c_ulonglong),
        ("c_size_t", c_size_t),
        ("c_ssize_t", c_ssize_t),
        ("c_int8", make_ctype("c_int8")),
        ("c_int16", make_ctype("c_int16")),
        ("c_int32", make_ctype("c_int32")),
        ("c_int64", make_ctype("c_int64")),
        ("c_uint8", make_ctype("c_uint8")),
        ("c_uint16", make_ctype("c_uint16")),
        ("c_uint32", make_ctype("c_uint32")),
        ("c_uint64", make_ctype("c_uint64")),
        ("Structure", structure_cls),
        ("Union", union_cls),
        ("Array", array_cls),
        ("CDLL", cdll_fn.clone()),
        ("cdll", cdll_fn.clone()),
        ("windll", cdll_fn.clone()),
        ("oledll", cdll_fn),
        ("POINTER", pointer_fn.clone()),
        ("pointer", pointer_fn),
        ("cast", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("cast requires obj and type")); }
            // Return a new instance of the target type wrapping the source value
            let source = &args[0];
            let target_type = &args[1];
            match &target_type.payload {
                PyObjectPayload::NativeFunction(nf) => (nf.func)(&[source.clone()]),
                PyObjectPayload::NativeClosure(nc) => (nc.func)(&[source.clone()]),
                _ => Ok(source.clone()),
            }
        })),
        ("byref", byref_fn),
        ("addressof", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("addressof requires an argument")); }
            // Return a fake address based on the Arc pointer
            let ptr = PyObjectRef::as_ptr(&args[0]) as usize;
            Ok(PyObject::int(ptr as i64))
        })),
        ("sizeof", sizeof_fn),
        ("create_string_buffer", make_builtin(|args| {
            let size = args.first().and_then(|a| a.as_int()).unwrap_or(256) as usize;
            Ok(PyObject::bytes(vec![0u8; size]))
        })),
        ("create_unicode_buffer", make_builtin(|args| {
            let size = args.first().and_then(|a| a.as_int()).unwrap_or(256) as usize;
            Ok(PyObject::str_val(CompactString::from("\0".repeat(size))))
        })),
        ("get_errno", make_builtin(|_| {
            #[cfg(unix)]
            {
                let e = unsafe { *libc::__errno_location() };
                Ok(PyObject::int(e as i64))
            }
            #[cfg(not(unix))]
            Err(PyException::os_error("get_errno() is not supported on this platform"))
        })),
        ("set_errno", make_builtin(|args| {
            let new_val = args.first().and_then(|a| a.as_int()).unwrap_or(0);
            #[cfg(unix)]
            {
                let old = unsafe { *libc::__errno_location() };
                unsafe { *libc::__errno_location() = new_val as i32; }
                Ok(PyObject::int(old as i64))
            }
            #[cfg(not(unix))]
            {
                let _ = new_val;
                Err(PyException::os_error("set_errno() is not supported on this platform"))
            }
        })),
        ("get_last_error", make_builtin(|_| Err(PyException::os_error("get_last_error() is not supported on this platform")))),
        ("set_last_error", make_builtin(|_| Err(PyException::os_error("set_last_error() is not supported on this platform")))),
        ("util", {
            // ctypes.util.find_library
            let mut util_attrs = IndexMap::new();
            util_attrs.insert(CompactString::from("find_library"), make_builtin(|args| {
                let name = args.first().map(|a| a.py_to_string()).unwrap_or_default();
                // Try common library paths
                let candidates = vec![
                    format!("lib{}.so", name),
                    format!("lib{}.dylib", name),
                    format!("{}.dll", name),
                ];
                for candidate in &candidates {
                    if std::path::Path::new(candidate).exists() {
                        return Ok(PyObject::str_val(CompactString::from(candidate.as_str())));
                    }
                    let path = format!("/usr/lib/{}", candidate);
                    if std::path::Path::new(&path).exists() {
                        return Ok(PyObject::str_val(CompactString::from(path)));
                    }
                    let path2 = format!("/usr/lib/x86_64-linux-gnu/{}", candidate);
                    if std::path::Path::new(&path2).exists() {
                        return Ok(PyObject::str_val(CompactString::from(path2)));
                    }
                }
                Ok(PyObject::none())
            }));
            PyObject::module_with_attrs(CompactString::from("ctypes.util"), util_attrs)
        }),
    ])
}
