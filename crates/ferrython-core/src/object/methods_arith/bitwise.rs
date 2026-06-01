use crate::error::{PyException, PyResult};
use crate::intern::intern_or_new;
use crate::types::{HashableKey, PyInt};
use compact_str::CompactString;
use indexmap::IndexMap;
use std::rc::Rc;

use super::super::helpers::*;
use super::super::methods::PyObjectMethods;
use super::super::payload::*;
use super::{extract_view_keys, keys_to_set};

fn int_shift_operands(a: &PyObjectRef, b: &PyObjectRef, op_name: &str) -> PyResult<(PyInt, i64)> {
    let ua = unwrap_builtin_subclass(a);
    let ub = unwrap_builtin_subclass(b);
    let lhs = match &ua.payload {
        PyObjectPayload::Int(n) => n.clone(),
        PyObjectPayload::Bool(flag) => PyInt::Small(*flag as i64),
        _ => {
            return Err(PyException::type_error(format!(
                "unsupported operand type(s) for {}: '{}' and '{}'",
                op_name,
                a.type_name(),
                b.type_name()
            )))
        }
    };
    let rhs = match &ub.payload {
        PyObjectPayload::Int(n) => n
            .to_i64()
            .ok_or_else(|| PyException::overflow_error("shift count too large"))?,
        PyObjectPayload::Bool(flag) => *flag as i64,
        _ => {
            return Err(PyException::type_error(format!(
                "unsupported operand type(s) for {}: '{}' and '{}'",
                op_name,
                a.type_name(),
                b.type_name()
            )))
        }
    };
    if rhs < 0 {
        return Err(PyException::value_error("negative shift count"));
    }
    Ok((lhs, rhs))
}

pub(crate) fn py_lshift(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    let (lhs, shift) = int_shift_operands(a, b, "<<")?;
    let shift = shift as usize;
    if !lhs.is_zero() {
        guard_eager_allocation(shift / 8 + 1, "int left shift")?;
    }
    Ok(PyInt::lshift_op(&lhs, shift).to_object())
}

pub(crate) fn py_rshift(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    let (lhs, shift) = int_shift_operands(a, b, ">>")?;
    Ok(PyInt::rshift_op(&lhs, shift as usize).to_object())
}

pub(crate) fn py_bit_and(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
            let ra = a.read();
            let rb = b.read();
            let mut result = new_fx_hashkey_flatmap();
            for (k, v) in ra.iter() {
                if rb.contains_key(k) {
                    result.insert(k.clone(), v.clone());
                }
            }
            Ok(PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(
                result,
            )))))
        }
        (PyObjectPayload::FrozenSet(a), PyObjectPayload::FrozenSet(b)) => {
            let mut result = new_fx_hashkey_map();
            for (k, v) in a.iter() {
                if b.contains_key(k) {
                    result.insert(k.clone(), v.clone());
                }
            }
            Ok(PyObject::wrap(PyObjectPayload::FrozenSet(Box::new(
                FrozenSetData::new(result),
            ))))
        }
        (PyObjectPayload::FrozenSet(a), PyObjectPayload::Set(b)) => {
            let rb = b.read();
            let mut result = new_fx_hashkey_map();
            for (k, v) in a.iter() {
                if rb.contains_key(k) {
                    result.insert(k.clone(), v.clone());
                }
            }
            Ok(PyObject::wrap(PyObjectPayload::FrozenSet(Box::new(
                FrozenSetData::new(result),
            ))))
        }
        (PyObjectPayload::Set(a), PyObjectPayload::FrozenSet(b)) => {
            let ra = a.read();
            let mut result = new_fx_hashkey_flatmap();
            for (k, v) in ra.iter() {
                if b.contains_key(k) {
                    result.insert(k.clone(), v.clone());
                }
            }
            Ok(PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(
                result,
            )))))
        }
        // Counter & Counter: minimum of counts (intersection)
        (PyObjectPayload::Dict(a_map), PyObjectPayload::Dict(b_map)) => {
            let ra = a_map.read();
            let rb = b_map.read();
            let counter_key = HashableKey::str_key(intern_or_new("__counter__"));
            let a_counter = ra.contains_key(&counter_key);
            let b_counter = rb.contains_key(&counter_key);
            if a_counter && b_counter {
                let mut result = new_fx_hashkey_map();
                result.insert(
                    HashableKey::str_key(intern_or_new("__defaultdict_factory__")),
                    PyObject::builtin_type(CompactString::from("int")),
                );
                result.insert(counter_key, PyObject::bool_val(true));
                for (k, v) in ra.iter() {
                    if let HashableKey::Str(s) = k {
                        if s.starts_with("__") && s.ends_with("__") {
                            continue;
                        }
                    }
                    if let Some(bv) = rb.get(k) {
                        let a_count = v.as_int().unwrap_or(0);
                        let b_count = bv.as_int().unwrap_or(0);
                        let min_count = a_count.min(b_count);
                        if min_count > 0 {
                            result.insert(k.clone(), PyObject::int(min_count));
                        }
                    }
                }
                Ok(PyObject::dict(result))
            } else {
                Err(PyException::type_error(format!(
                    "unsupported operand type(s) for &: '{}' and '{}'",
                    a.type_name(),
                    b.type_name()
                )))
            }
        }
        // DictKeys/DictItems set-like intersection
        (PyObjectPayload::DictKeys { .. } | PyObjectPayload::DictItems { .. }, _)
        | (_, PyObjectPayload::DictKeys { .. } | PyObjectPayload::DictItems { .. }) => {
            let ak = extract_view_keys(a)?;
            let bk = extract_view_keys(b)?;
            let mut result = new_fx_hashkey_flatmap();
            for (k, v) in ak.iter() {
                if bk.contains_key(k) {
                    result.insert(k.clone(), v.clone());
                }
            }
            Ok(keys_to_set(result))
        }
        _ => int_bitop(a, b, "&", |a, b| a & b),
    }
}

pub(crate) fn py_bit_or(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
            let ra = a.read();
            let rb = b.read();
            let mut result = ra.clone();
            for (k, v) in rb.iter() {
                result.insert(k.clone(), v.clone());
            }
            Ok(PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(
                result,
            )))))
        }
        (PyObjectPayload::FrozenSet(a), PyObjectPayload::FrozenSet(b)) => {
            let mut result = a.items.clone();
            for (k, v) in b.iter() {
                result.insert(k.clone(), v.clone());
            }
            Ok(PyObject::wrap(PyObjectPayload::FrozenSet(Box::new(
                FrozenSetData::new(result),
            ))))
        }
        (PyObjectPayload::FrozenSet(a), PyObjectPayload::Set(b)) => {
            let mut result = a.items.clone();
            let rb = b.read();
            for (k, v) in rb.iter() {
                result.insert(k.clone(), v.clone());
            }
            Ok(PyObject::wrap(PyObjectPayload::FrozenSet(Box::new(
                FrozenSetData::new(result),
            ))))
        }
        (PyObjectPayload::Set(a), PyObjectPayload::FrozenSet(b)) => {
            let ra = a.read();
            let mut result = ra.clone();
            for (k, v) in b.iter() {
                result.insert(k.clone(), v.clone());
            }
            Ok(PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(
                result,
            )))))
        }
        // PEP 584: dict | dict (also Counter | Counter with max semantics)
        (PyObjectPayload::Dict(a_map), PyObjectPayload::Dict(b_map)) => {
            let ra = a_map.read();
            let rb = b_map.read();
            let counter_key = HashableKey::str_key(intern_or_new("__counter__"));
            let a_counter = ra.contains_key(&counter_key);
            let b_counter = rb.contains_key(&counter_key);
            if a_counter && b_counter {
                // Counter | Counter: maximum of counts (union)
                let mut result = new_fx_hashkey_map();
                result.insert(
                    HashableKey::str_key(intern_or_new("__defaultdict_factory__")),
                    PyObject::builtin_type(CompactString::from("int")),
                );
                result.insert(counter_key, PyObject::bool_val(true));
                let mut all_keys: IndexMap<HashableKey, i64> = IndexMap::new();
                for (k, v) in ra.iter() {
                    if let HashableKey::Str(s) = k {
                        if s.starts_with("__") && s.ends_with("__") {
                            continue;
                        }
                    }
                    all_keys.insert(k.clone(), v.as_int().unwrap_or(0));
                }
                for (k, v) in rb.iter() {
                    if let HashableKey::Str(s) = k {
                        if s.starts_with("__") && s.ends_with("__") {
                            continue;
                        }
                    }
                    let b_count = v.as_int().unwrap_or(0);
                    let entry = all_keys.entry(k.clone()).or_insert(0);
                    *entry = (*entry).max(b_count);
                }
                for (k, count) in all_keys {
                    if count > 0 {
                        result.insert(k, PyObject::int(count));
                    }
                }
                Ok(PyObject::dict(result))
            } else {
                // Regular PEP 584: dict | dict
                let mut result = ra.clone();
                for (k, v) in rb.iter() {
                    result.insert(k.clone(), v.clone());
                }
                Ok(PyObject::dict(result))
            }
        }
        // DictKeys/DictItems set-like union
        (PyObjectPayload::DictKeys { .. } | PyObjectPayload::DictItems { .. }, _)
        | (_, PyObjectPayload::DictKeys { .. } | PyObjectPayload::DictItems { .. }) => {
            let ak = extract_view_keys(a)?;
            let bk = extract_view_keys(b)?;
            let mut result = ak;
            for (k, v) in bk.iter() {
                result.entry(k.clone()).or_insert_with(|| v.clone());
            }
            Ok(keys_to_set(result))
        }
        _ => int_bitop(a, b, "|", |a, b| a | b),
    }
}

pub(crate) fn py_bit_xor(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
            let ra = a.read();
            let rb = b.read();
            let mut result = new_fx_hashkey_flatmap();
            for (k, v) in ra.iter() {
                if !rb.contains_key(k) {
                    result.insert(k.clone(), v.clone());
                }
            }
            for (k, v) in rb.iter() {
                if !ra.contains_key(k) {
                    result.insert(k.clone(), v.clone());
                }
            }
            Ok(PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(
                result,
            )))))
        }
        (PyObjectPayload::FrozenSet(a), PyObjectPayload::FrozenSet(b)) => {
            let mut result = new_fx_hashkey_map();
            for (k, v) in a.iter() {
                if !b.contains_key(k) {
                    result.insert(k.clone(), v.clone());
                }
            }
            for (k, v) in b.iter() {
                if !a.contains_key(k) {
                    result.insert(k.clone(), v.clone());
                }
            }
            Ok(PyObject::wrap(PyObjectPayload::FrozenSet(Box::new(
                FrozenSetData::new(result),
            ))))
        }
        (PyObjectPayload::FrozenSet(a), PyObjectPayload::Set(b)) => {
            let rb = b.read();
            let mut result = new_fx_hashkey_map();
            for (k, v) in a.iter() {
                if !rb.contains_key(k) {
                    result.insert(k.clone(), v.clone());
                }
            }
            for (k, v) in rb.iter() {
                if !a.contains_key(k) {
                    result.insert(k.clone(), v.clone());
                }
            }
            Ok(PyObject::wrap(PyObjectPayload::FrozenSet(Box::new(
                FrozenSetData::new(result),
            ))))
        }
        (PyObjectPayload::Set(a), PyObjectPayload::FrozenSet(b)) => {
            let ra = a.read();
            let mut result = new_fx_hashkey_flatmap();
            for (k, v) in ra.iter() {
                if !b.contains_key(k) {
                    result.insert(k.clone(), v.clone());
                }
            }
            for (k, v) in b.iter() {
                if !ra.contains_key(k) {
                    result.insert(k.clone(), v.clone());
                }
            }
            Ok(PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(
                result,
            )))))
        }
        // DictKeys/DictItems set-like symmetric difference
        (PyObjectPayload::DictKeys { .. } | PyObjectPayload::DictItems { .. }, _)
        | (_, PyObjectPayload::DictKeys { .. } | PyObjectPayload::DictItems { .. }) => {
            let ak = extract_view_keys(a)?;
            let bk = extract_view_keys(b)?;
            let mut result = new_fx_hashkey_flatmap();
            for (k, v) in ak.iter() {
                if !bk.contains_key(k) {
                    result.insert(k.clone(), v.clone());
                }
            }
            for (k, v) in bk.iter() {
                if !ak.contains_key(k) {
                    result.insert(k.clone(), v.clone());
                }
            }
            Ok(keys_to_set(result))
        }
        _ => int_bitop(a, b, "^", |a, b| a ^ b),
    }
}
