use crate::{
    crypto_modules, import_modules, introspection_modules, misc_modules, network_modules,
    serial_modules, sys_modules, testing_modules, text_modules,
};
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef};

pub(super) fn testing_debug(name: &str) -> Option<PyObjectRef> {
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

pub(super) fn introspection_extended(name: &str) -> Option<PyObjectRef> {
    match name {
        "tokenize" => Some(introspection_modules::create_tokenize_module()),
        "symtable" => Some(introspection_modules::create_symtable_module()),
        _ => None,
    }
}

pub(super) fn serialization_extended(name: &str) -> Option<PyObjectRef> {
    match name {
        "shelve" => Some(serial_modules::create_shelve_module()),
        "marshal" => Some(serial_modules::create_marshal_module()),
        _ => None,
    }
}

pub(super) fn misc_extended(name: &str) -> Option<PyObjectRef> {
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

pub(super) fn network_stubs(name: &str) -> Option<PyObjectRef> {
    match name {
        "smtplib" => Some(network_modules::create_smtplib_module()),
        "ftplib" => Some(network_modules::create_ftplib_module()),
        "imaplib" => Some(network_modules::create_imaplib_module()),
        "poplib" => Some(network_modules::create_poplib_module()),
        "cgi" => Some(network_modules::create_cgi_module()),
        _ => None,
    }
}

pub(super) fn dbm_xmlrpc(name: &str) -> Option<PyObjectRef> {
    match name {
        "dbm" | "dbm.dumb" | "dbm.gnu" | "dbm.ndbm" => Some(serial_modules::create_dbm_module()),
        "xmlrpc" | "xmlrpc.client" | "xmlrpc.server" => {
            Some(network_modules::create_xmlrpc_module())
        }
        _ => None,
    }
}

pub(super) fn additional_misc(name: &str) -> Option<PyObjectRef> {
    match name {
        "uuid" => Some(crypto_modules::create_uuid_module()),
        "secrets" => Some(crypto_modules::create_secrets_module()),
        "hashlib" => Some(crypto_modules::create_hashlib_module()),
        "hmac" => Some(crypto_modules::create_hmac_module()),
        "concurrent" => Some(make_module("concurrent", vec![])),
        _ => None,
    }
}

pub(super) fn system_config(name: &str) -> Option<PyObjectRef> {
    match name {
        "sysconfig" => Some(sys_modules::create_sysconfig_module()),
        "_sysconfig" => Some(sys_modules::create_sysconfig_module()),
        _ => None,
    }
}

pub(super) fn encodings(name: &str) -> Option<PyObjectRef> {
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

pub(super) fn unix_terminal_ctypes(name: &str) -> Option<PyObjectRef> {
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

pub(super) fn import_resources_and_fallbacks(name: &str) -> Option<PyObjectRef> {
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
