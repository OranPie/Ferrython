//! Ferrython Core — object model, error types, and runtime primitives.
//!
//! This crate defines `PyObject`, `PyObjectRef`, the `PyObjectPayload` enum
//! holding every built-in Python value kind, the exception hierarchy, and
//! helper types like `HashableKey` and `PyInt`.

pub mod error;
pub mod intern;
pub mod object;
pub mod types;

// ── Shared configuration for cross-crate communication ──

use parking_lot::RwLock;
use std::sync::LazyLock;

/// Extra paths to include in sys.path — populated by the import system,
/// read by the stdlib sys module builder. Avoids circular dependency between
/// ferrython-import and ferrython-stdlib.
static EXTRA_SYS_PATHS: LazyLock<RwLock<Vec<String>>> = LazyLock::new(|| RwLock::new(Vec::new()));

/// Set additional sys.path entries (called by ferrython-import on initialization).
pub fn set_extra_sys_paths(paths: Vec<String>) {
    *EXTRA_SYS_PATHS.write() = paths;
}

/// Get the extra sys.path entries (called by ferrython-stdlib when building sys module).
pub fn get_extra_sys_paths() -> Vec<String> {
    EXTRA_SYS_PATHS.read().clone()
}
