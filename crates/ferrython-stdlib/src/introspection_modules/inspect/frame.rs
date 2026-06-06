use super::*;

fn frame_info_class() -> PyObjectRef {
    let cls = PyObject::class(CompactString::from("FrameInfo"), vec![], IndexMap::new());
    if let PyObjectPayload::Class(cd) = &cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(
            CompactString::from("__getitem__"),
            PyObject::native_closure("FrameInfo.__getitem__", |args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Err(PyException::type_error("__getitem__ requires index"));
                }
                let Some(idx) = args[1].as_int() else {
                    return Err(PyException::type_error("tuple indices must be integers"));
                };
                let key = match idx {
                    0 | -6 => "frame",
                    1 | -5 => "filename",
                    2 | -4 => "lineno",
                    3 | -3 => "function",
                    4 | -2 => "code_context",
                    5 | -1 => "index",
                    _ => return Err(PyException::index_error("tuple index out of range")),
                };
                Ok(args[0].get_attr(key).unwrap_or_else(PyObject::none))
            }),
        );
    }
    cls
}

fn frame_code_attr(frame: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    frame
        .get_attr("f_code")
        .and_then(|code| code.get_attr(name))
}

fn frame_info_from_frame(cls: &PyObjectRef, frame: PyObjectRef) -> PyObjectRef {
    let filename = frame_code_attr(&frame, "co_filename")
        .unwrap_or_else(|| PyObject::str_val(CompactString::from("<unknown>")));
    let function = frame_code_attr(&frame, "co_name")
        .unwrap_or_else(|| PyObject::str_val(CompactString::from("<module>")));
    let lineno = frame
        .get_attr("f_lineno")
        .unwrap_or_else(|| PyObject::int(0));

    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("frame"), frame);
    attrs.insert(CompactString::from("filename"), filename);
    attrs.insert(CompactString::from("lineno"), lineno);
    attrs.insert(CompactString::from("function"), function);
    attrs.insert(CompactString::from("code_context"), PyObject::none());
    attrs.insert(CompactString::from("index"), PyObject::none());
    PyObject::instance_with_attrs(cls.clone(), attrs)
}

pub(super) fn inspect_currentframe(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if let Some(frame) = crate::get_current_frame() {
        return Ok(frame);
    }

    let cls = PyObject::class(CompactString::from("frame"), vec![], IndexMap::new());
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("f_lineno"), PyObject::int(0));
    attrs.insert(CompactString::from("f_code"), {
        let code_cls = PyObject::class(CompactString::from("code"), vec![], IndexMap::new());
        let mut code_attrs = IndexMap::new();
        code_attrs.insert(
            CompactString::from("co_filename"),
            PyObject::str_val(CompactString::from("<unknown>")),
        );
        code_attrs.insert(
            CompactString::from("co_name"),
            PyObject::str_val(CompactString::from("<module>")),
        );
        code_attrs.insert(CompactString::from("co_firstlineno"), PyObject::int(0));
        PyObject::instance_with_attrs(code_cls, code_attrs)
    });
    attrs.insert(
        CompactString::from("f_locals"),
        PyObject::dict(IndexMap::new()),
    );
    attrs.insert(
        CompactString::from("f_globals"),
        PyObject::dict(IndexMap::new()),
    );
    attrs.insert(CompactString::from("f_back"), PyObject::none());
    Ok(PyObject::instance_with_attrs(cls, attrs))
}

pub(super) fn inspect_stack(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let cls = frame_info_class();
    if let Some(frame) = crate::get_current_frame() {
        let mut out = Vec::new();
        let mut current = frame;
        let mut depth = 0usize;
        loop {
            out.push(frame_info_from_frame(&cls, current.clone()));
            depth += 1;
            if depth >= 64 {
                break;
            }
            let Some(back) = current.get_attr("f_back") else {
                break;
            };
            if matches!(&back.payload, PyObjectPayload::None) {
                break;
            }
            current = back;
        }
        return Ok(PyObject::list(out));
    }

    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("frame"), PyObject::none());
    attrs.insert(
        CompactString::from("filename"),
        PyObject::str_val(CompactString::from("<unknown>")),
    );
    attrs.insert(CompactString::from("lineno"), PyObject::int(0));
    attrs.insert(
        CompactString::from("function"),
        PyObject::str_val(CompactString::from("<module>")),
    );
    attrs.insert(CompactString::from("code_context"), PyObject::none());
    attrs.insert(CompactString::from("index"), PyObject::none());
    let frame_info = PyObject::instance_with_attrs(cls, attrs);
    Ok(PyObject::list(vec![frame_info]))
}
