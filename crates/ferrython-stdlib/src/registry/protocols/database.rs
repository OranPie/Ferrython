use crate::db_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "sqlite3" => Some(db_modules::create_sqlite3_module()),
        _ => None,
    }
}
