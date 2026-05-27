use crate::network_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "smtplib" => Some(network_modules::create_smtplib_module()),
        "ftplib" => Some(network_modules::create_ftplib_module()),
        "imaplib" => Some(network_modules::create_imaplib_module()),
        "poplib" => Some(network_modules::create_poplib_module()),
        "cgi" => Some(network_modules::create_cgi_module()),
        _ => None,
    }
}
