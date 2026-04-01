//! The Python object model — `PyObject`, `PyObjectRef`, `PyObjectPayload`.

use crate::error::{ExceptionKind, PyException, PyResult};
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

// ── Singletons ──
use std::sync::LazyLock;
static NONE_SINGLETON: LazyLock<PyObjectRef> = LazyLock::new(|| Arc::new(PyObject { payload: PyObjectPayload::None }));
static TRUE_SINGLETON: LazyLock<PyObjectRef> = LazyLock::new(|| Arc::new(PyObject { payload: PyObjectPayload::Bool(true) }));
static FALSE_SINGLETON: LazyLock<PyObjectRef> = LazyLock::new(|| Arc::new(PyObject { payload: PyObjectPayload::Bool(false) }));

// ── PyObject constructors ──

impl PyObject {
    pub fn wrap(payload: PyObjectPayload) -> PyObjectRef {
        Arc::new(PyObject { payload })
    }
    pub fn none() -> PyObjectRef { NONE_SINGLETON.clone() }
    pub fn ellipsis() -> PyObjectRef { Self::wrap(PyObjectPayload::Ellipsis) }
    pub fn not_implemented() -> PyObjectRef { Self::wrap(PyObjectPayload::NotImplemented) }
    pub fn bool_val(v: bool) -> PyObjectRef { if v { TRUE_SINGLETON.clone() } else { FALSE_SINGLETON.clone() } }
    pub fn int(v: i64) -> PyObjectRef { Self::wrap(PyObjectPayload::Int(PyInt::Small(v))) }
    pub fn big_int(v: BigInt) -> PyObjectRef { Self::wrap(PyObjectPayload::Int(PyInt::Big(Box::new(v)))) }
    pub fn float(v: f64) -> PyObjectRef { Self::wrap(PyObjectPayload::Float(v)) }
    pub fn complex(real: f64, imag: f64) -> PyObjectRef { Self::wrap(PyObjectPayload::Complex { real, imag }) }
    pub fn str_val(v: CompactString) -> PyObjectRef { Self::wrap(PyObjectPayload::Str(v)) }
    pub fn bytes(v: Vec<u8>) -> PyObjectRef { Self::wrap(PyObjectPayload::Bytes(v)) }
    pub fn list(items: Vec<PyObjectRef>) -> PyObjectRef { Self::wrap(PyObjectPayload::List(Arc::new(RwLock::new(items)))) }
    pub fn tuple(items: Vec<PyObjectRef>) -> PyObjectRef { Self::wrap(PyObjectPayload::Tuple(items)) }
    pub fn set(items: IndexMap<HashableKey, PyObjectRef>) -> PyObjectRef { Self::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(items)))) }
    pub fn dict(items: IndexMap<HashableKey, PyObjectRef>) -> PyObjectRef { Self::wrap(PyObjectPayload::Dict(Arc::new(RwLock::new(items)))) }
    pub fn function(func: PyFunction) -> PyObjectRef { Self::wrap(PyObjectPayload::Function(func)) }
    pub fn builtin_function(name: CompactString) -> PyObjectRef { Self::wrap(PyObjectPayload::BuiltinFunction(name)) }
    pub fn builtin_type(name: CompactString) -> PyObjectRef { Self::wrap(PyObjectPayload::BuiltinType(name)) }
    pub fn code(code: ferrython_bytecode::CodeObject) -> PyObjectRef { Self::wrap(PyObjectPayload::Code(Box::new(code))) }
    pub fn class(name: CompactString, bases: Vec<PyObjectRef>, namespace: IndexMap<CompactString, PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Class(ClassData { name, bases, namespace: Arc::new(RwLock::new(namespace)), mro: Vec::new() }))
    }
    pub fn instance(class: PyObjectRef) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Instance(InstanceData { class, attrs: Arc::new(RwLock::new(IndexMap::new())) }))
    }
    pub fn module(name: CompactString) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Module(ModuleData { name, attrs: IndexMap::new() }))
    }
    pub fn module_with_attrs(name: CompactString, attrs: IndexMap<CompactString, PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Module(ModuleData { name, attrs }))
    }
    pub fn native_function(name: &str, func: fn(&[PyObjectRef]) -> PyResult<PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::NativeFunction { name: CompactString::from(name), func })
    }
    pub fn dict_from_pairs(pairs: Vec<(PyObjectRef, PyObjectRef)>) -> PyObjectRef {
        let mut map = IndexMap::new();
        for (k, v) in pairs {
            if let Ok(hk) = k.to_hashable_key() {
                map.insert(hk, v);
            }
        }
        Self::wrap(PyObjectPayload::Dict(Arc::new(RwLock::new(map))))
    }
    pub fn slice(start: Option<PyObjectRef>, stop: Option<PyObjectRef>, step: Option<PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Slice { start, stop, step })
    }
    pub fn frozenset(items: IndexMap<HashableKey, PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::FrozenSet(items))
    }
    pub fn range(start: i64, stop: i64, step: i64) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Iterator(Arc::new(std::sync::Mutex::new(IteratorData::Range { current: start, stop, step }))))
    }
    pub fn cell(cell: Arc<RwLock<Option<PyObjectRef>>>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Cell(cell))
    }
    pub fn exception_type(kind: ExceptionKind) -> PyObjectRef {
        Self::wrap(PyObjectPayload::ExceptionType(kind))
    }
    pub fn exception_instance(kind: ExceptionKind, message: impl Into<String>) -> PyObjectRef {
        let msg: String = message.into();
        Self::wrap(PyObjectPayload::ExceptionInstance {
            kind,
            message: CompactString::from(msg),
            args: vec![],
        })
    }
    pub fn generator(name: CompactString, frame: Box<dyn Any + Send + Sync>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Generator(Arc::new(RwLock::new(GeneratorState {
            name,
            frame: Some(frame),
            started: false,
            finished: false,
        }))))
    }
}

// ── Extension trait for methods on PyObjectRef ──

pub trait PyObjectMethods {
    fn type_name(&self) -> &'static str;
    fn is_truthy(&self) -> bool;
    fn is_callable(&self) -> bool;
    fn is_same(&self, other: &Self) -> bool;
    fn py_to_string(&self) -> String;
    fn repr(&self) -> String;
    fn to_list(&self) -> PyResult<Vec<PyObjectRef>>;
    fn to_int(&self) -> PyResult<i64>;
    fn to_float(&self) -> PyResult<f64>;
    fn as_int(&self) -> Option<i64>;
    fn as_str(&self) -> Option<&str>;
    fn to_hashable_key(&self) -> PyResult<HashableKey>;
    fn add(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn sub(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn mul(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn floor_div(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn true_div(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn modulo(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn power(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn lshift(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn rshift(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn bit_and(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn bit_or(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn bit_xor(&self, other: &Self) -> PyResult<PyObjectRef>;
    fn negate(&self) -> PyResult<PyObjectRef>;
    fn positive(&self) -> PyResult<PyObjectRef>;
    fn invert(&self) -> PyResult<PyObjectRef>;
    fn py_abs(&self) -> PyResult<PyObjectRef>;
    fn compare(&self, other: &Self, op: CompareOp) -> PyResult<PyObjectRef>;
    fn get_attr(&self, name: &str) -> Option<PyObjectRef>;
    fn py_len(&self) -> PyResult<usize>;
    fn get_item(&self, key: &PyObjectRef) -> PyResult<PyObjectRef>;
    fn contains(&self, item: &PyObjectRef) -> PyResult<bool>;
    fn get_iter(&self) -> PyResult<PyObjectRef>;
    fn format_value(&self, spec: &str) -> PyResult<String>;
    fn dir(&self) -> Vec<CompactString>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp { Lt, Le, Eq, Ne, Gt, Ge }

/// Walk a class and its base classes (MRO) to find an attribute.
pub fn lookup_in_class_mro(class: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    if let PyObjectPayload::Class(cd) = &class.payload {
        // Check own namespace first
        if let Some(v) = cd.namespace.read().get(name).cloned() {
            return Some(v);
        }
        // Use computed MRO if available, otherwise walk bases recursively
        if !cd.mro.is_empty() {
            for base in &cd.mro {
                if let PyObjectPayload::Class(bcd) = &base.payload {
                    if let Some(v) = bcd.namespace.read().get(name).cloned() {
                        return Some(v);
                    }
                }
            }
        } else {
            for base in &cd.bases {
                if let Some(v) = lookup_in_class_mro(base, name) {
                    return Some(v);
                }
            }
        }
    }
    None
}

impl PyObjectMethods for PyObjectRef {
    fn type_name(&self) -> &'static str {
        match &self.payload {
            PyObjectPayload::None => "NoneType",
            PyObjectPayload::Ellipsis => "ellipsis",
            PyObjectPayload::NotImplemented => "NotImplementedType",
            PyObjectPayload::Bool(_) => "bool",
            PyObjectPayload::Int(_) => "int",
            PyObjectPayload::Float(_) => "float",
            PyObjectPayload::Complex { .. } => "complex",
            PyObjectPayload::Str(_) => "str",
            PyObjectPayload::Bytes(_) => "bytes",
            PyObjectPayload::ByteArray(_) => "bytearray",
            PyObjectPayload::List(_) => "list",
            PyObjectPayload::Tuple(_) => "tuple",
            PyObjectPayload::Set(_) => "set",
            PyObjectPayload::FrozenSet(_) => "frozenset",
            PyObjectPayload::Dict(_) => "dict",
            PyObjectPayload::Function(_) => "function",
            PyObjectPayload::BuiltinFunction(_) => "builtin_function_or_method",
            PyObjectPayload::BuiltinType(_) => "type",
            PyObjectPayload::BoundMethod { .. } => "method",
            PyObjectPayload::BuiltinBoundMethod { .. } => "builtin_method",
            PyObjectPayload::Code(_) => "code",
            PyObjectPayload::Class(_) => "type",
            PyObjectPayload::Instance(inst) => {
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    Box::leak(cd.name.to_string().into_boxed_str())
                } else { "object" }
            }
            PyObjectPayload::Module(_) => "module",
            PyObjectPayload::Iterator(_) => "iterator",
            PyObjectPayload::Slice { .. } => "slice",
            PyObjectPayload::Cell(_) => "cell",
            PyObjectPayload::ExceptionType(_) => "type",
            PyObjectPayload::ExceptionInstance { .. } => "exception",
            PyObjectPayload::Generator(_) => "generator",
            PyObjectPayload::NativeFunction { .. } => "builtin_function_or_method",
            PyObjectPayload::Property { .. } => "property",
            PyObjectPayload::StaticMethod(_) => "staticmethod",
            PyObjectPayload::ClassMethod(_) => "classmethod",
            PyObjectPayload::Super { .. } => "super",
        }
    }

    fn is_truthy(&self) -> bool {
        match &self.payload {
            PyObjectPayload::None => false,
            PyObjectPayload::Bool(b) => *b,
            PyObjectPayload::Int(n) => !n.is_zero(),
            PyObjectPayload::Float(f) => *f != 0.0,
            PyObjectPayload::Complex { real, imag } => *real != 0.0 || *imag != 0.0,
            PyObjectPayload::Str(s) => !s.is_empty(),
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => !b.is_empty(),
            PyObjectPayload::List(v) => !v.read().is_empty(),
            PyObjectPayload::Tuple(v) => !v.is_empty(),
            PyObjectPayload::Set(m) => !m.read().is_empty(),
            PyObjectPayload::FrozenSet(m) => !m.is_empty(),
            PyObjectPayload::Dict(m) => !m.read().is_empty(),
            _ => true,
        }
    }

    fn is_callable(&self) -> bool {
        matches!(&self.payload, PyObjectPayload::Function(_) | PyObjectPayload::BuiltinFunction(_)
            | PyObjectPayload::BuiltinType(_) | PyObjectPayload::BoundMethod { .. }
            | PyObjectPayload::Class(_) | PyObjectPayload::ExceptionType(_)
            | PyObjectPayload::NativeFunction { .. })
    }

    fn is_same(&self, other: &Self) -> bool { Arc::ptr_eq(self, other) }

    fn py_to_string(&self) -> String {
        match &self.payload {
            PyObjectPayload::None => "None".into(),
            PyObjectPayload::Bool(true) => "True".into(),
            PyObjectPayload::Bool(false) => "False".into(),
            PyObjectPayload::Int(n) => n.to_string(),
            PyObjectPayload::Float(f) => float_to_str(*f),
            PyObjectPayload::Complex { real, imag } => {
                if *real == 0.0 { format!("{}j", imag) }
                else { format!("({}+{}j)", real, imag) }
            }
            PyObjectPayload::Str(s) => s.to_string(),
            PyObjectPayload::Bytes(b) => format!("b{:?}", String::from_utf8_lossy(b)),
            PyObjectPayload::List(items) => format_collection("[", "]", &items.read()),
            PyObjectPayload::Tuple(items) => {
                if items.len() == 1 { format!("({},)", items[0].repr()) }
                else { format_collection("(", ")", items) }
            }
            PyObjectPayload::Set(m) => {
                let m = m.read();
                if m.is_empty() { "set()".into() }
                else { format_set("{", "}", &m) }
            }
            PyObjectPayload::Dict(m) => format_dict(&m.read()),
            PyObjectPayload::Ellipsis => "Ellipsis".into(),
            PyObjectPayload::NotImplemented => "NotImplemented".into(),
            PyObjectPayload::Function(f) => format!("<function {}>", f.name),
            PyObjectPayload::BuiltinFunction(n) => format!("<built-in function {}>", n),
            PyObjectPayload::BuiltinType(n) => format!("<class '{}'>", n),
            PyObjectPayload::Code(c) => format!("<code object {}>", c.name),
            PyObjectPayload::Class(cd) => format!("<class '{}'>", cd.name),
            PyObjectPayload::Instance(inst) => {
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    format!("<{} object>", cd.name)
                } else { "<object>".into() }
            }
            PyObjectPayload::Module(m) => format!("<module '{}'>", m.name),
            PyObjectPayload::Iterator(_) => "<iterator>".into(),
            PyObjectPayload::ExceptionType(kind) => format!("<class '{}'>", kind),
            PyObjectPayload::ExceptionInstance { kind, message, .. } => {
                // str(exception) returns just the message, like CPython
                if message.is_empty() {
                    String::new()
                } else {
                    message.to_string()
                }
            }
            _ => format!("<{}>", self.type_name()),
        }
    }

    fn repr(&self) -> String {
        match &self.payload {
            PyObjectPayload::Str(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
            PyObjectPayload::ExceptionInstance { kind, message, .. } => {
                if message.is_empty() {
                    format!("{}()", kind)
                } else {
                    format!("{}('{}')", kind, message)
                }
            }
            _ => self.py_to_string(),
        }
    }

    fn to_list(&self) -> PyResult<Vec<PyObjectRef>> {
        match &self.payload {
            PyObjectPayload::List(v) => Ok(v.read().clone()),
            PyObjectPayload::Tuple(v) => Ok(v.clone()),
            PyObjectPayload::Set(m) => Ok(m.read().values().cloned().collect()),
            PyObjectPayload::FrozenSet(m) => Ok(m.values().cloned().collect()),
            PyObjectPayload::Str(s) => Ok(s.chars().map(|c| PyObject::str_val(CompactString::from(c.to_string()))).collect()),
            PyObjectPayload::Dict(m) => Ok(m.read().keys().map(|k| k.to_object()).collect()),
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                Ok(b.iter().map(|byte| PyObject::int(*byte as i64)).collect())
            }
            PyObjectPayload::Iterator(iter_data) => {
                let data = iter_data.lock().unwrap();
                match &*data {
                    IteratorData::List { items, index } => Ok(items[*index..].to_vec()),
                    IteratorData::Tuple { items, index } => Ok(items[*index..].to_vec()),
                    IteratorData::Range { current, stop, step } => {
                        let mut result = Vec::new();
                        let mut val = *current;
                        while (*step > 0 && val < *stop) || (*step < 0 && val > *stop) {
                            result.push(PyObject::int(val));
                            val += step;
                        }
                        Ok(result)
                    }
                    IteratorData::Str { chars, index } => {
                        Ok(chars[*index..].iter().map(|c| PyObject::str_val(CompactString::from(c.to_string()))).collect())
                    }
                }
            }
            _ => Err(PyException::type_error(format!("'{}' object is not iterable", self.type_name()))),
        }
    }

    fn to_int(&self) -> PyResult<i64> {
        match &self.payload {
            PyObjectPayload::Int(n) => n.to_i64().ok_or_else(|| PyException::overflow_error("int too large")),
            PyObjectPayload::Bool(b) => Ok(if *b { 1 } else { 0 }),
            PyObjectPayload::Float(f) => Ok(*f as i64),
            PyObjectPayload::Str(s) => s.trim().parse::<i64>().map_err(|_|
                PyException::value_error(format!("invalid literal for int(): '{}'", s))),
            _ => Err(PyException::type_error(format!("int() argument must be a string or number, not '{}'", self.type_name()))),
        }
    }

    fn to_float(&self) -> PyResult<f64> {
        match &self.payload {
            PyObjectPayload::Float(f) => Ok(*f),
            PyObjectPayload::Int(n) => Ok(n.to_f64()),
            PyObjectPayload::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            PyObjectPayload::Str(s) => s.trim().parse::<f64>().map_err(|_|
                PyException::value_error(format!("could not convert string to float: '{}'", s))),
            _ => Err(PyException::type_error(format!("float() argument must be a string or number, not '{}'", self.type_name()))),
        }
    }

    fn as_int(&self) -> Option<i64> {
        match &self.payload {
            PyObjectPayload::Int(n) => n.to_i64(),
            PyObjectPayload::Bool(b) => Some(if *b { 1 } else { 0 }),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match &self.payload {
            PyObjectPayload::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    fn to_hashable_key(&self) -> PyResult<HashableKey> { HashableKey::from_object(self) }

    // ── Arithmetic ──

    fn add(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => Ok(PyInt::add_op(a, b).to_object()),
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a + b)),
            (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.to_f64() + b)),
            (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => Ok(PyObject::float(a + b.to_f64())),
            (PyObjectPayload::Complex { real: ar, imag: ai }, PyObjectPayload::Complex { real: br, imag: bi }) => {
                Ok(PyObject::complex(ar + br, ai + bi))
            }
            (PyObjectPayload::Int(a), PyObjectPayload::Complex { real, imag }) => {
                Ok(PyObject::complex(a.to_f64() + real, *imag))
            }
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Int(b)) => {
                Ok(PyObject::complex(real + b.to_f64(), *imag))
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Complex { real, imag }) => {
                Ok(PyObject::complex(a + real, *imag))
            }
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Float(b)) => {
                Ok(PyObject::complex(real + b, *imag))
            }
            (PyObjectPayload::Str(a), PyObjectPayload::Str(b)) => {
                let mut s = a.to_string(); s.push_str(b.as_str());
                Ok(PyObject::str_val(CompactString::from(s)))
            }
            (PyObjectPayload::List(a), PyObjectPayload::List(b)) => {
                let mut r = a.read().clone(); r.extend(b.read().iter().cloned()); Ok(PyObject::list(r))
            }
            (PyObjectPayload::Tuple(a), PyObjectPayload::Tuple(b)) => {
                let mut r = a.clone(); r.extend(b.iter().cloned()); Ok(PyObject::tuple(r))
            }
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for +: '{}' and '{}'", self.type_name(), other.type_name()))),
        }
    }

    fn sub(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => Ok(PyInt::sub_op(a, b).to_object()),
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a - b)),
            (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.to_f64() - b)),
            (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => Ok(PyObject::float(a - b.to_f64())),
            (PyObjectPayload::Complex { real: ar, imag: ai }, PyObjectPayload::Complex { real: br, imag: bi }) => {
                Ok(PyObject::complex(ar - br, ai - bi))
            }
            (PyObjectPayload::Int(a), PyObjectPayload::Complex { real, imag }) => {
                Ok(PyObject::complex(a.to_f64() - real, -*imag))
            }
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Int(b)) => {
                Ok(PyObject::complex(real - b.to_f64(), *imag))
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Complex { real, imag }) => {
                Ok(PyObject::complex(a - real, -*imag))
            }
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Float(b)) => {
                Ok(PyObject::complex(real - b, *imag))
            }
            (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
                let ra = a.read(); let rb = b.read();
                let mut result = IndexMap::new();
                for (k, v) in ra.iter() { if !rb.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(result)))))
            }
            (PyObjectPayload::FrozenSet(a), PyObjectPayload::FrozenSet(b)) => {
                let mut result = IndexMap::new();
                for (k, v) in a.iter() { if !b.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::FrozenSet(result)))
            }
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for -: '{}' and '{}'", self.type_name(), other.type_name()))),
        }
    }

    fn mul(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => Ok(PyInt::mul_op(a, b).to_object()),
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a * b)),
            (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.to_f64() * b)),
            (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => Ok(PyObject::float(a * b.to_f64())),
            (PyObjectPayload::Complex { real: ar, imag: ai }, PyObjectPayload::Complex { real: br, imag: bi }) => {
                Ok(PyObject::complex(ar * br - ai * bi, ar * bi + ai * br))
            }
            (PyObjectPayload::Int(a), PyObjectPayload::Complex { real, imag }) |
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Int(a)) => {
                let af = a.to_f64();
                Ok(PyObject::complex(af * real, af * imag))
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Complex { real, imag }) |
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Float(a)) => {
                Ok(PyObject::complex(a * real, a * imag))
            }
            (PyObjectPayload::Str(s), PyObjectPayload::Int(n)) | (PyObjectPayload::Int(n), PyObjectPayload::Str(s)) => {
                let count = n.to_i64().unwrap_or(0).max(0) as usize;
                Ok(PyObject::str_val(CompactString::from(s.repeat(count))))
            }
            (PyObjectPayload::List(items), PyObjectPayload::Int(n)) | (PyObjectPayload::Int(n), PyObjectPayload::List(items)) => {
                let count = n.to_i64().unwrap_or(0).max(0) as usize;
                let read = items.read();
                let mut result = Vec::with_capacity(read.len() * count);
                for _ in 0..count { result.extend(read.iter().cloned()); }
                Ok(PyObject::list(result))
            }
            (PyObjectPayload::Tuple(items), PyObjectPayload::Int(n)) | (PyObjectPayload::Int(n), PyObjectPayload::Tuple(items)) => {
                let count = n.to_i64().unwrap_or(0).max(0) as usize;
                let mut result = Vec::with_capacity(items.len() * count);
                for _ in 0..count { result.extend(items.iter().cloned()); }
                Ok(PyObject::tuple(result))
            }
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for *: '{}' and '{}'", self.type_name(), other.type_name()))),
        }
    }

    fn floor_div(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => {
                if b.is_zero() { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                Ok(PyInt::floor_div_op(a, b).to_object())
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => {
                if *b == 0.0 { return Err(PyException::zero_division_error("float floor division by zero")); }
                Ok(PyObject::float((a / b).floor()))
            }
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for //: '{}' and '{}'", self.type_name(), other.type_name()))),
        }
    }

    fn true_div(&self, other: &Self) -> PyResult<PyObjectRef> {
        // Complex division
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Complex { real: ar, imag: ai }, PyObjectPayload::Complex { real: br, imag: bi }) => {
                let denom = br * br + bi * bi;
                if denom == 0.0 { return Err(PyException::zero_division_error("complex division by zero")); }
                return Ok(PyObject::complex((ar * br + ai * bi) / denom, (ai * br - ar * bi) / denom));
            }
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Int(b)) => {
                let bf = b.to_f64();
                if bf == 0.0 { return Err(PyException::zero_division_error("complex division by zero")); }
                return Ok(PyObject::complex(real / bf, imag / bf));
            }
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Float(b)) => {
                if *b == 0.0 { return Err(PyException::zero_division_error("complex division by zero")); }
                return Ok(PyObject::complex(real / b, imag / b));
            }
            (PyObjectPayload::Int(a), PyObjectPayload::Complex { real: br, imag: bi }) => {
                let af = a.to_f64();
                let denom = br * br + bi * bi;
                if denom == 0.0 { return Err(PyException::zero_division_error("complex division by zero")); }
                return Ok(PyObject::complex((af * br) / denom, (-af * bi) / denom));
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Complex { real: br, imag: bi }) => {
                let denom = br * br + bi * bi;
                if denom == 0.0 { return Err(PyException::zero_division_error("complex division by zero")); }
                return Ok(PyObject::complex((a * br) / denom, (-a * bi) / denom));
            }
            _ => {}
        }
        let a = coerce_to_f64(self)?;
        let b = coerce_to_f64(other)?;
        if b == 0.0 { return Err(PyException::zero_division_error("division by zero")); }
        Ok(PyObject::float(a / b))
    }

    fn modulo(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => {
                if b.is_zero() { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                Ok(PyInt::modulo_op(a, b).to_object())
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => {
                if *b == 0.0 { return Err(PyException::zero_division_error("float modulo")); }
                Ok(PyObject::float(python_fmod(*a, *b)))
            }
            (PyObjectPayload::Str(fmt_str), _) => {
                // printf-style string formatting: "Hello %s" % "world"
                let args_list = match &other.payload {
                    PyObjectPayload::Tuple(items) => items.clone(),
                    _ => vec![other.clone()],
                };
                let mut result = String::new();
                let mut arg_idx = 0;
                let chars: Vec<char> = fmt_str.chars().collect();
                let mut i = 0;
                while i < chars.len() {
                    if chars[i] == '%' && i + 1 < chars.len() {
                        i += 1;
                        // Parse optional flags, width, precision
                        let mut spec_chars = String::new();
                        while i < chars.len() && "-+ #0123456789.".contains(chars[i]) {
                            spec_chars.push(chars[i]);
                            i += 1;
                        }
                        if i >= chars.len() { break; }
                        let conv = chars[i];
                        i += 1;
                        if conv == '%' {
                            result.push('%');
                            continue;
                        }
                        if arg_idx >= args_list.len() {
                            return Err(PyException::type_error("not enough arguments for format string"));
                        }
                        let arg = &args_list[arg_idx];
                        arg_idx += 1;
                        match conv {
                            's' => result.push_str(&arg.py_to_string()),
                            'r' => result.push_str(&arg.repr()),
                            'd' | 'i' => {
                                let n = arg.to_int()?;
                                if spec_chars.is_empty() {
                                    result.push_str(&n.to_string());
                                } else {
                                    result.push_str(&format_int_spec(n, &spec_chars));
                                }
                            }
                            'f' | 'F' => {
                                let f = arg.to_float()?;
                                if spec_chars.is_empty() {
                                    result.push_str(&format!("{:.6}", f));
                                } else {
                                    result.push_str(&format_float_spec(f, &spec_chars));
                                }
                            }
                            'x' => result.push_str(&format!("{:x}", arg.to_int()?)),
                            'X' => result.push_str(&format!("{:X}", arg.to_int()?)),
                            'o' => result.push_str(&format!("{:o}", arg.to_int()?)),
                            _ => {
                                result.push('%');
                                result.push_str(&spec_chars);
                                result.push(conv);
                            }
                        }
                    } else {
                        result.push(chars[i]);
                        i += 1;
                    }
                }
                Ok(PyObject::str_val(CompactString::from(result)))
            }
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for %: '{}' and '{}'", self.type_name(), other.type_name()))),
        }
    }

    fn power(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => {
                if let Some(exp) = b.to_i64() {
                    if exp >= 0 && exp <= 63 { return Ok(PyInt::pow_op(a, exp as u32).to_object()); }
                }
                Ok(PyObject::float(a.to_f64().powf(b.to_f64())))
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.powf(*b))),
            (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.to_f64().powf(*b))),
            (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => Ok(PyObject::float(a.powf(b.to_f64()))),
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for **: '{}' and '{}'", self.type_name(), other.type_name()))),
        }
    }

    fn lshift(&self, other: &Self) -> PyResult<PyObjectRef> { int_bitop(self, other, "<<", |a, b| a << b) }
    fn rshift(&self, other: &Self) -> PyResult<PyObjectRef> { int_bitop(self, other, ">>", |a, b| a >> b) }
    fn bit_and(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
                let ra = a.read(); let rb = b.read();
                let mut result = IndexMap::new();
                for (k, v) in ra.iter() { if rb.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(result)))))
            }
            _ => int_bitop(self, other, "&", |a, b| a & b),
        }
    }
    fn bit_or(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
                let ra = a.read(); let rb = b.read();
                let mut result = ra.clone();
                for (k, v) in rb.iter() { result.insert(k.clone(), v.clone()); }
                Ok(PyObject::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(result)))))
            }
            _ => int_bitop(self, other, "|", |a, b| a | b),
        }
    }
    fn bit_xor(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
                let ra = a.read(); let rb = b.read();
                let mut result = IndexMap::new();
                for (k, v) in ra.iter() { if !rb.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                for (k, v) in rb.iter() { if !ra.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(result)))))
            }
            _ => int_bitop(self, other, "^", |a, b| a ^ b),
        }
    }

    fn negate(&self) -> PyResult<PyObjectRef> {
        match &self.payload {
            PyObjectPayload::Int(n) => Ok(n.negate().to_object()),
            PyObjectPayload::Float(f) => Ok(PyObject::float(-f)),
            PyObjectPayload::Bool(b) => Ok(PyObject::int(-(*b as i64))),
            PyObjectPayload::Complex { real, imag } => Ok(PyObject::complex(-real, -imag)),
            _ => Err(PyException::type_error(format!("bad operand type for unary -: '{}'", self.type_name()))),
        }
    }

    fn positive(&self) -> PyResult<PyObjectRef> {
        match &self.payload {
            PyObjectPayload::Int(_) | PyObjectPayload::Float(_) | PyObjectPayload::Bool(_) |
            PyObjectPayload::Complex { .. } => Ok(self.clone()),
            _ => Err(PyException::type_error(format!("bad operand type for unary +: '{}'", self.type_name()))),
        }
    }

    fn invert(&self) -> PyResult<PyObjectRef> {
        match &self.payload {
            PyObjectPayload::Int(n) => Ok(n.invert().to_object()),
            PyObjectPayload::Bool(b) => Ok(PyObject::int(!(*b as i64))),
            _ => Err(PyException::type_error(format!("bad operand type for unary ~: '{}'", self.type_name()))),
        }
    }

    fn py_abs(&self) -> PyResult<PyObjectRef> {
        match &self.payload {
            PyObjectPayload::Int(n) => Ok(n.abs().to_object()),
            PyObjectPayload::Float(f) => Ok(PyObject::float(f.abs())),
            PyObjectPayload::Bool(b) => Ok(PyObject::int(*b as i64)),
            PyObjectPayload::Complex { real, imag } => Ok(PyObject::float((real * real + imag * imag).sqrt())),
            _ => Err(PyException::type_error(format!("bad operand type for abs(): '{}'", self.type_name()))),
        }
    }

    fn compare(&self, other: &Self, op: CompareOp) -> PyResult<PyObjectRef> {
        // Set comparisons: subset/superset semantics
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
                let ra = a.read(); let rb = b.read();
                let result = match op {
                    CompareOp::Eq => ra.len() == rb.len() && ra.keys().all(|k| rb.contains_key(k)),
                    CompareOp::Ne => !(ra.len() == rb.len() && ra.keys().all(|k| rb.contains_key(k))),
                    CompareOp::Le => ra.keys().all(|k| rb.contains_key(k)),  // issubset
                    CompareOp::Lt => ra.len() < rb.len() && ra.keys().all(|k| rb.contains_key(k)),
                    CompareOp::Ge => rb.keys().all(|k| ra.contains_key(k)),  // issuperset
                    CompareOp::Gt => ra.len() > rb.len() && rb.keys().all(|k| ra.contains_key(k)),
                };
                return Ok(PyObject::bool_val(result));
            }
            _ => {}
        }
        let ord = partial_cmp_objects(self, other);
        let result = match op {
            CompareOp::Eq => ord == Some(std::cmp::Ordering::Equal),
            CompareOp::Ne => ord != Some(std::cmp::Ordering::Equal),
            CompareOp::Lt => ord == Some(std::cmp::Ordering::Less),
            CompareOp::Le => matches!(ord, Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)),
            CompareOp::Gt => ord == Some(std::cmp::Ordering::Greater),
            CompareOp::Ge => matches!(ord, Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)),
        };
        Ok(PyObject::bool_val(result))
    }

    fn get_attr(&self, name: &str) -> Option<PyObjectRef> {
        match &self.payload {
            PyObjectPayload::Instance(inst) => {
                // Check class MRO for data descriptors (Property) first
                if let Some(v) = lookup_in_class_mro(&inst.class, name) {
                    match &v.payload {
                        PyObjectPayload::Property { .. } => {
                            // Return the property descriptor — VM will call fget
                            return Some(v.clone());
                        }
                        PyObjectPayload::StaticMethod(func) => {
                            return Some(func.clone());
                        }
                        PyObjectPayload::ClassMethod(func) => {
                            return Some(Arc::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: inst.class.clone(),
                                    method: func.clone(),
                                }
                            }));
                        }
                        _ => {}
                    }
                }
                // Instance attributes
                if let Some(v) = inst.attrs.read().get(name) { return Some(v.clone()); }
                // Walk the MRO for methods/class attrs
                if let Some(v) = lookup_in_class_mro(&inst.class, name) {
                    if matches!(&v.payload, PyObjectPayload::Function(_)) {
                        return Some(Arc::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: self.clone(),
                                method: v.clone(),
                            }
                        }));
                    }
                    return Some(v.clone());
                }
                None
            }
            PyObjectPayload::Class(cd) => {
                // Special class attributes
                if name == "__name__" {
                    return Some(PyObject::str_val(cd.name.clone()));
                }
                if name == "__bases__" {
                    return Some(PyObject::tuple(cd.bases.clone()));
                }
                if name == "__mro__" {
                    let mut mro_list = vec![self.clone()];
                    mro_list.extend(cd.mro.iter().cloned());
                    return Some(PyObject::tuple(mro_list));
                }
                // Check own namespace first, then bases
                if let Some(v) = cd.namespace.read().get(name).cloned() {
                    match &v.payload {
                        PyObjectPayload::StaticMethod(func) => return Some(func.clone()),
                        PyObjectPayload::ClassMethod(func) => {
                            return Some(Arc::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: self.clone(),
                                    method: func.clone(),
                                }
                            }));
                        }
                        _ => return Some(v),
                    }
                }
                for base in &cd.bases {
                    if let Some(v) = base.get_attr(name) { return Some(v); }
                }
                None
            }
            PyObjectPayload::Module(m) => m.attrs.get(name).cloned(),
            PyObjectPayload::Slice { start, stop, step } => {
                match name {
                    "start" => Some(start.clone().unwrap_or_else(PyObject::none)),
                    "stop" => Some(stop.clone().unwrap_or_else(PyObject::none)),
                    "step" => Some(step.clone().unwrap_or_else(PyObject::none)),
                    _ => None,
                }
            }
            PyObjectPayload::Complex { real, imag } => {
                match name {
                    "real" => Some(PyObject::float(*real)),
                    "imag" => Some(PyObject::float(*imag)),
                    "conjugate" => Some(Arc::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod {
                            receiver: self.clone(),
                            method_name: CompactString::from("conjugate"),
                        }
                    })),
                    _ => None,
                }
            }
            PyObjectPayload::BuiltinType(n) => {
                match name {
                    "__name__" => Some(PyObject::str_val(n.clone())),
                    _ => None,
                }
            }
            PyObjectPayload::Property { fget, fset, fdel } => {
                match name {
                    "setter" | "getter" | "deleter" | "fget" | "fset" | "fdel" => {
                        match name {
                            "fget" => return fget.clone().or_else(|| Some(PyObject::none())),
                            "fset" => return fset.clone().or_else(|| Some(PyObject::none())),
                            "fdel" => return fdel.clone().or_else(|| Some(PyObject::none())),
                            _ => {}
                        }
                        // Return a BuiltinBoundMethod that the VM will handle
                        Some(Arc::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod {
                                receiver: self.clone(),
                                method_name: CompactString::from(name),
                            }
                        }))
                    }
                    _ => None,
                }
            }
            PyObjectPayload::ExceptionType(kind) => {
                match name {
                    "__name__" => Some(PyObject::str_val(CompactString::from(format!("{:?}", kind)))),
                    _ => None,
                }
            }
            PyObjectPayload::ExceptionInstance { kind, message, args } => {
                match name {
                    "args" => {
                        if args.is_empty() {
                            if message.is_empty() {
                                Some(PyObject::tuple(vec![]))
                            } else {
                                Some(PyObject::tuple(vec![PyObject::str_val(message.clone())]))
                            }
                        } else {
                            Some(PyObject::tuple(args.clone()))
                        }
                    }
                    "__class__" => Some(PyObject::exception_type(kind.clone())),
                    _ => None,
                }
            }
            // Built-in type methods — return bound method names
            PyObjectPayload::Str(_) | PyObjectPayload::List(_) |
            PyObjectPayload::Dict(_) | PyObjectPayload::Tuple(_) |
            PyObjectPayload::Set(_) | PyObjectPayload::Int(_) |
            PyObjectPayload::Float(_) | PyObjectPayload::Bytes(_) => {
                // Create a BoundMethod wrapping (self_obj, method_name)
                Some(Arc::new(PyObject {
                    payload: PyObjectPayload::BuiltinBoundMethod {
                        receiver: self.clone(),
                        method_name: CompactString::from(name),
                    }
                }))
            }
            PyObjectPayload::Generator(_) => {
                match name {
                    "send" | "throw" | "close" | "__next__" => {
                        Some(Arc::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod {
                                receiver: self.clone(),
                                method_name: CompactString::from(name),
                            }
                        }))
                    }
                    _ => None,
                }
            }
            PyObjectPayload::Super { cls, instance } => {
                // super() proxy: look up in the RUNTIME class MRO, skipping up to and including cls
                let runtime_cls = if let PyObjectPayload::Instance(inst) = &instance.payload {
                    Some(&inst.class)
                } else {
                    None
                };
                if let Some(rt_cls) = runtime_cls {
                    if let PyObjectPayload::Class(cd) = &rt_cls.payload {
                        let mro = &cd.mro;
                        let mut found_cls = false;
                        for base in mro {
                            if !found_cls {
                                if std::sync::Arc::ptr_eq(base, cls) {
                                    found_cls = true;
                                }
                                continue;
                            }
                            // Look in this base's namespace directly
                            if let PyObjectPayload::Class(bcd) = &base.payload {
                                if let Some(v) = bcd.namespace.read().get(name) {
                                    if matches!(&v.payload, PyObjectPayload::Function(_)) {
                                        return Some(Arc::new(PyObject {
                                            payload: PyObjectPayload::BoundMethod {
                                                receiver: instance.clone(),
                                                method: v.clone(),
                                            }
                                        }));
                                    }
                                    return Some(v.clone());
                                }
                            }
                        }
                        // Fallback: if cls not found in MRO, look in cls's own bases
                        if !found_cls {
                            if let PyObjectPayload::Class(ccd) = &cls.payload {
                                for base in &ccd.bases {
                                    if let PyObjectPayload::Class(bcd) = &base.payload {
                                        if let Some(v) = bcd.namespace.read().get(name) {
                                            if matches!(&v.payload, PyObjectPayload::Function(_)) {
                                                return Some(Arc::new(PyObject {
                                                    payload: PyObjectPayload::BoundMethod {
                                                        receiver: instance.clone(),
                                                        method: v.clone(),
                                                    }
                                                }));
                                            }
                                            return Some(v.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn py_len(&self) -> PyResult<usize> {
        match &self.payload {
            PyObjectPayload::Str(s) => Ok(s.chars().count()),
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Ok(b.len()),
            PyObjectPayload::List(v) => Ok(v.read().len()),
            PyObjectPayload::Tuple(v) => Ok(v.len()),
            PyObjectPayload::Set(m) => Ok(m.read().len()),
            PyObjectPayload::FrozenSet(m) => Ok(m.len()),
            PyObjectPayload::Dict(m) => Ok(m.read().len()),
            _ => Err(PyException::type_error(format!("object of type '{}' has no len()", self.type_name()))),
        }
    }

    fn get_item(&self, key: &PyObjectRef) -> PyResult<PyObjectRef> {
        // Check for slice key first
        if let PyObjectPayload::Slice { start, stop, step } = &key.payload {
            return get_slice_impl(self, start, stop, step);
        }
        match &self.payload {
            PyObjectPayload::List(items) => {
                let items = items.read();
                let idx = key.to_int()?;
                let len = items.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len { return Err(PyException::index_error("index out of range")); }
                Ok(items[actual as usize].clone())
            }
            PyObjectPayload::Tuple(items) => {
                let idx = key.to_int()?;
                let len = items.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len { return Err(PyException::index_error("index out of range")); }
                Ok(items[actual as usize].clone())
            }
            PyObjectPayload::Dict(map) => {
                let hk = key.to_hashable_key()?;
                map.read().get(&hk).cloned().ok_or_else(|| PyException::key_error(key.repr()))
            }
            PyObjectPayload::Str(s) => {
                let idx = key.to_int()?;
                let chars: Vec<char> = s.chars().collect();
                let len = chars.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len { return Err(PyException::index_error("string index out of range")); }
                Ok(PyObject::str_val(CompactString::from(chars[actual as usize].to_string())))
            }
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                let idx = key.to_int()?;
                let len = b.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len { return Err(PyException::index_error("index out of range")); }
                Ok(PyObject::int(b[actual as usize] as i64))
            }
            _ => Err(PyException::type_error(format!("'{}' object is not subscriptable", self.type_name()))),
        }
    }

    fn contains(&self, item: &PyObjectRef) -> PyResult<bool> {
        match &self.payload {
            PyObjectPayload::List(v) => {
                let v = v.read();
                Ok(v.iter().any(|x| partial_cmp_objects(x, item) == Some(std::cmp::Ordering::Equal)))
            }
            PyObjectPayload::Tuple(v) => {
                Ok(v.iter().any(|x| partial_cmp_objects(x, item) == Some(std::cmp::Ordering::Equal)))
            }
            PyObjectPayload::Str(haystack) => {
                if let Some(needle) = item.as_str() { Ok(haystack.contains(needle)) }
                else { Err(PyException::type_error("'in <string>' requires string as left operand")) }
            }
            PyObjectPayload::Set(m) => {
                let hk = item.to_hashable_key()?;
                Ok(m.read().contains_key(&hk))
            }
            PyObjectPayload::FrozenSet(m) => {
                let hk = item.to_hashable_key()?;
                Ok(m.contains_key(&hk))
            }
            PyObjectPayload::Dict(m) => {
                let hk = item.to_hashable_key()?;
                Ok(m.read().contains_key(&hk))
            }
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                // Support: int in bytes (single byte) or bytes in bytes (subsequence)
                match &item.payload {
                    PyObjectPayload::Int(n) => {
                        let val = n.to_i64().unwrap_or(-1);
                        if val < 0 || val > 255 { return Ok(false); }
                        Ok(b.contains(&(val as u8)))
                    }
                    PyObjectPayload::Bytes(needle) | PyObjectPayload::ByteArray(needle) => {
                        if needle.is_empty() { return Ok(true); }
                        Ok(b.windows(needle.len()).any(|w| w == needle.as_slice()))
                    }
                    _ => Err(PyException::type_error("a bytes-like object is required")),
                }
            }
            _ => Err(PyException::type_error(format!("argument of type '{}' is not iterable", self.type_name()))),
        }
    }

    fn get_iter(&self) -> PyResult<PyObjectRef> {
        use std::sync::Mutex;
        match &self.payload {
            PyObjectPayload::List(items) => Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items: items.read().clone(), index: 0 }))))),
            PyObjectPayload::Tuple(items) => Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::Tuple { items: items.clone(), index: 0 }))))),
            PyObjectPayload::Str(s) => Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::Str { chars: s.chars().collect(), index: 0 }))))),
            PyObjectPayload::Dict(m) => {
                let keys: Vec<PyObjectRef> = m.read().keys().map(|k| k.to_object()).collect();
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items: keys, index: 0 })))))
            }
            PyObjectPayload::Set(m) => {
                let vals: Vec<PyObjectRef> = m.read().values().cloned().collect();
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items: vals, index: 0 })))))
            }
            PyObjectPayload::FrozenSet(m) => {
                let vals: Vec<PyObjectRef> = m.values().cloned().collect();
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items: vals, index: 0 })))))
            }
            PyObjectPayload::Iterator(_) => Ok(self.clone()),
            PyObjectPayload::Generator(_) => Ok(self.clone()), // generators are their own iterators
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                let items: Vec<PyObjectRef> = b.iter().map(|byte| PyObject::int(*byte as i64)).collect();
                Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items, index: 0 })))))
            }
            _ => Err(PyException::type_error(format!("'{}' object is not iterable", self.type_name()))),
        }
    }

    fn format_value(&self, spec: &str) -> PyResult<String> {
        if spec.is_empty() {
            return Ok(self.py_to_string());
        }
        // Parse format spec: [[fill]align][sign][#][0][width][grouping_option][.precision][type]
        let spec_bytes = spec.as_bytes();
        let len = spec_bytes.len();
        // Simple parsing for common cases
        let type_char = spec_bytes[len - 1] as char;
        match type_char {
            'd' => {
                let n = self.to_int()?;
                let inner_spec = &spec[..len - 1];
                if inner_spec.is_empty() {
                    return Ok(n.to_string());
                }
                return Ok(apply_string_format_spec(&n.to_string(), inner_spec));
            }
            'f' | 'F' => {
                let f = self.to_float()?;
                let inner_spec = &spec[..len - 1];
                if let Some(dot_pos) = inner_spec.rfind('.') {
                    let prec: usize = inner_spec[dot_pos + 1..].parse().unwrap_or(6);
                    let num_str = format!("{:.prec$}", f, prec = prec);
                    let pre_dot = &inner_spec[..dot_pos];
                    if pre_dot.is_empty() {
                        return Ok(num_str);
                    }
                    return Ok(apply_string_format_spec(&num_str, pre_dot));
                }
                return Ok(format!("{:.6}", f));
            }
            'e' | 'E' => {
                let f = self.to_float()?;
                let inner_spec = &spec[..len - 1];
                let prec = if let Some(dot_pos) = inner_spec.rfind('.') {
                    inner_spec[dot_pos + 1..].parse().unwrap_or(6)
                } else { 6 };
                if type_char == 'e' {
                    return Ok(format!("{:.prec$e}", f, prec = prec));
                } else {
                    return Ok(format!("{:.prec$E}", f, prec = prec));
                }
            }
            'b' => return Ok(format!("{:b}", self.to_int()?)),
            'o' => return Ok(format!("{:o}", self.to_int()?)),
            'x' => return Ok(format!("{:x}", self.to_int()?)),
            'X' => return Ok(format!("{:X}", self.to_int()?)),
            's' => {
                let s = self.py_to_string();
                let inner_spec = &spec[..len - 1];
                if inner_spec.is_empty() { return Ok(s); }
                return Ok(apply_string_format_spec(&s, inner_spec));
            }
            _ => {
                // No type char — treat entire spec as alignment spec
                let s = self.py_to_string();
                return Ok(apply_string_format_spec(&s, spec));
            }
        }
    }

    fn dir(&self) -> Vec<CompactString> {
        match &self.payload {
            PyObjectPayload::Instance(inst) => {
                let mut names: Vec<CompactString> = inst.attrs.read().keys().cloned().collect();
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    names.extend(cd.namespace.read().keys().cloned());
                }
                names.sort(); names.dedup(); names
            }
            PyObjectPayload::Class(cd) => { let mut n: Vec<_> = cd.namespace.read().keys().cloned().collect(); n.sort(); n }
            PyObjectPayload::Module(m) => { let mut n: Vec<_> = m.attrs.keys().cloned().collect(); n.sort(); n }
            _ => vec![],
        }
    }
}

// ── Helpers ──

fn coerce_to_f64(obj: &PyObjectRef) -> PyResult<f64> {
    match &obj.payload {
        PyObjectPayload::Float(f) => Ok(*f),
        PyObjectPayload::Int(n) => Ok(n.to_f64()),
        PyObjectPayload::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        _ => Err(PyException::type_error(format!("must be real number, not {}", obj.type_name()))),
    }
}

fn int_bitop(a: &PyObjectRef, b: &PyObjectRef, op_name: &str, op: fn(i64, i64) -> i64) -> PyResult<PyObjectRef> {
    let ai = a.to_int().map_err(|_| PyException::type_error(format!(
        "unsupported operand type(s) for {}: '{}' and '{}'", op_name, a.type_name(), b.type_name())))?;
    let bi = b.to_int().map_err(|_| PyException::type_error(format!(
        "unsupported operand type(s) for {}: '{}' and '{}'", op_name, a.type_name(), b.type_name())))?;
    Ok(PyObject::int(op(ai, bi)))
}

fn partial_cmp_objects(a: &PyObjectRef, b: &PyObjectRef) -> Option<std::cmp::Ordering> {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::None, PyObjectPayload::None) => Some(std::cmp::Ordering::Equal),
        (PyObjectPayload::Bool(a), PyObjectPayload::Bool(b)) => a.partial_cmp(b),
        (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => a.partial_cmp(b),
        (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => a.partial_cmp(b),
        (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => a.to_f64().partial_cmp(b),
        (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => a.partial_cmp(&b.to_f64()),
        (PyObjectPayload::Str(a), PyObjectPayload::Str(b)) => a.partial_cmp(b),
        (PyObjectPayload::Bool(a), PyObjectPayload::Int(b)) => PyInt::Small(*a as i64).partial_cmp(b),
        (PyObjectPayload::Int(a), PyObjectPayload::Bool(b)) => a.partial_cmp(&PyInt::Small(*b as i64)),
        (PyObjectPayload::List(a), PyObjectPayload::List(b)) => {
            let a = a.read(); let b = b.read();
            for (x, y) in a.iter().zip(b.iter()) {
                match partial_cmp_objects(x, y) {
                    Some(std::cmp::Ordering::Equal) => continue,
                    other => return other,
                }
            }
            a.len().partial_cmp(&b.len())
        }
        (PyObjectPayload::Tuple(a), PyObjectPayload::Tuple(b)) => {
            for (x, y) in a.iter().zip(b.iter()) {
                match partial_cmp_objects(x, y) {
                    Some(std::cmp::Ordering::Equal) => continue,
                    other => return other,
                }
            }
            a.len().partial_cmp(&b.len())
        }
        (PyObjectPayload::BuiltinFunction(a), PyObjectPayload::BuiltinFunction(b)) => {
            if a == b { Some(std::cmp::Ordering::Equal) } else { None }
        }
        (PyObjectPayload::Bytes(a), PyObjectPayload::Bytes(b)) => a.partial_cmp(b),
        (PyObjectPayload::ByteArray(a), PyObjectPayload::ByteArray(b)) => a.partial_cmp(b),
        (PyObjectPayload::Complex { real: ar, imag: ai }, PyObjectPayload::Complex { real: br, imag: bi }) => {
            if ar == br && ai == bi { Some(std::cmp::Ordering::Equal) } else { None }
        }
        (PyObjectPayload::BuiltinType(a), PyObjectPayload::BuiltinType(b)) => {
            if a == b { Some(std::cmp::Ordering::Equal) } else { None }
        }
        (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
            let a = a.read(); let b = b.read();
            if a.len() != b.len() { return None; }
            for k in a.keys() {
                if !b.contains_key(k) { return None; }
            }
            Some(std::cmp::Ordering::Equal)
        }
        (PyObjectPayload::FrozenSet(a), PyObjectPayload::FrozenSet(b)) => {
            // Set equality: same keys
            if a.len() != b.len() { return None; }
            for k in a.keys() {
                if !b.contains_key(k) { return None; }
            }
            Some(std::cmp::Ordering::Equal)
        }
        (PyObjectPayload::Dict(a), PyObjectPayload::Dict(b)) => {
            let a = a.read(); let b = b.read();
            if a.len() != b.len() { return None; }
            for (k, v1) in a.iter() {
                match b.get(k) {
                    Some(v2) if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                    _ => return None,
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        // Class identity comparison (same Arc pointer = same class)
        (PyObjectPayload::Class(a), PyObjectPayload::Class(b)) => {
            if a.name == b.name { Some(std::cmp::Ordering::Equal) } else { None }
        }
        // ExceptionType comparison
        (PyObjectPayload::ExceptionType(a), PyObjectPayload::ExceptionType(b)) => {
            if a == b { Some(std::cmp::Ordering::Equal) } else { None }
        }
        _ => None,
    }
}

fn float_to_str(f: f64) -> String {
    if f == f64::INFINITY { "inf".into() }
    else if f == f64::NEG_INFINITY { "-inf".into() }
    else if f.is_nan() { "nan".into() }
    else {
        let s = format!("{}", f);
        if s.contains('.') || s.contains('e') { s } else { format!("{}.0", s) }
    }
}

fn python_fmod(a: f64, b: f64) -> f64 {
    let r = a % b;
    if (r != 0.0) && ((r < 0.0) != (b < 0.0)) { r + b } else { r }
}

fn format_int_spec(n: i64, spec: &str) -> String {
    // Parse width from spec
    let width: usize = spec.trim_start_matches(|c: char| "- +#0".contains(c))
        .parse().unwrap_or(0);
    let zero_pad = spec.starts_with('0');
    let left_align = spec.starts_with('-');
    let s = n.to_string();
    if width == 0 { return s; }
    if zero_pad && !left_align {
        if n < 0 {
            format!("-{:0>width$}", &s[1..], width = width - 1)
        } else {
            format!("{:0>width$}", s, width = width)
        }
    } else if left_align {
        format!("{:<width$}", s, width = width)
    } else {
        format!("{:>width$}", s, width = width)
    }
}

fn format_float_spec(f: f64, spec: &str) -> String {
    // Parse precision from spec (e.g., ".2")
    if let Some(dot_pos) = spec.find('.') {
        let prec_str = &spec[dot_pos + 1..];
        let prec: usize = prec_str.parse().unwrap_or(6);
        format!("{:.prec$}", f, prec = prec)
    } else {
        format!("{:.6}", f)
    }
}

pub fn apply_string_format_spec(s: &str, spec: &str) -> String {
    if spec.is_empty() { return s.to_string(); }
    let chars: Vec<char> = spec.chars().collect();
    let mut i = 0;
    let mut fill = ' ';
    let mut align = None;
    // Check for fill+align
    if chars.len() >= 2 && "<>^=".contains(chars[1]) {
        fill = chars[0];
        align = Some(chars[1]);
        i = 2;
    } else if !chars.is_empty() && "<>^".contains(chars[0]) {
        align = Some(chars[0]);
        i = 1;
    }
    // Check for sign
    if i < chars.len() && "+-".contains(chars[i]) {
        i += 1;
    }
    // Check for 0 fill (only when no explicit fill+align given)
    if i < chars.len() && chars[i] == '0' && align.is_none() {
        fill = '0';
        align = Some('>');
        i += 1;
    }
    // Parse width
    let width_str: String = chars[i..].iter().take_while(|c| c.is_ascii_digit()).collect();
    let width: usize = width_str.parse().unwrap_or(0);
    if width <= s.len() { return s.to_string(); }
    let pad_len = width - s.len();
    match align.unwrap_or('>') {
        '<' => format!("{}{}", s, std::iter::repeat(fill).take(pad_len).collect::<String>()),
        '>' | '=' => format!("{}{}", std::iter::repeat(fill).take(pad_len).collect::<String>(), s),
        '^' => {
            let left = pad_len / 2;
            let right = pad_len - left;
            format!("{}{}{}", std::iter::repeat(fill).take(left).collect::<String>(), s, std::iter::repeat(fill).take(right).collect::<String>())
        }
        _ => s.to_string(),
    }
}

/// Resolve slice start/stop/step into actual indices for a sequence of given length.
fn resolve_slice(
    start: &Option<PyObjectRef>,
    stop: &Option<PyObjectRef>,
    step: &Option<PyObjectRef>,
    len: i64,
) -> (i64, i64, i64) {
    let step_val = step.as_ref()
        .and_then(|s| if matches!(s.payload, PyObjectPayload::None) { None } else { Some(s) })
        .and_then(|s| s.as_int())
        .unwrap_or(1);

    let (default_start, default_stop) = if step_val < 0 { (len - 1, -len - 1) } else { (0, len) };

    let start_val = start.as_ref()
        .and_then(|s| if matches!(s.payload, PyObjectPayload::None) { None } else { Some(s) })
        .and_then(|s| s.as_int())
        .map(|i| {
            if i < 0 { (len + i).max(if step_val < 0 { -1 } else { 0 }) }
            else { i.min(len) }
        })
        .unwrap_or(default_start);

    let stop_val = stop.as_ref()
        .and_then(|s| if matches!(s.payload, PyObjectPayload::None) { None } else { Some(s) })
        .and_then(|s| s.as_int())
        .map(|i| {
            if i < 0 { (len + i).max(if step_val < 0 { -1 } else { 0 }) }
            else { i.min(len) }
        })
        .unwrap_or(default_stop);

    (start_val, stop_val, step_val)
}

fn get_slice_impl(
    obj: &PyObjectRef,
    start: &Option<PyObjectRef>,
    stop: &Option<PyObjectRef>,
    step: &Option<PyObjectRef>,
) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::List(items) => {
            let items = items.read();
            let len = items.len() as i64;
            let (s, e, step) = resolve_slice(start, stop, step, len);
            let mut result = Vec::new();
            let mut i = s;
            if step > 0 {
                while i < e && i < len { result.push(items[i as usize].clone()); i += step; }
            } else if step < 0 {
                while i > e && i >= 0 { result.push(items[i as usize].clone()); i += step; }
            }
            Ok(PyObject::list(result))
        }
        PyObjectPayload::Tuple(items) => {
            let len = items.len() as i64;
            let (s, e, step) = resolve_slice(start, stop, step, len);
            let mut result = Vec::new();
            let mut i = s;
            if step > 0 {
                while i < e && i < len { result.push(items[i as usize].clone()); i += step; }
            } else if step < 0 {
                while i > e && i >= 0 { result.push(items[i as usize].clone()); i += step; }
            }
            Ok(PyObject::tuple(result))
        }
        PyObjectPayload::Str(s) => {
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as i64;
            let (sv, ev, step) = resolve_slice(start, stop, step, len);
            let mut result = String::new();
            let mut i = sv;
            if step > 0 {
                while i < ev && i < len { result.push(chars[i as usize]); i += step; }
            } else if step < 0 {
                while i > ev && i >= 0 { result.push(chars[i as usize]); i += step; }
            }
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
            let len = b.len() as i64;
            let (sv, ev, step) = resolve_slice(start, stop, step, len);
            let mut result = Vec::new();
            let mut i = sv;
            if step > 0 {
                while i < ev && i < len { result.push(b[i as usize]); i += step; }
            } else if step < 0 {
                while i > ev && i >= 0 { result.push(b[i as usize]); i += step; }
            }
            Ok(PyObject::bytes(result))
        }
        _ => Err(PyException::type_error(format!("'{}' object is not subscriptable", obj.type_name()))),
    }
}

fn format_collection(open: &str, close: &str, items: &[PyObjectRef]) -> String {
    let inner: Vec<String> = items.iter().map(|i| i.repr()).collect();
    format!("{}{}{}", open, inner.join(", "), close)
}

fn format_set(open: &str, close: &str, map: &IndexMap<HashableKey, PyObjectRef>) -> String {
    let inner: Vec<String> = map.values().map(|v| v.repr()).collect();
    format!("{}{}{}", open, inner.join(", "), close)
}

fn format_dict(map: &IndexMap<HashableKey, PyObjectRef>) -> String {
    let inner: Vec<String> = map.iter().map(|(k, v)| format!("{}: {}", k.to_object().repr(), v.repr())).collect();
    format!("{{{}}}", inner.join(", "))
}
