//! Core builtin function implementations (print, len, type, etc.)

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    check_args, check_args_min,
    IteratorData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

use super::iter_advance;

pub(super) fn builtin_print(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let parts: Vec<String> = args.iter().map(|a| a.py_to_string()).collect();
    println!("{}", parts.join(" "));
    Ok(PyObject::none())
}

pub(super) fn builtin_len(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("len", args, 1)?;
    let n = args[0].py_len()?;
    Ok(PyObject::int(n as i64))
}

pub(super) fn builtin_repr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("repr", args, 1)?;
    // Check for user-defined __repr__
    if let Some(repr_method) = args[0].get_attr("__repr__") {
        if matches!(&repr_method.payload, PyObjectPayload::BoundMethod { .. }) {
            // We can't call it here (no VM reference), so use py_to_string on the method
            // Actually, let's extract the result from the repr method
            // For now, fall through to default
        }
    }
    Ok(PyObject::str_val(CompactString::from(args[0].repr())))
}

pub(super) fn builtin_str(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::str_val(CompactString::from("")));
    }
    Ok(PyObject::str_val(CompactString::from(args[0].py_to_string())))
}

pub(super) fn builtin_int(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::int(0));
    }
    if args.len() >= 2 {
        // int(string, base)
        let s = args[0].as_str().ok_or_else(||
            PyException::type_error("int() can't convert non-string with explicit base"))?;
        let base = args[1].to_int()? as u32;
        let s = s.trim();
        // Strip base prefix if present
        let s = if base == 16 && (s.starts_with("0x") || s.starts_with("0X")) {
            &s[2..]
        } else if base == 8 && (s.starts_with("0o") || s.starts_with("0O")) {
            &s[2..]
        } else if base == 2 && (s.starts_with("0b") || s.starts_with("0B")) {
            &s[2..]
        } else {
            s
        };
        let val = i64::from_str_radix(s, base).map_err(|_|
            PyException::value_error(format!("invalid literal for int() with base {}: '{}'", base, args[0].as_str().unwrap())))?;
        return Ok(PyObject::int(val));
    }
    Ok(PyObject::int(args[0].to_int()?))
}

pub(super) fn builtin_float(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::float(0.0));
    }
    Ok(PyObject::float(args[0].to_float()?))
}

pub(super) fn builtin_bool(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(args[0].is_truthy()))
}

pub(super) fn builtin_type(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // type.__new__(mcs, name, bases, dict) — called from metaclass __new__
    if args.len() == 4 {
        // First arg is the metaclass (mcs), skip it; use name, bases, dict
        return builtin_type_create(&args[1], &args[2], &args[3]);
    }
    if args.len() == 3 {
        // type(name, bases, dict) → dynamic class creation
        return builtin_type_create(&args[0], &args[1], &args[2]);
    }
    check_args("type", args, 1)?;
    let name = args[0].type_name();
    match &args[0].payload {
        PyObjectPayload::Instance(inst) => {
            Ok(inst.class.clone())
        }
        PyObjectPayload::ExceptionInstance { kind, .. } => {
            Ok(PyObject::exception_type(kind.clone()))
        }
        _ => Ok(PyObject::builtin_type(CompactString::from(name)))
    }
}

fn builtin_type_create(name_obj: &PyObjectRef, bases_obj: &PyObjectRef, dict_obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    let name = name_obj.as_str().ok_or_else(||
        PyException::type_error("type() argument 1 must be str"))?;
    let bases = bases_obj.to_list()?;
    let namespace = match &dict_obj.payload {
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
                if !mro.iter().any(|existing| Arc::ptr_eq(existing, m)) {
                    mro.push(m.clone());
                }
            }
        }
    }
    Ok(PyObject::wrap(PyObjectPayload::Class(ferrython_core::object::ClassData {
        name: CompactString::from(name),
        bases,
        namespace: Arc::new(RwLock::new(namespace)),
        mro,
        metaclass: None,
    })))
}

pub(super) fn builtin_id(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("id", args, 1)?;
    let ptr = std::sync::Arc::as_ptr(&args[0]) as usize;
    Ok(PyObject::int(ptr as i64))
}

pub(crate) fn builtin_abs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("abs", args, 1)?;
    args[0].py_abs()
}

pub(super) fn builtin_min(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("min expected at least 1 argument, got 0"));
    }
    let items = if args.len() == 1 { args[0].to_list()? } else { args.to_vec() };
    if items.is_empty() {
        return Err(PyException::value_error("min() arg is an empty sequence"));
    }
    let mut best = items[0].clone();
    for item in &items[1..] {
        if item.compare(&best, ferrython_core::object::CompareOp::Lt)?.is_truthy() {
            best = item.clone();
        }
    }
    Ok(best)
}

pub(super) fn builtin_max(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("max expected at least 1 argument, got 0"));
    }
    let items = if args.len() == 1 { args[0].to_list()? } else { args.to_vec() };
    if items.is_empty() {
        return Err(PyException::value_error("max() arg is an empty sequence"));
    }
    let mut best = items[0].clone();
    for item in &items[1..] {
        if item.compare(&best, ferrython_core::object::CompareOp::Gt)?.is_truthy() {
            best = item.clone();
        }
    }
    Ok(best)
}

pub(super) fn builtin_sum(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("sum expected at least 1 argument, got 0"));
    }
    let items = args[0].to_list()?;
    let start = if args.len() > 1 { args[1].clone() } else { PyObject::int(0) };
    let mut total = start;
    for item in items {
        total = total.add(&item)?;
    }
    Ok(total)
}

pub(super) fn builtin_round(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("round", args, 1)?;
    let ndigits = if args.len() >= 2 { Some(args[1].to_int()?) } else { None };
    match &args[0].payload {
        PyObjectPayload::Int(_) => Ok(args[0].clone()),
        PyObjectPayload::Float(f) => {
            if let Some(n) = ndigits {
                let factor = 10f64.powi(n as i32);
                let scaled = f * factor;
                let rounded = round_half_to_even(scaled);
                Ok(PyObject::float(rounded / factor))
            } else {
                Ok(PyObject::int(round_half_to_even(*f) as i64))
            }
        }
        PyObjectPayload::Bool(b) => Ok(PyObject::int(if *b { 1 } else { 0 })),
        _ => Err(PyException::type_error(format!(
            "type '{}' doesn't define __round__ method", args[0].type_name()
        ))),
    }
}

/// IEEE 754 round-half-to-even (banker's rounding).
/// When the value is exactly halfway between two integers, round to the nearest even integer.
fn round_half_to_even(x: f64) -> f64 {
    let rounded = x.round();
    // Check if we're exactly at a .5 boundary
    if (x - x.floor() - 0.5).abs() < 1e-9 {
        // Exactly halfway — round to even
        if rounded as i64 % 2 != 0 {
            // rounded is odd, go the other way
            if x > 0.0 { rounded - 1.0 } else { rounded + 1.0 }
        } else {
            rounded
        }
    } else {
        rounded
    }
}

pub(super) fn builtin_pow(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("pow", args, 2)?;
    let result = args[0].power(&args[1])?;
    if args.len() >= 3 {
        result.modulo(&args[2])
    } else {
        Ok(result)
    }
}

pub(super) fn builtin_divmod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("divmod", args, 2)?;
    let q = args[0].floor_div(&args[1])?;
    let r = args[0].modulo(&args[1])?;
    Ok(PyObject::tuple(vec![q, r]))
}

pub(super) fn builtin_hash(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("hash", args, 1)?;
    // Check for custom __hash__ on Instance objects first
    if let PyObjectPayload::Instance(_) = &args[0].payload {
        if let Some(_hash_fn) = args[0].get_attr("__hash__") {
            // __hash__ found but we can't call it from here (no VM).
            // Return identity-based hash instead.
            let ptr = std::sync::Arc::as_ptr(&args[0]) as usize;
            return Ok(PyObject::int(ptr as i64));
        }
    }
    let key = args[0].to_hashable_key()?;
    let h = match key {
        HashableKey::Int(n) => n.to_i64().unwrap_or(0),
        HashableKey::Bool(b) => b as i64,
        HashableKey::Str(ref s) => {
            let mut h: u64 = 5381;
            for c in s.bytes() { h = h.wrapping_mul(33).wrapping_add(c as u64); }
            h as i64
        }
        HashableKey::Float(f) => f.0.to_bits() as i64,
        HashableKey::None => 0,
        HashableKey::Tuple(_) => 0,
        HashableKey::FrozenSet(_) => 0,
        HashableKey::Bytes(b) => { let mut h: u64 = 5381; for x in b { h = h.wrapping_mul(33).wrapping_add(x as u64); } h as i64 }
        HashableKey::Identity(ptr) => ptr as i64,
        HashableKey::Custom { hash_value, .. } => hash_value,
    };
    Ok(PyObject::int(h))
}

pub(super) fn builtin_isinstance(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("isinstance", args, 2)?;
    let obj = &args[0];
    let cls = &args[1];
    // Handle tuple of types: isinstance(x, (int, str))
    if let PyObjectPayload::Tuple(types) = &cls.payload {
        for t in types {
            if is_instance_of(obj, t) {
                return Ok(PyObject::bool_val(true));
            }
        }
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(is_instance_of(obj, cls)))
}

/// Check if obj is an instance of cls (including inheritance).
pub(crate) fn is_instance_of(obj: &PyObjectRef, cls: &PyObjectRef) -> bool {
    match &cls.payload {
        PyObjectPayload::BuiltinFunction(type_name) | PyObjectPayload::BuiltinType(type_name) => {
            // Everything is an instance of object
            if type_name.as_str() == "object" {
                return true;
            }
            let obj_type = obj.type_name();
            if obj_type == type_name.as_str() {
                return true;
            }
            // Built-in subtype relationships: bool is subclass of int
            if type_name.as_str() == "int" && obj_type == "bool" {
                return true;
            }
            // Check user-defined classes that inherit from builtins
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                return class_is_subclass_of(&inst.class, type_name.as_str());
            }
            false
        }
        PyObjectPayload::Class(target_cd) => {
            // User-defined class check: walk the instance's class MRO
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                class_is_subclass_of(&inst.class, &target_cd.name)
            } else {
                false
            }
        }
        PyObjectPayload::ExceptionType(kind) => {
            // Check if obj is an exception instance of this type
            if let PyObjectPayload::ExceptionInstance { kind: obj_kind, .. } = &obj.payload {
                obj_kind == kind
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Check if a class (or any of its bases) has the given name.
pub(crate) fn class_is_subclass_of(cls: &PyObjectRef, target_name: &str) -> bool {
    if let PyObjectPayload::Class(cd) = &cls.payload {
        if cd.name.as_str() == target_name {
            return true;
        }
        for base in &cd.bases {
            if class_is_subclass_of(base, target_name) {
                return true;
            }
        }
    }
    false
}

pub(super) fn builtin_callable(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("callable", args, 1)?;
    Ok(PyObject::bool_val(args[0].is_callable()))
}

pub(super) fn builtin_input(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if !args.is_empty() {
        print!("{}", args[0].py_to_string());
        use std::io::Write;
        std::io::stdout().flush().ok();
    }
    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf).map_err(|e|
        PyException::runtime_error(format!("input error: {}", e))
    )?;
    if buf.ends_with('\n') { buf.pop(); }
    if buf.ends_with('\r') { buf.pop(); }
    Ok(PyObject::str_val(CompactString::from(buf)))
}

pub(super) fn builtin_ord(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("ord", args, 1)?;
    let s = args[0].as_str().ok_or_else(|| PyException::type_error("ord() expected string"))?;
    let mut chars = s.chars();
    let c = chars.next().ok_or_else(|| PyException::type_error("ord() expected a character"))?;
    if chars.next().is_some() {
        return Err(PyException::type_error("ord() expected a character, but string of length > 1 found"));
    }
    Ok(PyObject::int(c as i64))
}

pub(super) fn builtin_chr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("chr", args, 1)?;
    let n = args[0].as_int().ok_or_else(|| PyException::type_error("chr() expects int"))?;
    let c = char::from_u32(n as u32).ok_or_else(|| PyException::value_error(
        format!("chr() arg not in range(0x110000): {}", n)))?;
    Ok(PyObject::str_val(CompactString::from(c.to_string())))
}

pub(super) fn builtin_hex(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("hex", args, 1)?;
    let n = args[0].as_int().ok_or_else(|| PyException::type_error("hex() expects int"))?;
    Ok(PyObject::str_val(CompactString::from(format!("0x{:x}", n))))
}

pub(super) fn builtin_oct(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("oct", args, 1)?;
    let n = args[0].as_int().ok_or_else(|| PyException::type_error("oct() expects int"))?;
    Ok(PyObject::str_val(CompactString::from(format!("0o{:o}", n))))
}

pub(super) fn builtin_bin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("bin", args, 1)?;
    let n = args[0].as_int().ok_or_else(|| PyException::type_error("bin() expects int"))?;
    Ok(PyObject::str_val(CompactString::from(format!("0b{:b}", n))))
}

pub(super) fn builtin_sorted(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("sorted", args, 1)?;
    let mut items = args[0].to_list()?;
    items.sort_by(|a, b| {
        if let Ok(r) = a.compare(b, ferrython_core::object::CompareOp::Lt) {
            if r.is_truthy() { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater }
        } else {
            std::cmp::Ordering::Equal
        }
    });
    Ok(PyObject::list(items))
}

pub(super) fn builtin_reversed(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("reversed", args, 1)?;
    let mut items = args[0].to_list()?;
    items.reverse();
    Ok(PyObject::wrap(PyObjectPayload::Iterator(
        Arc::new(std::sync::Mutex::new(ferrython_core::object::IteratorData::List { items, index: 0 }))
    )))
}

pub(super) fn builtin_enumerate(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("enumerate", args, 1)?;
    let start = if args.len() > 1 {
        args[1].as_int().unwrap_or(0)
    } else { 0 };
    // Get an iterator from the source
    let source = get_iter_from_obj(&args[0])?;
    Ok(PyObject::wrap(PyObjectPayload::Iterator(
        Arc::new(std::sync::Mutex::new(IteratorData::Enumerate { source, index: start }))
    )))
}

pub(super) fn builtin_zip(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(
            Arc::new(std::sync::Mutex::new(IteratorData::List { items: vec![], index: 0 }))
        )));
    }
    let sources: Vec<PyObjectRef> = args.iter()
        .map(|a| get_iter_from_obj(a))
        .collect::<PyResult<Vec<_>>>()?;
    Ok(PyObject::wrap(PyObjectPayload::Iterator(
        Arc::new(std::sync::Mutex::new(IteratorData::Zip { sources }))
    )))
}

/// Get an iterator from any iterable object.
pub(super) fn get_iter_from_obj(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::Iterator(_) | PyObjectPayload::Generator(_) => Ok(obj.clone()),
        PyObjectPayload::Range { start, stop, step } => {
            Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Arc::new(std::sync::Mutex::new(IteratorData::Range { current: *start, stop: *stop, step: *step }))
            )))
        }
        PyObjectPayload::List(items) => {
            let items = items.read().clone();
            Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Arc::new(std::sync::Mutex::new(IteratorData::List { items, index: 0 }))
            )))
        }
        PyObjectPayload::Tuple(items) => {
            Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Arc::new(std::sync::Mutex::new(IteratorData::Tuple { items: items.clone(), index: 0 }))
            )))
        }
        PyObjectPayload::Str(s) => {
            let chars: Vec<char> = s.chars().collect();
            Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Arc::new(std::sync::Mutex::new(IteratorData::Str { chars, index: 0 }))
            )))
        }
        PyObjectPayload::Set(m) => {
            let items: Vec<PyObjectRef> = m.read().values().cloned().collect();
            Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Arc::new(std::sync::Mutex::new(IteratorData::List { items, index: 0 }))
            )))
        }
        PyObjectPayload::Dict(m) => {
            let items: Vec<PyObjectRef> = m.read().keys().map(|k| k.to_object()).collect();
            Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Arc::new(std::sync::Mutex::new(IteratorData::List { items, index: 0 }))
            )))
        }
        PyObjectPayload::Instance(_) => {
            // Check for __iter__ method — return the object itself as it likely has __next__
            if obj.get_attr("__iter__").is_some() || obj.get_attr("__next__").is_some() {
                Ok(obj.clone())
            } else {
                Err(PyException::type_error(format!("'{}' object is not iterable", obj.type_name())))
            }
        }
        _ => Err(PyException::type_error(format!("'{}' object is not iterable", obj.type_name()))),
    }
}

pub(super) fn builtin_range(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let (start, stop, step) = match args.len() {
        1 => {
            let stop = args[0].as_int().ok_or_else(||
                PyException::type_error("range() integer expected"))?;
            (0i64, stop, 1i64)
        }
        2 => {
            let start = args[0].as_int().ok_or_else(||
                PyException::type_error("range() integer expected"))?;
            let stop = args[1].as_int().ok_or_else(||
                PyException::type_error("range() integer expected"))?;
            (start, stop, 1)
        }
        3 => {
            let start = args[0].as_int().ok_or_else(||
                PyException::type_error("range() integer expected"))?;
            let stop = args[1].as_int().ok_or_else(||
                PyException::type_error("range() integer expected"))?;
            let step = args[2].as_int().ok_or_else(||
                PyException::type_error("range() integer expected"))?;
            if step == 0 {
                return Err(PyException::value_error("range() arg 3 must not be zero"));
            }
            (start, stop, step)
        }
        _ => return Err(PyException::type_error("range expected 1 to 3 arguments")),
    };
    Ok(PyObject::range(start, stop, step))
}

pub(super) fn builtin_list(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::list(vec![]));
    }
    let items = args[0].to_list()?;
    Ok(PyObject::list(items))
}

pub(super) fn builtin_tuple(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::tuple(vec![]));
    }
    let items = args[0].to_list()?;
    Ok(PyObject::tuple(items))
}

pub(super) fn builtin_dict(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::dict(IndexMap::new()));
    }
    match &args[0].payload {
        PyObjectPayload::Dict(m) => {
            let mut new_map = m.read().clone();
            new_map.swap_remove(&HashableKey::Str(CompactString::from("__defaultdict_factory__")));
            Ok(PyObject::dict(new_map))
        },
        // dict from iterable of (key, value) pairs
        PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) | PyObjectPayload::Iterator(_) | PyObjectPayload::Set(_) => {
            let pairs = args[0].to_list()?;
            let mut map = IndexMap::new();
            for pair in &pairs {
                let kv = pair.to_list()?;
                if kv.len() != 2 {
                    return Err(PyException::value_error(
                        format!("dictionary update sequence element has length {}; 2 is required", kv.len())));
                }
                let key = kv[0].to_hashable_key()?;
                map.insert(key, kv[1].clone());
            }
            Ok(PyObject::dict(map))
        }
        _ => Err(PyException::type_error("dict() argument must be a mapping or iterable")),
    }
}

pub(super) fn builtin_set(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::set(IndexMap::new()));
    }
    let items = args[0].to_list()?;
    let mut set = IndexMap::new();
    for item in items {
        if let Ok(key) = item.to_hashable_key() {
            set.insert(key, item);
        }
    }
    Ok(PyObject::set(set))
}

pub(super) fn builtin_frozenset(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::frozenset(IndexMap::new()));
    }
    let items = args[0].to_list()?;
    let mut set = IndexMap::new();
    for item in items {
        if let Ok(key) = item.to_hashable_key() {
            set.insert(key, item);
        }
    }
    Ok(PyObject::frozenset(set))
}

pub(super) fn builtin_all(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("all", args, 1)?;
    let items = args[0].to_list()?;
    for item in items {
        if !item.is_truthy() { return Ok(PyObject::bool_val(false)); }
    }
    Ok(PyObject::bool_val(true))
}

pub(super) fn builtin_any(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("any", args, 1)?;
    let items = args[0].to_list()?;
    for item in items {
        if item.is_truthy() { return Ok(PyObject::bool_val(true)); }
    }
    Ok(PyObject::bool_val(false))
}

pub(super) fn builtin_iter(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() == 2 {
        // iter(callable, sentinel) — creates a lazy sentinel iterator
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(
            Arc::new(std::sync::Mutex::new(IteratorData::Sentinel {
                callable: args[0].clone(),
                sentinel: args[1].clone(),
            }))
        )));
    }
    check_args("iter", args, 1)?;
    args[0].get_iter()
}

pub(super) fn builtin_next(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("next", args, 1)?;
    match iter_advance(&args[0])? {
        Some((_new_iter, value)) => Ok(value),
        None => {
            if args.len() > 1 {
                Ok(args[1].clone())
            } else {
                Err(PyException::stop_iteration())
            }
        }
    }
}

pub(super) fn builtin_hasattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("hasattr", args, 2)?;
    let name = args[1].as_str().ok_or_else(||
        PyException::type_error("hasattr(): attribute name must be string"))?;
    Ok(PyObject::bool_val(args[0].get_attr(name).is_some()))
}

pub(super) fn builtin_getattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("getattr", args, 2)?;
    let name = args[1].as_str().ok_or_else(||
        PyException::type_error("getattr(): attribute name must be string"))?;
    match args[0].get_attr(name) {
        Some(v) => Ok(v),
        None => {
            if args.len() > 2 {
                Ok(args[2].clone())
            } else {
                Err(PyException::attribute_error(format!(
                    "'{}' object has no attribute '{}'", args[0].type_name(), name
                )))
            }
        }
    }
}

pub(crate) fn builtin_dir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::list(vec![]));
    }
    let names = args[0].dir();
    let items: Vec<PyObjectRef> = names.into_iter().map(|n| PyObject::str_val(n)).collect();
    Ok(PyObject::list(items))
}

pub(super) fn builtin_format(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("format", args, 1)?;
    if args.len() >= 2 {
        let spec = args[1].py_to_string();
        if !spec.is_empty() {
            return args[0].format_value(&spec).map(|s| PyObject::str_val(CompactString::from(s)));
        }
    }
    Ok(PyObject::str_val(CompactString::from(args[0].py_to_string())))
}

pub(super) fn builtin_ascii(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("ascii", args, 1)?;
    let s = args[0].py_to_string();
    let escaped: String = s.chars().map(|c| {
        if c.is_ascii() { c.to_string() }
        else { format!("\\u{:04x}", c as u32) }
    }).collect();
    Ok(PyObject::str_val(CompactString::from(format!("'{}'", escaped))))
}

pub(super) fn builtin_property(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let fget = args.first().cloned();
    let fset = args.get(1).cloned();
    let fdel = args.get(2).cloned();
    Ok(Arc::new(PyObject {
        payload: PyObjectPayload::Property { fget, fset, fdel },
    }))
}

pub(super) fn builtin_staticmethod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("staticmethod", args, 1)?;
    Ok(Arc::new(PyObject {
        payload: PyObjectPayload::StaticMethod(args[0].clone()),
    }))
}

pub(super) fn builtin_classmethod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("classmethod", args, 1)?;
    Ok(Arc::new(PyObject {
        payload: PyObjectPayload::ClassMethod(args[0].clone()),
    }))
}

pub(super) fn builtin_setattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() != 3 {
        return Err(PyException::type_error("setattr() takes exactly 3 arguments"));
    }
    let name = args[1].py_to_string();
    match &args[0].payload {
        PyObjectPayload::Instance(inst) => {
            inst.attrs.write().insert(CompactString::from(name), args[2].clone());
        }
        PyObjectPayload::Class(cd) => {
            cd.namespace.write().insert(CompactString::from(name), args[2].clone());
        }
        PyObjectPayload::Module(_m) => {
            // Modules are immutable in our current design; skip for now
        }
        _ => return Err(PyException::attribute_error(format!(
            "'{}' object does not support attribute assignment", args[0].type_name()
        ))),
    }
    Ok(PyObject::none())
}

pub(super) fn builtin_delattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("delattr", args, 2)?;
    let name = args[1].py_to_string();
    match &args[0].payload {
        PyObjectPayload::Instance(inst) => {
            inst.attrs.write().shift_remove(name.as_str());
        }
        _ => return Err(PyException::attribute_error(format!(
            "'{}' object does not support attribute deletion", args[0].type_name()
        ))),
    }
    Ok(PyObject::none())
}

pub(super) fn builtin_vars(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::dict_from_pairs(vec![]));
    }
    match &args[0].payload {
        PyObjectPayload::Instance(inst) => {
            let attrs = inst.attrs.read();
            let pairs: Vec<(PyObjectRef, PyObjectRef)> = attrs.iter()
                .map(|(k, v)| (PyObject::str_val(k.clone()), v.clone()))
                .collect();
            Ok(PyObject::dict_from_pairs(pairs))
        }
        PyObjectPayload::Class(cd) => {
            let ns = cd.namespace.read();
            let pairs: Vec<(PyObjectRef, PyObjectRef)> = ns.iter()
                .map(|(k, v)| (PyObject::str_val(k.clone()), v.clone()))
                .collect();
            Ok(PyObject::dict_from_pairs(pairs))
        }
        PyObjectPayload::Module(md) => {
            let pairs: Vec<(PyObjectRef, PyObjectRef)> = md.attrs.iter()
                .map(|(k, v)| (PyObject::str_val(k.clone()), v.clone()))
                .collect();
            Ok(PyObject::dict_from_pairs(pairs))
        }
        _ => Err(PyException::type_error("vars() argument must have __dict__ attribute")),
    }
}

pub(super) fn builtin_globals(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::dict_from_pairs(vec![]))
}

pub(super) fn builtin_locals(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::dict_from_pairs(vec![]))
}

pub(super) fn builtin_slice(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let to_opt = |a: &PyObjectRef| -> Option<PyObjectRef> {
        if matches!(a.payload, PyObjectPayload::None) { None } else { Some(a.clone()) }
    };
    match args.len() {
        0 => Err(PyException::type_error("slice expected at least 1 argument, got 0")),
        1 => Ok(PyObject::slice(None, to_opt(&args[0]), None)),
        2 => Ok(PyObject::slice(to_opt(&args[0]), to_opt(&args[1]), None)),
        _ => Ok(PyObject::slice(to_opt(&args[0]), to_opt(&args[1]), to_opt(&args[2]))),
    }
}

pub(super) fn builtin_bytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::bytes(vec![]));
    }
    match &args[0].payload {
        PyObjectPayload::Bytes(b) => Ok(PyObject::bytes(b.clone())),
        PyObjectPayload::ByteArray(b) => Ok(PyObject::bytes(b.clone())),
        PyObjectPayload::Str(s) => {
            // bytes(string, encoding) — require encoding argument
            if args.len() >= 2 {
                Ok(PyObject::bytes(s.as_bytes().to_vec()))
            } else {
                Err(PyException::type_error("string argument without an encoding"))
            }
        }
        PyObjectPayload::Int(n) => {
            let size = n.to_i64().unwrap_or(0) as usize;
            Ok(PyObject::bytes(vec![0u8; size]))
        }
        PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) => {
            let items = args[0].to_list()?;
            let mut result = Vec::with_capacity(items.len());
            for item in items {
                let v = item.to_int().map_err(|_| PyException::type_error("an integer is required"))?;
                if v < 0 || v > 255 {
                    return Err(PyException::value_error("bytes must be in range(0, 256)"));
                }
                result.push(v as u8);
            }
            Ok(PyObject::bytes(result))
        }
        _ => Err(PyException::type_error("cannot convert to bytes")),
    }
}

pub(super) fn builtin_bytearray(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::bytearray(vec![]));
    }
    match &args[0].payload {
        PyObjectPayload::Bytes(b) => Ok(PyObject::bytearray(b.clone())),
        PyObjectPayload::ByteArray(b) => Ok(PyObject::bytearray(b.clone())),
        PyObjectPayload::Str(s) => {
            if args.len() >= 2 {
                Ok(PyObject::bytearray(s.as_bytes().to_vec()))
            } else {
                Err(PyException::type_error("string argument without an encoding"))
            }
        }
        PyObjectPayload::Int(n) => {
            let size = n.to_i64().unwrap_or(0) as usize;
            Ok(PyObject::bytearray(vec![0u8; size]))
        }
        PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) => {
            let items = args[0].to_list()?;
            let mut result = Vec::with_capacity(items.len());
            for item in items {
                let v = item.to_int().map_err(|_| PyException::type_error("an integer is required"))?;
                if v < 0 || v > 255 {
                    return Err(PyException::value_error("bytes must be in range(0, 256)"));
                }
                result.push(v as u8);
            }
            Ok(PyObject::bytearray(result))
        }
        _ => Err(PyException::type_error("cannot convert to bytearray")),
    }
}

pub(super) fn builtin_complex(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let real = if !args.is_empty() { args[0].to_float().unwrap_or(0.0) } else { 0.0 };
    let imag = if args.len() > 1 { args[1].to_float().unwrap_or(0.0) } else { 0.0 };
    Ok(PyObject::complex(real, imag))
}

pub(super) fn builtin_issubclass(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("issubclass", args, 2)?;
    let sub = &args[0];
    let sup = &args[1];
    // Handle tuple of types: issubclass(A, (B, C))
    if let PyObjectPayload::Tuple(types) = &sup.payload {
        for t in types {
            if check_subclass(sub, t) {
                return Ok(PyObject::bool_val(true));
            }
        }
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(check_subclass(sub, sup)))
}

pub(crate) fn check_subclass(sub: &PyObjectRef, sup: &PyObjectRef) -> bool {
    match (&sub.payload, &sup.payload) {
        (PyObjectPayload::Class(sub_cd), PyObjectPayload::Class(sup_cd)) => {
            if sub_cd.name == sup_cd.name { return true; }
            // Walk full MRO
            for base in &sub_cd.mro {
                if let PyObjectPayload::Class(bc) = &base.payload {
                    if bc.name == sup_cd.name { return true; }
                }
            }
            // Also check direct bases
            for base in &sub_cd.bases {
                if let PyObjectPayload::Class(bc) = &base.payload {
                    if bc.name == sup_cd.name { return true; }
                }
            }
            false
        }
        // Class inheriting from ExceptionType (e.g. class MyError(ValueError))
        (PyObjectPayload::Class(sub_cd), PyObjectPayload::ExceptionType(target_kind)) => {
            let _target_name = format!("{:?}", target_kind);
            // Check bases: is any base an ExceptionType matching target?
            for base in &sub_cd.bases {
                if let PyObjectPayload::ExceptionType(bk) = &base.payload {
                    if bk == target_kind { return true; }
                    // Check exception hierarchy
                    if is_exception_subclass(bk, target_kind) { return true; }
                }
                // Recursively check class bases
                if check_subclass(base, sup) { return true; }
            }
            false
        }
        (PyObjectPayload::ExceptionType(a), PyObjectPayload::ExceptionType(b)) => {
            a == b || is_exception_subclass(a, b)
        }
        // BuiltinType subclass (bool is subclass of int)
        (PyObjectPayload::BuiltinType(a), PyObjectPayload::BuiltinType(b)) => {
            a == b
            || (a.as_str() == "bool" && b.as_str() == "int")
            || b.as_str() == "object"  // everything is a subclass of object
        }
        // Any type is subclass of object
        (_, PyObjectPayload::BuiltinType(b)) if b.as_str() == "object" => true,
        _ => false,
    }
}

/// Check if exception kind `child` is a subclass of `parent` in the hierarchy.
pub(crate) fn is_exception_subclass(child: &ExceptionKind, parent: &ExceptionKind) -> bool {
    if std::mem::discriminant(child) == std::mem::discriminant(parent) { return true; }
    match parent {
        ExceptionKind::BaseException => true,
        ExceptionKind::Exception => !matches!(child,
            ExceptionKind::SystemExit | ExceptionKind::KeyboardInterrupt | ExceptionKind::GeneratorExit
        ),
        ExceptionKind::ArithmeticError => matches!(child,
            ExceptionKind::ArithmeticError | ExceptionKind::FloatingPointError |
            ExceptionKind::OverflowError | ExceptionKind::ZeroDivisionError
        ),
        ExceptionKind::LookupError => matches!(child,
            ExceptionKind::LookupError | ExceptionKind::IndexError | ExceptionKind::KeyError
        ),
        ExceptionKind::OSError => matches!(child,
            ExceptionKind::OSError | ExceptionKind::FileExistsError |
            ExceptionKind::FileNotFoundError | ExceptionKind::PermissionError
        ),
        ExceptionKind::ValueError => matches!(child,
            ExceptionKind::ValueError | ExceptionKind::UnicodeError |
            ExceptionKind::UnicodeDecodeError | ExceptionKind::UnicodeEncodeError
        ),
        ExceptionKind::Warning => matches!(child,
            ExceptionKind::Warning | ExceptionKind::DeprecationWarning |
            ExceptionKind::RuntimeWarning | ExceptionKind::UserWarning
        ),
        ExceptionKind::ImportError => matches!(child,
            ExceptionKind::ImportError | ExceptionKind::ModuleNotFoundError
        ),
        _ => false,
    }
}

pub(super) fn builtin_object(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::instance(PyObject::class(
        CompactString::from("object"),
        vec![],
        IndexMap::new(),
    )))
}

pub(super) fn builtin_super(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Simplified: return None for now
    Ok(PyObject::none())
}

/// dict.fromkeys(iterable, value=None) — create dict with keys from iterable
pub(super) fn builtin_dict_fromkeys(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("dict.fromkeys", args, 1)?;
    let iterable = &args[0];
    let value = if args.len() >= 2 { args[1].clone() } else { PyObject::none() };
    let mut map = IndexMap::new();
    match &iterable.payload {
        PyObjectPayload::List(items) => {
            for item in items.read().iter() {
                let hk = item.to_hashable_key()?;
                map.insert(hk, value.clone());
            }
        }
        PyObjectPayload::Tuple(items) => {
            for item in items.iter() {
                let hk = item.to_hashable_key()?;
                map.insert(hk, value.clone());
            }
        }
        PyObjectPayload::Set(items) => {
            for (item, _) in items.read().iter() {
                map.insert(item.clone(), value.clone());
            }
        }
        PyObjectPayload::Str(s) => {
            for ch in s.chars() {
                let hk = HashableKey::Str(CompactString::from(ch.to_string()));
                map.insert(hk, value.clone());
            }
        }
        PyObjectPayload::Dict(d) => {
            for key in d.read().keys() {
                map.insert(key.clone(), value.clone());
            }
        }
        _ => {
            return Err(PyException::type_error(format!(
                "'{}' object is not iterable", iterable.type_name()
            )));
        }
    }
    Ok(PyObject::dict(map))
}
