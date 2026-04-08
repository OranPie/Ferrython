//! Core Python object types — PyObject, PyObjectPayload, and supporting data types.

use crate::error::{PyResult, ExceptionKind};
use crate::object::methods::PyObjectMethods;
use crate::types::{HashableKey, PyFunction, PyInt};
use compact_str::CompactString;
use indexmap::IndexMap;
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
#[derive(Clone)]
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
    /// A dict that is a live view of an instance's __dict__ (shares backing store)
    InstanceDict(Arc<RwLock<IndexMap<CompactString, PyObjectRef>>>),
    /// Read-only view of a class namespace (types.MappingProxyType)
    MappingProxy(Arc<RwLock<IndexMap<HashableKey, PyObjectRef>>>),
    Function(PyFunction),
    BuiltinFunction(CompactString),
    /// Built-in type object (int, str, float, etc.) — callable as constructor
    BuiltinType(CompactString),
    BoundMethod { receiver: PyObjectRef, method: PyObjectRef },
    BuiltinBoundMethod { receiver: PyObjectRef, method_name: CompactString },
    Code(std::sync::Arc<ferrython_bytecode::CodeObject>),
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
    ExceptionInstance { kind: ExceptionKind, message: CompactString, args: Vec<PyObjectRef>, attrs: Arc<RwLock<IndexMap<CompactString, PyObjectRef>>> },
    /// Generator object (suspended coroutine with opaque frame storage)
    Generator(Arc<RwLock<GeneratorState>>),
    /// Coroutine object (from async def — uses same frame machinery as Generator)
    Coroutine(Arc<RwLock<GeneratorState>>),
    /// Async generator object (from async def with yield)
    AsyncGenerator(Arc<RwLock<GeneratorState>>),
    /// Awaitable returned by async generator protocol methods (__anext__, asend, athrow, aclose).
    /// When driven via send(None), resumes the underlying async generator with the specified action.
    AsyncGenAwaitable {
        gen: Arc<RwLock<GeneratorState>>,
        action: AsyncGenAction,
    },
    /// Native Rust function callable from Python (for module functions)
    NativeFunction {
        name: CompactString,
        func: fn(&[PyObjectRef]) -> PyResult<PyObjectRef>,
    },
    /// Native closure — a Rust function that captures state (for itemgetter, partial, etc.)
    NativeClosure {
        name: CompactString,
        func: Arc<dyn Fn(&[PyObjectRef]) -> PyResult<PyObjectRef> + Send + Sync>,
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
    /// Range object — preserves start/stop/step, creates fresh iterators
    Range { start: i64, stop: i64, step: i64 },
    /// Awaitable that immediately resolves to a pre-computed value.
    /// Used by asyncio.sleep(), asyncio.gather(), etc. to return proper awaitables
    /// from native functions that don't have their own coroutine frame.
    BuiltinAwaitable(PyObjectRef),
    /// Deferred sleep awaitable — carries sleep duration (secs) and result value.
    /// The actual thread::sleep happens when the VM drives this in YIELD_FROM,
    /// allowing asyncio.wait_for to enforce timeouts via a deadline.
    DeferredSleep { secs: f64, result: PyObjectRef },
    /// Dict view objects — live views backed by the underlying dict's Arc
    DictKeys(Arc<RwLock<IndexMap<HashableKey, PyObjectRef>>>),
    DictValues(Arc<RwLock<IndexMap<HashableKey, PyObjectRef>>>),
    DictItems(Arc<RwLock<IndexMap<HashableKey, PyObjectRef>>>),
}

impl fmt::Debug for PyObjectPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Ellipsis => write!(f, "Ellipsis"),
            Self::NotImplemented => write!(f, "NotImplemented"),
            Self::Bool(b) => write!(f, "Bool({b})"),
            Self::Int(n) => write!(f, "Int({n:?})"),
            Self::Float(v) => write!(f, "Float({v})"),
            Self::Complex { real, imag } => write!(f, "Complex({real}+{imag}j)"),
            Self::Str(s) => write!(f, "Str({s:?})"),
            Self::Bytes(b) => write!(f, "Bytes({b:?})"),
            Self::ByteArray(b) => write!(f, "ByteArray({b:?})"),
            Self::List(_) => write!(f, "List(...)"),
            Self::Tuple(items) => write!(f, "Tuple(len={})", items.len()),
            Self::Set(_) => write!(f, "Set(...)"),
            Self::FrozenSet(_) => write!(f, "FrozenSet(...)"),
            Self::Dict(_) => write!(f, "Dict(...)"),
            Self::Function(pf) => write!(f, "Function({:?})", pf.name),
            Self::BuiltinFunction(name) => write!(f, "BuiltinFunction({name})"),
            Self::BuiltinType(name) => write!(f, "BuiltinType({name})"),
            Self::BoundMethod { .. } => write!(f, "BoundMethod(...)"),
            Self::BuiltinBoundMethod { method_name, .. } => write!(f, "BuiltinBoundMethod({method_name})"),
            Self::Code(_) => write!(f, "Code(...)"),
            Self::Class(cd) => write!(f, "Class({})", cd.name),
            Self::Instance(id) => write!(f, "Instance(class={:?})", id.class.payload),
            Self::Module(md) => write!(f, "Module({})", md.name),
            Self::Iterator(_) => write!(f, "Iterator(...)"),
            Self::Slice { .. } => write!(f, "Slice(...)"),
            Self::Cell(_) => write!(f, "Cell(...)"),
            Self::ExceptionType(k) => write!(f, "ExceptionType({k:?})"),
            Self::ExceptionInstance { kind, message, .. } => write!(f, "ExceptionInstance({kind:?}, {message:?})"),
            Self::Generator(_) => write!(f, "Generator(...)"),
            Self::Coroutine(_) => write!(f, "Coroutine(...)"),
            Self::AsyncGenerator(_) => write!(f, "AsyncGenerator(...)"),
            Self::AsyncGenAwaitable { action, .. } => write!(f, "AsyncGenAwaitable({action:?})"),
            Self::NativeFunction { name, .. } => write!(f, "NativeFunction({name})"),
            Self::NativeClosure { name, .. } => write!(f, "NativeClosure({name})"),
            Self::InstanceDict(_) => write!(f, "InstanceDict(...)"),
            Self::MappingProxy(_) => write!(f, "MappingProxy(...)"),
            Self::Partial { .. } => write!(f, "Partial(...)"),
            Self::Property { .. } => write!(f, "Property(...)"),
            Self::StaticMethod(_) => write!(f, "StaticMethod(...)"),
            Self::ClassMethod(_) => write!(f, "ClassMethod(...)"),
            Self::Super { .. } => write!(f, "Super(...)"),
            Self::Range { start, stop, step } => write!(f, "Range({start}, {stop}, {step})"),
            Self::BuiltinAwaitable(_) => write!(f, "BuiltinAwaitable(...)"),
            Self::DeferredSleep { secs, .. } => write!(f, "DeferredSleep({secs}s)"),
            Self::DictKeys(_) => write!(f, "dict_keys(...)"),
            Self::DictValues(_) => write!(f, "dict_values(...)"),
            Self::DictItems(_) => write!(f, "dict_items(...)"),
        }
    }
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

/// The operation an `AsyncGenAwaitable` should perform when driven.
#[derive(Debug, Clone)]
pub enum AsyncGenAction {
    /// `__anext__()` — resume with None, raise StopAsyncIteration on exhaustion
    Next,
    /// `asend(val)` — resume with val
    Send(PyObjectRef),
    /// `athrow(exc_type, msg)` — throw exception into generator
    Throw(ExceptionKind, CompactString),
    /// `aclose()` — throw GeneratorExit, expect generator to finish
    Close,
}

#[derive(Debug, Clone)]
pub struct ClassData {
    pub name: CompactString,
    pub bases: Vec<PyObjectRef>,
    pub namespace: Arc<RwLock<IndexMap<CompactString, PyObjectRef>>>,
    pub mro: Vec<PyObjectRef>,
    /// Custom metaclass, if any (e.g., SingletonMeta). None = default `type`.
    pub metaclass: Option<PyObjectRef>,
    /// Per-class method resolution cache: avoids repeated MRO scans for the same attr name.
    /// Cleared on any namespace mutation (class attr assignment).
    pub method_cache: Arc<RwLock<IndexMap<CompactString, Option<PyObjectRef>>>>,
    /// Weak references to direct subclasses (for type.__subclasses__()).
    pub subclasses: Arc<RwLock<Vec<std::sync::Weak<PyObject>>>>,
    /// `__slots__` declared on *this* class (None means no __slots__ declared).
    pub slots: Option<Vec<CompactString>>,
    /// Fast-path flag: true if this class (or any base) defines a custom __getattribute__.
    /// When false, the VM skips the expensive MRO lookup on every LoadAttr.
    pub has_getattribute: bool,
}

impl ClassData {
    pub fn new(
        name: CompactString,
        bases: Vec<PyObjectRef>,
        namespace: IndexMap<CompactString, PyObjectRef>,
        mro: Vec<PyObjectRef>,
        metaclass: Option<PyObjectRef>,
    ) -> Self {
        // Extract __slots__ from the namespace if present
        let slots: Option<Vec<CompactString>> = namespace.get("__slots__").and_then(|s| {
            match &s.payload {
                PyObjectPayload::List(items) => {
                    let items = items.read();
                    Some(items.iter().map(|item: &PyObjectRef| CompactString::from(item.py_to_string())).collect::<Vec<_>>())
                }
                PyObjectPayload::Tuple(items) => {
                    Some(items.iter().map(|item: &PyObjectRef| CompactString::from(item.py_to_string())).collect::<Vec<_>>())
                }
                PyObjectPayload::Str(s) => {
                    // Single string slot: __slots__ = "x"
                    Some(vec![s.clone()])
                }
                _ => None,
            }
        });
        // Detect __getattribute__ override in namespace or any base class
        let has_getattribute = namespace.contains_key("__getattribute__") || mro.iter().any(|base| {
            if let PyObjectPayload::Class(bcd) = &base.payload {
                bcd.namespace.read().contains_key("__getattribute__")
            } else {
                false
            }
        });
        Self {
            name,
            bases,
            namespace: Arc::new(RwLock::new(namespace)),
            mro,
            metaclass,
            method_cache: Arc::new(RwLock::new(IndexMap::new())),
            subclasses: Arc::new(RwLock::new(Vec::new())),
            slots,
            has_getattribute,
        }
    }

    /// Collect all allowed slot names from this class and its MRO.
    /// Returns `None` if no class in the hierarchy defines `__slots__`.
    pub fn collect_all_slots(&self) -> Option<Vec<CompactString>> {
        let mut all_slots: Vec<CompactString> = Vec::new();
        let mut found_any = false;

        if let Some(ref s) = self.slots {
            found_any = true;
            for name in s {
                if !all_slots.contains(name) {
                    all_slots.push(name.clone());
                }
            }
        }
        for cls in &self.mro {
            if let PyObjectPayload::Class(cd) = &cls.payload {
                if let Some(ref s) = cd.slots {
                    found_any = true;
                    for name in s {
                        if !all_slots.contains(name) {
                            all_slots.push(name.clone());
                        }
                    }
                }
            }
        }
        if found_any { Some(all_slots) } else { None }
    }

    /// Whether `__dict__` is allowed on instances of this class.
    pub fn has_dict_slot(&self) -> bool {
        if let Some(ref slots) = self.collect_all_slots() {
            slots.iter().any(|s| s.as_str() == "__dict__")
        } else {
            true // no __slots__ → __dict__ is always available
        }
    }

    /// Invalidate the method cache (call after any namespace mutation).
    pub fn invalidate_cache(&self) {
        self.method_cache.write().clear();
    }
}

#[derive(Debug, Clone)]
pub struct InstanceData {
    pub class: PyObjectRef,
    pub attrs: Arc<RwLock<IndexMap<CompactString, PyObjectRef>>>,
    /// Internal dict storage for dict subclasses
    pub dict_storage: Option<Arc<RwLock<IndexMap<HashableKey, PyObjectRef>>>>,
    /// Fast-path flag: true if this instance has special markers (__namedtuple__, __deque__, etc.)
    /// When true, LoadMethod uses the full get_attr path.
    pub is_special: bool,
}

#[derive(Debug, Clone)]
pub struct ModuleData {
    pub name: CompactString,
    pub attrs: Arc<parking_lot::RwLock<IndexMap<CompactString, PyObjectRef>>>,
}

#[derive(Debug, Clone)]
pub enum IteratorData {
    List { items: Vec<PyObjectRef>, index: usize },
    Tuple { items: Vec<PyObjectRef>, index: usize },
    Range { current: i64, stop: i64, step: i64 },
    Str { chars: Vec<char>, index: usize },
    Enumerate { source: PyObjectRef, index: i64 },
    Zip { sources: Vec<PyObjectRef>, strict: bool },
    Map { func: PyObjectRef, source: PyObjectRef },
    Filter { func: PyObjectRef, source: PyObjectRef },
    Sentinel { callable: PyObjectRef, sentinel: PyObjectRef },
    TakeWhile { func: PyObjectRef, source: PyObjectRef, done: bool },
    DropWhile { func: PyObjectRef, source: PyObjectRef, dropping: bool },
}

