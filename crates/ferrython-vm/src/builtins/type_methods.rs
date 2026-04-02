//! Collection/numeric type method dispatch (list, dict, set, tuple, int, float, bytes)

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args_min,
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

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
            for (i, x) in items.read().iter().enumerate() {
                if x.py_to_string() == target.py_to_string() {
                    return Ok(PyObject::int(i as i64));
                }
            }
            Err(PyException::value_error("x not in list"))
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
                .filter(|k| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__"))
                .map(|k| k.to_object()).collect();
            Ok(PyObject::list(keys))
        }
        "values" => {
            let r = map.read();
            let vals: Vec<PyObjectRef> = r.iter()
                .filter(|(k, _)| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__"))
                .map(|(_, v)| v.clone()).collect();
            Ok(PyObject::list(vals))
        }
        "items" => {
            let pairs: Vec<PyObjectRef> = map.read().iter()
                .filter(|(k, _)| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__"))
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
            if let PyObjectPayload::Dict(other) = &args[0].payload {
                let other_items = other.read().clone();
                let mut w = map.write();
                for (k, v) in other_items {
                    w.insert(k, v);
                }
            }
            Ok(PyObject::none())
        }
        "pop" => {
            check_args_min("pop", args, 1)?;
            let key = args[0].to_hashable_key()?;
            let default = if args.len() >= 2 { Some(args[1].clone()) } else { None };
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
            let default = if args.len() >= 2 { args[1].clone() } else { PyObject::none() };
            let mut w = map.write();
            Ok(w.entry(key).or_insert(default).clone())
        }
        "clear" => {
            map.write().clear();
            Ok(PyObject::none())
        }
        "popitem" => {
            match map.write().pop() {
                Some((k, v)) => Ok(PyObject::tuple(vec![k.to_object(), v])),
                None => Err(PyException::key_error("popitem(): dictionary is empty")),
            }
        }
        "most_common" => {
            // Counter.most_common(n) — return n most common (key, count) pairs sorted by count
            let r = map.read();
            let mut pairs: Vec<(HashableKey, i64)> = r.iter()
                .filter(|(k, _)| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__"))
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
                if matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__") { continue; }
                let count = v.as_int().unwrap_or(0);
                for _ in 0..count {
                    result.push(k.to_object());
                }
            }
            Ok(PyObject::list(result))
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
            let items = args[0].to_list()?;
            let mut guard = m.write();
            for item in items {
                if let Ok(hk) = item.to_hashable_key() {
                    guard.insert(hk, item);
                }
            }
            Ok(PyObject::none())
        }
        _ => Err(PyException::attribute_error(format!(
            "'set' object has no attribute '{}'", method
        ))),
    }
}

pub(super) fn call_int_method(_receiver: &PyObjectRef, method: &str, _args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "bit_length" => {
            let n = _receiver.to_int()?;
            Ok(PyObject::int(if n == 0 { 0 } else { 64 - n.abs().leading_zeros() as i64 }))
        }
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
        _ => Err(PyException::attribute_error(format!(
            "'float' object has no attribute '{}'", method
        ))),
    }
}

pub(super) fn call_bytes_method(b: &[u8], method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "decode" => {
            // Simple UTF-8 decode
            let s = String::from_utf8_lossy(b);
            Ok(PyObject::str_val(CompactString::from(s)))
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
            // TODO: would need VM-level collect_iterable; simple list case for now
            if let PyObjectPayload::List(items) = &args[0].payload {
                let items = items.read();
                let mut result = Vec::new();
                for (i, item) in items.iter().enumerate() {
                    if i > 0 { result.extend_from_slice(b); }
                    if let PyObjectPayload::Bytes(ib) | PyObjectPayload::ByteArray(ib) = &item.payload {
                        result.extend_from_slice(ib);
                    } else {
                        return Err(PyException::type_error("sequence item: expected a bytes-like object"));
                    }
                }
                Ok(PyObject::bytes(result))
            } else {
                Err(PyException::type_error("can only join an iterable"))
            }
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

