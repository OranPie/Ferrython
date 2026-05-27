use super::*;

mod io;
mod lifecycle;

use io::install_io_methods;
use lifecycle::install_lifecycle_methods;

// ── Internal socket state ──────────────────────────────────────────────

pub(super) struct SocketInner {
    family: i64,
    sock_type: i64,
    proto: i64,
    pub(super) tcp_stream: Option<TcpStream>,
    tcp_listener: Option<TcpListener>,
    udp_socket: Option<UdpSocket>,
    bound_addr: Option<String>,
    pub(super) timeout: Option<Duration>,
    closed: bool,
    options: Vec<(i64, i64, i64)>,
}

impl SocketInner {
    pub(super) fn new(family: i64, sock_type: i64, proto: i64) -> Self {
        let timeout = DEFAULT_TIMEOUT
            .lock()
            .ok()
            .and_then(|g| *g)
            .map(Duration::from_secs_f64);
        Self {
            family,
            sock_type,
            proto,
            tcp_stream: None,
            tcp_listener: None,
            udp_socket: None,
            bound_addr: None,
            timeout,
            closed: false,
            options: Vec::new(),
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

pub(super) fn extract_host_port(addr: &PyObjectRef) -> PyResult<(String, u16)> {
    match &addr.payload {
        PyObjectPayload::Tuple(items) => {
            if items.len() < 2 {
                return Err(PyException::value_error(
                    "address tuple must have at least 2 elements",
                ));
            }
            let host = items[0].py_to_string();
            let port = items[1]
                .to_int()
                .map_err(|_| PyException::type_error("port must be an integer"))?;
            if port < 0 || port > 65535 {
                return Err(PyException::value_error("port must be 0-65535"));
            }
            Ok((host, port as u16))
        }
        _ => Err(PyException::type_error("a tuple (host, port) is required")),
    }
}

fn lock_inner(st: &Arc<Mutex<SocketInner>>) -> PyResult<std::sync::MutexGuard<'_, SocketInner>> {
    st.lock()
        .map_err(|e| PyException::runtime_error(format!("socket lock poisoned: {}", e)))
}

pub(super) fn socket_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let family = if !args.is_empty() {
        args[0].as_int().unwrap_or(2)
    } else {
        2
    };
    let sock_type = if args.len() > 1 {
        args[1].as_int().unwrap_or(1)
    } else {
        1
    };
    let proto = if args.len() > 2 {
        args[2].as_int().unwrap_or(0)
    } else {
        0
    };
    Ok(build_socket_object(family, sock_type, proto, None))
}

/// Build a socket-like module object.  If `existing` is provided, the inner
/// state is pre-populated (used by `accept()` and `create_connection()`).
pub(super) fn build_socket_object(
    family: i64,
    sock_type: i64,
    proto: i64,
    existing: Option<SocketInner>,
) -> PyObjectRef {
    let inner = Arc::new(Mutex::new(
        existing.unwrap_or_else(|| SocketInner::new(family, sock_type, proto)),
    ));

    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("__socket__"), PyObject::bool_val(true));
    attrs.insert(CompactString::from("family"), PyObject::int(family));
    attrs.insert(CompactString::from("type"), PyObject::int(sock_type));
    attrs.insert(CompactString::from("proto"), PyObject::int(proto));

    // ── connect((host, port)) ──
    let st = inner.clone();
    attrs.insert(
        CompactString::from("connect"),
        PyObject::native_closure("connect", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "connect() requires an address argument",
                ));
            }
            let (host, port) = extract_host_port(&args[0])?;
            let addr_str = format!("{}:{}", host, port);
            // Get timeout, check closed — release lock before blocking I/O
            let timeout = {
                let guard = lock_inner(&st)?;
                if guard.closed {
                    return Err(PyException::os_error("[Errno 9] Bad file descriptor"));
                }
                guard.timeout
            };
            let connect_result = if let Some(t) = timeout {
                use std::net::ToSocketAddrs;
                let addrs: Vec<_> = addr_str
                    .to_socket_addrs()
                    .map_err(|e| PyException::os_error(format!("getaddrinfo: {}", e)))?
                    .collect();
                let mut last_err = None;
                let mut result = None;
                for addr in &addrs {
                    match TcpStream::connect_timeout(addr, t) {
                        Ok(s) => {
                            result = Some(s);
                            break;
                        }
                        Err(e) => last_err = Some(e),
                    }
                }
                match result {
                    Some(s) => Ok(s),
                    None => Err(last_err.unwrap_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::AddrNotAvailable,
                            "no addresses to connect to",
                        )
                    })),
                }
            } else {
                TcpStream::connect(&addr_str)
            };
            match connect_result {
                Ok(stream) => {
                    if let Some(t) = timeout {
                        stream.set_read_timeout(Some(t)).ok();
                        stream.set_write_timeout(Some(t)).ok();
                    }
                    let mut guard = lock_inner(&st)?;
                    guard.tcp_stream = Some(stream);
                    Ok(PyObject::none())
                }
                Err(e) => {
                    use std::io::ErrorKind;
                    let exc = match e.kind() {
                        ErrorKind::ConnectionRefused => PyException::new(
                            ExceptionKind::ConnectionRefusedError,
                            format!("[Errno 111] Connection refused: {}", e),
                        ),
                        ErrorKind::ConnectionReset => PyException::new(
                            ExceptionKind::ConnectionResetError,
                            format!("[Errno 104] Connection reset: {}", e),
                        ),
                        ErrorKind::ConnectionAborted => PyException::new(
                            ExceptionKind::ConnectionAbortedError,
                            format!("[Errno 103] Connection aborted: {}", e),
                        ),
                        ErrorKind::TimedOut | ErrorKind::WouldBlock => PyException::new(
                            ExceptionKind::TimeoutError,
                            format!("[Errno 110] Connection timed out: {}", e),
                        ),
                        _ => PyException::os_error(format!("{}", e)),
                    };
                    Err(exc)
                }
            }
        }),
    );

    // ── bind((host, port)) ──
    let st = inner.clone();
    attrs.insert(
        CompactString::from("bind"),
        PyObject::native_closure("bind", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "bind() requires an address argument",
                ));
            }
            let (host, port) = extract_host_port(&args[0])?;
            let addr_str = format!("{}:{}", host, port);
            let mut guard = lock_inner(&st)?;
            if guard.closed {
                return Err(PyException::os_error("[Errno 9] Bad file descriptor"));
            }
            guard.bound_addr = Some(addr_str.clone());
            if guard.sock_type == 2 {
                // UDP — bind immediately
                match UdpSocket::bind(&addr_str) {
                    Ok(sock) => {
                        if let Some(t) = guard.timeout {
                            sock.set_read_timeout(Some(t)).ok();
                            sock.set_write_timeout(Some(t)).ok();
                        }
                        // Update bound_addr with actual assigned port
                        if let Ok(real_addr) = sock.local_addr() {
                            guard.bound_addr = Some(real_addr.to_string());
                        }
                        guard.udp_socket = Some(sock);
                    }
                    Err(e) => {
                        return Err(PyException::os_error(format!("bind: {}", e)));
                    }
                }
            } else {
                // TCP — create listener now so getsockname() returns real port
                match TcpListener::bind(&addr_str) {
                    Ok(listener) => {
                        if let Some(t) = guard.timeout {
                            listener.set_nonblocking(false).ok();
                            let _ = t; // timeout applied at accept time
                        }
                        if let Ok(real_addr) = listener.local_addr() {
                            guard.bound_addr = Some(real_addr.to_string());
                        }
                        guard.tcp_listener = Some(listener);
                    }
                    Err(e) => {
                        return Err(PyException::os_error(format!("bind: {}", e)));
                    }
                }
            }
            Ok(PyObject::none())
        }),
    );

    // ── listen(backlog=128) ──
    let st = inner.clone();
    attrs.insert(
        CompactString::from("listen"),
        PyObject::native_closure("listen", move |args| {
            let _backlog = if !args.is_empty() {
                args[0].as_int().unwrap_or(128)
            } else {
                128
            };
            let mut guard = lock_inner(&st)?;
            if guard.closed {
                return Err(PyException::os_error("[Errno 9] Bad file descriptor"));
            }
            // If listener already created by bind(), just apply options
            if guard.tcp_listener.is_some() {
                #[cfg(unix)]
                if let Some(ref listener) = guard.tcp_listener {
                    use std::os::unix::io::AsRawFd;
                    let fd = listener.as_raw_fd();
                    if let Some(t) = guard.timeout {
                        listener.set_nonblocking(false).ok();
                        let tv = libc::timeval {
                            tv_sec: t.as_secs() as libc::time_t,
                            tv_usec: t.subsec_micros() as libc::suseconds_t,
                        };
                        unsafe {
                            libc::setsockopt(
                                fd,
                                libc::SOL_SOCKET,
                                libc::SO_RCVTIMEO,
                                &tv as *const libc::timeval as *const libc::c_void,
                                std::mem::size_of::<libc::timeval>() as libc::socklen_t,
                            );
                        }
                    }
                    for &(level, optname, value) in &guard.options {
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
                return Ok(PyObject::none());
            }
            let addr = guard
                .bound_addr
                .clone()
                .unwrap_or_else(|| "0.0.0.0:0".to_string());
            match TcpListener::bind(&addr) {
                Ok(listener) => {
                    // Apply timeout so accept() won't block forever
                    if let Some(t) = guard.timeout {
                        listener.set_nonblocking(false).ok();
                        // Use raw fd to set SO_RCVTIMEO for accept timeout
                        #[cfg(unix)]
                        {
                            use std::os::unix::io::AsRawFd;
                            let fd = listener.as_raw_fd();
                            let tv = libc::timeval {
                                tv_sec: t.as_secs() as libc::time_t,
                                tv_usec: t.subsec_micros() as libc::suseconds_t,
                            };
                            unsafe {
                                libc::setsockopt(
                                    fd,
                                    libc::SOL_SOCKET,
                                    libc::SO_RCVTIMEO,
                                    &tv as *const libc::timeval as *const libc::c_void,
                                    std::mem::size_of::<libc::timeval>() as libc::socklen_t,
                                );
                            }
                        }
                    }
                    // Apply stored socket options
                    #[cfg(unix)]
                    {
                        use std::os::unix::io::AsRawFd;
                        let fd = listener.as_raw_fd();
                        for &(level, optname, value) in &guard.options {
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
                    guard.tcp_listener = Some(listener);
                    // Update bound_addr with actual address (resolves port 0)
                    if let Some(ref l) = guard.tcp_listener {
                        if let Ok(addr) = l.local_addr() {
                            guard.bound_addr = Some(format!("{}:{}", addr.ip(), addr.port()));
                        }
                    }
                    Ok(PyObject::none())
                }
                Err(e) => Err(PyException::os_error(format!("listen: {}", e))),
            }
        }),
    );

    // ── accept() → (conn_socket, (host, port)) ──
    let st = inner.clone();
    attrs.insert(
        CompactString::from("accept"),
        PyObject::native_closure("accept", move |_args| {
            // Briefly lock to clone the listener and get state, then release lock
            let (listener_clone, timeout, fam, stype, pr) = {
                let guard = lock_inner(&st)?;
                if guard.closed {
                    return Err(PyException::os_error("[Errno 9] Bad file descriptor"));
                }
                let listener = guard.tcp_listener.as_ref().ok_or_else(|| {
                    PyException::os_error("[Errno 22] Invalid argument: not listening")
                })?;
                let lc = listener
                    .try_clone()
                    .map_err(|e| PyException::os_error(format!("accept clone: {}", e)))?;
                (
                    lc,
                    guard.timeout,
                    guard.family,
                    guard.sock_type,
                    guard.proto,
                )
            };
            // Accept outside the lock so other threads can proceed
            if let Some(t) = timeout {
                listener_clone.set_nonblocking(true).ok();
                let start = std::time::Instant::now();
                loop {
                    match listener_clone.accept() {
                        Ok((stream, addr)) => {
                            stream.set_nonblocking(false).ok();
                            let peer_host = addr.ip().to_string();
                            let peer_port = addr.port() as i64;
                            let mut si = SocketInner::new(fam, stype, pr);
                            si.tcp_stream = Some(stream);
                            let conn = build_socket_object(fam, stype, pr, Some(si));
                            let addr_tuple = PyObject::tuple(vec![
                                PyObject::str_val(CompactString::from(&peer_host)),
                                PyObject::int(peer_port),
                            ]);
                            return Ok(PyObject::tuple(vec![conn, addr_tuple]));
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            if start.elapsed() >= t {
                                return Err(PyException::new(
                                    ExceptionKind::TimeoutError,
                                    "timed out",
                                ));
                            }
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(e) => return Err(PyException::os_error(format!("accept: {}", e))),
                    }
                }
            } else {
                // Blocking accept
                match listener_clone.accept() {
                    Ok((stream, addr)) => {
                        let peer_host = addr.ip().to_string();
                        let peer_port = addr.port() as i64;
                        let mut si = SocketInner::new(fam, stype, pr);
                        si.tcp_stream = Some(stream);
                        let conn = build_socket_object(fam, stype, pr, Some(si));
                        let addr_tuple = PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from(&peer_host)),
                            PyObject::int(peer_port),
                        ]);
                        Ok(PyObject::tuple(vec![conn, addr_tuple]))
                    }
                    Err(e) => Err(PyException::os_error(format!("accept: {}", e))),
                }
            }
        }),
    );

    install_io_methods(&mut attrs, &inner);

    install_lifecycle_methods(&mut attrs, &inner);

    attrs.insert(
        CompactString::from("_bind_methods"),
        PyObject::bool_val(true),
    );

    let socket_cls = PyObject::class(CompactString::from("socket"), vec![], IndexMap::new());
    PyObject::instance_with_attrs(socket_cls, attrs)
}
