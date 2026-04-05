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
        Self {
            family,
            sock_type,
            proto,
            tcp_stream: None,
            tcp_listener: None,
            udp_socket: None,
            bound_addr: None,
            timeout: None,
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
            ("create_connection", make_builtin(socket_create_connection)),
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
            let mut guard = lock_inner(&st)?;
            if guard.closed {
                return Err(PyException::os_error("[Errno 9] Bad file descriptor"));
            }
            match TcpStream::connect(&addr_str) {
                Ok(stream) => {
                    if let Some(t) = guard.timeout {
                        stream.set_read_timeout(Some(t)).ok();
                        stream.set_write_timeout(Some(t)).ok();
                    }
                    guard.tcp_stream = Some(stream);
                    Ok(PyObject::none())
                }
                Err(e) => Err(PyException::os_error(format!(
                    "[Errno 111] Connection refused: {}",
                    e
                ))),
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
                        guard.udp_socket = Some(sock);
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
            let addr = guard
                .bound_addr
                .clone()
                .unwrap_or_else(|| "0.0.0.0:0".to_string());
            match TcpListener::bind(&addr) {
                Ok(listener) => {
                    guard.tcp_listener = Some(listener);
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
            let guard = lock_inner(&st)?;
            if guard.closed {
                return Err(PyException::os_error("[Errno 9] Bad file descriptor"));
            }
            let listener = guard.tcp_listener.as_ref().ok_or_else(|| {
                PyException::os_error("[Errno 22] Invalid argument: not listening")
            })?;
            match listener.accept() {
                Ok((stream, addr)) => {
                    let peer_host = addr.ip().to_string();
                    let peer_port = addr.port() as i64;
                    let fam = guard.family;
                    let st = guard.sock_type;
                    let pr = guard.proto;
                    drop(guard);
                    let mut si = SocketInner::new(fam, st, pr);
                    si.tcp_stream = Some(stream);
                    let conn = build_socket_object(fam, st, pr, Some(si));
                    let addr_tuple = PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from(&peer_host)),
                        PyObject::int(peer_port),
                    ]);
                    Ok(PyObject::tuple(vec![conn, addr_tuple]))
                }
                Err(e) => Err(PyException::os_error(format!("accept: {}", e))),
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
            let mut guard = lock_inner(&st)?;
            if guard.closed {
                return Err(PyException::os_error("[Errno 9] Bad file descriptor"));
            }
            if let Some(ref mut stream) = guard.tcp_stream {
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
            } else if let Some(ref sock) = guard.udp_socket {
                let mut buf = vec![0u8; bufsize as usize];
                match sock.recv(&mut buf) {
                    Ok(n) => {
                        buf.truncate(n);
                        Ok(PyObject::bytes(buf))
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

    // ── setsockopt(level, optname, value) — stub ──
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
            Ok(PyObject::none())
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

    // ── fileno() — stub returning -1 ──
    attrs.insert(
        CompactString::from("fileno"),
        PyObject::native_closure("fileno", |_args| Ok(PyObject::int(-1))),
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

    PyObject::module_with_attrs(CompactString::from("socket"), attrs)
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
