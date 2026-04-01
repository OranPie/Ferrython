//! Built-in functions available in Python's builtins module.

mod modules;
pub use modules::*;

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, ClassData, IteratorData, CompareOp, InstanceData};
use ferrython_core::types::{HashableKey, PyInt};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::{Arc, Mutex};

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
        "bytes" => Some(builtin_bytes),
        "bytearray" => Some(builtin_bytearray),
        "complex" => Some(builtin_complex),
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

fn apply_format_spec(val: &PyObjectRef, spec: &str) -> String {
    match val.format_value(spec) {
        Ok(s) => s,
        Err(_) => val.py_to_string(),
    }
}

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
    if args.len() == 3 {
        // type(name, bases, dict) → dynamic class creation
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
        // Compute simple MRO from bases
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
        return Ok(PyObject::wrap(PyObjectPayload::Class(ferrython_core::object::ClassData {
            name: CompactString::from(name),
            bases,
            namespace: Arc::new(RwLock::new(namespace)),
            mro,
        })));
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

fn builtin_id(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("id", args, 1)?;
    let ptr = std::sync::Arc::as_ptr(&args[0]) as usize;
    Ok(PyObject::int(ptr as i64))
}

pub(crate) fn builtin_abs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
        HashableKey::FrozenSet(_) => 0,
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
    Ok(PyObject::wrap(PyObjectPayload::Iterator(
        Arc::new(std::sync::Mutex::new(ferrython_core::object::IteratorData::List { items, index: 0 }))
    )))
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
    Ok(PyObject::wrap(PyObjectPayload::Iterator(
        Arc::new(std::sync::Mutex::new(ferrython_core::object::IteratorData::List { items: result, index: 0 }))
    )))
}

fn builtin_zip(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(
            Arc::new(std::sync::Mutex::new(ferrython_core::object::IteratorData::List { items: vec![], index: 0 }))
        )));
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
    Ok(PyObject::wrap(PyObjectPayload::Iterator(
        Arc::new(std::sync::Mutex::new(ferrython_core::object::IteratorData::List { items: result, index: 0 }))
    )))
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

pub(crate) fn builtin_dir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn builtin_bytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn builtin_bytearray(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn builtin_complex(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Stub: complex(real, imag) → just return float for now
    let real = if !args.is_empty() { args[0].to_float().unwrap_or(0.0) } else { 0.0 };
    let _imag = if args.len() > 1 { args[1].to_float().unwrap_or(0.0) } else { 0.0 };
    // TODO: proper complex type
    Ok(PyObject::float(real))
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

pub(crate) fn check_args(name: &str, args: &[PyObjectRef], expected: usize) -> PyResult<()> {
    if args.len() != expected {
        Err(PyException::type_error(format!(
            "{}() takes exactly {} argument(s) ({} given)", name, expected, args.len()
        )))
    } else { Ok(()) }
}

pub(crate) fn check_args_min(name: &str, args: &[PyObjectRef], min: usize) -> PyResult<()> {
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
        PyObjectPayload::Instance(inst) => call_instance_method(inst, method, args),
        _ => Err(PyException::attribute_error(format!(
            "'{}' object has no attribute '{}'", receiver.type_name(), method
        ))),
    }
}

fn call_instance_method(inst: &ferrython_core::object::InstanceData, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Namedtuple methods
    if inst.class.get_attr("__namedtuple__").is_some() {
        return call_namedtuple_method(inst, method, args);
    }
    // Deque methods (except extend/extendleft which need VM for iterable collection)
    if inst.attrs.read().contains_key("__deque__") {
        return call_deque_method(inst, method, args);
    }
    // Hashlib hash object methods
    let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload { cd.name.to_string() } else { String::new() };
    if matches!(class_name.as_str(), "md5" | "sha1" | "sha256" | "sha224" | "sha384" | "sha512") {
        return call_hashlib_method(inst, method, args);
    }
    Err(PyException::attribute_error(format!(
        "'{}' object has no attribute '{}'", 
        if let PyObjectPayload::Class(cd) = &inst.class.payload { cd.name.as_str() } else { "instance" },
        method
    )))
}

fn call_namedtuple_method(inst: &ferrython_core::object::InstanceData, method: &str, _args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "_asdict" => {
            if let Some(fields) = inst.class.get_attr("_fields") {
                if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                    let mut map = IndexMap::new();
                    let attrs = inst.attrs.read();
                    for field in field_names {
                        let name = field.py_to_string();
                        let val = attrs.get(name.as_str()).cloned().unwrap_or_else(PyObject::none);
                        map.insert(HashableKey::Str(CompactString::from(name.as_str())), val);
                    }
                    return Ok(PyObject::dict(map));
                }
            }
            Ok(PyObject::dict(IndexMap::new()))
        }
        "__len__" => {
            if let Some(tup) = inst.attrs.read().get("_tuple") {
                if let PyObjectPayload::Tuple(items) = &tup.payload {
                    return Ok(PyObject::int(items.len() as i64));
                }
            }
            Ok(PyObject::int(0))
        }
        "__iter__" => {
            if let Some(tup) = inst.attrs.read().get("_tuple").cloned() {
                if let PyObjectPayload::Tuple(items) = &tup.payload {
                    return Ok(PyObject::wrap(PyObjectPayload::Iterator(
                        Arc::new(std::sync::Mutex::new(
                            ferrython_core::object::IteratorData::Tuple { items: items.clone(), index: 0 }
                        ))
                    )));
                }
            }
            Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Arc::new(std::sync::Mutex::new(
                    ferrython_core::object::IteratorData::Tuple { items: vec![], index: 0 }
                ))
            )))
        }
        _ => Err(PyException::attribute_error(format!("namedtuple has no attribute '{}'", method))),
    }
}

fn call_deque_method(inst: &ferrython_core::object::InstanceData, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let get_data = || -> PyObjectRef {
        inst.attrs.read().get("_data").cloned().unwrap_or_else(|| PyObject::list(vec![]))
    };
    match method {
        "append" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                list.write().push(args[0].clone());
            }
            Ok(PyObject::none())
        }
        "appendleft" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                list.write().insert(0, args[0].clone());
            }
            Ok(PyObject::none())
        }
        "pop" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let mut v = list.write();
                if v.is_empty() {
                    return Err(PyException::new(ExceptionKind::IndexError, "pop from an empty deque"));
                }
                return Ok(v.pop().unwrap());
            }
            Ok(PyObject::none())
        }
        "popleft" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let mut v = list.write();
                if v.is_empty() {
                    return Err(PyException::new(ExceptionKind::IndexError, "pop from an empty deque"));
                }
                return Ok(v.remove(0));
            }
            Ok(PyObject::none())
        }
        "extend" => {
            // args[0] should be pre-collected items as a List (VM collects iterable before calling)
            let items = args[0].to_list().unwrap_or_default();
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                list.write().extend(items);
            }
            Ok(PyObject::none())
        }
        "extendleft" => {
            let items = args[0].to_list().unwrap_or_default();
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let mut v = list.write();
                for item in items.into_iter().rev() {
                    v.insert(0, item);
                }
            }
            Ok(PyObject::none())
        }
        "rotate" => {
            let n = if args.is_empty() { 1i64 } else { args[0].as_int().unwrap_or(1) };
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let mut v = list.write();
                let len = v.len() as i64;
                if len > 0 {
                    let n = ((n % len) + len) % len;
                    let split = v.len() - n as usize;
                    let tail: Vec<_> = v.drain(split..).collect();
                    for (i, item) in tail.into_iter().enumerate() {
                        v.insert(i, item);
                    }
                }
            }
            Ok(PyObject::none())
        }
        "clear" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                list.write().clear();
            }
            Ok(PyObject::none())
        }
        "copy" => {
            let data = get_data();
            let items = data.to_list()?;
            dispatch("deque", &[PyObject::list(items)])
        }
        "count" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                let v = list.read();
                let count = v.iter().filter(|x| x.py_to_string() == args[0].py_to_string()).count();
                return Ok(PyObject::int(count as i64));
            }
            Ok(PyObject::int(0))
        }
        "reverse" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                list.write().reverse();
            }
            Ok(PyObject::none())
        }
        "__len__" => {
            let data = get_data();
            if let PyObjectPayload::List(list) = &data.payload {
                return Ok(PyObject::int(list.read().len() as i64));
            }
            Ok(PyObject::int(0))
        }
        "__iter__" => {
            Ok(get_data())
        }
        _ => Err(PyException::attribute_error(format!("deque has no attribute '{}'", method))),
    }
}

fn call_hashlib_method(inst: &ferrython_core::object::InstanceData, method: &str, _args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "hexdigest" => {
            let attrs = inst.attrs.read();
            if let Some(hd) = attrs.get("_hexdigest") {
                return Ok(hd.clone());
            }
            Ok(PyObject::str_val(CompactString::from("")))
        }
        "digest" => {
            let attrs = inst.attrs.read();
            if let Some(d) = attrs.get("_digest") {
                return Ok(d.clone());
            }
            Ok(PyObject::bytes(vec![]))
        }
        _ => {
            let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload { cd.name.to_string() } else { "hash".to_string() };
            Err(PyException::attribute_error(format!("'{}' object has no attribute '{}'", class_name, method)))
        }
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
            match &args[0].payload {
                PyObjectPayload::Tuple(prefixes) => {
                    let result = prefixes.iter().any(|p| {
                        p.as_str().map(|ps| s.starts_with(ps)).unwrap_or(false)
                    });
                    Ok(PyObject::bool_val(result))
                }
                _ => {
                    let prefix = args[0].as_str().ok_or_else(|| PyException::type_error("startswith() argument must be str or tuple"))?;
                    Ok(PyObject::bool_val(s.starts_with(prefix)))
                }
            }
        }
        "endswith" => {
            check_args_min("endswith", args, 1)?;
            match &args[0].payload {
                PyObjectPayload::Tuple(suffixes) => {
                    let result = suffixes.iter().any(|p| {
                        p.as_str().map(|ps| s.ends_with(ps)).unwrap_or(false)
                    });
                    Ok(PyObject::bool_val(result))
                }
                _ => {
                    let suffix = args[0].as_str().ok_or_else(|| PyException::type_error("endswith() argument must be str or tuple"))?;
                    Ok(PyObject::bool_val(s.ends_with(suffix)))
                }
            }
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
        "partition" => {
            check_args_min("partition", args, 1)?;
            let sep = args[0].py_to_string();
            if let Some(idx) = s.find(&sep) {
                Ok(PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from(&s[..idx])),
                    PyObject::str_val(CompactString::from(&sep)),
                    PyObject::str_val(CompactString::from(&s[idx + sep.len()..])),
                ]))
            } else {
                Ok(PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from(s)),
                    PyObject::str_val(CompactString::from("")),
                    PyObject::str_val(CompactString::from("")),
                ]))
            }
        }
        "rpartition" => {
            check_args_min("rpartition", args, 1)?;
            let sep = args[0].py_to_string();
            if let Some(idx) = s.rfind(&sep) {
                Ok(PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from(&s[..idx])),
                    PyObject::str_val(CompactString::from(&sep)),
                    PyObject::str_val(CompactString::from(&s[idx + sep.len()..])),
                ]))
            } else {
                Ok(PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from("")),
                    PyObject::str_val(CompactString::from("")),
                    PyObject::str_val(CompactString::from(s)),
                ]))
            }
        }
        "casefold" => {
            Ok(PyObject::str_val(CompactString::from(s.to_lowercase())))
        }
        "removeprefix" => {
            check_args_min("removeprefix", args, 1)?;
            let prefix = args[0].py_to_string();
            if s.starts_with(&prefix) {
                Ok(PyObject::str_val(CompactString::from(&s[prefix.len()..])))
            } else {
                Ok(PyObject::str_val(CompactString::from(s)))
            }
        }
        "removesuffix" => {
            check_args_min("removesuffix", args, 1)?;
            let suffix = args[0].py_to_string();
            if s.ends_with(&suffix) {
                Ok(PyObject::str_val(CompactString::from(&s[..s.len() - suffix.len()])))
            } else {
                Ok(PyObject::str_val(CompactString::from(s)))
            }
        }
        "splitlines" => {
            let keepends = !args.is_empty() && args[0].is_truthy();
            let mut lines = Vec::new();
            let mut start = 0;
            let bytes = s.as_bytes();
            let len = bytes.len();
            let mut i = 0;
            while i < len {
                if bytes[i] == b'\r' && i + 1 < len && bytes[i + 1] == b'\n' {
                    if keepends { lines.push(PyObject::str_val(CompactString::from(&s[start..i + 2]))); }
                    else { lines.push(PyObject::str_val(CompactString::from(&s[start..i]))); }
                    i += 2; start = i;
                } else if bytes[i] == b'\n' || bytes[i] == b'\r' {
                    if keepends { lines.push(PyObject::str_val(CompactString::from(&s[start..i + 1]))); }
                    else { lines.push(PyObject::str_val(CompactString::from(&s[start..i]))); }
                    i += 1; start = i;
                } else {
                    i += 1;
                }
            }
            if start < len {
                lines.push(PyObject::str_val(CompactString::from(&s[start..])));
            }
            Ok(PyObject::list(lines))
        }
        "istitle" => {
            let mut prev_cased = false;
            let mut is_title = false;
            for c in s.chars() {
                if c.is_uppercase() {
                    if prev_cased { return Ok(PyObject::bool_val(false)); }
                    prev_cased = true;
                    is_title = true;
                } else if c.is_lowercase() {
                    if !prev_cased { return Ok(PyObject::bool_val(false)); }
                    prev_cased = true;
                } else {
                    prev_cased = false;
                }
            }
            Ok(PyObject::bool_val(is_title))
        }
        "isprintable" => {
            Ok(PyObject::bool_val(!s.is_empty() && s.chars().all(|c| !c.is_control() || c == ' ')))
        }
        "isidentifier" => {
            let mut chars = s.chars();
            let valid = match chars.next() {
                Some(c) if c == '_' || c.is_alphabetic() => chars.all(|c| c == '_' || c.is_alphanumeric()),
                _ => false,
            };
            Ok(PyObject::bool_val(valid))
        }
        "isascii" => {
            Ok(PyObject::bool_val(s.is_ascii()))
        }
        "isdecimal" => {
            Ok(PyObject::bool_val(!s.is_empty() && s.chars().all(|c| c.is_ascii_digit())))
        }
        "isnumeric" => {
            Ok(PyObject::bool_val(!s.is_empty() && s.chars().all(|c| c.is_numeric())))
        }
        "format" => {
            let mut result = String::new();
            let mut chars = s.chars().peekable();
            let mut auto_idx = 0usize;
            while let Some(c) = chars.next() {
                if c == '{' {
                    if chars.peek() == Some(&'{') {
                        chars.next();
                        result.push('{');
                    } else {
                        // Collect everything until '}'
                        let mut field_spec = String::new();
                        for c in chars.by_ref() {
                            if c == '}' { break; }
                            field_spec.push(c);
                        }
                        // Split field_spec on ':' → field_name : format_spec
                        let (field_name, format_spec) = if let Some(colon_pos) = field_spec.find(':') {
                            (&field_spec[..colon_pos], Some(&field_spec[colon_pos+1..]))
                        } else {
                            (field_spec.as_str(), None)
                        };
                        // Resolve the value
                        let value = if field_name.is_empty() {
                            let v = args.get(auto_idx).cloned();
                            auto_idx += 1;
                            v
                        } else if let Ok(idx) = field_name.parse::<usize>() {
                            args.get(idx).cloned()
                        } else {
                            None
                        };
                        if let Some(val) = value {
                            if let Some(spec) = format_spec {
                                result.push_str(&apply_format_spec(&val, spec));
                            } else {
                                result.push_str(&val.py_to_string());
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
        "translate" => {
            check_args_min("translate", args, 1)?;
            let table = &args[0];
            let mut result = String::new();
            if let PyObjectPayload::Dict(map) = &table.payload {
                let map = map.read();
                for ch in s.chars() {
                    let key = HashableKey::Int(PyInt::Small(ch as i64));
                    match map.get(&key) {
                        Some(val) => {
                            if matches!(&val.payload, PyObjectPayload::None) {
                                // Delete the character
                            } else if let Ok(n) = val.to_int() {
                                if let Some(c) = char::from_u32(n as u32) {
                                    result.push(c);
                                }
                            } else {
                                result.push_str(&val.py_to_string());
                            }
                        }
                        None => result.push(ch),
                    }
                }
            } else {
                result = s.to_string();
            }
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "maketrans" => {
            if args.is_empty() {
                return Err(PyException::type_error("maketrans() requires at least 1 argument"));
            }
            let mut result_map = IndexMap::new();
            if args.len() == 1 {
                if let PyObjectPayload::Dict(map) = &args[0].payload {
                    for (k, v) in map.read().iter() {
                        let key = match k {
                            HashableKey::Int(n) => n.clone(),
                            HashableKey::Str(s) => {
                                if let Some(c) = s.chars().next() { PyInt::Small(c as i64) } else { continue; }
                            }
                            _ => continue,
                        };
                        result_map.insert(HashableKey::Int(key), v.clone());
                    }
                }
            } else {
                let x = args[0].py_to_string();
                let y = args[1].py_to_string();
                for (cx, cy) in x.chars().zip(y.chars()) {
                    result_map.insert(HashableKey::Int(PyInt::Small(cx as i64)), PyObject::int(cy as i64));
                }
                if args.len() > 2 {
                    let z = args[2].py_to_string();
                    for cz in z.chars() {
                        result_map.insert(HashableKey::Int(PyInt::Small(cz as i64)), PyObject::none());
                    }
                }
            }
            Ok(PyObject::dict(result_map))
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
            let keys: Vec<PyObjectRef> = map.read().keys()
                .filter(|k| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__"))
                .map(|k| k.to_object()).collect();
            Ok(PyObject::list(keys))
        }
        "values" => {
            let r = map.read();
            let vals: Vec<PyObjectRef> = r.iter()
                .filter(|(k, _)| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__"))
                .map(|(_, v)| v.clone()).collect();
            Ok(PyObject::list(vals))
        }
        "items" => {
            let pairs: Vec<PyObjectRef> = map.read().iter()
                .filter(|(k, _)| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__"))
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
        "most_common" => {
            // Counter.most_common(n) — return n most common (key, count) pairs sorted by count
            let r = map.read();
            let mut pairs: Vec<(HashableKey, i64)> = r.iter()
                .filter(|(k, _)| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__"))
                .map(|(k, v)| (k.clone(), v.as_int().unwrap_or(0)))
                .collect();
            pairs.sort_by(|a, b| b.1.cmp(&a.1));
            let n = if !args.is_empty() { args[0].as_int().unwrap_or(pairs.len() as i64) as usize } else { pairs.len() };
            let result: Vec<PyObjectRef> = pairs.into_iter().take(n)
                .map(|(k, v)| PyObject::tuple(vec![k.to_object(), PyObject::int(v)]))
                .collect();
            Ok(PyObject::list(result))
        }
        "elements" => {
            // Counter.elements() — return elements repeated by count
            let r = map.read();
            let mut result = Vec::new();
            for (k, v) in r.iter() {
                if matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__") { continue; }
                let count = v.as_int().unwrap_or(0);
                for _ in 0..count {
                    result.push(k.to_object());
                }
            }
            Ok(PyObject::list(result))
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

fn call_bytes_method(b: &[u8], method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "decode" => {
            // Simple UTF-8 decode
            let s = String::from_utf8_lossy(b);
            Ok(PyObject::str_val(CompactString::from(s)))
        }
        "hex" => Ok(PyObject::str_val(CompactString::from(hex::encode(b)))),
        "count" => {
            if args.is_empty() { return Err(PyException::type_error("count requires an argument")); }
            match &args[0].payload {
                PyObjectPayload::Int(n) => {
                    let byte = n.to_i64().unwrap_or(-1);
                    if byte < 0 || byte > 255 { return Ok(PyObject::int(0)); }
                    Ok(PyObject::int(b.iter().filter(|&&x| x == byte as u8).count() as i64))
                }
                PyObjectPayload::Bytes(needle) | PyObjectPayload::ByteArray(needle) => {
                    if needle.is_empty() { return Ok(PyObject::int(b.len() as i64 + 1)); }
                    let mut count = 0i64;
                    let mut start = 0;
                    while start + needle.len() <= b.len() {
                        if &b[start..start + needle.len()] == needle.as_slice() {
                            count += 1;
                            start += needle.len();
                        } else {
                            start += 1;
                        }
                    }
                    Ok(PyObject::int(count))
                }
                _ => Err(PyException::type_error("a bytes-like object is required")),
            }
        }
        "find" => {
            if args.is_empty() { return Err(PyException::type_error("find requires an argument")); }
            if let PyObjectPayload::Bytes(needle) | PyObjectPayload::ByteArray(needle) = &args[0].payload {
                let pos = b.windows(needle.len()).position(|w| w == needle.as_slice());
                Ok(PyObject::int(pos.map(|p| p as i64).unwrap_or(-1)))
            } else if let Some(n) = args[0].as_int() {
                let byte = n as u8;
                Ok(PyObject::int(b.iter().position(|&x| x == byte).map(|p| p as i64).unwrap_or(-1)))
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "startswith" => {
            if args.is_empty() { return Err(PyException::type_error("startswith requires an argument")); }
            if let PyObjectPayload::Bytes(prefix) | PyObjectPayload::ByteArray(prefix) = &args[0].payload {
                Ok(PyObject::bool_val(b.starts_with(prefix)))
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "endswith" => {
            if args.is_empty() { return Err(PyException::type_error("endswith requires an argument")); }
            if let PyObjectPayload::Bytes(suffix) | PyObjectPayload::ByteArray(suffix) = &args[0].payload {
                Ok(PyObject::bool_val(b.ends_with(suffix)))
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "upper" => Ok(PyObject::bytes(b.to_ascii_uppercase())),
        "lower" => Ok(PyObject::bytes(b.to_ascii_lowercase())),
        "strip" => {
            let stripped = b.iter().copied()
                .skip_while(|c| c.is_ascii_whitespace())
                .collect::<Vec<u8>>();
            let stripped: Vec<u8> = stripped.into_iter().rev()
                .skip_while(|c| c.is_ascii_whitespace())
                .collect::<Vec<u8>>().into_iter().rev().collect();
            Ok(PyObject::bytes(stripped))
        }
        "split" => {
            if args.is_empty() {
                // Split on whitespace
                let parts: Vec<PyObjectRef> = String::from_utf8_lossy(b)
                    .split_whitespace()
                    .map(|s| PyObject::bytes(s.as_bytes().to_vec()))
                    .collect();
                Ok(PyObject::list(parts))
            } else if let PyObjectPayload::Bytes(sep) | PyObjectPayload::ByteArray(sep) = &args[0].payload {
                let mut parts = Vec::new();
                let mut start = 0;
                while start <= b.len() {
                    if let Some(pos) = b[start..].windows(sep.len()).position(|w| w == sep.as_slice()) {
                        parts.push(PyObject::bytes(b[start..start + pos].to_vec()));
                        start = start + pos + sep.len();
                    } else {
                        parts.push(PyObject::bytes(b[start..].to_vec()));
                        break;
                    }
                }
                Ok(PyObject::list(parts))
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "join" => {
            if args.is_empty() { return Err(PyException::type_error("join requires an argument")); }
            // TODO: would need VM-level collect_iterable; simple list case for now
            if let PyObjectPayload::List(items) = &args[0].payload {
                let items = items.read();
                let mut result = Vec::new();
                for (i, item) in items.iter().enumerate() {
                    if i > 0 { result.extend_from_slice(b); }
                    if let PyObjectPayload::Bytes(ib) | PyObjectPayload::ByteArray(ib) = &item.payload {
                        result.extend_from_slice(ib);
                    } else {
                        return Err(PyException::type_error("sequence item: expected a bytes-like object"));
                    }
                }
                Ok(PyObject::bytes(result))
            } else {
                Err(PyException::type_error("can only join an iterable"))
            }
        }
        "replace" => {
            if args.len() < 2 { return Err(PyException::type_error("replace requires 2 arguments")); }
            if let (PyObjectPayload::Bytes(old) | PyObjectPayload::ByteArray(old),
                    PyObjectPayload::Bytes(new) | PyObjectPayload::ByteArray(new)) = (&args[0].payload, &args[1].payload) {
                let s = String::from_utf8_lossy(b);
                let old_s = String::from_utf8_lossy(old);
                let new_s = String::from_utf8_lossy(new);
                Ok(PyObject::bytes(s.replace(old_s.as_ref(), new_s.as_ref()).into_bytes()))
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "isdigit" => Ok(PyObject::bool_val(!b.is_empty() && b.iter().all(|c| c.is_ascii_digit()))),
        "isalpha" => Ok(PyObject::bool_val(!b.is_empty() && b.iter().all(|c| c.is_ascii_alphabetic()))),
        "isalnum" => Ok(PyObject::bool_val(!b.is_empty() && b.iter().all(|c| c.is_ascii_alphanumeric()))),
        "isspace" => Ok(PyObject::bool_val(!b.is_empty() && b.iter().all(|c| c.is_ascii_whitespace()))),
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
        (PyObjectPayload::Tuple(x), PyObjectPayload::Tuple(y)) => {
            for (a_item, b_item) in x.iter().zip(y.iter()) {
                match partial_cmp_for_sort(a_item, b_item) {
                    Some(std::cmp::Ordering::Equal) => continue,
                    other => return other,
                }
            }
            x.len().partial_cmp(&y.len())
        }
        (PyObjectPayload::List(x), PyObjectPayload::List(y)) => {
            let x = x.read(); let y = y.read();
            for (a_item, b_item) in x.iter().zip(y.iter()) {
                match partial_cmp_for_sort(a_item, b_item) {
                    Some(std::cmp::Ordering::Equal) => continue,
                    other => return other,
                }
            }
            x.len().partial_cmp(&y.len())
        }
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
    
    all_attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));
    
    Ok(PyObject::module_with_attrs(CompactString::from("_file"), all_attrs))
}

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

pub(crate) fn make_module(name: &str, attrs: Vec<(&str, PyObjectRef)>) -> PyObjectRef {
    let mut map = IndexMap::new();
    map.insert(CompactString::from("__name__"), PyObject::str_val(CompactString::from(name)));
    for (k, v) in attrs {
        map.insert(CompactString::from(k), v);
    }
    PyObject::module_with_attrs(CompactString::from(name), map)
}

pub(crate) fn make_builtin(f: BuiltinFn) -> PyObjectRef {
    PyObject::native_function("", f)
}

