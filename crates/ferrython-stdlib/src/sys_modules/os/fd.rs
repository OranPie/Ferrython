use super::*;

pub(super) fn os_link(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("os.link requires src and dst"));
    }
    std::fs::hard_link(args[0].py_to_string(), args[1].py_to_string())
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::none())
}

pub(super) fn os_truncate(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "os.truncate requires path and length",
        ));
    }
    let path = args[0].py_to_string();
    let length = args[1].as_int().unwrap_or(0) as u64;
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
    f.set_len(length)
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::none())
}

pub(super) fn os_pipe(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    #[cfg(unix)]
    {
        let mut fds = [0i32; 2];
        let ret = unsafe { libc::pipe(fds.as_mut_ptr()) };
        if ret != 0 {
            return Err(PyException::os_error("pipe() failed".to_string()));
        }
        Ok(PyObject::tuple(vec![
            PyObject::int(fds[0] as i64),
            PyObject::int(fds[1] as i64),
        ]))
    }
    #[cfg(not(unix))]
    {
        Err(PyException::not_implemented_error(
            "os.pipe not available on this platform",
        ))
    }
}

pub(super) fn os_dup(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.dup", args, 1)?;
    let fd = args[0]
        .as_int()
        .ok_or_else(|| PyException::type_error("fd must be int"))?;
    #[cfg(unix)]
    {
        let new_fd = unsafe { libc::dup(fd as i32) };
        if new_fd < 0 {
            return Err(PyException::os_error("dup() failed".to_string()));
        }
        Ok(PyObject::int(new_fd as i64))
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(PyException::not_implemented_error("os.dup not available"))
    }
}

pub(super) fn os_dup2(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("os.dup2 requires oldfd and newfd"));
    }
    let oldfd = args[0]
        .as_int()
        .ok_or_else(|| PyException::type_error("fd must be int"))?;
    let newfd = args[1]
        .as_int()
        .ok_or_else(|| PyException::type_error("fd must be int"))?;
    #[cfg(unix)]
    {
        let ret = unsafe { libc::dup2(oldfd as i32, newfd as i32) };
        if ret < 0 {
            return Err(PyException::os_error("dup2() failed".to_string()));
        }
        Ok(PyObject::int(ret as i64))
    }
    #[cfg(not(unix))]
    {
        let _ = (oldfd, newfd);
        Err(PyException::not_implemented_error("os.dup2 not available"))
    }
}

pub(super) fn os_close(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.close", args, 1)?;
    let fd = args[0]
        .as_int()
        .ok_or_else(|| PyException::type_error("fd must be int"))? as i32;
    #[cfg(unix)]
    {
        let ret = unsafe { libc::close(fd) };
        if ret != 0 {
            return Err(PyException::os_error(format!(
                "Bad file descriptor: {}",
                fd
            )));
        }
    }
    Ok(PyObject::none())
}

pub(super) fn os_open(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "os.open requires path, flags, and optional mode",
        ));
    }
    let path = args[0].py_to_string();
    let flags = if args.len() > 1 {
        args[1].as_int().unwrap_or(0) as i32
    } else {
        0
    };
    let mode = if args.len() > 2 {
        args[2].as_int().unwrap_or(0o666) as u32
    } else {
        0o666
    };
    #[cfg(unix)]
    {
        let cpath = std::ffi::CString::new(path.as_str())
            .map_err(|_| PyException::value_error("invalid path"))?;
        let fd = unsafe { libc::open(cpath.as_ptr(), flags, mode) };
        if fd < 0 {
            return Err(PyException::os_error(format!(
                "No such file or directory: '{}'",
                path
            )));
        }
        Ok(PyObject::int(fd as i64))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, flags, mode);
        Err(PyException::not_implemented_error("os.open not available"))
    }
}

pub(super) fn os_read(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("os.read requires fd and count"));
    }
    let fd = args[0]
        .as_int()
        .ok_or_else(|| PyException::type_error("fd must be int"))? as i32;
    let count = args[1]
        .as_int()
        .ok_or_else(|| PyException::type_error("count must be int"))? as usize;
    #[cfg(unix)]
    {
        let mut buf = vec![0u8; count];
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, count) };
        if n < 0 {
            return Err(PyException::os_error("read failed".to_string()));
        }
        buf.truncate(n as usize);
        Ok(PyObject::bytes(buf))
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, count);
        Err(PyException::not_implemented_error("os.read not available"))
    }
}

pub(super) fn os_write(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("os.write requires fd and data"));
    }
    let fd = args[0]
        .as_int()
        .ok_or_else(|| PyException::type_error("fd must be int"))? as i32;
    let data = match &args[1].payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => (**b).clone(),
        PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
        _ => return Err(PyException::type_error("data must be bytes-like")),
    };
    #[cfg(unix)]
    {
        let n = unsafe { libc::write(fd, data.as_ptr() as *const libc::c_void, data.len()) };
        if n < 0 {
            return Err(PyException::os_error("write failed".to_string()));
        }
        Ok(PyObject::int(n as i64))
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, data);
        Err(PyException::not_implemented_error("os.write not available"))
    }
}

pub(super) fn os_fdopen(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("os.fdopen requires fd"));
    }
    let fd = args[0]
        .as_int()
        .ok_or_else(|| PyException::type_error("fd must be int"))? as i32;
    let mode = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        "r".to_string()
    };
    #[cfg(unix)]
    {
        let is_binary = mode.contains('b');
        let state = Rc::new(PyCell::new((fd, false)));
        let mode_str = mode.clone();
        let name_str = format!("<fdopen fd={}>", fd);
        let mut attrs = IndexMap::new();
        attrs.insert(
            CompactString::from("mode"),
            PyObject::str_val(CompactString::from(&mode_str)),
        );
        attrs.insert(
            CompactString::from("name"),
            PyObject::str_val(CompactString::from(&name_str)),
        );
        attrs.insert(CompactString::from("closed"), PyObject::bool_val(false));

        let s1 = state.clone();
        let is_bin_r = is_binary;
        attrs.insert(
            CompactString::from("read"),
            PyObject::native_closure("fdopen.read", move |a| {
                let g = s1.read();
                if g.1 {
                    return Err(PyException::value_error("I/O operation on closed file"));
                }
                let fd = g.0;
                drop(g);
                let size = if !a.is_empty() && a.len() > 1 {
                    a[1].as_int().unwrap_or(-1) as isize
                } else if !a.is_empty() {
                    a[0].as_int().unwrap_or(-1) as isize
                } else {
                    -1isize
                };
                let buf = if size < 0 {
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 8192];
                    loop {
                        let n = unsafe {
                            libc::read(fd, tmp.as_mut_ptr() as *mut libc::c_void, tmp.len())
                        };
                        if n <= 0 {
                            break;
                        }
                        buf.extend_from_slice(&tmp[..n as usize]);
                    }
                    buf
                } else {
                    let mut buf = vec![0u8; size as usize];
                    let n =
                        unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
                    if n < 0 {
                        return Err(PyException::os_error("read failed".to_string()));
                    }
                    buf.truncate(n as usize);
                    buf
                };
                if is_bin_r {
                    Ok(PyObject::bytes(buf))
                } else {
                    Ok(PyObject::str_val(CompactString::from(
                        String::from_utf8_lossy(&buf).as_ref(),
                    )))
                }
            }),
        );

        let s2 = state.clone();
        attrs.insert(
            CompactString::from("write"),
            PyObject::native_closure("fdopen.write", move |a| {
                let g = s2.read();
                if g.1 {
                    return Err(PyException::value_error("I/O operation on closed file"));
                }
                let fd = g.0;
                drop(g);
                if a.is_empty()
                    || (a.len() == 1 && matches!(a[0].payload, PyObjectPayload::Instance(_)))
                {
                    return Err(PyException::type_error("write requires data"));
                }
                let data_arg = if a.len() > 1 { &a[1] } else { &a[0] };
                let data_bytes = match &data_arg.payload {
                    PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => (**b).clone(),
                    PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                    _ => return Err(PyException::type_error("write requires str or bytes")),
                };
                let n = unsafe {
                    libc::write(
                        fd,
                        data_bytes.as_ptr() as *const libc::c_void,
                        data_bytes.len(),
                    )
                };
                if n < 0 {
                    return Err(PyException::os_error("write failed".to_string()));
                }
                Ok(PyObject::int(n as i64))
            }),
        );

        let s3 = state.clone();
        attrs.insert(
            CompactString::from("seek"),
            PyObject::native_closure("fdopen.seek", move |a| {
                let g = s3.read();
                if g.1 {
                    return Err(PyException::value_error("I/O operation on closed file"));
                }
                let fd = g.0;
                drop(g);
                let offset = if a.len() > 1 {
                    a[1].as_int().unwrap_or(0) as i64
                } else if !a.is_empty() {
                    a[0].as_int().unwrap_or(0) as i64
                } else {
                    0i64
                };
                let whence = if a.len() > 2 {
                    a[2].as_int().unwrap_or(0) as i32
                } else {
                    0i32
                };
                let pos = unsafe { libc::lseek(fd, offset as libc::off_t, whence) };
                if pos < 0 {
                    return Err(PyException::os_error("seek failed".to_string()));
                }
                Ok(PyObject::int(pos as i64))
            }),
        );

        let s4 = state.clone();
        attrs.insert(
            CompactString::from("tell"),
            PyObject::native_closure("fdopen.tell", move |_a| {
                let g = s4.read();
                if g.1 {
                    return Err(PyException::value_error("I/O operation on closed file"));
                }
                let fd = g.0;
                drop(g);
                let pos = unsafe { libc::lseek(fd, 0, libc::SEEK_CUR) };
                Ok(PyObject::int(pos as i64))
            }),
        );

        let s5 = state.clone();
        attrs.insert(
            CompactString::from("flush"),
            PyObject::native_closure("fdopen.flush", move |_a| {
                let g = s5.read();
                if g.1 {
                    return Err(PyException::value_error("I/O operation on closed file"));
                }
                let fd = g.0;
                drop(g);
                unsafe {
                    libc::fsync(fd);
                }
                Ok(PyObject::none())
            }),
        );

        let s6 = state.clone();
        attrs.insert(
            CompactString::from("close"),
            PyObject::native_closure("fdopen.close", move |_| {
                let mut g = s6.write();
                if !g.1 {
                    g.1 = true;
                    unsafe {
                        libc::close(g.0);
                    }
                }
                Ok(PyObject::none())
            }),
        );

        attrs.insert(
            CompactString::from("__enter__"),
            PyObject::native_closure("fdopen.__enter__", move |a| {
                if a.is_empty() {
                    return Ok(PyObject::none());
                }
                Ok(a[0].clone())
            }),
        );

        let s7 = state.clone();
        attrs.insert(
            CompactString::from("__exit__"),
            PyObject::native_closure("fdopen.__exit__", move |_| {
                let mut g = s7.write();
                if !g.1 {
                    g.1 = true;
                    unsafe {
                        libc::close(g.0);
                    }
                }
                Ok(PyObject::bool_val(false))
            }),
        );

        let class = PyObject::class(CompactString::from("_io.FileIO"), vec![], IndexMap::new());
        Ok(PyObject::instance_with_attrs(class, attrs))
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, mode);
        Err(PyException::not_implemented_error(
            "os.fdopen not available on this platform",
        ))
    }
}

pub(super) fn os_fstat(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("os.fstat requires fd"));
    }
    let fd = args[0]
        .as_int()
        .ok_or_else(|| PyException::type_error("fd must be int"))? as i32;
    #[cfg(unix)]
    {
        use std::os::unix::io::FromRawFd;
        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        let meta = file
            .metadata()
            .map_err(|e| PyException::os_error(format!("{}", e)));
        std::mem::forget(file);
        let meta = meta?;
        super::stat::build_stat_result_from_meta(&meta)
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(PyException::not_implemented_error(
            "os.fstat not supported on this platform",
        ))
    }
}

pub(super) fn os_ftruncate(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("os.ftruncate", args, 2)?;
    let fd = args[0]
        .as_int()
        .ok_or_else(|| PyException::type_error("fd must be int"))? as i32;
    let length = args[1]
        .as_int()
        .ok_or_else(|| PyException::type_error("length must be int"))? as u64;
    #[cfg(unix)]
    {
        use std::os::unix::io::FromRawFd;
        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        let result = file
            .set_len(length)
            .map_err(|e| PyException::os_error(format!("{}", e)));
        std::mem::forget(file);
        result?;
        Ok(PyObject::none())
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, length);
        Err(PyException::not_implemented_error(
            "os.ftruncate not supported on this platform",
        ))
    }
}

pub(super) fn os_lseek(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("os.lseek", args, 3)?;
    let fd = args[0]
        .as_int()
        .ok_or_else(|| PyException::type_error("fd must be int"))? as i32;
    let offset = args[1]
        .as_int()
        .ok_or_else(|| PyException::type_error("offset must be int"))? as i64;
    let whence = args[2]
        .as_int()
        .ok_or_else(|| PyException::type_error("whence must be int"))? as i32;
    #[cfg(unix)]
    {
        use std::io::Seek;
        use std::os::unix::io::FromRawFd;
        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        let seek_from = match whence {
            0 => std::io::SeekFrom::Start(offset as u64),
            1 => std::io::SeekFrom::Current(offset),
            2 => std::io::SeekFrom::End(offset),
            _ => {
                std::mem::forget(file);
                return Err(PyException::value_error("invalid whence"));
            }
        };
        let result = file.seek(seek_from);
        std::mem::forget(file);
        match result {
            Ok(pos) => Ok(PyObject::int(pos as i64)),
            Err(e) => Err(PyException::os_error(format!("{}", e))),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (fd, offset, whence);
        Err(PyException::not_implemented_error(
            "os.lseek not supported on this platform",
        ))
    }
}

pub(super) fn os_fsync(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("os.fsync", args, 1)?;
    let fd = args[0]
        .as_int()
        .ok_or_else(|| PyException::type_error("fd must be int"))? as i32;
    #[cfg(unix)]
    {
        use std::os::unix::io::FromRawFd;
        let file = unsafe { std::fs::File::from_raw_fd(fd) };
        let result = file
            .sync_all()
            .map_err(|e| PyException::os_error(format!("{}", e)));
        std::mem::forget(file);
        result?;
        Ok(PyObject::none())
    }
    #[cfg(not(unix))]
    {
        let _ = fd;
        Err(PyException::not_implemented_error(
            "os.fsync not supported on this platform",
        ))
    }
}
