use crate::serial_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "shelve" => Some(serial_modules::create_shelve_module()),
        "marshal" => Some(serial_modules::create_marshal_module()),
        "uu" => Some(crate::misc_modules::create_uu_module()),
        "xdrlib" => Some(crate::misc_modules::create_xdrlib_module()),
        _ => None,
    }
}
