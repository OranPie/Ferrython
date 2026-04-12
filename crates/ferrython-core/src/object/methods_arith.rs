//! Arithmetic operation methods.

use std::rc::Rc;
use crate::error::{PyException, PyResult};
use crate::intern::intern_or_new;
use crate::types::{PyInt, HashableKey};
use compact_str::CompactString;
use indexmap::IndexMap;

use super::payload::*;
use super::helpers::*;
use super::methods::PyObjectMethods;

/// Extract keys as HashableKey set from DictKeys or DictItems view.
fn extract_view_keys(obj: &PyObjectRef) -> Option<FxHashKeyMap> {
    match &obj.payload {
        PyObjectPayload::DictKeys(m) => {
            let r = m.read();
            Some(r.keys().map(|k| (k.clone(), k.to_object())).collect())
        }
        PyObjectPayload::DictItems(m) => {
            let r = m.read();
            Some(r.iter().map(|(k, v)| {
                let tuple_obj = PyObject::tuple(vec![k.to_object(), v.clone()]);
                let tuple_key = HashableKey::Tuple(vec![k.clone(), HashableKey::from_object(v).unwrap_or(HashableKey::None)]);
                (tuple_key, tuple_obj)
            }).collect())
        }
        PyObjectPayload::Set(s) => Some(s.read().clone()),
        PyObjectPayload::FrozenSet(s) => Some(s.as_ref().clone()),
        _ => None,
    }
}

/// Build a set result from an IndexMap of HashableKey.
fn keys_to_set(keys: FxHashKeyMap) -> PyObjectRef {
    PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(keys))))
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
            (PyObjectPayload::Bool(a), PyObjectPayload::Bool(b)) => Ok(PyObject::int(*a as i64 + *b as i64)),
            (PyObjectPayload::Bool(a), PyObjectPayload::Int(b)) => Ok(PyInt::add_op(&PyInt::Small(*a as i64), b).to_object()),
            (PyObjectPayload::Int(a), PyObjectPayload::Bool(b)) => Ok(PyInt::add_op(a, &PyInt::Small(*b as i64)).to_object()),
            (PyObjectPayload::Bool(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(*a as i64 as f64 + b)),
            (PyObjectPayload::Float(a), PyObjectPayload::Bool(b)) => Ok(PyObject::float(a + *b as i64 as f64)),
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => Ok(PyInt::add_op(a, b).to_object()),
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a + b)),
            (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.to_f64() + b)),
            (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => Ok(PyObject::float(a + b.to_f64())),
            (PyObjectPayload::Complex { real: ar, imag: ai }, PyObjectPayload::Complex { real: br, imag: bi }) => {
                Ok(PyObject::complex(ar + br, ai + bi))
            }
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
                let mut s = a.to_string(); s.push_str(b.as_str());
                Ok(PyObject::str_val(CompactString::from(s)))
            }
            (PyObjectPayload::List(a), PyObjectPayload::List(b)) => {
                let mut r = a.read().clone(); r.extend(b.read().iter().cloned()); Ok(PyObject::list(r))
            }
            (PyObjectPayload::Tuple(a), PyObjectPayload::Tuple(b)) => {
                let mut r = a.clone(); r.extend(b.iter().cloned()); Ok(PyObject::tuple(r))
            }
            (PyObjectPayload::Bytes(a), PyObjectPayload::Bytes(b)) | (PyObjectPayload::ByteArray(a), PyObjectPayload::Bytes(b)) | (PyObjectPayload::Bytes(a), PyObjectPayload::ByteArray(b)) => {
                let mut r = a.clone(); r.extend(b); Ok(PyObject::bytes(r))
            }
            // Dict addition (Counter + Counter)
            (PyObjectPayload::Dict(a_map), PyObjectPayload::Dict(b_map)) => {
                let ra = a_map.read();
                let rb = b_map.read();
                let mut result = new_fx_hashkey_map();
                // Copy non-marker keys from a
                for (k, v) in ra.iter() {
                    if let HashableKey::Str(s) = k {
                        if s.starts_with("__") && s.ends_with("__") { continue; }
                    }
                    result.insert(k.clone(), v.clone());
                }
                // Merge non-marker keys from b
                for (k, v) in rb.iter() {
                    if let HashableKey::Str(s) = k {
                        if s.starts_with("__") && s.ends_with("__") { continue; }
                    }
                    let existing = result.get(k).and_then(|e| e.as_int()).unwrap_or(0);
                    let new_val = existing + v.as_int().unwrap_or(0);
                    result.insert(k.clone(), PyObject::int(new_val));
                }
                // Preserve __counter__ and __defaultdict_factory__ markers if both inputs are counters
                let a_is_counter = ra.contains_key(&HashableKey::Str(intern_or_new("__counter__")));
                let b_is_counter = rb.contains_key(&HashableKey::Str(intern_or_new("__counter__")));
                if a_is_counter && b_is_counter {
                    result.insert(HashableKey::Str(intern_or_new("__counter__")), PyObject::bool_val(true));
                    if let Some(factory) = ra.get(&HashableKey::Str(intern_or_new("__defaultdict_factory__"))) {
                        result.insert(HashableKey::Str(intern_or_new("__defaultdict_factory__")), factory.clone());
                    }
                }
                Ok(PyObject::wrap(PyObjectPayload::Dict(Rc::new(PyCell::new(result)))))
            }
            // IntEnum: Instance + Instance → extract .value and add
            (PyObjectPayload::Instance(a_inst), PyObjectPayload::Instance(b_inst)) => {
                let a_val = a_inst.attrs.read().get("value").cloned();
                let b_val = b_inst.attrs.read().get("value").cloned();
                if let (Some(av), Some(bv)) = (a_val, b_val) {
                    return av.add(&bv);
                }
                Err(PyException::type_error(format!("unsupported operand type(s) for +: '{}' and '{}'", a.type_name(), b.type_name())))
            }
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for +: '{}' and '{}'", a.type_name(), b.type_name()))),
        }
}

pub(super) fn py_sub(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
        let ua = unwrap_builtin_subclass(a);
        let ub = unwrap_builtin_subclass(b);
        if !PyObjectRef::ptr_eq(&ua, a) || !PyObjectRef::ptr_eq(&ub, b) {
            return py_sub(&ua, &ub);
        }
        match (&a.payload, &b.payload) {
            (PyObjectPayload::Bool(a), PyObjectPayload::Bool(b)) => Ok(PyObject::int(*a as i64 - *b as i64)),
            (PyObjectPayload::Bool(a), PyObjectPayload::Int(b)) => Ok(PyInt::sub_op(&PyInt::Small(*a as i64), b).to_object()),
            (PyObjectPayload::Int(a), PyObjectPayload::Bool(b)) => Ok(PyInt::sub_op(a, &PyInt::Small(*b as i64)).to_object()),
            (PyObjectPayload::Bool(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(*a as i64 as f64 - b)),
            (PyObjectPayload::Float(a), PyObjectPayload::Bool(b)) => Ok(PyObject::float(a - *b as i64 as f64)),
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => Ok(PyInt::sub_op(a, b).to_object()),
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a - b)),
            (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.to_f64() - b)),
            (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => Ok(PyObject::float(a - b.to_f64())),
            (PyObjectPayload::Complex { real: ar, imag: ai }, PyObjectPayload::Complex { real: br, imag: bi }) => {
                Ok(PyObject::complex(ar - br, ai - bi))
            }
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
                let ra = a.read(); let rb = b.read();
                let mut result = new_fx_hashkey_map();
                for (k, v) in ra.iter() { if !rb.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(result)))))
            }
            (PyObjectPayload::FrozenSet(a), PyObjectPayload::FrozenSet(b)) => {
                let mut result = new_fx_hashkey_map();
                for (k, v) in a.iter() { if !b.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::FrozenSet(Box::new(result))))
            }
            (PyObjectPayload::FrozenSet(a), PyObjectPayload::Set(b)) => {
                let rb = b.read();
                let mut result = new_fx_hashkey_map();
                for (k, v) in a.iter() { if !rb.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::FrozenSet(Box::new(result))))
            }
            (PyObjectPayload::Set(a), PyObjectPayload::FrozenSet(b)) => {
                let ra = a.read();
                let mut result = new_fx_hashkey_map();
                for (k, v) in ra.iter() { if !b.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(result)))))
            }
            // DictKeys/DictItems set-like difference
            (PyObjectPayload::DictKeys(_) | PyObjectPayload::DictItems(_), _)
            | (_, PyObjectPayload::DictKeys(_) | PyObjectPayload::DictItems(_))
                if extract_view_keys(a).is_some() && extract_view_keys(b).is_some() => {
                if let (Some(ak), Some(bk)) = (extract_view_keys(a), extract_view_keys(b)) {
                    let mut result = new_fx_hashkey_map();
                    for (k, v) in ak.iter() { if !bk.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                    Ok(keys_to_set(result))
                } else {
                    Err(PyException::type_error("dict view changed during operation"))
                }
            }
            // Counter - Counter: subtract counts, keep positive
            (PyObjectPayload::Dict(a_map), PyObjectPayload::Dict(b_map)) => {
                let ra = a_map.read(); let rb = b_map.read();
                let counter_key = HashableKey::Str(intern_or_new("__counter__"));
                if ra.contains_key(&counter_key) && rb.contains_key(&counter_key) {
                    let mut result = new_fx_hashkey_map();
                    result.insert(HashableKey::Str(intern_or_new("__defaultdict_factory__")),
                        PyObject::builtin_type(CompactString::from("int")));
                    result.insert(counter_key, PyObject::bool_val(true));
                    for (k, v) in ra.iter() {
                        if let HashableKey::Str(s) = k {
                            if s.starts_with("__") && s.ends_with("__") { continue; }
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
                        "unsupported operand type(s) for -: '{}' and '{}'", a.type_name(), b.type_name()
                    )))
                }
            }
            // Instance subtraction (date - date → timedelta)
            (PyObjectPayload::Instance(a_inst), PyObjectPayload::Instance(b_inst)) => {
                let a_attrs = a_inst.attrs.read();
                let b_attrs = b_inst.attrs.read();
                // date - date or datetime - datetime → timedelta
                if (a_attrs.contains_key("year") && a_attrs.contains_key("month") && a_attrs.contains_key("day"))
                    && (b_attrs.contains_key("year") && b_attrs.contains_key("month") && b_attrs.contains_key("day"))
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
                    let cls = PyObject::class(CompactString::from("timedelta"), vec![], IndexMap::new());
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref d) = inst.payload {
                        let mut w = d.attrs.write();
                        w.insert(intern_or_new("__timedelta__"), PyObject::bool_val(true));
                        w.insert(CompactString::from("days"), PyObject::int(diff_days));
                        w.insert(CompactString::from("seconds"), PyObject::int(0));
                        w.insert(CompactString::from("microseconds"), PyObject::int(0));
                        w.insert(CompactString::from("total_seconds"), PyObject::float(diff_days as f64 * 86400.0));
                        w.insert(CompactString::from("_total_us"), PyObject::int(diff_days * 86_400_000_000));
                    }
                    Ok(inst)
                } else {
                    Err(PyException::type_error(format!("unsupported operand type(s) for -: '{}' and '{}'", a.type_name(), b.type_name())))
                }
            }
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for -: '{}' and '{}'", a.type_name(), b.type_name()))),
        }
}

pub(super) fn py_mul(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
        let ua = unwrap_builtin_subclass(a);
        let ub = unwrap_builtin_subclass(b);
        if !PyObjectRef::ptr_eq(&ua, a) || !PyObjectRef::ptr_eq(&ub, b) {
            return py_mul(&ua, &ub);
        }
        match (&a.payload, &b.payload) {
            (PyObjectPayload::Bool(a), PyObjectPayload::Bool(b)) => Ok(PyObject::int(*a as i64 * *b as i64)),
            (PyObjectPayload::Bool(a), PyObjectPayload::Int(b)) => Ok(PyInt::mul_op(&PyInt::Small(*a as i64), b).to_object()),
            (PyObjectPayload::Int(a), PyObjectPayload::Bool(b)) => Ok(PyInt::mul_op(a, &PyInt::Small(*b as i64)).to_object()),
            (PyObjectPayload::Bool(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(*a as i64 as f64 * b)),
            (PyObjectPayload::Float(a), PyObjectPayload::Bool(b)) => Ok(PyObject::float(a * *b as i64 as f64)),
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => Ok(PyInt::mul_op(a, b).to_object()),
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a * b)),
            (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.to_f64() * b)),
            (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => Ok(PyObject::float(a * b.to_f64())),
            (PyObjectPayload::Complex { real: ar, imag: ai }, PyObjectPayload::Complex { real: br, imag: bi }) => {
                Ok(PyObject::complex(ar * br - ai * bi, ar * bi + ai * br))
            }
            (PyObjectPayload::Int(a), PyObjectPayload::Complex { real, imag }) |
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Int(a)) => {
                let af = a.to_f64();
                Ok(PyObject::complex(af * real, af * imag))
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Complex { real, imag }) |
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Float(a)) => {
                Ok(PyObject::complex(a * real, a * imag))
            }
            (PyObjectPayload::Str(s), PyObjectPayload::Int(n)) | (PyObjectPayload::Int(n), PyObjectPayload::Str(s)) => {
                let count = n.to_i64().unwrap_or(0).max(0) as usize;
                Ok(PyObject::str_val(CompactString::from(s.repeat(count))))
            }
            (PyObjectPayload::Str(s), PyObjectPayload::Bool(b)) | (PyObjectPayload::Bool(b), PyObjectPayload::Str(s)) => {
                Ok(PyObject::str_val(CompactString::from(s.repeat(*b as usize))))
            }
            (PyObjectPayload::List(items), PyObjectPayload::Int(n)) | (PyObjectPayload::Int(n), PyObjectPayload::List(items)) => {
                let count = n.to_i64().unwrap_or(0).max(0) as usize;
                let read = items.read();
                let mut result = Vec::with_capacity(read.len() * count);
                for _ in 0..count { result.extend(read.iter().cloned()); }
                Ok(PyObject::list(result))
            }
            (PyObjectPayload::List(items), PyObjectPayload::Bool(b)) | (PyObjectPayload::Bool(b), PyObjectPayload::List(items)) => {
                let count = *b as usize;
                let read = items.read();
                let mut result = Vec::with_capacity(read.len() * count);
                for _ in 0..count { result.extend(read.iter().cloned()); }
                Ok(PyObject::list(result))
            }
            (PyObjectPayload::Tuple(items), PyObjectPayload::Int(n)) | (PyObjectPayload::Int(n), PyObjectPayload::Tuple(items)) => {
                let count = n.to_i64().unwrap_or(0).max(0) as usize;
                let mut result = Vec::with_capacity(items.len() * count);
                for _ in 0..count { result.extend(items.iter().cloned()); }
                Ok(PyObject::tuple(result))
            }
            (PyObjectPayload::Tuple(items), PyObjectPayload::Bool(b)) | (PyObjectPayload::Bool(b), PyObjectPayload::Tuple(items)) => {
                let count = *b as usize;
                let mut result = Vec::with_capacity(items.len() * count);
                for _ in 0..count { result.extend(items.iter().cloned()); }
                Ok(PyObject::tuple(result))
            }
            (PyObjectPayload::Bytes(b), PyObjectPayload::Int(n)) | (PyObjectPayload::Int(n), PyObjectPayload::Bytes(b)) => {
                let count = n.to_i64().unwrap_or(0).max(0) as usize;
                let mut result = Vec::with_capacity(b.len() * count);
                for _ in 0..count { result.extend(b); }
                Ok(PyObject::bytes(result))
            }
            (PyObjectPayload::Bytes(bytes), PyObjectPayload::Bool(bl)) | (PyObjectPayload::Bool(bl), PyObjectPayload::Bytes(bytes)) => {
                let count = *bl as usize;
                let mut result = Vec::with_capacity(bytes.len() * count);
                for _ in 0..count { result.extend(bytes); }
                Ok(PyObject::bytes(result))
            }
            (PyObjectPayload::ByteArray(b), PyObjectPayload::Int(n)) | (PyObjectPayload::Int(n), PyObjectPayload::ByteArray(b)) => {
                let count = n.to_i64().unwrap_or(0).max(0) as usize;
                let mut result = Vec::with_capacity(b.len() * count);
                for _ in 0..count { result.extend(b.iter()); }
                Ok(PyObject::bytearray(result))
            }
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for *: '{}' and '{}'", a.type_name(), b.type_name()))),
        }
}

pub(super) fn py_floor_div(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
        let ua = unwrap_builtin_subclass(a);
        let ub = unwrap_builtin_subclass(b);
        if !PyObjectRef::ptr_eq(&ua, a) || !PyObjectRef::ptr_eq(&ub, b) {
            return py_floor_div(&ua, &ub);
        }
        match (&a.payload, &b.payload) {
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => {
                if b.is_zero() { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                Ok(PyInt::floor_div_op(a, b).to_object())
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => {
                if *b == 0.0 { return Err(PyException::zero_division_error("float floor division by zero")); }
                Ok(PyObject::float((a / b).floor()))
            }
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for //: '{}' and '{}'", a.type_name(), b.type_name()))),
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
            (PyObjectPayload::Complex { real: ar, imag: ai }, PyObjectPayload::Complex { real: br, imag: bi }) => {
                let denom = br * br + bi * bi;
                if denom == 0.0 { return Err(PyException::zero_division_error("complex division by zero")); }
                return Ok(PyObject::complex((ar * br + ai * bi) / denom, (ai * br - ar * bi) / denom));
            }
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Int(b)) => {
                let bf = b.to_f64();
                if bf == 0.0 { return Err(PyException::zero_division_error("complex division by zero")); }
                return Ok(PyObject::complex(real / bf, imag / bf));
            }
            (PyObjectPayload::Complex { real, imag }, PyObjectPayload::Float(b)) => {
                if *b == 0.0 { return Err(PyException::zero_division_error("complex division by zero")); }
                return Ok(PyObject::complex(real / b, imag / b));
            }
            (PyObjectPayload::Int(a), PyObjectPayload::Complex { real: br, imag: bi }) => {
                let af = a.to_f64();
                let denom = br * br + bi * bi;
                if denom == 0.0 { return Err(PyException::zero_division_error("complex division by zero")); }
                return Ok(PyObject::complex((af * br) / denom, (-af * bi) / denom));
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Complex { real: br, imag: bi }) => {
                let denom = br * br + bi * bi;
                if denom == 0.0 { return Err(PyException::zero_division_error("complex division by zero")); }
                return Ok(PyObject::complex((a * br) / denom, (-a * bi) / denom));
            }
            _ => {}
        }
        // Path / str → joined path (pathlib)
        if let PyObjectPayload::Instance(inst) = &a.payload {
            if inst.attrs.read().contains_key("__pathlib_path__") {
                let base = inst.attrs.read().get("_path").map(|v| v.py_to_string()).unwrap_or_default();
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
                    w.insert(CompactString::from("_path"), PyObject::str_val(CompactString::from(&joined_str)));
                    w.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(
                        path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
                    )));
                    w.insert(CompactString::from("stem"), PyObject::str_val(CompactString::from(
                        path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
                    )));
                    w.insert(CompactString::from("suffix"), PyObject::str_val(CompactString::from(
                        path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default()
                    )));
                    w.insert(CompactString::from("parent"), PyObject::str_val(CompactString::from(
                        path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
                    )));
                    let parts: Vec<PyObjectRef> = {
                        let mut p = Vec::new();
                        if joined_str.starts_with('/') { p.push(PyObject::str_val(CompactString::from("/"))); }
                        for c in path.components() {
                            let s = c.as_os_str().to_string_lossy().to_string();
                            if s != "/" { p.push(PyObject::str_val(CompactString::from(&s))); }
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
        if b == 0.0 { return Err(PyException::zero_division_error("division by zero")); }
        Ok(PyObject::float(a / b))
}

pub(super) fn py_modulo(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
        let ua = unwrap_builtin_subclass(a);
        let ub = unwrap_builtin_subclass(b);
        if !PyObjectRef::ptr_eq(&ua, a) || !PyObjectRef::ptr_eq(&ub, b) {
            return py_modulo(&ua, &ub);
        }
        match (&a.payload, &b.payload) {
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => {
                if b.is_zero() { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                Ok(PyInt::modulo_op(a, b).to_object())
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => {
                if *b == 0.0 { return Err(PyException::zero_division_error("float modulo")); }
                Ok(PyObject::float(python_fmod(*a, *b)))
            }
            (PyObjectPayload::Str(fmt_str), _) => {
                // printf-style string formatting: "Hello %s" % "world"
                // Also supports dict-keyed format: "%(name)s" % {"name": "Bob"}
                let args_list = match &b.payload {
                    PyObjectPayload::Tuple(items) => items.clone(),
                    _ => vec![b.clone()],
                };
                let mut result = String::new();
                let mut arg_idx = 0;
                let chars: Vec<char> = fmt_str.chars().collect();
                let mut i = 0;
                while i < chars.len() {
                    if chars[i] == '%' && i + 1 < chars.len() {
                        i += 1;
                        // Check for %(name) dict-keyed format
                        let dict_key = if i < chars.len() && chars[i] == '(' {
                            i += 1; // skip '('
                            let start = i;
                            while i < chars.len() && chars[i] != ')' {
                                i += 1;
                            }
                            let key: String = chars[start..i].iter().collect();
                            if i < chars.len() { i += 1; } // skip ')'
                            Some(key)
                        } else {
                            None
                        };
                        // Parse optional flags, width, precision
                        let mut spec_chars = String::new();
                        while i < chars.len() && "-+ #0123456789.".contains(chars[i]) {
                            spec_chars.push(chars[i]);
                            i += 1;
                        }
                        if i >= chars.len() { break; }
                        let conv = chars[i];
                        i += 1;
                        if conv == '%' {
                            result.push('%');
                            continue;
                        }
                        // Resolve the argument: dict-keyed or positional
                        let arg = if let Some(ref key) = dict_key {
                            let key_obj = PyObject::str_val(CompactString::from(key.as_str()));
                            b.get_item(&key_obj)?
                        } else {
                            if arg_idx >= args_list.len() {
                                return Err(PyException::type_error("not enough arguments for format string"));
                            }
                            let a = args_list[arg_idx].clone();
                            arg_idx += 1;
                            a
                        };
                        match conv {
                            's' => {
                                let s = arg.py_to_string();
                                if spec_chars.is_empty() {
                                    result.push_str(&s);
                                } else {
                                    result.push_str(&format_str_spec(&s, &spec_chars));
                                }
                            }
                            'r' => {
                                let s = arg.repr();
                                if spec_chars.is_empty() {
                                    result.push_str(&s);
                                } else {
                                    result.push_str(&format_str_spec(&s, &spec_chars));
                                }
                            }
                            'd' | 'i' => {
                                let n = arg.to_int()?;
                                if spec_chars.is_empty() {
                                    result.push_str(&n.to_string());
                                } else {
                                    result.push_str(&format_int_spec(n, &spec_chars));
                                }
                            }
                            'f' | 'F' => {
                                let f = arg.to_float()?;
                                if spec_chars.is_empty() {
                                    result.push_str(&format!("{:.6}", f));
                                } else {
                                    result.push_str(&format_float_spec(f, &spec_chars));
                                }
                            }
                            'x' => result.push_str(&format!("{:x}", arg.to_int()?)),
                            'X' => result.push_str(&format!("{:X}", arg.to_int()?)),
                            'o' => result.push_str(&format!("{:o}", arg.to_int()?)),
                            'e' | 'E' => {
                                let f = arg.to_float()?;
                                let prec = parse_precision(&spec_chars).unwrap_or(6);
                                let raw = if conv == 'e' {
                                    format!("{:.prec$e}", f, prec = prec)
                                } else {
                                    format!("{:.prec$E}", f, prec = prec)
                                };
                                result.push_str(&normalize_scientific_exponent(&raw, conv));
                            }
                            'g' | 'G' => {
                                let f = arg.to_float()?;
                                let prec = parse_precision(&spec_chars).unwrap_or(6);
                                let abs_f = f.abs();
                                let use_sci = abs_f != 0.0 && (abs_f >= 10f64.powi(prec as i32) || abs_f < 1e-4);
                                if use_sci {
                                    let sci_prec = if prec > 0 { prec - 1 } else { 0 };
                                    let e_char = if conv == 'g' { 'e' } else { 'E' };
                                    let raw = if e_char == 'e' {
                                        format!("{:.prec$e}", f, prec = sci_prec)
                                    } else {
                                        format!("{:.prec$E}", f, prec = sci_prec)
                                    };
                                    result.push_str(&normalize_scientific_exponent(&raw, e_char));
                                } else {
                                    // Remove trailing zeros for %g
                                    let s = format!("{:.prec$}", f, prec = prec);
                                    let s = if s.contains('.') {
                                        s.trim_end_matches('0').trim_end_matches('.').to_string()
                                    } else { s };
                                    result.push_str(&s);
                                }
                            }
                            _ => {
                                result.push('%');
                                result.push_str(&spec_chars);
                                result.push(conv);
                            }
                        }
                    } else {
                        result.push(chars[i]);
                        i += 1;
                    }
                }
                Ok(PyObject::str_val(CompactString::from(result)))
            }
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for %: '{}' and '{}'", a.type_name(), b.type_name()))),
        }
}

pub(super) fn py_power(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
        let ua = unwrap_builtin_subclass(a);
        let ub = unwrap_builtin_subclass(b);
        if !PyObjectRef::ptr_eq(&ua, a) || !PyObjectRef::ptr_eq(&ub, b) {
            return py_power(&ua, &ub);
        }
        match (&a.payload, &b.payload) {
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => {
                if let Some(exp) = b.to_i64() {
                    if exp >= 0 {
                        let e = exp as u32;
                        return Ok(PyInt::pow_op(a, e).to_object());
                    }
                }
                // Negative exponent → float result
                Ok(PyObject::float(a.to_f64().powf(b.to_f64())))
            }
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.powf(*b))),
            (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => Ok(PyObject::float(a.to_f64().powf(*b))),
            (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => Ok(PyObject::float(a.powf(b.to_f64()))),
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for **: '{}' and '{}'", a.type_name(), b.type_name()))),
        }
}

pub(super) fn py_lshift(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    int_bitop(a, b, "<<", |a, b| a << b)
}

pub(super) fn py_rshift(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    int_bitop(a, b, ">>", |a, b| a >> b)
}

pub(super) fn py_bit_and(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
        match (&a.payload, &b.payload) {
            (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
                let ra = a.read(); let rb = b.read();
                let mut result = new_fx_hashkey_map();
                for (k, v) in ra.iter() { if rb.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(result)))))
            }
            (PyObjectPayload::FrozenSet(a), PyObjectPayload::FrozenSet(b)) => {
                let mut result = new_fx_hashkey_map();
                for (k, v) in a.iter() { if b.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::FrozenSet(Box::new(result))))
            }
            (PyObjectPayload::FrozenSet(a), PyObjectPayload::Set(b)) => {
                let rb = b.read();
                let mut result = new_fx_hashkey_map();
                for (k, v) in a.iter() { if rb.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::FrozenSet(Box::new(result))))
            }
            (PyObjectPayload::Set(a), PyObjectPayload::FrozenSet(b)) => {
                let ra = a.read();
                let mut result = new_fx_hashkey_map();
                for (k, v) in ra.iter() { if b.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(result)))))
            }
            // Counter & Counter: minimum of counts (intersection)
            (PyObjectPayload::Dict(a_map), PyObjectPayload::Dict(b_map)) => {
                let ra = a_map.read(); let rb = b_map.read();
                let counter_key = HashableKey::Str(intern_or_new("__counter__"));
                let a_counter = ra.contains_key(&counter_key);
                let b_counter = rb.contains_key(&counter_key);
                if a_counter && b_counter {
                    let mut result = new_fx_hashkey_map();
                    result.insert(HashableKey::Str(intern_or_new("__defaultdict_factory__")),
                        PyObject::builtin_type(CompactString::from("int")));
                    result.insert(counter_key, PyObject::bool_val(true));
                    for (k, v) in ra.iter() {
                        if let HashableKey::Str(s) = k {
                            if s.starts_with("__") && s.ends_with("__") { continue; }
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
                        "unsupported operand type(s) for &: '{}' and '{}'", a.type_name(), b.type_name()
                    )))
                }
            }
            // DictKeys/DictItems set-like intersection
            (PyObjectPayload::DictKeys(_) | PyObjectPayload::DictItems(_), _)
            | (_, PyObjectPayload::DictKeys(_) | PyObjectPayload::DictItems(_))
                if extract_view_keys(a).is_some() && extract_view_keys(b).is_some() => {
                if let (Some(ak), Some(bk)) = (extract_view_keys(a), extract_view_keys(b)) {
                    let mut result = new_fx_hashkey_map();
                    for (k, v) in ak.iter() { if bk.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                    Ok(keys_to_set(result))
                } else {
                    Err(PyException::type_error("dict view changed during operation"))
                }
            }
            _ => int_bitop(a, b, "&", |a, b| a & b),
        }
}

pub(super) fn py_bit_or(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
        match (&a.payload, &b.payload) {
            (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
                let ra = a.read(); let rb = b.read();
                let mut result = ra.clone();
                for (k, v) in rb.iter() { result.insert(k.clone(), v.clone()); }
                Ok(PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(result)))))
            }
            (PyObjectPayload::FrozenSet(a), PyObjectPayload::FrozenSet(b)) => {
                let mut result = a.clone();
                for (k, v) in b.iter() { result.insert(k.clone(), v.clone()); }
                Ok(PyObject::wrap(PyObjectPayload::FrozenSet(result)))
            }
            (PyObjectPayload::FrozenSet(a), PyObjectPayload::Set(b)) => {
                let mut result = a.clone();
                let rb = b.read();
                for (k, v) in rb.iter() { result.insert(k.clone(), v.clone()); }
                Ok(PyObject::wrap(PyObjectPayload::FrozenSet(result)))
            }
            (PyObjectPayload::Set(a), PyObjectPayload::FrozenSet(b)) => {
                let ra = a.read();
                let mut result = ra.clone();
                for (k, v) in b.iter() { result.insert(k.clone(), v.clone()); }
                Ok(PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(result)))))
            }
            // PEP 584: dict | dict (also Counter | Counter with max semantics)
            (PyObjectPayload::Dict(a_map), PyObjectPayload::Dict(b_map)) => {
                let ra = a_map.read(); let rb = b_map.read();
                let counter_key = HashableKey::Str(intern_or_new("__counter__"));
                let a_counter = ra.contains_key(&counter_key);
                let b_counter = rb.contains_key(&counter_key);
                if a_counter && b_counter {
                    // Counter | Counter: maximum of counts (union)
                    let mut result = new_fx_hashkey_map();
                    result.insert(HashableKey::Str(intern_or_new("__defaultdict_factory__")),
                        PyObject::builtin_type(CompactString::from("int")));
                    result.insert(counter_key, PyObject::bool_val(true));
                    let mut all_keys: IndexMap<HashableKey, i64> = IndexMap::new();
                    for (k, v) in ra.iter() {
                        if let HashableKey::Str(s) = k {
                            if s.starts_with("__") && s.ends_with("__") { continue; }
                        }
                        all_keys.insert(k.clone(), v.as_int().unwrap_or(0));
                    }
                    for (k, v) in rb.iter() {
                        if let HashableKey::Str(s) = k {
                            if s.starts_with("__") && s.ends_with("__") { continue; }
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
                    for (k, v) in rb.iter() { result.insert(k.clone(), v.clone()); }
                    Ok(PyObject::dict(result))
                }
            }
            // DictKeys/DictItems set-like union
            (PyObjectPayload::DictKeys(_) | PyObjectPayload::DictItems(_), _)
            | (_, PyObjectPayload::DictKeys(_) | PyObjectPayload::DictItems(_))
                if extract_view_keys(a).is_some() && extract_view_keys(b).is_some() => {
                if let (Some(ak), Some(bk)) = (extract_view_keys(a), extract_view_keys(b)) {
                    let mut result = ak;
                    for (k, v) in bk.iter() { result.entry(k.clone()).or_insert_with(|| v.clone()); }
                    Ok(keys_to_set(result))
                } else {
                    Err(PyException::type_error("dict view changed during operation"))
                }
            }
            _ => int_bitop(a, b, "|", |a, b| a | b),
        }
}

pub(super) fn py_bit_xor(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
        match (&a.payload, &b.payload) {
            (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
                let ra = a.read(); let rb = b.read();
                let mut result = new_fx_hashkey_map();
                for (k, v) in ra.iter() { if !rb.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                for (k, v) in rb.iter() { if !ra.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(result)))))
            }
            (PyObjectPayload::FrozenSet(a), PyObjectPayload::FrozenSet(b)) => {
                let mut result = new_fx_hashkey_map();
                for (k, v) in a.iter() { if !b.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                for (k, v) in b.iter() { if !a.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::FrozenSet(Box::new(result))))
            }
            (PyObjectPayload::FrozenSet(a), PyObjectPayload::Set(b)) => {
                let rb = b.read();
                let mut result = new_fx_hashkey_map();
                for (k, v) in a.iter() { if !rb.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                for (k, v) in rb.iter() { if !a.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::FrozenSet(Box::new(result))))
            }
            (PyObjectPayload::Set(a), PyObjectPayload::FrozenSet(b)) => {
                let ra = a.read();
                let mut result = new_fx_hashkey_map();
                for (k, v) in ra.iter() { if !b.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                for (k, v) in b.iter() { if !ra.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(result)))))
            }
            // DictKeys/DictItems set-like symmetric difference
            (PyObjectPayload::DictKeys(_) | PyObjectPayload::DictItems(_), _)
            | (_, PyObjectPayload::DictKeys(_) | PyObjectPayload::DictItems(_))
                if extract_view_keys(a).is_some() && extract_view_keys(b).is_some() => {
                if let (Some(ak), Some(bk)) = (extract_view_keys(a), extract_view_keys(b)) {
                    let mut result = new_fx_hashkey_map();
                    for (k, v) in ak.iter() { if !bk.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                    for (k, v) in bk.iter() { if !ak.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                    Ok(keys_to_set(result))
                } else {
                    Err(PyException::type_error("dict view changed during operation"))
                }
            }
            _ => int_bitop(a, b, "^", |a, b| a ^ b),
        }
}

pub(super) fn py_negate(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
        let u = unwrap_builtin_subclass(obj);
        if !PyObjectRef::ptr_eq(&u, obj) { return py_negate(&u); }
        match &obj.payload {
            PyObjectPayload::Int(n) => Ok(n.negate().to_object()),
            PyObjectPayload::Float(f) => Ok(PyObject::float(-f)),
            PyObjectPayload::Bool(b) => Ok(PyObject::int(-(*b as i64))),
            PyObjectPayload::Complex { real, imag } => Ok(PyObject::complex(-real, -imag)),
            _ => Err(PyException::type_error(format!("bad operand type for unary -: '{}'", obj.type_name()))),
        }
}

pub(super) fn py_positive(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
        let u = unwrap_builtin_subclass(obj);
        if !PyObjectRef::ptr_eq(&u, obj) { return py_positive(&u); }
        match &obj.payload {
            PyObjectPayload::Int(_) | PyObjectPayload::Float(_) | PyObjectPayload::Bool(_) |
            PyObjectPayload::Complex { .. } => Ok(obj.clone()),
            _ => Err(PyException::type_error(format!("bad operand type for unary +: '{}'", obj.type_name()))),
        }
}

pub(super) fn py_invert(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
        let u = unwrap_builtin_subclass(obj);
        if !PyObjectRef::ptr_eq(&u, obj) { return py_invert(&u); }
        match &obj.payload {
            PyObjectPayload::Int(n) => Ok(n.invert().to_object()),
            PyObjectPayload::Bool(b) => Ok(PyObject::int(!(*b as i64))),
            _ => Err(PyException::type_error(format!("bad operand type for unary ~: '{}'", obj.type_name()))),
        }
}

pub(super) fn py_abs(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
        let u = unwrap_builtin_subclass(obj);
        if !PyObjectRef::ptr_eq(&u, obj) { return py_abs(&u); }
        match &obj.payload {
            PyObjectPayload::Int(n) => Ok(n.abs().to_object()),
            PyObjectPayload::Float(f) => Ok(PyObject::float(f.abs())),
            PyObjectPayload::Bool(b) => Ok(PyObject::int(*b as i64)),
            PyObjectPayload::Complex { real, imag } => Ok(PyObject::float((real * real + imag * imag).sqrt())),
            _ => Err(PyException::type_error(format!("bad operand type for abs(): '{}'", obj.type_name()))),
        }
}
