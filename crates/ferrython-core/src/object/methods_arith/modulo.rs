use crate::error::{PyException, PyResult};
use crate::types::PyInt;
use compact_str::CompactString;
use num_bigint::BigInt;
use num_traits::Signed;

use super::super::helpers::*;
use super::super::methods::PyObjectMethods;
use super::super::payload::*;
use super::bool_as_int;

fn format_percent_radix(arg: &PyObjectRef, conv: char, spec: &str) -> PyResult<String> {
    let n = match &arg.payload {
        PyObjectPayload::Bool(v) => BigInt::from(if *v { 1 } else { 0 }),
        PyObjectPayload::Int(PyInt::Small(v)) => BigInt::from(*v),
        PyObjectPayload::Int(PyInt::Big(v)) => v.as_ref().clone(),
        _ => {
            return Err(PyException::type_error(format!(
                "%{} format: an integer is required, not {}",
                conv,
                arg.type_name()
            )))
        }
    };
    let negative = n < BigInt::from(0);
    let magnitude = n.abs();
    let radix = match conv {
        'o' => 8,
        'x' | 'X' => 16,
        _ => unreachable!(),
    };
    let mut digits = magnitude.to_str_radix(radix);
    if conv == 'X' {
        digits.make_ascii_uppercase();
    }
    let prefix = if spec.contains('#') {
        match conv {
            'o' => "0o",
            'x' => "0x",
            'X' => "0X",
            _ => "",
        }
    } else {
        ""
    };
    if negative {
        Ok(format!("-{}{}", prefix, digits))
    } else {
        Ok(format!("{}{}", prefix, digits))
    }
}

pub(crate) fn py_modulo(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    let ua = unwrap_builtin_subclass(a);
    let ub = unwrap_builtin_subclass(b);
    if !PyObjectRef::ptr_eq(&ua, a) || !PyObjectRef::ptr_eq(&ub, b) {
        return py_modulo(&ua, &ub);
    }
    // Bool → Int coercion (except str % b, which is string formatting)
    if !matches!(
        &a.payload,
        PyObjectPayload::Str(_) | PyObjectPayload::Bytes(_)
    ) {
        if let Some(ai) = bool_as_int(a) {
            return py_modulo(&ai, b);
        }
        if let Some(bi) = bool_as_int(b) {
            return py_modulo(a, &bi);
        }
    }
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => {
            if b.is_zero() {
                return Err(PyException::zero_division_error(
                    "integer division or modulo by zero",
                ));
            }
            Ok(PyInt::modulo_op(a, b).to_object())
        }
        (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => {
            if *b == 0.0 {
                return Err(PyException::zero_division_error("float modulo"));
            }
            Ok(PyObject::float(python_fmod(*a, *b)))
        }
        (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => {
            if *b == 0.0 {
                return Err(PyException::zero_division_error("float modulo"));
            }
            Ok(PyObject::float(python_fmod(a.to_f64(), *b)))
        }
        (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => {
            if b.is_zero() {
                return Err(PyException::zero_division_error("float modulo"));
            }
            Ok(PyObject::float(python_fmod(*a, b.to_f64())))
        }
        (PyObjectPayload::Str(fmt_str), _) => {
            // printf-style string formatting: "Hello %s" % "world"
            // Also supports dict-keyed format: "%(name)s" % {"name": "Bob"}
            let args_list = match &b.payload {
                PyObjectPayload::Tuple(items) => (**items).clone(),
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
                        if i < chars.len() {
                            i += 1;
                        } // skip ')'
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
                    if i >= chars.len() {
                        break;
                    }
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
                            return Err(PyException::type_error(
                                "not enough arguments for format string",
                            ));
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
                            let formatted = match &arg.payload {
                                PyObjectPayload::Int(n) => {
                                    if spec_chars.is_empty() {
                                        n.to_string()
                                    } else if let Some(value) = n.to_i64() {
                                        format_int_spec(value, &spec_chars)
                                    } else {
                                        format_str_spec(&n.to_string(), &spec_chars)
                                    }
                                }
                                PyObjectPayload::Bool(b) => {
                                    let value = i64::from(*b);
                                    if spec_chars.is_empty() {
                                        value.to_string()
                                    } else {
                                        format_int_spec(value, &spec_chars)
                                    }
                                }
                                _ => {
                                    return Err(PyException::type_error(format!(
                                        "%{} format: a number is required, not {}",
                                        conv,
                                        arg.type_name()
                                    )));
                                }
                            };
                            result.push_str(&formatted);
                        }
                        'f' | 'F' => {
                            let f = arg.to_float()?;
                            if spec_chars.is_empty() {
                                result.push_str(&format!("{:.6}", f));
                            } else {
                                result.push_str(&format_float_spec(f, &spec_chars));
                            }
                        }
                        'x' | 'X' | 'o' => {
                            result.push_str(&format_percent_radix(&arg, conv, &spec_chars)?)
                        }
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
                            let use_sci =
                                abs_f != 0.0 && (abs_f >= 10f64.powi(prec as i32) || abs_f < 1e-4);
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
                                } else {
                                    s
                                };
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
        _ => Err(PyException::type_error(format!(
            "unsupported operand type(s) for %: '{}' and '{}'",
            a.type_name(),
            b.type_name()
        ))),
    }
}
