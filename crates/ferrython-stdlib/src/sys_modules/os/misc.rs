use super::*;

pub(super) fn os_urandom(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let n = if args.is_empty() {
        16
    } else {
        args[0].as_int().unwrap_or(16) as usize
    };
    let mut buf = vec![0u8; n];
    #[cfg(unix)]
    {
        use std::io::Read;
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            let _ = f.read_exact(&mut buf);
        }
    }
    Ok(PyObject::bytes(buf))
}

pub(super) fn os_access(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::bool_val(false));
    }
    let path = args[0].py_to_string();
    Ok(PyObject::bool_val(std::path::Path::new(&path).exists()))
}

pub(super) fn os_umask(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(0o022))
}

pub(super) fn os_getlogin(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .or_else(|_| {
            std::process::Command::new("whoami")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .map_err(|_| std::env::VarError::NotPresent)
        })
        .unwrap_or_else(|_| String::from("unknown"));
    Ok(PyObject::str_val(CompactString::from(user)))
}

pub(super) fn os_strerror(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("strerror requires an error code"));
    }
    let code = args[0].as_int().unwrap_or(0) as i32;
    #[cfg(unix)]
    {
        let msg = unsafe {
            let p = libc::strerror(code);
            if p.is_null() {
                "Unknown error".to_string()
            } else {
                std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        };
        Ok(PyObject::str_val(CompactString::from(msg)))
    }
    #[cfg(not(unix))]
    {
        Ok(PyObject::str_val(CompactString::from(format!(
            "Error {}",
            code
        ))))
    }
}

pub(super) fn os_putenv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("putenv requires 2 arguments"));
    }
    let key = args[0].py_to_string();
    let val = args[1].py_to_string();
    unsafe {
        std::env::set_var(&key, &val);
    }
    Ok(PyObject::none())
}

pub(super) fn os_unsetenv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("unsetenv requires 1 argument"));
    }
    let key = args[0].py_to_string();
    unsafe {
        std::env::remove_var(&key);
    }
    Ok(PyObject::none())
}

pub(super) fn os_expanduser(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("expanduser requires path"));
    }
    let path = args[0].py_to_string();
    if path.starts_with("~/") || path == "~" {
        if let Ok(home) = std::env::var("HOME") {
            let expanded = if path == "~" {
                home
            } else {
                format!("{}{}", home, &path[1..])
            };
            return Ok(PyObject::str_val(CompactString::from(expanded)));
        }
    }
    Ok(PyObject::str_val(CompactString::from(path)))
}

pub(super) fn os_fsencode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.fsencode", args, 1)?;
    let s = args[0].py_to_string();
    Ok(PyObject::bytes(s.into_bytes()))
}

pub(super) fn os_fsdecode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.fsdecode", args, 1)?;
    match &args[0].payload {
        PyObjectPayload::Bytes(b) => {
            let s = String::from_utf8_lossy(b).to_string();
            Ok(PyObject::str_val(CompactString::from(s)))
        }
        _ => Ok(PyObject::str_val(CompactString::from(
            args[0].py_to_string(),
        ))),
    }
}
