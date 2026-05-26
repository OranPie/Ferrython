use super::*;
use std::cell::RefCell;

fn finalize_kwargs_marker_key() -> HashableKey {
    HashableKey::str_key(CompactString::from("__finalize_kwargs__"))
}

fn extract_marked_kwargs(
    args: &[PyObjectRef],
    marker: HashableKey,
) -> (Vec<PyObjectRef>, IndexMap<HashableKey, PyObjectRef>) {
    let Some((last, rest)) = args.split_last() else {
        return (Vec::new(), IndexMap::new());
    };
    let PyObjectPayload::Dict(map) = &last.payload else {
        return (args.to_vec(), IndexMap::new());
    };
    let map = map.read();
    if !map.contains_key(&marker) {
        return (args.to_vec(), IndexMap::new());
    }
    let kwargs = map
        .iter()
        .filter(|(key, _)| *key != &marker)
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    (rest.to_vec(), kwargs)
}

fn kwargs_without_keys(
    kwargs: &IndexMap<HashableKey, PyObjectRef>,
    names: &[&str],
) -> IndexMap<HashableKey, PyObjectRef> {
    kwargs
        .iter()
        .filter(|(key, _)| {
            !matches!(key, HashableKey::Str(s) if names.iter().any(|name| s.as_str() == *name))
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn kwargs_to_call_pairs(
    kwargs: &IndexMap<HashableKey, PyObjectRef>,
) -> PyResult<Vec<(CompactString, PyObjectRef)>> {
    let mut pairs = Vec::with_capacity(kwargs.len());
    for (key, value) in kwargs {
        let HashableKey::Str(name) = key else {
            return Err(PyException::type_error("keywords must be strings"));
        };
        pairs.push((CompactString::from(name.as_str()), value.clone()));
    }
    Ok(pairs)
}

fn warn_finalize_keyword_form() -> PyResult<()> {
    if let Some(warnings) = crate::load_module("warnings") {
        if let (Some(warn_fn), Some(dep_cls)) = (
            warnings.get_attr("warn"),
            warnings.get_attr("DeprecationWarning"),
        ) {
            call_callable(
                &warn_fn,
                &[
                    PyObject::str_val(CompactString::from(
                        "Passing obj or func as keyword arguments to weakref.finalize is deprecated",
                    )),
                    dep_cls,
                ],
            )?;
        }
    }
    Ok(())
}

fn finalize_call_from_state(
    alive_state: &Rc<Cell<bool>>,
    attrs_ref: &Rc<PyCell<FxAttrMap>>,
    func: &PyObjectRef,
    extra: &[PyObjectRef],
    kwargs: &[(CompactString, PyObjectRef)],
) -> PyResult<PyObjectRef> {
    if !alive_state.replace(false) {
        return Ok(PyObject::none());
    }
    {
        let mut attrs = attrs_ref.write();
        attrs.insert(CompactString::from("alive"), PyObject::bool_val(false));
        attrs.shift_remove("_func");
        attrs.shift_remove("_args");
        attrs.shift_remove("_kwargs");
        attrs.insert(
            CompactString::from("__call__"),
            PyObject::native_closure("finalize.__call__", |_| Ok(PyObject::none())),
        );
        attrs.insert(
            CompactString::from("detach"),
            PyObject::native_closure("finalize.detach", |_| Ok(PyObject::none())),
        );
        attrs.insert(
            CompactString::from("peek"),
            PyObject::native_closure("finalize.peek", |_| Ok(PyObject::none())),
        );
    }
    match &func.payload {
        PyObjectPayload::NativeFunction(nf) if kwargs.is_empty() => (nf.func)(extra),
        PyObjectPayload::NativeClosure(nc) if kwargs.is_empty() => (nc.func)(extra),
        _ => call_callable_kw(func, extra, kwargs.to_vec()),
    }
}

fn finalize_new(args: &[PyObjectRef], reference_type: &PyObjectRef) -> PyResult<PyObjectRef> {
    let marker = finalize_kwargs_marker_key();
    let (pos_args, kwargs) = extract_marked_kwargs(args, marker);
    if pos_args.is_empty() {
        return Err(PyException::type_error("finalize requires obj and func"));
    }
    let cls = pos_args[0].clone();
    let user_args = &pos_args[1..];
    let (obj, func, extra_start, call_kwargs) = if user_args.len() >= 2 {
        (user_args[0].clone(), user_args[1].clone(), 2, kwargs)
    } else {
        warn_finalize_keyword_form()?;
        let obj = if !user_args.is_empty() {
            Some(user_args[0].clone())
        } else {
            kwargs
                .get(&HashableKey::str_key(CompactString::from("obj")))
                .cloned()
        };
        let func = kwargs
            .get(&HashableKey::str_key(CompactString::from("func")))
            .cloned();
        let Some(obj) = obj else {
            return Err(PyException::type_error("finalize requires obj and func"));
        };
        let Some(func) = func else {
            return Err(PyException::type_error("finalize requires obj and func"));
        };
        (
            obj,
            func,
            user_args.len(),
            kwargs_without_keys(
                &kwargs,
                if user_args.is_empty() {
                    &["obj", "func"]
                } else {
                    &["func"]
                },
            ),
        )
    };

    let weak: PyWeakRef = PyObjectRef::downgrade(&obj);
    let extra = user_args[extra_start..].to_vec();
    let kwargs_obj = PyObject::dict(call_kwargs.clone());
    let kwargs_for_call = kwargs_to_call_pairs(&call_kwargs)?;

    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let alive_state = Rc::new(Cell::new(true));
        let atexit_handle: Rc<RefCell<Option<PyObjectRef>>> = Rc::new(RefCell::new(None));
        let attrs_ref = inst_data.attrs.clone();
        {
            let mut attrs = attrs_ref.write();
            attrs.insert(CompactString::from("alive"), PyObject::bool_val(true));
            attrs.insert(CompactString::from("atexit"), PyObject::bool_val(true));
            attrs.insert(CompactString::from("_func"), func.clone());
            attrs.insert(CompactString::from("_args"), PyObject::tuple(extra.clone()));
            attrs.insert(CompactString::from("_kwargs"), kwargs_obj.clone());

            let w_det = weak.clone();
            let f_det = func.clone();
            let e_det = extra.clone();
            let k_det = kwargs_obj.clone();
            let alive_det = alive_state.clone();
            let attrs_det = attrs_ref.clone();
            let atexit_det = atexit_handle.clone();
            attrs.insert(
                CompactString::from("detach"),
                PyObject::native_closure("finalize.detach", move |_| {
                    if !alive_det.replace(false) {
                        return Ok(PyObject::none());
                    }
                    if let Some(callback) = atexit_det.borrow_mut().take() {
                        crate::sys_modules::unregister_atexit_callback(&callback);
                    }
                    attrs_det
                        .write()
                        .insert(CompactString::from("alive"), PyObject::bool_val(false));
                    match w_det.upgrade() {
                        Some(obj) => Ok(PyObject::tuple(vec![
                            obj,
                            f_det.clone(),
                            PyObject::tuple(e_det.clone()),
                            k_det.clone(),
                        ])),
                        None => Ok(PyObject::none()),
                    }
                }),
            );

            let w_peek = weak.clone();
            let f_peek = func.clone();
            let e_peek = extra.clone();
            let k_peek = kwargs_obj.clone();
            let alive_peek = alive_state.clone();
            attrs.insert(
                CompactString::from("peek"),
                PyObject::native_closure("finalize.peek", move |_| {
                    if !alive_peek.get() {
                        return Ok(PyObject::none());
                    }
                    match w_peek.upgrade() {
                        Some(obj) => Ok(PyObject::tuple(vec![
                            obj,
                            f_peek.clone(),
                            PyObject::tuple(e_peek.clone()),
                            k_peek.clone(),
                        ])),
                        None => Ok(PyObject::none()),
                    }
                }),
            );

            let f_call = func.clone();
            let e_call = extra.clone();
            let k_call = kwargs_for_call.clone();
            let alive_call = alive_state.clone();
            let attrs_call = attrs_ref.clone();
            let atexit_call = atexit_handle.clone();
            attrs.insert(
                CompactString::from("__call__"),
                PyObject::native_closure("finalize.__call__", move |_| {
                    if let Some(callback) = atexit_call.borrow_mut().take() {
                        crate::sys_modules::unregister_atexit_callback(&callback);
                    }
                    finalize_call_from_state(&alive_call, &attrs_call, &f_call, &e_call, &k_call)
                }),
            );
        }

        let weak_inst = PyObject::instance(reference_type.clone());
        if let PyObjectPayload::Instance(ref weak_data) = weak_inst.payload {
            let mut weak_attrs = weak_data.attrs.write();
            let w_target = weak.clone();
            weak_attrs.insert(
                CompactString::from("__weakref_target__"),
                PyObject::native_closure("finalize.weakref.__target__", move |_| {
                    Ok(upgrade_or_none(&w_target))
                }),
            );
        }

        let f_exit = func.clone();
        let e_exit = extra.clone();
        let k_exit = kwargs_for_call.clone();
        let alive_exit = alive_state.clone();
        let attrs_exit = attrs_ref.clone();
        let inst_exit = inst.clone();
        let atexit_callback = PyObject::native_closure("finalize.__atexit__", move |_| {
            if let Some(atexit) = inst_exit.get_attr("atexit") {
                if !atexit.is_truthy() {
                    return Ok(PyObject::none());
                }
            }
            finalize_call_from_state(&alive_exit, &attrs_exit, &f_exit, &e_exit, &k_exit)
        });
        *atexit_handle.borrow_mut() = Some(atexit_callback.clone());
        crate::sys_modules::register_atexit_callback(atexit_callback, Vec::new(), Vec::new());

        let f_auto = func;
        let e_auto = extra;
        let k_auto = kwargs_for_call;
        let alive_auto = alive_state;
        let attrs_auto = attrs_ref;
        let atexit_auto = atexit_handle;
        let inst_keepalive = inst.clone();
        let weak_keepalive = weak_inst.clone();
        let callback = Some(PyObject::native_closure(
            "finalize.__callback__",
            move |_| {
                if let Some(callback) = atexit_auto.borrow_mut().take() {
                    crate::sys_modules::unregister_atexit_callback(&callback);
                }
                let _keepalive_inst = &inst_keepalive;
                let _keepalive = &weak_keepalive;
                finalize_call_from_state(&alive_auto, &attrs_auto, &f_auto, &e_auto, &k_auto)
            },
        ));
        PyObjectRef::register_weak_object(&obj, &weak_inst, callback, WeakObjectKind::Ref);
    }
    Ok(inst)
}

pub(super) fn create_finalize_type(reference_type: &PyObjectRef) -> PyObjectRef {
    let mut finalize_namespace = IndexMap::new();
    let finalize_new_ref_type = reference_type.clone();
    finalize_namespace.insert(
        CompactString::from("__new__"),
        PyObject::native_closure("finalize.__new__", move |args| {
            finalize_new(args, &finalize_new_ref_type)
        }),
    );
    finalize_namespace.insert(
        CompactString::from("__init__"),
        PyObject::native_function("finalize.__init__", |_| Ok(PyObject::none())),
    );
    PyObject::class(CompactString::from("finalize"), vec![], finalize_namespace)
}
