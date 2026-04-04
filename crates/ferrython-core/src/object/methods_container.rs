//! Container operation methods (len, getitem, contains, iter).

use crate::error::{PyException, PyResult};
use crate::types::HashableKey;
use compact_str::CompactString;
use indexmap::IndexMap;
use std::sync::Arc;

use super::payload::*;
use super::helpers::*;
use super::methods::PyObjectMethods;

pub(super) fn py_len(obj: &PyObjectRef) -> PyResult<usize> {
        match &obj.payload {
            PyObjectPayload::Str(s) => Ok(s.chars().count()),
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Ok(b.len()),
            PyObjectPayload::List(v) => Ok(v.read().len()),
            PyObjectPayload::Tuple(v) => Ok(v.len()),
            PyObjectPayload::Set(m) => Ok(m.read().len()),
            PyObjectPayload::FrozenSet(m) => Ok(m.len()),
            PyObjectPayload::Dict(m) => {
                let map = m.read();
                let mut hidden = 0;
                if map.contains_key(&HashableKey::Str(CompactString::from("__defaultdict_factory__"))) { hidden += 1; }
                if map.contains_key(&HashableKey::Str(CompactString::from("__counter__"))) { hidden += 1; }
                Ok(map.len() - hidden)
            },
            PyObjectPayload::Instance(inst) => {
                if let Some(ref ds) = inst.dict_storage {
                    return Ok(ds.read().len());
                }
                Err(PyException::type_error(format!("object of type '{}' has no len()", obj.type_name())))
            },
            PyObjectPayload::Class(cd) => {
                // Support len() on classes with __len__ (e.g., Enum)
                // Check own namespace and MRO
                let len_fn = {
                    let ns = cd.namespace.read();
                    let mut found = ns.get("__len__").cloned();
                    if found.is_none() {
                        for base in &cd.mro {
                            if let PyObjectPayload::Class(bcd) = &base.payload {
                                let bns = bcd.namespace.read();
                                if let Some(f) = bns.get("__len__") {
                                    found = Some(f.clone());
                                    break;
                                }
                            }
                        }
                    }
                    found
                };
                if let Some(len_method) = len_fn {
                    if let PyObjectPayload::NativeFunction { func, .. } = &len_method.payload {
                        let result = func(&[obj.clone()])?;
                        if let Some(n) = result.as_int() {
                            return Ok(n as usize);
                        }
                    }
                }
                Err(PyException::type_error(format!("object of type '{}' has no len()", obj.type_name())))
            },
            PyObjectPayload::Range { start, stop, step } => {
                if *step > 0 && *start < *stop {
                    Ok(((stop - start + step - 1) / step) as usize)
                } else if *step < 0 && *start > *stop {
                    Ok(((start - stop - step - 1) / (-step)) as usize)
                } else {
                    Ok(0)
                }
            },
            PyObjectPayload::Iterator(iter_data) => {
                let data = iter_data.lock().unwrap();
                match &*data {
                    IteratorData::Range { current, stop, step } => {
                        if *step > 0 && *current < *stop {
                            Ok(((stop - current + step - 1) / step) as usize)
                        } else if *step < 0 && *current > *stop {
                            Ok(((current - stop - step - 1) / (-step)) as usize)
                        } else {
                            Ok(0)
                        }
                    }
                    IteratorData::List { items, index } => Ok(items.len() - index),
                    IteratorData::Tuple { items, index } => Ok(items.len() - index),
                    IteratorData::Str { chars, index } => Ok(chars.len() - index),
                    _ => Err(PyException::type_error("object of type 'iterator' has no len()")),
                }
            }
            PyObjectPayload::DictKeys(m) | PyObjectPayload::DictValues(m) | PyObjectPayload::DictItems(m) => {
                let map = m.read();
                let mut hidden = 0;
                if map.contains_key(&HashableKey::Str(CompactString::from("__defaultdict_factory__"))) { hidden += 1; }
                if map.contains_key(&HashableKey::Str(CompactString::from("__counter__"))) { hidden += 1; }
                Ok(map.len() - hidden)
            },
            _ => Err(PyException::type_error(format!("object of type '{}' has no len()", obj.type_name()))),
        }
}

pub(super) fn py_get_item(obj: &PyObjectRef, key: &PyObjectRef) -> PyResult<PyObjectRef> {
        // Check for slice key first
        if let PyObjectPayload::Slice { start, stop, step } = &key.payload {
            return get_slice_impl(obj, start, stop, step);
        }
        match &obj.payload {
            PyObjectPayload::List(items) => {
                let items = items.read();
                let idx = key.to_int()?;
                let len = items.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len { return Err(PyException::index_error("index out of range")); }
                Ok(items[actual as usize].clone())
            }
            PyObjectPayload::Tuple(items) => {
                let idx = key.to_int()?;
                let len = items.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len { return Err(PyException::index_error("index out of range")); }
                Ok(items[actual as usize].clone())
            }
            PyObjectPayload::Dict(map) => {
                let hk = key.to_hashable_key()?;
                let map_r = map.read();
                if let Some(val) = map_r.get(&hk) {
                    return Ok(val.clone());
                }
                // Check for __defaultdict_factory__ (Counter / defaultdict)
                let factory_key = HashableKey::Str(CompactString::from("__defaultdict_factory__"));
                if let Some(factory) = map_r.get(&factory_key) {
                    let factory = factory.clone();
                    drop(map_r);
                    // Create default value by "calling" the factory
                    // For common factories: int -> 0, list -> [], str -> "", float -> 0.0
                    let default = match &factory.payload {
                        PyObjectPayload::BuiltinType(name) => {
                            match name.as_str() {
                                "int" => PyObject::int(0),
                                "float" => PyObject::float(0.0),
                                "str" => PyObject::str_val(CompactString::new("")),
                                "list" => PyObject::list(vec![]),
                                "bool" => PyObject::bool_val(false),
                                "tuple" => PyObject::tuple(vec![]),
                                "set" => PyObject::set(IndexMap::new()),
                                "dict" => PyObject::dict(IndexMap::new()),
                                _ => return Err(PyException::key_error(key.repr())),
                            }
                        }
                        _ => return Err(PyException::key_error(key.repr())),
                    };
                    // Store the default value
                    map.write().insert(hk, default.clone());
                    return Ok(default);
                }
                Err(PyException::key_error(key.repr()))
            }
            PyObjectPayload::Str(s) => {
                let idx = key.to_int()?;
                let chars: Vec<char> = s.chars().collect();
                let len = chars.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len { return Err(PyException::index_error("string index out of range")); }
                Ok(PyObject::str_val(CompactString::from(chars[actual as usize].to_string())))
            }
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                let idx = key.to_int()?;
                let len = b.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len { return Err(PyException::index_error("index out of range")); }
                Ok(PyObject::int(b[actual as usize] as i64))
            }
            PyObjectPayload::InstanceDict(attrs) => {
                let key_str = key.py_to_string();
                let attrs_r = attrs.read();
                if let Some(val) = attrs_r.get(key_str.as_str()) {
                    Ok(val.clone())
                } else {
                    Err(PyException::key_error(key.repr()))
                }
            }
            PyObjectPayload::Range { start, stop, step } => {
                let idx = key.to_int()?;
                let len = if *step > 0 && *start < *stop {
                    (stop - start + step - 1) / step
                } else if *step < 0 && *start > *stop {
                    (start - stop - step - 1) / (-step)
                } else { 0 };
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len { return Err(PyException::index_error("range object index out of range")); }
                Ok(PyObject::int(start + actual * step))
            }
            _ => Err(PyException::type_error(format!("'{}' object is not subscriptable", obj.type_name()))),
        }
}

pub(super) fn py_contains(obj: &PyObjectRef, item: &PyObjectRef) -> PyResult<bool> {
        match &obj.payload {
            PyObjectPayload::List(v) => {
                let v = v.read();
                Ok(v.iter().any(|x| partial_cmp_objects(x, item) == Some(std::cmp::Ordering::Equal)))
            }
            PyObjectPayload::Tuple(v) => {
                Ok(v.iter().any(|x| partial_cmp_objects(x, item) == Some(std::cmp::Ordering::Equal)))
            }
            PyObjectPayload::Str(haystack) => {
                if let Some(needle) = item.as_str() { Ok(haystack.contains(needle)) }
                else { Err(PyException::type_error("'in <string>' requires string as left operand")) }
            }
            PyObjectPayload::Set(m) => {
                let hk = item.to_hashable_key()?;
                Ok(m.read().contains_key(&hk))
            }
            PyObjectPayload::FrozenSet(m) => {
                let hk = item.to_hashable_key()?;
                Ok(m.contains_key(&hk))
            }
            PyObjectPayload::Dict(m) => {
                let hk = item.to_hashable_key()?;
                Ok(m.read().contains_key(&hk))
            }
            PyObjectPayload::Instance(inst) if inst.dict_storage.is_some() => {
                let hk = item.to_hashable_key()?;
                Ok(inst.dict_storage.as_ref().unwrap().read().contains_key(&hk))
            }
            PyObjectPayload::InstanceDict(attrs) => {
                let key_str = item.py_to_string();
                Ok(attrs.read().contains_key(key_str.as_str()))
            }
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                // Support: int in bytes (single byte) or bytes in bytes (subsequence)
                match &item.payload {
                    PyObjectPayload::Int(n) => {
                        let val = n.to_i64().unwrap_or(-1);
                        if val < 0 || val > 255 { return Ok(false); }
                        Ok(b.contains(&(val as u8)))
                    }
                    PyObjectPayload::Bytes(needle) | PyObjectPayload::ByteArray(needle) => {
                        if needle.is_empty() { return Ok(true); }
                        Ok(b.windows(needle.len()).any(|w| w == needle.as_slice()))
                    }
                    _ => Err(PyException::type_error("a bytes-like object is required")),
                }
            }
            PyObjectPayload::Range { start, stop, step } => {
                if let Some(val) = item.as_int() {
                    if *step > 0 {
                        Ok(val >= *start && val < *stop && (val - start) % step == 0)
                    } else {
                        Ok(val <= *start && val > *stop && (start - val) % (-step) == 0)
                    }
                } else {
                    Ok(false)
                }
            }
            PyObjectPayload::Iterator(iter_data) => {
                let data = iter_data.lock().unwrap();
                match &*data {
                    IteratorData::Range { current, stop, step } => {
                        if let Some(val) = item.as_int() {
                            if *step > 0 {
                                Ok(val >= *current && val < *stop && (val - current) % step == 0)
                            } else {
                                Ok(val <= *current && val > *stop && (current - val) % (-step) == 0)
                            }
                        } else {
                            Ok(false)
                        }
                    }
                    _ => {
                        drop(data);
                        let items = obj.to_list()?;
                        Ok(items.iter().any(|x| partial_cmp_objects(x, item) == Some(std::cmp::Ordering::Equal)))
                    }
                }
            }
            PyObjectPayload::DictKeys(m) => {
                let hk = item.to_hashable_key()?;
                Ok(m.read().contains_key(&hk))
            }
            PyObjectPayload::DictValues(m) => {
                let r = m.read();
                Ok(r.values().any(|v| partial_cmp_objects(v, item) == Some(std::cmp::Ordering::Equal)))
            }
            PyObjectPayload::DictItems(m) => {
                // item should be a (key, value) tuple
                if let PyObjectPayload::Tuple(pair) = &item.payload {
                    if pair.len() == 2 {
                        let hk = pair[0].to_hashable_key()?;
                        let r = m.read();
                        if let Some(val) = r.get(&hk) {
                            return Ok(partial_cmp_objects(val, &pair[1]) == Some(std::cmp::Ordering::Equal));
                        }
                    }
                }
                Ok(false)
            }
            _ => Err(PyException::type_error(format!("argument of type '{}' is not iterable", obj.type_name()))),
        }
}

pub(super) fn py_get_iter(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
        use std::sync::Mutex;
        match &obj.payload {
            PyObjectPayload::List(items) => Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items: items.read().clone(), index: 0 }))))),
            PyObjectPayload::Tuple(items) => Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::Tuple { items: items.clone(), index: 0 }))))),
            PyObjectPayload::Str(s) => Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::Str { chars: s.chars().collect(), index: 0 }))))),
            PyObjectPayload::Dict(m) => {
                let keys: Vec<PyObjectRef> = m.read().keys()
                    .filter(|k| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__"))
                    .map(|k| k.to_object()).collect();
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items: keys, index: 0 })))))
            }
            PyObjectPayload::Instance(inst) if inst.dict_storage.is_some() => {
                let keys: Vec<PyObjectRef> = inst.dict_storage.as_ref().unwrap().read().keys().map(|k| k.to_object()).collect();
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items: keys, index: 0 })))))
            }
            PyObjectPayload::Set(m) => {
                let vals: Vec<PyObjectRef> = m.read().values().cloned().collect();
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items: vals, index: 0 })))))
            }
            PyObjectPayload::FrozenSet(m) => {
                let vals: Vec<PyObjectRef> = m.values().cloned().collect();
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items: vals, index: 0 })))))
            }
            PyObjectPayload::Range { start, stop, step } => {
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::Range { current: *start, stop: *stop, step: *step })))))
            }
            PyObjectPayload::Iterator(_) => Ok(obj.clone()),
            PyObjectPayload::Generator(_) => Ok(obj.clone()), // generators are their own iterators
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                let items: Vec<PyObjectRef> = b.iter().map(|byte| PyObject::int(*byte as i64)).collect();
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items, index: 0 })))))
            }
            // namedtuple instances: iterate over the underlying _tuple
            PyObjectPayload::Instance(inst) if inst.class.get_attr("__namedtuple__").is_some() => {
                if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                    if let PyObjectPayload::Tuple(items) = &tup.payload {
                        return Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::Tuple { items: items.clone(), index: 0 })))));
                    }
                }
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::Tuple { items: vec![], index: 0 })))))
            }
            PyObjectPayload::DictKeys(m) => {
                let keys: Vec<PyObjectRef> = m.read().keys()
                    .filter(|k| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__"))
                    .map(|k| k.to_object()).collect();
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items: keys, index: 0 })))))
            }
            PyObjectPayload::DictValues(m) => {
                let vals: Vec<PyObjectRef> = m.read().iter()
                    .filter(|(k, _)| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__"))
                    .map(|(_, v)| v.clone()).collect();
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items: vals, index: 0 })))))
            }
            PyObjectPayload::DictItems(m) => {
                let items: Vec<PyObjectRef> = m.read().iter()
                    .filter(|(k, _)| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__"))
                    .map(|(k, v)| PyObject::tuple(vec![k.to_object(), v.clone()])).collect();
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items, index: 0 })))))
            }
            _ => Err(PyException::type_error(format!("'{}' object is not iterable", obj.type_name()))),
        }
}
