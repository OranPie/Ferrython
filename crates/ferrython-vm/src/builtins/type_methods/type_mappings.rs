//! Dict-like type method dispatch.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::helpers::is_hidden_dict_key;
use ferrython_core::object::{
    check_args_min, new_fx_hashkey_map, FxHashKeyMap, PyCell, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use std::rc::Rc;

use super::extract_kwarg;

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
            Ok(map.read().get(&key).cloned().unwrap_or(default))
        }
        "copy" => Ok(PyObject::dict(map.read().clone())),
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
                            w.insert(key, PyObject::int(count + 1));
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
                            let existing = w.get(&k).and_then(|v| v.as_int()).unwrap_or(0);
                            let add = v.as_int().unwrap_or(0);
                            w.insert(k, PyObject::int(existing + add));
                        }
                    }
                    PyObjectPayload::List(items) => {
                        let items = items.read().clone();
                        let mut w = map.write();
                        for item in &items {
                            let key = item.to_hashable_key()?;
                            let count = w.get(&key).and_then(|v| v.as_int()).unwrap_or(0);
                            w.insert(key, PyObject::int(count + 1));
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
                        let other_items = other.read().clone();
                        let mut w = map.write();
                        for (k, v) in other_items {
                            w.insert(k, v);
                        }
                    }
                    PyObjectPayload::Instance(_) => {
                        if let Some(keys_method) = args[0].get_attr("keys") {
                            let keys_obj =
                                ferrython_core::object::call_callable(&keys_method, &[])?;
                            let keys = keys_obj.to_list()?;
                            let mut w = map.write();
                            for key_obj in keys {
                                let value = args[0].get_item(&key_obj)?;
                                w.insert(key_obj.to_hashable_key()?, value);
                            }
                        } else {
                            let items = args[0].to_list()?;
                            let mut w = map.write();
                            for item in &items {
                                match &item.payload {
                                    PyObjectPayload::Tuple(pair) if pair.len() == 2 => {
                                        let key = pair[0].to_hashable_key()?;
                                        w.insert(key, pair[1].clone());
                                    }
                                    PyObjectPayload::Tuple(pair) => {
                                        return Err(PyException::value_error(
                                            format!("dictionary update sequence element has length {}; 2 is required", pair.len())
                                        ));
                                    }
                                    PyObjectPayload::List(pair_items) => {
                                        let pair = pair_items.read();
                                        if pair.len() == 2 {
                                            let key = pair[0].to_hashable_key()?;
                                            w.insert(key, pair[1].clone());
                                        } else {
                                            return Err(PyException::value_error(
                                                format!("dictionary update sequence element has length {}; 2 is required", pair.len())
                                            ));
                                        }
                                    }
                                    _ => {
                                        return Err(PyException::type_error("cannot convert dictionary update sequence element to a sequence"));
                                    }
                                }
                            }
                        }
                    }
                    PyObjectPayload::List(items) => {
                        let items = items.read().clone();
                        let mut w = map.write();
                        for item in &items {
                            match &item.payload {
                                PyObjectPayload::Tuple(pair) if pair.len() == 2 => {
                                    let key = pair[0].to_hashable_key()?;
                                    w.insert(key, pair[1].clone());
                                }
                                PyObjectPayload::Tuple(pair) => {
                                    return Err(PyException::value_error(
                                        format!("dictionary update sequence element has length {}; 2 is required", pair.len())
                                    ));
                                }
                                PyObjectPayload::List(pair_items) => {
                                    let pair = pair_items.read();
                                    if pair.len() == 2 {
                                        let key = pair[0].to_hashable_key()?;
                                        w.insert(key, pair[1].clone());
                                    } else {
                                        return Err(PyException::value_error(
                                            format!("dictionary update sequence element has length {}; 2 is required", pair.len())
                                        ));
                                    }
                                }
                                _ => {
                                    return Err(PyException::type_error("cannot convert dictionary update sequence element to a sequence"));
                                }
                            }
                        }
                    }
                    _ => {
                        return Err(PyException::type_error(
                            "update() argument must be a mapping or iterable",
                        ));
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
                        let existing = w.get(&k).and_then(|v| v.as_int()).unwrap_or(0);
                        let sub = v.as_int().unwrap_or(0);
                        w.insert(k, PyObject::int(existing - sub));
                    }
                }
                PyObjectPayload::Str(s) => {
                    let mut w = map.write();
                    for ch in s.chars() {
                        let key = HashableKey::str_key(CompactString::from(ch.to_string()));
                        let count = w.get(&key).and_then(|v| v.as_int()).unwrap_or(0);
                        w.insert(key, PyObject::int(count - 1));
                    }
                }
                PyObjectPayload::List(items) => {
                    let items = items.read().clone();
                    let mut w = map.write();
                    for item in &items {
                        let key = item.to_hashable_key()?;
                        let count = w.get(&key).and_then(|v| v.as_int()).unwrap_or(0);
                        w.insert(key, PyObject::int(count - 1));
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
            match map.write().swap_remove(&key) {
                Some(v) => Ok(v),
                None => match default {
                    Some(d) => Ok(d),
                    None => Err(PyException::key_error(args[0].repr())),
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
            Ok(w.entry(key).or_insert(default).clone())
        }
        "clear" => {
            map.write().clear();
            Ok(PyObject::none())
        }
        "popitem" => {
            let last = if let Some(v) = extract_kwarg(args, "last") {
                v.is_truthy()
            } else if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::Dict(_)) {
                args[0].is_truthy()
            } else {
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
            if let Some(val) = w.shift_remove(&key) {
                if last {
                    w.insert(key, val);
                } else {
                    let mut new_map = new_fx_hashkey_map();
                    new_map.insert(key, val);
                    for (k, v) in w.drain(..) {
                        new_map.insert(k, v);
                    }
                    *w = new_map;
                }
                Ok(PyObject::none())
            } else {
                Err(PyException::key_error(args[0].repr()))
            }
        }
        "__contains__" => {
            check_args_min("dict.__contains__", args, 1)?;
            let key =
                args[0]
                    .to_hashable_key()
                    .unwrap_or(HashableKey::str_key(CompactString::from(
                        args[0].py_to_string(),
                    )));
            if is_hidden_dict_key(&key) {
                return Ok(PyObject::bool_val(false));
            }
            Ok(PyObject::bool_val(map.read().contains_key(&key)))
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
            let key =
                args[0]
                    .to_hashable_key()
                    .unwrap_or(HashableKey::str_key(CompactString::from(
                        args[0].py_to_string(),
                    )));
            if is_hidden_dict_key(&key) {
                return Err(PyException::key_error(args[0].repr()));
            }
            match map.read().get(&key).cloned() {
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
                                _ => return Err(PyException::key_error(args[0].repr())),
                            },
                            _ => return Err(PyException::key_error(args[0].repr())),
                        };
                        map.write().insert(key, default.clone());
                        Ok(default)
                    } else {
                        Err(PyException::key_error(args[0].repr()))
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
                let eq = a_visible.iter().all(|(k, v)| {
                    b.get(*k)
                        .map_or(false, |v2| v.py_to_string() == v2.py_to_string())
                });
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
                let eq = a_visible.iter().all(|(k, v)| {
                    b.get(*k)
                        .map_or(false, |v2| v.py_to_string() == v2.py_to_string())
                });
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
            map.write().insert(key, args[1].clone());
            Ok(PyObject::none())
        }
        "__delitem__" => {
            check_args_min("dict.__delitem__", args, 1)?;
            let key = args[0].to_hashable_key()?;
            let is_counter = map
                .read()
                .contains_key(&HashableKey::str_key(CompactString::from("__counter__")));
            match map.write().swap_remove(&key) {
                Some(_) => Ok(PyObject::none()),
                None if is_counter => Ok(PyObject::none()),
                None => Err(PyException::key_error(args[0].py_to_string())),
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
