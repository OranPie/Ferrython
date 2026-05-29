use crate::error::{ExceptionKind, PyException};
use crate::object::payload::ExceptionInstanceData;
use compact_str::CompactString;

use super::*;

pub(super) fn exception_type_attr(
    obj: &PyObjectRef,
    kind: ExceptionKind,
    name: &str,
) -> Option<PyObjectRef> {
    match name {
        "__name__" | "__qualname__" => Some(PyObject::str_val(CompactString::from(format!(
            "{:?}",
            kind
        )))),
        "__bases__" => {
            // Return the parent exception type in the hierarchy
            use crate::error::ExceptionKind;
            let parent = match kind {
                ExceptionKind::BaseException => None,
                ExceptionKind::Exception
                | ExceptionKind::SystemExit
                | ExceptionKind::KeyboardInterrupt
                | ExceptionKind::GeneratorExit => Some(ExceptionKind::BaseException),
                ExceptionKind::ArithmeticError
                | ExceptionKind::LookupError
                | ExceptionKind::OSError
                | ExceptionKind::ValueError
                | ExceptionKind::Warning
                | ExceptionKind::ImportError
                | ExceptionKind::RuntimeError
                | ExceptionKind::SyntaxError
                | ExceptionKind::NameError
                | ExceptionKind::TypeError
                | ExceptionKind::AttributeError
                | ExceptionKind::AssertionError
                | ExceptionKind::BufferError
                | ExceptionKind::EOFError
                | ExceptionKind::MemoryError
                | ExceptionKind::ReferenceError
                | ExceptionKind::SystemError
                | ExceptionKind::StopIteration
                | ExceptionKind::StopAsyncIteration => Some(ExceptionKind::Exception),
                ExceptionKind::FloatingPointError
                | ExceptionKind::OverflowError
                | ExceptionKind::ZeroDivisionError => Some(ExceptionKind::ArithmeticError),
                ExceptionKind::IndexError | ExceptionKind::KeyError => {
                    Some(ExceptionKind::LookupError)
                }
                ExceptionKind::FileExistsError
                | ExceptionKind::FileNotFoundError
                | ExceptionKind::PermissionError
                | ExceptionKind::TimeoutError
                | ExceptionKind::IsADirectoryError
                | ExceptionKind::NotADirectoryError
                | ExceptionKind::ProcessLookupError
                | ExceptionKind::ConnectionError
                | ExceptionKind::InterruptedError
                | ExceptionKind::ChildProcessError
                | ExceptionKind::BlockingIOError
                | ExceptionKind::BrokenPipeError => Some(ExceptionKind::OSError),
                ExceptionKind::ConnectionResetError
                | ExceptionKind::ConnectionAbortedError
                | ExceptionKind::ConnectionRefusedError => Some(ExceptionKind::ConnectionError),
                ExceptionKind::UnicodeError
                | ExceptionKind::UnicodeDecodeError
                | ExceptionKind::UnicodeEncodeError
                | ExceptionKind::UnicodeTranslateError => Some(ExceptionKind::ValueError),
                ExceptionKind::JSONDecodeError | ExceptionKind::CsvError => {
                    Some(ExceptionKind::ValueError)
                }
                ExceptionKind::ModuleNotFoundError => Some(ExceptionKind::ImportError),
                ExceptionKind::NotImplementedError
                | ExceptionKind::RecursionError
                | ExceptionKind::ReError => Some(ExceptionKind::RuntimeError),
                ExceptionKind::UnboundLocalError => Some(ExceptionKind::NameError),
                ExceptionKind::IndentationError => Some(ExceptionKind::SyntaxError),
                ExceptionKind::TabError => Some(ExceptionKind::IndentationError),
                ExceptionKind::SubprocessError => Some(ExceptionKind::Exception),
                ExceptionKind::CalledProcessError | ExceptionKind::TimeoutExpired => {
                    Some(ExceptionKind::SubprocessError)
                }
                ExceptionKind::DeprecationWarning
                | ExceptionKind::RuntimeWarning
                | ExceptionKind::UserWarning
                | ExceptionKind::SyntaxWarning
                | ExceptionKind::FutureWarning
                | ExceptionKind::ImportWarning
                | ExceptionKind::UnicodeWarning
                | ExceptionKind::EncodingWarning
                | ExceptionKind::BytesWarning
                | ExceptionKind::ResourceWarning
                | ExceptionKind::PendingDeprecationWarning => Some(ExceptionKind::Warning),
                ExceptionKind::BaseExceptionGroup => Some(ExceptionKind::BaseException),
                ExceptionKind::ExceptionGroup => Some(ExceptionKind::BaseExceptionGroup),
            };
            let bases = match parent {
                Some(p) => vec![PyObject::exception_type(p)],
                None => vec![PyObject::builtin_type(CompactString::from("object"))],
            };
            Some(PyObject::tuple(bases))
        }
        "__mro__" => {
            // Build the MRO chain by walking up the hierarchy
            use crate::error::ExceptionKind;
            let mut mro = vec![obj.clone()];
            let mut current = kind;
            loop {
                let parent = match current {
                    ExceptionKind::BaseException => break,
                    ExceptionKind::Exception
                    | ExceptionKind::SystemExit
                    | ExceptionKind::KeyboardInterrupt
                    | ExceptionKind::GeneratorExit => ExceptionKind::BaseException,
                    ExceptionKind::ArithmeticError
                    | ExceptionKind::LookupError
                    | ExceptionKind::OSError
                    | ExceptionKind::ValueError
                    | ExceptionKind::Warning
                    | ExceptionKind::ImportError
                    | ExceptionKind::RuntimeError
                    | ExceptionKind::SyntaxError
                    | ExceptionKind::NameError
                    | ExceptionKind::TypeError
                    | ExceptionKind::AttributeError
                    | ExceptionKind::AssertionError
                    | ExceptionKind::BufferError
                    | ExceptionKind::EOFError
                    | ExceptionKind::MemoryError
                    | ExceptionKind::ReferenceError
                    | ExceptionKind::SystemError
                    | ExceptionKind::StopIteration
                    | ExceptionKind::StopAsyncIteration => ExceptionKind::Exception,
                    ExceptionKind::FloatingPointError
                    | ExceptionKind::OverflowError
                    | ExceptionKind::ZeroDivisionError => ExceptionKind::ArithmeticError,
                    ExceptionKind::IndexError | ExceptionKind::KeyError => {
                        ExceptionKind::LookupError
                    }
                    ExceptionKind::FileExistsError
                    | ExceptionKind::FileNotFoundError
                    | ExceptionKind::PermissionError
                    | ExceptionKind::TimeoutError
                    | ExceptionKind::IsADirectoryError
                    | ExceptionKind::NotADirectoryError
                    | ExceptionKind::ProcessLookupError
                    | ExceptionKind::ConnectionError
                    | ExceptionKind::InterruptedError
                    | ExceptionKind::ChildProcessError
                    | ExceptionKind::BlockingIOError
                    | ExceptionKind::BrokenPipeError => ExceptionKind::OSError,
                    ExceptionKind::ConnectionResetError
                    | ExceptionKind::ConnectionAbortedError
                    | ExceptionKind::ConnectionRefusedError => ExceptionKind::ConnectionError,
                    ExceptionKind::UnicodeError
                    | ExceptionKind::UnicodeDecodeError
                    | ExceptionKind::UnicodeEncodeError
                    | ExceptionKind::UnicodeTranslateError => ExceptionKind::ValueError,
                    ExceptionKind::JSONDecodeError | ExceptionKind::CsvError => {
                        ExceptionKind::ValueError
                    }
                    ExceptionKind::ModuleNotFoundError => ExceptionKind::ImportError,
                    ExceptionKind::NotImplementedError
                    | ExceptionKind::RecursionError
                    | ExceptionKind::ReError => ExceptionKind::RuntimeError,
                    ExceptionKind::UnboundLocalError => ExceptionKind::NameError,
                    ExceptionKind::IndentationError => ExceptionKind::SyntaxError,
                    ExceptionKind::TabError => ExceptionKind::IndentationError,
                    ExceptionKind::SubprocessError => ExceptionKind::Exception,
                    ExceptionKind::CalledProcessError | ExceptionKind::TimeoutExpired => {
                        ExceptionKind::SubprocessError
                    }
                    ExceptionKind::DeprecationWarning
                    | ExceptionKind::RuntimeWarning
                    | ExceptionKind::UserWarning
                    | ExceptionKind::SyntaxWarning
                    | ExceptionKind::FutureWarning
                    | ExceptionKind::ImportWarning
                    | ExceptionKind::UnicodeWarning
                    | ExceptionKind::EncodingWarning
                    | ExceptionKind::BytesWarning
                    | ExceptionKind::ResourceWarning
                    | ExceptionKind::PendingDeprecationWarning => ExceptionKind::Warning,
                    ExceptionKind::BaseExceptionGroup => ExceptionKind::BaseException,
                    ExceptionKind::ExceptionGroup => ExceptionKind::BaseExceptionGroup,
                };
                mro.push(PyObject::exception_type(parent));
                current = parent;
            }
            mro.push(PyObject::builtin_type(CompactString::from("object")));
            Some(PyObject::tuple(mro))
        }
        "__module__" => Some(PyObject::str_val(CompactString::from("builtins"))),
        "__class__" => Some(PyObject::builtin_type(CompactString::from("type"))),
        "__init__" => {
            // Unbound __init__: ExcType.__init__(self, *args)
            Some(PyObject::native_function("__init__", |args| {
                if args.is_empty() {
                    return Ok(PyObject::none());
                }
                let target = &args[0];
                let init_args: Vec<PyObjectRef> = args[1..].to_vec();
                match &target.payload {
                    PyObjectPayload::Instance(idata) => {
                        idata
                            .attrs
                            .write()
                            .insert(CompactString::from("args"), PyObject::tuple(init_args));
                    }
                    PyObjectPayload::ExceptionInstance(ei) => {
                        let mut attrs = ei.ensure_attrs().write();
                        attrs.insert(
                            CompactString::from("args"),
                            PyObject::tuple(init_args.clone()),
                        );
                        if ei.kind.is_subclass_of(&ExceptionKind::ImportError) {
                            attrs.insert(
                                CompactString::from("msg"),
                                init_args.first().cloned().unwrap_or_else(PyObject::none),
                            );
                            attrs.insert(CompactString::from("name"), PyObject::none());
                            attrs.insert(CompactString::from("path"), PyObject::none());
                        }
                        match ei.kind {
                            ExceptionKind::UnicodeEncodeError
                            | ExceptionKind::UnicodeDecodeError
                                if init_args.len() >= 5 =>
                            {
                                attrs.insert(CompactString::from("encoding"), init_args[0].clone());
                                attrs.insert(CompactString::from("object"), init_args[1].clone());
                                attrs.insert(CompactString::from("start"), init_args[2].clone());
                                attrs.insert(CompactString::from("end"), init_args[3].clone());
                                attrs.insert(CompactString::from("reason"), init_args[4].clone());
                            }
                            ExceptionKind::UnicodeTranslateError if init_args.len() >= 4 => {
                                attrs.insert(CompactString::from("object"), init_args[0].clone());
                                attrs.insert(CompactString::from("start"), init_args[1].clone());
                                attrs.insert(CompactString::from("end"), init_args[2].clone());
                                attrs.insert(CompactString::from("reason"), init_args[3].clone());
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
                Ok(PyObject::none())
            }))
        }
        "__new__" => {
            let new_kind = kind;
            Some(PyObject::native_closure("__new__", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("__new__ requires cls"));
                }
                let cls = &args[0];
                let actual_kind = match &cls.payload {
                    PyObjectPayload::ExceptionType(actual) => *actual,
                    _ => {
                        return Err(PyException::type_error(
                            "exception __new__ requires an exception type",
                        ))
                    }
                };
                if !actual_kind.is_subclass_of(&new_kind) {
                    return Err(PyException::type_error(format!(
                        "{}.__new__({}): {} is not a subtype of {}",
                        new_kind, actual_kind, actual_kind, new_kind
                    )));
                }
                Ok(PyObject::exception_instance_with_args(
                    actual_kind,
                    CompactString::default(),
                    vec![],
                ))
            }))
        }
        "__str__" => Some(PyObject::native_function("__str__", |args| {
            if args.is_empty() {
                return Ok(PyObject::str_val(CompactString::from("")));
            }
            Ok(PyObject::str_val(CompactString::from(
                args[0].py_to_string(),
            )))
        })),
        "__repr__" => {
            let kind_clone = kind;
            Some(PyObject::native_closure("__repr__", move |args| {
                if args.is_empty() {
                    return Ok(PyObject::str_val(CompactString::from(format!(
                        "{:?}()",
                        kind_clone
                    ))));
                }
                let s = args[0].py_to_string();
                Ok(PyObject::str_val(CompactString::from(format!(
                    "{:?}({})",
                    kind_clone, s
                ))))
            }))
        }
        "mro" => {
            // mro() method: returns __mro__ as a list
            let mro_val = obj.get_attr("__mro__");
            Some(PyObject::native_closure("mro", move |_args| {
                if let Some(ref mro_tuple) = mro_val {
                    if let PyObjectPayload::Tuple(items) = &mro_tuple.payload {
                        return Ok(PyObject::list((**items).clone()));
                    }
                }
                Ok(PyObject::list(vec![]))
            }))
        }
        "__subclasses__" => Some(PyObject::native_function("__subclasses__", |_args| {
            Ok(PyObject::list(vec![]))
        })),
        _ => None,
    }
}

pub(super) fn exception_instance_attr(
    obj: &PyObjectRef,
    ei: &ExceptionInstanceData,
    name: &str,
) -> Option<PyObjectRef> {
    match name {
        "args" => {
            // Check attrs first (may be overwritten by __init__)
            if let Some(v) = ei.get_attrs().and_then(|a| a.read().get("args").cloned()) {
                return Some(v);
            }
            if ei.args.is_empty() {
                if ei.message.is_empty() {
                    Some(PyObject::tuple(vec![]))
                } else {
                    Some(PyObject::tuple(vec![PyObject::str_val(ei.message.clone())]))
                }
            } else {
                Some(PyObject::tuple(ei.args.clone()))
            }
        }
        "__class__" => Some(PyObject::exception_type(ei.kind)),
        "__init__" => {
            let obj_ref = obj.clone();
            Some(PyObject::native_closure("__init__", move |args| {
                if let PyObjectPayload::ExceptionInstance(ref ei) = obj_ref.payload {
                    let init_args = args.to_vec();
                    let mut attrs = ei.ensure_attrs().write();
                    attrs.insert(
                        CompactString::from("args"),
                        PyObject::tuple(init_args.clone()),
                    );
                    if ei.kind.is_subclass_of(&ExceptionKind::ImportError) {
                        attrs.insert(
                            CompactString::from("msg"),
                            init_args.first().cloned().unwrap_or_else(PyObject::none),
                        );
                        attrs.insert(CompactString::from("name"), PyObject::none());
                        attrs.insert(CompactString::from("path"), PyObject::none());
                    }
                    match ei.kind {
                        ExceptionKind::UnicodeEncodeError | ExceptionKind::UnicodeDecodeError
                            if init_args.len() >= 5 =>
                        {
                            attrs.insert(CompactString::from("encoding"), init_args[0].clone());
                            attrs.insert(CompactString::from("object"), init_args[1].clone());
                            attrs.insert(CompactString::from("start"), init_args[2].clone());
                            attrs.insert(CompactString::from("end"), init_args[3].clone());
                            attrs.insert(CompactString::from("reason"), init_args[4].clone());
                        }
                        ExceptionKind::UnicodeTranslateError if init_args.len() >= 4 => {
                            attrs.insert(CompactString::from("object"), init_args[0].clone());
                            attrs.insert(CompactString::from("start"), init_args[1].clone());
                            attrs.insert(CompactString::from("end"), init_args[2].clone());
                            attrs.insert(CompactString::from("reason"), init_args[3].clone());
                        }
                        _ => {}
                    }
                }
                Ok(PyObject::none())
            }))
        }
        "__str__" => Some(PyObject::native_function("__str__", |args| {
            if args.is_empty() {
                return Ok(PyObject::str_val(CompactString::new("")));
            }
            Ok(PyObject::str_val(CompactString::from(
                args[0].py_to_string(),
            )))
        })),
        "__repr__" => Some(PyObject::native_function("__repr__", |args| {
            if args.is_empty() {
                return Ok(PyObject::str_val(CompactString::new("")));
            }
            Ok(PyObject::str_val(CompactString::from(args[0].repr())))
        })),
        "code" if ei.kind == ExceptionKind::SystemExit => {
            // SystemExit.code: first arg or message
            if !ei.args.is_empty() {
                Some(ei.args[0].clone())
            } else if !ei.message.is_empty() {
                // Try to parse as int, otherwise return as string
                if let Ok(n) = ei.message.parse::<i64>() {
                    Some(PyObject::int(n))
                } else {
                    Some(PyObject::str_val(ei.message.clone()))
                }
            } else {
                Some(PyObject::none())
            }
        }
        "value" => {
            // StopIteration.value — check attrs first, then args[0], then None
            if let Some(v) = ei.get_attrs().and_then(|a| a.read().get("value").cloned()) {
                Some(v)
            } else if !ei.args.is_empty() {
                Some(ei.args[0].clone())
            } else {
                Some(PyObject::none())
            }
        }
        "msg" if ei.kind.is_subclass_of(&ExceptionKind::ImportError) => ei
            .get_attrs()
            .and_then(|a| a.read().get("msg").cloned())
            .or_else(|| {
                if !ei.args.is_empty() {
                    Some(ei.args[0].clone())
                } else if !ei.message.is_empty() {
                    Some(PyObject::str_val(ei.message.clone()))
                } else {
                    Some(PyObject::none())
                }
            }),
        "name" | "path" if ei.kind.is_subclass_of(&ExceptionKind::ImportError) => ei
            .get_attrs()
            .and_then(|a| a.read().get(name).cloned())
            .or_else(|| Some(PyObject::none())),
        "__cause__" => ei
            .get_attrs()
            .and_then(|a| a.read().get("__cause__").cloned())
            .or_else(|| Some(PyObject::none())),
        "__context__" => ei
            .get_attrs()
            .and_then(|a| a.read().get("__context__").cloned())
            .or_else(|| Some(PyObject::none())),
        "__suppress_context__" => ei
            .get_attrs()
            .and_then(|a| a.read().get("__suppress_context__").cloned())
            .or_else(|| Some(PyObject::bool_val(false))),
        "__traceback__" => ei
            .get_attrs()
            .and_then(|a| a.read().get("__traceback__").cloned())
            .or_else(|| Some(PyObject::none())),
        "__notes__" => ei
            .get_attrs()
            .and_then(|a| a.read().get("__notes__").cloned()),
        "add_note" => {
            let obj_ref = obj.clone();
            Some(PyObject::native_closure("add_note", move |args| {
                if args.is_empty() {
                    return Err(crate::error::PyException::type_error(
                        "add_note() missing required argument: 'note'",
                    ));
                }
                let note = &args[0];
                if let PyObjectPayload::ExceptionInstance(ref ei) = obj_ref.payload {
                    let mut w = ei.ensure_attrs().write();
                    let notes = w
                        .entry(CompactString::from("__notes__"))
                        .or_insert_with(|| PyObject::list(vec![]));
                    if let PyObjectPayload::List(list) = &notes.payload {
                        list.write().push(note.clone());
                    }
                }
                Ok(PyObject::none())
            }))
        }
        "with_traceback" => {
            let obj_ref = obj.clone();
            Some(PyObject::native_closure("with_traceback", move |args| {
                if !args.is_empty() {
                    if let PyObjectPayload::ExceptionInstance(ref ei) = obj_ref.payload {
                        ei.ensure_attrs()
                            .write()
                            .insert(CompactString::from("__traceback__"), args[0].clone());
                    }
                }
                Ok(obj_ref.clone())
            }))
        }
        // OSError attributes: .errno, .strerror, .filename
        "errno" | "strerror" | "filename" if ei.kind.is_subclass_of(&ExceptionKind::OSError) => ei
            .get_attrs()
            .and_then(|a| a.read().get(name).cloned())
            .or_else(|| Some(PyObject::none())),
        _ => {
            // Check user-set attrs (e.g., __cause__)
            ei.get_attrs().and_then(|a| a.read().get(name).cloned())
        }
    }
}

/// Resolve methods on builtin ExceptionType bases (e.g. Exception.__init__).
/// Used by super() proxy when the parent class is a builtin exception type.
pub(super) fn resolve_exception_type_method(
    name: &str,
    instance: &PyObjectRef,
) -> Option<PyObjectRef> {
    match name {
        "__init__" => {
            Some(PyObject::native_function("__init__", |args| {
                // Exception.__init__(self, *args) — only set self.args (CPython behavior)
                if args.is_empty() {
                    return Ok(PyObject::none());
                }
                let target = &args[0];
                if let PyObjectPayload::Instance(idata) = &target.payload {
                    let exc_args: Vec<PyObjectRef> = if args.len() > 1 {
                        args[1..].to_vec()
                    } else {
                        vec![]
                    };
                    idata
                        .attrs
                        .write()
                        .insert(CompactString::from("args"), PyObject::tuple(exc_args));
                }
                Ok(PyObject::none())
            }))
        }
        "__str__" => Some(PyObject::native_function("__str__", |args| {
            if args.is_empty() {
                return Ok(PyObject::str_val(CompactString::from("")));
            }
            let target = &args[0];
            if let Some(a) = target.get_attr("args") {
                if let PyObjectPayload::Tuple(items) = &a.payload {
                    if items.len() == 1 {
                        return Ok(PyObject::str_val(CompactString::from(
                            items[0].py_to_string(),
                        )));
                    } else if items.is_empty() {
                        return Ok(PyObject::str_val(CompactString::from("")));
                    }
                    return Ok(PyObject::str_val(CompactString::from(a.repr())));
                }
            }
            Ok(PyObject::str_val(CompactString::from(String::new())))
        })),
        "__repr__" => Some(PyObject::native_function("__repr__", |args| {
            if args.is_empty() {
                return Ok(PyObject::str_val(CompactString::from("Exception()")));
            }
            let target = &args[0];
            let cls_name = if let PyObjectPayload::Instance(idata) = &target.payload {
                if let PyObjectPayload::Class(cd) = &idata.class.payload {
                    cd.name.to_string()
                } else {
                    "Exception".to_string()
                }
            } else {
                "Exception".to_string()
            };
            if let Some(a) = target.get_attr("args") {
                Ok(PyObject::str_val(CompactString::from(format!(
                    "{}({})",
                    cls_name,
                    a.py_to_string()
                ))))
            } else {
                Ok(PyObject::str_val(CompactString::from(format!(
                    "{}()",
                    cls_name
                ))))
            }
        })),
        "add_note" => Some(PyObject::native_function("add_note", |args| {
            if args.len() < 2 {
                return Err(crate::error::PyException::type_error(
                    "add_note() missing required argument: 'note'",
                ));
            }
            let target = &args[0];
            let note = &args[1];
            if let PyObjectPayload::Instance(idata) = &target.payload {
                let mut w = idata.attrs.write();
                let notes = w
                    .entry(CompactString::from("__notes__"))
                    .or_insert_with(|| PyObject::list(vec![]));
                if let PyObjectPayload::List(list) = &notes.payload {
                    list.write().push(note.clone());
                }
            } else if let PyObjectPayload::ExceptionInstance(ref ei) = target.payload {
                let mut w = ei.ensure_attrs().write();
                let notes = w
                    .entry(CompactString::from("__notes__"))
                    .or_insert_with(|| PyObject::list(vec![]));
                if let PyObjectPayload::List(list) = &notes.payload {
                    list.write().push(note.clone());
                }
            }
            Ok(PyObject::none())
        })),
        "with_traceback" => {
            let inst = instance.clone();
            Some(PyObject::native_closure("with_traceback", move |args| {
                if !args.is_empty() {
                    if let PyObjectPayload::Instance(idata) = &inst.payload {
                        idata
                            .attrs
                            .write()
                            .insert(CompactString::from("__traceback__"), args[0].clone());
                    } else if let PyObjectPayload::ExceptionInstance(ref ei) = inst.payload {
                        ei.ensure_attrs()
                            .write()
                            .insert(CompactString::from("__traceback__"), args[0].clone());
                    }
                }
                Ok(inst.clone())
            }))
        }
        _ => None,
    }
}
