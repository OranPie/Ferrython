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
mod crypto_modules;
mod config_modules;
mod testing_modules;
mod type_modules;
mod introspection_modules;
mod concurrency_modules;
mod async_modules;
mod import_modules;
mod network_modules;
pub mod xml_modules;
pub mod db_modules;
mod email_modules;

use ferrython_core::object::PyObjectRef;

pub use sys_modules::get_recursion_limit;
pub use sys_modules::{set_exc_info, clear_exc_info, get_exc_info};
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
        "fractions" => Some(math_modules::create_fractions_module()),
        "cmath" => Some(math_modules::create_cmath_module()),
        // System & OS
        "sys" => Some(sys_modules::create_sys_module()),
        "os" => Some(sys_modules::create_os_module()),
        "os.path" => Some(sys_modules::create_os_path_module()),
        "platform" => Some(sys_modules::create_platform_module()),
        "locale" => Some(sys_modules::create_locale_module()),
        "getpass" => Some(sys_modules::create_getpass_module()),
        // Text processing
        "string" => Some(text_modules::create_string_module()),
        "re" => Some(text_modules::create_re_module()),
        "textwrap" => Some(text_modules::create_textwrap_module()),
        "fnmatch" => Some(text_modules::create_fnmatch_module()),
        "html" => Some(text_modules::create_html_module()),
        "shlex" => Some(text_modules::create_shlex_module()),
        "difflib" => Some(text_modules::create_difflib_module()),
        // Collections & functional
        "collections" => Some(collection_modules::create_collections_module()),
        "functools" => Some(collection_modules::create_functools_module()),
        "itertools" => Some(collection_modules::create_itertools_module()),
        "queue" => Some(collection_modules::create_queue_module()),
        "array" => Some(collection_modules::create_array_module()),
        // Serialization
        "json" => Some(serial_modules::create_json_module()),
        "csv" => Some(serial_modules::create_csv_module()),
        "base64" => Some(serial_modules::create_base64_module()),
        "struct" => Some(serial_modules::create_struct_module()),
        "pickle" => Some(serial_modules::create_pickle_module()),
        // Filesystem & process
        "pathlib" => Some(fs_modules::create_pathlib_module()),
        "shutil" => Some(fs_modules::create_shutil_module()),
        "glob" => Some(fs_modules::create_glob_module()),
        "tempfile" => Some(fs_modules::create_tempfile_module()),
        "io" => Some(fs_modules::create_io_module()),
        "subprocess" => Some(fs_modules::create_subprocess_module()),
        "gzip" => Some(fs_modules::create_gzip_module()),
        "zlib" => Some(fs_modules::create_zlib_module()),
        // Time & datetime
        "time" => Some(time_modules::create_time_module()),
        "datetime" => Some(time_modules::create_datetime_module()),
        "calendar" => Some(time_modules::create_calendar_module()),
        // Type system
        "typing" => Some(type_modules::create_typing_module()),
        "typing_extensions" => Some(type_modules::create_typing_module()),
        "abc" => Some(type_modules::create_abc_module()),
        "enum" => Some(type_modules::create_enum_module()),
        "types" => Some(type_modules::create_types_module()),
        "collections.abc" => Some(type_modules::create_collections_abc_module()),
        // Misc
        "contextlib" => Some(misc_modules::create_contextlib_module()),
        "dataclasses" => Some(misc_modules::create_dataclasses_module()),
        "copy" => Some(misc_modules::create_copy_module()),
        "operator" => Some(collection_modules::create_operator_module()),
        "hashlib" => Some(crypto_modules::create_hashlib_module()),
        "logging" => Some(testing_modules::create_logging_module()),
        "unittest" => Some(testing_modules::create_unittest_module()),
        "pprint" => Some(text_modules::create_pprint_module()),
        "argparse" => Some(config_modules::create_argparse_module()),
        "errno" => Some(sys_modules::create_errno_module()),
        "uuid" => Some(crypto_modules::create_uuid_module()),
        "codecs" => Some(serial_modules::create_codecs_module()),
        "secrets" => Some(crypto_modules::create_secrets_module()),
        "hmac" => Some(crypto_modules::create_hmac_module()),
        "configparser" => Some(config_modules::create_configparser_module()),
        // Introspection
        "warnings" => Some(introspection_modules::create_warnings_module()),
        "traceback" => Some(introspection_modules::create_traceback_module()),
        "inspect" => Some(introspection_modules::create_inspect_module()),
        "dis" => Some(introspection_modules::create_dis_module()),
        "ast" => Some(introspection_modules::create_ast_module()),
        "linecache" => Some(introspection_modules::create_linecache_module()),
        "token" => Some(introspection_modules::create_token_module()),
        // Concurrency
        "threading" => Some(concurrency_modules::create_threading_module()),
        "weakref" => Some(concurrency_modules::create_weakref_module()),
        "gc" => Some(concurrency_modules::create_gc_module()),
        "_thread" => Some(concurrency_modules::create_thread_module()),
        "signal" => Some(concurrency_modules::create_signal_module()),
        "multiprocessing" => Some(concurrency_modules::create_multiprocessing_module()),
        "selectors" => Some(concurrency_modules::create_selectors_module()),
        // Async
        "asyncio" => Some(async_modules::create_asyncio_module()),
        // Import system
        "importlib" => Some(import_modules::create_importlib_module()),
        // Networking
        "socket" => Some(network_modules::create_socket_module()),
        "urllib" => Some(network_modules::create_urllib_module()),
        "urllib.request" => Some(network_modules::create_urllib_module()),
        "urllib.parse" => Some(network_modules::create_urllib_parse_module()),
        "http" => Some(network_modules::create_http_module()),
        "http.client" => Some(network_modules::create_http_module()),
        // XML
        "xml" => Some(xml_modules::create_xml_module()),
        "xml.etree" => Some(xml_modules::create_xml_etree_module()),
        "xml.etree.ElementTree" => Some(xml_modules::create_xml_etree_elementtree_module()),
        // Database
        "sqlite3" => Some(db_modules::create_sqlite3_module()),
        // Email
        "email" => Some(email_modules::create_email_module()),
        "email.message" => Some(email_modules::create_email_message_module()),
        "email.mime" => Some(email_modules::create_email_mime_module()),
        "email.mime.text" => Some(email_modules::create_email_mime_text_module()),
        "email.mime.multipart" => Some(email_modules::create_email_mime_multipart_module()),
        "email.mime.base" => Some(email_modules::create_email_mime_base_module()),
        "email.utils" => Some(email_modules::create_email_utils_module()),
        // Zip
        "zipfile" => Some(fs_modules::create_zipfile_module()),
        // Internal C-extension aliases
        "_collections_abc" => Some(type_modules::create_collections_abc_module()),
        "_functools" => Some(collection_modules::create_functools_module()),
        "_operator" => Some(collection_modules::create_operator_module()),
        "_csv" => Some(serial_modules::create_csv_module()),
        "_heapq" => Some(math_modules::create_heapq_module()),
        "_json" => Some(serial_modules::create_json_module()),
        "_io" => Some(fs_modules::create_io_module()),
        "_collections" => Some(collection_modules::create_collections_module()),
        // Compatibility
        "__future__" => Some(misc_modules::create_future_module()),
        "builtins" => Some(misc_modules::create_builtins_module()),
        "_builtins" => Some(misc_modules::create_builtins_module()),
        "atexit" => Some(sys_modules::create_atexit_module()),
        "site" => Some(sys_modules::create_site_module()),
        "sched" => Some(sys_modules::create_sched_module()),
        // Binary & encoding
        "binascii" => Some(serial_modules::create_binascii_module()),
        // concurrent.futures
        "concurrent" => {
            let futures = concurrency_modules::create_concurrent_futures_module();
            Some(ferrython_core::object::make_module("concurrent", vec![("futures", futures)]))
        }
        "concurrent.futures" => Some(concurrency_modules::create_concurrent_futures_module()),
        // HTML parser & unicode
        "html.parser" => Some(text_modules::create_html_parser_module()),
        "unicodedata" => Some(text_modules::create_unicodedata_module()),
        // HTTP server, cookies & SSL
        "http.server" => Some(network_modules::create_http_server_module()),
        "http.cookiejar" => Some(network_modules::create_http_cookiejar_module()),
        "ssl" => Some(network_modules::create_ssl_module()),
        // XML DOM & SAX
        "xml.dom" => Some(xml_modules::create_xml_dom_module()),
        "xml.dom.minidom" => Some(xml_modules::create_xml_dom_minidom_module()),
        "xml.sax" => Some(xml_modules::create_xml_sax_module()),
        // Testing & debugging
        "unittest.mock" => Some(testing_modules::create_unittest_mock_module()),
        "doctest" => Some(testing_modules::create_doctest_module()),
        "pdb" => Some(testing_modules::create_pdb_module()),
        "profile" => Some(testing_modules::create_profile_module()),
        "cProfile" => Some(testing_modules::create_cprofile_module()),
        // Persistence
        "shelve" => Some(serial_modules::create_shelve_module()),
        _ => None,
    }
}