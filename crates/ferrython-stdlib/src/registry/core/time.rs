use crate::time_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "time" => Some(time_modules::create_time_module()),
        "datetime" => Some(time_modules::create_datetime_module()),
        "zoneinfo" => Some(time_modules::create_zoneinfo_module()),
        _ => None,
    }
}
