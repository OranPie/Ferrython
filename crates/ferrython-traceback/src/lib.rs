//! Python-compatible traceback system for Ferrython.
//!
//! Provides:
//! - Source line caching (like CPython's `linecache`)
//! - Line number resolution from bytecode instruction indices
//! - Rich traceback formatting with exception chaining
//! - Python `traceback` module API (format_exception, extract_tb, etc.)
//! - Frame info extraction for introspection

mod source_cache;
mod formatting;
mod module_api;

pub use source_cache::SourceCache;
pub use formatting::{format_traceback, resolve_lineno, format_exception_only};
pub use module_api::create_traceback_module;
