use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, BuiltinFn, CompareOp, PyCell, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef, SharedFxAttrMap,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

fn slice_bounds(
    len: i64,
    start: &Option<PyObjectRef>,
    stop: &Option<PyObjectRef>,
    step: &Option<PyObjectRef>,
) -> PyResult<(i64, i64, i64)> {
    let step_val = step.as_ref().map(|v| v.to_int()).transpose()?.unwrap_or(1);
    if step_val == 0 {
        return Err(PyException::value_error("slice step cannot be zero"));
    }
    let start_default = if step_val > 0 { 0 } else { len - 1 };
    let stop_default = if step_val > 0 { len } else { -len - 1 };
    let start_val = start
        .as_ref()
        .map(|v| v.to_int())
        .transpose()?
        .unwrap_or(start_default);
    let stop_val = stop
        .as_ref()
        .map(|v| v.to_int())
        .transpose()?
        .unwrap_or(stop_default);
    let start_idx = if start_val < 0 {
        (len + start_val).max(if step_val > 0 { 0 } else { -1 })
    } else {
        start_val.min(len)
    };
    let stop_idx = if stop_val < 0 {
        (len + stop_val).max(if step_val > 0 { 0 } else { -1 })
    } else {
        stop_val.min(len)
    };
    Ok((start_idx, stop_idx, step_val))
}

fn userlist_set_slice(
    items: &mut Vec<PyObjectRef>,
    sd: &ferrython_core::object::SliceData,
    value: &PyObjectRef,
) -> PyResult<()> {
    let len = items.len() as i64;
    let (start, stop, step) = slice_bounds(len, &sd.start, &sd.stop, &sd.step)?;
    let new_items = value.to_list()?;
    if step == 1 {
        let s = start.max(0).min(len) as usize;
        let e = stop.max(start).min(len) as usize;
        items.splice(s..e, new_items);
        return Ok(());
    }

    let mut indices = Vec::new();
    let mut i = start;
    if step > 0 {
        while i < stop {
            if i >= 0 && i < len {
                indices.push(i as usize);
            }
            i += step;
        }
    } else {
        while i > stop {
            if i >= 0 && i < len {
                indices.push(i as usize);
            }
            i += step;
        }
    }
    if indices.len() != new_items.len() {
        return Err(PyException::value_error(format!(
            "attempt to assign sequence of size {} to extended slice of size {}",
            new_items.len(),
            indices.len()
        )));
    }
    for (idx, val) in indices.into_iter().zip(new_items.into_iter()) {
        items[idx] = val;
    }
    Ok(())
}

fn userlist_delete_slice(
    items: &mut Vec<PyObjectRef>,
    sd: &ferrython_core::object::SliceData,
) -> PyResult<()> {
    let len = items.len() as i64;
    let (start, stop, step) = slice_bounds(len, &sd.start, &sd.stop, &sd.step)?;
    if step == 1 {
        let s = start.max(0).min(len) as usize;
        let e = stop.max(start).min(len) as usize;
        items.drain(s..e);
        return Ok(());
    }
    let mut indices = Vec::new();
    let mut i = start;
    if step > 0 {
        while i < stop {
            if i >= 0 && i < len {
                indices.push(i as usize);
            }
            i += step;
        }
    } else {
        while i > stop {
            if i >= 0 && i < len {
                indices.push(i as usize);
            }
            i += step;
        }
    }
    indices.sort_unstable_by(|a, b| b.cmp(a));
    for idx in indices {
        items.remove(idx);
    }
    Ok(())
}
fn native_method(class_name: &str, method_name: &str, f: BuiltinFn) -> PyObjectRef {
    PyObject::native_function(&format!("{class_name}.{method_name}"), f)
}
fn copy_instance_attrs(src_attrs: &SharedFxAttrMap, dst_attrs: &SharedFxAttrMap, skip: &[&str]) {
    let src = src_attrs.read();
    let mut dst = dst_attrs.write();
    for (name, value) in src.iter() {
        if skip.iter().any(|s| *s == name.as_str()) {
            continue;
        }
        dst.insert(name.clone(), value.clone());
    }
}

fn build_userdict_copy(
    data: &PyObjectRef,
    owner_class: PyObjectRef,
    src_attrs: &SharedFxAttrMap,
) -> PyResult<PyObjectRef> {
    let new_data = if let PyObjectPayload::Dict(m) = &data.payload {
        PyObject::dict(m.read().clone())
    } else {
        PyObject::dict_from_pairs(vec![])
    };
    let new_inst = PyObject::instance(owner_class);
    if let PyObjectPayload::Instance(ref dst_inst) = new_inst.payload {
        dst_inst
            .attrs
            .write()
            .insert(CompactString::from("data"), new_data.clone());
        install_dict_methods(&dst_inst.attrs, &new_data, dst_inst.class.clone());
        copy_instance_attrs(
            src_attrs,
            &dst_inst.attrs,
            &[
                "data",
                "keys",
                "values",
                "items",
                "get",
                "pop",
                "setdefault",
                "update",
                "copy",
                "clear",
            ],
        );
    }
    Ok(new_inst)
}

fn build_userlist_copy(
    data: &PyObjectRef,
    owner_class: PyObjectRef,
    src_attrs: &SharedFxAttrMap,
) -> PyResult<PyObjectRef> {
    let new_data = if let PyObjectPayload::List(items) = &data.payload {
        PyObject::list(items.read().clone())
    } else {
        PyObject::list(vec![])
    };
    let new_inst = PyObject::instance(owner_class);
    if let PyObjectPayload::Instance(ref dst_inst) = new_inst.payload {
        dst_inst
            .attrs
            .write()
            .insert(CompactString::from("data"), new_data.clone());
        install_list_methods(&dst_inst.attrs, &new_data, dst_inst.class.clone());
        copy_instance_attrs(
            src_attrs,
            &dst_inst.attrs,
            &[
                "data", "append", "extend", "insert", "pop", "remove", "clear", "reverse", "count",
                "index", "sort", "copy",
            ],
        );
    }
    Ok(new_inst)
}

// --- UserDict / UserList / UserString ---

pub(super) fn make_user_dict_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        native_method("UserDict", "__init__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("UserDict.__init__ requires self"));
            }
            let inst = &args[0];
            let data = if args.len() > 1 {
                if let PyObjectPayload::Dict(d) = &args[1].payload {
                    PyObject::wrap(PyObjectPayload::Dict(Rc::new(PyCell::new(
                        d.read().clone(),
                    ))))
                } else {
                    PyObject::dict_from_pairs(vec![])
                }
            } else {
                PyObject::dict_from_pairs(vec![])
            };
            if let PyObjectPayload::Instance(d) = &inst.payload {
                d.attrs
                    .write()
                    .insert(CompactString::from("data"), data.clone());
                // Install instance methods that directly operate on the data
                install_dict_methods(&d.attrs, &data, d.class.clone());
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__getitem__"),
        native_method("UserDict", "__getitem__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected key"));
            }
            let data = get_user_data(&args[0], "data")?;
            data.get_item(&args[1])
        }),
    );
    ns.insert(
        CompactString::from("__setitem__"),
        native_method("UserDict", "__setitem__", |args| {
            if args.len() < 3 {
                return Err(PyException::type_error("expected key and value"));
            }
            let data = get_user_data(&args[0], "data")?;
            if let PyObjectPayload::Dict(d) = &data.payload {
                let key = HashableKey::from_object(&args[1])?;
                d.write().insert(key, args[2].clone());
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__delitem__"),
        native_method("UserDict", "__delitem__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected key"));
            }
            let data = get_user_data(&args[0], "data")?;
            if let PyObjectPayload::Dict(d) = &data.payload {
                let key = HashableKey::from_object(&args[1])?;
                if d.write().shift_remove(&key).is_none() {
                    return Err(PyException::key_error(args[1].py_to_string()));
                }
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__len__"),
        native_method("UserDict", "__len__", |args| {
            let data = get_user_data(&args[0], "data")?;
            Ok(PyObject::int(data.py_len()? as i64))
        }),
    );
    ns.insert(
        CompactString::from("__contains__"),
        native_method("UserDict", "__contains__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected key"));
            }
            let data = get_user_data(&args[0], "data")?;
            if let PyObjectPayload::Dict(d) = &data.payload {
                let key = HashableKey::from_object(&args[1])?;
                Ok(PyObject::bool_val(d.read().contains_key(&key)))
            } else {
                Ok(PyObject::bool_val(false))
            }
        }),
    );
    ns.insert(
        CompactString::from("__repr__"),
        native_method("UserDict", "__repr__", |args| {
            let data = get_user_data(&args[0], "data")?;
            Ok(PyObject::str_val(CompactString::from(data.py_to_string())))
        }),
    );
    ns.insert(
        CompactString::from("__iter__"),
        native_method("UserDict", "__iter__", |args| {
            let data = get_user_data(&args[0], "data")?;
            data.get_iter()
        }),
    );
    ns.insert(
        CompactString::from("__eq__"),
        native_method("UserDict", "__eq__", |args| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let data = get_user_data(&args[0], "data")?;
            let other_data = if let Ok(od) = get_user_data(&args[1], "data") {
                od
            } else {
                args[1].clone()
            };
            if let (PyObjectPayload::Dict(a), PyObjectPayload::Dict(b)) =
                (&data.payload, &other_data.payload)
            {
                let ra = a.read();
                let rb = b.read();
                if ra.len() != rb.len() {
                    return Ok(PyObject::bool_val(false));
                }
                for (k, v) in ra.iter() {
                    match rb.get(k) {
                        Some(ov)
                            if v.compare(ov, CompareOp::Eq)
                                .map_or(false, |r| r.is_truthy()) => {}
                        _ => return Ok(PyObject::bool_val(false)),
                    }
                }
                Ok(PyObject::bool_val(true))
            } else {
                Ok(PyObject::bool_val(false))
            }
        }),
    );
    ns.insert(
        CompactString::from("__bool__"),
        native_method("UserDict", "__bool__", |args| {
            let data = get_user_data(&args[0], "data")?;
            Ok(PyObject::bool_val(data.py_len()? > 0))
        }),
    );
    ns.insert(
        CompactString::from("__or__"),
        native_method("UserDict", "__or__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected other"));
            }
            let data = get_user_data(&args[0], "data")?;
            let mut merged = IndexMap::new();
            if let PyObjectPayload::Dict(d) = &data.payload {
                for (k, v) in d.read().iter() {
                    merged.insert(k.clone(), v.clone());
                }
            }
            let other = if let Ok(od) = get_user_data(&args[1], "data") {
                od
            } else {
                args[1].clone()
            };
            if let PyObjectPayload::Dict(d) = &other.payload {
                for (k, v) in d.read().iter() {
                    merged.insert(k.clone(), v.clone());
                }
            }
            Ok(PyObject::dict(merged))
        }),
    );
    ns.insert(
        CompactString::from("__copy__"),
        native_method("UserDict", "__copy__", copy_userdict_instance),
    );
    ns.insert(
        CompactString::from("__ior__"),
        native_method("UserDict", "__ior__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected other"));
            }
            let data = get_user_data(&args[0], "data")?;
            let other = if let Ok(od) = get_user_data(&args[1], "data") {
                od
            } else {
                args[1].clone()
            };
            if let (PyObjectPayload::Dict(dst), PyObjectPayload::Dict(src)) =
                (&data.payload, &other.payload)
            {
                let mut w = dst.write();
                for (k, v) in src.read().iter() {
                    w.insert(k.clone(), v.clone());
                }
            }
            Ok(args[0].clone())
        }),
    );
    PyObject::class(CompactString::from("UserDict"), vec![], ns)
}

fn install_dict_methods(attrs: &SharedFxAttrMap, data: &PyObjectRef, owner_class: PyObjectRef) {
    let map = if let PyObjectPayload::Dict(m) = &data.payload {
        m.clone()
    } else {
        return;
    };
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("keys"),
        PyObject::native_closure("keys", move |_| {
            Ok(PyObject::wrap(PyObjectPayload::DictKeys {
                map: m.clone(),
                owner: None,
            }))
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("values"),
        PyObject::native_closure("values", move |_| {
            Ok(PyObject::wrap(PyObjectPayload::DictValues {
                map: m.clone(),
                owner: None,
            }))
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("items"),
        PyObject::native_closure("items", move |_| {
            Ok(PyObject::wrap(PyObjectPayload::DictItems {
                map: m.clone(),
                owner: None,
            }))
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("get"),
        PyObject::native_closure("get", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "get() requires at least 1 argument",
                ));
            }
            let key = args[0].to_hashable_key()?;
            let default = if args.len() >= 2 {
                args[1].clone()
            } else {
                PyObject::none()
            };
            Ok(m.read().get(&key).cloned().unwrap_or(default))
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("pop"),
        PyObject::native_closure("pop", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "pop() requires at least 1 argument",
                ));
            }
            let key = args[0].to_hashable_key()?;
            let default = if args.len() >= 2 {
                Some(args[1].clone())
            } else {
                None
            };
            match m.write().shift_remove(&key) {
                Some(v) => Ok(v),
                None => default.ok_or_else(|| PyException::key_error(args[0].py_to_string())),
            }
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("setdefault"),
        PyObject::native_closure("setdefault", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "setdefault() requires at least 1 argument",
                ));
            }
            let key = args[0].to_hashable_key()?;
            let default = if args.len() >= 2 {
                args[1].clone()
            } else {
                PyObject::none()
            };
            let mut w = m.write();
            if let Some(v) = w.get(&key) {
                return Ok(v.clone());
            }
            w.insert(key, default.clone());
            Ok(default)
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("update"),
        PyObject::native_closure("update", move |args| {
            if !args.is_empty() {
                if let PyObjectPayload::Dict(other) = &args[0].payload {
                    let mut w = m.write();
                    for (k, v) in other.read().iter() {
                        w.insert(k.clone(), v.clone());
                    }
                }
            }
            Ok(PyObject::none())
        }),
    );
    attrs.write().insert(CompactString::from("copy"), {
        let attrs = attrs.clone();
        let data = data.clone();
        let owner_class = owner_class.clone();
        PyObject::native_closure("copy", move |_| {
            build_userdict_copy(&data, owner_class.clone(), &attrs)
        })
    });
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("clear"),
        PyObject::native_closure("clear", move |_| {
            m.write().clear();
            Ok(PyObject::none())
        }),
    );
}

fn copy_userdict_instance(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("UserDict.__copy__ requires self"));
    }
    let self_obj = &args[0];
    let inst = if let PyObjectPayload::Instance(inst) = &self_obj.payload {
        inst
    } else {
        return Err(PyException::type_error(
            "UserDict.__copy__ requires an instance",
        ));
    };
    let data = get_user_data(self_obj, "data")?;
    build_userdict_copy(&data, inst.class.clone(), &inst.attrs)
}

fn copy_userlist_instance(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("UserList.__copy__ requires self"));
    }
    let self_obj = &args[0];
    let inst = if let PyObjectPayload::Instance(inst) = &self_obj.payload {
        inst
    } else {
        return Err(PyException::type_error(
            "UserList.__copy__ requires an instance",
        ));
    };
    let data = get_user_data(self_obj, "data")?;
    build_userlist_copy(&data, inst.class.clone(), &inst.attrs)
}

pub(super) fn make_user_list_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        native_method("UserList", "__init__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("UserList.__init__ requires self"));
            }
            let inst = &args[0];
            let data = if args.len() > 1 {
                let items = args[1].to_list()?;
                PyObject::list(items)
            } else {
                PyObject::list(vec![])
            };
            if let PyObjectPayload::Instance(d) = &inst.payload {
                d.attrs
                    .write()
                    .insert(CompactString::from("data"), data.clone());
                install_list_methods(&d.attrs, &data, d.class.clone());
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__getitem__"),
        native_method("UserList", "__getitem__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected index"));
            }
            let data = get_user_data(&args[0], "data")?;
            data.get_item(&args[1])
        }),
    );
    ns.insert(
        CompactString::from("__setitem__"),
        native_method("UserList", "__setitem__", |args| {
            if args.len() < 3 {
                return Err(PyException::type_error("expected index and value"));
            }
            let data = get_user_data(&args[0], "data")?;
            if let PyObjectPayload::List(l) = &data.payload {
                if let PyObjectPayload::Slice(sd) = &args[1].payload {
                    userlist_set_slice(&mut l.write(), sd, &args[2])?;
                    return Ok(PyObject::none());
                }
                let idx = args[1].to_int()? as i64;
                let mut w = l.write();
                let len = w.len() as i64;
                let i = if idx < 0 {
                    (len + idx).max(0) as usize
                } else {
                    idx as usize
                };
                if i < w.len() {
                    w[i] = args[2].clone();
                } else {
                    return Err(PyException::index_error(
                        "list assignment index out of range",
                    ));
                }
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__len__"),
        native_method("UserList", "__len__", |args| {
            let data = get_user_data(&args[0], "data")?;
            Ok(PyObject::int(data.py_len()? as i64))
        }),
    );
    ns.insert(
        CompactString::from("__contains__"),
        native_method("UserList", "__contains__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected item"));
            }
            let data = get_user_data(&args[0], "data")?;
            if let PyObjectPayload::List(l) = &data.payload {
                let target = &args[1];
                Ok(PyObject::bool_val(l.read().iter().any(|x| {
                    x.compare(target, CompareOp::Eq)
                        .map_or(false, |v| v.is_truthy())
                })))
            } else {
                Ok(PyObject::bool_val(false))
            }
        }),
    );
    ns.insert(
        CompactString::from("__repr__"),
        native_method("UserList", "__repr__", |args| {
            let data = get_user_data(&args[0], "data")?;
            Ok(PyObject::str_val(CompactString::from(data.py_to_string())))
        }),
    );
    ns.insert(
        CompactString::from("__iter__"),
        native_method("UserList", "__iter__", |args| {
            let data = get_user_data(&args[0], "data")?;
            data.get_iter()
        }),
    );
    ns.insert(
        CompactString::from("__delitem__"),
        native_method("UserList", "__delitem__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected index"));
            }
            let data = get_user_data(&args[0], "data")?;
            if let PyObjectPayload::List(l) = &data.payload {
                if let PyObjectPayload::Slice(sd) = &args[1].payload {
                    userlist_delete_slice(&mut l.write(), sd)?;
                    return Ok(PyObject::none());
                }
                let idx = args[1].to_int()? as i64;
                let mut w = l.write();
                let len = w.len() as i64;
                let i = if idx < 0 {
                    (len + idx).max(0) as usize
                } else {
                    idx as usize
                };
                if i < w.len() {
                    w.remove(i);
                    Ok(PyObject::none())
                } else {
                    Err(PyException::index_error(
                        "list assignment index out of range",
                    ))
                }
            } else {
                Err(PyException::type_error("expected list data"))
            }
        }),
    );
    ns.insert(
        CompactString::from("__add__"),
        native_method("UserList", "__add__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected other"));
            }
            let data = get_user_data(&args[0], "data")?;
            let other = if let Ok(od) = get_user_data(&args[1], "data") {
                od
            } else {
                args[1].clone()
            };
            let mut items = data.to_list()?;
            items.extend(other.to_list()?);
            Ok(PyObject::list(items))
        }),
    );
    ns.insert(
        CompactString::from("__iadd__"),
        native_method("UserList", "__iadd__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected other"));
            }
            let data = get_user_data(&args[0], "data")?;
            let other = if let Ok(od) = get_user_data(&args[1], "data") {
                od
            } else {
                args[1].clone()
            };
            if let PyObjectPayload::List(l) = &data.payload {
                l.write().extend(other.to_list()?);
            }
            Ok(args[0].clone())
        }),
    );
    ns.insert(
        CompactString::from("__mul__"),
        native_method("UserList", "__mul__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected int"));
            }
            let data = get_user_data(&args[0], "data")?;
            let n = args[1].to_int()?.max(0) as usize;
            let items = data.to_list()?;
            let mut result = Vec::with_capacity(items.len() * n);
            for _ in 0..n {
                result.extend(items.iter().cloned());
            }
            Ok(PyObject::list(result))
        }),
    );
    ns.insert(
        CompactString::from("__copy__"),
        native_method("UserList", "__copy__", copy_userlist_instance),
    );
    ns.insert(
        CompactString::from("__eq__"),
        native_method("UserList", "__eq__", |args| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let data = get_user_data(&args[0], "data")?;
            let other = if let Ok(od) = get_user_data(&args[1], "data") {
                od
            } else {
                args[1].clone()
            };
            if let (PyObjectPayload::List(a), PyObjectPayload::List(b)) =
                (&data.payload, &other.payload)
            {
                let ra = a.read();
                let rb = b.read();
                if ra.len() != rb.len() {
                    return Ok(PyObject::bool_val(false));
                }
                for (x, y) in ra.iter().zip(rb.iter()) {
                    if !x.compare(y, CompareOp::Eq).map_or(false, |v| v.is_truthy()) {
                        return Ok(PyObject::bool_val(false));
                    }
                }
                Ok(PyObject::bool_val(true))
            } else {
                Ok(PyObject::bool_val(false))
            }
        }),
    );
    ns.insert(
        CompactString::from("__bool__"),
        native_method("UserList", "__bool__", |args| {
            let data = get_user_data(&args[0], "data")?;
            Ok(PyObject::bool_val(data.py_len()? > 0))
        }),
    );
    PyObject::class(CompactString::from("UserList"), vec![], ns)
}

fn install_list_methods(attrs: &SharedFxAttrMap, data: &PyObjectRef, owner_class: PyObjectRef) {
    if !matches!(&data.payload, PyObjectPayload::List(_)) {
        return;
    }
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("append"),
        PyObject::native_closure("append", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("append() requires 1 argument"));
            }
            if let PyObjectPayload::List(items) = &l.payload {
                items.write().push(args[0].clone());
            }
            Ok(PyObject::none())
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("extend"),
        PyObject::native_closure("extend", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("extend() requires 1 argument"));
            }
            let new_items = args[0].to_list()?;
            if let PyObjectPayload::List(items) = &l.payload {
                items.write().extend(new_items);
            }
            Ok(PyObject::none())
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("insert"),
        PyObject::native_closure("insert", move |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("insert() requires 2 arguments"));
            }
            let idx = args[0].to_int()? as usize;
            if let PyObjectPayload::List(items) = &l.payload {
                let mut w = items.write();
                let idx = idx.min(w.len());
                w.insert(idx, args[1].clone());
            }
            Ok(PyObject::none())
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("pop"),
        PyObject::native_closure("pop", move |args| {
            if let PyObjectPayload::List(items) = &l.payload {
                let mut w = items.write();
                if w.is_empty() {
                    return Err(PyException::index_error("pop from empty list"));
                }
                let idx = if !args.is_empty() {
                    let i = args[0].to_int()? as i64;
                    let len = w.len() as i64;
                    (if i < 0 {
                        (len + i).max(0)
                    } else {
                        i.min(len - 1)
                    }) as usize
                } else {
                    w.len() - 1
                };
                if idx < w.len() {
                    Ok(w.remove(idx))
                } else {
                    Err(PyException::index_error("pop index out of range"))
                }
            } else {
                Err(PyException::type_error("not a list"))
            }
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("remove"),
        PyObject::native_closure("remove", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("remove() requires 1 argument"));
            }
            if let PyObjectPayload::List(items) = &l.payload {
                let mut w = items.write();
                let target = &args[0];
                if let Some(pos) = w.iter().position(|x| {
                    x.compare(target, CompareOp::Eq)
                        .map_or(false, |v| v.is_truthy())
                }) {
                    w.remove(pos);
                    Ok(PyObject::none())
                } else {
                    Err(PyException::value_error("list.remove(x): x not in list"))
                }
            } else {
                Err(PyException::type_error("not a list"))
            }
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("clear"),
        PyObject::native_closure("clear", move |_| {
            if let PyObjectPayload::List(items) = &l.payload {
                items.write().clear();
            }
            Ok(PyObject::none())
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("reverse"),
        PyObject::native_closure("reverse", move |_| {
            if let PyObjectPayload::List(items) = &l.payload {
                items.write().reverse();
            }
            Ok(PyObject::none())
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("count"),
        PyObject::native_closure("count", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("count() requires 1 argument"));
            }
            let target = &args[0];
            if let PyObjectPayload::List(items) = &l.payload {
                let count = items
                    .read()
                    .iter()
                    .filter(|x| {
                        x.compare(target, CompareOp::Eq)
                            .map_or(false, |v| v.is_truthy())
                    })
                    .count();
                Ok(PyObject::int(count as i64))
            } else {
                Ok(PyObject::int(0))
            }
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("index"),
        PyObject::native_closure("index", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("index() requires 1 argument"));
            }
            let target = &args[0];
            if let PyObjectPayload::List(items) = &l.payload {
                let r = items.read();
                for (i, x) in r.iter().enumerate() {
                    if x.compare(target, CompareOp::Eq)
                        .map_or(false, |v| v.is_truthy())
                    {
                        return Ok(PyObject::int(i as i64));
                    }
                }
            }
            Err(PyException::value_error("x not in list"))
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("sort"),
        PyObject::native_closure("sort", move |_| {
            if let PyObjectPayload::List(items) = &l.payload {
                let mut w = items.write();
                let mut sorted: Vec<_> = w.drain(..).collect();
                sorted.sort_by(|a, b| {
                    a.compare(b, CompareOp::Lt)
                        .map_or(std::cmp::Ordering::Equal, |v| {
                            if v.is_truthy() {
                                std::cmp::Ordering::Less
                            } else {
                                std::cmp::Ordering::Greater
                            }
                        })
                });
                *w = sorted;
            }
            Ok(PyObject::none())
        }),
    );
    attrs.write().insert(CompactString::from("copy"), {
        let attrs = attrs.clone();
        let data = data.clone();
        let owner_class = owner_class.clone();
        PyObject::native_closure("copy", move |_| {
            build_userlist_copy(&data, owner_class.clone(), &attrs)
        })
    });
}

pub(super) fn make_user_string_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        make_builtin(|args| {
            if args.is_empty() {
                return Err(PyException::type_error("UserString.__init__ requires self"));
            }
            let inst = &args[0];
            let data = if args.len() > 1 {
                PyObject::str_val(CompactString::from(args[1].py_to_string()))
            } else {
                PyObject::str_val(CompactString::from(""))
            };
            if let PyObjectPayload::Instance(d) = &inst.payload {
                d.attrs
                    .write()
                    .insert(CompactString::from("data"), data.clone());
                install_string_methods(&d.attrs, &data);
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__str__"),
        make_builtin(|args| get_user_data(&args[0], "data")),
    );
    ns.insert(
        CompactString::from("__repr__"),
        make_builtin(|args| {
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("");
            Ok(PyObject::str_val(CompactString::from(format!("'{}'", s))))
        }),
    );
    ns.insert(
        CompactString::from("__len__"),
        make_builtin(|args| {
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("");
            Ok(PyObject::int(s.len() as i64))
        }),
    );
    ns.insert(
        CompactString::from("__contains__"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected item"));
            }
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("");
            let sub = args[1].py_to_string();
            Ok(PyObject::bool_val(s.contains(&*sub)))
        }),
    );
    ns.insert(
        CompactString::from("__add__"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected other"));
            }
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("").to_string();
            let other = args[1].py_to_string();
            Ok(PyObject::str_val(CompactString::from(format!(
                "{}{}",
                s, other
            ))))
        }),
    );
    ns.insert(
        CompactString::from("__eq__"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("");
            let other = args[1].py_to_string();
            Ok(PyObject::bool_val(s == other.as_str()))
        }),
    );
    ns.insert(
        CompactString::from("__getitem__"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected index"));
            }
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("");
            let idx = args[1].to_int()? as i64;
            let len = s.chars().count() as i64;
            let i = if idx < 0 {
                (len + idx).max(0) as usize
            } else {
                idx as usize
            };
            match s.chars().nth(i) {
                Some(c) => Ok(PyObject::str_val(CompactString::from(c.to_string()))),
                None => Err(PyException::index_error("string index out of range")),
            }
        }),
    );
    ns.insert(
        CompactString::from("__iter__"),
        make_builtin(|args| {
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("").to_string();
            let chars: Vec<PyObjectRef> = s
                .chars()
                .map(|c| PyObject::str_val(CompactString::from(c.to_string())))
                .collect();
            Ok(PyObject::list(chars))
        }),
    );
    ns.insert(
        CompactString::from("__mul__"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected int"));
            }
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("");
            let n = args[1].to_int()?.max(0) as usize;
            Ok(PyObject::str_val(CompactString::from(s.repeat(n))))
        }),
    );
    ns.insert(
        CompactString::from("__bool__"),
        make_builtin(|args| {
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("");
            Ok(PyObject::bool_val(!s.is_empty()))
        }),
    );
    ns.insert(
        CompactString::from("__hash__"),
        make_builtin(|args| {
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("");
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            s.hash(&mut hasher);
            Ok(PyObject::int(hasher.finish() as i64))
        }),
    );
    PyObject::class(CompactString::from("UserString"), vec![], ns)
}

fn install_string_methods(attrs: &SharedFxAttrMap, data: &PyObjectRef) {
    let s_val = data.as_str().unwrap_or("").to_string();

    macro_rules! str_method {
        ($attrs:expr, $name:expr, $s:expr, $body:expr) => {{
            let captured = $s.clone();
            $attrs.write().insert(
                CompactString::from($name),
                PyObject::native_closure($name, move |args| {
                    let s = &captured;
                    #[allow(clippy::redundant_closure_call)]
                    ($body)(s, args)
                }),
            );
        }};
    }

    str_method!(attrs, "upper", s_val, |s: &String,
                                        _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::str_val(CompactString::from(s.to_uppercase())))
    });
    str_method!(attrs, "lower", s_val, |s: &String,
                                        _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::str_val(CompactString::from(s.to_lowercase())))
    });
    str_method!(attrs, "strip", s_val, |s: &String,
                                        _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::str_val(CompactString::from(s.trim())))
    });
    str_method!(attrs, "lstrip", s_val, |s: &String,
                                         _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::str_val(CompactString::from(s.trim_start())))
    });
    str_method!(attrs, "rstrip", s_val, |s: &String,
                                         _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::str_val(CompactString::from(s.trim_end())))
    });
    str_method!(attrs, "title", s_val, |s: &String,
                                        _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        let mut title = String::with_capacity(s.len());
        let mut capitalize_next = true;
        for c in s.chars() {
            if c.is_whitespace() || !c.is_alphanumeric() {
                capitalize_next = true;
                title.push(c);
            } else if capitalize_next {
                title.extend(c.to_uppercase());
                capitalize_next = false;
            } else {
                title.extend(c.to_lowercase());
            }
        }
        Ok(PyObject::str_val(CompactString::from(title)))
    });
    str_method!(attrs, "capitalize", s_val, |s: &String,
                                             _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        let mut chars = s.chars();
        let cap = match chars.next() {
            None => String::new(),
            Some(c) => c.to_uppercase().to_string() + &chars.as_str().to_lowercase(),
        };
        Ok(PyObject::str_val(CompactString::from(cap)))
    });
    str_method!(attrs, "swapcase", s_val, |s: &String,
                                           _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        let swapped: String = s
            .chars()
            .map(|c| {
                if c.is_uppercase() {
                    c.to_lowercase().to_string()
                } else if c.is_lowercase() {
                    c.to_uppercase().to_string()
                } else {
                    c.to_string()
                }
            })
            .collect();
        Ok(PyObject::str_val(CompactString::from(swapped)))
    });
    str_method!(attrs, "split", s_val, |s: &String,
                                        args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        let parts: Vec<PyObjectRef> =
            if args.is_empty() || matches!(args[0].payload, PyObjectPayload::None) {
                s.split_whitespace()
                    .map(|p| PyObject::str_from_utf8_slice(p.as_bytes()))
                    .collect()
            } else if let PyObjectPayload::Str(sr) = &args[0].payload {
                s.split(sr.as_str())
                    .map(|p| PyObject::str_from_utf8_slice(p.as_bytes()))
                    .collect()
            } else {
                let sep = args[0].py_to_string();
                s.split(&*sep)
                    .map(|p| PyObject::str_from_utf8_slice(p.as_bytes()))
                    .collect()
            };
        Ok(PyObject::list(parts))
    });
    str_method!(attrs, "rsplit", s_val, |s: &String,
                                         args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        let parts: Vec<PyObjectRef> =
            if args.is_empty() || matches!(args[0].payload, PyObjectPayload::None) {
                s.split_whitespace()
                    .rev()
                    .map(|p| PyObject::str_from_utf8_slice(p.as_bytes()))
                    .collect()
            } else if let PyObjectPayload::Str(sr) = &args[0].payload {
                s.rsplit(sr.as_str())
                    .map(|p| PyObject::str_from_utf8_slice(p.as_bytes()))
                    .collect()
            } else {
                let sep = args[0].py_to_string();
                s.rsplit(&*sep)
                    .map(|p| PyObject::str_from_utf8_slice(p.as_bytes()))
                    .collect()
            };
        Ok(PyObject::list(parts))
    });
    str_method!(attrs, "replace", s_val, |s: &String,
                                          args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "replace() requires at least 2 arguments",
            ));
        }
        let result = match (&args[0].payload, &args[1].payload) {
            (PyObjectPayload::Str(old_s), PyObjectPayload::Str(new_s)) => {
                s.replace(old_s.as_str(), new_s.as_str())
            }
            _ => {
                let old = args[0].py_to_string();
                let new = args[1].py_to_string();
                s.replace(&*old, &*new)
            }
        };
        Ok(PyObject::str_val(CompactString::from(result)))
    });
    str_method!(attrs, "find", s_val, |s: &String,
                                       args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("find() requires 1 argument"));
        }
        let idx = if let PyObjectPayload::Str(sr) = &args[0].payload {
            s.find(sr.as_str())
        } else {
            let sub = args[0].py_to_string();
            s.find(&*sub)
        };
        Ok(PyObject::int(idx.map_or(-1, |i| i as i64)))
    });
    str_method!(attrs, "rfind", s_val, |s: &String,
                                        args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("rfind() requires 1 argument"));
        }
        let idx = if let PyObjectPayload::Str(sr) = &args[0].payload {
            s.rfind(sr.as_str())
        } else {
            let sub = args[0].py_to_string();
            s.rfind(&*sub)
        };
        Ok(PyObject::int(idx.map_or(-1, |i| i as i64)))
    });
    str_method!(attrs, "count", s_val, |s: &String,
                                        args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("count() requires 1 argument"));
        }
        let n = if let PyObjectPayload::Str(sr) = &args[0].payload {
            s.matches(sr.as_str()).count()
        } else {
            let sub = args[0].py_to_string();
            s.matches(&*sub).count()
        };
        Ok(PyObject::int(n as i64))
    });
    str_method!(attrs, "startswith", s_val, |s: &String,
                                             args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("startswith() requires 1 argument"));
        }
        let result = if let PyObjectPayload::Str(sr) = &args[0].payload {
            s.starts_with(sr.as_str())
        } else {
            let prefix = args[0].py_to_string();
            s.starts_with(&*prefix)
        };
        Ok(PyObject::bool_val(result))
    });
    str_method!(attrs, "endswith", s_val, |s: &String,
                                           args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("endswith() requires 1 argument"));
        }
        let result = if let PyObjectPayload::Str(sr) = &args[0].payload {
            s.ends_with(sr.as_str())
        } else {
            let suffix = args[0].py_to_string();
            s.ends_with(&*suffix)
        };
        Ok(PyObject::bool_val(result))
    });
    str_method!(attrs, "join", s_val, |s: &String,
                                       args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("join() requires 1 argument"));
        }
        // Direct access to list/tuple data via data_ptr — avoids to_list() Vec clone
        let (items_slice, _owned): (&[PyObjectRef], Option<Vec<PyObjectRef>>) =
            match &args[0].payload {
                PyObjectPayload::List(v) => {
                    let vec = unsafe { &*v.data_ptr() };
                    (vec.as_slice(), None)
                }
                PyObjectPayload::Tuple(v) => (&**v, None),
                _ => {
                    let list = args[0].to_list()?;
                    // Need owned Vec to live long enough — store it and take slice
                    (
                        unsafe { std::slice::from_raw_parts(list.as_ptr(), list.len()) },
                        Some(list),
                    )
                }
            };
        if items_slice.is_empty() {
            return Ok(PyObject::str_val(CompactString::new("")));
        }
        // Single-allocation join: pre-compute total length, then build
        let sep_len = s.len();
        let mut total_len = 0usize;
        for (i, item) in items_slice.iter().enumerate() {
            if i > 0 {
                total_len += sep_len;
            }
            if let PyObjectPayload::Str(sr) = &item.payload {
                total_len += sr.as_str().len();
            } else {
                total_len += item.py_to_string().len();
            }
        }
        let mut result = String::with_capacity(total_len);
        for (i, item) in items_slice.iter().enumerate() {
            if i > 0 {
                result.push_str(s);
            }
            if let PyObjectPayload::Str(sr) = &item.payload {
                result.push_str(sr.as_str());
            } else {
                result.push_str(&item.py_to_string());
            }
        }
        Ok(PyObject::str_from_utf8_slice(result.as_bytes()))
    });
    str_method!(attrs, "isalpha", s_val, |s: &String,
                                          _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::bool_val(
            !s.is_empty() && s.chars().all(|c| c.is_alphabetic()),
        ))
    });
    str_method!(attrs, "isdigit", s_val, |s: &String,
                                          _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::bool_val(
            !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()),
        ))
    });
    str_method!(attrs, "isalnum", s_val, |s: &String,
                                          _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::bool_val(
            !s.is_empty() && s.chars().all(|c| c.is_alphanumeric()),
        ))
    });
    str_method!(attrs, "isspace", s_val, |s: &String,
                                          _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::bool_val(
            !s.is_empty() && s.chars().all(|c| c.is_whitespace()),
        ))
    });
    str_method!(attrs, "isupper", s_val, |s: &String,
                                          _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::bool_val(
            s.chars().any(|c| c.is_uppercase()) && !s.chars().any(|c| c.is_lowercase()),
        ))
    });
    str_method!(attrs, "islower", s_val, |s: &String,
                                          _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::bool_val(
            s.chars().any(|c| c.is_lowercase()) && !s.chars().any(|c| c.is_uppercase()),
        ))
    });
}

fn get_user_data(obj: &PyObjectRef, attr: &str) -> PyResult<PyObjectRef> {
    if let PyObjectPayload::Instance(d) = &obj.payload {
        if let Some(v) = d.attrs.read().get(attr) {
            return Ok(v.clone());
        }
    }
    Err(PyException::attribute_error(format!(
        "'{}' object has no attribute '{}'",
        obj.type_name(),
        attr
    )))
}
