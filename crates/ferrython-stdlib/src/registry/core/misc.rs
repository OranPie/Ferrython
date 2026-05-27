use crate::{config_modules, misc_modules};
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "dataclasses" => Some(misc_modules::create_dataclasses_module()),
        "configparser" => Some(config_modules::create_configparser_module()),
        _ => None,
    }
}
