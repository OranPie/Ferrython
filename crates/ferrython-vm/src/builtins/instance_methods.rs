//! Instance method dispatch for stdlib types (deque, CSV, hashlib, IO, pathlib, datetime, queue).
//!
//! These are method handlers for Python objects that are implemented as Instance
//! payloads with special marker attributes (e.g., __deque__, __stringio__).

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{ FxHashKeyMap, new_fx_hashkey_map, PyCell, 
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, InstanceData,
    NativeFunctionData, CompareOp, check_args_min, SharedFxAttrMap,
};
use ferrython_core::types::{HashableKey, PyInt};
use indexmap::IndexMap;
use std::rc::Rc;

use super::core_fns::{builtin_dict_fromkeys, builtin_type};
use super::dispatch;

pub(super) fn call_namedtuple_method(inst: &ferrython_core::object::InstanceData, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "_asdict" => {
            if let Some(fields) = inst.class.get_attr("_fields") {
                if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                    let mut map = IndexMap::new();
                    let attrs = inst.attrs.read();
                    for field in field_names {
                        let name = field.py_to_string();
                        let val = attrs.get(name.as_str()).cloned().unwrap_or_else(PyObject::none);
                        map.insert(HashableKey::Str(CompactString::from(name.as_str())), val);
                    }
                    return Ok(PyObject::dict(map));
                }
            }
            Ok(PyObject::dict(new_fx_hashkey_map()))
        }
        "_replace" => {
            // _replace(**kwargs) — create a new instance with some fields replaced
            // In our dispatch, kwargs are passed as a trailing dict argument
            let kwargs_dict = if !args.is_empty() {
                if let PyObjectPayload::Dict(map) = &args[0].payload {
                    Some(map.read().clone())
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(fields) = inst.class.get_attr("_fields") {
                if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                    let attrs = inst.attrs.read();
                    let mut new_values: Vec<PyObjectRef> = Vec::new();
                    for field in field_names {
                        let name = field.py_to_string();
                        let hk = HashableKey::Str(CompactString::from(name.as_str()));
                        let val = if let Some(ref kw) = kwargs_dict {
                            kw.get(&hk).cloned().unwrap_or_else(|| {
                                attrs.get(name.as_str()).cloned().unwrap_or_else(PyObject::none)
                            })
                        } else {
                            attrs.get(name.as_str()).cloned().unwrap_or_else(PyObject::none)
                        };
                        new_values.push(val);
                    }
                    drop(attrs);
                    // Construct a new namedtuple instance
                    let new_inst = PyObject::instance(inst.class.clone());
                    if let PyObjectPayload::Instance(ref new_data) = new_inst.payload {
                        let mut new_attrs = new_data.attrs.write();
                        for (field, val) in field_names.iter().zip(new_values.iter()) {
                            let name = field.py_to_string();
                            new_attrs.insert(CompactString::from(name.as_str()), val.clone());
                        }
                        new_attrs.insert(CompactString::from("_tuple"), PyObject::tuple(new_values));
                    }
                    return Ok(new_inst);
                }
            }
            Ok(PyObject::none())
        }
        "_make" => {
            // _make(iterable) — create instance from iterable
            if args.is_empty() {
                return Err(PyException::type_error("_make() requires an iterable argument"));
            }
            let items = args[0].to_list()?;
            if let Some(fields) = inst.class.get_attr("_fields") {
                if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                    let new_inst = PyObject::instance(inst.class.clone());
                    if let PyObjectPayload::Instance(ref new_data) = new_inst.payload {
                        let mut new_attrs = new_data.attrs.write();
                        for (i, field) in field_names.iter().enumerate() {
                            let name = field.py_to_string();
                            let val = items.get(i).cloned().unwrap_or_else(PyObject::none);
                            new_attrs.insert(CompactString::from(name.as_str()), val);
                        }
                        new_attrs.insert(CompactString::from("_tuple"), PyObject::tuple(items));
                    }
                    return Ok(new_inst);
                }
            }
            Ok(PyObject::none())
        }
        "__len__" => {
            if let Some(tup) = inst.attrs.read().get("_tuple") {
                if let PyObjectPayload::Tuple(items) = &tup.payload {
                    return Ok(PyObject::int(items.len() as i64));
                }
            }
            Ok(PyObject::int(0))
        }
        "__iter__" => {
            if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                if let PyObjectPayload::Tuple(items) = &tup.payload {
                    return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                        Rc::new(PyCell::new(
                            ferrython_core::object::IteratorData::Tuple { items: items.clone(), index: 0 }
                        ))
                    )));
                }
            }
            Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Rc::new(PyCell::new(
                    ferrython_core::object::IteratorData::Tuple { items: vec![], index: 0 }
                ))
            )))
        }
        "__repr__" | "__str__" => {
            let typename = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                cd.name.to_string()
            } else {
                "namedtuple".to_string()
            };
            if let Some(fields) = inst.class.get_attr("_fields") {
                if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                    let attrs = inst.attrs.read();
                    let parts: Vec<String> = field_names.iter()
                        .map(|f| {
                            let name = f.py_to_string();
                            let val = attrs.get(name.as_str()).cloned().unwrap_or_else(PyObject::none);
                            format!("{}={}", name, val.py_to_string())
                        })
                        .collect();
                    return Ok(PyObject::str_val(CompactString::from(format!("{}({})", typename, parts.join(", ")))));
                }
            }
            Ok(PyObject::str_val(CompactString::from(format!("{}()", typename))))
        }
        "__eq__" => {
            // Compare namedtuple instances by their _tuple values
            if args.is_empty() { return Ok(PyObject::bool_val(false)); }
            let other = &args[0];
            let self_tuple = inst.attrs.read().get("_tuple").cloned();
            let other_tuple = other.get_attr("_tuple");
            if let (Some(st), Some(ot)) = (self_tuple, other_tuple) {
                if let (PyObjectPayload::Tuple(a), PyObjectPayload::Tuple(b)) = (&st.payload, &ot.payload) {
                    if a.len() != b.len() { return Ok(PyObject::bool_val(false)); }
                    for (av, bv) in a.iter().zip(b.iter()) {
                        if !av.compare(bv, ferrython_core::object::CompareOp::Eq)?.is_truthy() {
                            return Ok(PyObject::bool_val(false));
                        }
                    }
                    return Ok(PyObject::bool_val(true));
                }
            }
            Ok(PyObject::bool_val(false))
        }
        "__hash__" => {
            // Simple hash based on field count
            if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                if let PyObjectPayload::Tuple(items) = &tup.payload {
                    return Ok(PyObject::int(items.len() as i64 * 31));
                }
            }
            Ok(PyObject::int(0))
        }
        "__contains__" => {
            if args.is_empty() { return Ok(PyObject::bool_val(false)); }
            if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                return Ok(PyObject::bool_val(tup.contains(&args[0])?));
            }
            Ok(PyObject::bool_val(false))
        }
        "__getitem__" => {
            if args.is_empty() { return Err(PyException::type_error("__getitem__ requires an argument")); }
            if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                return tup.get_item(&args[0]);
            }
            Err(PyException::index_error("index out of range"))
        }
        _ => Err(PyException::attribute_error(format!("namedtuple has no attribute '{}'", method))),
    }
}

pub(super) fn call_deque_method(inst: &ferrython_core::object::InstanceData, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let get_data = || -> PyObjectRef {
        inst.attrs.read().get("_data").cloned().unwrap_or_else(|| PyObject::list(vec![]))
    };
    let get_maxlen = || -> Option<usize> {
        inst.attrs.read().get("__maxlen__").and_then(|v| v.as_int()).map(|n| n as usize)
    };
    // Helper: enforce maxlen by trimming from the appropriate end
    let enforce_maxlen_right = |list: &PyCell<Vec<PyObjectRef>>| {
        if let Some(ml) = get_maxlen() {
            let mut v = list.write();
            while v.len() > ml {
                v.remove(0); // trim from left when appending to right
            }
        }
    };
    let enforce_maxlen_left = |list: &PyCell<Vec<PyObjectRef>>| {
        if let Some(ml) = get_maxlen() {
            let mut v = list.write();
            while v.len() > ml {
                v.pop(); // trim from right when appending to left
            }
        }
    };
    match method {
        "append" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                list.write().push(args[0].clone());
                enforce_maxlen_right(list);
            }
            Ok(PyObject::none())
        }
        "appendleft" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                list.write().insert(0, args[0].clone());
                enforce_maxlen_left(list);
            }
            Ok(PyObject::none())
        }
        "pop" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let mut v = list.write();
                if v.is_empty() {
                    return Err(PyException::new(ExceptionKind::IndexError, "pop from an empty deque"));
                }
                return Ok(v.pop().unwrap());
            }
            Ok(PyObject::none())
        }
        "popleft" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let mut v = list.write();
                if v.is_empty() {
                    return Err(PyException::new(ExceptionKind::IndexError, "pop from an empty deque"));
                }
                return Ok(v.remove(0));
            }
            Ok(PyObject::none())
        }
        "extend" => {
            // args[0] should be pre-collected items as a List (VM collects iterable before calling)
            let items = args[0].to_list().unwrap_or_default();
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                list.write().extend(items);
                enforce_maxlen_right(list);
            }
            Ok(PyObject::none())
        }
        "extendleft" => {
            let items = args[0].to_list().unwrap_or_default();
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let mut v = list.write();
                // CPython: appendleft each item in order — insert(0) naturally reverses
                for item in items.into_iter() {
                    v.insert(0, item);
                }
                drop(v);
                enforce_maxlen_left(list);
            }
            Ok(PyObject::none())
        }
        "rotate" => {
            let n = if args.is_empty() { 1i64 } else { args[0].as_int().unwrap_or(1) };
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let mut v = list.write();
                let len = v.len() as i64;
                if len > 0 {
                    let n = ((n % len) + len) % len;
                    let split = v.len() - n as usize;
                    let tail: Vec<_> = v.drain(split..).collect();
                    for (i, item) in tail.into_iter().enumerate() {
                        v.insert(i, item);
                    }
                }
            }
            Ok(PyObject::none())
        }
        "clear" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                list.write().clear();
            }
            Ok(PyObject::none())
        }
        "copy" => {
            let data = get_data();
            let items = data.to_list()?;
            let maxlen_obj = inst.attrs.read().get("__maxlen__").cloned().unwrap_or_else(PyObject::none);
            dispatch("deque", &[PyObject::list(items), maxlen_obj])
        }
        "count" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let v = list.read();
                let count = v.iter().filter(|x| x.py_to_string() == args[0].py_to_string()).count();
                return Ok(PyObject::int(count as i64));
            }
            Ok(PyObject::int(0))
        }
        "index" => {
            if args.is_empty() {
                return Err(PyException::type_error("index() requires at least 1 argument"));
            }
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let v = list.read();
                let target = args[0].py_to_string();
                let start = if args.len() > 1 { args[1].as_int().unwrap_or(0) as usize } else { 0 };
                let stop = if args.len() > 2 { args[2].as_int().unwrap_or(v.len() as i64) as usize } else { v.len() };
                for i in start..stop.min(v.len()) {
                    if v[i].py_to_string() == target {
                        return Ok(PyObject::int(i as i64));
                    }
                }
                return Err(PyException::new(ExceptionKind::ValueError, format!("{} is not in deque", args[0].py_to_string())));
            }
            Err(PyException::new(ExceptionKind::ValueError, "deque index error"))
        }
        "insert" => {
            if args.len() < 2 {
                return Err(PyException::type_error("insert() requires 2 arguments"));
            }
            if let Some(ml) = get_maxlen() {
                let data = get_data();
                if let PyObjectPayload::List(list) = &data.payload {
                    if list.read().len() >= ml {
                        return Err(PyException::new(ExceptionKind::IndexError, "deque already at its maximum size"));
                    }
                }
            }
            let idx = args[0].to_int()? as usize;
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let mut v = list.write();
                let idx = idx.min(v.len());
                v.insert(idx, args[1].clone());
            }
            Ok(PyObject::none())
        }
        "remove" => {
            if args.is_empty() {
                return Err(PyException::type_error("remove() requires 1 argument"));
            }
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let mut v = list.write();
                let target = args[0].py_to_string();
                if let Some(pos) = v.iter().position(|x| x.py_to_string() == target) {
                    v.remove(pos);
                    return Ok(PyObject::none());
                }
                return Err(PyException::new(ExceptionKind::ValueError, "deque.remove(x): x not in deque"));
            }
            Ok(PyObject::none())
        }
        "reverse" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                list.write().reverse();
            }
            Ok(PyObject::none())
        }
        "maxlen" => {
            // Property-like access: return maxlen value
            let ml = inst.attrs.read().get("__maxlen__").cloned().unwrap_or_else(PyObject::none);
            Ok(ml)
        }
        "__len__" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                return Ok(PyObject::int(list.read().len() as i64));
            }
            Ok(PyObject::int(0))
        }
        "__contains__" => {
            if args.is_empty() { return Ok(PyObject::bool_val(false)); }
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let v = list.read();
                let target = args[0].py_to_string();
                return Ok(PyObject::bool_val(v.iter().any(|x| x.py_to_string() == target)));
            }
            Ok(PyObject::bool_val(false))
        }
        "__getitem__" => {
            if args.is_empty() {
                return Err(PyException::type_error("__getitem__() requires 1 argument"));
            }
            let idx = args[0].to_int()?;
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let v = list.read();
                let len = v.len() as i64;
                let actual_idx = if idx < 0 { len + idx } else { idx };
                if actual_idx < 0 || actual_idx >= len {
                    return Err(PyException::new(ExceptionKind::IndexError, "deque index out of range"));
                }
                return Ok(v[actual_idx as usize].clone());
            }
            Err(PyException::new(ExceptionKind::IndexError, "deque index out of range"))
        }
        "__iter__" => {
            Ok(get_data())
        }
        _ => Err(PyException::attribute_error(format!("deque has no attribute '{}'", method))),
    }
}

pub(super) fn call_instance_dict_method(
    attrs: &SharedFxAttrMap,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match method {
        "get" => {
            check_args_min("get", args, 1)?;
            let key_str = args[0].py_to_string();
            let default = if args.len() >= 2 { args[1].clone() } else { PyObject::none() };
            Ok(attrs.read().get(key_str.as_str()).cloned().unwrap_or(default))
        }
        "keys" => {
            let guard = attrs.read();
            let keys: Vec<PyObjectRef> = guard.keys()
                .map(|k| PyObject::str_val(k.clone()))
                .collect();
            Ok(PyObject::list(keys))
        }
        "values" => {
            let guard = attrs.read();
            let vals: Vec<PyObjectRef> = guard.values().cloned().collect();
            Ok(PyObject::list(vals))
        }
        "items" => {
            let guard = attrs.read();
            let items: Vec<PyObjectRef> = guard.iter()
                .map(|(k, v)| PyObject::tuple(vec![PyObject::str_val(k.clone()), v.clone()]))
                .collect();
            Ok(PyObject::list(items))
        }
        "__contains__" => {
            check_args_min("__contains__", args, 1)?;
            let key_str = args[0].py_to_string();
            Ok(PyObject::bool_val(attrs.read().contains_key(key_str.as_str())))
        }
        "pop" => {
            check_args_min("pop", args, 1)?;
            let key_str = CompactString::from(args[0].py_to_string());
            let default = if args.len() >= 2 { Some(args[1].clone()) } else { None };
            match attrs.write().shift_remove(&key_str) {
                Some(v) => Ok(v),
                None => match default {
                    Some(d) => Ok(d),
                    None => Err(PyException::key_error(args[0].repr())),
                },
            }
        }
        "update" => {
            check_args_min("update", args, 1)?;
            if let PyObjectPayload::Dict(other) = &args[0].payload {
                let other_items = other.read().clone();
                let mut w = attrs.write();
                for (k, v) in other_items {
                    w.insert(CompactString::from(k.to_object().py_to_string()), v);
                }
            } else if let PyObjectPayload::InstanceDict(other) = &args[0].payload {
                let other_items: Vec<_> = other.read().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                let mut w = attrs.write();
                for (k, v) in other_items {
                    w.insert(k, v);
                }
            }
            Ok(PyObject::none())
        }
        "copy" => {
            let guard = attrs.read();
            let copy: FxHashKeyMap = guard.iter()
                .map(|(k, v)| (HashableKey::Str(k.clone()), v.clone()))
                .collect();
            Ok(PyObject::dict(copy))
        }
        "clear" => {
            attrs.write().clear();
            Ok(PyObject::none())
        }
        "setdefault" => {
            check_args_min("setdefault", args, 1)?;
            let key = CompactString::from(args[0].py_to_string());
            let default = if args.len() >= 2 { args[1].clone() } else { PyObject::none() };
            let mut w = attrs.write();
            if let Some(v) = w.get(key.as_str()) {
                Ok(v.clone())
            } else {
                w.insert(key, default.clone());
                Ok(default)
            }
        }
        _ => Err(PyException::attribute_error(format!("'dict' object has no attribute '{}'", method))),
    }
}

pub(super) fn call_csv_writer_method(inst: &ferrython_core::object::InstanceData, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let attrs = inst.attrs.read();
    let fileobj = attrs.get("_fileobj").cloned().unwrap_or_else(PyObject::none);
    let rows = attrs.get("_rows").cloned().unwrap_or_else(|| PyObject::list(vec![]));
    drop(attrs);

    match method {
        "writerow" => {
            if args.is_empty() { return Err(PyException::type_error("writerow() requires a sequence")); }
            let items = args[0].to_list()?;
            let fields: Vec<String> = items.iter().map(|item| {
                let s = item.py_to_string();
                if s.contains(',') || s.contains('"') || s.contains('\n') {
                    format!("\"{}\"", s.replace('"', "\"\""))
                } else {
                    s
                }
            }).collect();
            let line = format!("{}\r\n", fields.join(","));
            // Write to the file object's write method or accumulate in _rows
            if let PyObjectPayload::Instance(fobj_inst) = &fileobj.payload {
                // StringIO write
                if fobj_inst.attrs.read().contains_key("__stringio__") {
                    let mut fobj_attrs = fobj_inst.attrs.write();
                    if let Some(buf) = fobj_attrs.get("_buffer") {
                        let existing = buf.py_to_string();
                        fobj_attrs.insert(CompactString::from("_buffer"),
                            PyObject::str_val(CompactString::from(format!("{}{}", existing, line))));
                    }
                }
            }
            // Also store in _rows
            if let PyObjectPayload::List(row_list) = &rows.payload {
                row_list.write().push(PyObject::str_val(CompactString::from(&line)));
            }
            Ok(PyObject::none())
        }
        "writerows" => {
            if args.is_empty() { return Err(PyException::type_error("writerows() requires an iterable")); }
            let rows_list = args[0].to_list()?;
            for row in rows_list {
                // Recursively call writerow
                call_csv_writer_method(inst, "writerow", &[row])?;
            }
            Ok(PyObject::none())
        }
        _ => Err(PyException::attribute_error(format!("'csv.writer' object has no attribute '{}'", method))),
    }
}

pub(super) fn call_csv_dictwriter_method(inst: &ferrython_core::object::InstanceData, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let attrs = inst.attrs.read();
    let fileobj = attrs.get("_fileobj").cloned().unwrap_or_else(PyObject::none);
    let fieldnames = attrs.get("_fieldnames").cloned().unwrap_or_else(|| PyObject::list(vec![]));
    drop(attrs);

    let field_list = fieldnames.to_list()?;
    let names: Vec<String> = field_list.iter().map(|f| f.py_to_string()).collect();

    match method {
        "writeheader" => {
            let line = format!("{}\r\n", names.join(","));
            if let PyObjectPayload::Instance(fobj_inst) = &fileobj.payload {
                if fobj_inst.attrs.read().contains_key("__stringio__") {
                    let mut fobj_attrs = fobj_inst.attrs.write();
                    if let Some(buf) = fobj_attrs.get("_buffer") {
                        let existing = buf.py_to_string();
                        fobj_attrs.insert(CompactString::from("_buffer"),
                            PyObject::str_val(CompactString::from(format!("{}{}", existing, line))));
                    }
                }
            }
            Ok(PyObject::none())
        }
        "writerow" => {
            if args.is_empty() { return Err(PyException::type_error("writerow() requires a dict")); }
            let row_dict = &args[0];
            let mut fields = Vec::new();
            for name in &names {
                // Dict key lookup first (avoids clashing with dict method names like "pop")
                let val = if let PyObjectPayload::Dict(map) = &row_dict.payload {
                    map.read().get(&HashableKey::Str(CompactString::from(name.as_str())))
                        .cloned().unwrap_or_else(PyObject::none)
                } else if let Some(v) = row_dict.get_attr(name) {
                    v
                } else {
                    PyObject::none()
                };
                let s = val.py_to_string();
                if s.contains(',') || s.contains('"') || s.contains('\n') {
                    fields.push(format!("\"{}\"", s.replace('"', "\"\"")));
                } else {
                    fields.push(s);
                }
            }
            let line = format!("{}\r\n", fields.join(","));
            if let PyObjectPayload::Instance(fobj_inst) = &fileobj.payload {
                if fobj_inst.attrs.read().contains_key("__stringio__") {
                    let mut fobj_attrs = fobj_inst.attrs.write();
                    if let Some(buf) = fobj_attrs.get("_buffer") {
                        let existing = buf.py_to_string();
                        fobj_attrs.insert(CompactString::from("_buffer"),
                            PyObject::str_val(CompactString::from(format!("{}{}", existing, line))));
                    }
                }
            }
            Ok(PyObject::none())
        }
        "writerows" => {
            if args.is_empty() { return Err(PyException::type_error("writerows() requires an iterable")); }
            let rows = args[0].to_list()?;
            for row in rows.iter() {
                call_csv_dictwriter_method(inst, "writerow", &[row.clone()])?;
            }
            Ok(PyObject::none())
        }
        _ => Err(PyException::attribute_error(format!("'csv.DictWriter' object has no attribute '{}'", method))),
    }
}

pub(super) fn compute_hash_digest(algo: &str, data: &[u8]) -> (String, Vec<u8>) {
    use digest::Digest;
    match algo {
        "md5" => {
            let mut h = md5::Md5::new();
            h.update(data);
            let r = h.finalize();
            (r.iter().map(|b| format!("{:02x}", b)).collect(), r.to_vec())
        }
        "sha1" => {
            let mut h = sha1::Sha1::new();
            h.update(data);
            let r = h.finalize();
            (r.iter().map(|b| format!("{:02x}", b)).collect(), r.to_vec())
        }
        "sha224" => {
            let mut h = sha2::Sha224::new();
            h.update(data);
            let r = h.finalize();
            (r.iter().map(|b| format!("{:02x}", b)).collect(), r.to_vec())
        }
        "sha384" => {
            let mut h = sha2::Sha384::new();
            h.update(data);
            let r = h.finalize();
            (r.iter().map(|b| format!("{:02x}", b)).collect(), r.to_vec())
        }
        "sha512" => {
            let mut h = sha2::Sha512::new();
            h.update(data);
            let r = h.finalize();
            (r.iter().map(|b| format!("{:02x}", b)).collect(), r.to_vec())
        }
        _ => {
            // Default to sha256
            let mut h = sha2::Sha256::new();
            h.update(data);
            let r = h.finalize();
            (r.iter().map(|b| format!("{:02x}", b)).collect(), r.to_vec())
        }
    }
}

pub(super) fn call_hashlib_method(inst: &ferrython_core::object::InstanceData, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "update" => {
            // Append data to _data buffer, recompute digest lazily
            if args.is_empty() {
                return Err(PyException::type_error("update() takes exactly 1 argument"));
            }
            let new_data = match &args[0].payload {
                PyObjectPayload::Bytes(b) => b.clone(),
                PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                _ => return Err(PyException::type_error("a bytes-like object is required")),
            };
            let mut w = inst.attrs.write();
            // Append to accumulated data
            let mut accumulated = if let Some(d) = w.get("_data") {
                if let PyObjectPayload::Bytes(b) = &d.payload { b.clone() } else { vec![] }
            } else {
                vec![]
            };
            accumulated.extend_from_slice(&new_data);
            w.insert(CompactString::from("_data"), PyObject::bytes(accumulated.clone()));
            // Recompute digest
            let algo = if let Some(n) = w.get("name") { n.py_to_string() } else { String::from("sha256") };
            let (hex, digest_bytes) = compute_hash_digest(&algo, &accumulated);
            w.insert(CompactString::from("_hexdigest"), PyObject::str_val(CompactString::from(&hex)));
            w.insert(CompactString::from("_digest"), PyObject::bytes(digest_bytes));
            Ok(PyObject::none())
        }
        "hexdigest" => {
            let attrs = inst.attrs.read();
            if let Some(hd) = attrs.get("_hexdigest") {
                return Ok(hd.clone());
            }
            Ok(PyObject::str_val(CompactString::from("")))
        }
        "digest" => {
            let attrs = inst.attrs.read();
            if let Some(d) = attrs.get("_digest") {
                return Ok(d.clone());
            }
            Ok(PyObject::bytes(vec![]))
        }
        "copy" => {
            // Return a new hash object with same state
            let attrs = inst.attrs.read();
            let cls = inst.class.clone();
            let class_flags = InstanceData::compute_flags(&cls);
            let new_inst = PyObject::wrap(PyObjectPayload::Instance(Box::new(InstanceData {
                class: cls,
                attrs: Rc::new(PyCell::new(attrs.clone())),
                is_special: true, dict_storage: None,
                class_flags,
            })));
            Ok(new_inst)
        }
        _ => {
            let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload { cd.name.to_string() } else { "hash".to_string() };
            Err(PyException::attribute_error(format!("'{}' object has no attribute '{}'", class_name, method)))
        }
    }
}

/// Resolve class-level methods on builtin types (e.g., dict.fromkeys, int.from_bytes).
pub fn resolve_type_class_method(type_name: &str, method_name: &str) -> Option<PyObjectRef> {
    match (type_name, method_name) {
        ("dict", "fromkeys") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(NativeFunctionData {
            name: CompactString::from("dict.fromkeys"),
            func: builtin_dict_fromkeys,
        })))),
        ("int", "from_bytes") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(NativeFunctionData {
            name: CompactString::from("int.from_bytes"),
            func: builtin_int_from_bytes,
        })))),
        ("str", "maketrans") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(NativeFunctionData {
            name: CompactString::from("str.maketrans"),
            func: builtin_str_maketrans,
        })))),
        ("bytes", "fromhex") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(NativeFunctionData {
            name: CompactString::from("bytes.fromhex"),
            func: builtin_bytes_fromhex,
        })))),
        ("bytes", "maketrans") | ("bytearray", "maketrans") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(NativeFunctionData {
            name: CompactString::from("bytes.maketrans"),
            func: builtin_bytes_maketrans,
        })))),
        ("object", "__getattribute__") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(NativeFunctionData {
            name: CompactString::from("object.__getattribute__"),
            func: builtin_object_getattribute,
        })))),
        ("object", "__setattr__") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(NativeFunctionData {
            name: CompactString::from("object.__setattr__"),
            func: builtin_object_setattr,
        })))),
        ("object", "__delattr__") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(NativeFunctionData {
            name: CompactString::from("object.__delattr__"),
            func: builtin_object_delattr,
        })))),
        ("type", "__new__") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(NativeFunctionData {
            name: CompactString::from("type.__new__"),
            func: builtin_type,
        })))),
        ("float", "fromhex") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(NativeFunctionData {
            name: CompactString::from("float.fromhex"),
            func: builtin_float_fromhex,
        })))),
        // property descriptor methods: property.__get__(self, obj, type)
        ("property", "__get__") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(NativeFunctionData {
            name: CompactString::from("property.__get__"),
            func: |args: &[PyObjectRef]| {
                // property.__get__(self, obj, objtype=None)
                // self is the property object, obj is the instance
                if args.is_empty() {
                    return Err(PyException::type_error("descriptor '__get__' requires a property object"));
                }
                let prop = &args[0];
                let obj = args.get(1);
                // If obj is None or not provided, return the property itself
                let obj = match obj {
                    Some(o) if !matches!(&o.payload, PyObjectPayload::None) => o,
                    _ => return Ok(prop.clone()),
                };
                // Get the fget from the property
                if let PyObjectPayload::Property(pd) = &prop.payload {
                    if let Some(getter) = pd.fget.as_ref() {
                        let getter = crate::builtins::core_fns::unwrap_abstract_fget(getter);
                        return Ok(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: obj.clone(),
                                method: getter,
                            }
                        }));
                    }
                    return Err(PyException::attribute_error("unreadable attribute"));
                }
                // For InstanceProperty (subclass of property), look for fget in instance attrs
                if let PyObjectPayload::Instance(inst) = &prop.payload {
                    if let Some(fget) = inst.attrs.read().get("fget").cloned() {
                        return Ok(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: obj.clone(),
                                method: fget,
                            }
                        }));
                    }
                }
                Ok(prop.clone())
            },
        })))),
        ("property", "__init__") => Some(PyObject::wrap(PyObjectPayload::NativeFunction(Box::new(NativeFunctionData {
            name: CompactString::from("property.__init__"),
            func: |args: &[PyObjectRef]| {
                // property.__init__(self, fget=None, fset=None, fdel=None, doc=None)
                // Store fget/fset/fdel on the instance so subclasses work
                if args.is_empty() {
                    return Ok(PyObject::none());
                }
                let fget = args.get(1).cloned();
                let fset = args.get(2).cloned();
                let fdel = args.get(3).cloned();
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    let mut w = inst.attrs.write();
                    if let Some(f) = &fget { w.insert(CompactString::from("fget"), f.clone()); }
                    if let Some(f) = &fset { w.insert(CompactString::from("fset"), f.clone()); }
                    if let Some(f) = &fdel { w.insert(CompactString::from("fdel"), f.clone()); }
                }
                Ok(PyObject::none())
            },
        })))),
        _ => None,
    }
}

pub(super) fn builtin_int_from_bytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("int.from_bytes requires at least 1 argument"));
    }
    let bytes = match &args[0].payload {
        PyObjectPayload::Bytes(b) => b.clone(),
        _ => return Err(PyException::type_error("expected bytes")),
    };
    // Extract byteorder and signed from positional or kwargs dict
    let mut byteorder = "big".to_string();
    let mut signed = false;
    // Check if last arg is a kwargs dict
    if let Some(last) = args.last() {
        if args.len() >= 2 {
            if let PyObjectPayload::Dict(map) = &last.payload {
                let map_r = map.read();
                if let Some(bo) = map_r.get(&HashableKey::Str(CompactString::from("byteorder"))) {
                    byteorder = bo.py_to_string();
                }
                if let Some(s) = map_r.get(&HashableKey::Str(CompactString::from("signed"))) {
                    signed = s.is_truthy();
                }
            } else {
                byteorder = args[1].py_to_string();
            }
        }
    }
    // Also check positional arg 2 for signed (if not from kwargs)
    if args.len() >= 3 && !matches!(&args[2].payload, PyObjectPayload::Dict(_)) {
        signed = args[2].is_truthy();
    }
    let mut result: i64 = 0;
    match byteorder.as_str() {
        "big" => {
            for &b in &bytes {
                result = result * 256 + b as i64;
            }
        }
        "little" => {
            for (i, &b) in bytes.iter().enumerate() {
                result += (b as i64) << (8 * i);
            }
        }
        _ => return Err(PyException::value_error("byteorder must be 'big' or 'little'")),
    }
    if signed {
        let bits = bytes.len() * 8;
        let sign_bit = 1i64 << (bits - 1);
        if result & sign_bit != 0 {
            result -= 1i64 << bits;
        }
    }
    Ok(PyObject::int(result))
}

pub(super) fn builtin_str_maketrans(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("str.maketrans requires at least 1 argument"));
    }
    let mut map = IndexMap::new();
    if args.len() >= 2 {
        let from = args[0].py_to_string();
        let to = args[1].py_to_string();
        for (fc, tc) in from.chars().zip(to.chars()) {
            map.insert(
                HashableKey::Int(PyInt::Small(fc as i64)),
                PyObject::str_val(CompactString::from(tc.to_string())),
            );
        }
        if args.len() >= 3 {
            let delete = args[2].py_to_string();
            for c in delete.chars() {
                map.insert(HashableKey::Int(PyInt::Small(c as i64)), PyObject::none());
            }
        }
    } else if let PyObjectPayload::Dict(d) = &args[0].payload {
        let r = d.read();
        for (k, v) in r.iter() {
            map.insert(k.clone(), v.clone());
        }
    }
    Ok(PyObject::dict(map))
}

pub(super) fn builtin_bytes_fromhex(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("bytes.fromhex requires 1 argument"));
    }
    let hex_str = args[0].py_to_string();
    let clean: String = hex_str.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.len() % 2 != 0 {
        return Err(PyException::value_error("non-hexadecimal number found in fromhex() arg"));
    }
    let mut bytes = Vec::new();
    for i in (0..clean.len()).step_by(2) {
        match u8::from_str_radix(&clean[i..i+2], 16) {
            Ok(b) => bytes.push(b),
            Err(_) => return Err(PyException::value_error("non-hexadecimal number found in fromhex() arg")),
        }
    }
    Ok(PyObject::bytes(bytes))
}

pub(super) fn builtin_bytes_maketrans(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("maketrans requires 2 arguments"));
    }
    let from_bytes = match &args[0].payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => b.clone(),
        _ => return Err(PyException::type_error("a bytes-like object is required")),
    };
    let to_bytes = match &args[1].payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => b.clone(),
        _ => return Err(PyException::type_error("a bytes-like object is required")),
    };
    if from_bytes.len() != to_bytes.len() {
        return Err(PyException::value_error("maketrans arguments must have same length"));
    }
    let mut table: Vec<u8> = (0..=255u8).collect();
    for (f, t) in from_bytes.iter().zip(to_bytes.iter()) {
        table[*f as usize] = *t;
    }
    Ok(PyObject::bytes(table))
}

pub(super) fn builtin_float_fromhex(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("float.fromhex requires 1 argument"));
    }
    let hex_str = args[0].py_to_string().trim().to_lowercase();
    // Handle special values
    match hex_str.as_str() {
        "inf" | "+inf" | "infinity" | "+infinity" => return Ok(PyObject::float(f64::INFINITY)),
        "-inf" | "-infinity" => return Ok(PyObject::float(f64::NEG_INFINITY)),
        "nan" | "+nan" | "-nan" => return Ok(PyObject::float(f64::NAN)),
        _ => {}
    }
    // Parse hex float format: [sign] "0x" hex_mantissa "p" exp
    let (sign, rest) = if hex_str.starts_with('-') {
        (-1.0f64, &hex_str[1..])
    } else if hex_str.starts_with('+') {
        (1.0, &hex_str[1..])
    } else {
        (1.0, hex_str.as_str())
    };
    let rest = rest.strip_prefix("0x").unwrap_or(rest);
    if let Some(p_idx) = rest.find('p') {
        let mantissa_str = &rest[..p_idx];
        let exp: i32 = rest[p_idx + 1..].parse().map_err(|_|
            PyException::value_error("invalid hexadecimal floating-point string"))?;
        let (int_part, frac_part) = if let Some(dot) = mantissa_str.find('.') {
            (&mantissa_str[..dot], &mantissa_str[dot + 1..])
        } else {
            (mantissa_str, "")
        };
        let int_val = i64::from_str_radix(int_part, 16).unwrap_or(0);
        let frac_val: f64 = if frac_part.is_empty() {
            0.0
        } else {
            let frac_int = i64::from_str_radix(frac_part, 16).unwrap_or(0);
            frac_int as f64 / (16.0f64).powi(frac_part.len() as i32)
        };
        let value = sign * (int_val as f64 + frac_val) * (2.0f64).powi(exp);
        Ok(PyObject::float(value))
    } else {
        Err(PyException::value_error("invalid hexadecimal floating-point string"))
    }
}
pub(super) fn builtin_object_getattribute(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("object.__getattribute__ requires 2 arguments"));
    }
    let obj = &args[0];
    let name = args[1].py_to_string();
    match obj.get_attr(&name) {
        Some(v) => Ok(v),
        None => Err(PyException::attribute_error(format!(
            "'{}' object has no attribute '{}'", obj.type_name(), name
        ))),
    }
}

/// object.__setattr__(self, name, value)
pub(super) fn builtin_object_setattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error("object.__setattr__ requires 3 arguments"));
    }
    let obj = &args[0];
    let name = args[1].py_to_string();
    let value = args[2].clone();
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        inst.attrs.write().insert(CompactString::from(name), value);
        Ok(PyObject::none())
    } else if let PyObjectPayload::ExceptionInstance(ei) = &obj.payload {
        ei.ensure_attrs().write().insert(CompactString::from(name), value);
        Ok(PyObject::none())
    } else if let PyObjectPayload::Function(f) = &obj.payload {
        f.attrs.write().insert(CompactString::from(name), value);
        Ok(PyObject::none())
    } else if matches!(&obj.payload, PyObjectPayload::NativeFunction(_) | PyObjectPayload::NativeClosure(_) | PyObjectPayload::BuiltinFunction(_)) {
        // Silently accept for native functions
        Ok(PyObject::none())
    } else {
        Err(PyException::attribute_error(format!(
            "'{}' object does not support attribute assignment", obj.type_name()
        )))
    }
}

/// object.__delattr__(self, name)
pub(super) fn builtin_object_delattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("object.__delattr__ requires 2 arguments"));
    }
    let obj = &args[0];
    let name = args[1].py_to_string();
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        if inst.attrs.write().swap_remove(name.as_str()).is_some() {
            Ok(PyObject::none())
        } else {
            Err(PyException::attribute_error(format!(
                "'{}' object has no attribute '{}'", obj.type_name(), name
            )))
        }
    } else {
        Err(PyException::attribute_error(format!(
            "'{}' object does not support attribute deletion", obj.type_name()
        )))
    }
}

// ── StringIO methods ──

pub(super) fn call_stringio_method(inst: &ferrython_core::object::InstanceData, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "write" => {
            check_args_min("write", args, 1)?;
            let text = args[0].py_to_string();
            let len = text.len() as i64;
            let mut attrs = inst.attrs.write();
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) as usize;
            let mut buf = attrs.get("_buffer").map(|b| b.py_to_string()).unwrap_or_default();
            // Insert/overwrite at position
            if pos >= buf.len() {
                buf.push_str(&text);
            } else {
                let end = (pos + text.len()).min(buf.len());
                buf.replace_range(pos..end, &text);
                if pos + text.len() > end {
                    buf.push_str(&text[end - pos..]);
                }
            }
            let new_pos = pos + text.len();
            attrs.insert(CompactString::from("_buffer"), PyObject::str_val(CompactString::from(&buf)));
            attrs.insert(CompactString::from("_pos"), PyObject::int(new_pos as i64));
            Ok(PyObject::int(len))
        }
        "read" => {
            let mut attrs = inst.attrs.write();
            let buf = attrs.get("_buffer").map(|b| b.py_to_string()).unwrap_or_default();
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) as usize;
            let n = if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None) {
                args[0].as_int().unwrap_or(-1)
            } else { -1 };
            let result = if n < 0 {
                buf[pos..].to_string()
            } else {
                let end = (pos + n as usize).min(buf.len());
                buf[pos..end].to_string()
            };
            let new_pos = if n < 0 { buf.len() } else { (pos + n as usize).min(buf.len()) };
            attrs.insert(CompactString::from("_pos"), PyObject::int(new_pos as i64));
            Ok(PyObject::str_val(CompactString::from(&result)))
        }
        "readline" => {
            let mut attrs = inst.attrs.write();
            let buf = attrs.get("_buffer").map(|b| b.py_to_string()).unwrap_or_default();
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) as usize;
            let remaining = &buf[pos..];
            let line = if let Some(nl) = remaining.find('\n') {
                &remaining[..=nl]
            } else {
                remaining
            };
            let result = line.to_string();
            attrs.insert(CompactString::from("_pos"), PyObject::int((pos + result.len()) as i64));
            Ok(PyObject::str_val(CompactString::from(&result)))
        }
        "readlines" => {
            let mut attrs = inst.attrs.write();
            let buf = attrs.get("_buffer").map(|b| b.py_to_string()).unwrap_or_default();
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) as usize;
            let remaining = &buf[pos..];
            let lines: Vec<PyObjectRef> = remaining.split_inclusive('\n')
                .map(|l| PyObject::str_val(CompactString::from(l)))
                .collect();
            attrs.insert(CompactString::from("_pos"), PyObject::int(buf.len() as i64));
            Ok(PyObject::list(lines))
        }
        "getvalue" => {
            let attrs = inst.attrs.read();
            let buf = attrs.get("_buffer").map(|b| b.py_to_string()).unwrap_or_default();
            Ok(PyObject::str_val(CompactString::from(&buf)))
        }
        "seek" => {
            check_args_min("seek", args, 1)?;
            let pos = args[0].as_int().unwrap_or(0);
            let whence = if args.len() >= 2 { args[1].as_int().unwrap_or(0) } else { 0 };
            let mut attrs = inst.attrs.write();
            let buf_len = attrs.get("_buffer").map(|b| b.py_to_string().len()).unwrap_or(0) as i64;
            let new_pos = match whence {
                0 => pos,                      // SEEK_SET
                1 => {                         // SEEK_CUR
                    let cur = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0);
                    cur + pos
                }
                2 => buf_len + pos,            // SEEK_END
                _ => pos,
            };
            let new_pos = new_pos.max(0);
            attrs.insert(CompactString::from("_pos"), PyObject::int(new_pos));
            Ok(PyObject::int(new_pos))
        }
        "tell" => {
            let attrs = inst.attrs.read();
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0);
            Ok(PyObject::int(pos))
        }
        "truncate" => {
            let mut attrs = inst.attrs.write();
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) as usize;
            let size = if !args.is_empty() { args[0].as_int().unwrap_or(pos as i64) as usize } else { pos };
            let buf = attrs.get("_buffer").map(|b| b.py_to_string()).unwrap_or_default();
            let truncated: String = buf.chars().take(size).collect();
            attrs.insert(CompactString::from("_buffer"), PyObject::str_val(CompactString::from(&truncated)));
            Ok(PyObject::int(size as i64))
        }
        "close" => {
            inst.attrs.write().insert(CompactString::from("_closed"), PyObject::bool_val(true));
            Ok(PyObject::none())
        }
        "closed" => {
            Ok(inst.attrs.read().get("_closed").cloned().unwrap_or_else(|| PyObject::bool_val(false)))
        }
        "__enter__" => {
            // Return self — reconstruct from instance data
            let cls = inst.class.clone();
            let inst_obj = PyObject::instance(cls);
            if let PyObjectPayload::Instance(new_inst) = &inst_obj.payload {
                *new_inst.attrs.write() = inst.attrs.read().clone();
            }
            Ok(inst_obj)
        }
        "__exit__" => {
            inst.attrs.write().insert(CompactString::from("_closed"), PyObject::bool_val(true));
            Ok(PyObject::none())
        }
        "__iter__" => {
            // StringIO is its own iterator — reconstruct self
            let cls = inst.class.clone();
            let inst_obj = PyObject::instance(cls);
            if let PyObjectPayload::Instance(new_inst) = &inst_obj.payload {
                *new_inst.attrs.write() = inst.attrs.read().clone();
            }
            Ok(inst_obj)
        }
        "__next__" => {
            // Read next line, raise StopIteration when exhausted
            let mut attrs = inst.attrs.write();
            let buf = attrs.get("_buffer").map(|b| b.py_to_string()).unwrap_or_default();
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) as usize;
            if pos >= buf.len() {
                return Err(PyException::stop_iteration());
            }
            let remaining = &buf[pos..];
            let line = if let Some(nl) = remaining.find('\n') {
                &remaining[..=nl]
            } else {
                remaining
            };
            let result = line.to_string();
            attrs.insert(CompactString::from("_pos"), PyObject::int((pos + result.len()) as i64));
            Ok(PyObject::str_val(CompactString::from(&result)))
        }
        _ => Err(PyException::attribute_error(format!("'StringIO' object has no attribute '{}'", method))),
    }
}

// ── BytesIO methods ──

pub(super) fn call_bytesio_method(inst: &ferrython_core::object::InstanceData, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "write" => {
            check_args_min("write", args, 1)?;
            let new_bytes = match &args[0].payload {
                PyObjectPayload::Bytes(b) => b.clone(),
                PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                _ => return Err(PyException::type_error("a bytes-like object is required")),
            };
            let len = new_bytes.len() as i64;
            let mut attrs = inst.attrs.write();
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) as usize;
            let mut buf = match attrs.get("_buffer").map(|b| &b.payload) {
                Some(PyObjectPayload::Bytes(b)) => b.clone(),
                _ => vec![],
            };
            // Extend if needed
            if pos + new_bytes.len() > buf.len() {
                buf.resize(pos + new_bytes.len(), 0);
            }
            buf[pos..pos + new_bytes.len()].copy_from_slice(&new_bytes);
            let new_pos = pos + new_bytes.len();
            attrs.insert(CompactString::from("_buffer"), PyObject::bytes(buf));
            attrs.insert(CompactString::from("_pos"), PyObject::int(new_pos as i64));
            Ok(PyObject::int(len))
        }
        "read" => {
            let mut attrs = inst.attrs.write();
            let buf = match attrs.get("_buffer").map(|b| &b.payload) {
                Some(PyObjectPayload::Bytes(b)) => b.clone(),
                _ => vec![],
            };
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) as usize;
            let n = if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None) {
                args[0].as_int().unwrap_or(-1)
            } else { -1 };
            let result = if n < 0 {
                buf[pos..].to_vec()
            } else {
                let end = (pos + n as usize).min(buf.len());
                buf[pos..end].to_vec()
            };
            let new_pos = if n < 0 { buf.len() } else { (pos + n as usize).min(buf.len()) };
            attrs.insert(CompactString::from("_pos"), PyObject::int(new_pos as i64));
            Ok(PyObject::bytes(result))
        }
        "getvalue" => {
            let attrs = inst.attrs.read();
            match attrs.get("_buffer").map(|b| &b.payload) {
                Some(PyObjectPayload::Bytes(b)) => Ok(PyObject::bytes(b.clone())),
                _ => Ok(PyObject::bytes(vec![])),
            }
        }
        "seek" => {
            check_args_min("seek", args, 1)?;
            let pos = args[0].as_int().unwrap_or(0);
            let whence = if args.len() >= 2 { args[1].as_int().unwrap_or(0) } else { 0 };
            let mut attrs = inst.attrs.write();
            let buf_len = match attrs.get("_buffer").map(|b| &b.payload) {
                Some(PyObjectPayload::Bytes(b)) => b.len() as i64,
                _ => 0,
            };
            let new_pos = match whence {
                0 => pos,
                1 => attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) + pos,
                2 => buf_len + pos,
                _ => pos,
            };
            let new_pos = new_pos.max(0);
            attrs.insert(CompactString::from("_pos"), PyObject::int(new_pos));
            Ok(PyObject::int(new_pos))
        }
        "tell" => {
            Ok(PyObject::int(inst.attrs.read().get("_pos").and_then(|p| p.as_int()).unwrap_or(0)))
        }
        "truncate" => {
            let mut attrs = inst.attrs.write();
            let pos = attrs.get("_pos").and_then(|p| p.as_int()).unwrap_or(0) as usize;
            let size = if !args.is_empty() { args[0].as_int().unwrap_or(pos as i64) as usize } else { pos };
            let buf = match attrs.get("_buffer").map(|b| &b.payload) {
                Some(PyObjectPayload::Bytes(b)) => b[..size.min(b.len())].to_vec(),
                _ => vec![],
            };
            attrs.insert(CompactString::from("_buffer"), PyObject::bytes(buf));
            Ok(PyObject::int(size as i64))
        }
        "close" => {
            inst.attrs.write().insert(CompactString::from("_closed"), PyObject::bool_val(true));
            Ok(PyObject::none())
        }
        "__enter__" => {
            let cls = inst.class.clone();
            let inst_obj = PyObject::instance(cls);
            if let PyObjectPayload::Instance(new_inst) = &inst_obj.payload {
                *new_inst.attrs.write() = inst.attrs.read().clone();
            }
            Ok(inst_obj)
        }
        "__exit__" => {
            inst.attrs.write().insert(CompactString::from("_closed"), PyObject::bool_val(true));
            Ok(PyObject::none())
        }
        _ => Err(PyException::attribute_error(format!("'BytesIO' object has no attribute '{}'", method))),
    }
}

// ── pathlib.Path methods ──

pub(super) fn simple_glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" { return true; }
    if !pattern.contains('*') && !pattern.contains('?') { return pattern == text; }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() { continue; }
        if let Some(idx) = text[pos..].find(part) {
            if i == 0 && idx != 0 { return false; }
            pos += idx + part.len();
        } else { return false; }
    }
    parts.last().map_or(true, |p| p.is_empty() || pos == text.len())
}

pub(super) fn call_pathlib_method(inst: &ferrython_core::object::InstanceData, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let get_path = || -> String {
        inst.attrs.read().get("_path").map(|p| p.py_to_string()).unwrap_or_else(|| ".".to_string())
    };
    match method {
        "exists" => Ok(PyObject::bool_val(std::path::Path::new(&get_path()).exists())),
        "is_file" => Ok(PyObject::bool_val(std::path::Path::new(&get_path()).is_file())),
        "is_dir" => Ok(PyObject::bool_val(std::path::Path::new(&get_path()).is_dir())),
        "is_absolute" => Ok(PyObject::bool_val(std::path::Path::new(&get_path()).is_absolute())),
        "read_text" => {
            let path = get_path();
            let content = std::fs::read_to_string(&path)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::str_val(CompactString::from(&content)))
        }
        "read_bytes" => {
            let path = get_path();
            let content = std::fs::read(&path)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::bytes(content))
        }
        "write_text" => {
            check_args_min("write_text", args, 1)?;
            let path = get_path();
            let text = args[0].py_to_string();
            let len = text.len();
            std::fs::write(&path, &text)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::int(len as i64))
        }
        "write_bytes" => {
            check_args_min("write_bytes", args, 1)?;
            let path = get_path();
            let data = match &args[0].payload {
                PyObjectPayload::Bytes(b) => b.clone(),
                _ => return Err(PyException::type_error("expected bytes")),
            };
            let len = data.len();
            std::fs::write(&path, &data)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::int(len as i64))
        }
        "mkdir" => {
            let path = get_path();
            // Check for parents=True, exist_ok=True kwargs
            let parents = args.iter().any(|a| {
                if let PyObjectPayload::Dict(m) = &a.payload {
                    m.read().get(&HashableKey::Str(CompactString::from("parents")))
                        .map(|v| v.is_truthy()).unwrap_or(false)
                } else { false }
            });
            let exist_ok = args.iter().any(|a| {
                if let PyObjectPayload::Dict(m) = &a.payload {
                    m.read().get(&HashableKey::Str(CompactString::from("exist_ok")))
                        .map(|v| v.is_truthy()).unwrap_or(false)
                } else { false }
            });
            let result = if parents {
                std::fs::create_dir_all(&path)
            } else {
                std::fs::create_dir(&path)
            };
            match result {
                Ok(()) => Ok(PyObject::none()),
                Err(e) if exist_ok && e.kind() == std::io::ErrorKind::AlreadyExists => Ok(PyObject::none()),
                Err(e) => Err(PyException::runtime_error(format!("{}: '{}'", e, path))),
            }
        }
        "rmdir" => {
            let path = get_path();
            std::fs::remove_dir(&path)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::none())
        }
        "unlink" => {
            let path = get_path();
            std::fs::remove_file(&path)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::none())
        }
        "iterdir" => {
            let path = get_path();
            let entries = std::fs::read_dir(&path)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            let mut items = Vec::new();
            for entry in entries.flatten() {
                let p = entry.path().to_string_lossy().to_string();
                items.push(PyObject::str_val(CompactString::from(&p)));
            }
            Ok(PyObject::list(items))
        }
        "glob" => {
            check_args_min("glob", args, 1)?;
            let base = get_path();
            let pattern = args[0].py_to_string();
            let dir = std::path::Path::new(&base);
            let mut results = Vec::new();
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if simple_glob_match(&pattern, &name) {
                        let full = entry.path().to_string_lossy().to_string();
                        results.push(PyObject::str_val(CompactString::from(&full)));
                    }
                }
            }
            Ok(PyObject::list(results))
        }
        "name" => {
            let path = get_path();
            let p = std::path::Path::new(&path);
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            Ok(PyObject::str_val(CompactString::from(name)))
        }
        "stem" => {
            let path = get_path();
            let p = std::path::Path::new(&path);
            let stem = p.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            Ok(PyObject::str_val(CompactString::from(stem)))
        }
        "suffix" => {
            let path = get_path();
            let p = std::path::Path::new(&path);
            let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
            Ok(PyObject::str_val(CompactString::from(ext)))
        }
        "suffixes" => {
            let path = get_path();
            let name = std::path::Path::new(&path).file_name()
                .map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let parts: Vec<PyObjectRef> = name.match_indices('.')
                .map(|(i, _)| PyObject::str_val(CompactString::from(&name[i..])))
                .collect();
            // Actually need individual suffixes: ".tar.gz" → [".tar", ".gz"]
            let mut suffixes = Vec::new();
            let mut remaining = name.as_str();
            if let Some(first_dot) = remaining.find('.') {
                remaining = &remaining[first_dot..];
                for part in remaining.split('.').skip(1) {
                    suffixes.push(PyObject::str_val(CompactString::from(format!(".{}", part))));
                }
            }
            let _ = parts; // replaced
            Ok(PyObject::list(suffixes))
        }
        "parent" => {
            let path = get_path();
            let p = std::path::Path::new(&path);
            let parent = p.parent().map(|pp| pp.to_string_lossy().to_string()).unwrap_or_else(|| ".".to_string());
            Ok(PyObject::str_val(CompactString::from(parent)))
        }
        "parents" => {
            let path = get_path();
            let p = std::path::Path::new(&path);
            let mut parents = Vec::new();
            let mut cur = p.parent();
            while let Some(pp) = cur {
                parents.push(PyObject::str_val(CompactString::from(pp.to_string_lossy().to_string())));
                cur = pp.parent();
                if pp.as_os_str().is_empty() { break; }
            }
            Ok(PyObject::list(parents))
        }
        "parts" => {
            let path = get_path();
            let p = std::path::Path::new(&path);
            let parts: Vec<PyObjectRef> = p.components()
                .map(|c| PyObject::str_val(CompactString::from(c.as_os_str().to_string_lossy().to_string())))
                .collect();
            Ok(PyObject::tuple(parts))
        }
        "as_posix" => {
            let path = get_path();
            Ok(PyObject::str_val(CompactString::from(path.replace('\\', "/"))))
        }
        "relative_to" => {
            check_args_min("relative_to", args, 1)?;
            let path = get_path();
            let base = args[0].py_to_string();
            if let Ok(rel) = std::path::Path::new(&path).strip_prefix(&base) {
                Ok(PyObject::str_val(CompactString::from(rel.to_string_lossy().to_string())))
            } else {
                Err(PyException::value_error(format!(
                    "'{}' is not relative to '{}'", path, base
                )))
            }
        }
        "is_symlink" => {
            let path = get_path();
            Ok(PyObject::bool_val(std::path::Path::new(&path).is_symlink()))
        }
        "absolute" => {
            let path = get_path();
            let p = std::path::Path::new(&path);
            if p.is_absolute() {
                Ok(PyObject::str_val(CompactString::from(path)))
            } else {
                let abs = std::env::current_dir().unwrap_or_default().join(p);
                Ok(PyObject::str_val(CompactString::from(abs.to_string_lossy().to_string())))
            }
        }
        "resolve" => {
            let path = get_path();
            let resolved = std::fs::canonicalize(&path)
                .unwrap_or_else(|_| std::path::PathBuf::from(&path));
            Ok(PyObject::str_val(CompactString::from(resolved.to_string_lossy().to_string())))
        }
        "with_suffix" => {
            check_args_min("with_suffix", args, 1)?;
            let path = get_path();
            let new_suffix = args[0].py_to_string();
            let p = std::path::Path::new(&path);
            let new_path = p.with_extension(new_suffix.trim_start_matches('.'));
            Ok(PyObject::str_val(CompactString::from(new_path.to_string_lossy().to_string())))
        }
        "with_name" => {
            check_args_min("with_name", args, 1)?;
            let path = get_path();
            let new_name = args[0].py_to_string();
            let p = std::path::Path::new(&path);
            let new_path = p.with_file_name(&new_name);
            Ok(PyObject::str_val(CompactString::from(new_path.to_string_lossy().to_string())))
        }
        "joinpath" | "__truediv__" => {
            check_args_min("joinpath", args, 1)?;
            let base = get_path();
            let mut joined = std::path::PathBuf::from(&base);
            for arg in args {
                joined = joined.join(arg.py_to_string().as_str());
            }
            Ok(PyObject::str_val(CompactString::from(joined.to_string_lossy().to_string())))
        }
        "stat" => {
            let path = get_path();
            let meta = std::fs::metadata(&path)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            let mut ns = IndexMap::new();
            ns.insert(CompactString::from("st_size"), PyObject::int(meta.len() as i64));
            ns.insert(CompactString::from("st_mode"), PyObject::int(0));
            let cls = PyObject::class(CompactString::from("stat_result"), vec![], IndexMap::new());
            let inst_obj = PyObject::instance(cls);
            if let PyObjectPayload::Instance(inst_data) = &inst_obj.payload {
                let mut attrs = inst_data.attrs.write();
                for (k, v) in ns { attrs.insert(k, v); }
            }
            Ok(inst_obj)
        }
        "__str__" | "__repr__" | "__fspath__" => {
            Ok(PyObject::str_val(CompactString::from(get_path())))
        }
        "touch" => {
            let path = get_path();
            // touch(mode=0o666, exist_ok=True) — create file if doesn't exist
            let exist_ok = args.iter().any(|a| {
                if let PyObjectPayload::Dict(m) = &a.payload {
                    m.read().get(&HashableKey::Str(CompactString::from("exist_ok")))
                        .map(|v| v.is_truthy()).unwrap_or(true)
                } else { true }
            });
            let p = std::path::Path::new(&path);
            if p.exists() {
                if !exist_ok {
                    return Err(PyException::runtime_error(format!("FileExistsError: '{}'", path)));
                }
                // Update modification time by opening and closing
                std::fs::OpenOptions::new().write(true).open(&path)
                    .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            } else {
                std::fs::File::create(&path)
                    .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            }
            Ok(PyObject::none())
        }
        "rglob" => {
            check_args_min("rglob", args, 1)?;
            let base = get_path();
            let pattern = args[0].py_to_string();
            let dir = std::path::Path::new(&base);
            let mut results = Vec::new();
            fn walk_dir_rglob(dir: &std::path::Path, pattern: &str, results: &mut Vec<PyObjectRef>) {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let name = entry.file_name().to_string_lossy().to_string();
                        if super::instance_methods::simple_glob_match(pattern, &name) {
                            results.push(PyObject::str_val(CompactString::from(path.to_string_lossy().to_string())));
                        }
                        if path.is_dir() {
                            walk_dir_rglob(&path, pattern, results);
                        }
                    }
                }
            }
            walk_dir_rglob(dir, &pattern, &mut results);
            Ok(PyObject::list(results))
        }
        "chmod" => {
            check_args_min("chmod", args, 1)?;
            let path = get_path();
            let mode = args[0].to_int()? as u32;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(mode);
                std::fs::set_permissions(&path, perms)
                    .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            }
            #[cfg(not(unix))]
            {
                let _ = mode;
            }
            Ok(PyObject::none())
        }
        "match" => {
            check_args_min("match", args, 1)?;
            let path = get_path();
            let pattern = args[0].py_to_string();
            // Match against the full path or just the filename
            let p = std::path::Path::new(&path);
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let matched = simple_glob_match(&pattern, &name) || simple_glob_match(&pattern, &path);
            Ok(PyObject::bool_val(matched))
        }
        "samefile" => {
            check_args_min("samefile", args, 1)?;
            let path = get_path();
            let other = args[0].py_to_string();
            let meta1 = std::fs::canonicalize(&path)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
            let meta2 = std::fs::canonicalize(&other)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, other)))?;
            Ok(PyObject::bool_val(meta1 == meta2))
        }
        "rename" => {
            check_args_min("rename", args, 1)?;
            let path = get_path();
            let target = args[0].py_to_string();
            std::fs::rename(&path, &target)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}' -> '{}'", e, path, target)))?;
            Ok(PyObject::str_val(CompactString::from(&target)))
        }
        "replace" => {
            check_args_min("replace", args, 1)?;
            let path = get_path();
            let target = args[0].py_to_string();
            // replace is like rename but silently replaces target if it exists
            std::fs::rename(&path, &target)
                .map_err(|e| PyException::runtime_error(format!("{}: '{}' -> '{}'", e, path, target)))?;
            Ok(PyObject::str_val(CompactString::from(&target)))
        }
        "open" => {
            // Simple open: return the text content for read mode
            let path = get_path();
            let mode = if !args.is_empty() { args[0].py_to_string() } else { "r".to_string() };
            if mode.contains('r') {
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
                Ok(PyObject::str_val(CompactString::from(&content)))
            } else {
                // For write modes, create/truncate the file and return None
                std::fs::File::create(&path)
                    .map_err(|e| PyException::runtime_error(format!("{}: '{}'", e, path)))?;
                Ok(PyObject::none())
            }
        }
        _ => Err(PyException::attribute_error(format!("'Path' object has no attribute '{}'", method))),
    }
}

// ── datetime methods ──

/// Given a base datetime string and an optional tzinfo, append the UTC offset suffix.
fn append_tz_offset(base: &str, tzinfo: &Option<PyObjectRef>) -> String {
    if let Some(ref tz) = tzinfo {
        if !matches!(&tz.payload, PyObjectPayload::None) {
            if let PyObjectPayload::Instance(ref tz_inst) = tz.payload {
                let tz_attrs = tz_inst.attrs.read();
                let offset_secs = tz_attrs.get("_offset_seconds")
                    .and_then(|v| match &v.payload {
                        PyObjectPayload::Float(f) => Some(*f as i64),
                        PyObjectPayload::Int(i) => i.to_i64(),
                        _ => None,
                    })
                    .unwrap_or(0);
                let sign = if offset_secs < 0 { '-' } else { '+' };
                let abs_secs = offset_secs.unsigned_abs();
                let oh = abs_secs / 3600;
                let om = (abs_secs % 3600) / 60;
                return format!("{}{}{:02}:{:02}", base, sign, oh, om);
            }
        }
    }
    base.to_string()
}

pub(super) fn call_datetime_method(inst: &ferrython_core::object::InstanceData, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let attrs = inst.attrs.read();
    let year = attrs.get("year").and_then(|v| v.as_int()).unwrap_or(1970);
    let month = attrs.get("month").and_then(|v| v.as_int()).unwrap_or(1);
    let day = attrs.get("day").and_then(|v| v.as_int()).unwrap_or(1);
    let hour = attrs.get("hour").and_then(|v| v.as_int()).unwrap_or(0);
    let minute = attrs.get("minute").and_then(|v| v.as_int()).unwrap_or(0);
    let second = attrs.get("second").and_then(|v| v.as_int()).unwrap_or(0);
    let microsecond = attrs.get("microsecond").and_then(|v| v.as_int()).unwrap_or(0);
    let date_only = attrs.contains_key("__date_only__");
    let time_only = attrs.contains_key("__time_only__");
    let tzinfo = attrs.get("tzinfo").cloned();
    drop(attrs);
    match method {
        "strftime" => {
            check_args_min("strftime", args, 1)?;
            let fmt = args[0].py_to_string();
            let result = datetime_strftime(&fmt, year, month, day, hour, minute, second, microsecond);
            Ok(PyObject::str_val(CompactString::from(&result)))
        }
        "isoformat" => {
            if time_only {
                let s = if microsecond != 0 {
                    format!("{:02}:{:02}:{:02}.{:06}", hour, minute, second, microsecond)
                } else {
                    format!("{:02}:{:02}:{:02}", hour, minute, second)
                };
                Ok(PyObject::str_val(CompactString::from(&s)))
            } else if date_only {
                let s = format!("{:04}-{:02}-{:02}", year, month, day);
                Ok(PyObject::str_val(CompactString::from(&s)))
            } else {
                let sep = if !args.is_empty() { args[0].py_to_string() } else { "T".to_string() };
                let base = if microsecond != 0 {
                    format!("{:04}-{:02}-{:02}{}{:02}:{:02}:{:02}.{:06}", year, month, day, sep, hour, minute, second, microsecond)
                } else {
                    format!("{:04}-{:02}-{:02}{}{:02}:{:02}:{:02}", year, month, day, sep, hour, minute, second)
                };
                let s = append_tz_offset(&base, &tzinfo);
                Ok(PyObject::str_val(CompactString::from(&s)))
            }
        }
        "date" => {
            let cls = PyObject::class(CompactString::from("date"), vec![], IndexMap::new());
            let inst_obj = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst_obj.payload {
                let mut w = d.attrs.write();
                w.insert(intern_or_new("__datetime__"), PyObject::bool_val(true));
                w.insert(intern_or_new("__date_only__"), PyObject::bool_val(true));
                w.insert(CompactString::from("year"), PyObject::int(year));
                w.insert(CompactString::from("month"), PyObject::int(month));
                w.insert(CompactString::from("day"), PyObject::int(day));
            }
            Ok(inst_obj)
        }
        "time" => {
            let cls = PyObject::class(CompactString::from("time"), vec![], IndexMap::new());
            let inst_obj = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst_obj.payload {
                let mut w = d.attrs.write();
                w.insert(intern_or_new("__datetime__"), PyObject::bool_val(true));
                w.insert(intern_or_new("__time_only__"), PyObject::bool_val(true));
                w.insert(CompactString::from("hour"), PyObject::int(hour));
                w.insert(CompactString::from("minute"), PyObject::int(minute));
                w.insert(CompactString::from("second"), PyObject::int(second));
                w.insert(CompactString::from("microsecond"), PyObject::int(microsecond));
            }
            Ok(inst_obj)
        }
        "replace" => {
            // replace(year=None, month=None, ...) via kwargs dict
            let mut ny = year; let mut nm = month; let mut nd = day;
            let mut nh = hour; let mut nmi = minute; let mut ns = second; let mut nus = microsecond;
            if let Some(kw) = args.last() {
                if let PyObjectPayload::Dict(map) = &kw.payload {
                    let r = map.read();
                    if let Some(v) = r.get(&HashableKey::Str(CompactString::from("year"))) { ny = v.as_int().unwrap_or(ny); }
                    if let Some(v) = r.get(&HashableKey::Str(CompactString::from("month"))) { nm = v.as_int().unwrap_or(nm); }
                    if let Some(v) = r.get(&HashableKey::Str(CompactString::from("day"))) { nd = v.as_int().unwrap_or(nd); }
                    if let Some(v) = r.get(&HashableKey::Str(CompactString::from("hour"))) { nh = v.as_int().unwrap_or(nh); }
                    if let Some(v) = r.get(&HashableKey::Str(CompactString::from("minute"))) { nmi = v.as_int().unwrap_or(nmi); }
                    if let Some(v) = r.get(&HashableKey::Str(CompactString::from("second"))) { ns = v.as_int().unwrap_or(ns); }
                    if let Some(v) = r.get(&HashableKey::Str(CompactString::from("microsecond"))) { nus = v.as_int().unwrap_or(nus); }
                }
            }
            let cls = PyObject::class(CompactString::from("datetime"), vec![], IndexMap::new());
            let inst_obj = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst_obj.payload {
                let mut w = d.attrs.write();
                w.insert(intern_or_new("__datetime__"), PyObject::bool_val(true));
                w.insert(CompactString::from("year"), PyObject::int(ny));
                w.insert(CompactString::from("month"), PyObject::int(nm));
                w.insert(CompactString::from("day"), PyObject::int(nd));
                w.insert(CompactString::from("hour"), PyObject::int(nh));
                w.insert(CompactString::from("minute"), PyObject::int(nmi));
                w.insert(CompactString::from("second"), PyObject::int(ns));
                w.insert(CompactString::from("microsecond"), PyObject::int(nus));
            }
            Ok(inst_obj)
        }
        "timestamp" => {
            // Rough UNIX timestamp (ignoring timezone)
            let days = ymd_to_days(year, month, day) - 719468;
            let total = days as f64 * 86400.0 + hour as f64 * 3600.0 + minute as f64 * 60.0 + second as f64 + microsecond as f64 / 1_000_000.0;
            Ok(PyObject::float(total))
        }
        "weekday" => {
            let days = ymd_to_days(year, month, day);
            Ok(PyObject::int((days + 2) % 7)) // Monday=0
        }
        "isoweekday" => {
            let days = ymd_to_days(year, month, day);
            let wd = (days + 2) % 7; // Monday=0
            Ok(PyObject::int(wd + 1)) // Monday=1, Sunday=7
        }
        "toordinal" => {
            // Proleptic Gregorian ordinal: Jan 1 of year 1 = ordinal 1
            let days = ymd_to_days(year, month, day);
            // ymd_to_days returns civil days from epoch; year 1, Jan 1 ordinal = 1
            // Offset: ymd_to_days(1,1,1) gives the civil day number for year 1 Jan 1
            let epoch = ymd_to_days(1, 1, 1);
            Ok(PyObject::int(days - epoch + 1))
        }
        "ctime" => {
            let weekday_short = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
            let month_short = ["", "Jan", "Feb", "Mar", "Apr", "May", "Jun",
                               "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
            let days = ymd_to_days(year, month, day);
            let wday = ((days + 2) % 7) as usize;
            let s = format!("{} {} {:2} {:02}:{:02}:{:02} {:04}",
                weekday_short.get(wday).unwrap_or(&""),
                month_short.get(month as usize).unwrap_or(&""),
                day, hour, minute, second, year);
            Ok(PyObject::str_val(CompactString::from(&s)))
        }
        "timetuple" => {
            let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
            let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
            let yday: i64 = month_days[..(month - 1) as usize].iter().map(|&d| d as i64).sum::<i64>() + day;
            Ok(PyObject::tuple(vec![
                PyObject::int(year), PyObject::int(month), PyObject::int(day),
                PyObject::int(hour), PyObject::int(minute), PyObject::int(second),
                PyObject::int((ymd_to_days(year, month, day) + 2) % 7),
                PyObject::int(yday), PyObject::int(-1),
            ]))
        }
        "isocalendar" => {
            // ISO calendar: (year, week, weekday) where Monday=1, Sunday=7
            let days = ymd_to_days(year, month, day);
            let dow = ((days + 2) % 7 + 7) % 7; // 0=Monday
            // Find Thursday of the same ISO week
            let thu = days + 3 - dow;
            // ISO year is the year containing that Thursday
            let (iso_year, _, _) = days_to_ymd_civil(thu);
            let jan1_of_iso_year = ymd_to_days(iso_year, 1, 1);
            let jan1_dow = ((jan1_of_iso_year + 2) % 7 + 7) % 7;
            // Monday of ISO week 1
            let iso_week1_mon = if jan1_dow <= 3 {
                jan1_of_iso_year - jan1_dow
            } else {
                jan1_of_iso_year + 7 - jan1_dow
            };
            let week_num = (days - iso_week1_mon) / 7 + 1;
            Ok(PyObject::tuple(vec![
                PyObject::int(iso_year),
                PyObject::int(week_num),
                PyObject::int(dow + 1), // Monday=1
            ]))
        }
        "__str__" | "__repr__" => {
            if time_only {
                let s = format!("{:02}:{:02}:{:02}", hour, minute, second);
                Ok(PyObject::str_val(CompactString::from(&s)))
            } else if date_only {
                let s = format!("{:04}-{:02}-{:02}", year, month, day);
                Ok(PyObject::str_val(CompactString::from(&s)))
            } else {
                let base = format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", year, month, day, hour, minute, second);
                let s = append_tz_offset(&base, &tzinfo);
                Ok(PyObject::str_val(CompactString::from(&s)))
            }
        }
        "astimezone" => {
            // Stub: return self (datetime with same values)
            let cls = PyObject::class(CompactString::from("datetime"), vec![], IndexMap::new());
            let inst_obj = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst_obj.payload {
                let mut w = d.attrs.write();
                w.insert(intern_or_new("__datetime__"), PyObject::bool_val(true));
                w.insert(CompactString::from("year"), PyObject::int(year));
                w.insert(CompactString::from("month"), PyObject::int(month));
                w.insert(CompactString::from("day"), PyObject::int(day));
                w.insert(CompactString::from("hour"), PyObject::int(hour));
                w.insert(CompactString::from("minute"), PyObject::int(minute));
                w.insert(CompactString::from("second"), PyObject::int(second));
                w.insert(CompactString::from("microsecond"), PyObject::int(microsecond));
            }
            Ok(inst_obj)
        }
        "utcoffset" => {
            if let Some(ref tz) = tzinfo {
                if !matches!(&tz.payload, PyObjectPayload::None) {
                    if let PyObjectPayload::Instance(ref tz_inst) = tz.payload {
                        let tz_attrs = tz_inst.attrs.read();
                        let offset_secs = tz_attrs.get("_offset_seconds")
                            .and_then(|v| match &v.payload {
                                PyObjectPayload::Float(f) => Some(*f as i64),
                                PyObjectPayload::Int(i) => i.to_i64(),
                                _ => None,
                            })
                            .unwrap_or(0);
                        let td_cls = PyObject::class(CompactString::from("timedelta"), vec![], IndexMap::new());
                        let td = PyObject::instance(td_cls);
                        if let PyObjectPayload::Instance(ref d) = td.payload {
                            let mut w = d.attrs.write();
                            w.insert(intern_or_new("__timedelta__"), PyObject::bool_val(true));
                            w.insert(CompactString::from("days"), PyObject::int(0));
                            w.insert(CompactString::from("seconds"), PyObject::int(offset_secs));
                            w.insert(CompactString::from("microseconds"), PyObject::int(0));
                            w.insert(CompactString::from("_total_seconds"), PyObject::float(offset_secs as f64));
                        }
                        return Ok(td);
                    }
                }
            }
            Ok(PyObject::none())
        }
        "tzname" => {
            if let Some(ref tz) = tzinfo {
                if !matches!(&tz.payload, PyObjectPayload::None) {
                    if let PyObjectPayload::Instance(ref tz_inst) = tz.payload {
                        let tz_attrs = tz_inst.attrs.read();
                        if let Some(name) = tz_attrs.get("_name") {
                            return Ok(name.clone());
                        }
                    }
                }
            }
            Ok(PyObject::none())
        }
        _ => Err(PyException::attribute_error(format!("'datetime' object has no attribute '{}'", method))),
    }
}

pub(super) fn call_timedelta_method(inst: &ferrython_core::object::InstanceData, method: &str, _args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let attrs = inst.attrs.read();
    match method {
        "total_seconds" => {
            Ok(attrs.get("_total_seconds").cloned().unwrap_or_else(|| PyObject::float(0.0)))
        }
        "__str__" | "__repr__" => {
            let days = attrs.get("days").and_then(|v| v.as_int()).unwrap_or(0);
            let secs = attrs.get("seconds").and_then(|v| v.as_int()).unwrap_or(0);
            let h = secs / 3600;
            let m = (secs % 3600) / 60;
            let s = secs % 60;
            let result = if days != 0 {
                format!("{} day{}, {}:{:02}:{:02}", days, if days.abs() != 1 { "s" } else { "" }, h, m, s)
            } else {
                format!("{}:{:02}:{:02}", h, m, s)
            };
            Ok(PyObject::str_val(CompactString::from(&result)))
        }
        "__neg__" => {
            let total_us = attrs.get("_total_us").and_then(|v| v.as_int()).unwrap_or(0);
            let neg = -total_us;
            let days = neg / 86_400_000_000;
            let rem = neg % 86_400_000_000;
            let seconds = rem / 1_000_000;
            let microseconds = rem % 1_000_000;
            let total = neg as f64 / 1_000_000.0;
            drop(attrs);
            let cls = PyObject::class(CompactString::from("timedelta"), vec![], IndexMap::new());
            let inst_obj = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst_obj.payload {
                let mut w = d.attrs.write();
                w.insert(intern_or_new("__timedelta__"), PyObject::bool_val(true));
                w.insert(CompactString::from("days"), PyObject::int(days));
                w.insert(CompactString::from("seconds"), PyObject::int(seconds));
                w.insert(CompactString::from("microseconds"), PyObject::int(microseconds));
                w.insert(CompactString::from("total_seconds"), PyObject::float(total));
                w.insert(CompactString::from("_total_us"), PyObject::int(neg));
            }
            Ok(inst_obj)
        }
        _ => Err(PyException::attribute_error(format!("'timedelta' object has no attribute '{}'", method))),
    }
}

pub(super) fn datetime_strftime(fmt: &str, year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64, _microsecond: i64) -> String {
    let weekday_names = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    let weekday_short = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let month_names = ["", "January", "February", "March", "April", "May", "June",
                       "July", "August", "September", "October", "November", "December"];
    let month_short = ["", "Jan", "Feb", "Mar", "Apr", "May", "Jun",
                       "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let days_civil = ymd_to_days(year, month, day);
    let wday = ((days_civil + 2) % 7) as usize; // Monday=0
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_lengths = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let yday: i64 = month_lengths[..(month - 1) as usize].iter().map(|&d| d as i64).sum::<i64>() + day;

    let mut result = String::new();
    let mut chars = fmt.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            match chars.next() {
                Some('Y') => result.push_str(&format!("{:04}", year)),
                Some('y') => result.push_str(&format!("{:02}", year % 100)),
                Some('m') => result.push_str(&format!("{:02}", month)),
                Some('d') => result.push_str(&format!("{:02}", day)),
                Some('H') => result.push_str(&format!("{:02}", hour)),
                Some('I') => result.push_str(&format!("{:02}", if hour % 12 == 0 { 12 } else { hour % 12 })),
                Some('M') => result.push_str(&format!("{:02}", minute)),
                Some('S') => result.push_str(&format!("{:02}", second)),
                Some('f') => result.push_str(&format!("{:06}", _microsecond)),
                Some('p') => result.push_str(if hour < 12 { "AM" } else { "PM" }),
                Some('A') => result.push_str(weekday_names.get(wday).unwrap_or(&"")),
                Some('a') => result.push_str(weekday_short.get(wday).unwrap_or(&"")),
                Some('B') => result.push_str(month_names.get(month as usize).unwrap_or(&"")),
                Some('b') | Some('h') => result.push_str(month_short.get(month as usize).unwrap_or(&"")),
                Some('w') => result.push_str(&format!("{}", (wday + 1) % 7)), // Sunday=0
                Some('j') => result.push_str(&format!("{:03}", yday)),
                Some('c') => result.push_str(&format!("{} {} {:2} {:02}:{:02}:{:02} {:04}",
                    weekday_short.get(wday).unwrap_or(&""), month_short.get(month as usize).unwrap_or(&""),
                    day, hour, minute, second, year)),
                Some('x') => result.push_str(&format!("{:02}/{:02}/{:02}", month, day, year % 100)),
                Some('X') => result.push_str(&format!("{:02}:{:02}:{:02}", hour, minute, second)),
                Some('%') => result.push('%'),
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some(c) => { result.push('%'); result.push(c); }
                None => result.push('%'),
            }
        } else {
            result.push(ch);
        }
    }
    result
}

pub(super) fn ymd_to_days(year: i64, month: i64, day: i64) -> i64 {
    // Inverse of days_to_ymd (Hinnant civil_from_days)
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = month;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe
}

fn days_to_ymd_civil(z: i64) -> (i64, i64, i64) {
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe/1460 + doe/36524 - doe/146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365*yoe + yoe/4 - yoe/100);
    let mp = (5*doy + 2) / 153;
    let d = doy - (153*mp + 2)/5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ── queue.Queue / LifoQueue / PriorityQueue methods ──

pub(super) fn call_queue_method(inst: &ferrython_core::object::InstanceData, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let attrs = inst.attrs.read();
    let kind = attrs.get("__queue__").map(|v| v.py_to_string()).unwrap_or_default();
    let items_ref = attrs.get("_items").cloned();
    let maxsize = attrs.get("maxsize").and_then(|v| v.as_int()).unwrap_or(0);
    drop(attrs);

    let items_obj = items_ref.ok_or_else(|| PyException::runtime_error("queue has no _items"))?;

    match method {
        "put" | "put_nowait" => {
            check_args_min(method, args, 1)?;
            if let PyObjectPayload::List(lock) = &items_obj.payload {
                let mut items = lock.write();
                if maxsize > 0 && items.len() as i64 >= maxsize {
                    return Err(PyException::runtime_error("queue.Full"));
                }
                items.push(args[0].clone());
                // PriorityQueue: keep sorted (min-heap via sort)
                if kind == "PriorityQueue" {
                    items.sort_by(|a, b| {
                        let lt = a.compare(b, CompareOp::Lt).map(|v| v.is_truthy()).unwrap_or(false);
                        if lt { std::cmp::Ordering::Less }
                        else {
                            let gt = a.compare(b, CompareOp::Gt).map(|v| v.is_truthy()).unwrap_or(false);
                            if gt { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Equal }
                        }
                    });
                }
            }
            Ok(PyObject::none())
        }
        "get" | "get_nowait" => {
            if let PyObjectPayload::List(lock) = &items_obj.payload {
                let mut items = lock.write();
                if items.is_empty() {
                    return Err(PyException::runtime_error("Empty"));
                }
                let result = match kind.as_str() {
                    "LifoQueue" => items.pop().unwrap(),
                    _ => items.remove(0), // FIFO or PriorityQueue (sorted, take smallest)
                };
                Ok(result)
            } else {
                Err(PyException::type_error("queue internal error"))
            }
        }
        "empty" => {
            if let PyObjectPayload::List(lock) = &items_obj.payload {
                Ok(PyObject::bool_val(lock.read().is_empty()))
            } else {
                Ok(PyObject::bool_val(true))
            }
        }
        "full" => {
            if maxsize <= 0 {
                Ok(PyObject::bool_val(false))
            } else if let PyObjectPayload::List(lock) = &items_obj.payload {
                Ok(PyObject::bool_val(lock.read().len() as i64 >= maxsize))
            } else {
                Ok(PyObject::bool_val(false))
            }
        }
        "qsize" => {
            if let PyObjectPayload::List(lock) = &items_obj.payload {
                Ok(PyObject::int(lock.read().len() as i64))
            } else {
                Ok(PyObject::int(0))
            }
        }
        "task_done" | "join" => Ok(PyObject::none()),
        _ => Err(PyException::attribute_error(format!("'{}' object has no attribute '{}'", kind, method))),
    }
}
