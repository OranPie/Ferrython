use super::*;

mod weak_key_dictionary;
mod weak_set;
mod weak_value_dictionary;

pub(super) use weak_key_dictionary::make_weak_key_dictionary;
pub(super) use weak_set::make_weak_set_class;
pub(super) use weak_value_dictionary::make_weak_value_dictionary;

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
