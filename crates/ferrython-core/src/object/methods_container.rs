//! Container operation methods (len, getitem, contains, iter).

use crate::error::{PyException, PyResult};
use crate::intern::intern_or_new;
use crate::types::HashableKey;
use compact_str::CompactString;
use std::rc::Rc;

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
            PyObjectPayload::Dict(m) | PyObjectPayload::MappingProxy(m) => {
                let map = m.read();
                let hidden = map.keys().filter(|k| is_hidden_dict_key(k)).count();
                Ok(map.len() - hidden)
            },
            PyObjectPayload::Instance(inst) => {
                if let Some(ref ds) = inst.dict_storage {
                    return Ok(ds.read().len());
                }
                // Builtin base type subclass: delegate to __builtin_value__
                if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                    return py_len(&bv);
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
                    if let PyObjectPayload::NativeFunction(nf) = &len_method.payload {
                        let result = (nf.func)(&[obj.clone()])?;
                        if let Some(n) = result.as_int() {
                            return Ok(n as usize);
                        }
                    }
                }
                Err(PyException::type_error(format!("object of type '{}' has no len()", obj.type_name())))
            },
            PyObjectPayload::Range(rd) => {
                if rd.step > 0 && rd.start < rd.stop {
                    Ok(((rd.stop - rd.start + rd.step - 1) / rd.step) as usize)
                } else if rd.step < 0 && rd.start > rd.stop {
                    Ok(((rd.start - rd.stop - rd.step - 1) / (-rd.step)) as usize)
                } else {
                    Ok(0)
                }
            },
            PyObjectPayload::Iterator(iter_data) => {
                let data = iter_data.read();
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
            PyObjectPayload::RangeIter(ri) => {
                let cur = ri.current.get();
                if ri.step > 0 && cur < ri.stop {
                    Ok(((ri.stop - cur + ri.step - 1) / ri.step) as usize)
                } else if ri.step < 0 && cur > ri.stop {
                    Ok(((cur - ri.stop - ri.step - 1) / (-ri.step)) as usize)
                } else {
                    Ok(0)
                }
            }
            PyObjectPayload::VecIter(data) => {
                let idx = data.index.get();
                Ok(if idx < data.items.len() { data.items.len() - idx } else { 0 })
            }
            PyObjectPayload::RefIter { source, index } => {
                let idx = index.get();
                let total = match &source.payload {
                    PyObjectPayload::List(cell) => unsafe { &*cell.data_ptr() }.len(),
                    PyObjectPayload::Tuple(items) => items.len(),
                    _ => 0,
                };
                Ok(if idx < total { total - idx } else { 0 })
            }
            PyObjectPayload::DictKeys(m) | PyObjectPayload::DictValues(m) | PyObjectPayload::DictItems(m) => {
                let map = m.read();
                let hidden = map.keys().filter(|k| is_hidden_dict_key(k)).count();
                Ok(map.len() - hidden)
            },
            PyObjectPayload::InstanceDict(attrs) => Ok(attrs.read().len()),
            _ => Err(PyException::type_error(format!("object of type '{}' has no len()", obj.type_name()))),
        }
}

pub(super) fn py_get_item(obj: &PyObjectRef, key: &PyObjectRef) -> PyResult<PyObjectRef> {
        // Check for slice key first
        if let PyObjectPayload::Slice(sd) = &key.payload {
            return get_slice_impl(obj, &sd.start, &sd.stop, &sd.step);
        }
        match &obj.payload {
            PyObjectPayload::List(items) => {
                let items = items.read();
                let idx = key.to_int()?;
                let len = items.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len { return Err(PyException::index_error("list index out of range")); }
                Ok(items[actual as usize].clone())
            }
            PyObjectPayload::Tuple(items) => {
                let idx = key.to_int()?;
                let len = items.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len { return Err(PyException::index_error("tuple index out of range")); }
                Ok(items[actual as usize].clone())
            }
            PyObjectPayload::Dict(map) => {
                let hk = key.to_hashable_key()?;
                if is_hidden_dict_key(&hk) {
                    return Err(PyException::key_error(key.repr()));
                }
                let map_r = map.read();
                if let Some(val) = map_r.get(&hk) {
                    return Ok(val.clone());
                }
                // Check for __defaultdict_factory__ (Counter / defaultdict)
                let factory_key = HashableKey::str_key(intern_or_new("__defaultdict_factory__"));
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
                                "set" => PyObject::set(new_fx_hashkey_map()),
                                "dict" => PyObject::dict(new_fx_hashkey_map()),
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
            PyObjectPayload::MappingProxy(map) => {
                let hk = key.to_hashable_key()?;
                if let Some(val) = map.read().get(&hk) {
                    return Ok(val.clone());
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
            PyObjectPayload::Range(rd) => {
                let idx = key.to_int()?;
                let len = if rd.step > 0 && rd.start < rd.stop {
                    (rd.stop - rd.start + rd.step - 1) / rd.step
                } else if rd.step < 0 && rd.start > rd.stop {
                    (rd.start - rd.stop - rd.step - 1) / (-rd.step)
                } else { 0 };
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len { return Err(PyException::index_error("range object index out of range")); }
                Ok(PyObject::int(rd.start + actual * rd.step))
            }
            _ => {
                // Builtin base type subclass: delegate to __builtin_value__
                if let PyObjectPayload::Instance(inst) = &obj.payload {
                    if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                        return py_get_item(&bv, key);
                    }
                }
                Err(PyException::type_error(format!("'{}' object is not subscriptable", obj.type_name())))
            }
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
            PyObjectPayload::Dict(m) | PyObjectPayload::MappingProxy(m) => {
                let hk = item.to_hashable_key()?;
                if is_hidden_dict_key(&hk) { return Ok(false); }
                Ok(m.read().contains_key(&hk))
            }
            PyObjectPayload::Instance(inst) if inst.dict_storage.is_some() => {
                let hk = item.to_hashable_key()?;
                if let Some(storage) = inst.dict_storage.as_ref() {
                    Ok(storage.read().contains_key(&hk))
                } else {
                    Ok(false)
                }
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
            PyObjectPayload::Range(rd) => {
                if let Some(val) = item.as_int() {
                    if rd.step > 0 {
                        Ok(val >= rd.start && val < rd.stop && (val - rd.start) % rd.step == 0)
                    } else {
                        Ok(val <= rd.start && val > rd.stop && (rd.start - val) % (-rd.step) == 0)
                    }
                } else {
                    Ok(false)
                }
            }
            PyObjectPayload::Iterator(iter_data) => {
                let data = iter_data.read();
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
            PyObjectPayload::RangeIter(ri) => {
                if let Some(val) = item.as_int() {
                    let cur = ri.current.get();
                    if ri.step > 0 {
                        Ok(val >= cur && val < ri.stop && (val - cur) % ri.step == 0)
                    } else {
                        Ok(val <= cur && val > ri.stop && (cur - val) % (-ri.step) == 0)
                    }
                } else {
                    Ok(false)
                }
            }
            PyObjectPayload::VecIter(data) => {
                let idx = data.index.get();
                if idx >= data.items.len() { return Ok(false); }
                Ok(data.items[idx..].iter().any(|x| partial_cmp_objects(x, item) == Some(std::cmp::Ordering::Equal)))
            }
            PyObjectPayload::RefIter { source, index } => {
                let idx = index.get();
                match &source.payload {
                    PyObjectPayload::List(cell) => {
                        let items = unsafe { &*cell.data_ptr() };
                        if idx >= items.len() { return Ok(false); }
                        Ok(items[idx..].iter().any(|x| partial_cmp_objects(x, item) == Some(std::cmp::Ordering::Equal)))
                    }
                    PyObjectPayload::Tuple(items) => {
                        if idx >= items.len() { return Ok(false); }
                        Ok(items[idx..].iter().any(|x| partial_cmp_objects(x, item) == Some(std::cmp::Ordering::Equal)))
                    }
                    _ => Ok(false),
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
        
        match &obj.payload {
            PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) => Ok(PyObject::wrap(PyObjectPayload::RefIter { source: obj.clone(), index: SyncUsize::new(0) })),
            PyObjectPayload::Str(s) => Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(PyCell::new(IteratorData::Str { chars: s.chars().collect(), index: 0 }))))),
            PyObjectPayload::Dict(_) | PyObjectPayload::MappingProxy(_) => {
                Ok(PyObject::wrap(PyObjectPayload::RefIter { source: obj.clone(), index: SyncUsize::new(0) }))
            }
            PyObjectPayload::Instance(inst) if inst.dict_storage.is_some() => {
                if let Some(storage) = inst.dict_storage.as_ref() {
                    let keys: Vec<PyObjectRef> = storage.read().keys().map(|k| k.to_object()).collect();
                    Ok(PyObject::wrap(PyObjectPayload::VecIter(Box::new(VecIterData { items: keys, index: SyncUsize::new(0) }))))
                } else {
                    Err(PyException::type_error(format!("'{}' object is not iterable", obj.type_name())))
                }
            }
            PyObjectPayload::Set(m) => {
                let vals: Vec<PyObjectRef> = m.read().values().cloned().collect();
                Ok(PyObject::wrap(PyObjectPayload::VecIter(Box::new(VecIterData { items: vals, index: SyncUsize::new(0) }))))
            }
            PyObjectPayload::FrozenSet(m) => {
                let vals: Vec<PyObjectRef> = m.values().cloned().collect();
                Ok(PyObject::wrap(PyObjectPayload::VecIter(Box::new(VecIterData { items: vals, index: SyncUsize::new(0) }))))
            }
            PyObjectPayload::Range(rd) => {
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(PyCell::new(IteratorData::Range { current: rd.start, stop: rd.stop, step: rd.step })))))
            }
            PyObjectPayload::Iterator(_) | PyObjectPayload::RangeIter(..) | PyObjectPayload::VecIter(_) | PyObjectPayload::RefIter { .. } => Ok(obj.clone()),
            PyObjectPayload::Generator(_) => Ok(obj.clone()), // generators are their own iterators
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                let items: Vec<PyObjectRef> = b.iter().map(|byte| PyObject::int(*byte as i64)).collect();
                Ok(PyObject::wrap(PyObjectPayload::VecIter(Box::new(VecIterData { items, index: SyncUsize::new(0) }))))
            }
            // namedtuple instances: iterate over the underlying _tuple
            PyObjectPayload::Instance(inst) if inst.class.get_attr("__namedtuple__").is_some() => {
                if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                    if let PyObjectPayload::Tuple(items) = &tup.payload {
                        return Ok(PyObject::wrap(PyObjectPayload::VecIter(Box::new(VecIterData { items: (**items).clone(), index: SyncUsize::new(0) }))));
                    }
                }
                Ok(PyObject::wrap(PyObjectPayload::VecIter(Box::new(VecIterData { items: vec![], index: SyncUsize::new(0) }))))
            }
            // Builtin base type subclass: delegate to __builtin_value__
            PyObjectPayload::Instance(inst) if inst.attrs.read().contains_key("__builtin_value__") => {
                let bv = inst.attrs.read().get("__builtin_value__").cloned().unwrap();
                py_get_iter(&bv)
            }
            PyObjectPayload::DictKeys(_) => {
                Ok(PyObject::wrap(PyObjectPayload::RefIter { source: obj.clone(), index: SyncUsize::new(0) }))
            }
            PyObjectPayload::DictValues(m) => {
                let vals: Vec<PyObjectRef> = m.read().iter()
                    .filter(|(k, _)| !is_hidden_dict_key(k))
                    .map(|(_, v)| v.clone()).collect();
                Ok(PyObject::wrap(PyObjectPayload::VecIter(Box::new(VecIterData { items: vals, index: SyncUsize::new(0) }))))
            }
            PyObjectPayload::DictItems(m) => {
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(PyCell::new(
                    IteratorData::DictEntries { source: m.clone(), index: 0, cached_tuple: None }
                )))))
            }
            _ => Err(PyException::type_error(format!("'{}' object is not iterable", obj.type_name()))),
        }
}
