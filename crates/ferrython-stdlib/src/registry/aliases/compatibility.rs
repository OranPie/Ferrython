use crate::{misc_modules, sys_modules};
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "__future__" => Some(misc_modules::create_future_module()),
        "builtins" => Some(misc_modules::create_builtins_module()),
        "_builtins" => Some(misc_modules::create_builtins_module()),
        "atexit" => Some(sys_modules::create_atexit_module()),
        "site" => Some(sys_modules::create_site_module()),
        "errno" => Some(sys_modules::create_errno_module()),
        _ => None,
    }
}
