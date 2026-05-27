use crate::{compression_modules, db_modules, email_modules, network_modules, xml_modules};
use ferrython_core::object::PyObjectRef;

pub(super) fn network(name: &str) -> Option<PyObjectRef> {
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

pub(super) fn xml(name: &str) -> Option<PyObjectRef> {
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

pub(super) fn database(name: &str) -> Option<PyObjectRef> {
    match name {
        "sqlite3" => Some(db_modules::create_sqlite3_module()),
        _ => None,
    }
}

pub(super) fn email(name: &str) -> Option<PyObjectRef> {
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

pub(super) fn compression(name: &str) -> Option<PyObjectRef> {
    match name {
        "gzip" => Some(compression_modules::create_gzip_module()),
        "zipfile" => Some(compression_modules::create_zipfile_module()),
        "bz2" => Some(compression_modules::create_bz2_module()),
        "lzma" => Some(compression_modules::create_lzma_module()),
        "tarfile" => Some(compression_modules::create_tarfile_module()),
        _ => None,
    }
}
