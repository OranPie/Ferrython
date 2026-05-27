use super::*;

fn build_userdict_copy(
    data: &PyObjectRef,
    owner_class: PyObjectRef,
    src_attrs: &SharedFxAttrMap,
) -> PyResult<PyObjectRef> {
    let new_data = if let PyObjectPayload::Dict(m) = &data.payload {
        PyObject::dict(m.read().clone())
    } else {
        PyObject::dict_from_pairs(vec![])
    };
    let new_inst = PyObject::instance(owner_class);
    if let PyObjectPayload::Instance(ref dst_inst) = new_inst.payload {
        dst_inst
            .attrs
            .write()
            .insert(CompactString::from("data"), new_data.clone());
        install_dict_methods(&dst_inst.attrs, &new_data, dst_inst.class.clone());
        copy_instance_attrs(
            src_attrs,
            &dst_inst.attrs,
            &[
                "data",
                "keys",
                "values",
                "items",
                "get",
                "pop",
                "setdefault",
                "update",
                "copy",
                "clear",
            ],
        );
    }
    Ok(new_inst)
}

// --- UserDict / UserList / UserString ---

pub(in crate::collection_modules) fn make_user_dict_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        native_method("UserDict", "__init__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("UserDict.__init__ requires self"));
            }
            let inst = &args[0];
            let data = if args.len() > 1 {
                if let PyObjectPayload::Dict(d) = &args[1].payload {
                    PyObject::wrap(PyObjectPayload::Dict(Rc::new(PyCell::new(
                        d.read().clone(),
                    ))))
                } else {
                    PyObject::dict_from_pairs(vec![])
                }
            } else {
                PyObject::dict_from_pairs(vec![])
            };
            if let PyObjectPayload::Instance(d) = &inst.payload {
                d.attrs
                    .write()
                    .insert(CompactString::from("data"), data.clone());
                // Install instance methods that directly operate on the data
                install_dict_methods(&d.attrs, &data, d.class.clone());
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__getitem__"),
        native_method("UserDict", "__getitem__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected key"));
            }
            let data = get_user_data(&args[0], "data")?;
            data.get_item(&args[1])
        }),
    );
    ns.insert(
        CompactString::from("__setitem__"),
        native_method("UserDict", "__setitem__", |args| {
            if args.len() < 3 {
                return Err(PyException::type_error("expected key and value"));
            }
            let data = get_user_data(&args[0], "data")?;
            if let PyObjectPayload::Dict(d) = &data.payload {
                let key = HashableKey::from_object(&args[1])?;
                d.write().insert(key, args[2].clone());
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__delitem__"),
        native_method("UserDict", "__delitem__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected key"));
            }
            let data = get_user_data(&args[0], "data")?;
            if let PyObjectPayload::Dict(d) = &data.payload {
                let key = HashableKey::from_object(&args[1])?;
                if d.write().shift_remove(&key).is_none() {
                    return Err(PyException::key_error(args[1].py_to_string()));
                }
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__len__"),
        native_method("UserDict", "__len__", |args| {
            let data = get_user_data(&args[0], "data")?;
            Ok(PyObject::int(data.py_len()? as i64))
        }),
    );
    ns.insert(
        CompactString::from("__contains__"),
        native_method("UserDict", "__contains__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected key"));
            }
            let data = get_user_data(&args[0], "data")?;
            if let PyObjectPayload::Dict(d) = &data.payload {
                let key = HashableKey::from_object(&args[1])?;
                Ok(PyObject::bool_val(d.read().contains_key(&key)))
            } else {
                Ok(PyObject::bool_val(false))
            }
        }),
    );
    ns.insert(
        CompactString::from("__repr__"),
        native_method("UserDict", "__repr__", |args| {
            let data = get_user_data(&args[0], "data")?;
            Ok(PyObject::str_val(CompactString::from(data.py_to_string())))
        }),
    );
    ns.insert(
        CompactString::from("__iter__"),
        native_method("UserDict", "__iter__", |args| {
            let data = get_user_data(&args[0], "data")?;
            data.get_iter()
        }),
    );
    ns.insert(
        CompactString::from("__eq__"),
        native_method("UserDict", "__eq__", |args| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let data = get_user_data(&args[0], "data")?;
            let other_data = if let Ok(od) = get_user_data(&args[1], "data") {
                od
            } else {
                args[1].clone()
            };
            if let (PyObjectPayload::Dict(a), PyObjectPayload::Dict(b)) =
                (&data.payload, &other_data.payload)
            {
                let ra = a.read();
                let rb = b.read();
                if ra.len() != rb.len() {
                    return Ok(PyObject::bool_val(false));
                }
                for (k, v) in ra.iter() {
                    match rb.get(k) {
                        Some(ov)
                            if v.compare(ov, CompareOp::Eq)
                                .map_or(false, |r| r.is_truthy()) => {}
                        _ => return Ok(PyObject::bool_val(false)),
                    }
                }
                Ok(PyObject::bool_val(true))
            } else {
                Ok(PyObject::bool_val(false))
            }
        }),
    );
    ns.insert(
        CompactString::from("__bool__"),
        native_method("UserDict", "__bool__", |args| {
            let data = get_user_data(&args[0], "data")?;
            Ok(PyObject::bool_val(data.py_len()? > 0))
        }),
    );
    ns.insert(
        CompactString::from("__or__"),
        native_method("UserDict", "__or__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected other"));
            }
            let data = get_user_data(&args[0], "data")?;
            let mut merged = IndexMap::new();
            if let PyObjectPayload::Dict(d) = &data.payload {
                for (k, v) in d.read().iter() {
                    merged.insert(k.clone(), v.clone());
                }
            }
            let other = if let Ok(od) = get_user_data(&args[1], "data") {
                od
            } else {
                args[1].clone()
            };
            if let PyObjectPayload::Dict(d) = &other.payload {
                for (k, v) in d.read().iter() {
                    merged.insert(k.clone(), v.clone());
                }
            }
            Ok(PyObject::dict(merged))
        }),
    );
    ns.insert(
        CompactString::from("__copy__"),
        native_method("UserDict", "__copy__", copy_userdict_instance),
    );
    ns.insert(
        CompactString::from("__ior__"),
        native_method("UserDict", "__ior__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected other"));
            }
            let data = get_user_data(&args[0], "data")?;
            let other = if let Ok(od) = get_user_data(&args[1], "data") {
                od
            } else {
                args[1].clone()
            };
            if let (PyObjectPayload::Dict(dst), PyObjectPayload::Dict(src)) =
                (&data.payload, &other.payload)
            {
                let mut w = dst.write();
                for (k, v) in src.read().iter() {
                    w.insert(k.clone(), v.clone());
                }
            }
            Ok(args[0].clone())
        }),
    );
    PyObject::class(CompactString::from("UserDict"), vec![], ns)
}

fn install_dict_methods(attrs: &SharedFxAttrMap, data: &PyObjectRef, owner_class: PyObjectRef) {
    let map = if let PyObjectPayload::Dict(m) = &data.payload {
        m.clone()
    } else {
        return;
    };
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("keys"),
        PyObject::native_closure("keys", move |_| {
            Ok(PyObject::wrap(PyObjectPayload::DictKeys {
                map: m.clone(),
                owner: None,
            }))
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("values"),
        PyObject::native_closure("values", move |_| {
            Ok(PyObject::wrap(PyObjectPayload::DictValues {
                map: m.clone(),
                owner: None,
            }))
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("items"),
        PyObject::native_closure("items", move |_| {
            Ok(PyObject::wrap(PyObjectPayload::DictItems {
                map: m.clone(),
                owner: None,
            }))
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("get"),
        PyObject::native_closure("get", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "get() requires at least 1 argument",
                ));
            }
            let key = args[0].to_hashable_key()?;
            let default = if args.len() >= 2 {
                args[1].clone()
            } else {
                PyObject::none()
            };
            Ok(m.read().get(&key).cloned().unwrap_or(default))
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("pop"),
        PyObject::native_closure("pop", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "pop() requires at least 1 argument",
                ));
            }
            let key = args[0].to_hashable_key()?;
            let default = if args.len() >= 2 {
                Some(args[1].clone())
            } else {
                None
            };
            match m.write().shift_remove(&key) {
                Some(v) => Ok(v),
                None => default.ok_or_else(|| PyException::key_error(args[0].py_to_string())),
            }
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("setdefault"),
        PyObject::native_closure("setdefault", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "setdefault() requires at least 1 argument",
                ));
            }
            let key = args[0].to_hashable_key()?;
            let default = if args.len() >= 2 {
                args[1].clone()
            } else {
                PyObject::none()
            };
            let mut w = m.write();
            if let Some(v) = w.get(&key) {
                return Ok(v.clone());
            }
            w.insert(key, default.clone());
            Ok(default)
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("update"),
        PyObject::native_closure("update", move |args| {
            if !args.is_empty() {
                if let PyObjectPayload::Dict(other) = &args[0].payload {
                    let mut w = m.write();
                    for (k, v) in other.read().iter() {
                        w.insert(k.clone(), v.clone());
                    }
                }
            }
            Ok(PyObject::none())
        }),
    );
    attrs.write().insert(CompactString::from("copy"), {
        let attrs = attrs.clone();
        let data = data.clone();
        let owner_class = owner_class.clone();
        PyObject::native_closure("copy", move |_| {
            build_userdict_copy(&data, owner_class.clone(), &attrs)
        })
    });
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("clear"),
        PyObject::native_closure("clear", move |_| {
            m.write().clear();
            Ok(PyObject::none())
        }),
    );
}

fn copy_userdict_instance(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("UserDict.__copy__ requires self"));
    }
    let self_obj = &args[0];
    let inst = if let PyObjectPayload::Instance(inst) = &self_obj.payload {
        inst
    } else {
        return Err(PyException::type_error(
            "UserDict.__copy__ requires an instance",
        ));
    };
    let data = get_user_data(self_obj, "data")?;
    build_userdict_copy(&data, inst.class.clone(), &inst.attrs)
}
