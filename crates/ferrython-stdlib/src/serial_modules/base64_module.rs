use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use ferrython_core::types::HashableKey;

mod base32;
mod base85;
mod helpers;
mod standard;

use base32::{base16_decode, base16_encode, base32_decode, base32_encode};
use base85::{ascii85_decode, ascii85_encode, base85_decode, base85_encode};
pub(crate) use helpers::extract_bytes;
pub(super) use helpers::extract_bytes_like;
pub(super) use standard::{b64_decode_bytes, b64_encode_bytes};
use standard::{
    base64_decode, base64_decodebytes, base64_decodestring, base64_encode, base64_encodebytes,
    base64_encodestring, base64_file_decode, base64_file_encode, base64_standard_decode,
    base64_urlsafe_decode, base64_urlsafe_encode,
};

pub fn create_base64_module() -> PyObjectRef {
    make_module(
        "base64",
        vec![
            ("b64encode", make_builtin(base64_encode)),
            ("b64decode", make_builtin(base64_decode)),
            ("encodebytes", make_builtin(base64_encodebytes)),
            ("decodebytes", make_builtin(base64_decodebytes)),
            ("encodestring", make_builtin(base64_encodestring)),
            ("decodestring", make_builtin(base64_decodestring)),
            ("encode", make_builtin(base64_file_encode)),
            ("decode", make_builtin(base64_file_decode)),
            ("b16encode", make_builtin(|args| base16_encode(args))),
            ("b16decode", make_builtin(|args| base16_decode(args))),
            ("b32encode", make_builtin(|args| base32_encode(args))),
            ("b32decode", make_builtin(|args| base32_decode(args))),
            ("urlsafe_b64encode", make_builtin(base64_urlsafe_encode)),
            ("urlsafe_b64decode", make_builtin(base64_urlsafe_decode)),
            ("standard_b64encode", make_builtin(base64_encode)),
            ("standard_b64decode", make_builtin(base64_standard_decode)),
            ("a85encode", make_builtin(ascii85_encode)),
            ("a85decode", make_builtin(ascii85_decode)),
            ("b85encode", make_builtin(base85_encode)),
            ("b85decode", make_builtin(base85_decode)),
        ],
    )
}
