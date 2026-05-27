use crate::{
    collection_modules, compression_modules, concurrency_modules, config_modules, crypto_modules,
    db_modules, email_modules, fs_modules, import_modules, introspection_modules, math_modules,
    misc_modules, network_modules, serial_modules, sys_modules, testing_modules, text_modules,
    time_modules, type_modules, xml_modules,
};
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef};

pub(crate) fn load_module(name: &str) -> Option<PyObjectRef> {
    math(name)
        .or_else(|| system(name))
        .or_else(|| text_core(name))
        .or_else(|| collections(name))
        .or_else(|| serialization(name))
        .or_else(|| filesystem(name))
        .or_else(|| time(name))
        .or_else(|| type_system(name))
        .or_else(|| misc_core(name))
        .or_else(|| introspection(name))
        .or_else(|| concurrency(name))
        .or_else(|| os_level(name))
        .or_else(|| async_modules(name))
        .or_else(|| import_system(name))
        .or_else(|| network_core(name))
        .or_else(|| xml(name))
        .or_else(|| database(name))
        .or_else(|| email(name))
        .or_else(|| compression(name))
        .or_else(|| internal_aliases(name))
        .or_else(|| compatibility(name))
        .or_else(|| binary_encoding(name))
        .or_else(|| html_unicode(name))
        .or_else(|| network_extended(name))
        .or_else(|| xml_dom(name))
        .or_else(|| testing_debug(name))
        .or_else(|| introspection_extended(name))
        .or_else(|| serialization_extended(name))
        .or_else(|| misc_extended(name))
        .or_else(|| network_stubs(name))
        .or_else(|| dbm_xmlrpc(name))
        .or_else(|| additional_misc(name))
        .or_else(|| system_config(name))
        .or_else(|| encodings(name))
        .or_else(|| unix_terminal_ctypes(name))
        .or_else(|| import_resources_and_fallbacks(name))
}

fn math(name: &str) -> Option<PyObjectRef> {
    match name {
        "math" => Some(math_modules::create_math_module()),
        "statistics" => Some(math_modules::create_statistics_module()),
        "numbers" => Some(math_modules::create_numbers_module()),
        "decimal" => Some(math_modules::create_decimal_module()),
        "random" => Some(math_modules::create_random_module()),
        "heapq" => Some(math_modules::create_heapq_module()),
        "bisect" => Some(math_modules::create_bisect_module()),
        "fractions" => Some(math_modules::create_fractions_module()),
        "cmath" => Some(math_modules::create_cmath_module()),
        _ => None,
    }
}

fn system(name: &str) -> Option<PyObjectRef> {
    match name {
        "sys" => Some(sys_modules::create_sys_module()),
        "os" => Some(sys_modules::create_os_module()),
        "os.path" => Some(sys_modules::create_os_path_module()),
        "platform" => Some(sys_modules::create_platform_module()),
        "locale" => Some(sys_modules::create_locale_module()),
        "getpass" => Some(sys_modules::create_getpass_module()),
        _ => None,
    }
}

fn text_core(name: &str) -> Option<PyObjectRef> {
    match name {
        "string" => Some(text_modules::create_string_module()),
        "re" => Some(text_modules::create_re_module()),
        "_sre" => Some(text_modules::create_sre_module()),
        "textwrap" => Some(text_modules::create_textwrap_module()),
        "fnmatch" => Some(text_modules::create_fnmatch_module()),
        "html" => Some(text_modules::create_html_module()),
        "shlex" => Some(text_modules::create_shlex_module()),
        "pprint" => Some(text_modules::create_pprint_module()),
        _ => None,
    }
}

fn collections(name: &str) -> Option<PyObjectRef> {
    match name {
        "collections" => Some(collection_modules::create_collections_module()),
        "functools" => Some(collection_modules::create_functools_module()),
        "itertools" => Some(collection_modules::create_itertools_module()),
        "queue" => Some(collection_modules::create_queue_module()),
        "array" => Some(collection_modules::create_array_module()),
        "operator" => Some(collection_modules::create_operator_module()),
        _ => None,
    }
}

fn serialization(name: &str) -> Option<PyObjectRef> {
    match name {
        "json" => Some(serial_modules::create_json_module()),
        "json.decoder" => Some(serial_modules::create_json_decoder_module()),
        "json.encoder" => Some(serial_modules::create_json_encoder_module()),
        "csv" => Some(serial_modules::create_csv_module()),
        "base64" => Some(serial_modules::create_base64_module()),
        "struct" => Some(serial_modules::create_struct_module()),
        "pickle" => Some(serial_modules::create_pickle_module()),
        "codecs" => Some(serial_modules::create_codecs_module()),
        _ => None,
    }
}

fn filesystem(name: &str) -> Option<PyObjectRef> {
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

fn time(name: &str) -> Option<PyObjectRef> {
    match name {
        "time" => Some(time_modules::create_time_module()),
        "datetime" => Some(time_modules::create_datetime_module()),
        "zoneinfo" => Some(time_modules::create_zoneinfo_module()),
        _ => None,
    }
}

fn type_system(name: &str) -> Option<PyObjectRef> {
    match name {
        "typing" => Some(type_modules::create_typing_module()),
        "abc" => Some(type_modules::create_abc_module()),
        "enum" => Some(type_modules::create_enum_module()),
        "types" => Some(type_modules::create_types_module()),
        "collections.abc" => Some(type_modules::create_collections_abc_module()),
        _ => None,
    }
}

fn misc_core(name: &str) -> Option<PyObjectRef> {
    match name {
        "dataclasses" => Some(misc_modules::create_dataclasses_module()),
        "configparser" => Some(config_modules::create_configparser_module()),
        _ => None,
    }
}

fn introspection(name: &str) -> Option<PyObjectRef> {
    match name {
        "warnings" => Some(introspection_modules::create_warnings_module()),
        "traceback" => Some(introspection_modules::create_traceback_module()),
        "inspect" => Some(introspection_modules::create_inspect_module()),
        "dis" => Some(introspection_modules::create_dis_module()),
        "_ast" => Some(introspection_modules::create_ast_module()),
        "linecache" => Some(introspection_modules::create_linecache_module()),
        "token" => Some(introspection_modules::create_token_module()),
        _ => None,
    }
}

fn concurrency(name: &str) -> Option<PyObjectRef> {
    match name {
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
        _ => None,
    }
}

fn os_level(name: &str) -> Option<PyObjectRef> {
    match name {
        "mmap" => Some(sys_modules::create_mmap_module()),
        "resource" => Some(sys_modules::create_resource_module()),
        "fcntl" => Some(sys_modules::create_fcntl_module()),
        _ => None,
    }
}

fn async_modules(name: &str) -> Option<PyObjectRef> {
    match name {
        "asyncio"
        | "asyncio.events"
        | "asyncio.tasks"
        | "asyncio.futures"
        | "asyncio.queues"
        | "asyncio.locks"
        | "asyncio.runners"
        | "asyncio.streams"
        | "asyncio.subprocess"
        | "asyncio.protocols"
        | "asyncio.transports"
        | "asyncio.exceptions"
        | "asyncio.base_events" => Some(ferrython_async::create_asyncio_module()),
        _ => None,
    }
}

fn import_system(name: &str) -> Option<PyObjectRef> {
    match name {
        "importlib" => Some(import_modules::create_importlib_module()),
        "importlib.metadata" => Some(import_modules::create_importlib_metadata_module()),
        _ => None,
    }
}

fn network_core(name: &str) -> Option<PyObjectRef> {
    match name {
        "socket" => Some(network_modules::create_socket_module()),
        "socketserver" => Some(network_modules::create_socketserver_module()),
        "urllib" => Some(network_modules::create_urllib_module()),
        "urllib.request" => Some(network_modules::create_urllib_module()),
        "urllib.parse" => Some(network_modules::create_urllib_parse_module()),
        "http" => Some(network_modules::create_http_module()),
        "http.client" => Some(network_modules::create_http_client_module()),
        _ => None,
    }
}

fn xml(name: &str) -> Option<PyObjectRef> {
    match name {
        "xml" => Some(xml_modules::create_xml_module()),
        "xml.etree" => Some(xml_modules::create_xml_etree_module()),
        "xml.etree.ElementTree" => Some(xml_modules::create_xml_etree_elementtree_module()),
        "xml.parsers" => Some(xml_modules::create_xml_parsers_module()),
        "xml.parsers.expat" => Some(xml_modules::create_xml_parsers_expat_module()),
        "xml.sax" => Some(xml_modules::create_xml_sax_module()),
        "xml.sax.handler" => Some(xml_modules::create_xml_sax_handler_module()),
        "xml.sax.saxutils" => Some(xml_modules::create_xml_sax_saxutils_module()),
        "xml.sax.xmlreader" => Some(xml_modules::create_xml_sax_xmlreader_module()),
        _ => None,
    }
}

fn database(name: &str) -> Option<PyObjectRef> {
    match name {
        "sqlite3" => Some(db_modules::create_sqlite3_module()),
        _ => None,
    }
}

fn email(name: &str) -> Option<PyObjectRef> {
    match name {
        "email" => Some(email_modules::create_email_module()),
        "email.errors" => Some(email_modules::create_email_errors_module()),
        "email.message" => Some(email_modules::create_email_message_module()),
        "email.mime" => Some(email_modules::create_email_mime_module()),
        "email.mime.text" => Some(email_modules::create_email_mime_text_module()),
        "email.mime.multipart" => Some(email_modules::create_email_mime_multipart_module()),
        "email.mime.base" => Some(email_modules::create_email_mime_base_module()),
        "email.mime.application" => Some(email_modules::create_email_mime_application_module()),
        "email.mime.image" => Some(email_modules::create_email_mime_image_module()),
        "email.utils" => Some(email_modules::create_email_utils_module()),
        "email.policy" => Some(email_modules::create_email_policy_module()),
        "email.contentmanager" => Some(email_modules::create_email_contentmanager_module()),
        "email.charset" => Some(email_modules::create_email_charset_module()),
        _ => None,
    }
}

fn compression(name: &str) -> Option<PyObjectRef> {
    match name {
        "gzip" => Some(compression_modules::create_gzip_module()),
        "zipfile" => Some(compression_modules::create_zipfile_module()),
        "bz2" => Some(compression_modules::create_bz2_module()),
        "lzma" => Some(compression_modules::create_lzma_module()),
        "tarfile" => Some(compression_modules::create_tarfile_module()),
        _ => None,
    }
}

fn internal_aliases(name: &str) -> Option<PyObjectRef> {
    match name {
        "_collections_abc" => Some(type_modules::create_collections_abc_module()),
        "_functools" => Some(collection_modules::create_functools_module()),
        "_operator" => Some(collection_modules::create_operator_module()),
        "_csv" => Some(serial_modules::create_csv_module()),
        "_heapq" => Some(math_modules::create_heapq_accel_module()),
        "_json" => Some(serial_modules::create_json_module()),
        "_io" => Some(fs_modules::create_io_module()),
        "_collections" => Some(collection_modules::create_collections_module()),
        "_multibytecodec" => Some(text_modules::create_multibytecodec_module()),
        "_codecs" => Some(serial_modules::create_codecs_module()),
        "_string" => Some(text_modules::create_string_internal_module()),
        "_strptime" => Some(time_modules::create_strptime_module()),
        _ => None,
    }
}

fn compatibility(name: &str) -> Option<PyObjectRef> {
    match name {
        "__future__" => Some(misc_modules::create_future_module()),
        "builtins" => Some(misc_modules::create_builtins_module()),
        "_builtins" => Some(misc_modules::create_builtins_module()),
        "atexit" => Some(sys_modules::create_atexit_module()),
        "site" => Some(sys_modules::create_site_module()),
        "sched" => Some(sys_modules::create_sched_module()),
        "errno" => Some(sys_modules::create_errno_module()),
        _ => None,
    }
}

fn binary_encoding(name: &str) -> Option<PyObjectRef> {
    match name {
        "binascii" => Some(serial_modules::create_binascii_module()),
        _ => None,
    }
}

fn html_unicode(name: &str) -> Option<PyObjectRef> {
    match name {
        "html.parser" => Some(text_modules::create_html_parser_module()),
        "unicodedata" => Some(text_modules::create_unicodedata_module()),
        _ => None,
    }
}

fn network_extended(name: &str) -> Option<PyObjectRef> {
    match name {
        "http.server" => Some(network_modules::create_http_server_module()),
        "http.cookiejar" => Some(network_modules::create_http_cookiejar_module()),
        "http.cookies" => Some(network_modules::create_http_cookies_module()),
        "ssl" => Some(network_modules::create_ssl_module()),
        _ => None,
    }
}

fn xml_dom(name: &str) -> Option<PyObjectRef> {
    match name {
        "xml.dom" => Some(xml_modules::create_xml_dom_module()),
        "xml.dom.minidom" => Some(xml_modules::create_xml_dom_minidom_module()),
        _ => None,
    }
}

fn testing_debug(name: &str) -> Option<PyObjectRef> {
    match name {
        "logging" => Some(testing_modules::create_logging_module()),
        "logging.handlers" => Some(testing_modules::create_logging_handlers_module()),
        "logging.config" => Some(testing_modules::create_logging_config_module()),
        "unittest.mock" => Some(testing_modules::create_unittest_mock_module()),
        "pdb" => Some(testing_modules::create_pdb_module()),
        "profile" => Some(testing_modules::create_profile_module()),
        "cProfile" => Some(testing_modules::create_cprofile_module()),
        "timeit" => Some(testing_modules::create_timeit_module()),
        "faulthandler" => Some(testing_modules::create_faulthandler_module()),
        "tracemalloc" => Some(testing_modules::create_tracemalloc_module()),
        "pydoc" => Some(testing_modules::create_pydoc_module()),
        "_testcapi" => Some(testing_modules::create_testcapi_module()),
        "pickletools" => Some(testing_modules::create_pickletools_module()),
        _ => None,
    }
}

fn introspection_extended(name: &str) -> Option<PyObjectRef> {
    match name {
        "tokenize" => Some(introspection_modules::create_tokenize_module()),
        "symtable" => Some(introspection_modules::create_symtable_module()),
        _ => None,
    }
}

fn serialization_extended(name: &str) -> Option<PyObjectRef> {
    match name {
        "shelve" => Some(serial_modules::create_shelve_module()),
        "marshal" => Some(serial_modules::create_marshal_module()),
        _ => None,
    }
}

fn misc_extended(name: &str) -> Option<PyObjectRef> {
    match name {
        "contextvars" => Some(misc_modules::create_contextvars_module()),
        "mimetypes" => Some(misc_modules::create_mimetypes_module()),
        "readline" => Some(misc_modules::create_readline_module()),
        "runpy" => Some(misc_modules::create_runpy_module()),
        "cmd" => Some(misc_modules::create_cmd_module()),
        "compileall" => Some(misc_modules::create_compileall_module()),
        "pstats" => Some(misc_modules::create_pstats_module()),
        "quopri" => Some(misc_modules::create_quopri_module()),
        "stringprep" => Some(misc_modules::create_stringprep_module()),
        "plistlib" => Some(misc_modules::create_plistlib_module()),
        _ => None,
    }
}

fn network_stubs(name: &str) -> Option<PyObjectRef> {
    match name {
        "smtplib" => Some(network_modules::create_smtplib_module()),
        "ftplib" => Some(network_modules::create_ftplib_module()),
        "imaplib" => Some(network_modules::create_imaplib_module()),
        "poplib" => Some(network_modules::create_poplib_module()),
        "cgi" => Some(network_modules::create_cgi_module()),
        _ => None,
    }
}

fn dbm_xmlrpc(name: &str) -> Option<PyObjectRef> {
    match name {
        "dbm" | "dbm.dumb" | "dbm.gnu" | "dbm.ndbm" => Some(serial_modules::create_dbm_module()),
        "xmlrpc" | "xmlrpc.client" | "xmlrpc.server" => {
            Some(network_modules::create_xmlrpc_module())
        }
        _ => None,
    }
}

fn additional_misc(name: &str) -> Option<PyObjectRef> {
    match name {
        "uuid" => Some(crypto_modules::create_uuid_module()),
        "secrets" => Some(crypto_modules::create_secrets_module()),
        "hashlib" => Some(crypto_modules::create_hashlib_module()),
        "hmac" => Some(crypto_modules::create_hmac_module()),
        "concurrent" => Some(make_module("concurrent", vec![])),
        _ => None,
    }
}

fn system_config(name: &str) -> Option<PyObjectRef> {
    match name {
        "sysconfig" => Some(sys_modules::create_sysconfig_module()),
        "_sysconfig" => Some(sys_modules::create_sysconfig_module()),
        _ => None,
    }
}

fn encodings(name: &str) -> Option<PyObjectRef> {
    match name {
        "encodings" => Some(text_modules::create_encodings_module()),
        "encodings.utf_8" => Some(text_modules::create_encodings_codec_module(
            "encodings.utf_8",
        )),
        "encodings.ascii" => Some(text_modules::create_encodings_codec_module(
            "encodings.ascii",
        )),
        "encodings.latin_1" => Some(text_modules::create_encodings_codec_module(
            "encodings.latin_1",
        )),
        "encodings.aliases" => Some(text_modules::create_encodings_aliases_module()),
        "encodings.idna" => Some(text_modules::create_encodings_idna_module()),
        _ if name.starts_with("encodings.") => {
            Some(text_modules::create_encodings_codec_module(name))
        }
        _ => None,
    }
}

fn unix_terminal_ctypes(name: &str) -> Option<PyObjectRef> {
    match name {
        "grp" => Some(sys_modules::create_grp_module()),
        "pwd" => Some(sys_modules::create_pwd_module()),
        "curses" => Some(misc_modules::create_curses_module()),
        "_curses" => Some(misc_modules::create_curses_module()),
        "ctypes" => Some(misc_modules::create_ctypes_module()),
        "_ctypes" => Some(misc_modules::create_ctypes_module()),
        "ctypes.util" => {
            let m = misc_modules::create_ctypes_module();
            m.get_attr("util")
        }
        _ => None,
    }
}

fn import_resources_and_fallbacks(name: &str) -> Option<PyObjectRef> {
    match name {
        "importlib.resources" => Some(import_modules::create_importlib_resources_module()),
        "tabnanny" => Some(make_tabnanny_module()),
        "pyclbr" => Some(make_pyclbr_module()),
        _ => None,
    }
}

fn make_tabnanny_module() -> PyObjectRef {
    make_module(
        "tabnanny",
        vec![
            ("check", make_builtin(|_| Ok(PyObject::none()))),
            ("verbose", PyObject::int(0)),
        ],
    )
}

fn make_pyclbr_module() -> PyObjectRef {
    make_module(
        "pyclbr",
        vec![
            (
                "readmodule",
                make_builtin(|_| Ok(PyObject::dict_from_pairs(vec![]))),
            ),
            (
                "readmodule_ex",
                make_builtin(|_| Ok(PyObject::dict_from_pairs(vec![]))),
            ),
        ],
    )
}
