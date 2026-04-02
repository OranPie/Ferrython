//! Core Python object types — PyObject, PyObjectPayload, and supporting data types.

use crate::error::{PyResult, ExceptionKind};
use crate::types::{HashableKey, PyFunction, PyInt};
use compact_str::CompactString;
use indexmap::IndexMap;
use num_bigint::BigInt;
use parking_lot::RwLock;
use std::any::Any;
use std::fmt;
use std::sync::Arc;

/// A reference-counted handle to a Python object.
pub type PyObjectRef = Arc<PyObject>;

/// A Python object.
#[derive(Debug, Clone)]
pub struct PyObject {
    pub payload: PyObjectPayload,
}

/// The actual data of a Python value.
#[derive(Debug, Clone)]
pub enum PyObjectPayload {
    None,
    Ellipsis,
    NotImplemented,
    Bool(bool),
    Int(PyInt),
    Float(f64),
    Complex { real: f64, imag: f64 },
    Str(CompactString),
    Bytes(Vec<u8>),
    ByteArray(Vec<u8>),
    List(Arc<RwLock<Vec<PyObjectRef>>>),
    Tuple(Vec<PyObjectRef>),
    Set(Arc<RwLock<IndexMap<HashableKey, PyObjectRef>>>),
    FrozenSet(IndexMap<HashableKey, PyObjectRef>),
    Dict(Arc<RwLock<IndexMap<HashableKey, PyObjectRef>>>),
    Function(PyFunction),
    BuiltinFunction(CompactString),
    /// Built-in type object (int, str, float, etc.) — callable as constructor
    BuiltinType(CompactString),
    BoundMethod { receiver: PyObjectRef, method: PyObjectRef },
    BuiltinBoundMethod { receiver: PyObjectRef, method_name: CompactString },
    Code(Box<ferrython_bytecode::CodeObject>),
    Class(ClassData),
    Instance(InstanceData),
    Module(ModuleData),
    Iterator(Arc<std::sync::Mutex<IteratorData>>),
    Slice { start: Option<PyObjectRef>, stop: Option<PyObjectRef>, step: Option<PyObjectRef> },
    /// A cell object wrapping a shared mutable reference (for closures).
    Cell(Arc<RwLock<Option<PyObjectRef>>>),
    /// Exception type object (e.g. ValueError, TypeError)
    ExceptionType(ExceptionKind),
    /// Exception instance (raised exception with kind, message, and optional args)
    ExceptionInstance { kind: ExceptionKind, message: CompactString, args: Vec<PyObjectRef> },
    /// Generator object (suspended coroutine with opaque frame storage)
    Generator(Arc<RwLock<GeneratorState>>),
    /// Native Rust function callable from Python (for module functions)
    NativeFunction {
        name: CompactString,
        func: fn(&[PyObjectRef]) -> PyResult<PyObjectRef>,
    },
    /// Partial application (functools.partial)
    Partial {
        func: PyObjectRef,
        args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    },
    /// Property descriptor
    Property { fget: Option<PyObjectRef>, fset: Option<PyObjectRef>, fdel: Option<PyObjectRef> },
    /// Static method wrapper
    StaticMethod(PyObjectRef),
    /// Class method wrapper  
    ClassMethod(PyObjectRef),
    /// super() proxy — wraps (class, instance) for parent method dispatch
    Super { cls: PyObjectRef, instance: PyObjectRef },
}

/// Opaque generator state. The actual frame is stored as `Box<dyn Any>` and
/// downcast by the VM crate which owns the Frame type.
pub struct GeneratorState {
    pub name: CompactString,
    pub frame: Option<Box<dyn Any + Send + Sync>>,
    pub started: bool,
    pub finished: bool,
}

impl fmt::Debug for GeneratorState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GeneratorState")
            .field("name", &self.name)
            .field("started", &self.started)
            .field("finished", &self.finished)
            .finish()
    }
}

impl Clone for GeneratorState {
    fn clone(&self) -> Self {
        // Generators are not truly clonable; this is a placeholder for the derive requirement
        Self { name: self.name.clone(), frame: None, started: self.started, finished: self.finished }
    }
}

#[derive(Debug, Clone)]
pub struct ClassData {
    pub name: CompactString,
    pub bases: Vec<PyObjectRef>,
    pub namespace: Arc<RwLock<IndexMap<CompactString, PyObjectRef>>>,
    pub mro: Vec<PyObjectRef>,
}

#[derive(Debug, Clone)]
pub struct InstanceData {
    pub class: PyObjectRef,
    pub attrs: Arc<RwLock<IndexMap<CompactString, PyObjectRef>>>,
}

#[derive(Debug, Clone)]
pub struct ModuleData {
    pub name: CompactString,
    pub attrs: IndexMap<CompactString, PyObjectRef>,
}

#[derive(Debug, Clone)]
pub enum IteratorData {
    List { items: Vec<PyObjectRef>, index: usize },
    Tuple { items: Vec<PyObjectRef>, index: usize },
    Range { current: i64, stop: i64, step: i64 },
    Str { chars: Vec<char>, index: usize },
}

