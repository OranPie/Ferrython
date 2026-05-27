use crate::compression_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "gzip" => Some(compression_modules::create_gzip_module()),
        "zipfile" => Some(compression_modules::create_zipfile_module()),
        "bz2" => Some(compression_modules::create_bz2_module()),
        "lzma" => Some(compression_modules::create_lzma_module()),
        "tarfile" => Some(compression_modules::create_tarfile_module()),
        _ => None,
    }
}
