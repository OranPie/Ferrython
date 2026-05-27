use super::*;

pub(super) fn os_getcwd(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let cwd = std::env::current_dir().map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::str_val(CompactString::from(
        cwd.to_string_lossy().to_string(),
    )))
}

pub(super) fn os_listdir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let path = if args.is_empty() {
        ".".to_string()
    } else {
        args[0].py_to_string()
    };
    let entries = std::fs::read_dir(&path)
        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    let items: Vec<PyObjectRef> = entries
        .filter_map(|e| e.ok())
        .map(|e| {
            PyObject::str_val(CompactString::from(
                e.file_name().to_string_lossy().to_string(),
            ))
        })
        .collect();
    Ok(PyObject::list(items))
}

pub(super) fn os_mkdir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "os.mkdir() requires at least 1 argument",
        ));
    }
    let path = args[0].py_to_string();
    let exist_ok = args.iter().skip(1).any(|a| {
        if let PyObjectPayload::Dict(kw) = &a.payload {
            kw.read()
                .get(&HashableKey::str_key(CompactString::from("exist_ok")))
                .map(|v| matches!(&v.payload, PyObjectPayload::Bool(true)))
                .unwrap_or(false)
        } else {
            false
        }
    });
    match std::fs::create_dir(&path) {
        Ok(_) => Ok(PyObject::none()),
        Err(e) if exist_ok && e.kind() == std::io::ErrorKind::AlreadyExists => Ok(PyObject::none()),
        Err(e) => Err(PyException::from_io_error(&e, Some(&path))),
    }
}

pub(super) fn os_makedirs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "os.makedirs() requires at least 1 argument",
        ));
    }
    let path = args[0].py_to_string();
    // Check for exist_ok kwarg (may be in trailing dict)
    let exist_ok = args.iter().skip(1).any(|a| {
        if let PyObjectPayload::Dict(kw) = &a.payload {
            kw.read()
                .get(&HashableKey::str_key(CompactString::from("exist_ok")))
                .map(|v| matches!(&v.payload, PyObjectPayload::Bool(true)))
                .unwrap_or(false)
        } else {
            matches!(&a.payload, PyObjectPayload::Bool(true))
        }
    });
    match std::fs::create_dir_all(&path) {
        Ok(_) => Ok(PyObject::none()),
        Err(e) if exist_ok && e.kind() == std::io::ErrorKind::AlreadyExists => Ok(PyObject::none()),
        Err(e) => Err(PyException::os_error(format!("{}", e))),
    }
}

pub(super) fn os_remove(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.remove", args, 1)?;
    std::fs::remove_file(args[0].py_to_string())
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::none())
}

pub(super) fn os_rmdir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.rmdir", args, 1)?;
    std::fs::remove_dir(args[0].py_to_string())
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::none())
}

pub(super) fn os_removedirs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.removedirs", args, 1)?;
    let path_str = args[0].py_to_string();
    let mut path = std::path::PathBuf::from(&*path_str);
    // Remove the leaf directory first
    std::fs::remove_dir(&path).map_err(|e| PyException::os_error(format!("{}", e)))?;
    // Walk up, removing empty parent directories until one fails
    while let Some(parent) = path.parent() {
        if parent.as_os_str().is_empty() {
            break;
        }
        if std::fs::remove_dir(parent).is_err() {
            break;
        }
        path = parent.to_path_buf();
    }
    Ok(PyObject::none())
}

pub(super) fn os_rename(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.rename", args, 2)?;
    std::fs::rename(args[0].py_to_string(), args[1].py_to_string())
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::none())
}

pub(super) fn os_replace(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.replace", args, 2)?;
    // On Unix, rename is atomic and replaces the destination
    std::fs::rename(args[0].py_to_string(), args[1].py_to_string())
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::none())
}

pub(super) fn os_chdir(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.chdir", args, 1)?;
    let path = args[0].py_to_string();
    std::env::set_current_dir(&path)
        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    Ok(PyObject::none())
}
