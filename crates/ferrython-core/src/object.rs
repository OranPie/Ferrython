//! The Python object model — `PyObject`, `PyObjectRef`, `PyObjectPayload`.

use crate::error::{PyException, PyResult};
use crate::types::{HashableKey, PyFunction, PyInt};
use compact_str::CompactString;
use indexmap::IndexMap;
use num_bigint::BigInt;
use parking_lot::RwLock;
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
    Set(IndexMap<HashableKey, PyObjectRef>),
    FrozenSet(IndexMap<HashableKey, PyObjectRef>),
    Dict(IndexMap<HashableKey, PyObjectRef>),
    Function(PyFunction),
    BuiltinFunction(CompactString),
    BoundMethod { receiver: PyObjectRef, method: PyObjectRef },
    BuiltinBoundMethod { receiver: PyObjectRef, method_name: CompactString },
    Code(Box<ferrython_bytecode::CodeObject>),
    Class(ClassData),
    Instance(InstanceData),
    Module(ModuleData),
    Iterator(IteratorData),
    Slice { start: Option<PyObjectRef>, stop: Option<PyObjectRef>, step: Option<PyObjectRef> },
}

#[derive(Debug, Clone)]
pub struct ClassData {
    pub name: CompactString,
    pub bases: Vec<PyObjectRef>,
    pub namespace: IndexMap<CompactString, PyObjectRef>,
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
    pub fn set(items: IndexMap<HashableKey, PyObjectRef>) -> PyObjectRef { Self::wrap(PyObjectPayload::Set(items)) }
    pub fn dict(items: IndexMap<HashableKey, PyObjectRef>) -> PyObjectRef { Self::wrap(PyObjectPayload::Dict(items)) }
    pub fn function(func: PyFunction) -> PyObjectRef { Self::wrap(PyObjectPayload::Function(func)) }
    pub fn builtin_function(name: CompactString) -> PyObjectRef { Self::wrap(PyObjectPayload::BuiltinFunction(name)) }
    pub fn code(code: ferrython_bytecode::CodeObject) -> PyObjectRef { Self::wrap(PyObjectPayload::Code(Box::new(code))) }
    pub fn class(name: CompactString, bases: Vec<PyObjectRef>, namespace: IndexMap<CompactString, PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Class(ClassData { name, bases, namespace, mro: Vec::new() }))
    }
    pub fn instance(class: PyObjectRef) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Instance(InstanceData { class, attrs: Arc::new(RwLock::new(IndexMap::new())) }))
    }
    pub fn module(name: CompactString) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Module(ModuleData { name, attrs: IndexMap::new() }))
    }
    pub fn slice(start: Option<PyObjectRef>, stop: Option<PyObjectRef>, step: Option<PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Slice { start, stop, step })
    }
    pub fn frozenset(items: IndexMap<HashableKey, PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::FrozenSet(items))
    }
    pub fn range(start: i64, stop: i64, step: i64) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Iterator(IteratorData::Range { current: start, stop, step }))
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
            PyObjectPayload::Set(m) | PyObjectPayload::FrozenSet(m) | PyObjectPayload::Dict(m) => !m.is_empty(),
            _ => true,
        }
    }

    fn is_callable(&self) -> bool {
        matches!(&self.payload, PyObjectPayload::Function(_) | PyObjectPayload::BuiltinFunction(_)
            | PyObjectPayload::BoundMethod { .. } | PyObjectPayload::Class(_))
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
            PyObjectPayload::Set(m) if m.is_empty() => "set()".into(),
            PyObjectPayload::Set(m) => format_set("{", "}", m),
            PyObjectPayload::Dict(m) => format_dict(m),
            PyObjectPayload::Ellipsis => "Ellipsis".into(),
            PyObjectPayload::NotImplemented => "NotImplemented".into(),
            PyObjectPayload::Function(f) => format!("<function {}>", f.name),
            PyObjectPayload::BuiltinFunction(n) => format!("<built-in function {}>", n),
            PyObjectPayload::Code(c) => format!("<code object {}>", c.name),
            PyObjectPayload::Class(cd) => format!("<class '{}'>", cd.name),
            PyObjectPayload::Instance(inst) => {
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    format!("<{} object>", cd.name)
                } else { "<object>".into() }
            }
            PyObjectPayload::Module(m) => format!("<module '{}'>", m.name),
            PyObjectPayload::Iterator(_) => "<iterator>".into(),
            _ => format!("<{}>", self.type_name()),
        }
    }

    fn repr(&self) -> String {
        match &self.payload {
            PyObjectPayload::Str(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
            _ => self.py_to_string(),
        }
    }

    fn to_list(&self) -> PyResult<Vec<PyObjectRef>> {
        match &self.payload {
            PyObjectPayload::List(v) => Ok(v.read().clone()),
            PyObjectPayload::Tuple(v) => Ok(v.clone()),
            PyObjectPayload::Set(m) | PyObjectPayload::FrozenSet(m) => Ok(m.values().cloned().collect()),
            PyObjectPayload::Str(s) => Ok(s.chars().map(|c| PyObject::str_val(CompactString::from(c.to_string()))).collect()),
            PyObjectPayload::Dict(m) => Ok(m.keys().map(|k| k.to_object()).collect()),
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
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for -: '{}' and '{}'", self.type_name(), other.type_name()))),
        }
    }

    fn mul(&self, other: &Self) -> PyResult<PyObjectRef> {
        match (&self.payload, &other.payload) {
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => Ok(PyInt::mul_op(a, b).to_object()),
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a * b)),
            (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.to_f64() * b)),
            (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => Ok(PyObject::float(a * b.to_f64())),
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
    fn bit_and(&self, other: &Self) -> PyResult<PyObjectRef> { int_bitop(self, other, "&", |a, b| a & b) }
    fn bit_or(&self, other: &Self) -> PyResult<PyObjectRef> { int_bitop(self, other, "|", |a, b| a | b) }
    fn bit_xor(&self, other: &Self) -> PyResult<PyObjectRef> { int_bitop(self, other, "^", |a, b| a ^ b) }

    fn negate(&self) -> PyResult<PyObjectRef> {
        match &self.payload {
            PyObjectPayload::Int(n) => Ok(n.negate().to_object()),
            PyObjectPayload::Float(f) => Ok(PyObject::float(-f)),
            PyObjectPayload::Bool(b) => Ok(PyObject::int(-(*b as i64))),
            _ => Err(PyException::type_error(format!("bad operand type for unary -: '{}'", self.type_name()))),
        }
    }

    fn positive(&self) -> PyResult<PyObjectRef> {
        match &self.payload {
            PyObjectPayload::Int(_) | PyObjectPayload::Float(_) | PyObjectPayload::Bool(_) => Ok(self.clone()),
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
            _ => Err(PyException::type_error(format!("bad operand type for abs(): '{}'", self.type_name()))),
        }
    }

    fn compare(&self, other: &Self, op: CompareOp) -> PyResult<PyObjectRef> {
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
                // Instance attributes take priority
                if let Some(v) = inst.attrs.read().get(name) { return Some(v.clone()); }
                // Then check class namespace
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    if let Some(v) = cd.namespace.get(name) {
                        // If it's a function, return a BoundMethod (auto-pass self)
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
                }
                None
            }
            PyObjectPayload::Class(cd) => cd.namespace.get(name).cloned(),
            PyObjectPayload::Module(m) => m.attrs.get(name).cloned(),
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
            _ => None,
        }
    }

    fn py_len(&self) -> PyResult<usize> {
        match &self.payload {
            PyObjectPayload::Str(s) => Ok(s.chars().count()),
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Ok(b.len()),
            PyObjectPayload::List(v) => Ok(v.read().len()),
            PyObjectPayload::Tuple(v) => Ok(v.len()),
            PyObjectPayload::Set(m) | PyObjectPayload::FrozenSet(m) | PyObjectPayload::Dict(m) => Ok(m.len()),
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
                map.get(&hk).cloned().ok_or_else(|| PyException::key_error(key.repr()))
            }
            PyObjectPayload::Str(s) => {
                let idx = key.to_int()?;
                let chars: Vec<char> = s.chars().collect();
                let len = chars.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len { return Err(PyException::index_error("string index out of range")); }
                Ok(PyObject::str_val(CompactString::from(chars[actual as usize].to_string())))
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
            PyObjectPayload::Set(m) | PyObjectPayload::FrozenSet(m) | PyObjectPayload::Dict(m) => {
                let hk = item.to_hashable_key()?;
                Ok(m.contains_key(&hk))
            }
            _ => Err(PyException::type_error(format!("argument of type '{}' is not iterable", self.type_name()))),
        }
    }

    fn get_iter(&self) -> PyResult<PyObjectRef> {
        match &self.payload {
            PyObjectPayload::List(items) => Ok(PyObject::wrap(PyObjectPayload::Iterator(IteratorData::List { items: items.read().clone(), index: 0 }))),
            PyObjectPayload::Tuple(items) => Ok(PyObject::wrap(PyObjectPayload::Iterator(IteratorData::Tuple { items: items.clone(), index: 0 }))),
            PyObjectPayload::Str(s) => Ok(PyObject::wrap(PyObjectPayload::Iterator(IteratorData::Str { chars: s.chars().collect(), index: 0 }))),
            PyObjectPayload::Dict(m) => {
                let keys: Vec<PyObjectRef> = m.keys().map(|k| k.to_object()).collect();
                Ok(PyObject::wrap(PyObjectPayload::Iterator(IteratorData::List { items: keys, index: 0 })))
            }
            PyObjectPayload::Set(m) | PyObjectPayload::FrozenSet(m) => {
                let vals: Vec<PyObjectRef> = m.values().cloned().collect();
                Ok(PyObject::wrap(PyObjectPayload::Iterator(IteratorData::List { items: vals, index: 0 })))
            }
            PyObjectPayload::Iterator(_) => Ok(self.clone()),
            _ => Err(PyException::type_error(format!("'{}' object is not iterable", self.type_name()))),
        }
    }

    fn format_value(&self, _spec: &str) -> PyResult<String> { Ok(self.py_to_string()) }

    fn dir(&self) -> Vec<CompactString> {
        match &self.payload {
            PyObjectPayload::Instance(inst) => {
                let mut names: Vec<CompactString> = inst.attrs.read().keys().cloned().collect();
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    names.extend(cd.namespace.keys().cloned());
                }
                names.sort(); names.dedup(); names
            }
            PyObjectPayload::Class(cd) => { let mut n: Vec<_> = cd.namespace.keys().cloned().collect(); n.sort(); n }
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
