//! List, tuple, and range method dispatch.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::helpers::{index_to_i64, index_to_usize_repeat};
use ferrython_core::object::IteratorData;
use ferrython_core::object::{
    check_args_min, checked_repeat_len, CompareOp, PyCell, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::PyInt;
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use std::rc::Rc;

use super::partial_cmp_for_sort;

fn sequence_item_matches(item: &PyObjectRef, target: &PyObjectRef) -> PyResult<bool> {
    if PyObjectRef::ptr_eq(item, target) {
        return Ok(true);
    }
    if item.compare(target, CompareOp::Eq)?.is_truthy() {
        return Ok(true);
    }
    Ok(target.compare(item, CompareOp::Eq)?.is_truthy())
}

pub(crate) fn call_list_method(
    receiver: &PyObjectRef,
    items: &PyCell<Vec<PyObjectRef>>,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match method {
        "copy" => Ok(PyObject::list(items.read().to_vec())),
        "count" => {
            check_args_min("count", args, 1)?;
            let target = &args[0];
            let snapshot = items.read().clone();
            let mut c = 0usize;
            for item in &snapshot {
                if sequence_item_matches(item, target)? {
                    c += 1;
                }
            }
            Ok(PyObject::int(c as i64))
        }
        "index" => {
            check_args_min("index", args, 1)?;
            let target = &args[0];
            let snapshot = items.read().clone();
            let len = snapshot.len();
            let start = if args.len() > 1 {
                let s = args[1].to_int().unwrap_or(0);
                if s < 0 {
                    (len as i64 + s).max(0) as usize
                } else {
                    s as usize
                }
            } else {
                0
            };
            let stop = if args.len() > 2 {
                let s = args[2].to_int().unwrap_or(len as i64);
                if s < 0 {
                    (len as i64 + s).max(0) as usize
                } else {
                    (s as usize).min(len)
                }
            } else {
                len
            };
            for i in start..stop {
                if sequence_item_matches(&snapshot[i], target)? {
                    return Ok(PyObject::int(i as i64));
                }
            }
            Err(PyException::value_error(format!(
                "{} is not in list",
                target.py_to_string()
            )))
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
            let idx = index_to_i64(&args[0])?;
            let mut w = items.write();
            let len = w.len() as i64;
            let actual = if idx < 0 {
                (len + idx).max(0) as usize
            } else {
                (idx as usize).min(w.len())
            };
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
                let idx = index_to_i64(&args[0])?;
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
            let snapshot = items.read().clone();
            let mut pos = None;
            for (i, item) in snapshot.iter().enumerate() {
                if sequence_item_matches(item, target)? {
                    pos = Some(i);
                    break;
                }
            }
            match pos {
                Some(i) => {
                    items.write().remove(i);
                    Ok(PyObject::none())
                }
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
            // Homogeneous small-int sort: only for large lists (≥32 elements)
            if v.len() >= 32 {
                let all_small_int = v.iter().all(|x| {
                    matches!(
                        &x.payload,
                        PyObjectPayload::Int(ferrython_core::types::PyInt::Small(_))
                    )
                });
                if all_small_int {
                    v.sort_unstable_by(|a, b| {
                        let av =
                            if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(val)) =
                                &a.payload
                            {
                                *val
                            } else {
                                0
                            };
                        let bv =
                            if let PyObjectPayload::Int(ferrython_core::types::PyInt::Small(val)) =
                                &b.payload
                            {
                                *val
                            } else {
                                0
                            };
                        av.cmp(&bv)
                    });
                } else {
                    v.sort_by(|a, b| {
                        partial_cmp_for_sort(a, b).unwrap_or(std::cmp::Ordering::Equal)
                    });
                }
            } else if v.len() > 1 {
                v.sort_by(|a, b| partial_cmp_for_sort(a, b).unwrap_or(std::cmp::Ordering::Equal));
            }
            w.extend(v);
            Ok(PyObject::none())
        }
        "clear" => {
            items.write().clear();
            Ok(PyObject::none())
        }
        "__iter__" => {
            let snapshot = items.read().clone();
            Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(IteratorData::List {
                    items: snapshot,
                    index: 0,
                }),
            ))))
        }
        "__len__" => Ok(PyObject::int(items.read().len() as i64)),
        "__contains__" => {
            check_args_min("__contains__", args, 1)?;
            let target = &args[0];
            let snapshot = items.read().clone();
            for item in &snapshot {
                if sequence_item_matches(item, target)? {
                    return Ok(PyObject::bool_val(true));
                }
            }
            Ok(PyObject::bool_val(false))
        }
        "__getitem__" => {
            check_args_min("__getitem__", args, 1)?;
            if let PyObjectPayload::Slice(sd) = &args[0].payload {
                let r = items.read();
                let len = r.len() as i64;
                let step_val = sd
                    .step
                    .as_ref()
                    .map(|v| v.as_int().unwrap_or(1))
                    .unwrap_or(1);
                if step_val == 0 {
                    return Err(PyException::value_error("slice step cannot be zero"));
                }
                let s_val = sd
                    .start
                    .as_ref()
                    .map(|v| v.as_int().unwrap_or(if step_val > 0 { 0 } else { len - 1 }))
                    .unwrap_or(if step_val > 0 { 0 } else { len - 1 });
                let e_val = sd
                    .stop
                    .as_ref()
                    .map(|v| {
                        v.as_int()
                            .unwrap_or(if step_val > 0 { len } else { -len - 1 })
                    })
                    .unwrap_or(if step_val > 0 { len } else { -len - 1 });
                let s = (if s_val < 0 {
                    (len + s_val).max(0)
                } else {
                    s_val.min(len)
                }) as usize;
                let e = (if e_val < 0 {
                    (len + e_val).max(0)
                } else {
                    e_val.min(len)
                }) as usize;
                let mut result = Vec::new();
                if step_val == 1 {
                    if s < e {
                        result = r[s..e].to_vec();
                    }
                } else if step_val > 0 {
                    let mut i = s;
                    while i < e {
                        result.push(r[i].clone());
                        i += step_val as usize;
                    }
                } else {
                    let mut i = s as i64;
                    let end = e as i64;
                    while i > end {
                        result.push(r[i as usize].clone());
                        i += step_val;
                    }
                }
                Ok(PyObject::list(result))
            } else {
                let idx = index_to_i64(&args[0])?;
                let r = items.read();
                let len = r.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len {
                    return Err(PyException::index_error("list index out of range"));
                }
                Ok(r[actual as usize].clone())
            }
        }
        "__setitem__" => {
            check_args_min("__setitem__", args, 2)?;
            if let PyObjectPayload::Slice(sd) = &args[0].payload {
                let new_items = args[1].to_list()?;
                let mut w = items.write();
                let len = w.len() as i64;
                let step_val = sd
                    .step
                    .as_ref()
                    .map(|v| v.as_int().unwrap_or(1))
                    .unwrap_or(1);
                if step_val == 1 || step_val == 0 {
                    let s_val = sd
                        .start
                        .as_ref()
                        .map(|v| v.as_int().unwrap_or(0))
                        .unwrap_or(0);
                    let e_val = sd
                        .stop
                        .as_ref()
                        .map(|v| v.as_int().unwrap_or(len))
                        .unwrap_or(len);
                    let s = (if s_val < 0 {
                        (len + s_val).max(0)
                    } else {
                        s_val.min(len)
                    }) as usize;
                    let e = (if e_val < 0 {
                        (len + e_val).max(0)
                    } else {
                        e_val.min(len)
                    }) as usize;
                    let e = e.max(s);
                    w.splice(s..e, new_items);
                } else {
                    let s_val = if step_val > 0 {
                        sd.start
                            .as_ref()
                            .map(|v| v.as_int().unwrap_or(0))
                            .unwrap_or(0)
                    } else {
                        sd.start
                            .as_ref()
                            .map(|v| v.as_int().unwrap_or(len - 1))
                            .unwrap_or(len - 1)
                    };
                    let e_val = if step_val > 0 {
                        sd.stop
                            .as_ref()
                            .map(|v| v.as_int().unwrap_or(len))
                            .unwrap_or(len)
                    } else {
                        sd.stop
                            .as_ref()
                            .map(|v| v.as_int().unwrap_or(-len - 1))
                            .unwrap_or(-len - 1)
                    };
                    let mut indices = Vec::new();
                    let mut i = if s_val < 0 {
                        (len + s_val).max(0)
                    } else {
                        s_val.min(len)
                    };
                    let end = if e_val < 0 {
                        (len + e_val).max(-1)
                    } else {
                        e_val.min(len)
                    };
                    if step_val > 0 {
                        while i < end {
                            indices.push(i as usize);
                            i += step_val;
                        }
                    } else {
                        while i > end {
                            indices.push(i as usize);
                            i += step_val;
                        }
                    }
                    if indices.len() != new_items.len() {
                        return Err(PyException::value_error(format!(
                            "attempt to assign sequence of size {} to extended slice of size {}",
                            new_items.len(),
                            indices.len()
                        )));
                    }
                    for (idx, val) in indices.iter().zip(new_items.iter()) {
                        w[*idx] = val.clone();
                    }
                }
                Ok(PyObject::none())
            } else {
                let idx = index_to_i64(&args[0])?;
                let mut w = items.write();
                let len = w.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len {
                    return Err(PyException::index_error(
                        "list assignment index out of range",
                    ));
                }
                w[actual as usize] = args[1].clone();
                Ok(PyObject::none())
            }
        }
        "__delitem__" => {
            check_args_min("__delitem__", args, 1)?;
            if let PyObjectPayload::Slice(sd) = &args[0].payload {
                let mut w = items.write();
                let len = w.len() as i64;
                let step_val = sd
                    .step
                    .as_ref()
                    .map(|v| v.as_int().unwrap_or(1))
                    .unwrap_or(1);
                let s_val = sd
                    .start
                    .as_ref()
                    .map(|v| v.as_int().unwrap_or(if step_val > 0 { 0 } else { len - 1 }))
                    .unwrap_or(if step_val > 0 { 0 } else { len - 1 });
                let e_val = sd
                    .stop
                    .as_ref()
                    .map(|v| {
                        v.as_int()
                            .unwrap_or(if step_val > 0 { len } else { -len - 1 })
                    })
                    .unwrap_or(if step_val > 0 { len } else { -len - 1 });
                let mut indices = Vec::new();
                let mut i = if s_val < 0 {
                    (len + s_val).max(0)
                } else {
                    s_val.min(len)
                };
                let end = if e_val < 0 {
                    (len + e_val).max(if step_val > 0 { 0 } else { -1 })
                } else {
                    e_val.min(len)
                };
                if step_val > 0 {
                    while i < end {
                        indices.push(i as usize);
                        i += step_val;
                    }
                } else if step_val < 0 {
                    while i > end {
                        indices.push(i as usize);
                        i += step_val;
                    }
                }
                // Remove in reverse order to preserve indices
                indices.sort_unstable();
                indices.reverse();
                for idx in indices {
                    if idx < w.len() {
                        w.remove(idx);
                    }
                }
                Ok(PyObject::none())
            } else {
                let idx = index_to_i64(&args[0])?;
                let mut w = items.write();
                let len = w.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len {
                    return Err(PyException::index_error(
                        "list assignment index out of range",
                    ));
                }
                w.remove(actual as usize);
                Ok(PyObject::none())
            }
        }
        "__add__" => {
            check_args_min("__add__", args, 1)?;
            let other = args[0].to_list()?;
            let mut result = items.read().clone();
            result.extend(other);
            Ok(PyObject::list(result))
        }
        "__mul__" | "__rmul__" => {
            check_args_min("__mul__", args, 1)?;
            let n = index_to_usize_repeat(&args[0])?;
            let base = items.read().clone();
            let size = checked_repeat_len(base.len(), n, "list repeat")?;
            let mut result = Vec::with_capacity(size);
            for _ in 0..n {
                result.extend_from_slice(&base);
            }
            Ok(PyObject::list(result))
        }
        "__iadd__" => {
            check_args_min("__iadd__", args, 1)?;
            let other = args[0].to_list()?;
            items.write().extend(other);
            Ok(receiver.clone())
        }
        "__imul__" => {
            check_args_min("__imul__", args, 1)?;
            let n = index_to_usize_repeat(&args[0])?;
            let mut w = items.write();
            let base = w.clone();
            checked_repeat_len(base.len(), n, "list repeat")?;
            w.clear();
            for _ in 0..n {
                w.extend_from_slice(&base);
            }
            Ok(receiver.clone())
        }
        "__reversed__" => {
            let len = items.read().len();
            Ok(PyObject::wrap(PyObjectPayload::RevRefIter {
                source: receiver.clone(),
                index: ferrython_core::object::SyncUsize::new(len),
            }))
        }
        "__repr__" | "__str__" => {
            let r = items.read();
            let parts: Vec<String> = r.iter().map(|x| x.repr()).collect();
            Ok(PyObject::str_val(CompactString::from(format!(
                "[{}]",
                parts.join(", ")
            ))))
        }
        "__eq__" => {
            check_args_min("__eq__", args, 1)?;
            if let PyObjectPayload::List(other) = &args[0].payload {
                let a = items.read();
                let b = other.read();
                if a.len() != b.len() {
                    return Ok(PyObject::bool_val(false));
                }
                for (x, y) in a.iter().zip(b.iter()) {
                    if x.py_to_string() != y.py_to_string() {
                        return Ok(PyObject::bool_val(false));
                    }
                }
                Ok(PyObject::bool_val(true))
            } else {
                Ok(PyObject::not_implemented())
            }
        }
        "__ne__" => {
            check_args_min("__ne__", args, 1)?;
            if let PyObjectPayload::List(other) = &args[0].payload {
                let a = items.read();
                let b = other.read();
                if a.len() != b.len() {
                    return Ok(PyObject::bool_val(true));
                }
                for (x, y) in a.iter().zip(b.iter()) {
                    if x.py_to_string() != y.py_to_string() {
                        return Ok(PyObject::bool_val(true));
                    }
                }
                Ok(PyObject::bool_val(false))
            } else {
                Ok(PyObject::not_implemented())
            }
        }
        "__bool__" => Ok(PyObject::bool_val(!items.read().is_empty())),
        "__hash__" => Err(PyException::type_error("unhashable type: 'list'")),
        "__sizeof__" => Ok(PyObject::int(
            (std::mem::size_of::<Vec<PyObjectRef>>()
                + items.read().len() * std::mem::size_of::<PyObjectRef>()) as i64,
        )),
        _ => Err(PyException::attribute_error(format!(
            "'list' object has no attribute '{}'",
            method
        ))),
    }
}

pub(crate) fn call_range_method(
    receiver: &PyObjectRef,
    rd: &ferrython_core::object::RangeData,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match method {
        "__len__" => Ok(PyObject::int(
            ferrython_core::object::helpers::range_data_len_i128(rd).min(i64::MAX as i128) as i64,
        )),
        "__iter__" => receiver.get_iter(),
        "__contains__" => {
            check_args_min("range.__contains__", args, 1)?;
            Ok(PyObject::bool_val(receiver.contains(&args[0])?))
        }
        "__getitem__" => {
            check_args_min("range.__getitem__", args, 1)?;
            receiver.get_item(&args[0])
        }
        "count" => {
            check_args_min("range.count", args, 1)?;
            if let Some(value) = ferrython_core::object::helpers::py_int_bigint(&args[0]) {
                Ok(PyObject::int(
                    if ferrython_core::object::helpers::range_contains_bigint(rd, &value) {
                        1
                    } else {
                        0
                    },
                ))
            } else {
                let len = ferrython_core::object::helpers::range_data_len_bigint(rd);
                if len > BigInt::from(1024usize) {
                    return Ok(PyObject::int(0));
                }
                let mut count = 0i64;
                let mut idx = BigInt::zero();
                while idx < len {
                    let value = ferrython_core::object::helpers::range_item_bigint(rd, &idx);
                    let candidate = if let Some(value) = value.to_i64() {
                        PyObject::int(value)
                    } else {
                        PyObject::big_int(value)
                    };
                    if sequence_item_matches(&candidate, &args[0])? {
                        count += 1;
                    }
                    idx += 1;
                }
                Ok(PyObject::int(count))
            }
        }
        "index" => {
            check_args_min("range.index", args, 1)?;
            if let Some(value) = ferrython_core::object::helpers::py_int_bigint(&args[0]) {
                if ferrython_core::object::helpers::range_contains_bigint(rd, &value) {
                    let (start, _, step) = ferrython_core::object::helpers::range_parts_bigint(rd);
                    let idx = (value - start) / step;
                    if let Some(idx) = idx.to_i64() {
                        return Ok(PyObject::int(idx));
                    }
                    return Ok(PyObject::big_int(idx));
                }
            } else {
                let len = ferrython_core::object::helpers::range_data_len_bigint(rd);
                if len <= BigInt::from(1024usize) {
                    let mut idx = BigInt::zero();
                    while idx < len {
                        let value = ferrython_core::object::helpers::range_item_bigint(rd, &idx);
                        let candidate = if let Some(value) = value.to_i64() {
                            PyObject::int(value)
                        } else {
                            PyObject::big_int(value)
                        };
                        if sequence_item_matches(&candidate, &args[0])? {
                            if let Some(idx) = idx.to_i64() {
                                return Ok(PyObject::int(idx));
                            }
                            return Ok(PyObject::big_int(idx));
                        }
                        idx += 1;
                    }
                }
            }
            Err(PyException::value_error(format!(
                "{} is not in range",
                args[0].py_to_string()
            )))
        }
        _ => Err(PyException::attribute_error(format!(
            "'range' object has no attribute '{}'",
            method
        ))),
    }
}

pub(crate) fn call_tuple_method(
    receiver: &PyObjectRef,
    items: &[PyObjectRef],
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match method {
        "count" => {
            check_args_min("count", args, 1)?;
            let target = &args[0];
            let mut c = 0usize;
            for item in items {
                if sequence_item_matches(item, target)? {
                    c += 1;
                }
            }
            Ok(PyObject::int(c as i64))
        }
        "index" => {
            check_args_min("index", args, 1)?;
            let target = &args[0];
            for (i, x) in items.iter().enumerate() {
                if sequence_item_matches(x, target)? {
                    return Ok(PyObject::int(i as i64));
                }
            }
            Err(PyException::value_error("tuple.index(x): x not in tuple"))
        }
        "__iter__" => Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::List {
                items: items.to_vec(),
                index: 0,
            }),
        )))),
        "__len__" => Ok(PyObject::int(items.len() as i64)),
        "__contains__" => {
            check_args_min("__contains__", args, 1)?;
            let target = &args[0];
            for item in items {
                if sequence_item_matches(item, target)? {
                    return Ok(PyObject::bool_val(true));
                }
            }
            Ok(PyObject::bool_val(false))
        }
        "__getitem__" => {
            check_args_min("__getitem__", args, 1)?;
            let idx = index_to_i64(&args[0])?;
            let len = items.len() as i64;
            let actual = if idx < 0 { len + idx } else { idx };
            if actual < 0 || actual >= len {
                return Err(PyException::index_error("tuple index out of range"));
            }
            Ok(items[actual as usize].clone())
        }
        "__add__" => {
            check_args_min("__add__", args, 1)?;
            if let PyObjectPayload::Tuple(other) = &args[0].payload {
                let mut result = items.to_vec();
                result.extend_from_slice(other);
                Ok(PyObject::tuple(result))
            } else {
                Err(PyException::type_error(
                    "can only concatenate tuple to tuple",
                ))
            }
        }
        "__mul__" | "__rmul__" => {
            check_args_min("__mul__", args, 1)?;
            let index = args[0].to_index()?;
            if matches!(index, PyInt::Small(1)) {
                return Ok(receiver.clone());
            }
            let n = index_to_usize_repeat(&index.to_object())?;
            let size = checked_repeat_len(items.len(), n, "tuple repeat")?;
            let mut result = Vec::with_capacity(size);
            for _ in 0..n {
                result.extend_from_slice(items);
            }
            Ok(PyObject::tuple(result))
        }
        "__eq__" => {
            check_args_min("__eq__", args, 1)?;
            if let PyObjectPayload::Tuple(other) = &args[0].payload {
                if items.len() != other.len() {
                    return Ok(PyObject::bool_val(false));
                }
                for (a, b) in items.iter().zip(other.iter()) {
                    if a.py_to_string() != b.py_to_string() {
                        return Ok(PyObject::bool_val(false));
                    }
                }
                Ok(PyObject::bool_val(true))
            } else {
                Ok(PyObject::not_implemented())
            }
        }
        "__ne__" => {
            check_args_min("__ne__", args, 1)?;
            if let PyObjectPayload::Tuple(other) = &args[0].payload {
                if items.len() != other.len() {
                    return Ok(PyObject::bool_val(true));
                }
                for (a, b) in items.iter().zip(other.iter()) {
                    if a.py_to_string() != b.py_to_string() {
                        return Ok(PyObject::bool_val(true));
                    }
                }
                Ok(PyObject::bool_val(false))
            } else {
                Ok(PyObject::not_implemented())
            }
        }
        "__repr__" | "__str__" => {
            let parts: Vec<String> = items.iter().map(|x| x.repr()).collect();
            if items.len() == 1 {
                Ok(PyObject::str_val(CompactString::from(format!(
                    "({},)",
                    parts[0]
                ))))
            } else {
                Ok(PyObject::str_val(CompactString::from(format!(
                    "({})",
                    parts.join(", ")
                ))))
            }
        }
        "__bool__" => Ok(PyObject::bool_val(!items.is_empty())),
        "__hash__" => {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            for item in items {
                if let Ok(hk) = item.to_hashable_key() {
                    hk.hash(&mut hasher);
                }
            }
            Ok(PyObject::int(hasher.finish() as i64))
        }
        "__sizeof__" => Ok(PyObject::int(
            (std::mem::size_of::<Vec<PyObjectRef>>()
                + items.len() * std::mem::size_of::<PyObjectRef>()) as i64,
        )),
        _ => Err(PyException::attribute_error(format!(
            "'tuple' object has no attribute '{}'",
            method
        ))),
    }
}
