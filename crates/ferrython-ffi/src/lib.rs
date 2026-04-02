//! Ferrython FFI — Rust-native extension API for Python modules.
//!
//! This crate provides the framework for writing Python modules in Rust.
//! Extensions define a `ModuleDef`, which can be built into a `PyObjectRef`
//! module and registered with the import system.
//!
//! # Usage
//!
//! ```ignore
//! use ferrython_ffi::{ModuleDef, NativeMethod};
//!
//! fn hello(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
//!     Ok(PyObject::str_val("Hello from Rust!".into()))
//! }
//!
//! let module = ModuleDef::new("mymod")
//!     .method("hello", hello)
//!     .doc("A sample native module")
//!     .constant_int("VERSION", 1)
//!     .build();
//! ```

use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectRef};
use parking_lot::RwLock;

/// A function pointer type for native extension functions.
pub type NativeMethod = fn(&[PyObjectRef]) -> PyResult<PyObjectRef>;

/// A single method definition within a native module.
pub struct MethodDef {
    pub name: &'static str,
    pub func: NativeMethod,
    pub doc: &'static str,
}

/// A constant to inject into the module namespace.
pub enum ConstantDef {
    Int(i64),
    Float(f64),
    Str(CompactString),
    Bool(bool),
    None,
}

/// A native module definition. Extensions create this to register
/// their functions and constants with the interpreter.
pub struct ModuleDef {
    pub name: &'static str,
    pub doc: &'static str,
    pub methods: Vec<MethodDef>,
    pub constants: Vec<(&'static str, ConstantDef)>,
}

impl ModuleDef {
    /// Create a new module definition with the given name.
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            doc: "",
            methods: Vec::new(),
            constants: Vec::new(),
        }
    }

    /// Set the module docstring.
    pub fn doc(mut self, doc: &'static str) -> Self {
        self.doc = doc;
        self
    }

    /// Add a method to the module.
    pub fn method(mut self, name: &'static str, func: NativeMethod) -> Self {
        self.methods.push(MethodDef { name, func, doc: "" });
        self
    }

    /// Add a method with documentation.
    pub fn method_doc(mut self, name: &'static str, func: NativeMethod, doc: &'static str) -> Self {
        self.methods.push(MethodDef { name, func, doc });
        self
    }

    /// Add an integer constant to the module.
    pub fn constant_int(mut self, name: &'static str, value: i64) -> Self {
        self.constants.push((name, ConstantDef::Int(value)));
        self
    }

    /// Add a float constant to the module.
    pub fn constant_float(mut self, name: &'static str, value: f64) -> Self {
        self.constants.push((name, ConstantDef::Float(value)));
        self
    }

    /// Add a string constant to the module.
    pub fn constant_str(mut self, name: &'static str, value: &str) -> Self {
        self.constants.push((name, ConstantDef::Str(CompactString::from(value))));
        self
    }

    /// Add a boolean constant to the module.
    pub fn constant_bool(mut self, name: &'static str, value: bool) -> Self {
        self.constants.push((name, ConstantDef::Bool(value)));
        self
    }

    /// Build the module definition into a live `PyObjectRef` module.
    pub fn build(self) -> PyObjectRef {
        let mut entries: Vec<(&str, PyObjectRef)> = Vec::new();

        // Add methods
        for m in &self.methods {
            entries.push((m.name, make_builtin(m.func)));
        }

        // Add constants
        for (name, constant) in &self.constants {
            let val = match constant {
                ConstantDef::Int(v) => PyObject::int(*v),
                ConstantDef::Float(v) => PyObject::float(*v),
                ConstantDef::Str(v) => PyObject::str_val(v.clone()),
                ConstantDef::Bool(v) => PyObject::bool_val(*v),
                ConstantDef::None => PyObject::none(),
            };
            entries.push((name, val));
        }

        // Add __doc__
        if !self.doc.is_empty() {
            entries.push(("__doc__", PyObject::str_val(CompactString::from(self.doc))));
        }

        make_module(self.name, entries)
    }
}

// ── Native module registry ──

/// Global registry for native extension modules (loaded via FFI).
static NATIVE_MODULES: std::sync::LazyLock<RwLock<Vec<(&'static str, fn() -> ModuleDef)>>> =
    std::sync::LazyLock::new(|| RwLock::new(Vec::new()));

/// Register a native module factory. The factory will be called lazily
/// when the module is first imported.
pub fn register_native_module(name: &'static str, factory: fn() -> ModuleDef) {
    NATIVE_MODULES.write().push((name, factory));
}

/// Try to load a registered native module by name.
pub fn load_native_module(name: &str) -> Option<PyObjectRef> {
    let modules = NATIVE_MODULES.read();
    for &(mod_name, factory) in modules.iter() {
        if mod_name == name {
            return Some(factory().build());
        }
    }
    None
}