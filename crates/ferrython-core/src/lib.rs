//! Ferrython Core — object model, error types, and runtime primitives.
//!
//! This crate defines `PyObject`, `PyObjectRef`, the `PyObjectPayload` enum
//! holding every built-in Python value kind, the exception hierarchy, and
//! helper types like `HashableKey` and `PyInt`.

pub mod error;
pub mod intern;
pub mod object;
pub mod types;
