//! Arithmetic operation methods.

use crate::error::{PyException, PyResult};
use crate::types::PyInt;
use compact_str::CompactString;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

use super::payload::*;
use super::helpers::*;
use super::methods::PyObjectMethods;

pub(super) fn py_add(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
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
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for +: '{}' and '{}'", a.type_name(), b.type_name()))),
        }
}

pub(super) fn py_sub(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
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
                let mut result = IndexMap::new();
                for (k, v) in ra.iter() { if !rb.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(result)))))
            }
            (PyObjectPayload::FrozenSet(a), PyObjectPayload::FrozenSet(b)) => {
                let mut result = IndexMap::new();
                for (k, v) in a.iter() { if !b.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::FrozenSet(result)))
            }
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for -: '{}' and '{}'", a.type_name(), b.type_name()))),
        }
}

pub(super) fn py_mul(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
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
            (PyObjectPayload::List(items), PyObjectPayload::Int(n)) | (PyObjectPayload::Int(n), PyObjectPayload::List(items)) => {
                let count = n.to_i64().unwrap_or(0).max(0) as usize;
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
            (PyObjectPayload::Bytes(b), PyObjectPayload::Int(n)) | (PyObjectPayload::Int(n), PyObjectPayload::Bytes(b)) => {
                let count = n.to_i64().unwrap_or(0).max(0) as usize;
                let mut result = Vec::with_capacity(b.len() * count);
                for _ in 0..count { result.extend(b); }
                Ok(PyObject::bytes(result))
            }
            _ => Err(PyException::type_error(format!("unsupported operand type(s) for *: '{}' and '{}'", a.type_name(), b.type_name()))),
        }
}

pub(super) fn py_floor_div(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
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
        let a = coerce_to_f64(a)?;
        let b = coerce_to_f64(b)?;
        if b == 0.0 { return Err(PyException::zero_division_error("division by zero")); }
        Ok(PyObject::float(a / b))
}

pub(super) fn py_modulo(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
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
                        if arg_idx >= args_list.len() {
                            return Err(PyException::type_error("not enough arguments for format string"));
                        }
                        let arg = &args_list[arg_idx];
                        arg_idx += 1;
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
        match (&a.payload, &b.payload) {
            (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => {
                if let Some(exp) = b.to_i64() {
                    if exp >= 0 && exp <= 63 { return Ok(PyInt::pow_op(a, exp as u32).to_object()); }
                }
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
                let mut result = IndexMap::new();
                for (k, v) in ra.iter() { if rb.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(result)))))
            }
            (PyObjectPayload::FrozenSet(a), PyObjectPayload::FrozenSet(b)) => {
                let mut result = IndexMap::new();
                for (k, v) in a.iter() { if b.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::FrozenSet(result)))
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
                Ok(PyObject::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(result)))))
            }
            (PyObjectPayload::FrozenSet(a), PyObjectPayload::FrozenSet(b)) => {
                let mut result = a.clone();
                for (k, v) in b.iter() { result.insert(k.clone(), v.clone()); }
                Ok(PyObject::wrap(PyObjectPayload::FrozenSet(result)))
            }
            // PEP 584: dict | dict
            (PyObjectPayload::Dict(a), PyObjectPayload::Dict(b)) => {
                let ra = a.read(); let rb = b.read();
                let mut result = ra.clone();
                for (k, v) in rb.iter() { result.insert(k.clone(), v.clone()); }
                Ok(PyObject::dict(result))
            }
            _ => int_bitop(a, b, "|", |a, b| a | b),
        }
}

pub(super) fn py_bit_xor(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
        match (&a.payload, &b.payload) {
            (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
                let ra = a.read(); let rb = b.read();
                let mut result = IndexMap::new();
                for (k, v) in ra.iter() { if !rb.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                for (k, v) in rb.iter() { if !ra.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(result)))))
            }
            (PyObjectPayload::FrozenSet(a), PyObjectPayload::FrozenSet(b)) => {
                let mut result = IndexMap::new();
                for (k, v) in a.iter() { if !b.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                for (k, v) in b.iter() { if !a.contains_key(k) { result.insert(k.clone(), v.clone()); } }
                Ok(PyObject::wrap(PyObjectPayload::FrozenSet(result)))
            }
            _ => int_bitop(a, b, "^", |a, b| a ^ b),
        }
}

pub(super) fn py_negate(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
        match &obj.payload {
            PyObjectPayload::Int(n) => Ok(n.negate().to_object()),
            PyObjectPayload::Float(f) => Ok(PyObject::float(-f)),
            PyObjectPayload::Bool(b) => Ok(PyObject::int(-(*b as i64))),
            PyObjectPayload::Complex { real, imag } => Ok(PyObject::complex(-real, -imag)),
            _ => Err(PyException::type_error(format!("bad operand type for unary -: '{}'", obj.type_name()))),
        }
}

pub(super) fn py_positive(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
        match &obj.payload {
            PyObjectPayload::Int(_) | PyObjectPayload::Float(_) | PyObjectPayload::Bool(_) |
            PyObjectPayload::Complex { .. } => Ok(obj.clone()),
            _ => Err(PyException::type_error(format!("bad operand type for unary +: '{}'", obj.type_name()))),
        }
}

pub(super) fn py_invert(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
        match &obj.payload {
            PyObjectPayload::Int(n) => Ok(n.invert().to_object()),
            PyObjectPayload::Bool(b) => Ok(PyObject::int(!(*b as i64))),
            _ => Err(PyException::type_error(format!("bad operand type for unary ~: '{}'", obj.type_name()))),
        }
}

pub(super) fn py_abs(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
        match &obj.payload {
            PyObjectPayload::Int(n) => Ok(n.abs().to_object()),
            PyObjectPayload::Float(f) => Ok(PyObject::float(f.abs())),
            PyObjectPayload::Bool(b) => Ok(PyObject::int(*b as i64)),
            PyObjectPayload::Complex { real, imag } => Ok(PyObject::float((real * real + imag * imag).sqrt())),
            _ => Err(PyException::type_error(format!("bad operand type for abs(): '{}'", obj.type_name()))),
        }
}
