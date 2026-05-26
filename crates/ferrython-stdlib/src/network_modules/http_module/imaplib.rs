use ferrython_core::error::PyException;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectRef};

// ── imaplib module ──

pub fn create_imaplib_module() -> PyObjectRef {
    make_module(
        "imaplib",
        vec![
            (
                "IMAP4",
                make_builtin(|_args: &[PyObjectRef]| {
                    Err(PyException::runtime_error(
                        "imaplib.IMAP4: connection required (stub)",
                    ))
                }),
            ),
            (
                "IMAP4_SSL",
                make_builtin(|_args: &[PyObjectRef]| {
                    Err(PyException::runtime_error(
                        "imaplib.IMAP4_SSL: connection required (stub)",
                    ))
                }),
            ),
            ("IMAP4_PORT", PyObject::int(143)),
            ("IMAP4_SSL_PORT", PyObject::int(993)),
        ],
    )
}
