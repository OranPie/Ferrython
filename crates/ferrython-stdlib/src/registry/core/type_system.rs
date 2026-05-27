use crate::type_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "typing" => Some(type_modules::create_typing_module()),
        "abc" => Some(type_modules::create_abc_module()),
        "enum" => Some(type_modules::create_enum_module()),
        "types" => Some(type_modules::create_types_module()),
        "collections.abc" => Some(type_modules::create_collections_abc_module()),
        _ => None,
    }
}
