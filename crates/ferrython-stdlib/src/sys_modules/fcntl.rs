use ferrython_core::error::PyException;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef};

pub fn create_fcntl_module() -> PyObjectRef {
    let fcntl_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 {
            return Err(PyException::type_error("fcntl() requires at least 2 args"));
        }
        let fd = args[0].to_int()? as i32;
        let cmd = args[1].to_int()? as i32;
        #[cfg(unix)]
        {
            let result = if args.len() > 2 {
                let arg = args[2].to_int()? as libc::c_long;
                unsafe { libc::fcntl(fd, cmd, arg) }
            } else {
                unsafe { libc::fcntl(fd, cmd) }
            };
            if result == -1 {
                let err = std::io::Error::last_os_error();
                return Err(PyException::os_error(format!("fcntl failed: {}", err)));
            }
            Ok(PyObject::int(result as i64))
        }
        #[cfg(not(unix))]
        {
            let _ = (fd, cmd);
            Err(PyException::os_error(
                "fcntl() is not supported on this platform",
            ))
        }
    });

    let flock_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 {
            return Err(PyException::type_error("flock() requires 2 args"));
        }
        let fd = args[0].to_int()? as i32;
        let operation = args[1].to_int()? as i32;
        #[cfg(unix)]
        {
            let result = unsafe { libc::flock(fd, operation) };
            if result == -1 {
                let err = std::io::Error::last_os_error();
                return Err(PyException::os_error(format!("flock failed: {}", err)));
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (fd, operation);
            return Err(PyException::os_error(
                "flock() is not supported on this platform",
            ));
        }
        Ok(PyObject::none())
    });

    let lockf_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 {
            return Err(PyException::type_error("lockf() requires at least 2 args"));
        }
        let fd = args[0].to_int()? as i32;
        let cmd = args[1].to_int()? as i32;
        let len: i64 = if args.len() > 2 { args[2].to_int()? } else { 0 };
        let start: i64 = if args.len() > 3 { args[3].to_int()? } else { 0 };
        let whence: i32 = if args.len() > 4 {
            args[4].to_int()? as i32
        } else {
            0
        };
        #[cfg(unix)]
        {
            let mut lock: libc::flock = unsafe { std::mem::zeroed() };
            lock.l_type = cmd as i16;
            lock.l_whence = whence as i16;
            lock.l_start = start as libc::off_t;
            lock.l_len = len as libc::off_t;
            let result = unsafe { libc::fcntl(fd, libc::F_SETLK, &lock) };
            if result == -1 {
                let err = std::io::Error::last_os_error();
                return Err(PyException::os_error(format!("lockf failed: {}", err)));
            }
        }
        #[cfg(not(unix))]
        {
            let _ = (fd, cmd, len, start, whence);
            return Err(PyException::os_error(
                "lockf() is not supported on this platform",
            ));
        }
        Ok(PyObject::none())
    });

    let ioctl_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 {
            return Err(PyException::type_error("ioctl() requires at least 2 args"));
        }
        let fd = args[0].to_int()? as i32;
        let request = args[1].to_int()? as u64;
        #[cfg(unix)]
        {
            let result = if args.len() > 2 {
                let arg = args[2].to_int()? as libc::c_ulong;
                unsafe { libc::ioctl(fd, request as libc::c_ulong, arg) }
            } else {
                unsafe { libc::ioctl(fd, request as libc::c_ulong) }
            };
            if result == -1 {
                let err = std::io::Error::last_os_error();
                return Err(PyException::os_error(format!("ioctl failed: {}", err)));
            }
            Ok(PyObject::int(result as i64))
        }
        #[cfg(not(unix))]
        {
            let _ = (fd, request);
            Err(PyException::os_error(
                "ioctl() is not supported on this platform",
            ))
        }
    });

    make_module(
        "fcntl",
        vec![
            ("fcntl", fcntl_fn),
            ("flock", flock_fn),
            ("lockf", lockf_fn),
            ("ioctl", ioctl_fn),
            // Lock constants
            ("LOCK_SH", PyObject::int(1)),
            ("LOCK_EX", PyObject::int(2)),
            ("LOCK_NB", PyObject::int(4)),
            ("LOCK_UN", PyObject::int(8)),
            // fcntl constants
            ("F_DUPFD", PyObject::int(0)),
            ("F_GETFD", PyObject::int(1)),
            ("F_SETFD", PyObject::int(2)),
            ("F_GETFL", PyObject::int(3)),
            ("F_SETFL", PyObject::int(4)),
            ("F_GETLK", PyObject::int(5)),
            ("F_SETLK", PyObject::int(6)),
            ("F_SETLKW", PyObject::int(7)),
            ("FD_CLOEXEC", PyObject::int(1)),
        ],
    )
}

// ── sysconfig module ──
