//! Comparison methods.

use crate::error::{PyException, PyResult};

use super::helpers::{call_callable, partial_cmp_objects, unwrap_builtin_subclass};
use super::methods::{CompareOp, PyObjectMethods};
use super::methods_attr::{lookup_in_class_mro, wrap_class_attr_for_instance};
use super::payload::*;
use compact_str::CompactString;
use std::cell::RefCell;

thread_local! {
    static ACTIVE_CONTAINER_COMPARISONS: RefCell<Vec<ComparisonKey>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ComparisonKey {
    left: usize,
    right: usize,
    op: CompareOp,
}

struct ContainerComparisonGuard {
    key: ComparisonKey,
}

impl Drop for ContainerComparisonGuard {
    fn drop(&mut self) {
        ACTIVE_CONTAINER_COMPARISONS.with(|active| {
            let mut active = active.borrow_mut();
            if let Some(pos) = active.iter().rposition(|key| *key == self.key) {
                active.remove(pos);
            }
        });
    }
}

fn enter_container_comparison(
    a: &PyObjectRef,
    b: &PyObjectRef,
    op: CompareOp,
) -> PyResult<ContainerComparisonGuard> {
    let mut left = PyObjectRef::as_ptr(a) as usize;
    let mut right = PyObjectRef::as_ptr(b) as usize;
    if right < left {
        std::mem::swap(&mut left, &mut right);
    }
    let key = ComparisonKey { left, right, op };
    ACTIVE_CONTAINER_COMPARISONS.with(|active| {
        let mut active = active.borrow_mut();
        if active.contains(&key) {
            return Err(PyException::recursion_error(
                "maximum recursion depth exceeded in comparison",
            ));
        }
        active.push(key);
        Ok(ContainerComparisonGuard { key })
    })
}

fn compare_len_order(a_len: usize, b_len: usize, op: CompareOp) -> bool {
    match op {
        CompareOp::Eq => a_len == b_len,
        CompareOp::Ne => a_len != b_len,
        CompareOp::Lt => a_len < b_len,
        CompareOp::Le => a_len <= b_len,
        CompareOp::Gt => a_len > b_len,
        CompareOp::Ge => a_len >= b_len,
    }
}

fn compare_ordering(ordering: std::cmp::Ordering, op: CompareOp) -> bool {
    match op {
        CompareOp::Eq => ordering == std::cmp::Ordering::Equal,
        CompareOp::Ne => ordering != std::cmp::Ordering::Equal,
        CompareOp::Lt => ordering == std::cmp::Ordering::Less,
        CompareOp::Le => matches!(
            ordering,
            std::cmp::Ordering::Less | std::cmp::Ordering::Equal
        ),
        CompareOp::Gt => ordering == std::cmp::Ordering::Greater,
        CompareOp::Ge => matches!(
            ordering,
            std::cmp::Ordering::Greater | std::cmp::Ordering::Equal
        ),
    }
}

fn compare_op_symbol(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Lt => "<",
        CompareOp::Le => "<=",
        CompareOp::Eq => "==",
        CompareOp::Ne => "!=",
        CompareOp::Gt => ">",
        CompareOp::Ge => ">=",
    }
}

fn set_order_type_error(a: &PyObjectRef, b: &PyObjectRef, op: CompareOp) -> PyException {
    PyException::type_error(format!(
        "'{}' not supported between instances of '{}' and '{}'",
        compare_op_symbol(op),
        a.type_name(),
        b.type_name()
    ))
}

fn is_set_like(obj: &PyObjectRef) -> bool {
    matches!(
        obj.payload,
        PyObjectPayload::Set(_) | PyObjectPayload::FrozenSet(_)
    )
}

fn instance_class(obj: &PyObjectRef) -> Option<PyObjectRef> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        Some(inst.class.clone())
    } else {
        None
    }
}

fn class_is_strict_subclass(child: &PyObjectRef, parent: &PyObjectRef) -> bool {
    if PyObjectRef::ptr_eq(child, parent) {
        return false;
    }
    if let PyObjectPayload::Class(cd) = &child.payload {
        cd.mro.iter().any(|base| PyObjectRef::ptr_eq(base, parent))
    } else {
        false
    }
}

fn is_recursive_container_pair(a: &PyObjectRef, b: &PyObjectRef) -> bool {
    matches!(
        (&a.payload, &b.payload),
        (PyObjectPayload::List(_), PyObjectPayload::List(_))
            | (PyObjectPayload::Tuple(_), PyObjectPayload::Tuple(_))
            | (PyObjectPayload::Dict(_), PyObjectPayload::Dict(_))
            | (PyObjectPayload::DictItems(_), PyObjectPayload::DictItems(_))
            | (
                PyObjectPayload::MappingProxy(_),
                PyObjectPayload::MappingProxy(_)
            )
            | (PyObjectPayload::Dict(_), PyObjectPayload::MappingProxy(_))
            | (PyObjectPayload::MappingProxy(_), PyObjectPayload::Dict(_))
    )
}

fn compare_sequence_items(
    a_items: &[PyObjectRef],
    b_items: &[PyObjectRef],
    op: CompareOp,
) -> PyResult<PyObjectRef> {
    if matches!(op, CompareOp::Eq | CompareOp::Ne) && a_items.len() != b_items.len() {
        return Ok(PyObject::bool_val(matches!(op, CompareOp::Ne)));
    }

    for (left, right) in a_items.iter().zip(b_items.iter()) {
        if PyObjectRef::ptr_eq(left, right) {
            continue;
        }

        if !is_recursive_container_pair(left, right) {
            if let Some(ordering) = partial_cmp_objects(left, right) {
                if ordering == std::cmp::Ordering::Equal {
                    continue;
                }
                return Ok(PyObject::bool_val(compare_ordering(ordering, op)));
            }
        }

        let eq = left.compare(right, CompareOp::Eq)?.is_truthy();
        if eq {
            continue;
        }
        if matches!(op, CompareOp::Eq | CompareOp::Ne) {
            return Ok(PyObject::bool_val(matches!(op, CompareOp::Ne)));
        }
        return left.compare(right, op);
    }

    Ok(PyObject::bool_val(compare_len_order(
        a_items.len(),
        b_items.len(),
        op,
    )))
}

fn compare_dict_value_equal(left: &PyObjectRef, right: &PyObjectRef) -> PyResult<bool> {
    if PyObjectRef::ptr_eq(left, right) {
        return Ok(true);
    }
    if !is_recursive_container_pair(left, right) {
        if let Some(ordering) = partial_cmp_objects(left, right) {
            return Ok(ordering == std::cmp::Ordering::Equal);
        }
    }
    Ok(left.compare(right, CompareOp::Eq)?.is_truthy())
}

fn compare_dict_maps_equal(a: &FxHashKeyMap, b: &FxHashKeyMap) -> PyResult<bool> {
    let od_key = crate::types::HashableKey::str_key(CompactString::from("__ordered_dict__"));
    let a_is_od = a.contains_key(&od_key);
    let b_is_od = b.contains_key(&od_key);
    if a_is_od && b_is_od {
        let a_items: Vec<_> = a
            .iter()
            .filter(|(k, _)| !super::helpers::is_hidden_dict_key(k))
            .collect();
        let b_items: Vec<_> = b
            .iter()
            .filter(|(k, _)| !super::helpers::is_hidden_dict_key(k))
            .collect();
        if a_items.len() != b_items.len() {
            return Ok(false);
        }
        for ((ak, av), (bk, bv)) in a_items.iter().zip(b_items.iter()) {
            if ak != bk || !compare_dict_value_equal(av, bv)? {
                return Ok(false);
            }
        }
        return Ok(true);
    }

    let a_effective: Vec<_> = a
        .iter()
        .filter(|(k, _)| !super::helpers::is_hidden_dict_key(k))
        .collect();
    let b_effective: Vec<_> = b
        .iter()
        .filter(|(k, _)| !super::helpers::is_hidden_dict_key(k))
        .collect();
    if a_effective.len() != b_effective.len() {
        return Ok(false);
    }
    for (key, left) in &a_effective {
        let Some(right) = b.get(*key) else {
            return Ok(false);
        };
        if !compare_dict_value_equal(left, right)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn compare_dict_objects(
    a: &PyObjectRef,
    b: &PyObjectRef,
    a_map: &FxHashKeyMap,
    b_map: &FxHashKeyMap,
    op: CompareOp,
) -> PyResult<PyObjectRef> {
    if !matches!(op, CompareOp::Eq | CompareOp::Ne) {
        return Err(set_order_type_error(a, b, op));
    }
    let _comparison_guard = enter_container_comparison(a, b, op)?;
    let eq = compare_dict_maps_equal(a_map, b_map)?;
    Ok(PyObject::bool_val(if matches!(op, CompareOp::Eq) {
        eq
    } else {
        !eq
    }))
}

fn instance_dict_storage(obj: &PyObjectRef) -> Option<&std::rc::Rc<PyCell<FxHashKeyMap>>> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        inst.dict_storage.as_ref()
    } else {
        None
    }
}

fn is_generic_alias_instance(obj: &PyObjectRef) -> bool {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        if let PyObjectPayload::Class(cd) = &inst.class.payload {
            return cd.name.contains("GenericAlias") || cd.name.contains("_GenericAlias");
        }
    }
    false
}

fn compare_generic_alias_objects(
    a: &PyObjectRef,
    b: &PyObjectRef,
    op: CompareOp,
) -> PyResult<Option<PyObjectRef>> {
    if !matches!(op, CompareOp::Eq | CompareOp::Ne) {
        return Ok(None);
    }
    if !is_generic_alias_instance(a) || !is_generic_alias_instance(b) {
        return Ok(None);
    }

    let (a_origin, a_args, b_origin, b_args) = match (&a.payload, &b.payload) {
        (PyObjectPayload::Instance(a_inst), PyObjectPayload::Instance(b_inst)) => {
            let a_attrs = a_inst.attrs.read();
            let b_attrs = b_inst.attrs.read();
            let (Some(a_origin), Some(a_args), Some(b_origin), Some(b_args)) = (
                a_attrs.get("__origin__"),
                a_attrs.get("__args__"),
                b_attrs.get("__origin__"),
                b_attrs.get("__args__"),
            ) else {
                return Ok(None);
            };
            (
                a_origin.clone(),
                a_args.clone(),
                b_origin.clone(),
                b_args.clone(),
            )
        }
        _ => return Ok(None),
    };

    let origin_eq = a_origin.compare(&b_origin, CompareOp::Eq)?.is_truthy();
    let args_eq = a_args.compare(&b_args, CompareOp::Eq)?.is_truthy();
    let eq = origin_eq && args_eq;
    Ok(Some(PyObject::bool_val(if matches!(op, CompareOp::Eq) {
        eq
    } else {
        !eq
    })))
}

fn compare_mapping_proxy_objects(
    a: &PyObjectRef,
    b: &PyObjectRef,
    op: CompareOp,
) -> PyResult<PyObjectRef> {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Dict(a_map), PyObjectPayload::Dict(b_map)) => {
            let a_read = a_map.read();
            let b_read = b_map.read();
            compare_dict_objects(a, b, &a_read, &b_read, op)
        }
        (PyObjectPayload::Dict(a_map), PyObjectPayload::MappingProxy(b_map))
        | (PyObjectPayload::MappingProxy(a_map), PyObjectPayload::Dict(b_map))
        | (PyObjectPayload::MappingProxy(a_map), PyObjectPayload::MappingProxy(b_map)) => {
            let a_read = a_map.read();
            let b_read = b_map.read();
            compare_dict_objects(a, b, &a_read, &b_read, op)
        }
        _ => unreachable!(),
    }
}

fn compare_dict_item_values_equal(left: &PyObjectRef, right: &PyObjectRef) -> bool {
    match compare_dict_value_equal(left, right) {
        Ok(eq) => eq,
        Err(_) => false,
    }
}

pub(super) fn py_compare(a: &PyObjectRef, b: &PyObjectRef, op: CompareOp) -> PyResult<PyObjectRef> {
    // Unwrap builtin subclass instances for comparison
    let ua = unwrap_builtin_subclass(a);
    let ub = unwrap_builtin_subclass(b);
    if !PyObjectRef::ptr_eq(&ua, a) || !PyObjectRef::ptr_eq(&ub, b) {
        return py_compare(&ua, &ub, op);
    }
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Tuple(a_items), PyObjectPayload::Tuple(b_items)) => {
            let _comparison_guard = enter_container_comparison(a, b, op)?;
            return compare_sequence_items(a_items, b_items, op);
        }
        (PyObjectPayload::List(a_items), PyObjectPayload::List(b_items)) => {
            let _comparison_guard = enter_container_comparison(a, b, op)?;
            let a_snapshot = a_items.read().clone();
            let b_snapshot = b_items.read().clone();
            return compare_sequence_items(&a_snapshot, &b_snapshot, op);
        }
        (PyObjectPayload::Dict(_), PyObjectPayload::Dict(_))
        | (PyObjectPayload::Dict(_), PyObjectPayload::MappingProxy(_))
        | (PyObjectPayload::MappingProxy(_), PyObjectPayload::Dict(_))
        | (PyObjectPayload::MappingProxy(_), PyObjectPayload::MappingProxy(_)) => {
            return compare_mapping_proxy_objects(a, b, op);
        }
        (PyObjectPayload::Instance(_), PyObjectPayload::Dict(b_map))
            if instance_dict_storage(a).is_some() =>
        {
            let a_storage = instance_dict_storage(a).unwrap();
            let a_read = a_storage.read();
            let b_read = b_map.read();
            return compare_dict_objects(a, b, &a_read, &b_read, op);
        }
        (PyObjectPayload::Dict(a_map), PyObjectPayload::Instance(_))
            if instance_dict_storage(b).is_some() =>
        {
            let b_storage = instance_dict_storage(b).unwrap();
            let a_read = a_map.read();
            let b_read = b_storage.read();
            return compare_dict_objects(a, b, &a_read, &b_read, op);
        }
        (PyObjectPayload::Instance(_), PyObjectPayload::Instance(_))
            if instance_dict_storage(a).is_some() && instance_dict_storage(b).is_some() =>
        {
            let a_storage = instance_dict_storage(a).unwrap();
            let b_storage = instance_dict_storage(b).unwrap();
            let a_read = a_storage.read();
            let b_read = b_storage.read();
            return compare_dict_objects(a, b, &a_read, &b_read, op);
        }
        _ => {}
    }
    // Set comparisons: subset/superset semantics
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Set(_) | PyObjectPayload::FrozenSet(_), _)
        | (_, PyObjectPayload::Set(_) | PyObjectPayload::FrozenSet(_))
            if !matches!(op, CompareOp::Eq | CompareOp::Ne)
                && (!is_set_like(a) || !is_set_like(b)) =>
        {
            return Err(set_order_type_error(a, b, op));
        }
        (PyObjectPayload::DictKeys(a_map), PyObjectPayload::DictKeys(b_map)) => {
            let a_keys: Vec<_> = a_map
                .read()
                .keys()
                .filter(|k| !super::helpers::is_hidden_dict_key(k))
                .cloned()
                .collect();
            let b_keys: Vec<_> = b_map
                .read()
                .keys()
                .filter(|k| !super::helpers::is_hidden_dict_key(k))
                .cloned()
                .collect();
            let eq = a_keys.len() == b_keys.len() && a_keys.iter().all(|k| b_keys.contains(k));
            let result = match op {
                CompareOp::Eq => eq,
                CompareOp::Ne => !eq,
                CompareOp::Le => a_keys.iter().all(|k| b_keys.contains(k)),
                CompareOp::Lt => {
                    a_keys.len() < b_keys.len() && a_keys.iter().all(|k| b_keys.contains(k))
                }
                CompareOp::Ge => b_keys.iter().all(|k| a_keys.contains(k)),
                CompareOp::Gt => {
                    a_keys.len() > b_keys.len() && b_keys.iter().all(|k| a_keys.contains(k))
                }
            };
            return Ok(PyObject::bool_val(result));
        }
        (PyObjectPayload::DictItems(a_map), PyObjectPayload::DictItems(b_map)) => {
            let _comparison_guard = enter_container_comparison(a, b, op)?;
            let a_items: Vec<_> = a_map
                .read()
                .iter()
                .filter(|(k, _)| !super::helpers::is_hidden_dict_key(k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let b_items: Vec<_> = b_map
                .read()
                .iter()
                .filter(|(k, _)| !super::helpers::is_hidden_dict_key(k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let eq = a_items.len() == b_items.len()
                && a_items.iter().all(|(key, value)| {
                    b_items.iter().any(|(other_key, other_value)| {
                        key == other_key
                            && (PyObjectRef::ptr_eq(value, other_value)
                                || compare_dict_item_values_equal(value, other_value))
                    })
                });
            let result = match op {
                CompareOp::Eq => eq,
                CompareOp::Ne => !eq,
                _ => return Ok(PyObject::not_implemented()),
            };
            return Ok(PyObject::bool_val(result));
        }
        (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
            let ra = a.read();
            let rb = b.read();
            let result = match op {
                CompareOp::Eq => ra.len() == rb.len() && ra.keys().all(|k| rb.contains_key(k)),
                CompareOp::Ne => !(ra.len() == rb.len() && ra.keys().all(|k| rb.contains_key(k))),
                CompareOp::Le => ra.keys().all(|k| rb.contains_key(k)), // issubset
                CompareOp::Lt => ra.len() < rb.len() && ra.keys().all(|k| rb.contains_key(k)),
                CompareOp::Ge => rb.keys().all(|k| ra.contains_key(k)), // issuperset
                CompareOp::Gt => ra.len() > rb.len() && rb.keys().all(|k| ra.contains_key(k)),
            };
            return Ok(PyObject::bool_val(result));
        }
        // FrozenSet comparisons (with FrozenSet or Set)
        (PyObjectPayload::FrozenSet(a_map), PyObjectPayload::FrozenSet(b_map)) => {
            let result = match op {
                CompareOp::Eq => {
                    a_map.len() == b_map.len() && a_map.keys().all(|k| b_map.contains_key(k))
                }
                CompareOp::Ne => {
                    !(a_map.len() == b_map.len() && a_map.keys().all(|k| b_map.contains_key(k)))
                }
                CompareOp::Le => a_map.keys().all(|k| b_map.contains_key(k)),
                CompareOp::Lt => {
                    a_map.len() < b_map.len() && a_map.keys().all(|k| b_map.contains_key(k))
                }
                CompareOp::Ge => b_map.keys().all(|k| a_map.contains_key(k)),
                CompareOp::Gt => {
                    a_map.len() > b_map.len() && b_map.keys().all(|k| a_map.contains_key(k))
                }
            };
            return Ok(PyObject::bool_val(result));
        }
        (PyObjectPayload::FrozenSet(a_map), PyObjectPayload::Set(b_rw)) => {
            let rb = b_rw.read();
            let result = match op {
                CompareOp::Eq => {
                    a_map.len() == rb.len() && a_map.keys().all(|k| rb.contains_key(k))
                }
                CompareOp::Ne => {
                    !(a_map.len() == rb.len() && a_map.keys().all(|k| rb.contains_key(k)))
                }
                CompareOp::Le => a_map.keys().all(|k| rb.contains_key(k)),
                CompareOp::Lt => a_map.len() < rb.len() && a_map.keys().all(|k| rb.contains_key(k)),
                CompareOp::Ge => rb.keys().all(|k| a_map.contains_key(k)),
                CompareOp::Gt => a_map.len() > rb.len() && rb.keys().all(|k| a_map.contains_key(k)),
            };
            return Ok(PyObject::bool_val(result));
        }
        (PyObjectPayload::Set(a_rw), PyObjectPayload::FrozenSet(b_map)) => {
            let ra = a_rw.read();
            let result = match op {
                CompareOp::Eq => {
                    ra.len() == b_map.len() && ra.keys().all(|k| b_map.contains_key(k))
                }
                CompareOp::Ne => {
                    !(ra.len() == b_map.len() && ra.keys().all(|k| b_map.contains_key(k)))
                }
                CompareOp::Le => ra.keys().all(|k| b_map.contains_key(k)),
                CompareOp::Lt => ra.len() < b_map.len() && ra.keys().all(|k| b_map.contains_key(k)),
                CompareOp::Ge => b_map.keys().all(|k| ra.contains_key(k)),
                CompareOp::Gt => ra.len() > b_map.len() && b_map.keys().all(|k| ra.contains_key(k)),
            };
            return Ok(PyObject::bool_val(result));
        }
        _ => {}
    }
    let call_instance_dunder =
        |obj: &PyObjectRef, other: &PyObjectRef, name: &str| -> PyResult<Option<PyObjectRef>> {
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                if let Some(method) = lookup_in_class_mro(&inst.class, name) {
                    let bound = wrap_class_attr_for_instance(obj, inst, name, method);
                    let result = call_callable(&bound, &[other.clone()])?;
                    if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                        return Ok(Some(result));
                    }
                }
            }
            Ok(None)
        };
    let has_instance_dunder = |obj: &PyObjectRef, name: &str| -> bool {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            lookup_in_class_mro(&inst.class, name).is_some()
        } else {
            false
        }
    };
    let call_instance_ne =
        |obj: &PyObjectRef, other: &PyObjectRef| -> PyResult<Option<PyObjectRef>> {
            if has_instance_dunder(obj, "__ne__") {
                return call_instance_dunder(obj, other, "__ne__");
            }
            if let Some(result) = call_instance_dunder(obj, other, "__eq__")? {
                return Ok(Some(PyObject::bool_val(!result.is_truthy())));
            }
            Ok(None)
        };

    if matches!(op, CompareOp::Eq | CompareOp::Ne) {
        if let Some(result) = compare_generic_alias_objects(a, b, op)? {
            return Ok(result);
        }
        if matches!(op, CompareOp::Ne) {
            let right_is_subclass = match (instance_class(a), instance_class(b)) {
                (Some(left), Some(right)) => class_is_strict_subclass(&right, &left),
                _ => false,
            };
            if right_is_subclass {
                if let Some(result) = call_instance_ne(b, a)? {
                    return Ok(result);
                }
            }
            if let Some(result) = call_instance_ne(a, b)? {
                return Ok(result);
            }
            if !right_is_subclass {
                if let Some(result) = call_instance_ne(b, a)? {
                    return Ok(result);
                }
            }
        } else {
            let right_is_subclass = match (instance_class(a), instance_class(b)) {
                (Some(left), Some(right)) => class_is_strict_subclass(&right, &left),
                _ => false,
            };
            if right_is_subclass {
                if let Some(result) = call_instance_dunder(b, a, "__eq__")? {
                    return Ok(result);
                }
            }
            if let Some(result) = call_instance_dunder(a, b, "__eq__")? {
                return Ok(result);
            }
            if !right_is_subclass {
                if let Some(result) = call_instance_dunder(b, a, "__eq__")? {
                    return Ok(result);
                }
            }
        }
    }

    // Check for ordering dunder methods on instances (__lt__, __le__, __gt__, __ge__)
    if matches!(
        op,
        CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge
    ) {
        let (dunder, rdunder) = match op {
            CompareOp::Lt => ("__lt__", "__gt__"),
            CompareOp::Le => ("__le__", "__ge__"),
            CompareOp::Gt => ("__gt__", "__lt__"),
            CompareOp::Ge => ("__ge__", "__le__"),
            CompareOp::Eq | CompareOp::Ne => unreachable!(),
        };
        let right_is_subclass = match (instance_class(a), instance_class(b)) {
            (Some(left), Some(right)) => class_is_strict_subclass(&right, &left),
            _ => false,
        };
        if right_is_subclass {
            if let Some(result) = call_instance_dunder(b, a, rdunder)? {
                return Ok(result);
            }
        }
        if let Some(result) = call_instance_dunder(a, b, dunder)? {
            return Ok(result);
        }
        if !right_is_subclass {
            if let Some(result) = call_instance_dunder(b, a, rdunder)? {
                return Ok(result);
            }
        }
        if matches!(&a.payload, PyObjectPayload::Instance(_))
            || matches!(&b.payload, PyObjectPayload::Instance(_))
        {
            return Err(set_order_type_error(a, b, op));
        }
    }
    // BoundMethod equality: equal if __func__ and __self__ are the same
    if matches!(op, CompareOp::Eq | CompareOp::Ne) {
        if let (
            PyObjectPayload::BoundMethod {
                method: m1,
                receiver: r1,
            },
            PyObjectPayload::BoundMethod {
                method: m2,
                receiver: r2,
            },
        ) = (&a.payload, &b.payload)
        {
            let eq = PyObjectRef::ptr_eq(r1, r2) && PyObjectRef::ptr_eq(m1, m2);
            let result = if matches!(op, CompareOp::Eq) { eq } else { !eq };
            return Ok(PyObject::bool_val(result));
        }
    }
    // NaN is never equal to anything, including itself
    let has_nan = match (&a.payload, &b.payload) {
        (PyObjectPayload::Float(f), _) if f.is_nan() => true,
        (_, PyObjectPayload::Float(f)) if f.is_nan() => true,
        _ => false,
    };
    // Ordering comparisons on complex numbers must raise TypeError (CPython behavior)
    if matches!(
        op,
        CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge
    ) {
        if matches!(
            (&a.payload, &b.payload),
            (PyObjectPayload::Complex { .. }, _) | (_, PyObjectPayload::Complex { .. })
        ) {
            return Err(crate::error::PyException::type_error(
                "'<' not supported between instances of 'complex' and 'complex'".to_string(),
            ));
        }
    }
    let ord = partial_cmp_objects(a, b);
    let result = match op {
        CompareOp::Eq => {
            if has_nan {
                false
            } else {
                match ord {
                    Some(o) => o == std::cmp::Ordering::Equal,
                    None => std::ptr::eq(a.as_ref(), b.as_ref()),
                }
            }
        }
        CompareOp::Ne => {
            if has_nan {
                true
            } else {
                match ord {
                    Some(o) => o != std::cmp::Ordering::Equal,
                    None => !std::ptr::eq(a.as_ref(), b.as_ref()),
                }
            }
        }
        CompareOp::Lt => ord == Some(std::cmp::Ordering::Less),
        CompareOp::Le => matches!(
            ord,
            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        ),
        CompareOp::Gt => ord == Some(std::cmp::Ordering::Greater),
        CompareOp::Ge => matches!(
            ord,
            Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
        ),
    };
    Ok(PyObject::bool_val(result))
}
