mod compression;
mod database;
mod email;
mod network;
mod xml;

use ferrython_core::object::PyObjectRef;

pub(super) fn network(name: &str) -> Option<PyObjectRef> {
    network::resolve(name)
}

pub(super) fn xml(name: &str) -> Option<PyObjectRef> {
    xml::resolve(name)
}

pub(super) fn database(name: &str) -> Option<PyObjectRef> {
    database::resolve(name)
}

pub(super) fn email(name: &str) -> Option<PyObjectRef> {
    email::resolve(name)
}

pub(super) fn compression(name: &str) -> Option<PyObjectRef> {
    compression::resolve(name)
}
