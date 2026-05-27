mod additional;
mod dbm_xmlrpc;
mod encodings;
mod import_resources;
mod introspection;
mod misc;
mod network_stubs;
mod serialization;
mod system_config;
mod testing;
mod unix_terminal;

use ferrython_core::object::PyObjectRef;

pub(super) fn testing_debug(name: &str) -> Option<PyObjectRef> {
    testing::resolve(name)
}

pub(super) fn introspection_extended(name: &str) -> Option<PyObjectRef> {
    introspection::resolve(name)
}

pub(super) fn serialization_extended(name: &str) -> Option<PyObjectRef> {
    serialization::resolve(name)
}

pub(super) fn misc_extended(name: &str) -> Option<PyObjectRef> {
    misc::resolve(name)
}

pub(super) fn network_stubs(name: &str) -> Option<PyObjectRef> {
    network_stubs::resolve(name)
}

pub(super) fn dbm_xmlrpc(name: &str) -> Option<PyObjectRef> {
    dbm_xmlrpc::resolve(name)
}

pub(super) fn additional_misc(name: &str) -> Option<PyObjectRef> {
    additional::resolve(name)
}

pub(super) fn system_config(name: &str) -> Option<PyObjectRef> {
    system_config::resolve(name)
}

pub(super) fn encodings(name: &str) -> Option<PyObjectRef> {
    encodings::resolve(name)
}

pub(super) fn unix_terminal_ctypes(name: &str) -> Option<PyObjectRef> {
    unix_terminal::resolve(name)
}

pub(super) fn import_resources_and_fallbacks(name: &str) -> Option<PyObjectRef> {
    import_resources::resolve(name)
}
