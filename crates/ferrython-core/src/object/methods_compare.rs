//! Comparison methods.

use crate::error::PyResult;

use super::payload::*;
use super::helpers::partial_cmp_objects;
use super::methods::CompareOp;

pub(super) fn py_compare(a: &PyObjectRef, b: &PyObjectRef, op: CompareOp) -> PyResult<PyObjectRef> {
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
        let ord = partial_cmp_objects(a, b);
        let result = match op {
            // For Eq/Ne, if types don't define comparison (ord is None),
            // fall back to identity comparison (like CPython's default __eq__)
            CompareOp::Eq => match ord {
                Some(o) => o == std::cmp::Ordering::Equal,
                None => std::ptr::eq(a.as_ref(), b.as_ref()),
            },
            CompareOp::Ne => match ord {
                Some(o) => o != std::cmp::Ordering::Equal,
                None => !std::ptr::eq(a.as_ref(), b.as_ref()),
            },
            CompareOp::Lt => ord == Some(std::cmp::Ordering::Less),
            CompareOp::Le => matches!(ord, Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)),
            CompareOp::Gt => ord == Some(std::cmp::Ordering::Greater),
            CompareOp::Ge => matches!(ord, Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)),
        };
        Ok(PyObject::bool_val(result))
}
