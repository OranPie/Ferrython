use ferrython_core::object::PyObjectRef;

mod aliases;
mod core;
mod extras;
mod platform;
mod protocols;

pub(crate) fn load_module(name: &str) -> Option<PyObjectRef> {
    core::math(name)
        .or_else(|| core::system(name))
        .or_else(|| core::text(name))
        .or_else(|| core::collections(name))
        .or_else(|| core::serialization(name))
        .or_else(|| core::filesystem(name))
        .or_else(|| core::time(name))
        .or_else(|| core::type_system(name))
        .or_else(|| core::misc(name))
        .or_else(|| platform::introspection(name))
        .or_else(|| platform::concurrency(name))
        .or_else(|| platform::os_level(name))
        .or_else(|| platform::async_modules(name))
        .or_else(|| platform::import_system(name))
        .or_else(|| protocols::network(name))
        .or_else(|| protocols::xml(name))
        .or_else(|| protocols::database(name))
        .or_else(|| protocols::email(name))
        .or_else(|| protocols::compression(name))
        .or_else(|| aliases::internal_aliases(name))
        .or_else(|| aliases::compatibility(name))
        .or_else(|| aliases::binary_encoding(name))
        .or_else(|| aliases::html_unicode(name))
        .or_else(|| aliases::network_extended(name))
        .or_else(|| aliases::xml_dom(name))
        .or_else(|| extras::testing_debug(name))
        .or_else(|| extras::introspection_extended(name))
        .or_else(|| extras::serialization_extended(name))
        .or_else(|| extras::misc_extended(name))
        .or_else(|| extras::network_stubs(name))
        .or_else(|| extras::dbm_xmlrpc(name))
        .or_else(|| extras::additional_misc(name))
        .or_else(|| extras::system_config(name))
        .or_else(|| extras::encodings(name))
        .or_else(|| extras::unix_terminal_ctypes(name))
        .or_else(|| extras::import_resources_and_fallbacks(name))
}
