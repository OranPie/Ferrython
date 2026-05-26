use ferrython_core::object::{make_module, PyObject, PyObjectRef};

// ── __future__ module ──

pub fn create_future_module() -> PyObjectRef {
    make_module(
        "__future__",
        vec![
            ("division", PyObject::bool_val(true)),
            ("absolute_import", PyObject::bool_val(true)),
            ("print_function", PyObject::bool_val(true)),
            ("unicode_literals", PyObject::bool_val(true)),
            ("generator_stop", PyObject::bool_val(true)),
            ("annotations", PyObject::bool_val(true)),
            ("CO_FUTURE_DIVISION", PyObject::int(0x20000)),
            ("CO_FUTURE_ABSOLUTE_IMPORT", PyObject::int(0x40000)),
            ("CO_FUTURE_PRINT_FUNCTION", PyObject::int(0x10000)),
            ("CO_FUTURE_UNICODE_LITERALS", PyObject::int(0x20000)),
            ("CO_FUTURE_GENERATOR_STOP", PyObject::int(0x80000)),
            ("CO_FUTURE_ANNOTATIONS", PyObject::int(0x100000)),
        ],
    )
}
