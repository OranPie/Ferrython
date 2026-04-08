//! Socket stdlib module.

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin,
};
use indexmap::IndexMap;

use std::io::{Read, Write};
use std::net::{TcpStream, TcpListener, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ── Global default timeout ──────────────────────────────────────────────

static DEFAULT_TIMEOUT: Mutex<Option<f64>> = Mutex::new(None);

// ── Internal socket state ──────────────────────────────────────────────

struct SocketInner {
    family: i64,
    sock_type: i64,
    proto: i64,
    tcp_stream: Option<TcpStream>,
    tcp_listener: Option<TcpListener>,
    udp_socket: Option<UdpSocket>,
    bound_addr: Option<String>,
    timeout: Option<Duration>,
    closed: bool,
    options: Vec<(i64, i64, i64)>,
}

impl SocketInner {
    fn new(family: i64, sock_type: i64, proto: i64) -> Self {
        let timeout = DEFAULT_TIMEOUT.lock().ok()
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

fn extract_host_port(addr: &PyObjectRef) -> PyResult<(String, u16)> {
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


fn lock_inner(
    st: &Arc<Mutex<SocketInner>>,
) -> PyResult<std::sync::MutexGuard<'_, SocketInner>> {
    st.lock()
        .map_err(|e| PyException::runtime_error(format!("socket lock poisoned: {}", e)))
}

// ════════════════════════════════════════════════════════════════════════
// socket module
// ════════════════════════════════════════════════════════════════════════

pub fn create_socket_module() -> PyObjectRef {
    make_module(
        "socket",
        vec![
            // Address families
            ("AF_INET", PyObject::int(2)),
            ("AF_INET6", PyObject::int(10)),
            ("AF_UNIX", PyObject::int(1)),
            // Socket types
            ("SOCK_STREAM", PyObject::int(1)),
            ("SOCK_DGRAM", PyObject::int(2)),
            // Socket options
            ("SOL_SOCKET", PyObject::int(1)),
            ("SO_REUSEADDR", PyObject::int(2)),
            ("SO_KEEPALIVE", PyObject::int(9)),
            ("SO_LINGER", PyObject::int(13)),
            ("SO_RCVBUF", PyObject::int(8)),
            ("SO_SNDBUF", PyObject::int(7)),
            ("SO_REUSEPORT", PyObject::int(15)),
            ("SO_BROADCAST", PyObject::int(6)),
            ("SO_OOBINLINE", PyObject::int(10)),
            ("SO_RCVTIMEO", PyObject::int(20)),
            ("SO_SNDTIMEO", PyObject::int(21)),
            ("SO_ERROR", PyObject::int(4)),
            ("SO_TYPE", PyObject::int(3)),
            // TCP options
            ("IPPROTO_IP", PyObject::int(0)),
            ("SOL_TCP", PyObject::int(6)),
            ("TCP_NODELAY", PyObject::int(1)),
            ("TCP_KEEPIDLE", PyObject::int(4)),
            ("TCP_KEEPINTVL", PyObject::int(5)),
            ("TCP_KEEPCNT", PyObject::int(6)),
            // Socket types extras
            ("SOCK_RAW", PyObject::int(3)),
            ("SOCK_NONBLOCK", PyObject::int(2048)),
            ("SOCK_CLOEXEC", PyObject::int(524288)),
            // Protocols
            ("IPPROTO_TCP", PyObject::int(6)),
            ("IPPROTO_UDP", PyObject::int(17)),
            // Shutdown constants
            ("SHUT_RD", PyObject::int(0)),
            ("SHUT_WR", PyObject::int(1)),
            ("SHUT_RDWR", PyObject::int(2)),
            // Exception types
            ("error", PyObject::exception_type(ExceptionKind::OSError)),
            ("timeout", PyObject::exception_type(ExceptionKind::TimeoutError)),
            // Module-level functions
            ("socket", make_builtin(socket_constructor)),
            ("gethostname", make_builtin(socket_gethostname)),
            ("gethostbyname", make_builtin(socket_gethostbyname)),
            ("getaddrinfo", make_builtin(socket_getaddrinfo)),
            ("getfqdn", make_builtin(socket_getfqdn)),
            ("create_connection", make_builtin(socket_create_connection)),
            ("socketpair", make_builtin(|_args: &[PyObjectRef]| {
                // Stub: socketpair is Unix-specific and rarely needed in pure Python
                Err(PyException::os_error("socketpair not available in Ferrython"))
            })),
            ("inet_aton", make_builtin(|args: &[PyObjectRef]| {
                let addr = args.first().map(|a| a.py_to_string()).unwrap_or_default();
                let parts: Vec<&str> = addr.split('.').collect();
                if parts.len() != 4 { return Err(PyException::os_error("illegal IP address string")); }
                let mut bytes = Vec::with_capacity(4);
                for p in parts {
                    let b: u8 = p.parse().map_err(|_| PyException::os_error("illegal IP address string"))?;
                    bytes.push(b);
                }
                Ok(PyObject::bytes(bytes))
            })),
            ("inet_ntoa", make_builtin(|args: &[PyObjectRef]| {
                let data = args.first().and_then(|a| match &a.payload { PyObjectPayload::Bytes(b) => Some(b.clone()), _ => None })
                    .unwrap_or_default();
                if data.len() != 4 { return Err(PyException::os_error("packed IP wrong length")); }
                Ok(PyObject::str_val(CompactString::from(format!("{}.{}.{}.{}", data[0], data[1], data[2], data[3]))))
            })),
            ("htons", make_builtin(|args: &[PyObjectRef]| {
                let v = args.first().and_then(|a| a.as_int()).unwrap_or(0) as u16;
                Ok(PyObject::int(v.to_be() as i64))
            })),
            ("htonl", make_builtin(|args: &[PyObjectRef]| {
                let v = args.first().and_then(|a| a.as_int()).unwrap_or(0) as u32;
                Ok(PyObject::int(v.to_be() as i64))
            })),
            ("ntohs", make_builtin(|args: &[PyObjectRef]| {
                let v = args.first().and_then(|a| a.as_int()).unwrap_or(0) as u16;
                Ok(PyObject::int(u16::from_be(v) as i64))
            })),
            ("ntohl", make_builtin(|args: &[PyObjectRef]| {
                let v = args.first().and_then(|a| a.as_int()).unwrap_or(0) as u32;
                Ok(PyObject::int(u32::from_be(v) as i64))
            })),
            ("getdefaulttimeout", make_builtin(|_| {
                let guard = DEFAULT_TIMEOUT.lock().unwrap();
                match *guard {
                    Some(t) => Ok(PyObject::float(t)),
                    None => Ok(PyObject::none()),
                }
            })),
            ("setdefaulttimeout", make_builtin(|args| {
                let val = args.first().ok_or_else(|| PyException::type_error("setdefaulttimeout requires 1 argument"))?;
                let mut guard = DEFAULT_TIMEOUT.lock().unwrap();
                match &val.payload {
                    PyObjectPayload::None => { *guard = None; }
                    PyObjectPayload::Float(f) => {
                        if *f < 0.0 {
                            return Err(PyException::value_error("Timeout value out of range"));
                        }
                        *guard = Some(*f);
                    }
                    _ => {
                        if let Some(i) = val.as_int() {
                            if i < 0 {
                                return Err(PyException::value_error("Timeout value out of range"));
                            }
                            *guard = Some(i as f64);
                        } else {
                            return Err(PyException::type_error("a float is required"));
                        }
                    }
                }
                Ok(PyObject::none())
            })),
            ("has_ipv6", PyObject::bool_val(true)),
            ("SOMAXCONN", PyObject::int(128)),
            ("AI_PASSIVE", PyObject::int(1)),
            ("AI_CANONNAME", PyObject::int(2)),
            ("AI_NUMERICHOST", PyObject::int(4)),
            ("NI_MAXHOST", PyObject::int(1025)),
            ("NI_MAXSERV", PyObject::int(32)),
            ("NI_NUMERICHOST", PyObject::int(1)),
            ("NI_NUMERICSERV", PyObject::int(2)),
            ("INADDR_ANY", PyObject::int(0)),
            ("INADDR_BROADCAST", PyObject::int(0xFFFFFFFFu32 as i64)),
            ("INADDR_LOOPBACK", PyObject::int(0x7F000001)),
            ("MSG_PEEK", PyObject::int(2)),
            ("MSG_OOB", PyObject::int(1)),
            ("MSG_WAITALL", PyObject::int(256)),
            ("MSG_DONTWAIT", PyObject::int(64)),
            // AddressFamily and SocketKind IntEnum classes
            ("AddressFamily", {
                let cls = PyObject::class(CompactString::from("AddressFamily"), vec![], IndexMap::new());
                if let PyObjectPayload::Class(ref cd) = cls.payload {
                    let mut ns = cd.namespace.write();
                    ns.insert(CompactString::from("AF_INET"), PyObject::int(2));
                    ns.insert(CompactString::from("AF_INET6"), PyObject::int(10));
                    ns.insert(CompactString::from("AF_UNIX"), PyObject::int(1));
                }
                cls
            }),
            ("SocketKind", {
                let cls = PyObject::class(CompactString::from("SocketKind"), vec![], IndexMap::new());
                if let PyObjectPayload::Class(ref cd) = cls.payload {
                    let mut ns = cd.namespace.write();
                    ns.insert(CompactString::from("SOCK_STREAM"), PyObject::int(1));
                    ns.insert(CompactString::from("SOCK_DGRAM"), PyObject::int(2));
                    ns.insert(CompactString::from("SOCK_RAW"), PyObject::int(3));
                }
                cls
            }),
        ],
    )
}

fn socket_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
    Ok(build_socket_object(
        family,
        sock_type,
        proto,
        None,
    ))
}

/// Build a socket-like module object.  If `existing` is provided, the inner
/// state is pre-populated (used by `accept()` and `create_connection()`).
fn build_socket_object(
    family: i64,
    sock_type: i64,
    proto: i64,
    existing: Option<SocketInner>,
) -> PyObjectRef {
    let inner = Arc::new(Mutex::new(
        existing.unwrap_or_else(|| SocketInner::new(family, sock_type, proto)),
    ));

    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("__socket__"),
        PyObject::bool_val(true),
    );
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
                        Ok(s) => { result = Some(s); break; }
                        Err(e) => last_err = Some(e),
                    }
                }
                match result {
                    Some(s) => Ok(s),
                    None => Err(last_err.unwrap_or_else(|| {
                        std::io::Error::new(std::io::ErrorKind::AddrNotAvailable, "no addresses to connect to")
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
                                fd, libc::SOL_SOCKET, libc::SO_RCVTIMEO,
                                &tv as *const libc::timeval as *const libc::c_void,
                                std::mem::size_of::<libc::timeval>() as libc::socklen_t,
                            );
                        }
                    }
                    for &(level, optname, value) in &guard.options {
                        let val = value as libc::c_int;
                        unsafe {
                            libc::setsockopt(
                                fd, level as libc::c_int, optname as libc::c_int,
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
                                    fd, libc::SOL_SOCKET, libc::SO_RCVTIMEO,
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
                                    fd, level as libc::c_int, optname as libc::c_int,
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
                let lc = listener.try_clone()
                    .map_err(|e| PyException::os_error(format!("accept clone: {}", e)))?;
                (lc, guard.timeout, guard.family, guard.sock_type, guard.proto)
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
                                    ExceptionKind::TimeoutError, "timed out",
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

    // ── send(data) → int ──
    let st = inner.clone();
    attrs.insert(
        CompactString::from("send"),
        PyObject::native_closure("send", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("send() requires a data argument"));
            }
            let data = match &args[0].payload {
                PyObjectPayload::Bytes(b) => b.clone(),
                PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                _ => {
                    return Err(PyException::type_error(
                        "a bytes-like object is required, not 'str'",
                    ))
                }
            };
            let mut guard = lock_inner(&st)?;
            if guard.closed {
                return Err(PyException::os_error("[Errno 9] Bad file descriptor"));
            }
            if let Some(ref mut stream) = guard.tcp_stream {
                match stream.write(&data) {
                    Ok(n) => Ok(PyObject::int(n as i64)),
                    Err(e) => Err(PyException::os_error(format!("send: {}", e))),
                }
            } else if let Some(ref sock) = guard.udp_socket {
                match sock.send(&data) {
                    Ok(n) => Ok(PyObject::int(n as i64)),
                    Err(e) => Err(PyException::os_error(format!("send: {}", e))),
                }
            } else {
                Err(PyException::os_error(
                    "[Errno 32] Broken pipe: socket is not connected",
                ))
            }
        }),
    );

    // ── sendall(data) ──
    let st = inner.clone();
    attrs.insert(
        CompactString::from("sendall"),
        PyObject::native_closure("sendall", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "sendall() requires a data argument",
                ));
            }
            let data = match &args[0].payload {
                PyObjectPayload::Bytes(b) => b.clone(),
                PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                _ => {
                    return Err(PyException::type_error(
                        "a bytes-like object is required",
                    ))
                }
            };
            let mut guard = lock_inner(&st)?;
            if guard.closed {
                return Err(PyException::os_error("[Errno 9] Bad file descriptor"));
            }
            if let Some(ref mut stream) = guard.tcp_stream {
                stream
                    .write_all(&data)
                    .map_err(|e| PyException::os_error(format!("sendall: {}", e)))?;
                Ok(PyObject::none())
            } else {
                Err(PyException::os_error("socket is not connected"))
            }
        }),
    );

    // ── recv(bufsize) → bytes ──
    let st = inner.clone();
    attrs.insert(
        CompactString::from("recv"),
        PyObject::native_closure("recv", move |args| {
            let bufsize = if !args.is_empty() {
                args[0].as_int().unwrap_or(4096)
            } else {
                4096
            };
            if bufsize < 0 {
                return Err(PyException::value_error("negative buffersize in recv"));
            }
            // Clone the stream so we can read without holding the lock
            let stream_clone = {
                let guard = lock_inner(&st)?;
                if guard.closed {
                    return Err(PyException::os_error("[Errno 9] Bad file descriptor"));
                }
                if let Some(ref stream) = guard.tcp_stream {
                    Some(stream.try_clone().map_err(|e|
                        PyException::os_error(format!("recv clone: {}", e)))?)
                } else if let Some(ref sock) = guard.udp_socket {
                    // For UDP, read directly under lock (typically non-blocking)
                    let mut buf = vec![0u8; bufsize as usize];
                    return match sock.recv(&mut buf) {
                        Ok(n) => { buf.truncate(n); Ok(PyObject::bytes(buf)) }
                        Err(e) => Err(PyException::os_error(format!("recv: {}", e))),
                    };
                } else {
                    None
                }
            };
            // Read outside the lock for TCP
            if let Some(mut stream) = stream_clone {
                let mut buf = vec![0u8; bufsize as usize];
                match stream.read(&mut buf) {
                    Ok(n) => {
                        buf.truncate(n);
                        Ok(PyObject::bytes(buf))
                    }
                    Err(e)
                        if e.kind() == std::io::ErrorKind::TimedOut
                            || e.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        Err(PyException::new(ExceptionKind::TimeoutError, "timed out"))
                    }
                    Err(e) => Err(PyException::os_error(format!("recv: {}", e))),
                }
            } else {
                Err(PyException::os_error("socket is not connected"))
            }
        }),
    );

    // ── close() ──
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

    // ── shutdown(how) ──
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

    // ── settimeout(seconds) ──
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
            // Apply to existing stream/socket
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

    // ── setsockopt(level, optname, value) ──
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
                let blocking = if !args.is_empty() { args[0].is_truthy() } else { true };
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
            // Apply option to real socket if connected
            #[cfg(unix)]
            {
                use std::os::unix::io::AsRawFd;
                let fd = guard.tcp_stream.as_ref().map(|s| s.as_raw_fd())
                    .or_else(|| guard.udp_socket.as_ref().map(|s| s.as_raw_fd()))
                    .or_else(|| guard.tcp_listener.as_ref().map(|s| s.as_raw_fd()));
                if let Some(fd) = fd {
                    let val = value as libc::c_int;
                    unsafe {
                        libc::setsockopt(
                            fd, level as libc::c_int, optname as libc::c_int,
                            &val as *const libc::c_int as *const libc::c_void,
                            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                        );
                    }
                }
            }
            Ok(PyObject::none())
        }),
    );

    // ── getsockopt(level, optname) → int ──
    let st = inner.clone();
    attrs.insert(
        CompactString::from("getsockopt"),
        PyObject::native_closure("getsockopt", move |args| {
            let level = if !args.is_empty() { args[0].as_int().unwrap_or(0) } else { 0 };
            let optname = if args.len() > 1 { args[1].as_int().unwrap_or(0) } else { 0 };
            let guard = lock_inner(&st)?;
            #[cfg(unix)]
            {
                use std::os::unix::io::AsRawFd;
                let fd = guard.tcp_stream.as_ref().map(|s| s.as_raw_fd())
                    .or_else(|| guard.udp_socket.as_ref().map(|s| s.as_raw_fd()))
                    .or_else(|| guard.tcp_listener.as_ref().map(|s| s.as_raw_fd()));
                if let Some(fd) = fd {
                    let mut val: libc::c_int = 0;
                    let mut len: libc::socklen_t = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
                    let rc = unsafe {
                        libc::getsockopt(fd, level as libc::c_int, optname as libc::c_int,
                            &mut val as *mut libc::c_int as *mut libc::c_void, &mut len)
                    };
                    if rc == 0 {
                        return Ok(PyObject::int(val as i64));
                    }
                }
            }
            // Fallback: check stored options
            for &(l, o, v) in &guard.options {
                if l == level && o == optname { return Ok(PyObject::int(v)); }
            }
            Ok(PyObject::int(0))
        }),
    );

    // ── getsockname() → (host, port) ──
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

    // ── getpeername() → (host, port) ──
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

    // ── sendto(data, address) → int (UDP) ──
    let st = inner.clone();
    attrs.insert(
        CompactString::from("sendto"),
        PyObject::native_closure("sendto", move |args| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "sendto() requires data and address arguments",
                ));
            }
            let data = match &args[0].payload {
                PyObjectPayload::Bytes(b) => b.clone(),
                PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                _ => {
                    return Err(PyException::type_error(
                        "a bytes-like object is required",
                    ))
                }
            };
            let (host, port) = extract_host_port(&args[1])?;
            let dest = format!("{}:{}", host, port);
            let mut guard = lock_inner(&st)?;
            if guard.closed {
                return Err(PyException::os_error("[Errno 9] Bad file descriptor"));
            }
            if guard.udp_socket.is_none() && guard.sock_type == 2 {
                match UdpSocket::bind("0.0.0.0:0") {
                    Ok(sock) => {
                        if let Some(t) = guard.timeout {
                            sock.set_read_timeout(Some(t)).ok();
                            sock.set_write_timeout(Some(t)).ok();
                        }
                        guard.udp_socket = Some(sock);
                    }
                    Err(e) => return Err(PyException::os_error(format!("sendto bind: {}", e))),
                }
            }
            if let Some(ref sock) = guard.udp_socket {
                match sock.send_to(&data, &dest) {
                    Ok(n) => Ok(PyObject::int(n as i64)),
                    Err(e) => Err(PyException::os_error(format!("sendto: {}", e))),
                }
            } else {
                Err(PyException::os_error("sendto() on non-UDP socket without connection"))
            }
        }),
    );

    // ── recvfrom(bufsize) → (bytes, (host, port)) ──
    let st = inner.clone();
    attrs.insert(
        CompactString::from("recvfrom"),
        PyObject::native_closure("recvfrom", move |args| {
            let bufsize = if !args.is_empty() {
                args[0].as_int().unwrap_or(4096)
            } else {
                4096
            };
            if bufsize < 0 {
                return Err(PyException::value_error("negative buffersize in recvfrom"));
            }
            let guard = lock_inner(&st)?;
            if guard.closed {
                return Err(PyException::os_error("[Errno 9] Bad file descriptor"));
            }
            if let Some(ref sock) = guard.udp_socket {
                let mut buf = vec![0u8; bufsize as usize];
                match sock.recv_from(&mut buf) {
                    Ok((n, addr)) => {
                        buf.truncate(n);
                        let addr_tuple = PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from(addr.ip().to_string())),
                            PyObject::int(addr.port() as i64),
                        ]);
                        Ok(PyObject::tuple(vec![PyObject::bytes(buf), addr_tuple]))
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut
                        || e.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        Err(PyException::new(ExceptionKind::TimeoutError, "timed out"))
                    }
                    Err(e) => Err(PyException::os_error(format!("recvfrom: {}", e))),
                }
            } else {
                Err(PyException::os_error("recvfrom() requires a bound UDP socket"))
            }
        }),
    );

    // ── makefile(mode='r', buffering=-1) → file-like wrapper ──
    let st = inner.clone();
    attrs.insert(
        CompactString::from("makefile"),
        PyObject::native_closure("makefile", move |args| {
            let mode = if !args.is_empty() {
                args[0].py_to_string()
            } else {
                "r".to_string()
            };
            let guard = lock_inner(&st)?;
            if guard.closed {
                return Err(PyException::os_error("[Errno 9] Bad file descriptor"));
            }
            if let Some(ref stream) = guard.tcp_stream {
                let cloned = stream.try_clone()
                    .map_err(|e| PyException::os_error(format!("makefile: {}", e)))?;
                let inner_stream = Arc::new(Mutex::new(cloned));
                let mut file_attrs = IndexMap::new();
                file_attrs.insert(
                    CompactString::from("mode"),
                    PyObject::str_val(CompactString::from(&mode)),
                );
                let is = inner_stream.clone();
                file_attrs.insert(
                    CompactString::from("read"),
                    PyObject::native_closure("read", move |args| {
                        let size = if !args.is_empty() { args[0].as_int().unwrap_or(-1) } else { -1 };
                        let mut stream = is.lock().map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;
                        if size < 0 {
                            let mut buf = Vec::new();
                            stream.read_to_end(&mut buf).map_err(|e| PyException::os_error(format!("read: {}", e)))?;
                            Ok(PyObject::bytes(buf))
                        } else {
                            let mut buf = vec![0u8; size as usize];
                            let n = stream.read(&mut buf).map_err(|e| PyException::os_error(format!("read: {}", e)))?;
                            buf.truncate(n);
                            Ok(PyObject::bytes(buf))
                        }
                    }),
                );
                let is = inner_stream.clone();
                file_attrs.insert(
                    CompactString::from("readline"),
                    PyObject::native_closure("readline", {
                        let mode = mode.clone();
                        move |_args| {
                            let mut stream = is.lock().map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;
                            let mut line = Vec::new();
                            let mut byte = [0u8; 1];
                            loop {
                                match stream.read(&mut byte) {
                                    Ok(0) => break,
                                    Ok(_) => {
                                        line.push(byte[0]);
                                        if byte[0] == b'\n' { break; }
                                    }
                                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut
                                        || e.kind() == std::io::ErrorKind::WouldBlock => break,
                                    Err(e) => return Err(PyException::os_error(format!("readline: {}", e))),
                                }
                            }
                            if mode.contains('b') {
                                Ok(PyObject::bytes(line))
                            } else {
                                Ok(PyObject::str_val(CompactString::from(String::from_utf8_lossy(&line).as_ref())))
                            }
                        }
                    }),
                );
                let is = inner_stream.clone();
                file_attrs.insert(
                    CompactString::from("write"),
                    PyObject::native_closure("write", move |args| {
                        if args.is_empty() { return Err(PyException::type_error("write() requires data")); }
                        let data = match &args[0].payload {
                            PyObjectPayload::Bytes(b) => b.clone(),
                            PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                            _ => return Err(PyException::type_error("a bytes-like object is required")),
                        };
                        let mut stream = is.lock().map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;
                        let n = stream.write(&data).map_err(|e| PyException::os_error(format!("write: {}", e)))?;
                        Ok(PyObject::int(n as i64))
                    }),
                );
                let is = inner_stream.clone();
                file_attrs.insert(
                    CompactString::from("flush"),
                    PyObject::native_closure("flush", move |_args| {
                        let mut stream = is.lock().map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;
                        stream.flush().map_err(|e| PyException::os_error(format!("flush: {}", e)))?;
                        Ok(PyObject::none())
                    }),
                );
                file_attrs.insert(
                    CompactString::from("close"),
                    PyObject::native_closure("close", move |_args| Ok(PyObject::none())),
                );
                file_attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));
                Ok(PyObject::module_with_attrs(CompactString::from("socket.makefile"), file_attrs))
            } else {
                Err(PyException::os_error("makefile() requires a connected TCP socket"))
            }
        }),
    );

    // ── __repr__ ──
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
                    let fd = guard.tcp_stream.as_ref().map(|s| s.as_raw_fd())
                        .or_else(|| guard.udp_socket.as_ref().map(|s| s.as_raw_fd()))
                        .or_else(|| guard.tcp_listener.as_ref().map(|s| s.as_raw_fd()));
                    match fd {
                        Some(fd) => format!("fd={}", fd),
                        None => "fd=-1".to_string(),
                    }
                }
                #[cfg(not(unix))]
                { "fd=-1".to_string() }
            };
            Ok(PyObject::str_val(CompactString::from(format!(
                "<socket.socket {}, family={}, type={}, proto={}>",
                fd_str, guard.family, guard.sock_type, guard.proto
            ))))
        }),
    );

    // ── __enter__ / __exit__ for context manager ──
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

    attrs.insert(
        CompactString::from("_bind_methods"),
        PyObject::bool_val(true),
    );

    let socket_cls = PyObject::class(CompactString::from("socket"), vec![], IndexMap::new());
    PyObject::instance_with_attrs(socket_cls, attrs)
}

// ── Module-level socket functions ──────────────────────────────────────

fn socket_gethostname(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Try the libc gethostname approach via /etc/hostname, then env
    let hostname = std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "localhost".to_string());
    Ok(PyObject::str_val(CompactString::from(hostname)))
}

fn socket_getfqdn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let name = if args.is_empty() || args[0].py_to_string().is_empty() {
        std::fs::read_to_string("/etc/hostname")
            .map(|s| s.trim().to_string())
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "localhost".to_string())
    } else {
        args[0].py_to_string()
    };
    // Try to resolve the hostname to get FQDN
    use std::net::ToSocketAddrs;
    match format!("{}:0", name).to_socket_addrs() {
        Ok(mut addrs) => {
            if let Some(_addr) = addrs.next() {
                // In practice, getting the reverse DNS name would require more work.
                // Return the hostname as-is, matching CPython behavior when DNS doesn't resolve FQDN.
                Ok(PyObject::str_val(CompactString::from(name)))
            } else {
                Ok(PyObject::str_val(CompactString::from(name)))
            }
        }
        Err(_) => Ok(PyObject::str_val(CompactString::from(name))),
    }
}

fn socket_gethostbyname(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "gethostbyname() takes exactly 1 argument",
        ));
    }
    let hostname = args[0].py_to_string();
    if hostname == "localhost" || hostname == "127.0.0.1" {
        return Ok(PyObject::str_val(CompactString::from("127.0.0.1")));
    }
    // Try DNS resolution
    use std::net::ToSocketAddrs;
    let addr_str = format!("{}:0", hostname);
    match addr_str.to_socket_addrs() {
        Ok(mut addrs) => {
            if let Some(addr) = addrs.next() {
                Ok(PyObject::str_val(CompactString::from(
                    addr.ip().to_string(),
                )))
            } else {
                Err(PyException::os_error(format!(
                    "getaddrinfo failed for host '{}'",
                    hostname
                )))
            }
        }
        Err(e) => Err(PyException::os_error(format!(
            "getaddrinfo failed for host '{}': {}",
            hostname, e
        ))),
    }
}

fn socket_getaddrinfo(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "getaddrinfo() requires at least 2 arguments",
        ));
    }
    let host = args[0].py_to_string();
    let port = args[1].as_int().unwrap_or(0);
    let family = if args.len() > 2 {
        args[2].as_int().unwrap_or(0)
    } else {
        0
    };
    let stype = if args.len() > 3 {
        args[3].as_int().unwrap_or(0)
    } else {
        0
    };

    use std::net::ToSocketAddrs;
    let addr_str = format!("{}:{}", host, port);
    match addr_str.to_socket_addrs() {
        Ok(addrs) => {
            let mut results = Vec::new();
            for addr in addrs {
                let af = if addr.is_ipv4() { 2 } else { 10 };
                if family != 0 && af != family {
                    continue;
                }
                let st = if stype != 0 { stype } else { 1 }; // default SOCK_STREAM
                let proto = if st == 2 { 17 } else { 6 }; // UDP=17, TCP=6
                let entry = PyObject::tuple(vec![
                    PyObject::int(af),
                    PyObject::int(st),
                    PyObject::int(proto),
                    PyObject::str_val(CompactString::from("")),
                    PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from(addr.ip().to_string())),
                        PyObject::int(addr.port() as i64),
                    ]),
                ]);
                results.push(entry);
            }
            if results.is_empty() {
                // Fallback
                results.push(PyObject::tuple(vec![
                    PyObject::int(2),
                    PyObject::int(1),
                    PyObject::int(6),
                    PyObject::str_val(CompactString::from("")),
                    PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from(host)),
                        PyObject::int(port),
                    ]),
                ]));
            }
            Ok(PyObject::list(results))
        }
        Err(_) => {
            // Return a fallback entry using the host string as-is
            let entry = PyObject::tuple(vec![
                PyObject::int(2),
                PyObject::int(1),
                PyObject::int(6),
                PyObject::str_val(CompactString::from("")),
                PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from(host)),
                    PyObject::int(port),
                ]),
            ]);
            Ok(PyObject::list(vec![entry]))
        }
    }
}

fn socket_create_connection(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "create_connection() requires an address argument",
        ));
    }
    let (host, port) = extract_host_port(&args[0])?;
    let timeout = if args.len() > 1 && !matches!(&args[1].payload, PyObjectPayload::None) {
        Some(Duration::from_secs_f64(
            args[1].to_float().unwrap_or(30.0),
        ))
    } else {
        None
    };

    let addr_str = format!("{}:{}", host, port);
    let stream = if let Some(t) = timeout {
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
            Some(s) => s,
            None => {
                return Err(PyException::os_error(format!(
                    "create_connection: {}",
                    last_err
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "connection failed".to_string())
                )))
            }
        }
    } else {
        TcpStream::connect(&addr_str)
            .map_err(|e| PyException::os_error(format!("create_connection: {}", e)))?
    };

    let family: i64 = if stream.local_addr().map(|a| a.is_ipv4()).unwrap_or(true) {
        2
    } else {
        10
    };
    let mut si = SocketInner::new(family, 1, 6);
    if let Some(t) = timeout {
        stream.set_read_timeout(Some(t)).ok();
        stream.set_write_timeout(Some(t)).ok();
        si.timeout = Some(t);
    }
    si.tcp_stream = Some(stream);

    Ok(build_socket_object(family, 1, 6, Some(si)))
}
