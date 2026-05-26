use ferrython_core::object::{make_module, PyObject, PyObjectRef};

// ─── logging.config submodule ───────────────────────────────────────────────

pub fn create_logging_config_module() -> PyObjectRef {
    make_module(
        "logging.config",
        vec![
            (
                "dictConfig",
                PyObject::native_function("dictConfig", |_args| Ok(PyObject::none())),
            ),
            (
                "fileConfig",
                PyObject::native_function("fileConfig", |_args| Ok(PyObject::none())),
            ),
            (
                "listen",
                PyObject::native_function("listen", |_args| Ok(PyObject::none())),
            ),
            (
                "stopListening",
                PyObject::native_function("stopListening", |_args| Ok(PyObject::none())),
            ),
        ],
    )
}
