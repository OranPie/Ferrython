use super::*;

pub(super) fn os_chmod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.chmod", args, 2)?;
    #[cfg(unix)]
    {
        let path = args[0].py_to_string();
        let mode = args[1]
            .as_int()
            .ok_or_else(|| PyException::type_error("an integer is required"))?;
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(mode as u32);
        std::fs::set_permissions(&path, perms)
            .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    }
    Ok(PyObject::none())
}

pub(super) fn os_chown(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error(
            "chown requires at least 3 arguments",
        ));
    }
    #[cfg(unix)]
    {
        let path = args[0].py_to_string();
        let uid = args[1].as_int().unwrap_or(-1);
        let gid = args[2].as_int().unwrap_or(-1);
        let cpath = std::ffi::CString::new(path.as_str())
            .map_err(|_| PyException::value_error("embedded null in path"))?;
        let ret = unsafe { libc::chown(cpath.as_ptr(), uid as libc::uid_t, gid as libc::gid_t) };
        if ret != 0 {
            return Err(PyException::os_error(format!(
                "{}: '{}'",
                std::io::Error::last_os_error(),
                path
            )));
        }
    }
    Ok(PyObject::none())
}

pub(super) fn os_symlink(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.symlink", args, 2)?;
    let src = args[0].py_to_string();
    let dst = args[1].py_to_string();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&src, &dst)
            .map_err(|e| PyException::os_error(format!("{}: '{}' -> '{}'", e, dst, src)))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (&src, &dst);
        return Err(PyException::os_error(
            "os.symlink() not available on this platform",
        ));
    }
    Ok(PyObject::none())
}

pub(super) fn os_readlink(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.readlink", args, 1)?;
    let path = args[0].py_to_string();
    let target = std::fs::read_link(&path)
        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    Ok(PyObject::str_val(CompactString::from(
        target.to_string_lossy().to_string(),
    )))
}

pub(super) fn os_isatty(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.isatty", args, 1)?;
    let fd = args[0]
        .as_int()
        .ok_or_else(|| PyException::type_error("an integer is required"))?;
    Ok(PyObject::bool_val(is_fd_terminal(fd)))
}

#[cfg(unix)]
pub(super) fn is_fd_terminal(fd: i64) -> bool {
    unsafe {
        extern "C" {
            fn isatty(fd: i32) -> i32;
        }
        isatty(fd as i32) != 0
    }
}

#[cfg(not(unix))]
pub(super) fn is_fd_terminal(_fd: i64) -> bool {
    false
}
