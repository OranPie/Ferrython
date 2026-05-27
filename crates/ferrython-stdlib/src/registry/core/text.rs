use crate::text_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "string" => Some(text_modules::create_string_module()),
        "re" => Some(text_modules::create_re_module()),
        "_sre" => Some(text_modules::create_sre_module()),
        "textwrap" => Some(text_modules::create_textwrap_module()),
        "fnmatch" => Some(text_modules::create_fnmatch_module()),
        "html" => Some(text_modules::create_html_module()),
        "shlex" => Some(text_modules::create_shlex_module()),
        "pprint" => Some(text_modules::create_pprint_module()),
        _ => None,
    }
}
