use super::*;

/// BufferedReader: wraps a raw binary stream with buffering
pub(super) fn io_buffered_reader(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "BufferedReader() requires a raw stream",
        ));
    }
    let raw = args[0].clone();
    let cls = PyObject::class(
        CompactString::from("BufferedReader"),
        vec![],
        IndexMap::new(),
    );
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("raw"), raw.clone());

        let r = raw.clone();
        attrs.insert(
            CompactString::from("read"),
            PyObject::native_closure("BufferedReader.read", move |a: &[PyObjectRef]| {
                if let Some(read_fn) = r.get_attr("read") {
                    call_native(&read_fn, a)
                } else {
                    Err(PyException::type_error("raw stream has no read method"))
                }
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("readline"),
            PyObject::native_closure("BufferedReader.readline", move |a: &[PyObjectRef]| {
                if let Some(readline_fn) = r.get_attr("readline") {
                    call_native(&readline_fn, a)
                } else {
                    Err(PyException::type_error("raw stream has no readline method"))
                }
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("readlines"),
            PyObject::native_closure("BufferedReader.readlines", move |_: &[PyObjectRef]| {
                let mut lines = Vec::new();
                loop {
                    if let Some(readline_fn) = r.get_attr("readline") {
                        let result = call_native(&readline_fn, &[])?;
                        let is_empty = match &result.payload {
                            PyObjectPayload::Bytes(b) => b.is_empty(),
                            _ => result.py_to_string().is_empty(),
                        };
                        if is_empty {
                            break;
                        }
                        lines.push(result);
                    } else {
                        break;
                    }
                }
                Ok(PyObject::list(lines))
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("seek"),
            PyObject::native_closure("BufferedReader.seek", move |a: &[PyObjectRef]| {
                if let Some(seek_fn) = r.get_attr("seek") {
                    call_native(&seek_fn, a)
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("tell"),
            PyObject::native_closure("BufferedReader.tell", move |_: &[PyObjectRef]| {
                if let Some(tell_fn) = r.get_attr("tell") {
                    call_native(&tell_fn, &[])
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );

        attrs.insert(
            CompactString::from("readable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        attrs.insert(
            CompactString::from("writable"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );
        let inst_for_close = inst.clone();
        let r = raw.clone();
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("BufferedReader.close", move |_| {
                if let Some(close_fn) = r.get_attr("close") {
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

        let inst_ref = inst.clone();
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("BufferedReader.__enter__", move |_| Ok(inst_ref.clone())),
        );
        attrs.insert(
            CompactString::from("__exit__"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );
    }
    Ok(inst)
}

/// BufferedWriter: wraps a raw binary stream with write buffering
pub(super) fn io_buffered_writer(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "BufferedWriter() requires a raw stream",
        ));
    }
    let raw = args[0].clone();
    let cls = PyObject::class(
        CompactString::from("BufferedWriter"),
        vec![],
        IndexMap::new(),
    );
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("raw"), raw.clone());

        let r = raw.clone();
        attrs.insert(
            CompactString::from("write"),
            PyObject::native_closure("BufferedWriter.write", move |a: &[PyObjectRef]| {
                if let Some(write_fn) = r.get_attr("write") {
                    call_native(&write_fn, a)
                } else {
                    Err(PyException::type_error("raw stream has no write method"))
                }
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("flush"),
            PyObject::native_closure("BufferedWriter.flush", move |_: &[PyObjectRef]| {
                if let Some(flush_fn) = r.get_attr("flush") {
                    call_native(&flush_fn, &[])
                } else {
                    Ok(PyObject::none())
                }
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("seek"),
            PyObject::native_closure("BufferedWriter.seek", move |a: &[PyObjectRef]| {
                if let Some(seek_fn) = r.get_attr("seek") {
                    call_native(&seek_fn, a)
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );

        let r = raw.clone();
        attrs.insert(
            CompactString::from("tell"),
            PyObject::native_closure("BufferedWriter.tell", move |_: &[PyObjectRef]| {
                if let Some(tell_fn) = r.get_attr("tell") {
                    call_native(&tell_fn, &[])
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );

        attrs.insert(
            CompactString::from("readable"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );
        attrs.insert(
            CompactString::from("writable"),
            make_builtin(|_| Ok(PyObject::bool_val(true))),
        );
        let inst_for_close = inst.clone();
        let r = raw;
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("BufferedWriter.close", move |_| {
                if let Some(flush_fn) = r.get_attr("flush") {
                    let _ = call_native(&flush_fn, &[]);
                }
                if let Some(close_fn) = r.get_attr("close") {
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

        let inst_ref = inst.clone();
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("BufferedWriter.__enter__", move |_| Ok(inst_ref.clone())),
        );
        attrs.insert(
            CompactString::from("__exit__"),
            make_builtin(|_| Ok(PyObject::bool_val(false))),
        );
    }
    Ok(inst)
}
