//! Socket stdlib module.

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ── Global default timeout ──────────────────────────────────────────────

static DEFAULT_TIMEOUT: Mutex<Option<f64>> = Mutex::new(None);

mod functions;
mod object;

use functions::{
    socket_create_connection, socket_getaddrinfo, socket_getfqdn, socket_gethostbyname,
    socket_gethostname,
};
use object::socket_constructor;

// ════════════════════════════════════════════════════════════════════════
// socket module
// ════════════════════════════════════════════════════════════════════════

pub fn create_socket_module() -> PyObjectRef {
    make_module(
        "socket",
        vec![
            // Address families
            ("AF_UNSPEC", PyObject::int(0)),
            ("AF_UNIX", PyObject::int(1)),
            ("AF_LOCAL", PyObject::int(1)),
            ("AF_INET", PyObject::int(2)),
            ("AF_INET6", PyObject::int(10)),
            ("AF_NETLINK", PyObject::int(16)),
            ("AF_PACKET", PyObject::int(17)),
            ("AF_TIPC", PyObject::int(30)),
            ("AF_BLUETOOTH", PyObject::int(31)),
            ("AF_ALG", PyObject::int(38)),
            ("AF_VSOCK", PyObject::int(40)),
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
            ("IPPROTO_IP", PyObject::int(0)),
            ("IPPROTO_TCP", PyObject::int(6)),
            ("IPPROTO_UDP", PyObject::int(17)),
            ("IPPROTO_IPV6", PyObject::int(41)),
            ("IPPROTO_RAW", PyObject::int(255)),
            // IPv6 options
            ("IPV6_V6ONLY", PyObject::int(26)),
            // getaddrinfo flags
            ("AI_PASSIVE", PyObject::int(1)),
            ("AI_CANONNAME", PyObject::int(2)),
            ("AI_NUMERICHOST", PyObject::int(4)),
            ("AI_V4MAPPED", PyObject::int(8)),
            ("AI_ALL", PyObject::int(16)),
            ("AI_ADDRCONFIG", PyObject::int(32)),
            ("AI_NUMERICSERV", PyObject::int(1024)),
            ("AI_MASK", PyObject::int(0x1407)),
            // getnameinfo flags
            ("NI_MAXHOST", PyObject::int(1025)),
            ("NI_MAXSERV", PyObject::int(32)),
            ("NI_NUMERICHOST", PyObject::int(1)),
            ("NI_NUMERICSERV", PyObject::int(2)),
            ("NI_NOFQDN", PyObject::int(4)),
            ("NI_NAMEREQD", PyObject::int(8)),
            ("NI_DGRAM", PyObject::int(16)),
            // EAI error codes
            ("EAI_ADDRFAMILY", PyObject::int(1)),
            ("EAI_AGAIN", PyObject::int(2)),
            ("EAI_BADFLAGS", PyObject::int(3)),
            ("EAI_FAIL", PyObject::int(4)),
            ("EAI_FAMILY", PyObject::int(5)),
            ("EAI_MEMORY", PyObject::int(6)),
            ("EAI_NODATA", PyObject::int(7)),
            ("EAI_NONAME", PyObject::int(8)),
            ("EAI_SERVICE", PyObject::int(9)),
            ("EAI_SOCKTYPE", PyObject::int(10)),
            ("EAI_SYSTEM", PyObject::int(11)),
            ("EAI_OVERFLOW", PyObject::int(14)),
            // MSG flags
            ("MSG_PEEK", PyObject::int(2)),
            ("MSG_WAITALL", PyObject::int(256)),
            ("MSG_DONTWAIT", PyObject::int(64)),
            ("MSG_NOSIGNAL", PyObject::int(16384)),
            // Maximum backlog
            ("SOMAXCONN", PyObject::int(4096)),
            // SO_EXCLUSIVEADDRUSE (Windows-specific, but many packages reference it)
            ("SO_EXCLUSIVEADDRUSE", PyObject::int(-5)),
            // Shutdown constants
            ("SHUT_RD", PyObject::int(0)),
            ("SHUT_WR", PyObject::int(1)),
            ("SHUT_RDWR", PyObject::int(2)),
            // Exception types
            ("error", PyObject::exception_type(ExceptionKind::OSError)),
            (
                "timeout",
                PyObject::exception_type(ExceptionKind::TimeoutError),
            ),
            // Module-level functions
            ("socket", make_builtin(socket_constructor)),
            ("gethostname", make_builtin(socket_gethostname)),
            ("gethostbyname", make_builtin(socket_gethostbyname)),
            ("getaddrinfo", make_builtin(socket_getaddrinfo)),
            ("getfqdn", make_builtin(socket_getfqdn)),
            ("create_connection", make_builtin(socket_create_connection)),
            (
                "socketpair",
                make_builtin(|_args: &[PyObjectRef]| {
                    // Stub: socketpair is Unix-specific and rarely needed in pure Python
                    Err(PyException::os_error(
                        "socketpair not available in Ferrython",
                    ))
                }),
            ),
            (
                "inet_aton",
                make_builtin(|args: &[PyObjectRef]| {
                    let addr = args.first().map(|a| a.py_to_string()).unwrap_or_default();
                    let parts: Vec<&str> = addr.split('.').collect();
                    if parts.len() != 4 {
                        return Err(PyException::os_error("illegal IP address string"));
                    }
                    let mut bytes = Vec::with_capacity(4);
                    for p in parts {
                        let b: u8 = p
                            .parse()
                            .map_err(|_| PyException::os_error("illegal IP address string"))?;
                        bytes.push(b);
                    }
                    Ok(PyObject::bytes(bytes))
                }),
            ),
            (
                "inet_ntoa",
                make_builtin(|args: &[PyObjectRef]| {
                    let data = args
                        .first()
                        .and_then(|a| match &a.payload {
                            PyObjectPayload::Bytes(b) => Some((**b).clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    if data.len() != 4 {
                        return Err(PyException::os_error("packed IP wrong length"));
                    }
                    Ok(PyObject::str_val(CompactString::from(format!(
                        "{}.{}.{}.{}",
                        data[0], data[1], data[2], data[3]
                    ))))
                }),
            ),
            (
                "htons",
                make_builtin(|args: &[PyObjectRef]| {
                    let v = args.first().and_then(|a| a.as_int()).unwrap_or(0) as u16;
                    Ok(PyObject::int(v.to_be() as i64))
                }),
            ),
            (
                "htonl",
                make_builtin(|args: &[PyObjectRef]| {
                    let v = args.first().and_then(|a| a.as_int()).unwrap_or(0) as u32;
                    Ok(PyObject::int(v.to_be() as i64))
                }),
            ),
            (
                "ntohs",
                make_builtin(|args: &[PyObjectRef]| {
                    let v = args.first().and_then(|a| a.as_int()).unwrap_or(0) as u16;
                    Ok(PyObject::int(u16::from_be(v) as i64))
                }),
            ),
            (
                "ntohl",
                make_builtin(|args: &[PyObjectRef]| {
                    let v = args.first().and_then(|a| a.as_int()).unwrap_or(0) as u32;
                    Ok(PyObject::int(u32::from_be(v) as i64))
                }),
            ),
            (
                "getdefaulttimeout",
                make_builtin(|_| {
                    let guard = DEFAULT_TIMEOUT.lock().unwrap();
                    match *guard {
                        Some(t) => Ok(PyObject::float(t)),
                        None => Ok(PyObject::none()),
                    }
                }),
            ),
            (
                "setdefaulttimeout",
                make_builtin(|args| {
                    let val = args.first().ok_or_else(|| {
                        PyException::type_error("setdefaulttimeout requires 1 argument")
                    })?;
                    let mut guard = DEFAULT_TIMEOUT.lock().unwrap();
                    match &val.payload {
                        PyObjectPayload::None => {
                            *guard = None;
                        }
                        PyObjectPayload::Float(f) => {
                            if *f < 0.0 {
                                return Err(PyException::value_error("Timeout value out of range"));
                            }
                            *guard = Some(*f);
                        }
                        _ => {
                            if let Some(i) = val.as_int() {
                                if i < 0 {
                                    return Err(PyException::value_error(
                                        "Timeout value out of range",
                                    ));
                                }
                                *guard = Some(i as f64);
                            } else {
                                return Err(PyException::type_error("a float is required"));
                            }
                        }
                    }
                    Ok(PyObject::none())
                }),
            ),
            ("has_ipv6", PyObject::bool_val(true)),
            (
                "has_dualstack_ipv6",
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::bool_val(false))),
            ),
            (
                "getnameinfo",
                make_builtin(|args: &[PyObjectRef]| {
                    // getnameinfo((host, port), flags) -> (hostname, servname)
                    let sockaddr = args.first().ok_or_else(|| {
                        PyException::type_error("getnameinfo() argument 1 must be a tuple")
                    })?;
                    let items = sockaddr.to_list().unwrap_or_default();
                    let host = items.first().map(|h| h.py_to_string()).unwrap_or_default();
                    let port = items.get(1).and_then(|p| p.as_int()).unwrap_or(0);
                    Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from(host.as_str())),
                        PyObject::str_val(CompactString::from(port.to_string().as_str())),
                    ]))
                }),
            ),
            ("INADDR_ANY", PyObject::int(0)),
            ("INADDR_BROADCAST", PyObject::int(0xFFFFFFFFu32 as i64)),
            ("INADDR_LOOPBACK", PyObject::int(0x7F000001)),
            // AddressFamily and SocketKind IntEnum classes
            ("AddressFamily", {
                let cls = PyObject::class(
                    CompactString::from("AddressFamily"),
                    vec![],
                    IndexMap::new(),
                );
                if let PyObjectPayload::Class(ref cd) = cls.payload {
                    let mut ns = cd.namespace.write();
                    ns.insert(CompactString::from("AF_UNSPEC"), PyObject::int(0));
                    ns.insert(CompactString::from("AF_UNIX"), PyObject::int(1));
                    ns.insert(CompactString::from("AF_INET"), PyObject::int(2));
                    ns.insert(CompactString::from("AF_INET6"), PyObject::int(10));
                }
                cls
            }),
            ("SocketKind", {
                let cls =
                    PyObject::class(CompactString::from("SocketKind"), vec![], IndexMap::new());
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
