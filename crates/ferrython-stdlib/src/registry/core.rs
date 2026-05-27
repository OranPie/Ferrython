use crate::{
    collection_modules, config_modules, fs_modules, math_modules, misc_modules, serial_modules,
    sys_modules, text_modules, time_modules, type_modules,
};
use ferrython_core::object::PyObjectRef;

pub(super) fn math(name: &str) -> Option<PyObjectRef> {
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

pub(super) fn system(name: &str) -> Option<PyObjectRef> {
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

pub(super) fn text(name: &str) -> Option<PyObjectRef> {
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

pub(super) fn collections(name: &str) -> Option<PyObjectRef> {
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

pub(super) fn serialization(name: &str) -> Option<PyObjectRef> {
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

pub(super) fn filesystem(name: &str) -> Option<PyObjectRef> {
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

pub(super) fn time(name: &str) -> Option<PyObjectRef> {
    match name {
        "time" => Some(time_modules::create_time_module()),
        "datetime" => Some(time_modules::create_datetime_module()),
        "zoneinfo" => Some(time_modules::create_zoneinfo_module()),
        _ => None,
    }
}

pub(super) fn type_system(name: &str) -> Option<PyObjectRef> {
    match name {
        "typing" => Some(type_modules::create_typing_module()),
        "abc" => Some(type_modules::create_abc_module()),
        "enum" => Some(type_modules::create_enum_module()),
        "types" => Some(type_modules::create_types_module()),
        "collections.abc" => Some(type_modules::create_collections_abc_module()),
        _ => None,
    }
}

pub(super) fn misc(name: &str) -> Option<PyObjectRef> {
    match name {
        "dataclasses" => Some(misc_modules::create_dataclasses_module()),
        "configparser" => Some(config_modules::create_configparser_module()),
        _ => None,
    }
}
