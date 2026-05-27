use crate::text_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "encodings" => Some(text_modules::create_encodings_module()),
        "encodings.utf_8" => Some(text_modules::create_encodings_codec_module(
            "encodings.utf_8",
        )),
        "encodings.ascii" => Some(text_modules::create_encodings_codec_module(
            "encodings.ascii",
        )),
        "encodings.latin_1" => Some(text_modules::create_encodings_codec_module(
            "encodings.latin_1",
        )),
        "encodings.aliases" => Some(text_modules::create_encodings_aliases_module()),
        "encodings.idna" => Some(text_modules::create_encodings_idna_module()),
        _ if name.starts_with("encodings.") => {
            Some(text_modules::create_encodings_codec_module(name))
        }
        _ => None,
    }
}
