use crate::introspection_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "tokenize" => Some(introspection_modules::create_tokenize_module()),
        "symtable" => Some(introspection_modules::create_symtable_module()),
        _ => None,
    }
}
