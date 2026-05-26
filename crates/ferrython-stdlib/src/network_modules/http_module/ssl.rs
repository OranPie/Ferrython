use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

// ── ssl module ──

/// Helper: invoke a callable PyObject (NativeFunction, NativeClosure, or BoundMethod).
fn ssl_call_fn(func: &PyObjectRef, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match &func.payload {
        PyObjectPayload::NativeFunction(nf) => (nf.func)(args),
        PyObjectPayload::NativeClosure(nc) => (nc.func)(args),
        PyObjectPayload::BoundMethod { receiver, method } => {
            let mut full_args = vec![receiver.clone()];
            full_args.extend_from_slice(args);
            match &method.payload {
                PyObjectPayload::NativeFunction(nf) => (nf.func)(&full_args),
                PyObjectPayload::NativeClosure(nc) => (nc.func)(&full_args),
                _ => Ok(PyObject::none()),
            }
        }
        _ => Ok(PyObject::none()),
    }
}

/// Build the SSLSocket class — wraps an underlying socket and delegates I/O.
fn make_ssl_socket_class() -> PyObjectRef {
    let mut ns = IndexMap::new();

    // __init__(self, sock=None, server_hostname=None)
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("SSLSocket.__init__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            let self_obj = &args[0];
            if let PyObjectPayload::Instance(ref d) = self_obj.payload {
                let mut w = d.attrs.write();
                let sock = args.get(1).cloned().unwrap_or_else(PyObject::none);
                let hostname = args.get(2).cloned().unwrap_or_else(PyObject::none);
                w.insert(CompactString::from("_socket"), sock);
                w.insert(CompactString::from("server_hostname"), hostname);
                w.insert(CompactString::from("_closed"), PyObject::bool_val(false));
            }
            Ok(PyObject::none())
        }),
    );

    // read(self, nbytes=4096) → bytes  (delegates to underlying socket recv)
    ns.insert(
        CompactString::from("read"),
        PyObject::native_closure("SSLSocket.read", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::bytes(vec![]));
            }
            let self_obj = &args[0];
            let nbytes = if args.len() > 1 {
                args[1].as_int().unwrap_or(4096)
            } else {
                4096
            };
            if let Some(sock) = self_obj.get_attr("_socket") {
                if let Some(recv_fn) = sock.get_attr("recv") {
                    return ssl_call_fn(&recv_fn, &[PyObject::int(nbytes)]);
                }
            }
            Ok(PyObject::bytes(vec![]))
        }),
    );

    // write(self, data) → int  (delegates to underlying socket send)
    ns.insert(
        CompactString::from("write"),
        PyObject::native_closure("SSLSocket.write", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::int(0));
            }
            let self_obj = &args[0];
            if let Some(sock) = self_obj.get_attr("_socket") {
                if let Some(send_fn) = sock.get_attr("send") {
                    return ssl_call_fn(&send_fn, &[args[1].clone()]);
                }
            }
            Ok(PyObject::int(0))
        }),
    );

    // recv(self, bufsize) → bytes
    ns.insert(
        CompactString::from("recv"),
        PyObject::native_closure("SSLSocket.recv", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::bytes(vec![]));
            }
            let self_obj = &args[0];
            let bufsize = if args.len() > 1 {
                args[1].as_int().unwrap_or(4096)
            } else {
                4096
            };
            if let Some(sock) = self_obj.get_attr("_socket") {
                if let Some(recv_fn) = sock.get_attr("recv") {
                    return ssl_call_fn(&recv_fn, &[PyObject::int(bufsize)]);
                }
            }
            Ok(PyObject::bytes(vec![]))
        }),
    );

    // send(self, data) → int
    ns.insert(
        CompactString::from("send"),
        PyObject::native_closure("SSLSocket.send", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::int(0));
            }
            let self_obj = &args[0];
            if let Some(sock) = self_obj.get_attr("_socket") {
                if let Some(send_fn) = sock.get_attr("send") {
                    return ssl_call_fn(&send_fn, &[args[1].clone()]);
                }
            }
            Ok(PyObject::int(0))
        }),
    );

    // sendall(self, data)
    ns.insert(
        CompactString::from("sendall"),
        PyObject::native_closure("SSLSocket.sendall", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::none());
            }
            let self_obj = &args[0];
            if let Some(sock) = self_obj.get_attr("_socket") {
                if let Some(send_fn) = sock.get_attr("sendall") {
                    return ssl_call_fn(&send_fn, &[args[1].clone()]);
                } else if let Some(send_fn) = sock.get_attr("send") {
                    let _ = ssl_call_fn(&send_fn, &[args[1].clone()]);
                }
            }
            Ok(PyObject::none())
        }),
    );

    // close(self)
    ns.insert(
        CompactString::from("close"),
        PyObject::native_closure("SSLSocket.close", |args: &[PyObjectRef]| {
            if !args.is_empty() {
                let self_obj = &args[0];
                if let PyObjectPayload::Instance(ref d) = self_obj.payload {
                    let mut w = d.attrs.write();
                    w.insert(CompactString::from("_closed"), PyObject::bool_val(true));
                }
                if let Some(sock) = self_obj.get_attr("_socket") {
                    if let Some(close_fn) = sock.get_attr("close") {
                        let _ = ssl_call_fn(&close_fn, &[]);
                    }
                }
            }
            Ok(PyObject::none())
        }),
    );

    // getpeername(self)
    ns.insert(
        CompactString::from("getpeername"),
        PyObject::native_closure("SSLSocket.getpeername", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            if let Some(sock) = args[0].get_attr("_socket") {
                if let Some(f) = sock.get_attr("getpeername") {
                    return ssl_call_fn(&f, &[]);
                }
            }
            Ok(PyObject::none())
        }),
    );

    // settimeout(self, timeout)
    ns.insert(
        CompactString::from("settimeout"),
        PyObject::native_closure("SSLSocket.settimeout", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::none());
            }
            if let Some(sock) = args[0].get_attr("_socket") {
                if let Some(f) = sock.get_attr("settimeout") {
                    return ssl_call_fn(&f, &[args[1].clone()]);
                }
            }
            Ok(PyObject::none())
        }),
    );

    // fileno(self)
    ns.insert(
        CompactString::from("fileno"),
        PyObject::native_closure("SSLSocket.fileno", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::int(-1));
            }
            if let Some(sock) = args[0].get_attr("_socket") {
                if let Some(f) = sock.get_attr("fileno") {
                    return ssl_call_fn(&f, &[]);
                }
            }
            Ok(PyObject::int(-1))
        }),
    );

    // __enter__(self) → self
    ns.insert(
        CompactString::from("__enter__"),
        PyObject::native_closure("SSLSocket.__enter__", |args: &[PyObjectRef]| {
            if !args.is_empty() {
                Ok(args[0].clone())
            } else {
                Ok(PyObject::none())
            }
        }),
    );

    // __exit__(self, *args)
    ns.insert(
        CompactString::from("__exit__"),
        PyObject::native_closure("SSLSocket.__exit__", |args: &[PyObjectRef]| {
            if !args.is_empty() {
                if let Some(sock) = args[0].get_attr("_socket") {
                    if let Some(close_fn) = sock.get_attr("close") {
                        let _ = ssl_call_fn(&close_fn, &[]);
                    }
                }
            }
            Ok(PyObject::bool_val(false))
        }),
    );

    PyObject::class(CompactString::from("SSLSocket"), vec![], ns)
}

/// Helper: create an SSLSocket instance wrapping the given socket object.
fn build_ssl_socket_instance(sock: PyObjectRef, server_hostname: Option<String>) -> PyObjectRef {
    let cls = make_ssl_socket_class();
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("_socket"), sock);
    attrs.insert(
        CompactString::from("server_hostname"),
        server_hostname
            .map(|h| PyObject::str_val(CompactString::from(h)))
            .unwrap_or_else(PyObject::none),
    );
    attrs.insert(CompactString::from("_closed"), PyObject::bool_val(false));
    PyObject::instance_with_attrs(cls, attrs)
}

/// Helper: create an SSLContext instance with given protocol.
fn build_ssl_context_instance(protocol: i64) -> PyObjectRef {
    let mut ctx_ns = IndexMap::new();

    // __init__
    ctx_ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("SSLContext.__init__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            let proto = if args.len() > 1 {
                args[1].as_int().unwrap_or(2)
            } else {
                2
            };
            if let PyObjectPayload::Instance(ref d) = args[0].payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("protocol"), PyObject::int(proto));
                w.insert(
                    CompactString::from("check_hostname"),
                    PyObject::bool_val(true),
                );
                w.insert(CompactString::from("verify_mode"), PyObject::int(2));
            }
            Ok(PyObject::none())
        }),
    );

    // wrap_socket(self, sock, server_side=False, do_handshake_on_connect=True,
    //             suppress_ragged_eofs=True, server_hostname=None)
    ctx_ns.insert(
        CompactString::from("wrap_socket"),
        PyObject::native_closure("SSLContext.wrap_socket", |args: &[PyObjectRef]| {
            // args[0]=self, args[1]=sock, remaining are optional kwargs
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "wrap_socket() requires a socket argument",
                ));
            }
            let sock = args[1].clone();
            // Extract server_hostname from positional or keyword args
            let hostname = if args.len() > 5 && !matches!(&args[5].payload, PyObjectPayload::None) {
                Some(args[5].py_to_string())
            } else {
                // Check trailing dict for server_hostname kwarg
                args.last().and_then(|last| {
                    if let PyObjectPayload::Dict(d) = &last.payload {
                        let map = d.read();
                        map.get(&HashableKey::str_key(CompactString::from(
                            "server_hostname",
                        )))
                        .map(|v| v.py_to_string())
                    } else {
                        None
                    }
                })
            };
            Ok(build_ssl_socket_instance(sock, hostname))
        }),
    );

    // load_cert_chain(self, certfile, keyfile=None, password=None)
    ctx_ns.insert(
        CompactString::from("load_cert_chain"),
        PyObject::native_closure("SSLContext.load_cert_chain", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );

    // load_verify_locations(self, cafile=None, capath=None, cadata=None)
    ctx_ns.insert(
        CompactString::from("load_verify_locations"),
        PyObject::native_closure(
            "SSLContext.load_verify_locations",
            |_args: &[PyObjectRef]| Ok(PyObject::none()),
        ),
    );

    // set_ciphers(self, ciphers)
    ctx_ns.insert(
        CompactString::from("set_ciphers"),
        PyObject::native_closure("SSLContext.set_ciphers", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );

    // set_default_verify_paths(self)
    ctx_ns.insert(
        CompactString::from("set_default_verify_paths"),
        PyObject::native_closure(
            "SSLContext.set_default_verify_paths",
            |_args: &[PyObjectRef]| Ok(PyObject::none()),
        ),
    );

    // load_default_certs(self, purpose=None)
    ctx_ns.insert(
        CompactString::from("load_default_certs"),
        PyObject::native_closure(
            "SSLContext.load_default_certs",
            |_args: &[PyObjectRef]| Ok(PyObject::none()),
        ),
    );

    let ctx_cls = PyObject::class(CompactString::from("SSLContext"), vec![], ctx_ns);
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("protocol"), PyObject::int(protocol));
    attrs.insert(
        CompactString::from("check_hostname"),
        PyObject::bool_val(true),
    );
    attrs.insert(CompactString::from("verify_mode"), PyObject::int(2));
    PyObject::instance_with_attrs(ctx_cls, attrs)
}

pub fn create_ssl_module() -> PyObjectRef {
    let ssl_context_fn = make_builtin(|args: &[PyObjectRef]| {
        let protocol = if !args.is_empty() {
            args[0].to_int().unwrap_or(2)
        } else {
            2
        };
        Ok(build_ssl_context_instance(protocol))
    });

    let create_default_context_fn =
        make_builtin(|_args: &[PyObjectRef]| Ok(build_ssl_context_instance(2)));

    // wrap_socket module-level convenience function
    let wrap_socket_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "wrap_socket() requires a socket argument",
            ));
        }
        let sock = args[0].clone();
        let hostname = if args.len() > 1 {
            // Check trailing dict for server_hostname kwarg
            args.last().and_then(|last| {
                if let PyObjectPayload::Dict(d) = &last.payload {
                    let map = d.read();
                    map.get(&HashableKey::str_key(CompactString::from(
                        "server_hostname",
                    )))
                    .map(|v| v.py_to_string())
                } else {
                    None
                }
            })
        } else {
            None
        };
        Ok(build_ssl_socket_instance(sock, hostname))
    });

    let ssl_socket_cls = make_ssl_socket_class();

    make_module(
        "ssl",
        vec![
            ("SSLContext", ssl_context_fn),
            ("create_default_context", create_default_context_fn),
            ("wrap_socket", wrap_socket_fn),
            ("SSLError", PyObject::exception_type(ExceptionKind::OSError)),
            (
                "SSLCertVerificationError",
                PyObject::exception_type(ExceptionKind::OSError),
            ),
            (
                "SSLEOFError",
                PyObject::exception_type(ExceptionKind::OSError),
            ),
            (
                "SSLZeroReturnError",
                PyObject::exception_type(ExceptionKind::OSError),
            ),
            (
                "SSLWantReadError",
                PyObject::exception_type(ExceptionKind::OSError),
            ),
            (
                "SSLWantWriteError",
                PyObject::exception_type(ExceptionKind::OSError),
            ),
            (
                "SSLSyscallError",
                PyObject::exception_type(ExceptionKind::OSError),
            ),
            (
                "CertificateError",
                PyObject::exception_type(ExceptionKind::ValueError),
            ),
            ("PROTOCOL_TLS", PyObject::int(2)),
            ("PROTOCOL_TLS_CLIENT", PyObject::int(16)),
            ("PROTOCOL_TLS_SERVER", PyObject::int(17)),
            ("PROTOCOL_SSLv23", PyObject::int(2)),
            ("CERT_NONE", PyObject::int(0)),
            ("CERT_OPTIONAL", PyObject::int(1)),
            ("CERT_REQUIRED", PyObject::int(2)),
            ("OP_NO_SSLv2", PyObject::int(0x01000000)),
            ("OP_NO_SSLv3", PyObject::int(0x02000000)),
            ("OP_NO_TLSv1", PyObject::int(0x04000000)),
            ("OP_NO_TLSv1_1", PyObject::int(0x10000000)),
            ("OP_NO_TLSv1_2", PyObject::int(0x08000000)),
            ("OP_NO_TLSv1_3", PyObject::int(0x20000000)),
            ("OP_NO_COMPRESSION", PyObject::int(0x00020000)),
            ("OP_NO_TICKET", PyObject::int(0x00004000)),
            ("OP_ALL", PyObject::int(0x80000BFF_u64 as i64)),
            ("HAS_SNI", PyObject::bool_val(true)),
            ("HAS_ECDH", PyObject::bool_val(true)),
            ("HAS_NPN", PyObject::bool_val(false)),
            ("HAS_ALPN", PyObject::bool_val(true)),
            ("HAS_NEVER_CHECK_COMMON_NAME", PyObject::bool_val(false)),
            ("HAS_SSLv2", PyObject::bool_val(false)),
            ("HAS_SSLv3", PyObject::bool_val(false)),
            ("HAS_TLSv1", PyObject::bool_val(true)),
            ("HAS_TLSv1_1", PyObject::bool_val(true)),
            ("HAS_TLSv1_2", PyObject::bool_val(true)),
            ("HAS_TLSv1_3", PyObject::bool_val(true)),
            (
                "OPENSSL_VERSION",
                PyObject::str_val(CompactString::from("OpenSSL 3.0.0 (stub)")),
            ),
            ("OPENSSL_VERSION_NUMBER", PyObject::int(0x30000000)),
            ("OPENSSL_VERSION_INFO", {
                let info = vec![
                    PyObject::int(3),
                    PyObject::int(0),
                    PyObject::int(0),
                    PyObject::int(0),
                    PyObject::int(0),
                ];
                PyObject::tuple(info)
            }),
            // Verify flags
            ("VERIFY_DEFAULT", PyObject::int(0)),
            ("VERIFY_CRL_CHECK_LEAF", PyObject::int(0x4)),
            ("VERIFY_CRL_CHECK_CHAIN", PyObject::int(0x0C)),
            ("VERIFY_X509_STRICT", PyObject::int(0x20)),
            ("VERIFY_X509_PARTIAL_CHAIN", PyObject::int(0x80000)),
            ("VERIFY_X509_TRUSTED_FIRST", PyObject::int(0x8000)),
            // Purpose
            ("Purpose", {
                let mut attrs = IndexMap::new();
                attrs.insert(
                    CompactString::from("SERVER_AUTH"),
                    PyObject::str_val(CompactString::from("1.3.6.1.5.5.7.3.1")),
                );
                attrs.insert(
                    CompactString::from("CLIENT_AUTH"),
                    PyObject::str_val(CompactString::from("1.3.6.1.5.5.7.3.2")),
                );
                PyObject::module_with_attrs(CompactString::from("Purpose"), attrs)
            }),
            // TLSVersion enum
            ("TLSVersion", {
                let mut attrs = IndexMap::new();
                attrs.insert(
                    CompactString::from("MINIMUM_SUPPORTED"),
                    PyObject::int(0x0300),
                );
                attrs.insert(CompactString::from("SSLv3"), PyObject::int(0x0300));
                attrs.insert(CompactString::from("TLSv1"), PyObject::int(0x0301));
                attrs.insert(CompactString::from("TLSv1_1"), PyObject::int(0x0302));
                attrs.insert(CompactString::from("TLSv1_2"), PyObject::int(0x0303));
                attrs.insert(CompactString::from("TLSv1_3"), PyObject::int(0x0304));
                attrs.insert(
                    CompactString::from("MAXIMUM_SUPPORTED"),
                    PyObject::int(0x0304),
                );
                PyObject::module_with_attrs(CompactString::from("TLSVersion"), attrs)
            }),
            // VerifyMode enum
            (
                "VerifyMode",
                PyObject::class(CompactString::from("VerifyMode"), vec![], IndexMap::new()),
            ),
            // VerifyFlags enum
            (
                "VerifyFlags",
                PyObject::class(CompactString::from("VerifyFlags"), vec![], IndexMap::new()),
            ),
            // SSLSocket class
            ("SSLSocket", ssl_socket_cls),
            // SSLObject
            (
                "SSLObject",
                PyObject::class(CompactString::from("SSLObject"), vec![], IndexMap::new()),
            ),
            // AlertDescription
            (
                "AlertDescription",
                PyObject::class(
                    CompactString::from("AlertDescription"),
                    vec![],
                    IndexMap::new(),
                ),
            ),
            // match_hostname (deprecated, removed in 3.12+)
            (
                "match_hostname",
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
            ),
        ],
    )
}
