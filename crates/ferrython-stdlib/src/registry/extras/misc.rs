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
        "imghdr" => Some(misc_modules::create_imghdr_module()),
        "sndhdr" => Some(misc_modules::create_sndhdr_module()),
        "nturl2path" => Some(misc_modules::create_nturl2path_module()),
        "chunk" => Some(misc_modules::create_chunk_module()),
        "tomllib" => Some(misc_modules::create_tomllib_module()),
        "graphlib" => Some(misc_modules::create_graphlib_module()),
        "netrc" => Some(misc_modules::create_netrc_module()),
        "webbrowser" => Some(misc_modules::create_webbrowser_module()),
        "pstats" => Some(misc_modules::create_pstats_module()),
        "quopri" => Some(misc_modules::create_quopri_module()),
        "stringprep" => Some(misc_modules::create_stringprep_module()),
        "plistlib" => Some(misc_modules::create_plistlib_module()),
        _ => None,
    }
}
