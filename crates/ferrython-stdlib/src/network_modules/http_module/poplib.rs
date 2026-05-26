use ferrython_core::error::PyException;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectRef};

// ── poplib module ──

pub fn create_poplib_module() -> PyObjectRef {
    make_module(
        "poplib",
        vec![
            (
                "POP3",
                make_builtin(|_args: &[PyObjectRef]| {
                    Err(PyException::runtime_error(
                        "poplib.POP3: connection required (stub)",
                    ))
                }),
            ),
            (
                "POP3_SSL",
                make_builtin(|_args: &[PyObjectRef]| {
                    Err(PyException::runtime_error(
                        "poplib.POP3_SSL: connection required (stub)",
                    ))
                }),
            ),
            ("POP3_PORT", PyObject::int(110)),
            ("POP3_SSL_PORT", PyObject::int(995)),
        ],
    )
}
