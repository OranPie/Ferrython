use super::protocol::{guess_content_type, http_status_reason, HttpRequest};
use super::*;

pub(super) fn build_handler_instance(
    req: &HttpRequest,
    handler_cls: &PyObjectRef,
) -> (PyObjectRef, Arc<Mutex<Vec<u8>>>) {
    let inst = if let PyObjectPayload::Class(_) = &handler_cls.payload {
        PyObject::instance(handler_cls.clone())
    } else {
        let cls = PyObject::class(
            CompactString::from("BaseHTTPRequestHandler"),
            vec![],
            IndexMap::new(),
        );
        PyObject::instance(cls)
    };

    let wfile_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));

    if let PyObjectPayload::Instance(ref data) = inst.payload {
        let mut w = data.attrs.write();
        w.insert(
            CompactString::from("command"),
            PyObject::str_val(CompactString::from(req.method.as_str())),
        );
        w.insert(
            CompactString::from("path"),
            PyObject::str_val(CompactString::from(req.path.as_str())),
        );
        w.insert(
            CompactString::from("request_version"),
            PyObject::str_val(CompactString::from(req.version.as_str())),
        );

        let mut hdr_map = IndexMap::new();
        for (k, v) in &req.headers {
            hdr_map.insert(
                HashableKey::str_key(CompactString::from(k.as_str())),
                PyObject::str_val(CompactString::from(v.as_str())),
            );
        }
        w.insert(CompactString::from("headers"), PyObject::dict(hdr_map));

        let body_data = Arc::new(req.body.clone());
        let body_pos = Arc::new(Mutex::new(0usize));
        let bd = body_data.clone();
        let bp = body_pos.clone();
        w.insert(CompactString::from("rfile"), {
            let mut rfile_attrs = IndexMap::new();
            let bd2 = bd.clone();
            let bp2 = bp.clone();
            rfile_attrs.insert(
                CompactString::from("read"),
                PyObject::native_closure("rfile.read", move |args| {
                    let n = if !args.is_empty() {
                        args[0].as_int().unwrap_or(-1)
                    } else {
                        -1
                    };
                    let mut p = bp2.lock().unwrap();
                    let remaining = &bd2[*p..];
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
            rfile_attrs.insert(
                CompactString::from("_bind_methods"),
                PyObject::bool_val(true),
            );
            PyObject::module_with_attrs(CompactString::from("rfile"), rfile_attrs)
        });

        let wbuf = wfile_buf.clone();
        let mut wfile_attrs = IndexMap::new();
        let wbuf2 = wbuf.clone();
        wfile_attrs.insert(
            CompactString::from("write"),
            PyObject::native_closure("wfile.write", move |args| {
                if !args.is_empty() {
                    let data = match &args[0].payload {
                        PyObjectPayload::Bytes(b) => (**b).clone(),
                        PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                        _ => args[0].py_to_string().into_bytes(),
                    };
                    wbuf2.lock().unwrap().extend_from_slice(&data);
                    Ok(PyObject::int(data.len() as i64))
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );
        let wbuf3 = wbuf.clone();
        wfile_attrs.insert(
            CompactString::from("flush"),
            PyObject::native_closure("wfile.flush", move |_args| {
                let _ = &wbuf3;
                Ok(PyObject::none())
            }),
        );
        wfile_attrs.insert(
            CompactString::from("_bind_methods"),
            PyObject::bool_val(true),
        );
        w.insert(
            CompactString::from("wfile"),
            PyObject::module_with_attrs(CompactString::from("wfile"), wfile_attrs),
        );

        let resp_headers: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let resp_status: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

        let rs = resp_status.clone();
        let wb = wfile_buf.clone();
        w.insert(
            CompactString::from("send_response"),
            PyObject::native_closure("send_response", move |args| {
                let code = if !args.is_empty() {
                    args[0].as_int().unwrap_or(200) as u16
                } else {
                    200
                };
                let message = if args.len() > 1 {
                    args[1].py_to_string()
                } else {
                    http_status_reason(code).to_string()
                };
                let line = format!("HTTP/1.1 {} {}\r\n", code, message);
                *rs.lock().unwrap() = Some(line.clone());
                wb.lock().unwrap().extend_from_slice(line.as_bytes());
                Ok(PyObject::none())
            }),
        );

        let rh = resp_headers.clone();
        let wb = wfile_buf.clone();
        w.insert(
            CompactString::from("send_header"),
            PyObject::native_closure("send_header", move |args| {
                if args.len() >= 2 {
                    let key = args[0].py_to_string();
                    let val = args[1].py_to_string();
                    let line = format!("{}: {}\r\n", key, val);
                    rh.lock().unwrap().push(line.clone());
                    wb.lock().unwrap().extend_from_slice(line.as_bytes());
                }
                Ok(PyObject::none())
            }),
        );

        let wb = wfile_buf.clone();
        w.insert(
            CompactString::from("end_headers"),
            PyObject::native_closure("end_headers", move |_args| {
                wb.lock().unwrap().extend_from_slice(b"\r\n");
                Ok(PyObject::none())
            }),
        );

        let wb = wfile_buf.clone();
        w.insert(
            CompactString::from("send_error"),
            PyObject::native_closure("send_error", move |args| {
                let code = if !args.is_empty() {
                    args[0].as_int().unwrap_or(500) as u16
                } else {
                    500
                };
                let message = if args.len() > 1 {
                    args[1].py_to_string()
                } else {
                    http_status_reason(code).to_string()
                };
                let reason = http_status_reason(code);
                let body = format!(
                    "<html><head><title>Error {code}</title></head>\
                     <body><h1>Error {code}: {reason}</h1><p>{message}</p></body></html>"
                );
                let resp = format!(
                    "HTTP/1.1 {code} {reason}\r\n\
                     Content-Type: text/html; charset=utf-8\r\n\
                     Content-Length: {}\r\n\
                     Connection: close\r\n\r\n{body}",
                    body.len()
                );
                wb.lock().unwrap().clear();
                wb.lock().unwrap().extend_from_slice(resp.as_bytes());
                Ok(PyObject::none())
            }),
        );

        for method_name in &[
            "do_GET",
            "do_POST",
            "do_PUT",
            "do_DELETE",
            "do_HEAD",
            "do_PATCH",
            "do_OPTIONS",
        ] {
            let wb = wfile_buf.clone();
            let mname = *method_name;
            w.insert(
                CompactString::from(mname),
                PyObject::native_closure(mname, move |_args| {
                    let body = format!(
                        "<html><body><h1>501 Not Implemented</h1><p>{} not implemented</p></body></html>",
                        mname
                    );
                    let resp = format!(
                        "HTTP/1.1 501 Not Implemented\r\n\
                         Content-Type: text/html; charset=utf-8\r\n\
                         Content-Length: {}\r\n\
                         Connection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    wb.lock().unwrap().clear();
                    wb.lock().unwrap().extend_from_slice(resp.as_bytes());
                    Ok(PyObject::none())
                }),
            );
        }

        w.insert(
            CompactString::from("log_message"),
            PyObject::native_closure("log_message", |_args| Ok(PyObject::none())),
        );
        w.insert(
            CompactString::from("log_request"),
            PyObject::native_closure("log_request", |_args| Ok(PyObject::none())),
        );

        let mut responses_map = IndexMap::new();
        let status_data: Vec<(i64, &str, &str)> = vec![
            (100, "Continue", "Request received, please continue"),
            (200, "OK", "Request fulfilled, document follows"),
            (201, "Created", "Document created, URL follows"),
            (204, "No Content", "Request fulfilled, nothing follows"),
            (301, "Moved Permanently", "Object moved permanently"),
            (302, "Found", "Object moved temporarily"),
            (
                304,
                "Not Modified",
                "Document has not changed since given time",
            ),
            (
                400,
                "Bad Request",
                "Bad request syntax or unsupported method",
            ),
            (
                401,
                "Unauthorized",
                "No permission -- see authorization schemes",
            ),
            (
                403,
                "Forbidden",
                "Request forbidden -- authorization will not help",
            ),
            (404, "Not Found", "Nothing matches the given URI"),
            (
                405,
                "Method Not Allowed",
                "Specified method is invalid for this resource",
            ),
            (500, "Internal Server Error", "Server got itself in trouble"),
            (
                501,
                "Not Implemented",
                "Server does not support this operation",
            ),
            (502, "Bad Gateway", "Invalid responses from another gateway"),
            (
                503,
                "Service Unavailable",
                "The server cannot process the request due to load",
            ),
        ];
        for (code, short, long) in &status_data {
            responses_map.insert(
                ferrython_core::types::HashableKey::Int(ferrython_core::types::PyInt::Small(*code)),
                PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from(*short)),
                    PyObject::str_val(CompactString::from(*long)),
                ]),
            );
        }
        w.insert(
            CompactString::from("responses"),
            PyObject::dict(responses_map),
        );

        w.insert(
            CompactString::from("server_version"),
            PyObject::str_val(CompactString::from("BaseHTTP/0.6")),
        );
        w.insert(
            CompactString::from("sys_version"),
            PyObject::str_val(CompactString::from("Ferrython/0.1")),
        );
        w.insert(
            CompactString::from("protocol_version"),
            PyObject::str_val(CompactString::from("HTTP/1.0")),
        );

        w.insert(
            CompactString::from("version_string"),
            PyObject::native_closure("version_string", |_args| {
                Ok(PyObject::str_val(CompactString::from(
                    "BaseHTTP/0.6 Ferrython/0.1",
                )))
            }),
        );

        w.insert(
            CompactString::from("date_time_string"),
            PyObject::native_closure("date_time_string", |args| {
                use std::time::{SystemTime, UNIX_EPOCH};
                let ts = if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None) {
                    args[0].as_int().unwrap_or(0) as u64
                } else {
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                };
                let secs = ts;
                let sec = secs % 60;
                let mins = secs / 60;
                let min = mins % 60;
                let hrs = mins / 60;
                let hour = hrs % 24;
                let mut days = (hrs / 24) as i64;
                let mut year = 1970i64;
                loop {
                    let dy = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                        366
                    } else {
                        365
                    };
                    if days < dy {
                        break;
                    }
                    days -= dy;
                    year += 1;
                }
                let month_days = [
                    31,
                    if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                        29
                    } else {
                        28
                    },
                    31,
                    30,
                    31,
                    30,
                    31,
                    31,
                    30,
                    31,
                    30,
                    31,
                ];
                let mut month = 0;
                while month < 12 && days >= month_days[month] {
                    days -= month_days[month];
                    month += 1;
                }
                let day = days + 1;
                let weekdays = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
                let months = [
                    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov",
                    "Dec",
                ];
                let wday = ((ts / 86400 + 3) % 7) as usize;
                let formatted = format!(
                    "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
                    weekdays[wday], day, months[month], year, hour, min, sec
                );
                Ok(PyObject::str_val(CompactString::from(formatted)))
            }),
        );

        w.insert(
            CompactString::from("translate_path"),
            PyObject::native_closure("translate_path", |args| {
                let request_path = if !args.is_empty() {
                    args[0].py_to_string()
                } else {
                    "/".to_string()
                };
                let path = if let Some(idx) = request_path.find('?') {
                    &request_path[..idx]
                } else {
                    request_path.as_str()
                };
                let decoded = percent_decode(path);
                let rel_path = decoded.trim_start_matches('/');
                if rel_path.is_empty() {
                    Ok(PyObject::str_val(CompactString::from(".")))
                } else {
                    Ok(PyObject::str_val(CompactString::from(rel_path)))
                }
            }),
        );
    }

    (inst, wfile_buf)
}

pub(super) fn simple_handler_do_get(
    wfile_buf: Arc<Mutex<Vec<u8>>>,
    head_only: bool,
) -> PyObjectRef {
    let name = if head_only { "do_HEAD" } else { "do_GET" };
    PyObject::native_closure(name, move |args| {
        let request_path = if !args.is_empty() {
            if let Some(p) = args[0].get_attr("path") {
                p.py_to_string()
            } else {
                args[0].py_to_string()
            }
        } else {
            "/".to_string()
        };

        let fs_path = if let Some(idx) = request_path.find('?') {
            &request_path[..idx]
        } else {
            request_path.as_str()
        };

        let decoded = percent_decode(fs_path);
        let rel_path = decoded.trim_start_matches('/');
        let target = if rel_path.is_empty() {
            std::path::PathBuf::from(".")
        } else {
            std::path::PathBuf::from(rel_path)
        };

        let mut buf = wfile_buf.lock().unwrap();
        buf.clear();

        if target.is_dir() {
            let index = target.join("index.html");
            if index.is_file() {
                match std::fs::read(&index) {
                    Ok(contents) => {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\n\
                             Content-Type: text/html; charset=utf-8\r\n\
                             Content-Length: {}\r\n\
                             Connection: close\r\n\r\n",
                            contents.len()
                        );
                        buf.extend_from_slice(header.as_bytes());
                        if !head_only {
                            buf.extend_from_slice(&contents);
                        }
                    }
                    Err(_) => {
                        let body = b"<html><body><h1>500 Internal Server Error</h1></body></html>";
                        let header = format!(
                            "HTTP/1.1 500 Internal Server Error\r\n\
                             Content-Type: text/html; charset=utf-8\r\n\
                             Content-Length: {}\r\n\
                             Connection: close\r\n\r\n",
                            body.len()
                        );
                        buf.extend_from_slice(header.as_bytes());
                        if !head_only {
                            buf.extend_from_slice(body);
                        }
                    }
                }
            } else {
                let mut body =
                    String::from("<html><head><title>Directory listing</title></head><body>\n");
                body.push_str(&format!(
                    "<h1>Directory listing for /{}</h1>\n<hr><ul>\n",
                    rel_path
                ));
                if let Ok(entries) = std::fs::read_dir(&target) {
                    let mut names: Vec<String> = entries
                        .filter_map(|e| e.ok())
                        .map(|e| {
                            let name = e.file_name().to_string_lossy().to_string();
                            if e.path().is_dir() {
                                format!("{}/", name)
                            } else {
                                name
                            }
                        })
                        .collect();
                    names.sort();
                    for name in &names {
                        let href = if rel_path.is_empty() {
                            format!("/{}", name)
                        } else {
                            format!("/{}/{}", rel_path, name)
                        };
                        body.push_str(&format!("<li><a href=\"{}\">{}</a></li>\n", href, name));
                    }
                }
                body.push_str("</ul><hr></body></html>\n");
                let body_bytes = body.as_bytes();
                let header = format!(
                    "HTTP/1.1 200 OK\r\n\
                     Content-Type: text/html; charset=utf-8\r\n\
                     Content-Length: {}\r\n\
                     Connection: close\r\n\r\n",
                    body_bytes.len()
                );
                buf.extend_from_slice(header.as_bytes());
                if !head_only {
                    buf.extend_from_slice(body_bytes);
                }
            }
        } else if target.is_file() {
            match std::fs::read(&target) {
                Ok(contents) => {
                    let ctype = guess_content_type(target.to_str().unwrap_or(""));
                    let header = format!(
                        "HTTP/1.1 200 OK\r\n\
                         Content-Type: {}\r\n\
                         Content-Length: {}\r\n\
                         Connection: close\r\n\r\n",
                        ctype,
                        contents.len()
                    );
                    buf.extend_from_slice(header.as_bytes());
                    if !head_only {
                        buf.extend_from_slice(&contents);
                    }
                }
                Err(_) => {
                    let body = b"<html><body><h1>403 Forbidden</h1></body></html>";
                    let header = format!(
                        "HTTP/1.1 403 Forbidden\r\n\
                         Content-Type: text/html; charset=utf-8\r\n\
                         Content-Length: {}\r\n\
                         Connection: close\r\n\r\n",
                        body.len()
                    );
                    buf.extend_from_slice(header.as_bytes());
                    if !head_only {
                        buf.extend_from_slice(body);
                    }
                }
            }
        } else {
            let body = b"<html><body><h1>404 Not Found</h1></body></html>";
            let header = format!(
                "HTTP/1.1 404 Not Found\r\n\
                 Content-Type: text/html; charset=utf-8\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\r\n",
                body.len()
            );
            buf.extend_from_slice(header.as_bytes());
            if !head_only {
                buf.extend_from_slice(body);
            }
        }

        Ok(PyObject::none())
    })
}
