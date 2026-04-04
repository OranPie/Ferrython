//! Formatting helpers, slice resolution, coercion, and module-building utilities.

use crate::error::{PyException, PyResult};
use crate::types::{HashableKey, PyInt};
use compact_str::CompactString;
use indexmap::IndexMap;
use std::sync::Arc;

use super::payload::*;
use super::methods::PyObjectMethods;

// ── Helpers ──

/// Check if a class inherits from a builtin type (int, str, float, etc.)
/// and return the builtin type name if so.
pub fn get_builtin_base_type_name(class: &PyObjectRef) -> Option<CompactString> {
    if let PyObjectPayload::Class(cd) = &class.payload {
        for base in &cd.bases {
            match &base.payload {
                PyObjectPayload::BuiltinType(name) => {
                    if matches!(name.as_str(), "int" | "str" | "float" | "list" | "tuple"
                        | "set" | "frozenset" | "bytes" | "bytearray")
                    {
                        return Some(name.clone());
                    }
                }
                PyObjectPayload::Class(_) => {
                    if let Some(name) = get_builtin_base_type_name(base) {
                        return Some(name);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// If obj is an Instance of a builtin subclass with __builtin_value__, return the value.
/// Otherwise, return the original object unchanged.
pub fn unwrap_builtin_subclass(obj: &PyObjectRef) -> PyObjectRef {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
            return val;
        }
    }
    obj.clone()
}

pub(super) fn coerce_to_f64(obj: &PyObjectRef) -> PyResult<f64> {
    match &obj.payload {
        PyObjectPayload::Float(f) => Ok(*f),
        PyObjectPayload::Int(n) => Ok(n.to_f64()),
        PyObjectPayload::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        _ => Err(PyException::type_error(format!("must be real number, not {}", obj.type_name()))),
    }
}

pub(super) fn int_bitop(a: &PyObjectRef, b: &PyObjectRef, op_name: &str, op: fn(i64, i64) -> i64) -> PyResult<PyObjectRef> {
    let ai = a.to_int().map_err(|_| PyException::type_error(format!(
        "unsupported operand type(s) for {}: '{}' and '{}'", op_name, a.type_name(), b.type_name())))?;
    let bi = b.to_int().map_err(|_| PyException::type_error(format!(
        "unsupported operand type(s) for {}: '{}' and '{}'", op_name, a.type_name(), b.type_name())))?;
    Ok(PyObject::int(op(ai, bi)))
}

pub(super) fn partial_cmp_objects(a: &PyObjectRef, b: &PyObjectRef) -> Option<std::cmp::Ordering> {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::None, PyObjectPayload::None) => Some(std::cmp::Ordering::Equal),
        (PyObjectPayload::Bool(a), PyObjectPayload::Bool(b)) => a.partial_cmp(b),
        (PyObjectPayload::Int(a), PyObjectPayload::Int(b)) => a.partial_cmp(b),
        (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => a.partial_cmp(b),
        (PyObjectPayload::Int(a), PyObjectPayload::Float(b)) => a.to_f64().partial_cmp(b),
        (PyObjectPayload::Float(a), PyObjectPayload::Int(b)) => a.partial_cmp(&b.to_f64()),
        (PyObjectPayload::Str(a), PyObjectPayload::Str(b)) => a.partial_cmp(b),
        (PyObjectPayload::Bool(a), PyObjectPayload::Int(b)) => PyInt::Small(*a as i64).partial_cmp(b),
        (PyObjectPayload::Int(a), PyObjectPayload::Bool(b)) => a.partial_cmp(&PyInt::Small(*b as i64)),
        (PyObjectPayload::List(a), PyObjectPayload::List(b)) => {
            let a = a.read(); let b = b.read();
            for (x, y) in a.iter().zip(b.iter()) {
                match partial_cmp_objects(x, y) {
                    Some(std::cmp::Ordering::Equal) => continue,
                    other => return other,
                }
            }
            a.len().partial_cmp(&b.len())
        }
        (PyObjectPayload::Tuple(a), PyObjectPayload::Tuple(b)) => {
            for (x, y) in a.iter().zip(b.iter()) {
                match partial_cmp_objects(x, y) {
                    Some(std::cmp::Ordering::Equal) => continue,
                    other => return other,
                }
            }
            a.len().partial_cmp(&b.len())
        }
        (PyObjectPayload::BuiltinFunction(a), PyObjectPayload::BuiltinFunction(b)) => {
            if a == b { Some(std::cmp::Ordering::Equal) } else { None }
        }
        (PyObjectPayload::Bytes(a), PyObjectPayload::Bytes(b)) => a.partial_cmp(b),
        (PyObjectPayload::ByteArray(a), PyObjectPayload::ByteArray(b)) => a.partial_cmp(b),
        (PyObjectPayload::Bytes(a), PyObjectPayload::ByteArray(b)) | (PyObjectPayload::ByteArray(a), PyObjectPayload::Bytes(b)) => a.partial_cmp(b),
        (PyObjectPayload::Complex { real: ar, imag: ai }, PyObjectPayload::Complex { real: br, imag: bi }) => {
            if ar == br && ai == bi { Some(std::cmp::Ordering::Equal) } else { None }
        }
        (PyObjectPayload::BuiltinType(a), PyObjectPayload::BuiltinType(b)) => {
            if a == b { Some(std::cmp::Ordering::Equal) } else { None }
        }
        (PyObjectPayload::Set(a), PyObjectPayload::Set(b)) => {
            let a = a.read(); let b = b.read();
            if a.len() != b.len() { return None; }
            for k in a.keys() {
                if !b.contains_key(k) { return None; }
            }
            Some(std::cmp::Ordering::Equal)
        }
        (PyObjectPayload::FrozenSet(a), PyObjectPayload::FrozenSet(b)) => {
            // Set equality: same keys
            if a.len() != b.len() { return None; }
            for k in a.keys() {
                if !b.contains_key(k) { return None; }
            }
            Some(std::cmp::Ordering::Equal)
        }
        // frozenset == set cross-type
        (PyObjectPayload::FrozenSet(a), PyObjectPayload::Set(b)) => {
            let rb = b.read();
            if a.len() != rb.len() { return None; }
            for k in a.keys() {
                if !rb.contains_key(k) { return None; }
            }
            Some(std::cmp::Ordering::Equal)
        }
        (PyObjectPayload::Set(a), PyObjectPayload::FrozenSet(b)) => {
            let ra = a.read();
            if ra.len() != b.len() { return None; }
            for k in ra.keys() {
                if !b.contains_key(k) { return None; }
            }
            Some(std::cmp::Ordering::Equal)
        }
        (PyObjectPayload::Dict(a), PyObjectPayload::Dict(b)) => {
            let a = a.read(); let b = b.read();
            if a.len() != b.len() { return None; }
            for (k, v1) in a.iter() {
                match b.get(k) {
                    Some(v2) if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                    _ => return None,
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        // Class identity comparison (same Arc pointer = same class)
        (PyObjectPayload::Class(a), PyObjectPayload::Class(b)) => {
            if a.name == b.name { Some(std::cmp::Ordering::Equal) } else { None }
        }
        // ExceptionType comparison
        (PyObjectPayload::ExceptionType(a), PyObjectPayload::ExceptionType(b)) => {
            if a == b { Some(std::cmp::Ordering::Equal) } else { None }
        }
        (PyObjectPayload::Range { start: s1, stop: e1, step: st1 }, PyObjectPayload::Range { start: s2, stop: e2, step: st2 }) => {
            // CPython: ranges are equal if they produce the same sequence
            // Simple shortcut: normalize empty ranges
            let len1 = range_len(*s1, *e1, *st1);
            let len2 = range_len(*s2, *e2, *st2);
            if len1 == 0 && len2 == 0 { return Some(std::cmp::Ordering::Equal); }
            if len1 != len2 { return None; }
            if *s1 != *s2 { return None; }
            if len1 == 1 { return Some(std::cmp::Ordering::Equal); }
            if *st1 == *st2 { Some(std::cmp::Ordering::Equal) } else { None }
        }
        (PyObjectPayload::InstanceDict(a), PyObjectPayload::InstanceDict(b)) => {
            let a = a.read(); let b = b.read();
            if a.len() != b.len() { return None; }
            for (k, v1) in a.iter() {
                match b.get(k.as_str()) {
                    Some(v2) if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                    _ => return None,
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        // Cross-type: InstanceDict == Dict
        (PyObjectPayload::InstanceDict(a), PyObjectPayload::Dict(b)) => {
            let a = a.read(); let b = b.read();
            if a.len() != b.len() { return None; }
            for (k, v1) in a.iter() {
                let hk = match PyObject::str_val(CompactString::from(k.as_str())).to_hashable_key() {
                    Ok(hk) => hk,
                    Err(_) => return None,
                };
                match b.get(&hk) {
                    Some(v2) if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                    _ => return None,
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        (PyObjectPayload::Dict(a_dict), PyObjectPayload::InstanceDict(b_idict)) => {
            let a_r = a_dict.read(); let b_r = b_idict.read();
            if a_r.len() != b_r.len() { return None; }
            for (k, v1) in b_r.iter() {
                let hk = match PyObject::str_val(CompactString::from(k.as_str())).to_hashable_key() {
                    Ok(hk) => hk,
                    Err(_) => return None,
                };
                match a_r.get(&hk) {
                    Some(v2) if partial_cmp_objects(v1, v2) == Some(std::cmp::Ordering::Equal) => {}
                    _ => return None,
                }
            }
            Some(std::cmp::Ordering::Equal)
        }
        // Instance comparison: check __eq__ method on class (for dataclass, custom __eq__)
        (PyObjectPayload::Instance(inst_a), PyObjectPayload::Instance(_inst_b)) => {
            // Check if they are the same object
            if Arc::ptr_eq(a, b) { return Some(std::cmp::Ordering::Equal); }
            // Look for __eq__ in the class hierarchy
            fn find_in_mro(cls: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
                if let PyObjectPayload::Class(cd) = &cls.payload {
                    let ns = cd.namespace.read();
                    if let Some(f) = ns.get(name) { return Some(f.clone()); }
                    for base in &cd.mro {
                        if let PyObjectPayload::Class(bcd) = &base.payload {
                            let bns = bcd.namespace.read();
                            if let Some(f) = bns.get(name) { return Some(f.clone()); }
                        }
                    }
                }
                None
            }
            if let Some(eq_fn) = find_in_mro(&inst_a.class, "__eq__") {
                match &eq_fn.payload {
                    PyObjectPayload::NativeFunction { func, .. } => {
                        if let Ok(result) = func(&[a.clone(), b.clone()]) {
                            return if result.is_truthy() { Some(std::cmp::Ordering::Equal) } else { None };
                        }
                    }
                    PyObjectPayload::NativeClosure { func, .. } => {
                        if let Ok(result) = func(&[a.clone(), b.clone()]) {
                            return if result.is_truthy() { Some(std::cmp::Ordering::Equal) } else { None };
                        }
                    }
                    _ => {}
                }
            }
            // For __lt__ comparison (used by sorted), also check
            if let Some(lt_fn) = find_in_mro(&inst_a.class, "__lt__") {
                match &lt_fn.payload {
                    PyObjectPayload::NativeFunction { func, .. } => {
                        if let Ok(result) = func(&[a.clone(), b.clone()]) {
                            if result.is_truthy() { return Some(std::cmp::Ordering::Less); }
                        }
                        if let Ok(result) = func(&[b.clone(), a.clone()]) {
                            if result.is_truthy() { return Some(std::cmp::Ordering::Greater); }
                        }
                        return Some(std::cmp::Ordering::Equal);
                    }
                    PyObjectPayload::NativeClosure { func, .. } => {
                        if let Ok(result) = func(&[a.clone(), b.clone()]) {
                            if result.is_truthy() { return Some(std::cmp::Ordering::Less); }
                        }
                        if let Ok(result) = func(&[b.clone(), a.clone()]) {
                            if result.is_truthy() { return Some(std::cmp::Ordering::Greater); }
                        }
                        return Some(std::cmp::Ordering::Equal);
                    }
                    _ => {}
                }
            }
            None
        }
        _ => None,
    }
}

fn range_len(start: i64, stop: i64, step: i64) -> i64 {
    if step > 0 && start < stop { (stop - start + step - 1) / step }
    else if step < 0 && start > stop { (start - stop - step - 1) / (-step) }
    else { 0 }
}

pub(super) fn float_to_str(f: f64) -> String {
    if f == f64::INFINITY { return "inf".into(); }
    if f == f64::NEG_INFINITY { return "-inf".into(); }
    if f.is_nan() { return "nan".into(); }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    
    let abs_f = f.abs();
    // CPython uses scientific notation for |f| >= 1e16 or |f| < 1e-4
    if abs_f >= 1e16 || abs_f < 1e-4 {
        // Format as shortest scientific notation
        let s = format!("{:e}", f);
        // Python uses e+XX format (pad exponent to 2 digits minimum)
        // Rust gives e.g. "1e20", Python wants "1e+20"
        let s = if let Some(pos) = s.find('e') {
            let (mantissa, exp_part) = s.split_at(pos);
            let exp_str = &exp_part[1..]; // skip 'e'
            let exp: i32 = exp_str.parse().unwrap_or(0);
            if exp >= 0 {
                format!("{}e+{:02}", mantissa, exp)
            } else {
                format!("{}e-{:02}", mantissa, exp.abs())
            }
        } else {
            s
        };
        // Clean up trailing zeros in mantissa: 1.00000000000000000e+20 -> 1e+20
        if let Some(dot_pos) = s.find('.') {
            if let Some(e_pos) = s.find('e') {
                let frac = &s[dot_pos+1..e_pos];
                let trimmed = frac.trim_end_matches('0');
                if trimmed.is_empty() {
                    format!("{}{}", &s[..dot_pos], &s[e_pos..])
                } else {
                    format!("{}.{}{}", &s[..dot_pos], trimmed, &s[e_pos..])
                }
            } else {
                s
            }
        } else {
            s
        }
    } else {
        // Use Rust's Debug which preserves precision
        let s = format!("{}", f);
        // Ensure it has a decimal point
        if s.contains('.') || s.contains('e') { s } else { format!("{}.0", s) }
    }
}

pub(super) fn python_fmod(a: f64, b: f64) -> f64 {
    let r = a % b;
    if (r != 0.0) && ((r < 0.0) != (b < 0.0)) { r + b } else { r }
}

pub(super) fn format_int_spec(n: i64, spec: &str) -> String {
    // Parse width from spec
    let width: usize = spec.trim_start_matches(|c: char| "- +#0".contains(c))
        .parse().unwrap_or(0);
    let zero_pad = spec.starts_with('0');
    let left_align = spec.starts_with('-');
    let s = n.to_string();
    if width == 0 { return s; }
    if zero_pad && !left_align {
        if n < 0 {
            format!("-{:0>width$}", &s[1..], width = width - 1)
        } else {
            format!("{:0>width$}", s, width = width)
        }
    } else if left_align {
        format!("{:<width$}", s, width = width)
    } else {
        format!("{:>width$}", s, width = width)
    }
}

pub(super) fn format_float_spec(f: f64, spec: &str) -> String {
    // Parse precision from spec (e.g., ".2")
    if let Some(dot_pos) = spec.find('.') {
        let prec_str = &spec[dot_pos + 1..];
        let prec: usize = prec_str.parse().unwrap_or(6);
        format!("{:.prec$}", f, prec = prec)
    } else {
        format!("{:.6}", f)
    }
}

pub fn format_str_spec(s: &str, spec: &str) -> String {
    let left_align = spec.starts_with('-');
    let width_str = spec.trim_start_matches(|c: char| "-+ #0".contains(c));
    // Parse precision (max string length)
    let (width_part, precision) = if let Some(dot) = width_str.find('.') {
        (&width_str[..dot], width_str[dot + 1..].parse::<usize>().ok())
    } else {
        (width_str, None)
    };
    let width: usize = width_part.parse().unwrap_or(0);
    let display = if let Some(prec) = precision {
        if s.len() > prec { &s[..prec] } else { s }
    } else {
        s
    };
    if width == 0 { return display.to_string(); }
    if left_align {
        format!("{:<width$}", display, width = width)
    } else {
        format!("{:>width$}", display, width = width)
    }
}

/// Python format spec mini-language: [[fill]align][sign][#][0][width][grouping][.precision][type]
pub fn format_value_spec(s: &str, spec: &str) -> String {
    if spec.is_empty() { return s.to_string(); }
    let chars: Vec<char> = spec.chars().collect();
    let mut i = 0;
    // Parse optional fill and align
    let (fill, align) = if chars.len() >= 2 && matches!(chars[1], '<' | '>' | '^' | '=') {
        i = 2;
        (chars[0], chars[1])
    } else if !chars.is_empty() && matches!(chars[0], '<' | '>' | '^' | '=') {
        i = 1;
        (' ', chars[0])
    } else {
        (' ', '<') // default: left-align for strings
    };
    // Parse width
    let mut width = 0usize;
    while i < chars.len() && chars[i].is_ascii_digit() {
        width = width * 10 + (chars[i] as usize - '0' as usize);
        i += 1;
    }
    // Parse .precision
    let mut precision: Option<usize> = None;
    if i < chars.len() && chars[i] == '.' {
        i += 1;
        let mut p = 0usize;
        while i < chars.len() && chars[i].is_ascii_digit() {
            p = p * 10 + (chars[i] as usize - '0' as usize);
            i += 1;
        }
        precision = Some(p);
    }
    // Parse type (d, f, s, etc.) — ignored for string formatting
    // Apply precision (truncation for strings)
    let display = if let Some(prec) = precision {
        let chars_vec: Vec<char> = s.chars().collect();
        if chars_vec.len() > prec { chars_vec[..prec].iter().collect() } else { s.to_string() }
    } else {
        s.to_string()
    };
    if width == 0 || display.len() >= width { return display; }
    let pad = width - display.len();
    match align {
        '<' => format!("{}{}", display, std::iter::repeat(fill).take(pad).collect::<String>()),
        '>' => format!("{}{}", std::iter::repeat(fill).take(pad).collect::<String>(), display),
        '^' => {
            let left = pad / 2;
            let right = pad - left;
            format!("{}{}{}", 
                std::iter::repeat(fill).take(left).collect::<String>(),
                display,
                std::iter::repeat(fill).take(right).collect::<String>())
        }
        _ => display,
    }
}

pub(super) fn add_thousands_separator(s: &str, sep: char) -> String {
    // Find the integer part (before any decimal point)
    let (sign, rest) = if s.starts_with('-') { ("-", &s[1..]) } else { ("", s) };
    let (int_part, frac_part) = if let Some(dot) = rest.find('.') {
        (&rest[..dot], &rest[dot..])
    } else {
        (rest, "")
    };
    let mut result = String::new();
    for (i, ch) in int_part.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 { result.push(sep); }
        result.push(ch);
    }
    let grouped: String = result.chars().rev().collect();
    format!("{}{}{}", sign, grouped, frac_part)
}

/// Apply sign and alignment to a numeric string. Handles +, -, space signs and width/fill.
pub fn apply_numeric_sign(value_str: &str, spec: &str) -> String {
    if spec.is_empty() { return value_str.to_string(); }
    let chars: Vec<char> = spec.chars().collect();
    let mut i = 0;
    let mut fill = ' ';
    let mut align = None;
    let mut sign = '-'; // default: only show negative

    // Check for fill+align
    if chars.len() >= 2 && "<>^=".contains(chars[1]) {
        fill = chars[0];
        align = Some(chars[1]);
        i = 2;
    } else if !chars.is_empty() && "<>^=".contains(chars[0]) {
        align = Some(chars[0]);
        i = 1;
    }
    // Check for sign
    if i < chars.len() && "+-  ".contains(chars[i]) {
        sign = chars[i];
        i += 1;
    }
    // Check for # (alt form)
    if i < chars.len() && chars[i] == '#' {
        i += 1;
    }
    // Check for 0 fill (zero padding)
    if i < chars.len() && chars[i] == '0' && align.is_none() {
        fill = '0';
        align = Some('=');
        i += 1;
    }
    // Parse width
    let width_str: String = chars[i..].iter().take_while(|c| c.is_ascii_digit()).collect();
    i += width_str.len();
    let width: usize = width_str.parse().unwrap_or(0);

    // Parse .precision
    if i < chars.len() && chars[i] == '.' {
        i += 1;
        // skip precision digits
        while i < chars.len() && chars[i].is_ascii_digit() { i += 1; }
    }

    // Apply sign to the numeric value
    let is_negative = value_str.starts_with('-');
    let digits = if is_negative { &value_str[1..] } else { value_str };
    let sign_str = if is_negative {
        "-"
    } else {
        match sign {
            '+' => "+",
            ' ' => " ",
            _ => "",
        }
    };

    let full = format!("{}{}", sign_str, digits);
    if width == 0 || full.len() >= width {
        return full;
    }

    let pad_len = width - full.len();
    let actual_align = align.unwrap_or('>');
    match actual_align {
        '<' => format!("{}{}", full, std::iter::repeat(fill).take(pad_len).collect::<String>()),
        '>' => format!("{}{}", std::iter::repeat(fill).take(pad_len).collect::<String>(), full),
        '=' => format!("{}{}{}", sign_str, std::iter::repeat(fill).take(pad_len).collect::<String>(), digits),
        '^' => {
            let left = pad_len / 2;
            let right = pad_len - left;
            format!("{}{}{}", std::iter::repeat(fill).take(left).collect::<String>(), full, std::iter::repeat(fill).take(right).collect::<String>())
        }
        _ => full,
    }
}

/// Apply formatting to a prefixed number (0x, 0o, 0b). Handles zero-padding between prefix and digits.
pub fn apply_prefixed_format(digits: &str, prefix: &str, spec: &str) -> String {
    if spec.is_empty() {
        return format!("{}{}", prefix, digits);
    }
    let chars: Vec<char> = spec.chars().collect();
    let mut i = 0;
    let mut fill = ' ';
    let mut align = None;

    // Check for fill+align
    if chars.len() >= 2 && "<>^=".contains(chars[1]) {
        fill = chars[0];
        align = Some(chars[1]);
        i = 2;
    } else if !chars.is_empty() && "<>^=".contains(chars[0]) {
        align = Some(chars[0]);
        i = 1;
    }
    // Skip sign
    if i < chars.len() && "+-  ".contains(chars[i]) { i += 1; }
    // Check for 0 fill
    if i < chars.len() && chars[i] == '0' && align.is_none() {
        fill = '0';
        align = Some('=');
        i += 1;
    }
    // Parse width
    let width_str: String = chars[i..].iter().take_while(|c| c.is_ascii_digit()).collect();
    let width: usize = width_str.parse().unwrap_or(0);

    let full = format!("{}{}", prefix, digits);
    if width == 0 || full.len() >= width {
        return full;
    }

    let pad_len = width - full.len();
    match align.unwrap_or('>') {
        '=' | '>' if fill == '0' => {
            format!("{}{}{}", prefix, std::iter::repeat('0').take(pad_len).collect::<String>(), digits)
        }
        '<' => format!("{}{}", full, std::iter::repeat(fill).take(pad_len).collect::<String>()),
        '>' => format!("{}{}", std::iter::repeat(fill).take(pad_len).collect::<String>(), full),
        '^' => {
            let left = pad_len / 2;
            let right = pad_len - left;
            format!("{}{}{}", std::iter::repeat(fill).take(left).collect::<String>(), full, std::iter::repeat(fill).take(right).collect::<String>())
        }
        _ => full,
    }
}

pub fn apply_string_format_spec(s: &str, spec: &str) -> String {
    if spec.is_empty() { return s.to_string(); }
    let chars: Vec<char> = spec.chars().collect();
    let mut i = 0;
    let mut fill = ' ';
    let mut align = None;
    // Check for fill+align
    if chars.len() >= 2 && "<>^=".contains(chars[1]) {
        fill = chars[0];
        align = Some(chars[1]);
        i = 2;
    } else if !chars.is_empty() && "<>^".contains(chars[0]) {
        align = Some(chars[0]);
        i = 1;
    }
    // Check for sign
    if i < chars.len() && "+-".contains(chars[i]) {
        i += 1;
    }
    // Check for 0 fill (only when no explicit fill+align given)
    if i < chars.len() && chars[i] == '0' && align.is_none() {
        fill = '0';
        align = Some('>');
        i += 1;
    }
    // Parse width
    let width_str: String = chars[i..].iter().take_while(|c| c.is_ascii_digit()).collect();
    let width: usize = width_str.parse().unwrap_or(0);
    if width <= s.len() { return s.to_string(); }
    let pad_len = width - s.len();
    match align.unwrap_or('>') {
        '<' => format!("{}{}", s, std::iter::repeat(fill).take(pad_len).collect::<String>()),
        '>' | '=' => format!("{}{}", std::iter::repeat(fill).take(pad_len).collect::<String>(), s),
        '^' => {
            let left = pad_len / 2;
            let right = pad_len - left;
            format!("{}{}{}", std::iter::repeat(fill).take(left).collect::<String>(), s, std::iter::repeat(fill).take(right).collect::<String>())
        }
        _ => s.to_string(),
    }
}

/// Resolve slice start/stop/step into actual indices for a sequence of given length.
pub(super) fn resolve_slice(
    start: &Option<PyObjectRef>,
    stop: &Option<PyObjectRef>,
    step: &Option<PyObjectRef>,
    len: i64,
) -> (i64, i64, i64) {
    let step_val = step.as_ref()
        .and_then(|s| if matches!(s.payload, PyObjectPayload::None) { None } else { Some(s) })
        .and_then(|s| s.as_int())
        .unwrap_or(1);

    let (default_start, default_stop) = if step_val < 0 { (len - 1, -len - 1) } else { (0, len) };

    let start_val = start.as_ref()
        .and_then(|s| if matches!(s.payload, PyObjectPayload::None) { None } else { Some(s) })
        .and_then(|s| s.as_int())
        .map(|i| {
            if i < 0 { (len + i).max(if step_val < 0 { -1 } else { 0 }) }
            else { i.min(len) }
        })
        .unwrap_or(default_start);

    let stop_val = stop.as_ref()
        .and_then(|s| if matches!(s.payload, PyObjectPayload::None) { None } else { Some(s) })
        .and_then(|s| s.as_int())
        .map(|i| {
            if i < 0 { (len + i).max(if step_val < 0 { -1 } else { 0 }) }
            else { i.min(len) }
        })
        .unwrap_or(default_stop);

    (start_val, stop_val, step_val)
}

pub(super) fn get_slice_impl(
    obj: &PyObjectRef,
    start: &Option<PyObjectRef>,
    stop: &Option<PyObjectRef>,
    step: &Option<PyObjectRef>,
) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::List(items) => {
            let items = items.read();
            let len = items.len() as i64;
            let (s, e, step) = resolve_slice(start, stop, step, len);
            let mut result = Vec::new();
            let mut i = s;
            if step > 0 {
                while i < e && i < len { result.push(items[i as usize].clone()); i += step; }
            } else if step < 0 {
                while i > e && i >= 0 { result.push(items[i as usize].clone()); i += step; }
            }
            Ok(PyObject::list(result))
        }
        PyObjectPayload::Tuple(items) => {
            let len = items.len() as i64;
            let (s, e, step) = resolve_slice(start, stop, step, len);
            let mut result = Vec::new();
            let mut i = s;
            if step > 0 {
                while i < e && i < len { result.push(items[i as usize].clone()); i += step; }
            } else if step < 0 {
                while i > e && i >= 0 { result.push(items[i as usize].clone()); i += step; }
            }
            Ok(PyObject::tuple(result))
        }
        PyObjectPayload::Str(s) => {
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as i64;
            let (sv, ev, step) = resolve_slice(start, stop, step, len);
            let mut result = String::new();
            let mut i = sv;
            if step > 0 {
                while i < ev && i < len { result.push(chars[i as usize]); i += step; }
            } else if step < 0 {
                while i > ev && i >= 0 { result.push(chars[i as usize]); i += step; }
            }
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
            let len = b.len() as i64;
            let (sv, ev, step) = resolve_slice(start, stop, step, len);
            let mut result = Vec::new();
            let mut i = sv;
            if step > 0 {
                while i < ev && i < len { result.push(b[i as usize]); i += step; }
            } else if step < 0 {
                while i > ev && i >= 0 { result.push(b[i as usize]); i += step; }
            }
            Ok(PyObject::bytes(result))
        }
        _ => Err(PyException::type_error(format!("'{}' object is not subscriptable", obj.type_name()))),
    }
}

/// Format a bytes literal like b'...' with proper escaping (shared by bytes and bytearray repr).
pub(super) fn format_bytes_literal(b: &[u8], prefix: &str) -> String {
    let mut out = String::new();
    out.push_str(prefix);
    out.push('\'');
    for &byte in b {
        match byte {
            b'\\' => out.push_str("\\\\"),
            b'\'' => out.push_str("\\'"),
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            0x20..=0x7e => out.push(byte as char),
            _ => out.push_str(&format!("\\x{:02x}", byte)),
        }
    }
    out.push('\'');
    out
}

pub(super) fn format_collection(open: &str, close: &str, items: &[PyObjectRef]) -> String {
    let inner: Vec<String> = items.iter().map(|i| i.repr()).collect();
    format!("{}{}{}", open, inner.join(", "), close)
}

pub(super) fn format_set(open: &str, close: &str, map: &IndexMap<HashableKey, PyObjectRef>) -> String {
    let inner: Vec<String> = map.values().map(|v| v.repr()).collect();
    format!("{}{}{}", open, inner.join(", "), close)
}

pub(super) fn format_dict(map: &IndexMap<HashableKey, PyObjectRef>) -> String {
    let inner: Vec<String> = map.iter()
        .filter(|(k, _)| {
            // Hide internal defaultdict/counter marker keys
            match k {
                HashableKey::Str(s) => {
                    let key = s.as_str();
                    key != "__defaultdict_factory__" && key != "__counter__"
                }
                _ => true,
            }
        })
        .map(|(k, v)| format!("{}: {}", k.to_object().repr(), v.repr())).collect();
    format!("{{{}}}", inner.join(", "))
}

// ── Module-building utilities (used by ferrython-stdlib and ferrython-vm) ──

/// The function pointer type for built-in functions.
pub type BuiltinFn = fn(&[PyObjectRef]) -> PyResult<PyObjectRef>;

/// Create a module object with named attributes.
pub fn make_module(name: &str, attrs: Vec<(&str, PyObjectRef)>) -> PyObjectRef {
    let mut map = IndexMap::new();
    map.insert(CompactString::from("__name__"), PyObject::str_val(CompactString::from(name)));
    for (k, v) in attrs {
        map.insert(CompactString::from(k), v);
    }
    PyObject::module_with_attrs(CompactString::from(name), map)
}

/// Wrap a bare function pointer as a NativeFunction object.
pub fn make_builtin(f: BuiltinFn) -> PyObjectRef {
    PyObject::native_function("", f)
}

/// Check that exactly `expected` arguments were provided.
pub fn check_args(name: &str, args: &[PyObjectRef], expected: usize) -> PyResult<()> {
    if args.len() != expected {
        Err(PyException::type_error(format!(
            "{}() takes exactly {} argument(s) ({} given)", name, expected, args.len()
        )))
    } else { Ok(()) }
}

/// Check that at least `min` arguments were provided.
pub fn check_args_min(name: &str, args: &[PyObjectRef], min: usize) -> PyResult<()> {
    if args.len() < min {
        Err(PyException::type_error(format!(
            "{}() takes at least {} argument(s) ({} given)", name, min, args.len()
        )))
    } else { Ok(()) }
}

/// Resolve known built-in type methods that can be defined without VM access.
/// This is used by super() resolution when a base is a BuiltinType.
pub fn resolve_builtin_type_method(type_name: &str, method_name: &str) -> Option<PyObjectRef> {
    match (type_name, method_name) {
        ("type", "__new__") => Some(PyObject::native_function("type.__new__", |args| {
            // type.__new__(mcs, name, bases, dict) or type(name, bases, dict)
            if args.len() == 4 {
                let name = args[1].as_str().ok_or_else(||
                    PyException::type_error("type.__new__ argument 2 must be str"))?;
                let bases = args[2].to_list()?;
                let namespace = match &args[3].payload {
                    PyObjectPayload::Dict(m) => {
                        let r = m.read();
                        let mut ns = IndexMap::new();
                        for (k, v) in r.iter() {
                            let key_str = match k {
                                HashableKey::Str(s) => s.clone(),
                                _ => CompactString::from(k.to_object().py_to_string()),
                            };
                            ns.insert(key_str, v.clone());
                        }
                        ns
                    }
                    _ => return Err(PyException::type_error("type.__new__ argument 4 must be dict")),
                };
                let mut mro = Vec::new();
                for base in &bases {
                    mro.push(base.clone());
                    if let PyObjectPayload::Class(cd) = &base.payload {
                        for m in &cd.mro {
                            if !mro.iter().any(|existing| std::sync::Arc::ptr_eq(existing, m)) {
                                mro.push(m.clone());
                            }
                        }
                    }
                }
                Ok(PyObject::wrap(PyObjectPayload::Class(ClassData::new(
                    CompactString::from(name),
                    bases,
                    namespace,
                    mro,
                    None,
                ))))
            } else if args.len() == 3 {
                // type(name, bases, dict) — no mcs
                let name = args[0].as_str().ok_or_else(||
                    PyException::type_error("type() argument 1 must be str"))?;
                let bases = args[1].to_list()?;
                let namespace = match &args[2].payload {
                    PyObjectPayload::Dict(m) => {
                        let r = m.read();
                        let mut ns = IndexMap::new();
                        for (k, v) in r.iter() {
                            let key_str = match k {
                                HashableKey::Str(s) => s.clone(),
                                _ => CompactString::from(k.to_object().py_to_string()),
                            };
                            ns.insert(key_str, v.clone());
                        }
                        ns
                    }
                    _ => return Err(PyException::type_error("type() argument 3 must be dict")),
                };
                let mut mro = Vec::new();
                for base in &bases {
                    mro.push(base.clone());
                    if let PyObjectPayload::Class(cd) = &base.payload {
                        for m in &cd.mro {
                            if !mro.iter().any(|existing| std::sync::Arc::ptr_eq(existing, m)) {
                                mro.push(m.clone());
                            }
                        }
                    }
                }
                Ok(PyObject::wrap(PyObjectPayload::Class(ClassData::new(
                    CompactString::from(name),
                    bases,
                    namespace,
                    mro,
                    None,
                ))))
            } else {
                Err(PyException::type_error("type.__new__ requires 3 or 4 arguments"))
            }
        })),
        ("object", "__new__") => Some(PyObject::native_function("object.__new__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("object.__new__ requires cls"));
            }
            Ok(PyObject::instance(args[0].clone()))
        })),
        // __init__ on any builtin type base is a no-op (instance already created)
        (_, "__init__") => Some(PyObject::native_function("builtin.__init__", |_args| {
            Ok(PyObject::none())
        })),
        // dict.__getitem__(self, key) — access dict_storage on dict subclass
        ("dict", "__getitem__") => Some(PyObject::native_function("dict.__getitem__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("dict.__getitem__() takes exactly 2 arguments"));
            }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(ref ds) = inst.dict_storage {
                    let hk = args[1].to_hashable_key()?;
                    if let Some(val) = ds.read().get(&hk) {
                        return Ok(val.clone());
                    }
                    return Err(PyException::key_error(args[1].py_to_string()));
                }
            }
            Err(PyException::type_error("dict.__getitem__ requires a dict instance"))
        })),
        // dict.__setitem__(self, key, value)
        ("dict", "__setitem__") => Some(PyObject::native_function("dict.__setitem__", |args| {
            if args.len() != 3 {
                return Err(PyException::type_error("dict.__setitem__() takes exactly 3 arguments"));
            }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(ref ds) = inst.dict_storage {
                    let hk = args[1].to_hashable_key()?;
                    ds.write().insert(hk, args[2].clone());
                    return Ok(PyObject::none());
                }
            }
            Err(PyException::type_error("dict.__setitem__ requires a dict instance"))
        })),
        // dict.__delitem__(self, key)
        ("dict", "__delitem__") => Some(PyObject::native_function("dict.__delitem__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("dict.__delitem__() takes exactly 2 arguments"));
            }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(ref ds) = inst.dict_storage {
                    let hk = args[1].to_hashable_key()?;
                    if ds.write().shift_remove(&hk).is_some() {
                        return Ok(PyObject::none());
                    }
                    return Err(PyException::key_error(args[1].py_to_string()));
                }
            }
            Err(PyException::type_error("dict.__delitem__ requires a dict instance"))
        })),
        // dict.__contains__(self, key)
        ("dict", "__contains__") => Some(PyObject::native_function("dict.__contains__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("dict.__contains__() takes exactly 2 arguments"));
            }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(ref ds) = inst.dict_storage {
                    let hk = args[1].to_hashable_key()?;
                    return Ok(PyObject::bool_val(ds.read().contains_key(&hk)));
                }
            }
            Ok(PyObject::bool_val(false))
        })),
        _ => None,
    }
}
