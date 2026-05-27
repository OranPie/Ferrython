use crate::fs_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "pathlib" => Some(fs_modules::create_pathlib_module()),
        "shutil" => Some(fs_modules::create_shutil_module()),
        "glob" => Some(fs_modules::create_glob_module()),
        "tempfile" => Some(fs_modules::create_tempfile_module()),
        "io" => Some(fs_modules::create_io_module()),
        "subprocess" => Some(fs_modules::create_subprocess_module()),
        "zlib" => Some(fs_modules::create_zlib_module()),
        _ => None,
    }
}
