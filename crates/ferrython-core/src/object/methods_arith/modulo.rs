use crate::error::{PyException, PyResult};
use crate::types::PyInt;
use compact_str::CompactString;
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive};

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

fn percent_int_string(arg: &PyObjectRef, conv: char) -> PyResult<String> {
    match &arg.payload {
        PyObjectPayload::Bool(v) => Ok(if *v { "1" } else { "0" }.to_string()),
        PyObjectPayload::Int(n) => Ok(n.to_string()),
        PyObjectPayload::Float(f) => Ok((*f as i64).to_string()),
        _ => Err(PyException::type_error(format!(
            "%{} format: a number is required, not {}",
            conv,
            arg.type_name()
        ))),
    }
}

fn apply_numeric_sign(mut formatted: String, flags: &str) -> String {
    if formatted.starts_with('-') {
        formatted
    } else if flags.contains('+') {
        formatted.insert(0, '+');
        formatted
    } else if flags.contains(' ') {
        formatted.insert(0, ' ');
        formatted
    } else {
        formatted
    }
}

fn apply_percent_width(formatted: String, width: usize, flags: &str) -> String {
    let len = formatted.chars().count();
    if width <= len {
        return formatted;
    }
    let pad_len = width - len;
    if flags.contains('-') {
        format!("{}{}", formatted, " ".repeat(pad_len))
    } else if flags.contains('0') {
        if let Some(first) = formatted.chars().next() {
            if first == '-' || first == '+' || first == ' ' {
                let rest = &formatted[first.len_utf8()..];
                return format!("{}{}{}", first, "0".repeat(pad_len), rest);
            }
        }
        format!("{}{}", "0".repeat(pad_len), formatted)
    } else {
        format!("{}{}", " ".repeat(pad_len), formatted)
    }
}

fn apply_str_precision(s: String, precision: Option<usize>) -> String {
    if let Some(precision) = precision {
        s.chars().take(precision).collect()
    } else {
        s
    }
}

fn percent_char_string(arg: &PyObjectRef) -> PyResult<String> {
    match &arg.payload {
        PyObjectPayload::Bool(v) => Ok(char::from_u32(if *v { 1 } else { 0 })
            .unwrap_or('\0')
            .to_string()),
        PyObjectPayload::Int(n) => {
            let value = n.to_bigint();
            if value.is_negative() || value > BigInt::from(0x10ffff_u32) {
                return Err(PyException::overflow_error("%c arg not in range(0x110000)"));
            }
            let ordinal = value
                .to_u32()
                .ok_or_else(|| PyException::overflow_error("%c arg not in range(0x110000)"))?;
            char::from_u32(ordinal)
                .map(|c| c.to_string())
                .ok_or_else(|| PyException::overflow_error("%c arg not in range(0x110000)"))
        }
        PyObjectPayload::Str(s) => {
            let mut chars = s.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Ok(c.to_string()),
                _ => Err(PyException::type_error("%c requires int or char")),
            }
        }
        _ => Err(PyException::type_error(format!(
            "%c requires int or char, not {}",
            arg.type_name()
        ))),
    }
}

fn parse_percent_number(chars: &[char], i: &mut usize) -> PyResult<usize> {
    let mut value = 0usize;
    while *i < chars.len() && chars[*i].is_ascii_digit() {
        let digit = chars[*i] as usize - '0' as usize;
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(digit))
            .ok_or_else(|| PyException::value_error("format width or precision too large"))?;
        *i += 1;
    }
    Ok(value)
}

fn next_percent_arg(args: &[PyObjectRef], arg_idx: &mut usize) -> PyResult<PyObjectRef> {
    if *arg_idx >= args.len() {
        return Err(PyException::type_error(
            "not enough arguments for format string",
        ));
    }
    let arg = args[*arg_idx].clone();
    *arg_idx += 1;
    Ok(arg)
}

fn percent_star_value(arg: &PyObjectRef) -> PyResult<i64> {
    arg.to_index()?
        .to_i64()
        .ok_or_else(|| PyException::overflow_error("format width or precision too large"))
}

fn parse_mapping_key(chars: &[char], i: &mut usize) -> PyResult<Option<String>> {
    if *i >= chars.len() || chars[*i] != '(' {
        return Ok(None);
    }
    *i += 1;
    let start = *i;
    let mut depth = 1usize;
    while *i < chars.len() {
        match chars[*i] {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    let key: String = chars[start..*i].iter().collect();
                    *i += 1;
                    return Ok(Some(key));
                }
            }
            _ => {}
        }
        *i += 1;
    }
    Err(PyException::value_error("incomplete format key"))
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
            let using_tuple_args = matches!(&b.payload, PyObjectPayload::Tuple(_));
            let mut used_mapping_key = false;
            let mut result = String::new();
            let mut arg_idx = 0;
            let chars: Vec<char> = fmt_str.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                if chars[i] == '%' && i + 1 < chars.len() {
                    i += 1;
                    // Check for %(name) dict-keyed format
                    let dict_key = {
                        let parsed = parse_mapping_key(&chars, &mut i)?;
                        if parsed.is_some() {
                            used_mapping_key = true;
                        }
                        parsed
                    };
                    // Parse optional flags, width, precision
                    let mut flags = String::new();
                    while i < chars.len() && "-+ #0".contains(chars[i]) {
                        if !flags.contains(chars[i]) {
                            flags.push(chars[i]);
                        }
                        i += 1;
                    }
                    let width;
                    if i < chars.len() && chars[i] == '*' {
                        i += 1;
                        let value =
                            percent_star_value(&next_percent_arg(&args_list, &mut arg_idx)?)?;
                        if value < 0 {
                            if !flags.contains('-') {
                                flags.push('-');
                            }
                            width = value.saturating_abs() as usize;
                        } else {
                            width = value as usize;
                        }
                    } else {
                        width = parse_percent_number(&chars, &mut i)?;
                    }
                    let mut precision = None;
                    if i < chars.len() && chars[i] == '.' {
                        i += 1;
                        if i < chars.len() && chars[i] == '*' {
                            i += 1;
                            let value =
                                percent_star_value(&next_percent_arg(&args_list, &mut arg_idx)?)?;
                            if value >= 0 {
                                precision = Some(value as usize);
                            }
                        } else {
                            precision = Some(parse_percent_number(&chars, &mut i)?);
                        }
                    }
                    while i < chars.len() && matches!(chars[i], 'h' | 'l' | 'L') {
                        i += 1;
                    }
                    if i >= chars.len() {
                        return Err(PyException::value_error("incomplete format"));
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
                        next_percent_arg(&args_list, &mut arg_idx)?
                    };
                    let formatted = match conv {
                        's' => {
                            let s = apply_str_precision(arg.py_to_string(), precision);
                            apply_percent_width(s, width, &flags)
                        }
                        'r' => {
                            let s = apply_str_precision(arg.repr(), precision);
                            apply_percent_width(s, width, &flags)
                        }
                        'd' | 'i' => {
                            let value = apply_numeric_sign(percent_int_string(&arg, conv)?, &flags);
                            apply_percent_width(value, width, &flags)
                        }
                        'f' | 'F' => {
                            let f = arg.to_float()?;
                            let prec = precision.unwrap_or(6);
                            let value =
                                apply_numeric_sign(format!("{:.prec$}", f, prec = prec), &flags);
                            apply_percent_width(value, width, &flags)
                        }
                        'x' | 'X' | 'o' => {
                            let value = format_percent_radix(&arg, conv, &flags)?;
                            apply_percent_width(value, width, &flags)
                        }
                        'e' | 'E' => {
                            let f = arg.to_float()?;
                            let prec = precision.unwrap_or(6);
                            let raw = if conv == 'e' {
                                format!("{:.prec$e}", f, prec = prec)
                            } else {
                                format!("{:.prec$E}", f, prec = prec)
                            };
                            let value = apply_numeric_sign(
                                normalize_scientific_exponent(&raw, conv),
                                &flags,
                            );
                            apply_percent_width(value, width, &flags)
                        }
                        'g' | 'G' => {
                            let f = arg.to_float()?;
                            let prec = precision.unwrap_or(6);
                            let abs_f = f.abs();
                            let use_sci =
                                abs_f != 0.0 && (abs_f >= 10f64.powi(prec as i32) || abs_f < 1e-4);
                            let value = if use_sci {
                                let sci_prec = if prec > 0 { prec - 1 } else { 0 };
                                let e_char = if conv == 'g' { 'e' } else { 'E' };
                                let raw = if e_char == 'e' {
                                    format!("{:.prec$e}", f, prec = sci_prec)
                                } else {
                                    format!("{:.prec$E}", f, prec = sci_prec)
                                };
                                normalize_scientific_exponent(&raw, e_char)
                            } else {
                                let s = format!("{:.prec$}", f, prec = prec);
                                if s.contains('.') {
                                    s.trim_end_matches('0').trim_end_matches('.').to_string()
                                } else {
                                    s
                                }
                            };
                            apply_percent_width(apply_numeric_sign(value, &flags), width, &flags)
                        }
                        'c' => {
                            let value = percent_char_string(&arg)?;
                            apply_percent_width(value, width, &flags)
                        }
                        _ => {
                            return Err(PyException::value_error(format!(
                                "unsupported format character '{}'",
                                conv
                            )))
                        }
                    };
                    result.push_str(&formatted);
                } else {
                    result.push(chars[i]);
                    i += 1;
                }
            }
            if !used_mapping_key && arg_idx < args_list.len() {
                if using_tuple_args || !args_list.is_empty() {
                    return Err(PyException::type_error(
                        "not all arguments converted during string formatting",
                    ));
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
