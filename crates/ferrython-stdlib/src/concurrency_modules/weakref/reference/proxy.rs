use super::*;

pub(crate) fn make_proxy_fn(
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
                    Ok(PyObject::str_val(CompactString::from(referent.repr())))
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
