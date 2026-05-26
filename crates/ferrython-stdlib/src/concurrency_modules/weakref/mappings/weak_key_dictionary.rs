use super::*;

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

pub(crate) fn make_weak_key_dictionary(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let storage: WeakKeyStorage = Rc::new(PyCell::new(IndexMap::new()));
    let inst = build_weak_key_dictionary(storage.clone());
    weak_key_update_args(&storage, args)?;
    Ok(inst)
}
