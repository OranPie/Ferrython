use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    call_callable, is_hidden_dict_key, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    WeakKeyIterKind, WeakValueIterKind,
};

use super::core_fns::get_iter_from_obj;

// ── Iterator helpers (used by VM for FOR_ITER) ──

/// Advance an iterator by one step. Returns (new_iterator, value) or None if exhausted.
pub fn iter_advance(iter_obj: &PyObjectRef) -> PyResult<Option<(PyObjectRef, PyObjectRef)>> {
    match &iter_obj.payload {
        PyObjectPayload::Iterator(iter_data) => {
            use ferrython_core::object::IteratorData;
            let mut data = iter_data.write();
            match &mut *data {
                IteratorData::List { items, index } => {
                    if *index < items.len() {
                        let v = items[*index].clone();
                        *index += 1;
                        Ok(Some((iter_obj.clone(), v)))
                    } else {
                        Ok(None)
                    }
                }
                IteratorData::Tuple { items, index } => {
                    if *index < items.len() {
                        let v = items[*index].clone();
                        *index += 1;
                        Ok(Some((iter_obj.clone(), v)))
                    } else {
                        Ok(None)
                    }
                }
                IteratorData::Range {
                    current,
                    stop,
                    step,
                } => {
                    let done = if *step > 0 {
                        *current >= *stop
                    } else {
                        *current <= *stop
                    };
                    if done {
                        Ok(None)
                    } else {
                        let v = PyObject::int(*current);
                        *current += *step;
                        Ok(Some((iter_obj.clone(), v)))
                    }
                }
                IteratorData::Str { chars, index } => {
                    if *index < chars.len() {
                        let v = PyObject::str_val(CompactString::from(chars[*index].to_string()));
                        *index += 1;
                        Ok(Some((iter_obj.clone(), v)))
                    } else {
                        Ok(None)
                    }
                }
                IteratorData::DictEntries {
                    source,
                    owner: _,
                    index,
                    cached_tuple,
                } => {
                    let map = unsafe { &*source.data_ptr() };
                    while *index < map.len() {
                        let (hk, _) = map.get_index(*index).unwrap();
                        if !is_hidden_dict_key(hk) {
                            break;
                        }
                        *index += 1;
                    }
                    if *index < map.len() {
                        let (hk, v) = map.get_index(*index).unwrap();
                        let k = hk.to_object();
                        let v = v.clone();
                        *index += 1;
                        let tuple = if let Some(ref ct) = cached_tuple {
                            if PyObjectRef::strong_count(ct) == 1 {
                                unsafe {
                                    let obj_ptr = PyObjectRef::as_ptr(ct) as *mut PyObject;
                                    if let PyObjectPayload::Tuple(ref mut items) =
                                        (*obj_ptr).payload
                                    {
                                        items[0] = k;
                                        items[1] = v;
                                    }
                                }
                                ct.clone()
                            } else {
                                let t = PyObject::tuple(vec![k, v]);
                                *cached_tuple = Some(t.clone());
                                t
                            }
                        } else {
                            let t = PyObject::tuple(vec![k, v]);
                            *cached_tuple = Some(t.clone());
                            t
                        };
                        Ok(Some((iter_obj.clone(), tuple)))
                    } else {
                        Ok(None)
                    }
                }
                IteratorData::DictKeys { keys, index } => {
                    if *index < keys.len() {
                        let obj = keys[*index].clone();
                        *index += 1;
                        Ok(Some((iter_obj.clone(), obj)))
                    } else {
                        Ok(None)
                    }
                }
                IteratorData::Count { current, step } => {
                    let v = PyObject::int(*current);
                    *current += *step;
                    Ok(Some((iter_obj.clone(), v)))
                }
                IteratorData::Repeat { item, remaining } => {
                    if let Some(ref mut rem) = remaining {
                        if *rem == 0 {
                            Ok(None)
                        } else {
                            *rem -= 1;
                            Ok(Some((iter_obj.clone(), item.clone())))
                        }
                    } else {
                        Ok(Some((iter_obj.clone(), item.clone())))
                    }
                }
                IteratorData::Cycle { items, index } => {
                    if items.is_empty() {
                        Ok(None)
                    } else {
                        let v = items[*index].clone();
                        *index = (*index + 1) % items.len();
                        Ok(Some((iter_obj.clone(), v)))
                    }
                }
                // Lazy iterators that truly need VM context (call user functions)
                IteratorData::Enumerate { .. }
                | IteratorData::Zip { .. }
                | IteratorData::MapOne { .. }
                | IteratorData::Map { .. }
                | IteratorData::Filter { .. }
                | IteratorData::FilterFalse { .. }
                | IteratorData::Sentinel { .. }
                | IteratorData::TakeWhile { .. }
                | IteratorData::DropWhile { .. }
                | IteratorData::Chain { .. }
                | IteratorData::SeqIter { .. }
                | IteratorData::Starmap { .. }
                | IteratorData::Tee { .. }
                | IteratorData::HeldIter { .. } => Err(PyException::type_error(
                    "lazy iterator requires VM-level iteration",
                )),
            }
        }
        PyObjectPayload::RangeIter(ri) => {
            let cur = ri.current.get();
            let done = if ri.step > 0 {
                cur >= ri.stop
            } else {
                cur <= ri.stop
            };
            if done {
                Ok(None)
            } else {
                let v = PyObject::int(cur);
                ri.current.set(cur + ri.step);
                Ok(Some((iter_obj.clone(), v)))
            }
        }
        PyObjectPayload::VecIter(data) => {
            let idx = data.index.get();
            if idx < data.items.len() {
                let v = data.items[idx].clone();
                let new_idx = idx + 1;
                data.index.set(if new_idx >= data.items.len() {
                    usize::MAX
                } else {
                    new_idx
                });
                Ok(Some((iter_obj.clone(), v)))
            } else {
                Ok(None)
            }
        }
        PyObjectPayload::RefIter { source, index } => {
            if index.get() == usize::MAX {
                return Ok(None);
            }
            let idx = index.get();
            match &source.payload {
                PyObjectPayload::List(cell) => {
                    let items = unsafe { &*cell.data_ptr() };
                    if idx < items.len() {
                        let v = items[idx].clone();
                        index.set(idx + 1);
                        Ok(Some((iter_obj.clone(), v)))
                    } else {
                        index.set(usize::MAX);
                        Ok(None)
                    }
                }
                PyObjectPayload::Tuple(items) => {
                    if idx < items.len() {
                        let v = items[idx].clone();
                        index.set(idx + 1);
                        Ok(Some((iter_obj.clone(), v)))
                    } else {
                        index.set(usize::MAX);
                        Ok(None)
                    }
                }
                PyObjectPayload::Dict(cell)
                | PyObjectPayload::MappingProxy(cell)
                | PyObjectPayload::DictKeys { map: cell, .. } => {
                    let map = unsafe { &*cell.data_ptr() };
                    if idx < map.len() {
                        let v = map.get_index(idx).unwrap().0.to_object();
                        index.set(idx + 1);
                        Ok(Some((iter_obj.clone(), v)))
                    } else {
                        index.set(usize::MAX);
                        Ok(None)
                    }
                }
                _ => Ok(None),
            }
        }
        PyObjectPayload::RevRefIter { source, index } => {
            let idx = index.get();
            if idx == usize::MAX {
                return Ok(None);
            }
            if idx == 0 {
                index.set(usize::MAX);
                return Ok(None);
            }
            match &source.payload {
                PyObjectPayload::List(cell) => {
                    let pos = idx - 1;
                    let items = unsafe { &*cell.data_ptr() };
                    if pos < items.len() {
                        let v = items[pos].clone();
                        index.set(pos);
                        Ok(Some((iter_obj.clone(), v)))
                    } else {
                        index.set(usize::MAX);
                        Ok(None)
                    }
                }
                _ => Ok(None),
            }
        }
        PyObjectPayload::Module(_) => {
            // File-like objects and other "module-backed" iterators with __next__.
            if let Some(next_fn) = iter_obj.get_attr("__next__") {
                match &next_fn.payload {
                    PyObjectPayload::NativeFunction(nf) => match (nf.func)(&[iter_obj.clone()]) {
                        Ok(v) => Ok(Some((iter_obj.clone(), v))),
                        Err(e) if e.kind == ExceptionKind::StopIteration => Ok(None),
                        Err(e) => Err(e),
                    },
                    PyObjectPayload::NativeClosure(nc) => match (nc.func)(&[iter_obj.clone()]) {
                        Ok(v) => Ok(Some((iter_obj.clone(), v))),
                        Err(e) if e.kind == ExceptionKind::StopIteration => Ok(None),
                        Err(e) => Err(e),
                    },
                    _ => Err(PyException::type_error(
                        "module __next__ is not callable from iter_advance",
                    )),
                }
            } else {
                Err(PyException::type_error(format!(
                    "'{}' object is not an iterator",
                    iter_obj.type_name()
                )))
            }
        }
        _ => Err(PyException::type_error("iter_advance on non-iterator")),
    }
}

/// Advance an in-place iterator, returning only the next value.
/// Avoids cloning the iterator itself (used in ForIter hot path).
pub fn iter_next_value(iter_obj: &PyObjectRef) -> PyResult<Option<PyObjectRef>> {
    match &iter_obj.payload {
        PyObjectPayload::Iterator(iter_data) => {
            use ferrython_core::object::IteratorData;
            let mut data = iter_data.write();
            match &mut *data {
                IteratorData::List { items, index } => {
                    if *index < items.len() {
                        let v = items[*index].clone();
                        *index += 1;
                        Ok(Some(v))
                    } else {
                        Ok(None)
                    }
                }
                IteratorData::Tuple { items, index } => {
                    if *index < items.len() {
                        let v = items[*index].clone();
                        *index += 1;
                        Ok(Some(v))
                    } else {
                        Ok(None)
                    }
                }
                IteratorData::Range {
                    current,
                    stop,
                    step,
                } => {
                    let done = if *step > 0 {
                        *current >= *stop
                    } else {
                        *current <= *stop
                    };
                    if done {
                        Ok(None)
                    } else {
                        let v = PyObject::int(*current);
                        *current += *step;
                        Ok(Some(v))
                    }
                }
                IteratorData::Str { chars, index } => {
                    if *index < chars.len() {
                        let v = PyObject::str_val(CompactString::from(chars[*index].to_string()));
                        *index += 1;
                        Ok(Some(v))
                    } else {
                        Ok(None)
                    }
                }
                IteratorData::DictEntries {
                    source,
                    owner: _,
                    index,
                    cached_tuple,
                } => {
                    let map = unsafe { &*source.data_ptr() };
                    while *index < map.len() {
                        let (hk, _) = map.get_index(*index).unwrap();
                        if !is_hidden_dict_key(hk) {
                            break;
                        }
                        *index += 1;
                    }
                    if *index < map.len() {
                        let (hk, v) = map.get_index(*index).unwrap();
                        let k = hk.to_object();
                        let v = v.clone();
                        *index += 1;
                        let tuple = if let Some(ref ct) = cached_tuple {
                            if PyObjectRef::strong_count(ct) == 1 {
                                unsafe {
                                    let obj_ptr = PyObjectRef::as_ptr(ct) as *mut PyObject;
                                    if let PyObjectPayload::Tuple(ref mut items) =
                                        (*obj_ptr).payload
                                    {
                                        items[0] = k;
                                        items[1] = v;
                                    }
                                }
                                ct.clone()
                            } else {
                                let t = PyObject::tuple(vec![k, v]);
                                *cached_tuple = Some(t.clone());
                                t
                            }
                        } else {
                            let t = PyObject::tuple(vec![k, v]);
                            *cached_tuple = Some(t.clone());
                            t
                        };
                        Ok(Some(tuple))
                    } else {
                        Ok(None)
                    }
                }
                IteratorData::DictKeys { keys, index } => {
                    if *index < keys.len() {
                        let obj = keys[*index].clone();
                        *index += 1;
                        Ok(Some(obj))
                    } else {
                        Ok(None)
                    }
                }
                _ => Err(PyException::type_error(
                    "lazy iterator requires VM-level iteration",
                )),
            }
        }
        PyObjectPayload::RangeIter(ri) => {
            let cur = ri.current.get();
            let done = if ri.step > 0 {
                cur >= ri.stop
            } else {
                cur <= ri.stop
            };
            if done {
                Ok(None)
            } else {
                let v = PyObject::int(cur);
                ri.current.set(cur + ri.step);
                Ok(Some(v))
            }
        }
        PyObjectPayload::VecIter(data) => {
            let idx = data.index.get();
            if idx < data.items.len() {
                let v = data.items[idx].clone();
                data.index.set(idx + 1);
                Ok(Some(v))
            } else {
                Ok(None)
            }
        }
        PyObjectPayload::WeakValueIter(data) => loop {
            let idx = data.index.get();
            if idx >= data.entries.len() {
                return Ok(None);
            }
            data.index.set(idx + 1);
            let (key, ref_obj) = &data.entries[idx];
            let Some(target_fn) = ref_obj.get_attr("__weakref_target__") else {
                continue;
            };
            let value = match call_callable(&target_fn, &[]) {
                Ok(obj) if !matches!(&obj.payload, PyObjectPayload::None) => obj,
                Ok(_) => continue,
                Err(_) => continue,
            };
            return Ok(Some(match data.kind {
                WeakValueIterKind::Keys => key.clone(),
                WeakValueIterKind::Values => value,
                WeakValueIterKind::Items => PyObject::tuple(vec![key.clone(), value]),
            }));
        },
        PyObjectPayload::WeakKeyIter(data) => loop {
            let idx = data.index.get();
            if idx >= data.entries.len() {
                return Ok(None);
            }
            data.index.set(idx + 1);
            let (ref_obj, value) = &data.entries[idx];
            let Some(target_fn) = ref_obj.get_attr("__weakref_target__") else {
                continue;
            };
            let key = match call_callable(&target_fn, &[]) {
                Ok(obj) if !matches!(&obj.payload, PyObjectPayload::None) => obj,
                Ok(_) => continue,
                Err(_) => continue,
            };
            return Ok(Some(match data.kind {
                WeakKeyIterKind::Keys => key,
                WeakKeyIterKind::Items => PyObject::tuple(vec![key, value.clone()]),
            }));
        },
        PyObjectPayload::RefIter { source, index } => {
            if index.get() == usize::MAX {
                return Ok(None);
            }
            let idx = index.get();
            match &source.payload {
                PyObjectPayload::List(cell) => {
                    let items = unsafe { &*cell.data_ptr() };
                    if idx < items.len() {
                        let v = items[idx].clone();
                        index.set(idx + 1);
                        Ok(Some(v))
                    } else {
                        index.set(usize::MAX);
                        Ok(None)
                    }
                }
                PyObjectPayload::Tuple(items) => {
                    if idx < items.len() {
                        let v = items[idx].clone();
                        index.set(idx + 1);
                        Ok(Some(v))
                    } else {
                        index.set(usize::MAX);
                        Ok(None)
                    }
                }
                PyObjectPayload::Dict(cell)
                | PyObjectPayload::MappingProxy(cell)
                | PyObjectPayload::DictKeys { map: cell, .. } => {
                    let map = unsafe { &*cell.data_ptr() };
                    if idx < map.len() {
                        let v = map.get_index(idx).unwrap().0.to_object();
                        index.set(idx + 1);
                        Ok(Some(v))
                    } else {
                        index.set(usize::MAX);
                        Ok(None)
                    }
                }
                _ => Ok(None),
            }
        }
        PyObjectPayload::RevRefIter { source, index } => {
            let idx = index.get();
            if idx == usize::MAX {
                return Ok(None);
            }
            if idx == 0 {
                index.set(usize::MAX);
                return Ok(None);
            }
            match &source.payload {
                PyObjectPayload::List(cell) => {
                    let pos = idx - 1;
                    let items = unsafe { &*cell.data_ptr() };
                    if pos < items.len() {
                        let v = items[pos].clone();
                        index.set(pos);
                        Ok(Some(v))
                    } else {
                        index.set(usize::MAX);
                        Ok(None)
                    }
                }
                _ => Ok(None),
            }
        }
        _ => Err(PyException::type_error("iter_next_value on non-iterator")),
    }
}

/// Public access to get_iter_from_obj for lazy iterator construction.
pub(crate) fn get_iter_from_obj_pub(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    get_iter_from_obj(obj)
}
