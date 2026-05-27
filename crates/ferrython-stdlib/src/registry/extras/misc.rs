use crate::misc_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "contextvars" => Some(misc_modules::create_contextvars_module()),
        "mimetypes" => Some(misc_modules::create_mimetypes_module()),
        "readline" => Some(misc_modules::create_readline_module()),
        "runpy" => Some(misc_modules::create_runpy_module()),
        "cmd" => Some(misc_modules::create_cmd_module()),
        "compileall" => Some(misc_modules::create_compileall_module()),
        "pstats" => Some(misc_modules::create_pstats_module()),
        "quopri" => Some(misc_modules::create_quopri_module()),
        "stringprep" => Some(misc_modules::create_stringprep_module()),
        "plistlib" => Some(misc_modules::create_plistlib_module()),
        _ => None,
    }
}
