//! Formatting helpers, slice resolution, coercion, and module-building utilities.

use crate::error::{PyException, PyResult};
use crate::types::{HashableKey, PyInt};
use compact_str::CompactString;
use indexmap::IndexMap;

use super::payload::*;
use super::methods::PyObjectMethods;

// ── Helpers ──

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
        _ => None,
    }
}

pub(super) fn float_to_str(f: f64) -> String {
    if f == f64::INFINITY { "inf".into() }
    else if f == f64::NEG_INFINITY { "-inf".into() }
    else if f.is_nan() { "nan".into() }
    else {
        let s = format!("{}", f);
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
            // Hide internal defaultdict factory key
            if let HashableKey::Str(s) = k { s.as_str() != "__defaultdict_factory__" } else { true }
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
                Ok(PyObject::wrap(PyObjectPayload::Class(ClassData {
                    name: CompactString::from(name),
                    bases,
                    namespace: std::sync::Arc::new(parking_lot::RwLock::new(namespace)),
                    mro,
                })))
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
                Ok(PyObject::wrap(PyObjectPayload::Class(ClassData {
                    name: CompactString::from(name),
                    bases,
                    namespace: std::sync::Arc::new(parking_lot::RwLock::new(namespace)),
                    mro,
                })))
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
        _ => None,
    }
}
