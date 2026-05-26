//! Compression stdlib modules.

use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObjectPayload, PyObjectRef};

mod bz2;
mod gzip;
mod lzma;
mod tarfile;
mod zipfile;

pub use bz2::create_bz2_module;
pub use gzip::create_gzip_module;
pub use lzma::create_lzma_module;
pub use tarfile::create_tarfile_module;
pub use zipfile::create_zipfile_module;

// ── helpers ──

pub(super) fn extract_bytes(obj: &PyObjectRef) -> PyResult<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Ok((**b).clone()),
        PyObjectPayload::Str(s) => Ok(s.as_bytes().to_vec()),
        _ => Err(PyException::type_error("expected bytes-like object")),
    }
}
