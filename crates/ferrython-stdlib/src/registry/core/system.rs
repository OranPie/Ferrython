use crate::sys_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "sys" => Some(sys_modules::create_sys_module()),
        "os" => Some(sys_modules::create_os_module()),
        "os.path" => Some(sys_modules::create_os_path_module()),
        "stat" => Some(sys_modules::create_stat_module()),
        "platform" => Some(sys_modules::create_platform_module()),
        "locale" => Some(sys_modules::create_locale_module()),
        "getpass" => Some(sys_modules::create_getpass_module()),
        _ => None,
    }
}
