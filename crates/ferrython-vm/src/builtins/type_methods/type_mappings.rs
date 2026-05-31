//! Dict-like type method dispatch.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::helpers::{
    dict_storage_version, is_hidden_dict_key, mark_dict_storage_mutated,
};
use ferrython_core::object::{
    check_args, check_args_min, new_fx_hashkey_map, FxHashKeyMap, PyCell, PyObject,
    PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{take_pending_eq_error, HashableKey};
use indexmap::map::Entry;
use std::rc::Rc;

use super::extract_kwarg;

#[inline]
fn clear_key_compare_error() {
    let _ = take_pending_eq_error();
}

#[inline]
fn finish_key_compare() -> PyResult<()> {
    match take_pending_eq_error() {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

#[inline]
fn dict_get(map: &FxHashKeyMap, key: &HashableKey) -> PyResult<Option<PyObjectRef>> {
    clear_key_compare_error();
    let value = map.get(key).cloned();
    finish_key_compare()?;
    Ok(value)
}

#[inline]
fn dict_contains_key(map: &FxHashKeyMap, key: &HashableKey) -> PyResult<bool> {
    clear_key_compare_error();
    let contains = map.contains_key(key);
    finish_key_compare()?;
    Ok(contains)
}

#[inline]
fn dict_insert(
    owner: &Rc<PyCell<FxHashKeyMap>>,
    map: &mut FxHashKeyMap,
    key: HashableKey,
    value: PyObjectRef,
) -> PyResult<Option<PyObjectRef>> {
    clear_key_compare_error();
    let rollback_key = key.clone();
    let old = map.insert(key, value);
    if let Some(err) = take_pending_eq_error() {
        let _ = map.swap_remove(&rollback_key);
        clear_key_compare_error();
        return Err(err);
    }
    if old.is_none() {
        mark_dict_storage_mutated(owner);
    }
    Ok(old)
}

#[inline]
fn dict_shift_remove(
    owner: &Rc<PyCell<FxHashKeyMap>>,
    map: &mut FxHashKeyMap,
    key: &HashableKey,
) -> PyResult<Option<PyObjectRef>> {
    clear_key_compare_error();
    let removed = map.shift_remove(key);
    finish_key_compare()?;
    if removed.is_some() {
        mark_dict_storage_mutated(owner);
    }
    Ok(removed)
}

#[inline]
fn missing_key_error(key: &PyObjectRef) -> PyException {
    PyException::key_error_value(key.clone())
}

fn source_dict_state(obj: &PyObjectRef) -> Option<(Rc<PyCell<FxHashKeyMap>>, usize, u64)> {
    match &obj.payload {
        PyObjectPayload::Dict(map) => {
            Some((map.clone(), map.read().len(), dict_storage_version(map)))
        }
        PyObjectPayload::Instance(inst) => inst
            .dict_storage
            .as_ref()
            .map(|map| (map.clone(), map.read().len(), dict_storage_version(map))),
        _ => None,
    }
}

fn check_source_dict_unchanged(
    state: &Option<(Rc<PyCell<FxHashKeyMap>>, usize, u64)>,
) -> PyResult<()> {
    if let Some((map, expected_len, expected_version)) = state {
        if map.read().len() != *expected_len || dict_storage_version(map) != *expected_version {
            return Err(PyException::runtime_error(
                "dictionary changed size during update",
            ));
        }
    }
    Ok(())
}

fn merge_update_sequence(
    owner: &Rc<PyCell<FxHashKeyMap>>,
    target: &mut FxHashKeyMap,
    items: Vec<PyObjectRef>,
) -> PyResult<()> {
    for (index, item) in items.iter().enumerate() {
        let pair = item.to_list().map_err(|_| {
            PyException::type_error(
                "cannot convert dictionary update sequence element to a sequence",
            )
        })?;
        if pair.len() != 2 {
            return Err(PyException::value_error(format!(
                "dictionary update sequence element #{} has length {}; 2 is required",
                index,
                pair.len()
            )));
        }
        let key = pair[0].to_hashable_key()?;
        dict_insert(owner, target, key, pair[1].clone())?;
    }
    Ok(())
}

pub(crate) fn call_dict_method(
    map: &Rc<PyCell<FxHashKeyMap>>,
    method: &str,
    args: &[PyObjectRef],
    owner: Option<PyObjectRef>,
) -> PyResult<PyObjectRef> {
    match method {
        "keys" => {
            if !args.is_empty() {
                return Err(PyException::type_error("keys() takes no arguments"));
            }
            Ok(PyObject::wrap(PyObjectPayload::DictKeys {
                map: map.clone(),
                owner,
            }))
        }
        "values" => {
            if !args.is_empty() {
                return Err(PyException::type_error("values() takes no arguments"));
            }
            Ok(PyObject::wrap(PyObjectPayload::DictValues {
                map: map.clone(),
                owner,
            }))
        }
        "items" => {
            if !args.is_empty() {
                return Err(PyException::type_error("items() takes no arguments"));
            }
            Ok(PyObject::wrap(PyObjectPayload::DictItems {
                map: map.clone(),
                owner,
            }))
        }
        "fromkeys" => crate::builtins::core_fns::builtin_dict_fromkeys(args),
        "get" => {
            check_args_min("get", args, 1)?;
            if args.len() > 2 {
                return Err(PyException::type_error("get expected at most 2 arguments"));
            }
            let key = args[0].to_hashable_key()?;
            let default = if args.len() >= 2 {
                args[1].clone()
            } else {
                PyObject::none()
            };
            let value = {
                let read = map.read();
                dict_get(&read, &key)?
            };
            Ok(value.unwrap_or(default))
        }
        "copy" => {
            check_args("copy", args, 0)?;
            Ok(PyObject::dict(map.read().clone()))
        }
        "update" => {
            if args.len() > 1 {
                return Err(PyException::type_error(
                    "update expected at most 1 positional argument",
                ));
            }
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            // Check if this is a Counter (has __counter__ key)
            let is_counter = map
                .read()
                .contains_key(&HashableKey::str_key(CompactString::from("__counter__")));
            if is_counter {
                // Counter.update: add counts from iterable or mapping
                match &args[0].payload {
                    PyObjectPayload::Str(s) => {
                        let mut w = map.write();
                        for ch in s.chars() {
                            let key = HashableKey::str_key(CompactString::from(ch.to_string()));
                            let count = w.get(&key).and_then(|v| v.as_int()).unwrap_or(0);
                            dict_insert(map, &mut w, key, PyObject::int(count + 1))?;
                        }
                    }
                    PyObjectPayload::Dict(other) => {
                        let other_items = other.read().clone();
                        let mut w = map.write();
                        for (k, v) in other_items {
                            if matches!(&k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__")
                            {
                                continue;
                            }
                            let existing = dict_get(&w, &k)?.and_then(|v| v.as_int()).unwrap_or(0);
                            let add = v.as_int().unwrap_or(0);
                            dict_insert(map, &mut w, k, PyObject::int(existing + add))?;
                        }
                    }
                    PyObjectPayload::List(items) => {
                        let items = items.read().clone();
                        let mut w = map.write();
                        for item in &items {
                            let key = item.to_hashable_key()?;
                            let count = dict_get(&w, &key)?.and_then(|v| v.as_int()).unwrap_or(0);
                            dict_insert(map, &mut w, key, PyObject::int(count + 1))?;
                        }
                    }
                    _ => {
                        return Err(PyException::type_error(
                            "Counter.update() argument must be a mapping or iterable",
                        ));
                    }
                }
            } else {
                match &args[0].payload {
                    PyObjectPayload::Dict(other) => {
                        let source_state = source_dict_state(&args[0]);
                        let other_items = other.read().clone();
                        let mut w = map.write();
                        for (k, v) in other_items {
                            dict_insert(map, &mut w, k, v)?;
                            check_source_dict_unchanged(&source_state)?;
                        }
                    }
                    PyObjectPayload::Instance(_) => {
                        let source_state = source_dict_state(&args[0]);
                        if let Some(keys_method) = args[0].get_attr("keys") {
                            let keys_obj =
                                ferrython_core::object::call_callable(&keys_method, &[])?;
                            let keys = keys_obj.to_list()?;
                            let source_len = keys.len();
                            for key_obj in keys {
                                if keys_obj.py_len().unwrap_or(source_len) != source_len {
                                    return Err(PyException::runtime_error(
                                        "dictionary changed size during update",
                                    ));
                                }
                                let value = args[0].get_item(&key_obj)?;
                                let key = key_obj.to_hashable_key()?;
                                let mut w = map.write();
                                dict_insert(map, &mut w, key, value)?;
                                check_source_dict_unchanged(&source_state)?;
                            }
                        } else {
                            let items = args[0].to_list()?;
                            let mut w = map.write();
                            merge_update_sequence(map, &mut w, items)?;
                        }
                    }
                    PyObjectPayload::List(items) => {
                        let items = items.read().clone();
                        let mut w = map.write();
                        merge_update_sequence(map, &mut w, items)?;
                    }
                    _ => {
                        let items = args[0].to_list()?;
                        let mut w = map.write();
                        merge_update_sequence(map, &mut w, items)?;
                    }
                }
            }
            Ok(PyObject::none())
        }
        "subtract" => {
            // Counter.subtract — subtract counts from iterable or mapping
            check_args_min("subtract", args, 1)?;
            if args.len() > 1 {
                return Err(PyException::type_error(
                    "subtract expected at most 1 positional argument",
                ));
            }
            match &args[0].payload {
                PyObjectPayload::Dict(other) => {
                    let other_items = other.read().clone();
                    let mut w = map.write();
                    for (k, v) in other_items {
                        if matches!(&k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__")
                        {
                            continue;
                        }
                        let existing = dict_get(&w, &k)?.and_then(|v| v.as_int()).unwrap_or(0);
                        let sub = v.as_int().unwrap_or(0);
                        dict_insert(map, &mut w, k, PyObject::int(existing - sub))?;
                    }
                }
                PyObjectPayload::Str(s) => {
                    let mut w = map.write();
                    for ch in s.chars() {
                        let key = HashableKey::str_key(CompactString::from(ch.to_string()));
                        let count = w.get(&key).and_then(|v| v.as_int()).unwrap_or(0);
                        dict_insert(map, &mut w, key, PyObject::int(count - 1))?;
                    }
                }
                PyObjectPayload::List(items) => {
                    let items = items.read().clone();
                    let mut w = map.write();
                    for item in &items {
                        let key = item.to_hashable_key()?;
                        let count = dict_get(&w, &key)?.and_then(|v| v.as_int()).unwrap_or(0);
                        dict_insert(map, &mut w, key, PyObject::int(count - 1))?;
                    }
                }
                _ => {
                    return Err(PyException::type_error(
                        "Counter.subtract() argument must be a mapping or iterable",
                    ));
                }
            }
            Ok(PyObject::none())
        }
        "pop" => {
            check_args_min("pop", args, 1)?;
            let key = args[0].to_hashable_key()?;
            let default = if args.len() >= 2 {
                Some(args[1].clone())
            } else {
                None
            };
            let removed = {
                let mut w = map.write();
                dict_shift_remove(map, &mut w, &key)?
            };
            match removed {
                Some(v) => Ok(v),
                None => match default {
                    Some(d) => Ok(d),
                    None => Err(missing_key_error(&args[0])),
                },
            }
        }
        "setdefault" => {
            check_args_min("setdefault", args, 1)?;
            let key = args[0].to_hashable_key()?;
            let default = if args.len() >= 2 {
                args[1].clone()
            } else {
                PyObject::none()
            };
            let mut w = map.write();
            clear_key_compare_error();
            let result = match w.entry(key) {
                Entry::Occupied(entry) => entry.get().clone(),
                Entry::Vacant(entry) => {
                    entry.insert(default.clone());
                    mark_dict_storage_mutated(map);
                    default
                }
            };
            if let Some(err) = take_pending_eq_error() {
                clear_key_compare_error();
                return Err(err);
            }
            Ok(result)
        }
        "clear" => {
            check_args("clear", args, 0)?;
            let mut w = map.write();
            if !w.is_empty() {
                w.clear();
                mark_dict_storage_mutated(map);
            }
            Ok(PyObject::none())
        }
        "popitem" => {
            let is_ordered = map
                .read()
                .contains_key(&HashableKey::str_key(CompactString::from(
                    "__ordered_dict__",
                )));
            let last = if is_ordered {
                if let Some(v) = extract_kwarg(args, "last") {
                    v.is_truthy()
                } else if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::Dict(_))
                {
                    args[0].is_truthy()
                } else {
                    true
                }
            } else {
                check_args("popitem", args, 0)?;
                true
            };
            let mut w = map.write();
            // Skip hidden internal marker keys (__ordered_dict__, __move_to_end_fn__, etc.)
            let len = w.len();
            let entry = if last {
                let mut result = None;
                for i in (0..len).rev() {
                    if let Some((k, _)) = w.get_index(i) {
                        if !is_hidden_dict_key(k) {
                            result = w.shift_remove_index(i);
                            break;
                        }
                    }
                }
                result
            } else {
                let mut result = None;
                for i in 0..len {
                    if let Some((k, _)) = w.get_index(i) {
                        if !is_hidden_dict_key(k) {
                            result = w.shift_remove_index(i);
                            break;
                        }
                    }
                }
                result
            };
            if entry.is_some() {
                mark_dict_storage_mutated(map);
            }
            match entry {
                Some((k, v)) => Ok(PyObject::tuple(vec![k.to_object(), v])),
                None => Err(PyException::key_error("popitem(): dictionary is empty")),
            }
        }
        "most_common" => {
            // Counter.most_common(n) — return n most common (key, count) pairs sorted by count
            let r = map.read();
            let mut pairs: Vec<(HashableKey, i64)> = r.iter()
                .filter(|(k, _)| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__"))
                .map(|(k, v)| (k.clone(), v.as_int().unwrap_or(0)))
                .collect();
            pairs.sort_by(|a, b| b.1.cmp(&a.1));
            let n = if !args.is_empty() {
                args[0].as_int().unwrap_or(pairs.len() as i64) as usize
            } else {
                pairs.len()
            };
            let result: Vec<PyObjectRef> = pairs
                .into_iter()
                .take(n)
                .map(|(k, v)| PyObject::tuple(vec![k.to_object(), PyObject::int(v)]))
                .collect();
            Ok(PyObject::list(result))
        }
        "elements" => {
            // Counter.elements() — return elements repeated by count
            let r = map.read();
            let mut result = Vec::new();
            for (k, v) in r.iter() {
                if matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__")
                {
                    continue;
                }
                let count = v.as_int().unwrap_or(0);
                for _ in 0..count {
                    result.push(k.to_object());
                }
            }
            Ok(PyObject::list(result))
        }
        "move_to_end" => {
            check_args_min("move_to_end", args, 1)?;
            let key = args[0].to_hashable_key()?;
            let last = if let Some(v) = extract_kwarg(args, "last") {
                v.is_truthy()
            } else if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::Dict(_)) {
                args[1].is_truthy()
            } else {
                true
            };
            let mut w = map.write();
            if let Some(val) = dict_shift_remove(map, &mut w, &key)? {
                if last {
                    dict_insert(map, &mut w, key, val)?;
                } else {
                    let mut new_map = new_fx_hashkey_map();
                    new_map.insert(key, val);
                    for (k, v) in w.drain(..) {
                        new_map.insert(k, v);
                    }
                    *w = new_map;
                }
                mark_dict_storage_mutated(map);
                Ok(PyObject::none())
            } else {
                Err(missing_key_error(&args[0]))
            }
        }
        "__contains__" => {
            check_args_min("dict.__contains__", args, 1)?;
            let key = args[0].to_hashable_key()?;
            if is_hidden_dict_key(&key) {
                return Ok(PyObject::bool_val(false));
            }
            let contains = {
                let read = map.read();
                dict_contains_key(&read, &key)?
            };
            Ok(PyObject::bool_val(contains))
        }
        "__len__" => {
            let r = map.read();
            let hidden = r.keys().filter(|k| is_hidden_dict_key(k)).count();
            Ok(PyObject::int((r.len() - hidden) as i64))
        }
        "__iter__" => {
            let keys: Vec<PyObjectRef> = map
                .read()
                .keys()
                .filter(|k| !is_hidden_dict_key(k))
                .map(|k| k.to_object())
                .collect();
            Ok(PyObject::list(keys))
        }
        "__getitem__" => {
            check_args_min("dict.__getitem__", args, 1)?;
            let key = args[0].to_hashable_key()?;
            if is_hidden_dict_key(&key) {
                return Err(missing_key_error(&args[0]));
            }
            let found = {
                let read = map.read();
                dict_get(&read, &key)?
            };
            match found {
                Some(v) => Ok(v),
                None => {
                    let factory_key =
                        HashableKey::str_key(CompactString::from("__defaultdict_factory__"));
                    if let Some(factory) = map.read().get(&factory_key).cloned() {
                        let default = match &factory.payload {
                            PyObjectPayload::BuiltinType(name) => match name.as_str() {
                                "int" => PyObject::int(0),
                                "float" => PyObject::float(0.0),
                                "str" => PyObject::str_val(CompactString::new("")),
                                "list" => PyObject::list(vec![]),
                                "bool" => PyObject::bool_val(false),
                                "tuple" => PyObject::tuple(vec![]),
                                "set" => PyObject::set(new_fx_hashkey_map()),
                                "dict" => PyObject::dict(new_fx_hashkey_map()),
                                _ => return Err(missing_key_error(&args[0])),
                            },
                            _ => return Err(missing_key_error(&args[0])),
                        };
                        {
                            let mut w = map.write();
                            dict_insert(map, &mut w, key, default.clone())?;
                        }
                        Ok(default)
                    } else {
                        Err(missing_key_error(&args[0]))
                    }
                }
            }
        }
        "__eq__" => {
            check_args_min("dict.__eq__", args, 1)?;
            if let PyObjectPayload::Dict(other) = &args[0].payload {
                let a = map.read();
                let b = other.read();
                let a_visible: Vec<_> = a.iter().filter(|(k, _)| !is_hidden_dict_key(k)).collect();
                let b_visible: Vec<_> = b.iter().filter(|(k, _)| !is_hidden_dict_key(k)).collect();
                if a_visible.len() != b_visible.len() {
                    return Ok(PyObject::bool_val(false));
                }
                let mut eq = true;
                for (k, v) in a_visible {
                    match dict_get(&b, k)? {
                        Some(v2) if v.py_to_string() == v2.py_to_string() => {}
                        _ => {
                            eq = false;
                            break;
                        }
                    }
                }
                Ok(PyObject::bool_val(eq))
            } else {
                Ok(PyObject::bool_val(false))
            }
        }
        "__ne__" => {
            check_args_min("dict.__ne__", args, 1)?;
            if let PyObjectPayload::Dict(other) = &args[0].payload {
                let a = map.read();
                let b = other.read();
                let a_visible: Vec<_> = a.iter().filter(|(k, _)| !is_hidden_dict_key(k)).collect();
                let b_visible: Vec<_> = b.iter().filter(|(k, _)| !is_hidden_dict_key(k)).collect();
                if a_visible.len() != b_visible.len() {
                    return Ok(PyObject::bool_val(true));
                }
                let mut eq = true;
                for (k, v) in a_visible {
                    match dict_get(&b, k)? {
                        Some(v2) if v.py_to_string() == v2.py_to_string() => {}
                        _ => {
                            eq = false;
                            break;
                        }
                    }
                }
                Ok(PyObject::bool_val(!eq))
            } else {
                Ok(PyObject::bool_val(true))
            }
        }
        "__repr__" | "__str__" => {
            let r = map.read();
            let is_counter =
                r.contains_key(&HashableKey::str_key(CompactString::from("__counter__")));
            let mut visible: Vec<(HashableKey, PyObjectRef)> = r
                .iter()
                .filter(|(k, _)| !is_hidden_dict_key(k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if is_counter {
                visible.sort_by(|a, b| b.1.as_int().unwrap_or(0).cmp(&a.1.as_int().unwrap_or(0)));
                if visible.is_empty() {
                    return Ok(PyObject::str_val(CompactString::from("Counter()")));
                }
                let inner: Vec<String> = visible
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.to_object().repr(), v.repr()))
                    .collect();
                return Ok(PyObject::str_val(CompactString::from(format!(
                    "Counter({{{}}})",
                    inner.join(", ")
                ))));
            }
            let inner: Vec<String> = visible
                .iter()
                .map(|(k, v)| format!("{}: {}", k.to_object().repr(), v.repr()))
                .collect();
            Ok(PyObject::str_val(CompactString::from(format!(
                "{{{}}}",
                inner.join(", ")
            ))))
        }
        "__bool__" => Ok(PyObject::bool_val(!map.read().is_empty())),
        "__setitem__" => {
            check_args_min("dict.__setitem__", args, 2)?;
            let key = args[0].to_hashable_key()?;
            {
                let mut w = map.write();
                dict_insert(map, &mut w, key, args[1].clone())?;
            }
            Ok(PyObject::none())
        }
        "__delitem__" => {
            check_args_min("dict.__delitem__", args, 1)?;
            let key = args[0].to_hashable_key()?;
            let is_counter = map
                .read()
                .contains_key(&HashableKey::str_key(CompactString::from("__counter__")));
            let removed = {
                let mut w = map.write();
                dict_shift_remove(map, &mut w, &key)?
            };
            match removed {
                Some(_) => Ok(PyObject::none()),
                None if is_counter => Ok(PyObject::none()),
                None => Err(missing_key_error(&args[0])),
            }
        }
        "__hash__" => Err(PyException::type_error("unhashable type: 'dict'")),
        "__sizeof__" => Ok(PyObject::int(
            (std::mem::size_of::<FxHashKeyMap>() + map.read().len() * 64) as i64,
        )),
        _ => Err(PyException::attribute_error(format!(
            "'dict' object has no attribute '{}'",
            method
        ))),
    }
}
