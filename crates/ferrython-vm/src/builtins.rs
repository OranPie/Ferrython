//! Built-in functions available in Python's builtins module.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

// ── Builtin registry ──

type BuiltinFn = fn(&[PyObjectRef]) -> PyResult<PyObjectRef>;

pub fn init_builtins() -> IndexMap<CompactString, PyObjectRef> {
    let mut m = IndexMap::new();
    let names = [
        "print", "len", "repr", "str", "int", "float", "bool", "type", "id",
        "abs", "min", "max", "sum", "round", "pow", "divmod", "hash",
        "isinstance", "issubclass", "callable", "input", "ord", "chr",
        "hex", "oct", "bin", "sorted", "reversed", "enumerate", "zip",
        "range", "list", "tuple", "dict", "set", "frozenset",
        "all", "any", "iter", "next", "hasattr", "getattr", "setattr",
        "delattr", "dir", "vars", "globals", "locals", "format",
        "ascii", "exec", "eval", "compile", "help", "breakpoint",
        "object", "super", "classmethod", "staticmethod", "property",
        "map", "filter", "slice",
    ];
    for name in names {
        m.insert(
            CompactString::from(name),
            PyObject::builtin_function(CompactString::from(name)),
        );
    }
    m.insert(CompactString::from("None"), PyObject::none());
    m.insert(CompactString::from("True"), PyObject::bool_val(true));
    m.insert(CompactString::from("False"), PyObject::bool_val(false));
    m.insert(CompactString::from("Ellipsis"), PyObject::ellipsis());
    m.insert(CompactString::from("NotImplemented"), PyObject::not_implemented());
    m
}

pub fn get_builtin_fn(name: &str) -> Option<BuiltinFn> {
    match name {
        "print" => Some(builtin_print),
        "len" => Some(builtin_len),
        "repr" => Some(builtin_repr),
        "str" => Some(builtin_str),
        "int" => Some(builtin_int),
        "float" => Some(builtin_float),
        "bool" => Some(builtin_bool),
        "type" => Some(builtin_type),
        "id" => Some(builtin_id),
        "abs" => Some(builtin_abs),
        "min" => Some(builtin_min),
        "max" => Some(builtin_max),
        "sum" => Some(builtin_sum),
        "round" => Some(builtin_round),
        "pow" => Some(builtin_pow),
        "divmod" => Some(builtin_divmod),
        "hash" => Some(builtin_hash),
        "isinstance" => Some(builtin_isinstance),
        "callable" => Some(builtin_callable),
        "input" => Some(builtin_input),
        "ord" => Some(builtin_ord),
        "chr" => Some(builtin_chr),
        "hex" => Some(builtin_hex),
        "oct" => Some(builtin_oct),
        "bin" => Some(builtin_bin),
        "sorted" => Some(builtin_sorted),
        "reversed" => Some(builtin_reversed),
        "enumerate" => Some(builtin_enumerate),
        "zip" => Some(builtin_zip),
        "range" => Some(builtin_range),
        "list" => Some(builtin_list),
        "tuple" => Some(builtin_tuple),
        "dict" => Some(builtin_dict),
        "set" => Some(builtin_set),
        "frozenset" => Some(builtin_frozenset),
        "all" => Some(builtin_all),
        "any" => Some(builtin_any),
        "iter" => Some(builtin_iter),
        "next" => Some(builtin_next),
        "hasattr" => Some(builtin_hasattr),
        "getattr" => Some(builtin_getattr),
        "dir" => Some(builtin_dir),
        "format" => Some(builtin_format),
        "ascii" => Some(builtin_ascii),
        _ => None,
    }
}

// ── Iterator helpers (used by VM for FOR_ITER) ──

/// Advance an iterator by one step. Returns (new_iterator, value) or None if exhausted.
pub fn iter_advance(iter_obj: &PyObjectRef) -> PyResult<Option<(PyObjectRef, PyObjectRef)>> {
    match &iter_obj.payload {
        PyObjectPayload::Iterator(data) => {
            use ferrython_core::object::IteratorData;
            match data {
                IteratorData::List { items, index } => {
                    if *index < items.len() {
                        let v = items[*index].clone();
                        let d = IteratorData::List { items: items.clone(), index: index + 1 };
                        Ok(Some((PyObject::wrap(PyObjectPayload::Iterator(d)), v)))
                    } else { Ok(None) }
                }
                IteratorData::Tuple { items, index } => {
                    if *index < items.len() {
                        let v = items[*index].clone();
                        let d = IteratorData::Tuple { items: items.clone(), index: index + 1 };
                        Ok(Some((PyObject::wrap(PyObjectPayload::Iterator(d)), v)))
                    } else { Ok(None) }
                }
                IteratorData::Range { current, stop, step } => {
                    let done = if *step > 0 { *current >= *stop } else { *current <= *stop };
                    if done { Ok(None) } else {
                        let v = PyObject::int(*current);
                        let d = IteratorData::Range { current: current + step, stop: *stop, step: *step };
                        Ok(Some((PyObject::wrap(PyObjectPayload::Iterator(d)), v)))
                    }
                }
                IteratorData::Str { chars, index } => {
                    if *index < chars.len() {
                        let v = PyObject::str_val(CompactString::from(chars[*index].to_string()));
                        let d = IteratorData::Str { chars: chars.clone(), index: index + 1 };
                        Ok(Some((PyObject::wrap(PyObjectPayload::Iterator(d)), v)))
                    } else { Ok(None) }
                }
            }
        }
        _ => Err(PyException::type_error("iter_advance on non-iterator")),
    }
}

fn hashable_key_to_object(key: &HashableKey) -> PyObjectRef { key.to_object() }

// ── Builtin function implementations ──

fn builtin_print(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let parts: Vec<String> = args.iter().map(|a| a.py_to_string()).collect();
    println!("{}", parts.join(" "));
    Ok(PyObject::none())
}

fn builtin_len(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("len", args, 1)?;
    let n = args[0].py_len()?;
    Ok(PyObject::int(n as i64))
}

fn builtin_repr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("repr", args, 1)?;
    Ok(PyObject::str_val(CompactString::from(args[0].repr())))
}

fn builtin_str(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::str_val(CompactString::from("")));
    }
    Ok(PyObject::str_val(CompactString::from(args[0].py_to_string())))
}

fn builtin_int(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::int(0));
    }
    Ok(PyObject::int(args[0].to_int()?))
}

fn builtin_float(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::float(0.0));
    }
    Ok(PyObject::float(args[0].to_float()?))
}

fn builtin_bool(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::bool_val(false));
    }
    Ok(PyObject::bool_val(args[0].is_truthy()))
}

fn builtin_type(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("type", args, 1)?;
    Ok(PyObject::str_val(CompactString::from(args[0].type_name())))
}

fn builtin_id(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("id", args, 1)?;
    let ptr = std::sync::Arc::as_ptr(&args[0]) as usize;
    Ok(PyObject::int(ptr as i64))
}

fn builtin_abs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("abs", args, 1)?;
    args[0].py_abs()
}

fn builtin_min(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn builtin_max(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn builtin_sum(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn builtin_round(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("round", args, 1)?;
    match &args[0].payload {
        PyObjectPayload::Int(_) => Ok(args[0].clone()),
        PyObjectPayload::Float(f) => Ok(PyObject::int(f.round() as i64)),
        _ => Err(PyException::type_error("type has no round()")),
    }
}

fn builtin_pow(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("pow", args, 2)?;
    args[0].power(&args[1])
}

fn builtin_divmod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("divmod", args, 2)?;
    let q = args[0].floor_div(&args[1])?;
    let r = args[0].modulo(&args[1])?;
    Ok(PyObject::tuple(vec![q, r]))
}

fn builtin_hash(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("hash", args, 1)?;
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
        HashableKey::Bytes(b) => { let mut h: u64 = 5381; for x in b { h = h.wrapping_mul(33).wrapping_add(x as u64); } h as i64 }
    };
    Ok(PyObject::int(h))
}

fn builtin_isinstance(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("isinstance", args, 2)?;
    let type_name_obj = args[1].py_to_string();
    let obj_type = args[0].type_name();
    Ok(PyObject::bool_val(obj_type == type_name_obj))
}

fn builtin_callable(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("callable", args, 1)?;
    Ok(PyObject::bool_val(args[0].is_callable()))
}

fn builtin_input(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn builtin_ord(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("ord", args, 1)?;
    let s = args[0].as_str().ok_or_else(|| PyException::type_error("ord() expected string"))?;
    let mut chars = s.chars();
    let c = chars.next().ok_or_else(|| PyException::type_error("ord() expected a character"))?;
    if chars.next().is_some() {
        return Err(PyException::type_error("ord() expected a character, but string of length > 1 found"));
    }
    Ok(PyObject::int(c as i64))
}

fn builtin_chr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("chr", args, 1)?;
    let n = args[0].as_int().ok_or_else(|| PyException::type_error("chr() expects int"))?;
    let c = char::from_u32(n as u32).ok_or_else(|| PyException::value_error(
        format!("chr() arg not in range(0x110000): {}", n)))?;
    Ok(PyObject::str_val(CompactString::from(c.to_string())))
}

fn builtin_hex(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("hex", args, 1)?;
    let n = args[0].as_int().ok_or_else(|| PyException::type_error("hex() expects int"))?;
    Ok(PyObject::str_val(CompactString::from(format!("0x{:x}", n))))
}

fn builtin_oct(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("oct", args, 1)?;
    let n = args[0].as_int().ok_or_else(|| PyException::type_error("oct() expects int"))?;
    Ok(PyObject::str_val(CompactString::from(format!("0o{:o}", n))))
}

fn builtin_bin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("bin", args, 1)?;
    let n = args[0].as_int().ok_or_else(|| PyException::type_error("bin() expects int"))?;
    Ok(PyObject::str_val(CompactString::from(format!("0b{:b}", n))))
}

fn builtin_sorted(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn builtin_reversed(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("reversed", args, 1)?;
    let mut items = args[0].to_list()?;
    items.reverse();
    Ok(PyObject::list(items))
}

fn builtin_enumerate(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("enumerate", args, 1)?;
    let items = args[0].to_list()?;
    let start = if args.len() > 1 {
        args[1].as_int().unwrap_or(0)
    } else { 0 };
    let result: Vec<PyObjectRef> = items.into_iter().enumerate().map(|(i, v)| {
        PyObject::tuple(vec![PyObject::int(start + i as i64), v])
    }).collect();
    Ok(PyObject::list(result))
}

fn builtin_zip(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::list(vec![]));
    }
    let lists: Vec<Vec<PyObjectRef>> = args.iter()
        .map(|a| a.to_list())
        .collect::<PyResult<Vec<_>>>()?;
    let min_len = lists.iter().map(|l| l.len()).min().unwrap_or(0);
    let mut result = Vec::with_capacity(min_len);
    for i in 0..min_len {
        let tuple: Vec<PyObjectRef> = lists.iter().map(|l| l[i].clone()).collect();
        result.push(PyObject::tuple(tuple));
    }
    Ok(PyObject::list(result))
}

fn builtin_range(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn builtin_list(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::list(vec![]));
    }
    let items = args[0].to_list()?;
    Ok(PyObject::list(items))
}

fn builtin_tuple(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::tuple(vec![]));
    }
    let items = args[0].to_list()?;
    Ok(PyObject::tuple(items))
}

fn builtin_dict(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::dict(IndexMap::new()));
    }
    // Simple: copy a dict
    match &args[0].payload {
        PyObjectPayload::Dict(m) => Ok(PyObject::dict(m.clone())),
        _ => Err(PyException::type_error("dict() argument must be a mapping")),
    }
}

fn builtin_set(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn builtin_frozenset(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn builtin_all(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("all", args, 1)?;
    let items = args[0].to_list()?;
    for item in items {
        if !item.is_truthy() { return Ok(PyObject::bool_val(false)); }
    }
    Ok(PyObject::bool_val(true))
}

fn builtin_any(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("any", args, 1)?;
    let items = args[0].to_list()?;
    for item in items {
        if item.is_truthy() { return Ok(PyObject::bool_val(true)); }
    }
    Ok(PyObject::bool_val(false))
}

fn builtin_iter(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("iter", args, 1)?;
    args[0].get_iter()
}

fn builtin_next(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn builtin_hasattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("hasattr", args, 2)?;
    let name = args[1].as_str().ok_or_else(||
        PyException::type_error("hasattr(): attribute name must be string"))?;
    Ok(PyObject::bool_val(args[0].get_attr(name).is_some()))
}

fn builtin_getattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn builtin_dir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::list(vec![]));
    }
    let names = args[0].dir();
    let items: Vec<PyObjectRef> = names.into_iter().map(|n| PyObject::str_val(n)).collect();
    Ok(PyObject::list(items))
}

fn builtin_format(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("format", args, 1)?;
    Ok(PyObject::str_val(CompactString::from(args[0].py_to_string())))
}

fn builtin_ascii(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("ascii", args, 1)?;
    let s = args[0].py_to_string();
    let escaped: String = s.chars().map(|c| {
        if c.is_ascii() { c.to_string() }
        else { format!("\\u{:04x}", c as u32) }
    }).collect();
    Ok(PyObject::str_val(CompactString::from(format!("'{}'", escaped))))
}

// ── Argument checking helpers ──

fn check_args(name: &str, args: &[PyObjectRef], expected: usize) -> PyResult<()> {
    if args.len() != expected {
        Err(PyException::type_error(format!(
            "{}() takes exactly {} argument(s) ({} given)", name, expected, args.len()
        )))
    } else { Ok(()) }
}

fn check_args_min(name: &str, args: &[PyObjectRef], min: usize) -> PyResult<()> {
    if args.len() < min {
        Err(PyException::type_error(format!(
            "{}() takes at least {} argument(s) ({} given)", name, min, args.len()
        )))
    } else { Ok(()) }
}
