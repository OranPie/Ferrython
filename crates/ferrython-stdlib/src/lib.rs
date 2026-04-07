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
mod import_modules;
mod network_modules;
pub mod xml_modules;
pub mod db_modules;
mod email_modules;
mod compression_modules;

use ferrython_core::object::{PyObjectRef, PyObjectMethods};
use parking_lot::RwLock;

pub use sys_modules::get_recursion_limit;
pub use sys_modules::{set_exc_info, clear_exc_info, get_exc_info};
pub use sys_modules::{get_trace_func, set_trace_func, get_profile_func, set_profile_func, get_excepthook, set_excepthook};
pub use concurrency_modules::drain_deferred_calls;
pub use ferrython_async::take_asyncio_run_coro;
pub use import_modules::{take_import_module_request, take_reload_request, ImportModuleRequest, ReloadRequest};
pub use serial_modules::json_dumps_fn;

// ── Global stdout/stderr override for redirect_stdout/redirect_stderr ──
// When set, print() writes here instead of real stdout.
static STDOUT_OVERRIDE: std::sync::LazyLock<RwLock<Vec<PyObjectRef>>> =
    std::sync::LazyLock::new(|| RwLock::new(Vec::new()));
static STDERR_OVERRIDE: std::sync::LazyLock<RwLock<Vec<PyObjectRef>>> =
    std::sync::LazyLock::new(|| RwLock::new(Vec::new()));

/// Push a new stdout override (for redirect_stdout).
pub fn push_stdout_override(target: PyObjectRef) {
    STDOUT_OVERRIDE.write().push(target);
}
/// Pop the current stdout override (for redirect_stdout.__exit__).
pub fn pop_stdout_override() -> Option<PyObjectRef> {
    STDOUT_OVERRIDE.write().pop()
}
/// Get the current stdout override (None = use real stdout).
pub fn get_stdout_override() -> Option<PyObjectRef> {
    STDOUT_OVERRIDE.read().last().cloned()
}
/// Push a new stderr override.
pub fn push_stderr_override(target: PyObjectRef) {
    STDERR_OVERRIDE.write().push(target);
}
/// Pop the current stderr override.
pub fn pop_stderr_override() -> Option<PyObjectRef> {
    STDERR_OVERRIDE.write().pop()
}
/// Get the current stderr override.
pub fn get_stderr_override() -> Option<PyObjectRef> {
    STDERR_OVERRIDE.read().last().cloned()
}

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
        "gzip" => Some(compression_modules::create_gzip_module()),
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
        // unittest: pure Python module (stdlib/Lib/unittest/__init__.py)
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
        "_ast" => Some(introspection_modules::create_ast_module()),
        "linecache" => Some(introspection_modules::create_linecache_module()),
        "token" => Some(introspection_modules::create_token_module()),
        // Concurrency
        "threading" => Some(concurrency_modules::create_threading_module()),
        "weakref" => Some(concurrency_modules::create_weakref_module()),
        "gc" => Some(concurrency_modules::create_gc_module()),
        "_thread" => Some(concurrency_modules::create_thread_module()),
        "signal" => Some(concurrency_modules::create_signal_module()),
        "multiprocessing" => Some(concurrency_modules::create_multiprocessing_module()),
        "multiprocessing.pool" => Some(concurrency_modules::create_multiprocessing_module()),
        "multiprocessing.managers" => Some(concurrency_modules::create_multiprocessing_module()),
        "multiprocessing.queues" => Some(concurrency_modules::create_multiprocessing_module()),
        "selectors" => Some(concurrency_modules::create_selectors_module()),
        "select" => Some(concurrency_modules::create_select_module()),
        // OS-level
        "mmap" => Some(sys_modules::create_mmap_module()),
        "resource" => Some(sys_modules::create_resource_module()),
        "fcntl" => Some(sys_modules::create_fcntl_module()),
        // Async
        "asyncio" => Some(ferrython_async::create_asyncio_module()),
        "asyncio.events" => Some(ferrython_async::create_asyncio_module()),
        "asyncio.tasks" => Some(ferrython_async::create_asyncio_module()),
        "asyncio.futures" => Some(ferrython_async::create_asyncio_module()),
        "asyncio.queues" => Some(ferrython_async::create_asyncio_module()),
        "asyncio.locks" => Some(ferrython_async::create_asyncio_module()),
        "asyncio.runners" => Some(ferrython_async::create_asyncio_module()),
        // Import system
        "importlib" => Some(import_modules::create_importlib_module()),
        "importlib.metadata" => Some(import_modules::create_importlib_metadata_module()),
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
        // Zip / Compression
        "zipfile" => Some(compression_modules::create_zipfile_module()),
        "bz2" => Some(compression_modules::create_bz2_module()),
        "lzma" => Some(compression_modules::create_lzma_module()),
        "tarfile" => Some(compression_modules::create_tarfile_module()),
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
        // concurrent — handled by stdlib/Lib/concurrent/futures.py (pure Python)
        "concurrent" => {
            Some(ferrython_core::object::make_module("concurrent", vec![]))
        }
        // HTML parser & unicode
        "html.parser" => Some(text_modules::create_html_parser_module()),
        "unicodedata" => Some(text_modules::create_unicodedata_module()),
        // HTTP server, cookies & SSL
        "http.server" => Some(network_modules::create_http_server_module()),
        "http.cookiejar" => Some(network_modules::create_http_cookiejar_module()),
        "http.cookies" => Some(network_modules::create_http_cookies_module()),
        "ssl" => Some(network_modules::create_ssl_module()),
        // XML DOM & SAX
        "xml.dom" => Some(xml_modules::create_xml_dom_module()),
        "xml.dom.minidom" => Some(xml_modules::create_xml_dom_minidom_module()),
        "xml.sax" => Some(xml_modules::create_xml_sax_module()),
        // Testing & debugging
        "unittest.mock" => Some(testing_modules::create_unittest_mock_module()),
        // doctest: pure Python module (stdlib/Lib/doctest.py)
        "pdb" => Some(testing_modules::create_pdb_module()),
        "profile" => Some(testing_modules::create_profile_module()),
        "cProfile" => Some(testing_modules::create_cprofile_module()),
        "timeit" => Some(testing_modules::create_timeit_module()),
        "faulthandler" => Some(testing_modules::create_faulthandler_module()),
        "tracemalloc" => Some(testing_modules::create_tracemalloc_module()),
        "pydoc" => Some(testing_modules::create_pydoc_module()),
        // Introspection (extended)
        "tokenize" => Some(introspection_modules::create_tokenize_module()),
        "symtable" => Some(introspection_modules::create_symtable_module()),
        // Persistence
        "shelve" => Some(serial_modules::create_shelve_module()),
        // Context variables
        "contextvars" => Some(misc_modules::create_contextvars_module()),
        // MIME types
        "mimetypes" => Some(misc_modules::create_mimetypes_module()),
        // Readline
        "readline" => Some(misc_modules::create_readline_module()),
        // runpy
        "runpy" => Some(misc_modules::create_runpy_module()),
        // Network protocol stubs
        "smtplib" => Some(network_modules::create_smtplib_module()),
        "ftplib" => Some(network_modules::create_ftplib_module()),
        "imaplib" => Some(network_modules::create_imaplib_module()),
        "poplib" => Some(network_modules::create_poplib_module()),
        "cgi" => Some(network_modules::create_cgi_module()),
        // dbm stub
        "dbm" | "dbm.dumb" | "dbm.gnu" | "dbm.ndbm" => Some(serial_modules::create_dbm_module()),
        // xmlrpc stubs
        "xmlrpc" | "xmlrpc.client" | "xmlrpc.server" => Some(network_modules::create_xmlrpc_module()),
        // Additional modules
        "cmd" => Some(misc_modules::create_cmd_module()),
        "compileall" => Some(misc_modules::create_compileall_module()),
        "pstats" => Some(misc_modules::create_pstats_module()),
        "quopri" => Some(misc_modules::create_quopri_module()),
        "stringprep" => Some(misc_modules::create_stringprep_module()),
        "plistlib" => Some(misc_modules::create_plistlib_module()),
        // System configuration
        "sysconfig" => Some(sys_modules::create_sysconfig_module()),
        "_sysconfig" => Some(sys_modules::create_sysconfig_module()),
        // Encodings
        "encodings" => Some(text_modules::create_encodings_module()),
        "encodings.utf_8" => Some(text_modules::create_encodings_module()),
        "encodings.ascii" => Some(text_modules::create_encodings_module()),
        "encodings.latin_1" => Some(text_modules::create_encodings_module()),
        // Unix user/group info
        "grp" => Some(sys_modules::create_grp_module()),
        "pwd" => Some(sys_modules::create_pwd_module()),
        // Terminal / curses
        "curses" => Some(misc_modules::create_curses_module()),
        "_curses" => Some(misc_modules::create_curses_module()),
        // FFI / ctypes
        "ctypes" => Some(misc_modules::create_ctypes_module()),
        "_ctypes" => Some(misc_modules::create_ctypes_module()),
        "ctypes.util" => {
            let m = misc_modules::create_ctypes_module();
            m.get_attr("util")
        },
        // Import resources
        "importlib.resources" => Some(import_modules::create_importlib_resources_module()),
        // Misc sub-module aliases
        "html.entities" => None, // uses pure-python fallback in stdlib/Lib/html/entities.py
        "email.parser" => None,  // uses pure-python fallback
        "email.header" => None,  // uses pure-python fallback
        _ => None,
    }
}