use super::*;

fn is_weak_method_ref(obj: &PyObjectRef) -> bool {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        inst.attrs.read().contains_key("__weakmethod__")
    } else {
        false
    }
}

fn weak_method_parts(obj: &PyObjectRef) -> PyResult<Option<(PyObjectRef, PyObjectRef)>> {
    let Some(call) = obj.get_attr("__call__") else {
        return Ok(None);
    };
    let bound = call_callable(&call, &[])?;
    if matches!(&bound.payload, PyObjectPayload::None) {
        return Ok(None);
    }
    if let PyObjectPayload::BoundMethod { receiver, method } = &bound.payload {
        Ok(Some((receiver.clone(), method.clone())))
    } else {
        Ok(None)
    }
}

fn compare_weak_methods(
    this: &PyObjectRef,
    other: &PyObjectRef,
    op: CompareOp,
) -> PyResult<PyObjectRef> {
    let eq = match (weak_method_parts(this)?, weak_method_parts(other)?) {
        (Some((this_receiver, this_func)), Some((other_receiver, other_func))) => {
            PyObjectRef::ptr_eq(&this_func, &other_func)
                && this_receiver
                    .compare(&other_receiver, CompareOp::Eq)?
                    .is_truthy()
        }
        _ => false,
    };
    Ok(PyObject::bool_val(if matches!(op, CompareOp::Eq) {
        eq
    } else {
        !eq
    }))
}

fn weak_ref_call(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if let Some(this) = args.first() {
        if let PyObjectPayload::Instance(inst) = &this.payload {
            if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
                return call_callable(&target_fn, &[]);
            }
        }
    }
    Ok(PyObject::none())
}

fn weak_ref_init(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let marker = HashableKey::str_key(CompactString::from("__weakref_ref_kwargs__"));
    let (args, has_kwargs_marker) =
        if let Some(PyObjectPayload::Dict(map)) = args.last().map(|arg| &arg.payload) {
            let has_marker = map.read().contains_key(&marker);
            if has_marker {
                (&args[..args.len() - 1], true)
            } else {
                (args, false)
            }
        } else {
            (args, false)
        };
    if has_kwargs_marker {
        return Err(PyException::type_error("ref() takes no keyword arguments"));
    }
    if args.len() > 3 {
        return Err(PyException::type_error(format!(
            "__init__() takes at most 2 arguments ({} given)",
            args.len().saturating_sub(1)
        )));
    }
    Ok(PyObject::none())
}

fn weak_ref_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "ref.__new__ requires type and object",
        ));
    }
    let marker = HashableKey::str_key(CompactString::from("__weakref_ref_kwargs__"));
    let (args, has_kwargs_marker) =
        if let Some(PyObjectPayload::Dict(map)) = args.last().map(|arg| &arg.payload) {
            let has_marker = map.read().contains_key(&marker);
            if has_marker {
                (&args[..args.len() - 1], true)
            } else {
                (args, false)
            }
        } else {
            (args, false)
        };
    if args.len() > 3 {
        return Err(PyException::type_error(format!(
            "ref() takes at most 2 arguments ({} given)",
            args.len() - 1
        )));
    }
    let cls = args[0].clone();
    if has_kwargs_marker {
        if let PyObjectPayload::Class(cd) = &cls.payload {
            if cd.name.as_str() == "weakref" {
                return Err(PyException::type_error("ref() takes no keyword arguments"));
            }
        }
    }
    let target = args[1].clone();
    let callback = args.get(2).cloned().unwrap_or_else(PyObject::none);
    let callback = if matches!(callback.payload, PyObjectPayload::None) {
        None
    } else {
        Some(callback)
    };
    if callback.is_none() {
        if let PyObjectPayload::Class(cd) = &cls.payload {
            if cd.name.as_str() == "weakref" {
                if let Some(existing) =
                    PyObjectRef::find_shared_weak_object(&target, WeakObjectKind::Ref)
                {
                    if let PyObjectPayload::Instance(inst) = &existing.payload {
                        if let PyObjectPayload::Class(cd) = &inst.class.payload {
                            if cd.name.as_str() == "weakref" {
                                return Ok(existing);
                            }
                        }
                    }
                }
            }
        }
    }
    let weak: PyWeakRef = PyObjectRef::downgrade(&target);
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(
            CompactString::from("__weakref_ref__"),
            PyObject::bool_val(true),
        );
        attrs.insert(
            CompactString::from("__weakref_callback__"),
            callback.clone().unwrap_or_else(PyObject::none),
        );
        let target_weak = weak.clone();
        attrs.insert(
            CompactString::from("__weakref_target__"),
            PyObject::native_closure("weakref.__target__", move |_| {
                Ok(upgrade_or_none(&target_weak))
            }),
        );
        let repr_weak = weak.clone();
        attrs.insert(
            CompactString::from("__repr__"),
            PyObject::native_closure("weakref.__repr__", move |_| {
                if repr_weak.upgrade().is_some() {
                    Ok(PyObject::str_val(CompactString::from("<weakref (alive)>")))
                } else {
                    Ok(PyObject::str_val(CompactString::from("<weakref (dead)>")))
                }
            }),
        );
        let bool_weak = weak.clone();
        attrs.insert(
            CompactString::from("__bool__"),
            PyObject::native_closure("weakref.__bool__", move |_| {
                Ok(PyObject::bool_val(bool_weak.upgrade().is_some()))
            }),
        );
    }
    PyObjectRef::register_weak_object(&target, &inst, callback, WeakObjectKind::Ref);
    Ok(inst)
}

pub(super) fn configure_reference_type(reference_type: &PyObjectRef) {
    let mut reference_namespace = IndexMap::new();
    reference_namespace.insert(
        CompactString::from("__new__"),
        PyObject::native_function("weakref.__new__", weak_ref_new),
    );
    reference_namespace.insert(
        CompactString::from("__init__"),
        PyObject::native_function("weakref.__init__", weak_ref_init),
    );
    reference_namespace.insert(
        CompactString::from("__call__"),
        PyObject::native_function("weakref.__call__", weak_ref_call),
    );
    reference_namespace.insert(
        CompactString::from("__eq__"),
        PyObject::native_closure("weakref.__eq__", |args| {
            if let (Some(this), Some(other)) = (args.first(), args.get(1)) {
                if PyObjectRef::ptr_eq(this, other) {
                    return Ok(PyObject::bool_val(true));
                }
                if is_weak_method_ref(this) || is_weak_method_ref(other) {
                    if !is_weak_method_ref(this) || !is_weak_method_ref(other) {
                        return Ok(PyObject::bool_val(false));
                    }
                    return compare_weak_methods(this, other, CompareOp::Eq);
                }
                let this_obj = weak_ref_call(&[this.clone()])?;
                let other_obj = weak_ref_call(&[other.clone()])?;
                if matches!(&this_obj.payload, PyObjectPayload::None)
                    || matches!(&other_obj.payload, PyObjectPayload::None)
                {
                    return Ok(PyObject::bool_val(false));
                }
                return this_obj.compare(&other_obj, CompareOp::Eq);
            }
            Ok(PyObject::bool_val(false))
        }),
    );
    reference_namespace.insert(
        CompactString::from("__ne__"),
        PyObject::native_closure("weakref.__ne__", |args| {
            if let (Some(this), Some(other)) = (args.first(), args.get(1)) {
                if PyObjectRef::ptr_eq(this, other) {
                    return Ok(PyObject::bool_val(false));
                }
                if is_weak_method_ref(this) || is_weak_method_ref(other) {
                    if !is_weak_method_ref(this) || !is_weak_method_ref(other) {
                        return Ok(PyObject::bool_val(true));
                    }
                    return compare_weak_methods(this, other, CompareOp::Ne);
                }
                let this_obj = weak_ref_call(&[this.clone()])?;
                let other_obj = weak_ref_call(&[other.clone()])?;
                if matches!(&this_obj.payload, PyObjectPayload::None)
                    || matches!(&other_obj.payload, PyObjectPayload::None)
                {
                    return Ok(PyObject::bool_val(true));
                }
                return this_obj.compare(&other_obj, CompareOp::Ne);
            }
            Ok(PyObject::bool_val(true))
        }),
    );
    if let PyObjectPayload::Class(cd) = &reference_type.payload {
        cd.namespace.write().extend(reference_namespace);
        cd.has_custom_new.set(true);
        cd.is_simple_class.set(false);
        cd.method_cache.write().clear();
    }
}

pub(super) fn make_proxy_fn(
    proxy_type: &PyObjectRef,
    callable_proxy_type: &PyObjectRef,
) -> PyObjectRef {
    let proxy_constructor_type = proxy_type.clone();
    let callable_proxy_constructor_type = callable_proxy_type.clone();
    PyObject::native_closure("weakref.proxy", move |args| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "proxy() requires at least 1 argument",
            ));
        }
        let callback = args.get(1).cloned().unwrap_or_else(PyObject::none);
        let callback = if matches!(callback.payload, PyObjectPayload::None) {
            None
        } else {
            Some(callback)
        };
        if callback.is_none() {
            if let Some(existing) =
                PyObjectRef::find_shared_weak_object(&args[0], WeakObjectKind::Proxy)
            {
                return Ok(existing);
            }
        }
        let weak: PyWeakRef = PyObjectRef::downgrade(&args[0]);

        let callable = args[0].is_callable();
        let cls = if callable {
            callable_proxy_constructor_type.clone()
        } else {
            proxy_constructor_type.clone()
        };
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();

            let w_target = weak.clone();
            attrs.insert(
                CompactString::from("__weakref_target__"),
                PyObject::native_closure("__weakref_target__", move |_| upgrade_or_err(&w_target)),
            );

            let w_ga = weak.clone();
            attrs.insert(
                CompactString::from("__getattr__"),
                PyObject::native_closure("weakproxy.__getattr__", move |args| {
                    let referent = upgrade_or_err(&w_ga)?;
                    if let Some(name_obj) = args.first() {
                        let name = name_obj.py_to_string();
                        referent.get_attr(&name).ok_or_else(|| {
                            PyException::attribute_error(format!(
                                "'weakproxy' object has no attribute '{}'",
                                name
                            ))
                        })
                    } else {
                        Err(PyException::type_error(
                            "__getattr__ requires a name argument",
                        ))
                    }
                }),
            );

            let w_r = weak.clone();
            attrs.insert(
                CompactString::from("__repr__"),
                PyObject::native_closure("weakproxy.__repr__", move |_| match w_r.upgrade() {
                    Some(obj) => Ok(PyObject::str_val(CompactString::from(format!(
                        "<weakproxy at {:p}>",
                        PyObjectRef::as_ptr(&obj)
                    )))),
                    None => Err(PyException::new(
                        ExceptionKind::ReferenceError,
                        "weakly-referenced object no longer exists",
                    )),
                }),
            );

            let w_b = weak.clone();
            attrs.insert(
                CompactString::from("__bool__"),
                PyObject::native_closure("weakproxy.__bool__", move |_| {
                    let referent = upgrade_or_err(&w_b)?;
                    Ok(PyObject::bool_val(referent.is_truthy()))
                }),
            );

            let w_s = weak.clone();
            attrs.insert(
                CompactString::from("__str__"),
                PyObject::native_closure("weakproxy.__str__", move |_| {
                    let referent = upgrade_or_err(&w_s)?;
                    Ok(PyObject::str_val(CompactString::from(
                        referent.py_to_string(),
                    )))
                }),
            );

            let w_c = weak.clone();
            attrs.insert(
                CompactString::from("__call__"),
                PyObject::native_closure("weakproxy.__call__", move |args| {
                    let referent = upgrade_or_err(&w_c)?;
                    if !referent.is_callable() {
                        return Err(PyException::type_error(
                            "weakproxy object is not directly callable; access attributes instead",
                        ));
                    }
                    let mut call_args = args.to_vec();
                    let kwargs = match call_args.last() {
                        Some(last) => match &last.payload {
                            PyObjectPayload::Dict(map) => {
                                let mut kwargs = Vec::new();
                                for (key, value) in map.read().iter() {
                                    if let HashableKey::Str(name) = key {
                                        kwargs.push((name.to_compact_string(), value.clone()));
                                    } else {
                                        return Err(PyException::type_error(
                                            "keywords must be strings",
                                        ));
                                    }
                                }
                                call_args.pop();
                                kwargs
                            }
                            _ => Vec::new(),
                        },
                        None => Vec::new(),
                    };
                    if kwargs.is_empty() {
                        call_callable(&referent, &call_args)
                    } else {
                        call_callable_kw(&referent, &call_args, kwargs)
                    }
                }),
            );
        }
        PyObjectRef::register_weak_object(&args[0], &inst, callback, WeakObjectKind::Proxy);
        Ok(inst)
    })
}

pub(super) fn make_weak_method_fn(reference_type: &PyObjectRef) -> PyObjectRef {
    let reference_type = reference_type.clone();
    PyObject::native_closure("weakref.WeakMethod", move |args| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "WeakMethod requires at least 1 argument",
            ));
        }
        if args.len() > 2 {
            return Err(PyException::type_error(
                "WeakMethod expected at most 2 arguments",
            ));
        }
        let method = args[0].clone();
        let (receiver, func) = match &method.payload {
            PyObjectPayload::BoundMethod { receiver, method } => (receiver.clone(), method.clone()),
            PyObjectPayload::BuiltinBoundMethod(bbm) => (
                bbm.receiver.clone(),
                PyObject::str_val(bbm.method_name.clone()),
            ),
            _ => {
                let receiver = method.get_attr("__self__").ok_or_else(|| {
                    PyException::type_error("argument should be a bound method, not other callable")
                })?;
                let func = method.get_attr("__func__").ok_or_else(|| {
                    PyException::type_error("argument should be a bound method, not other callable")
                })?;
                (receiver, func)
            }
        };
        let callback = args.get(1).cloned().unwrap_or_else(PyObject::none);
        let callback = if matches!(callback.payload, PyObjectPayload::None) {
            None
        } else {
            Some(callback)
        };
        let weak_receiver = PyObjectRef::downgrade(&receiver);
        let weak_func = PyObjectRef::downgrade(&func);
        let cls = PyObject::class(
            CompactString::from("WeakMethod"),
            vec![reference_type.clone()],
            IndexMap::new(),
        );
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(
                CompactString::from("__weakref_ref__"),
                PyObject::bool_val(true),
            );
            w.insert(
                CompactString::from("__weakmethod__"),
                PyObject::bool_val(true),
            );
            w.insert(
                CompactString::from("__weakref_callback__"),
                callback.clone().unwrap_or_else(PyObject::none),
            );
            let call_receiver = weak_receiver.clone();
            let call_func = weak_func.clone();
            w.insert(
                CompactString::from("__call__"),
                PyObject::native_closure("WeakMethod.__call__", move |_| {
                    let Some(receiver) = call_receiver.upgrade() else {
                        return Ok(PyObject::none());
                    };
                    let Some(func) = call_func.upgrade() else {
                        return Ok(PyObject::none());
                    };
                    Ok(PyObject::wrap(PyObjectPayload::BoundMethod {
                        receiver,
                        method: func,
                    }))
                }),
            );
            let bool_receiver = weak_receiver.clone();
            let bool_func = weak_func.clone();
            w.insert(
                CompactString::from("__bool__"),
                PyObject::native_closure("WeakMethod.__bool__", move |_| {
                    Ok(PyObject::bool_val(
                        bool_receiver.upgrade().is_some() && bool_func.upgrade().is_some(),
                    ))
                }),
            );
        }
        if let Some(callback) = callback {
            let fired = Rc::new(Cell::new(false));
            let cb1 = callback.clone();
            let weak_method = inst.clone();
            let fired1 = fired.clone();
            let callback_wrapper = PyObject::native_closure("WeakMethod.callback", move |_| {
                if !fired1.replace(true) {
                    call_callable(&cb1, &[weak_method.clone()])?;
                }
                Ok(PyObject::none())
            });
            PyObjectRef::register_weak_object(
                &receiver,
                &inst,
                Some(callback_wrapper.clone()),
                WeakObjectKind::Ref,
            );
            let cb2 = callback;
            let weak_method = inst.clone();
            let fired2 = fired;
            let callback_wrapper = PyObject::native_closure("WeakMethod.callback", move |_| {
                if !fired2.replace(true) {
                    call_callable(&cb2, &[weak_method.clone()])?;
                }
                Ok(PyObject::none())
            });
            PyObjectRef::register_weak_object(
                &func,
                &inst,
                Some(callback_wrapper),
                WeakObjectKind::Ref,
            );
        }
        Ok(inst)
    })
}
