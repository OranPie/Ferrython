use super::*;

/// Default sys.excepthook: prints exception to stderr.
pub(super) fn sys_excepthook_default(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error("excepthook requires 3 arguments"));
    }
    let exc_type = &args[0];
    let exc_value = &args[1];
    let exc_tb = &args[2];
    let mut text = format_traceback_chain(exc_tb);
    let type_name = exception_display_type(exc_type, exc_value);
    let value_str = exception_display_value(exc_value);
    if value_str.is_empty() {
        text.push_str(&type_name);
        text.push('\n');
    } else {
        text.push_str(&format!("{}: {}\n", type_name, value_str));
    }
    write_stderr(&text);
    Ok(PyObject::none())
}

/// Default sys.unraisablehook: prints an unraisable exception summary.
pub(super) fn sys_unraisablehook_default(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "unraisablehook requires an argument",
        ));
    }
    let unraisable = &args[0];
    let exc_type = unraisable
        .get_attr("exc_type")
        .unwrap_or_else(|| PyObject::none());
    let exc_value = unraisable
        .get_attr("exc_value")
        .unwrap_or_else(|| PyObject::none());
    let object = unraisable
        .get_attr("object")
        .unwrap_or_else(|| PyObject::none());
    write_stderr(&format!(
        "Exception ignored in: {}\n{}: {}\n",
        object.py_to_string(),
        exception_display_type(&exc_type, &exc_value),
        exception_display_value(&exc_value)
    ));
    Ok(PyObject::none())
}

fn exception_display_type(exc_type: &PyObjectRef, exc_value: &PyObjectRef) -> String {
    if let PyObjectPayload::Instance(inst) = &exc_value.payload {
        if let PyObjectPayload::Class(cd) = &inst.class.payload {
            return cd.name.to_string();
        }
    }
    match &exc_type.payload {
        PyObjectPayload::ExceptionType(kind) => format!("{}", kind),
        PyObjectPayload::Class(cd) => cd.name.to_string(),
        _ => exc_type.py_to_string(),
    }
}

fn exception_display_value(exc_value: &PyObjectRef) -> String {
    if let PyObjectPayload::Instance(inst) = &exc_value.payload {
        if let PyObjectPayload::Class(cd) = &inst.class.payload {
            if cd.namespace.read().contains_key("__str__") {
                return "<exception str() failed>".to_string();
            }
        }
        if let Some(args) = inst.attrs.read().get("args") {
            if let PyObjectPayload::Tuple(items) = &args.payload {
                return match items.len() {
                    0 => String::new(),
                    1 => items[0].py_to_string(),
                    _ => args.py_to_string(),
                };
            }
        }
    }
    exc_value.py_to_string()
}

fn format_traceback_chain(tb: &PyObjectRef) -> String {
    let mut entries = Vec::new();
    let mut current = tb.clone();
    loop {
        let attrs = match &current.payload {
            PyObjectPayload::None => break,
            PyObjectPayload::Instance(inst) => inst.attrs.read().clone(),
            _ => break,
        };
        let filename = attrs
            .get("tb_filename")
            .map(|v| v.py_to_string())
            .unwrap_or_else(|| "<unknown>".to_string());
        let lineno = attrs
            .get("tb_lineno")
            .and_then(|v| match &v.payload {
                PyObjectPayload::Int(n) => n.to_i64(),
                _ => None,
            })
            .unwrap_or(0);
        let function = attrs
            .get("tb_name")
            .map(|v| v.py_to_string())
            .unwrap_or_else(|| "<unknown>".to_string());
        entries.push((filename, lineno, function));
        match attrs.get("tb_next") {
            Some(next) if !matches!(next.payload, PyObjectPayload::None) => {
                current = next.clone();
            }
            _ => break,
        }
    }
    if entries.is_empty() {
        return String::new();
    }
    let mut text = String::from("Traceback (most recent call last):\n");
    for (filename, lineno, function) in entries {
        text.push_str(&format!(
            "  File \"{}\", line {}, in {}\n",
            filename, lineno, function
        ));
        if let Some(line) = source_line(&filename, lineno) {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                text.push_str(&format!("    {}\n", trimmed));
            }
        }
    }
    text
}

fn source_line(filename: &str, lineno: i64) -> Option<String> {
    if lineno <= 0 {
        return None;
    }
    let content = std::fs::read_to_string(filename).ok()?;
    content
        .lines()
        .nth((lineno as usize).saturating_sub(1))
        .map(|line| line.to_string())
}

fn write_stderr(text: &str) {
    let Some(stderr) =
        CURRENT_SYS_MODULE.with(|c| c.borrow().as_ref().and_then(|sys| sys.get_attr("stderr")))
    else {
        eprint!("{}", text);
        return;
    };
    let Some(write) = stderr.get_attr("write") else {
        eprint!("{}", text);
        return;
    };
    if ferrython_core::object::call_callable(
        &write,
        &[PyObject::str_val(CompactString::from(text))],
    )
    .is_err()
    {
        eprint!("{}", text);
    }
}
