//! Singleton values and PyObject factory/constructor methods.

use std::rc::Rc;
use std::cell::UnsafeCell;
use std::mem::ManuallyDrop;
use crate::error::{ExceptionKind, PyResult};
use crate::types::{HashableKey, PyFunction, PyInt};
use compact_str::CompactString;
use indexmap::IndexMap;
use num_bigint::BigInt;

use super::payload::*;
use super::methods::PyObjectMethods;

// ── Dict/Set inner Rc freelist (like CPython's dict_freelist) ──
// When a Dict or Set is dropped and holds the sole reference to its inner
// Rc<PyCell<FxHashKeyMap>>, we recycle it instead of freeing. On next
// dict/set creation, we pop from the freelist (clear + reuse), avoiding malloc.
// SAFETY: Single-threaded interpreter (GIL) — no concurrent access to TLS.
const MAP_FREELIST_MAX: usize = 80; // CPython uses 80 for dicts
thread_local! {
    static MAP_FREELIST: UnsafeCell<Vec<Rc<PyCell<FxHashKeyMap>>>> =
        UnsafeCell::new(Vec::with_capacity(MAP_FREELIST_MAX));
}

// ── Exception instance freelist ──
// Recycle Box<ExceptionInstanceData> to avoid malloc/free per raise/catch cycle.
// SAFETY: Single-threaded interpreter (GIL) — no concurrent access to TLS.
const EXCEPTION_FREELIST_MAX: usize = 16;
thread_local! {
    static EXCEPTION_FREELIST: UnsafeCell<Vec<Box<ExceptionInstanceData>>> =
        UnsafeCell::new(Vec::with_capacity(EXCEPTION_FREELIST_MAX));
}

// ── Instance freelist ──
// Recycle Box<InstanceData> + its inner Rc<PyCell<FxAttrMap>> to avoid 2 mallocs per instance.
// SAFETY: Single-threaded interpreter (GIL) — no concurrent access to TLS.
const INSTANCE_FREELIST_MAX: usize = 80;
thread_local! {
    static INSTANCE_FREELIST: UnsafeCell<Vec<Box<InstanceData>>> =
        UnsafeCell::new(Vec::with_capacity(INSTANCE_FREELIST_MAX));
}

// ── Attr map freelist ──
// Recycle Rc<PyCell<FxAttrMap>> for instance attrs.
const ATTR_FREELIST_MAX: usize = 80;
thread_local! {
    static ATTR_FREELIST: UnsafeCell<Vec<SharedFxAttrMap>> =
        UnsafeCell::new(Vec::with_capacity(ATTR_FREELIST_MAX));
}

// ── String Box freelist ──
// Recycle Box<CompactString> to avoid malloc/free per string creation/destruction.
// str_split creates 6+ short strings per call — this eliminates the Box alloc overhead.
// SAFETY: Single-threaded interpreter (GIL) — no concurrent access.
const STR_BOX_FREELIST_MAX: usize = 128;
struct StrBoxHolder(UnsafeCell<Vec<Box<CompactString>>>);
unsafe impl Sync for StrBoxHolder {}
static STR_BOX_FREELIST: StrBoxHolder = StrBoxHolder(UnsafeCell::new(Vec::new()));

/// Allocate a Box<CompactString>, reusing from freelist if possible.
#[inline(always)]
pub fn alloc_str_box(s: CompactString) -> Box<CompactString> {
    unsafe {
        let list = &mut *STR_BOX_FREELIST.0.get();
        if let Some(mut b) = list.pop() {
            *b = s;
            b
        } else {
            Box::new(s)
        }
    }
}

/// Return a Box<CompactString> to the freelist.
#[inline(always)]
pub(crate) fn recycle_str_box(mut b: Box<CompactString>) {
    // Clear to release any heap-allocated string data
    b.clear();
    unsafe {
        let list = &mut *STR_BOX_FREELIST.0.get();
        if list.len() < STR_BOX_FREELIST_MAX {
            list.push(b);
        }
    }
}

// ── Tuple Box freelist ──
// Recycle Box<Vec<PyObjectRef>> for tuple creation/destruction.
const TUPLE_BOX_FREELIST_MAX: usize = 80;
struct TupleBoxHolder(UnsafeCell<Vec<Box<Vec<PyObjectRef>>>>);
unsafe impl Sync for TupleBoxHolder {}
static TUPLE_BOX_FREELIST: TupleBoxHolder = TupleBoxHolder(UnsafeCell::new(Vec::new()));

/// Allocate a Box<Vec<PyObjectRef>> for Tuple, reusing from freelist if possible.
#[inline(always)]
pub fn alloc_tuple_box(items: Vec<PyObjectRef>) -> Box<Vec<PyObjectRef>> {
    unsafe {
        let list = &mut *TUPLE_BOX_FREELIST.0.get();
        if let Some(mut b) = list.pop() {
            *b = items;
            b
        } else {
            Box::new(items)
        }
    }
}

/// Return a Box<Vec<PyObjectRef>> to the freelist.
#[inline(always)]
pub(crate) fn recycle_tuple_box(mut b: Box<Vec<PyObjectRef>>) {
    b.clear();
    unsafe {
        let list = &mut *TUPLE_BOX_FREELIST.0.get();
        if list.len() < TUPLE_BOX_FREELIST_MAX {
            list.push(b);
        }
    }
}

// ── List Box freelist ──
// Recycle Box<PyCell<Vec<PyObjectRef>>> for list creation/destruction.
const LIST_BOX_FREELIST_MAX: usize = 80;
struct ListBoxHolder(UnsafeCell<Vec<Box<PyCell<Vec<PyObjectRef>>>>>);
unsafe impl Sync for ListBoxHolder {}
static LIST_BOX_FREELIST: ListBoxHolder = ListBoxHolder(UnsafeCell::new(Vec::new()));

/// Allocate a Box<PyCell<Vec<PyObjectRef>>> for List, reusing from freelist if possible.
#[inline(always)]
pub fn alloc_list_box(items: Vec<PyObjectRef>) -> Box<PyCell<Vec<PyObjectRef>>> {
    unsafe {
        let list = &mut *LIST_BOX_FREELIST.0.get();
        if let Some(b) = list.pop() {
            *b.data_ptr() = items;
            b
        } else {
            Box::new(PyCell::new(items))
        }
    }
}

/// Return a Box<PyCell<Vec<PyObjectRef>>> to the freelist.
#[inline(always)]
pub(crate) fn recycle_list_box(b: Box<PyCell<Vec<PyObjectRef>>>) {
    unsafe { (*b.data_ptr()).clear(); }
    unsafe {
        let list = &mut *LIST_BOX_FREELIST.0.get();
        if list.len() < LIST_BOX_FREELIST_MAX {
            list.push(b);
        }
    }
}

// ── BuiltinBoundMethod Box freelist ──
// Recycle Box<BuiltinBoundMethodData> for builtin method calls (str.replace, list.append, etc.)
const BBM_BOX_FREELIST_MAX: usize = 64;
struct BbmBoxHolder(UnsafeCell<Vec<Box<BuiltinBoundMethodData>>>);
unsafe impl Sync for BbmBoxHolder {}
static BBM_BOX_FREELIST: BbmBoxHolder = BbmBoxHolder(UnsafeCell::new(Vec::new()));

/// Allocate a Box<BuiltinBoundMethodData>, reusing from freelist if possible.
#[inline(always)]
pub fn alloc_bbm_box(receiver: PyObjectRef, method_name: CompactString) -> Box<BuiltinBoundMethodData> {
    unsafe {
        let list = &mut *BBM_BOX_FREELIST.0.get();
        if let Some(mut b) = list.pop() {
            b.receiver = receiver;
            b.method_name = method_name;
            b
        } else {
            Box::new(BuiltinBoundMethodData { receiver, method_name })
        }
    }
}

/// Return a Box<BuiltinBoundMethodData> to the freelist.
#[inline(always)]
pub(crate) fn recycle_bbm_box(mut b: Box<BuiltinBoundMethodData>) {
    // Drop receiver ref to prevent leaks
    b.receiver = PyObject::none();
    b.method_name = CompactString::new("");
    unsafe {
        let list = &mut *BBM_BOX_FREELIST.0.get();
        if list.len() < BBM_BOX_FREELIST_MAX {
            list.push(b);
        }
    }
}

/// Allocate an ExceptionInstanceData box, reusing from freelist if possible.
#[inline]
pub fn alloc_exception_box(
    kind: ExceptionKind,
    message: CompactString,
    args: Vec<PyObjectRef>,
) -> Box<ExceptionInstanceData> {
    EXCEPTION_FREELIST.with(|fl| {
        // SAFETY: single-threaded (GIL), no reentrant access during pop
        let list = unsafe { &mut *fl.get() };
        if let Some(mut data) = list.pop() {
            data.kind = kind;
            data.message = message;
            data.args = args;
            data.attrs = None;
            data
        } else {
            Box::new(ExceptionInstanceData {
                kind,
                message,
                args,
                attrs: None,
            })
        }
    })
}

/// Return an ExceptionInstanceData box to the freelist.
/// Clears inner references to avoid holding PyObjectRef alive.
#[inline]
pub(crate) fn recycle_exception_box(mut data: Box<ExceptionInstanceData>) {
    // Clear inner references BEFORE accessing the freelist.
    // Dropping PyObjectRefs can cascade into more exception drops.
    data.args.clear();
    data.attrs = None;
    data.message = CompactString::default();
    EXCEPTION_FREELIST.with(|fl| {
        // SAFETY: single-threaded (GIL), inner refs already cleared (no reentrant drops)
        let list = unsafe { &mut *fl.get() };
        if list.len() < EXCEPTION_FREELIST_MAX {
            list.push(data);
        }
    })
}

/// Allocate an InstanceData box, reusing from freelist if possible.
/// The returned InstanceData has a recycled attrs map with cleared entries but retained capacity.
#[inline]
pub fn alloc_instance_box(class: PyObjectRef, class_flags: u8, dict_storage: Option<Rc<PyCell<FxHashKeyMap>>>, expected_attrs: usize) -> Box<InstanceData> {
    INSTANCE_FREELIST.with(|fl| {
        let list = unsafe { &mut *fl.get() };
        if let Some(mut data) = list.pop() {
            data.class = class;
            data.class_flags = class_flags;
            data.dict_storage = dict_storage;
            data.is_special = false;
            // Check if attrs Rc is uniquely owned — if shared (e.g., __dict__ created
            // an InstanceDict), we must not clear it; allocate a fresh one instead.
            if Rc::strong_count(&data.attrs) == 1 {
                unsafe { &mut *data.attrs.data_ptr() }.clear();
            } else {
                data.attrs = alloc_attr_map();
            }
            data
        } else {
            let attrs: FxAttrMap = if expected_attrs > 0 {
                FxAttrMap::with_capacity_and_hasher(expected_attrs, Default::default())
            } else {
                FxAttrMap::default()
            };
            Box::new(InstanceData {
                class,
                attrs: alloc_attr_map_with(attrs),
                dict_storage,
                is_special: false,
                class_flags,
            })
        }
    })
}

/// Return an InstanceData box to the freelist.
/// Clears inner references to avoid holding PyObjectRef alive.
#[inline]
pub(crate) fn recycle_instance_box(mut data: Box<InstanceData>) {
    // Clear attrs: if uniquely owned, clear entries (retain capacity); else drop reference.
    if Rc::strong_count(&data.attrs) == 1 {
        unsafe { &mut *data.attrs.data_ptr() }.clear();
    } else {
        // Someone else (e.g., InstanceDict from __dict__) holds a reference.
        // Replace with a fresh empty Rc so they keep their data.
        data.attrs = alloc_attr_map();
    }
    data.dict_storage = None;
    let old_class = std::mem::replace(&mut data.class, NONE_SINGLETON.clone());
    drop(old_class);
    INSTANCE_FREELIST.with(|fl| {
        let list = unsafe { &mut *fl.get() };
        if list.len() < INSTANCE_FREELIST_MAX {
            list.push(data);
        }
    })
}

/// Allocate an attr map (Rc<PyCell<FxAttrMap>>), reusing from freelist if possible.
#[inline]
pub fn alloc_attr_map() -> SharedFxAttrMap {
    ATTR_FREELIST.with(|fl| {
        let list = unsafe { &mut *fl.get() };
        if let Some(rc) = list.pop() {
            rc
        } else {
            Rc::new(PyCell::new(FxAttrMap::default()))
        }
    })
}

/// Allocate an attr map with initial data.
#[inline]
fn alloc_attr_map_with(attrs: FxAttrMap) -> SharedFxAttrMap {
    ATTR_FREELIST.with(|fl| {
        let list = unsafe { &mut *fl.get() };
        if let Some(rc) = list.pop() {
            // Replace contents with new data (retaining allocation if capacity fits)
            let map = unsafe { &mut *rc.data_ptr() };
            *map = attrs;
            rc
        } else {
            Rc::new(PyCell::new(attrs))
        }
    })
}

/// Try to recycle an attr map back to the freelist (if uniquely owned).
#[inline]
pub(crate) fn try_recycle_attr_map(rc: &SharedFxAttrMap) -> bool {
    if Rc::strong_count(rc) == 1 {
        unsafe { &mut *rc.data_ptr() }.clear();
        ATTR_FREELIST.with(|fl| {
            let list = unsafe { &mut *fl.get() };
            if list.len() < ATTR_FREELIST_MAX {
                list.push(rc.clone());
                true
            } else {
                false
            }
        })
    } else {
        false
    }
}

/// Allocate an inner Rc<PyCell<FxHashKeyMap>>, reusing from freelist if possible.
#[inline]
pub fn alloc_map_inner() -> Rc<PyCell<FxHashKeyMap>> {
    MAP_FREELIST.with(|fl| {
        // SAFETY: single-threaded (GIL), no reentrant access during pop
        let list = unsafe { &mut *fl.get() };
        if let Some(rc) = list.pop() {
            rc
        } else {
            Rc::new(PyCell::new(new_fx_hashkey_map()))
        }
    })
}

/// Return an inner Rc<PyCell<FxHashKeyMap>> to the freelist if it's uniquely owned.
/// Returns true if successfully recycled (caller should NOT drop it normally).
#[inline]
pub(crate) fn try_recycle_map(rc: &mut Rc<PyCell<FxHashKeyMap>>) -> bool {
    if Rc::strong_count(rc) == 1 {
        unsafe { &mut *rc.data_ptr() }.clear();
        MAP_FREELIST.with(|fl| {
            // SAFETY: single-threaded (GIL), map already cleared (no reentrant drops)
            let list = unsafe { &mut *fl.get() };
            if list.len() < MAP_FREELIST_MAX {
                list.push(rc.clone());
                true
            } else {
                false
            }
        })
    } else {
        false
    }
}

// ── Singletons ──
use std::sync::LazyLock;
static NONE_SINGLETON: LazyLock<PyObjectRef> = LazyLock::new(|| PyObjectRef::new_immortal(PyObject { payload: PyObjectPayload::None }));
static TRUE_SINGLETON: LazyLock<PyObjectRef> = LazyLock::new(|| PyObjectRef::new_immortal(PyObject { payload: PyObjectPayload::Bool(true) }));
static FALSE_SINGLETON: LazyLock<PyObjectRef> = LazyLock::new(|| PyObjectRef::new_immortal(PyObject { payload: PyObjectPayload::Bool(false) }));
static ELLIPSIS_SINGLETON: LazyLock<PyObjectRef> = LazyLock::new(|| PyObjectRef::new_immortal(PyObject { payload: PyObjectPayload::Ellipsis }));
static NOT_IMPLEMENTED_SINGLETON: LazyLock<PyObjectRef> = LazyLock::new(|| PyObjectRef::new_immortal(PyObject { payload: PyObjectPayload::NotImplemented }));

// ── Small-int cache (CPython caches -5..=256, we go wider for loop bounds) ──
const SMALL_INT_MIN: i64 = -5;
const SMALL_INT_MAX: i64 = 65536;

static SMALL_INT_CACHE: LazyLock<Vec<PyObjectRef>> = LazyLock::new(|| {
    (SMALL_INT_MIN..=SMALL_INT_MAX)
        .map(|n| PyObjectRef::new_immortal(PyObject { payload: PyObjectPayload::Int(PyInt::Small(n)) }))
        .collect()
});

// ── Float singleton cache for common values ──
static FLOAT_ZERO: LazyLock<PyObjectRef> = LazyLock::new(|| PyObjectRef::new_immortal(PyObject { payload: PyObjectPayload::Float(0.0) }));
static FLOAT_ONE: LazyLock<PyObjectRef> = LazyLock::new(|| PyObjectRef::new_immortal(PyObject { payload: PyObjectPayload::Float(1.0) }));
static FLOAT_NEG_ONE: LazyLock<PyObjectRef> = LazyLock::new(|| PyObjectRef::new_immortal(PyObject { payload: PyObjectPayload::Float(-1.0) }));

// ── Empty collection singletons ──
static EMPTY_TUPLE: LazyLock<PyObjectRef> = LazyLock::new(|| PyObjectRef::new_immortal(PyObject { payload: PyObjectPayload::Tuple(Box::new(vec![])) }));
static EMPTY_STR: LazyLock<PyObjectRef> = LazyLock::new(|| PyObjectRef::new_immortal(PyObject { payload: PyObjectPayload::Str(Box::new(CompactString::const_new(""))) }));
static EMPTY_BYTES: LazyLock<PyObjectRef> = LazyLock::new(|| PyObjectRef::new_immortal(PyObject { payload: PyObjectPayload::Bytes(Box::new(vec![])) }));

// ── GC Tracking for cycle-capable objects (Instance, Dict, List) ──
// Static UnsafeCell: no TLS overhead — single-threaded GIL interpreter.
struct TrackedHolder(std::cell::UnsafeCell<Vec<PyWeakRef>>);
unsafe impl Sync for TrackedHolder {}
static TRACKED_OBJECTS: TrackedHolder = TrackedHolder(std::cell::UnsafeCell::new(Vec::new()));

/// Register the cycle collector callback with the GC crate.
pub fn init_gc() {
    ferrython_gc::register_cycle_collector(run_cycle_collection);
}

/// Cycle collection: find objects that are only reachable through
/// other tracked objects (i.e., they form a reference cycle).
/// Covers Instance, Dict, and List objects.
///
/// Algorithm (trial deletion, simplified for Arc):
/// 1. Purge dead weak refs from TRACKED_OBJECTS
/// 2. For each live tracked object, count PyObjectRef::strong_count()
/// 3. Count how many references each tracked object receives from other tracked objects
/// 4. If strong_count == internal_refs, the object is only reachable from within cycles
/// 5. Clear contents on unreachable objects to break cycles (dropping internal refs)
fn run_cycle_collection() -> usize {
    unsafe {
        let tracked = &mut *TRACKED_OBJECTS.0.get();

        // 1. Upgrade weak refs, purge dead ones
        let alive: Vec<PyObjectRef> = tracked.iter()
            .filter_map(|w| w.upgrade())
            .collect();
        tracked.retain(|w| w.strong_count() > 0);

        if alive.is_empty() {
            return 0;
        }

        // 2. Build pointer → index map for fast lookup
        let ptr_map: std::collections::HashMap<usize, usize> = alive.iter()
            .enumerate()
            .map(|(i, obj)| (PyObjectRef::as_ptr(obj) as usize, i))
            .collect();

        // 3. Count internal references (refs from one tracked object to another)
        let mut internal_refs = vec![0usize; alive.len()];
        for obj in &alive {
            count_internal_refs(&obj.payload, &ptr_map, &mut internal_refs);
        }

        // 4. Trial deletion: objects where strong_count == internal_refs + 1
        // (+1 for our own `alive` Vec holding a ref)
        let mut garbage_indices: Vec<usize> = Vec::new();
        for (i, obj) in alive.iter().enumerate() {
            let strong = PyObjectRef::strong_count(obj);
            if strong <= internal_refs[i] + 1 {
                garbage_indices.push(i);
            }
        }

        // 5. Verify: all garbage objects must only reference other garbage objects
        // (conservative: only collect fully isolated cycles)
        let garbage_set: std::collections::HashSet<usize> = garbage_indices.iter().copied().collect();
        let mut confirmed_garbage: Vec<usize> = Vec::new();
        for &gi in &garbage_indices {
            let obj = &alive[gi];
            if verify_all_refs_in_garbage(&obj.payload, &ptr_map, &garbage_set) {
                confirmed_garbage.push(gi);
            }
        }

        // 6. Break cycles by clearing contents on garbage objects
        let collected = confirmed_garbage.len();
        for &gi in &confirmed_garbage {
            break_cycles(&alive[gi].payload);
        }

        collected
    }
}

/// Count references from a payload to other tracked objects.
fn count_internal_refs(
    payload: &PyObjectPayload,
    ptr_map: &std::collections::HashMap<usize, usize>,
    internal_refs: &mut [usize],
) {
    match payload {
        PyObjectPayload::Instance(inst) => {
            let attrs = inst.attrs.read();
            for attr_val in attrs.values() {
                let ptr = PyObjectRef::as_ptr(attr_val) as usize;
                if let Some(&target_idx) = ptr_map.get(&ptr) {
                    internal_refs[target_idx] += 1;
                }
            }
            let class_ptr = PyObjectRef::as_ptr(&inst.class) as usize;
            if let Some(&target_idx) = ptr_map.get(&class_ptr) {
                internal_refs[target_idx] += 1;
            }
        }
        PyObjectPayload::List(items) => {
            let items = items.read();
            for item in items.iter() {
                let ptr = PyObjectRef::as_ptr(item) as usize;
                if let Some(&target_idx) = ptr_map.get(&ptr) {
                    internal_refs[target_idx] += 1;
                }
            }
        }
        PyObjectPayload::Dict(map) => {
            let map = map.read();
            for val in map.values() {
                let ptr = PyObjectRef::as_ptr(val) as usize;
                if let Some(&target_idx) = ptr_map.get(&ptr) {
                    internal_refs[target_idx] += 1;
                }
            }
        }
        _ => {}
    }
}

/// Verify that all references from a payload point to objects in the garbage set.
fn verify_all_refs_in_garbage(
    payload: &PyObjectPayload,
    ptr_map: &std::collections::HashMap<usize, usize>,
    garbage_set: &std::collections::HashSet<usize>,
) -> bool {
    match payload {
        PyObjectPayload::Instance(inst) => {
            let attrs = inst.attrs.read();
            for attr_val in attrs.values() {
                let ptr = PyObjectRef::as_ptr(attr_val) as usize;
                if let Some(&target_idx) = ptr_map.get(&ptr) {
                    if !garbage_set.contains(&target_idx) {
                        return false;
                    }
                }
            }
        }
        PyObjectPayload::List(items) => {
            let items = items.read();
            for item in items.iter() {
                let ptr = PyObjectRef::as_ptr(item) as usize;
                if let Some(&target_idx) = ptr_map.get(&ptr) {
                    if !garbage_set.contains(&target_idx) {
                        return false;
                    }
                }
            }
        }
        PyObjectPayload::Dict(map) => {
            let map = map.read();
            for val in map.values() {
                let ptr = PyObjectRef::as_ptr(val) as usize;
                if let Some(&target_idx) = ptr_map.get(&ptr) {
                    if !garbage_set.contains(&target_idx) {
                        return false;
                    }
                }
            }
        }
        _ => {}
    }
    true
}

/// Break cycles by clearing contents of a garbage object.
fn break_cycles(payload: &PyObjectPayload) {
    match payload {
        PyObjectPayload::Instance(inst) => {
            inst.attrs.write().clear();
        }
        PyObjectPayload::List(items) => {
            items.write().clear();
        }
        PyObjectPayload::Dict(map) => {
            map.write().clear();
        }
        _ => {}
    }
}

fn track_object(obj: &PyObjectRef) {
    unsafe {
        (*TRACKED_OBJECTS.0.get()).push(PyObjectRef::downgrade(obj));
    }
}

// ── PyObject constructors ──

impl PyObject {
    #[inline]
    
    pub fn wrap(payload: PyObjectPayload) -> PyObjectRef {
        ferrython_gc::notify_alloc();
        PyObjectRef::new(PyObject { payload })
    }
    /// Like `wrap` but skips GC allocation tracking.
    /// Use for leaf types (Int, Float, Str, etc.) that cannot form reference cycles.
    #[inline(always)]
    pub fn wrap_leaf(payload: PyObjectPayload) -> PyObjectRef {
        PyObjectRef::new(PyObject { payload })
    }
    #[inline(always)]
    pub fn none() -> PyObjectRef { NONE_SINGLETON.clone() }
    #[inline(always)]
    pub fn ellipsis() -> PyObjectRef { ELLIPSIS_SINGLETON.clone() }
    #[inline(always)]
    pub fn not_implemented() -> PyObjectRef { NOT_IMPLEMENTED_SINGLETON.clone() }
    #[inline(always)]
    pub fn bool_val(v: bool) -> PyObjectRef { if v { TRUE_SINGLETON.clone() } else { FALSE_SINGLETON.clone() } }
    #[inline(always)]
    pub fn int(v: i64) -> PyObjectRef {
        if v >= SMALL_INT_MIN && v <= SMALL_INT_MAX {
            // SAFETY: bounds checked above
            unsafe { SMALL_INT_CACHE.get_unchecked((v - SMALL_INT_MIN) as usize).clone() }
        } else {
            Self::wrap_leaf(PyObjectPayload::Int(PyInt::Small(v)))
        }
    }
    /// Unchecked small-int lookup — caller guarantees SMALL_INT_MIN <= v <= SMALL_INT_MAX.
    #[inline(always)]
    pub unsafe fn int_cached_unchecked(v: i64) -> PyObjectRef {
        SMALL_INT_CACHE.get_unchecked((v - SMALL_INT_MIN) as usize).clone()
    }
    /// Returns the small int cache bounds (min, max inclusive).
    #[inline(always)]
    pub const fn small_int_range() -> (i64, i64) { (SMALL_INT_MIN, SMALL_INT_MAX) }
    pub fn big_int(v: BigInt) -> PyObjectRef { Self::wrap_leaf(PyObjectPayload::Int(PyInt::Big(Box::new(v)))) }
    #[inline(always)]
    pub fn float(v: f64) -> PyObjectRef {
        if v == 0.0 && !v.is_sign_negative() { return FLOAT_ZERO.clone(); }
        if v == 1.0 { return FLOAT_ONE.clone(); }
        if v == -1.0 { return FLOAT_NEG_ONE.clone(); }
        Self::wrap_leaf(PyObjectPayload::Float(v))
    }
    pub fn complex(real: f64, imag: f64) -> PyObjectRef { Self::wrap_leaf(PyObjectPayload::Complex { real, imag }) }
    #[inline]
    pub fn str_val(v: CompactString) -> PyObjectRef {
        if v.is_empty() { return EMPTY_STR.clone(); }
        Self::wrap_leaf(PyObjectPayload::Str(alloc_str_box(v)))
    }
    pub fn bytes(v: Vec<u8>) -> PyObjectRef {
        if v.is_empty() { return EMPTY_BYTES.clone(); }
        Self::wrap_leaf(PyObjectPayload::Bytes(Box::new(v)))
    }
    pub fn bytearray(v: Vec<u8>) -> PyObjectRef { Self::wrap_leaf(PyObjectPayload::ByteArray(Box::new(v))) }
    pub fn list(items: Vec<PyObjectRef>) -> PyObjectRef {
        let obj = Self::wrap(PyObjectPayload::List(alloc_list_box(items)));
        track_object(&obj);
        obj
    }
    /// Create a list of known leaf types (str, int, float, etc.) — skips GC tracking
    /// since leaf-only lists cannot form reference cycles.
    #[inline]
    pub fn list_leaf(items: Vec<PyObjectRef>) -> PyObjectRef {
        Self::wrap_leaf(PyObjectPayload::List(alloc_list_box(items)))
    }
    pub fn tuple(items: Vec<PyObjectRef>) -> PyObjectRef {
        if items.is_empty() { return EMPTY_TUPLE.clone(); }
        Self::wrap_leaf(PyObjectPayload::Tuple(alloc_tuple_box(items)))
    }
    pub fn set<S: std::hash::BuildHasher>(items: IndexMap<HashableKey, PyObjectRef, S>) -> PyObjectRef {
        if items.is_empty() {
            // Reuse from freelist
            let inner = alloc_map_inner();
            Self::wrap(PyObjectPayload::Set(inner))
        } else {
            let fx: FxHashKeyMap = items.into_iter().collect();
            Self::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(fx))))
        }
    }
    pub fn dict<S: std::hash::BuildHasher>(items: IndexMap<HashableKey, PyObjectRef, S>) -> PyObjectRef {
        let inner = if items.is_empty() {
            alloc_map_inner()
        } else {
            let fx: FxHashKeyMap = items.into_iter().collect();
            Rc::new(PyCell::new(fx))
        };
        let obj = Self::wrap(PyObjectPayload::Dict(inner));
        track_object(&obj);
        obj
    }
    /// Build dict directly from a FxHashKeyMap — avoids re-hashing from IndexMap.
    /// Reuses freelist Rc when available to avoid Rc+PyCell allocation.
    pub fn dict_fx(items: FxHashKeyMap) -> PyObjectRef {
        let inner = if items.is_empty() {
            alloc_map_inner()
        } else {
            // Try to reuse a freelist Rc (already cleared) — just write items into it
            MAP_FREELIST.with(|fl| {
                let list = unsafe { &mut *fl.get() };
                if let Some(rc) = list.pop() {
                    *rc.write() = items;
                    rc
                } else {
                    Rc::new(PyCell::new(items))
                }
            })
        };
        let obj = Self::wrap(PyObjectPayload::Dict(inner));
        track_object(&obj);
        obj
    }
    pub fn function(func: PyFunction) -> PyObjectRef { Self::wrap(PyObjectPayload::Function(Box::new(func))) }
    pub fn builtin_function(name: CompactString) -> PyObjectRef { Self::wrap(PyObjectPayload::BuiltinFunction(Box::new(name))) }
    pub fn builtin_type(name: CompactString) -> PyObjectRef { Self::wrap(PyObjectPayload::BuiltinType(Box::new(name))) }
    pub fn code(code: ferrython_bytecode::CodeObject) -> PyObjectRef { Self::wrap(PyObjectPayload::Code(std::rc::Rc::new(code))) }
    pub fn class(name: CompactString, bases: Vec<PyObjectRef>, namespace: IndexMap<CompactString, PyObjectRef>) -> PyObjectRef {
        let fx_ns: FxAttrMap = namespace.into_iter().collect();
        Self::wrap(PyObjectPayload::Class(Box::new(ClassData::new(name, bases, fx_ns, Vec::new(), None))))
    }
    pub fn instance(class: PyObjectRef) -> PyObjectRef {
        // Use cached flags from ClassData to avoid hierarchy traversal
        let (dict_storage, expected_attrs, class_flags) = if let PyObjectPayload::Class(cd) = &class.payload {
            let ds = if cd.is_dict_subclass {
                Some(alloc_map_inner())
            } else { None };
            (ds, cd.expected_attrs, cd.instance_flags)
        } else {
            (Self::detect_dict_subclass(&class), 0, 0xFF)
        };
        let data = alloc_instance_box(class, class_flags, dict_storage, expected_attrs);
        let obj = Self::wrap(PyObjectPayload::Instance(ManuallyDrop::new(data)));
        track_object(&obj);
        obj
    }
    pub fn instance_with_attrs(class: PyObjectRef, attrs: IndexMap<CompactString, PyObjectRef>) -> PyObjectRef {
        let dict_storage = if let PyObjectPayload::Class(cd) = &class.payload {
            if cd.is_dict_subclass {
                Some(alloc_map_inner())
            } else { None }
        } else {
            Self::detect_dict_subclass(&class)
        };
        let fx_attrs: FxAttrMap = attrs.into_iter().collect();
        let class_flags = InstanceData::compute_flags(&class);
        let obj = Self::wrap(PyObjectPayload::Instance(ManuallyDrop::new(Box::new(InstanceData { class, attrs: Rc::new(PyCell::new(fx_attrs)), dict_storage, is_special: false, class_flags }))));
        track_object(&obj);
        obj
    }

    /// Check if a class inherits from dict and return dict storage if so
    fn detect_dict_subclass(class: &PyObjectRef) -> Option<Rc<PyCell<FxHashKeyMap>>> {
        if let PyObjectPayload::Class(cd) = &class.payload {
            for base in &cd.bases {
                let is_dict = match &base.payload {
                    PyObjectPayload::BuiltinType(n) => n.as_str() == "dict",
                    PyObjectPayload::Class(bcd) => bcd.name.as_str() == "dict",
                    _ => false,
                };
                if is_dict {
                    return Some(alloc_map_inner());
                }
                // Recurse into base classes
                if let Some(storage) = Self::detect_dict_subclass(base) {
                    drop(storage); // We create fresh storage for each instance
                    return Some(alloc_map_inner());
                }
            }
        }
        None
    }
    pub fn module(name: CompactString) -> PyObjectRef {
        let mut attrs = FxAttrMap::default();
        attrs.insert(CompactString::from("__name__"), PyObject::str_val(name.clone()));
        attrs.insert(CompactString::from("__loader__"), PyObject::none());
        attrs.insert(CompactString::from("__spec__"), PyObject::none());
        attrs.insert(CompactString::from("__package__"), PyObject::none());
        Self::wrap(PyObjectPayload::Module(Box::new(ModuleData { name, attrs: Rc::new(PyCell::new(attrs)) })))
    }
    pub fn module_with_attrs(name: CompactString, attrs: IndexMap<CompactString, PyObjectRef>) -> PyObjectRef {
        let mut fx_attrs: FxAttrMap = attrs.into_iter().collect();
        if !fx_attrs.contains_key("__name__") {
            fx_attrs.insert(CompactString::from("__name__"), PyObject::str_val(name.clone()));
        }
        if !fx_attrs.contains_key("__loader__") {
            fx_attrs.insert(CompactString::from("__loader__"), PyObject::none());
        }
        if !fx_attrs.contains_key("__spec__") {
            fx_attrs.insert(CompactString::from("__spec__"), PyObject::none());
        }
        if !fx_attrs.contains_key("__package__") {
            fx_attrs.insert(CompactString::from("__package__"), PyObject::none());
        }
        Self::wrap(PyObjectPayload::Module(Box::new(ModuleData { name, attrs: Rc::new(PyCell::new(fx_attrs)) })))
    }
    /// Create a module that shares an existing globals Arc (for circular import support).
    pub fn module_with_shared_globals(name: CompactString, globals: Rc<PyCell<FxAttrMap>>) -> PyObjectRef {
        {
            let mut g = globals.write();
            if !g.contains_key("__name__") {
                g.insert(CompactString::from("__name__"), PyObject::str_val(name.clone()));
            }
            if !g.contains_key("__loader__") {
                g.insert(CompactString::from("__loader__"), PyObject::none());
            }
            if !g.contains_key("__spec__") {
                g.insert(CompactString::from("__spec__"), PyObject::none());
            }
            if !g.contains_key("__package__") {
                g.insert(CompactString::from("__package__"), PyObject::none());
            }
        }
        Self::wrap(PyObjectPayload::Module(Box::new(ModuleData { name, attrs: globals })))
    }
    pub fn native_function(name: &str, func: fn(&[PyObjectRef]) -> PyResult<PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::NativeFunction(Box::new(NativeFunctionData { name: CompactString::from(name), func })))
    }
    pub fn native_closure(name: &str, func: impl Fn(&[PyObjectRef]) -> PyResult<PyObjectRef> + 'static) -> PyObjectRef {
        Self::wrap(PyObjectPayload::NativeClosure(Box::new(NativeClosureData { name: CompactString::from(name), func: std::rc::Rc::new(func) })))
    }
    pub fn dict_from_pairs(pairs: Vec<(PyObjectRef, PyObjectRef)>) -> PyObjectRef {
        let mut map = new_fx_hashkey_map();
        for (k, v) in pairs {
            if let Ok(hk) = k.to_hashable_key() {
                map.insert(hk, v);
            }
        }
        let obj = Self::wrap(PyObjectPayload::Dict(Rc::new(PyCell::new(map))));
        track_object(&obj);
        obj
    }
    pub fn slice(start: Option<PyObjectRef>, stop: Option<PyObjectRef>, step: Option<PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Slice(Box::new(SliceData { start, stop, step })))
    }
    pub fn frozenset<S: std::hash::BuildHasher>(items: IndexMap<HashableKey, PyObjectRef, S>) -> PyObjectRef {
        let fx: FxHashKeyMap = items.into_iter().collect();
        Self::wrap(PyObjectPayload::FrozenSet(Box::new(fx)))
    }
    pub fn range(start: i64, stop: i64, step: i64) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Range(Box::new(RangeData { start, stop, step })))
    }
    pub fn cell(cell: Rc<PyCell<Option<PyObjectRef>>>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Cell(cell))
    }
    pub fn exception_type(kind: ExceptionKind) -> PyObjectRef {
        Self::wrap(PyObjectPayload::ExceptionType(kind))
    }
    pub fn exception_instance(kind: ExceptionKind, message: impl Into<CompactString>) -> PyObjectRef {
        let msg: CompactString = message.into();
        let args = if msg.is_empty() { vec![] } else { vec![PyObject::str_val(msg.clone())] };
        Self::wrap(PyObjectPayload::ExceptionInstance(ManuallyDrop::new(
            alloc_exception_box(kind, msg, args),
        )))
    }
    pub fn exception_instance_with_args(kind: ExceptionKind, message: impl Into<CompactString>, args: Vec<PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::ExceptionInstance(ManuallyDrop::new(
            alloc_exception_box(kind, message.into(), args),
        )))
    }
    pub fn generator(name: CompactString, frame_ptr: *mut u8) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Generator(Rc::new(PyCell::new(GeneratorState {
            name,
            frame_ptr,
            started: false,
            finished: false,
        }))))
    }

    pub fn coroutine(name: CompactString, frame_ptr: *mut u8) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Coroutine(Rc::new(PyCell::new(GeneratorState {
            name,
            frame_ptr,
            started: false,
            finished: false,
        }))))
    }

    pub fn async_generator(name: CompactString, frame_ptr: *mut u8) -> PyObjectRef {
        Self::wrap(PyObjectPayload::AsyncGenerator(Rc::new(PyCell::new(GeneratorState {
            name,
            frame_ptr,
            started: false,
            finished: false,
        }))))
    }

    /// Create a builtin awaitable that immediately resolves to the given value when awaited.
    pub fn builtin_awaitable(value: PyObjectRef) -> PyObjectRef {
        Self::wrap(PyObjectPayload::BuiltinAwaitable(value))
    }

    /// Create a deferred sleep awaitable. The actual sleep happens in the VM's
    /// YIELD_FROM handler, so wait_for can enforce its deadline before the sleep.
    pub fn deferred_sleep(secs: f64, result: PyObjectRef) -> PyObjectRef {
        Self::wrap(PyObjectPayload::DeferredSleep { secs, result })
    }
}

