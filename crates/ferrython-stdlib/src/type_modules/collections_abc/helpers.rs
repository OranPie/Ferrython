use super::*;

pub(super) fn add_method(cls: &PyObjectRef, name: &str, func: PyObjectRef) {
    if let PyObjectPayload::Class(cd) = &cls.payload {
        cd.namespace.write().insert(CompactString::from(name), func);
    }
}

pub(super) fn drop_abstract(cls: &PyObjectRef, names: &[&str]) {
    if let PyObjectPayload::Class(cd) = &cls.payload {
        let mut ns = cd.namespace.write();
        if let Some(abs) = ns.get("__abstractmethods__").cloned() {
            let new_abs = match &abs.payload {
                PyObjectPayload::Set(set) => {
                    let mut w = set.read().clone();
                    for name in names {
                        w.remove(&HashableKey::str_key(CompactString::from(*name)));
                    }
                    PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(w))))
                }
                PyObjectPayload::FrozenSet(set) => {
                    let mut w = set.items.clone();
                    for name in names {
                        w.shift_remove(&HashableKey::str_key(CompactString::from(*name)));
                    }
                    PyObject::frozenset(w)
                }
                PyObjectPayload::Tuple(items) => {
                    let filtered: Vec<_> = items
                        .iter()
                        .filter(|item| !names.iter().any(|name| item.py_to_string() == *name))
                        .cloned()
                        .collect();
                    PyObject::tuple(filtered)
                }
                PyObjectPayload::List(items) => {
                    let filtered: Vec<_> = items
                        .read()
                        .iter()
                        .filter(|item| !names.iter().any(|name| item.py_to_string() == *name))
                        .cloned()
                        .collect();
                    PyObject::list(filtered)
                }
                _ => abs.clone(),
            };
            ns.insert(CompactString::from("__abstractmethods__"), new_abs);
        }
    }
}

pub(super) fn make_index_iterator(obj: &PyObjectRef, reverse: bool) -> PyResult<PyObjectRef> {
    let len = obj.py_len()? as i64;
    let mut items = Vec::new();
    if reverse {
        for i in (0..len).rev() {
            items.push(obj.get_item(&PyObject::int(i))?);
        }
    } else {
        for i in 0..len {
            items.push(obj.get_item(&PyObject::int(i))?);
        }
    }
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
        PyCell::new(ferrython_core::object::IteratorData::List { items, index: 0 }),
    ))))
}
