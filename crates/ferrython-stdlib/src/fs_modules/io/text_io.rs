use super::*;

/// TextIOWrapper.__init__: installs buffer-delegating methods on self.
/// Called as __init__(self, buffer, encoding='utf-8', errors='strict', ...)
pub(super) fn io_text_io_wrapper_init(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // args[0] = self, args[1] = buffer, optional encoding/kwargs
    if args.len() < 2 {
        return Err(PyException::type_error(
            "TextIOWrapper.__init__() requires a buffer argument",
        ));
    }
    let self_obj = args[0].clone();
    let buffer = args[1].clone();
    let encoding = if args.len() > 2 {
        args[2].py_to_string()
    } else {
        "utf-8".to_string()
    };
    // Extract kwargs if trailing dict
    let (enc, _errors) = if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(kw) = &last.payload {
            let r = kw.read();
            let e = r
                .get(&HashableKey::str_key(CompactString::from("encoding")))
                .map(|v| v.py_to_string())
                .unwrap_or(encoding);
            let er = r
                .get(&HashableKey::str_key(CompactString::from("errors")))
                .map(|v| v.py_to_string())
                .unwrap_or_else(|| "strict".to_string());
            (e, er)
        } else {
            (encoding, "strict".to_string())
        }
    } else {
        (encoding, "strict".to_string())
    };

    if let PyObjectPayload::Instance(inst_data) = &self_obj.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("buffer"), buffer.clone());
        attrs.insert(
            CompactString::from("encoding"),
            PyObject::str_val(CompactString::from(&enc)),
        );
        attrs.insert(
            CompactString::from("mode"),
            PyObject::str_val(CompactString::from("r")),
        );
        attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));
        attrs.insert(
            CompactString::from("name"),
            PyObject::str_val(CompactString::from("<TextIOWrapper>")),
        );

        // read(size=-1) — decode bytes from buffer
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("read"),
            PyObject::native_closure("TextIOWrapper.read", move |a: &[PyObjectRef]| {
                let size = if a.is_empty() {
                    -1i64
                } else {
                    a[0].as_int().unwrap_or(-1)
                };
                if let Some(read_fn) = buf.get_attr("read") {
                    let bytes_result = if size < 0 {
                        call_native(&read_fn, &[])?
                    } else {
                        call_native(&read_fn, &[PyObject::int(size)])?
                    };
                    if let PyObjectPayload::Bytes(b) = &bytes_result.payload {
                        Ok(PyObject::str_val(CompactString::from(
                            String::from_utf8_lossy(b).as_ref(),
                        )))
                    } else {
                        Ok(bytes_result)
                    }
                } else {
                    Err(PyException::type_error("buffer has no read method"))
                }
            }),
        );

        // write(s) — encode str to bytes and write to buffer (rejects bytes like CPython)
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("write"),
            PyObject::native_closure("TextIOWrapper.write", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("write() requires 1 argument"));
                }
                // TextIOWrapper only accepts str, not bytes
                if matches!(&a[0].payload, PyObjectPayload::Bytes(_)) {
                    return Err(PyException::type_error(
                        "write() argument must be str, not bytes",
                    ));
                }
                let text = a[0].py_to_string();
                let bytes_obj = PyObject::bytes(text.as_bytes().to_vec());
                if let Some(write_fn) = buf.get_attr("write") {
                    call_native(&write_fn, &[bytes_obj])
                } else {
                    Err(PyException::type_error("buffer has no write method"))
                }
            }),
        );

        // readline() — read line from buffer
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("readline"),
            PyObject::native_closure("TextIOWrapper.readline", move |_: &[PyObjectRef]| {
                if let Some(readline_fn) = buf.get_attr("readline") {
                    let result = call_native(&readline_fn, &[])?;
                    if let PyObjectPayload::Bytes(b) = &result.payload {
                        Ok(PyObject::str_val(CompactString::from(
                            String::from_utf8_lossy(b).as_ref(),
                        )))
                    } else {
                        Ok(result)
                    }
                } else {
                    Err(PyException::type_error("buffer has no readline method"))
                }
            }),
        );

        // readlines(hint=-1) — read all lines
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("readlines"),
            PyObject::native_closure("TextIOWrapper.readlines", move |a: &[PyObjectRef]| {
                let hint = if a.is_empty() {
                    -1i64
                } else {
                    a[0].as_int().unwrap_or(-1)
                };
                let mut lines = Vec::new();
                let mut total_bytes = 0i64;
                loop {
                    if let Some(readline_fn) = buf.get_attr("readline") {
                        let result = call_native(&readline_fn, &[])?;
                        let line_str = if let PyObjectPayload::Bytes(b) = &result.payload {
                            String::from_utf8_lossy(b).to_string()
                        } else {
                            result.py_to_string()
                        };
                        if line_str.is_empty() {
                            break;
                        }
                        total_bytes += line_str.len() as i64;
                        lines.push(PyObject::str_val(CompactString::from(line_str)));
                        if hint > 0 && total_bytes >= hint {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                Ok(PyObject::list(lines))
            }),
        );

        // writelines(lines) — write an iterable of strings
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("writelines"),
            PyObject::native_closure("TextIOWrapper.writelines", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("writelines() requires 1 argument"));
                }
                if let Some(write_fn) = buf.get_attr("write") {
                    if let PyObjectPayload::List(items) = &a[0].payload {
                        for item in items.read().iter() {
                            let text = item.py_to_string();
                            let bytes_obj = PyObject::bytes(text.as_bytes().to_vec());
                            call_native(&write_fn, &[bytes_obj])?;
                        }
                    }
                }
                Ok(PyObject::none())
            }),
        );

        // seek/tell — delegate to buffer
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("seek"),
            PyObject::native_closure("TextIOWrapper.seek", move |a: &[PyObjectRef]| {
                if let Some(seek_fn) = buf.get_attr("seek") {
                    call_native(&seek_fn, a)
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("tell"),
            PyObject::native_closure("TextIOWrapper.tell", move |_: &[PyObjectRef]| {
                if let Some(tell_fn) = buf.get_attr("tell") {
                    call_native(&tell_fn, &[])
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );

        // flush — delegate to buffer
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("flush"),
            PyObject::native_closure("TextIOWrapper.flush", move |_: &[PyObjectRef]| {
                if let Some(flush_fn) = buf.get_attr("flush") {
                    call_native(&flush_fn, &[])
                } else {
                    Ok(PyObject::none())
                }
            }),
        );

        // readable/writable/seekable
        attrs.insert(
            CompactString::from("readable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        attrs.insert(
            CompactString::from("writable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        attrs.insert(
            CompactString::from("seekable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );

        // close — delegate to buffer and mark closed
        let buf = buffer.clone();
        let inst_for_close = self_obj.clone();
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("TextIOWrapper.close", move |_| {
                if let Some(close_fn) = buf.get_attr("close") {
                    let _ = call_native(&close_fn, &[]);
                }
                if let PyObjectPayload::Instance(ref d) = inst_for_close.payload {
                    d.attrs
                        .write()
                        .insert(CompactString::from("closed"), PyObject::bool_val(true));
                }
                Ok(PyObject::none())
            }),
        );

        // __enter__ / __exit__
        let inst_ref = self_obj.clone();
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("TextIOWrapper.__enter__", move |_| Ok(inst_ref.clone())),
        );
        let inst_for_exit = self_obj.clone();
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("__exit__"),
            PyObject::native_closure("TextIOWrapper.__exit__", move |_| {
                if let Some(close_fn) = buf.get_attr("close") {
                    let _ = call_native(&close_fn, &[]);
                }
                if let PyObjectPayload::Instance(ref d) = inst_for_exit.payload {
                    d.attrs
                        .write()
                        .insert(CompactString::from("closed"), PyObject::bool_val(true));
                }
                Ok(PyObject::bool_val(false))
            }),
        );

        // getvalue() — delegate to buffer (common for StringIO/BytesIO wrappers)
        let buf = buffer.clone();
        attrs.insert(
            CompactString::from("getvalue"),
            PyObject::native_closure("TextIOWrapper.getvalue", move |_: &[PyObjectRef]| {
                if let Some(gv) = buf.get_attr("getvalue") {
                    let result = call_native(&gv, &[])?;
                    if let PyObjectPayload::Bytes(b) = &result.payload {
                        Ok(PyObject::str_val(CompactString::from(
                            String::from_utf8_lossy(b).as_ref(),
                        )))
                    } else {
                        Ok(result)
                    }
                } else {
                    Err(PyException::attribute_error(
                        "underlying buffer has no getvalue",
                    ))
                }
            }),
        );
    }
    Ok(PyObject::none())
}
