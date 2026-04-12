//! Auxiliary Python types: `PyInt`, `PyFunction`, `HashableKey`.

use std::rc::Rc;
use crate::error::{PyException, PyResult};
use crate::object::{PyObject, PyObjectMethods, PyObjectRef, PyCell};
use compact_str::CompactString;
use ferrython_bytecode::CodeObject;
use ferrython_bytecode::code::CodeFlags;
use indexmap::IndexMap;
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use parking_lot::RwLock;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::cell::RefCell;

/// Thread-local dispatch for calling Python __eq__ from PartialEq on HashableKey.
/// The VM sets this before any dict/set operation that may compare Custom keys.
type EqDispatchFn = Box<dyn FnMut(&PyObjectRef, &PyObjectRef) -> Option<bool>>;
type HashDispatchFn = Box<dyn FnMut(&PyObjectRef) -> Option<i64>>;

thread_local! {
    static EQ_DISPATCH: RefCell<Option<EqDispatchFn>> = RefCell::new(None);
    static HASH_DISPATCH: RefCell<Option<HashDispatchFn>> = RefCell::new(None);
}

/// Install an __eq__ dispatch callback (called by VM before dict/set ops).
pub fn set_eq_dispatch<F: FnMut(&PyObjectRef, &PyObjectRef) -> Option<bool> + 'static>(f: F) {
    EQ_DISPATCH.with(|cell| {
        *cell.borrow_mut() = Some(Box::new(f));
    });
}

/// Install a __hash__ dispatch callback (called from HashableKey::from_object for Instance).
pub fn set_hash_dispatch<F: FnMut(&PyObjectRef) -> Option<i64> + 'static>(f: F) {
    HASH_DISPATCH.with(|cell| {
        *cell.borrow_mut() = Some(Box::new(f));
    });
}

/// Call the installed __eq__ dispatch, if any.
fn call_eq_dispatch(a: &PyObjectRef, b: &PyObjectRef) -> Option<bool> {
    EQ_DISPATCH.with(|cell| {
        // Take the closure out to avoid re-entrant borrow panic
        let func = cell.borrow_mut().take();
        if let Some(mut f) = func {
            let result = f(a, b);
            *cell.borrow_mut() = Some(f);
            result
        } else {
            None
        }
    })
}

/// Call the installed __hash__ dispatch, if any.
fn call_hash_dispatch(obj: &PyObjectRef) -> Option<i64> {
    HASH_DISPATCH.with(|cell| {
        // Take the closure out to avoid re-entrant borrow panic
        let func = cell.borrow_mut().take();
        if let Some(mut f) = func {
            let result = f(obj);
            *cell.borrow_mut() = Some(f);
            result
        } else {
            None
        }
    })
}

use crate::object::FxAttrMap;

/// Shared globals dictionary — all functions defined in the same module share
/// one instance so that `global` mutations are visible across calls.
pub type SharedGlobals = Rc<PyCell<FxAttrMap>>;

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

/// Pre-built constant cache shared across all frames using the same code object.
pub type SharedConstantCache = Arc<Vec<PyObjectRef>>;

#[derive(Clone)]
pub struct PyFunction {
    pub name: CompactString,
    pub qualname: CompactString,
    pub code: Arc<CodeObject>,
    /// Pre-built constant cache — built once at function creation, shared with all frames.
    pub constant_cache: SharedConstantCache,
    pub defaults: Vec<PyObjectRef>,
    pub kw_defaults: IndexMap<CompactString, PyObjectRef>,
    pub globals: SharedGlobals,
    /// Closure cells: Vec of shared cell references from enclosing scope.
    pub closure: Vec<Rc<PyCell<Option<PyObjectRef>>>>,
    pub annotations: IndexMap<CompactString, PyObjectRef>,
    /// User-settable attributes (e.g., __name__, __doc__, __wrapped__)
    pub attrs: Rc<PyCell<FxAttrMap>>,
    /// Cached: true if function can use the fast inline CallFunction path
    /// (exact positional args, no *args/**kwargs/generators/closures/cells)
    pub is_simple: bool,
}

impl PyFunction {
    /// Check if function supports fast inline CallFunction
    /// (exact positional args, no *args/**kwargs/generators/closures/cells).
    #[inline]
    fn compute_is_simple(code: &CodeObject, closure: &[Rc<PyCell<Option<PyObjectRef>>>]) -> bool {
        code.kwonlyarg_count == 0
            && !code.flags.contains(CodeFlags::VARARGS)
            && !code.flags.contains(CodeFlags::VARKEYWORDS)
            && !code.flags.contains(CodeFlags::GENERATOR)
            && !code.flags.contains(CodeFlags::COROUTINE)
            && closure.is_empty()
            && code.cellvars.is_empty()
            && code.freevars.is_empty()
    }

    /// Public static version for external construction sites.
    #[inline]
    pub fn compute_is_simple_static(code: &CodeObject, closure: &[Rc<PyCell<Option<PyObjectRef>>>]) -> bool {
        Self::compute_is_simple(code, closure)
    }

    pub fn new(name: CompactString, code: CodeObject) -> Self {
        let code = Arc::new(code);
        let constant_cache = Arc::new(Self::build_constant_cache(&code));
        let is_simple = Self::compute_is_simple(&code, &[]);
        Self {
            qualname: name.clone(), name, code, constant_cache,
            defaults: Vec::new(), kw_defaults: IndexMap::new(),
            globals: Rc::new(PyCell::new(FxAttrMap::default())),
            closure: Vec::new(), annotations: IndexMap::new(),
            attrs: Rc::new(PyCell::new(FxAttrMap::default())),
            is_simple,
        }
    }

    /// Build from a pre-existing Arc<CodeObject> (avoids double-wrapping).
    pub fn with_arc_code(name: CompactString, code: Arc<CodeObject>) -> Self {
        let constant_cache = Arc::new(Self::build_constant_cache(&code));
        let is_simple = Self::compute_is_simple(&code, &[]);
        Self {
            qualname: name.clone(), name, code, constant_cache,
            defaults: Vec::new(), kw_defaults: IndexMap::new(),
            globals: Rc::new(PyCell::new(FxAttrMap::default())),
            closure: Vec::new(), annotations: IndexMap::new(),
            attrs: Rc::new(PyCell::new(FxAttrMap::default())),
            is_simple,
        }
    }

    /// Pre-convert all constants to PyObjectRef once.
    pub fn build_constant_cache(code: &CodeObject) -> Vec<PyObjectRef> {
        use ferrython_bytecode::code::ConstantValue;
        use crate::object::PyObjectPayload;
        fn convert(c: &ConstantValue) -> PyObjectRef {
            match c {
                ConstantValue::None => PyObject::none(),
                ConstantValue::Bool(b) => PyObject::bool_val(*b),
                ConstantValue::Integer(n) => PyObject::int(*n),
                ConstantValue::BigInteger(n) => PyObject::big_int(n.as_ref().clone()),
                ConstantValue::Float(f) => PyObject::float(*f),
                ConstantValue::Complex { real, imag } => PyObject::complex(*real, *imag),
                ConstantValue::Str(s) => PyObject::str_val(s.clone()),
                ConstantValue::Bytes(b) => PyObject::bytes(b.clone()),
                ConstantValue::Ellipsis => PyObject::ellipsis(),
                ConstantValue::Code(co) => PyObject::wrap(PyObjectPayload::Code(Arc::clone(co))),
                ConstantValue::Tuple(items) => {
                    PyObject::tuple(items.iter().map(|i| convert(i)).collect())
                }
                ConstantValue::FrozenSet(items) => {
                    let mut set = indexmap::IndexMap::new();
                    for item in items {
                        let obj = convert(item);
                        if let Ok(key) = obj.to_hashable_key() {
                            set.insert(key, obj);
                        }
                    }
                    PyObject::set(set)
                }
            }
        }
        code.constants.iter().map(|c| convert(c)).collect()
    }
}

// ── HashableKey ──

#[derive(Debug, Clone)]
pub enum HashableKey {
    None,
    Bool(bool),
    Int(PyInt),
    Float(OrderedFloat),
    Str(CompactString),
    Bytes(Vec<u8>),
    Tuple(Vec<HashableKey>),
    FrozenSet(Vec<HashableKey>),
    /// Identity-based key using the Arc pointer address, preserving the original object.
    Identity(usize, PyObjectRef),
    /// Custom hashable key for objects with __hash__/__eq__.
    Custom {
        hash_value: i64,
        object: PyObjectRef,
    },
}

impl Eq for HashableKey {}

impl HashableKey {
    pub fn from_object(obj: &PyObjectRef) -> PyResult<Self> {
        use crate::object::PyObjectPayload;
        match &obj.payload {
            PyObjectPayload::None => Ok(HashableKey::None),
            PyObjectPayload::Bool(b) => Ok(HashableKey::Int(PyInt::Small(*b as i64))),
            PyObjectPayload::Int(n) => Ok(HashableKey::Int(n.clone())),
            PyObjectPayload::Float(f) => Ok(HashableKey::Float(OrderedFloat(*f))),
            PyObjectPayload::Str(s) => Ok(HashableKey::Str(s.clone())),
            PyObjectPayload::Bytes(b) => Ok(HashableKey::Bytes(b.clone())),
            PyObjectPayload::Tuple(items) => {
                let mut keys = Vec::with_capacity(items.len());
                for item in items { keys.push(HashableKey::from_object(item)?); }
                Ok(HashableKey::Tuple(keys))
            }
            PyObjectPayload::FrozenSet(m) => {
                let mut keys: Vec<HashableKey> = Vec::with_capacity(m.len());
                for (k, _) in m.iter() { keys.push(k.clone()); }
                keys.sort_by(|a, b| format!("{:?}", a).cmp(&format!("{:?}", b)));
                Ok(HashableKey::FrozenSet(keys))
            }
            PyObjectPayload::Ellipsis => Ok(HashableKey::Str(CompactString::from("Ellipsis"))),
            // Instance objects: use __hash__ if available via dispatch, else identity
            PyObjectPayload::Instance(_) => {
                if let Some(hash_val) = call_hash_dispatch(obj) {
                    Ok(HashableKey::Custom {
                        hash_value: hash_val,
                        object: obj.clone(),
                    })
                } else {
                    let ptr = PyObjectRef::as_ptr(obj) as usize;
                    Ok(HashableKey::Identity(ptr, obj.clone()))
                }
            }
            // Functions/methods are hashable by identity in CPython
            PyObjectPayload::Function(_) | PyObjectPayload::NativeFunction { .. } |
            PyObjectPayload::NativeClosure(_) | PyObjectPayload::BuiltinFunction(_) |
            PyObjectPayload::BoundMethod { .. } | PyObjectPayload::BuiltinBoundMethod { .. } => {
                let ptr = PyObjectRef::as_ptr(obj) as usize;
                Ok(HashableKey::Identity(ptr, obj.clone()))
            }
            // Class objects: hash by identity (each class definition is unique)
            PyObjectPayload::Class(_) => {
                let ptr = PyObjectRef::as_ptr(obj) as usize;
                Ok(HashableKey::Identity(ptr, obj.clone()))
            }
            // BuiltinType: hash by type name so type(42) matches int as dict key
            PyObjectPayload::BuiltinType(name) => {
                Ok(HashableKey::Str(CompactString::from(format!("<type:{}>", name))))
            }
            // Module objects: hashable if they have __hash__ (e.g. re.Pattern objects),
            // otherwise use identity (CPython modules are hashable by identity)
            PyObjectPayload::Module(_) => {
                if let Some(hash_val) = call_hash_dispatch(obj) {
                    Ok(HashableKey::Custom {
                        hash_value: hash_val,
                        object: obj.clone(),
                    })
                } else {
                    let ptr = PyObjectRef::as_ptr(obj) as usize;
                    Ok(HashableKey::Identity(ptr, obj.clone()))
                }
            }
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
            HashableKey::FrozenSet(keys) => {
                let mut map = indexmap::IndexMap::new();
                for k in keys { map.insert(k.clone(), k.to_object()); }
                PyObject::frozenset(map)
            },
            HashableKey::Identity(_ptr, obj) => {
                obj.clone()
            },
            HashableKey::Custom { object, .. } => object.clone(),
        }
    }
}

impl PartialEq for HashableKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (HashableKey::None, HashableKey::None) => true,
            // Bool/Bool
            (HashableKey::Bool(a), HashableKey::Bool(b)) => a == b,
            // Int/Int
            (HashableKey::Int(a), HashableKey::Int(b)) => a == b,
            // Float/Float
            (HashableKey::Float(a), HashableKey::Float(b)) => a == b,
            // Str/Str
            (HashableKey::Str(a), HashableKey::Str(b)) => a == b,
            // Bytes/Bytes
            (HashableKey::Bytes(a), HashableKey::Bytes(b)) => a == b,
            // Tuple/Tuple
            (HashableKey::Tuple(a), HashableKey::Tuple(b)) => a == b,
            // FrozenSet/FrozenSet
            (HashableKey::FrozenSet(a), HashableKey::FrozenSet(b)) => a == b,
            // Bool/Int cross-comparison (True == 1, False == 0)
            (HashableKey::Bool(b), HashableKey::Int(n)) | (HashableKey::Int(n), HashableKey::Bool(b)) => {
                *n == PyInt::Small(*b as i64)
            }
            // Int/Float cross-comparison (0 == 0.0, 1 == 1.0, etc.)
            (HashableKey::Int(n), HashableKey::Float(f)) | (HashableKey::Float(f), HashableKey::Int(n)) => {
                let nv = n.to_i64().unwrap_or(i64::MIN);
                f.0.is_finite() && f.0 == nv as f64 && (nv as f64) as i64 == nv
            }
            // Bool/Float cross-comparison (True == 1.0, False == 0.0)
            (HashableKey::Bool(b), HashableKey::Float(f)) | (HashableKey::Float(f), HashableKey::Bool(b)) => {
                f.0 == (*b as i64 as f64)
            }
            // Identity
            (HashableKey::Identity(a, _), HashableKey::Identity(b, _)) => a == b,
            // Custom
            (HashableKey::Custom { hash_value: ha, object: oa }, HashableKey::Custom { hash_value: hb, object: ob }) => {
                if ha != hb { return false; }
                if let Some(result) = call_eq_dispatch(oa, ob) {
                    return result;
                }
                PyObjectRef::ptr_eq(oa, ob)
            }
            _ => false,
        }
    }
}

impl Hash for HashableKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // CPython hash consistency: hash(0) == hash(0.0) == hash(False)
        // Numeric types must produce the same hash for equal values.
        match self {
            HashableKey::None => { 0u8.hash(state); 0i64.hash(state); },
            HashableKey::Bool(b) => { 1u8.hash(state); (*b as i64).hash(state); },
            HashableKey::Int(n) => {
                let v = n.to_i64().unwrap_or(0);
                // Check if this int is representable exactly as f64
                // Use numeric tag so Int(n) and Float(n.0) produce same hash
                1u8.hash(state);
                v.hash(state);
            },
            HashableKey::Float(f) => {
                let fv = f.0;
                if fv.is_finite() && fv == fv.trunc() && fv.abs() < (i64::MAX as f64) {
                    // Exact integer float: hash like the integer
                    1u8.hash(state);
                    (fv as i64).hash(state);
                } else {
                    2u8.hash(state);
                    f.hash(state);
                }
            },
            HashableKey::Str(s) => { 3u8.hash(state); s.hash(state); },
            HashableKey::Bytes(b) => { 4u8.hash(state); b.hash(state); },
            HashableKey::Tuple(items) => { 5u8.hash(state); items.hash(state); },
            HashableKey::FrozenSet(items) => { 6u8.hash(state); items.hash(state); },
            HashableKey::Identity(ptr, _) => { 7u8.hash(state); ptr.hash(state); },
            HashableKey::Custom { hash_value, .. } => { 8u8.hash(state); hash_value.hash(state); },
        }
    }
}

// ── Zero-clone dict lookup keys ──
// These types allow IndexMap::get without cloning the key.
// They hash and compare identically to their HashableKey counterparts.

/// Borrowed str key for zero-clone dict[str] lookups.
pub struct BorrowedStrKey<'a>(pub &'a str);

impl Hash for BorrowedStrKey<'_> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        3u8.hash(state);
        self.0.hash(state);
    }
}

impl indexmap::Equivalent<HashableKey> for BorrowedStrKey<'_> {
    #[inline]
    fn equivalent(&self, key: &HashableKey) -> bool {
        matches!(key, HashableKey::Str(s) if s.as_str() == self.0)
    }
}

impl PartialEq for BorrowedStrKey<'_> {
    fn eq(&self, other: &Self) -> bool { self.0 == other.0 }
}
impl Eq for BorrowedStrKey<'_> {}

/// Borrowed small-int key for zero-clone dict[int] lookups.
pub struct BorrowedIntKey(pub i64);

impl Hash for BorrowedIntKey {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        1u8.hash(state);
        self.0.hash(state);
    }
}

impl indexmap::Equivalent<HashableKey> for BorrowedIntKey {
    #[inline]
    fn equivalent(&self, key: &HashableKey) -> bool {
        match key {
            HashableKey::Int(PyInt::Small(n)) => *n == self.0,
            HashableKey::Bool(b) => (*b as i64) == self.0,
            _ => false,
        }
    }
}

impl PartialEq for BorrowedIntKey {
    fn eq(&self, other: &Self) -> bool { self.0 == other.0 }
}
impl Eq for BorrowedIntKey {}

// ── OrderedFloat ──

#[derive(Debug, Clone, Copy)]
pub struct OrderedFloat(pub f64);
impl PartialEq for OrderedFloat { fn eq(&self, other: &Self) -> bool { self.0.to_bits() == other.0.to_bits() } }
impl Eq for OrderedFloat {}
impl Hash for OrderedFloat { fn hash<H: Hasher>(&self, state: &mut H) { self.0.to_bits().hash(state); } }
impl PartialOrd for OrderedFloat { fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) } }
impl Ord for OrderedFloat { fn cmp(&self, other: &Self) -> std::cmp::Ordering { self.0.total_cmp(&other.0) } }
impl std::fmt::Display for OrderedFloat { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) } }
