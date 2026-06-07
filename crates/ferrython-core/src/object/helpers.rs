//! Formatting helpers, slice resolution, coercion, and module-building utilities.

use crate::error::ExceptionKind;
use crate::error::{PyException, PyResult};
use crate::types::{HashableKey, PyInt};
use compact_str::CompactString;
use indexmap::IndexMap;
use num_bigint::Sign;
use num_traits::ToPrimitive;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};

use super::methods::PyObjectMethods;
use super::payload::*;

pub const INSTANCE_DICT_EXTRA_KEY: &str = "__ferrython_instance_dict_extra__";

pub fn string_key_name(obj: &PyObjectRef) -> Option<CompactString> {
    match &obj.payload {
        PyObjectPayload::Str(s) => Some(s.to_compact_string()),
        PyObjectPayload::Instance(inst) => {
            if let PyObjectPayload::Class(cd) = &inst.class.payload {
                if cd.builtin_base_name.as_deref() == Some("str") {
                    let value = inst.attrs.read().get("__builtin_value__").cloned()?;
                    if let PyObjectPayload::Str(s) = &value.payload {
                        return Some(s.to_compact_string());
                    }
                }
            }
            None
        }
        _ => None,
    }
}

pub fn is_instance_dict_internal_key(name: &str) -> bool {
    name == INSTANCE_DICT_EXTRA_KEY
}

pub fn instance_dict_extra_map(attrs: &SharedFxAttrMap) -> Option<Rc<PyCell<FxHashKeyMap>>> {
    let obj = attrs.read().get(INSTANCE_DICT_EXTRA_KEY).cloned()?;
    match &obj.payload {
        PyObjectPayload::Dict(map) => Some(map.clone()),
        _ => None,
    }
}

pub fn instance_dict_ensure_extra_map(attrs: &SharedFxAttrMap) -> Rc<PyCell<FxHashKeyMap>> {
    if let Some(map) = instance_dict_extra_map(attrs) {
        return map;
    }
    let obj = PyObject::dict(new_fx_hashkey_map());
    let map = match &obj.payload {
        PyObjectPayload::Dict(map) => map.clone(),
        _ => unreachable!(),
    };
    attrs
        .write()
        .insert(CompactString::from(INSTANCE_DICT_EXTRA_KEY), obj);
    map
}

pub fn instance_dict_as_hashkey_map(attrs: &SharedFxAttrMap) -> FxHashKeyMap {
    let mut result = instance_dict_extra_map(attrs)
        .map(|map| map.read().clone())
        .unwrap_or_else(new_fx_hashkey_map);
    for (key, value) in attrs.read().iter() {
        if !is_instance_dict_internal_key(key.as_str()) {
            result.insert(HashableKey::str_key(key.clone()), value.clone());
        }
    }
    result
}

pub fn instance_dict_visible_len(attrs: &SharedFxAttrMap) -> usize {
    let attr_len = attrs
        .read()
        .keys()
        .filter(|key| !is_instance_dict_internal_key(key.as_str()))
        .count();
    let extra_len = instance_dict_extra_map(attrs)
        .map(|map| map.read().len())
        .unwrap_or(0);
    attr_len + extra_len
}

pub fn instance_dict_get_item(
    attrs: &SharedFxAttrMap,
    key: &PyObjectRef,
) -> PyResult<Option<PyObjectRef>> {
    if let Some(name) = string_key_name(key) {
        if let Some(value) = attrs.read().get(name.as_str()).cloned() {
            return Ok(Some(value));
        }
    }
    let hk = key.to_hashable_key()?;
    Ok(instance_dict_extra_map(attrs).and_then(|map| map.read().get(&hk).cloned()))
}

pub fn instance_dict_set_item(
    attrs: &SharedFxAttrMap,
    key: &PyObjectRef,
    value: PyObjectRef,
) -> PyResult<()> {
    if let Some(name) = string_key_name(key) {
        if !is_instance_dict_internal_key(name.as_str()) {
            attrs.write().insert(name, value);
            return Ok(());
        }
    }
    let hk = key.to_hashable_key()?;
    instance_dict_ensure_extra_map(attrs)
        .write()
        .insert(hk, value);
    Ok(())
}

pub fn instance_dict_remove_item(
    attrs: &SharedFxAttrMap,
    key: &PyObjectRef,
) -> PyResult<Option<PyObjectRef>> {
    if let Some(name) = string_key_name(key) {
        if !is_instance_dict_internal_key(name.as_str()) {
            if let Some(value) = attrs.write().shift_remove(name.as_str()) {
                return Ok(Some(value));
            }
        }
    }
    let hk = key.to_hashable_key()?;
    Ok(instance_dict_extra_map(attrs).and_then(|map| map.write().shift_remove(&hk)))
}

pub fn instance_class_special_method(
    obj: &PyObjectRef,
    inst: &InstanceData,
    name: &str,
) -> Option<PyObjectRef> {
    super::methods_attr_helpers::lookup_in_class_mro(&inst.class, name).map(|method| {
        super::methods_attr_helpers::wrap_class_attr_for_instance(obj, inst, name, method)
    })
}

// ── Post-call intercept fast flag ──
// Set when asyncio.run(), __import__(), importlib.import_module(), or reload()
// needs to be intercepted after a function call returns. Avoids 4 thread-local
// checks on every normal function return.
thread_local! {
    static INTERCEPT_PENDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Mark that a post-call intercept is pending.
pub fn set_intercept_pending() {
    INTERCEPT_PENDING.with(|c| c.set(true));
}

/// Check and clear the intercept flag (returns true if intercept was pending).
#[inline(always)]
pub fn check_intercept_pending() -> bool {
    INTERCEPT_PENDING.with(|c| {
        if c.get() {
            c.set(false);
            true
        } else {
            false
        }
    })
}

// ── Thread-local VM call dispatch ──
// Allows NativeClosures (which lack VM access) to call arbitrary Python objects
// (Functions, BoundMethods, etc.) by delegating to the VM through a registered callback.
thread_local! {
    static VM_CALL_DISPATCH: std::cell::RefCell<Option<Box<dyn FnMut(PyObjectRef, Vec<PyObjectRef>) -> PyResult<PyObjectRef>>>>
        = std::cell::RefCell::new(None);
    static VM_CALL_KW_DISPATCH: std::cell::RefCell<Option<Box<dyn FnMut(PyObjectRef, Vec<PyObjectRef>, Vec<(CompactString, PyObjectRef)>) -> PyResult<PyObjectRef>>>>
        = std::cell::RefCell::new(None);
}

static mut GLOBAL_LOOKUP_INVALIDATE: Option<fn()> = None;
static BYTEARRAY_EXPORTS: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();
static DICT_STORAGE_VERSIONS: OnceLock<Mutex<HashMap<usize, u64>>> = OnceLock::new();

fn dict_storage_version_map() -> &'static Mutex<HashMap<usize, u64>> {
    DICT_STORAGE_VERSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn dict_storage_key(map: &Rc<PyCell<FxHashKeyMap>>) -> usize {
    Rc::as_ptr(map) as usize
}

pub fn dict_storage_version(map: &Rc<PyCell<FxHashKeyMap>>) -> u64 {
    dict_storage_version_map()
        .lock()
        .map(|versions| versions.get(&dict_storage_key(map)).copied().unwrap_or(0))
        .unwrap_or(0)
}

pub fn mark_dict_storage_mutated(map: &Rc<PyCell<FxHashKeyMap>>) {
    if let Ok(mut versions) = dict_storage_version_map().lock() {
        let entry = versions.entry(dict_storage_key(map)).or_insert(0);
        *entry = entry.wrapping_add(1);
    }
}

pub fn dict_storage_iteration_changed(
    map: &Rc<PyCell<FxHashKeyMap>>,
    expected_len: usize,
    expected_version: u64,
) -> bool {
    map.read().len() != expected_len || dict_storage_version(map) != expected_version
}

/// Register the VM's call dispatch function. Called once by the VM at startup.
pub fn register_vm_call_dispatch<F>(f: F)
where
    F: FnMut(PyObjectRef, Vec<PyObjectRef>) -> PyResult<PyObjectRef> + 'static,
{
    VM_CALL_DISPATCH.with(|cell| {
        *cell.borrow_mut() = Some(Box::new(f));
    });
}

/// Register the VM's keyword-aware call dispatch function.
pub fn register_vm_call_kw_dispatch<F>(f: F)
where
    F: FnMut(
            PyObjectRef,
            Vec<PyObjectRef>,
            Vec<(CompactString, PyObjectRef)>,
        ) -> PyResult<PyObjectRef>
        + 'static,
{
    VM_CALL_KW_DISPATCH.with(|cell| {
        *cell.borrow_mut() = Some(Box::new(f));
    });
}

pub fn register_global_lookup_invalidate(f: fn()) {
    unsafe {
        GLOBAL_LOOKUP_INVALIDATE = Some(f);
    }
}

pub fn invalidate_global_lookups() {
    if let Some(f) = unsafe { GLOBAL_LOOKUP_INVALIDATE } {
        f();
    }
}

fn bytearray_export_set() -> &'static Mutex<HashSet<usize>> {
    BYTEARRAY_EXPORTS.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn register_bytearray_export(obj: &PyObjectRef) {
    if matches!(obj.payload, PyObjectPayload::ByteArray(_)) {
        let key = PyObjectRef::as_ptr(obj) as usize;
        if let Ok(mut exports) = bytearray_export_set().lock() {
            exports.insert(key);
        }
    }
}

pub fn consume_bytearray_export(obj: &PyObjectRef) -> bool {
    if !matches!(obj.payload, PyObjectPayload::ByteArray(_)) {
        return false;
    }
    let key = PyObjectRef::as_ptr(obj) as usize;
    bytearray_export_set()
        .lock()
        .map(|mut exports| exports.remove(&key))
        .unwrap_or(false)
}

/// Call any Python callable (NativeFunction, NativeClosure, Function, BoundMethod, etc.)
/// through the VM dispatch. Falls back to direct native calls if no VM is registered.
pub fn call_callable(func: &PyObjectRef, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Fast path: native functions don't need VM
    match &func.payload {
        PyObjectPayload::NativeFunction(nf) => return (nf.func)(args),
        PyObjectPayload::NativeClosure(nc) => return (nc.func)(args),
        PyObjectPayload::BoundMethod { receiver, method } => {
            // If the underlying method is native, call directly
            match &method.payload {
                PyObjectPayload::NativeFunction(nf) => {
                    let mut full_args = vec![receiver.clone()];
                    full_args.extend_from_slice(args);
                    return (nf.func)(&full_args);
                }
                PyObjectPayload::NativeClosure(nc) => {
                    let mut full_args = vec![receiver.clone()];
                    full_args.extend_from_slice(args);
                    return (nc.func)(&full_args);
                }
                _ => {} // Fall through to VM dispatch
            }
        }
        _ => {}
    }
    // Slow path: delegate to VM for Python functions, classes, etc.
    // Take the dispatch fn out of the cell to avoid holding the borrow during
    // the call — the dispatched code may re-enter call_callable.
    let dispatch_fn = VM_CALL_DISPATCH.with(|cell| cell.borrow_mut().take());
    if let Some(mut dispatch) = dispatch_fn {
        let result = dispatch(func.clone(), args.to_vec());
        VM_CALL_DISPATCH.with(|cell| {
            *cell.borrow_mut() = Some(dispatch);
        });
        result
    } else {
        Err(PyException::type_error(
            "not a callable (no VM dispatch registered)",
        ))
    }
}

/// Call any Python callable with keyword arguments through the VM dispatch.
pub fn call_callable_kw(
    func: &PyObjectRef,
    args: &[PyObjectRef],
    kwargs: Vec<(CompactString, PyObjectRef)>,
) -> PyResult<PyObjectRef> {
    if kwargs.is_empty() {
        return call_callable(func, args);
    }
    let dispatch_fn = VM_CALL_KW_DISPATCH.with(|cell| cell.borrow_mut().take());
    if let Some(mut dispatch) = dispatch_fn {
        let result = dispatch(func.clone(), args.to_vec(), kwargs);
        VM_CALL_KW_DISPATCH.with(|cell| {
            *cell.borrow_mut() = Some(dispatch);
        });
        result
    } else {
        Err(PyException::type_error(
            "not a callable (no VM keyword dispatch registered)",
        ))
    }
}

// ── Recursive repr guard ──
// Prevents infinite recursion when repr()ing self-referential structures
// like `lst = []; lst.append(lst)`.
thread_local! {
    static REPR_ACTIVE: std::cell::RefCell<HashSet<usize>> = std::cell::RefCell::new(HashSet::new());
    static REPR_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static REPR_OVERFLOW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

const DEFAULT_REPR_RECURSION_LIMIT: usize = 1000;

const DEFAULT_MAX_EAGER_ALLOCATION_ITEMS: usize = 8 * 1024 * 1024;
static MAX_EAGER_ALLOCATION_ITEMS: OnceLock<usize> = OnceLock::new();

fn eager_allocation_limit() -> usize {
    *MAX_EAGER_ALLOCATION_ITEMS.get_or_init(|| {
        std::env::var("FERRYTHON_MAX_EAGER_ITEMS")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .filter(|limit| *limit > 0)
            .unwrap_or(DEFAULT_MAX_EAGER_ALLOCATION_ITEMS)
    })
}

fn eager_allocation_error(context: &str, requested: usize) -> PyException {
    PyException::new(
        ExceptionKind::MemoryError,
        format!(
            "{} would allocate {} items; limit is {} (set FERRYTHON_MAX_EAGER_ITEMS to adjust)",
            context,
            requested,
            eager_allocation_limit()
        ),
    )
}

/// Check if a dict key is a hidden internal marker (should be excluded from iteration).
/// Used by defaultdict (__defaultdict_factory__), Counter (__counter__), and OrderedDict (__ordered_dict__, __move_to_end_fn__).
#[inline]
pub fn is_hidden_dict_key(k: &HashableKey) -> bool {
    matches!(k, HashableKey::Str(s) if
        s.as_str() == "__defaultdict_factory__"
        || s.as_str() == "__defaultdict_kwargs__"
        || s.as_str() == "__counter__"
        || s.as_str() == "__ordered_dict__"
        || s.as_str() == "__ordered_dict_broken__"
        || s.as_str() == "__move_to_end_fn__"
        || s.as_str() == "_tuple"
    )
}

/// Enter repr for an object identified by its pointer. Returns true if this is
/// a new entry (safe to proceed). Returns false if already active (cycle detected).
pub fn repr_enter(ptr: usize) -> bool {
    if REPR_ACTIVE.with(|set| set.borrow().contains(&ptr)) {
        return false;
    }
    REPR_DEPTH.with(|depth| {
        let next = depth.get().saturating_add(1);
        if next > DEFAULT_REPR_RECURSION_LIMIT {
            REPR_OVERFLOW.with(|flag| flag.set(true));
            return false;
        }
        depth.set(next);
        REPR_ACTIVE.with(|set| set.borrow_mut().insert(ptr))
    })
}

pub fn repr_leave(ptr: usize) {
    REPR_ACTIVE.with(|set| {
        set.borrow_mut().remove(&ptr);
    });
    REPR_DEPTH.with(|depth| {
        depth.set(depth.get().saturating_sub(1));
    });
}

pub fn repr_depth_exceeded() -> bool {
    REPR_OVERFLOW.with(|flag| flag.get())
}

pub fn repr_reset_overflow() {
    REPR_OVERFLOW.with(|flag| flag.set(false));
}

pub fn repr_recursion_limit() -> usize {
    DEFAULT_REPR_RECURSION_LIMIT
}

pub fn guard_eager_allocation(requested: usize, context: &str) -> PyResult<()> {
    if requested > eager_allocation_limit() {
        return Err(eager_allocation_error(context, requested));
    }
    Ok(())
}

pub fn checked_repeat_len(unit_len: usize, count: usize, context: &str) -> PyResult<usize> {
    let requested = unit_len
        .checked_mul(count)
        .ok_or_else(|| eager_allocation_error(context, usize::MAX))?;
    guard_eager_allocation(requested, context)?;
    Ok(requested)
}

pub fn index_to_isize(value: &PyObjectRef) -> PyResult<isize> {
    value
        .to_index()?
        .to_i64()
        .and_then(|n| isize::try_from(n).ok())
        .ok_or_else(|| PyException::overflow_error("cannot fit 'int' into an index-sized integer"))
}

pub fn index_to_i64(value: &PyObjectRef) -> PyResult<i64> {
    index_to_isize(value).map(|n| n as i64)
}

pub fn index_to_usize_repeat(value: &PyObjectRef) -> PyResult<usize> {
    let index = value.to_index()?;
    let Some(n) = index.to_i64() else {
        return Err(PyException::overflow_error(
            "cannot fit 'int' into an index-sized integer",
        ));
    };
    if isize::try_from(n).is_err() {
        return Err(PyException::overflow_error(
            "cannot fit 'int' into an index-sized integer",
        ));
    }
    Ok(n.max(0) as usize)
}

pub fn guarded_push<T>(items: &mut Vec<T>, item: T, context: &str) -> PyResult<()> {
    let next_len = items.len().saturating_add(1);
    guard_eager_allocation(next_len, context)?;
    items.push(item);
    Ok(())
}

// ── Helpers ──

/// Check if a class inherits from a builtin type (int, str, float, etc.)
/// and return the builtin type name if so.
pub fn get_builtin_base_type_name(class: &PyObjectRef) -> Option<CompactString> {
    if let PyObjectPayload::Class(cd) = &class.payload {
        return get_builtin_base_type_name_from_bases(&cd.bases);
    }
    None
}

/// Check bases list directly for a builtin type ancestor.
pub fn get_builtin_base_type_name_from_bases(bases: &[PyObjectRef]) -> Option<CompactString> {
    for base in bases {
        match &base.payload {
            PyObjectPayload::BuiltinType(name) => {
                if matches!(
                    name.as_str(),
                    "int"
                        | "str"
                        | "float"
                        | "complex"
                        | "dict"
                        | "list"
                        | "tuple"
                        | "set"
                        | "frozenset"
                        | "bytes"
                        | "bytearray"
                        | "enumerate"
                ) {
                    return Some((**name).clone());
                }
            }
            PyObjectPayload::NativeFunction(nf) => match nf.name.as_str() {
                "collections.deque" => return Some(CompactString::from("deque")),
                "datetime.time" => return Some(CompactString::from("time")),
                _ => {}
            },
            PyObjectPayload::BuiltinFunction(name) if name.as_str() == "enumerate" => {
                return Some(CompactString::from("enumerate"));
            }
            PyObjectPayload::Class(cd) => {
                if let Some(name) = get_builtin_base_type_name_from_bases(&cd.bases) {
                    return Some(name);
                }
            }
            _ => {}
        }
    }
    None
}

/// If obj is an Instance of a builtin subclass with __builtin_value__, return the value.
/// Otherwise, return the original object unchanged.
pub fn unwrap_builtin_subclass(obj: &PyObjectRef) -> PyObjectRef {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        if inst.attrs.read().contains_key("__deque__") {
            return obj.clone();
        }
        if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
            return val;
        }
    }
    obj.clone()
}

pub fn is_property_subclass_class(class: &PyObjectRef) -> bool {
    if let PyObjectPayload::Class(cd) = &class.payload {
        if cd.name.as_str() == "property" || is_dynamic_class_attribute_class(class) {
            return true;
        }
        for base in &cd.bases {
            match &base.payload {
                PyObjectPayload::BuiltinType(name) | PyObjectPayload::BuiltinFunction(name)
                    if name.as_str() == "property" =>
                {
                    return true;
                }
                PyObjectPayload::Class(_) if is_property_subclass_class(base) => return true,
                _ => {}
            }
        }
    }
    false
}

pub fn is_dynamic_class_attribute_class(class: &PyObjectRef) -> bool {
    if let PyObjectPayload::Class(cd) = &class.payload {
        if cd.name.as_str() == "DynamicClassAttribute"
            || cd
                .namespace
                .read()
                .get("__dynamic_class_attribute_class__")
                .map(|v| v.is_truthy())
                .unwrap_or(false)
        {
            return true;
        }
        for base in cd.bases.iter().chain(cd.mro.iter()) {
            match &base.payload {
                PyObjectPayload::BuiltinType(name) | PyObjectPayload::BuiltinFunction(name)
                    if name.as_str() == "DynamicClassAttribute" =>
                {
                    return true;
                }
                PyObjectPayload::Class(_) if is_dynamic_class_attribute_class(base) => {
                    return true;
                }
                _ => {}
            }
        }
    }
    false
}

#[inline]
pub fn is_property_like(obj: &PyObjectRef) -> bool {
    match &obj.payload {
        PyObjectPayload::Property(_) => true,
        PyObjectPayload::Instance(inst) => is_property_subclass_class(&inst.class),
        _ => false,
    }
}

#[inline]
pub fn is_dynamic_class_attribute(obj: &PyObjectRef) -> bool {
    match &obj.payload {
        PyObjectPayload::Instance(inst) => {
            inst.attrs
                .read()
                .get("__dynamic_class_attribute__")
                .map(|v| v.is_truthy())
                .unwrap_or(false)
                || is_dynamic_class_attribute_class(&inst.class)
        }
        _ => false,
    }
}

pub fn property_doc_from_getter(fget: Option<&PyObjectRef>) -> Option<PyObjectRef> {
    fget.and_then(|fg| fg.get_attr("__doc__"))
}

pub fn property_init_doc(
    fget: Option<&PyObjectRef>,
    explicit_doc: Option<PyObjectRef>,
) -> (Option<PyObjectRef>, bool) {
    if let Some(doc) = explicit_doc {
        if matches!(&doc.payload, PyObjectPayload::None) {
            (property_doc_from_getter(fget), true)
        } else {
            (Some(doc), false)
        }
    } else {
        (property_doc_from_getter(fget), true)
    }
}

pub fn property_field(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::Property(pd) => match name {
            "fget" => pd.fget.clone(),
            "fset" => pd.fset.clone(),
            "fdel" => pd.fdel.clone(),
            "__doc__" => pd.doc.read().clone(),
            _ => None,
        },
        PyObjectPayload::Instance(inst) if is_property_subclass_class(&inst.class) => {
            inst.attrs.read().get(name).cloned()
        }
        _ => None,
    }
}

pub fn property_set_doc(obj: &PyObjectRef, value: PyObjectRef) -> PyResult<()> {
    match &obj.payload {
        PyObjectPayload::Property(pd) => {
            *pd.doc.write() = Some(value);
            Ok(())
        }
        PyObjectPayload::Instance(inst) if is_property_subclass_class(&inst.class) => {
            if let PyObjectPayload::Class(cd) = &inst.class.payload {
                if let Some(own_slots) = &cd.slots {
                    if !own_slots.iter().any(|s| s.as_str() == "__dict__")
                        && !own_slots.iter().any(|s| s.as_str() == "__doc__")
                    {
                        return Err(PyException::attribute_error(format!(
                            "'{}' object attribute '__doc__' is read-only",
                            cd.name
                        )));
                    }
                }
                if let Some(allowed) = cd.collect_all_slots() {
                    if !allowed.iter().any(|s| s.as_str() == "__dict__")
                        && !allowed.iter().any(|s| s.as_str() == "__doc__")
                    {
                        return Err(PyException::attribute_error(format!(
                            "'{}' object attribute '__doc__' is read-only",
                            cd.name
                        )));
                    }
                }
            }
            inst.attrs
                .write()
                .insert(CompactString::from("__doc__"), value);
            Ok(())
        }
        _ => Err(PyException::attribute_error(format!(
            "'{}' object does not support attribute assignment",
            obj.type_name()
        ))),
    }
}

pub(in crate::object) fn index_to_i128_unbounded(obj: &PyObjectRef) -> PyResult<i128> {
    match obj.to_index()? {
        PyInt::Small(n) => Ok(n as i128),
        PyInt::Big(n) => Ok(n.to_i128().unwrap_or_else(|| {
            if n.sign() == Sign::Minus {
                i128::MIN
            } else {
                i128::MAX
            }
        })),
    }
}

pub(super) fn coerce_to_f64(obj: &PyObjectRef) -> PyResult<f64> {
    match &obj.payload {
        PyObjectPayload::Float(f) => Ok(*f),
        PyObjectPayload::Int(n) => Ok(n.to_f64()),
        PyObjectPayload::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        _ => Err(PyException::type_error(format!(
            "must be real number, not {}",
            obj.type_name()
        ))),
    }
}

pub(super) fn int_bitop(
    a: &PyObjectRef,
    b: &PyObjectRef,
    op_name: &str,
    op: fn(i64, i64) -> i64,
) -> PyResult<PyObjectRef> {
    let int_operand = |obj: &PyObjectRef| -> PyResult<PyInt> {
        let obj = unwrap_builtin_subclass(obj);
        match &obj.payload {
            PyObjectPayload::Int(n) => Ok(n.clone()),
            PyObjectPayload::Bool(flag) => Ok(PyInt::Small(*flag as i64)),
            _ => Err(PyException::type_error(format!(
                "unsupported operand type(s) for {}: '{}' and '{}'",
                op_name,
                a.type_name(),
                b.type_name()
            ))),
        }
    };
    let ai = int_operand(a)?;
    let bi = int_operand(b)?;
    if let (Some(ai), Some(bi)) = (ai.to_i64(), bi.to_i64()) {
        return Ok(PyObject::int(op(ai, bi)));
    }
    let result = match op_name {
        "&" => ai.to_bigint() & bi.to_bigint(),
        "|" => ai.to_bigint() | bi.to_bigint(),
        "^" => ai.to_bigint() ^ bi.to_bigint(),
        _ => {
            return Err(PyException::type_error(format!(
                "unsupported operand type(s) for {}: '{}' and '{}'",
                op_name,
                a.type_name(),
                b.type_name()
            )))
        }
    };
    Ok(PyInt::from_bigint(result).to_object())
}

mod builtin_type_methods;
mod comparison;
mod formatting;

pub use builtin_type_methods::*;
pub use comparison::*;
pub use formatting::*;

// ── Module-building utilities (used by ferrython-stdlib and ferrython-vm) ──

/// The function pointer type for built-in functions.
pub type BuiltinFn = fn(&[PyObjectRef]) -> PyResult<PyObjectRef>;

/// Create a module object with named attributes.
pub fn make_module(name: &str, attrs: Vec<(&str, PyObjectRef)>) -> PyObjectRef {
    let mut map = IndexMap::new();
    map.insert(
        CompactString::from("__name__"),
        PyObject::str_val(CompactString::from(name)),
    );
    for (k, v) in attrs {
        map.insert(CompactString::from(k), v);
    }
    PyObject::module_with_attrs(CompactString::from(name), map)
}

/// Wrap a bare function pointer as a NativeFunction object.
pub fn make_builtin(f: BuiltinFn) -> PyObjectRef {
    PyObject::native_function("", f)
}

/// Check that exactly `expected` arguments were provided.
pub fn check_args(name: &str, args: &[PyObjectRef], expected: usize) -> PyResult<()> {
    if args.len() != expected {
        Err(PyException::type_error(format!(
            "{}() takes exactly {} argument(s) ({} given)",
            name,
            expected,
            args.len()
        )))
    } else {
        Ok(())
    }
}

/// Check that at least `min` arguments were provided.
pub fn check_args_min(name: &str, args: &[PyObjectRef], min: usize) -> PyResult<()> {
    if args.len() < min {
        Err(PyException::type_error(format!(
            "{}() takes at least {} argument(s) ({} given)",
            name,
            min,
            args.len()
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{checked_repeat_len, guard_eager_allocation};
    use crate::error::ExceptionKind;

    #[test]
    fn repeat_guard_rejects_overflowing_repeat() {
        let err = checked_repeat_len(usize::MAX, 2, "repeat").unwrap_err();
        assert_eq!(err.kind, ExceptionKind::MemoryError);
    }

    #[test]
    fn eager_guard_rejects_large_requests() {
        let err = guard_eager_allocation(usize::MAX, "collect").unwrap_err();
        assert_eq!(err.kind, ExceptionKind::MemoryError);
    }
}
