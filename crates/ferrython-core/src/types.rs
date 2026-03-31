//! Auxiliary Python types: `PyInt`, `PyFunction`, `HashableKey`.

use crate::error::{PyException, PyResult};
use crate::object::{PyObject, PyObjectMethods, PyObjectRef};
use compact_str::CompactString;
use ferrython_bytecode::CodeObject;
use indexmap::IndexMap;
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use parking_lot::RwLock;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Shared globals dictionary — all functions defined in the same module share
/// one instance so that `global` mutations are visible across calls.
pub type SharedGlobals = Arc<RwLock<IndexMap<CompactString, PyObjectRef>>>;

// ── PyInt ──

#[derive(Debug, Clone)]
pub enum PyInt {
    Small(i64),
    Big(Box<BigInt>),
}

impl PyInt {
    pub fn is_zero(&self) -> bool {
        match self { PyInt::Small(n) => *n == 0, PyInt::Big(n) => n.is_zero() }
    }
    pub fn to_i64(&self) -> Option<i64> {
        match self { PyInt::Small(n) => Some(*n), PyInt::Big(n) => n.to_i64() }
    }
    pub fn to_f64(&self) -> f64 {
        match self { PyInt::Small(n) => *n as f64, PyInt::Big(n) => n.to_f64().unwrap_or(f64::INFINITY) }
    }
    pub fn to_object(&self) -> PyObjectRef {
        match self { PyInt::Small(n) => PyObject::int(*n), PyInt::Big(n) => PyObject::big_int(n.as_ref().clone()) }
    }
    fn to_bigint(&self) -> BigInt {
        match self { PyInt::Small(n) => BigInt::from(*n), PyInt::Big(n) => n.as_ref().clone() }
    }

    pub fn add_op(a: &PyInt, b: &PyInt) -> PyInt {
        match (a, b) {
            (PyInt::Small(a), PyInt::Small(b)) => match a.checked_add(*b) {
                Some(r) => PyInt::Small(r),
                None => PyInt::Big(Box::new(BigInt::from(*a) + BigInt::from(*b))),
            },
            _ => PyInt::Big(Box::new(a.to_bigint() + b.to_bigint())),
        }
    }
    pub fn sub_op(a: &PyInt, b: &PyInt) -> PyInt {
        match (a, b) {
            (PyInt::Small(a), PyInt::Small(b)) => match a.checked_sub(*b) {
                Some(r) => PyInt::Small(r),
                None => PyInt::Big(Box::new(BigInt::from(*a) - BigInt::from(*b))),
            },
            _ => PyInt::Big(Box::new(a.to_bigint() - b.to_bigint())),
        }
    }
    pub fn mul_op(a: &PyInt, b: &PyInt) -> PyInt {
        match (a, b) {
            (PyInt::Small(a), PyInt::Small(b)) => match a.checked_mul(*b) {
                Some(r) => PyInt::Small(r),
                None => PyInt::Big(Box::new(BigInt::from(*a) * BigInt::from(*b))),
            },
            _ => PyInt::Big(Box::new(a.to_bigint() * b.to_bigint())),
        }
    }
    pub fn floor_div_op(a: &PyInt, b: &PyInt) -> PyInt {
        match (a, b) {
            (PyInt::Small(a), PyInt::Small(b)) => {
                let (q, r) = (*a / *b, *a % *b);
                if (r != 0) && (r ^ *b) < 0 { PyInt::Small(q - 1) } else { PyInt::Small(q) }
            }
            _ => {
                let ba = a.to_bigint(); let bb = b.to_bigint();
                use num_integer::Integer;
                PyInt::Big(Box::new(ba.div_floor(&bb)))
            }
        }
    }
    pub fn modulo_op(a: &PyInt, b: &PyInt) -> PyInt {
        match (a, b) {
            (PyInt::Small(a), PyInt::Small(b)) => {
                let r = *a % *b;
                if (r != 0) && (r ^ *b) < 0 { PyInt::Small(r + *b) } else { PyInt::Small(r) }
            }
            _ => {
                let ba = a.to_bigint(); let bb = b.to_bigint();
                use num_integer::Integer;
                PyInt::Big(Box::new(ba.mod_floor(&bb)))
            }
        }
    }
    pub fn pow_op(base: &PyInt, exp: u32) -> PyInt {
        match base {
            PyInt::Small(b) => match b.checked_pow(exp) {
                Some(r) => PyInt::Small(r),
                None => PyInt::Big(Box::new(BigInt::from(*b).pow(exp))),
            },
            PyInt::Big(b) => PyInt::Big(Box::new(b.as_ref().pow(exp))),
        }
    }
    pub fn negate(&self) -> PyInt {
        match self {
            PyInt::Small(n) => match n.checked_neg() {
                Some(r) => PyInt::Small(r),
                None => PyInt::Big(Box::new(-BigInt::from(*n))),
            },
            PyInt::Big(n) => PyInt::Big(Box::new(-n.as_ref())),
        }
    }
    pub fn invert(&self) -> PyInt {
        match self {
            PyInt::Small(n) => PyInt::Small(!n),
            PyInt::Big(n) => PyInt::Big(Box::new(-(n.as_ref() + BigInt::from(1)))),
        }
    }
    pub fn abs(&self) -> PyInt {
        match self {
            PyInt::Small(n) => match n.checked_abs() {
                Some(r) => PyInt::Small(r),
                None => PyInt::Big(Box::new(BigInt::from(*n).magnitude().clone().into())),
            },
            PyInt::Big(n) => PyInt::Big(Box::new(n.as_ref().magnitude().clone().into())),
        }
    }
}

impl std::fmt::Display for PyInt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { PyInt::Small(n) => write!(f, "{}", n), PyInt::Big(n) => write!(f, "{}", n) }
    }
}
impl PartialEq for PyInt {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (PyInt::Small(a), PyInt::Small(b)) => a == b,
            _ => self.to_bigint() == other.to_bigint(),
        }
    }
}
impl Eq for PyInt {}
impl PartialOrd for PyInt {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}
impl Ord for PyInt {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (PyInt::Small(a), PyInt::Small(b)) => a.cmp(b),
            _ => self.to_bigint().cmp(&other.to_bigint()),
        }
    }
}
impl Hash for PyInt {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            PyInt::Small(n) => n.hash(state),
            PyInt::Big(n) => {
                if let Some(small) = n.to_i64() { small.hash(state); }
                else { let (sign, digits) = n.to_bytes_le(); sign.hash(state); digits.hash(state); }
            }
        }
    }
}

// ── PyFunction ──

#[derive(Debug, Clone)]
pub struct PyFunction {
    pub name: CompactString,
    pub qualname: CompactString,
    pub code: CodeObject,
    pub defaults: Vec<PyObjectRef>,
    pub kw_defaults: IndexMap<CompactString, PyObjectRef>,
    pub globals: SharedGlobals,
    /// Closure cells: Vec of shared cell references from enclosing scope.
    pub closure: Vec<Arc<RwLock<Option<PyObjectRef>>>>,
    pub annotations: IndexMap<CompactString, PyObjectRef>,
}

impl PyFunction {
    pub fn new(name: CompactString, code: CodeObject) -> Self {
        Self {
            qualname: name.clone(), name, code,
            defaults: Vec::new(), kw_defaults: IndexMap::new(),
            globals: Arc::new(RwLock::new(IndexMap::new())),
            closure: Vec::new(), annotations: IndexMap::new(),
        }
    }
}

// ── HashableKey ──

#[derive(Debug, Clone, Eq)]
pub enum HashableKey {
    None,
    Bool(bool),
    Int(PyInt),
    Float(OrderedFloat),
    Str(CompactString),
    Bytes(Vec<u8>),
    Tuple(Vec<HashableKey>),
}

impl HashableKey {
    pub fn from_object(obj: &PyObjectRef) -> PyResult<Self> {
        use crate::object::PyObjectPayload;
        match &obj.payload {
            PyObjectPayload::None => Ok(HashableKey::None),
            PyObjectPayload::Bool(b) => Ok(HashableKey::Bool(*b)),
            PyObjectPayload::Int(n) => Ok(HashableKey::Int(n.clone())),
            PyObjectPayload::Float(f) => Ok(HashableKey::Float(OrderedFloat(*f))),
            PyObjectPayload::Str(s) => Ok(HashableKey::Str(s.clone())),
            PyObjectPayload::Bytes(b) => Ok(HashableKey::Bytes(b.clone())),
            PyObjectPayload::Tuple(items) => {
                let mut keys = Vec::with_capacity(items.len());
                for item in items { keys.push(HashableKey::from_object(item)?); }
                Ok(HashableKey::Tuple(keys))
            }
            PyObjectPayload::Ellipsis => Ok(HashableKey::Str(CompactString::from("Ellipsis"))),
            _ => Err(PyException::type_error(format!("unhashable type: '{}'", obj.type_name()))),
        }
    }
    pub fn to_object(&self) -> PyObjectRef {
        match self {
            HashableKey::None => PyObject::none(),
            HashableKey::Bool(b) => PyObject::bool_val(*b),
            HashableKey::Int(n) => n.to_object(),
            HashableKey::Float(f) => PyObject::float(f.0),
            HashableKey::Str(s) => PyObject::str_val(s.clone()),
            HashableKey::Bytes(b) => PyObject::bytes(b.clone()),
            HashableKey::Tuple(keys) => PyObject::tuple(keys.iter().map(|k| k.to_object()).collect()),
        }
    }
}

impl PartialEq for HashableKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (HashableKey::None, HashableKey::None) => true,
            (HashableKey::Bool(a), HashableKey::Bool(b)) => a == b,
            (HashableKey::Int(a), HashableKey::Int(b)) => a == b,
            (HashableKey::Float(a), HashableKey::Float(b)) => a == b,
            (HashableKey::Str(a), HashableKey::Str(b)) => a == b,
            (HashableKey::Bytes(a), HashableKey::Bytes(b)) => a == b,
            (HashableKey::Tuple(a), HashableKey::Tuple(b)) => a == b,
            (HashableKey::Bool(b), HashableKey::Int(n)) | (HashableKey::Int(n), HashableKey::Bool(b)) => {
                *n == PyInt::Small(*b as i64)
            }
            _ => false,
        }
    }
}

impl Hash for HashableKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            HashableKey::None => {},
            HashableKey::Bool(b) => b.hash(state),
            HashableKey::Int(n) => n.hash(state),
            HashableKey::Float(f) => f.hash(state),
            HashableKey::Str(s) => s.hash(state),
            HashableKey::Bytes(b) => b.hash(state),
            HashableKey::Tuple(items) => items.hash(state),
        }
    }
}

// ── OrderedFloat ──

#[derive(Debug, Clone, Copy)]
pub struct OrderedFloat(pub f64);
impl PartialEq for OrderedFloat { fn eq(&self, other: &Self) -> bool { self.0.to_bits() == other.0.to_bits() } }
impl Eq for OrderedFloat {}
impl Hash for OrderedFloat { fn hash<H: Hasher>(&self, state: &mut H) { self.0.to_bits().hash(state); } }
impl PartialOrd for OrderedFloat { fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) } }
impl Ord for OrderedFloat { fn cmp(&self, other: &Self) -> std::cmp::Ordering { self.0.total_cmp(&other.0) } }
