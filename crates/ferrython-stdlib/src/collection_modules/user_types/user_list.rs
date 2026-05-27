use super::*;

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

pub(in crate::collection_modules) fn make_user_list_class() -> PyObjectRef {
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
