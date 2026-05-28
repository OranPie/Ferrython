use super::*;

pub(super) fn inspect_getmembers(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("inspect.getmembers", args, 1)?;
    let dir_names = args[0].dir();
    let mut result = Vec::new();
    for n in &dir_names {
        if let Some(val) = args[0].get_attr(n.as_str()) {
            result.push(PyObject::tuple(vec![PyObject::str_val(n.clone()), val]));
        }
    }
    Ok(PyObject::list(result))
}

pub(super) fn inspect_getdoc(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.getdoc", args, 1)?;
    match args[0].get_attr("__doc__") {
        Some(doc) if !matches!(&doc.payload, PyObjectPayload::None) => {
            let s = doc.py_to_string();
            let lines: Vec<&str> = s.lines().collect();
            if lines.is_empty() {
                return Ok(PyObject::none());
            }
            let min_indent = lines
                .iter()
                .skip(1)
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.len() - l.trim_start().len())
                .min()
                .unwrap_or(0);
            let mut result = String::from(lines[0].trim());
            for line in &lines[1..] {
                result.push('\n');
                if line.len() > min_indent {
                    result.push_str(&line[min_indent..]);
                } else {
                    result.push_str(line.trim());
                }
            }
            let cleaned: String = result
                .lines()
                .map(|l| l.trim_end())
                .collect::<Vec<_>>()
                .join("\n");
            Ok(PyObject::str_val(CompactString::from(cleaned.trim_end())))
        }
        _ => Ok(PyObject::none()),
    }
}

pub(super) fn inspect_getmodule(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.getmodule", args, 1)?;
    Ok(args[0]
        .get_attr("__module__")
        .unwrap_or_else(PyObject::none))
}

pub(super) fn inspect_getfile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.getfile", args, 1)?;
    if let PyObjectPayload::Function(f) = &args[0].payload {
        return Ok(PyObject::str_val(f.code.filename.clone()));
    }
    if let PyObjectPayload::Module(m) = &args[0].payload {
        if let Some(file) = m.attrs.read().get("__file__").cloned() {
            return Ok(file);
        }
    }
    Err(PyException::type_error("could not get file for object"))
}

pub(super) fn inspect_getsourcefile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.getsourcefile", args, 1)?;
    let filename = if let PyObjectPayload::Function(f) = &args[0].payload {
        Some(f.code.filename.clone())
    } else if let PyObjectPayload::Module(m) = &args[0].payload {
        m.attrs
            .read()
            .get("__file__")
            .map(|f| CompactString::from(f.py_to_string()))
    } else {
        None
    };
    match filename {
        Some(f) if f.ends_with(".py") => Ok(PyObject::str_val(f)),
        Some(_) => Ok(PyObject::none()),
        None => Err(PyException::type_error("could not find source file")),
    }
}

pub(super) fn inspect_getsource(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.getsource", args, 1)?;
    let filename = match &args[0].payload {
        PyObjectPayload::Function(f) => f.code.filename.clone(),
        PyObjectPayload::Module(m) => {
            if let Some(f) = m.attrs.read().get("__file__") {
                CompactString::from(f.py_to_string())
            } else {
                return Err(PyException::runtime_error("could not find source"));
            }
        }
        _ => return Err(PyException::runtime_error("could not find source")),
    };
    match std::fs::read_to_string(filename.as_str()) {
        Ok(src) => {
            if let PyObjectPayload::Function(f) = &args[0].payload {
                let lines: Vec<&str> = src.lines().collect();
                let start = (f.code.first_line_number as usize).saturating_sub(1);
                if start < lines.len() {
                    let indent = lines[start].len() - lines[start].trim_start().len();
                    let mut end = start + 1;
                    while end < lines.len() {
                        let line = lines[end];
                        if line.trim().is_empty() {
                            end += 1;
                            continue;
                        }
                        let li = line.len() - line.trim_start().len();
                        if li <= indent {
                            break;
                        }
                        end += 1;
                    }
                    return Ok(PyObject::str_val(CompactString::from(
                        lines[start..end].join("\n"),
                    )));
                }
            }
            Ok(PyObject::str_val(CompactString::from(src)))
        }
        Err(_) => Err(PyException::runtime_error("could not read source file")),
    }
}

pub(super) fn inspect_getsourcelines(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.getsourcelines", args, 1)?;
    let filename = match &args[0].payload {
        PyObjectPayload::Function(f) => Some((f.code.filename.clone(), f.code.first_line_number)),
        _ => None,
    };
    if let Some((fname, lineno)) = filename {
        match std::fs::read_to_string(fname.as_str()) {
            Ok(src) => {
                let all_lines: Vec<&str> = src.lines().collect();
                let start = (lineno as usize).saturating_sub(1);
                if start >= all_lines.len() {
                    return Err(PyException::runtime_error("could not find source lines"));
                }
                let base_indent = all_lines[start].len() - all_lines[start].trim_start().len();
                let mut end = start + 1;
                while end < all_lines.len() {
                    let line = all_lines[end];
                    if line.trim().is_empty() {
                        end += 1;
                        continue;
                    }
                    let indent = line.len() - line.trim_start().len();
                    if indent <= base_indent {
                        break;
                    }
                    end += 1;
                }
                let lines: Vec<PyObjectRef> = all_lines[start..end]
                    .iter()
                    .map(|l| PyObject::str_val(CompactString::from(format!("{}\n", l))))
                    .collect();
                Ok(PyObject::tuple(vec![
                    PyObject::list(lines),
                    PyObject::int(lineno as i64),
                ]))
            }
            Err(_) => Err(PyException::runtime_error("could not read source")),
        }
    } else {
        Err(PyException::runtime_error("could not find source lines"))
    }
}

pub(super) fn inspect_cleandoc(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.cleandoc", args, 1)?;
    let doc = args[0].py_to_string();
    Ok(PyObject::str_val(CompactString::from(clean_docstring(
        &doc,
    ))))
}

pub(super) fn inspect_unwrap(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.unwrap", args, 1)?;
    let mut func = args[0].clone();
    for _ in 0..100 {
        if let Some(wrapped) = func.get_attr("__wrapped__") {
            func = wrapped;
        } else {
            break;
        }
    }
    Ok(func)
}

pub(super) fn inspect_getattr_static(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() || args.len() < 2 {
        return Err(PyException::type_error(
            "getattr_static() requires at least 2 arguments",
        ));
    }
    let name_str = args[1].py_to_string();
    if let Some(v) = args[0].get_attr(&name_str) {
        Ok(v)
    } else if args.len() >= 3 {
        Ok(args[2].clone())
    } else {
        Err(PyException::attribute_error(format!(
            "'{}' object has no attribute '{}'",
            args[0].type_name(),
            name_str
        )))
    }
}
