use super::*;

// ── traceback module ──

pub fn create_traceback_module() -> PyObjectRef {
    // Delegate to the dedicated ferrython-traceback crate
    ferrython_traceback::create_traceback_module()
}
