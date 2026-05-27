use super::*;

/// Create a file-like object for stdin/stdout/stderr
pub(super) fn make_stdio_object(name: &str, mode: &str, fileno: i64) -> PyObjectRef {
    use indexmap::IndexMap;
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("name"),
        PyObject::str_val(CompactString::from(name)),
    );
    attrs.insert(
        CompactString::from("mode"),
        PyObject::str_val(CompactString::from(mode)),
    );
    attrs.insert(
        CompactString::from("encoding"),
        PyObject::str_val(CompactString::from("utf-8")),
    );
    attrs.insert(
        CompactString::from("errors"),
        PyObject::str_val(CompactString::from("strict")),
    );
    attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));
    attrs.insert(
        CompactString::from("line_buffering"),
        PyObject::bool_val(fileno != 0),
    );
    attrs.insert(CompactString::from("_fileno"), PyObject::int(fileno));
    attrs.insert(CompactString::from("newlines"), PyObject::none());
    attrs.insert(
        CompactString::from("buffer"),
        make_stdio_buffer_object(name, mode, fileno),
    );
    attrs.insert(
        CompactString::from("write"),
        PyObject::native_function("write", stdio_write),
    );
    attrs.insert(
        CompactString::from("writelines"),
        PyObject::native_function("writelines", stdio_writelines),
    );
    attrs.insert(
        CompactString::from("read"),
        PyObject::native_function("read", stdio_read),
    );
    attrs.insert(
        CompactString::from("readline"),
        PyObject::native_function("readline", stdio_readline),
    );
    attrs.insert(
        CompactString::from("readlines"),
        PyObject::native_function("readlines", stdio_readlines),
    );
    attrs.insert(
        CompactString::from("flush"),
        PyObject::native_function("flush", stdio_flush),
    );
    attrs.insert(
        CompactString::from("fileno"),
        PyObject::native_function("fileno", stdio_fileno),
    );
    attrs.insert(
        CompactString::from("isatty"),
        PyObject::native_function("isatty", stdio_isatty),
    );
    attrs.insert(
        CompactString::from("readable"),
        PyObject::native_function("readable", stdio_readable),
    );
    attrs.insert(
        CompactString::from("writable"),
        PyObject::native_function("writable", stdio_writable),
    );
    attrs.insert(
        CompactString::from("seekable"),
        PyObject::native_function("seekable", stdio_seekable),
    );
    attrs.insert(
        CompactString::from("_bind_methods"),
        PyObject::bool_val(true),
    );

    PyObject::module_with_attrs(CompactString::from("_io.TextIOWrapper"), attrs)
}

fn make_stdio_buffer_object(name: &str, mode: &str, fileno: i64) -> PyObjectRef {
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("name"),
        PyObject::str_val(CompactString::from(name)),
    );
    attrs.insert(
        CompactString::from("mode"),
        PyObject::str_val(CompactString::from(format!("{}b", mode))),
    );
    attrs.insert(CompactString::from("_fileno"), PyObject::int(fileno));
    attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));
    attrs.insert(
        CompactString::from("write"),
        PyObject::native_function("write", stdio_buffer_write),
    );
    attrs.insert(
        CompactString::from("flush"),
        PyObject::native_function("flush", stdio_flush),
    );
    attrs.insert(
        CompactString::from("fileno"),
        PyObject::native_function("fileno", stdio_fileno),
    );
    attrs.insert(
        CompactString::from("isatty"),
        PyObject::native_function("isatty", stdio_isatty),
    );
    attrs.insert(
        CompactString::from("readable"),
        PyObject::native_function("readable", stdio_readable),
    );
    attrs.insert(
        CompactString::from("writable"),
        PyObject::native_function("writable", stdio_writable),
    );
    attrs.insert(
        CompactString::from("seekable"),
        PyObject::native_function("seekable", stdio_seekable),
    );
    attrs.insert(
        CompactString::from("_bind_methods"),
        PyObject::bool_val(true),
    );
    PyObject::module_with_attrs(CompactString::from("_io.BufferedWriter"), attrs)
}

fn get_stdio_fd(args: &[PyObjectRef]) -> i64 {
    args.first()
        .and_then(|s| s.get_attr("_fileno"))
        .and_then(|v| v.to_int().ok())
        .unwrap_or(-1)
}

fn stdio_buffer_write(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let fd = get_stdio_fd(args);
    let arg = if args.len() > 1 {
        &args[1]
    } else {
        return Err(PyException::type_error("write() requires 1 argument"));
    };
    let bytes = match &arg.payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => (**b).clone(),
        _ => {
            return Err(PyException::type_error(format!(
                "a bytes-like object is required, not '{}'",
                arg.type_name()
            )))
        }
    };
    use std::io::Write;
    if fd == 2 {
        let _ = std::io::stderr().write_all(&bytes);
    } else {
        let _ = std::io::stdout().write_all(&bytes);
    }
    Ok(PyObject::int(bytes.len() as i64))
}

fn stdio_write(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let fd = get_stdio_fd(args);
    let arg = if args.len() > 1 {
        &args[1]
    } else {
        return Err(PyException::type_error("write() requires 1 argument"));
    };
    // TextIOWrapper rejects bytes (like CPython)
    if matches!(&arg.payload, PyObjectPayload::Bytes(_)) {
        return Err(PyException::type_error(
            "write() argument must be str, not bytes",
        ));
    }
    let text = arg.py_to_string();
    let len = text.len();
    if fd == 2 {
        eprint!("{}", text);
    } else {
        print!("{}", text);
    }
    Ok(PyObject::int(len as i64))
}

fn stdio_writelines(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let fd = get_stdio_fd(args);
    let lines_obj = if args.len() > 1 {
        &args[1]
    } else {
        return Err(PyException::type_error("writelines() missing argument"));
    };
    let items = lines_obj.to_list()?;
    for item in items {
        let text = item.py_to_string();
        if fd == 2 {
            eprint!("{}", text);
        } else {
            print!("{}", text);
        }
    }
    Ok(PyObject::none())
}

fn stdio_read(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let fd = get_stdio_fd(args);
    if fd != 0 {
        return Err(PyException::runtime_error("not readable"));
    }
    use std::io::Read;
    let max = if args.len() > 1 {
        args[1].to_int().unwrap_or(-1)
    } else {
        -1
    };
    let mut buf = String::new();
    if max < 0 {
        std::io::stdin().read_to_string(&mut buf).unwrap_or(0);
    } else {
        let mut handle = std::io::stdin().take(max as u64);
        handle.read_to_string(&mut buf).unwrap_or(0);
    }
    Ok(PyObject::str_val(CompactString::from(buf)))
}

fn stdio_readline(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let fd = get_stdio_fd(args);
    if fd != 0 {
        return Err(PyException::runtime_error("not readable"));
    }
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap_or(0);
    Ok(PyObject::str_val(CompactString::from(line)))
}

fn stdio_readlines(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let fd = get_stdio_fd(args);
    if fd != 0 {
        return Err(PyException::runtime_error("not readable"));
    }
    use std::io::BufRead;
    let stdin = std::io::stdin();
    let lines: Vec<PyObjectRef> = stdin
        .lock()
        .lines()
        .filter_map(|l| l.ok())
        .map(|l| PyObject::str_val(CompactString::from(format!("{}\n", l))))
        .collect();
    Ok(PyObject::list(lines))
}

fn stdio_flush(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    Ok(PyObject::none())
}

fn stdio_fileno(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(get_stdio_fd(args)))
}

fn stdio_isatty(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::bool_val(false))
}

fn stdio_readable(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::bool_val(get_stdio_fd(args) == 0))
}

fn stdio_writable(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::bool_val(get_stdio_fd(args) != 0))
}

fn stdio_seekable(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::bool_val(false))
}
