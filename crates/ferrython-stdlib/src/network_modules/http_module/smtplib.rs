use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;

// ── smtplib module ──

pub fn create_smtplib_module() -> PyObjectRef {
    make_module(
        "smtplib",
        vec![
            ("SMTP", make_builtin(smtp_connect)),
            (
                "SMTP_SSL",
                make_builtin(|_args: &[PyObjectRef]| {
                    Err(PyException::runtime_error(
                        "SMTP_SSL requires ssl module (not available)",
                    ))
                }),
            ),
            (
                "SMTPException",
                PyObject::class(
                    CompactString::from("SMTPException"),
                    vec![],
                    IndexMap::new(),
                ),
            ),
            (
                "SMTPAuthenticationError",
                PyObject::class(
                    CompactString::from("SMTPAuthenticationError"),
                    vec![],
                    IndexMap::new(),
                ),
            ),
            (
                "SMTPResponseException",
                PyObject::class(
                    CompactString::from("SMTPResponseException"),
                    vec![],
                    IndexMap::new(),
                ),
            ),
            (
                "SMTPServerDisconnected",
                PyObject::class(
                    CompactString::from("SMTPServerDisconnected"),
                    vec![],
                    IndexMap::new(),
                ),
            ),
            ("SMTP_PORT", PyObject::int(25)),
            ("SMTP_SSL_PORT", PyObject::int(465)),
        ],
    )
}

fn smtp_connect(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;

    let host = if !args.is_empty() {
        args[0].py_to_string()
    } else {
        "localhost".to_string()
    };
    let port = if args.len() > 1 {
        args[1].as_int().unwrap_or(25)
    } else {
        25
    };
    let timeout_secs = if args.len() > 2 {
        args[2].to_float().unwrap_or(30.0)
    } else {
        30.0
    };

    let addr = format!("{}:{}", host, port);
    let stream = TcpStream::connect_timeout(
        &addr
            .parse()
            .map_err(|_| PyException::os_error(&format!("invalid address: {}", addr)))?,
        std::time::Duration::from_secs_f64(timeout_secs),
    )
    .map_err(|e| PyException::os_error(&format!("SMTP connect to {} failed: {}", addr, e)))?;

    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .ok();
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(30)))
        .ok();

    // Read greeting
    let stream = std::sync::Arc::new(std::sync::Mutex::new(stream));
    {
        let sock = stream.lock().unwrap();
        let mut reader = BufReader::new(&*sock);
        let mut greeting = String::new();
        reader
            .read_line(&mut greeting)
            .map_err(|e| PyException::os_error(&format!("SMTP read greeting: {}", e)))?;
        if !greeting.starts_with("220") {
            return Err(PyException::runtime_error(&format!(
                "SMTP unexpected greeting: {}",
                greeting.trim()
            )));
        }
    }

    let cls = PyObject::class(CompactString::from("SMTP"), vec![], IndexMap::new());
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("host"),
        PyObject::str_val(CompactString::from(&host)),
    );
    attrs.insert(CompactString::from("port"), PyObject::int(port));

    // ehlo(hostname=None)
    let s = stream.clone();
    let h = host.clone();
    attrs.insert(
        CompactString::from("ehlo"),
        PyObject::native_closure("ehlo", move |args: &[PyObjectRef]| {
            let name = if !args.is_empty() {
                args[0].py_to_string()
            } else {
                h.clone()
            };
            let mut sock = s.lock().unwrap();
            write!(sock, "EHLO {}\r\n", name)
                .map_err(|e| PyException::os_error(&format!("SMTP write: {}", e)))?;
            sock.flush().ok();
            let (code, msg) = smtp_read_response(&*sock)?;
            Ok(PyObject::tuple(vec![
                PyObject::int(code as i64),
                PyObject::str_val(CompactString::from(msg)),
            ]))
        }),
    );

    // login(user, password)
    let s = stream.clone();
    attrs.insert(
        CompactString::from("login"),
        PyObject::native_closure("login", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("login requires user and password"));
            }
            let user = args[0].py_to_string();
            let pass = args[1].py_to_string();
            let mut sock = s.lock().unwrap();
            // AUTH LOGIN
            write!(sock, "AUTH LOGIN\r\n")
                .map_err(|e| PyException::os_error(&format!("SMTP write: {}", e)))?;
            sock.flush().ok();
            let (code, _) = smtp_read_response(&*sock)?;
            if code == 334 {
                write!(sock, "{}\r\n", simple_base64_encode(user.as_bytes())).ok();
                sock.flush().ok();
                smtp_read_response(&*sock)?;
                write!(sock, "{}\r\n", simple_base64_encode(pass.as_bytes())).ok();
                sock.flush().ok();
                let (code2, msg2) = smtp_read_response(&*sock)?;
                if code2 != 235 {
                    return Err(PyException::runtime_error(&format!(
                        "SMTP AUTH failed: {} {}",
                        code2, msg2
                    )));
                }
            }
            Ok(PyObject::tuple(vec![
                PyObject::int(235),
                PyObject::str_val(CompactString::from("Authentication successful")),
            ]))
        }),
    );

    // sendmail(from_addr, to_addrs, msg)
    let s = stream.clone();
    attrs.insert(
        CompactString::from("sendmail"),
        PyObject::native_closure("sendmail", move |args: &[PyObjectRef]| {
            if args.len() < 3 {
                return Err(PyException::type_error(
                    "sendmail requires from_addr, to_addrs, msg",
                ));
            }
            let from_addr = args[0].py_to_string();
            let msg = args[2].py_to_string();
            let mut sock = s.lock().unwrap();

            // MAIL FROM
            write!(sock, "MAIL FROM:<{}>\r\n", from_addr)
                .map_err(|e| PyException::os_error(&format!("SMTP: {}", e)))?;
            sock.flush().ok();
            smtp_read_response(&*sock)?;

            // RCPT TO (handle list or single string)
            let to_list = match &args[1].payload {
                PyObjectPayload::List(items) => items
                    .read()
                    .iter()
                    .map(|i| i.py_to_string())
                    .collect::<Vec<_>>(),
                PyObjectPayload::Tuple(items) => {
                    items.iter().map(|i| i.py_to_string()).collect::<Vec<_>>()
                }
                _ => vec![args[1].py_to_string()],
            };
            for to in &to_list {
                write!(sock, "RCPT TO:<{}>\r\n", to)
                    .map_err(|e| PyException::os_error(&format!("SMTP: {}", e)))?;
                sock.flush().ok();
                smtp_read_response(&*sock)?;
            }

            // DATA
            write!(sock, "DATA\r\n").map_err(|e| PyException::os_error(&format!("SMTP: {}", e)))?;
            sock.flush().ok();
            smtp_read_response(&*sock)?;

            // Send message body + terminator
            write!(sock, "{}\r\n.\r\n", msg)
                .map_err(|e| PyException::os_error(&format!("SMTP: {}", e)))?;
            sock.flush().ok();
            smtp_read_response(&*sock)?;

            Ok(PyObject::dict(IndexMap::new()))
        }),
    );

    // send_message(msg)
    let s = stream.clone();
    attrs.insert(
        CompactString::from("send_message"),
        PyObject::native_closure("send_message", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("send_message requires a Message"));
            }
            let msg_obj = &args[0];
            let from_addr = msg_obj
                .get_attr("__getitem__")
                .and_then(|gi| {
                    if let PyObjectPayload::NativeClosure(nc) = &gi.payload {
                        (nc.func)(&[PyObject::str_val(CompactString::from("From"))]).ok()
                    } else {
                        None
                    }
                })
                .map(|v| v.py_to_string())
                .unwrap_or_default();
            let to_addr = msg_obj
                .get_attr("__getitem__")
                .and_then(|gi| {
                    if let PyObjectPayload::NativeClosure(nc) = &gi.payload {
                        (nc.func)(&[PyObject::str_val(CompactString::from("To"))]).ok()
                    } else {
                        None
                    }
                })
                .map(|v| v.py_to_string())
                .unwrap_or_default();
            let body = if let Some(as_string) = msg_obj.get_attr("as_string") {
                match &as_string.payload {
                    PyObjectPayload::NativeFunction(nf) => {
                        (nf.func)(&[]).map(|v| v.py_to_string()).unwrap_or_default()
                    }
                    PyObjectPayload::NativeClosure(nc) => {
                        (nc.func)(&[]).map(|v| v.py_to_string()).unwrap_or_default()
                    }
                    _ => msg_obj.py_to_string(),
                }
            } else {
                msg_obj.py_to_string()
            };

            let mut sock = s.lock().unwrap();
            write!(sock, "MAIL FROM:<{}>\r\n", from_addr).ok();
            sock.flush().ok();
            smtp_read_response(&*sock)?;
            write!(sock, "RCPT TO:<{}>\r\n", to_addr).ok();
            sock.flush().ok();
            smtp_read_response(&*sock)?;
            write!(sock, "DATA\r\n").ok();
            sock.flush().ok();
            smtp_read_response(&*sock)?;
            write!(sock, "{}\r\n.\r\n", body).ok();
            sock.flush().ok();
            smtp_read_response(&*sock)?;
            Ok(PyObject::dict(IndexMap::new()))
        }),
    );

    // starttls()
    attrs.insert(
        CompactString::from("starttls"),
        make_builtin(|_| {
            Err(PyException::runtime_error(
                "STARTTLS requires ssl module (not available)",
            ))
        }),
    );

    // quit()
    let s = stream.clone();
    attrs.insert(
        CompactString::from("quit"),
        PyObject::native_closure("quit", move |_| {
            let mut sock = s.lock().unwrap();
            write!(sock, "QUIT\r\n").ok();
            sock.flush().ok();
            smtp_read_response(&*sock).ok();
            Ok(PyObject::none())
        }),
    );

    // close() — alias for quit without reading response
    let s = stream.clone();
    attrs.insert(
        CompactString::from("close"),
        PyObject::native_closure("close", move |_| {
            if let Ok(mut sock) = s.lock() {
                write!(sock, "QUIT\r\n").ok();
                sock.flush().ok();
            }
            Ok(PyObject::none())
        }),
    );

    // noop()
    let s = stream.clone();
    attrs.insert(
        CompactString::from("noop"),
        PyObject::native_closure("noop", move |_| {
            let mut sock = s.lock().unwrap();
            write!(sock, "NOOP\r\n").map_err(|e| PyException::os_error(&format!("SMTP: {}", e)))?;
            sock.flush().ok();
            let (code, msg) = smtp_read_response(&*sock)?;
            Ok(PyObject::tuple(vec![
                PyObject::int(code as i64),
                PyObject::str_val(CompactString::from(msg)),
            ]))
        }),
    );

    Ok(PyObject::instance_with_attrs(cls, attrs))
}

fn smtp_read_response(stream: &std::net::TcpStream) -> PyResult<(u16, String)> {
    use std::io::{BufRead, BufReader};
    let mut reader = BufReader::new(stream);
    let mut full_msg = String::new();
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| PyException::os_error(&format!("SMTP read: {}", e)))?;
        full_msg.push_str(&line);
        if line.len() >= 4 && line.as_bytes()[3] == b' ' {
            break;
        }
        if line.is_empty() {
            break;
        }
    }
    let code = full_msg
        .get(..3)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let msg = full_msg.get(4..).unwrap_or("").trim().to_string();
    Ok((code, msg))
}

fn simple_base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((n >> 18) & 63) as usize] as char);
        result.push(CHARS[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((n >> 6) & 63) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(n & 63) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}
