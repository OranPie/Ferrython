mod binary_encoding;
mod compatibility;
mod html_unicode;
mod internal;
mod network_extended;
mod xml_dom;

use ferrython_core::object::PyObjectRef;

pub(super) fn internal_aliases(name: &str) -> Option<PyObjectRef> {
    internal::resolve(name)
}

pub(super) fn compatibility(name: &str) -> Option<PyObjectRef> {
    compatibility::resolve(name)
}

pub(super) fn binary_encoding(name: &str) -> Option<PyObjectRef> {
    binary_encoding::resolve(name)
}

pub(super) fn html_unicode(name: &str) -> Option<PyObjectRef> {
    html_unicode::resolve(name)
}

pub(super) fn network_extended(name: &str) -> Option<PyObjectRef> {
    network_extended::resolve(name)
}

pub(super) fn xml_dom(name: &str) -> Option<PyObjectRef> {
    xml_dom::resolve(name)
}
