//! Comparison methods.

use crate::error::PyResult;

use super::payload::*;
use super::helpers::{partial_cmp_objects, unwrap_builtin_subclass};
use super::methods::CompareOp;
use std::sync::Arc;

pub(super) fn py_compare(a: &PyObjectRef, b: &PyObjectRef, op: CompareOp) -> PyResult<PyObjectRef> {
        // Unwrap builtin subclass instances for comparison
        let ua = unwrap_builtin_subclass(a);
        let ub = unwrap_builtin_subclass(b);
        if !Arc::ptr_eq(&ua, a) || !Arc::ptr_eq(&ub, b) {
            return py_compare(&ua, &ub, op);
        }
        // Set comparisons: subset/superset semantics
        match (&a.payload, &b.payload) {
            (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
                let ra = a.read(); let rb = b.read();
                let result = match op {
                    CompareOp::Eq => ra.len() == rb.len() && ra.keys().all(|k| rb.contains_key(k)),
                    CompareOp::Ne => !(ra.len() == rb.len() && ra.keys().all(|k| rb.contains_key(k))),
                    CompareOp::Le => ra.keys().all(|k| rb.contains_key(k)),  // issubset
                    CompareOp::Lt => ra.len() < rb.len() && ra.keys().all(|k| rb.contains_key(k)),
                    CompareOp::Ge => rb.keys().all(|k| ra.contains_key(k)),  // issuperset
                    CompareOp::Gt => ra.len() > rb.len() && rb.keys().all(|k| ra.contains_key(k)),
                };
                return Ok(PyObject::bool_val(result));
            }
            // FrozenSet comparisons (with FrozenSet or Set)
            (PyObjectPayload::FrozenSet(a_map), PyObjectPayload::FrozenSet(b_map)) => {
                let result = match op {
                    CompareOp::Eq => a_map.len() == b_map.len() && a_map.keys().all(|k| b_map.contains_key(k)),
                    CompareOp::Ne => !(a_map.len() == b_map.len() && a_map.keys().all(|k| b_map.contains_key(k))),
                    CompareOp::Le => a_map.keys().all(|k| b_map.contains_key(k)),
                    CompareOp::Lt => a_map.len() < b_map.len() && a_map.keys().all(|k| b_map.contains_key(k)),
                    CompareOp::Ge => b_map.keys().all(|k| a_map.contains_key(k)),
                    CompareOp::Gt => a_map.len() > b_map.len() && b_map.keys().all(|k| a_map.contains_key(k)),
                };
                return Ok(PyObject::bool_val(result));
            }
            (PyObjectPayload::FrozenSet(a_map), PyObjectPayload::Set(b_rw)) => {
                let rb = b_rw.read();
                let result = match op {
                    CompareOp::Eq => a_map.len() == rb.len() && a_map.keys().all(|k| rb.contains_key(k)),
                    CompareOp::Ne => !(a_map.len() == rb.len() && a_map.keys().all(|k| rb.contains_key(k))),
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
                    CompareOp::Eq => ra.len() == b_map.len() && ra.keys().all(|k| b_map.contains_key(k)),
                    CompareOp::Ne => !(ra.len() == b_map.len() && ra.keys().all(|k| b_map.contains_key(k))),
                    CompareOp::Le => ra.keys().all(|k| b_map.contains_key(k)),
                    CompareOp::Lt => ra.len() < b_map.len() && ra.keys().all(|k| b_map.contains_key(k)),
                    CompareOp::Ge => b_map.keys().all(|k| ra.contains_key(k)),
                    CompareOp::Gt => ra.len() > b_map.len() && b_map.keys().all(|k| ra.contains_key(k)),
                };
                return Ok(PyObject::bool_val(result));
            }
            _ => {}
        }
        // Check for dunder comparison methods on instances (__eq__, __lt__, __le__, __gt__, __ge__, __ne__)
        {
            let dunder = match op {
                CompareOp::Eq => "__eq__",
                CompareOp::Ne => "__ne__",
                CompareOp::Lt => "__lt__",
                CompareOp::Le => "__le__",
                CompareOp::Gt => "__gt__",
                CompareOp::Ge => "__ge__",
            };
            if let PyObjectPayload::Instance(inst) = &a.payload {
                if let Some(method) = inst.attrs.read().get(dunder).cloned() {
                    match &method.payload {
                        PyObjectPayload::NativeClosure(nc) => {
                            return (nc.func)(&[a.clone(), b.clone()]);
                        }
                        PyObjectPayload::NativeFunction { func, .. } => {
                            return func(&[a.clone(), b.clone()]);
                        }
                        _ => {}
                    }
                }
            }
        }
        // BoundMethod equality: equal if __func__ and __self__ are the same
        if matches!(op, CompareOp::Eq | CompareOp::Ne) {
            if let (PyObjectPayload::BoundMethod { method: m1, receiver: r1 },
                    PyObjectPayload::BoundMethod { method: m2, receiver: r2 }) = (&a.payload, &b.payload)
            {
                let eq = Arc::ptr_eq(r1, r2) && Arc::ptr_eq(m1, m2);
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
        if matches!(op, CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge) {
            if matches!((&a.payload, &b.payload),
                (PyObjectPayload::Complex { .. }, _) | (_, PyObjectPayload::Complex { .. })) {
                return Err(crate::error::PyException::type_error(
                    "'<' not supported between instances of 'complex' and 'complex'".to_string()
                ));
            }
        }
        let ord = partial_cmp_objects(a, b);
        let result = match op {
            CompareOp::Eq => if has_nan { false } else {
                match ord {
                    Some(o) => o == std::cmp::Ordering::Equal,
                    None => std::ptr::eq(a.as_ref(), b.as_ref()),
                }
            },
            CompareOp::Ne => if has_nan { true } else {
                match ord {
                    Some(o) => o != std::cmp::Ordering::Equal,
                    None => !std::ptr::eq(a.as_ref(), b.as_ref()),
                }
            },
            CompareOp::Lt => ord == Some(std::cmp::Ordering::Less),
            CompareOp::Le => matches!(ord, Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)),
            CompareOp::Gt => ord == Some(std::cmp::Ordering::Greater),
            CompareOp::Ge => matches!(ord, Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)),
        };
        Ok(PyObject::bool_val(result))
}
