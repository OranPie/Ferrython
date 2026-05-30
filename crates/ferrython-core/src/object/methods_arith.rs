//! Arithmetic operation methods.

use crate::error::{PyException, PyResult};
use crate::intern::intern_or_new;
use crate::types::{HashableKey, PyInt};
use compact_str::CompactString;
use indexmap::IndexMap;
use std::rc::Rc;

use super::helpers::*;
use super::methods::PyObjectMethods;
use super::payload::*;

mod bitwise;
mod modulo;

pub(super) use bitwise::{py_bit_and, py_bit_or, py_bit_xor, py_lshift, py_rshift};
pub(super) use modulo::py_modulo;

/// Extract keys as HashableKey set from DictKeys or DictItems view.
fn extract_view_keys(obj: &PyObjectRef) -> Option<FxHashKeyFlatMap> {
    match &obj.payload {
        PyObjectPayload::DictKeys { map: m, .. } => {
            let r = m.read();
            Some(r.keys().map(|k| (k.clone(), k.to_object())).collect())
        }
        PyObjectPayload::DictItems { map: m, .. } => {
            let r = m.read();
            Some(
                r.iter()
                    .map(|(k, v)| {
                        let tuple_obj = PyObject::tuple(vec![k.to_object(), v.clone()]);
                        let tuple_key = HashableKey::Tuple(Box::new(vec![
                            k.clone(),
                            HashableKey::from_object(v).unwrap_or(HashableKey::None),
                        ]));
                        (tuple_key, tuple_obj)
                    })
                    .collect(),
            )
        }
        PyObjectPayload::Set(s) => Some(s.read().clone()),
        PyObjectPayload::FrozenSet(s) => {
            Some(s.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        }
        _ => None,
    }
}

/// Build a set result from a FxHashKeyFlatMap.
fn keys_to_set(keys: FxHashKeyFlatMap) -> PyObjectRef {
    PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(keys))))
}

/// Coerce a Bool operand to Int for arithmetic that doesn't preserve bool type
/// (e.g., /, //, %, **, <<, >>). Returns Some(int_obj) if it was a Bool.
fn bool_as_int(o: &PyObjectRef) -> Option<PyObjectRef> {
    if let PyObjectPayload::Bool(b) = &o.payload {
        Some(PyObject::int(if *b { 1 } else { 0 }))
    } else {
        None
    }
}

pub(super) fn py_add(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    // Unwrap builtin subclass instances to their underlying values
    let ua = unwrap_builtin_subclass(a);
    let ub = unwrap_builtin_subclass(b);
    if !PyObjectRef::ptr_eq(&ua, a) || !PyObjectRef::ptr_eq(&ub, b) {
        return py_add(&ua, &ub);
    }
    match (&a.payload, &b.payload) {
        // Bool → Int coercion for arithmetic
        (PyObjectPayload::Bool(a), PyObjectPayload::Bool(b)) => {
            Ok(PyObject::int(*a as i64 + *b as i64))
        }
        (PyObjectPayload::Bool(a), PyObjectPayload::Int(b)) => {
            Ok(PyInt::add_op(&PyInt::Small(*a as i64), b).to_object())
        }
        (PyObjectPayload::Int(a), PyObjectPayload::Bool(b)) => {
            Ok(PyInt::add_op(a, &PyInt::Small(*b as i64)).to_object())
        }
        (PyObjectPayload::Bool(a), PyObjectPayload::Float(b)) => {
            Ok(PyObject::float(*a as i64 as f64 + b))
        }
        (PyObjectPayload::Float(a), PyObjectPayload::Bool(b)) => {
            Ok(PyObject::float(a + *b as i64 as f64))
        }
        (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => Ok(PyInt::add_op(a, b).to_object()),
        (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a + b)),
        (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.to_f64() + b)),
        (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => Ok(PyObject::float(a + b.to_f64())),
        (
            PyObjectPayload::Complex { real: ar, imag: ai },
            PyObjectPayload::Complex { real: br, imag: bi },
        ) => Ok(PyObject::complex(ar + br, ai + bi)),
        (PyObjectPayload::Int(a), PyObjectPayload::Complex { real, imag }) => {
            Ok(PyObject::complex(a.to_f64() + real, *imag))
        }
        (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Int(b)) => {
            Ok(PyObject::complex(real + b.to_f64(), *imag))
        }
        (PyObjectPayload::Float(a), PyObjectPayload::Complex { real, imag }) => {
            Ok(PyObject::complex(a + real, *imag))
        }
        (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Float(b)) => {
            Ok(PyObject::complex(real + b, *imag))
        }
        (PyObjectPayload::Str(a), PyObjectPayload::Str(b)) => {
            let mut s = a.to_string();
            s.push_str(b.as_str());
            Ok(PyObject::str_val(CompactString::from(s)))
        }
        (PyObjectPayload::List(a), PyObjectPayload::List(b)) => {
            let mut r = a.read().clone();
            r.extend(b.read().iter().cloned());
            Ok(PyObject::list(r))
        }
        (PyObjectPayload::Tuple(a), PyObjectPayload::Tuple(b)) => {
            let mut r = (**a).clone();
            r.extend(b.iter().cloned());
            Ok(PyObject::tuple(r))
        }
        (PyObjectPayload::Bytes(a), PyObjectPayload::Bytes(b))
        | (PyObjectPayload::ByteArray(a), PyObjectPayload::Bytes(b))
        | (PyObjectPayload::Bytes(a), PyObjectPayload::ByteArray(b)) => {
            let mut r = (**a).clone();
            r.extend(b.iter());
            Ok(PyObject::bytes(r))
        }
        // Dict addition (Counter + Counter)
        (PyObjectPayload::Dict(a_map), PyObjectPayload::Dict(b_map)) => {
            let ra = a_map.read();
            let rb = b_map.read();
            let mut result = new_fx_hashkey_map();
            // Copy non-marker keys from a
            for (k, v) in ra.iter() {
                if let HashableKey::Str(s) = k {
                    if s.starts_with("__") && s.ends_with("__") {
                        continue;
                    }
                }
                result.insert(k.clone(), v.clone());
            }
            // Merge non-marker keys from b
            for (k, v) in rb.iter() {
                if let HashableKey::Str(s) = k {
                    if s.starts_with("__") && s.ends_with("__") {
                        continue;
                    }
                }
                let existing = result.get(k).and_then(|e| e.as_int()).unwrap_or(0);
                let new_val = existing + v.as_int().unwrap_or(0);
                result.insert(k.clone(), PyObject::int(new_val));
            }
            // Preserve __counter__ and __defaultdict_factory__ markers if both inputs are counters
            let a_is_counter = ra.contains_key(&HashableKey::str_key(intern_or_new("__counter__")));
            let b_is_counter = rb.contains_key(&HashableKey::str_key(intern_or_new("__counter__")));
            if a_is_counter && b_is_counter {
                result.insert(
                    HashableKey::str_key(intern_or_new("__counter__")),
                    PyObject::bool_val(true),
                );
                if let Some(factory) = ra.get(&HashableKey::str_key(intern_or_new(
                    "__defaultdict_factory__",
                ))) {
                    result.insert(
                        HashableKey::str_key(intern_or_new("__defaultdict_factory__")),
                        factory.clone(),
                    );
                }
            }
            Ok(PyObject::wrap(PyObjectPayload::Dict(Rc::new(PyCell::new(
                result,
            )))))
        }
        // IntEnum: Instance + Instance → extract .value and add
        (PyObjectPayload::Instance(a_inst), PyObjectPayload::Instance(b_inst)) => {
            let a_val = a_inst.attrs.read().get("value").cloned();
            let b_val = b_inst.attrs.read().get("value").cloned();
            if let (Some(av), Some(bv)) = (a_val, b_val) {
                return av.add(&bv);
            }
            Err(PyException::type_error(format!(
                "unsupported operand type(s) for +: '{}' and '{}'",
                a.type_name(),
                b.type_name()
            )))
        }
        _ => Err(PyException::type_error(format!(
            "unsupported operand type(s) for +: '{}' and '{}'",
            a.type_name(),
            b.type_name()
        ))),
    }
}

pub(super) fn py_sub(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    let ua = unwrap_builtin_subclass(a);
    let ub = unwrap_builtin_subclass(b);
    if !PyObjectRef::ptr_eq(&ua, a) || !PyObjectRef::ptr_eq(&ub, b) {
        return py_sub(&ua, &ub);
    }
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Bool(a), PyObjectPayload::Bool(b)) => {
            Ok(PyObject::int(*a as i64 - *b as i64))
        }
        (PyObjectPayload::Bool(a), PyObjectPayload::Int(b)) => {
            Ok(PyInt::sub_op(&PyInt::Small(*a as i64), b).to_object())
        }
        (PyObjectPayload::Int(a), PyObjectPayload::Bool(b)) => {
            Ok(PyInt::sub_op(a, &PyInt::Small(*b as i64)).to_object())
        }
        (PyObjectPayload::Bool(a), PyObjectPayload::Float(b)) => {
            Ok(PyObject::float(*a as i64 as f64 - b))
        }
        (PyObjectPayload::Float(a), PyObjectPayload::Bool(b)) => {
            Ok(PyObject::float(a - *b as i64 as f64))
        }
        (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => Ok(PyInt::sub_op(a, b).to_object()),
        (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a - b)),
        (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.to_f64() - b)),
        (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => Ok(PyObject::float(a - b.to_f64())),
        (
            PyObjectPayload::Complex { real: ar, imag: ai },
            PyObjectPayload::Complex { real: br, imag: bi },
        ) => Ok(PyObject::complex(ar - br, ai - bi)),
        (PyObjectPayload::Int(a), PyObjectPayload::Complex { real, imag }) => {
            Ok(PyObject::complex(a.to_f64() - real, -*imag))
        }
        (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Int(b)) => {
            Ok(PyObject::complex(real - b.to_f64(), *imag))
        }
        (PyObjectPayload::Float(a), PyObjectPayload::Complex { real, imag }) => {
            Ok(PyObject::complex(a - real, -*imag))
        }
        (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Float(b)) => {
            Ok(PyObject::complex(real - b, *imag))
        }
        (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
            let ra = a.read();
            let rb = b.read();
            let mut result = new_fx_hashkey_flatmap();
            for (k, v) in ra.iter() {
                if !rb.contains_key(k) {
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
            Ok(PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(
                result,
            )))))
        }
        // DictKeys/DictItems set-like difference
        (PyObjectPayload::DictKeys { .. } | PyObjectPayload::DictItems { .. }, _)
        | (_, PyObjectPayload::DictKeys { .. } | PyObjectPayload::DictItems { .. })
            if extract_view_keys(a).is_some() && extract_view_keys(b).is_some() =>
        {
            if let (Some(ak), Some(bk)) = (extract_view_keys(a), extract_view_keys(b)) {
                let mut result = new_fx_hashkey_flatmap();
                for (k, v) in ak.iter() {
                    if !bk.contains_key(k) {
                        result.insert(k.clone(), v.clone());
                    }
                }
                Ok(keys_to_set(result))
            } else {
                Err(PyException::type_error(
                    "dict view changed during operation",
                ))
            }
        }
        // Counter - Counter: subtract counts, keep positive
        (PyObjectPayload::Dict(a_map), PyObjectPayload::Dict(b_map)) => {
            let ra = a_map.read();
            let rb = b_map.read();
            let counter_key = HashableKey::str_key(intern_or_new("__counter__"));
            if ra.contains_key(&counter_key) && rb.contains_key(&counter_key) {
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
                    let a_count = v.as_int().unwrap_or(0);
                    let b_count = rb.get(k).and_then(|v| v.as_int()).unwrap_or(0);
                    let diff = a_count - b_count;
                    if diff > 0 {
                        result.insert(k.clone(), PyObject::int(diff));
                    }
                }
                Ok(PyObject::dict(result))
            } else {
                Err(PyException::type_error(format!(
                    "unsupported operand type(s) for -: '{}' and '{}'",
                    a.type_name(),
                    b.type_name()
                )))
            }
        }
        // Instance subtraction (date - date → timedelta)
        (PyObjectPayload::Instance(a_inst), PyObjectPayload::Instance(b_inst)) => {
            let a_attrs = a_inst.attrs.read();
            let b_attrs = b_inst.attrs.read();
            // date - date or datetime - datetime → timedelta
            if (a_attrs.contains_key("year")
                && a_attrs.contains_key("month")
                && a_attrs.contains_key("day"))
                && (b_attrs.contains_key("year")
                    && b_attrs.contains_key("month")
                    && b_attrs.contains_key("day"))
            {
                let a_y = a_attrs.get("year").and_then(|v| v.as_int()).unwrap_or(0);
                let a_m = a_attrs.get("month").and_then(|v| v.as_int()).unwrap_or(1);
                let a_d = a_attrs.get("day").and_then(|v| v.as_int()).unwrap_or(1);
                let b_y = b_attrs.get("year").and_then(|v| v.as_int()).unwrap_or(0);
                let b_m = b_attrs.get("month").and_then(|v| v.as_int()).unwrap_or(1);
                let b_d = b_attrs.get("day").and_then(|v| v.as_int()).unwrap_or(1);
                fn date_to_days(y: i64, m: i64, d: i64) -> i64 {
                    let m = if m <= 2 { m + 9 } else { m - 3 };
                    let y = if m >= 10 { y - 1 } else { y };
                    365 * y + y / 4 - y / 100 + y / 400 + (m * 306 + 5) / 10 + d - 1
                }
                let diff_days = date_to_days(a_y, a_m, a_d) - date_to_days(b_y, b_m, b_d);
                // Return a timedelta instance
                let cls =
                    PyObject::class(CompactString::from("timedelta"), vec![], IndexMap::new());
                let inst = PyObject::instance(cls);
                if let PyObjectPayload::Instance(ref d) = inst.payload {
                    let mut w = d.attrs.write();
                    w.insert(intern_or_new("__timedelta__"), PyObject::bool_val(true));
                    w.insert(CompactString::from("days"), PyObject::int(diff_days));
                    w.insert(CompactString::from("seconds"), PyObject::int(0));
                    w.insert(CompactString::from("microseconds"), PyObject::int(0));
                    w.insert(
                        CompactString::from("total_seconds"),
                        PyObject::float(diff_days as f64 * 86400.0),
                    );
                    w.insert(
                        CompactString::from("_total_us"),
                        PyObject::int(diff_days * 86_400_000_000),
                    );
                }
                Ok(inst)
            } else {
                Err(PyException::type_error(format!(
                    "unsupported operand type(s) for -: '{}' and '{}'",
                    a.type_name(),
                    b.type_name()
                )))
            }
        }
        _ => Err(PyException::type_error(format!(
            "unsupported operand type(s) for -: '{}' and '{}'",
            a.type_name(),
            b.type_name()
        ))),
    }
}

pub(super) fn py_mul(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    let ua = unwrap_builtin_subclass(a);
    let ub = unwrap_builtin_subclass(b);
    if !PyObjectRef::ptr_eq(&ua, a) || !PyObjectRef::ptr_eq(&ub, b) {
        return py_mul(&ua, &ub);
    }
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Bool(a), PyObjectPayload::Bool(b)) => {
            Ok(PyObject::int(*a as i64 * *b as i64))
        }
        (PyObjectPayload::Bool(a), PyObjectPayload::Int(b)) => {
            Ok(PyInt::mul_op(&PyInt::Small(*a as i64), b).to_object())
        }
        (PyObjectPayload::Int(a), PyObjectPayload::Bool(b)) => {
            Ok(PyInt::mul_op(a, &PyInt::Small(*b as i64)).to_object())
        }
        (PyObjectPayload::Bool(a), PyObjectPayload::Float(b)) => {
            Ok(PyObject::float(*a as i64 as f64 * b))
        }
        (PyObjectPayload::Float(a), PyObjectPayload::Bool(b)) => {
            Ok(PyObject::float(a * *b as i64 as f64))
        }
        (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => Ok(PyInt::mul_op(a, b).to_object()),
        (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a * b)),
        (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.to_f64() * b)),
        (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => Ok(PyObject::float(a * b.to_f64())),
        (
            PyObjectPayload::Complex { real: ar, imag: ai },
            PyObjectPayload::Complex { real: br, imag: bi },
        ) => Ok(PyObject::complex(ar * br - ai * bi, ar * bi + ai * br)),
        (PyObjectPayload::Int(a), PyObjectPayload::Complex { real, imag })
        | (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Int(a)) => {
            let af = a.to_f64();
            Ok(PyObject::complex(af * real, af * imag))
        }
        (PyObjectPayload::Float(a), PyObjectPayload::Complex { real, imag })
        | (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Float(a)) => {
            Ok(PyObject::complex(a * real, a * imag))
        }
        (PyObjectPayload::Str(s), PyObjectPayload::Int(n))
        | (PyObjectPayload::Int(n), PyObjectPayload::Str(s)) => {
            let count = index_to_usize_repeat(&n.to_object())?;
            checked_repeat_len(s.len(), count, "str repeat")?;
            Ok(PyObject::str_val(CompactString::from(s.repeat(count))))
        }
        (PyObjectPayload::Str(s), PyObjectPayload::Bool(b))
        | (PyObjectPayload::Bool(b), PyObjectPayload::Str(s)) => {
            let count = *b as usize;
            checked_repeat_len(s.len(), count, "str repeat")?;
            Ok(PyObject::str_val(CompactString::from(s.repeat(count))))
        }
        (PyObjectPayload::List(items), PyObjectPayload::Int(n))
        | (PyObjectPayload::Int(n), PyObjectPayload::List(items)) => {
            let count = index_to_usize_repeat(&n.to_object())?;
            let read = items.read();
            let size = checked_repeat_len(read.len(), count, "list repeat")?;
            let mut result = Vec::with_capacity(size);
            for _ in 0..count {
                result.extend(read.iter().cloned());
            }
            Ok(PyObject::list(result))
        }
        (PyObjectPayload::List(items), PyObjectPayload::Bool(b))
        | (PyObjectPayload::Bool(b), PyObjectPayload::List(items)) => {
            let count = *b as usize;
            let read = items.read();
            let size = checked_repeat_len(read.len(), count, "list repeat")?;
            let mut result = Vec::with_capacity(size);
            for _ in 0..count {
                result.extend(read.iter().cloned());
            }
            Ok(PyObject::list(result))
        }
        (PyObjectPayload::Tuple(items), PyObjectPayload::Int(n))
        | (PyObjectPayload::Int(n), PyObjectPayload::Tuple(items)) => {
            if matches!(n, PyInt::Small(1)) {
                return Ok(if matches!(a.payload, PyObjectPayload::Tuple(_)) {
                    a.clone()
                } else {
                    b.clone()
                });
            }
            let count = index_to_usize_repeat(&n.to_object())?;
            let size = checked_repeat_len(items.len(), count, "tuple repeat")?;
            let mut result = Vec::with_capacity(size);
            for _ in 0..count {
                result.extend(items.iter().cloned());
            }
            Ok(PyObject::tuple(result))
        }
        (PyObjectPayload::Tuple(items), PyObjectPayload::Bool(flag))
        | (PyObjectPayload::Bool(flag), PyObjectPayload::Tuple(items)) => {
            let count = *flag as usize;
            if count == 1 {
                return Ok(if matches!(a.payload, PyObjectPayload::Tuple(_)) {
                    a.clone()
                } else {
                    b.clone()
                });
            }
            let size = checked_repeat_len(items.len(), count, "tuple repeat")?;
            let mut result = Vec::with_capacity(size);
            for _ in 0..count {
                result.extend(items.iter().cloned());
            }
            Ok(PyObject::tuple(result))
        }
        (PyObjectPayload::Bytes(b), PyObjectPayload::Int(n))
        | (PyObjectPayload::Int(n), PyObjectPayload::Bytes(b)) => {
            let count = index_to_usize_repeat(&n.to_object())?;
            let size = checked_repeat_len(b.len(), count, "bytes repeat")?;
            let mut result = Vec::with_capacity(size);
            for _ in 0..count {
                result.extend(b.iter());
            }
            Ok(PyObject::bytes(result))
        }
        (PyObjectPayload::Bytes(bytes), PyObjectPayload::Bool(bl))
        | (PyObjectPayload::Bool(bl), PyObjectPayload::Bytes(bytes)) => {
            let count = *bl as usize;
            let size = checked_repeat_len(bytes.len(), count, "bytes repeat")?;
            let mut result = Vec::with_capacity(size);
            for _ in 0..count {
                result.extend(bytes.iter());
            }
            Ok(PyObject::bytes(result))
        }
        (PyObjectPayload::ByteArray(b), PyObjectPayload::Int(n))
        | (PyObjectPayload::Int(n), PyObjectPayload::ByteArray(b)) => {
            let count = index_to_usize_repeat(&n.to_object())?;
            let size = checked_repeat_len(b.len(), count, "bytearray repeat")?;
            let mut result = Vec::with_capacity(size);
            for _ in 0..count {
                result.extend(b.iter());
            }
            Ok(PyObject::bytearray(result))
        }
        (PyObjectPayload::Str(s), _) | (_, PyObjectPayload::Str(s)) => {
            let count = index_to_usize_repeat(if matches!(a.payload, PyObjectPayload::Str(_)) {
                b
            } else {
                a
            })?;
            checked_repeat_len(s.len(), count, "str repeat")?;
            Ok(PyObject::str_val(CompactString::from(s.repeat(count))))
        }
        (PyObjectPayload::List(items), _) | (_, PyObjectPayload::List(items)) => {
            let count = index_to_usize_repeat(if matches!(a.payload, PyObjectPayload::List(_)) {
                b
            } else {
                a
            })?;
            let read = items.read();
            let size = checked_repeat_len(read.len(), count, "list repeat")?;
            let mut result = Vec::with_capacity(size);
            for _ in 0..count {
                result.extend(read.iter().cloned());
            }
            Ok(PyObject::list(result))
        }
        (PyObjectPayload::Tuple(items), _) | (_, PyObjectPayload::Tuple(items)) => {
            let count_obj = if matches!(a.payload, PyObjectPayload::Tuple(_)) {
                b
            } else {
                a
            };
            if matches!(count_obj.to_index()?, PyInt::Small(1)) {
                return Ok(if matches!(a.payload, PyObjectPayload::Tuple(_)) {
                    a.clone()
                } else {
                    b.clone()
                });
            }
            let count = index_to_usize_repeat(count_obj)?;
            let size = checked_repeat_len(items.len(), count, "tuple repeat")?;
            let mut result = Vec::with_capacity(size);
            for _ in 0..count {
                result.extend(items.iter().cloned());
            }
            Ok(PyObject::tuple(result))
        }
        (PyObjectPayload::Bytes(bytes), _) | (_, PyObjectPayload::Bytes(bytes)) => {
            let count = index_to_usize_repeat(if matches!(a.payload, PyObjectPayload::Bytes(_)) {
                b
            } else {
                a
            })?;
            let size = checked_repeat_len(bytes.len(), count, "bytes repeat")?;
            let mut result = Vec::with_capacity(size);
            for _ in 0..count {
                result.extend(bytes.iter());
            }
            Ok(PyObject::bytes(result))
        }
        (PyObjectPayload::ByteArray(bytes), _) | (_, PyObjectPayload::ByteArray(bytes)) => {
            let count =
                index_to_usize_repeat(if matches!(a.payload, PyObjectPayload::ByteArray(_)) {
                    b
                } else {
                    a
                })?;
            let size = checked_repeat_len(bytes.len(), count, "bytearray repeat")?;
            let mut result = Vec::with_capacity(size);
            for _ in 0..count {
                result.extend(bytes.iter());
            }
            Ok(PyObject::bytearray(result))
        }
        _ => Err(PyException::type_error(format!(
            "unsupported operand type(s) for *: '{}' and '{}'",
            a.type_name(),
            b.type_name()
        ))),
    }
}

pub(super) fn py_floor_div(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    let ua = unwrap_builtin_subclass(a);
    let ub = unwrap_builtin_subclass(b);
    if !PyObjectRef::ptr_eq(&ua, a) || !PyObjectRef::ptr_eq(&ub, b) {
        return py_floor_div(&ua, &ub);
    }
    if let (Some(ai), _) = (bool_as_int(a), ()) {
        return py_floor_div(&ai, b);
    }
    if let (_, Some(bi)) = ((), bool_as_int(b)) {
        return py_floor_div(a, &bi);
    }
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => {
            if b.is_zero() {
                return Err(PyException::zero_division_error(
                    "integer division or modulo by zero",
                ));
            }
            Ok(PyInt::floor_div_op(a, b).to_object())
        }
        (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => {
            if *b == 0.0 {
                return Err(PyException::zero_division_error(
                    "float floor division by zero",
                ));
            }
            Ok(PyObject::float((a / b).floor()))
        }
        (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => {
            if *b == 0.0 {
                return Err(PyException::zero_division_error(
                    "float floor division by zero",
                ));
            }
            Ok(PyObject::float((a.to_f64() / b).floor()))
        }
        (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => {
            if b.is_zero() {
                return Err(PyException::zero_division_error(
                    "float floor division by zero",
                ));
            }
            Ok(PyObject::float((a / b.to_f64()).floor()))
        }
        _ => Err(PyException::type_error(format!(
            "unsupported operand type(s) for //: '{}' and '{}'",
            a.type_name(),
            b.type_name()
        ))),
    }
}

pub(super) fn py_true_div(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    let ua = unwrap_builtin_subclass(a);
    let ub = unwrap_builtin_subclass(b);
    if !PyObjectRef::ptr_eq(&ua, a) || !PyObjectRef::ptr_eq(&ub, b) {
        return py_true_div(&ua, &ub);
    }
    // Complex division
    match (&a.payload, &b.payload) {
        (
            PyObjectPayload::Complex { real: ar, imag: ai },
            PyObjectPayload::Complex { real: br, imag: bi },
        ) => {
            // Smith's algorithm (CPython _Py_c_quot) — numerically stable
            let (abs_breal, abs_bimag) = (br.abs(), bi.abs());
            if abs_breal == 0.0 && abs_bimag == 0.0 {
                return Err(PyException::zero_division_error("complex division by zero"));
            }
            let (rr, ii) = if abs_breal >= abs_bimag {
                let ratio = bi / br;
                let denom = br + bi * ratio;
                ((ar + ai * ratio) / denom, (ai - ar * ratio) / denom)
            } else {
                let ratio = br / bi;
                let denom = br * ratio + bi;
                ((ar * ratio + ai) / denom, (ai * ratio - ar) / denom)
            };
            return Ok(PyObject::complex(rr, ii));
        }
        (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Int(b)) => {
            let bf = b.to_f64();
            if bf == 0.0 {
                return Err(PyException::zero_division_error("complex division by zero"));
            }
            return Ok(PyObject::complex(real / bf, imag / bf));
        }
        (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Float(b)) => {
            if *b == 0.0 {
                return Err(PyException::zero_division_error("complex division by zero"));
            }
            return Ok(PyObject::complex(real / b, imag / b));
        }
        (PyObjectPayload::Int(a), PyObjectPayload::Complex { real: br, imag: bi }) => {
            let af = a.to_f64();
            let (abs_breal, abs_bimag) = (br.abs(), bi.abs());
            if abs_breal == 0.0 && abs_bimag == 0.0 {
                return Err(PyException::zero_division_error("complex division by zero"));
            }
            let (rr, ii) = if abs_breal >= abs_bimag {
                let ratio = bi / br;
                let denom = br + bi * ratio;
                (af / denom, -af * ratio / denom)
            } else {
                let ratio = br / bi;
                let denom = br * ratio + bi;
                (af * ratio / denom, -af / denom)
            };
            return Ok(PyObject::complex(rr, ii));
        }
        (PyObjectPayload::Float(a), PyObjectPayload::Complex { real: br, imag: bi }) => {
            let (abs_breal, abs_bimag) = (br.abs(), bi.abs());
            if abs_breal == 0.0 && abs_bimag == 0.0 {
                return Err(PyException::zero_division_error("complex division by zero"));
            }
            let (rr, ii) = if abs_breal >= abs_bimag {
                let ratio = bi / br;
                let denom = br + bi * ratio;
                (a / denom, -a * ratio / denom)
            } else {
                let ratio = br / bi;
                let denom = br * ratio + bi;
                (a * ratio / denom, -a / denom)
            };
            return Ok(PyObject::complex(rr, ii));
        }
        _ => {}
    }
    // Path / str → joined path (pathlib)
    if let PyObjectPayload::Instance(inst) = &a.payload {
        if inst.attrs.read().contains_key("__pathlib_path__") {
            let base = inst
                .attrs
                .read()
                .get("_path")
                .map(|v| v.py_to_string())
                .unwrap_or_default();
            let child = b.py_to_string();
            let joined = std::path::Path::new(&base).join(&child);
            let joined_str = joined.to_string_lossy().to_string();
            // Return a new Path-like instance
            let path = std::path::Path::new(&joined_str);
            let cls = PyObject::class(CompactString::from("Path"), vec![], IndexMap::new());
            let new_inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = new_inst.payload {
                let mut w = d.attrs.write();
                w.insert(intern_or_new("__pathlib_path__"), PyObject::bool_val(true));
                w.insert(
                    CompactString::from("_path"),
                    PyObject::str_val(CompactString::from(&joined_str)),
                );
                w.insert(
                    CompactString::from("name"),
                    PyObject::str_val(CompactString::from(
                        path.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default(),
                    )),
                );
                w.insert(
                    CompactString::from("stem"),
                    PyObject::str_val(CompactString::from(
                        path.file_stem()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default(),
                    )),
                );
                w.insert(
                    CompactString::from("suffix"),
                    PyObject::str_val(CompactString::from(
                        path.extension()
                            .map(|e| format!(".{}", e.to_string_lossy()))
                            .unwrap_or_default(),
                    )),
                );
                w.insert(
                    CompactString::from("parent"),
                    PyObject::str_val(CompactString::from(
                        path.parent()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_default(),
                    )),
                );
                let parts: Vec<PyObjectRef> = {
                    let mut p = Vec::new();
                    if joined_str.starts_with('/') {
                        p.push(PyObject::str_val(CompactString::from("/")));
                    }
                    for c in path.components() {
                        let s = c.as_os_str().to_string_lossy().to_string();
                        if s != "/" {
                            p.push(PyObject::str_val(CompactString::from(&s)));
                        }
                    }
                    p
                };
                w.insert(CompactString::from("parts"), PyObject::tuple(parts));
            }
            return Ok(new_inst);
        }
    }
    let a = coerce_to_f64(a)?;
    let b = coerce_to_f64(b)?;
    if b == 0.0 {
        return Err(PyException::zero_division_error("division by zero"));
    }
    Ok(PyObject::float(a / b))
}

pub(super) fn py_power(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    let ua = unwrap_builtin_subclass(a);
    let ub = unwrap_builtin_subclass(b);
    if !PyObjectRef::ptr_eq(&ua, a) || !PyObjectRef::ptr_eq(&ub, b) {
        return py_power(&ua, &ub);
    }
    if let Some(ai) = bool_as_int(a) {
        return py_power(&ai, b);
    }
    if let Some(bi) = bool_as_int(b) {
        return py_power(a, &bi);
    }
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => {
            if let Some(exp) = b.to_i64() {
                if exp >= 0 {
                    let e = exp as u32;
                    return Ok(PyInt::pow_op(a, e).to_object());
                } else if a.is_zero() {
                    return Err(PyException::zero_division_error(
                        "0.0 cannot be raised to a negative power",
                    ));
                }
            }
            // Negative exponent → float result
            Ok(PyObject::float(a.to_f64().powf(b.to_f64())))
        }
        (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => {
            if *a == 0.0 && *b < 0.0 {
                return Err(PyException::zero_division_error(
                    "0.0 cannot be raised to a negative power",
                ));
            }
            Ok(PyObject::float(a.powf(*b)))
        }
        (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => {
            if a.is_zero() && *b < 0.0 {
                return Err(PyException::zero_division_error(
                    "0.0 cannot be raised to a negative power",
                ));
            }
            Ok(PyObject::float(a.to_f64().powf(*b)))
        }
        (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => {
            if *a == 0.0 && b.to_f64() < 0.0 {
                return Err(PyException::zero_division_error(
                    "0.0 cannot be raised to a negative power",
                ));
            }
            Ok(PyObject::float(a.powf(b.to_f64())))
        }
        // Complex exponentiation — delegate to call_complex_method via dunder dispatch path
        (PyObjectPayload::Complex { real: ar, imag: ai }, _) => {
            let (br, bi) = match &b.payload {
                PyObjectPayload::Complex { real, imag } => (*real, *imag),
                PyObjectPayload::Int(n) => (n.to_f64(), 0.0),
                PyObjectPayload::Float(f) => (*f, 0.0),
                PyObjectPayload::Bool(x) => (if *x { 1.0 } else { 0.0 }, 0.0),
                _ => {
                    return Err(PyException::type_error(format!(
                        "unsupported operand type(s) for **: '{}' and '{}'",
                        a.type_name(),
                        b.type_name()
                    )))
                }
            };
            complex_pow_inline(*ar, *ai, br, bi)
        }
        (_, PyObjectPayload::Complex { real: br, imag: bi }) => {
            let (ar, ai) = match &a.payload {
                PyObjectPayload::Int(n) => (n.to_f64(), 0.0),
                PyObjectPayload::Float(f) => (*f, 0.0),
                PyObjectPayload::Bool(x) => (if *x { 1.0 } else { 0.0 }, 0.0),
                _ => {
                    return Err(PyException::type_error(format!(
                        "unsupported operand type(s) for **: '{}' and '{}'",
                        a.type_name(),
                        b.type_name()
                    )))
                }
            };
            complex_pow_inline(ar, ai, *br, *bi)
        }
        _ => Err(PyException::type_error(format!(
            "unsupported operand type(s) for **: '{}' and '{}'",
            a.type_name(),
            b.type_name()
        ))),
    }
}

fn complex_pow_inline(ar: f64, ai: f64, br: f64, bi: f64) -> PyResult<PyObjectRef> {
    if ar == 0.0 && ai == 0.0 {
        if bi != 0.0 || br < 0.0 {
            return Err(PyException::zero_division_error(
                "0.0 to a negative or complex power",
            ));
        }
        if br == 0.0 {
            return Ok(PyObject::complex(1.0, 0.0));
        }
        return Ok(PyObject::complex(0.0, 0.0));
    }
    let r = (ar * ar + ai * ai).sqrt();
    let theta = ai.atan2(ar);
    let new_r = r.powf(br) * (-bi * theta).exp();
    let new_theta = bi * r.ln() + br * theta;
    if !new_r.is_finite() || !new_theta.is_finite() {
        return Err(PyException::overflow_error("complex exponentiation"));
    }
    Ok(PyObject::complex(
        new_r * new_theta.cos(),
        new_r * new_theta.sin(),
    ))
}

pub(super) fn py_negate(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    let u = unwrap_builtin_subclass(obj);
    if !PyObjectRef::ptr_eq(&u, obj) {
        return py_negate(&u);
    }
    match &obj.payload {
        PyObjectPayload::Int(n) => Ok(n.negate().to_object()),
        PyObjectPayload::Float(f) => Ok(PyObject::float(-f)),
        PyObjectPayload::Bool(b) => Ok(PyObject::int(-(*b as i64))),
        PyObjectPayload::Complex { real, imag } => Ok(PyObject::complex(-real, -imag)),
        _ => Err(PyException::type_error(format!(
            "bad operand type for unary -: '{}'",
            obj.type_name()
        ))),
    }
}

pub(super) fn py_positive(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    let u = unwrap_builtin_subclass(obj);
    if !PyObjectRef::ptr_eq(&u, obj) {
        return py_positive(&u);
    }
    match &obj.payload {
        PyObjectPayload::Int(_)
        | PyObjectPayload::Float(_)
        | PyObjectPayload::Bool(_)
        | PyObjectPayload::Complex { .. } => Ok(obj.clone()),
        _ => Err(PyException::type_error(format!(
            "bad operand type for unary +: '{}'",
            obj.type_name()
        ))),
    }
}

pub(super) fn py_invert(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    let u = unwrap_builtin_subclass(obj);
    if !PyObjectRef::ptr_eq(&u, obj) {
        return py_invert(&u);
    }
    match &obj.payload {
        PyObjectPayload::Int(n) => Ok(n.invert().to_object()),
        PyObjectPayload::Bool(b) => Ok(PyObject::int(!(*b as i64))),
        _ => Err(PyException::type_error(format!(
            "bad operand type for unary ~: '{}'",
            obj.type_name()
        ))),
    }
}

pub(super) fn py_abs(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    let u = unwrap_builtin_subclass(obj);
    if !PyObjectRef::ptr_eq(&u, obj) {
        return py_abs(&u);
    }
    match &obj.payload {
        PyObjectPayload::Int(n) => Ok(n.abs().to_object()),
        PyObjectPayload::Float(f) => Ok(PyObject::float(f.abs())),
        PyObjectPayload::Bool(b) => Ok(PyObject::int(*b as i64)),
        PyObjectPayload::Complex { real, imag } => {
            let result = real.hypot(*imag);
            if result.is_infinite() && real.is_finite() && imag.is_finite() {
                return Err(PyException::overflow_error("absolute value too large"));
            }
            Ok(PyObject::float(result))
        }
        _ => Err(PyException::type_error(format!(
            "bad operand type for abs(): '{}'",
            obj.type_name()
        ))),
    }
}
