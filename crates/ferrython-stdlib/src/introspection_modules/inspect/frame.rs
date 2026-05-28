use super::*;

pub(super) fn inspect_currentframe(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
    let cls = PyObject::class(CompactString::from("FrameInfo"), vec![], IndexMap::new());
    let mut attrs = IndexMap::new();
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
