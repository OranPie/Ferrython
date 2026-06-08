use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

use super::exception_group::attach_eg_methods;

fn exception_message_arg(args: &[PyObjectRef]) -> CompactString {
    if args.is_empty() {
        CompactString::default()
    } else if let PyObjectPayload::Str(s) = &args[0].payload {
        s.to_compact_string()
    } else {
        CompactString::from(args[0].py_to_string())
    }
}

fn exception_kwarg<'a>(
    kwargs: &'a [(CompactString, PyObjectRef)],
    name: &str,
) -> Option<&'a PyObjectRef> {
    kwargs
        .iter()
        .rev()
        .find(|(key, _)| key.as_str() == name)
        .map(|(_, value)| value)
}

fn is_unicode_error_kind(kind: ExceptionKind) -> bool {
    matches!(
        kind,
        ExceptionKind::UnicodeEncodeError
            | ExceptionKind::UnicodeDecodeError
            | ExceptionKind::UnicodeTranslateError
    )
}

fn set_syntax_error_attrs(inst: &PyObjectRef, kind: ExceptionKind, args: &[PyObjectRef]) {
    if !kind.is_subclass_of(&ExceptionKind::SyntaxError) {
        return;
    }
    let PyObjectPayload::ExceptionInstance(ei) = &inst.payload else {
        return;
    };
    let mut attrs = ei.ensure_attrs().write();
    attrs.insert(
        CompactString::from("msg"),
        args.first().cloned().unwrap_or_else(PyObject::none),
    );
    attrs.insert(CompactString::from("filename"), PyObject::none());
    attrs.insert(CompactString::from("lineno"), PyObject::none());
    attrs.insert(CompactString::from("offset"), PyObject::none());
    attrs.insert(CompactString::from("text"), PyObject::none());
    attrs.insert(CompactString::from("print_file_and_line"), PyObject::none());

    if args.len() == 2 {
        if let PyObjectPayload::Tuple(items) = &args[1].payload {
            if items.len() >= 4 {
                attrs.insert(CompactString::from("filename"), items[0].clone());
                attrs.insert(CompactString::from("lineno"), items[1].clone());
                attrs.insert(CompactString::from("offset"), items[2].clone());
                attrs.insert(CompactString::from("text"), items[3].clone());
            }
        }
    }
}

fn os_error_kind_for_errno(errno: i64) -> ExceptionKind {
    match errno {
        11 | 114 | 115 => ExceptionKind::BlockingIOError,
        10 => ExceptionKind::ChildProcessError,
        32 | 108 => ExceptionKind::BrokenPipeError,
        103 => ExceptionKind::ConnectionAbortedError,
        111 => ExceptionKind::ConnectionRefusedError,
        104 => ExceptionKind::ConnectionResetError,
        17 => ExceptionKind::FileExistsError,
        2 => ExceptionKind::FileNotFoundError,
        4 => ExceptionKind::InterruptedError,
        21 => ExceptionKind::IsADirectoryError,
        20 => ExceptionKind::NotADirectoryError,
        13 | 1 => ExceptionKind::PermissionError,
        3 => ExceptionKind::ProcessLookupError,
        110 => ExceptionKind::TimeoutError,
        _ => ExceptionKind::OSError,
    }
}

fn set_unicode_error_attrs(inst: &PyObjectRef, kind: ExceptionKind, args: &[PyObjectRef]) {
    let PyObjectPayload::ExceptionInstance(ei) = &inst.payload else {
        return;
    };
    let mut attrs = ei.ensure_attrs().write();
    match kind {
        ExceptionKind::UnicodeEncodeError | ExceptionKind::UnicodeDecodeError => {
            if args.len() >= 5 {
                attrs.insert(CompactString::from("encoding"), args[0].clone());
                let object = if kind == ExceptionKind::UnicodeDecodeError {
                    match &args[1].payload {
                        PyObjectPayload::ByteArray(bytes) => PyObject::bytes((**bytes).clone()),
                        _ => args[1].clone(),
                    }
                } else {
                    args[1].clone()
                };
                attrs.insert(CompactString::from("object"), object);
                attrs.insert(CompactString::from("start"), args[2].clone());
                attrs.insert(CompactString::from("end"), args[3].clone());
                attrs.insert(CompactString::from("reason"), args[4].clone());
            }
        }
        ExceptionKind::UnicodeTranslateError => {
            if args.len() >= 4 {
                attrs.insert(CompactString::from("object"), args[0].clone());
                attrs.insert(CompactString::from("start"), args[1].clone());
                attrs.insert(CompactString::from("end"), args[2].clone());
                attrs.insert(CompactString::from("reason"), args[3].clone());
            }
        }
        _ => {}
    }
}

pub(crate) fn build_builtin_exception_instance(
    mut kind: ExceptionKind,
    args: Vec<PyObjectRef>,
    kwargs: &[(CompactString, PyObjectRef)],
) -> PyResult<PyObjectRef> {
    if !kind.is_subclass_of(&ExceptionKind::ImportError) && !kwargs.is_empty() {
        let (key, _) = &kwargs[0];
        return Err(PyException::type_error(format!(
            "'{}' is an invalid keyword argument for {}",
            key, kind
        )));
    }
    if kind.is_subclass_of(&ExceptionKind::ImportError) {
        for (key, _) in kwargs {
            if key.as_str() != "name" && key.as_str() != "path" {
                return Err(PyException::type_error(format!(
                    "'{}' is an invalid keyword argument for {}",
                    key, kind
                )));
            }
        }
    }

    if kind == ExceptionKind::OSError && args.len() >= 2 {
        if let Some(errno) = args[0].as_int() {
            kind = os_error_kind_for_errno(errno);
        }
    }

    let msg = exception_message_arg(&args);
    let needs_post = matches!(
        kind,
        ExceptionKind::ExceptionGroup | ExceptionKind::BaseExceptionGroup
    ) || (kind.is_subclass_of(&ExceptionKind::OSError) && args.len() >= 2)
        || (kind == ExceptionKind::SystemExit && !args.is_empty())
        || kind.is_subclass_of(&ExceptionKind::ImportError)
        || kind.is_subclass_of(&ExceptionKind::SyntaxError)
        || is_unicode_error_kind(kind);

    if !needs_post {
        return Ok(PyObject::exception_instance_with_args(kind, msg, args));
    }

    let inst = PyObject::exception_instance_with_args(kind, msg, args.clone());

    if matches!(
        kind,
        ExceptionKind::ExceptionGroup | ExceptionKind::BaseExceptionGroup
    ) {
        if let PyObjectPayload::ExceptionInstance(ei) = &inst.payload {
            let mut a = ei.ensure_attrs().write();
            if !args.is_empty() {
                a.insert(CompactString::from("message"), args[0].clone());
            }
            if args.len() >= 2 {
                let exc_list = match &args[1].payload {
                    PyObjectPayload::List(_) => args[1].clone(),
                    PyObjectPayload::Tuple(items) => PyObject::list((**items).clone()),
                    _ => PyObject::list(vec![args[1].clone()]),
                };
                a.insert(CompactString::from("exceptions"), exc_list);
                drop(a);
                attach_eg_methods(&inst);
            }
        }
    }

    set_syntax_error_attrs(&inst, kind, &args);

    if kind.is_subclass_of(&ExceptionKind::OSError) && args.len() >= 2 {
        if let PyObjectPayload::ExceptionInstance(ei) = &inst.payload {
            let mut a = ei.ensure_attrs().write();
            if args.len() >= 3 {
                a.insert(
                    CompactString::from("args"),
                    PyObject::tuple(vec![args[0].clone(), args[1].clone()]),
                );
            }
            a.insert(CompactString::from("errno"), args[0].clone());
            a.insert(CompactString::from("strerror"), args[1].clone());
            if args.len() >= 3 {
                a.insert(CompactString::from("filename"), args[2].clone());
            } else {
                a.insert(CompactString::from("filename"), PyObject::none());
            }
            if args.len() >= 5 {
                a.insert(CompactString::from("filename2"), args[4].clone());
            } else {
                a.insert(CompactString::from("filename2"), PyObject::none());
            }
            if kind == ExceptionKind::BlockingIOError
                && args.len() >= 3
                && args[2].as_int().is_some()
            {
                a.insert(CompactString::from("characters_written"), args[2].clone());
            }
        }
    }

    if kind == ExceptionKind::SystemExit && !args.is_empty() {
        if let PyObjectPayload::ExceptionInstance(ei) = &inst.payload {
            ei.ensure_attrs()
                .write()
                .insert(CompactString::from("code"), args[0].clone());
        }
    }

    if kind.is_subclass_of(&ExceptionKind::ImportError) {
        if let PyObjectPayload::ExceptionInstance(ei) = &inst.payload {
            let mut a = ei.ensure_attrs().write();
            a.insert(
                CompactString::from("msg"),
                args.first().cloned().unwrap_or_else(PyObject::none),
            );
            a.insert(
                CompactString::from("name"),
                exception_kwarg(kwargs, "name")
                    .cloned()
                    .unwrap_or_else(PyObject::none),
            );
            a.insert(
                CompactString::from("path"),
                exception_kwarg(kwargs, "path")
                    .cloned()
                    .unwrap_or_else(PyObject::none),
            );
        }
    }

    set_unicode_error_attrs(&inst, kind, &args);

    Ok(inst)
}
