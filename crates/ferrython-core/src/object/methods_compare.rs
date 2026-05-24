//! Comparison methods.

use crate::error::{PyException, PyResult};

use super::helpers::{call_callable, partial_cmp_objects, unwrap_builtin_subclass};
use super::methods::{CompareOp, PyObjectMethods};
use super::methods_attr::{lookup_in_class_mro, wrap_class_attr_for_instance};
use super::payload::*;

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

        if let Some(ordering) = partial_cmp_objects(left, right) {
            if ordering == std::cmp::Ordering::Equal {
                continue;
            }
            return Ok(PyObject::bool_val(compare_ordering(ordering, op)));
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

pub(super) fn py_compare(a: &PyObjectRef, b: &PyObjectRef, op: CompareOp) -> PyResult<PyObjectRef> {
    // Unwrap builtin subclass instances for comparison
    let ua = unwrap_builtin_subclass(a);
    let ub = unwrap_builtin_subclass(b);
    if !PyObjectRef::ptr_eq(&ua, a) || !PyObjectRef::ptr_eq(&ub, b) {
        return py_compare(&ua, &ub, op);
    }
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Tuple(a_items), PyObjectPayload::Tuple(b_items)) => {
            return compare_sequence_items(a_items, b_items, op);
        }
        (PyObjectPayload::List(a_items), PyObjectPayload::List(b_items)) => {
            let a_snapshot = a_items.read().clone();
            let b_snapshot = b_items.read().clone();
            return compare_sequence_items(&a_snapshot, &b_snapshot, op);
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
                                || value
                                    .compare(other_value, CompareOp::Eq)
                                    .map(|r| r.is_truthy())
                                    .unwrap_or(false))
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

    if matches!(op, CompareOp::Eq | CompareOp::Ne) {
        if matches!(op, CompareOp::Ne) {
            let right_is_subclass = match (instance_class(a), instance_class(b)) {
                (Some(left), Some(right)) => class_is_strict_subclass(&right, &left),
                _ => false,
            };
            if right_is_subclass {
                if let Some(result) = call_instance_dunder(b, a, "__ne__")? {
                    return Ok(result);
                }
            }
            if let Some(result) = call_instance_dunder(a, b, "__ne__")? {
                return Ok(result);
            }
            if right_is_subclass {
                if let Some(result) = call_instance_dunder(b, a, "__eq__")? {
                    return Ok(PyObject::bool_val(!result.is_truthy()));
                }
            }
            if let Some(result) = call_instance_dunder(a, b, "__eq__")? {
                return Ok(PyObject::bool_val(!result.is_truthy()));
            }
            if !right_is_subclass {
                if let Some(result) = call_instance_dunder(b, a, "__eq__")? {
                    return Ok(PyObject::bool_val(!result.is_truthy()));
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
