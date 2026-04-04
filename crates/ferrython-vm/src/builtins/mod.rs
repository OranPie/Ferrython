//! Built-in functions available in Python's builtins module.

mod core_fns;
mod string_methods;
mod type_methods;
mod file_io;
mod instance_methods;

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use indexmap::IndexMap;

use core_fns::*;
use string_methods::*;
use type_methods::*;
use file_io::*;
use instance_methods::*;

pub(crate) use core_fns::{builtin_abs, builtin_dir};
pub(crate) use core_fns::take_import_request;
pub(crate) use type_methods::partial_cmp_for_sort;
pub use instance_methods::resolve_type_class_method;

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
        "map", "filter", "memoryview",
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
                // Lazy iterators need VM context — shouldn't reach here
                IteratorData::Enumerate { .. }
                | IteratorData::Zip { .. }
                | IteratorData::Map { .. }
                | IteratorData::Filter { .. }
                | IteratorData::Sentinel { .. }
                | IteratorData::TakeWhile { .. }
                | IteratorData::DropWhile { .. } => {
                    Err(PyException::type_error("lazy iterator requires VM-level iteration"))
                }
            }
        }
        _ => Err(PyException::type_error("iter_advance on non-iterator")),
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


// ── Argument checking helpers (re-exported from core) ──

#[allow(unused_imports)]
pub(crate) use ferrython_core::object::{check_args, check_args_min, make_module, make_builtin};

// ── Built-in type method dispatch ──

pub fn call_method(receiver: &PyObjectRef, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match &receiver.payload {
        PyObjectPayload::Str(s) => call_str_method(s, method, args),
        PyObjectPayload::List(items) => call_list_method(items.clone(), method, args),
        PyObjectPayload::Dict(map) => call_dict_method(map, method, args),
        PyObjectPayload::InstanceDict(attrs) => call_instance_dict_method(attrs, method, args),
        PyObjectPayload::Int(_) => call_int_method(receiver, method, args),
        PyObjectPayload::Float(f) => call_float_method(*f, method, args),
        PyObjectPayload::Tuple(items) => call_tuple_method(items, method, args),
        PyObjectPayload::Set(m) => call_set_method(m, method, args),
        PyObjectPayload::FrozenSet(m) => call_frozenset_method(m, method, args),
        PyObjectPayload::Bytes(b) => call_bytes_method(b, method, args),
        PyObjectPayload::ByteArray(b) => call_bytearray_method(receiver, b, method, args),
        PyObjectPayload::Complex { real, imag } => {
            match method {
                "conjugate" => Ok(PyObject::complex(*real, -*imag)),
                "__abs__" => Ok(PyObject::float((real * real + imag * imag).sqrt())),
                _ => Err(PyException::attribute_error(format!("'complex' object has no attribute '{}'", method))),
            }
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
    Err(PyException::attribute_error(format!(
        "'{}' object has no attribute '{}'", 
        if let PyObjectPayload::Class(cd) = &inst.class.payload { cd.name.as_str() } else { "instance" },
        method
    )))
}

