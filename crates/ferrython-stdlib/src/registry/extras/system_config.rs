use crate::sys_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "sysconfig" => Some(sys_modules::create_sysconfig_module()),
        "_sysconfig" => Some(sys_modules::create_sysconfig_module()),
        _ => None,
    }
}
