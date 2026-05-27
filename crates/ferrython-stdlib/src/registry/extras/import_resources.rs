use crate::import_modules;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectRef};

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "importlib.resources" => Some(import_modules::create_importlib_resources_module()),
        "tabnanny" => Some(make_tabnanny_module()),
        "pyclbr" => Some(make_pyclbr_module()),
        _ => None,
    }
}

fn make_tabnanny_module() -> PyObjectRef {
    make_module(
        "tabnanny",
        vec![
            ("check", make_builtin(|_| Ok(PyObject::none()))),
            ("verbose", PyObject::int(0)),
        ],
    )
}

fn make_pyclbr_module() -> PyObjectRef {
    make_module(
        "pyclbr",
        vec![
            (
                "readmodule",
                make_builtin(|_| Ok(PyObject::dict_from_pairs(vec![]))),
            ),
            (
                "readmodule_ex",
                make_builtin(|_| Ok(PyObject::dict_from_pairs(vec![]))),
            ),
        ],
    )
}
