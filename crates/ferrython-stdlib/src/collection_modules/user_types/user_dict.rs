use super::*;

const USERDICT_KW_MARKER: &str = "__userdict_kwargs__";

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
                "popitem",
                "setdefault",
                "update",
                "copy",
                "clear",
            ],
        );
    }
    Ok(new_inst)
}

fn userdict_pair_from_object(item: PyObjectRef) -> PyResult<(HashableKey, PyObjectRef)> {
    match &item.payload {
        PyObjectPayload::Tuple(items) if items.len() == 2 => {
            return Ok((items[0].to_hashable_key()?, items[1].clone()));
        }
        PyObjectPayload::List(items) if items.read().len() == 2 => {
            let items = items.read();
            return Ok((items[0].to_hashable_key()?, items[1].clone()));
        }
        _ => {}
    }

    let pair = item.to_list()?;
    if pair.len() != 2 {
        return Err(PyException::value_error(
            "dictionary update sequence element has length other than 2",
        ));
    }
    Ok((pair[0].to_hashable_key()?, pair[1].clone()))
}

fn userdict_iter_objects(source: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
    match source.to_list() {
        Ok(items) => return Ok(items),
        Err(err) if err.kind == ExceptionKind::TypeError => {}
        Err(err) => return Err(err),
    }

    let iter = source.get_iter()?;
    let next = iter.get_attr("__next__").ok_or_else(|| {
        PyException::type_error(format!("'{}' object is not an iterator", iter.type_name()))
    })?;
    let mut items = Vec::new();
    loop {
        match call_callable(&next, &[]) {
            Ok(item) => items.push(item),
            Err(err) if err.kind == ExceptionKind::StopIteration => break,
            Err(err) => return Err(err),
        }
    }
    Ok(items)
}

fn userdict_value_repr(value: &PyObjectRef, depth: usize) -> PyResult<String> {
    if let PyObjectPayload::Instance(_) = &value.payload {
        if let Ok(data) = get_user_data(value, "data") {
            if let PyObjectPayload::Dict(map) = &data.payload {
                let ptr = PyObjectRef::as_ptr(value) as usize;
                if !repr_enter(ptr) {
                    if ferrython_core::object::helpers::repr_depth_exceeded() {
                        return Err(PyException::recursion_error(
                            "maximum recursion depth exceeded while getting the repr of an object",
                        ));
                    }
                    return Ok("{...}".to_string());
                }
                let result = userdict_format_map(&map.read(), depth + 1);
                repr_leave(ptr);
                return result;
            }
        }
        if let Some(repr_method) = value.get_attr("__repr__") {
            if !matches!(&repr_method.payload, PyObjectPayload::BuiltinBoundMethod(_)) {
                let args = match &repr_method.payload {
                    PyObjectPayload::NativeFunction(_) | PyObjectPayload::NativeClosure(_) => {
                        vec![value.clone()]
                    }
                    _ => vec![],
                };
                return Ok(call_callable(&repr_method, &args)?.py_to_string());
            }
        }
    }
    Ok(value.repr())
}

fn userdict_format_map(map: &FxHashKeyMap, depth: usize) -> PyResult<String> {
    if depth > ferrython_core::object::repr_recursion_limit() {
        return Err(PyException::recursion_error(
            "maximum recursion depth exceeded while getting the repr of an object",
        ));
    }
    let mut inner = Vec::new();
    for (key, value) in map.iter().filter(|(key, _)| !is_hidden_dict_key(key)) {
        inner.push(format!(
            "{}: {}",
            key.to_object().repr(),
            userdict_value_repr(value, depth)?
        ));
    }
    Ok(format!("{{{}}}", inner.join(", ")))
}

fn userdict_repr(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    let data = get_user_data(obj, "data")?;
    if let PyObjectPayload::Dict(map) = &data.payload {
        let ptr = PyObjectRef::as_ptr(obj) as usize;
        if !repr_enter(ptr) {
            if ferrython_core::object::helpers::repr_depth_exceeded() {
                return Err(PyException::recursion_error(
                    "maximum recursion depth exceeded while getting the repr of an object",
                ));
            }
            return Ok(PyObject::str_val(CompactString::from("{...}")));
        }
        let result = userdict_format_map(&map.read(), 0);
        repr_leave(ptr);
        Ok(PyObject::str_val(CompactString::from(result?)))
    } else {
        Ok(PyObject::str_val(CompactString::from(data.py_to_string())))
    }
}

fn userdict_update_from_object(target: &mut FxHashKeyMap, source: &PyObjectRef) -> PyResult<()> {
    if let PyObjectPayload::Dict(src) = &source.payload {
        for (key, value) in src.read().iter() {
            if let HashableKey::Str(name) = key {
                if name.as_str() == USERDICT_KW_MARKER {
                    continue;
                }
            }
            target.insert(key.clone(), value.clone());
        }
        return Ok(());
    }

    if let Ok(data) = get_user_data(source, "data") {
        return userdict_update_from_object(target, &data);
    }

    if let Some(keys_fn) = source.get_attr("keys") {
        let keys = call_callable(&keys_fn, &[])?;
        for key in userdict_iter_objects(&keys)? {
            let value = source.get_item(&key)?;
            target.insert(key.to_hashable_key()?, value);
        }
        return Ok(());
    }

    for item in userdict_iter_objects(source)? {
        let (key, value) = userdict_pair_from_object(item)?;
        target.insert(key, value);
    }
    Ok(())
}

fn userdict_update_from_kwargs(target: &mut FxHashKeyMap, kwargs: &PyObjectRef) -> PyResult<bool> {
    let mut had_marker = false;
    let PyObjectPayload::Dict(src) = &kwargs.payload else {
        return Ok(false);
    };

    for (key, value) in src.read().iter() {
        if let HashableKey::Str(name) = key {
            if name.as_str() == USERDICT_KW_MARKER {
                had_marker = true;
                continue;
            }
        }
        target.insert(key.clone(), value.clone());
    }
    Ok(had_marker)
}

fn userdict_has_construct_kwargs(kwargs: &PyObjectRef) -> bool {
    let PyObjectPayload::Dict(src) = &kwargs.payload else {
        return false;
    };

    let marker_key = HashableKey::str_key(CompactString::from(USERDICT_KW_MARKER));
    if !src.read().contains_key(&marker_key) {
        return false;
    }
    src.write().shift_remove(&marker_key);
    true
}

fn userdict_pop_legacy_dict_kw(kwargs: &PyObjectRef) -> Option<PyObjectRef> {
    let PyObjectPayload::Dict(src) = &kwargs.payload else {
        return None;
    };
    if src.read().len() == 1 {
        let dict_key = HashableKey::str_key(CompactString::from("dict"));
        if let Some(value) = src.write().shift_remove(&dict_key) {
            emit_deprecation_warning("Passing 'dict' as keyword argument is deprecated");
            return Some(value);
        }
    }
    None
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
            let mut data_map = new_fx_hashkey_map();
            if args.len() > 3 {
                return Err(PyException::type_error(
                    "UserDict expected at most 1 positional argument",
                ));
            }
            let mut positional_end = args.len();
            let mut has_kwargs = false;
            if let Some(last) = args.last() {
                if userdict_has_construct_kwargs(last) {
                    has_kwargs = true;
                    positional_end -= 1;
                }
            }
            if positional_end > 2 {
                return Err(PyException::type_error(
                    "UserDict expected at most 1 positional argument",
                ));
            }

            let legacy_dict_kw = if has_kwargs && positional_end == 1 {
                userdict_pop_legacy_dict_kw(&args[args.len() - 1])
            } else {
                None
            };
            if let Some(source) = legacy_dict_kw {
                userdict_update_from_object(&mut data_map, &source)?;
            } else if positional_end > 1 {
                userdict_update_from_object(&mut data_map, &args[1])?;
            }
            if has_kwargs {
                userdict_update_from_kwargs(&mut data_map, &args[args.len() - 1])?;
            }
            let data = PyObject::dict(data_map);
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
        CompactString::from("fromkeys"),
        PyObject::wrap(PyObjectPayload::ClassMethod(PyObject::native_closure(
            "UserDict.fromkeys",
            move |args| {
                if args.len() < 2 {
                    return Err(PyException::type_error(
                        "fromkeys() requires at least 1 argument",
                    ));
                }
                let owner_class = args[0].clone();
                let value = args.get(2).cloned().unwrap_or_else(PyObject::none);
                let result = call_callable(&owner_class, &[])?;
                let setitem = result.get_attr("__setitem__").ok_or_else(|| {
                    PyException::attribute_error(format!(
                        "'{}' object has no attribute '__setitem__'",
                        result.type_name()
                    ))
                })?;
                for key in userdict_iter_objects(&args[1])? {
                    call_callable(&setitem, &[key, value.clone()])?;
                }
                Ok(result)
            },
        ))),
    );
    ns.insert(
        CompactString::from("__getitem__"),
        native_method("UserDict", "__getitem__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected key"));
            }
            let data = get_user_data(&args[0], "data")?;
            match data.get_item(&args[1]) {
                Ok(value) => Ok(value),
                Err(err) if err.kind == ExceptionKind::KeyError => {
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        if let Some(missing) = lookup_in_class_mro(&inst.class, "__missing__") {
                            let bound = match &missing.payload {
                                PyObjectPayload::Function(_)
                                | PyObjectPayload::NativeFunction(_) => {
                                    PyObjectRef::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod {
                                            receiver: args[0].clone(),
                                            method: missing,
                                        },
                                    })
                                }
                                _ => missing,
                            };
                            return call_callable(&bound, &[args[1].clone()]);
                        }
                    }
                    let message = CompactString::from(args[1].py_to_string());
                    let original = PyObject::exception_instance_with_args(
                        ExceptionKind::KeyError,
                        message.clone(),
                        vec![args[1].clone()],
                    );
                    Err(PyException::with_original(
                        ExceptionKind::KeyError,
                        message,
                        original,
                    ))
                }
                Err(err) => Err(err),
            }
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
        native_method("UserDict", "__repr__", |args| userdict_repr(&args[0])),
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
            data.compare(&other_data, CompareOp::Eq)
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
    ns.insert(
        CompactString::from("update"),
        native_method("UserDict", "update", |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "update expected at least 1 argument",
                ));
            }
            let data = get_user_data(&args[0], "data")?;
            if let PyObjectPayload::Dict(map) = &data.payload {
                userdict_update_bound(map, &args[1..])
            } else {
                Err(PyException::type_error(
                    "UserDict.update requires mapping data",
                ))
            }
        }),
    );
    PyObject::class(CompactString::from("UserDict"), vec![], ns)
}

fn userdict_update_bound(
    map: &std::rc::Rc<PyCell<FxHashKeyMap>>,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    if args.len() > 2
        || (args.len() == 2
            && !matches!(
                &args[1].payload,
                PyObjectPayload::Dict(kw_map)
                    if kw_map.read().contains_key(&HashableKey::str_key(
                        CompactString::from(USERDICT_KW_MARKER)
                    ))
            ))
    {
        return Err(PyException::type_error(
            "update expected at most 1 positional argument",
        ));
    }

    let mut w = map.write();
    if let Some(first) = args.first() {
        if args.len() == 1 && userdict_update_from_kwargs(&mut w, first)? {
            return Ok(PyObject::none());
        }
        userdict_update_from_object(&mut w, first)?;
    }
    if let Some(kwargs) = args.get(1) {
        userdict_update_from_kwargs(&mut w, kwargs)?;
    }
    Ok(PyObject::none())
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
        PyObject::native_closure("keys", move |args| {
            if !args.is_empty() {
                return Err(PyException::type_error("keys() takes no arguments"));
            }
            Ok(PyObject::wrap(PyObjectPayload::DictKeys {
                map: m.clone(),
                owner: None,
            }))
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("values"),
        PyObject::native_closure("values", move |args| {
            if !args.is_empty() {
                return Err(PyException::type_error("values() takes no arguments"));
            }
            Ok(PyObject::wrap(PyObjectPayload::DictValues {
                map: m.clone(),
                owner: None,
            }))
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("items"),
        PyObject::native_closure("items", move |args| {
            if !args.is_empty() {
                return Err(PyException::type_error("items() takes no arguments"));
            }
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
        CompactString::from("popitem"),
        PyObject::native_closure("popitem", move |args| {
            if !args.is_empty() {
                return Err(PyException::type_error("popitem() takes no arguments"));
            }
            match m.write().pop() {
                Some((key, value)) => Ok(PyObject::tuple(vec![key.to_object(), value])),
                None => Err(PyException::key_error("popitem(): dictionary is empty")),
            }
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("update"),
        PyObject::native_closure("UserDict.update", move |args| {
            userdict_update_bound(&m, args)
        }),
    );
    attrs.write().insert(CompactString::from("copy"), {
        let attrs = attrs.clone();
        let data = data.clone();
        let owner_class = owner_class.clone();
        PyObject::native_closure("copy", move |args| {
            if !args.is_empty() {
                return Err(PyException::type_error("copy() takes no arguments"));
            }
            build_userdict_copy(&data, owner_class.clone(), &attrs)
        })
    });
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("clear"),
        PyObject::native_closure("clear", move |args| {
            if !args.is_empty() {
                return Err(PyException::type_error("clear() takes no arguments"));
            }
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
