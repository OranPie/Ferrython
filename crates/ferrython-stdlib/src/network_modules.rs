//! Network stdlib modules: socket, urllib, http

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin,
};
use ferrython_core::types::HashableKey;
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

struct ParsedUrl {
    scheme: String,
    host: String,
    port: u16,
    path: String,
    query: String,
    fragment: String,
    netloc: String,
}

fn parse_url_string(url: &str) -> ParsedUrl {
    let (scheme, rest) = if let Some(idx) = url.find("://") {
        (url[..idx].to_string(), &url[idx + 3..])
    } else {
        ("http".to_string(), url)
    };

    let (rest2, fragment) = if let Some(idx) = rest.find('#') {
        (&rest[..idx], rest[idx + 1..].to_string())
    } else {
        (rest, String::new())
    };

    let (rest3, query) = if let Some(idx) = rest2.find('?') {
        (&rest2[..idx], rest2[idx + 1..].to_string())
    } else {
        (rest2, String::new())
    };

    let (host_port, path) = if let Some(idx) = rest3.find('/') {
        (&rest3[..idx], rest3[idx..].to_string())
    } else {
        (rest3, "/".to_string())
    };

    let netloc = host_port.to_string();
    let (host, port) = if let Some(idx) = host_port.rfind(':') {
        let port_str = &host_port[idx + 1..];
        if let Ok(p) = port_str.parse::<u16>() {
            (host_port[..idx].to_string(), p)
        } else {
            (
                host_port.to_string(),
                if scheme == "https" { 443 } else { 80 },
            )
        }
    } else {
        (
            host_port.to_string(),
            if scheme == "https" { 443 } else { 80 },
        )
    };

    ParsedUrl {
        scheme,
        host,
        port,
        path,
        query,
        fragment,
        netloc,
    }
}

fn percent_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", b));
            }
        }
    }
    result
}

fn percent_decode(s: &str) -> String {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(val) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00"), 16)
            {
                result.push(val);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).to_string()
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

// ════════════════════════════════════════════════════════════════════════
// urllib module (urllib.request)
// ════════════════════════════════════════════════════════════════════════

pub fn create_urllib_module() -> PyObjectRef {
    make_module(
        "urllib.request",
        vec![
            ("urlopen", make_builtin(urllib_urlopen)),
            ("Request", make_builtin(urllib_request_constructor)),
        ],
    )
}

fn build_http_get(parsed: &ParsedUrl) -> String {
    let full_path = if parsed.query.is_empty() {
        parsed.path.clone()
    } else {
        format!("{}?{}", parsed.path, parsed.query)
    };
    format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: ferrython/1.0\r\nAccept: */*\r\n\r\n",
        full_path, parsed.host
    )
}

fn do_http_request(url: &str) -> PyResult<(u16, IndexMap<String, String>, Vec<u8>)> {
    let parsed = parse_url_string(url);
    if parsed.scheme == "https" {
        return Err(PyException::os_error(
            "HTTPS is not supported (no TLS available)",
        ));
    }

    let addr = format!("{}:{}", parsed.host, parsed.port);
    let mut stream = TcpStream::connect(&addr)
        .map_err(|e| PyException::os_error(format!("urlopen: {}", e)))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .ok();

    let request = build_http_get(&parsed);
    stream
        .write_all(request.as_bytes())
        .map_err(|e| PyException::os_error(format!("urlopen write: {}", e)))?;

    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| PyException::os_error(format!("urlopen read: {}", e)))?;

    // Parse HTTP response
    let raw_str = String::from_utf8_lossy(&raw);
    let header_end = raw_str.find("\r\n\r\n").unwrap_or(raw_str.len());
    let header_section = &raw_str[..header_end];
    let body_start = if header_end + 4 <= raw.len() {
        header_end + 4
    } else {
        raw.len()
    };
    let body = raw[body_start..].to_vec();

    let mut lines = header_section.lines();
    let status_line = lines.next().unwrap_or("HTTP/1.1 200 OK");
    let status_code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);

    let mut headers = IndexMap::new();
    for line in lines {
        if let Some(idx) = line.find(':') {
            let key = line[..idx].trim().to_string();
            let val = line[idx + 1..].trim().to_string();
            headers.insert(key, val);
        }
    }

    Ok((status_code, headers, body))
}

fn build_response_object(
    url: &str,
    status: u16,
    headers: IndexMap<String, String>,
    body: Vec<u8>,
) -> PyObjectRef {
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("__urllib_response__"),
        PyObject::bool_val(true),
    );
    attrs.insert(
        CompactString::from("url"),
        PyObject::str_val(CompactString::from(url)),
    );
    attrs.insert(CompactString::from("status"), PyObject::int(status as i64));
    attrs.insert(CompactString::from("code"), PyObject::int(status as i64));
    attrs.insert(
        CompactString::from("reason"),
        PyObject::str_val(CompactString::from(http_reason(status))),
    );

    // Build headers dict
    let mut hdr_map = IndexMap::new();
    for (k, v) in &headers {
        hdr_map.insert(
            HashableKey::Str(CompactString::from(k.as_str())),
            PyObject::str_val(CompactString::from(v.as_str())),
        );
    }
    attrs.insert(CompactString::from("headers"), PyObject::dict(hdr_map));

    let body_arc = Arc::new(body);
    let body_pos = Arc::new(Mutex::new(0usize));

    // read(n=-1) → bytes
    let b = body_arc.clone();
    let pos = body_pos.clone();
    attrs.insert(
        CompactString::from("read"),
        PyObject::native_closure("read", move |args| {
            let n = if !args.is_empty() {
                args[0].as_int().unwrap_or(-1)
            } else {
                -1
            };
            let mut p = pos.lock().unwrap();
            let remaining = &b[*p..];
            let chunk = if n < 0 {
                remaining.to_vec()
            } else {
                let end = std::cmp::min(n as usize, remaining.len());
                remaining[..end].to_vec()
            };
            *p += chunk.len();
            Ok(PyObject::bytes(chunk))
        }),
    );

    // readline() → bytes
    let b = body_arc.clone();
    let pos = body_pos.clone();
    attrs.insert(
        CompactString::from("readline"),
        PyObject::native_closure("readline", move |_args| {
            let mut p = pos.lock().unwrap();
            let remaining = &b[*p..];
            let end = remaining
                .iter()
                .position(|&c| c == b'\n')
                .map(|i| i + 1)
                .unwrap_or(remaining.len());
            let line = remaining[..end].to_vec();
            *p += line.len();
            Ok(PyObject::bytes(line))
        }),
    );

    // getcode() → int
    let sc = status;
    attrs.insert(
        CompactString::from("getcode"),
        PyObject::native_closure("getcode", move |_args| Ok(PyObject::int(sc as i64))),
    );

    // geturl() → str
    let u = url.to_string();
    attrs.insert(
        CompactString::from("geturl"),
        PyObject::native_closure("geturl", move |_args| {
            Ok(PyObject::str_val(CompactString::from(u.as_str())))
        }),
    );

    // close() — no-op
    attrs.insert(
        CompactString::from("close"),
        PyObject::native_closure("close", |_args| Ok(PyObject::none())),
    );

    // __enter__ / __exit__
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
    attrs.insert(
        CompactString::from("__exit__"),
        PyObject::native_closure("__exit__", |_args| Ok(PyObject::bool_val(false))),
    );

    attrs.insert(
        CompactString::from("_bind_methods"),
        PyObject::bool_val(true),
    );

    PyObject::module_with_attrs(CompactString::from("http.client.HTTPResponse"), attrs)
}

fn urllib_urlopen(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "urlopen() requires a url argument",
        ));
    }
    // Accept a string URL or a Request object
    let url = if let Some(u) = args[0].get_attr("full_url") {
        u.py_to_string()
    } else {
        args[0].py_to_string()
    };

    let (status, headers, body) = do_http_request(&url)?;
    Ok(build_response_object(&url, status, headers, body))
}

fn urllib_request_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "Request() requires a url argument",
        ));
    }
    let url = args[0].py_to_string();
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("full_url"),
        PyObject::str_val(CompactString::from(url.as_str())),
    );
    attrs.insert(
        CompactString::from("host"),
        PyObject::str_val(CompactString::from(
            parse_url_string(&url).host.as_str(),
        )),
    );
    attrs.insert(
        CompactString::from("type"),
        PyObject::str_val(CompactString::from(
            parse_url_string(&url).scheme.as_str(),
        )),
    );
    attrs.insert(
        CompactString::from("method"),
        PyObject::str_val(CompactString::from("GET")),
    );
    attrs.insert(
        CompactString::from("headers"),
        PyObject::dict(IndexMap::new()),
    );
    attrs.insert(
        CompactString::from("add_header"),
        PyObject::native_closure("add_header", |_args| Ok(PyObject::none())),
    );
    Ok(PyObject::module_with_attrs(
        CompactString::from("urllib.request.Request"),
        attrs,
    ))
}

// ════════════════════════════════════════════════════════════════════════
// urllib.parse module
// ════════════════════════════════════════════════════════════════════════

pub fn create_urllib_parse_module() -> PyObjectRef {
    make_module(
        "urllib.parse",
        vec![
            ("urlencode", make_builtin(urllib_parse_urlencode)),
            ("quote", make_builtin(urllib_parse_quote)),
            ("quote_plus", make_builtin(urllib_parse_quote_plus)),
            ("unquote", make_builtin(urllib_parse_unquote)),
            ("unquote_plus", make_builtin(urllib_parse_unquote_plus)),
            ("urlparse", make_builtin(urllib_parse_urlparse)),
            ("urljoin", make_builtin(urllib_parse_urljoin)),
            ("parse_qs", make_builtin(urllib_parse_parse_qs)),
            ("parse_qsl", make_builtin(urllib_parse_parse_qsl)),
        ],
    )
}

fn urllib_parse_urlencode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "urlencode() requires a mapping argument",
        ));
    }
    let mut pairs = Vec::new();
    match &args[0].payload {
        PyObjectPayload::Dict(d) => {
            let d = d.read();
            for (k, v) in d.iter() {
                let ks = match k {
                    HashableKey::Str(s) => s.to_string(),
                    HashableKey::Int(i) => i.to_string(),
                    _ => format!("{:?}", k),
                };
                pairs.push(format!(
                    "{}={}",
                    percent_encode(&ks),
                    percent_encode(&v.py_to_string())
                ));
            }
        }
        PyObjectPayload::List(items) => {
            let items = items.read();
            for item in items.iter() {
                if let PyObjectPayload::Tuple(pair) = &item.payload {
                    if pair.len() >= 2 {
                        pairs.push(format!(
                            "{}={}",
                            percent_encode(&pair[0].py_to_string()),
                            percent_encode(&pair[1].py_to_string())
                        ));
                    }
                }
            }
        }
        _ => {
            return Err(PyException::type_error(
                "urlencode requires a mapping or sequence",
            ))
        }
    }
    Ok(PyObject::str_val(CompactString::from(pairs.join("&"))))
}

fn urllib_parse_quote(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "quote() requires a string argument",
        ));
    }
    let s = args[0].py_to_string();
    let safe = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        "/".to_string()
    };
    let mut result = String::with_capacity(s.len());
    for b in s.bytes() {
        if (b as char).is_ascii_alphanumeric()
            || b == b'-'
            || b == b'_'
            || b == b'.'
            || b == b'~'
            || safe.as_bytes().contains(&b)
        {
            result.push(b as char);
        } else {
            result.push_str(&format!("%{:02X}", b));
        }
    }
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn urllib_parse_quote_plus(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "quote_plus() requires a string argument",
        ));
    }
    let s = args[0].py_to_string();
    let safe = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        String::new()
    };
    let mut result = String::with_capacity(s.len());
    for b in s.bytes() {
        if b == b' ' {
            result.push('+');
        } else if (b as char).is_ascii_alphanumeric()
            || b == b'-'
            || b == b'_'
            || b == b'.'
            || b == b'~'
            || safe.as_bytes().contains(&b)
        {
            result.push(b as char);
        } else {
            result.push_str(&format!("%{:02X}", b));
        }
    }
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn urllib_parse_unquote(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "unquote() requires a string argument",
        ));
    }
    let s = args[0].py_to_string();
    Ok(PyObject::str_val(CompactString::from(percent_decode(&s))))
}

fn urllib_parse_unquote_plus(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "unquote_plus() requires a string argument",
        ));
    }
    let s = args[0].py_to_string().replace('+', " ");
    Ok(PyObject::str_val(CompactString::from(percent_decode(&s))))
}

fn urllib_parse_urlparse(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "urlparse() requires a string argument",
        ));
    }
    let url = args[0].py_to_string();
    let p = parse_url_string(&url);
    // Return a named-tuple-like object with 6 components:
    // (scheme, netloc, path, params, query, fragment)
    let result = PyObject::tuple(vec![
        PyObject::str_val(CompactString::from(p.scheme)),
        PyObject::str_val(CompactString::from(p.netloc)),
        PyObject::str_val(CompactString::from(p.path)),
        PyObject::str_val(CompactString::from("")), // params (rarely used)
        PyObject::str_val(CompactString::from(p.query)),
        PyObject::str_val(CompactString::from(p.fragment)),
    ]);

    // Also expose as a module with named attrs for attribute access
    let mut attrs = IndexMap::new();
    let pr = parse_url_string(&url);
    attrs.insert(
        CompactString::from("scheme"),
        PyObject::str_val(CompactString::from(pr.scheme)),
    );
    attrs.insert(
        CompactString::from("netloc"),
        PyObject::str_val(CompactString::from(pr.netloc)),
    );
    attrs.insert(
        CompactString::from("path"),
        PyObject::str_val(CompactString::from(pr.path)),
    );
    attrs.insert(
        CompactString::from("params"),
        PyObject::str_val(CompactString::from("")),
    );
    attrs.insert(
        CompactString::from("query"),
        PyObject::str_val(CompactString::from(pr.query)),
    );
    attrs.insert(
        CompactString::from("fragment"),
        PyObject::str_val(CompactString::from(pr.fragment)),
    );
    attrs.insert(
        CompactString::from("hostname"),
        PyObject::str_val(CompactString::from(pr.host)),
    );
    attrs.insert(CompactString::from("port"), PyObject::int(pr.port as i64));

    // geturl()
    let url_c = url.clone();
    attrs.insert(
        CompactString::from("geturl"),
        PyObject::native_closure("geturl", move |_args| {
            Ok(PyObject::str_val(CompactString::from(url_c.as_str())))
        }),
    );

    // Iteration support — when used as a tuple, return the 6 components
    // For now, return the simple tuple since that's the most common usage
    let _ = attrs; // We build the named attrs but return the tuple for simplicity
    Ok(result)
}

fn urllib_parse_urljoin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "urljoin() requires 2 arguments",
        ));
    }
    let base = args[0].py_to_string();
    let url = args[1].py_to_string();

    // If url is absolute, return it directly
    if url.contains("://") {
        return Ok(PyObject::str_val(CompactString::from(url)));
    }

    let bp = parse_url_string(&base);

    let result = if url.starts_with('/') {
        format!("{}://{}{}", bp.scheme, bp.netloc, url)
    } else if url.starts_with("//") {
        format!("{}:{}", bp.scheme, url)
    } else if url.is_empty() {
        base
    } else {
        // Relative path — resolve against base path
        let base_dir = if let Some(idx) = bp.path.rfind('/') {
            &bp.path[..=idx]
        } else {
            "/"
        };
        format!("{}://{}{}{}", bp.scheme, bp.netloc, base_dir, url)
    };

    Ok(PyObject::str_val(CompactString::from(result)))
}

fn urllib_parse_parse_qs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "parse_qs() requires a string argument",
        ));
    }
    let qs = args[0].py_to_string();
    let mut result: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();

    if qs.is_empty() {
        return Ok(PyObject::dict(result));
    }

    for pair in qs.split('&') {
        let parts: Vec<&str> = pair.splitn(2, '=').collect();
        let key = percent_decode(parts[0]);
        let val = if parts.len() > 1 {
            percent_decode(parts[1])
        } else {
            String::new()
        };
        let hk = HashableKey::Str(CompactString::from(key.as_str()));
        let entry = result
            .entry(hk.clone())
            .or_insert_with(|| PyObject::list(vec![]));
        // Append to the list
        if let PyObjectPayload::List(items) = &entry.payload {
            items
                .write()
                .push(PyObject::str_val(CompactString::from(val.as_str())));
        }
    }

    Ok(PyObject::dict(result))
}

fn urllib_parse_parse_qsl(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "parse_qsl() requires a string argument",
        ));
    }
    let qs = args[0].py_to_string();
    let mut result = Vec::new();

    if qs.is_empty() {
        return Ok(PyObject::list(result));
    }

    for pair in qs.split('&') {
        let parts: Vec<&str> = pair.splitn(2, '=').collect();
        let key = percent_decode(parts[0]);
        let val = if parts.len() > 1 {
            percent_decode(parts[1])
        } else {
            String::new()
        };
        result.push(PyObject::tuple(vec![
            PyObject::str_val(CompactString::from(key)),
            PyObject::str_val(CompactString::from(val)),
        ]));
    }

    Ok(PyObject::list(result))
}

// ════════════════════════════════════════════════════════════════════════
// http module
// ════════════════════════════════════════════════════════════════════════

fn http_reason(code: u16) -> &'static str {
    match code {
        100 => "Continue",
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}

pub fn create_http_module() -> PyObjectRef {
    // Build HTTPStatus as an object with named constants
    let mut status_attrs = IndexMap::new();
    let statuses: Vec<(i64, &str)> = vec![
        (100, "CONTINUE"),
        (200, "OK"),
        (201, "CREATED"),
        (204, "NO_CONTENT"),
        (301, "MOVED_PERMANENTLY"),
        (302, "FOUND"),
        (304, "NOT_MODIFIED"),
        (400, "BAD_REQUEST"),
        (401, "UNAUTHORIZED"),
        (403, "FORBIDDEN"),
        (404, "NOT_FOUND"),
        (405, "METHOD_NOT_ALLOWED"),
        (408, "REQUEST_TIMEOUT"),
        (500, "INTERNAL_SERVER_ERROR"),
        (502, "BAD_GATEWAY"),
        (503, "SERVICE_UNAVAILABLE"),
        (504, "GATEWAY_TIMEOUT"),
    ];
    for (code, name) in &statuses {
        status_attrs.insert(CompactString::from(*name), PyObject::int(*code));
    }
    let http_status = PyObject::module_with_attrs(CompactString::from("HTTPStatus"), status_attrs);

    // HTTPConnection class
    let http_connection_fn = make_builtin(http_connection_constructor);

    make_module(
        "http",
        vec![
            ("HTTPStatus", http_status),
            ("HTTPConnection", http_connection_fn.clone()),
            // http.client sub-attributes
            ("client", {
                let mut client_attrs = IndexMap::new();
                client_attrs.insert(
                    CompactString::from("HTTPConnection"),
                    http_connection_fn,
                );
                client_attrs.insert(
                    CompactString::from("HTTPSConnection"),
                    make_builtin(|_args| {
                        Err(PyException::os_error(
                            "HTTPS is not supported (no TLS available)",
                        ))
                    }),
                );
                // Status code constants on client module
                client_attrs.insert(CompactString::from("OK"), PyObject::int(200));
                client_attrs.insert(CompactString::from("NOT_FOUND"), PyObject::int(404));
                client_attrs.insert(
                    CompactString::from("INTERNAL_SERVER_ERROR"),
                    PyObject::int(500),
                );
                PyObject::module_with_attrs(CompactString::from("http.client"), client_attrs)
            }),
            // Common status codes at top level
            ("OK", PyObject::int(200)),
            ("CREATED", PyObject::int(201)),
            ("NO_CONTENT", PyObject::int(204)),
            ("MOVED_PERMANENTLY", PyObject::int(301)),
            ("FOUND", PyObject::int(302)),
            ("NOT_MODIFIED", PyObject::int(304)),
            ("BAD_REQUEST", PyObject::int(400)),
            ("UNAUTHORIZED", PyObject::int(401)),
            ("FORBIDDEN", PyObject::int(403)),
            ("NOT_FOUND", PyObject::int(404)),
            ("INTERNAL_SERVER_ERROR", PyObject::int(500)),
            ("BAD_GATEWAY", PyObject::int(502)),
            ("SERVICE_UNAVAILABLE", PyObject::int(503)),
        ],
    )
}

fn http_connection_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "HTTPConnection() requires a host argument",
        ));
    }
    let host = args[0].py_to_string();
    let port: u16 = if args.len() > 1 {
        args[1].as_int().unwrap_or(80) as u16
    } else {
        // Check if host contains port
        if let Some(idx) = host.rfind(':') {
            host[idx + 1..].parse().unwrap_or(80)
        } else {
            80
        }
    };
    let host_only = if let Some(idx) = host.rfind(':') {
        if host[idx + 1..].parse::<u16>().is_ok() {
            host[..idx].to_string()
        } else {
            host.clone()
        }
    } else {
        host.clone()
    };

    let conn_state: Arc<Mutex<HttpConnState>> =
        Arc::new(Mutex::new(HttpConnState {
            host: host_only,
            port,
            stream: None,
            response_data: None,
        }));

    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("host"),
        PyObject::str_val(CompactString::from(host.as_str())),
    );
    attrs.insert(CompactString::from("port"), PyObject::int(port as i64));

    // request(method, url, body=None, headers=None)
    let cs = conn_state.clone();
    attrs.insert(
        CompactString::from("request"),
        PyObject::native_closure("request", move |args| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "request() requires method and url arguments",
                ));
            }
            let method = args[0].py_to_string();
            let url = args[1].py_to_string();
            let body = if args.len() > 2 && !matches!(&args[2].payload, PyObjectPayload::None) {
                Some(args[2].py_to_string())
            } else {
                None
            };

            let mut extra_headers = IndexMap::new();
            if args.len() > 3 {
                if let PyObjectPayload::Dict(d) = &args[3].payload {
                    let d = d.read();
                    for (k, v) in d.iter() {
                        let ks = match k {
                            HashableKey::Str(s) => s.to_string(),
                            HashableKey::Int(i) => i.to_string(),
                            _ => format!("{:?}", k),
                        };
                        extra_headers.insert(ks, v.py_to_string());
                    }
                }
            }

            let mut guard = cs
                .lock()
                .map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;

            let addr = format!("{}:{}", guard.host, guard.port);
            let mut stream = TcpStream::connect(&addr)
                .map_err(|e| PyException::os_error(format!("HTTPConnection: {}", e)))?;
            stream.set_read_timeout(Some(Duration::from_secs(30))).ok();

            let mut req = format!(
                "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
                method, url, guard.host
            );
            for (k, v) in &extra_headers {
                req.push_str(&format!("{}: {}\r\n", k, v));
            }
            if let Some(ref b) = body {
                req.push_str(&format!("Content-Length: {}\r\n", b.len()));
            }
            req.push_str("\r\n");
            if let Some(ref b) = body {
                req.push_str(b);
            }

            stream
                .write_all(req.as_bytes())
                .map_err(|e| PyException::os_error(format!("HTTPConnection write: {}", e)))?;

            let mut raw = Vec::new();
            stream
                .read_to_end(&mut raw)
                .map_err(|e| PyException::os_error(format!("HTTPConnection read: {}", e)))?;

            guard.response_data = Some(raw);
            guard.stream = None;
            Ok(PyObject::none())
        }),
    );

    // getresponse() → response object
    let cs = conn_state.clone();
    attrs.insert(
        CompactString::from("getresponse"),
        PyObject::native_closure("getresponse", move |_args| {
            let mut guard = cs
                .lock()
                .map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;
            let raw = guard
                .response_data
                .take()
                .ok_or_else(|| PyException::runtime_error("no response available"))?;

            let raw_str = String::from_utf8_lossy(&raw);
            let header_end = raw_str.find("\r\n\r\n").unwrap_or(raw_str.len());
            let header_section = &raw_str[..header_end];
            let body_start = if header_end + 4 <= raw.len() {
                header_end + 4
            } else {
                raw.len()
            };
            let body = raw[body_start..].to_vec();

            let mut lines = header_section.lines();
            let status_line = lines.next().unwrap_or("HTTP/1.1 200 OK");
            let status_code: u16 = status_line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(200);

            let mut headers = IndexMap::new();
            for line in lines {
                if let Some(idx) = line.find(':') {
                    let key = line[..idx].trim().to_string();
                    let val = line[idx + 1..].trim().to_string();
                    headers.insert(key, val);
                }
            }

            let url_str = format!("http://{}:{}/", guard.host, guard.port);
            Ok(build_response_object(&url_str, status_code, headers, body))
        }),
    );

    // close()
    let cs = conn_state.clone();
    attrs.insert(
        CompactString::from("close"),
        PyObject::native_closure("close", move |_args| {
            let mut guard = cs
                .lock()
                .map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;
            guard.stream = None;
            guard.response_data = None;
            Ok(PyObject::none())
        }),
    );

    attrs.insert(
        CompactString::from("_bind_methods"),
        PyObject::bool_val(true),
    );

    Ok(PyObject::module_with_attrs(CompactString::from("http.client.HTTPConnection"), attrs))
}

struct HttpConnState {
    host: String,
    port: u16,
    stream: Option<TcpStream>,
    response_data: Option<Vec<u8>>,
}
