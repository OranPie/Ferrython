use crate::introspection_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "warnings" => Some(introspection_modules::create_warnings_module()),
        "traceback" => Some(introspection_modules::create_traceback_module()),
        "inspect" => Some(introspection_modules::create_inspect_module()),
        "dis" => Some(introspection_modules::create_dis_module()),
        "_ast" => Some(introspection_modules::create_ast_module()),
        "linecache" => Some(introspection_modules::create_linecache_module()),
        "token" => Some(introspection_modules::create_token_module()),
        _ => None,
    }
}
