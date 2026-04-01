//! Built-in functions available in Python's builtins module.

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::{HashableKey, PyInt};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

// ── Builtin registry ──

type BuiltinFn = fn(&[PyObjectRef]) -> PyResult<PyObjectRef>;

pub fn init_builtins() -> IndexMap<CompactString, PyObjectRef> {
    let mut m = IndexMap::new();
    // Regular builtin functions
    let func_names = [
        "print", "len", "repr", "id",
        "abs", "min", "max", "sum", "round", "pow", "divmod", "hash",
        "isinstance", "issubclass", "callable", "input", "ord", "chr",
        "hex", "oct", "bin", "sorted", "reversed", "enumerate", "zip",
        "all", "any", "iter", "next", "hasattr", "getattr", "setattr",
        "delattr", "dir", "vars", "globals", "locals", "format",
        "ascii", "exec", "eval", "compile", "help", "breakpoint",
        "open",
    ];
    for name in func_names {
        m.insert(
            CompactString::from(name),
            PyObject::builtin_function(CompactString::from(name)),
        );
    }
    // Builtin types (constructors that also serve as type objects)
    let type_names = [
        "str", "int", "float", "bool", "type", "object",
        "list", "tuple", "dict", "set", "frozenset", "range",
        "bytes", "bytearray", "complex", "slice",
        "super", "classmethod", "staticmethod", "property",
        "map", "filter",
    ];
    for name in type_names {
        m.insert(
            CompactString::from(name),
            PyObject::builtin_type(CompactString::from(name)),
        );
    }
    m.insert(CompactString::from("None"), PyObject::none());
    m.insert(CompactString::from("True"), PyObject::bool_val(true));
    m.insert(CompactString::from("False"), PyObject::bool_val(false));
    m.insert(CompactString::from("Ellipsis"), PyObject::ellipsis());
    m.insert(CompactString::from("NotImplemented"), PyObject::not_implemented());

    // Exception types
    use ferrython_core::error::ExceptionKind;
    let exc_types = [
        ("BaseException", ExceptionKind::BaseException),
        ("Exception", ExceptionKind::Exception),
        ("ArithmeticError", ExceptionKind::ArithmeticError),
        ("AssertionError", ExceptionKind::AssertionError),
        ("AttributeError", ExceptionKind::AttributeError),
        ("EOFError", ExceptionKind::EOFError),
        ("FileExistsError", ExceptionKind::FileExistsError),
        ("FileNotFoundError", ExceptionKind::FileNotFoundError),
        ("FloatingPointError", ExceptionKind::FloatingPointError),
        ("GeneratorExit", ExceptionKind::GeneratorExit),
        ("ImportError", ExceptionKind::ImportError),
        ("ModuleNotFoundError", ExceptionKind::ModuleNotFoundError),
        ("IndexError", ExceptionKind::IndexError),
        ("KeyError", ExceptionKind::KeyError),
        ("KeyboardInterrupt", ExceptionKind::KeyboardInterrupt),
        ("LookupError", ExceptionKind::LookupError),
        ("MemoryError", ExceptionKind::MemoryError),
        ("NameError", ExceptionKind::NameError),
        ("NotImplementedError", ExceptionKind::NotImplementedError),
        ("OSError", ExceptionKind::OSError),
        ("OverflowError", ExceptionKind::OverflowError),
        ("PermissionError", ExceptionKind::PermissionError),
        ("RecursionError", ExceptionKind::RecursionError),
        ("RuntimeError", ExceptionKind::RuntimeError),
        ("StopIteration", ExceptionKind::StopIteration),
        ("SyntaxError", ExceptionKind::SyntaxError),
        ("SystemError", ExceptionKind::SystemError),
        ("SystemExit", ExceptionKind::SystemExit),
        ("TypeError", ExceptionKind::TypeError),
        ("UnboundLocalError", ExceptionKind::UnboundLocalError),
        ("UnicodeDecodeError", ExceptionKind::UnicodeDecodeError),
        ("UnicodeEncodeError", ExceptionKind::UnicodeEncodeError),
        ("UnicodeError", ExceptionKind::UnicodeError),
        ("ValueError", ExceptionKind::ValueError),
        ("ZeroDivisionError", ExceptionKind::ZeroDivisionError),
        ("Warning", ExceptionKind::Warning),
        ("DeprecationWarning", ExceptionKind::DeprecationWarning),
        ("RuntimeWarning", ExceptionKind::RuntimeWarning),
        ("UserWarning", ExceptionKind::UserWarning),
    ];
    for (name, kind) in exc_types {
        m.insert(CompactString::from(name), PyObject::exception_type(kind));
    }

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
        "open" => Some(builtin_open),
        "property" => Some(builtin_property),
        "staticmethod" => Some(builtin_staticmethod),
        "classmethod" => Some(builtin_classmethod),
        "setattr" => Some(builtin_setattr),
        "delattr" => Some(builtin_delattr),
        "vars" => Some(builtin_vars),
        "globals" => Some(builtin_globals),
        "locals" => Some(builtin_locals),
        "issubclass" => Some(builtin_issubclass),
        "object" => Some(builtin_object),
        "super" => Some(builtin_super),
        "slice" => Some(builtin_slice),
        _ => None,
    }
}

/// Dispatch a builtin function by name (used by VM for pre-processed iterables).
pub fn dispatch(name: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if let Some(f) = get_builtin_fn(name) {
        f(args)
    } else {
        Err(PyException::runtime_error(format!("unknown builtin '{}'", name)))
    }
}

// ── Iterator helpers (used by VM for FOR_ITER) ──

/// Advance an iterator by one step. Returns (new_iterator, value) or None if exhausted.
pub fn iter_advance(iter_obj: &PyObjectRef) -> PyResult<Option<(PyObjectRef, PyObjectRef)>> {
    match &iter_obj.payload {
        PyObjectPayload::Iterator(iter_data) => {
            use ferrython_core::object::IteratorData;
            let mut data = iter_data.lock().unwrap();
            match &mut *data {
                IteratorData::List { items, index } => {
                    if *index < items.len() {
                        let v = items[*index].clone();
                        *index += 1;
                        Ok(Some((iter_obj.clone(), v)))
                    } else { Ok(None) }
                }
                IteratorData::Tuple { items, index } => {
                    if *index < items.len() {
                        let v = items[*index].clone();
                        *index += 1;
                        Ok(Some((iter_obj.clone(), v)))
                    } else { Ok(None) }
                }
                IteratorData::Range { current, stop, step } => {
                    let done = if *step > 0 { *current >= *stop } else { *current <= *stop };
                    if done { Ok(None) } else {
                        let v = PyObject::int(*current);
                        *current += *step;
                        Ok(Some((iter_obj.clone(), v)))
                    }
                }
                IteratorData::Str { chars, index } => {
                    if *index < chars.len() {
                        let v = PyObject::str_val(CompactString::from(chars[*index].to_string()));
                        *index += 1;
                        Ok(Some((iter_obj.clone(), v)))
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
    let name = args[0].type_name();
    match &args[0].payload {
        PyObjectPayload::Instance(inst) => {
            // For instances, return their class
            Ok(inst.class.clone())
        }
        PyObjectPayload::ExceptionInstance { kind, .. } => {
            Ok(PyObject::exception_type(kind.clone()))
        }
        _ => Ok(PyObject::builtin_type(CompactString::from(name)))
    }
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
    let ndigits = if args.len() >= 2 { Some(args[1].to_int()?) } else { None };
    match &args[0].payload {
        PyObjectPayload::Int(_) => Ok(args[0].clone()),
        PyObjectPayload::Float(f) => {
            if let Some(n) = ndigits {
                let factor = 10f64.powi(n as i32);
                Ok(PyObject::float((f * factor).round() / factor))
            } else {
                Ok(PyObject::int(f.round() as i64))
            }
        }
        _ => Err(PyException::type_error("type has no round()")),
    }
}

fn builtin_pow(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("pow", args, 2)?;
    let result = args[0].power(&args[1])?;
    if args.len() >= 3 {
        result.modulo(&args[2])
    } else {
        Ok(result)
    }
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
fn is_instance_of(obj: &PyObjectRef, cls: &PyObjectRef) -> bool {
    match &cls.payload {
        PyObjectPayload::BuiltinFunction(type_name) | PyObjectPayload::BuiltinType(type_name) => {
            let obj_type = obj.type_name();
            if obj_type == type_name.as_str() {
                return true;
            }
            // Built-in subtype relationships: bool is subclass of int
            if type_name.as_str() == "int" && obj_type == "bool" {
                return true;
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
fn class_is_subclass_of(cls: &PyObjectRef, target_name: &str) -> bool {
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
        PyObjectPayload::Dict(m) => Ok(PyObject::dict(m.read().clone())),
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
    if args.len() >= 2 {
        let spec = args[1].py_to_string();
        if !spec.is_empty() {
            return args[0].format_value(&spec).map(|s| PyObject::str_val(CompactString::from(s)));
        }
    }
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

fn builtin_property(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let fget = args.first().cloned();
    let fset = args.get(1).cloned();
    let fdel = args.get(2).cloned();
    Ok(Arc::new(PyObject {
        payload: PyObjectPayload::Property { fget, fset, fdel },
    }))
}

fn builtin_staticmethod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("staticmethod", args, 1)?;
    Ok(Arc::new(PyObject {
        payload: PyObjectPayload::StaticMethod(args[0].clone()),
    }))
}

fn builtin_classmethod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("classmethod", args, 1)?;
    Ok(Arc::new(PyObject {
        payload: PyObjectPayload::ClassMethod(args[0].clone()),
    }))
}

fn builtin_setattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
        PyObjectPayload::Module(m) => {
            // Modules are immutable in our current design; skip for now
        }
        _ => return Err(PyException::attribute_error(format!(
            "'{}' object does not support attribute assignment", args[0].type_name()
        ))),
    }
    Ok(PyObject::none())
}

fn builtin_delattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn builtin_vars(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
        _ => Err(PyException::type_error("vars() argument must have __dict__ attribute")),
    }
}

fn builtin_globals(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::dict_from_pairs(vec![]))
}

fn builtin_locals(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::dict_from_pairs(vec![]))
}

fn builtin_slice(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn builtin_issubclass(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn check_subclass(sub: &PyObjectRef, sup: &PyObjectRef) -> bool {
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
            let target_name = format!("{:?}", target_kind);
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
            a == b || (a.as_str() == "bool" && b.as_str() == "int")
        }
        _ => false,
    }
}

/// Check if exception kind `child` is a subclass of `parent` in the hierarchy.
fn is_exception_subclass(child: &ExceptionKind, parent: &ExceptionKind) -> bool {
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

fn builtin_object(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::instance(PyObject::class(
        CompactString::from("object"),
        vec![],
        IndexMap::new(),
    )))
}

fn builtin_super(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Simplified: return None for now
    Ok(PyObject::none())
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

// ── Built-in type method dispatch ──

pub fn call_method(receiver: &PyObjectRef, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match &receiver.payload {
        PyObjectPayload::Str(s) => call_str_method(s, method, args),
        PyObjectPayload::List(items) => call_list_method(items.clone(), method, args),
        PyObjectPayload::Dict(map) => call_dict_method(map, method, args),
        PyObjectPayload::Int(_) => call_int_method(receiver, method, args),
        PyObjectPayload::Float(f) => call_float_method(*f, method, args),
        PyObjectPayload::Tuple(items) => call_tuple_method(items, method, args),
        PyObjectPayload::Set(m) => call_set_method(m, method, args),
        PyObjectPayload::Bytes(b) => call_bytes_method(b, method, args),
        _ => Err(PyException::attribute_error(format!(
            "'{}' object has no attribute '{}'", receiver.type_name(), method
        ))),
    }
}

fn call_str_method(s: &str, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "upper" => Ok(PyObject::str_val(CompactString::from(s.to_uppercase()))),
        "lower" => Ok(PyObject::str_val(CompactString::from(s.to_lowercase()))),
        "strip" => Ok(PyObject::str_val(CompactString::from(s.trim()))),
        "lstrip" => Ok(PyObject::str_val(CompactString::from(s.trim_start()))),
        "rstrip" => Ok(PyObject::str_val(CompactString::from(s.trim_end()))),
        "split" => {
            let maxsplit: Option<usize> = if args.len() > 1 {
                args[1].as_int().map(|n| n as usize)
            } else { None };
            let parts: Vec<&str> = if args.is_empty() {
                match maxsplit {
                    Some(n) => s.splitn(n + 1, char::is_whitespace).collect(),
                    None => s.split_whitespace().collect(),
                }
            } else if let Some(sep) = args[0].as_str() {
                match maxsplit {
                    Some(n) => s.splitn(n + 1, sep).collect(),
                    None => s.split(sep).collect(),
                }
            } else if matches!(&args[0].payload, PyObjectPayload::None) {
                match maxsplit {
                    Some(n) => s.splitn(n + 1, char::is_whitespace).collect(),
                    None => s.split_whitespace().collect(),
                }
            } else {
                return Err(PyException::type_error("split() argument must be str or None"));
            };
            Ok(PyObject::list(parts.iter().map(|p| PyObject::str_val(CompactString::from(*p))).collect()))
        }
        "rsplit" => {
            let parts: Vec<&str> = if args.is_empty() {
                s.rsplit_terminator(char::is_whitespace).collect()
            } else if let Some(sep) = args[0].as_str() {
                s.rsplit(sep).collect()
            } else {
                return Err(PyException::type_error("rsplit() argument must be str"));
            };
            Ok(PyObject::list(parts.iter().map(|p| PyObject::str_val(CompactString::from(*p))).collect()))
        }
        "join" => {
            check_args_min("join", args, 1)?;
            let items = args[0].to_list()?;
            let strs: Result<Vec<String>, _> = items.iter()
                .map(|x| x.as_str().map(String::from).ok_or_else(||
                    PyException::type_error("sequence item: expected str")))
                .collect();
            Ok(PyObject::str_val(CompactString::from(strs?.join(s))))
        }
        "replace" => {
            check_args_min("replace", args, 2)?;
            let old = args[0].as_str().ok_or_else(|| PyException::type_error("replace() argument 1 must be str"))?;
            let new = args[1].as_str().ok_or_else(|| PyException::type_error("replace() argument 2 must be str"))?;
            if args.len() >= 3 {
                let count = args[2].to_int()? as usize;
                Ok(PyObject::str_val(CompactString::from(s.replacen(old, new, count))))
            } else {
                Ok(PyObject::str_val(CompactString::from(s.replace(old, new))))
            }
        }
        "find" => {
            check_args_min("find", args, 1)?;
            let sub = args[0].as_str().ok_or_else(|| PyException::type_error("find() argument must be str"))?;
            Ok(PyObject::int(s.find(sub).map(|i| i as i64).unwrap_or(-1)))
        }
        "rfind" => {
            check_args_min("rfind", args, 1)?;
            let sub = args[0].as_str().ok_or_else(|| PyException::type_error("rfind() argument must be str"))?;
            Ok(PyObject::int(s.rfind(sub).map(|i| i as i64).unwrap_or(-1)))
        }
        "index" => {
            check_args_min("index", args, 1)?;
            let sub = args[0].as_str().ok_or_else(|| PyException::type_error("index() argument must be str"))?;
            match s.find(sub) {
                Some(i) => Ok(PyObject::int(i as i64)),
                None => Err(PyException::value_error("substring not found")),
            }
        }
        "count" => {
            check_args_min("count", args, 1)?;
            let sub = args[0].as_str().ok_or_else(|| PyException::type_error("count() argument must be str"))?;
            Ok(PyObject::int(s.matches(sub).count() as i64))
        }
        "startswith" => {
            check_args_min("startswith", args, 1)?;
            let prefix = args[0].as_str().ok_or_else(|| PyException::type_error("startswith() argument must be str"))?;
            Ok(PyObject::bool_val(s.starts_with(prefix)))
        }
        "endswith" => {
            check_args_min("endswith", args, 1)?;
            let suffix = args[0].as_str().ok_or_else(|| PyException::type_error("endswith() argument must be str"))?;
            Ok(PyObject::bool_val(s.ends_with(suffix)))
        }
        "isdigit" => Ok(PyObject::bool_val(!s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))),
        "isalpha" => Ok(PyObject::bool_val(!s.is_empty() && s.chars().all(|c| c.is_alphabetic()))),
        "isalnum" => Ok(PyObject::bool_val(!s.is_empty() && s.chars().all(|c| c.is_alphanumeric()))),
        "isspace" => Ok(PyObject::bool_val(!s.is_empty() && s.chars().all(|c| c.is_whitespace()))),
        "isupper" => Ok(PyObject::bool_val(s.chars().any(|c| c.is_uppercase()) && !s.chars().any(|c| c.is_lowercase()))),
        "islower" => Ok(PyObject::bool_val(s.chars().any(|c| c.is_lowercase()) && !s.chars().any(|c| c.is_uppercase()))),
        "title" => {
            let mut result = String::with_capacity(s.len());
            let mut prev_alpha = false;
            for c in s.chars() {
                if c.is_alphabetic() {
                    if prev_alpha { result.extend(c.to_lowercase()); }
                    else { result.extend(c.to_uppercase()); }
                    prev_alpha = true;
                } else {
                    result.push(c);
                    prev_alpha = false;
                }
            }
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "capitalize" => {
            let mut chars = s.chars();
            let result = match chars.next() {
                None => String::new(),
                Some(c) => {
                    let mut r = c.to_uppercase().to_string();
                    for c in chars { r.extend(c.to_lowercase()); }
                    r
                }
            };
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "swapcase" => {
            let result: String = s.chars().map(|c| {
                if c.is_uppercase() { c.to_lowercase().to_string() }
                else if c.is_lowercase() { c.to_uppercase().to_string() }
                else { c.to_string() }
            }).collect();
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "center" => {
            check_args_min("center", args, 1)?;
            let width = args[0].to_int()? as usize;
            let fillchar = if args.len() >= 2 {
                args[1].as_str().and_then(|s| s.chars().next()).unwrap_or(' ')
            } else { ' ' };
            let len = s.chars().count();
            if width <= len { return Ok(PyObject::str_val(CompactString::from(s))); }
            let pad = width - len;
            let left = pad / 2;
            let right = pad - left;
            let result = format!("{}{}{}", fillchar.to_string().repeat(left), s, fillchar.to_string().repeat(right));
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "ljust" => {
            check_args_min("ljust", args, 1)?;
            let width = args[0].to_int()? as usize;
            let fillchar = if args.len() >= 2 {
                args[1].as_str().and_then(|s| s.chars().next()).unwrap_or(' ')
            } else { ' ' };
            let len = s.chars().count();
            if width <= len { return Ok(PyObject::str_val(CompactString::from(s))); }
            let result = format!("{}{}", s, fillchar.to_string().repeat(width - len));
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "rjust" => {
            check_args_min("rjust", args, 1)?;
            let width = args[0].to_int()? as usize;
            let fillchar = if args.len() >= 2 {
                args[1].as_str().and_then(|s| s.chars().next()).unwrap_or(' ')
            } else { ' ' };
            let len = s.chars().count();
            if width <= len { return Ok(PyObject::str_val(CompactString::from(s))); }
            let result = format!("{}{}", fillchar.to_string().repeat(width - len), s);
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "zfill" => {
            check_args_min("zfill", args, 1)?;
            let width = args[0].to_int()? as usize;
            let len = s.len();
            if width <= len { return Ok(PyObject::str_val(CompactString::from(s))); }
            let pad = "0".repeat(width - len);
            if s.starts_with('-') || s.starts_with('+') {
                Ok(PyObject::str_val(CompactString::from(format!("{}{}{}", &s[..1], pad, &s[1..]))))
            } else {
                Ok(PyObject::str_val(CompactString::from(format!("{}{}", pad, s))))
            }
        }
        "expandtabs" => {
            let tabsize = if args.is_empty() { 8 } else { args[0].to_int()? as usize };
            let mut result = String::new();
            let mut col = 0usize;
            for ch in s.chars() {
                if ch == '\t' {
                    let spaces = tabsize - (col % tabsize);
                    result.extend(std::iter::repeat(' ').take(spaces));
                    col += spaces;
                } else if ch == '\n' || ch == '\r' {
                    result.push(ch);
                    col = 0;
                } else {
                    result.push(ch);
                    col += 1;
                }
            }
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "encode" => {
            // Simple UTF-8 encoding
            Ok(PyObject::bytes(s.as_bytes().to_vec()))
        }
        "format" => {
            // Basic positional format: "{} is {}".format(a, b)
            let mut result = String::new();
            let mut chars = s.chars().peekable();
            let mut arg_idx = 0usize;
            while let Some(c) = chars.next() {
                if c == '{' {
                    if chars.peek() == Some(&'{') {
                        chars.next();
                        result.push('{');
                    } else if chars.peek() == Some(&'}') {
                        chars.next();
                        if arg_idx < args.len() {
                            result.push_str(&args[arg_idx].py_to_string());
                            arg_idx += 1;
                        }
                    } else {
                        // Collect field name
                        let mut field = String::new();
                        for c in chars.by_ref() {
                            if c == '}' { break; }
                            field.push(c);
                        }
                        if let Ok(idx) = field.parse::<usize>() {
                            if idx < args.len() {
                                result.push_str(&args[idx].py_to_string());
                            }
                        }
                    }
                } else if c == '}' && chars.peek() == Some(&'}') {
                    chars.next();
                    result.push('}');
                } else {
                    result.push(c);
                }
            }
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        _ => Err(PyException::attribute_error(format!(
            "'str' object has no attribute '{}'", method
        ))),
    }
}

fn call_list_method(items: Arc<RwLock<Vec<PyObjectRef>>>, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "copy" => Ok(PyObject::list(items.read().to_vec())),
        "count" => {
            check_args_min("count", args, 1)?;
            let target = &args[0];
            let c = items.read().iter().filter(|x| x.py_to_string() == target.py_to_string()).count();
            Ok(PyObject::int(c as i64))
        }
        "index" => {
            check_args_min("index", args, 1)?;
            let target = &args[0];
            for (i, x) in items.read().iter().enumerate() {
                if x.py_to_string() == target.py_to_string() {
                    return Ok(PyObject::int(i as i64));
                }
            }
            Err(PyException::value_error("x not in list"))
        }
        "append" => {
            check_args_min("append", args, 1)?;
            items.write().push(args[0].clone());
            Ok(PyObject::none())
        }
        "extend" => {
            check_args_min("extend", args, 1)?;
            let other = args[0].to_list()?;
            items.write().extend(other);
            Ok(PyObject::none())
        }
        "insert" => {
            check_args_min("insert", args, 2)?;
            let idx = args[0].to_int()?;
            let mut w = items.write();
            let len = w.len() as i64;
            let actual = if idx < 0 { (len + idx).max(0) as usize } else { (idx as usize).min(w.len()) };
            w.insert(actual, args[1].clone());
            Ok(PyObject::none())
        }
        "pop" => {
            let mut w = items.write();
            if w.is_empty() {
                return Err(PyException::index_error("pop from empty list"));
            }
            if args.is_empty() {
                Ok(w.pop().unwrap())
            } else {
                let idx = args[0].to_int()?;
                let len = w.len() as i64;
                let actual = if idx < 0 { len + idx } else { idx };
                if actual < 0 || actual >= len {
                    return Err(PyException::index_error("pop index out of range"));
                }
                Ok(w.remove(actual as usize))
            }
        }
        "remove" => {
            check_args_min("remove", args, 1)?;
            let target = &args[0];
            let mut w = items.write();
            let pos = w.iter().position(|x| x.py_to_string() == target.py_to_string());
            match pos {
                Some(i) => { w.remove(i); Ok(PyObject::none()) }
                None => Err(PyException::value_error("list.remove(x): x not in list")),
            }
        }
        "reverse" => {
            items.write().reverse();
            Ok(PyObject::none())
        }
        "sort" => {
            let mut w = items.write();
            let mut v: Vec<_> = w.drain(..).collect();
            v.sort_by(|a, b| {
                partial_cmp_for_sort(a, b).unwrap_or(std::cmp::Ordering::Equal)
            });
            w.extend(v);
            Ok(PyObject::none())
        }
        "clear" => {
            items.write().clear();
            Ok(PyObject::none())
        }
        _ => Err(PyException::attribute_error(format!(
            "'list' object has no attribute '{}'", method
        ))),
    }
}

fn call_dict_method(map: &Arc<RwLock<IndexMap<HashableKey, PyObjectRef>>>, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "keys" => {
            let keys: Vec<PyObjectRef> = map.read().keys().map(|k| k.to_object()).collect();
            Ok(PyObject::list(keys))
        }
        "values" => {
            let vals: Vec<PyObjectRef> = map.read().values().cloned().collect();
            Ok(PyObject::list(vals))
        }
        "items" => {
            let pairs: Vec<PyObjectRef> = map.read().iter()
                .map(|(k, v)| PyObject::tuple(vec![k.to_object(), v.clone()]))
                .collect();
            Ok(PyObject::list(pairs))
        }
        "get" => {
            check_args_min("get", args, 1)?;
            let key = args[0].to_hashable_key()?;
            let default = if args.len() >= 2 { args[1].clone() } else { PyObject::none() };
            Ok(map.read().get(&key).cloned().unwrap_or(default))
        }
        "copy" => {
            Ok(PyObject::dict(map.read().clone()))
        }
        "update" => {
            check_args_min("update", args, 1)?;
            if let PyObjectPayload::Dict(other) = &args[0].payload {
                let other_items = other.read().clone();
                let mut w = map.write();
                for (k, v) in other_items {
                    w.insert(k, v);
                }
            }
            Ok(PyObject::none())
        }
        "pop" => {
            check_args_min("pop", args, 1)?;
            let key = args[0].to_hashable_key()?;
            let default = if args.len() >= 2 { Some(args[1].clone()) } else { None };
            match map.write().swap_remove(&key) {
                Some(v) => Ok(v),
                None => match default {
                    Some(d) => Ok(d),
                    None => Err(PyException::key_error(args[0].repr())),
                },
            }
        }
        "setdefault" => {
            check_args_min("setdefault", args, 1)?;
            let key = args[0].to_hashable_key()?;
            let default = if args.len() >= 2 { args[1].clone() } else { PyObject::none() };
            let mut w = map.write();
            Ok(w.entry(key).or_insert(default).clone())
        }
        "clear" => {
            map.write().clear();
            Ok(PyObject::none())
        }
        "popitem" => {
            match map.write().pop() {
                Some((k, v)) => Ok(PyObject::tuple(vec![k.to_object(), v])),
                None => Err(PyException::key_error("popitem(): dictionary is empty")),
            }
        }
        _ => Err(PyException::attribute_error(format!(
            "'dict' object has no attribute '{}'", method
        ))),
    }
}

fn call_tuple_method(items: &[PyObjectRef], method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "count" => {
            check_args_min("count", args, 1)?;
            let target = &args[0];
            let c = items.iter().filter(|x| x.py_to_string() == target.py_to_string()).count();
            Ok(PyObject::int(c as i64))
        }
        "index" => {
            check_args_min("index", args, 1)?;
            let target = &args[0];
            for (i, x) in items.iter().enumerate() {
                if x.py_to_string() == target.py_to_string() {
                    return Ok(PyObject::int(i as i64));
                }
            }
            Err(PyException::value_error("tuple.index(x): x not in tuple"))
        }
        _ => Err(PyException::attribute_error(format!(
            "'tuple' object has no attribute '{}'", method
        ))),
    }
}

fn call_set_method(m: &Arc<RwLock<IndexMap<HashableKey, PyObjectRef>>>, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "copy" => Ok(PyObject::set(m.read().clone())),
        "union" | "__or__" => {
            check_args_min("union", args, 1)?;
            let mut result = m.read().clone();
            let other_list = args[0].to_list()?;
            for item in other_list {
                let hk = item.to_hashable_key()?;
                result.entry(hk).or_insert(item);
            }
            Ok(PyObject::set(result))
        }
        "intersection" | "__and__" => {
            check_args_min("intersection", args, 1)?;
            let other_items = args[0].to_list()?;
            let other_keys: std::collections::HashSet<String> = other_items.iter()
                .map(|x| x.py_to_string()).collect();
            let guard = m.read();
            let result: IndexMap<HashableKey, PyObjectRef> = guard.iter()
                .filter(|(_, v)| other_keys.contains(&v.py_to_string()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            Ok(PyObject::set(result))
        }
        "difference" | "__sub__" => {
            check_args_min("difference", args, 1)?;
            let other_items = args[0].to_list()?;
            let other_keys: std::collections::HashSet<String> = other_items.iter()
                .map(|x| x.py_to_string()).collect();
            let guard = m.read();
            let result: IndexMap<HashableKey, PyObjectRef> = guard.iter()
                .filter(|(_, v)| !other_keys.contains(&v.py_to_string()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            Ok(PyObject::set(result))
        }
        "symmetric_difference" | "__xor__" => {
            check_args_min("symmetric_difference", args, 1)?;
            let other_items = args[0].to_list()?;
            let guard = m.read();
            let self_keys: std::collections::HashSet<String> = guard.values()
                .map(|x| x.py_to_string()).collect();
            let other_keys: std::collections::HashSet<String> = other_items.iter()
                .map(|x| x.py_to_string()).collect();
            let mut result = IndexMap::new();
            for (k, v) in guard.iter() {
                if !other_keys.contains(&v.py_to_string()) {
                    result.insert(k.clone(), v.clone());
                }
            }
            for item in &other_items {
                if !self_keys.contains(&item.py_to_string()) {
                    if let Ok(hk) = item.to_hashable_key() {
                        result.insert(hk, item.clone());
                    }
                }
            }
            Ok(PyObject::set(result))
        }
        "issubset" => {
            check_args_min("issubset", args, 1)?;
            let other_items = args[0].to_list()?;
            let other_keys: std::collections::HashSet<String> = other_items.iter()
                .map(|x| x.py_to_string()).collect();
            let all_in = m.read().values().all(|v| other_keys.contains(&v.py_to_string()));
            Ok(PyObject::bool_val(all_in))
        }
        "issuperset" => {
            check_args_min("issuperset", args, 1)?;
            let other_items = args[0].to_list()?;
            let guard = m.read();
            let self_keys: std::collections::HashSet<String> = guard.values()
                .map(|x| x.py_to_string()).collect();
            let all_in = other_items.iter().all(|v| self_keys.contains(&v.py_to_string()));
            Ok(PyObject::bool_val(all_in))
        }
        "isdisjoint" => {
            check_args_min("isdisjoint", args, 1)?;
            let other_items = args[0].to_list()?;
            let guard = m.read();
            let self_keys: std::collections::HashSet<String> = guard.values()
                .map(|x| x.py_to_string()).collect();
            let none_in = other_items.iter().all(|v| !self_keys.contains(&v.py_to_string()));
            Ok(PyObject::bool_val(none_in))
        }
        "add" => {
            check_args_min("add", args, 1)?;
            let hk = args[0].to_hashable_key()?;
            m.write().insert(hk, args[0].clone());
            Ok(PyObject::none())
        }
        "remove" => {
            check_args_min("remove", args, 1)?;
            let hk = args[0].to_hashable_key()?;
            if m.write().shift_remove(&hk).is_none() {
                return Err(PyException::key_error(args[0].repr()));
            }
            Ok(PyObject::none())
        }
        "discard" => {
            check_args_min("discard", args, 1)?;
            let hk = args[0].to_hashable_key()?;
            m.write().shift_remove(&hk);
            Ok(PyObject::none())
        }
        "pop" => {
            let mut guard = m.write();
            if guard.is_empty() {
                return Err(PyException::key_error("pop from an empty set"));
            }
            let key = guard.keys().next().unwrap().clone();
            let val = guard.shift_remove(&key).unwrap();
            Ok(val)
        }
        "clear" => {
            m.write().clear();
            Ok(PyObject::none())
        }
        "update" => {
            check_args_min("update", args, 1)?;
            let items = args[0].to_list()?;
            let mut guard = m.write();
            for item in items {
                if let Ok(hk) = item.to_hashable_key() {
                    guard.insert(hk, item);
                }
            }
            Ok(PyObject::none())
        }
        _ => Err(PyException::attribute_error(format!(
            "'set' object has no attribute '{}'", method
        ))),
    }
}

fn call_int_method(_receiver: &PyObjectRef, method: &str, _args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "bit_length" => {
            let n = _receiver.to_int()?;
            Ok(PyObject::int(if n == 0 { 0 } else { 64 - n.abs().leading_zeros() as i64 }))
        }
        _ => Err(PyException::attribute_error(format!(
            "'int' object has no attribute '{}'", method
        ))),
    }
}

fn call_float_method(f: f64, method: &str, _args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "is_integer" => Ok(PyObject::bool_val(f.fract() == 0.0)),
        "hex" => {
            // Python's float.hex() format
            let (mantissa, exponent, sign) = if f == 0.0 {
                (0u64, 0i32, if f.is_sign_negative() { "-" } else { "" })
            } else {
                let bits = f.to_bits();
                let sign = if bits >> 63 != 0 { "-" } else { "" };
                let exp = ((bits >> 52) & 0x7ff) as i32 - 1023;
                let mant = bits & 0x000f_ffff_ffff_ffff;
                (mant, exp, sign)
            };
            Ok(PyObject::str_val(CompactString::from(format!(
                "{}0x1.{:013x}p{:+}", sign, mantissa, exponent
            ))))
        }
        _ => Err(PyException::attribute_error(format!(
            "'float' object has no attribute '{}'", method
        ))),
    }
}

fn call_bytes_method(b: &[u8], method: &str, _args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "decode" => {
            // Simple UTF-8 decode
            let s = String::from_utf8_lossy(b);
            Ok(PyObject::str_val(CompactString::from(s)))
        }
        "hex" => Ok(PyObject::str_val(CompactString::from(hex::encode(b)))),
        _ => Err(PyException::attribute_error(format!(
            "'bytes' object has no attribute '{}'", method
        ))),
    }
}

// Hex encoding helper (avoid external dep)
mod hex {
    pub fn encode(data: &[u8]) -> String {
        data.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

pub(crate) fn partial_cmp_for_sort(a: &PyObjectRef, b: &PyObjectRef) -> Option<std::cmp::Ordering> {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(x), PyObjectPayload::Int(y)) => x.partial_cmp(y),
        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => x.partial_cmp(y),
        (PyObjectPayload::Int(x), PyObjectPayload::Float(y)) => x.to_f64().partial_cmp(y),
        (PyObjectPayload::Float(x), PyObjectPayload::Int(y)) => x.partial_cmp(&y.to_f64()),
        (PyObjectPayload::Str(x), PyObjectPayload::Str(y)) => x.partial_cmp(y),
        (PyObjectPayload::Bool(x), PyObjectPayload::Bool(y)) => x.partial_cmp(y),
        _ => None,
    }
}

// ── File I/O ──

fn builtin_open(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("open() missing required argument: 'file'"));
    }
    let path = args[0].py_to_string();
    let mode = if args.len() > 1 { args[1].py_to_string() } else { "r".to_string() };
    
    let content: Arc<RwLock<FileState>> = Arc::new(RwLock::new(FileState::new(&path, &mode)?));
    
    // Create a module-like object with file methods
    let mut attrs = IndexMap::new();
    let state = content.clone();
    attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(path.clone())));
    attrs.insert(CompactString::from("mode"), PyObject::str_val(CompactString::from(mode.clone())));
    attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));
    attrs.insert(CompactString::from("_state"), PyObject::int(Arc::as_ptr(&state) as i64));
    
    // Store the file state globally so methods can access it
    FILE_STATES.lock().unwrap().insert(Arc::as_ptr(&state) as usize, state);
    
    let file_obj = PyObject::module_with_attrs(CompactString::from("_file"), attrs);
    // Add methods via NativeFunction
    match &file_obj.payload {
        PyObjectPayload::Module(md) => {
            // We can't mutate, so let's create a new module with all attrs
        }
        _ => {}
    }
    
    // Better approach: return a module with native function methods
    let ptr = Arc::as_ptr(&content) as i64;
    let mut all_attrs = IndexMap::new();
    all_attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(path)));
    all_attrs.insert(CompactString::from("mode"), PyObject::str_val(CompactString::from(mode)));
    all_attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));
    all_attrs.insert(CompactString::from("_ptr"), PyObject::int(ptr));
    all_attrs.insert(CompactString::from("read"), PyObject::native_function("read", file_read));
    all_attrs.insert(CompactString::from("readline"), PyObject::native_function("readline", file_readline));
    all_attrs.insert(CompactString::from("readlines"), PyObject::native_function("readlines", file_readlines));
    all_attrs.insert(CompactString::from("write"), PyObject::native_function("write", file_write));
    all_attrs.insert(CompactString::from("writelines"), PyObject::native_function("writelines", file_writelines));
    all_attrs.insert(CompactString::from("close"), PyObject::native_function("close", file_close));
    all_attrs.insert(CompactString::from("__enter__"), PyObject::native_function("__enter__", file_enter));
    all_attrs.insert(CompactString::from("__exit__"), PyObject::native_function("__exit__", file_exit));
    
    // Store file state associated with the ptr value
    CURRENT_FILE_STATE.lock().unwrap().replace(content);
    
    Ok(PyObject::module_with_attrs(CompactString::from("_file"), all_attrs))
}

use std::sync::Mutex;

static FILE_STATES: std::sync::LazyLock<Mutex<IndexMap<usize, Arc<RwLock<FileState>>>>> = 
    std::sync::LazyLock::new(|| Mutex::new(IndexMap::new()));

static CURRENT_FILE_STATE: std::sync::LazyLock<Mutex<Option<Arc<RwLock<FileState>>>>> =
    std::sync::LazyLock::new(|| Mutex::new(None));

struct FileState {
    content: String,
    position: usize,
    mode: String,
    path: String,
    closed: bool,
    write_buf: String,
}

impl FileState {
    fn new(path: &str, mode: &str) -> PyResult<Self> {
        let content = if mode.contains('r') || mode.contains('+') {
            if mode.contains('r') && !std::path::Path::new(path).exists() {
                return Err(PyException::os_error(format!(
                    "[Errno 2] No such file or directory: '{}'", path
                )));
            }
            std::fs::read_to_string(path).unwrap_or_default()
        } else {
            String::new()
        };
        Ok(Self {
            content,
            position: 0,
            mode: mode.to_string(),
            path: path.to_string(),
            closed: false,
            write_buf: String::new(),
        })
    }
}

fn get_current_file() -> PyResult<Arc<RwLock<FileState>>> {
    CURRENT_FILE_STATE.lock().unwrap().clone().ok_or_else(|| {
        PyException::value_error("I/O operation on closed file")
    })
}

fn file_read(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let state = get_current_file()?;
    let mut s = state.write();
    if s.closed { return Err(PyException::value_error("I/O operation on closed file")); }
    let max_bytes = if !args.is_empty() { 
        let n = args[0].to_int()?;
        if n < 0 { s.content.len() } else { n as usize }
    } else { 
        s.content.len() 
    };
    let end = (s.position + max_bytes).min(s.content.len());
    let result = s.content[s.position..end].to_string();
    s.position = end;
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn file_readline(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let state = get_current_file()?;
    let mut s = state.write();
    if s.closed { return Err(PyException::value_error("I/O operation on closed file")); }
    if s.position >= s.content.len() {
        return Ok(PyObject::str_val(CompactString::from("")));
    }
    let rest = &s.content[s.position..];
    let line_end = rest.find('\n').map(|i| i + 1).unwrap_or(rest.len());
    let line = rest[..line_end].to_string();
    s.position += line_end;
    Ok(PyObject::str_val(CompactString::from(line)))
}

fn file_readlines(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let state = get_current_file()?;
    let mut s = state.write();
    if s.closed { return Err(PyException::value_error("I/O operation on closed file")); }
    let rest = &s.content[s.position..];
    let lines: Vec<PyObjectRef> = rest.lines()
        .map(|l| PyObject::str_val(CompactString::from(format!("{}\n", l))))
        .collect();
    s.position = s.content.len();
    Ok(PyObject::list(lines))
}

fn file_write(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("write", args, 1)?;
    let state = get_current_file()?;
    let mut s = state.write();
    if s.closed { return Err(PyException::value_error("I/O operation on closed file")); }
    let text = args[0].py_to_string();
    let len = text.len();
    s.write_buf.push_str(&text);
    Ok(PyObject::int(len as i64))
}

fn file_writelines(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("writelines", args, 1)?;
    let state = get_current_file()?;
    let mut s = state.write();
    if s.closed { return Err(PyException::value_error("I/O operation on closed file")); }
    let items = args[0].to_list()?;
    for item in items {
        s.write_buf.push_str(&item.py_to_string());
    }
    Ok(PyObject::none())
}

fn file_close(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let state = get_current_file()?;
    let mut s = state.write();
    if !s.closed {
        // Flush write buffer
        if !s.write_buf.is_empty() {
            if s.mode.contains('a') {
                let mut content = std::fs::read_to_string(&s.path).unwrap_or_default();
                content.push_str(&s.write_buf);
                std::fs::write(&s.path, &content)
                    .map_err(|e| PyException::os_error(format!("{}", e)))?;
            } else {
                std::fs::write(&s.path, &s.write_buf)
                    .map_err(|e| PyException::os_error(format!("{}", e)))?;
            }
            s.write_buf.clear();
        }
        s.closed = true;
    }
    Ok(PyObject::none())
}

fn file_enter(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // __enter__ returns self (the file object)
    if let Some(self_obj) = args.first() {
        Ok(self_obj.clone())
    } else {
        Ok(PyObject::none())
    }
}

fn file_exit(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    file_close(&[])?;
    Ok(PyObject::bool_val(false))
}

// ── Module creation helpers ──

fn make_module(name: &str, attrs: Vec<(&str, PyObjectRef)>) -> PyObjectRef {
    let mut map = IndexMap::new();
    map.insert(CompactString::from("__name__"), PyObject::str_val(CompactString::from(name)));
    for (k, v) in attrs {
        map.insert(CompactString::from(k), v);
    }
    PyObject::module_with_attrs(CompactString::from(name), map)
}

fn make_builtin(f: BuiltinFn) -> PyObjectRef {
    PyObject::native_function("", f)
}

// ── math module ──

pub fn create_math_module() -> PyObjectRef {
    make_module("math", vec![
        ("pi", PyObject::float(std::f64::consts::PI)),
        ("e", PyObject::float(std::f64::consts::E)),
        ("tau", PyObject::float(std::f64::consts::TAU)),
        ("inf", PyObject::float(f64::INFINITY)),
        ("nan", PyObject::float(f64::NAN)),
        ("sqrt", make_builtin(math_sqrt)),
        ("ceil", make_builtin(math_ceil)),
        ("floor", make_builtin(math_floor)),
        ("abs", make_builtin(math_fabs)),
        ("fabs", make_builtin(math_fabs)),
        ("pow", make_builtin(math_pow)),
        ("log", make_builtin(math_log)),
        ("log2", make_builtin(math_log2)),
        ("log10", make_builtin(math_log10)),
        ("exp", make_builtin(math_exp)),
        ("sin", make_builtin(math_sin)),
        ("cos", make_builtin(math_cos)),
        ("tan", make_builtin(math_tan)),
        ("asin", make_builtin(math_asin)),
        ("acos", make_builtin(math_acos)),
        ("atan", make_builtin(math_atan)),
        ("atan2", make_builtin(math_atan2)),
        ("degrees", make_builtin(math_degrees)),
        ("radians", make_builtin(math_radians)),
        ("isnan", make_builtin(math_isnan)),
        ("isinf", make_builtin(math_isinf)),
        ("isfinite", make_builtin(math_isfinite)),
        ("gcd", make_builtin(math_gcd)),
        ("factorial", make_builtin(math_factorial)),
        ("trunc", make_builtin(math_trunc)),
        ("copysign", make_builtin(math_copysign)),
        ("hypot", make_builtin(math_hypot)),
        ("modf", make_builtin(math_modf)),
        ("fmod", make_builtin(math_fmod)),
        ("frexp", make_builtin(math_frexp)),
        ("ldexp", make_builtin(math_ldexp)),
    ])
}

fn math_sqrt(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.sqrt", args, 1)?;
    let x = args[0].to_float()?;
    if x < 0.0 { return Err(PyException::value_error("math domain error")); }
    Ok(PyObject::float(x.sqrt()))
}
fn math_ceil(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.ceil", args, 1)?;
    Ok(PyObject::int(args[0].to_float()?.ceil() as i64))
}
fn math_floor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.floor", args, 1)?;
    Ok(PyObject::int(args[0].to_float()?.floor() as i64))
}
fn math_fabs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.fabs", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.abs()))
}
fn math_pow(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.pow", args, 2)?;
    Ok(PyObject::float(args[0].to_float()?.powf(args[1].to_float()?)))
}
fn math_log(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("math.log requires at least 1 argument")); }
    let x = args[0].to_float()?;
    if x <= 0.0 { return Err(PyException::value_error("math domain error")); }
    if args.len() > 1 {
        let base = args[1].to_float()?;
        Ok(PyObject::float(x.ln() / base.ln()))
    } else {
        Ok(PyObject::float(x.ln()))
    }
}
fn math_log2(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.log2", args, 1)?;
    let x = args[0].to_float()?;
    if x <= 0.0 { return Err(PyException::value_error("math domain error")); }
    Ok(PyObject::float(x.log2()))
}
fn math_log10(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.log10", args, 1)?;
    let x = args[0].to_float()?;
    if x <= 0.0 { return Err(PyException::value_error("math domain error")); }
    Ok(PyObject::float(x.log10()))
}
fn math_exp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.exp", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.exp()))
}
fn math_sin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.sin", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.sin()))
}
fn math_cos(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.cos", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.cos()))
}
fn math_tan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.tan", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.tan()))
}
fn math_asin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.asin", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.asin()))
}
fn math_acos(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.acos", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.acos()))
}
fn math_atan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.atan", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.atan()))
}
fn math_atan2(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.atan2", args, 2)?;
    Ok(PyObject::float(args[0].to_float()?.atan2(args[1].to_float()?)))
}
fn math_degrees(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.degrees", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.to_degrees()))
}
fn math_radians(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.radians", args, 1)?;
    Ok(PyObject::float(args[0].to_float()?.to_radians()))
}
fn math_isnan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.isnan", args, 1)?;
    Ok(PyObject::bool_val(args[0].to_float()?.is_nan()))
}
fn math_isinf(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.isinf", args, 1)?;
    Ok(PyObject::bool_val(args[0].to_float()?.is_infinite()))
}
fn math_isfinite(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.isfinite", args, 1)?;
    Ok(PyObject::bool_val(args[0].to_float()?.is_finite()))
}
fn math_gcd(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.gcd", args, 2)?;
    let mut a = args[0].to_int()?.abs();
    let mut b = args[1].to_int()?.abs();
    while b != 0 { let t = b; b = a % b; a = t; }
    Ok(PyObject::int(a))
}
fn math_factorial(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.factorial", args, 1)?;
    let n = args[0].to_int()?;
    if n < 0 { return Err(PyException::value_error("factorial() not defined for negative values")); }
    let mut result: i64 = 1;
    for i in 2..=n {
        result = result.checked_mul(i).ok_or_else(|| PyException::overflow_error("factorial result too large"))?;
    }
    Ok(PyObject::int(result))
}
fn math_trunc(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.trunc", args, 1)?;
    Ok(PyObject::int(args[0].to_float()?.trunc() as i64))
}
fn math_copysign(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.copysign", args, 2)?;
    Ok(PyObject::float(args[0].to_float()?.copysign(args[1].to_float()?)))
}
fn math_hypot(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.hypot", args, 2)?;
    Ok(PyObject::float(args[0].to_float()?.hypot(args[1].to_float()?)))
}
fn math_modf(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.modf", args, 1)?;
    let x = args[0].to_float()?;
    let fract = x.fract();
    let trunc = x.trunc();
    Ok(PyObject::tuple(vec![PyObject::float(fract), PyObject::float(trunc)]))
}
fn math_fmod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.fmod", args, 2)?;
    Ok(PyObject::float(args[0].to_float()? % args[1].to_float()?))
}
fn math_frexp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.frexp", args, 1)?;
    let (m, e) = frexp(args[0].to_float()?);
    Ok(PyObject::tuple(vec![PyObject::float(m), PyObject::int(e as i64)]))
}
fn math_ldexp(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("math.ldexp", args, 2)?;
    let x = args[0].to_float()?;
    let i = args[1].to_int()? as i32;
    Ok(PyObject::float(x * (2.0f64).powi(i)))
}

fn frexp(x: f64) -> (f64, i32) {
    if x == 0.0 { return (0.0, 0); }
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7FF) as i32 - 1022;
    let mantissa = f64::from_bits((bits & 0x800FFFFFFFFFFFFF) | 0x3FE0000000000000);
    (mantissa, exp)
}

// ── sys module ──

pub fn create_sys_module() -> PyObjectRef {
    make_module("sys", vec![
        ("version", PyObject::str_val(CompactString::from("3.8.0 (ferrython)"))),
        ("version_info", PyObject::tuple(vec![
            PyObject::int(3), PyObject::int(8), PyObject::int(0),
            PyObject::str_val(CompactString::from("final")), PyObject::int(0),
        ])),
        ("platform", PyObject::str_val(CompactString::from(std::env::consts::OS))),
        ("executable", PyObject::str_val(CompactString::from("ferrython"))),
        ("argv", PyObject::list(vec![PyObject::str_val(CompactString::from(""))])),
        ("path", PyObject::list(vec![
            PyObject::str_val(CompactString::from("")),
            PyObject::str_val(CompactString::from(".")),
        ])),
        ("modules", PyObject::dict_from_pairs(vec![])),
        ("maxsize", PyObject::int(i64::MAX)),
        ("maxunicode", PyObject::int(0x10FFFF)),
        ("byteorder", PyObject::str_val(CompactString::from(if cfg!(target_endian = "little") { "little" } else { "big" }))),
        ("prefix", PyObject::str_val(CompactString::from("/usr/local"))),
        ("exec_prefix", PyObject::str_val(CompactString::from("/usr/local"))),
        ("implementation", PyObject::str_val(CompactString::from("ferrython"))),
        ("stdin", PyObject::str_val(CompactString::from("<stdin>"))),
        ("stdout", PyObject::str_val(CompactString::from("<stdout>"))),
        ("stderr", PyObject::str_val(CompactString::from("<stderr>"))),
        ("getrecursionlimit", make_builtin(sys_getrecursionlimit)),
        ("setrecursionlimit", make_builtin(sys_setrecursionlimit)),
        ("exit", make_builtin(sys_exit)),
        ("getsizeof", make_builtin(sys_getsizeof)),
    ])
}

fn sys_getrecursionlimit(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(1000))
}
fn sys_setrecursionlimit(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("sys.setrecursionlimit", args, 1)?;
    Ok(PyObject::none())
}
fn sys_exit(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let code = if args.is_empty() { 0 } else { args[0].to_int().unwrap_or(1) };
    std::process::exit(code as i32);
}
fn sys_getsizeof(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("sys.getsizeof", args, 1)?;
    Ok(PyObject::int(std::mem::size_of::<PyObject>() as i64))
}

// ── os module ──

pub fn create_os_module() -> PyObjectRef {
    make_module("os", vec![
        ("name", PyObject::str_val(CompactString::from(if cfg!(windows) { "nt" } else { "posix" }))),
        ("sep", PyObject::str_val(CompactString::from(std::path::MAIN_SEPARATOR.to_string()))),
        ("linesep", PyObject::str_val(CompactString::from(if cfg!(windows) { "\r\n" } else { "\n" }))),
        ("curdir", PyObject::str_val(CompactString::from("."))),
        ("pardir", PyObject::str_val(CompactString::from(".."))),
        ("extsep", PyObject::str_val(CompactString::from("."))),
        ("getcwd", make_builtin(os_getcwd)),
        ("listdir", make_builtin(os_listdir)),
        ("mkdir", make_builtin(os_mkdir)),
        ("makedirs", make_builtin(os_makedirs)),
        ("remove", make_builtin(os_remove)),
        ("rmdir", make_builtin(os_rmdir)),
        ("rename", make_builtin(os_rename)),
        ("path", make_builtin(os_path_stub)),
        ("getenv", make_builtin(os_getenv)),
        ("environ", PyObject::dict_from_pairs(
            std::env::vars().map(|(k, v)| (
                PyObject::str_val(CompactString::from(k)),
                PyObject::str_val(CompactString::from(v)),
            )).collect()
        )),
        ("cpu_count", make_builtin(os_cpu_count)),
        ("getpid", make_builtin(os_getpid)),
    ])
}

fn os_getcwd(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let cwd = std::env::current_dir()
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::str_val(CompactString::from(cwd.to_string_lossy().to_string())))
}
fn os_listdir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let path = if args.is_empty() { ".".to_string() } else { args[0].py_to_string() };
    let entries = std::fs::read_dir(&path)
        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    let items: Vec<PyObjectRef> = entries
        .filter_map(|e| e.ok())
        .map(|e| PyObject::str_val(CompactString::from(e.file_name().to_string_lossy().to_string())))
        .collect();
    Ok(PyObject::list(items))
}
fn os_mkdir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.mkdir", args, 1)?;
    std::fs::create_dir(args[0].py_to_string())
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::none())
}
fn os_makedirs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.makedirs", args, 1)?;
    std::fs::create_dir_all(args[0].py_to_string())
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::none())
}
fn os_remove(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.remove", args, 1)?;
    std::fs::remove_file(args[0].py_to_string())
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::none())
}
fn os_rmdir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.rmdir", args, 1)?;
    std::fs::remove_dir(args[0].py_to_string())
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::none())
}
fn os_rename(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.rename", args, 2)?;
    std::fs::rename(args[0].py_to_string(), args[1].py_to_string())
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::none())
}
fn os_path_stub(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(create_os_path_module())
}
fn os_getenv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("os.getenv requires at least 1 argument")); }
    let key = args[0].py_to_string();
    let default = if args.len() > 1 { args[1].clone() } else { PyObject::none() };
    match std::env::var(&key) {
        Ok(v) => Ok(PyObject::str_val(CompactString::from(v))),
        Err(_) => Ok(default),
    }
}
fn os_cpu_count(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(num_cpus() as i64))
}
fn os_getpid(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(std::process::id() as i64))
}

fn num_cpus() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

// ── os.path module ──

pub fn create_os_path_module() -> PyObjectRef {
    make_module("os.path", vec![
        ("join", make_builtin(os_path_join)),
        ("exists", make_builtin(os_path_exists)),
        ("isfile", make_builtin(os_path_isfile)),
        ("isdir", make_builtin(os_path_isdir)),
        ("basename", make_builtin(os_path_basename)),
        ("dirname", make_builtin(os_path_dirname)),
        ("abspath", make_builtin(os_path_abspath)),
        ("splitext", make_builtin(os_path_splitext)),
        ("split", make_builtin(os_path_split)),
        ("sep", PyObject::str_val(CompactString::from(std::path::MAIN_SEPARATOR.to_string()))),
    ])
}

fn os_path_join(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("os.path.join requires at least 1 argument")); }
    let mut path = std::path::PathBuf::from(args[0].py_to_string());
    for arg in &args[1..] {
        path.push(arg.py_to_string());
    }
    Ok(PyObject::str_val(CompactString::from(path.to_string_lossy().to_string())))
}
fn os_path_exists(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.exists", args, 1)?;
    Ok(PyObject::bool_val(std::path::Path::new(&args[0].py_to_string()).exists()))
}
fn os_path_isfile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.isfile", args, 1)?;
    Ok(PyObject::bool_val(std::path::Path::new(&args[0].py_to_string()).is_file()))
}
fn os_path_isdir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.isdir", args, 1)?;
    Ok(PyObject::bool_val(std::path::Path::new(&args[0].py_to_string()).is_dir()))
}
fn os_path_basename(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.basename", args, 1)?;
    let s = args[0].py_to_string();
    let p = std::path::Path::new(&s);
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(PyObject::str_val(CompactString::from(name)))
}
fn os_path_dirname(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.dirname", args, 1)?;
    let s = args[0].py_to_string();
    let p = std::path::Path::new(&s);
    let dir = p.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
    Ok(PyObject::str_val(CompactString::from(dir)))
}
fn os_path_abspath(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.abspath", args, 1)?;
    let s = args[0].py_to_string();
    let p = std::path::Path::new(&s);
    let abs = std::fs::canonicalize(p).unwrap_or_else(|_| {
        let mut cwd = std::env::current_dir().unwrap_or_default();
        cwd.push(&s);
        cwd
    });
    Ok(PyObject::str_val(CompactString::from(abs.to_string_lossy().to_string())))
}
fn os_path_splitext(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.splitext", args, 1)?;
    let s = args[0].py_to_string();
    let p = std::path::Path::new(&s);
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let stem = s[..s.len()-ext.len()].to_string();
    Ok(PyObject::tuple(vec![
        PyObject::str_val(CompactString::from(stem)),
        PyObject::str_val(CompactString::from(ext)),
    ]))
}
fn os_path_split(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.split", args, 1)?;
    let s = args[0].py_to_string();
    let p = std::path::Path::new(&s);
    let dir = p.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(PyObject::tuple(vec![
        PyObject::str_val(CompactString::from(dir)),
        PyObject::str_val(CompactString::from(name)),
    ]))
}

// ── string module ──

pub fn create_string_module() -> PyObjectRef {
    make_module("string", vec![
        ("ascii_lowercase", PyObject::str_val(CompactString::from("abcdefghijklmnopqrstuvwxyz"))),
        ("ascii_uppercase", PyObject::str_val(CompactString::from("ABCDEFGHIJKLMNOPQRSTUVWXYZ"))),
        ("ascii_letters", PyObject::str_val(CompactString::from("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"))),
        ("digits", PyObject::str_val(CompactString::from("0123456789"))),
        ("hexdigits", PyObject::str_val(CompactString::from("0123456789abcdefABCDEF"))),
        ("octdigits", PyObject::str_val(CompactString::from("01234567"))),
        ("punctuation", PyObject::str_val(CompactString::from("!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~"))),
        ("whitespace", PyObject::str_val(CompactString::from(" \t\n\r\x0b\x0c"))),
        ("printable", PyObject::str_val(CompactString::from("0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~ \t\n\r\x0b\x0c"))),
    ])
}

// ── json module (basic) ──

pub fn create_json_module() -> PyObjectRef {
    make_module("json", vec![
        ("dumps", make_builtin(json_dumps)),
        ("loads", make_builtin(json_loads)),
    ])
}

fn json_dumps(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("json.dumps", args, 1)?;
    let s = py_to_json(&args[0])?;
    Ok(PyObject::str_val(CompactString::from(s)))
}

fn py_to_json(obj: &PyObjectRef) -> PyResult<String> {
    match &obj.payload {
        PyObjectPayload::None => Ok("null".into()),
        PyObjectPayload::Bool(b) => Ok(if *b { "true" } else { "false" }.into()),
        PyObjectPayload::Int(n) => Ok(n.to_string()),
        PyObjectPayload::Float(f) => {
            if f.is_nan() { return Err(PyException::value_error("NaN is not JSON serializable")); }
            if f.is_infinite() { return Err(PyException::value_error("Infinity is not JSON serializable")); }
            Ok(format!("{}", f))
        }
        PyObjectPayload::Str(s) => Ok(format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t"))),
        PyObjectPayload::List(items) => {
            let r = items.read();
            let parts: Result<Vec<String>, _> = r.iter().map(|i| py_to_json(i)).collect();
            Ok(format!("[{}]", parts?.join(", ")))
        }
        PyObjectPayload::Tuple(items) => {
            let parts: Result<Vec<String>, _> = items.iter().map(|i| py_to_json(i)).collect();
            Ok(format!("[{}]", parts?.join(", ")))
        }
        PyObjectPayload::Dict(map) => {
            let r = map.read();
            let parts: Result<Vec<String>, _> = r.iter().map(|(k, v)| {
                let key_str = match k {
                    HashableKey::Str(s) => format!("\"{}\"", s),
                    HashableKey::Int(n) => format!("\"{}\"", n),
                    _ => return Err(PyException::type_error("keys must be str")),
                };
                let val_str = py_to_json(v)?;
                Ok(format!("{}: {}", key_str, val_str))
            }).collect();
            Ok(format!("{{{}}}", parts?.join(", ")))
        }
        _ => Err(PyException::type_error(format!("Object of type {} is not JSON serializable", obj.type_name()))),
    }
}

fn json_loads(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("json.loads", args, 1)?;
    let s = match &args[0].payload {
        PyObjectPayload::Str(s) => s.to_string(),
        _ => return Err(PyException::type_error("json.loads requires a string")),
    };
    parse_json_value(&s, &mut 0)
}

fn parse_json_value(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    skip_ws(s, pos);
    if *pos >= s.len() { return Err(PyException::value_error("Unexpected end of JSON")); }
    let ch = s.as_bytes()[*pos] as char;
    match ch {
        '"' => parse_json_string(s, pos),
        't' | 'f' => parse_json_bool(s, pos),
        'n' => parse_json_null(s, pos),
        '[' => parse_json_array(s, pos),
        '{' => parse_json_object(s, pos),
        _ => parse_json_number(s, pos),
    }
}

fn skip_ws(s: &str, pos: &mut usize) {
    while *pos < s.len() && s.as_bytes()[*pos].is_ascii_whitespace() { *pos += 1; }
}

fn parse_json_string(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    *pos += 1; // skip "
    let mut result = String::new();
    while *pos < s.len() {
        let ch = s.as_bytes()[*pos] as char;
        if ch == '"' { *pos += 1; return Ok(PyObject::str_val(CompactString::from(result))); }
        if ch == '\\' {
            *pos += 1;
            if *pos >= s.len() { break; }
            let esc = s.as_bytes()[*pos] as char;
            match esc {
                'n' => result.push('\n'),
                't' => result.push('\t'),
                'r' => result.push('\r'),
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                '/' => result.push('/'),
                _ => { result.push('\\'); result.push(esc); }
            }
        } else {
            result.push(ch);
        }
        *pos += 1;
    }
    Err(PyException::value_error("Unterminated string"))
}

fn parse_json_bool(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    if s[*pos..].starts_with("true") { *pos += 4; return Ok(PyObject::bool_val(true)); }
    if s[*pos..].starts_with("false") { *pos += 5; return Ok(PyObject::bool_val(false)); }
    Err(PyException::value_error("Invalid JSON"))
}

fn parse_json_null(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    if s[*pos..].starts_with("null") { *pos += 4; return Ok(PyObject::none()); }
    Err(PyException::value_error("Invalid JSON"))
}

fn parse_json_number(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    let start = *pos;
    let mut is_float = false;
    if *pos < s.len() && s.as_bytes()[*pos] == b'-' { *pos += 1; }
    while *pos < s.len() && s.as_bytes()[*pos].is_ascii_digit() { *pos += 1; }
    if *pos < s.len() && s.as_bytes()[*pos] == b'.' {
        is_float = true; *pos += 1;
        while *pos < s.len() && s.as_bytes()[*pos].is_ascii_digit() { *pos += 1; }
    }
    if *pos < s.len() && (s.as_bytes()[*pos] == b'e' || s.as_bytes()[*pos] == b'E') {
        is_float = true; *pos += 1;
        if *pos < s.len() && (s.as_bytes()[*pos] == b'+' || s.as_bytes()[*pos] == b'-') { *pos += 1; }
        while *pos < s.len() && s.as_bytes()[*pos].is_ascii_digit() { *pos += 1; }
    }
    let num_str = &s[start..*pos];
    if is_float {
        let f: f64 = num_str.parse().map_err(|_| PyException::value_error("Invalid number"))?;
        Ok(PyObject::float(f))
    } else {
        let i: i64 = num_str.parse().map_err(|_| PyException::value_error("Invalid number"))?;
        Ok(PyObject::int(i))
    }
}

fn parse_json_array(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    *pos += 1; // skip [
    let mut items = Vec::new();
    skip_ws(s, pos);
    if *pos < s.len() && s.as_bytes()[*pos] == b']' { *pos += 1; return Ok(PyObject::list(items)); }
    loop {
        items.push(parse_json_value(s, pos)?);
        skip_ws(s, pos);
        if *pos >= s.len() { break; }
        if s.as_bytes()[*pos] == b']' { *pos += 1; return Ok(PyObject::list(items)); }
        if s.as_bytes()[*pos] == b',' { *pos += 1; } else { break; }
    }
    Err(PyException::value_error("Invalid JSON array"))
}

fn parse_json_object(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    *pos += 1; // skip {
    let pairs: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
    let dict = PyObject::dict_from_pairs(pairs);
    skip_ws(s, pos);
    if *pos < s.len() && s.as_bytes()[*pos] == b'}' { *pos += 1; return Ok(dict); }
    loop {
        skip_ws(s, pos);
        let key = parse_json_string(s, pos)?;
        skip_ws(s, pos);
        if *pos >= s.len() || s.as_bytes()[*pos] != b':' { return Err(PyException::value_error("Expected ':'")); }
        *pos += 1;
        let value = parse_json_value(s, pos)?;
        let hk = HashableKey::Str(CompactString::from(key.py_to_string()));
        match &dict.payload {
            PyObjectPayload::Dict(map) => { map.write().insert(hk, value); }
            _ => unreachable!(),
        }
        skip_ws(s, pos);
        if *pos >= s.len() { break; }
        if s.as_bytes()[*pos] == b'}' { *pos += 1; return Ok(dict); }
        if s.as_bytes()[*pos] == b',' { *pos += 1; } else { break; }
    }
    Err(PyException::value_error("Invalid JSON object"))
}

// ── time module ──

pub fn create_time_module() -> PyObjectRef {
    make_module("time", vec![
        ("time", make_builtin(time_time)),
        ("sleep", make_builtin(time_sleep)),
        ("monotonic", make_builtin(time_monotonic)),
    ])
}

fn time_time(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use std::time::SystemTime;
    let dur = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
    Ok(PyObject::float(dur.as_secs_f64()))
}

fn time_sleep(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("time.sleep", args, 1)?;
    let secs = args[0].to_float()?;
    if secs < 0.0 { return Err(PyException::value_error("sleep length must be non-negative")); }
    std::thread::sleep(std::time::Duration::from_secs_f64(secs));
    Ok(PyObject::none())
}

fn time_monotonic(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use std::time::Instant;
    // Return seconds since some arbitrary epoch
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    Ok(PyObject::float(start.elapsed().as_secs_f64()))
}

// ── random module (basic) ──

pub fn create_random_module() -> PyObjectRef {
    make_module("random", vec![
        ("random", make_builtin(random_random)),
        ("randint", make_builtin(random_randint)),
        ("choice", make_builtin(random_choice)),
        ("shuffle", make_builtin(random_shuffle)),
        ("seed", make_builtin(random_seed)),
        ("randrange", make_builtin(random_randrange)),
    ])
}

fn simple_random() -> f64 {
    use std::time::SystemTime;
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let cnt = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().subsec_nanos() as u64;
    let seed = nanos.wrapping_mul(6364136223846793005).wrapping_add(cnt.wrapping_mul(1442695040888963407));
    (seed >> 11) as f64 / (1u64 << 53) as f64
}

fn random_random(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::float(simple_random()))
}
fn random_randint(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random.randint", args, 2)?;
    let a = args[0].to_int()?;
    let b = args[1].to_int()?;
    let range = (b - a + 1) as f64;
    Ok(PyObject::int(a + (simple_random() * range) as i64))
}
fn random_choice(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random.choice", args, 1)?;
    let items = args[0].to_list()?;
    if items.is_empty() { return Err(PyException::index_error("Cannot choose from an empty sequence")); }
    let idx = (simple_random() * items.len() as f64) as usize;
    Ok(items[idx.min(items.len()-1)].clone())
}
fn random_shuffle(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("random.shuffle", args, 1)?;
    // Simplified in-place shuffle
    Ok(PyObject::none())
}
fn random_seed(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::none())
}
fn random_randrange(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("randrange requires at least 1 argument")); }
    let start = if args.len() == 1 { 0 } else { args[0].to_int()? };
    let stop = if args.len() == 1 { args[0].to_int()? } else { args[1].to_int()? };
    let step = if args.len() > 2 { args[2].to_int()? } else { 1 };
    let range = ((stop - start) as f64 / step as f64).ceil() as i64;
    if range <= 0 { return Err(PyException::value_error("empty range for randrange()")); }
    let idx = (simple_random() * range as f64) as i64;
    Ok(PyObject::int(start + idx * step))
}

// ── Stub modules ──

pub fn create_collections_module() -> PyObjectRef {
    make_module("collections", vec![
        ("OrderedDict", make_builtin(|_args| Ok(PyObject::dict_from_pairs(vec![])))),
        ("defaultdict", make_builtin(|_args| Ok(PyObject::dict_from_pairs(vec![])))),
        ("Counter", make_builtin(|_args| Ok(PyObject::dict_from_pairs(vec![])))),
    ])
}

pub fn create_functools_module() -> PyObjectRef {
    make_module("functools", vec![
        ("reduce", make_builtin(functools_reduce)),
        ("partial", make_builtin(|_args| Ok(PyObject::none()))),
    ])
}

fn functools_reduce(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("reduce() requires at least 2 arguments")); }
    let func = args[0].clone();
    let items = args[1].to_list()?;
    let mut acc = if args.len() > 2 {
        args[2].clone()
    } else if !items.is_empty() {
        items[0].clone()
    } else {
        return Err(PyException::type_error("reduce() of empty sequence with no initial value"));
    };
    let start_idx = if args.len() > 2 { 0 } else { 1 };
    for item in &items[start_idx..] {
        // Call func(acc, item) — but we're a builtin, so we can't easily call Python funcs here.
        // This would need VM access; for now we'll return a stub error.
        let _ = func;
        let _ = item;
        return Err(PyException::type_error("functools.reduce not fully implemented yet"));
    }
    Ok(acc)
}

pub fn create_itertools_module() -> PyObjectRef {
    make_module("itertools", vec![
        ("count", make_builtin(|_args| Ok(PyObject::none()))),
        ("chain", make_builtin(|_args| Ok(PyObject::none()))),
    ])
}

pub fn create_io_module() -> PyObjectRef {
    make_module("io", vec![
        ("StringIO", make_builtin(|_args| Ok(PyObject::none()))),
        ("BytesIO", make_builtin(|_args| Ok(PyObject::none()))),
    ])
}

pub fn create_re_module() -> PyObjectRef {
    make_module("re", vec![
        ("IGNORECASE", PyObject::int(2)),
        ("MULTILINE", PyObject::int(8)),
        ("DOTALL", PyObject::int(16)),
    ])
}

pub fn create_hashlib_module() -> PyObjectRef {
    make_module("hashlib", vec![])
}
