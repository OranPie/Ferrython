use super::*;
use ferrython_core::types::PyInt;
use num_traits::Signed;
use std::rc::Rc;

fn saturated_index(obj: &PyObjectRef) -> PyResult<i64> {
    Ok(match obj.to_index()? {
        PyInt::Small(n) => n,
        PyInt::Big(n) if n.is_negative() => i64::MIN,
        PyInt::Big(_) => i64::MAX,
    })
}

fn slice_bounds(
    len: i64,
    start: &Option<PyObjectRef>,
    stop: &Option<PyObjectRef>,
    step: &Option<PyObjectRef>,
) -> PyResult<(i64, i64, i64)> {
    let step_val = step.as_ref().map(saturated_index).transpose()?.unwrap_or(1);
    if step_val == 0 {
        return Err(PyException::value_error("slice step cannot be zero"));
    }
    let start_default = if step_val > 0 { 0 } else { len - 1 };
    let stop_default = if step_val > 0 { len } else { -len - 1 };
    let start_val = start
        .as_ref()
        .map(saturated_index)
        .transpose()?
        .unwrap_or(start_default);
    let stop_val = stop
        .as_ref()
        .map(saturated_index)
        .transpose()?
        .unwrap_or(stop_default);
    let start_idx = if start_val < 0 {
        len.saturating_add(start_val)
            .max(if step_val > 0 { 0 } else { -1 })
    } else {
        start_val.min(len)
    };
    let stop_idx = if stop_val < 0 {
        len.saturating_add(stop_val)
            .max(if step_val > 0 { 0 } else { -1 })
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
            let Some(next) = i.checked_add(step) else {
                break;
            };
            i = next;
        }
    } else {
        while i > stop {
            if i >= 0 && i < len {
                indices.push(i as usize);
            }
            let Some(next) = i.checked_add(step) else {
                break;
            };
            i = next;
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
            let Some(next) = i.checked_add(step) else {
                break;
            };
            i = next;
        }
    } else {
        while i > stop {
            if i >= 0 && i < len {
                indices.push(i as usize);
            }
            let Some(next) = i.checked_add(step) else {
                break;
            };
            i = next;
        }
    }
    indices.sort_unstable_by(|a, b| b.cmp(a));
    for idx in indices {
        items.remove(idx);
    }
    Ok(())
}

fn userlist_class(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        Ok(inst.class.clone())
    } else {
        Err(PyException::type_error(
            "UserList method requires an instance",
        ))
    }
}

fn build_userlist_from_items(
    owner_class: PyObjectRef,
    items: Vec<PyObjectRef>,
) -> PyResult<PyObjectRef> {
    let data = PyObject::list(items);
    let inst = PyObject::instance(owner_class);
    if let PyObjectPayload::Instance(ref dst_inst) = inst.payload {
        dst_inst
            .attrs
            .write()
            .insert(CompactString::from("data"), data.clone());
        install_list_methods(&dst_inst.attrs, &data, dst_inst.class.clone());
    }
    Ok(inst)
}

fn is_kwargs_dict(obj: &PyObjectRef) -> bool {
    let PyObjectPayload::Dict(map) = &obj.payload else {
        return false;
    };
    map.read().keys().any(|key| match key {
        ferrython_core::types::HashableKey::Str(s) => matches!(s.as_str(), "key" | "reverse"),
        _ => false,
    })
}

fn kwarg_value(kwargs: Option<&PyObjectRef>, name: &str) -> Option<PyObjectRef> {
    let PyObjectPayload::Dict(map) = &kwargs?.payload else {
        return None;
    };
    map.read()
        .get(&HashableKey::str_key(CompactString::from(name)))
        .cloned()
}

fn userlist_item_matches(item: &PyObjectRef, target: &PyObjectRef) -> PyResult<bool> {
    if PyObjectRef::ptr_eq(item, target) {
        return Ok(true);
    }
    Ok(item.compare(target, CompareOp::Eq)?.is_truthy())
}

fn userlist_compare(args: &[PyObjectRef], op: CompareOp) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(matches!(op, CompareOp::Ne)));
    }
    let data = get_user_data(&args[0], "data")?;
    let other = if let Ok(od) = get_user_data(&args[1], "data") {
        od
    } else {
        args[1].clone()
    };
    data.compare(&other, op)
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
            if let PyObjectPayload::Slice(_) = &args[1].payload {
                let sliced = data.get_item(&args[1])?;
                return build_userlist_from_items(userlist_class(&args[0])?, sliced.to_list()?);
            }
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
                let idx = args[1]
                    .to_index()
                    .map_err(|_| PyException::type_error("list indices must be integers or slices"))
                    .and_then(|index| {
                        Ok(match index {
                            PyInt::Small(n) => n,
                            PyInt::Big(n) if n.is_negative() => i64::MIN,
                            PyInt::Big(_) => i64::MAX,
                        })
                    })?;
                let mut w = l.write();
                let len = w.len() as i64;
                let i = if idx < 0 { len + idx } else { idx };
                if i < 0 || i >= len {
                    return Err(PyException::index_error(
                        "list assignment index out of range",
                    ));
                }
                w[i as usize] = args[2].clone();
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
                for item in l.read().iter() {
                    if userlist_item_matches(item, target)? {
                        return Ok(PyObject::bool_val(true));
                    }
                }
                Ok(PyObject::bool_val(false))
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
            if args.is_empty() {
                return Err(PyException::type_error("__iter__ requires self"));
            }
            Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(ferrython_core::object::IteratorData::SeqIter {
                    obj: args[0].clone(),
                    index: 0,
                    exhausted: false,
                }),
            ))))
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
                let idx = args[1]
                    .to_index()
                    .map_err(|_| PyException::type_error("list indices must be integers or slices"))
                    .and_then(|index| {
                        Ok(match index {
                            PyInt::Small(n) => n,
                            PyInt::Big(n) if n.is_negative() => i64::MIN,
                            PyInt::Big(_) => i64::MAX,
                        })
                    })?;
                let mut w = l.write();
                let len = w.len() as i64;
                let i = if idx < 0 { len + idx } else { idx };
                if i < 0 || i >= len {
                    Err(PyException::index_error(
                        "list assignment index out of range",
                    ))
                } else {
                    w.remove(i as usize);
                    Ok(PyObject::none())
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
            build_userlist_from_items(userlist_class(&args[0])?, items)
        }),
    );
    ns.insert(
        CompactString::from("__radd__"),
        native_method("UserList", "__radd__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected other"));
            }
            let mut items = args[1].to_list()?;
            let data = get_user_data(&args[0], "data")?;
            items.extend(data.to_list()?);
            build_userlist_from_items(userlist_class(&args[0])?, items)
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
            build_userlist_from_items(userlist_class(&args[0])?, result)
        }),
    );
    ns.insert(
        CompactString::from("__rmul__"),
        native_method("UserList", "__rmul__", |args| {
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
            build_userlist_from_items(userlist_class(&args[0])?, result)
        }),
    );
    ns.insert(
        CompactString::from("__imul__"),
        native_method("UserList", "__imul__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected int"));
            }
            let n = args[1].to_int()?.max(0) as usize;
            let data = get_user_data(&args[0], "data")?;
            if let PyObjectPayload::List(l) = &data.payload {
                let original = l.read().clone();
                let mut result = Vec::with_capacity(original.len() * n);
                for _ in 0..n {
                    result.extend(original.iter().cloned());
                }
                *l.write() = result;
            }
            Ok(args[0].clone())
        }),
    );
    ns.insert(
        CompactString::from("__copy__"),
        native_method("UserList", "__copy__", copy_userlist_instance),
    );
    ns.insert(
        CompactString::from("__eq__"),
        native_method("UserList", "__eq__", |args| {
            userlist_compare(args, CompareOp::Eq)
        }),
    );
    ns.insert(
        CompactString::from("__ne__"),
        native_method("UserList", "__ne__", |args| {
            userlist_compare(args, CompareOp::Ne)
        }),
    );
    ns.insert(
        CompactString::from("__lt__"),
        native_method("UserList", "__lt__", |args| {
            userlist_compare(args, CompareOp::Lt)
        }),
    );
    ns.insert(
        CompactString::from("__le__"),
        native_method("UserList", "__le__", |args| {
            userlist_compare(args, CompareOp::Le)
        }),
    );
    ns.insert(
        CompactString::from("__gt__"),
        native_method("UserList", "__gt__", |args| {
            userlist_compare(args, CompareOp::Gt)
        }),
    );
    ns.insert(
        CompactString::from("__ge__"),
        native_method("UserList", "__ge__", |args| {
            userlist_compare(args, CompareOp::Ge)
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
            let idx = args[0].to_int()? as i64;
            if let PyObjectPayload::List(items) = &l.payload {
                let mut w = items.write();
                let len = w.len() as i64;
                let idx = if idx < 0 {
                    (len + idx).max(0)
                } else {
                    idx.min(len)
                };
                w.insert(idx as usize, args[1].clone());
            }
            Ok(PyObject::none())
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("pop"),
        PyObject::native_closure("pop", move |args| {
            if let PyObjectPayload::List(items) = &l.payload {
                if args.len() > 1 {
                    return Err(PyException::type_error("pop expected at most 1 argument"));
                }
                let mut w = items.write();
                if w.is_empty() {
                    return Err(PyException::index_error("pop from empty list"));
                }
                let idx = if !args.is_empty() {
                    let i = args[0].to_int()? as i64;
                    let len = w.len() as i64;
                    let resolved = if i < 0 { len + i } else { i };
                    if resolved < 0 || resolved >= len {
                        return Err(PyException::index_error("pop index out of range"));
                    }
                    resolved as usize
                } else {
                    w.len() - 1
                };
                Ok(w.remove(idx))
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
                let snapshot = w.clone();
                for (pos, item) in snapshot.iter().enumerate() {
                    if userlist_item_matches(item, target)? {
                        if pos < w.len() {
                            w.remove(pos);
                        }
                        return Ok(PyObject::none());
                    }
                }
                Err(PyException::value_error("list.remove(x): x not in list"))
            } else {
                Err(PyException::type_error("not a list"))
            }
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("clear"),
        PyObject::native_closure("clear", move |args| {
            if !args.is_empty() {
                return Err(PyException::type_error("clear expected no arguments"));
            }
            if let PyObjectPayload::List(items) = &l.payload {
                items.write().clear();
            }
            Ok(PyObject::none())
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("reverse"),
        PyObject::native_closure("reverse", move |args| {
            if !args.is_empty() {
                return Err(PyException::type_error("reverse expected no arguments"));
            }
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
                let mut count = 0usize;
                for item in items.read().iter() {
                    if userlist_item_matches(item, target)? {
                        count += 1;
                    }
                }
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
                let len = items.read().len() as i64;
                let normalize = |idx: usize, default: i64| -> PyResult<i64> {
                    if args.len() <= idx {
                        return Ok(default);
                    }
                    let raw = saturated_index(&args[idx])?;
                    Ok(if raw < 0 {
                        (len + raw).max(0)
                    } else {
                        raw.min(len)
                    })
                };
                let start = normalize(1, 0)? as usize;
                let stop = normalize(2, len)? as usize;
                let mut i = start;
                while i < stop {
                    let x = {
                        let r = items.read();
                        if i >= r.len() {
                            break;
                        }
                        r[i].clone()
                    };
                    if userlist_item_matches(&x, target)? {
                        return Ok(PyObject::int(i as i64));
                    }
                    i += 1;
                }
            }
            Err(PyException::value_error("x not in list"))
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("sort"),
        PyObject::native_closure("sort", move |args| {
            let pos_len = if args.last().is_some_and(is_kwargs_dict) {
                args.len() - 1
            } else {
                args.len()
            };
            if pos_len > 0 {
                return Err(PyException::type_error(
                    "sort expected no positional arguments",
                ));
            }
            if let PyObjectPayload::List(items) = &l.payload {
                let kwargs = args.last().filter(|arg| is_kwargs_dict(arg));
                let key_fn = kwarg_value(kwargs, "key")
                    .filter(|value| !matches!(value.payload, PyObjectPayload::None));
                let reverse = kwarg_value(kwargs, "reverse")
                    .map(|value| value.is_truthy())
                    .unwrap_or(false);
                let original = items.read().clone();
                let mut sorted = original.clone();
                let mut decorated = Vec::with_capacity(sorted.len());
                if let Some(key) = key_fn {
                    for item in sorted.into_iter() {
                        decorated.push((call_callable(&key, &[item.clone()])?, item));
                    }
                } else {
                    decorated = sorted
                        .into_iter()
                        .map(|item| (item.clone(), item))
                        .collect();
                }
                decorated.sort_by(|a, b| {
                    a.0.compare(&b.0, CompareOp::Lt)
                        .map_or(std::cmp::Ordering::Equal, |v| {
                            if v.is_truthy() {
                                std::cmp::Ordering::Less
                            } else {
                                std::cmp::Ordering::Greater
                            }
                        })
                });
                sorted = decorated.into_iter().map(|(_, item)| item).collect();
                if reverse {
                    sorted.reverse();
                }
                let mut w = items.write();
                if w.len() != original.len()
                    || w.iter()
                        .zip(original.iter())
                        .any(|(left, right)| !PyObjectRef::ptr_eq(left, right))
                {
                    *w = original;
                    return Err(PyException::value_error("list modified during sort"));
                }
                *w = sorted;
            }
            Ok(PyObject::none())
        }),
    );
    attrs.write().insert(CompactString::from("copy"), {
        let attrs = attrs.clone();
        let data = data.clone();
        let owner_class = owner_class.clone();
        PyObject::native_closure("copy", move |args| {
            if !args.is_empty() {
                return Err(PyException::type_error("copy expected no arguments"));
            }
            build_userlist_copy(&data, owner_class.clone(), &attrs)
        })
    });
}
