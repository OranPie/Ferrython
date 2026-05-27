use super::*;

pub(super) fn install_lifecycle_methods(
    attrs: &mut IndexMap<CompactString, PyObjectRef>,
    inner: &Arc<Mutex<SocketInner>>,
) {
    let st = inner.clone();
    attrs.insert(
        CompactString::from("close"),
        PyObject::native_closure("close", move |_args| {
            let mut guard = lock_inner(&st)?;
            guard.tcp_stream = None;
            guard.tcp_listener = None;
            guard.udp_socket = None;
            guard.closed = true;
            Ok(PyObject::none())
        }),
    );

    let st = inner.clone();
    attrs.insert(
        CompactString::from("shutdown"),
        PyObject::native_closure("shutdown", move |args| {
            let how = if !args.is_empty() {
                args[0].as_int().unwrap_or(2)
            } else {
                2
            };
            let guard = lock_inner(&st)?;
            if guard.closed {
                return Err(PyException::os_error("[Errno 9] Bad file descriptor"));
            }
            if let Some(ref stream) = guard.tcp_stream {
                let shutdown_kind = match how {
                    0 => std::net::Shutdown::Read,
                    1 => std::net::Shutdown::Write,
                    _ => std::net::Shutdown::Both,
                };
                stream
                    .shutdown(shutdown_kind)
                    .map_err(|e| PyException::os_error(format!("shutdown: {}", e)))?;
            }
            Ok(PyObject::none())
        }),
    );

    let st = inner.clone();
    attrs.insert(
        CompactString::from("settimeout"),
        PyObject::native_closure("settimeout", move |args| {
            let mut guard = lock_inner(&st)?;
            if args.is_empty() || matches!(&args[0].payload, PyObjectPayload::None) {
                guard.timeout = None;
            } else {
                let secs = args[0].to_float().unwrap_or(0.0);
                if secs < 0.0 {
                    return Err(PyException::value_error("timeout value out of range"));
                }
                guard.timeout = Some(Duration::from_secs_f64(secs));
            }
            if let Some(ref stream) = guard.tcp_stream {
                stream.set_read_timeout(guard.timeout).ok();
                stream.set_write_timeout(guard.timeout).ok();
            }
            if let Some(ref sock) = guard.udp_socket {
                sock.set_read_timeout(guard.timeout).ok();
                sock.set_write_timeout(guard.timeout).ok();
            }
            Ok(PyObject::none())
        }),
    );

    let st = inner.clone();
    attrs.insert(
        CompactString::from("gettimeout"),
        PyObject::native_closure("gettimeout", {
            let st2 = st.clone();
            move |_args| {
                let guard = lock_inner(&st2)?;
                match guard.timeout {
                    Some(d) => Ok(PyObject::float(d.as_secs_f64())),
                    None => Ok(PyObject::none()),
                }
            }
        }),
    );
    attrs.insert(
        CompactString::from("getblocking"),
        PyObject::native_closure("getblocking", {
            let st2 = st.clone();
            move |_args| {
                let guard = lock_inner(&st2)?;
                Ok(PyObject::bool_val(guard.timeout.is_none()))
            }
        }),
    );
    attrs.insert(
        CompactString::from("setblocking"),
        PyObject::native_closure("setblocking", {
            let st2 = st.clone();
            move |args| {
                let blocking = if !args.is_empty() {
                    args[0].is_truthy()
                } else {
                    true
                };
                let mut guard = lock_inner(&st2)?;
                if blocking {
                    guard.timeout = None;
                } else {
                    guard.timeout = Some(Duration::from_secs(0));
                }
                if let Some(ref stream) = guard.tcp_stream {
                    stream.set_nonblocking(!blocking).ok();
                }
                if let Some(ref sock) = guard.udp_socket {
                    sock.set_nonblocking(!blocking).ok();
                }
                Ok(PyObject::none())
            }
        }),
    );
    attrs.insert(
        CompactString::from("fileno"),
        PyObject::native_closure("fileno", {
            let st2 = st.clone();
            move |_args| {
                let guard = lock_inner(&st2)?;
                #[cfg(unix)]
                {
                    use std::os::unix::io::AsRawFd;
                    if let Some(ref stream) = guard.tcp_stream {
                        return Ok(PyObject::int(stream.as_raw_fd() as i64));
                    }
                    if let Some(ref sock) = guard.udp_socket {
                        return Ok(PyObject::int(sock.as_raw_fd() as i64));
                    }
                    if let Some(ref listener) = guard.tcp_listener {
                        return Ok(PyObject::int(listener.as_raw_fd() as i64));
                    }
                }
                Ok(PyObject::int(-1))
            }
        }),
    );

    let st = inner.clone();
    attrs.insert(
        CompactString::from("setsockopt"),
        PyObject::native_closure("setsockopt", move |args| {
            let level = if !args.is_empty() {
                args[0].as_int().unwrap_or(0)
            } else {
                0
            };
            let optname = if args.len() > 1 {
                args[1].as_int().unwrap_or(0)
            } else {
                0
            };
            let value = if args.len() > 2 {
                args[2].as_int().unwrap_or(0)
            } else {
                0
            };
            let mut guard = lock_inner(&st)?;
            guard.options.push((level, optname, value));
            #[cfg(unix)]
            {
                use std::os::unix::io::AsRawFd;
                let fd = guard
                    .tcp_stream
                    .as_ref()
                    .map(|s| s.as_raw_fd())
                    .or_else(|| guard.udp_socket.as_ref().map(|s| s.as_raw_fd()))
                    .or_else(|| guard.tcp_listener.as_ref().map(|s| s.as_raw_fd()));
                if let Some(fd) = fd {
                    let val = value as libc::c_int;
                    unsafe {
                        libc::setsockopt(
                            fd,
                            level as libc::c_int,
                            optname as libc::c_int,
                            &val as *const libc::c_int as *const libc::c_void,
                            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                        );
                    }
                }
            }
            Ok(PyObject::none())
        }),
    );

    let st = inner.clone();
    attrs.insert(
        CompactString::from("getsockopt"),
        PyObject::native_closure("getsockopt", move |args| {
            let level = if !args.is_empty() {
                args[0].as_int().unwrap_or(0)
            } else {
                0
            };
            let optname = if args.len() > 1 {
                args[1].as_int().unwrap_or(0)
            } else {
                0
            };
            let guard = lock_inner(&st)?;
            #[cfg(unix)]
            {
                use std::os::unix::io::AsRawFd;
                let fd = guard
                    .tcp_stream
                    .as_ref()
                    .map(|s| s.as_raw_fd())
                    .or_else(|| guard.udp_socket.as_ref().map(|s| s.as_raw_fd()))
                    .or_else(|| guard.tcp_listener.as_ref().map(|s| s.as_raw_fd()));
                if let Some(fd) = fd {
                    let mut val: libc::c_int = 0;
                    let mut len: libc::socklen_t =
                        std::mem::size_of::<libc::c_int>() as libc::socklen_t;
                    let rc = unsafe {
                        libc::getsockopt(
                            fd,
                            level as libc::c_int,
                            optname as libc::c_int,
                            &mut val as *mut libc::c_int as *mut libc::c_void,
                            &mut len,
                        )
                    };
                    if rc == 0 {
                        return Ok(PyObject::int(val as i64));
                    }
                }
            }
            for &(l, o, v) in &guard.options {
                if l == level && o == optname {
                    return Ok(PyObject::int(v));
                }
            }
            Ok(PyObject::int(0))
        }),
    );

    let st = inner.clone();
    attrs.insert(
        CompactString::from("getsockname"),
        PyObject::native_closure("getsockname", move |_args| {
            let guard = lock_inner(&st)?;
            if let Some(ref stream) = guard.tcp_stream {
                if let Ok(addr) = stream.local_addr() {
                    return Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from(addr.ip().to_string())),
                        PyObject::int(addr.port() as i64),
                    ]));
                }
            }
            if let Some(ref listener) = guard.tcp_listener {
                if let Ok(addr) = listener.local_addr() {
                    return Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from(addr.ip().to_string())),
                        PyObject::int(addr.port() as i64),
                    ]));
                }
            }
            if let Some(ref sock) = guard.udp_socket {
                if let Ok(addr) = sock.local_addr() {
                    return Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from(addr.ip().to_string())),
                        PyObject::int(addr.port() as i64),
                    ]));
                }
            }
            if let Some(ref addr) = guard.bound_addr {
                let parts: Vec<&str> = addr.rsplitn(2, ':').collect();
                if parts.len() == 2 {
                    let port: i64 = parts[0].parse().unwrap_or(0);
                    return Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from(parts[1])),
                        PyObject::int(port),
                    ]));
                }
            }
            Ok(PyObject::tuple(vec![
                PyObject::str_val(CompactString::from("0.0.0.0")),
                PyObject::int(0),
            ]))
        }),
    );

    let st = inner.clone();
    attrs.insert(
        CompactString::from("getpeername"),
        PyObject::native_closure("getpeername", move |_args| {
            let guard = lock_inner(&st)?;
            if let Some(ref stream) = guard.tcp_stream {
                if let Ok(addr) = stream.peer_addr() {
                    return Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from(addr.ip().to_string())),
                        PyObject::int(addr.port() as i64),
                    ]));
                }
            }
            Err(PyException::os_error(
                "[Errno 107] Transport endpoint is not connected",
            ))
        }),
    );

    let st = inner.clone();
    attrs.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("__repr__", move |_args| {
            let guard = lock_inner(&st)?;
            let fd_str = if guard.closed {
                "fd=-1".to_string()
            } else {
                #[cfg(unix)]
                {
                    use std::os::unix::io::AsRawFd;
                    let fd = guard
                        .tcp_stream
                        .as_ref()
                        .map(|s| s.as_raw_fd())
                        .or_else(|| guard.udp_socket.as_ref().map(|s| s.as_raw_fd()))
                        .or_else(|| guard.tcp_listener.as_ref().map(|s| s.as_raw_fd()));
                    match fd {
                        Some(fd) => format!("fd={}", fd),
                        None => "fd=-1".to_string(),
                    }
                }
                #[cfg(not(unix))]
                {
                    "fd=-1".to_string()
                }
            };
            Ok(PyObject::str_val(CompactString::from(format!(
                "<socket.socket {}, family={}, type={}, proto={}>",
                fd_str, guard.family, guard.sock_type, guard.proto
            ))))
        }),
    );

    attrs.insert(
        CompactString::from("__enter__"),
        PyObject::native_function("__enter__", |args| {
            if !args.is_empty() {
                Ok(args[0].clone())
            } else {
                Ok(PyObject::none())
            }
        }),
    );
    let st = inner.clone();
    attrs.insert(
        CompactString::from("__exit__"),
        PyObject::native_closure("__exit__", move |_args| {
            let mut guard = lock_inner(&st)?;
            guard.tcp_stream = None;
            guard.tcp_listener = None;
            guard.udp_socket = None;
            guard.closed = true;
            Ok(PyObject::bool_val(false))
        }),
    );
}
