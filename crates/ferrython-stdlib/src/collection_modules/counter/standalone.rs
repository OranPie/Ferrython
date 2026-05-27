use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;

/// Standalone most_common(counter_dict, n?) - also available as Counter.most_common()
pub(in crate::collection_modules) fn collections_most_common(
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "most_common() requires a Counter argument",
        ));
    }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let r = map.read();
        let mut pairs: Vec<(HashableKey, i64)> = r
            .iter()
            .filter(|(k, _)| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__"))
            .map(|(k, v)| (k.clone(), v.as_int().unwrap_or(0)))
            .collect();
        pairs.sort_by(|a, b| b.1.cmp(&a.1));
        let n = if args.len() > 1 {
            args[1].as_int().unwrap_or(pairs.len() as i64) as usize
        } else {
            pairs.len()
        };
        let result: Vec<PyObjectRef> = pairs
            .into_iter()
            .take(n)
            .map(|(k, v)| PyObject::tuple(vec![k.to_object(), PyObject::int(v)]))
            .collect();
        Ok(PyObject::list(result))
    } else {
        Err(PyException::type_error(
            "most_common() argument must be a Counter",
        ))
    }
}

fn is_counter_internal_key(k: &HashableKey) -> bool {
    matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__")
}

/// counter_elements(counter) -> list of elements repeated by their counts
pub(in crate::collection_modules) fn counter_elements(
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "counter_elements requires a Counter",
        ));
    }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let r = map.read();
        let mut result = Vec::new();
        for (k, v) in r.iter() {
            if is_counter_internal_key(k) {
                continue;
            }
            let count = v.as_int().unwrap_or(0);
            for _ in 0..count {
                result.push(k.to_object());
            }
        }
        Ok(PyObject::list(result))
    } else {
        Err(PyException::type_error(
            "counter_elements requires a Counter",
        ))
    }
}

/// counter_update(counter, iterable_or_dict) -> None (mutates counter in-place)
pub(in crate::collection_modules) fn counter_update(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "counter_update requires counter and data",
        ));
    }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let mut w = map.write();
        if let PyObjectPayload::Dict(other) = &args[1].payload {
            let r = other.read();
            for (k, v) in r.iter() {
                if is_counter_internal_key(k) {
                    continue;
                }
                let existing = w.get(k).and_then(|x| x.as_int()).unwrap_or(0);
                let add = v.as_int().unwrap_or(0);
                w.insert(k.clone(), PyObject::int(existing + add));
            }
        } else {
            let items = args[1].to_list()?;
            for item in &items {
                let key = item.to_hashable_key()?;
                let existing = w.get(&key).and_then(|x| x.as_int()).unwrap_or(0);
                w.insert(key, PyObject::int(existing + 1));
            }
        }
        Ok(PyObject::none())
    } else {
        Err(PyException::type_error(
            "counter_update requires a Counter as first argument",
        ))
    }
}

/// counter_subtract(counter, iterable_or_dict) -> None (mutates counter)
pub(in crate::collection_modules) fn counter_subtract(
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "counter_subtract requires counter and data",
        ));
    }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let mut w = map.write();
        if let PyObjectPayload::Dict(other) = &args[1].payload {
            let r = other.read();
            for (k, v) in r.iter() {
                if is_counter_internal_key(k) {
                    continue;
                }
                let existing = w.get(k).and_then(|x| x.as_int()).unwrap_or(0);
                let sub = v.as_int().unwrap_or(0);
                w.insert(k.clone(), PyObject::int(existing - sub));
            }
        } else {
            let items = args[1].to_list()?;
            for item in &items {
                let key = item.to_hashable_key()?;
                let existing = w.get(&key).and_then(|x| x.as_int()).unwrap_or(0);
                w.insert(key, PyObject::int(existing - 1));
            }
        }
        Ok(PyObject::none())
    } else {
        Err(PyException::type_error(
            "counter_subtract requires a Counter",
        ))
    }
}

/// counter_total(counter) -> int (sum of all counts)
pub(in crate::collection_modules) fn counter_total(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("counter_total requires a Counter"));
    }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let r = map.read();
        let total: i64 = r
            .iter()
            .filter(|(k, _)| !is_counter_internal_key(k))
            .map(|(_, v)| v.as_int().unwrap_or(0))
            .sum();
        Ok(PyObject::int(total))
    } else {
        Err(PyException::type_error("counter_total requires a Counter"))
    }
}

pub(in crate::collection_modules) fn counter_copy(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("counter_copy requires a Counter"));
    }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let r = map.read();
        Ok(PyObject::dict(r.clone()))
    } else {
        Err(PyException::type_error("counter_copy requires a Counter"))
    }
}

pub(in crate::collection_modules) fn counter_clear(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("counter_clear requires a Counter"));
    }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let mut w = map.write();
        let factory = w
            .get(&HashableKey::str_key(CompactString::from(
                "__defaultdict_factory__",
            )))
            .cloned();
        let marker = w
            .get(&HashableKey::str_key(CompactString::from("__counter__")))
            .cloned();
        w.clear();
        if let Some(f) = factory {
            w.insert(
                HashableKey::str_key(CompactString::from("__defaultdict_factory__")),
                f,
            );
        }
        if let Some(m) = marker {
            w.insert(HashableKey::str_key(CompactString::from("__counter__")), m);
        }
    }
    Ok(PyObject::none())
}

/// _count_elements(mapping, iterable) - C accelerator for Counter.__init__
pub(in crate::collection_modules) fn count_elements(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "_count_elements requires 2 arguments",
        ));
    }
    let mapping = &args[0];
    let iterable = &args[1];
    let items = iterable.to_list()?;
    for item in items {
        let key_str = item.py_to_string();
        let key = HashableKey::str_key(CompactString::from(key_str.as_str()));
        if let PyObjectPayload::Dict(map) = &mapping.payload {
            let current = {
                let r = map.read();
                r.get(&key).cloned()
            };
            let new_val = match current {
                Some(v) => {
                    let n = v.to_int().unwrap_or(0) + 1;
                    PyObject::int(n)
                }
                None => PyObject::int(1),
            };
            map.write().insert(key, new_val);
        }
    }
    Ok(PyObject::none())
}
