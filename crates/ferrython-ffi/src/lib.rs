//! Ferrython FFI — Rust-native extension API for Python modules.
//!
//! # Current Status
//!
//! Defines the trait and types for future native module extensions.
//! Not yet integrated with the VM.
//!
//! # Usage (future)
//!
//! ```ignore
//! use ferrython_ffi::{ModuleDef, ModuleMethod};
//!
//! pub fn init_module() -> ModuleDef {
//!     ModuleDef::new("mymodule")
//!         .method("hello", hello_fn)
//! }
//! ```

use ferrython_core::error::PyResult;
use ferrython_core::object::PyObjectRef;

/// A function pointer type for native extension functions.
pub type NativeMethod = fn(&[PyObjectRef]) -> PyResult<PyObjectRef>;

/// A single method definition within a native module.
pub struct ModuleMethod {
    pub name: &'static str,
    pub func: NativeMethod,
    pub doc: &'static str,
}

/// A native module definition. Extensions create this to register
/// their functions and constants with the interpreter.
pub struct ModuleDef {
    pub name: &'static str,
    pub methods: Vec<ModuleMethod>,
}

impl ModuleDef {
    /// Create a new module definition.
    pub fn new(name: &'static str) -> Self {
        Self { name, methods: Vec::new() }
    }

    /// Add a method to the module.
    pub fn method(mut self, name: &'static str, func: NativeMethod) -> Self {
        self.methods.push(ModuleMethod { name, func, doc: "" });
        self
    }
}