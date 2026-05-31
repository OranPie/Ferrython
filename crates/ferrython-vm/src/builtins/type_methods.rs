//! Collection/numeric type method dispatch (list, dict, set, tuple, int, float, bytes)

use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::{take_pending_eq_error, HashableKey};

mod type_bytes;
mod type_mappings;
mod type_numeric;
mod type_sequences;
mod type_sets;

pub(crate) use type_bytes::{call_bytearray_method, call_bytes_method};
pub(crate) use type_mappings::call_dict_method;
pub(crate) use type_numeric::{call_bool_method, call_float_method, call_int_method};
pub(crate) use type_sequences::{call_list_method, call_range_method, call_tuple_method};
pub(crate) use type_sets::{call_frozenset_method, call_set_method};

pub(super) fn collect_hash_entries(arg: &PyObjectRef) -> PyResult<Vec<(HashableKey, PyObjectRef)>> {
    match &arg.payload {
        PyObjectPayload::Dict(items) => {
            let read = items.read();
            Ok(read
                .keys()
                .map(|key| (key.clone(), key.to_object()))
                .collect())
        }
        PyObjectPayload::Set(items) => {
            let read = items.read();
            Ok(read
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect())
        }
        PyObjectPayload::FrozenSet(items) => Ok(items
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()),
        _ => {
            let items = arg.to_list()?;
            let mut entries = Vec::with_capacity(items.len());
            for item in items {
                let key = item.to_hashable_key()?;
                entries.push((key, item));
                check_key_error()?;
            }
            Ok(entries)
        }
    }
}

#[inline]
pub(super) fn check_key_error() -> PyResult<()> {
    match take_pending_eq_error() {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Extract a keyword argument from a trailing kwargs dict (if present).
/// The generic BuiltinBoundMethod kwargs handler passes kwargs as a trailing Dict arg.
pub(super) fn extract_kwarg(args: &[PyObjectRef], name: &str) -> Option<PyObjectRef> {
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(map) = &last.payload {
            let r = map.read();
            return r
                .get(&HashableKey::str_key(CompactString::from(name)))
                .cloned();
        }
    }
    None
}

pub(crate) fn partial_cmp_for_sort(a: &PyObjectRef, b: &PyObjectRef) -> Option<std::cmp::Ordering> {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(x), PyObjectPayload::Int(y)) => x.partial_cmp(y),
        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => x.partial_cmp(y),
        (PyObjectPayload::Int(x), PyObjectPayload::Float(y)) => x.to_f64().partial_cmp(y),
        (PyObjectPayload::Float(x), PyObjectPayload::Int(y)) => x.partial_cmp(&y.to_f64()),
        (PyObjectPayload::Str(x), PyObjectPayload::Str(y)) => x.partial_cmp(y),
        (PyObjectPayload::Bytes(x), PyObjectPayload::Bytes(y)) => x.partial_cmp(y),
        (PyObjectPayload::Bool(x), PyObjectPayload::Bool(y)) => x.partial_cmp(y),
        (PyObjectPayload::Tuple(x), PyObjectPayload::Tuple(y)) => {
            for (a_item, b_item) in x.iter().zip(y.iter()) {
                match partial_cmp_for_sort(a_item, b_item) {
                    Some(std::cmp::Ordering::Equal) => continue,
                    other => return other,
                }
            }
            x.len().partial_cmp(&y.len())
        }
        (PyObjectPayload::List(x), PyObjectPayload::List(y)) => {
            let x = x.read();
            let y = y.read();
            for (a_item, b_item) in x.iter().zip(y.iter()) {
                match partial_cmp_for_sort(a_item, b_item) {
                    Some(std::cmp::Ordering::Equal) => continue,
                    other => return other,
                }
            }
            x.len().partial_cmp(&y.len())
        }
        // Custom objects — can't call __lt__ from here (no VM), return None
        // so callers fall back to default ordering
        (PyObjectPayload::Instance(_), _) | (_, PyObjectPayload::Instance(_)) => None,
        _ => None,
    }
}
