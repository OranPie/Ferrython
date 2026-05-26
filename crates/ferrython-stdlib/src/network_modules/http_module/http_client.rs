use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use super::urllib_request::{build_response_object, make_http_response_class};

pub(super) fn make_http_connection_class(
    default_port: u16,
    class_name: &str,
    is_https: bool,
) -> PyObjectRef {
    let mut ns = IndexMap::new();
    let https_flag = is_https;
    let def_port = default_port;

    ns.insert(CompactString::from("__init__"), {
        let class_name_str = class_name.to_string();
        PyObject::native_closure(
            &format!("{}.__init__", class_name_str),
            move |args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Err(PyException::type_error(&format!(
                        "{}() requires a host argument",
                        class_name_str
                    )));
                }
                let self_obj = &args[0];
                let host = args[1].py_to_string();
                let port: u16 =
                    if args.len() > 2 && !matches!(&args[2].payload, PyObjectPayload::None) {
                        args[2].as_int().unwrap_or(def_port as i64) as u16
                    } else if let Some(idx) = host.rfind(':') {
                        host[idx + 1..].parse().unwrap_or(def_port)
                    } else {
                        def_port
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
                let timeout_secs: i64 =
                    if args.len() > 3 && !matches!(&args[3].payload, PyObjectPayload::None) {
                        args[3].as_int().unwrap_or(30)
                    } else {
                        30
                    };

                if let PyObjectPayload::Instance(ref d) = self_obj.payload {
                    let mut w = d.attrs.write();
                    w.insert(
                        CompactString::from("host"),
                        PyObject::str_val(CompactString::from(host_only.as_str())),
                    );
                    w.insert(CompactString::from("port"), PyObject::int(port as i64));
                    w.insert(CompactString::from("timeout"), PyObject::int(timeout_secs));
                    w.insert(CompactString::from("debuglevel"), PyObject::int(0));
                    w.insert(
                        CompactString::from("_https"),
                        PyObject::bool_val(https_flag),
                    );
                    w.insert(CompactString::from("_response_data"), PyObject::none());
                }
                Ok(PyObject::none())
            },
        )
    });

    ns.insert(
        CompactString::from("request"),
        PyObject::native_closure("HTTPConnection.request", |args: &[PyObjectRef]| {
            if args.len() < 3 {
                return Err(PyException::type_error(
                    "request() requires method and url arguments",
                ));
            }
            let self_obj = &args[0];
            let method = args[1].py_to_string();
            let url = args[2].py_to_string();
            let body = if args.len() > 3 && !matches!(&args[3].payload, PyObjectPayload::None) {
                Some(args[3].py_to_string())
            } else {
                None
            };

            let mut extra_headers = IndexMap::new();
            if args.len() > 4 {
                if let PyObjectPayload::Dict(d) = &args[4].payload {
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

            let host = self_obj
                .get_attr("host")
                .map(|h| h.py_to_string())
                .unwrap_or_default();
            let port = self_obj
                .get_attr("port")
                .and_then(|p| p.as_int())
                .unwrap_or(80) as u16;
            let timeout_secs = self_obj
                .get_attr("timeout")
                .and_then(|t| t.as_int())
                .unwrap_or(30) as u64;

            let addr = format!("{}:{}", host, port);
            let timeout = Duration::from_secs(timeout_secs);
            let socket_addr: std::net::SocketAddr = addr.parse().or_else(|_| {
                use std::net::ToSocketAddrs;
                addr.to_socket_addrs()
                    .map_err(|e| PyException::os_error(format!("HTTPConnection DNS: {}", e)))
                    .and_then(|mut addrs| {
                        addrs.next().ok_or_else(|| {
                            PyException::os_error(format!(
                                "HTTPConnection: could not resolve {}",
                                addr
                            ))
                        })
                    })
            })?;
            let mut stream = TcpStream::connect_timeout(&socket_addr, timeout)
                .map_err(|e| PyException::os_error(format!("HTTPConnection: {}", e)))?;
            stream.set_read_timeout(Some(timeout)).ok();
            stream.set_write_timeout(Some(timeout)).ok();

            let mut req = format!(
                "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
                method, url, host
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

            if let PyObjectPayload::Instance(ref d) = self_obj.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("_response_data"), PyObject::bytes(raw));
            }
            Ok(PyObject::none())
        }),
    );

    ns.insert(
        CompactString::from("getresponse"),
        PyObject::native_closure("HTTPConnection.getresponse", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::runtime_error("no response available"));
            }
            let self_obj = &args[0];
            let raw_obj = self_obj
                .get_attr("_response_data")
                .ok_or_else(|| PyException::runtime_error("no response available"))?;
            let raw = match &raw_obj.payload {
                PyObjectPayload::Bytes(b) => (**b).clone(),
                PyObjectPayload::None => {
                    return Err(PyException::runtime_error("no response available"))
                }
                _ => vec![],
            };

            if let PyObjectPayload::Instance(ref d) = self_obj.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("_response_data"), PyObject::none());
            }

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

            let host = self_obj
                .get_attr("host")
                .map(|h| h.py_to_string())
                .unwrap_or_default();
            let port = self_obj
                .get_attr("port")
                .and_then(|p| p.as_int())
                .unwrap_or(80);
            let url_str = format!("http://{}:{}/", host, port);
            Ok(build_response_object(&url_str, status_code, headers, body))
        }),
    );

    ns.insert(
        CompactString::from("connect"),
        PyObject::native_closure("HTTPConnection.connect", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );

    ns.insert(
        CompactString::from("close"),
        PyObject::native_closure("HTTPConnection.close", |args: &[PyObjectRef]| {
            if !args.is_empty() {
                if let PyObjectPayload::Instance(ref d) = args[0].payload {
                    let mut w = d.attrs.write();
                    w.insert(CompactString::from("_response_data"), PyObject::none());
                }
            }
            Ok(PyObject::none())
        }),
    );

    ns.insert(
        CompactString::from("set_debuglevel"),
        PyObject::native_closure("HTTPConnection.set_debuglevel", |args: &[PyObjectRef]| {
            if args.len() >= 2 {
                if let PyObjectPayload::Instance(ref d) = args[0].payload {
                    let mut w = d.attrs.write();
                    w.insert(CompactString::from("debuglevel"), args[1].clone());
                }
            }
            Ok(PyObject::none())
        }),
    );

    ns.insert(
        CompactString::from("set_tunnel"),
        PyObject::native_closure("HTTPConnection.set_tunnel", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );

    ns.insert(
        CompactString::from("putheader"),
        PyObject::native_closure("HTTPConnection.putheader", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );

    ns.insert(
        CompactString::from("putrequest"),
        PyObject::native_closure("HTTPConnection.putrequest", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );

    ns.insert(
        CompactString::from("endheaders"),
        PyObject::native_closure("HTTPConnection.endheaders", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );

    ns.insert(
        CompactString::from("__enter__"),
        PyObject::native_closure("HTTPConnection.__enter__", |args: &[PyObjectRef]| {
            if !args.is_empty() {
                Ok(args[0].clone())
            } else {
                Ok(PyObject::none())
            }
        }),
    );

    ns.insert(
        CompactString::from("__exit__"),
        PyObject::native_closure("HTTPConnection.__exit__", |args: &[PyObjectRef]| {
            if !args.is_empty() {
                if let PyObjectPayload::Instance(ref d) = args[0].payload {
                    let mut w = d.attrs.write();
                    w.insert(CompactString::from("_response_data"), PyObject::none());
                }
            }
            Ok(PyObject::bool_val(false))
        }),
    );

    PyObject::class(CompactString::from(class_name), vec![], ns)
}

pub fn create_http_client_module() -> PyObjectRef {
    let http_connection_cls = make_http_connection_class(80, "HTTPConnection", false);
    let https_connection_cls = make_http_connection_class(443, "HTTPSConnection", true);
    let http_response_cls = make_http_response_class();

    let mut client_attrs = IndexMap::new();
    client_attrs.insert(CompactString::from("HTTPConnection"), http_connection_cls);
    client_attrs.insert(CompactString::from("HTTPSConnection"), https_connection_cls);
    client_attrs.insert(CompactString::from("OK"), PyObject::int(200));
    client_attrs.insert(CompactString::from("NOT_FOUND"), PyObject::int(404));
    client_attrs.insert(
        CompactString::from("INTERNAL_SERVER_ERROR"),
        PyObject::int(500),
    );
    client_attrs.insert(CompactString::from("HTTPResponse"), http_response_cls);
    client_attrs.insert(
        CompactString::from("HTTPException"),
        PyObject::builtin_type(CompactString::from("HTTPException")),
    );
    client_attrs.insert(
        CompactString::from("RemoteDisconnected"),
        PyObject::builtin_type(CompactString::from("RemoteDisconnected")),
    );
    client_attrs.insert(
        CompactString::from("IncompleteRead"),
        PyObject::builtin_type(CompactString::from("IncompleteRead")),
    );
    client_attrs.insert(
        CompactString::from("ResponseNotReady"),
        PyObject::builtin_type(CompactString::from("ResponseNotReady")),
    );
    client_attrs.insert(
        CompactString::from("BadStatusLine"),
        PyObject::builtin_type(CompactString::from("BadStatusLine")),
    );
    client_attrs.insert(
        CompactString::from("CannotSendRequest"),
        PyObject::builtin_type(CompactString::from("CannotSendRequest")),
    );
    client_attrs.insert(
        CompactString::from("HTTPMessage"),
        PyObject::class(CompactString::from("HTTPMessage"), vec![], IndexMap::new()),
    );
    PyObject::module_with_attrs(CompactString::from("http.client"), client_attrs)
}
