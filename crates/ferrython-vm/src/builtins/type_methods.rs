//! Collection/numeric type method dispatch (list, dict, set, tuple, int, float, bytes)

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    check_args_min,
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

/// Extract a keyword argument from a trailing kwargs dict (if present).
/// The generic BuiltinBoundMethod kwargs handler passes kwargs as a trailing Dict arg.
fn extract_kwarg(args: &[PyObjectRef], name: &str) -> Option<PyObjectRef> {
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(map) = &last.payload {
            let r = map.read();
            return r.get(&HashableKey::Str(CompactString::from(name))).cloned();
        }
    }
    None
}

pub(super) fn call_list_method(items: Arc<RwLock<Vec<PyObjectRef>>>, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "copy" => Ok(PyObject::list(items.read().to_vec())),
        "count" => {
            check_args_min("count", args, 1)?;
            let target = &args[0];
            let c = items.read().iter().filter(|x| x.py_to_string() == target.py_to_string()).count();
            Ok(PyObject::int(c as i64))
        }
        "index" => {
            check_args_min("index", args, 1)?;
            let target = &args[0];
            let items_r = items.read();
            let len = items_r.len();
            let start = if args.len() > 1 {
                let s = args[1].to_int().unwrap_or(0);
                if s < 0 { (len as i64 + s).max(0) as usize } else { s as usize }
            } else { 0 };
            let stop = if args.len() > 2 {
                let s = args[2].to_int().unwrap_or(len as i64);
                if s < 0 { (len as i64 + s).max(0) as usize } else { (s as usize).min(len) }
            } else { len };
            for i in start..stop {
                if items_r[i].py_to_string() == target.py_to_string() {
                    return Ok(PyObject::int(i as i64));
                }
            }
            Err(PyException::value_error(format!("{} is not in list", target.py_to_string())))
        }
        "append" => {
            check_args_min("append", args, 1)?;
            items.write().push(args[0].clone());
            Ok(PyObject::none())
        }
        "extend" => {
            check_args_min("extend", args, 1)?;
            let other = args[0].to_list()?;
            items.write().extend(other);
            Ok(PyObject::none())
        }
        "insert" => {
            check_args_min("insert", args, 2)?;
            let idx = args[0].to_int()?;
            let mut w = items.write();
            let len = w.len() as i64;
            let actual = if idx < 0 { (len + idx).max(0) as usize } else { (idx as usize).min(w.len()) };
            w.insert(actual, args[1].clone());
            Ok(PyObject::none())
        }
        "pop" => {
            let mut w = items.write();
            if w.is_empty() {
                return Err(PyException::index_error("pop from empty list"));
            }
            if args.is_empty() {
                Ok(w.pop().unwrap())
            } else {
                let idx = args[0].to_int()?;
                let len = w.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len {
                    return Err(PyException::index_error("pop index out of range"));
                }
                Ok(w.remove(actual as usize))
            }
        }
        "remove" => {
            check_args_min("remove", args, 1)?;
            let target = &args[0];
            let mut w = items.write();
            let pos = w.iter().position(|x| x.py_to_string() == target.py_to_string());
            match pos {
                Some(i) => { w.remove(i); Ok(PyObject::none()) }
                None => Err(PyException::value_error("list.remove(x): x not in list")),
            }
        }
        "reverse" => {
            items.write().reverse();
            Ok(PyObject::none())
        }
        "sort" => {
            let mut w = items.write();
            let mut v: Vec<_> = w.drain(..).collect();
            v.sort_by(|a, b| {
                partial_cmp_for_sort(a, b).unwrap_or(std::cmp::Ordering::Equal)
            });
            w.extend(v);
            Ok(PyObject::none())
        }
        "clear" => {
            items.write().clear();
            Ok(PyObject::none())
        }
        _ => Err(PyException::attribute_error(format!(
            "'list' object has no attribute '{}'", method
        ))),
    }
}

pub(super) fn call_dict_method(map: &Arc<RwLock<IndexMap<HashableKey, PyObjectRef>>>, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "keys" => {
            let keys: Vec<PyObjectRef> = map.read().keys()
                .filter(|k| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__"))
                .map(|k| k.to_object()).collect();
            Ok(PyObject::list(keys))
        }
        "values" => {
            let r = map.read();
            let vals: Vec<PyObjectRef> = r.iter()
                .filter(|(k, _)| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__"))
                .map(|(_, v)| v.clone()).collect();
            Ok(PyObject::list(vals))
        }
        "items" => {
            let pairs: Vec<PyObjectRef> = map.read().iter()
                .filter(|(k, _)| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__"))
                .map(|(k, v)| PyObject::tuple(vec![k.to_object(), v.clone()]))
                .collect();
            Ok(PyObject::list(pairs))
        }
        "get" => {
            check_args_min("get", args, 1)?;
            let key = args[0].to_hashable_key()?;
            let default = if args.len() >= 2 { args[1].clone() } else { PyObject::none() };
            Ok(map.read().get(&key).cloned().unwrap_or(default))
        }
        "copy" => {
            Ok(PyObject::dict(map.read().clone()))
        }
        "update" => {
            check_args_min("update", args, 1)?;
            // Check if this is a Counter (has __counter__ key)
            let is_counter = map.read().contains_key(&HashableKey::Str(CompactString::from("__counter__")));
            if is_counter {
                // Counter.update: add counts from iterable or mapping
                match &args[0].payload {
                    PyObjectPayload::Str(s) => {
                        let mut w = map.write();
                        for ch in s.chars() {
                            let key = HashableKey::Str(CompactString::from(ch.to_string()));
                            let count = w.get(&key).and_then(|v| v.as_int()).unwrap_or(0);
                            w.insert(key, PyObject::int(count + 1));
                        }
                    }
                    PyObjectPayload::Dict(other) => {
                        let other_items = other.read().clone();
                        let mut w = map.write();
                        for (k, v) in other_items {
                            if matches!(&k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__") { continue; }
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
                    _ => {}
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
                    PyObjectPayload::List(items) => {
                        let items = items.read().clone();
                        let mut w = map.write();
                        for item in &items {
                            match &item.payload {
                                PyObjectPayload::Tuple(pair) if pair.len() == 2 => {
                                    let key = pair[0].to_hashable_key()?;
                                    w.insert(key, pair[1].clone());
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
                    _ => {}
                }
            }
            Ok(PyObject::none())
        }
        "subtract" => {
            // Counter.subtract — subtract counts from iterable or mapping
            check_args_min("subtract", args, 1)?;
            match &args[0].payload {
                PyObjectPayload::Dict(other) => {
                    let other_items = other.read().clone();
                    let mut w = map.write();
                    for (k, v) in other_items {
                        if matches!(&k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__") { continue; }
                        let existing = w.get(&k).and_then(|v| v.as_int()).unwrap_or(0);
                        let sub = v.as_int().unwrap_or(0);
                        w.insert(k, PyObject::int(existing - sub));
                    }
                }
                PyObjectPayload::Str(s) => {
                    let mut w = map.write();
                    for ch in s.chars() {
                        let key = HashableKey::Str(CompactString::from(ch.to_string()));
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
                _ => {}
            }
            Ok(PyObject::none())
        }
        "pop" => {
            check_args_min("pop", args, 1)?;
            let key = args[0].to_hashable_key()?;
            let default = if args.len() >= 2 { Some(args[1].clone()) } else { None };
            match map.write().shift_remove(&key) {
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
            let default = if args.len() >= 2 { args[1].clone() } else { PyObject::none() };
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
            let entry = if last {
                w.pop()
            } else {
                w.shift_remove_index(0).map(|(k, v)| (k, v))
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
            let n = if !args.is_empty() { args[0].as_int().unwrap_or(pairs.len() as i64) as usize } else { pairs.len() };
            let result: Vec<PyObjectRef> = pairs.into_iter().take(n)
                .map(|(k, v)| PyObject::tuple(vec![k.to_object(), PyObject::int(v)]))
                .collect();
            Ok(PyObject::list(result))
        }
        "elements" => {
            // Counter.elements() — return elements repeated by count
            let r = map.read();
            let mut result = Vec::new();
            for (k, v) in r.iter() {
                if matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__") { continue; }
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
                    let mut new_map = IndexMap::new();
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
        _ => Err(PyException::attribute_error(format!(
            "'dict' object has no attribute '{}'", method
        ))),
    }
}

pub(super) fn call_tuple_method(items: &[PyObjectRef], method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "count" => {
            check_args_min("count", args, 1)?;
            let target = &args[0];
            let c = items.iter().filter(|x| x.py_to_string() == target.py_to_string()).count();
            Ok(PyObject::int(c as i64))
        }
        "index" => {
            check_args_min("index", args, 1)?;
            let target = &args[0];
            for (i, x) in items.iter().enumerate() {
                if x.py_to_string() == target.py_to_string() {
                    return Ok(PyObject::int(i as i64));
                }
            }
            Err(PyException::value_error("tuple.index(x): x not in tuple"))
        }
        _ => Err(PyException::attribute_error(format!(
            "'tuple' object has no attribute '{}'", method
        ))),
    }
}

pub(super) fn call_set_method(m: &Arc<RwLock<IndexMap<HashableKey, PyObjectRef>>>, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "copy" => Ok(PyObject::set(m.read().clone())),
        "union" | "__or__" => {
            check_args_min("union", args, 1)?;
            let mut result = m.read().clone();
            let other_list = args[0].to_list()?;
            for item in other_list {
                let hk = item.to_hashable_key()?;
                result.entry(hk).or_insert(item);
            }
            Ok(PyObject::set(result))
        }
        "intersection" | "__and__" => {
            check_args_min("intersection", args, 1)?;
            let other_items = args[0].to_list()?;
            let other_keys: std::collections::HashSet<String> = other_items.iter()
                .map(|x| x.py_to_string()).collect();
            let guard = m.read();
            let result: IndexMap<HashableKey, PyObjectRef> = guard.iter()
                .filter(|(_, v)| other_keys.contains(&v.py_to_string()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            Ok(PyObject::set(result))
        }
        "difference" | "__sub__" => {
            check_args_min("difference", args, 1)?;
            let other_items = args[0].to_list()?;
            let other_keys: std::collections::HashSet<String> = other_items.iter()
                .map(|x| x.py_to_string()).collect();
            let guard = m.read();
            let result: IndexMap<HashableKey, PyObjectRef> = guard.iter()
                .filter(|(_, v)| !other_keys.contains(&v.py_to_string()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            Ok(PyObject::set(result))
        }
        "symmetric_difference" | "__xor__" => {
            check_args_min("symmetric_difference", args, 1)?;
            let other_items = args[0].to_list()?;
            let guard = m.read();
            let self_keys: std::collections::HashSet<String> = guard.values()
                .map(|x| x.py_to_string()).collect();
            let other_keys: std::collections::HashSet<String> = other_items.iter()
                .map(|x| x.py_to_string()).collect();
            let mut result = IndexMap::new();
            for (k, v) in guard.iter() {
                if !other_keys.contains(&v.py_to_string()) {
                    result.insert(k.clone(), v.clone());
                }
            }
            for item in &other_items {
                if !self_keys.contains(&item.py_to_string()) {
                    if let Ok(hk) = item.to_hashable_key() {
                        result.insert(hk, item.clone());
                    }
                }
            }
            Ok(PyObject::set(result))
        }
        "issubset" => {
            check_args_min("issubset", args, 1)?;
            let other_items = args[0].to_list()?;
            let other_keys: std::collections::HashSet<String> = other_items.iter()
                .map(|x| x.py_to_string()).collect();
            let all_in = m.read().values().all(|v| other_keys.contains(&v.py_to_string()));
            Ok(PyObject::bool_val(all_in))
        }
        "issuperset" => {
            check_args_min("issuperset", args, 1)?;
            let other_items = args[0].to_list()?;
            let guard = m.read();
            let self_keys: std::collections::HashSet<String> = guard.values()
                .map(|x| x.py_to_string()).collect();
            let all_in = other_items.iter().all(|v| self_keys.contains(&v.py_to_string()));
            Ok(PyObject::bool_val(all_in))
        }
        "isdisjoint" => {
            check_args_min("isdisjoint", args, 1)?;
            let other_items = args[0].to_list()?;
            let guard = m.read();
            let self_keys: std::collections::HashSet<String> = guard.values()
                .map(|x| x.py_to_string()).collect();
            let none_in = other_items.iter().all(|v| !self_keys.contains(&v.py_to_string()));
            Ok(PyObject::bool_val(none_in))
        }
        "add" => {
            check_args_min("add", args, 1)?;
            let hk = args[0].to_hashable_key()?;
            m.write().insert(hk, args[0].clone());
            Ok(PyObject::none())
        }
        "remove" => {
            check_args_min("remove", args, 1)?;
            let hk = args[0].to_hashable_key()?;
            if m.write().shift_remove(&hk).is_none() {
                return Err(PyException::key_error(args[0].repr()));
            }
            Ok(PyObject::none())
        }
        "discard" => {
            check_args_min("discard", args, 1)?;
            let hk = args[0].to_hashable_key()?;
            m.write().shift_remove(&hk);
            Ok(PyObject::none())
        }
        "pop" => {
            let mut guard = m.write();
            if guard.is_empty() {
                return Err(PyException::key_error("pop from an empty set"));
            }
            let key = guard.keys().next().unwrap().clone();
            let val = guard.shift_remove(&key).unwrap();
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
                let items = arg.to_list()?;
                for item in items {
                    if let Ok(hk) = item.to_hashable_key() {
                        guard.insert(hk, item);
                    }
                }
            }
            Ok(PyObject::none())
        }
        "difference_update" => {
            check_args_min("difference_update", args, 1)?;
            let other_items = args[0].to_list()?;
            let remove_keys: Vec<HashableKey> = other_items.iter()
                .filter_map(|x| x.to_hashable_key().ok())
                .collect();
            let mut guard = m.write();
            for k in &remove_keys {
                guard.shift_remove(k);
            }
            Ok(PyObject::none())
        }
        "intersection_update" => {
            check_args_min("intersection_update", args, 1)?;
            let other_items = args[0].to_list()?;
            let other_keys: std::collections::HashSet<String> = other_items.iter()
                .map(|x| x.py_to_string()).collect();
            let mut guard = m.write();
            guard.retain(|_, v| other_keys.contains(&v.py_to_string()));
            Ok(PyObject::none())
        }
        "symmetric_difference_update" => {
            check_args_min("symmetric_difference_update", args, 1)?;
            let other_items = args[0].to_list()?;
            let mut guard = m.write();
            let self_keys: std::collections::HashSet<String> = guard.values()
                .map(|x| x.py_to_string()).collect();
            // Remove items that are in both
            let mut to_remove = Vec::new();
            for item in &other_items {
                let s = item.py_to_string();
                if self_keys.contains(&s) {
                    if let Ok(hk) = item.to_hashable_key() {
                        to_remove.push(hk);
                    }
                }
            }
            for k in &to_remove {
                guard.shift_remove(k);
            }
            // Add items from other that weren't in self
            for item in &other_items {
                if !self_keys.contains(&item.py_to_string()) {
                    if let Ok(hk) = item.to_hashable_key() {
                        guard.insert(hk, item.clone());
                    }
                }
            }
            Ok(PyObject::none())
        }
        _ => Err(PyException::attribute_error(format!(
            "'set' object has no attribute '{}'", method
        ))),
    }
}

pub(super) fn call_frozenset_method(m: &IndexMap<HashableKey, PyObjectRef>, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "copy" => Ok(PyObject::frozenset(m.clone())),
        "union" | "__or__" => {
            check_args_min("union", args, 1)?;
            let mut result = m.clone();
            let other_list = args[0].to_list()?;
            for item in other_list {
                let hk = item.to_hashable_key()?;
                result.entry(hk).or_insert(item);
            }
            Ok(PyObject::frozenset(result))
        }
        "intersection" | "__and__" => {
            check_args_min("intersection", args, 1)?;
            let other_items = args[0].to_list()?;
            let other_keys: std::collections::HashSet<String> = other_items.iter()
                .map(|x| x.py_to_string()).collect();
            let result: IndexMap<HashableKey, PyObjectRef> = m.iter()
                .filter(|(_, v)| other_keys.contains(&v.py_to_string()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            Ok(PyObject::frozenset(result))
        }
        "difference" | "__sub__" => {
            check_args_min("difference", args, 1)?;
            let other_items = args[0].to_list()?;
            let other_keys: std::collections::HashSet<String> = other_items.iter()
                .map(|x| x.py_to_string()).collect();
            let result: IndexMap<HashableKey, PyObjectRef> = m.iter()
                .filter(|(_, v)| !other_keys.contains(&v.py_to_string()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            Ok(PyObject::frozenset(result))
        }
        "symmetric_difference" | "__xor__" => {
            check_args_min("symmetric_difference", args, 1)?;
            let other_items = args[0].to_list()?;
            let self_keys: std::collections::HashSet<String> = m.values()
                .map(|x| x.py_to_string()).collect();
            let other_keys: std::collections::HashSet<String> = other_items.iter()
                .map(|x| x.py_to_string()).collect();
            let mut result = IndexMap::new();
            for (k, v) in m.iter() {
                if !other_keys.contains(&v.py_to_string()) {
                    result.insert(k.clone(), v.clone());
                }
            }
            for item in &other_items {
                if !self_keys.contains(&item.py_to_string()) {
                    if let Ok(hk) = item.to_hashable_key() {
                        result.insert(hk, item.clone());
                    }
                }
            }
            Ok(PyObject::frozenset(result))
        }
        "issubset" => {
            check_args_min("issubset", args, 1)?;
            let other_items = args[0].to_list()?;
            let other_keys: std::collections::HashSet<String> = other_items.iter()
                .map(|x| x.py_to_string()).collect();
            let all_in = m.values().all(|v| other_keys.contains(&v.py_to_string()));
            Ok(PyObject::bool_val(all_in))
        }
        "issuperset" => {
            check_args_min("issuperset", args, 1)?;
            let other_items = args[0].to_list()?;
            let self_keys: std::collections::HashSet<String> = m.values()
                .map(|x| x.py_to_string()).collect();
            let all_in = other_items.iter().all(|v| self_keys.contains(&v.py_to_string()));
            Ok(PyObject::bool_val(all_in))
        }
        "isdisjoint" => {
            check_args_min("isdisjoint", args, 1)?;
            let other_items = args[0].to_list()?;
            let self_keys: std::collections::HashSet<String> = m.values()
                .map(|x| x.py_to_string()).collect();
            let none_in = other_items.iter().all(|v| !self_keys.contains(&v.py_to_string()));
            Ok(PyObject::bool_val(none_in))
        }
        "__len__" => Ok(PyObject::int(m.len() as i64)),
        _ => Err(PyException::attribute_error(format!(
            "'frozenset' object has no attribute '{}'", method
        ))),
    }
}

pub(super) fn call_int_method(_receiver: &PyObjectRef, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "bit_length" => {
            let n = _receiver.to_int()?;
            Ok(PyObject::int(if n == 0 { 0 } else { 64 - n.abs().leading_zeros() as i64 }))
        }
        "bit_count" => {
            let n = _receiver.to_int()?;
            Ok(PyObject::int(n.abs().count_ones() as i64))
        }
        "to_bytes" => {
            let n = _receiver.to_int()?;
            if args.is_empty() {
                return Err(PyException::type_error("to_bytes() requires at least 1 argument"));
            }
            let length = args[0].to_int()? as usize;
            // Extract byteorder and signed from positional or kwargs dict
            let mut byteorder = "big".to_string();
            let mut signed = false;
            let mut _kwarg_start = 1;
            // Check if last arg is a kwargs dict
            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(map) = &last.payload {
                    let map_r = map.read();
                    if let Some(bo) = map_r.get(&HashableKey::Str(CompactString::from("byteorder"))) {
                        byteorder = bo.py_to_string();
                    }
                    if let Some(s) = map_r.get(&HashableKey::Str(CompactString::from("signed"))) {
                        signed = s.is_truthy();
                    }
                    _kwarg_start = args.len(); // skip kwargs dict for positional scan
                }
            }
            if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::Dict(_)) {
                byteorder = args[1].py_to_string();
            }
            if args.len() >= 3 && !matches!(&args[2].payload, PyObjectPayload::Dict(_)) {
                signed = args[2].is_truthy();
            }
            let val_to_encode: u64 = if signed && n < 0 {
                // Two's complement for negative numbers
                let bits = length * 8;
                ((1i128 << bits) + n as i128) as u64
            } else {
                n.unsigned_abs()
            };
            let bytes: Vec<u8> = match byteorder.as_str() {
                "big" => {
                    let mut result = vec![0u8; length];
                    let mut val = val_to_encode;
                    for i in (0..length).rev() {
                        result[i] = (val & 0xff) as u8;
                        val >>= 8;
                    }
                    result
                }
                "little" => {
                    let mut result = vec![0u8; length];
                    let mut val = val_to_encode;
                    for byte in result.iter_mut().take(length) {
                        *byte = (val & 0xff) as u8;
                        val >>= 8;
                    }
                    result
                }
                _ => return Err(PyException::value_error("byteorder must be 'big' or 'little'")),
            };
            Ok(PyObject::bytes(bytes))
        }
        "as_integer_ratio" => {
            let n = _receiver.to_int()?;
            Ok(PyObject::tuple(vec![PyObject::int(n), PyObject::int(1)]))
        }
        "conjugate" => Ok(_receiver.clone()),
        "real" => Ok(_receiver.clone()),
        "imag" => Ok(PyObject::int(0)),
        "numerator" => Ok(_receiver.clone()),
        "denominator" => Ok(PyObject::int(1)),
        _ => Err(PyException::attribute_error(format!(
            "'int' object has no attribute '{}'", method
        ))),
    }
}

pub(super) fn call_float_method(f: f64, method: &str, _args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "is_integer" => Ok(PyObject::bool_val(f.fract() == 0.0)),
        "hex" => {
            // Python's float.hex() format
            let (mantissa, exponent, sign) = if f == 0.0 {
                (0u64, 0i32, if f.is_sign_negative() { "-" } else { "" })
            } else {
                let bits = f.to_bits();
                let sign = if bits >> 63 != 0 { "-" } else { "" };
                let exp = ((bits >> 52) & 0x7ff) as i32 - 1023;
                let mant = bits & 0x000f_ffff_ffff_ffff;
                (mant, exp, sign)
            };
            Ok(PyObject::str_val(CompactString::from(format!(
                "{}0x1.{:013x}p{:+}", sign, mantissa, exponent
            ))))
        }
        "as_integer_ratio" => {
            if f.is_infinite() || f.is_nan() {
                return Err(PyException::value_error("cannot convert Infinity or NaN to integer ratio"));
            }
            // Decompose f into mantissa * 2^exponent
            let (mantissa, exponent) = {
                let bits = f.to_bits();
                let sign: i64 = if bits >> 63 != 0 { -1 } else { 1 };
                let exp = ((bits >> 52) & 0x7ff) as i64;
                let frac = (bits & 0x000f_ffff_ffff_ffff) as i64;
                if exp == 0 {
                    // Subnormal
                    (sign * frac, -1022i64 - 52)
                } else {
                    (sign * ((1i64 << 52) | frac), exp - 1023 - 52)
                }
            };
            let (numer, denom) = if exponent >= 0 {
                (mantissa << exponent.min(62), 1i64)
            } else {
                (mantissa, 1i64 << (-exponent).min(62))
            };
            // Simplify by GCD
            fn gcd(mut a: i64, mut b: i64) -> i64 {
                a = a.abs(); b = b.abs();
                while b != 0 { let t = b; b = a % b; a = t; }
                a
            }
            let g = gcd(numer, denom);
            Ok(PyObject::tuple(vec![PyObject::int(numer / g), PyObject::int(denom / g)]))
        }
        "conjugate" => Ok(PyObject::float(f)),
        "real" => Ok(PyObject::float(f)),
        "imag" => Ok(PyObject::float(0.0)),
        _ => Err(PyException::attribute_error(format!(
            "'float' object has no attribute '{}'", method
        ))),
    }
}

pub(super) fn call_bytes_method(b: &[u8], method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "decode" => {
            let encoding = if !args.is_empty() { args[0].py_to_string().to_lowercase() } else { "utf-8".to_string() };
            let errors = if args.len() > 1 { args[1].py_to_string() } else { "strict".to_string() };
            match encoding.as_str() {
                "utf-8" | "utf8" => {
                    match errors.as_str() {
                        "strict" => {
                            match std::str::from_utf8(b) {
                                Ok(s) => Ok(PyObject::str_val(CompactString::from(s))),
                                Err(e) => Err(PyException::new(
                                    ExceptionKind::UnicodeDecodeError,
                                    format!("'utf-8' codec can't decode byte 0x{:02x} in position {}", b[e.valid_up_to()], e.valid_up_to()),
                                )),
                            }
                        }
                        "ignore" => {
                            let s: String = b.iter().filter(|&&x| x < 0x80).map(|&x| x as char).collect();
                            Ok(PyObject::str_val(CompactString::from(s)))
                        }
                        "replace" | _ => {
                            Ok(PyObject::str_val(CompactString::from(String::from_utf8_lossy(b))))
                        }
                    }
                }
                "ascii" => {
                    match errors.as_str() {
                        "strict" => {
                            for (i, &byte) in b.iter().enumerate() {
                                if byte > 127 {
                                    return Err(PyException::new(
                                        ExceptionKind::UnicodeDecodeError,
                                        format!("'ascii' codec can't decode byte 0x{:02x} in position {}", byte, i),
                                    ));
                                }
                            }
                            Ok(PyObject::str_val(CompactString::from(String::from_utf8_lossy(b))))
                        }
                        "ignore" => {
                            let s: String = b.iter().filter(|&&x| x < 128).map(|&x| x as char).collect();
                            Ok(PyObject::str_val(CompactString::from(s)))
                        }
                        "replace" | _ => {
                            let s: String = b.iter().map(|&x| if x < 128 { x as char } else { '\u{FFFD}' }).collect();
                            Ok(PyObject::str_val(CompactString::from(s)))
                        }
                    }
                }
                "latin-1" | "latin1" | "iso-8859-1" | "iso8859-1" => {
                    let s: String = b.iter().map(|&x| x as char).collect();
                    Ok(PyObject::str_val(CompactString::from(s)))
                }
                _ => Err(PyException::new(
                    ExceptionKind::LookupError,
                    format!("unknown encoding: {}", encoding),
                )),
            }
        }
        "hex" => Ok(PyObject::str_val(CompactString::from(hex::encode(b)))),
        "count" => {
            if args.is_empty() { return Err(PyException::type_error("count requires an argument")); }
            match &args[0].payload {
                PyObjectPayload::Int(n) => {
                    let byte = n.to_i64().unwrap_or(-1);
                    if byte < 0 || byte > 255 { return Ok(PyObject::int(0)); }
                    Ok(PyObject::int(b.iter().filter(|&&x| x == byte as u8).count() as i64))
                }
                PyObjectPayload::Bytes(needle) | PyObjectPayload::ByteArray(needle) => {
                    if needle.is_empty() { return Ok(PyObject::int(b.len() as i64 + 1)); }
                    let mut count = 0i64;
                    let mut start = 0;
                    while start + needle.len() <= b.len() {
                        if &b[start..start + needle.len()] == needle.as_slice() {
                            count += 1;
                            start += needle.len();
                        } else {
                            start += 1;
                        }
                    }
                    Ok(PyObject::int(count))
                }
                _ => Err(PyException::type_error("a bytes-like object is required")),
            }
        }
        "find" => {
            if args.is_empty() { return Err(PyException::type_error("find requires an argument")); }
            if let PyObjectPayload::Bytes(needle) | PyObjectPayload::ByteArray(needle) = &args[0].payload {
                let pos = b.windows(needle.len()).position(|w| w == needle.as_slice());
                Ok(PyObject::int(pos.map(|p| p as i64).unwrap_or(-1)))
            } else if let Some(n) = args[0].as_int() {
                let byte = n as u8;
                Ok(PyObject::int(b.iter().position(|&x| x == byte).map(|p| p as i64).unwrap_or(-1)))
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "startswith" => {
            if args.is_empty() { return Err(PyException::type_error("startswith requires an argument")); }
            if let PyObjectPayload::Bytes(prefix) | PyObjectPayload::ByteArray(prefix) = &args[0].payload {
                Ok(PyObject::bool_val(b.starts_with(prefix)))
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "endswith" => {
            if args.is_empty() { return Err(PyException::type_error("endswith requires an argument")); }
            if let PyObjectPayload::Bytes(suffix) | PyObjectPayload::ByteArray(suffix) = &args[0].payload {
                Ok(PyObject::bool_val(b.ends_with(suffix)))
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "upper" => Ok(PyObject::bytes(b.to_ascii_uppercase())),
        "lower" => Ok(PyObject::bytes(b.to_ascii_lowercase())),
        "strip" => {
            let stripped = b.iter().copied()
                .skip_while(|c| c.is_ascii_whitespace())
                .collect::<Vec<u8>>();
            let stripped: Vec<u8> = stripped.into_iter().rev()
                .skip_while(|c| c.is_ascii_whitespace())
                .collect::<Vec<u8>>().into_iter().rev().collect();
            Ok(PyObject::bytes(stripped))
        }
        "split" => {
            if args.is_empty() {
                // Split on whitespace
                let parts: Vec<PyObjectRef> = String::from_utf8_lossy(b)
                    .split_whitespace()
                    .map(|s| PyObject::bytes(s.as_bytes().to_vec()))
                    .collect();
                Ok(PyObject::list(parts))
            } else if let PyObjectPayload::Bytes(sep) | PyObjectPayload::ByteArray(sep) = &args[0].payload {
                let mut parts = Vec::new();
                let mut start = 0;
                while start <= b.len() {
                    if let Some(pos) = b[start..].windows(sep.len()).position(|w| w == sep.as_slice()) {
                        parts.push(PyObject::bytes(b[start..start + pos].to_vec()));
                        start = start + pos + sep.len();
                    } else {
                        parts.push(PyObject::bytes(b[start..].to_vec()));
                        break;
                    }
                }
                Ok(PyObject::list(parts))
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "join" => {
            if args.is_empty() { return Err(PyException::type_error("join requires an argument")); }
            // Extract items from list, tuple, or other sequence types
            let items: Vec<PyObjectRef> = match &args[0].payload {
                PyObjectPayload::List(items) => items.read().clone(),
                PyObjectPayload::Tuple(items) => items.clone(),
                PyObjectPayload::FrozenSet(items) => items.values().cloned().collect(),
                PyObjectPayload::Set(items) => items.read().values().cloned().collect(),
                _ => return Err(PyException::type_error("can only join an iterable")),
            };
            let mut result = Vec::new();
            for (i, item) in items.iter().enumerate() {
                if i > 0 { result.extend_from_slice(b); }
                match &item.payload {
                    PyObjectPayload::Bytes(ib) => result.extend_from_slice(ib),
                    PyObjectPayload::ByteArray(ib) => result.extend_from_slice(ib),
                    _ => return Err(PyException::type_error("sequence item: expected a bytes-like object")),
                }
            }
            Ok(PyObject::bytes(result))
        }
        "replace" => {
            if args.len() < 2 { return Err(PyException::type_error("replace requires 2 arguments")); }
            if let (PyObjectPayload::Bytes(old) | PyObjectPayload::ByteArray(old),
                    PyObjectPayload::Bytes(new) | PyObjectPayload::ByteArray(new)) = (&args[0].payload, &args[1].payload) {
                let s = String::from_utf8_lossy(b);
                let old_s = String::from_utf8_lossy(old);
                let new_s = String::from_utf8_lossy(new);
                Ok(PyObject::bytes(s.replace(old_s.as_ref(), new_s.as_ref()).into_bytes()))
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "isdigit" => Ok(PyObject::bool_val(!b.is_empty() && b.iter().all(|c| c.is_ascii_digit()))),
        "isalpha" => Ok(PyObject::bool_val(!b.is_empty() && b.iter().all(|c| c.is_ascii_alphabetic()))),
        "isalnum" => Ok(PyObject::bool_val(!b.is_empty() && b.iter().all(|c| c.is_ascii_alphanumeric()))),
        "isspace" => Ok(PyObject::bool_val(!b.is_empty() && b.iter().all(|c| c.is_ascii_whitespace()))),
        "islower" => Ok(PyObject::bool_val(
            b.iter().any(|c| c.is_ascii_lowercase()) &&
            b.iter().all(|c| !c.is_ascii_uppercase())
        )),
        "isupper" => Ok(PyObject::bool_val(
            b.iter().any(|c| c.is_ascii_uppercase()) &&
            b.iter().all(|c| !c.is_ascii_lowercase())
        )),
        "istitle" => {
            let s = String::from_utf8_lossy(b);
            let mut prev_cased = false;
            let mut found_cased = false;
            let mut is_title = true;
            for c in s.chars() {
                if c.is_uppercase() {
                    if prev_cased { is_title = false; break; }
                    prev_cased = true;
                    found_cased = true;
                } else if c.is_lowercase() {
                    if !prev_cased { is_title = false; break; }
                    prev_cased = true;
                    found_cased = true;
                } else {
                    prev_cased = false;
                }
            }
            Ok(PyObject::bool_val(found_cased && is_title))
        }
        "swapcase" => Ok(PyObject::bytes(b.iter().map(|&c| {
            if c.is_ascii_lowercase() { c.to_ascii_uppercase() }
            else if c.is_ascii_uppercase() { c.to_ascii_lowercase() }
            else { c }
        }).collect())),
        "title" => {
            let mut result = Vec::with_capacity(b.len());
            let mut prev_alpha = false;
            for &c in b {
                if c.is_ascii_alphabetic() {
                    if !prev_alpha {
                        result.push(c.to_ascii_uppercase());
                    } else {
                        result.push(c.to_ascii_lowercase());
                    }
                    prev_alpha = true;
                } else {
                    result.push(c);
                    prev_alpha = false;
                }
            }
            Ok(PyObject::bytes(result))
        }
        "capitalize" => {
            if b.is_empty() { return Ok(PyObject::bytes(vec![])); }
            let mut result = vec![b[0].to_ascii_uppercase()];
            result.extend(b[1..].iter().map(|c| c.to_ascii_lowercase()));
            Ok(PyObject::bytes(result))
        }
        "center" => {
            if args.is_empty() { return Err(PyException::type_error("center requires width argument")); }
            let width = args[0].to_int()? as usize;
            let fill = if args.len() > 1 {
                if let PyObjectPayload::Bytes(fb) = &args[1].payload { fb[0] } else { b' ' }
            } else { b' ' };
            if b.len() >= width { return Ok(PyObject::bytes(b.to_vec())); }
            let pad = width - b.len();
            let left = pad / 2;
            let right = pad - left;
            let mut result = vec![fill; left];
            result.extend_from_slice(b);
            result.extend(vec![fill; right]);
            Ok(PyObject::bytes(result))
        }
        "ljust" => {
            if args.is_empty() { return Err(PyException::type_error("ljust requires width argument")); }
            let width = args[0].to_int()? as usize;
            let fill = if args.len() > 1 {
                if let PyObjectPayload::Bytes(fb) = &args[1].payload { fb[0] } else { b' ' }
            } else { b' ' };
            if b.len() >= width { return Ok(PyObject::bytes(b.to_vec())); }
            let mut result = b.to_vec();
            result.extend(vec![fill; width - b.len()]);
            Ok(PyObject::bytes(result))
        }
        "rjust" => {
            if args.is_empty() { return Err(PyException::type_error("rjust requires width argument")); }
            let width = args[0].to_int()? as usize;
            let fill = if args.len() > 1 {
                if let PyObjectPayload::Bytes(fb) = &args[1].payload { fb[0] } else { b' ' }
            } else { b' ' };
            if b.len() >= width { return Ok(PyObject::bytes(b.to_vec())); }
            let mut result = vec![fill; width - b.len()];
            result.extend_from_slice(b);
            Ok(PyObject::bytes(result))
        }
        "lstrip" => {
            let stripped: Vec<u8> = b.iter().copied().skip_while(|c| c.is_ascii_whitespace()).collect();
            Ok(PyObject::bytes(stripped))
        }
        "rstrip" => {
            let mut result = b.to_vec();
            while result.last().map_or(false, |c| c.is_ascii_whitespace()) {
                result.pop();
            }
            Ok(PyObject::bytes(result))
        }
        "rfind" => {
            if args.is_empty() { return Err(PyException::type_error("rfind requires an argument")); }
            if let PyObjectPayload::Bytes(needle) | PyObjectPayload::ByteArray(needle) = &args[0].payload {
                let pos = b.windows(needle.len()).rposition(|w| w == needle.as_slice());
                Ok(PyObject::int(pos.map(|p| p as i64).unwrap_or(-1)))
            } else if let Some(n) = args[0].as_int() {
                let byte = n as u8;
                Ok(PyObject::int(b.iter().rposition(|&x| x == byte).map(|p| p as i64).unwrap_or(-1)))
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "index" => {
            if args.is_empty() { return Err(PyException::type_error("index requires an argument")); }
            if let PyObjectPayload::Bytes(needle) | PyObjectPayload::ByteArray(needle) = &args[0].payload {
                let pos = b.windows(needle.len()).position(|w| w == needle.as_slice());
                match pos {
                    Some(p) => Ok(PyObject::int(p as i64)),
                    None => Err(PyException::value_error("subsection not found")),
                }
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "rindex" => {
            if args.is_empty() { return Err(PyException::type_error("rindex requires an argument")); }
            if let PyObjectPayload::Bytes(needle) | PyObjectPayload::ByteArray(needle) = &args[0].payload {
                let pos = b.windows(needle.len()).rposition(|w| w == needle.as_slice());
                match pos {
                    Some(p) => Ok(PyObject::int(p as i64)),
                    None => Err(PyException::value_error("subsection not found")),
                }
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "zfill" => {
            if args.is_empty() { return Err(PyException::type_error("zfill requires width argument")); }
            let width = args[0].to_int()? as usize;
            if b.len() >= width { return Ok(PyObject::bytes(b.to_vec())); }
            let pad = width - b.len();
            let mut result = vec![b'0'; pad];
            result.extend_from_slice(b);
            Ok(PyObject::bytes(result))
        }
        "expandtabs" => {
            let tabsize = if !args.is_empty() { args[0].to_int()? as usize } else { 8 };
            let mut result = Vec::new();
            let mut col = 0;
            for &byte in b {
                if byte == b'\t' {
                    let spaces = tabsize - (col % tabsize);
                    result.extend(std::iter::repeat(b' ').take(spaces));
                    col += spaces;
                } else if byte == b'\n' || byte == b'\r' {
                    result.push(byte);
                    col = 0;
                } else {
                    result.push(byte);
                    col += 1;
                }
            }
            Ok(PyObject::bytes(result))
        }
        "isascii" => Ok(PyObject::bool_val(b.iter().all(|c| c.is_ascii()))),
        "partition" => {
            if args.is_empty() { return Err(PyException::type_error("partition requires an argument")); }
            if let PyObjectPayload::Bytes(sep) | PyObjectPayload::ByteArray(sep) = &args[0].payload {
                if let Some(pos) = b.windows(sep.len()).position(|w| w == sep.as_slice()) {
                    Ok(PyObject::tuple(vec![
                        PyObject::bytes(b[..pos].to_vec()),
                        PyObject::bytes(sep.clone()),
                        PyObject::bytes(b[pos + sep.len()..].to_vec()),
                    ]))
                } else {
                    Ok(PyObject::tuple(vec![
                        PyObject::bytes(b.to_vec()),
                        PyObject::bytes(vec![]),
                        PyObject::bytes(vec![]),
                    ]))
                }
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "rpartition" => {
            if args.is_empty() { return Err(PyException::type_error("rpartition requires an argument")); }
            if let PyObjectPayload::Bytes(sep) | PyObjectPayload::ByteArray(sep) = &args[0].payload {
                if let Some(pos) = b.windows(sep.len()).rposition(|w| w == sep.as_slice()) {
                    Ok(PyObject::tuple(vec![
                        PyObject::bytes(b[..pos].to_vec()),
                        PyObject::bytes(sep.clone()),
                        PyObject::bytes(b[pos + sep.len()..].to_vec()),
                    ]))
                } else {
                    Ok(PyObject::tuple(vec![
                        PyObject::bytes(vec![]),
                        PyObject::bytes(vec![]),
                        PyObject::bytes(b.to_vec()),
                    ]))
                }
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "removeprefix" => {
            if args.is_empty() { return Err(PyException::type_error("removeprefix requires an argument")); }
            if let PyObjectPayload::Bytes(prefix) | PyObjectPayload::ByteArray(prefix) = &args[0].payload {
                if b.starts_with(prefix) {
                    Ok(PyObject::bytes(b[prefix.len()..].to_vec()))
                } else {
                    Ok(PyObject::bytes(b.to_vec()))
                }
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "removesuffix" => {
            if args.is_empty() { return Err(PyException::type_error("removesuffix requires an argument")); }
            if let PyObjectPayload::Bytes(suffix) | PyObjectPayload::ByteArray(suffix) = &args[0].payload {
                if b.ends_with(suffix) {
                    Ok(PyObject::bytes(b[..b.len() - suffix.len()].to_vec()))
                } else {
                    Ok(PyObject::bytes(b.to_vec()))
                }
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "rsplit" => {
            if args.is_empty() {
                let parts: Vec<PyObjectRef> = String::from_utf8_lossy(b)
                    .split_whitespace()
                    .rev()
                    .map(|s| PyObject::bytes(s.as_bytes().to_vec()))
                    .collect::<Vec<_>>().into_iter().rev().collect();
                Ok(PyObject::list(parts))
            } else if let PyObjectPayload::Bytes(sep) | PyObjectPayload::ByteArray(sep) = &args[0].payload {
                let max_split = if args.len() > 1 { args[1].to_int().unwrap_or(-1) } else { -1 };
                let s = String::from_utf8_lossy(b);
                let sep_s = String::from_utf8_lossy(sep);
                let parts: Vec<&str> = if max_split < 0 {
                    s.rsplitn(usize::MAX, sep_s.as_ref()).collect()
                } else {
                    s.rsplitn(max_split as usize + 1, sep_s.as_ref()).collect()
                };
                let result: Vec<PyObjectRef> = parts.into_iter().rev()
                    .map(|p| PyObject::bytes(p.as_bytes().to_vec()))
                    .collect();
                Ok(PyObject::list(result))
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "splitlines" => {
            let s = String::from_utf8_lossy(b);
            let keep_ends = !args.is_empty() && args[0].is_truthy();
            let parts: Vec<PyObjectRef> = if keep_ends {
                s.split_inclusive('\n').map(|l| PyObject::bytes(l.as_bytes().to_vec())).collect()
            } else {
                s.lines().map(|l| PyObject::bytes(l.as_bytes().to_vec())).collect()
            };
            Ok(PyObject::list(parts))
        }
        _ => Err(PyException::attribute_error(format!(
            "'bytes' object has no attribute '{}'", method
        ))),
    }
}

// Hex encoding helper (avoid external dep)
mod hex {
    pub fn encode(data: &[u8]) -> String {
        data.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

pub(crate) fn partial_cmp_for_sort(a: &PyObjectRef, b: &PyObjectRef) -> Option<std::cmp::Ordering> {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(x), PyObjectPayload::Int(y)) => x.partial_cmp(y),
        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => x.partial_cmp(y),
        (PyObjectPayload::Int(x), PyObjectPayload::Float(y)) => x.to_f64().partial_cmp(y),
        (PyObjectPayload::Float(x), PyObjectPayload::Int(y)) => x.partial_cmp(&y.to_f64()),
        (PyObjectPayload::Str(x), PyObjectPayload::Str(y)) => x.partial_cmp(y),
        (PyObjectPayload::Bool(x), PyObjectPayload::Bool(y)) => x.partial_cmp(y),
        (PyObjectPayload::Tuple(x), PyObjectPayload::Tuple(y)) => {
            for (a_item, b_item) in x.iter().zip(y.iter()) {
                match partial_cmp_for_sort(a_item, b_item) {
                    Some(std::cmp::Ordering::Equal) => continue,
                    other => return other,
                }
            }
            x.len().partial_cmp(&y.len())
        }
        (PyObjectPayload::List(x), PyObjectPayload::List(y)) => {
            let x = x.read(); let y = y.read();
            for (a_item, b_item) in x.iter().zip(y.iter()) {
                match partial_cmp_for_sort(a_item, b_item) {
                    Some(std::cmp::Ordering::Equal) => continue,
                    other => return other,
                }
            }
            x.len().partial_cmp(&y.len())
        }
        // Custom objects — can't call __lt__ from here (no VM), return None
        // so callers fall back to default ordering
        (PyObjectPayload::Instance(_), _) | (_, PyObjectPayload::Instance(_)) => None,
        _ => None,
    }
}

/// Bytearray-specific method dispatch (mutable operations + delegates immutable ones to call_bytes_method).
pub(super) fn call_bytearray_method(receiver: &PyObjectRef, b: &[u8], method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "append" => {
            if args.is_empty() { return Err(PyException::type_error("append() takes exactly one argument")); }
            let byte_val = args[0].to_int()? as u8;
            // Safety: single-threaded access, Vec is owned inside Arc<PyObject>
            unsafe {
                let _ptr = b as *const [u8] as *const Vec<u8>;
                // Go from slice ptr back to Vec ptr (payload stores Vec<u8>)
                let vec_ptr = &receiver.payload as *const PyObjectPayload;
                if let PyObjectPayload::ByteArray(ref v) = *vec_ptr {
                    let vp = v as *const Vec<u8> as *mut Vec<u8>;
                    (*vp).push(byte_val);
                }
            }
            Ok(PyObject::none())
        }
        "extend" => {
            if args.is_empty() { return Err(PyException::type_error("extend() takes exactly one argument")); }
            let new_bytes: Vec<u8> = match &args[0].payload {
                PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => b.clone(),
                PyObjectPayload::List(items) => {
                    items.read().iter().map(|i| i.to_int().unwrap_or(0) as u8).collect()
                }
                _ => args[0].to_list()?.iter().map(|i| i.to_int().unwrap_or(0) as u8).collect(),
            };
            unsafe {
                let vec_ptr = &receiver.payload as *const PyObjectPayload;
                if let PyObjectPayload::ByteArray(ref v) = *vec_ptr {
                    let vp = v as *const Vec<u8> as *mut Vec<u8>;
                    (*vp).extend_from_slice(&new_bytes);
                }
            }
            Ok(PyObject::none())
        }
        "pop" => {
            unsafe {
                let vec_ptr = &receiver.payload as *const PyObjectPayload;
                if let PyObjectPayload::ByteArray(ref v) = *vec_ptr {
                    let vp = v as *const Vec<u8> as *mut Vec<u8>;
                    if let Some(idx) = if args.is_empty() { Some((*vp).len().wrapping_sub(1)) } else { Some(args[0].to_int()? as usize) } {
                        if idx < (*vp).len() {
                            let val = (*vp).remove(idx);
                            return Ok(PyObject::int(val as i64));
                        }
                    }
                    return Err(PyException::index_error("pop index out of range"));
                }
            }
            Err(PyException::index_error("pop from empty bytearray"))
        }
        "insert" => {
            if args.len() < 2 { return Err(PyException::type_error("insert() takes exactly 2 arguments")); }
            let idx = args[0].to_int()?;
            let byte_val = args[1].to_int()? as u8;
            unsafe {
                let vec_ptr = &receiver.payload as *const PyObjectPayload;
                if let PyObjectPayload::ByteArray(ref v) = *vec_ptr {
                    let vp = v as *const Vec<u8> as *mut Vec<u8>;
                    let len = (*vp).len() as i64;
                    let actual = if idx < 0 { (len + idx).max(0) as usize } else { (idx as usize).min((*vp).len()) };
                    (*vp).insert(actual, byte_val);
                }
            }
            Ok(PyObject::none())
        }
        "clear" => {
            unsafe {
                let vec_ptr = &receiver.payload as *const PyObjectPayload;
                if let PyObjectPayload::ByteArray(ref v) = *vec_ptr {
                    let vp = v as *const Vec<u8> as *mut Vec<u8>;
                    (*vp).clear();
                }
            }
            Ok(PyObject::none())
        }
        "reverse" => {
            unsafe {
                let vec_ptr = &receiver.payload as *const PyObjectPayload;
                if let PyObjectPayload::ByteArray(ref v) = *vec_ptr {
                    let vp = v as *const Vec<u8> as *mut Vec<u8>;
                    (*vp).reverse();
                }
            }
            Ok(PyObject::none())
        }
        "copy" => Ok(PyObject::bytearray(b.to_vec())),
        // Delegate immutable methods to bytes
        _ => call_bytes_method(b, method, args),
    }
}

