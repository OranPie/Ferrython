//! Ferrython standard library — Rust-implemented stdlib modules.
//!
//! This crate provides all built-in Python standard library modules.
//! The VM calls `load_module(name)` to resolve `import` statements.

mod modules;

use ferrython_core::object::PyObjectRef;

/// Look up a built-in stdlib module by name.
/// Returns `Some(module)` if found, `None` otherwise.
pub fn load_module(name: &str) -> Option<PyObjectRef> {
    match name {
        "math" => Some(modules::create_math_module()),
        "sys" => Some(modules::create_sys_module()),
        "os" => Some(modules::create_os_module()),
        "os.path" => Some(modules::create_os_path_module()),
        "string" => Some(modules::create_string_module()),
        "json" => Some(modules::create_json_module()),
        "time" => Some(modules::create_time_module()),
        "random" => Some(modules::create_random_module()),
        "collections" => Some(modules::create_collections_module()),
        "functools" => Some(modules::create_functools_module()),
        "itertools" => Some(modules::create_itertools_module()),
        "io" => Some(modules::create_io_module()),
        "re" => Some(modules::create_re_module()),
        "hashlib" => Some(modules::create_hashlib_module()),
        "copy" => Some(modules::create_copy_module()),
        "operator" => Some(modules::create_operator_module()),
        "typing" => Some(modules::create_typing_module()),
        "abc" => Some(modules::create_abc_module()),
        "enum" => Some(modules::create_enum_module()),
        "contextlib" => Some(modules::create_contextlib_module()),
        "dataclasses" => Some(modules::create_dataclasses_module()),
        "struct" => Some(modules::create_struct_module()),
        "textwrap" => Some(modules::create_textwrap_module()),
        "traceback" => Some(modules::create_traceback_module()),
        "warnings" => Some(modules::create_warnings_module()),
        "decimal" => Some(modules::create_decimal_module()),
        "statistics" => Some(modules::create_statistics_module()),
        "numbers" => Some(modules::create_numbers_module()),
        "platform" => Some(modules::create_platform_module()),
        "locale" => Some(modules::create_locale_module()),
        "inspect" => Some(modules::create_inspect_module()),
        "dis" => Some(modules::create_dis_module()),
        "logging" => Some(modules::create_logging_module()),
        "subprocess" => Some(modules::create_subprocess_module()),
        "pathlib" => Some(modules::create_pathlib_module()),
        "unittest" => Some(modules::create_unittest_module()),
        "threading" => Some(modules::create_threading_module()),
        "csv" => Some(modules::create_csv_module()),
        "shutil" => Some(modules::create_shutil_module()),
        "glob" => Some(modules::create_glob_module()),
        "tempfile" => Some(modules::create_tempfile_module()),
        "fnmatch" => Some(modules::create_fnmatch_module()),
        "base64" => Some(modules::create_base64_module()),
        "pprint" => Some(modules::create_pprint_module()),
        "argparse" => Some(modules::create_argparse_module()),
        "datetime" => Some(modules::create_datetime_module()),
        "weakref" => Some(modules::create_weakref_module()),
        "gc" => Some(modules::create_gc_module()),
        _ => None,
    }
}