use super::*;

pub(super) fn os_getenv(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "os.getenv requires at least 1 argument",
        ));
    }
    let key = args[0].py_to_string();
    let default = if args.len() > 1 {
        args[1].clone()
    } else {
        PyObject::none()
    };
    match std::env::var(&key) {
        Ok(v) => Ok(PyObject::str_val(CompactString::from(v))),
        Err(_) => Ok(default),
    }
}

pub(super) fn os_cpu_count(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(num_cpus() as i64))
}

pub(super) fn os_getpid(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(std::process::id() as i64))
}

pub(super) fn os_getppid(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    #[cfg(unix)]
    {
        Ok(PyObject::int(unsafe { libc::getppid() } as i64))
    }
    #[cfg(not(unix))]
    {
        Err(PyException::os_error(
            "getppid() is not supported on this platform",
        ))
    }
}

pub(super) fn os_fork(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    #[cfg(unix)]
    {
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err(PyException::os_error("fork failed".to_string()));
        }
        Ok(PyObject::int(pid as i64))
    }
    #[cfg(not(unix))]
    {
        Err(PyException::not_implemented_error("os.fork not available"))
    }
}

pub(super) fn os_exit(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let code = args.first().and_then(|obj| obj.as_int()).unwrap_or(0) as i32;
    #[cfg(unix)]
    unsafe {
        libc::_exit(code);
    }
    #[cfg(not(unix))]
    {
        std::process::exit(code);
    }
}

pub(super) fn os_getuid(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    #[cfg(unix)]
    {
        Ok(PyObject::int(unsafe { libc::getuid() } as i64))
    }
    #[cfg(not(unix))]
    {
        Err(PyException::os_error(
            "getuid() is not supported on this platform",
        ))
    }
}

pub(super) fn os_getgid(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    #[cfg(unix)]
    {
        Ok(PyObject::int(unsafe { libc::getgid() } as i64))
    }
    #[cfg(not(unix))]
    {
        Err(PyException::os_error(
            "getgid() is not supported on this platform",
        ))
    }
}

pub(super) fn os_geteuid(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    #[cfg(unix)]
    {
        Ok(PyObject::int(unsafe { libc::geteuid() } as i64))
    }
    #[cfg(not(unix))]
    {
        Err(PyException::os_error(
            "geteuid() is not supported on this platform",
        ))
    }
}

pub(super) fn os_getegid(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    #[cfg(unix)]
    {
        Ok(PyObject::int(unsafe { libc::getegid() } as i64))
    }
    #[cfg(not(unix))]
    {
        Err(PyException::os_error(
            "getegid() is not supported on this platform",
        ))
    }
}

pub(super) fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

pub(super) fn os_system(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.system", args, 1)?;
    let cmd = args[0].py_to_string();
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .status()
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    Ok(PyObject::int(status.code().unwrap_or(-1) as i64))
}

/// os.popen(cmd) → file-like object with read()/close()
pub(super) fn os_popen(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.popen", args, 1)?;
    let cmd = args[0].py_to_string();
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .output()
        .map_err(|e| PyException::os_error(format!("{}", e)))?;
    let data = String::from_utf8_lossy(&output.stdout).to_string();
    let data_arc = Rc::new(PyCell::new(data));

    let cls = PyObject::class(CompactString::from("_POpenFile"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        let d = data_arc.clone();
        attrs.insert(
            CompactString::from("read"),
            PyObject::native_closure("popen.read", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(d.read().as_str())))
            }),
        );
        attrs.insert(
            CompactString::from("close"),
            make_builtin(|_| Ok(PyObject::none())),
        );
        let d2 = data_arc;
        attrs.insert(
            CompactString::from("readline"),
            PyObject::native_closure("popen.readline", move |_: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(d2.read().as_str())))
            }),
        );
    }
    Ok(inst)
}

pub(super) fn os_kill(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("os.kill requires pid and signal"));
    }
    let pid = args[0]
        .as_int()
        .ok_or_else(|| PyException::type_error("pid must be int"))?;
    let sig = args[1]
        .as_int()
        .ok_or_else(|| PyException::type_error("signal must be int"))?;
    #[cfg(unix)]
    {
        let ret = unsafe { libc::kill(pid as i32, sig as i32) };
        if ret != 0 {
            return Err(PyException::os_error(format!("kill failed: errno {}", ret)));
        }
    }
    Ok(PyObject::none())
}

pub(super) fn os_waitpid(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "os.waitpid requires pid and options",
        ));
    }
    let pid = args[0]
        .as_int()
        .ok_or_else(|| PyException::type_error("pid must be int"))? as i32;
    let options = args[1]
        .as_int()
        .ok_or_else(|| PyException::type_error("options must be int"))? as i32;
    #[cfg(unix)]
    {
        let mut status: i32 = 0;
        let ret = unsafe { libc::waitpid(pid, &mut status, options) };
        if ret < 0 {
            return Err(PyException::os_error("waitpid failed".to_string()));
        }
        Ok(PyObject::tuple(vec![
            PyObject::int(ret as i64),
            PyObject::int(status as i64),
        ]))
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, options);
        Err(PyException::not_implemented_error(
            "os.waitpid not available",
        ))
    }
}

pub(super) fn os_wifexited(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.WIFEXITED", args, 1)?;
    let status = args[0].as_int().unwrap_or(0) as i32;
    Ok(PyObject::bool_val(libc::WIFEXITED(status)))
}

pub(super) fn os_wexitstatus(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.WEXITSTATUS", args, 1)?;
    let status = args[0].as_int().unwrap_or(0) as i32;
    Ok(PyObject::int(libc::WEXITSTATUS(status) as i64))
}

pub(super) fn os_wifsignaled(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.WIFSIGNALED", args, 1)?;
    let status = args[0].as_int().unwrap_or(0) as i32;
    Ok(PyObject::bool_val(libc::WIFSIGNALED(status)))
}

pub(super) fn os_wtermsig(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.WTERMSIG", args, 1)?;
    let status = args[0].as_int().unwrap_or(0) as i32;
    Ok(PyObject::int(libc::WTERMSIG(status) as i64))
}

pub(super) fn os_wifstopped(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.WIFSTOPPED", args, 1)?;
    let status = args[0].as_int().unwrap_or(0) as i32;
    Ok(PyObject::bool_val(libc::WIFSTOPPED(status)))
}

pub(super) fn os_wstopsig(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.WSTOPSIG", args, 1)?;
    let status = args[0].as_int().unwrap_or(0) as i32;
    Ok(PyObject::int(libc::WSTOPSIG(status) as i64))
}
