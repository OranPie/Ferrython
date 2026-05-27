use crate::xml_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
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
