//! Built-in functions available in Python's builtins module.

pub(crate) mod core_fns;
pub mod string_methods;
mod type_methods;
mod file_io;
mod instance_methods;

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, is_hidden_dict_key};
use indexmap::IndexMap;

use core_fns::*;
use type_methods::*;
use file_io::*;
use instance_methods::*;

pub(crate) use core_fns::{builtin_abs, builtin_dir};
pub(crate) use core_fns::take_import_request;
pub(crate) use core_fns::unwrap_abstract_fget;
pub(crate) use type_methods::partial_cmp_for_sort;
pub use instance_methods::resolve_type_class_method;

// Direct type-method dispatchers — bypasses call_method's __sizeof__ check + type re-match
pub(crate) use string_methods::call_str_method;
pub(crate) use type_methods::{call_list_method, call_dict_method, call_set_method, call_tuple_method};

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
        "open", "__import__",
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
        "bytes", "bytearray", "complex", "slice", "memoryview",
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
    m.insert(CompactString::from("__debug__"), PyObject::bool_val(true));

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
        ("IOError", ExceptionKind::OSError),           // alias
        ("EnvironmentError", ExceptionKind::OSError),   // alias
        ("OverflowError", ExceptionKind::OverflowError),
        ("PermissionError", ExceptionKind::PermissionError),
        ("RecursionError", ExceptionKind::RecursionError),
        ("RuntimeError", ExceptionKind::RuntimeError),
        ("StopIteration", ExceptionKind::StopIteration),
        ("StopAsyncIteration", ExceptionKind::StopAsyncIteration),
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
        // OS exceptions
        ("TimeoutError", ExceptionKind::TimeoutError),
        ("IsADirectoryError", ExceptionKind::IsADirectoryError),
        ("NotADirectoryError", ExceptionKind::NotADirectoryError),
        ("ProcessLookupError", ExceptionKind::ProcessLookupError),
        ("ConnectionError", ExceptionKind::ConnectionError),
        ("ConnectionResetError", ExceptionKind::ConnectionResetError),
        ("ConnectionAbortedError", ExceptionKind::ConnectionAbortedError),
        ("ConnectionRefusedError", ExceptionKind::ConnectionRefusedError),
        ("InterruptedError", ExceptionKind::InterruptedError),
        ("ChildProcessError", ExceptionKind::ChildProcessError),
        ("BlockingIOError", ExceptionKind::BlockingIOError),
        ("BrokenPipeError", ExceptionKind::BrokenPipeError),
        ("BufferError", ExceptionKind::BufferError),
        ("ReferenceError", ExceptionKind::ReferenceError),
        // Warning subtypes
        ("SyntaxWarning", ExceptionKind::SyntaxWarning),
        ("FutureWarning", ExceptionKind::FutureWarning),
        ("ImportWarning", ExceptionKind::ImportWarning),
        ("UnicodeWarning", ExceptionKind::UnicodeWarning),
        ("BytesWarning", ExceptionKind::BytesWarning),
        ("ResourceWarning", ExceptionKind::ResourceWarning),
        ("PendingDeprecationWarning", ExceptionKind::PendingDeprecationWarning),
        // Indentation
        ("IndentationError", ExceptionKind::IndentationError),
        ("TabError", ExceptionKind::TabError),
        // Python 3.11+ exception groups
        ("ExceptionGroup", ExceptionKind::ExceptionGroup),
        ("BaseExceptionGroup", ExceptionKind::BaseExceptionGroup),
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
        "memoryview" => Some(builtin_memoryview),
        "complex" => Some(builtin_complex),
        "breakpoint" => Some(builtin_breakpoint),
        "help" => Some(builtin_help),
        "__import__" => Some(builtin___import__),
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
            let mut data = iter_data.write();
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
                IteratorData::DictEntries { source, index, cached_tuple } => {
                    let map = unsafe { &*source.data_ptr() };
                    while *index < map.len() {
                        let (hk, _) = map.get_index(*index).unwrap();
                        if !is_hidden_dict_key(hk) { break; }
                        *index += 1;
                    }
                    if *index < map.len() {
                        let (hk, v) = map.get_index(*index).unwrap();
                        let k = hk.to_object();
                        let v = v.clone();
                        *index += 1;
                        let tuple = if let Some(ref ct) = cached_tuple {
                            if PyObjectRef::strong_count(ct) == 1 {
                                unsafe {
                                    let obj_ptr = PyObjectRef::as_ptr(ct) as *mut PyObject;
                                    if let PyObjectPayload::Tuple(ref mut items) = (*obj_ptr).payload {
                                        items[0] = k;
                                        items[1] = v;
                                    }
                                }
                                ct.clone()
                            } else {
                                let t = PyObject::tuple(vec![k, v]);
                                *cached_tuple = Some(t.clone());
                                t
                            }
                        } else {
                            let t = PyObject::tuple(vec![k, v]);
                            *cached_tuple = Some(t.clone());
                            t
                        };
                        Ok(Some((iter_obj.clone(), tuple)))
                    } else { Ok(None) }
                }
                IteratorData::DictKeys { keys, index } => {
                    if *index < keys.len() {
                        let obj = keys[*index].clone();
                        *index += 1;
                        Ok(Some((iter_obj.clone(), obj)))
                    } else { Ok(None) }
                }
                IteratorData::Count { current, step } => {
                    let v = PyObject::int(*current);
                    *current += *step;
                    Ok(Some((iter_obj.clone(), v)))
                }
                IteratorData::Repeat { item, remaining } => {
                    if let Some(ref mut rem) = remaining {
                        if *rem == 0 { Ok(None) }
                        else {
                            *rem -= 1;
                            Ok(Some((iter_obj.clone(), item.clone())))
                        }
                    } else {
                        Ok(Some((iter_obj.clone(), item.clone())))
                    }
                }
                IteratorData::Cycle { items, index } => {
                    if items.is_empty() { Ok(None) }
                    else {
                        let v = items[*index].clone();
                        *index = (*index + 1) % items.len();
                        Ok(Some((iter_obj.clone(), v)))
                    }
                }
                // Lazy iterators that truly need VM context (call user functions)
                IteratorData::Enumerate { .. }
                | IteratorData::Zip { .. }
                | IteratorData::Map { .. }
                | IteratorData::Filter { .. }
                | IteratorData::Sentinel { .. }
                | IteratorData::TakeWhile { .. }
                | IteratorData::DropWhile { .. }
                | IteratorData::Chain { .. }
                | IteratorData::SeqIter { .. }
                | IteratorData::Starmap { .. } => {
                    Err(PyException::type_error("lazy iterator requires VM-level iteration"))
                }
            }
        }
        PyObjectPayload::RangeIter(ri) => {
            let cur = ri.current.get();
            let done = if ri.step > 0 { cur >= ri.stop } else { cur <= ri.stop };
            if done { Ok(None) } else {
                let v = PyObject::int(cur);
                ri.current.set(cur + ri.step);
                Ok(Some((iter_obj.clone(), v)))
            }
        }
        PyObjectPayload::VecIter(data) => {
            let idx = data.index.get();
            if idx < data.items.len() {
                let v = data.items[idx].clone();
                let new_idx = idx + 1;
                data.index.set(if new_idx >= data.items.len() { usize::MAX } else { new_idx });
                Ok(Some((iter_obj.clone(), v)))
            } else { Ok(None) }
        }
        PyObjectPayload::RefIter { source, index } => {
            let idx = index.get();
            match &source.payload {
                PyObjectPayload::List(cell) => {
                    let items = unsafe { &*cell.data_ptr() };
                    if idx < items.len() {
                        let v = items[idx].clone();
                        let new_idx = idx + 1;
                        index.set(if new_idx >= items.len() { usize::MAX } else { new_idx });
                        Ok(Some((iter_obj.clone(), v)))
                    } else { Ok(None) }
                }
                PyObjectPayload::Tuple(items) => {
                    if idx < items.len() {
                        let v = items[idx].clone();
                        let new_idx = idx + 1;
                        index.set(if new_idx >= items.len() { usize::MAX } else { new_idx });
                        Ok(Some((iter_obj.clone(), v)))
                    } else { Ok(None) }
                }
                PyObjectPayload::Dict(cell) | PyObjectPayload::MappingProxy(cell) | PyObjectPayload::DictKeys(cell) => {
                    let map = unsafe { &*cell.data_ptr() };
                    if idx < map.len() {
                        let v = map.get_index(idx).unwrap().0.to_object();
                        index.set(idx + 1);
                        Ok(Some((iter_obj.clone(), v)))
                    } else { Ok(None) }
                }
                _ => Ok(None),
            }
        }
        PyObjectPayload::Module(_) => {
            // File-like objects and other "module-backed" iterators with __next__.
            if let Some(next_fn) = iter_obj.get_attr("__next__") {
                match &next_fn.payload {
                    PyObjectPayload::NativeFunction(nf) => {
                        match (nf.func)(&[iter_obj.clone()]) {
                            Ok(v) => Ok(Some((iter_obj.clone(), v))),
                            Err(e) if e.kind == ExceptionKind::StopIteration => Ok(None),
                            Err(e) => Err(e),
                        }
                    }
                    PyObjectPayload::NativeClosure(nc) => {
                        match (nc.func)(&[iter_obj.clone()]) {
                            Ok(v) => Ok(Some((iter_obj.clone(), v))),
                            Err(e) if e.kind == ExceptionKind::StopIteration => Ok(None),
                            Err(e) => Err(e),
                        }
                    }
                    _ => Err(PyException::type_error("module __next__ is not callable from iter_advance")),
                }
            } else {
                Err(PyException::type_error(format!(
                    "'{}' object is not an iterator", iter_obj.type_name()
                )))
            }
        }
        _ => Err(PyException::type_error("iter_advance on non-iterator")),
    }
}

/// Advance an in-place iterator, returning only the next value.
/// Avoids cloning the iterator itself (used in ForIter hot path).
pub fn iter_next_value(iter_obj: &PyObjectRef) -> PyResult<Option<PyObjectRef>> {
    match &iter_obj.payload {
        PyObjectPayload::Iterator(iter_data) => {
            use ferrython_core::object::IteratorData;
            let mut data = iter_data.write();
            match &mut *data {
                IteratorData::List { items, index } => {
                    if *index < items.len() {
                        let v = items[*index].clone();
                        *index += 1;
                        Ok(Some(v))
                    } else { Ok(None) }
                }
                IteratorData::Tuple { items, index } => {
                    if *index < items.len() {
                        let v = items[*index].clone();
                        *index += 1;
                        Ok(Some(v))
                    } else { Ok(None) }
                }
                IteratorData::Range { current, stop, step } => {
                    let done = if *step > 0 { *current >= *stop } else { *current <= *stop };
                    if done { Ok(None) } else {
                        let v = PyObject::int(*current);
                        *current += *step;
                        Ok(Some(v))
                    }
                }
                IteratorData::Str { chars, index } => {
                    if *index < chars.len() {
                        let v = PyObject::str_val(CompactString::from(chars[*index].to_string()));
                        *index += 1;
                        Ok(Some(v))
                    } else { Ok(None) }
                }
                IteratorData::DictEntries { source, index, cached_tuple } => {
                    let map = unsafe { &*source.data_ptr() };
                    while *index < map.len() {
                        let (hk, _) = map.get_index(*index).unwrap();
                        if !is_hidden_dict_key(hk) { break; }
                        *index += 1;
                    }
                    if *index < map.len() {
                        let (hk, v) = map.get_index(*index).unwrap();
                        let k = hk.to_object();
                        let v = v.clone();
                        *index += 1;
                        let tuple = if let Some(ref ct) = cached_tuple {
                            if PyObjectRef::strong_count(ct) == 1 {
                                unsafe {
                                    let obj_ptr = PyObjectRef::as_ptr(ct) as *mut PyObject;
                                    if let PyObjectPayload::Tuple(ref mut items) = (*obj_ptr).payload {
                                        items[0] = k;
                                        items[1] = v;
                                    }
                                }
                                ct.clone()
                            } else {
                                let t = PyObject::tuple(vec![k, v]);
                                *cached_tuple = Some(t.clone());
                                t
                            }
                        } else {
                            let t = PyObject::tuple(vec![k, v]);
                            *cached_tuple = Some(t.clone());
                            t
                        };
                        Ok(Some(tuple))
                    } else { Ok(None) }
                }
                IteratorData::DictKeys { keys, index } => {
                    if *index < keys.len() {
                        let obj = keys[*index].clone();
                        *index += 1;
                        Ok(Some(obj))
                    } else { Ok(None) }
                }
                _ => Err(PyException::type_error("lazy iterator requires VM-level iteration")),
            }
        }
        PyObjectPayload::RangeIter(ri) => {
            let cur = ri.current.get();
            let done = if ri.step > 0 { cur >= ri.stop } else { cur <= ri.stop };
            if done { Ok(None) } else {
                let v = PyObject::int(cur);
                ri.current.set(cur + ri.step);
                Ok(Some(v))
            }
        }
        PyObjectPayload::VecIter(data) => {
            let idx = data.index.get();
            if idx < data.items.len() {
                let v = data.items[idx].clone();
                data.index.set(idx + 1);
                Ok(Some(v))
            } else { Ok(None) }
        }
        PyObjectPayload::RefIter { source, index } => {
            let idx = index.get();
            match &source.payload {
                PyObjectPayload::List(cell) => {
                    let items = unsafe { &*cell.data_ptr() };
                    if idx < items.len() {
                        let v = items[idx].clone();
                        index.set(idx + 1);
                        Ok(Some(v))
                    } else { Ok(None) }
                }
                PyObjectPayload::Tuple(items) => {
                    if idx < items.len() {
                        let v = items[idx].clone();
                        index.set(idx + 1);
                        Ok(Some(v))
                    } else { Ok(None) }
                }
                PyObjectPayload::Dict(cell) | PyObjectPayload::MappingProxy(cell) | PyObjectPayload::DictKeys(cell) => {
                    let map = unsafe { &*cell.data_ptr() };
                    if idx < map.len() {
                        let v = map.get_index(idx).unwrap().0.to_object();
                        index.set(idx + 1);
                        Ok(Some(v))
                    } else { Ok(None) }
                }
                _ => Ok(None),
            }
        }
        _ => Err(PyException::type_error("iter_next_value on non-iterator")),
    }
}

/// Public access to get_iter_from_obj for lazy iterator construction.
pub(crate) fn get_iter_from_obj_pub(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    get_iter_from_obj(obj)
}

/// Apply format spec to an already-converted string value.
pub(crate) fn apply_format_spec_str(s: &str, spec: &str) -> String {
    ferrython_core::object::format_value_spec(s, spec)
}

pub(crate) fn apply_format_spec_int(n: i64, spec: &str) -> String {
    if spec.is_empty() { return n.to_string(); }
    // Parse format spec: [[fill]align][sign][#][0][width][,][.precision][type]
    let chars: Vec<char> = spec.chars().collect();
    let len = chars.len();
    let type_char = chars[len - 1];
    match type_char {
        'd' => format_int_with_spec(n, &n.to_string(), spec),
        'b' => { let s = format!("{:b}", n.unsigned_abs()); let prefix = if n < 0 { "-0b" } else { "0b" }; format!("{}{}", prefix, s) }
        'o' => { let s = format!("{:o}", n.unsigned_abs()); let prefix = if n < 0 { "-0o" } else { "0o" }; format!("{}{}", prefix, s) }
        'x' => { let s = format!("{:x}", n.unsigned_abs()); let prefix = if n < 0 { "-0x" } else { "0x" }; format!("{}{}", prefix, s) }
        'X' => { let s = format!("{:X}", n.unsigned_abs()); let prefix = if n < 0 { "-0X" } else { "0X" }; format!("{}{}", prefix, s) }
        'n' => format_int_with_spec(n, &n.to_string(), spec),
        'c' => { if n >= 0 && n <= 0x10FFFF { char::from_u32(n as u32).map_or_else(|| n.to_string(), |c| c.to_string()) } else { n.to_string() } }
        'e' | 'E' | 'f' | 'F' | 'g' | 'G' | '%' => {
            // Delegate to float formatting
            apply_format_spec_float(n as f64, spec)
        }
        _ => {
            // Try as width specifier: e.g., "5" means right-align in 5 chars
            if let Ok(width) = spec.parse::<usize>() {
                format!("{:>width$}", n, width = width)
            } else {
                format_int_with_spec(n, &n.to_string(), spec)
            }
        }
    }
}

fn format_int_with_spec(n: i64, formatted: &str, spec: &str) -> String {
    // Handle comma separator
    if spec.contains(',') || spec.contains('_') {
        let sep = if spec.contains('_') { '_' } else { ',' };
        let abs_str = n.unsigned_abs().to_string();
        let mut result = String::new();
        for (i, c) in abs_str.chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 { result.push(sep); }
            result.push(c);
        }
        let s: String = result.chars().rev().collect();
        let s = if n < 0 { format!("-{}", s) } else { s };
        // Apply width
        let width = spec.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<usize>().unwrap_or(0);
        if width > 0 { format!("{:>width$}", s, width = width) } else { s }
    } else {
        formatted.to_string()
    }
}

pub(crate) fn apply_format_spec_float(f: f64, spec: &str) -> String {
    if spec.is_empty() { return format_float_repr(f); }
    let chars: Vec<char> = spec.chars().collect();
    let len = chars.len();
    let type_char = chars[len - 1];
    // Extract precision from .N before type char
    let dot_pos = spec.find('.');
    let precision: usize = if let Some(dp) = dot_pos {
        spec[dp+1..len-1].parse().unwrap_or(6)
    } else {
        6
    };
    match type_char {
        'f' | 'F' => format!("{:.prec$}", f, prec = precision),
        'e' => format!("{:.prec$e}", f, prec = precision),
        'E' => format!("{:.prec$E}", f, prec = precision),
        'g' | 'G' => {
            if f.abs() >= 1e-4 && f.abs() < 10f64.powi(precision as i32) {
                let s = format!("{:.prec$}", f, prec = precision);
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            } else {
                format!("{:.prec$e}", f, prec = precision.saturating_sub(1))
            }
        }
        '%' => format!("{:.prec$}%", f * 100.0, prec = precision),
        'n' => format!("{}", f),
        _ => {
            if let Ok(width) = spec.parse::<usize>() {
                format!("{:>width$}", format_float_repr(f), width = width)
            } else {
                format_float_repr(f)
            }
        }
    }
}

pub(crate) fn format_float_repr(f: f64) -> String {
    if f.is_infinite() { return if f > 0.0 { "inf".into() } else { "-inf".into() }; }
    if f.is_nan() { return "nan".into(); }
    let s = format!("{}", f);
    // Python always shows decimal point for float repr
    if !s.contains('.') && !s.contains('e') && !s.contains('E') {
        format!("{}.0", s)
    } else {
        s
    }
}


// ── Argument checking helpers (re-exported from core) ──

#[allow(unused_imports)]
pub(crate) use ferrython_core::object::{check_args, check_args_min, make_module, make_builtin};

// ── Built-in type method dispatch ──

pub fn call_method(receiver: &PyObjectRef, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Universal methods available on all types
    if method == "__sizeof__" {
        let base = std::mem::size_of::<PyObject>() as i64;
        let extra = match &receiver.payload {
            PyObjectPayload::Str(s) => s.len() as i64,
            PyObjectPayload::Bytes(b) => b.len() as i64,
            PyObjectPayload::ByteArray(b) => b.len() as i64,
            PyObjectPayload::List(items) => (items.read().len() * std::mem::size_of::<PyObjectRef>()) as i64,
            PyObjectPayload::Dict(map) | PyObjectPayload::MappingProxy(map) => (map.read().len() * 64) as i64,
            PyObjectPayload::Set(set) => (set.read().len() * 32) as i64,
            PyObjectPayload::FrozenSet(set) => (set.len() * 32) as i64,
            PyObjectPayload::Tuple(items) => (items.len() * std::mem::size_of::<PyObjectRef>()) as i64,
            _ => 0,
        };
        return Ok(PyObject::int(base + extra));
    }
    match &receiver.payload {
        PyObjectPayload::Str(s) => call_str_method(s, method, args),
        PyObjectPayload::List(items) => call_list_method(items, method, args),
        PyObjectPayload::Dict(map) | PyObjectPayload::MappingProxy(map) => call_dict_method(map, method, args),
        PyObjectPayload::InstanceDict(attrs) => call_instance_dict_method(attrs, method, args),
        PyObjectPayload::Int(_) => call_int_method(receiver, method, args),
        PyObjectPayload::Float(f) => call_float_method(*f, method, args),
        PyObjectPayload::Tuple(items) => call_tuple_method(items, method, args),
        PyObjectPayload::Set(m) => call_set_method(m, method, args),
        PyObjectPayload::FrozenSet(m) => call_frozenset_method(m, method, args),
        PyObjectPayload::Bytes(b) => call_bytes_method(b, method, args),
        PyObjectPayload::ByteArray(b) => call_bytearray_method(receiver, b, method, args),
        PyObjectPayload::Complex { real, imag } => {
            call_complex_method(receiver, *real, *imag, method, args)
        }
        PyObjectPayload::Instance(inst) => call_instance_method(inst, method, args),
        _ => Err(PyException::attribute_error(format!(
            "'{}' object has no attribute '{}'", receiver.type_name(), method
        ))),
    }
}

fn call_instance_method(inst: &ferrython_core::object::InstanceData, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Dict subclass: delegate dict methods to dict_storage
    if let Some(ref ds) = inst.dict_storage {
        return call_dict_method(ds, method, args);
    }
    // Namedtuple methods
    if inst.class.get_attr("__namedtuple__").is_some() {
        return call_namedtuple_method(inst, method, args);
    }
    // Deque methods (except extend/extendleft which need VM for iterable collection)
    if inst.attrs.read().contains_key("__deque__") {
        return call_deque_method(inst, method, args);
    }
    // StringIO methods
    if inst.attrs.read().contains_key("__stringio__") {
        return call_stringio_method(inst, method, args);
    }
    // BytesIO methods
    if inst.attrs.read().contains_key("__bytesio__") {
        return call_bytesio_method(inst, method, args);
    }
    // pathlib.Path methods
    if inst.attrs.read().contains_key("__pathlib_path__") {
        return call_pathlib_method(inst, method, args);
    }
    // datetime methods (strftime, isoformat, timestamp, replace, total_seconds)
    if inst.attrs.read().contains_key("__datetime__") {
        return call_datetime_method(inst, method, args);
    }
    // timedelta methods (total_seconds)
    if inst.attrs.read().contains_key("__timedelta__") {
        return call_timedelta_method(inst, method, args);
    }
    // queue.Queue / LifoQueue / PriorityQueue methods
    if inst.attrs.read().contains_key("__queue__") {
        return call_queue_method(inst, method, args);
    }
    // CSV writer methods
    if inst.attrs.read().contains_key("__csv_writer__") {
        return call_csv_writer_method(inst, method, args);
    }
    // CSV DictWriter methods
    if inst.attrs.read().contains_key("__csv_dictwriter__") {
        return call_csv_dictwriter_method(inst, method, args);
    }
    // Hashlib hash object methods
    let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload { cd.name.to_string() } else { String::new() };
    if matches!(class_name.as_str(), "md5" | "sha1" | "sha256" | "sha224" | "sha384" | "sha512") {
        return call_hashlib_method(inst, method, args);
    }
    // Builtin type subclass: delegate to the underlying value's method dispatch
    if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
        return call_method(&val, method, args);
    }
    Err(PyException::attribute_error(format!(
        "'{}' object has no attribute '{}'", 
        if let PyObjectPayload::Class(cd) = &inst.class.payload { cd.name.as_str() } else { "instance" },
        method
    )))
}

// Helpers to coerce arg into (real, imag). Returns None if not a numeric type.
fn to_complex_parts(x: &PyObjectRef) -> Option<(f64, f64)> {
    match &x.payload {
        PyObjectPayload::Complex { real, imag } => Some((*real, *imag)),
        PyObjectPayload::Int(n) => Some((n.to_f64(), 0.0)),
        PyObjectPayload::Float(f) => Some((*f, 0.0)),
        PyObjectPayload::Bool(b) => Some((if *b { 1.0 } else { 0.0 }, 0.0)),
        _ => None,
    }
}

fn call_complex_method(receiver: &PyObjectRef, real: f64, imag: f64, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use ferrython_core::error::ExceptionKind;
    match method {
        "conjugate" => Ok(PyObject::complex(real, -imag)),
        "__abs__" => Ok(PyObject::float((real * real + imag * imag).sqrt())),
        "__neg__" => Ok(PyObject::complex(-real, -imag)),
        "__pos__" => Ok(PyObject::complex(real, imag)),
        "__bool__" => Ok(PyObject::bool_val(real != 0.0 || imag != 0.0)),
        "__complex__" => Ok(receiver.clone()),
        "__hash__" => {
            // Simple hash: combine real and imag hashes (Python uses a prime-based scheme)
            let rh = real.to_bits() as i64;
            let ih = imag.to_bits() as i64;
            Ok(PyObject::int(rh ^ ih.wrapping_mul(1_000_003)))
        }
        "__repr__" | "__str__" => Ok(PyObject::str_val(CompactString::from(format_complex_repr(real, imag)))),
        "__format__" => {
            let spec = args.first().and_then(|a| a.as_str().map(|s| s.to_string())).unwrap_or_default();
            if spec.is_empty() {
                Ok(PyObject::str_val(CompactString::from(format_complex_repr(real, imag))))
            } else {
                let s = ferrython_core::object::methods_format::format_complex_with_spec_pub(real, imag, &spec)?;
                Ok(PyObject::str_val(CompactString::from(s)))
            }
        }
        "__getnewargs__" => Ok(PyObject::tuple(vec![PyObject::float(real), PyObject::float(imag)])),
        "__eq__" | "__ne__" => {
            let Some(other) = args.first() else { return Ok(PyObject::not_implemented()); };
            let eq = match &other.payload {
                PyObjectPayload::Complex { real: r2, imag: i2 } => real == *r2 && imag == *i2,
                PyObjectPayload::Int(n) => imag == 0.0 && real == n.to_f64() && n.to_i64().map(|i| (real as i64) == i).unwrap_or(false),
                PyObjectPayload::Float(f) => imag == 0.0 && real == *f,
                PyObjectPayload::Bool(b) => imag == 0.0 && real == (if *b {1.0} else {0.0}),
                _ => return Ok(PyObject::not_implemented()),
            };
            Ok(PyObject::bool_val(if method == "__eq__" { eq } else { !eq }))
        }
        "__lt__" | "__le__" | "__gt__" | "__ge__" => Ok(PyObject::not_implemented()),
        "__add__" | "__radd__" => {
            let Some((r2, i2)) = args.first().and_then(to_complex_parts) else { return Ok(PyObject::not_implemented()); };
            Ok(PyObject::complex(real + r2, imag + i2))
        }
        "__sub__" => {
            let Some((r2, i2)) = args.first().and_then(to_complex_parts) else { return Ok(PyObject::not_implemented()); };
            Ok(PyObject::complex(real - r2, imag - i2))
        }
        "__rsub__" => {
            let Some((r2, i2)) = args.first().and_then(to_complex_parts) else { return Ok(PyObject::not_implemented()); };
            Ok(PyObject::complex(r2 - real, i2 - imag))
        }
        "__mul__" | "__rmul__" => {
            let Some((r2, i2)) = args.first().and_then(to_complex_parts) else { return Ok(PyObject::not_implemented()); };
            Ok(PyObject::complex(real * r2 - imag * i2, real * i2 + imag * r2))
        }
        "__truediv__" => {
            let Some((r2, i2)) = args.first().and_then(to_complex_parts) else { return Ok(PyObject::not_implemented()); };
            complex_truediv(real, imag, r2, i2)
        }
        "__rtruediv__" => {
            let Some((r2, i2)) = args.first().and_then(to_complex_parts) else { return Ok(PyObject::not_implemented()); };
            complex_truediv(r2, i2, real, imag)
        }
        "__floordiv__" | "__rfloordiv__" | "__mod__" | "__rmod__" | "__divmod__" => {
            Err(PyException::type_error(format!("can't take floor or mod of complex number.")))
        }
        "__pow__" => {
            let Some((r2, i2)) = args.first().and_then(to_complex_parts) else { return Ok(PyObject::not_implemented()); };
            complex_pow(real, imag, r2, i2)
        }
        "__rpow__" => {
            let Some((r2, i2)) = args.first().and_then(to_complex_parts) else { return Ok(PyObject::not_implemented()); };
            complex_pow(r2, i2, real, imag)
        }
        _ => Err(PyException::attribute_error(format!("'complex' object has no attribute '{}'", method))),
    }
    .map_err(|e| {
        let _ = ExceptionKind::SyntaxError; // keep import used
        e
    })
}

fn format_complex_repr(real: f64, imag: f64) -> String {
    // Match Python: if real==0 and +0 sign, just "Nj"; else "(R+Ij)" with sign handling
    let is_real_zero = real == 0.0 && !real.is_sign_negative();
    if is_real_zero {
        format!("{}j", fmt_float(imag))
    } else {
        let imag_str = fmt_float(imag);
        let sep = if imag_str.starts_with('-') || imag_str.starts_with('+') { "" } else { "+" };
        format!("({}{}{}j)", fmt_float(real), sep, imag_str)
    }
}

fn fmt_float(f: f64) -> String {
    if f.is_nan() { return "nan".to_string(); }
    if f.is_infinite() { return if f > 0.0 { "inf".to_string() } else { "-inf".to_string() }; }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0".to_string() } else { "0".to_string() };
    }
    if f == f.trunc() && f.abs() < 1e16 {
        return format!("{}", f as i64);
    }
    let s = format!("{}", f);
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{}.0", s)
    }
}

fn complex_truediv(ar: f64, ai: f64, br: f64, bi: f64) -> PyResult<PyObjectRef> {
    if br == 0.0 && bi == 0.0 {
        return Err(PyException::zero_division_error("complex division by zero"));
    }
    // Numerically stable version (CPython's _Py_c_quot)
    if bi.abs() <= br.abs() {
        let ratio = bi / br;
        let denom = br + bi * ratio;
        Ok(PyObject::complex((ar + ai * ratio) / denom, (ai - ar * ratio) / denom))
    } else {
        let ratio = br / bi;
        let denom = br * ratio + bi;
        Ok(PyObject::complex((ar * ratio + ai) / denom, (ai * ratio - ar) / denom))
    }
}

fn complex_pow(ar: f64, ai: f64, br: f64, bi: f64) -> PyResult<PyObjectRef> {
    if ar == 0.0 && ai == 0.0 {
        if bi != 0.0 || br < 0.0 {
            return Err(PyException::new(
                ferrython_core::error::ExceptionKind::ZeroDivisionError,
                "0.0 to a negative or complex power".to_string(),
            ));
        }
        if br == 0.0 { return Ok(PyObject::complex(1.0, 0.0)); }
        return Ok(PyObject::complex(0.0, 0.0));
    }
    // a = r * e^(i*theta); a^b = r^br * e^(-bi*theta) * e^(i*(bi*ln(r) + br*theta))
    let r = (ar * ar + ai * ai).sqrt();
    let theta = ai.atan2(ar);
    let new_r = r.powf(br) * (-bi * theta).exp();
    let new_theta = bi * r.ln() + br * theta;
    if !new_r.is_finite() || !new_theta.is_finite() {
        return Err(PyException::new(
            ferrython_core::error::ExceptionKind::OverflowError,
            "complex exponentiation".to_string(),
        ));
    }
    Ok(PyObject::complex(new_r * new_theta.cos(), new_r * new_theta.sin()))
}

