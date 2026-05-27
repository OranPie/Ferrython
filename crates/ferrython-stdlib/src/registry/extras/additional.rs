use crate::crypto_modules;
use ferrython_core::object::{make_module, PyObjectRef};

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "uuid" => Some(crypto_modules::create_uuid_module()),
        "secrets" => Some(crypto_modules::create_secrets_module()),
        "hashlib" => Some(crypto_modules::create_hashlib_module()),
        "hmac" => Some(crypto_modules::create_hmac_module()),
        "concurrent" => Some(make_module("concurrent", vec![])),
        _ => None,
    }
}
