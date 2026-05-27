use crate::{network_modules, serial_modules};
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "dbm" | "dbm.dumb" | "dbm.gnu" | "dbm.ndbm" => Some(serial_modules::create_dbm_module()),
        "xmlrpc" | "xmlrpc.client" | "xmlrpc.server" => {
            Some(network_modules::create_xmlrpc_module())
        }
        _ => None,
    }
}
