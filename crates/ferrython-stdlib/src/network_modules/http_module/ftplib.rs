use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;

// ── ftplib module ──

pub fn create_ftplib_module() -> PyObjectRef {
    make_module(
        "ftplib",
        vec![
            (
                "FTP",
                make_builtin(|args: &[PyObjectRef]| {
                    let host = if !args.is_empty() {
                        args[0].py_to_string()
                    } else {
                        String::new()
                    };
                    let cls = PyObject::class(CompactString::from("FTP"), vec![], IndexMap::new());
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref data) = inst.payload {
                        let mut attrs = data.attrs.write();
                        attrs.insert(
                            CompactString::from("host"),
                            PyObject::str_val(CompactString::from(host)),
                        );
                        attrs.insert(
                            CompactString::from("connect"),
                            make_builtin(|_| {
                                Ok(PyObject::str_val(CompactString::from(
                                    "220 FTP ready (stub)",
                                )))
                            }),
                        );
                        attrs.insert(
                            CompactString::from("login"),
                            make_builtin(|_| {
                                Ok(PyObject::str_val(CompactString::from("230 Login OK")))
                            }),
                        );
                        attrs.insert(
                            CompactString::from("cwd"),
                            make_builtin(|_| Ok(PyObject::str_val(CompactString::from("250 OK")))),
                        );
                        attrs.insert(
                            CompactString::from("pwd"),
                            make_builtin(|_| Ok(PyObject::str_val(CompactString::from("/")))),
                        );
                        attrs.insert(
                            CompactString::from("nlst"),
                            make_builtin(|_| Ok(PyObject::list(vec![]))),
                        );
                        attrs.insert(
                            CompactString::from("dir"),
                            make_builtin(|_| Ok(PyObject::none())),
                        );
                        attrs.insert(
                            CompactString::from("quit"),
                            make_builtin(|_| Ok(PyObject::str_val(CompactString::from("221 Bye")))),
                        );
                        attrs.insert(
                            CompactString::from("close"),
                            make_builtin(|_| Ok(PyObject::none())),
                        );
                    }
                    Ok(inst)
                }),
            ),
            (
                "FTP_TLS",
                make_builtin(|_| Err(PyException::not_implemented_error("ftplib.FTP_TLS"))),
            ),
            (
                "error_reply",
                PyObject::class(CompactString::from("error_reply"), vec![], IndexMap::new()),
            ),
            (
                "error_perm",
                PyObject::class(CompactString::from("error_perm"), vec![], IndexMap::new()),
            ),
        ],
    )
}
