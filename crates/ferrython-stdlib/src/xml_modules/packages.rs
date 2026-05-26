//! XML package namespace modules.

use ferrython_core::object::{make_module, PyObjectRef};

use super::create_xml_etree_elementtree_module;

pub fn create_xml_module() -> PyObjectRef {
    // xml package — just expose the etree sub-module path
    make_module("xml", vec![("etree", create_xml_etree_module())])
}

pub fn create_xml_etree_module() -> PyObjectRef {
    make_module(
        "xml.etree",
        vec![("ElementTree", create_xml_etree_elementtree_module())],
    )
}
