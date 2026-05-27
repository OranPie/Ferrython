use crate::{
    collection_modules, fs_modules, math_modules, misc_modules, network_modules, serial_modules,
    sys_modules, text_modules, time_modules, type_modules, xml_modules,
};
use ferrython_core::object::PyObjectRef;

pub(super) fn internal_aliases(name: &str) -> Option<PyObjectRef> {
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

pub(super) fn compatibility(name: &str) -> Option<PyObjectRef> {
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

pub(super) fn binary_encoding(name: &str) -> Option<PyObjectRef> {
    match name {
        "binascii" => Some(serial_modules::create_binascii_module()),
        _ => None,
    }
}

pub(super) fn html_unicode(name: &str) -> Option<PyObjectRef> {
    match name {
        "html.parser" => Some(text_modules::create_html_parser_module()),
        "unicodedata" => Some(text_modules::create_unicodedata_module()),
        _ => None,
    }
}

pub(super) fn network_extended(name: &str) -> Option<PyObjectRef> {
    match name {
        "http.server" => Some(network_modules::create_http_server_module()),
        "http.cookiejar" => Some(network_modules::create_http_cookiejar_module()),
        "http.cookies" => Some(network_modules::create_http_cookies_module()),
        "ssl" => Some(network_modules::create_ssl_module()),
        _ => None,
    }
}

pub(super) fn xml_dom(name: &str) -> Option<PyObjectRef> {
    match name {
        "xml.dom" => Some(xml_modules::create_xml_dom_module()),
        "xml.dom.minidom" => Some(xml_modules::create_xml_dom_minidom_module()),
        _ => None,
    }
}
