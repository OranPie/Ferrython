use crate::serial_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "json" => Some(serial_modules::create_json_module()),
        "json.decoder" => Some(serial_modules::create_json_decoder_module()),
        "json.encoder" => Some(serial_modules::create_json_encoder_module()),
        "csv" => Some(serial_modules::create_csv_module()),
        "base64" => Some(serial_modules::create_base64_module()),
        "struct" => Some(serial_modules::create_struct_module()),
        "pickle" => Some(serial_modules::create_pickle_module()),
        "codecs" => Some(serial_modules::create_codecs_module()),
        _ => None,
    }
}
