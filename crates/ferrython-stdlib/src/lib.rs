//! Ferrython standard library — Rust-implemented stdlib modules.
//!
//! This crate provides all built-in Python standard library modules.
//! The VM calls `load_module(name)` to resolve `import` statements.

mod math_modules;
mod sys_modules;
mod text_modules;
mod collection_modules;
mod serial_modules;
mod fs_modules;
mod time_modules;
mod misc_modules;
mod type_modules;
mod introspection_modules;
mod concurrency_modules;
mod async_modules;
mod import_modules;

use ferrython_core::object::PyObjectRef;

pub use sys_modules::get_recursion_limit;
pub use concurrency_modules::drain_deferred_calls;
pub use async_modules::take_asyncio_run_coro;
pub use import_modules::{take_import_module_request, take_reload_request, ImportModuleRequest, ReloadRequest};

/// Look up a built-in stdlib module by name.
/// Returns `Some(module)` if found, `None` otherwise.
pub fn load_module(name: &str) -> Option<PyObjectRef> {
    match name {
        // Math & statistics
        "math" => Some(math_modules::create_math_module()),
        "statistics" => Some(math_modules::create_statistics_module()),
        "numbers" => Some(math_modules::create_numbers_module()),
        "decimal" => Some(math_modules::create_decimal_module()),
        "random" => Some(math_modules::create_random_module()),
        "heapq" => Some(math_modules::create_heapq_module()),
        "bisect" => Some(math_modules::create_bisect_module()),
        // System & OS
        "sys" => Some(sys_modules::create_sys_module()),
        "os" => Some(sys_modules::create_os_module()),
        "os.path" => Some(sys_modules::create_os_path_module()),
        "platform" => Some(sys_modules::create_platform_module()),
        "locale" => Some(sys_modules::create_locale_module()),
        // Text processing
        "string" => Some(text_modules::create_string_module()),
        "re" => Some(text_modules::create_re_module()),
        "textwrap" => Some(text_modules::create_textwrap_module()),
        "fnmatch" => Some(text_modules::create_fnmatch_module()),
        // Collections & functional
        "collections" => Some(collection_modules::create_collections_module()),
        "functools" => Some(collection_modules::create_functools_module()),
        "itertools" => Some(collection_modules::create_itertools_module()),
        "queue" => Some(collection_modules::create_queue_module()),
        // Serialization
        "json" => Some(serial_modules::create_json_module()),
        "csv" => Some(serial_modules::create_csv_module()),
        "base64" => Some(serial_modules::create_base64_module()),
        "struct" => Some(serial_modules::create_struct_module()),
        // Filesystem & process
        "pathlib" => Some(fs_modules::create_pathlib_module()),
        "shutil" => Some(fs_modules::create_shutil_module()),
        "glob" => Some(fs_modules::create_glob_module()),
        "tempfile" => Some(fs_modules::create_tempfile_module()),
        "io" => Some(fs_modules::create_io_module()),
        "subprocess" => Some(fs_modules::create_subprocess_module()),
        // Time & datetime
        "time" => Some(time_modules::create_time_module()),
        "datetime" => Some(time_modules::create_datetime_module()),
        // Type system
        "typing" => Some(type_modules::create_typing_module()),
        "abc" => Some(type_modules::create_abc_module()),
        "enum" => Some(type_modules::create_enum_module()),
        "types" => Some(type_modules::create_types_module()),
        "collections.abc" => Some(type_modules::create_collections_abc_module()),
        // Misc
        "contextlib" => Some(misc_modules::create_contextlib_module()),
        "dataclasses" => Some(misc_modules::create_dataclasses_module()),
        "copy" => Some(misc_modules::create_copy_module()),
        "operator" => Some(misc_modules::create_operator_module()),
        "hashlib" => Some(misc_modules::create_hashlib_module()),
        "logging" => Some(misc_modules::create_logging_module()),
        "unittest" => Some(misc_modules::create_unittest_module()),
        "pprint" => Some(misc_modules::create_pprint_module()),
        "argparse" => Some(misc_modules::create_argparse_module()),
        "errno" => Some(misc_modules::create_errno_module()),
        // Introspection
        "warnings" => Some(introspection_modules::create_warnings_module()),
        "traceback" => Some(introspection_modules::create_traceback_module()),
        "inspect" => Some(introspection_modules::create_inspect_module()),
        "dis" => Some(introspection_modules::create_dis_module()),
        // Concurrency
        "threading" => Some(concurrency_modules::create_threading_module()),
        "weakref" => Some(concurrency_modules::create_weakref_module()),
        "gc" => Some(concurrency_modules::create_gc_module()),
        "_thread" => Some(concurrency_modules::create_thread_module()),
        // Async
        "asyncio" => Some(async_modules::create_asyncio_module()),
        // Import system
        "importlib" => Some(import_modules::create_importlib_module()),
        _ => None,
    }
}