use crate::error::{ExceptionKind, PyException, PyResult};
use compact_str::CompactString;
use indexmap::IndexMap;
use num_traits::ToPrimitive;

use super::super::helpers::*;
use super::super::methods::PyObjectMethods;
use super::super::payload::*;

fn ensure_iterator_for_to_list(owner: &PyObjectRef, iter: PyObjectRef) -> PyResult<PyObjectRef> {
    if iter.get_attr("__next__").is_some() {
        Ok(iter)
    } else {
        Err(PyException::type_error(format!(
            "iter() returned non-iterator of type '{}'",
            owner.type_name()
        )))
    }
}

fn collect_next_iterator(iter_obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
    let mut result = Vec::new();
    loop {
        let next = iter_obj.get_attr("__next__").ok_or_else(|| {
            PyException::type_error(format!(
                "'{}' object is not an iterator",
                iter_obj.type_name()
            ))
        })?;
        match call_callable(&next, &[]) {
            Ok(value) => guarded_push(&mut result, value, "iterator -> list")?,
            Err(err) if err.kind == ExceptionKind::StopIteration => break,
            Err(err) => return Err(err),
        }
    }
    Ok(result)
}

fn collect_instance_iterable(obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
    if let Some(iter_method) = obj.get_attr("__iter__") {
        let iter = ensure_iterator_for_to_list(obj, call_callable(&iter_method, &[])?)?;
        return collect_next_iterator(&iter);
    }
    if obj.get_attr("__next__").is_some() {
        return collect_next_iterator(obj);
    }
    if obj.get_attr("__getitem__").is_some() {
        let mut result = Vec::new();
        let mut idx = 0i64;
        loop {
            match obj.get_item(&PyObject::int(idx)) {
                Ok(value) => guarded_push(&mut result, value, "sequence -> list")?,
                Err(err)
                    if err.kind == ExceptionKind::IndexError
                        || err.kind == ExceptionKind::StopIteration =>
                {
                    break
                }
                Err(err) => return Err(err),
            }
            idx += 1;
        }
        return Ok(result);
    }
    Err(PyException::type_error(format!(
        "'{}' object is not iterable",
        obj.type_name()
    )))
}

fn call_module_dunder(obj: &PyObjectRef, method: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &method.payload {
        PyObjectPayload::NativeFunction(_) | PyObjectPayload::NativeClosure(_) => {
            call_callable(method, &[obj.clone()])
        }
        _ => call_callable(method, &[]),
    }
}

fn collect_module_iterable(obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
    let iter = if let Some(iter_method) = obj.get_attr("__iter__") {
        call_module_dunder(obj, &iter_method)?
    } else if obj.get_attr("__next__").is_some() {
        obj.clone()
    } else {
        return Err(PyException::type_error(format!(
            "'{}' object is not iterable",
            obj.type_name()
        )));
    };
    if !PyObjectRef::ptr_eq(&iter, obj) {
        return iter.to_list();
    }
    let mut result = Vec::new();
    loop {
        let next = obj.get_attr("__next__").ok_or_else(|| {
            PyException::type_error(format!("'{}' object is not an iterator", obj.type_name()))
        })?;
        match call_module_dunder(obj, &next) {
            Ok(value) => guarded_push(&mut result, value, "module iterator -> list")?,
            Err(err) if err.kind == ExceptionKind::StopIteration => break,
            Err(err) => return Err(err),
        }
    }
    Ok(result)
}

pub(in crate::object) fn py_to_list(obj: &PyObjectRef) -> PyResult<Vec<PyObjectRef>> {
    match &obj.payload {
        PyObjectPayload::List(v) => {
            let read = v.read();
            guard_eager_allocation(read.len(), "list -> list")?;
            Ok(read.clone())
        }
        PyObjectPayload::Tuple(v) => {
            guard_eager_allocation(v.len(), "tuple -> list")?;
            Ok((**v).clone())
        }
        PyObjectPayload::Set(m) => {
            let read = m.read();
            guard_eager_allocation(read.len(), "set -> list")?;
            Ok(read.values().cloned().collect())
        }
        PyObjectPayload::FrozenSet(m) => {
            guard_eager_allocation(m.len(), "frozenset -> list")?;
            Ok(m.values().cloned().collect())
        }
        PyObjectPayload::Str(s) => {
            let len = s.chars().count();
            guard_eager_allocation(len, "str -> list")?;
            Ok(s.chars()
                .map(|c| PyObject::str_val(CompactString::from(c.to_string())))
                .collect())
        }
        PyObjectPayload::Dict(m) | PyObjectPayload::MappingProxy(m) => {
            let read = m.read();
            let visible = read.keys().filter(|k| !is_hidden_dict_key(k)).count();
            guard_eager_allocation(visible, "dict -> list")?;
            Ok(read
                .keys()
                .filter(|k| !is_hidden_dict_key(k))
                .map(|k| k.to_object())
                .collect())
        }
        PyObjectPayload::Instance(inst) if inst.attrs.read().contains_key("__chainmap__") => {
            let maps = inst
                .attrs
                .read()
                .get("maps")
                .cloned()
                .ok_or_else(|| PyException::type_error("ChainMap missing maps"))?;
            let maps = maps.to_list()?;
            let mut combined = IndexMap::new();
            for mapping in maps.iter().rev() {
                for key in mapping.to_list()? {
                    let hk = key.to_hashable_key()?;
                    combined.insert(hk, key);
                }
            }
            Ok(combined.keys().map(|k| k.to_object()).collect())
        }
        PyObjectPayload::Instance(inst) if inst.attrs.read().contains_key("__deque__") => {
            if let Some(data) = inst.attrs.read().get("_data").cloned() {
                if let PyObjectPayload::List(items) = &data.payload {
                    return Ok(items.read().clone());
                }
            }
            Ok(vec![])
        }
        PyObjectPayload::Instance(inst) if inst.dict_storage.is_some() => {
            if let Some(storage) = inst.dict_storage.as_ref() {
                let read = storage.read();
                guard_eager_allocation(read.len(), "instance dict -> list")?;
                Ok(read.keys().map(|k| k.to_object()).collect())
            } else {
                Ok(vec![])
            }
        }
        PyObjectPayload::InstanceDict(attrs) => {
            let read = attrs.read();
            guard_eager_allocation(read.len(), "__dict__ -> list")?;
            Ok(read.keys().map(|k| PyObject::str_val(k.clone())).collect())
        }
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
            guard_eager_allocation(b.len(), "bytes -> list")?;
            Ok(b.iter().map(|byte| PyObject::int(*byte as i64)).collect())
        }
        PyObjectPayload::Range(rd) => {
            let len = range_data_len_bigint(rd);
            guard_eager_allocation(len.to_usize().unwrap_or(usize::MAX), "range -> list")?;
            let mut result = Vec::new();
            let mut idx = num_bigint::BigInt::from(0);
            while idx < len {
                let value = range_item_bigint(rd, &idx);
                let item = if let Some(value) = value.to_i64() {
                    PyObject::int(value)
                } else {
                    PyObject::big_int(value)
                };
                guarded_push(&mut result, item, "range -> list")?;
                idx += 1;
            }
            Ok(result)
        }
        PyObjectPayload::Iterator(iter_data) => {
            let mut data = iter_data.write();
            match &mut *data {
                IteratorData::List { items, index } => {
                    guard_eager_allocation(items.len().saturating_sub(*index), "iterator -> list")?;
                    let result = items[*index..].to_vec();
                    *index = items.len();
                    Ok(result)
                }
                IteratorData::Tuple { items, index } => {
                    guard_eager_allocation(items.len().saturating_sub(*index), "iterator -> list")?;
                    let result = items[*index..].to_vec();
                    *index = items.len();
                    Ok(result)
                }
                IteratorData::Range {
                    current,
                    stop,
                    step,
                } => {
                    guard_eager_allocation(
                        range_len(*current, *stop, *step).max(0) as usize,
                        "range iterator -> list",
                    )?;
                    let mut result = Vec::new();
                    while let Some((value, next)) = range_next_i64(*current, *stop, *step) {
                        guarded_push(&mut result, PyObject::int(value), "range iterator -> list")?;
                        *current = next;
                    }
                    *current = *stop;
                    Ok(result)
                }
                IteratorData::BigRange(iter) => {
                    let len = range_iter_len_bigint(iter);
                    guard_eager_allocation(
                        len.to_usize().unwrap_or(usize::MAX),
                        "range iterator -> list",
                    )?;
                    let mut result = Vec::new();
                    while range_iter_len_bigint(iter) > num_bigint::BigInt::from(0) {
                        let value = range_iter_item_bigint(iter);
                        guarded_push(
                            &mut result,
                            py_int_from_bigint(value),
                            "range iterator -> list",
                        )?;
                        iter.index += 1;
                    }
                    Ok(result)
                }
                IteratorData::Str { chars, index } => {
                    guard_eager_allocation(
                        chars.len().saturating_sub(*index),
                        "str iterator -> list",
                    )?;
                    let result: Vec<PyObjectRef> = chars[*index..]
                        .iter()
                        .map(|c| PyObject::str_val(CompactString::from(c.to_string())))
                        .collect();
                    *index = chars.len();
                    Ok(result)
                }
                IteratorData::DictKeys { keys, index } => {
                    guard_eager_allocation(
                        keys.len().saturating_sub(*index),
                        "dict keys iterator -> list",
                    )?;
                    let result = keys[*index..].to_vec();
                    *index = keys.len();
                    Ok(result)
                }
                IteratorData::DictKeyRefs {
                    source,
                    index,
                    expected_len,
                } => {
                    let map = source.read();
                    if map.len() != *expected_len {
                        return Err(PyException::runtime_error(
                            "dictionary changed size during iteration",
                        ));
                    }
                    guard_eager_allocation(
                        map.len().saturating_sub(*index),
                        "dict keys iterator -> list",
                    )?;
                    let result = map
                        .iter()
                        .skip(*index)
                        .map(|(key, _)| key.to_object())
                        .collect();
                    *index = map.len();
                    Ok(result)
                }
                IteratorData::Enumerate { .. }
                | IteratorData::Zip { .. }
                | IteratorData::ZipLongest { .. }
                | IteratorData::Islice { .. }
                | IteratorData::MapOne { .. }
                | IteratorData::Map { .. }
                | IteratorData::Filter { .. }
                | IteratorData::FilterFalse { .. }
                | IteratorData::Sentinel { .. }
                | IteratorData::TakeWhile { .. }
                | IteratorData::DropWhile { .. }
                | IteratorData::Count { .. }
                | IteratorData::Cycle { .. }
                | IteratorData::Repeat { .. }
                | IteratorData::Chain { .. }
                | IteratorData::SeqIter { .. }
                | IteratorData::Starmap { .. }
                | IteratorData::Tee { .. }
                | IteratorData::HeldIter { .. }
                | IteratorData::DictEntries { .. } => Err(PyException::type_error(
                    "lazy iterator requires VM to collect",
                )),
            }
        }
        PyObjectPayload::RangeIter(ri) => {
            guard_eager_allocation(
                range_len(ri.current.get(), ri.stop, ri.step).max(0) as usize,
                "range iterator -> list",
            )?;
            let mut result = Vec::new();
            while let Some((value, next)) = range_next_i64(ri.current.get(), ri.stop, ri.step) {
                guarded_push(&mut result, PyObject::int(value), "range iterator -> list")?;
                ri.current.set(next);
            }
            ri.current.set(ri.stop);
            Ok(result)
        }
        PyObjectPayload::VecIter(data) => {
            let idx = data.index.get();
            if idx >= data.items.len() {
                return Ok(vec![]);
            }
            guard_eager_allocation(data.items.len() - idx, "iterator -> list")?;
            let result = data.items[idx..].to_vec();
            data.index.set(usize::MAX);
            Ok(result)
        }
        PyObjectPayload::DequeIter(_) => Err(PyException::type_error(
            "lazy iterator requires VM to collect",
        )),
        PyObjectPayload::RefIter { source, index } => {
            if index.get() == usize::MAX {
                return Ok(vec![]);
            }
            let idx = index.get();
            match &source.payload {
                PyObjectPayload::List(cell) => {
                    let items = unsafe { &*cell.data_ptr() };
                    if idx >= items.len() {
                        return Ok(vec![]);
                    }
                    guard_eager_allocation(items.len() - idx, "iterator -> list")?;
                    let result = items[idx..].to_vec();
                    index.set(usize::MAX);
                    Ok(result)
                }
                PyObjectPayload::Tuple(items) => {
                    if idx >= items.len() {
                        return Ok(vec![]);
                    }
                    guard_eager_allocation(items.len() - idx, "iterator -> list")?;
                    let result = items[idx..].to_vec();
                    index.set(usize::MAX);
                    Ok(result)
                }
                PyObjectPayload::Dict(cell)
                | PyObjectPayload::MappingProxy(cell)
                | PyObjectPayload::DictKeys { map: cell, .. } => {
                    let map = unsafe { &*cell.data_ptr() };
                    if idx >= map.len() {
                        return Ok(vec![]);
                    }
                    guard_eager_allocation(map.len() - idx, "dict iterator -> list")?;
                    let result = map
                        .iter()
                        .skip(idx)
                        .map(|(key, _)| key.to_object())
                        .collect();
                    index.set(usize::MAX);
                    Ok(result)
                }
                _ => Ok(vec![]),
            }
        }
        PyObjectPayload::RevRefIter { source, index, .. } => {
            let mut idx = index.get();
            if idx == usize::MAX || idx == 0 {
                return Ok(vec![]);
            }
            match &source.payload {
                PyObjectPayload::List(cell) => {
                    let items = unsafe { &*cell.data_ptr() };
                    if idx > items.len() {
                        index.set(usize::MAX);
                        return Ok(vec![]);
                    }
                    guard_eager_allocation(idx, "reverse iterator -> list")?;
                    let mut result = Vec::with_capacity(idx);
                    while idx > 0 {
                        idx -= 1;
                        if idx < items.len() {
                            result.push(items[idx].clone());
                        }
                    }
                    index.set(usize::MAX);
                    Ok(result)
                }
                _ => Ok(vec![]),
            }
        }
        PyObjectPayload::Instance(inst) if inst.class.get_attr("__namedtuple__").is_some() => {
            if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                if let PyObjectPayload::Tuple(items) = &tup.payload {
                    return Ok((**items).clone());
                }
            }
            Ok(vec![])
        }
        PyObjectPayload::Instance(_) => collect_instance_iterable(obj),
        PyObjectPayload::DictKeys { map: m, .. } => {
            let read = m.read();
            let visible = read.keys().filter(|k| !is_hidden_dict_key(k)).count();
            guard_eager_allocation(visible, "dict_keys -> list")?;
            Ok(read
                .keys()
                .filter(|k| !is_hidden_dict_key(k))
                .map(|k| k.to_object())
                .collect())
        }
        PyObjectPayload::DictValues { map: m, .. } => {
            let read = m.read();
            let visible = read.iter().filter(|(k, _)| !is_hidden_dict_key(k)).count();
            guard_eager_allocation(visible, "dict_values -> list")?;
            Ok(read
                .iter()
                .filter(|(k, _)| !is_hidden_dict_key(k))
                .map(|(_, v)| v.clone())
                .collect())
        }
        PyObjectPayload::DictItems { map: m, .. } => {
            let read = m.read();
            let visible = read.iter().filter(|(k, _)| !is_hidden_dict_key(k)).count();
            guard_eager_allocation(visible, "dict_items -> list")?;
            Ok(read
                .iter()
                .filter(|(k, _)| !is_hidden_dict_key(k))
                .map(|(k, v)| PyObject::tuple(vec![k.to_object(), v.clone()]))
                .collect())
        }
        PyObjectPayload::Module(_) => collect_module_iterable(obj),
        _ => Err(PyException::type_error(format!(
            "'{}' object is not iterable",
            obj.type_name()
        ))),
    }
}
