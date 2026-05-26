use super::*;

fn weak_key_items(storage: &WeakKeyStorage) -> Vec<(PyObjectRef, PyObjectRef)> {
    let mut store = storage.write();
    store.retain(|_, (r, _)| weak_ref_target(r).is_some());
    store
        .iter()
        .filter_map(|(_, (r, v))| weak_ref_target(r).map(|k| (k, v.clone())))
        .collect()
}

fn weak_value_items(storage: &WeakValueStorage) -> Vec<(PyObjectRef, PyObjectRef)> {
    let mut store = storage.write();
    store.retain(|_, (_, r)| weak_ref_target(r).is_some());
    store
        .iter()
        .filter_map(|(_, (k, r))| weak_ref_target(r).map(|v| (k.clone(), v)))
        .collect()
}

fn weak_iter(items: Vec<PyObjectRef>) -> PyObjectRef {
    PyObject::wrap(PyObjectPayload::VecIter(Box::new(VecIterData {
        items,
        index: SyncUsize::new(0),
    })))
}

fn weak_value_iter(storage: &WeakValueStorage, kind: WeakValueIterKind) -> PyObjectRef {
    let mut store = storage.write();
    store.retain(|_, (_, r)| weak_ref_target(r).is_some());
    let entries = store
        .values()
        .map(|(key, ref_obj)| (key.clone(), ref_obj.clone()))
        .collect();
    PyObject::wrap(PyObjectPayload::WeakValueIter(Box::new(
        WeakValueIterData {
            entries,
            index: SyncUsize::new(0),
            kind,
        },
    )))
}

fn weak_key_iter(storage: &WeakKeyStorage, kind: WeakKeyIterKind) -> PyObjectRef {
    let mut store = storage.write();
    store.retain(|_, (r, _)| weak_ref_target(r).is_some());
    let entries = store
        .values()
        .map(|(ref_obj, value)| (ref_obj.clone(), value.clone()))
        .collect();
    PyObject::wrap(PyObjectPayload::WeakKeyIter(Box::new(WeakKeyIterData {
        entries,
        index: SyncUsize::new(0),
        kind,
    })))
}

fn pair_from_internal_item(item: PyObjectRef) -> PyResult<(PyObjectRef, PyObjectRef)> {
    match &item.payload {
        PyObjectPayload::Tuple(items) if items.len() == 2 => {
            Ok((items[0].clone(), items[1].clone()))
        }
        _ => Err(PyException::type_error("invalid weakdict item")),
    }
}

fn internal_mapping_items(
    obj: &PyObjectRef,
    name: &str,
) -> Option<PyResult<Vec<(PyObjectRef, PyObjectRef)>>> {
    let PyObjectPayload::Instance(inst) = &obj.payload else {
        return None;
    };
    let items_fn = inst.attrs.read().get(name).cloned()?;
    Some(call_callable(&items_fn, &[]).and_then(|items| {
        items
            .to_list()?
            .into_iter()
            .map(pair_from_internal_item)
            .collect()
    }))
}

fn weak_mapping_items(obj: &PyObjectRef) -> PyResult<Vec<(PyObjectRef, PyObjectRef)>> {
    fn pair_from_object(item: PyObjectRef) -> PyResult<(PyObjectRef, PyObjectRef)> {
        match &item.payload {
            PyObjectPayload::Tuple(items) if items.len() == 2 => {
                return Ok((items[0].clone(), items[1].clone()));
            }
            PyObjectPayload::List(items) if items.read().len() == 2 => {
                let items = items.read();
                return Ok((items[0].clone(), items[1].clone()));
            }
            _ => {}
        }
        let pair = item.to_list()?;
        if pair.len() != 2 {
            return Err(PyException::value_error(
                "dictionary update sequence element has length other than 2",
            ));
        }
        Ok((pair[0].clone(), pair[1].clone()))
    }

    if let Some(items) = internal_mapping_items(obj, "__weakvalue_items__") {
        return items;
    }
    if let Some(items) = internal_mapping_items(obj, "__weakkey_items__") {
        return items;
    }

    match &obj.payload {
        PyObjectPayload::Dict(map) => {
            return Ok(map
                .read()
                .iter()
                .map(|(k, v)| {
                    (
                        k.original_object().unwrap_or_else(|| k.to_object()),
                        v.clone(),
                    )
                })
                .collect())
        }
        PyObjectPayload::Instance(inst) => {
            if let Some(storage) = inst.dict_storage.as_ref() {
                return Ok(storage
                    .read()
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.original_object().unwrap_or_else(|| k.to_object()),
                            v.clone(),
                        )
                    })
                    .collect());
            }
            if let Some(items_fn) = obj.get_attr("items") {
                let items = call_callable(&items_fn, &[])?;
                return items.to_list()?.into_iter().map(pair_from_object).collect();
            }
            if let Some(keys_fn) = obj.get_attr("keys") {
                let keys = call_callable(&keys_fn, &[])?;
                let mut items = Vec::new();
                for key in keys.to_list()? {
                    let value = obj.get_item(&key)?;
                    items.push((key, value));
                }
                return Ok(items);
            }
        }
        _ => {}
    }
    obj.to_list()?.into_iter().map(pair_from_object).collect()
}

fn weak_key_update_from_dict_storage(
    storage: &WeakKeyStorage,
    source: &Rc<PyCell<FxHashKeyMap>>,
) -> PyResult<()> {
    for (key, value) in source.read().iter() {
        weak_key_set(
            storage,
            key.original_object().unwrap_or_else(|| key.to_object()),
            value.clone(),
        )?;
    }
    Ok(())
}

fn weak_ref_object(target: &PyObjectRef) -> PyObjectRef {
    let weak = PyObjectRef::downgrade(target);
    let cls = PyObject::class(CompactString::from("weakref"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(
            CompactString::from("__weakref_ref__"),
            PyObject::bool_val(true),
        );
        let w_call = weak.clone();
        attrs.insert(
            CompactString::from("__call__"),
            PyObject::native_closure("weakref.__call__", move |_| Ok(upgrade_or_none(&w_call))),
        );
        let w_target = weak.clone();
        attrs.insert(
            CompactString::from("__weakref_target__"),
            PyObject::native_closure("weakref.__target__", move |_| {
                Ok(upgrade_or_none(&w_target))
            }),
        );
        let w_repr = weak.clone();
        attrs.insert(
            CompactString::from("__repr__"),
            PyObject::native_closure("weakref.__repr__", move |_| {
                if w_repr.upgrade().is_some() {
                    Ok(PyObject::str_val(CompactString::from("<weakref (alive)>")))
                } else {
                    Ok(PyObject::str_val(CompactString::from("<weakref (dead)>")))
                }
            }),
        );
    }
    PyObjectRef::register_weak_object(target, &inst, None, WeakObjectKind::Ref);
    inst
}

fn weak_value_set(
    storage: &WeakValueStorage,
    key_obj: PyObjectRef,
    value: PyObjectRef,
) -> PyResult<()> {
    let key = key_obj.to_hashable_key()?;
    let ref_obj = weak_ref_object(&value);
    storage.write().insert(key, (key_obj, ref_obj));
    Ok(())
}

fn weak_value_get_alive(
    storage: &WeakValueStorage,
    key_obj: &PyObjectRef,
) -> PyResult<Option<PyObjectRef>> {
    let key = key_obj.to_hashable_key()?;
    let mut store = storage.write();
    match store
        .get(&key)
        .and_then(|(_, ref_obj)| weak_ref_target(ref_obj))
    {
        Some(obj) => Ok(Some(obj)),
        None if store.contains_key(&key) => {
            store.shift_remove(&key);
            Ok(None)
        }
        None => Ok(None),
    }
}

fn weak_key_require_weakable(key: &PyObjectRef) -> PyResult<()> {
    match &key.payload {
        PyObjectPayload::Int(_)
        | PyObjectPayload::Bool(_)
        | PyObjectPayload::Float(_)
        | PyObjectPayload::Complex { .. }
        | PyObjectPayload::Str(_)
        | PyObjectPayload::Bytes(_)
        | PyObjectPayload::ByteArray(_)
        | PyObjectPayload::Tuple(_)
        | PyObjectPayload::List(_)
        | PyObjectPayload::Dict(_)
        | PyObjectPayload::Set(_)
        | PyObjectPayload::FrozenSet(_) => Err(PyException::type_error(format!(
            "cannot create weak reference to '{}' object",
            key.type_name()
        ))),
        _ => Ok(()),
    }
}

fn weak_key_set(storage: &WeakKeyStorage, key: PyObjectRef, value: PyObjectRef) -> PyResult<()> {
    weak_key_require_weakable(&key)?;
    let ptr = PyObjectRef::as_ptr(&key) as usize;
    let ref_obj = weak_ref_object(&key);
    storage.write().insert(ptr, (ref_obj, value));
    Ok(())
}

fn weak_key_lookup_ptr(
    store: &IndexMap<usize, (PyObjectRef, PyObjectRef)>,
    key: &PyObjectRef,
) -> PyResult<Option<usize>> {
    let key_hash = key.to_hashable_key()?;
    for (ptr, (ref_obj, _)) in store.iter() {
        let Some(live_key) = weak_ref_target(ref_obj) else {
            continue;
        };
        if live_key.to_hashable_key()?.hash_key() != key_hash.hash_key() {
            continue;
        }
        let eq_result = if let Some(eq_method) = live_key.get_attr("__eq__") {
            call_callable(&eq_method, &[key.clone()])?
        } else {
            live_key.compare(key, CompareOp::Eq)?
        };
        if !matches!(&eq_result.payload, PyObjectPayload::NotImplemented) && eq_result.is_truthy() {
            return Ok(Some(*ptr));
        }
    }
    Ok(None)
}

fn weak_key_get_alive(
    storage: &WeakKeyStorage,
    key: &PyObjectRef,
    strict: bool,
) -> PyResult<Option<PyObjectRef>> {
    if strict {
        weak_key_require_weakable(key)?;
    }
    let mut store = storage.write();
    store.retain(|_, (ref_obj, _)| weak_ref_target(ref_obj).is_some());
    let Some(ptr) = weak_key_lookup_ptr(&store, key)? else {
        return Ok(None);
    };
    if let Some((_, val)) = store.get(&ptr) {
        Ok(Some(val.clone()))
    } else {
        Ok(None)
    }
}

fn py_default_key_error(key: &PyObjectRef) -> PyException {
    PyException::new(ExceptionKind::KeyError, key.repr())
}

fn weak_kwargs_marker_key() -> HashableKey {
    HashableKey::str_key(CompactString::from("__weakdict_kwargs__"))
}

fn weak_kwargs_items(obj: &PyObjectRef) -> Option<Vec<(PyObjectRef, PyObjectRef)>> {
    let PyObjectPayload::Dict(map) = &obj.payload else {
        return None;
    };
    let marker = weak_kwargs_marker_key();
    let map = map.read();
    if !map.contains_key(&marker) {
        return None;
    }
    Some(
        map.iter()
            .filter(|(k, _)| *k != &marker)
            .map(|(k, v)| {
                (
                    k.original_object().unwrap_or_else(|| k.to_object()),
                    v.clone(),
                )
            })
            .collect(),
    )
}

fn weak_mapping_eq(left: &[(PyObjectRef, PyObjectRef)], other: &PyObjectRef) -> PyResult<bool> {
    let right = if let Some(items) = internal_mapping_items(other, "__weakvalue_items__") {
        items?
    } else if let Some(items) = internal_mapping_items(other, "__weakkey_items__") {
        items?
    } else if let Some(items_fn) = other.get_attr("items") {
        let other_items = ferrython_core::object::call_callable(&items_fn, &[])?;
        other_items
            .to_list()?
            .into_iter()
            .map(pair_from_internal_item)
            .collect::<PyResult<Vec<_>>>()?
    } else {
        return Ok(false);
    };

    if left.len() != right.len() {
        return Ok(false);
    }
    for (lk, lv) in left {
        let mut found = false;
        for (rk, rv) in &right {
            let key_eq = lk.compare(rk, CompareOp::Eq)?.is_truthy();
            if key_eq {
                let value_eq = lv.compare(rv, CompareOp::Eq)?.is_truthy();
                if !value_eq {
                    return Ok(false);
                }
                found = true;
                break;
            }
        }
        if !found {
            return Ok(false);
        }
    }
    Ok(true)
}

fn weak_value_update_args(storage: &WeakValueStorage, args: &[PyObjectRef]) -> PyResult<()> {
    let (source, kwargs) = match args {
        [] => (None, None),
        [only] => {
            if let Some(items) = weak_kwargs_items(only) {
                (None, Some(items))
            } else {
                (Some(only), None)
            }
        }
        [source, kwargs] => match weak_kwargs_items(kwargs) {
            Some(items) => (Some(source), Some(items)),
            None => {
                return Err(PyException::type_error(
                    "WeakValueDictionary expected at most 1 argument",
                ))
            }
        },
        _ => {
            return Err(PyException::type_error(
                "WeakValueDictionary expected at most 1 argument",
            ))
        }
    };
    if let Some(source) = source {
        for (key, value) in weak_mapping_items(source)? {
            weak_value_set(storage, key, value)?;
        }
    }
    if let Some(items) = kwargs {
        for (key, value) in items {
            weak_value_set(storage, key, value)?;
        }
    }
    Ok(())
}

fn weak_key_update_args(storage: &WeakKeyStorage, args: &[PyObjectRef]) -> PyResult<()> {
    let (source, kwargs) = match args {
        [] => (None, None),
        [only] => {
            if let Some(items) = weak_kwargs_items(only) {
                (None, Some(items))
            } else {
                (Some(only), None)
            }
        }
        [source, kwargs] => match weak_kwargs_items(kwargs) {
            Some(items) => (Some(source), Some(items)),
            None => {
                return Err(PyException::type_error(
                    "WeakKeyDictionary expected at most 1 argument",
                ))
            }
        },
        _ => {
            return Err(PyException::type_error(
                "WeakKeyDictionary expected at most 1 argument",
            ))
        }
    };
    if let Some(source) = source {
        match &source.payload {
            PyObjectPayload::Dict(map) => weak_key_update_from_dict_storage(storage, map)?,
            PyObjectPayload::Instance(inst) => {
                if let Some(map) = inst.dict_storage.as_ref() {
                    weak_key_update_from_dict_storage(storage, map)?;
                } else {
                    for (key, value) in weak_mapping_items(source)? {
                        weak_key_set(storage, key, value)?;
                    }
                }
            }
            _ => {
                for (key, value) in weak_mapping_items(source)? {
                    weak_key_set(storage, key, value)?;
                }
            }
        }
    }
    if let Some(items) = kwargs {
        for (key, value) in items {
            weak_key_set(storage, key, value)?;
        }
    }
    Ok(())
}

fn build_weak_value_dictionary(storage: WeakValueStorage) -> PyObjectRef {
    let mut class_ns = IndexMap::new();
    let eq_storage = storage.clone();
    class_ns.insert(
        CompactString::from("__eq__"),
        PyObject::native_closure("WeakValueDictionary.__eq__", move |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("__eq__ requires an argument"));
            }
            let items = weak_value_items(&eq_storage);
            Ok(PyObject::bool_val(weak_mapping_eq(&items, &args[1])?))
        }),
    );
    let ne_storage = storage.clone();
    class_ns.insert(
        CompactString::from("__ne__"),
        PyObject::native_closure("WeakValueDictionary.__ne__", move |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("__ne__ requires an argument"));
            }
            let items = weak_value_items(&ne_storage);
            Ok(PyObject::bool_val(!weak_mapping_eq(&items, &args[1])?))
        }),
    );
    class_ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("WeakValueDictionary.__repr__", move |args| {
            let ptr = args
                .first()
                .map(|obj| PyObjectRef::as_ptr(obj) as usize)
                .unwrap_or(0);
            Ok(PyObject::str_val(CompactString::from(format!(
                "<WeakValueDictionary at 0x{:x}>",
                ptr
            ))))
        }),
    );
    let cls = PyObject::class(CompactString::from("WeakValueDictionary"), vec![], class_ns);
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(
            CompactString::from("__weakvaluedict__"),
            PyObject::bool_val(true),
        );
        let internal_items_storage = storage.clone();
        attrs.insert(
            CompactString::from("__weakvalue_items__"),
            PyObject::native_closure("WeakValueDictionary.__weakvalue_items__", move |_| {
                let items = weak_value_items(&internal_items_storage)
                    .into_iter()
                    .map(|(key, value)| PyObject::tuple(vec![key, value]))
                    .collect();
                Ok(PyObject::list(items))
            }),
        );

        let set_storage = storage.clone();
        attrs.insert(
            CompactString::from("__setitem__"),
            PyObject::native_closure("WeakValueDictionary.__setitem__", move |args| {
                if args.len() < 2 {
                    return Err(PyException::type_error(
                        "__setitem__ requires key and value",
                    ));
                }
                weak_value_set(&set_storage, args[0].clone(), args[1].clone())?;
                Ok(PyObject::none())
            }),
        );

        let get_storage = storage.clone();
        attrs.insert(
            CompactString::from("__getitem__"),
            PyObject::native_closure("WeakValueDictionary.__getitem__", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("__getitem__ requires a key"));
                }
                match weak_value_get_alive(&get_storage, &args[0])? {
                    Some(obj) => Ok(obj),
                    None => Err(py_default_key_error(&args[0])),
                }
            }),
        );

        let del_storage = storage.clone();
        attrs.insert(
            CompactString::from("__delitem__"),
            PyObject::native_closure("WeakValueDictionary.__delitem__", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("__delitem__ requires a key"));
                }
                let key = args[0].to_hashable_key()?;
                let mut store = del_storage.write();
                match store.get(&key).and_then(|(_, r)| weak_ref_target(r)) {
                    Some(_) => {
                        store.shift_remove(&key);
                        Ok(PyObject::none())
                    }
                    None if store.contains_key(&key) => {
                        store.shift_remove(&key);
                        Err(py_default_key_error(&args[0]))
                    }
                    None => Err(py_default_key_error(&args[0])),
                }
            }),
        );

        let contains_storage = storage.clone();
        attrs.insert(
            CompactString::from("__contains__"),
            PyObject::native_closure("WeakValueDictionary.__contains__", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("__contains__ requires a key"));
                }
                Ok(PyObject::bool_val(
                    weak_value_get_alive(&contains_storage, &args[0])?.is_some(),
                ))
            }),
        );

        let len_storage = storage.clone();
        attrs.insert(
            CompactString::from("__len__"),
            PyObject::native_closure("WeakValueDictionary.__len__", move |_| {
                let mut store = len_storage.write();
                store.retain(|_, (_, r)| weak_ref_target(r).is_some());
                Ok(PyObject::int(store.len() as i64))
            }),
        );

        let bool_storage = storage.clone();
        attrs.insert(
            CompactString::from("__bool__"),
            PyObject::native_closure("WeakValueDictionary.__bool__", move |_| {
                let mut store = bool_storage.write();
                store.retain(|_, (_, r)| weak_ref_target(r).is_some());
                Ok(PyObject::bool_val(!store.is_empty()))
            }),
        );

        let get_method_storage = storage.clone();
        attrs.insert(
            CompactString::from("get"),
            PyObject::native_closure("WeakValueDictionary.get", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("get() requires a key"));
                }
                if args.len() > 2 {
                    return Err(PyException::type_error("get expected at most 2 arguments"));
                }
                let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
                Ok(weak_value_get_alive(&get_method_storage, &args[0])?.unwrap_or(default))
            }),
        );

        let keys_storage = storage.clone();
        attrs.insert(
            CompactString::from("keys"),
            PyObject::native_closure("WeakValueDictionary.keys", move |args| {
                if !args.is_empty() {
                    return Err(PyException::type_error("keys() takes no arguments"));
                }
                Ok(weak_value_iter(&keys_storage, WeakValueIterKind::Keys))
            }),
        );

        let iter_storage = storage.clone();
        attrs.insert(
            CompactString::from("__iter__"),
            PyObject::native_closure("WeakValueDictionary.__iter__", move |_| {
                Ok(weak_value_iter(&iter_storage, WeakValueIterKind::Keys))
            }),
        );

        let values_storage = storage.clone();
        attrs.insert(
            CompactString::from("values"),
            PyObject::native_closure("WeakValueDictionary.values", move |args| {
                if !args.is_empty() {
                    return Err(PyException::type_error("values() takes no arguments"));
                }
                Ok(weak_value_iter(&values_storage, WeakValueIterKind::Values))
            }),
        );

        let items_storage = storage.clone();
        attrs.insert(
            CompactString::from("items"),
            PyObject::native_closure("WeakValueDictionary.items", move |args| {
                if !args.is_empty() {
                    return Err(PyException::type_error("items() takes no arguments"));
                }
                Ok(weak_value_iter(&items_storage, WeakValueIterKind::Items))
            }),
        );

        let update_storage = storage.clone();
        attrs.insert(
            CompactString::from("update"),
            PyObject::native_closure("WeakValueDictionary.update", move |args| {
                weak_value_update_args(&update_storage, args)?;
                Ok(PyObject::none())
            }),
        );

        let setdefault_storage = storage.clone();
        attrs.insert(
            CompactString::from("setdefault"),
            PyObject::native_closure("WeakValueDictionary.setdefault", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("setdefault() requires a key"));
                }
                if let Some(existing) = weak_value_get_alive(&setdefault_storage, &args[0])? {
                    return Ok(existing);
                }
                let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
                weak_value_set(&setdefault_storage, args[0].clone(), default.clone())?;
                Ok(default)
            }),
        );

        let pop_storage = storage.clone();
        attrs.insert(
            CompactString::from("pop"),
            PyObject::native_closure("WeakValueDictionary.pop", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("pop() requires a key"));
                }
                if args.len() > 2 {
                    return Err(PyException::type_error("pop expected at most 2 arguments"));
                }
                let key = args[0].to_hashable_key()?;
                let mut store = pop_storage.write();
                let state = store.get(&key).and_then(|(_, r)| weak_ref_target(r));
                match state {
                    Some(value) => {
                        store.shift_remove(&key);
                        Ok(value)
                    }
                    None if store.contains_key(&key) => {
                        store.shift_remove(&key);
                        args.get(1)
                            .cloned()
                            .ok_or_else(|| py_default_key_error(&args[0]))
                    }
                    None => args
                        .get(1)
                        .cloned()
                        .ok_or_else(|| py_default_key_error(&args[0])),
                }
            }),
        );

        let popitem_storage = storage.clone();
        attrs.insert(
            CompactString::from("popitem"),
            PyObject::native_closure("WeakValueDictionary.popitem", move |args| {
                if !args.is_empty() {
                    return Err(PyException::type_error("popitem() takes no arguments"));
                }
                let mut store = popitem_storage.write();
                store.retain(|_, (_, r)| weak_ref_target(r).is_some());
                let item = store.iter().next().and_then(|(key, (orig, ref_obj))| {
                    weak_ref_target(ref_obj).map(|v| (key.clone(), orig.clone(), v))
                });
                if let Some((key, orig, value)) = item {
                    store.shift_remove(&key);
                    Ok(PyObject::tuple(vec![orig, value]))
                } else {
                    Err(PyException::key_error("dictionary is empty"))
                }
            }),
        );

        let clear_storage = storage.clone();
        attrs.insert(
            CompactString::from("clear"),
            PyObject::native_closure("WeakValueDictionary.clear", move |_| {
                clear_storage.write().clear();
                Ok(PyObject::none())
            }),
        );

        let copy_storage = storage.clone();
        attrs.insert(
            CompactString::from("copy"),
            PyObject::native_closure("WeakValueDictionary.copy", move |_| {
                let new_storage: WeakValueStorage = Rc::new(PyCell::new(IndexMap::new()));
                for (key, value) in weak_value_items(&copy_storage) {
                    weak_value_set(&new_storage, key, value)?;
                }
                Ok(build_weak_value_dictionary(new_storage))
            }),
        );

        let refs_storage = storage.clone();
        attrs.insert(
            CompactString::from("valuerefs"),
            PyObject::native_closure("WeakValueDictionary.valuerefs", move |_| {
                let refs = {
                    let mut store = refs_storage.write();
                    store.retain(|_, (_, r)| weak_ref_target(r).is_some());
                    store.values().map(|(_, r)| r.clone()).collect()
                };
                Ok(PyObject::list(refs))
            }),
        );

        let iter_refs_storage = storage.clone();
        attrs.insert(
            CompactString::from("itervaluerefs"),
            PyObject::native_closure("WeakValueDictionary.itervaluerefs", move |_| {
                let refs = {
                    let mut store = iter_refs_storage.write();
                    store.retain(|_, (_, r)| weak_ref_target(r).is_some());
                    store.values().map(|(_, r)| r.clone()).collect()
                };
                Ok(weak_iter(refs))
            }),
        );
    }
    inst
}

pub(super) fn make_weak_value_dictionary(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let storage: WeakValueStorage = Rc::new(PyCell::new(IndexMap::new()));
    let inst = build_weak_value_dictionary(storage.clone());
    weak_value_update_args(&storage, args)?;
    Ok(inst)
}

fn build_weak_key_dictionary(storage: WeakKeyStorage) -> PyObjectRef {
    let mut class_ns = IndexMap::new();
    let eq_storage = storage.clone();
    class_ns.insert(
        CompactString::from("__eq__"),
        PyObject::native_closure("WeakKeyDictionary.__eq__", move |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("__eq__ requires an argument"));
            }
            let items = weak_key_items(&eq_storage);
            Ok(PyObject::bool_val(weak_mapping_eq(&items, &args[1])?))
        }),
    );
    let ne_storage = storage.clone();
    class_ns.insert(
        CompactString::from("__ne__"),
        PyObject::native_closure("WeakKeyDictionary.__ne__", move |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("__ne__ requires an argument"));
            }
            let items = weak_key_items(&ne_storage);
            Ok(PyObject::bool_val(!weak_mapping_eq(&items, &args[1])?))
        }),
    );
    class_ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("WeakKeyDictionary.__repr__", move |args| {
            let ptr = args
                .first()
                .map(|obj| PyObjectRef::as_ptr(obj) as usize)
                .unwrap_or(0);
            Ok(PyObject::str_val(CompactString::from(format!(
                "<WeakKeyDictionary at 0x{:x}>",
                ptr
            ))))
        }),
    );
    let cls = PyObject::class(CompactString::from("WeakKeyDictionary"), vec![], class_ns);
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(
            CompactString::from("__weakkeydict__"),
            PyObject::bool_val(true),
        );
        let internal_items_storage = storage.clone();
        attrs.insert(
            CompactString::from("__weakkey_items__"),
            PyObject::native_closure("WeakKeyDictionary.__weakkey_items__", move |_| {
                let items = weak_key_items(&internal_items_storage)
                    .into_iter()
                    .map(|(key, value)| PyObject::tuple(vec![key, value]))
                    .collect();
                Ok(PyObject::list(items))
            }),
        );

        let set_storage = storage.clone();
        attrs.insert(
            CompactString::from("__setitem__"),
            PyObject::native_closure("WeakKeyDictionary.__setitem__", move |args| {
                if args.len() < 2 {
                    return Err(PyException::type_error(
                        "__setitem__ requires key and value",
                    ));
                }
                weak_key_set(&set_storage, args[0].clone(), args[1].clone())?;
                Ok(PyObject::none())
            }),
        );

        let get_storage = storage.clone();
        attrs.insert(
            CompactString::from("__getitem__"),
            PyObject::native_closure("WeakKeyDictionary.__getitem__", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("__getitem__ requires a key"));
                }
                match weak_key_get_alive(&get_storage, &args[0], true)? {
                    Some(obj) => Ok(obj),
                    None => Err(py_default_key_error(&args[0])),
                }
            }),
        );

        let del_storage = storage.clone();
        attrs.insert(
            CompactString::from("__delitem__"),
            PyObject::native_closure("WeakKeyDictionary.__delitem__", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("__delitem__ requires a key"));
                }
                weak_key_require_weakable(&args[0])?;
                let mut store = del_storage.write();
                store.retain(|_, (ref_obj, _)| weak_ref_target(ref_obj).is_some());
                let Some(ptr) = weak_key_lookup_ptr(&store, &args[0])? else {
                    return Err(py_default_key_error(&args[0]));
                };
                store.shift_remove(&ptr);
                Ok(PyObject::none())
            }),
        );

        let contains_storage = storage.clone();
        attrs.insert(
            CompactString::from("__contains__"),
            PyObject::native_closure("WeakKeyDictionary.__contains__", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("__contains__ requires a key"));
                }
                Ok(PyObject::bool_val(
                    weak_key_get_alive(&contains_storage, &args[0], false)?.is_some(),
                ))
            }),
        );

        let len_storage = storage.clone();
        attrs.insert(
            CompactString::from("__len__"),
            PyObject::native_closure("WeakKeyDictionary.__len__", move |_| {
                let mut store = len_storage.write();
                store.retain(|_, (r, _)| weak_ref_target(r).is_some());
                Ok(PyObject::int(store.len() as i64))
            }),
        );

        let bool_storage = storage.clone();
        attrs.insert(
            CompactString::from("__bool__"),
            PyObject::native_closure("WeakKeyDictionary.__bool__", move |_| {
                let mut store = bool_storage.write();
                store.retain(|_, (r, _)| weak_ref_target(r).is_some());
                Ok(PyObject::bool_val(!store.is_empty()))
            }),
        );

        let get_method_storage = storage.clone();
        attrs.insert(
            CompactString::from("get"),
            PyObject::native_closure("WeakKeyDictionary.get", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("get() requires a key"));
                }
                if args.len() > 2 {
                    return Err(PyException::type_error("get expected at most 2 arguments"));
                }
                let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
                Ok(weak_key_get_alive(&get_method_storage, &args[0], false)?.unwrap_or(default))
            }),
        );

        let keys_storage = storage.clone();
        attrs.insert(
            CompactString::from("keys"),
            PyObject::native_closure("WeakKeyDictionary.keys", move |args| {
                if !args.is_empty() {
                    return Err(PyException::type_error("keys() takes no arguments"));
                }
                Ok(weak_key_iter(&keys_storage, WeakKeyIterKind::Keys))
            }),
        );

        let iter_storage = storage.clone();
        attrs.insert(
            CompactString::from("__iter__"),
            PyObject::native_closure("WeakKeyDictionary.__iter__", move |_| {
                Ok(weak_key_iter(&iter_storage, WeakKeyIterKind::Keys))
            }),
        );

        let values_storage = storage.clone();
        attrs.insert(
            CompactString::from("values"),
            PyObject::native_closure("WeakKeyDictionary.values", move |args| {
                if !args.is_empty() {
                    return Err(PyException::type_error("values() takes no arguments"));
                }
                let vals = weak_key_items(&values_storage)
                    .into_iter()
                    .map(|(_, v)| v)
                    .collect();
                Ok(weak_iter(vals))
            }),
        );

        let items_storage = storage.clone();
        attrs.insert(
            CompactString::from("items"),
            PyObject::native_closure("WeakKeyDictionary.items", move |args| {
                if !args.is_empty() {
                    return Err(PyException::type_error("items() takes no arguments"));
                }
                Ok(weak_key_iter(&items_storage, WeakKeyIterKind::Items))
            }),
        );

        let update_storage = storage.clone();
        attrs.insert(
            CompactString::from("update"),
            PyObject::native_closure("WeakKeyDictionary.update", move |args| {
                weak_key_update_args(&update_storage, args)?;
                Ok(PyObject::none())
            }),
        );

        let setdefault_storage = storage.clone();
        attrs.insert(
            CompactString::from("setdefault"),
            PyObject::native_closure("WeakKeyDictionary.setdefault", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("setdefault() requires a key"));
                }
                if let Some(existing) = weak_key_get_alive(&setdefault_storage, &args[0], true)? {
                    return Ok(existing);
                }
                let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
                weak_key_set(&setdefault_storage, args[0].clone(), default.clone())?;
                Ok(default)
            }),
        );

        let pop_storage = storage.clone();
        attrs.insert(
            CompactString::from("pop"),
            PyObject::native_closure("WeakKeyDictionary.pop", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("pop() requires a key"));
                }
                if args.len() > 2 {
                    return Err(PyException::type_error("pop expected at most 2 arguments"));
                }
                weak_key_require_weakable(&args[0])?;
                let mut store = pop_storage.write();
                store.retain(|_, (ref_obj, _)| weak_ref_target(ref_obj).is_some());
                let Some(ptr) = weak_key_lookup_ptr(&store, &args[0])? else {
                    return args
                        .get(1)
                        .cloned()
                        .ok_or_else(|| py_default_key_error(&args[0]));
                };
                match store.get(&ptr) {
                    Some((ref_obj, val)) if weak_ref_target(ref_obj).is_some() => {
                        let value = val.clone();
                        store.shift_remove(&ptr);
                        Ok(value)
                    }
                    Some(_) => {
                        store.shift_remove(&ptr);
                        args.get(1)
                            .cloned()
                            .ok_or_else(|| py_default_key_error(&args[0]))
                    }
                    None => args
                        .get(1)
                        .cloned()
                        .ok_or_else(|| py_default_key_error(&args[0])),
                }
            }),
        );

        let popitem_storage = storage.clone();
        attrs.insert(
            CompactString::from("popitem"),
            PyObject::native_closure("WeakKeyDictionary.popitem", move |args| {
                if !args.is_empty() {
                    return Err(PyException::type_error("popitem() takes no arguments"));
                }
                let mut store = popitem_storage.write();
                store.retain(|_, (r, _)| weak_ref_target(r).is_some());
                let item = store.iter().next().and_then(|(ptr, (ref_obj, val))| {
                    weak_ref_target(ref_obj).map(|k| (*ptr, k, val.clone()))
                });
                if let Some((ptr, key, value)) = item {
                    store.shift_remove(&ptr);
                    Ok(PyObject::tuple(vec![key, value]))
                } else {
                    Err(PyException::key_error("dictionary is empty"))
                }
            }),
        );

        let clear_storage = storage.clone();
        attrs.insert(
            CompactString::from("clear"),
            PyObject::native_closure("WeakKeyDictionary.clear", move |_| {
                clear_storage.write().clear();
                Ok(PyObject::none())
            }),
        );

        let copy_storage = storage.clone();
        attrs.insert(
            CompactString::from("copy"),
            PyObject::native_closure("WeakKeyDictionary.copy", move |_| {
                let new_storage: WeakKeyStorage = Rc::new(PyCell::new(IndexMap::new()));
                for (key, value) in weak_key_items(&copy_storage) {
                    weak_key_set(&new_storage, key, value)?;
                }
                Ok(build_weak_key_dictionary(new_storage))
            }),
        );

        let refs_storage = storage.clone();
        attrs.insert(
            CompactString::from("keyrefs"),
            PyObject::native_closure("WeakKeyDictionary.keyrefs", move |_| {
                let refs = {
                    let mut store = refs_storage.write();
                    store.retain(|_, (r, _)| weak_ref_target(r).is_some());
                    store.values().map(|(r, _)| r.clone()).collect()
                };
                Ok(PyObject::list(refs))
            }),
        );
    }
    inst
}

pub(super) fn make_weak_key_dictionary(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let storage: WeakKeyStorage = Rc::new(PyCell::new(IndexMap::new()));
    let inst = build_weak_key_dictionary(storage.clone());
    weak_key_update_args(&storage, args)?;
    Ok(inst)
}

pub(super) fn make_weak_set(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let storage: Rc<PyCell<IndexMap<usize, PyWeakRef>>> = Rc::new(PyCell::new(IndexMap::new()));

    let cls = PyObject::class(CompactString::from("WeakSet"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();

        let s1 = storage.clone();
        attrs.insert(
            CompactString::from("add"),
            PyObject::native_closure("WeakSet.add", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("add() requires an argument"));
                }
                let ptr = PyObjectRef::as_ptr(&args[0]) as usize;
                let weak = PyObjectRef::downgrade(&args[0]);
                s1.write().insert(ptr, weak);
                Ok(PyObject::none())
            }),
        );

        let s2 = storage.clone();
        attrs.insert(
            CompactString::from("discard"),
            PyObject::native_closure("WeakSet.discard", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("discard() requires an argument"));
                }
                let ptr = PyObjectRef::as_ptr(&args[0]) as usize;
                s2.write().shift_remove(&ptr);
                Ok(PyObject::none())
            }),
        );

        let s3 = storage.clone();
        attrs.insert(
            CompactString::from("__contains__"),
            PyObject::native_closure("WeakSet.__contains__", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("__contains__ requires an argument"));
                }
                let ptr = PyObjectRef::as_ptr(&args[0]) as usize;
                let mut store = s3.write();
                match store.get(&ptr) {
                    Some(weak) => {
                        if weak.upgrade().is_some() {
                            Ok(PyObject::bool_val(true))
                        } else {
                            store.shift_remove(&ptr);
                            Ok(PyObject::bool_val(false))
                        }
                    }
                    None => Ok(PyObject::bool_val(false)),
                }
            }),
        );

        let s4 = storage.clone();
        attrs.insert(
            CompactString::from("__len__"),
            PyObject::native_closure("WeakSet.__len__", move |_| {
                let mut store = s4.write();
                store.retain(|_, w| w.upgrade().is_some());
                Ok(PyObject::int(store.len() as i64))
            }),
        );

        let s5 = storage.clone();
        attrs.insert(
            CompactString::from("__iter__"),
            PyObject::native_closure("WeakSet.__iter__", move |_| {
                let mut store = s5.write();
                store.retain(|_, w| w.upgrade().is_some());
                let items: Vec<PyObjectRef> = store.values().filter_map(|w| w.upgrade()).collect();
                Ok(PyObject::list(items))
            }),
        );
    }
    Ok(inst)
}
