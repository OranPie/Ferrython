use crate::collection_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "collections" => Some(collection_modules::create_collections_module()),
        "itertools" => Some(collection_modules::create_itertools_module()),
        "queue" => Some(collection_modules::create_queue_module()),
        "array" => Some(collection_modules::create_array_module()),
        "operator" => Some(collection_modules::create_operator_module()),
        _ => None,
    }
}
