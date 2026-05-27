use crate::text_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "html.parser" => Some(text_modules::create_html_parser_module()),
        "unicodedata" => Some(text_modules::create_unicodedata_module()),
        _ => None,
    }
}
