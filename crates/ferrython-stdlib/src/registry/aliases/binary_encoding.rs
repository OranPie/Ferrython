use crate::serial_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "binascii" => Some(serial_modules::create_binascii_module()),
        _ => None,
    }
}
