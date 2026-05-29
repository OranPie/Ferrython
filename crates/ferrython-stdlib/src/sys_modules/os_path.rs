use super::*;

// ── os.path module ──

pub fn create_os_path_module() -> PyObjectRef {
    make_module(
        "os.path",
        vec![
            ("join", make_builtin(os_path_join)),
            ("exists", make_builtin(os_path_exists)),
            ("isfile", make_builtin(os_path_isfile)),
            ("isdir", make_builtin(os_path_isdir)),
            ("islink", make_builtin(os_path_islink)),
            ("basename", make_builtin(os_path_basename)),
            ("dirname", make_builtin(os_path_dirname)),
            ("abspath", make_builtin(os_path_abspath)),
            ("splitext", make_builtin(os_path_splitext)),
            ("split", make_builtin(os_path_split)),
            ("isabs", make_builtin(os_path_isabs)),
            ("normcase", make_builtin(os_path_normcase)),
            ("normpath", make_builtin(os_path_normpath)),
            ("expanduser", make_builtin(os_path_expanduser)),
            ("expandvars", make_builtin(os_path_expandvars)),
            ("getsize", make_builtin(os_path_getsize)),
            ("getmtime", make_builtin(os_path_getmtime)),
            ("getctime", make_builtin(os_path_getctime)),
            ("getatime", make_builtin(os_path_getatime)),
            (
                "sep",
                PyObject::str_val(CompactString::from(std::path::MAIN_SEPARATOR.to_string())),
            ),
            ("realpath", make_builtin(os_path_realpath)),
            ("relpath", make_builtin(os_path_relpath)),
            ("commonpath", make_builtin(os_path_commonpath)),
            ("commonprefix", make_builtin(os_path_commonprefix)),
            ("samefile", make_builtin(os_path_samefile)),
            ("pardir", PyObject::str_val(CompactString::from(".."))),
            ("curdir", PyObject::str_val(CompactString::from("."))),
            ("extsep", PyObject::str_val(CompactString::from("."))),
            ("altsep", PyObject::none()),
            (
                "pathsep",
                PyObject::str_val(CompactString::from(if cfg!(windows) { ";" } else { ":" })),
            ),
            (
                "defpath",
                PyObject::str_val(CompactString::from(if cfg!(windows) {
                    ".;C:\\bin"
                } else {
                    "/bin:/usr/bin"
                })),
            ),
            (
                "devnull",
                PyObject::str_val(CompactString::from(if cfg!(windows) {
                    "nul"
                } else {
                    "/dev/null"
                })),
            ),
        ],
    )
}

fn os_path_join(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "os.path.join requires at least 1 argument",
        ));
    }
    let mut path = std::path::PathBuf::from(args[0].py_to_string());
    for arg in &args[1..] {
        path.push(arg.py_to_string());
    }
    Ok(PyObject::str_val(CompactString::from(
        path.to_string_lossy().to_string(),
    )))
}
fn os_path_exists(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.exists", args, 1)?;
    Ok(PyObject::bool_val(
        std::path::Path::new(&args[0].py_to_string()).exists(),
    ))
}
fn os_path_isfile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.isfile", args, 1)?;
    Ok(PyObject::bool_val(
        std::path::Path::new(&args[0].py_to_string()).is_file(),
    ))
}
fn os_path_isdir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.isdir", args, 1)?;
    Ok(PyObject::bool_val(
        std::path::Path::new(&args[0].py_to_string()).is_dir(),
    ))
}
fn os_path_basename(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.basename", args, 1)?;
    let s = args[0].py_to_string();
    // Python: basename("/a/b/") → "", basename("/a/b") → "b"
    if s.ends_with('/') && s.len() > 1 {
        return Ok(PyObject::str_val(CompactString::from("")));
    }
    let name = if let Some(pos) = s.rfind('/') {
        &s[pos + 1..]
    } else {
        &s
    };
    Ok(PyObject::str_val(CompactString::from(name)))
}
fn os_path_dirname(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.dirname", args, 1)?;
    let s = args[0].py_to_string();
    let p = std::path::Path::new(&s);
    let dir = p
        .parent()
        .map(|d| d.to_string_lossy().to_string())
        .unwrap_or_default();
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
    Ok(PyObject::str_val(CompactString::from(
        abs.to_string_lossy().to_string(),
    )))
}
fn os_path_splitext(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.splitext", args, 1)?;
    let s = args[0].py_to_string();
    let p = std::path::Path::new(&s);
    let ext = p
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let stem = s[..s.len() - ext.len()].to_string();
    Ok(PyObject::tuple(vec![
        PyObject::str_val(CompactString::from(stem)),
        PyObject::str_val(CompactString::from(ext)),
    ]))
}
fn os_path_split(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.split", args, 1)?;
    let s = args[0].py_to_string();
    // Python's os.path.split: trailing slash → (path, "")
    if s.ends_with('/') && s.len() > 1 {
        let trimmed = s.trim_end_matches('/');
        return Ok(PyObject::tuple(vec![
            PyObject::str_val(CompactString::from(trimmed)),
            PyObject::str_val(CompactString::from("")),
        ]));
    }
    if let Some(pos) = s.rfind('/') {
        let head = if pos == 0 { "/" } else { &s[..pos] };
        let tail = &s[pos + 1..];
        Ok(PyObject::tuple(vec![
            PyObject::str_val(CompactString::from(head)),
            PyObject::str_val(CompactString::from(tail)),
        ]))
    } else {
        Ok(PyObject::tuple(vec![
            PyObject::str_val(CompactString::from("")),
            PyObject::str_val(CompactString::from(s)),
        ]))
    }
}
fn os_path_isabs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.isabs", args, 1)?;
    Ok(PyObject::bool_val(
        std::path::Path::new(&args[0].py_to_string()).is_absolute(),
    ))
}
fn os_path_normcase(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.normcase", args, 1)?;
    Ok(PyObject::str_val(CompactString::from(
        args[0].py_to_string(),
    )))
}
fn os_path_normpath(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.normpath", args, 1)?;
    let s = args[0].py_to_string();
    // Basic normpath: collapse separators and resolve . / ..
    let mut parts: Vec<&str> = Vec::new();
    for part in s.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let result = if s.starts_with('/') {
        format!("/{}", parts.join("/"))
    } else if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    };
    Ok(PyObject::str_val(CompactString::from(result)))
}
fn os_path_expanduser(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.expanduser", args, 1)?;
    let s = args[0].py_to_string();
    if s.starts_with('~') {
        if let Ok(home) = std::env::var("HOME") {
            return Ok(PyObject::str_val(CompactString::from(format!(
                "{}{}",
                home,
                &s[1..]
            ))));
        }
    }
    Ok(PyObject::str_val(CompactString::from(s)))
}
fn os_path_getsize(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.getsize", args, 1)?;
    let s = args[0].py_to_string();
    match std::fs::metadata(&s) {
        Ok(m) => Ok(PyObject::int(m.len() as i64)),
        Err(e) => Err(PyException::from_io_error(&e, Some(&s))),
    }
}

fn os_path_realpath(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.realpath", args, 1)?;
    let s = args[0].py_to_string();
    let p = std::path::Path::new(&s);
    let real = std::fs::canonicalize(p).unwrap_or_else(|_| {
        let mut cwd = std::env::current_dir().unwrap_or_default();
        cwd.push(&s);
        cwd
    });
    Ok(PyObject::str_val(CompactString::from(
        real.to_string_lossy().to_string(),
    )))
}

fn os_path_relpath(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "os.path.relpath() requires at least 1 argument",
        ));
    }
    let path_str = args[0].py_to_string();
    let start_str = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string())
    };
    let make_abs = |s: &str| -> std::path::PathBuf {
        let p = std::path::Path::new(s);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            let mut cwd = std::env::current_dir().unwrap_or_default();
            cwd.push(s);
            cwd
        }
    };
    let path_abs = make_abs(&path_str);
    let start_abs = make_abs(&start_str);
    let path_components: Vec<_> = path_abs.components().collect();
    let start_components: Vec<_> = start_abs.components().collect();
    let common_len = path_components
        .iter()
        .zip(start_components.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut result = std::path::PathBuf::new();
    for _ in common_len..start_components.len() {
        result.push("..");
    }
    for component in &path_components[common_len..] {
        result.push(component);
    }
    let result_str = if result.as_os_str().is_empty() {
        ".".to_string()
    } else {
        result.to_string_lossy().to_string()
    };
    Ok(PyObject::str_val(CompactString::from(result_str)))
}

fn os_path_commonpath(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.commonpath", args, 1)?;
    let paths = args[0].to_list()?;
    if paths.is_empty() {
        return Err(PyException::value_error(
            "commonpath() arg is an empty sequence",
        ));
    }
    let path_strs: Vec<String> = paths.iter().map(|p| p.py_to_string()).collect();
    let first_abs = path_strs[0].starts_with('/');
    for p in &path_strs[1..] {
        if p.starts_with('/') != first_abs {
            return Err(PyException::value_error(
                "Can't mix absolute and relative paths",
            ));
        }
    }
    let split: Vec<Vec<&str>> = path_strs
        .iter()
        .map(|p| p.split('/').filter(|s| !s.is_empty()).collect())
        .collect();
    let min_len = split.iter().map(|p| p.len()).min().unwrap_or(0);
    let mut common_len = 0;
    for i in 0..min_len {
        if split.iter().all(|p| p[i] == split[0][i]) {
            common_len = i + 1;
        } else {
            break;
        }
    }
    let common_parts: Vec<&str> = split[0][..common_len].to_vec();
    let result = if first_abs {
        if common_parts.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", common_parts.join("/"))
        }
    } else if common_parts.is_empty() {
        ".".to_string()
    } else {
        common_parts.join("/")
    };
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn os_path_getmtime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.getmtime", args, 1)?;
    let s = args[0].py_to_string();
    let meta = std::fs::metadata(&s).map_err(|e| PyException::from_io_error(&e, Some(&s)))?;
    let mtime = meta
        .modified()
        .map_err(|_| PyException::runtime_error("getmtime failed"))?;
    let epoch = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    Ok(PyObject::float(epoch.as_secs_f64()))
}

fn os_path_getctime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.getctime", args, 1)?;
    let s = args[0].py_to_string();
    let meta = std::fs::metadata(&s).map_err(|e| PyException::from_io_error(&e, Some(&s)))?;
    // On Unix, ctime is metadata change time (use created or modified as fallback)
    let ctime = meta
        .created()
        .or_else(|_| meta.modified())
        .map_err(|_| PyException::runtime_error("getctime failed"))?;
    let epoch = ctime
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    Ok(PyObject::float(epoch.as_secs_f64()))
}

fn os_path_getatime(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.getatime", args, 1)?;
    let s = args[0].py_to_string();
    let meta = std::fs::metadata(&s).map_err(|e| PyException::from_io_error(&e, Some(&s)))?;
    let atime = meta
        .accessed()
        .map_err(|_| PyException::runtime_error("getatime failed"))?;
    let epoch = atime
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    Ok(PyObject::float(epoch.as_secs_f64()))
}

fn os_path_expandvars(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.expandvars", args, 1)?;
    let s = args[0].py_to_string();
    let mut result = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            i += 1;
            if i < bytes.len() && bytes[i] == b'{' {
                i += 1; // skip {
                let start = i;
                while i < bytes.len() && bytes[i] != b'}' {
                    i += 1;
                }
                let var = &s[start..i];
                if i < bytes.len() {
                    i += 1;
                } // skip }
                result.push_str(&std::env::var(var).unwrap_or(format!("${{{}}}", var)));
            } else {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                let var = &s[start..i];
                if var.is_empty() {
                    result.push('$');
                } else {
                    result.push_str(&std::env::var(var).unwrap_or(format!("${}", var)));
                }
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn os_path_commonprefix(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.commonprefix", args, 1)?;
    let paths = args[0].to_list()?;
    if paths.is_empty() {
        return Ok(PyObject::str_val(CompactString::from("")));
    }
    let strs: Vec<String> = paths.iter().map(|p| p.py_to_string()).collect();
    let first = strs[0].as_bytes();
    let mut prefix_len = first.len();
    for s in &strs[1..] {
        let b = s.as_bytes();
        prefix_len = prefix_len.min(b.len());
        for i in 0..prefix_len {
            if first[i] != b[i] {
                prefix_len = i;
                break;
            }
        }
    }
    Ok(PyObject::str_val(CompactString::from(
        &strs[0][..prefix_len],
    )))
}

fn os_path_samefile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("samefile() requires 2 arguments"));
    }
    let a = std::fs::canonicalize(args[0].py_to_string());
    let b = std::fs::canonicalize(args[1].py_to_string());
    match (a, b) {
        (Ok(pa), Ok(pb)) => Ok(PyObject::bool_val(pa == pb)),
        _ => Ok(PyObject::bool_val(false)),
    }
}

fn os_path_islink(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.path.islink", args, 1)?;
    let s = args[0].py_to_string();
    Ok(PyObject::bool_val(
        std::fs::symlink_metadata(&s)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false),
    ))
}
