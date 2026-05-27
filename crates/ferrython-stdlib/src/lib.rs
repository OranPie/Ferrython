//! Ferrython standard library — Rust-implemented stdlib modules.
//!
//! This crate provides all built-in Python standard library modules.
//! The VM calls `load_module(name)` to resolve `import` statements.

mod collection_modules;
mod compression_modules;
mod concurrency_modules;
mod config_modules;
mod crypto_modules;
pub mod db_modules;
mod email_modules;
mod fs_modules;
mod import_modules;
mod introspection_modules;
mod math_modules;
mod misc_modules;
mod network_modules;
mod registry;
mod serial_modules;
mod sys_modules;
mod testing_modules;
pub mod text_modules;
mod time_modules;
mod type_modules;
pub mod xml_modules;

use ferrython_core::object::PyObjectRef;
use parking_lot::RwLock;

pub use concurrency_modules::drain_deferred_calls;
pub use ferrython_async::take_asyncio_run_coro;
pub use import_modules::{
    take_import_module_request, take_reload_request, ImportModuleRequest, ReloadRequest,
};
pub use introspection_modules::ast_unparse_module;
pub use introspection_modules::module_ast_to_pyobject;
pub use introspection_modules::pyobj_ast_to_module;
pub use serial_modules::json_dumps_fn;
pub use sys_modules::get_argv;
pub use sys_modules::get_current_ctype_locale;
pub use sys_modules::get_current_sys_module;
pub use sys_modules::get_exc_info;
pub use sys_modules::get_recursion_limit;
pub use sys_modules::set_argv;
pub use sys_modules::{get_current_frame, set_current_frame};
pub use sys_modules::{get_current_globals, set_current_globals};
pub use sys_modules::{
    get_excepthook, get_profile_func, get_trace_func, set_excepthook, set_profile_func,
    set_trace_func,
};
pub use sys_modules::{is_profile_active, is_trace_active};

// ── Global stdout/stderr override for redirect_stdout/redirect_stderr ──
// When set, print() writes here instead of real stdout.
static STDOUT_OVERRIDE: std::sync::LazyLock<RwLock<Vec<PyObjectRef>>> =
    std::sync::LazyLock::new(|| RwLock::new(Vec::new()));
static STDERR_OVERRIDE: std::sync::LazyLock<RwLock<Vec<PyObjectRef>>> =
    std::sync::LazyLock::new(|| RwLock::new(Vec::new()));

/// Push a new stdout override (for redirect_stdout).
pub fn push_stdout_override(target: PyObjectRef) {
    STDOUT_OVERRIDE.write().push(target);
}
/// Pop the current stdout override (for redirect_stdout.__exit__).
pub fn pop_stdout_override() -> Option<PyObjectRef> {
    STDOUT_OVERRIDE.write().pop()
}
/// Get the current stdout override (None = use real stdout).
pub fn get_stdout_override() -> Option<PyObjectRef> {
    STDOUT_OVERRIDE.read().last().cloned()
}
/// Push a new stderr override.
pub fn push_stderr_override(target: PyObjectRef) {
    STDERR_OVERRIDE.write().push(target);
}
/// Pop the current stderr override.
pub fn pop_stderr_override() -> Option<PyObjectRef> {
    STDERR_OVERRIDE.write().pop()
}
/// Get the current stderr override.
pub fn get_stderr_override() -> Option<PyObjectRef> {
    STDERR_OVERRIDE.read().last().cloned()
}

/// Check if a module name corresponds to a built-in Rust-implemented module.
pub fn is_builtin_module(name: &str) -> bool {
    load_module(name).is_some()
}

/// Look up a built-in stdlib module by name.
/// Returns `Some(module)` if found, `None` otherwise.
pub fn load_module(name: &str) -> Option<PyObjectRef> {
    registry::load_module(name)
}
