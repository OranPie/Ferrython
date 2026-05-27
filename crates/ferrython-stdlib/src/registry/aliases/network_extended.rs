use crate::network_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "http.server" => Some(network_modules::create_http_server_module()),
        "http.cookiejar" => Some(network_modules::create_http_cookiejar_module()),
        "http.cookies" => Some(network_modules::create_http_cookies_module()),
        "ssl" => Some(network_modules::create_ssl_module()),
        _ => None,
    }
}
