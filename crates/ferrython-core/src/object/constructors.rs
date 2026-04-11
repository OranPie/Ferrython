//! Singleton values and PyObject factory/constructor methods.

use crate::error::{ExceptionKind, PyResult};
use crate::types::{HashableKey, PyFunction, PyInt};
use compact_str::CompactString;
use indexmap::IndexMap;
use num_bigint::BigInt;
use parking_lot::RwLock;
use std::any::Any;
use std::sync::{Arc, Weak, Mutex};

use super::payload::*;
use super::methods::PyObjectMethods;

// ── Singletons ──
use std::sync::LazyLock;
static NONE_SINGLETON: LazyLock<PyObjectRef> = LazyLock::new(|| Arc::new(PyObject { payload: PyObjectPayload::None }));
static TRUE_SINGLETON: LazyLock<PyObjectRef> = LazyLock::new(|| Arc::new(PyObject { payload: PyObjectPayload::Bool(true) }));
static FALSE_SINGLETON: LazyLock<PyObjectRef> = LazyLock::new(|| Arc::new(PyObject { payload: PyObjectPayload::Bool(false) }));
static ELLIPSIS_SINGLETON: LazyLock<PyObjectRef> = LazyLock::new(|| Arc::new(PyObject { payload: PyObjectPayload::Ellipsis }));
static NOT_IMPLEMENTED_SINGLETON: LazyLock<PyObjectRef> = LazyLock::new(|| Arc::new(PyObject { payload: PyObjectPayload::NotImplemented }));

// ── Small-int cache (CPython caches -5..=256, we go wider for loop bounds) ──
const SMALL_INT_MIN: i64 = -5;
const SMALL_INT_MAX: i64 = 65536;

static SMALL_INT_CACHE: LazyLock<Vec<PyObjectRef>> = LazyLock::new(|| {
    (SMALL_INT_MIN..=SMALL_INT_MAX)
        .map(|n| Arc::new(PyObject { payload: PyObjectPayload::Int(PyInt::Small(n)) }))
        .collect()
});

// ── Float singleton cache for common values ──
static FLOAT_ZERO: LazyLock<PyObjectRef> = LazyLock::new(|| Arc::new(PyObject { payload: PyObjectPayload::Float(0.0) }));
static FLOAT_ONE: LazyLock<PyObjectRef> = LazyLock::new(|| Arc::new(PyObject { payload: PyObjectPayload::Float(1.0) }));
static FLOAT_NEG_ONE: LazyLock<PyObjectRef> = LazyLock::new(|| Arc::new(PyObject { payload: PyObjectPayload::Float(-1.0) }));

// ── Empty collection singletons ──
static EMPTY_TUPLE: LazyLock<PyObjectRef> = LazyLock::new(|| Arc::new(PyObject { payload: PyObjectPayload::Tuple(vec![]) }));
static EMPTY_STR: LazyLock<PyObjectRef> = LazyLock::new(|| Arc::new(PyObject { payload: PyObjectPayload::Str(CompactString::const_new("")) }));
static EMPTY_BYTES: LazyLock<PyObjectRef> = LazyLock::new(|| Arc::new(PyObject { payload: PyObjectPayload::Bytes(vec![]) }));

// ── GC Tracking for cycle-capable objects (Instance, Dict, List) ──
static TRACKED_OBJECTS: LazyLock<Mutex<Vec<Weak<PyObject>>>> = LazyLock::new(|| Mutex::new(Vec::new()));

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
/// 2. For each live tracked object, count Arc::strong_count()
/// 3. Count how many references each tracked object receives from other tracked objects
/// 4. If strong_count == internal_refs, the object is only reachable from within cycles
/// 5. Clear contents on unreachable objects to break cycles (dropping internal refs)
fn run_cycle_collection() -> usize {
    let mut tracked = TRACKED_OBJECTS.lock().unwrap();

    // 1. Upgrade weak refs, purge dead ones
    let alive: Vec<Arc<PyObject>> = tracked.iter()
        .filter_map(|w| w.upgrade())
        .collect();
    tracked.retain(|w| w.strong_count() > 0);

    if alive.is_empty() {
        return 0;
    }

    // 2. Build pointer → index map for fast lookup
    let ptr_map: std::collections::HashMap<usize, usize> = alive.iter()
        .enumerate()
        .map(|(i, obj)| (Arc::as_ptr(obj) as usize, i))
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
        let strong = Arc::strong_count(obj);
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
                let ptr = Arc::as_ptr(attr_val) as usize;
                if let Some(&target_idx) = ptr_map.get(&ptr) {
                    internal_refs[target_idx] += 1;
                }
            }
            let class_ptr = Arc::as_ptr(&inst.class) as usize;
            if let Some(&target_idx) = ptr_map.get(&class_ptr) {
                internal_refs[target_idx] += 1;
            }
        }
        PyObjectPayload::List(items) => {
            let items = items.read();
            for item in items.iter() {
                let ptr = Arc::as_ptr(item) as usize;
                if let Some(&target_idx) = ptr_map.get(&ptr) {
                    internal_refs[target_idx] += 1;
                }
            }
        }
        PyObjectPayload::Dict(map) => {
            let map = map.read();
            for val in map.values() {
                let ptr = Arc::as_ptr(val) as usize;
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
                let ptr = Arc::as_ptr(attr_val) as usize;
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
                let ptr = Arc::as_ptr(item) as usize;
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
                let ptr = Arc::as_ptr(val) as usize;
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
    if let Ok(mut tracked) = TRACKED_OBJECTS.lock() {
        tracked.push(Arc::downgrade(obj));
    }
}

// ── PyObject constructors ──

impl PyObject {
    #[inline]
    #[inline(always)]
    pub fn wrap(payload: PyObjectPayload) -> PyObjectRef {
        ferrython_gc::notify_alloc();
        Arc::new(PyObject { payload })
    }
    /// Like `wrap` but skips GC allocation tracking.
    /// Use for leaf types (Int, Float, Str, etc.) that cannot form reference cycles.
    #[inline(always)]
    pub fn wrap_leaf(payload: PyObjectPayload) -> PyObjectRef {
        Arc::new(PyObject { payload })
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
        Self::wrap_leaf(PyObjectPayload::Str(v))
    }
    pub fn bytes(v: Vec<u8>) -> PyObjectRef {
        if v.is_empty() { return EMPTY_BYTES.clone(); }
        Self::wrap_leaf(PyObjectPayload::Bytes(v))
    }
    pub fn bytearray(v: Vec<u8>) -> PyObjectRef { Self::wrap_leaf(PyObjectPayload::ByteArray(v)) }
    pub fn list(items: Vec<PyObjectRef>) -> PyObjectRef {
        let obj = Self::wrap(PyObjectPayload::List(Arc::new(RwLock::new(items))));
        track_object(&obj);
        obj
    }
    pub fn tuple(items: Vec<PyObjectRef>) -> PyObjectRef {
        if items.is_empty() { return EMPTY_TUPLE.clone(); }
        Self::wrap_leaf(PyObjectPayload::Tuple(items))
    }
    pub fn set(items: IndexMap<HashableKey, PyObjectRef>) -> PyObjectRef { Self::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(items)))) }
    pub fn dict(items: IndexMap<HashableKey, PyObjectRef>) -> PyObjectRef {
        let obj = Self::wrap(PyObjectPayload::Dict(Arc::new(RwLock::new(items))));
        track_object(&obj);
        obj
    }
    pub fn function(func: PyFunction) -> PyObjectRef { Self::wrap(PyObjectPayload::Function(Box::new(func))) }
    pub fn builtin_function(name: CompactString) -> PyObjectRef { Self::wrap(PyObjectPayload::BuiltinFunction(name)) }
    pub fn builtin_type(name: CompactString) -> PyObjectRef { Self::wrap(PyObjectPayload::BuiltinType(name)) }
    pub fn code(code: ferrython_bytecode::CodeObject) -> PyObjectRef { Self::wrap(PyObjectPayload::Code(std::sync::Arc::new(code))) }
    pub fn class(name: CompactString, bases: Vec<PyObjectRef>, namespace: IndexMap<CompactString, PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Class(Box::new(ClassData::new(name, bases, namespace, Vec::new(), None))))
    }
    pub fn instance(class: PyObjectRef) -> PyObjectRef {
        let dict_storage = Self::detect_dict_subclass(&class);
        let obj = Self::wrap(PyObjectPayload::Instance(InstanceData { class, attrs: Arc::new(RwLock::new(IndexMap::new())), dict_storage, is_special: false }));
        track_object(&obj);
        obj
    }
    pub fn instance_with_attrs(class: PyObjectRef, attrs: IndexMap<CompactString, PyObjectRef>) -> PyObjectRef {
        let dict_storage = Self::detect_dict_subclass(&class);
        let obj = Self::wrap(PyObjectPayload::Instance(InstanceData { class, attrs: Arc::new(RwLock::new(attrs)), dict_storage, is_special: false }));
        track_object(&obj);
        obj
    }

    /// Check if a class inherits from dict and return dict storage if so
    fn detect_dict_subclass(class: &PyObjectRef) -> Option<Arc<RwLock<IndexMap<crate::types::HashableKey, PyObjectRef>>>> {
        if let PyObjectPayload::Class(cd) = &class.payload {
            for base in &cd.bases {
                let is_dict = match &base.payload {
                    PyObjectPayload::BuiltinType(n) => n.as_str() == "dict",
                    PyObjectPayload::Class(bcd) => bcd.name.as_str() == "dict",
                    _ => false,
                };
                if is_dict {
                    return Some(Arc::new(RwLock::new(IndexMap::new())));
                }
                // Recurse into base classes
                if let Some(storage) = Self::detect_dict_subclass(base) {
                    drop(storage); // We create fresh storage for each instance
                    return Some(Arc::new(RwLock::new(IndexMap::new())));
                }
            }
        }
        None
    }
    pub fn module(name: CompactString) -> PyObjectRef {
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("__name__"), PyObject::str_val(name.clone()));
        attrs.insert(CompactString::from("__loader__"), PyObject::none());
        attrs.insert(CompactString::from("__spec__"), PyObject::none());
        attrs.insert(CompactString::from("__package__"), PyObject::none());
        Self::wrap(PyObjectPayload::Module(ModuleData { name, attrs: Arc::new(parking_lot::RwLock::new(attrs)) }))
    }
    pub fn module_with_attrs(name: CompactString, mut attrs: IndexMap<CompactString, PyObjectRef>) -> PyObjectRef {
        if !attrs.contains_key("__name__") {
            attrs.insert(CompactString::from("__name__"), PyObject::str_val(name.clone()));
        }
        if !attrs.contains_key("__loader__") {
            attrs.insert(CompactString::from("__loader__"), PyObject::none());
        }
        if !attrs.contains_key("__spec__") {
            attrs.insert(CompactString::from("__spec__"), PyObject::none());
        }
        if !attrs.contains_key("__package__") {
            attrs.insert(CompactString::from("__package__"), PyObject::none());
        }
        Self::wrap(PyObjectPayload::Module(ModuleData { name, attrs: Arc::new(parking_lot::RwLock::new(attrs)) }))
    }
    /// Create a module that shares an existing globals Arc (for circular import support).
    pub fn module_with_shared_globals(name: CompactString, globals: Arc<parking_lot::RwLock<IndexMap<CompactString, PyObjectRef>>>) -> PyObjectRef {
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
        Self::wrap(PyObjectPayload::Module(ModuleData { name, attrs: globals }))
    }
    pub fn native_function(name: &str, func: fn(&[PyObjectRef]) -> PyResult<PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::NativeFunction { name: CompactString::from(name), func })
    }
    pub fn native_closure(name: &str, func: impl Fn(&[PyObjectRef]) -> PyResult<PyObjectRef> + Send + Sync + 'static) -> PyObjectRef {
        Self::wrap(PyObjectPayload::NativeClosure(Box::new(NativeClosureData { name: CompactString::from(name), func: Arc::new(func) })))
    }
    pub fn dict_from_pairs(pairs: Vec<(PyObjectRef, PyObjectRef)>) -> PyObjectRef {
        let mut map = IndexMap::new();
        for (k, v) in pairs {
            if let Ok(hk) = k.to_hashable_key() {
                map.insert(hk, v);
            }
        }
        let obj = Self::wrap(PyObjectPayload::Dict(Arc::new(RwLock::new(map))));
        track_object(&obj);
        obj
    }
    pub fn slice(start: Option<PyObjectRef>, stop: Option<PyObjectRef>, step: Option<PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Slice { start, stop, step })
    }
    pub fn frozenset(items: IndexMap<HashableKey, PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::FrozenSet(Box::new(items)))
    }
    pub fn range(start: i64, stop: i64, step: i64) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Range { start, stop, step })
    }
    pub fn cell(cell: Arc<RwLock<Option<PyObjectRef>>>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Cell(cell))
    }
    pub fn exception_type(kind: ExceptionKind) -> PyObjectRef {
        Self::wrap(PyObjectPayload::ExceptionType(kind))
    }
    pub fn exception_instance(kind: ExceptionKind, message: impl Into<String>) -> PyObjectRef {
        let msg: String = message.into();
        let args = if msg.is_empty() { vec![] } else { vec![PyObject::str_val(CompactString::from(msg.as_str()))] };
        Self::wrap(PyObjectPayload::ExceptionInstance(Box::new(ExceptionInstanceData {
            kind,
            message: CompactString::from(msg),
            args,
            attrs: Arc::new(RwLock::new(IndexMap::new())),
        })))
    }
    pub fn exception_instance_with_args(kind: ExceptionKind, message: impl Into<String>, args: Vec<PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::ExceptionInstance(Box::new(ExceptionInstanceData {
            kind,
            message: CompactString::from(message.into()),
            args,
            attrs: Arc::new(RwLock::new(IndexMap::new())),
        })))
    }
    pub fn generator(name: CompactString, frame: Box<dyn Any + Send + Sync>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Generator(Arc::new(RwLock::new(GeneratorState {
            name,
            frame: Some(frame),
            started: false,
            finished: false,
        }))))
    }

    pub fn coroutine(name: CompactString, frame: Box<dyn Any + Send + Sync>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Coroutine(Arc::new(RwLock::new(GeneratorState {
            name,
            frame: Some(frame),
            started: false,
            finished: false,
        }))))
    }

    pub fn async_generator(name: CompactString, frame: Box<dyn Any + Send + Sync>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::AsyncGenerator(Arc::new(RwLock::new(GeneratorState {
            name,
            frame: Some(frame),
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

