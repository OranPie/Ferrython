use super::*;

pub(in crate::collection_modules) fn make_user_string_class() -> PyObjectRef {
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
