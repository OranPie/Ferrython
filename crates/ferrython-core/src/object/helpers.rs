//! Formatting helpers, slice resolution, coercion, and module-building utilities.

use crate::error::ExceptionKind;
use crate::error::{PyException, PyResult};
use crate::intern::intern_or_new;
use crate::types::{HashableKey, PyInt};
use compact_str::CompactString;
use ferrython_bytecode::{CodeObject, ConstantValue};
use indexmap::IndexMap;
use num_bigint::Sign;
use num_traits::ToPrimitive;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use super::methods::CompareOp;
use super::methods::PyObjectMethods;
use super::payload::*;

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

fn code_objects_equal(a: &CodeObject, b: &CodeObject) -> bool {
    a.instructions == b.instructions
        && code_constant_values_equal(&a.constants, &b.constants)
        && a.names == b.names
        && a.varnames == b.varnames
        && a.freevars == b.freevars
        && a.cellvars == b.cellvars
        && a.name == b.name
        && a.qualname == b.qualname
        && a.first_line_number == b.first_line_number
        && a.docstring == b.docstring
        && a.line_number_table == b.line_number_table
        && a.flags == b.flags
        && a.arg_count == b.arg_count
        && a.posonlyarg_count == b.posonlyarg_count
        && a.kwonlyarg_count == b.kwonlyarg_count
        && a.num_locals == b.num_locals
        && a.max_stack_size == b.max_stack_size
}

fn code_constant_values_equal(a: &[ConstantValue], b: &[ConstantValue]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(left, right)| code_constant_value_equal(left, right))
}

fn code_constant_value_equal(a: &ConstantValue, b: &ConstantValue) -> bool {
    match (a, b) {
        (ConstantValue::Code(a), ConstantValue::Code(b)) => code_objects_equal(a, b),
        (ConstantValue::Tuple(a), ConstantValue::Tuple(b))
        | (ConstantValue::FrozenSet(a), ConstantValue::FrozenSet(b)) => {
            code_constant_values_equal(a, b)
        }
        _ => a.bit_exact_eq(b),
    }
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
}

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
        || s.as_str() == "__move_to_end_fn__"
        || s.as_str() == "_tuple"
    )
}

/// Enter repr for an object identified by its pointer. Returns true if this is
/// a new entry (safe to proceed). Returns false if already active (cycle detected).
pub fn repr_enter(ptr: usize) -> bool {
    REPR_ACTIVE.with(|set| set.borrow_mut().insert(ptr))
}

pub fn repr_leave(ptr: usize) {
    REPR_ACTIVE.with(|set| {
        set.borrow_mut().remove(&ptr);
    });
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
                        | "list"
                        | "tuple"
                        | "set"
                        | "frozenset"
                        | "bytes"
                        | "bytearray"
                ) {
                    return Some((**name).clone());
                }
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
        if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
            return val;
        }
    }
    obj.clone()
}

pub fn is_property_subclass_class(class: &PyObjectRef) -> bool {
    if let PyObjectPayload::Class(cd) = &class.payload {
        if cd.name.as_str() == "property" {
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

#[inline]
pub fn is_property_like(obj: &PyObjectRef) -> bool {
    match &obj.payload {
        PyObjectPayload::Property(_) => true,
        PyObjectPayload::Instance(inst) => is_property_subclass_class(&inst.class),
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

fn index_to_i128_unbounded(obj: &PyObjectRef) -> PyResult<i128> {
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
    let ai = a.to_int().map_err(|_| {
        PyException::type_error(format!(
            "unsupported operand type(s) for {}: '{}' and '{}'",
            op_name,
            a.type_name(),
            b.type_name()
        ))
    })?;
    let bi = b.to_int().map_err(|_| {
        PyException::type_error(format!(
            "unsupported operand type(s) for {}: '{}' and '{}'",
            op_name,
            a.type_name(),
            b.type_name()
        ))
    })?;
    // Guard against shift overflow (UB when shift >= 64)
    if op_name == "<<" || op_name == ">>" {
        if bi < 0 {
            return Err(PyException::value_error(format!("negative shift count")));
        }
        if bi >= 64 {
            return if op_name == "<<" {
                // Python supports arbitrary-precision left shift — for i64 range this overflows
                Ok(PyObject::int(0)) // all bits shifted out
            } else {
                // Right shift by >= 64: sign-extends to 0 or -1
                Ok(PyObject::int(if ai < 0 { -1 } else { 0 }))
            };
        }
    }
    Ok(PyObject::int(op(ai, bi)))
}

fn dict_maps_equal(a: &FxHashKeyMap, b: &FxHashKeyMap) -> bool {
    let od_key = crate::types::HashableKey::str_key(CompactString::from("__ordered_dict__"));
    let a_is_od = a.contains_key(&od_key);
    let b_is_od = b.contains_key(&od_key);
    if a_is_od && b_is_od {
        let a_items: Vec<_> = a.iter().filter(|(k, _)| !is_hidden_dict_key(k)).collect();
        let b_items: Vec<_> = b.iter().filter(|(k, _)| !is_hidden_dict_key(k)).collect();
        if a_items.len() != b_items.len() {
            return false;
        }
        for ((ak, av), (bk, bv)) in a_items.iter().zip(b_items.iter()) {
            if ak != bk {
                return false;
            }
            if partial_cmp_objects(av, bv) != Some(std::cmp::Ordering::Equal) {
                return false;
            }
        }
        true
    } else {
        let a_effective: Vec<_> = a.iter().filter(|(k, _)| !is_hidden_dict_key(k)).collect();
        let b_effective: Vec<_> = b.iter().filter(|(k, _)| !is_hidden_dict_key(k)).collect();
        if a_effective.len() != b_effective.len() {
            return false;
        }
        for (k, v1) in &a_effective {
            match b.get(*k) {
                Some(v2) if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                _ => return false,
            }
        }
        true
    }
}

#[inline]
pub fn partial_cmp_objects(a: &PyObjectRef, b: &PyObjectRef) -> Option<std::cmp::Ordering> {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::None, PyObjectPayload::None) => Some(std::cmp::Ordering::Equal),
        (PyObjectPayload::Bool(a), PyObjectPayload::Bool(b)) => a.partial_cmp(b),
        (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => a.partial_cmp(b),
        (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => a.partial_cmp(b),
        (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => a.to_f64().partial_cmp(b),
        (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => a.partial_cmp(&b.to_f64()),
        (PyObjectPayload::Str(a), PyObjectPayload::Str(b)) => a.partial_cmp(b),
        (PyObjectPayload::Bool(a), PyObjectPayload::Int(b)) => {
            PyInt::Small(*a as i64).partial_cmp(b)
        }
        (PyObjectPayload::Int(a), PyObjectPayload::Bool(b)) => {
            a.partial_cmp(&PyInt::Small(*b as i64))
        }
        (PyObjectPayload::Bool(a), PyObjectPayload::Float(b)) => (*a as i64 as f64).partial_cmp(b),
        (PyObjectPayload::Float(a), PyObjectPayload::Bool(b)) => a.partial_cmp(&(*b as i64 as f64)),
        (PyObjectPayload::List(a), PyObjectPayload::List(b)) => {
            let a = a.read();
            let b = b.read();
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
            if a == b {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::NativeFunction(a), PyObjectPayload::NativeFunction(b)) => {
            if a.name == b.name {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::NativeClosure(a), PyObjectPayload::NativeClosure(b)) => {
            if a.name == b.name {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::Code(a), PyObjectPayload::Code(b)) => {
            if code_objects_equal(a, b) {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::BuiltinFunction(a), PyObjectPayload::NativeFunction(b)) => {
            if a.as_ref() == b.name {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::NativeFunction(a), PyObjectPayload::BuiltinFunction(b)) => {
            if a.name == b.as_ref() {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::BuiltinFunction(a), PyObjectPayload::NativeClosure(b)) => {
            if a.as_ref() == b.name {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::NativeClosure(a), PyObjectPayload::BuiltinFunction(b)) => {
            if a.name == b.as_ref() {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::NativeFunction(a), PyObjectPayload::NativeClosure(b)) => {
            if a.name == b.name {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::NativeClosure(a), PyObjectPayload::NativeFunction(b)) => {
            if a.name == b.name {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::Bytes(a), PyObjectPayload::Bytes(b)) => a.partial_cmp(b),
        (PyObjectPayload::ByteArray(a), PyObjectPayload::ByteArray(b)) => a.partial_cmp(b),
        (PyObjectPayload::Bytes(a), PyObjectPayload::ByteArray(b))
        | (PyObjectPayload::ByteArray(a), PyObjectPayload::Bytes(b)) => a.partial_cmp(b),
        (
            PyObjectPayload::Complex { real: ar, imag: ai },
            PyObjectPayload::Complex { real: br, imag: bi },
        ) => {
            if ar == br && ai == bi {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Int(n))
        | (PyObjectPayload::Int(n), PyObjectPayload::Complex { real, imag }) => {
            if *imag == 0.0
                && *real == n.to_f64()
                && n.to_i64().map(|i| (*real as i64) == i).unwrap_or(false)
            {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Float(f))
        | (PyObjectPayload::Float(f), PyObjectPayload::Complex { real, imag }) => {
            if *imag == 0.0 && real == f {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Bool(b))
        | (PyObjectPayload::Bool(b), PyObjectPayload::Complex { real, imag }) => {
            let bf = if *b { 1.0 } else { 0.0 };
            if *imag == 0.0 && *real == bf {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::BuiltinType(a), PyObjectPayload::BuiltinType(b)) => {
            if a == b {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
            let a = a.read();
            let b = b.read();
            if a.len() != b.len() {
                return None;
            }
            for k in a.keys() {
                if !b.contains_key(k) {
                    return None;
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        (PyObjectPayload::FrozenSet(a), PyObjectPayload::FrozenSet(b)) => {
            // Set equality: same keys
            if a.len() != b.len() {
                return None;
            }
            for k in a.keys() {
                if !b.contains_key(k) {
                    return None;
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        // frozenset == set cross-type
        (PyObjectPayload::FrozenSet(a), PyObjectPayload::Set(b)) => {
            let rb = b.read();
            if a.len() != rb.len() {
                return None;
            }
            for k in a.keys() {
                if !rb.contains_key(k) {
                    return None;
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        (PyObjectPayload::Set(a), PyObjectPayload::FrozenSet(b)) => {
            let ra = a.read();
            if ra.len() != b.len() {
                return None;
            }
            for k in ra.keys() {
                if !b.contains_key(k) {
                    return None;
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        (PyObjectPayload::Dict(a), PyObjectPayload::Dict(b)) => {
            let a = a.read();
            let b = b.read();
            if dict_maps_equal(&a, &b) {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::Dict(a), PyObjectPayload::MappingProxy(b))
        | (PyObjectPayload::MappingProxy(a), PyObjectPayload::Dict(b))
        | (PyObjectPayload::MappingProxy(a), PyObjectPayload::MappingProxy(b)) => {
            let a = a.read();
            let b = b.read();
            if dict_maps_equal(&a, &b) {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        // Class identity comparison (same Arc pointer = same class)
        (PyObjectPayload::Class(a), PyObjectPayload::Class(b)) => {
            if a.name == b.name {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        // ExceptionType comparison
        (PyObjectPayload::ExceptionType(a), PyObjectPayload::ExceptionType(b)) => {
            if a == b {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::Range(r1), PyObjectPayload::Range(r2)) => {
            // CPython: ranges are equal if they produce the same sequence
            // Simple shortcut: normalize empty ranges
            let len1 = range_len(r1.start, r1.stop, r1.step);
            let len2 = range_len(r2.start, r2.stop, r2.step);
            if len1 == 0 && len2 == 0 {
                return Some(std::cmp::Ordering::Equal);
            }
            if len1 != len2 {
                return None;
            }
            if r1.start != r2.start {
                return None;
            }
            if len1 == 1 {
                return Some(std::cmp::Ordering::Equal);
            }
            if r1.step == r2.step {
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::InstanceDict(a), PyObjectPayload::InstanceDict(b)) => {
            let a = a.read();
            let b = b.read();
            if a.len() != b.len() {
                return None;
            }
            for (k, v1) in a.iter() {
                match b.get(k.as_str()) {
                    Some(v2) if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                    _ => return None,
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        // Cross-type: InstanceDict == Dict
        (PyObjectPayload::InstanceDict(a), PyObjectPayload::Dict(b)) => {
            let a = a.read();
            let b = b.read();
            if a.len() != b.len() {
                return None;
            }
            for (k, v1) in a.iter() {
                let hk = match PyObject::str_val(CompactString::from(k.as_str())).to_hashable_key()
                {
                    Ok(hk) => hk,
                    Err(_) => return None,
                };
                match b.get(&hk) {
                    Some(v2) if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                    _ => return None,
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        (PyObjectPayload::Dict(a_dict), PyObjectPayload::InstanceDict(b_idict)) => {
            let a_r = a_dict.read();
            let b_r = b_idict.read();
            if a_r.len() != b_r.len() {
                return None;
            }
            for (k, v1) in b_r.iter() {
                let hk = match PyObject::str_val(CompactString::from(k.as_str())).to_hashable_key()
                {
                    Ok(hk) => hk,
                    Err(_) => return None,
                };
                match a_r.get(&hk) {
                    Some(v2) if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                    _ => return None,
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        // Instance comparison: check __eq__ method on class (for dataclass, custom __eq__)
        (PyObjectPayload::Instance(inst_a), PyObjectPayload::Instance(inst_b)) => {
            // Check if they are the same object
            if PyObjectRef::ptr_eq(a, b) {
                return Some(std::cmp::Ordering::Equal);
            }
            if inst_a.attrs.read().contains_key("__weakref_ref__")
                || inst_b.attrs.read().contains_key("__weakref_ref__")
            {
                return None;
            }
            // Dict subclass: compare dict_storage contents
            if let (Some(ref ds_a), Some(ref ds_b)) = (&inst_a.dict_storage, &inst_b.dict_storage) {
                let a_r = ds_a.read();
                let b_r = ds_b.read();
                if a_r.len() != b_r.len() {
                    return None;
                }
                for (k, v1) in a_r.iter() {
                    match b_r.get(k) {
                        Some(v2)
                            if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                        _ => return None,
                    }
                }
                return Some(std::cmp::Ordering::Equal);
            }
            // Look for __eq__ in the class hierarchy
            fn find_in_mro(cls: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
                if let PyObjectPayload::Class(cd) = &cls.payload {
                    let ns = cd.namespace.read();
                    if let Some(f) = ns.get(name) {
                        return Some(f.clone());
                    }
                    for base in &cd.mro {
                        if let PyObjectPayload::Class(bcd) = &base.payload {
                            let bns = bcd.namespace.read();
                            if let Some(f) = bns.get(name) {
                                return Some(f.clone());
                            }
                        }
                    }
                }
                None
            }
            if let Some(eq_fn) = find_in_mro(&inst_a.class, "__eq__") {
                match &eq_fn.payload {
                    PyObjectPayload::NativeFunction(nf) => {
                        if let Ok(result) = (nf.func)(&[a.clone(), b.clone()]) {
                            return if result.is_truthy() {
                                Some(std::cmp::Ordering::Equal)
                            } else {
                                None
                            };
                        }
                    }
                    PyObjectPayload::NativeClosure(nc) => {
                        if let Ok(result) = (nc.func)(&[a.clone(), b.clone()]) {
                            return if result.is_truthy() {
                                Some(std::cmp::Ordering::Equal)
                            } else {
                                None
                            };
                        }
                    }
                    _ => {}
                }
            }
            // For __lt__ comparison (used by sorted), also check
            if let Some(lt_fn) = find_in_mro(&inst_a.class, "__lt__") {
                match &lt_fn.payload {
                    PyObjectPayload::NativeFunction(nf) => {
                        if let Ok(result) = (nf.func)(&[a.clone(), b.clone()]) {
                            if result.is_truthy() {
                                return Some(std::cmp::Ordering::Less);
                            }
                        }
                        if let Ok(result) = (nf.func)(&[b.clone(), a.clone()]) {
                            if result.is_truthy() {
                                return Some(std::cmp::Ordering::Greater);
                            }
                        }
                        return Some(std::cmp::Ordering::Equal);
                    }
                    PyObjectPayload::NativeClosure(nc) => {
                        if let Ok(result) = (nc.func)(&[a.clone(), b.clone()]) {
                            if result.is_truthy() {
                                return Some(std::cmp::Ordering::Less);
                            }
                        }
                        if let Ok(result) = (nc.func)(&[b.clone(), a.clone()]) {
                            if result.is_truthy() {
                                return Some(std::cmp::Ordering::Greater);
                            }
                        }
                        return Some(std::cmp::Ordering::Equal);
                    }
                    _ => {}
                }
            }
            None
        }
        // Dict subclass (Instance with dict_storage) vs Dict
        (PyObjectPayload::Instance(inst), PyObjectPayload::Dict(b_dict)) => {
            if let Some(ref ds) = inst.dict_storage {
                let a_r = ds.read();
                let b_r = b_dict.read();
                if a_r.len() != b_r.len() {
                    return None;
                }
                for (k, v1) in a_r.iter() {
                    match b_r.get(k) {
                        Some(v2)
                            if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                        _ => return None,
                    }
                }
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        (PyObjectPayload::Dict(a_dict), PyObjectPayload::Instance(inst)) => {
            if let Some(ref ds) = inst.dict_storage {
                let a_r = a_dict.read();
                let b_r = ds.read();
                if a_r.len() != b_r.len() {
                    return None;
                }
                for (k, v1) in a_r.iter() {
                    match b_r.get(k) {
                        Some(v2)
                            if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                        _ => return None,
                    }
                }
                Some(std::cmp::Ordering::Equal)
            } else {
                None
            }
        }
        _ => None,
    }
}

mod formatting;
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

/// Resolve known built-in type methods that can be defined without VM access.
/// This is used by super() resolution when a base is a BuiltinType.
pub fn resolve_builtin_type_method(type_name: &str, method_name: &str) -> Option<PyObjectRef> {
    match (type_name, method_name) {
        ("property", "__get__") => Some(PyObject::native_function("property.__get__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "descriptor '__get__' requires a property object",
                ));
            }
            let prop = &args[0];
            let obj = args.get(1);
            let obj = match obj {
                Some(o) if !matches!(&o.payload, PyObjectPayload::None) => o,
                _ => return Ok(prop.clone()),
            };
            if let Some(getter) = property_field(prop, "fget") {
                if !matches!(&getter.payload, PyObjectPayload::None) {
                    return Ok(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BoundMethod {
                            receiver: obj.clone(),
                            method: getter,
                        },
                    }));
                }
            }
            Err(PyException::attribute_error("unreadable attribute"))
        })),
        ("type", "__new__") => Some(PyObject::native_function("type.__new__", |args| {
            // type.__new__(mcs, name, bases, dict) or type(name, bases, dict)
            if args.len() == 4 {
                let name = args[1].as_str().ok_or_else(|| {
                    PyException::type_error("type.__new__ argument 2 must be str")
                })?;
                let bases = args[2].to_list()?;
                let namespace = match &args[3].payload {
                    PyObjectPayload::Dict(m) => {
                        let r = m.read();
                        let mut ns = FxAttrMap::default();
                        for (k, v) in r.iter() {
                            let key_str = match k {
                                HashableKey::Str(s) => s.to_compact_string(),
                                _ => CompactString::from(k.to_object().py_to_string()),
                            };
                            ns.insert(key_str, v.clone());
                        }
                        ns
                    }
                    _ => {
                        return Err(PyException::type_error(
                            "type.__new__ argument 4 must be dict",
                        ))
                    }
                };
                let mut mro = Vec::new();
                for base in &bases {
                    mro.push(base.clone());
                    if let PyObjectPayload::Class(cd) = &base.payload {
                        for m in &cd.mro {
                            if !mro.iter().any(|existing| PyObjectRef::ptr_eq(existing, m)) {
                                mro.push(m.clone());
                            }
                        }
                    }
                }
                Ok(PyObject::wrap(PyObjectPayload::Class(Box::new(
                    ClassData::new(CompactString::from(name), bases, namespace, mro, None),
                ))))
            } else if args.len() == 3 {
                // type(name, bases, dict) — no mcs
                let name = args[0]
                    .as_str()
                    .ok_or_else(|| PyException::type_error("type() argument 1 must be str"))?;
                let bases = args[1].to_list()?;
                let namespace = match &args[2].payload {
                    PyObjectPayload::Dict(m) => {
                        let r = m.read();
                        let mut ns = FxAttrMap::default();
                        for (k, v) in r.iter() {
                            let key_str = match k {
                                HashableKey::Str(s) => s.to_compact_string(),
                                _ => CompactString::from(k.to_object().py_to_string()),
                            };
                            ns.insert(key_str, v.clone());
                        }
                        ns
                    }
                    _ => return Err(PyException::type_error("type() argument 3 must be dict")),
                };
                let mut mro = Vec::new();
                for base in &bases {
                    mro.push(base.clone());
                    if let PyObjectPayload::Class(cd) = &base.payload {
                        for m in &cd.mro {
                            if !mro.iter().any(|existing| PyObjectRef::ptr_eq(existing, m)) {
                                mro.push(m.clone());
                            }
                        }
                    }
                }
                Ok(PyObject::wrap(PyObjectPayload::Class(Box::new(
                    ClassData::new(CompactString::from(name), bases, namespace, mro, None),
                ))))
            } else {
                Err(PyException::type_error(
                    "type.__new__ requires 3 or 4 arguments",
                ))
            }
        })),
        // tuple.__new__(cls, iterable) — create tuple subclass instance with __builtin_value__
        ("tuple", "__new__") => Some(PyObject::native_function("tuple.__new__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("tuple.__new__ requires cls"));
            }
            let cls = &args[0];
            let inst = PyObject::instance(cls.clone());
            let items = if args.len() > 2 {
                // Multiple positional args (namedtuple-style): use all as items
                args[1..].to_vec()
            } else if args.len() == 2 {
                // Single arg: try to expand as iterable, else wrap
                args[1].to_list().unwrap_or_else(|_| vec![args[1].clone()])
            } else {
                vec![]
            };
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                inst_data
                    .attrs
                    .write()
                    .insert(intern_or_new("__builtin_value__"), PyObject::tuple(items));
            }
            Ok(inst)
        })),
        // list.__new__(cls, iterable) — create list subclass instance with __builtin_value__
        ("list", "__new__") => Some(PyObject::native_function("list.__new__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("list.__new__ requires cls"));
            }
            let cls = &args[0];
            let inst = PyObject::instance(cls.clone());
            let items = if args.len() > 1 {
                args[1].to_list().unwrap_or_default()
            } else {
                vec![]
            };
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                inst_data
                    .attrs
                    .write()
                    .insert(intern_or_new("__builtin_value__"), PyObject::list(items));
            }
            Ok(inst)
        })),
        ("str", "__new__") => Some(PyObject::native_function("str.__new__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("str.__new__ requires cls"));
            }
            let cls = &args[0];
            let value = if args.len() > 1 {
                args[1].py_to_string()
            } else {
                String::new()
            };
            let inst = PyObject::instance(cls.clone());
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                inst_data.attrs.write().insert(
                    intern_or_new("__builtin_value__"),
                    PyObject::str_val(CompactString::from(value)),
                );
            }
            Ok(inst)
        })),
        ("int", "__new__") => Some(PyObject::native_function("int.__new__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("int.__new__ requires cls"));
            }
            let cls = &args[0];
            // CPython: int.__new__(bool, ...) is not allowed
            if let PyObjectPayload::BuiltinType(name) = &cls.payload {
                if name.as_str() == "bool" {
                    return Err(PyException::type_error(
                        "int.__new__(bool) is not safe, use bool.__new__()",
                    ));
                }
            }
            let value = if args.len() > 1 { args[1].to_int()? } else { 0 };
            let inst = PyObject::instance(cls.clone());
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                inst_data
                    .attrs
                    .write()
                    .insert(intern_or_new("__builtin_value__"), PyObject::int(value));
            }
            Ok(inst)
        })),
        ("float", "__new__") => Some(PyObject::native_function("float.__new__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("float.__new__ requires cls"));
            }
            let cls = &args[0];
            let value = if args.len() > 1 {
                match &args[1].payload {
                    PyObjectPayload::Float(f) => *f,
                    PyObjectPayload::Int(n) => n.to_f64(),
                    PyObjectPayload::Bool(b) => {
                        if *b {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    PyObjectPayload::Str(s) => s.parse::<f64>().map_err(|_| {
                        PyException::value_error(format!(
                            "could not convert string to float: '{}'",
                            s
                        ))
                    })?,
                    _ => {
                        return Err(PyException::type_error(format!(
                            "float() argument must be a string or a number, not '{}'",
                            args[1].type_name()
                        )))
                    }
                }
            } else {
                0.0
            };
            let inst = PyObject::instance(cls.clone());
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                inst_data
                    .attrs
                    .write()
                    .insert(intern_or_new("__builtin_value__"), PyObject::float(value));
            }
            Ok(inst)
        })),
        ("complex", "__new__") => Some(PyObject::native_function("complex.__new__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("complex.__new__ requires cls"));
            }
            let cls = &args[0];
            // Extract (real, imag) from up to 2 more args
            let to_ri = |o: &PyObjectRef| -> (f64, f64) {
                match &o.payload {
                    PyObjectPayload::Complex { real, imag } => (*real, *imag),
                    PyObjectPayload::Int(n) => (n.to_f64(), 0.0),
                    PyObjectPayload::Float(f) => (*f, 0.0),
                    PyObjectPayload::Bool(b) => (if *b { 1.0 } else { 0.0 }, 0.0),
                    _ => (0.0, 0.0),
                }
            };
            let is_complex =
                |o: &PyObjectRef| matches!(&o.payload, PyObjectPayload::Complex { .. });
            let (real, imag) = match (args.get(1), args.get(2)) {
                (None, _) => (0.0, 0.0),
                (Some(a), None) => to_ri(a),
                (Some(a), Some(b)) => {
                    let (ar, ai) = to_ri(a);
                    let (br, bi) = to_ri(b);
                    let r = if is_complex(b) { ar - bi } else { ar };
                    let i = if is_complex(a) { ai + br } else { br };
                    (r, i)
                }
            };
            let inst = PyObject::instance(cls.clone());
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                inst_data.attrs.write().insert(
                    intern_or_new("__builtin_value__"),
                    PyObject::complex(real, imag),
                );
            }
            Ok(inst)
        })),
        ("object", "__new__") => Some(PyObject::native_function("object.__new__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("object.__new__ requires cls"));
            }
            Ok(PyObject::instance(args[0].clone()))
        })),
        // property.__init__(self, fget=None, fset=None, fdel=None, doc=None)
        // Store fget/fset/fdel on Instance attrs so property subclasses work
        ("property", "__init__") => Some(PyObject::native_function("property.__init__", |args| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            let property_arg = |idx: usize| {
                args.get(idx).and_then(|arg| {
                    if matches!(&arg.payload, PyObjectPayload::None) {
                        None
                    } else {
                        Some(arg.clone())
                    }
                })
            };
            let fget = property_arg(1);
            let fset = property_arg(2);
            let fdel = property_arg(3);
            if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                let mut w = inst.attrs.write();
                w.insert(
                    CompactString::from("fget"),
                    fget.clone().unwrap_or_else(PyObject::none),
                );
                w.insert(
                    CompactString::from("fset"),
                    fset.clone().unwrap_or_else(PyObject::none),
                );
                w.insert(
                    CompactString::from("fdel"),
                    fdel.clone().unwrap_or_else(PyObject::none),
                );
            }
            let (doc, doc_from_getter) = property_init_doc(fget.as_ref(), args.get(4).cloned());
            if let Some(doc) = doc {
                property_set_doc(&args[0], doc)?;
            }
            if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                inst.attrs.write().insert(
                    CompactString::from("__property_doc_from_getter__"),
                    PyObject::bool_val(doc_from_getter),
                );
            }
            Ok(PyObject::none())
        })),
        // dict.__init__(self, data=None, **kwargs) — populate dict_storage from positional/kw args
        ("dict", "__init__") => Some(PyObject::native_function("dict.__init__", |args| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            let self_obj = &args[0];
            if let PyObjectPayload::Instance(inst) = &self_obj.payload {
                if let Some(ref ds) = inst.dict_storage {
                    let mut storage = ds.write();
                    // If there's a positional arg (a dict or iterable of pairs), copy entries
                    if args.len() >= 2 {
                        match &args[1].payload {
                            PyObjectPayload::Dict(src) => {
                                for (k, v) in src.read().iter() {
                                    storage.insert(k.clone(), v.clone());
                                }
                            }
                            PyObjectPayload::Instance(src_inst)
                                if src_inst.dict_storage.is_some() =>
                            {
                                if let Some(src_ds) = src_inst.dict_storage.as_ref() {
                                    for (k, v) in src_ds.read().iter() {
                                        storage.insert(k.clone(), v.clone());
                                    }
                                }
                            }
                            _ => {
                                // Try treating as iterable of (key, value) pairs
                                if let Ok(items) = args[1].to_list() {
                                    for item in &items {
                                        if let Ok(pair) = item.to_list() {
                                            if pair.len() == 2 {
                                                let hk = pair[0].to_hashable_key()?;
                                                storage.insert(hk, pair[1].clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Ok(PyObject::none())
        })),
        // __init__ on any builtin type base is a no-op (instance already created)
        (_, "__init__") => Some(PyObject::native_function("builtin.__init__", |_args| {
            Ok(PyObject::none())
        })),
        // dict.__getitem__(self, key) — access dict_storage on dict subclass
        ("dict", "__getitem__") => Some(PyObject::native_function("dict.__getitem__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error(
                    "dict.__getitem__() takes exactly 2 arguments",
                ));
            }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(ref ds) = inst.dict_storage {
                    let hk = args[1].to_hashable_key()?;
                    if let Some(val) = ds.read().get(&hk) {
                        return Ok(val.clone());
                    }
                    return Err(PyException::key_error(args[1].py_to_string()));
                }
            }
            Err(PyException::type_error(
                "dict.__getitem__ requires a dict instance",
            ))
        })),
        // dict.__setitem__(self, key, value)
        ("dict", "__setitem__") => Some(PyObject::native_function("dict.__setitem__", |args| {
            if args.len() != 3 {
                return Err(PyException::type_error(
                    "dict.__setitem__() takes exactly 3 arguments",
                ));
            }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(ref ds) = inst.dict_storage {
                    let hk = args[1].to_hashable_key()?;
                    ds.write().insert(hk, args[2].clone());
                    return Ok(PyObject::none());
                }
            }
            Err(PyException::type_error(
                "dict.__setitem__ requires a dict instance",
            ))
        })),
        // dict.__delitem__(self, key)
        ("dict", "__delitem__") => Some(PyObject::native_function("dict.__delitem__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error(
                    "dict.__delitem__() takes exactly 2 arguments",
                ));
            }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(ref ds) = inst.dict_storage {
                    let hk = args[1].to_hashable_key()?;
                    if ds.write().shift_remove(&hk).is_some() {
                        return Ok(PyObject::none());
                    }
                    return Err(PyException::key_error(args[1].py_to_string()));
                }
            }
            Err(PyException::type_error(
                "dict.__delitem__ requires a dict instance",
            ))
        })),
        // dict.__contains__(self, key)
        ("dict", "__contains__") => Some(PyObject::native_function("dict.__contains__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error(
                    "dict.__contains__() takes exactly 2 arguments",
                ));
            }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(ref ds) = inst.dict_storage {
                    let hk = args[1].to_hashable_key()?;
                    return Ok(PyObject::bool_val(ds.read().contains_key(&hk)));
                }
            }
            Ok(PyObject::bool_val(false))
        })),
        // Arithmetic dunder wrappers for builtin types (unbound method access)
        (_, "__add__") => Some(PyObject::native_function("__add__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__add__ takes 2 arguments"));
            }
            args[0].add(&args[1])
        })),
        (_, "__sub__") => Some(PyObject::native_function("__sub__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__sub__ takes 2 arguments"));
            }
            args[0].sub(&args[1])
        })),
        (_, "__mul__") => Some(PyObject::native_function("__mul__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__mul__ takes 2 arguments"));
            }
            args[0].mul(&args[1])
        })),
        (_, "__truediv__") => Some(PyObject::native_function("__truediv__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__truediv__ takes 2 arguments"));
            }
            args[0].true_div(&args[1])
        })),
        (_, "__floordiv__") => Some(PyObject::native_function("__floordiv__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__floordiv__ takes 2 arguments"));
            }
            args[0].floor_div(&args[1])
        })),
        (_, "__mod__") => Some(PyObject::native_function("__mod__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__mod__ takes 2 arguments"));
            }
            args[0].modulo(&args[1])
        })),
        (_, "__eq__") => Some(PyObject::native_function("__eq__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__eq__ takes 2 arguments"));
            }
            args[0].compare(&args[1], CompareOp::Eq)
        })),
        (_, "__ne__") => Some(PyObject::native_function("__ne__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__ne__ takes 2 arguments"));
            }
            args[0].compare(&args[1], CompareOp::Ne)
        })),
        (_, "__lt__") => Some(PyObject::native_function("__lt__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__lt__ takes 2 arguments"));
            }
            if matches!(&args[0].payload, PyObjectPayload::Complex { .. }) {
                return Ok(PyObject::not_implemented());
            }
            args[0].compare(&args[1], CompareOp::Lt)
        })),
        (_, "__le__") => Some(PyObject::native_function("__le__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__le__ takes 2 arguments"));
            }
            if matches!(&args[0].payload, PyObjectPayload::Complex { .. }) {
                return Ok(PyObject::not_implemented());
            }
            args[0].compare(&args[1], CompareOp::Le)
        })),
        (_, "__gt__") => Some(PyObject::native_function("__gt__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__gt__ takes 2 arguments"));
            }
            if matches!(&args[0].payload, PyObjectPayload::Complex { .. }) {
                return Ok(PyObject::not_implemented());
            }
            args[0].compare(&args[1], CompareOp::Gt)
        })),
        (_, "__ge__") => Some(PyObject::native_function("__ge__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__ge__ takes 2 arguments"));
            }
            if matches!(&args[0].payload, PyObjectPayload::Complex { .. }) {
                return Ok(PyObject::not_implemented());
            }
            args[0].compare(&args[1], CompareOp::Ge)
        })),
        (_, "__neg__") => Some(PyObject::native_function("__neg__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__neg__ takes 1 argument"));
            }
            args[0].negate()
        })),
        (_, "__abs__") => Some(PyObject::native_function("__abs__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__abs__ takes 1 argument"));
            }
            args[0].py_abs()
        })),
        (_, "__len__") => Some(PyObject::native_function("__len__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__len__ takes 1 argument"));
            }
            Ok(PyObject::int(args[0].py_len()? as i64))
        })),
        (_, "__contains__") => Some(PyObject::native_function("__contains__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__contains__ takes 2 arguments"));
            }
            Ok(PyObject::bool_val(args[0].contains(&args[1])?))
        })),
        (_, "__repr__") => Some(PyObject::native_function("__repr__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__repr__ takes 1 argument"));
            }
            Ok(PyObject::str_val(CompactString::from(args[0].repr())))
        })),
        (_, "__str__") => Some(PyObject::native_function("__str__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__str__ takes 1 argument"));
            }
            Ok(PyObject::str_val(CompactString::from(
                args[0].py_to_string(),
            )))
        })),
        (_, "__hash__") => Some(PyObject::native_function("__hash__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__hash__ takes 1 argument"));
            }
            let value = unwrap_builtin_subclass(&args[0]);
            if let PyObjectPayload::Int(n) = &value.payload {
                return Ok(n.to_object());
            }
            if let PyObjectPayload::Bool(b) = &value.payload {
                return Ok(PyObject::int(*b as i64));
            }
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let hk = value.to_hashable_key()?;
            let mut hasher = DefaultHasher::new();
            hk.hash(&mut hasher);
            Ok(PyObject::int(hasher.finish() as i64))
        })),
        (_, "__bool__") => Some(PyObject::native_function("__bool__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__bool__ takes 1 argument"));
            }
            Ok(PyObject::bool_val(args[0].is_truthy()))
        })),
        (_, "__pow__") => Some(PyObject::native_function("__pow__", |args| {
            if args.len() < 2 || args.len() > 3 {
                return Err(PyException::type_error("__pow__ takes 2-3 arguments"));
            }
            args[0].power(&args[1])
        })),
        (_, "__lshift__") => Some(PyObject::native_function("__lshift__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__lshift__ takes 2 arguments"));
            }
            args[0].lshift(&args[1])
        })),
        (_, "__rshift__") => Some(PyObject::native_function("__rshift__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__rshift__ takes 2 arguments"));
            }
            args[0].rshift(&args[1])
        })),
        (_, "__and__") => Some(PyObject::native_function("__and__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__and__ takes 2 arguments"));
            }
            args[0].bit_and(&args[1])
        })),
        (_, "__or__") => Some(PyObject::native_function("__or__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__or__ takes 2 arguments"));
            }
            args[0].bit_or(&args[1])
        })),
        (_, "__xor__") => Some(PyObject::native_function("__xor__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__xor__ takes 2 arguments"));
            }
            args[0].bit_xor(&args[1])
        })),
        (_, "__pos__") => Some(PyObject::native_function("__pos__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__pos__ takes 1 argument"));
            }
            args[0].positive()
        })),
        (_, "__invert__") => Some(PyObject::native_function("__invert__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__invert__ takes 1 argument"));
            }
            args[0].invert()
        })),
        (_, "__getitem__") => Some(PyObject::native_function("__getitem__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__getitem__ takes 2 arguments"));
            }
            args[0].get_item(&args[1])
        })),
        (_, "__int__") => Some(PyObject::native_function("__int__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__int__ takes 1 argument"));
            }
            Ok(PyObject::int(args[0].to_int()?))
        })),
        (_, "__float__") => Some(PyObject::native_function("__float__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__float__ takes 1 argument"));
            }
            Ok(PyObject::float(args[0].to_float()?))
        })),
        (_, "__index__") => Some(PyObject::native_function("__index__", |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__index__ takes 1 argument"));
            }
            Ok(PyObject::int(args[0].to_int()?))
        })),
        (_, "__iter__") => None, // handled by VM iter() builtin
        (_, "__sizeof__") => Some(PyObject::native_function("__sizeof__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("__sizeof__ takes 1 argument"));
            }
            let size = std::mem::size_of::<PyObject>() as i64
                + match &args[0].payload {
                    PyObjectPayload::Str(s) => s.len() as i64,
                    PyObjectPayload::Bytes(b) => b.len() as i64,
                    PyObjectPayload::List(items) => {
                        (items.read().len() * std::mem::size_of::<PyObjectRef>()) as i64
                    }
                    PyObjectPayload::Dict(map) => (map.read().len() * 64) as i64,
                    PyObjectPayload::Set(set) => (set.read().len() * 32) as i64,
                    PyObjectPayload::Tuple(items) => {
                        (items.len() * std::mem::size_of::<PyObjectRef>()) as i64
                    }
                    _ => 0,
                };
            Ok(PyObject::int(size))
        })),
        _ => None,
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
