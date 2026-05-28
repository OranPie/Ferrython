use super::*;
use ferrython_core::object::PyCell;
use std::rc::Rc;
use std::sync::Arc;

pub(super) fn codecs_open(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("codecs.open", args, 1)?;
    let filename = args[0].py_to_string();
    let mode = if args.len() > 1 && !matches!(args[1].payload, PyObjectPayload::Dict(_)) {
        args[1].py_to_string()
    } else {
        "r".to_string()
    };
    let _encoding = if args.len() > 2 && !matches!(args[2].payload, PyObjectPayload::Dict(_)) {
        args[2].py_to_string()
    } else {
        "utf-8".to_string()
    };

    if mode.contains('w') {
        let _ = std::fs::File::create(&filename)
            .map_err(|e| PyException::os_error(format!("{}: {}", e, filename)))?;
        let mut attrs = IndexMap::new();
        let path = filename.clone();
        let buf = Rc::new(PyCell::new(String::new()));
        let buf_w = buf.clone();
        let buf_r = buf.clone();
        let path_w = path.clone();
        attrs.insert(
            CompactString::from("write"),
            PyObject::native_closure("write", move |wargs: &[PyObjectRef]| {
                if let Some(s) = wargs.first() {
                    buf_w.write().push_str(&s.py_to_string());
                }
                Ok(PyObject::none())
            }),
        );
        attrs.insert(
            CompactString::from("flush"),
            PyObject::native_closure("flush", move |_| {
                let content = buf_r.read().clone();
                std::fs::write(&path_w, content.as_bytes())
                    .map_err(|e| PyException::os_error(e.to_string()))?;
                Ok(PyObject::none())
            }),
        );
        let path_c = path.clone();
        let buf_c = buf.clone();
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("close", move |_| {
                let content = buf_c.read().clone();
                std::fs::write(&path_c, content.as_bytes())
                    .map_err(|e| PyException::os_error(e.to_string()))?;
                Ok(PyObject::none())
            }),
        );
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_function("__enter__", |a: &[PyObjectRef]| {
                Ok(if !a.is_empty() {
                    a[0].clone()
                } else {
                    PyObject::none()
                })
            }),
        );
        let path_e = path.clone();
        let buf_e = buf.clone();
        attrs.insert(
            CompactString::from("__exit__"),
            PyObject::native_closure("__exit__", move |_| {
                let content = buf_e.read().clone();
                let _ = std::fs::write(&path_e, content.as_bytes());
                Ok(PyObject::bool_val(false))
            }),
        );
        Ok(PyObject::module_with_attrs(
            CompactString::from("TextIOWrapper"),
            attrs,
        ))
    } else {
        let content = std::fs::read_to_string(&filename)
            .map_err(|e| PyException::os_error(format!("{}: {}", e, filename)))?;
        let content_arc = Arc::new(content);
        let c1 = content_arc.clone();
        let c2 = content_arc.clone();
        let mut attrs = IndexMap::new();
        attrs.insert(
            CompactString::from("read"),
            PyObject::native_closure("read", move |_| {
                Ok(PyObject::str_val(CompactString::from(c1.as_str())))
            }),
        );
        attrs.insert(
            CompactString::from("readlines"),
            PyObject::native_closure("readlines", move |_| {
                let lines: Vec<PyObjectRef> = c2
                    .lines()
                    .map(|l| PyObject::str_val(CompactString::from(format!("{}\n", l))))
                    .collect();
                Ok(PyObject::list(lines))
            }),
        );
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_function("close", |_: &[PyObjectRef]| Ok(PyObject::none())),
        );
        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_function("__enter__", |a: &[PyObjectRef]| {
                Ok(if !a.is_empty() {
                    a[0].clone()
                } else {
                    PyObject::none()
                })
            }),
        );
        attrs.insert(
            CompactString::from("__exit__"),
            PyObject::native_function("__exit__", |_: &[PyObjectRef]| {
                Ok(PyObject::bool_val(false))
            }),
        );
        Ok(PyObject::module_with_attrs(
            CompactString::from("TextIOWrapper"),
            attrs,
        ))
    }
}
