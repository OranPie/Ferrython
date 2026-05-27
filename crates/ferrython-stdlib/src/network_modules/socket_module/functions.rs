use super::object::{build_socket_object, extract_host_port, SocketInner};
use super::*;

// ── Module-level socket functions ──────────────────────────────────────

pub(super) fn socket_gethostname(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Try the libc gethostname approach via /etc/hostname, then env
    let hostname = std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "localhost".to_string());
    Ok(PyObject::str_val(CompactString::from(hostname)))
}

pub(super) fn socket_getfqdn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn socket_gethostbyname(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn socket_getaddrinfo(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn socket_create_connection(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "create_connection() requires an address argument",
        ));
    }
    let (host, port) = extract_host_port(&args[0])?;
    let timeout = if args.len() > 1 && !matches!(&args[1].payload, PyObjectPayload::None) {
        Some(Duration::from_secs_f64(args[1].to_float().unwrap_or(30.0)))
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
