use crate::{misc_modules, sys_modules};
use ferrython_core::object::{PyObjectMethods, PyObjectRef};

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "grp" => Some(sys_modules::create_grp_module()),
        "pwd" => Some(sys_modules::create_pwd_module()),
        "curses" => Some(misc_modules::create_curses_module()),
        "_curses" => Some(misc_modules::create_curses_module()),
        "ctypes" => Some(misc_modules::create_ctypes_module()),
        "_ctypes" => Some(misc_modules::create_ctypes_module()),
        "ctypes.util" => {
            let m = misc_modules::create_ctypes_module();
            m.get_attr("util")
        }
        _ => None,
    }
}
