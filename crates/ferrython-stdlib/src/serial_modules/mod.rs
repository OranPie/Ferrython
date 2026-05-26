//! Serialization stdlib modules (json, csv, base64, struct, pickle, binascii, codecs, shelve)

mod base64_module;
mod binascii_module;
mod csv_module;
mod json_module;
mod marshal_module;
mod other;
mod struct_module;

pub use base64_module::create_base64_module;
pub(crate) use base64_module::extract_bytes;
pub use binascii_module::create_binascii_module;
pub use csv_module::create_csv_module;
pub use json_module::{
    create_json_decoder_module, create_json_encoder_module, create_json_module,
    json_dumps as json_dumps_fn,
};
pub use marshal_module::create_marshal_module;
pub use other::{
    create_codecs_module, create_dbm_module, create_pickle_module, create_shelve_module,
};
pub use struct_module::create_struct_module;
