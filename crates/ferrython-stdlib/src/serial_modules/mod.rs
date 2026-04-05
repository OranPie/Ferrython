//! Serialization stdlib modules (json, csv, base64, struct, pickle, binascii, codecs, shelve)

mod json_module;
mod csv_module;
mod other;

pub use json_module::create_json_module;
pub use csv_module::create_csv_module;
pub use other::{
    create_base64_module,
    create_struct_module,
    create_pickle_module,
    create_binascii_module,
    create_codecs_module,
    create_shelve_module,
};
pub(crate) use other::extract_bytes;
