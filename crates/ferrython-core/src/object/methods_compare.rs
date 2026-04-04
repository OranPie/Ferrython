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
            _ => {}
        }
        // NaN is never equal to anything, including itself
        let has_nan = match (&a.payload, &b.payload) {
            (PyObjectPayload::Float(f), _) if f.is_nan() => true,
            (_, PyObjectPayload::Float(f)) if f.is_nan() => true,
            _ => false,
        };
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
