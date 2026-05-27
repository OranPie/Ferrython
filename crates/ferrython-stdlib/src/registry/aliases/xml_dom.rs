use crate::xml_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "xml.dom" => Some(xml_modules::create_xml_dom_module()),
        "xml.dom.minidom" => Some(xml_modules::create_xml_dom_minidom_module()),
        _ => None,
    }
}
