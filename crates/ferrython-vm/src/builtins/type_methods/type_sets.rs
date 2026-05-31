//! Set and frozenset method dispatch.

use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args_min, new_fx_hashkey_flatmap, new_fx_hashkey_map, FxHashKeyFlatMap, FxHashKeyMap,
    IteratorData, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

use super::{check_key_error, collect_hash_entries};

pub(crate) fn call_set_method(
    m: &Rc<PyCell<FxHashKeyFlatMap>>,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    fn set_lookup_key(obj: &PyObjectRef) -> PyResult<HashableKey> {
        match &obj.payload {
            PyObjectPayload::Set(items) => {
                let read = items.read();
                let mut keys: Vec<HashableKey> = read.keys().cloned().collect();
                keys.sort_by(|a, b| a.hash_key().cmp(&b.hash_key()));
                Ok(HashableKey::FrozenSet(std::rc::Rc::new(
                    ferrython_core::types::FrozenSetKeyData::new(keys),
                )))
            }
            PyObjectPayload::Instance(inst) => {
                let has_user_hash = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    cd.namespace.read().contains_key("__hash__")
                } else {
                    false
                };
                if has_user_hash {
                    if let Ok(key) = obj.to_hashable_key() {
                        return Ok(key);
                    }
                }
                if let Some(value) = inst.attrs.read().get("__builtin_value__").cloned() {
                    if matches!(
                        &value.payload,
                        PyObjectPayload::Set(_) | PyObjectPayload::FrozenSet(_)
                    ) {
                        return set_lookup_key(&value);
                    }
                }
                obj.to_hashable_key()
            }
            _ => obj.to_hashable_key(),
        }
    }

    match method {
        "__init__" => {
            if args.len() > 1 {
                return Err(PyException::type_error(format!(
                    "set expected at most 1 argument, got {}",
                    args.len()
                )));
            }
            let mut guard = m.write();
            guard.clear();
            if let Some(arg) = args.first() {
                for (key, value) in collect_hash_entries(arg)? {
                    guard.entry(key).or_insert(value);
                    check_key_error()?;
                }
            }
            Ok(PyObject::none())
        }
        "copy" => Ok(PyObject::set_from_flatmap(m.read().clone())),
        "union" | "__or__" => {
            check_args_min("union", args, 1)?;
            let mut result = m.read().clone();
            for arg in args {
                for (key, value) in collect_hash_entries(arg)? {
                    result.entry(key).or_insert(value);
                    check_key_error()?;
                }
            }
            Ok(PyObject::set_from_flatmap(result))
        }
        "intersection" | "__and__" => {
            let guard = m.read();
            let mut result = guard.clone();
            drop(guard);
            for arg in args {
                let entries = collect_hash_entries(arg)?;
                result.retain(|key, _| entries.iter().any(|(other_key, _)| other_key == key));
                check_key_error()?;
                if result.is_empty() {
                    break;
                }
            }
            Ok(PyObject::set_from_flatmap(result))
        }
        "difference" | "__sub__" => {
            let mut result = m.read().clone();
            for arg in args {
                for (key, _) in collect_hash_entries(arg)? {
                    result.remove(&key);
                    check_key_error()?;
                }
            }
            Ok(PyObject::set_from_flatmap(result))
        }
        "symmetric_difference" | "__xor__" => {
            check_args_min("symmetric_difference", args, 1)?;
            if args.len() != 1 {
                return Err(PyException::type_error(format!(
                    "symmetric_difference expected 1 argument, got {}",
                    args.len()
                )));
            }
            let guard = m.read();
            let mut result = new_fx_hashkey_flatmap();
            for (key, value) in guard.iter() {
                result.insert(key.clone(), value.clone());
            }
            drop(guard);
            let mut other = new_fx_hashkey_flatmap();
            for (key, value) in collect_hash_entries(&args[0])? {
                other.entry(key).or_insert(value);
                check_key_error()?;
            }
            for (key, value) in other {
                if result.remove(&key).is_none() {
                    result.insert(key, value);
                }
                check_key_error()?;
            }
            Ok(PyObject::set_from_flatmap(result))
        }
        "issubset" => {
            check_args_min("issubset", args, 1)?;
            let other_items = args[0].to_list()?;
            let other_keys: std::collections::HashSet<String> =
                other_items.iter().map(|x| x.py_to_string()).collect();
            let all_in = m
                .read()
                .values()
                .all(|v| other_keys.contains(&v.py_to_string()));
            Ok(PyObject::bool_val(all_in))
        }
        "issuperset" => {
            check_args_min("issuperset", args, 1)?;
            let other_items = args[0].to_list()?;
            let guard = m.read();
            let self_keys: std::collections::HashSet<String> =
                guard.values().map(|x| x.py_to_string()).collect();
            let all_in = other_items
                .iter()
                .all(|v| self_keys.contains(&v.py_to_string()));
            Ok(PyObject::bool_val(all_in))
        }
        "isdisjoint" => {
            check_args_min("isdisjoint", args, 1)?;
            let other_items = args[0].to_list()?;
            let guard = m.read();
            let self_keys: std::collections::HashSet<String> =
                guard.values().map(|x| x.py_to_string()).collect();
            let none_in = other_items
                .iter()
                .all(|v| !self_keys.contains(&v.py_to_string()));
            Ok(PyObject::bool_val(none_in))
        }
        "add" => {
            check_args_min("add", args, 1)?;
            let hk = set_lookup_key(&args[0])?;
            // Use entry API to avoid Rc::clone when key already exists
            m.write().entry(hk).or_insert_with(|| args[0].clone());
            check_key_error()?;
            Ok(PyObject::none())
        }
        "remove" => {
            check_args_min("remove", args, 1)?;
            let hk = set_lookup_key(&args[0])?;
            let removed = m.write().remove(&hk);
            check_key_error()?;
            if removed.is_none() {
                return Err(PyException::key_error_value(args[0].clone()));
            }
            Ok(PyObject::none())
        }
        "discard" => {
            check_args_min("discard", args, 1)?;
            let hk = set_lookup_key(&args[0])?;
            m.write().remove(&hk);
            check_key_error()?;
            Ok(PyObject::none())
        }
        "pop" => {
            let mut guard = m.write();
            if guard.is_empty() {
                return Err(PyException::key_error("pop from an empty set"));
            }
            let key = guard.keys().next().unwrap().clone();
            let val = guard.remove(&key).unwrap();
            Ok(val)
        }
        "clear" => {
            m.write().clear();
            Ok(PyObject::none())
        }
        "update" => {
            check_args_min("update", args, 1)?;
            let mut guard = m.write();
            for arg in args {
                for (key, value) in collect_hash_entries(arg)? {
                    guard.entry(key).or_insert(value);
                    check_key_error()?;
                }
            }
            Ok(PyObject::none())
        }
        "difference_update" => {
            let mut guard = m.write();
            for arg in args {
                for (key, _) in collect_hash_entries(arg)? {
                    guard.remove(&key);
                    check_key_error()?;
                }
            }
            Ok(PyObject::none())
        }
        "intersection_update" => {
            check_args_min("intersection_update", args, 1)?;
            let mut guard = m.write();
            for arg in args {
                let entries = collect_hash_entries(arg)?;
                guard.retain(|key, _| entries.iter().any(|(other_key, _)| other_key == key));
                check_key_error()?;
                if guard.is_empty() {
                    break;
                }
            }
            Ok(PyObject::none())
        }
        "symmetric_difference_update" => {
            check_args_min("symmetric_difference_update", args, 1)?;
            if args.len() != 1 {
                return Err(PyException::type_error(format!(
                    "symmetric_difference_update expected 1 argument, got {}",
                    args.len()
                )));
            }
            let mut guard = m.write();
            let mut other = new_fx_hashkey_flatmap();
            for (key, value) in collect_hash_entries(&args[0])? {
                other.entry(key).or_insert(value);
                check_key_error()?;
            }
            for (key, value) in other {
                if guard.remove(&key).is_none() {
                    guard.insert(key, value);
                }
                check_key_error()?;
            }
            Ok(PyObject::none())
        }
        "__contains__" => {
            check_args_min("set.__contains__", args, 1)?;
            let key = set_lookup_key(&args[0])?;
            let contains = m.read().contains_key(&key);
            check_key_error()?;
            Ok(PyObject::bool_val(contains))
        }
        "__len__" => Ok(PyObject::int(m.read().len() as i64)),
        "__bool__" => Ok(PyObject::bool_val(!m.read().is_empty())),
        "__iter__" => Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::SetRefs {
                source: m.clone(),
                index: 0,
                expected_len: m.read().len(),
            }),
        )))),
        "__hash__" => Err(PyException::type_error("unhashable type: 'set'")),
        _ => Err(PyException::attribute_error(format!(
            "'set' object has no attribute '{}'",
            method
        ))),
    }
}

pub(crate) fn call_frozenset_method(
    receiver: Option<PyObjectRef>,
    m: &FxHashKeyMap,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match method {
        "__init__" => {
            if args.len() > 1 {
                return Err(PyException::type_error(format!(
                    "frozenset expected at most 1 argument, got {}",
                    args.len()
                )));
            }
            Ok(PyObject::none())
        }
        "copy" => Ok(receiver.unwrap_or_else(|| PyObject::frozenset(m.clone()))),
        "union" | "__or__" => {
            check_args_min("union", args, 1)?;
            let mut result = m.clone();
            for arg in args {
                for (key, value) in collect_hash_entries(arg)? {
                    result.entry(key).or_insert(value);
                    check_key_error()?;
                }
            }
            Ok(PyObject::frozenset(result))
        }
        "intersection" | "__and__" => {
            let mut result = m.clone();
            for arg in args {
                let entries = collect_hash_entries(arg)?;
                result.retain(|key, _| entries.iter().any(|(other_key, _)| other_key == key));
                check_key_error()?;
                if result.is_empty() {
                    break;
                }
            }
            Ok(PyObject::frozenset(result))
        }
        "difference" | "__sub__" => {
            let mut result = m.clone();
            for arg in args {
                for (key, _) in collect_hash_entries(arg)? {
                    result.shift_remove(&key);
                    check_key_error()?;
                }
            }
            Ok(PyObject::frozenset(result))
        }
        "symmetric_difference" | "__xor__" => {
            check_args_min("symmetric_difference", args, 1)?;
            if args.len() != 1 {
                return Err(PyException::type_error(format!(
                    "symmetric_difference expected 1 argument, got {}",
                    args.len()
                )));
            }
            let mut result = IndexMap::new();
            for (key, value) in m.iter() {
                result.insert(key.clone(), value.clone());
            }
            let mut other = new_fx_hashkey_map();
            for (key, value) in collect_hash_entries(&args[0])? {
                other.entry(key).or_insert(value);
                check_key_error()?;
            }
            for (key, value) in other {
                if result.shift_remove(&key).is_none() {
                    result.insert(key, value);
                }
                check_key_error()?;
            }
            Ok(PyObject::frozenset(result))
        }
        "issubset" => {
            check_args_min("issubset", args, 1)?;
            let other_items = args[0].to_list()?;
            let other_keys: std::collections::HashSet<String> =
                other_items.iter().map(|x| x.py_to_string()).collect();
            let all_in = m.values().all(|v| other_keys.contains(&v.py_to_string()));
            Ok(PyObject::bool_val(all_in))
        }
        "issuperset" => {
            check_args_min("issuperset", args, 1)?;
            let other_items = args[0].to_list()?;
            let self_keys: std::collections::HashSet<String> =
                m.values().map(|x| x.py_to_string()).collect();
            let all_in = other_items
                .iter()
                .all(|v| self_keys.contains(&v.py_to_string()));
            Ok(PyObject::bool_val(all_in))
        }
        "isdisjoint" => {
            check_args_min("isdisjoint", args, 1)?;
            let other_items = args[0].to_list()?;
            let self_keys: std::collections::HashSet<String> =
                m.values().map(|x| x.py_to_string()).collect();
            let none_in = other_items
                .iter()
                .all(|v| !self_keys.contains(&v.py_to_string()));
            Ok(PyObject::bool_val(none_in))
        }
        "__contains__" => {
            check_args_min("frozenset.__contains__", args, 1)?;
            let key = {
                match &args[0].payload {
                    PyObjectPayload::Set(items) => {
                        let read = items.read();
                        let mut keys: Vec<HashableKey> = read.keys().cloned().collect();
                        keys.sort_by(|a, b| a.hash_key().cmp(&b.hash_key()));
                        HashableKey::FrozenSet(std::rc::Rc::new(
                            ferrython_core::types::FrozenSetKeyData::new(keys),
                        ))
                    }
                    PyObjectPayload::Instance(inst) => {
                        if let Some(value) = inst.attrs.read().get("__builtin_value__").cloned() {
                            if matches!(
                                &value.payload,
                                PyObjectPayload::Set(_) | PyObjectPayload::FrozenSet(_)
                            ) {
                                let list = value.to_list()?;
                                let mut keys = Vec::with_capacity(list.len());
                                for item in list {
                                    keys.push(item.to_hashable_key()?);
                                }
                                keys.sort_by(|a, b| a.hash_key().cmp(&b.hash_key()));
                                HashableKey::FrozenSet(std::rc::Rc::new(
                                    ferrython_core::types::FrozenSetKeyData::new(keys),
                                ))
                            } else {
                                args[0].to_hashable_key()?
                            }
                        } else {
                            args[0].to_hashable_key()?
                        }
                    }
                    _ => args[0].to_hashable_key()?,
                }
            };
            let contains = m.contains_key(&key);
            check_key_error()?;
            Ok(PyObject::bool_val(contains))
        }
        "__len__" => Ok(PyObject::int(m.len() as i64)),
        "__iter__" => {
            let items: Vec<PyObjectRef> = m.keys().map(|k| k.to_object()).collect();
            Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(IteratorData::FrozenSetItems { items, index: 0 }),
            ))))
        }
        "__lt__" => {
            check_args_min("__lt__", args, 1)?;
            let other_items = args[0].to_list()?;
            let other_keys: std::collections::HashSet<String> =
                other_items.iter().map(|x| x.py_to_string()).collect();
            let is_subset = m.values().all(|v| other_keys.contains(&v.py_to_string()));
            Ok(PyObject::bool_val(is_subset && m.len() < other_keys.len()))
        }
        "__le__" => {
            check_args_min("__le__", args, 1)?;
            let other_items = args[0].to_list()?;
            let other_keys: std::collections::HashSet<String> =
                other_items.iter().map(|x| x.py_to_string()).collect();
            Ok(PyObject::bool_val(
                m.values().all(|v| other_keys.contains(&v.py_to_string())),
            ))
        }
        "__gt__" => {
            check_args_min("__gt__", args, 1)?;
            let other_items = args[0].to_list()?;
            let other_keys: std::collections::HashSet<String> =
                other_items.iter().map(|x| x.py_to_string()).collect();
            let self_keys: std::collections::HashSet<String> =
                m.values().map(|x| x.py_to_string()).collect();
            let is_superset = other_keys.iter().all(|v| self_keys.contains(v));
            Ok(PyObject::bool_val(
                is_superset && m.len() > other_keys.len(),
            ))
        }
        "__ge__" => {
            check_args_min("__ge__", args, 1)?;
            let other_items = args[0].to_list()?;
            let self_keys: std::collections::HashSet<String> =
                m.values().map(|x| x.py_to_string()).collect();
            Ok(PyObject::bool_val(
                other_items
                    .iter()
                    .all(|v| self_keys.contains(&v.py_to_string())),
            ))
        }
        _ => Err(PyException::attribute_error(format!(
            "'frozenset' object has no attribute '{}'",
            method
        ))),
    }
}
