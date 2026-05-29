//! Core Python object types — PyObject, PyObjectPayload, and supporting data types.

use super::ClassData;
use crate::error::{ExceptionKind, PyResult};
use crate::object::methods::PyObjectMethods;
pub use crate::object::{
    FrozenSetData, FxAttrMap, FxHashKeyFlatMap, FxHashKeyMap, PyCell, SharedFxAttrMap, StrRepr,
};
use crate::types::{PyFunction, PyInt};
use compact_str::CompactString;
use num_bigint::BigInt;
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::{Cell, RefCell, UnsafeCell};
use std::fmt;
use std::mem::{ManuallyDrop, MaybeUninit};
use std::ptr::NonNull;
use std::rc::Rc;

#[allow(unused_imports)]
pub use crate::object::{
    new_fx_hashkey_flatmap, new_fx_hashkey_flatmap_with_capacity, new_fx_hashkey_map,
    new_shared_fx, to_fx_hashkey_map, to_shared_fx, FxBuildHasher, PyCellMut, PyCellRef,
};

#[derive(Clone)]
pub enum WeakObjectKind {
    Ref,
    Proxy,
}

#[derive(Clone)]
struct WeakObjectEntry {
    weak_obj: PyWeakRef,
    callback: Option<PyObjectRef>,
    kind: WeakObjectKind,
}

thread_local! {
    static WEAKREF_OBJECTS: RefCell<FxHashMap<usize, Vec<WeakObjectEntry>>> =
        RefCell::new(FxHashMap::default());
    static CYCLE_CLEARED_OBJECTS: RefCell<FxHashSet<usize>> =
        RefCell::new(FxHashSet::default());
}

// Compile-time size check: ensure enum stays compact after boxing all >16-byte variants.
// Target: ≤24 bytes (down from 32). All variants must have data ≤ 16 bytes.
const _PAYLOAD_SIZE_CHECK: () = assert!(std::mem::size_of::<PyObjectPayload>() <= 24);

// ── PyObject Freelist Allocator ──
// Replaces Rc<PyObject> with a custom ref-counted pointer backed by a
// thread-local freelist. Eliminates malloc/free for hot object creation
// (CPython uses the same freelist strategy via pymalloc + per-type freelists).

/// Internal block layout for a reference-counted Python object.
/// Placed on the heap; recycled via thread-local freelist when ref count hits 0.
#[repr(C)]
struct PyObjectBlock {
    /// Strong reference count (u32 is sufficient for single-threaded interpreter).
    /// Special value IMMORTAL_REFCOUNT means the object is never freed.
    strong: Cell<u32>,
    /// Weak reference count (tracked for PyWeakRef support).
    weak: Cell<u32>,
    /// The Python object data (may be uninitialized after strong reaches 0 but weak > 0).
    obj: MaybeUninit<PyObject>,
}

/// Sentinel refcount for immortal objects (True, False, None, small ints).
/// Clone and Drop are no-ops when strong == IMMORTAL_REFCOUNT.
const IMMORTAL_REFCOUNT: u32 = u32::MAX;

/// Marker set on strong count during drop_in_place of the payload.
/// Prevents PyWeakRef::drop from recycling the block while the payload
/// is still being dropped (which would cause a double-free if the payload's
/// destructor drops the last weak ref to this block).
const DROPPING_REFCOUNT: u32 = u32::MAX - 1;

const SLAB_SIZE: usize = 128;

// Pool state: intrusive singly-linked freelist through freed blocks.
// When a block is free, its `obj` area stores a `*mut PyObjectBlock` next-pointer.
// Blocks are never individually deallocated — they're allocated as contiguous slabs
// and recycled indefinitely (matching CPython's obmalloc strategy).
// Thread-local so parallel tests (and future threading support) can each have
// their own independent pool without any locking overhead.
thread_local! {
    static POOL: std::cell::Cell<*mut PyObjectBlock> = std::cell::Cell::new(std::ptr::null_mut());
}

/// Read the next-free pointer from a freed block's obj area.
#[inline(always)]
unsafe fn free_next(block: *mut PyObjectBlock) -> *mut PyObjectBlock {
    *((*block).obj.as_ptr() as *const *mut PyObjectBlock)
}

/// Write the next-free pointer into a freed block's obj area.
#[inline(always)]
unsafe fn set_free_next(block: *mut PyObjectBlock, next: *mut PyObjectBlock) {
    *((*block).obj.as_mut_ptr() as *mut *mut PyObjectBlock) = next;
}

/// Allocate a contiguous slab of SLAB_SIZE blocks, return one, push rest into pool.
/// Contiguous allocation ensures adjacent blocks share cache lines, reducing
/// cache misses when popping from the freelist in hot loops.
#[cold]
#[inline(never)]
fn alloc_slab_and_pop() -> NonNull<PyObjectBlock> {
    let layout = std::alloc::Layout::array::<PyObjectBlock>(SLAB_SIZE).unwrap();
    let base = unsafe { std::alloc::alloc(layout) as *mut PyObjectBlock };
    assert!(!base.is_null(), "allocation failed");

    // Link blocks 1..SLAB_SIZE into freelist in forward order.
    // Block[1] becomes freelist head → next alloc gets the adjacent block
    // (same cache line or next cache line as block[0] which is returned).
    POOL.with(|pool| unsafe {
        let old_head = pool.get();
        // Link last block to existing freelist head
        let last = base.add(SLAB_SIZE - 1);
        (*last).strong = Cell::new(FREELIST_SENTINEL);
        (*last).weak = Cell::new(0); // init weak once — never reset after recycle
        set_free_next(last, old_head);
        // Link blocks in reverse so block[1] ends up at head
        for i in (1..SLAB_SIZE - 1).rev() {
            let block = base.add(i);
            (*block).strong = Cell::new(FREELIST_SENTINEL);
            (*block).weak = Cell::new(0);
            set_free_next(block, base.add(i + 1));
        }
        // block[1] is new freelist head
        let first_free = base.add(1);
        (*first_free).strong = Cell::new(FREELIST_SENTINEL);
        (*first_free).weak = Cell::new(0);
        set_free_next(first_free, base.add(2));
        pool.set(first_free);
    });

    // Initialize block[0] weak count (returned directly, not through freelist)
    unsafe {
        (*base).weak = Cell::new(0);
    }
    unsafe { NonNull::new_unchecked(base) }
}

/// Sentinel value written to strong count when a block is on the freelist.
/// Used to detect double-free and use-after-free in debug mode.
const FREELIST_SENTINEL: u32 = 0xDEAD_BEEF;

#[inline(always)]
fn pool_alloc(obj: PyObject) -> NonNull<PyObjectBlock> {
    let block = POOL.with(|pool| {
        let head = pool.get();
        if !head.is_null() {
            unsafe {
                pool.set(free_next(head));
                NonNull::new_unchecked(head)
            }
        } else {
            alloc_slab_and_pop()
        }
    });
    unsafe {
        let p = block.as_ptr();
        CYCLE_CLEARED_OBJECTS.with(|cleared| {
            cleared.borrow_mut().remove(&(p as usize));
        });
        (*p).strong = Cell::new(1);
        // weak is already 0: initialized in alloc_slab_and_pop, and blocks are
        // only recycled when weak==0 (Drop fast path), so no reset needed.
        (*p).obj.as_mut_ptr().write(obj);
    }
    block
}

#[inline(always)]
fn pool_recycle(block: NonNull<PyObjectBlock>) {
    // Use try_with to gracefully handle the case where POOL is being destroyed
    // (can happen when thread-local freelists are dropped during thread exit).
    // If POOL is unavailable, just leak the block (it will be freed by the OS on exit).
    let _ = POOL.try_with(|pool| unsafe {
        let old_head = pool.get();
        (*block.as_ptr()).strong = Cell::new(FREELIST_SENTINEL);
        set_free_next(block.as_ptr(), old_head);
        pool.set(block.as_ptr());
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
    fn live_weak_entries(target_ptr: usize) -> (Vec<WeakObjectEntry>, Vec<PyObjectRef>) {
        let entries = WEAKREF_OBJECTS.with(|registry| {
            registry
                .borrow()
                .get(&target_ptr)
                .cloned()
                .unwrap_or_default()
        });
        let mut live_entries = Vec::new();
        let mut live_objects = Vec::new();
        for entry in entries {
            if let Some(obj) = entry.weak_obj.upgrade() {
                live_objects.push(obj);
                live_entries.push(entry);
            }
        }
        let old_entries = WEAKREF_OBJECTS.with(|registry| {
            let mut registry = registry.borrow_mut();
            let old_entries = registry.remove(&target_ptr);
            if !live_entries.is_empty() {
                registry.insert(target_ptr, live_entries.clone());
            }
            old_entries
        });
        drop(old_entries);
        (live_entries, live_objects)
    }

    #[inline(always)]
    pub fn new(obj: PyObject) -> Self {
        Self(pool_alloc(obj))
    }

    /// Create an immortal object that is never freed.
    /// Used for singletons (True, False, None) and small int cache.
    #[inline(always)]
    pub fn new_immortal(obj: PyObject) -> Self {
        let layout = std::alloc::Layout::new::<PyObjectBlock>();
        let ptr = unsafe { std::alloc::alloc(layout) as *mut PyObjectBlock };
        let block = NonNull::new(ptr).expect("allocation failed");
        unsafe {
            let p = block.as_ptr();
            (*p).strong = Cell::new(IMMORTAL_REFCOUNT);
            (*p).weak = Cell::new(0);
            (*p).obj.as_mut_ptr().write(obj);
        }
        Self(block)
    }

    #[inline(always)]
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        a.0 == b.0
    }

    #[inline(always)]
    pub fn as_ptr(this: &Self) -> *const PyObject {
        unsafe { (*this.0.as_ptr()).obj.as_ptr() }
    }

    #[inline(always)]
    unsafe fn from_block_borrowed(block: NonNull<PyObjectBlock>) -> Self {
        Self(block)
    }

    #[inline(always)]
    pub fn strong_count(this: &Self) -> usize {
        unsafe { (*this.0.as_ptr()).strong.get() as usize }
    }

    /// Returns true if this object is immortal (refcount never changes).
    #[inline(always)]
    pub fn is_immortal(this: &Self) -> bool {
        unsafe { (*this.0.as_ptr()).strong.get() == IMMORTAL_REFCOUNT }
    }

    /// Promote an existing object to immortal status.
    /// After this call, clone/drop become no-ops for all references.
    /// Used for constants in code objects — they live as long as the program.
    #[inline(always)]
    pub fn make_immortal(this: &Self) {
        unsafe {
            (*this.0.as_ptr()).strong.set(IMMORTAL_REFCOUNT);
        }
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
        let target_ptr = Self::as_ptr(this) as usize;
        Self::live_weak_entries(target_ptr).0.len()
    }

    #[inline(always)]
    pub fn register_weak_object(
        target: &Self,
        weak_obj: &Self,
        callback: Option<PyObjectRef>,
        kind: WeakObjectKind,
    ) {
        let target_ptr = Self::as_ptr(target) as usize;
        let weak_ref = Self::downgrade(weak_obj);
        WEAKREF_OBJECTS.with(|registry| {
            registry
                .borrow_mut()
                .entry(target_ptr)
                .or_default()
                .push(WeakObjectEntry {
                    weak_obj: weak_ref,
                    callback,
                    kind,
                });
        });
    }

    #[inline(always)]
    pub fn find_shared_weak_object(target: &Self, kind: WeakObjectKind) -> Option<Self> {
        let target_ptr = Self::as_ptr(target) as usize;
        let (entries, objects) = Self::live_weak_entries(target_ptr);
        entries
            .iter()
            .zip(objects)
            .find(|(entry, _)| {
                entry.callback.is_none()
                    && matches!(
                        (&entry.kind, &kind),
                        (WeakObjectKind::Ref, WeakObjectKind::Ref)
                            | (WeakObjectKind::Proxy, WeakObjectKind::Proxy)
                    )
            })
            .map(|(_, obj)| obj)
    }

    #[inline(always)]
    pub fn weak_objects(this: &Self) -> Vec<Self> {
        let target_ptr = Self::as_ptr(this) as usize;
        Self::live_weak_entries(target_ptr).1
    }

    pub fn mark_cycle_cleared(this: &Self) {
        CYCLE_CLEARED_OBJECTS.with(|cleared| {
            cleared.borrow_mut().insert(Self::as_ptr(this) as usize);
        });
    }

    pub fn notify_cycle_collected_weakrefs(
        this: &Self,
        garbage_ptrs: &std::collections::HashSet<usize>,
    ) {
        let target_ptr = Self::as_ptr(this) as usize;
        let entries = WEAKREF_OBJECTS
            .try_with(|registry| registry.borrow_mut().remove(&target_ptr))
            .ok()
            .flatten();
        if let Some(entries) = entries {
            for entry in entries.into_iter().rev() {
                let Some(weak_obj) = entry.weak_obj.upgrade() else {
                    continue;
                };
                if garbage_ptrs.contains(&(Self::as_ptr(&weak_obj) as usize)) {
                    continue;
                }
                if let Some(callback) = entry.callback {
                    crate::error::request_vm_call(callback, vec![weak_obj]);
                }
            }
        }
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
            let val = c.get();
            // Immortal objects skip refcount increment entirely
            if val != IMMORTAL_REFCOUNT {
                c.set(val + 1);
            }
        }
        Self(self.0)
    }
}

impl Drop for PyObjectRef {
    #[inline(always)]
    fn drop(&mut self) {
        unsafe {
            let p = self.0.as_ptr();
            let strong = (*p).strong.get();
            // Immortal objects are never freed
            if strong == IMMORTAL_REFCOUNT {
                return;
            }
            if strong == 1 {
                let obj = &*(*p).obj.as_ptr();
                if let PyObjectPayload::Instance(inst) = &obj.payload {
                    if inst.finalizer_state.get() & 1 == 0 {
                        let borrowed = PyObjectRef::from_block_borrowed(self.0);
                        let del_fn = borrowed.get_attr("__del__");
                        std::mem::forget(borrowed);
                        if let Some(del_fn) = del_fn {
                            if (*p).strong.get() > strong {
                                inst.finalizer_state.set(inst.finalizer_state.get() | 1);
                                crate::error::request_pending_finalizer(del_fn);
                            }
                        }
                    }
                }
            }
            let new_strong = (*p).strong.get() - 1;
            (*p).strong.set(new_strong);
            if new_strong == 0 {
                let entries = WEAKREF_OBJECTS
                    .try_with(|registry| {
                        registry
                            .borrow_mut()
                            .remove(&(PyObjectRef::as_ptr(self) as usize))
                    })
                    .ok()
                    .flatten();
                if let Some(entries) = entries {
                    for entry in entries.into_iter().rev() {
                        if let Some(callback) = entry.callback {
                            if let Some(arg) = entry.weak_obj.upgrade() {
                                crate::error::request_vm_call(callback, vec![arg]);
                            }
                        }
                    }
                }
                // Fast path: when no weak refs exist, skip the DROPPING_REFCOUNT
                // guard entirely. DROPPING_REFCOUNT is only needed when drop_in_place
                // might trigger PyWeakRef::drop on a self-referencing weak ref, which
                // would see strong==0 && weak==0 and try to double-free this block.
                // Without weak refs, this can't happen.
                if (*p).weak.get() == 0 {
                    std::ptr::drop_in_place((*p).obj.as_mut_ptr());
                    pool_recycle(self.0);
                } else {
                    // Slow path: has weak refs — need DROPPING guard
                    (*p).strong.set(DROPPING_REFCOUNT);
                    std::ptr::drop_in_place((*p).obj.as_mut_ptr());
                    (*p).strong.set(0);
                    if (*p).weak.get() == 0 {
                        pool_recycle(self.0);
                    }
                }
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
            let is_cycle_cleared = CYCLE_CLEARED_OBJECTS
                .with(|cleared| cleared.borrow().contains(&(self.0.as_ptr() as usize)));
            // Only upgrade if strong > 0 and not in a special state
            // (DROPPING_REFCOUNT means payload is being destroyed)
            let alive = if !is_cycle_cleared && s > 0 && s < DROPPING_REFCOUNT {
                let obj = &*(*p).obj.as_ptr();
                !matches!(
                    &obj.payload,
                    PyObjectPayload::Instance(inst) if inst.finalizer_state.get() & 2 != 0
                )
            } else {
                false
            };
            if alive {
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

/// Single-threaded i64 wrapper using Cell (no atomics needed under GIL).
#[repr(transparent)]
pub struct SyncI64(pub Cell<i64>);

impl SyncI64 {
    #[inline(always)]
    pub fn new(v: i64) -> Self {
        Self(Cell::new(v))
    }
    #[inline(always)]
    pub fn get(&self) -> i64 {
        self.0.get()
    }
    #[inline(always)]
    pub fn set(&self, v: i64) {
        self.0.set(v)
    }
}

impl Clone for SyncI64 {
    #[inline(always)]
    fn clone(&self) -> Self {
        Self::new(self.get())
    }
}

impl fmt::Debug for SyncI64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SyncI64({})", self.get())
    }
}

// Safety: SyncI64 is used inside PyObjectPayload which needs Send+Sync for static
// singletons. Under GIL semantics, Cell<i64> is safe (single-threaded access).
unsafe impl Send for SyncI64 {}
unsafe impl Sync for SyncI64 {}

/// Single-threaded usize wrapper using Cell (no atomics needed under GIL).
#[repr(transparent)]
pub struct SyncUsize(pub Cell<usize>);

impl SyncUsize {
    #[inline(always)]
    pub fn new(v: usize) -> Self {
        Self(Cell::new(v))
    }
    #[inline(always)]
    pub fn get(&self) -> usize {
        self.0.get()
    }
    #[inline(always)]
    pub fn set(&self, v: usize) {
        self.0.set(v)
    }
}

impl Clone for SyncUsize {
    #[inline(always)]
    fn clone(&self) -> Self {
        Self::new(self.get())
    }
}

impl fmt::Debug for SyncUsize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SyncUsize({})", self.get())
    }
}

unsafe impl Send for SyncUsize {}
unsafe impl Sync for SyncUsize {}

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
pub struct ExceptionInstanceData {
    pub kind: ExceptionKind,
    pub message: CompactString,
    pub args: Vec<PyObjectRef>,
    /// Lazy attrs — None until first write. Saves 1 Rc allocation per exception
    /// for the common case where exceptions are raised and caught without attr access.
    /// Wrapped in UnsafeCell for interior mutability (safe under GIL).
    pub attrs: UnsafeCell<Option<SharedFxAttrMap>>,
}

impl Clone for ExceptionInstanceData {
    fn clone(&self) -> Self {
        Self::new_attrs(
            self.kind,
            self.message.clone(),
            self.args.clone(),
            self.get_attrs().cloned(),
        )
    }
}

impl std::fmt::Debug for ExceptionInstanceData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExceptionInstanceData")
            .field("kind", &self.kind)
            .field("message", &self.message)
            .field("args", &self.args)
            .finish()
    }
}

impl ExceptionInstanceData {
    /// Build with an initial attrs value (None for most exceptions).
    #[inline]
    pub fn new_attrs(
        kind: ExceptionKind,
        message: CompactString,
        args: Vec<PyObjectRef>,
        attrs: Option<SharedFxAttrMap>,
    ) -> Self {
        Self {
            kind,
            message,
            args,
            attrs: UnsafeCell::new(attrs),
        }
    }

    /// Get attrs for reading. Returns None if no attrs have been set.
    #[inline]
    pub fn get_attrs(&self) -> Option<&SharedFxAttrMap> {
        // SAFETY: Single-threaded under GIL.
        unsafe { &*self.attrs.get() }.as_ref()
    }

    /// Get or create attrs for writing. Uses interior mutability (safe under GIL).
    #[inline]
    pub fn ensure_attrs(&self) -> &SharedFxAttrMap {
        // SAFETY: Single-threaded under GIL. UnsafeCell provides the interior-mutability
        // contract that &self here does not promise immutability.
        let ptr = self.attrs.get();
        unsafe { (*ptr).get_or_insert_with(|| Rc::new(PyCell::new(FxAttrMap::default()))) }
    }
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
    pub pickle_args: Option<Vec<PyObjectRef>>,
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
    pub doc: PyCell<Option<PyObjectRef>>,
    pub doc_from_getter: Cell<bool>,
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

/// Boxed VecIter data — moved out of enum to shrink PyObjectPayload from 40→32 bytes.
/// VecIter is used for dict-key/set/bytes iteration (materialized snapshot).
#[derive(Clone, Debug)]
pub struct VecIterData {
    pub items: Vec<PyObjectRef>,
    pub index: SyncUsize,
}

#[derive(Clone, Debug)]
pub enum WeakValueIterKind {
    Keys,
    Values,
    Items,
}

#[derive(Clone, Debug)]
pub enum WeakKeyIterKind {
    Keys,
    Items,
}

#[derive(Clone, Debug)]
pub struct WeakValueIterData {
    pub entries: Vec<(PyObjectRef, PyObjectRef)>,
    pub index: SyncUsize,
    pub kind: WeakValueIterKind,
}

#[derive(Clone, Debug)]
pub struct WeakKeyIterData {
    pub entries: Vec<(PyObjectRef, PyObjectRef)>,
    pub index: SyncUsize,
    pub kind: WeakKeyIterKind,
}

#[derive(Clone, Debug)]
pub struct DequeIterData {
    pub source: PyObjectRef,
    pub index: SyncUsize,
    pub expected_len: usize,
    pub reverse: bool,
}

/// Boxed range data — moved out of enum to shrink PyObjectPayload from 32→24 bytes.
#[derive(Clone, Debug)]
pub struct RangeData {
    pub start: i64,
    pub stop: i64,
    pub step: i64,
    pub start_obj: Option<PyObjectRef>,
    pub stop_obj: Option<PyObjectRef>,
    pub step_obj: Option<PyObjectRef>,
}

/// Boxed range iterator data — moved out of enum to shrink PyObjectPayload from 32→24 bytes.
#[derive(Clone, Debug)]
pub struct RangeIterData {
    pub current: SyncI64,
    pub stop: i64,
    pub step: i64,
}

/// Big integer range iterator state for ranges that cannot be represented in i64.
#[derive(Clone, Debug)]
pub struct BigRangeIterData {
    pub start: BigInt,
    pub stop: BigInt,
    pub step: BigInt,
    pub index: BigInt,
}

/// The actual data of a Python value.
/// All variants ≤ 16 bytes of data so the enum (with tag) fits in 24 bytes.
#[derive(Clone)]
pub enum PyObjectPayload {
    None,
    Ellipsis,
    NotImplemented,
    Bool(bool),
    Int(PyInt),
    Float(f64),
    Complex {
        real: f64,
        imag: f64,
    },
    /// Short strings (≤15 bytes) stored inline; longer strings use Box<CompactString>.
    /// Eliminates 1 freelist alloc + 1 dealloc per short string (covers most identifiers/split parts).
    Str(StrRepr),
    /// Boxed to keep PyObjectPayload at 24 bytes (Vec is 24 bytes).
    Bytes(Box<Vec<u8>>),
    ByteArray(Box<Vec<u8>>),
    List(Box<PyCell<Vec<PyObjectRef>>>),
    Tuple(Box<Vec<PyObjectRef>>),
    Set(Rc<PyCell<FxHashKeyFlatMap>>),
    FrozenSet(Box<FrozenSetData>),
    Dict(Rc<PyCell<FxHashKeyMap>>),
    /// A dict that is a live view of an instance's __dict__ (shares backing store)
    InstanceDict(SharedFxAttrMap),
    /// Read-only view of a class namespace (types.MappingProxyType)
    MappingProxy(Rc<PyCell<FxHashKeyMap>>),
    Function(Box<PyFunction>),
    /// Boxed to keep PyObjectPayload at 24 bytes.
    BuiltinFunction(Box<CompactString>),
    /// Built-in type object (int, str, float, etc.) — callable as constructor.
    /// Boxed to keep PyObjectPayload at 24 bytes.
    BuiltinType(Box<CompactString>),
    BoundMethod {
        receiver: PyObjectRef,
        method: PyObjectRef,
    },
    BuiltinBoundMethod(Box<BuiltinBoundMethodData>),
    Code(std::rc::Rc<ferrython_bytecode::CodeObject>),
    Class(Box<ClassData>),
    /// ManuallyDrop enables recycling the Box through the instance freelist.
    Instance(ManuallyDrop<Box<InstanceData>>),
    Module(Box<ModuleData>),
    Iterator(Rc<PyCell<IteratorData>>),
    /// Lock-free range iterator — avoids Mutex overhead for `for i in range(n)`.
    /// Boxed to keep PyObjectPayload at 24 bytes.
    RangeIter(Box<RangeIterData>),
    /// Lock-free snapshot iterator — items immutable after creation, only index advances.
    /// Used for dict-key/set/bytes iteration where items must be materialized.
    /// Boxed to keep PyObjectPayload at 32 bytes (Vec + SyncUsize = 32 > 24 limit).
    VecIter(Box<VecIterData>),
    WeakValueIter(Box<WeakValueIterData>),
    WeakKeyIter(Box<WeakKeyIterData>),
    DequeIter(Box<DequeIterData>),
    /// Lazy reference iterator — holds a reference to the source container (list/tuple)
    /// and iterates by index without cloning elements upfront. Saves n Rc::clone at
    /// creation + n Rc::drop at destruction. CPython-style: just a pointer + position.
    RefIter {
        source: PyObjectRef,
        index: SyncUsize,
    },
    RevRefIter {
        source: PyObjectRef,
        index: SyncUsize,
    },
    Slice(Box<SliceData>),
    /// A cell object wrapping a shared mutable reference (for closures).
    Cell(Rc<PyCell<Option<PyObjectRef>>>),
    /// Exception type object (e.g. ValueError, TypeError)
    ExceptionType(ExceptionKind),
    /// Exception instance (raised exception with kind, message, and optional args).
    /// ManuallyDrop enables recycling the Box through the exception freelist
    /// without the compiler-generated drop trying to free the allocation.
    ExceptionInstance(ManuallyDrop<Box<ExceptionInstanceData>>),
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
        action: Box<AsyncGenAction>,
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
    Super {
        cls: PyObjectRef,
        instance: PyObjectRef,
    },
    /// Range object — preserves start/stop/step, creates fresh iterators.
    /// Boxed to keep PyObjectPayload at 24 bytes.
    Range(Box<RangeData>),
    /// Awaitable that immediately resolves to a pre-computed value.
    /// Used by asyncio.sleep(), asyncio.gather(), etc. to return proper awaitables
    /// from native functions that don't have their own coroutine frame.
    BuiltinAwaitable(PyObjectRef),
    /// Deferred sleep awaitable — carries sleep duration (secs) and result value.
    /// The actual thread::sleep happens when the VM drives this in YIELD_FROM,
    /// allowing asyncio.wait_for to enforce timeouts via a deadline.
    DeferredSleep {
        secs: f64,
        result: PyObjectRef,
    },
    /// Dict view objects — live views backed by the underlying dict storage.
    /// `owner` keeps dict subclasses alive for CPython-style view/iterator lifetime.
    DictKeys {
        map: Rc<PyCell<FxHashKeyMap>>,
        owner: Option<PyObjectRef>,
    },
    DictValues {
        map: Rc<PyCell<FxHashKeyMap>>,
        owner: Option<PyObjectRef>,
    },
    DictItems {
        map: Rc<PyCell<FxHashKeyMap>>,
        owner: Option<PyObjectRef>,
    },
}

impl Drop for PyObjectPayload {
    #[inline]
    fn drop(&mut self) {
        match self {
            PyObjectPayload::Dict(rc) => {
                super::constructors::try_recycle_map(rc);
            }
            PyObjectPayload::Set(_) => {
                // HashMap-based set — no freelist recycling (cheap to drop)
            }
            PyObjectPayload::ExceptionInstance(data) => {
                let taken = unsafe { ManuallyDrop::take(data) };
                super::constructors::recycle_exception_box(taken);
            }
            PyObjectPayload::Instance(data) => {
                let taken = unsafe { ManuallyDrop::take(data) };
                super::constructors::recycle_instance_box(taken);
            }
            // Recycle boxed allocations to typed freelists — avoids malloc/free
            // for the hottest allocation paths.
            // std::mem::replace swaps variant to None so compiler's drop-glue is no-op.
            // ptr::read extracts the inner Box; forget prevents old's Drop from running.
            // Note: Str uses StrRepr with its own Drop (recycles heap Box, no-op for inline).
            PyObjectPayload::Tuple(_)
            | PyObjectPayload::List(_)
            | PyObjectPayload::BuiltinBoundMethod(_) => {
                let old = std::mem::replace(self, PyObjectPayload::None);
                unsafe {
                    match &old {
                        PyObjectPayload::Tuple(b) => {
                            super::constructors::recycle_tuple_box(std::ptr::read(b as *const _));
                        }
                        PyObjectPayload::List(b) => {
                            super::constructors::recycle_list_box(std::ptr::read(b as *const _));
                        }
                        PyObjectPayload::BuiltinBoundMethod(b) => {
                            super::constructors::recycle_bbm_box(std::ptr::read(b as *const _));
                        }
                        _ => std::hint::unreachable_unchecked(),
                    }
                    std::mem::forget(old);
                }
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
            Self::RangeIter(ri) => write!(
                f,
                "RangeIter({}, {}, {})",
                ri.current.get(),
                ri.stop,
                ri.step
            ),
            Self::VecIter(data) => write!(f, "VecIter({}/{})", data.index.get(), data.items.len()),
            Self::WeakValueIter(data) => {
                write!(
                    f,
                    "WeakValueIter({}/{})",
                    data.index.get(),
                    data.entries.len()
                )
            }
            Self::WeakKeyIter(data) => {
                write!(
                    f,
                    "WeakKeyIter({}/{})",
                    data.index.get(),
                    data.entries.len()
                )
            }
            Self::DequeIter(data) => write!(f, "DequeIter({})", data.index.get()),
            Self::RefIter { index, .. } => write!(f, "RefIter({})", index.get()),
            Self::RevRefIter { index, .. } => write!(f, "RevRefIter({})", index.get()),
            Self::Slice(_) => write!(f, "Slice(...)"),
            Self::Cell(_) => write!(f, "Cell(...)"),
            Self::ExceptionType(k) => write!(f, "ExceptionType({k:?})"),
            Self::ExceptionInstance(ei) => {
                write!(f, "ExceptionInstance({:?}, {:?})", ei.kind, ei.message)
            }
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
            Self::Range(rd) => write!(f, "Range({}, {}, {})", rd.start, rd.stop, rd.step),
            Self::BuiltinAwaitable(_) => write!(f, "BuiltinAwaitable(...)"),
            Self::DeferredSleep { secs, .. } => write!(f, "DeferredSleep({secs}s)"),
            Self::DictKeys { .. } => write!(f, "dict_keys(...)"),
            Self::DictValues { .. } => write!(f, "dict_values(...)"),
            Self::DictItems { .. } => write!(f, "dict_items(...)"),
        }
    }
}

/// Opaque generator state. The frame is stored as a raw pointer to a
/// heap-allocated Frame (owned, not reference-counted). The VM crate
/// casts to/from `*mut Frame` directly — no `dyn Any` downcast overhead.
pub struct GeneratorState {
    pub name: CompactString,
    /// Raw pointer to a heap-allocated Frame. Null when no frame is stored.
    /// SAFETY: owned exclusively by this GeneratorState; freed on drop.
    pub frame_ptr: *mut u8,
    pub started: bool,
    pub finished: bool,
}

impl GeneratorState {
    /// Returns true if a suspended frame is available.
    #[inline(always)]
    pub fn has_frame(&self) -> bool {
        !self.frame_ptr.is_null()
    }
    /// Takes the frame pointer out, leaving null.
    #[inline(always)]
    pub fn take_frame_ptr(&mut self) -> *mut u8 {
        let p = self.frame_ptr;
        self.frame_ptr = std::ptr::null_mut();
        p
    }
    /// Stores a frame pointer.
    #[inline(always)]
    pub fn set_frame_ptr(&mut self, p: *mut u8) {
        self.frame_ptr = p;
    }
    /// Clears the frame pointer (e.g., on generator finish).
    #[inline(always)]
    pub fn clear_frame(&mut self) {
        self.frame_ptr = std::ptr::null_mut();
    }
}

/// Global callback registered by the VM crate to drop generator frames.
/// The core crate doesn't know the concrete Frame type, so the VM registers
/// a cleanup function at startup.
static mut GEN_FRAME_DROP_FN: Option<fn(*mut u8)> = None;

/// Register the generator frame drop function (called once by VM init).
pub fn register_gen_frame_drop(f: fn(*mut u8)) {
    unsafe {
        GEN_FRAME_DROP_FN = Some(f);
    }
}

impl Drop for GeneratorState {
    fn drop(&mut self) {
        if !self.frame_ptr.is_null() {
            if let Some(drop_fn) = unsafe { GEN_FRAME_DROP_FN } {
                drop_fn(self.frame_ptr);
            }
            self.frame_ptr = std::ptr::null_mut();
        }
    }
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
        Self {
            name: self.name.clone(),
            frame_ptr: std::ptr::null_mut(),
            started: self.started,
            finished: self.finished,
        }
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
pub struct InstanceData {
    pub class: PyObjectRef,
    pub attrs: SharedFxAttrMap,
    /// Internal dict storage for dict subclasses
    pub dict_storage: Option<Rc<PyCell<FxHashKeyMap>>>,
    /// Fast-path flag: true if this instance has special markers (__namedtuple__, __deque__, etc.)
    /// When true, LoadMethod uses the full get_attr path.
    pub is_special: bool,
    /// Cached class flags — avoids dereferencing inst.class on every LoadAttr/StoreAttr.
    /// Bit layout: see CLASS_FLAG_* constants.
    pub class_flags: u8,
    /// Bit 0 = finalizer queued or already run, bit 1 = cleared by cycle GC.
    pub finalizer_state: Cell<u8>,
}

// Bit flags for InstanceData.class_flags (cached from ClassData at instance creation)
pub const CLASS_FLAG_HAS_GETATTRIBUTE: u8 = 1 << 0;
pub const CLASS_FLAG_HAS_DESCRIPTORS: u8 = 1 << 1;
pub const CLASS_FLAG_HAS_SETATTR: u8 = 1 << 2;
pub const CLASS_FLAG_HAS_SLOTS: u8 = 1 << 3;
pub const CLASS_FLAG_HAS_GETATTR: u8 = 1 << 4;

impl InstanceData {
    /// Compute class_flags from a class PyObjectRef.
    #[inline]
    pub fn compute_flags(class: &PyObjectRef) -> u8 {
        if let PyObjectPayload::Class(cd) = &class.payload {
            let mut f = 0u8;
            if cd.has_getattribute {
                f |= CLASS_FLAG_HAS_GETATTRIBUTE;
            }
            if cd.has_descriptors {
                f |= CLASS_FLAG_HAS_DESCRIPTORS;
            }
            if cd.has_setattr {
                f |= CLASS_FLAG_HAS_SETATTR;
            }
            if cd.slots.is_some() {
                f |= CLASS_FLAG_HAS_SLOTS;
            }
            if cd.has_getattr {
                f |= CLASS_FLAG_HAS_GETATTR;
            }
            f
        } else {
            // Not a class — set all flags to force slow path
            0xFF
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModuleData {
    pub name: CompactString,
    pub attrs: Rc<PyCell<FxAttrMap>>,
}

#[derive(Debug, Clone)]
pub enum IteratorData {
    List {
        items: Vec<PyObjectRef>,
        index: usize,
    },
    Tuple {
        items: Vec<PyObjectRef>,
        index: usize,
    },
    Range {
        current: i64,
        stop: i64,
        step: i64,
    },
    BigRange(BigRangeIterData),
    Str {
        chars: Vec<char>,
        index: usize,
    },
    Enumerate {
        source: PyObjectRef,
        index: i64,
        cached_tuple: Option<PyObjectRef>,
    },
    Zip {
        sources: Vec<PyObjectRef>,
        strict: bool,
        cached_tuple: Option<PyObjectRef>,
        items_buf: Vec<PyObjectRef>,
    },
    ZipLongest {
        sources: Vec<PyObjectRef>,
        active: Vec<bool>,
        fillvalue: PyObjectRef,
        cached_tuple: Option<PyObjectRef>,
    },
    Islice {
        source: PyObjectRef,
        index: usize,
        next_yield: usize,
        stop: usize,
        step: usize,
    },
    MapOne {
        func: PyObjectRef,
        source: PyObjectRef,
    },
    Map {
        func: PyObjectRef,
        /// One or more source iterators (multi-arg map is lazy, not eagerly zipped).
        sources: Vec<PyObjectRef>,
    },
    Filter {
        func: PyObjectRef,
        source: PyObjectRef,
    },
    FilterFalse {
        func: PyObjectRef,
        source: PyObjectRef,
    },
    Sentinel {
        callable: PyObjectRef,
        sentinel: PyObjectRef,
        done: bool,
    },
    TakeWhile {
        func: PyObjectRef,
        source: PyObjectRef,
        done: bool,
    },
    DropWhile {
        func: PyObjectRef,
        source: PyObjectRef,
        dropping: bool,
    },
    /// Lazy sequence-protocol iterator (old-style __getitem__(0),__getitem__(1),... iter)
    SeqIter {
        obj: PyObjectRef,
        index: i64,
        exhausted: bool,
    },
    /// Infinite counter: count(start, step)
    Count {
        current: i64,
        step: i64,
    },
    /// Infinite cycle over cached items
    Cycle {
        items: Vec<PyObjectRef>,
        index: usize,
    },
    /// Repeat item n times (None = infinite)
    Repeat {
        item: PyObjectRef,
        remaining: Option<usize>,
    },
    /// Chain multiple iterators sequentially
    Chain {
        sources: Vec<PyObjectRef>,
        current: usize,
    },
    /// Starmap: apply func to each tuple of args
    Starmap {
        func: PyObjectRef,
        source: PyObjectRef,
    },
    /// Tee: one leg of a tee() split.
    /// All legs share the same source iterator and buffer; each leg has its own index.
    Tee {
        /// Shared underlying source iterator.
        source: Rc<PyCell<PyObjectRef>>,
        /// Shared buffer of items already pulled from the source.
        buffer: Rc<PyCell<Vec<PyObjectRef>>>,
        /// Shared guard that detects recursive pulls from the underlying source.
        active: Rc<Cell<bool>>,
        /// This leg's current position in the buffer.
        index: usize,
    },
    /// Lazy dict entries iteration: stores reference to dict, iterates by index.
    /// Uses cached_tuple reuse (CPython-style) for (key, value) pairs.
    DictEntries {
        source: Rc<PyCell<FxHashKeyMap>>,
        owner: Option<PyObjectRef>,
        index: usize,
        cached_tuple: Option<PyObjectRef>,
    },
    /// Snapshot dict keys iteration — converts keys eagerly at iterator creation.
    /// Trades upfront Vec<PyObjectRef> for cache-friendly, branch-free iteration.
    DictKeys {
        keys: Vec<PyObjectRef>,
        index: usize,
    },
    /// Wraps an iterator while keeping the original iterable alive until exhaustion.
    HeldIter {
        iter: PyObjectRef,
        owner: Option<PyObjectRef>,
    },
}
