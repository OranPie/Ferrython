use crate::import_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "importlib" => Some(import_modules::create_importlib_module()),
        "importlib.metadata" => Some(import_modules::create_importlib_metadata_module()),
        _ => None,
    }
}
