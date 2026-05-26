use super::*;

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

pub(crate) fn make_weak_value_dictionary(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let storage: WeakValueStorage = Rc::new(PyCell::new(IndexMap::new()));
    let inst = build_weak_value_dictionary(storage.clone());
    weak_value_update_args(&storage, args)?;
    Ok(inst)
}
