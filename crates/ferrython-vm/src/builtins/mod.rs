//! Built-in functions available in Python's builtins module.

mod core_fns;
mod string_methods;
mod type_methods;
mod file_io;

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::sync::Arc;

use core_fns::*;
use string_methods::*;
use type_methods::*;
use file_io::*;

pub(crate) use core_fns::{builtin_abs, builtin_dir};
pub(crate) use type_methods::partial_cmp_for_sort;

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

#[allow(dead_code)]
pub(crate) fn hashable_key_to_object(key: &HashableKey) -> PyObjectRef { key.to_object() }

pub(crate) fn apply_format_spec(val: &PyObjectRef, spec: &str) -> String {
    match val.format_value(spec) {
        Ok(s) => s,
        Err(_) => val.py_to_string(),
    }
}


// ── Argument checking helpers (re-exported from core) ──

#[allow(unused_imports)]
pub(crate) use ferrython_core::object::{check_args, check_args_min, make_module, make_builtin};

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

