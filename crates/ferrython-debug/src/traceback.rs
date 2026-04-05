//! Traceback formatting and source line resolution.
//!
//! Delegates to the `ferrython-traceback` crate which owns the full
//! traceback system. This module re-exports for backward compatibility.

pub use ferrython_traceback::{resolve_lineno, format_traceback};

