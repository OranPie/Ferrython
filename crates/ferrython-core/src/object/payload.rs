//! Core Python object types — PyObject, PyObjectPayload, and supporting data types.

use crate::error::{PyResult, ExceptionKind};
use crate::object::methods::PyObjectMethods;
use crate::types::{HashableKey, PyFunction, PyInt};
use compact_str::CompactString;
use indexmap::IndexMap;
use rustc_hash::{FxHashMap, FxHasher};
use std::any::Any;
use std::cell::{Cell, RefCell, UnsafeCell};
use std::hash::BuildHasherDefault;
use std::fmt;
use std::mem::MaybeUninit;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

// ── PyCell: zero-overhead interior mutability (replaces parking_lot::RwLock) ──

/// Zero-overhead interior mutability cell for GIL-semantics interpreter.
/// Provides the same `.read()` / `.write()` / `.data_ptr()` API as parking_lot::RwLock
/// but with zero locking overhead — just returns references through UnsafeCell.
///
/// SAFETY: Ferrython uses GIL semantics (single-threaded Python execution).
/// No concurrent access to PyCell contents occurs.
pub struct PyCell<T>(UnsafeCell<T>);

unsafe impl<T> Send for PyCell<T> {}
unsafe impl<T> Sync for PyCell<T> {}

impl<T> PyCell<T> {
    #[inline(always)]
    pub fn new(val: T) -> Self { Self(UnsafeCell::new(val)) }

    #[inline(always)]
    pub fn read(&self) -> PyCellRef<'_, T> {
        PyCellRef(unsafe { &*self.0.get() })
    }

    #[inline(always)]
    pub fn write(&self) -> PyCellMut<'_, T> {
        PyCellMut(unsafe { &mut *self.0.get() })
    }

    #[inline(always)]
    pub fn data_ptr(&self) -> *mut T { self.0.get() }
}

impl<T: Clone> Clone for PyCell<T> {
    fn clone(&self) -> Self {
        Self::new(unsafe { &*self.0.get() }.clone())
    }
}

impl<T: fmt::Debug> fmt::Debug for PyCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        unsafe { &*self.0.get() }.fmt(f)
    }
}

/// Read guard for PyCell — Deref to &T (zero-cost wrapper).
pub struct PyCellRef<'a, T>(&'a T);

impl<'a, T> std::ops::Deref for PyCellRef<'a, T> {
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &T { self.0 }
}

/// Write guard for PyCell — DerefMut to &mut T (zero-cost wrapper).
pub struct PyCellMut<'a, T>(&'a mut T);

impl<'a, T> std::ops::Deref for PyCellMut<'a, T> {
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &T { self.0 }
}

impl<'a, T> std::ops::DerefMut for PyCellMut<'a, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut T { self.0 }
}

/// FxHash build hasher — ~3-4x faster than SipHash for short strings.
pub type FxBuildHasher = BuildHasherDefault<FxHasher>;

/// Attribute map using FxHash instead of SipHash.
/// Used for instance attrs, class namespaces, and module attrs (hot path).
pub type FxAttrMap = IndexMap<CompactString, PyObjectRef, FxBuildHasher>;

/// Dict/Set map using FxHash for fast key lookups (HashableKey → PyObjectRef).
pub type FxHashKeyMap = IndexMap<HashableKey, PyObjectRef, FxBuildHasher>;

/// Create a new empty FxHashKeyMap (with FxHash, not SipHash).
#[inline]
pub fn new_fx_hashkey_map() -> FxHashKeyMap {
    IndexMap::with_hasher(FxBuildHasher::default())
}

/// Convert a SipHash IndexMap to FxHashKeyMap.
#[inline]
pub fn to_fx_hashkey_map(map: IndexMap<HashableKey, PyObjectRef>) -> FxHashKeyMap {
    map.into_iter().collect()
}

/// Shared attribute map behind Rc<PyCell> — used by InstanceData and InstanceDict.
pub type SharedFxAttrMap = Rc<PyCell<FxAttrMap>>;

/// Convert a SipHash IndexMap to SharedFxAttrMap (for callers that build with IndexMap::new()).
#[inline]
pub fn to_shared_fx(attrs: IndexMap<CompactString, PyObjectRef>) -> SharedFxAttrMap {
    Rc::new(PyCell::new(attrs.into_iter().collect()))
}

/// Create a new empty SharedFxAttrMap.
#[inline]
pub fn new_shared_fx() -> SharedFxAttrMap {
    Rc::new(PyCell::new(FxAttrMap::default()))
}

/// Global monotonic counter for class versioning. Incremented each time a
/// ClassData is created or mutated. Used by inline caches to detect staleness.
static CLASS_VERSION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Allocate a fresh class version number.
#[inline(always)]
pub fn next_class_version() -> u64 {
    CLASS_VERSION_COUNTER.fetch_add(1, Ordering::Relaxed)
}

// Compile-time size check: ensure enum stays compact after boxing cold variants
// Target: ≤32 bytes (down from 40). Further reduction possible by boxing more variants.
const _PAYLOAD_SIZE_CHECK: () = assert!(std::mem::size_of::<PyObjectPayload>() <= 40);

// ── PyObject Freelist Allocator ──
// Replaces Rc<PyObject> with a custom ref-counted pointer backed by a
// thread-local freelist. Eliminates malloc/free for hot object creation
// (CPython uses the same freelist strategy via pymalloc + per-type freelists).

/// Internal block layout for a reference-counted Python object.
/// Placed on the heap; recycled via thread-local freelist when ref count hits 0.
#[repr(C)]
struct PyObjectBlock {
    /// Strong reference count (u32 is sufficient for single-threaded interpreter).
    strong: Cell<u32>,
    /// Weak reference count (tracked for PyWeakRef support).
    weak: Cell<u32>,
    /// The Python object data (may be uninitialized after strong reaches 0 but weak > 0).
    obj: MaybeUninit<PyObject>,
}

const MAX_OBJECT_POOL: usize = 512;

thread_local! {
    static OBJECT_POOL: RefCell<Vec<NonNull<PyObjectBlock>>> =
        RefCell::new(Vec::with_capacity(256));
}

#[inline(always)]
fn pool_alloc(obj: PyObject) -> NonNull<PyObjectBlock> {
    let block = OBJECT_POOL.with(|pool| pool.borrow_mut().pop())
        .unwrap_or_else(|| {
            // Allocate new block
            let layout = std::alloc::Layout::new::<PyObjectBlock>();
            let ptr = unsafe { std::alloc::alloc(layout) as *mut PyObjectBlock };
            NonNull::new(ptr).expect("allocation failed")
        });
    unsafe {
        let p = block.as_ptr();
        (*p).strong = Cell::new(1);
        (*p).weak = Cell::new(0);
        (*p).obj.as_mut_ptr().write(obj);
    }
    block
}

#[inline(always)]
fn pool_recycle(block: NonNull<PyObjectBlock>) {
    OBJECT_POOL.with(|pool| {
        let mut p = pool.borrow_mut();
        if p.len() < MAX_OBJECT_POOL {
            p.push(block);
        } else {
            unsafe {
                std::alloc::dealloc(
                    block.as_ptr() as *mut u8,
                    std::alloc::Layout::new::<PyObjectBlock>(),
                );
            }
        }
    });
}

/// A reference-counted handle to a Python object.
/// Backed by a thread-local freelist — allocation is a Vec::pop, not malloc.
pub struct PyObjectRef(NonNull<PyObjectBlock>);

// SAFETY: Ferrython is a single-threaded interpreter (GIL equivalent).
// PyObjectRef values never cross thread boundaries during normal operation.
// The unsafe Send+Sync impls are needed for: static singletons (LazyLock),
// OnceLock caches, and SharedBuiltins (Arc<IndexMap<..., PyObjectRef>>).
unsafe impl Send for PyObjectRef {}
unsafe impl Sync for PyObjectRef {}

impl PyObjectRef {
    #[inline(always)]
    pub fn new(obj: PyObject) -> Self { Self(pool_alloc(obj)) }

    #[inline(always)]
    pub fn ptr_eq(a: &Self, b: &Self) -> bool { a.0 == b.0 }

    #[inline(always)]
    pub fn as_ptr(this: &Self) -> *const PyObject {
        unsafe { (*this.0.as_ptr()).obj.as_ptr() }
    }

    #[inline(always)]
    pub fn strong_count(this: &Self) -> usize {
        unsafe { (*this.0.as_ptr()).strong.get() as usize }
    }

    #[inline(always)]
    pub fn downgrade(this: &Self) -> PyWeakRef {
        unsafe {
            let p = this.0.as_ptr();
            (*p).weak.set((*p).weak.get() + 1);
        }
        PyWeakRef(this.0)
    }

    #[inline(always)]
    pub fn weak_count(this: &Self) -> usize {
        unsafe { (*this.0.as_ptr()).weak.get() as usize }
    }

    #[inline(always)]
    pub fn get_mut(this: &mut Self) -> Option<&mut PyObject> {
        unsafe {
            let p = this.0.as_ptr();
            if (*p).strong.get() == 1 && (*p).weak.get() == 0 {
                Some(&mut *(*p).obj.as_mut_ptr())
            } else {
                None
            }
        }
    }
}

impl Clone for PyObjectRef {
    #[inline(always)]
    fn clone(&self) -> Self {
        unsafe {
            let c = &(*self.0.as_ptr()).strong;
            c.set(c.get() + 1);
        }
        Self(self.0)
    }
}

impl Drop for PyObjectRef {
    #[inline(always)]
    fn drop(&mut self) {
        unsafe {
            let p = self.0.as_ptr();
            let new_strong = (*p).strong.get() - 1;
            (*p).strong.set(new_strong);
            if new_strong == 0 {
                // Drop the PyObject value
                std::ptr::drop_in_place((*p).obj.as_mut_ptr());
                // If no weak refs, recycle immediately
                if (*p).weak.get() == 0 {
                    pool_recycle(self.0);
                }
                // else: block stays alive for weak refs; recycled when last weak drops
            }
        }
    }
}

impl std::ops::Deref for PyObjectRef {
    type Target = PyObject;
    #[inline(always)]
    fn deref(&self) -> &PyObject {
        unsafe { &*(*self.0.as_ptr()).obj.as_ptr() }
    }
}

impl AsRef<PyObject> for PyObjectRef {
    #[inline(always)]
    fn as_ref(&self) -> &PyObject {
        unsafe { &*(*self.0.as_ptr()).obj.as_ptr() }
    }
}

impl fmt::Debug for PyObjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}

/// Weak reference to a Python object (for GC cycle detection and weakref module).
/// Keeps the PyObjectBlock alive but does not prevent the PyObject from being dropped.
pub struct PyWeakRef(NonNull<PyObjectBlock>);

// SAFETY: Same as PyObjectRef — single-threaded interpreter.
unsafe impl Send for PyWeakRef {}
unsafe impl Sync for PyWeakRef {}

impl PyWeakRef {
    #[inline(always)]
    pub fn upgrade(&self) -> Option<PyObjectRef> {
        unsafe {
            let p = self.0.as_ptr();
            let s = (*p).strong.get();
            if s > 0 {
                (*p).strong.set(s + 1);
                Some(PyObjectRef(self.0))
            } else {
                None
            }
        }
    }

    #[inline(always)]
    pub fn strong_count(&self) -> usize {
        unsafe { (*self.0.as_ptr()).strong.get() as usize }
    }
}

impl Clone for PyWeakRef {
    #[inline(always)]
    fn clone(&self) -> Self {
        unsafe {
            let c = &(*self.0.as_ptr()).weak;
            c.set(c.get() + 1);
        }
        Self(self.0)
    }
}

impl Drop for PyWeakRef {
    #[inline(always)]
    fn drop(&mut self) {
        unsafe {
            let p = self.0.as_ptr();
            let new_weak = (*p).weak.get() - 1;
            (*p).weak.set(new_weak);
            // Recycle block when both strong and weak reach 0
            if new_weak == 0 && (*p).strong.get() == 0 {
                pool_recycle(self.0);
            }
        }
    }
}

impl fmt::Debug for PyWeakRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PyWeakRef(strong={})", self.strong_count())
    }
}

/// Wrapper around AtomicI64 that implements Clone (loads current value).
#[repr(transparent)]
pub struct SyncI64(pub AtomicI64);

impl SyncI64 {
    #[inline(always)]
    pub fn new(v: i64) -> Self { Self(AtomicI64::new(v)) }
    #[inline(always)]
    pub fn get(&self) -> i64 { self.0.load(Ordering::Relaxed) }
    #[inline(always)]
    pub fn set(&self, v: i64) { self.0.store(v, Ordering::Relaxed) }
}

impl Clone for SyncI64 {
    #[inline(always)]
    fn clone(&self) -> Self { Self::new(self.get()) }
}

impl fmt::Debug for SyncI64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SyncI64({})", self.get())
    }
}

/// A Python object.
#[derive(Debug, Clone)]
pub struct PyObject {
    pub payload: PyObjectPayload,
}

// Safety: Ferrython uses GIL semantics (single-threaded execution).
// PyObject/PyObjectPayload contain Rc and RefCell which are !Send+!Sync,
// but we need Send+Sync for static singletons and thread::spawn in _thread module.
unsafe impl Send for PyObject {}
unsafe impl Sync for PyObject {}
unsafe impl Send for PyObjectPayload {}
unsafe impl Sync for PyObjectPayload {}

/// Boxed exception instance data (moved out of enum to reduce PyObjectPayload size)
#[derive(Clone, Debug)]
pub struct ExceptionInstanceData {
    pub kind: ExceptionKind,
    pub message: CompactString,
    pub args: Vec<PyObjectRef>,
    pub attrs: SharedFxAttrMap,
}

/// Boxed partial application data
#[derive(Clone, Debug)]
pub struct PartialData {
    pub func: PyObjectRef,
    pub args: Vec<PyObjectRef>,
    pub kwargs: Vec<(CompactString, PyObjectRef)>,
}

/// Boxed native closure data
#[derive(Clone)]
pub struct NativeClosureData {
    pub name: CompactString,
    pub func: Rc<dyn Fn(&[PyObjectRef]) -> PyResult<PyObjectRef>>,
}

// SAFETY: Single-threaded interpreter — Rc-based closures never cross threads.
unsafe impl Send for NativeClosureData {}
unsafe impl Sync for NativeClosureData {}

/// Boxed slice data (cold variant, moved out to shrink PyObjectPayload)
#[derive(Clone, Debug)]
pub struct SliceData {
    pub start: Option<PyObjectRef>,
    pub stop: Option<PyObjectRef>,
    pub step: Option<PyObjectRef>,
}

/// Boxed property descriptor data (cold variant)
#[derive(Clone, Debug)]
pub struct PropertyData {
    pub fget: Option<PyObjectRef>,
    pub fset: Option<PyObjectRef>,
    pub fdel: Option<PyObjectRef>,
}

/// Boxed native function data (cold variant — registered once at startup)
#[derive(Clone, Debug)]
pub struct NativeFunctionData {
    pub name: CompactString,
    pub func: fn(&[PyObjectRef]) -> PyResult<PyObjectRef>,
}

/// Boxed builtin bound method data (cold variant)
#[derive(Clone, Debug)]
pub struct BuiltinBoundMethodData {
    pub receiver: PyObjectRef,
    pub method_name: CompactString,
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
    List(PyCell<Vec<PyObjectRef>>),
    Tuple(Vec<PyObjectRef>),
    Set(Rc<PyCell<FxHashKeyMap>>),
    FrozenSet(Box<FxHashKeyMap>),
    Dict(Rc<PyCell<FxHashKeyMap>>),
    /// A dict that is a live view of an instance's __dict__ (shares backing store)
    InstanceDict(SharedFxAttrMap),
    /// Read-only view of a class namespace (types.MappingProxyType)
    MappingProxy(Rc<PyCell<FxHashKeyMap>>),
    Function(Box<PyFunction>),
    BuiltinFunction(CompactString),
    /// Built-in type object (int, str, float, etc.) — callable as constructor
    BuiltinType(CompactString),
    BoundMethod { receiver: PyObjectRef, method: PyObjectRef },
    BuiltinBoundMethod(Box<BuiltinBoundMethodData>),
    Code(std::sync::Arc<ferrython_bytecode::CodeObject>),
    Class(Box<ClassData>),
    Instance(Box<InstanceData>),
    Module(Box<ModuleData>),
    Iterator(Rc<PyCell<IteratorData>>),
    /// Lock-free range iterator — avoids Mutex overhead for `for i in range(n)`.
    RangeIter { current: SyncI64, stop: i64, step: i64 },
    Slice(Box<SliceData>),
    /// A cell object wrapping a shared mutable reference (for closures).
    Cell(Rc<PyCell<Option<PyObjectRef>>>),
    /// Exception type object (e.g. ValueError, TypeError)
    ExceptionType(ExceptionKind),
    /// Exception instance (raised exception with kind, message, and optional args)
    ExceptionInstance(Box<ExceptionInstanceData>),
    /// Generator object (suspended coroutine with opaque frame storage)
    Generator(Rc<PyCell<GeneratorState>>),
    /// Coroutine object (from async def — uses same frame machinery as Generator)
    Coroutine(Rc<PyCell<GeneratorState>>),
    /// Async generator object (from async def with yield)
    AsyncGenerator(Rc<PyCell<GeneratorState>>),
    /// Awaitable returned by async generator protocol methods (__anext__, asend, athrow, aclose).
    /// When driven via send(None), resumes the underlying async generator with the specified action.
    AsyncGenAwaitable {
        gen: Rc<PyCell<GeneratorState>>,
        action: AsyncGenAction,
    },
    /// Native Rust function callable from Python (for module functions)
    NativeFunction(Box<NativeFunctionData>),
    /// Native closure — a Rust function that captures state (for itemgetter, partial, etc.)
    NativeClosure(Box<NativeClosureData>),
    /// Partial application (functools.partial)
    Partial(Box<PartialData>),
    /// Property descriptor
    Property(Box<PropertyData>),
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
    DictKeys(Rc<PyCell<FxHashKeyMap>>),
    DictValues(Rc<PyCell<FxHashKeyMap>>),
    DictItems(Rc<PyCell<FxHashKeyMap>>),
}

impl Drop for PyObjectPayload {
    #[inline]
    fn drop(&mut self) {
        match self {
            PyObjectPayload::Dict(rc) | PyObjectPayload::Set(rc) => {
                super::constructors::try_recycle_map(rc);
                // After try_recycle_map, the Rc is either recycled (refcount bumped to
                // freelist) or not. Either way, normal drop of `self` will decrement the
                // Rc — if recycled, it goes from 2→1 (freelist holds it); if not, it
                // goes from N→N-1 (normal behavior).
            }
            _ => {}
        }
    }
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
            Self::BuiltinBoundMethod(bbm) => write!(f, "BuiltinBoundMethod({})", bbm.method_name),
            Self::Code(_) => write!(f, "Code(...)"),
            Self::Class(cd) => write!(f, "Class({})", cd.name),
            Self::Instance(id) => write!(f, "Instance(class={:?})", id.class.payload),
            Self::Module(md) => write!(f, "Module({})", md.name),
            Self::Iterator(_) => write!(f, "Iterator(...)"),
            Self::RangeIter { current, stop, step } => write!(f, "RangeIter({}, {stop}, {step})", current.get()),
            Self::Slice(_) => write!(f, "Slice(...)"),
            Self::Cell(_) => write!(f, "Cell(...)"),
            Self::ExceptionType(k) => write!(f, "ExceptionType({k:?})"),
            Self::ExceptionInstance(ei) => write!(f, "ExceptionInstance({:?}, {:?})", ei.kind, ei.message),
            Self::Generator(_) => write!(f, "Generator(...)"),
            Self::Coroutine(_) => write!(f, "Coroutine(...)"),
            Self::AsyncGenerator(_) => write!(f, "AsyncGenerator(...)"),
            Self::AsyncGenAwaitable { action, .. } => write!(f, "AsyncGenAwaitable({action:?})"),
            Self::NativeFunction(nf) => write!(f, "NativeFunction({})", nf.name),
            Self::NativeClosure(nc) => write!(f, "NativeClosure({})", nc.name),
            Self::InstanceDict(_) => write!(f, "InstanceDict(...)"),
            Self::MappingProxy(_) => write!(f, "MappingProxy(...)"),
            Self::Partial(_) => write!(f, "Partial(...)"),
            Self::Property(_) => write!(f, "Property(...)"),
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
    pub frame: Option<Box<dyn Any>>,
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
    pub namespace: Rc<PyCell<FxAttrMap>>,
    pub mro: Vec<PyObjectRef>,
    /// Custom metaclass, if any (e.g., SingletonMeta). None = default `type`.
    pub metaclass: Option<PyObjectRef>,
    /// Per-class method resolution cache: avoids repeated MRO scans for the same attr name.
    /// Cleared on any namespace mutation (class attr assignment).
    /// Uses FxHashMap for faster hashing (no insertion-order needed).
    pub method_cache: Rc<PyCell<FxHashMap<CompactString, Option<PyObjectRef>>>>,
    /// Fast-path flag: true if this class or any base defines Property, __set__, or __delete__.
    /// When false, instance attr lookup can skip the descriptor protocol entirely.
    pub has_descriptors: bool,
    /// Weak references to direct subclasses (for type.__subclasses__()).
    pub subclasses: Rc<PyCell<Vec<PyWeakRef>>>,
    /// `__slots__` declared on *this* class (None means no __slots__ declared).
    pub slots: Option<Vec<CompactString>>,
    /// Fast-path flag: true if this class (or any base) defines a custom __getattribute__.
    /// When false, the VM skips the expensive MRO lookup on every LoadAttr.
    pub has_getattribute: bool,
    /// Fast-path flag: true if this class (or any base) defines a custom __setattr__.
    /// When false, StoreAttr can write directly to instance attrs dict.
    pub has_setattr: bool,
    /// Pre-computed method vtable: flattened MRO methods for O(1) lookup.
    /// Built at class creation time from own namespace + all bases in MRO order.
    /// Cleared on namespace mutation alongside method_cache.
    pub method_vtable: Rc<PyCell<FxHashMap<CompactString, PyObjectRef>>>,
    /// Instance attribute shape: maps attr name → dense index for O(1) attr access.
    /// Built from __init__ analysis or __slots__. Instances store values in a Vec
    /// indexed by these offsets. Attrs not in the shape fall back to overflow dict.
    pub attr_shape: Arc<FxHashMap<CompactString, usize>>,
    /// Monotonic version counter — incremented on any class mutation to invalidate
    /// inline caches and method vtable.
    pub class_version: u64,
    /// Cached flag: true if this class inherits from `dict`.
    /// Pre-computed at class creation to avoid walking the hierarchy per instance.
    pub is_dict_subclass: bool,
    /// Number of expected instance attrs (from attr_shape).
    /// Used to pre-allocate IndexMap capacity in instance creation.
    pub expected_attrs: usize,
}

impl ClassData {
    pub fn new(
        name: CompactString,
        bases: Vec<PyObjectRef>,
        namespace: FxAttrMap,
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
        // Detect __setattr__ override in namespace or any base class
        let has_setattr = namespace.contains_key("__setattr__") || mro.iter().any(|base| {
            if let PyObjectPayload::Class(bcd) = &base.payload {
                bcd.namespace.read().contains_key("__setattr__")
            } else {
                false
            }
        });
        // Detect data descriptors (Property, __set__, __delete__) in this class or bases
        let has_descriptors = Self::detect_descriptors(&namespace, &mro);
        // If MRO is empty but we have bases, build a simple linearization
        let mro = if mro.is_empty() && !bases.is_empty() {
            let mut result = Vec::new();
            for base in &bases {
                if !result.iter().any(|r: &PyObjectRef| PyObjectRef::ptr_eq(r, base)) {
                    result.push(base.clone());
                }
                if let PyObjectPayload::Class(cd) = &base.payload {
                    for m in &cd.mro {
                        if !result.iter().any(|r: &PyObjectRef| PyObjectRef::ptr_eq(r, m)) {
                            result.push(m.clone());
                        }
                    }
                }
            }
            result
        } else {
            mro
        };
        // Build method vtable by flattening MRO methods
        let mut vtable = FxHashMap::default();
        for base in mro.iter().rev() {
            if let PyObjectPayload::Class(bcd) = &base.payload {
                for (k, v) in bcd.namespace.read().iter() {
                    vtable.insert(k.clone(), v.clone());
                }
            }
        }
        for (k, v) in namespace.iter() {
            vtable.insert(k.clone(), v.clone());
        }

        // Build attribute shape from __slots__ and __init__ StoreAttr targets
        let mut attr_shape = FxHashMap::default();
        if let Some(ref s) = slots {
            for (i, name) in s.iter().enumerate() {
                attr_shape.insert(name.clone(), i);
            }
        }
        if let Some(init_fn) = namespace.get("__init__") {
            if let PyObjectPayload::Function(ref pf) = init_fn.payload {
                use ferrython_bytecode::Opcode;
                for instr in &pf.code.instructions {
                    if instr.op == Opcode::StoreAttr {
                        let name_idx = instr.arg as usize;
                        if name_idx < pf.code.names.len() {
                            let attr_name = &pf.code.names[name_idx];
                            if !attr_shape.contains_key(attr_name.as_str()) {
                                let idx = attr_shape.len();
                                attr_shape.insert(attr_name.clone(), idx);
                            }
                        }
                    }
                }
            }
        }

        // Detect dict subclass (cache once instead of per-instance traversal)
        let is_dict_subclass = Self::check_dict_subclass(&bases);

        let expected_attrs = attr_shape.len();

        Self {
            name,
            bases,
            namespace: Rc::new(PyCell::new(namespace)),
            mro,
            metaclass,
            method_cache: Rc::new(PyCell::new(FxHashMap::default())),
            subclasses: Rc::new(PyCell::new(Vec::new())),
            slots,
            has_getattribute,
            has_setattr,
            has_descriptors,
            method_vtable: Rc::new(PyCell::new(vtable)),
            attr_shape: Arc::new(attr_shape),
            class_version: next_class_version(),
            is_dict_subclass,
            expected_attrs,
        }
    }

    /// Rebuild method vtable after a class mutation. Call after modifying the namespace.
    pub fn rebuild_vtable(&mut self) {
        let mut vtable = FxHashMap::default();
        for base in self.mro.iter().rev() {
            if let PyObjectPayload::Class(bcd) = &base.payload {
                for (k, v) in bcd.namespace.read().iter() {
                    vtable.insert(k.clone(), v.clone());
                }
            }
        }
        for (k, v) in self.namespace.read().iter() {
            vtable.insert(k.clone(), v.clone());
        }
        self.method_vtable = Rc::new(PyCell::new(vtable));
        self.class_version = next_class_version();
    }

    /// Collect all allowed slot names from this class and its MRO.
    /// Returns `None` if no class in the hierarchy defines `__slots__`.
    pub fn collect_all_slots(&self) -> Option<Vec<CompactString>> {
        let mut all_slots: Vec<CompactString> = Vec::new();
        let mut found_any = false;

        // CPython rule: if ANY class in the MRO lacks __slots__, instances
        // get __dict__ and arbitrary attribute access is allowed.
        // Check that the class itself AND every base in MRO define __slots__.
        let mut all_have_slots = self.slots.is_some();

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
                } else {
                    all_have_slots = false;
                }
            } else if let PyObjectPayload::BuiltinType(n) = &cls.payload {
                // object has no __slots__ → allows __dict__
                if n.as_str() == "object" {
                    // object is special: it doesn't restrict __dict__
                    // (only restrict if ALL user classes in MRO have __slots__)
                }
            }
        }

        // If any non-object class in MRO lacks __slots__, allow __dict__
        if !all_have_slots {
            return None;
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

    /// Invalidate the method cache and vtable (call after any namespace mutation).
    pub fn invalidate_cache(&self) {
        self.method_cache.write().clear();
        self.method_vtable.write().clear();
    }

    /// Detect if this class or any base has data descriptors (Property, __set__, __delete__).
    /// When false, instance attribute lookup can skip the full descriptor protocol and
    /// check instance __dict__ directly — a significant hot-path optimization.
    fn detect_descriptors(namespace: &FxAttrMap, mro: &[PyObjectRef]) -> bool {
        // Check own namespace for Property or descriptor-like objects
        for v in namespace.values() {
            match &v.payload {
                PyObjectPayload::Property { .. } => return true,
                PyObjectPayload::Instance(inst) => {
                    let attrs = inst.attrs.read();
                    if attrs.contains_key("__set__") || attrs.contains_key("__delete__") {
                        return true;
                    }
                    // Check class for __set__/__delete__
                    if let PyObjectPayload::Class(icd) = &inst.class.payload {
                        if icd.namespace.read().contains_key("__set__")
                            || icd.namespace.read().contains_key("__delete__") {
                            return true;
                        }
                    }
                }
                _ => {}
            }
        }
        // Check bases
        for base in mro {
            if let PyObjectPayload::Class(bcd) = &base.payload {
                if bcd.has_descriptors {
                    return true;
                }
            }
        }
        false
    }

    /// Check if this class inherits from dict (cached at class creation).
    fn check_dict_subclass(bases: &[PyObjectRef]) -> bool {
        for base in bases {
            let is_dict = match &base.payload {
                PyObjectPayload::BuiltinType(n) => n.as_str() == "dict",
                PyObjectPayload::Class(bcd) => bcd.name.as_str() == "dict" || bcd.is_dict_subclass,
                _ => false,
            };
            if is_dict { return true; }
        }
        false
    }
}

#[derive(Debug, Clone)]
pub struct InstanceData {
    pub class: PyObjectRef,
    pub attrs: SharedFxAttrMap,
    /// Internal dict storage for dict subclasses
    pub dict_storage: Option<Rc<PyCell<FxHashKeyMap>>>,
    /// Fast-path flag: true if this instance has special markers (__namedtuple__, __deque__, etc.)
    /// When true, LoadMethod uses the full get_attr path.
    pub is_special: bool,
}

#[derive(Debug, Clone)]
pub struct ModuleData {
    pub name: CompactString,
    pub attrs: Rc<PyCell<FxAttrMap>>,
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
    /// Infinite counter: count(start, step)
    Count { current: i64, step: i64 },
    /// Infinite cycle over cached items
    Cycle { items: Vec<PyObjectRef>, index: usize },
    /// Repeat item n times (None = infinite)
    Repeat { item: PyObjectRef, remaining: Option<usize> },
    /// Chain multiple iterators sequentially
    Chain { sources: Vec<PyObjectRef>, current: usize },
    /// Starmap: apply func to each tuple of args
    Starmap { func: PyObjectRef, source: PyObjectRef },
}

