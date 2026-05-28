use super::*;

pub(super) fn operator_itemgetter(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("itemgetter", args, 1)?;
    let keys: Vec<PyObjectRef> = args.to_vec();
    Ok(PyObject::native_closure_with_pickle_args(
        "operator.itemgetter",
        keys.clone(),
        move |call_args| {
            check_args("itemgetter", call_args, 1)?;
            let obj = &call_args[0];
            if keys.len() == 1 {
                match obj.get_item(&keys[0]) {
                    Err(err) if err.kind == ExceptionKind::ValueError => {
                        Err(PyException::type_error(err.message))
                    }
                    result => result,
                }
            } else {
                let items: Vec<PyObjectRef> = keys
                    .iter()
                    .map(|k| match obj.get_item(k) {
                        Err(err) if err.kind == ExceptionKind::ValueError => {
                            Err(PyException::type_error(err.message))
                        }
                        result => result,
                    })
                    .collect::<PyResult<Vec<_>>>()?;
                Ok(PyObject::tuple(items))
            }
        },
    ))
}

pub(super) fn operator_attrgetter(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("attrgetter", args, 1)?;
    let mut attr_names = Vec::with_capacity(args.len());
    for arg in args {
        let name = arg
            .as_str()
            .ok_or_else(|| PyException::type_error("attribute name must be a string"))?;
        attr_names.push(name.to_string());
    }
    let pickle_args = attr_names
        .iter()
        .map(|name| PyObject::str_val(CompactString::from(name.as_str())))
        .collect();
    Ok(PyObject::native_closure_with_pickle_args(
        "operator.attrgetter",
        pickle_args,
        move |call_args| {
            check_args("attrgetter", call_args, 1)?;
            let obj = &call_args[0];
            let resolve = |name: &str, obj: &PyObjectRef| -> PyResult<PyObjectRef> {
                let parts: Vec<&str> = name.split('.').collect();
                let mut cur = obj.clone();
                for part in &parts {
                    if let Some(attr) = cur.get_attr(part) {
                        cur = attr;
                    } else if let Some(getattr) = cur.get_attr("__getattr__") {
                        cur = ferrython_core::object::call_callable(
                            &getattr,
                            &[PyObject::str_val(CompactString::from(*part))],
                        )?;
                    } else {
                        return Err(PyException::attribute_error(format!(
                            "'{}' object has no attribute '{}'",
                            cur.type_name(),
                            part
                        )));
                    }
                }
                Ok(cur)
            };
            if attr_names.len() == 1 {
                resolve(&attr_names[0], obj)
            } else {
                let items: Vec<PyObjectRef> = attr_names
                    .iter()
                    .map(|name| resolve(name, obj))
                    .collect::<PyResult<Vec<_>>>()?;
                Ok(PyObject::tuple(items))
            }
        },
    ))
}

pub(super) fn operator_methodcaller(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("methodcaller", args, 1)?;
    let method_name = args[0]
        .as_str()
        .ok_or_else(|| PyException::type_error("method name must be a string"))?
        .to_string();
    let extra_args: Vec<PyObjectRef> = if args.len() > 1 {
        args[1..].to_vec()
    } else {
        vec![]
    };
    let (extra_args, kw_args) = if let Some((last, positional)) = extra_args.split_last() {
        if let PyObjectPayload::Dict(map) = &last.payload {
            let kw_args = map
                .read()
                .iter()
                .map(|(k, v)| match k {
                    ferrython_core::types::HashableKey::Str(name) => {
                        Ok((name.to_compact_string(), v.clone()))
                    }
                    _ => Err(PyException::type_error(
                        "methodcaller keywords must be strings",
                    )),
                })
                .collect::<PyResult<Vec<_>>>()?;
            (positional.to_vec(), kw_args)
        } else {
            (extra_args, Vec::new())
        }
    } else {
        (extra_args, Vec::new())
    };
    let mut pickle_args = vec![PyObject::str_val(CompactString::from(method_name.as_str()))];
    pickle_args.extend(extra_args.iter().cloned());
    if !kw_args.is_empty() {
        let mut kw_map = ferrython_core::object::new_fx_hashkey_map();
        for (key, value) in &kw_args {
            kw_map.insert(
                ferrython_core::types::HashableKey::str_key(key.clone()),
                value.clone(),
            );
        }
        pickle_args.push(PyObject::dict(kw_map));
    }
    Ok(PyObject::native_closure_with_pickle_args(
        "operator.methodcaller",
        pickle_args,
        move |call_args| {
            check_args("methodcaller", call_args, 1)?;
            let obj = &call_args[0];
            let method = obj.get_attr(&method_name).ok_or_else(|| {
                PyException::attribute_error(format!(
                    "'{}' object has no attribute '{}'",
                    obj.type_name(),
                    method_name
                ))
            })?;
            let mut full_args = vec![obj.clone()];
            full_args.extend(extra_args.iter().cloned());
            match &method.payload {
                PyObjectPayload::NativeFunction(nf) => (nf.func)(&full_args),
                PyObjectPayload::NativeClosure(nc) => (nc.func)(&full_args),
                PyObjectPayload::BuiltinBoundMethod(bbm) => {
                    dispatch_builtin_bound_method(bbm, &method, &extra_args)
                }
                PyObjectPayload::BoundMethod {
                    receiver,
                    method: meth,
                    ..
                } => match &meth.payload {
                    PyObjectPayload::NativeFunction(nf) => {
                        let mut bound_args = vec![receiver.clone()];
                        bound_args.extend(extra_args.iter().cloned());
                        (nf.func)(&bound_args)
                    }
                    PyObjectPayload::NativeClosure(nc) => {
                        let mut bound_args = vec![receiver.clone()];
                        bound_args.extend(extra_args.iter().cloned());
                        (nc.func)(&bound_args)
                    }
                    _ => {
                        let mut bound_args = vec![receiver.clone()];
                        bound_args.extend(extra_args.iter().cloned());
                        ferrython_core::object::call_callable_kw(meth, &bound_args, kw_args.clone())
                    }
                },
                PyObjectPayload::Function(_) => {
                    let mut call_args_full = vec![obj.clone()];
                    call_args_full.extend(extra_args.iter().cloned());
                    ferrython_core::object::call_callable_kw(
                        &method,
                        &call_args_full,
                        kw_args.clone(),
                    )
                }
                _ => Ok(method),
            }
        },
    ))
}

fn dispatch_builtin_bound_method(
    bbm: &ferrython_core::object::BuiltinBoundMethodData,
    method: &PyObjectRef,
    extra_args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    if !extra_args.is_empty() {
        let s = bbm.receiver.py_to_string();
        match bbm.method_name.as_str() {
            "split" => {
                let sep = extra_args[0].py_to_string();
                let maxsplit = if extra_args.len() > 1 {
                    extra_args[1].as_int().unwrap_or(-1)
                } else {
                    -1
                };
                let parts: Vec<&str> = if maxsplit < 0 {
                    s.split(&*sep).collect()
                } else {
                    s.splitn(maxsplit as usize + 1, &*sep).collect()
                };
                return Ok(PyObject::list(
                    parts
                        .into_iter()
                        .map(|p| PyObject::str_val(CompactString::from(p)))
                        .collect(),
                ));
            }
            "rsplit" => {
                let sep = extra_args[0].py_to_string();
                let maxsplit = if extra_args.len() > 1 {
                    extra_args[1].as_int().unwrap_or(-1)
                } else {
                    -1
                };
                let parts: Vec<&str> = if maxsplit < 0 {
                    s.rsplit(&*sep).collect()
                } else {
                    s.rsplitn(maxsplit as usize + 1, &*sep).collect()
                };
                let mut result: Vec<PyObjectRef> = parts
                    .into_iter()
                    .map(|p| PyObject::str_val(CompactString::from(p)))
                    .collect();
                result.reverse();
                return Ok(PyObject::list(result));
            }
            "replace" => {
                let old = extra_args[0].py_to_string();
                let new = if extra_args.len() > 1 {
                    extra_args[1].py_to_string()
                } else {
                    String::new()
                };
                let count = if extra_args.len() > 2 {
                    extra_args[2].as_int().unwrap_or(-1)
                } else {
                    -1
                };
                let result = if count < 0 {
                    s.replace(&*old, &new)
                } else {
                    s.replacen(&*old, &new, count as usize)
                };
                return Ok(PyObject::str_val(CompactString::from(result)));
            }
            "join" => {
                let items = extra_args[0].to_list().unwrap_or_default();
                let joined: String = items
                    .iter()
                    .map(|i| i.py_to_string())
                    .collect::<Vec<_>>()
                    .join(&s);
                return Ok(PyObject::str_val(CompactString::from(joined)));
            }
            "encode" => return Ok(PyObject::bytes(s.into_bytes())),
            "find" => {
                let sub = extra_args[0].py_to_string();
                let idx = s.find(&*sub).map(|i| i as i64).unwrap_or(-1);
                return Ok(PyObject::int(idx));
            }
            "count" => {
                let sub = extra_args[0].py_to_string();
                return Ok(PyObject::int(s.matches(&*sub).count() as i64));
            }
            "startswith" => {
                let prefix = extra_args[0].py_to_string();
                return Ok(PyObject::bool_val(s.starts_with(&*prefix)));
            }
            "endswith" => {
                let suffix = extra_args[0].py_to_string();
                return Ok(PyObject::bool_val(s.ends_with(&*suffix)));
            }
            "center" | "ljust" | "rjust" => {
                let width = extra_args[0].as_int().unwrap_or(0) as usize;
                let fill = if extra_args.len() > 1 {
                    extra_args[1].py_to_string().chars().next().unwrap_or(' ')
                } else {
                    ' '
                };
                let result = if s.len() >= width {
                    s.clone()
                } else {
                    let pad = width - s.len();
                    match bbm.method_name.as_str() {
                        "center" => {
                            let left = pad / 2;
                            let right = pad - left;
                            format!(
                                "{}{}{}",
                                fill.to_string().repeat(left),
                                s,
                                fill.to_string().repeat(right)
                            )
                        }
                        "ljust" => format!("{}{}", s, fill.to_string().repeat(pad)),
                        "rjust" => format!("{}{}", fill.to_string().repeat(pad), s),
                        _ => s.clone(),
                    }
                };
                return Ok(PyObject::str_val(CompactString::from(result)));
            }
            _ => {
                crate::concurrency_modules::push_deferred_call(method.clone(), extra_args.to_vec());
                return Ok(PyObject::none());
            }
        }
    }
    let result_str = match bbm.method_name.as_str() {
        "upper" => Some(bbm.receiver.py_to_string().to_uppercase()),
        "lower" => Some(bbm.receiver.py_to_string().to_lowercase()),
        "strip" => Some(bbm.receiver.py_to_string().trim().to_string()),
        "lstrip" => Some(bbm.receiver.py_to_string().trim_start().to_string()),
        "rstrip" => Some(bbm.receiver.py_to_string().trim_end().to_string()),
        "title" => {
            let s = bbm.receiver.py_to_string();
            let mut result = String::with_capacity(s.len());
            let mut capitalize_next = true;
            for c in s.chars() {
                if c.is_alphanumeric() {
                    if capitalize_next {
                        result.extend(c.to_uppercase());
                        capitalize_next = false;
                    } else {
                        result.extend(c.to_lowercase());
                    }
                } else {
                    result.push(c);
                    capitalize_next = true;
                }
            }
            Some(result)
        }
        "capitalize" => {
            let s = bbm.receiver.py_to_string();
            let mut chars = s.chars();
            Some(match chars.next() {
                None => String::new(),
                Some(f) => {
                    f.to_uppercase().collect::<String>() + &chars.collect::<String>().to_lowercase()
                }
            })
        }
        "swapcase" => {
            let s = bbm.receiver.py_to_string();
            Some(
                s.chars()
                    .map(|c| {
                        if c.is_uppercase() {
                            c.to_lowercase().collect::<String>()
                        } else {
                            c.to_uppercase().collect::<String>()
                        }
                    })
                    .collect(),
            )
        }
        _ => None,
    };
    if let Some(s) = result_str {
        Ok(PyObject::str_val(CompactString::from(s)))
    } else {
        Ok(method.clone())
    }
}

pub(super) fn operator_length_hint(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("length_hint", args, 1)?;
    let default = if args.len() > 1 {
        args[1].to_int().unwrap_or(0)
    } else {
        0
    };
    let validate_hint = |value: PyObjectRef| -> PyResult<Option<i64>> {
        if matches!(&value.payload, PyObjectPayload::NotImplemented) {
            return Ok(None);
        }
        let n = value
            .as_int()
            .ok_or_else(|| PyException::type_error("__length_hint__ must be an integer"))?;
        if n < 0 {
            return Err(PyException::value_error(
                "__length_hint__() should return >= 0",
            ));
        }
        Ok(Some(n))
    };
    for dunder in &["__length_hint__", "__len__"] {
        if let Some(method) = args[0].get_attr(dunder) {
            match ferrython_core::object::call_callable(&method, &[]) {
                Ok(value) => {
                    if let Some(n) = validate_hint(value)? {
                        return Ok(PyObject::int(n));
                    }
                    return Ok(PyObject::int(default));
                }
                Err(err) if err.kind == ExceptionKind::TypeError => {
                    return Ok(PyObject::int(default));
                }
                Err(err) => return Err(err),
            }
        }
    }
    match args[0].py_len() {
        Ok(n) => Ok(PyObject::int(n as i64)),
        Err(_) => Ok(PyObject::int(default)),
    }
}
