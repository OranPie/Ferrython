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

// ── GC Tracking for Instance objects (cycle detection) ──
static TRACKED_INSTANCES: LazyLock<Mutex<Vec<Weak<PyObject>>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// Register the cycle collector callback with the GC crate.
pub fn init_gc() {
    ferrython_gc::register_cycle_collector(run_cycle_collection);
}

/// Cycle collection: find Instance objects that are only reachable through
/// other tracked objects (i.e., they form a reference cycle).
///
/// Algorithm (trial deletion, simplified for Arc):
/// 1. Purge dead weak refs from TRACKED_INSTANCES
/// 2. For each live tracked object, count Arc::strong_count()
/// 3. Count how many references each tracked object receives from other tracked objects
/// 4. If strong_count == internal_refs, the object is only reachable from within cycles
/// 5. Clear attrs on unreachable objects to break cycles (dropping internal refs)
fn run_cycle_collection() -> usize {
    let mut tracked = TRACKED_INSTANCES.lock().unwrap();

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
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            // Count refs from this instance's attrs to other tracked instances
            let attrs = inst.attrs.read();
            for attr_val in attrs.values() {
                let ptr = Arc::as_ptr(attr_val) as usize;
                if let Some(&target_idx) = ptr_map.get(&ptr) {
                    internal_refs[target_idx] += 1;
                }
            }
            // Count ref from class field
            let class_ptr = Arc::as_ptr(&inst.class) as usize;
            if let Some(&target_idx) = ptr_map.get(&class_ptr) {
                internal_refs[target_idx] += 1;
            }
        }
    }

    // 4. Trial deletion: objects where strong_count == internal_refs + 1
    // (+1 for our own `alive` Vec holding a ref)
    let mut garbage_indices: Vec<usize> = Vec::new();
    for (i, obj) in alive.iter().enumerate() {
        let strong = Arc::strong_count(obj);
        // strong_count includes: our `alive` vec (1) + internal refs + external refs
        // If strong == internal_refs + 1, there are no external refs
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
        let mut all_refs_in_garbage = true;
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            let attrs = inst.attrs.read();
            for attr_val in attrs.values() {
                let ptr = Arc::as_ptr(attr_val) as usize;
                if let Some(&target_idx) = ptr_map.get(&ptr) {
                    if !garbage_set.contains(&target_idx) {
                        all_refs_in_garbage = false;
                        break;
                    }
                }
            }
        }
        if all_refs_in_garbage {
            confirmed_garbage.push(gi);
        }
    }

    // 6. Break cycles by clearing attrs on garbage objects
    let collected = confirmed_garbage.len();
    for &gi in &confirmed_garbage {
        if let PyObjectPayload::Instance(inst) = &alive[gi].payload {
            inst.attrs.write().clear();
        }
    }

    collected
}

fn track_instance(obj: &PyObjectRef) {
    if let Ok(mut tracked) = TRACKED_INSTANCES.lock() {
        tracked.push(Arc::downgrade(obj));
    }
}

// ── PyObject constructors ──

impl PyObject {
    pub fn wrap(payload: PyObjectPayload) -> PyObjectRef {
        ferrython_gc::notify_alloc();
        Arc::new(PyObject { payload })
    }
    pub fn none() -> PyObjectRef { NONE_SINGLETON.clone() }
    pub fn ellipsis() -> PyObjectRef { ELLIPSIS_SINGLETON.clone() }
    pub fn not_implemented() -> PyObjectRef { NOT_IMPLEMENTED_SINGLETON.clone() }
    pub fn bool_val(v: bool) -> PyObjectRef { if v { TRUE_SINGLETON.clone() } else { FALSE_SINGLETON.clone() } }
    pub fn int(v: i64) -> PyObjectRef { Self::wrap(PyObjectPayload::Int(PyInt::Small(v))) }
    pub fn big_int(v: BigInt) -> PyObjectRef { Self::wrap(PyObjectPayload::Int(PyInt::Big(Box::new(v)))) }
    pub fn float(v: f64) -> PyObjectRef { Self::wrap(PyObjectPayload::Float(v)) }
    pub fn complex(real: f64, imag: f64) -> PyObjectRef { Self::wrap(PyObjectPayload::Complex { real, imag }) }
    pub fn str_val(v: CompactString) -> PyObjectRef { Self::wrap(PyObjectPayload::Str(v)) }
    pub fn bytes(v: Vec<u8>) -> PyObjectRef { Self::wrap(PyObjectPayload::Bytes(v)) }
    pub fn bytearray(v: Vec<u8>) -> PyObjectRef { Self::wrap(PyObjectPayload::ByteArray(v)) }
    pub fn list(items: Vec<PyObjectRef>) -> PyObjectRef { Self::wrap(PyObjectPayload::List(Arc::new(RwLock::new(items)))) }
    pub fn tuple(items: Vec<PyObjectRef>) -> PyObjectRef { Self::wrap(PyObjectPayload::Tuple(items)) }
    pub fn set(items: IndexMap<HashableKey, PyObjectRef>) -> PyObjectRef { Self::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(items)))) }
    pub fn dict(items: IndexMap<HashableKey, PyObjectRef>) -> PyObjectRef { Self::wrap(PyObjectPayload::Dict(Arc::new(RwLock::new(items)))) }
    pub fn function(func: PyFunction) -> PyObjectRef { Self::wrap(PyObjectPayload::Function(func)) }
    pub fn builtin_function(name: CompactString) -> PyObjectRef { Self::wrap(PyObjectPayload::BuiltinFunction(name)) }
    pub fn builtin_type(name: CompactString) -> PyObjectRef { Self::wrap(PyObjectPayload::BuiltinType(name)) }
    pub fn code(code: ferrython_bytecode::CodeObject) -> PyObjectRef { Self::wrap(PyObjectPayload::Code(Box::new(code))) }
    pub fn class(name: CompactString, bases: Vec<PyObjectRef>, namespace: IndexMap<CompactString, PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Class(ClassData { name, bases, namespace: Arc::new(RwLock::new(namespace)), mro: Vec::new(), metaclass: None }))
    }
    pub fn instance(class: PyObjectRef) -> PyObjectRef {
        let dict_storage = Self::detect_dict_subclass(&class);
        let obj = Self::wrap(PyObjectPayload::Instance(InstanceData { class, attrs: Arc::new(RwLock::new(IndexMap::new())), dict_storage }));
        track_instance(&obj);
        obj
    }
    pub fn instance_with_attrs(class: PyObjectRef, attrs: IndexMap<CompactString, PyObjectRef>) -> PyObjectRef {
        let dict_storage = Self::detect_dict_subclass(&class);
        let obj = Self::wrap(PyObjectPayload::Instance(InstanceData { class, attrs: Arc::new(RwLock::new(attrs)), dict_storage }));
        track_instance(&obj);
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
        Self::wrap(PyObjectPayload::Module(ModuleData { name, attrs: IndexMap::new() }))
    }
    pub fn module_with_attrs(name: CompactString, attrs: IndexMap<CompactString, PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::Module(ModuleData { name, attrs }))
    }
    pub fn native_function(name: &str, func: fn(&[PyObjectRef]) -> PyResult<PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::NativeFunction { name: CompactString::from(name), func })
    }
    pub fn native_closure(name: &str, func: impl Fn(&[PyObjectRef]) -> PyResult<PyObjectRef> + Send + Sync + 'static) -> PyObjectRef {
        Self::wrap(PyObjectPayload::NativeClosure { name: CompactString::from(name), func: Arc::new(func) })
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
        Self::wrap(PyObjectPayload::ExceptionInstance {
            kind,
            message: CompactString::from(msg),
            args,
            attrs: Arc::new(RwLock::new(IndexMap::new())),
        })
    }
    pub fn exception_instance_with_args(kind: ExceptionKind, message: impl Into<String>, args: Vec<PyObjectRef>) -> PyObjectRef {
        Self::wrap(PyObjectPayload::ExceptionInstance {
            kind,
            message: CompactString::from(message.into()),
            args,
            attrs: Arc::new(RwLock::new(IndexMap::new())),
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

