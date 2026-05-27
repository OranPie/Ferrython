use crate::{
    collection_modules, fs_modules, math_modules, serial_modules, text_modules, time_modules,
    type_modules,
};
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "_collections_abc" => Some(type_modules::create_collections_abc_module()),
        "_functools" => Some(collection_modules::create_functools_module()),
        "_operator" => Some(collection_modules::create_operator_module()),
        "_csv" => Some(serial_modules::create_csv_module()),
        "_heapq" => Some(math_modules::create_heapq_accel_module()),
        "_json" => Some(serial_modules::create_json_module()),
        "_io" => Some(fs_modules::create_io_module()),
        "_collections" => Some(collection_modules::create_collections_module()),
        "_multibytecodec" => Some(text_modules::create_multibytecodec_module()),
        "_codecs" => Some(serial_modules::create_codecs_module()),
        "_string" => Some(text_modules::create_string_internal_module()),
        "_strptime" => Some(time_modules::create_strptime_module()),
        _ => None,
    }
}
