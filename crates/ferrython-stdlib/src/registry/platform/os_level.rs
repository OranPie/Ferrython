use crate::sys_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "mmap" => Some(sys_modules::create_mmap_module()),
        "resource" => Some(sys_modules::create_resource_module()),
        "fcntl" => Some(sys_modules::create_fcntl_module()),
        _ => None,
    }
}
