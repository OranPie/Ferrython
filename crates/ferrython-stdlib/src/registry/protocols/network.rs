use crate::network_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
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
