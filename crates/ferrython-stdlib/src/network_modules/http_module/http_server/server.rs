use super::handlers::{build_handler_instance, simple_handler_do_get};
use super::protocol::{parse_http_request, write_error_response, HttpRequest};
use super::*;

#[allow(dead_code)]
struct HttpServerState {
    listener: Option<TcpListener>,
    host: String,
    port: u16,
}

pub fn create_http_server_module() -> PyObjectRef {
    let http_server_fn = make_builtin(|args: &[PyObjectRef]| {
        let (host, port) = if !args.is_empty() {
            let addr_obj = &args[0];
            match &addr_obj.payload {
                PyObjectPayload::Tuple(items) if items.len() >= 2 => {
                    let h = items[0].py_to_string();
                    let p = items[1].as_int().unwrap_or(8000) as u16;
                    let h = if h.is_empty() {
                        "0.0.0.0".to_string()
                    } else {
                        h
                    };
                    (h, p)
                }
                _ => {
                    let s = addr_obj.py_to_string();
                    if let Some(idx) = s.rfind(':') {
                        let port = s[idx + 1..].parse::<u16>().unwrap_or(8000);
                        (s[..idx].to_string(), port)
                    } else {
                        ("0.0.0.0".to_string(), 8000)
                    }
                }
            }
        } else {
            ("0.0.0.0".to_string(), 8000)
        };

        let handler_cls = if args.len() > 1 {
            args[1].clone()
        } else {
            PyObject::class(
                CompactString::from("BaseHTTPRequestHandler"),
                vec![],
                IndexMap::new(),
            )
        };

        let addr_str = format!("{}:{}", host, port);
        let listener = TcpListener::bind(&addr_str)
            .map_err(|e| PyException::os_error(format!("HTTPServer bind({}): {}", addr_str, e)))?;

        listener
            .set_nonblocking(false)
            .map_err(|e| PyException::os_error(format!("set_nonblocking: {}", e)))?;

        let server_state = Arc::new(Mutex::new(HttpServerState {
            listener: Some(listener),
            host: host.clone(),
            port,
        }));

        let running = Arc::new(AtomicBool::new(false));

        let cls = PyObject::class(CompactString::from("HTTPServer"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();

            w.insert(
                CompactString::from("server_address"),
                PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from(host.as_str())),
                    PyObject::int(port as i64),
                ]),
            );

            w.insert(
                CompactString::from("server_name"),
                PyObject::str_val(CompactString::from(host.as_str())),
            );
            w.insert(
                CompactString::from("server_port"),
                PyObject::int(port as i64),
            );

            let ss = server_state.clone();
            let r = running.clone();
            let hcls = handler_cls.clone();
            w.insert(
                CompactString::from("serve_forever"),
                PyObject::native_closure("serve_forever", move |_args| {
                    r.store(true, Ordering::SeqCst);
                    loop {
                        if !r.load(Ordering::SeqCst) {
                            break;
                        }
                        let listener_clone = {
                            let guard = ss
                                .lock()
                                .map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;
                            match &guard.listener {
                                Some(l) => l.try_clone().map_err(|e| {
                                    PyException::os_error(format!("try_clone: {}", e))
                                })?,
                                None => return Ok(PyObject::none()),
                            }
                        };

                        let _ = listener_clone.set_nonblocking(true);

                        match listener_clone.accept() {
                            Ok((mut stream, _addr)) => {
                                let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
                                let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
                                handle_one_connection(&mut stream, &hcls);
                            }
                            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(Duration::from_millis(100));
                                continue;
                            }
                            Err(_) => {
                                std::thread::sleep(Duration::from_millis(50));
                                continue;
                            }
                        }
                    }
                    Ok(PyObject::none())
                }),
            );

            let ss = server_state.clone();
            let hcls = handler_cls.clone();
            w.insert(
                CompactString::from("handle_request"),
                PyObject::native_closure("handle_request", move |_args| {
                    let listener_clone = {
                        let guard = ss
                            .lock()
                            .map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;
                        match &guard.listener {
                            Some(l) => l
                                .try_clone()
                                .map_err(|e| PyException::os_error(format!("try_clone: {}", e)))?,
                            None => {
                                return Err(PyException::runtime_error("server is closed"));
                            }
                        }
                    };
                    match listener_clone.accept() {
                        Ok((mut stream, _addr)) => {
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
                            let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
                            handle_one_connection(&mut stream, &hcls);
                            Ok(PyObject::none())
                        }
                        Err(e) => Err(PyException::os_error(format!("accept: {}", e))),
                    }
                }),
            );

            let r = running.clone();
            w.insert(
                CompactString::from("shutdown"),
                PyObject::native_closure("shutdown", move |_args| {
                    r.store(false, Ordering::SeqCst);
                    Ok(PyObject::none())
                }),
            );

            let ss = server_state.clone();
            w.insert(
                CompactString::from("server_close"),
                PyObject::native_closure("server_close", move |_args| {
                    let mut guard = ss
                        .lock()
                        .map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;
                    guard.listener = None;
                    Ok(PyObject::none())
                }),
            );

            w.insert(CompactString::from("socket"), PyObject::none());
        }
        Ok(inst)
    });

    let base_handler_fn = make_builtin(|args: &[PyObjectRef]| {
        let req = HttpRequest {
            method: String::new(),
            path: String::new(),
            version: "HTTP/1.1".to_string(),
            headers: IndexMap::new(),
            body: Vec::new(),
        };
        let dummy_cls = PyObject::class(
            CompactString::from("BaseHTTPRequestHandler"),
            vec![],
            IndexMap::new(),
        );
        let (inst, _wbuf) = build_handler_instance(&req, &dummy_cls);

        if args.len() > 1 {
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("client_address"), args[1].clone());
            }
        }
        if args.len() > 2 {
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("server"), args[2].clone());
            }
        }

        Ok(inst)
    });

    let simple_handler_fn = make_builtin(|args: &[PyObjectRef]| {
        let req = HttpRequest {
            method: "GET".to_string(),
            path: "/".to_string(),
            version: "HTTP/1.1".to_string(),
            headers: IndexMap::new(),
            body: Vec::new(),
        };
        let cls = PyObject::class(
            CompactString::from("SimpleHTTPRequestHandler"),
            vec![],
            IndexMap::new(),
        );
        let (inst, wbuf) = build_handler_instance(&req, &cls);

        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(
                CompactString::from("do_GET"),
                simple_handler_do_get(wbuf.clone(), false),
            );
            w.insert(
                CompactString::from("do_HEAD"),
                simple_handler_do_get(wbuf.clone(), true),
            );

            if args.len() > 1 {
                w.insert(CompactString::from("client_address"), args[1].clone());
            }
            if args.len() > 2 {
                w.insert(CompactString::from("server"), args[2].clone());
            }
        }

        Ok(inst)
    });

    make_module(
        "http.server",
        vec![
            ("HTTPServer", http_server_fn),
            ("BaseHTTPRequestHandler", base_handler_fn),
            ("SimpleHTTPRequestHandler", simple_handler_fn),
            (
                "CGIHTTPRequestHandler",
                make_builtin(|args: &[PyObjectRef]| {
                    let req = HttpRequest {
                        method: "GET".to_string(),
                        path: "/".to_string(),
                        version: "HTTP/1.1".to_string(),
                        headers: IndexMap::new(),
                        body: Vec::new(),
                    };
                    let cls = PyObject::class(
                        CompactString::from("CGIHTTPRequestHandler"),
                        vec![],
                        IndexMap::new(),
                    );
                    let (inst, wbuf) = build_handler_instance(&req, &cls);
                    if let PyObjectPayload::Instance(ref d) = inst.payload {
                        let mut w = d.attrs.write();
                        w.insert(
                            CompactString::from("do_GET"),
                            simple_handler_do_get(wbuf.clone(), false),
                        );
                        w.insert(
                            CompactString::from("do_HEAD"),
                            simple_handler_do_get(wbuf.clone(), true),
                        );
                        w.insert(
                            CompactString::from("cgi_directories"),
                            PyObject::list(vec![
                                PyObject::str_val(CompactString::from("/cgi-bin")),
                                PyObject::str_val(CompactString::from("/htbin")),
                            ]),
                        );
                        if args.len() > 1 {
                            w.insert(CompactString::from("client_address"), args[1].clone());
                        }
                    }
                    Ok(inst)
                }),
            ),
            (
                "DEFAULT_ERROR_MESSAGE",
                PyObject::str_val(CompactString::from(
                    "<html><head><title>Error</title></head><body><h1>%(code)d %(message)s</h1></body></html>",
                )),
            ),
            (
                "DEFAULT_ERROR_CONTENT_TYPE",
                PyObject::str_val(CompactString::from("text/html;charset=utf-8")),
            ),
        ],
    )
}

fn handle_one_connection(stream: &mut TcpStream, handler_cls: &PyObjectRef) {
    let req = {
        let mut reader = BufReader::new(&mut *stream);
        match parse_http_request(&mut reader) {
            Some(r) => r,
            None => return,
        }
    };

    let method = req.method.clone();
    let (handler_inst, wfile_buf) = build_handler_instance(&req, handler_cls);

    let is_simple = handler_cls
        .py_to_string()
        .contains("SimpleHTTPRequestHandler");

    if is_simple {
        if let PyObjectPayload::Instance(ref d) = handler_inst.payload {
            let mut w = d.attrs.write();
            w.insert(
                CompactString::from("do_GET"),
                simple_handler_do_get(wfile_buf.clone(), false),
            );
            w.insert(
                CompactString::from("do_HEAD"),
                simple_handler_do_get(wfile_buf.clone(), true),
            );
        }
    }

    if let PyObjectPayload::Class(ref cd) = handler_cls.payload {
        let ns = cd.namespace.read();
        for key in ns.keys() {
            if key.starts_with("do_") {
                if let Some(func) = ns.get(key) {
                    if let PyObjectPayload::Instance(ref d) = handler_inst.payload {
                        d.attrs.write().insert(key.clone(), func.clone());
                    }
                }
            }
        }
    }

    let do_method_name = format!("do_{}", method);
    let handler_method = if let PyObjectPayload::Instance(ref d) = handler_inst.payload {
        d.attrs.read().get(do_method_name.as_str()).cloned()
    } else {
        None
    };

    match handler_method {
        Some(func) => {
            let result = match &func.payload {
                PyObjectPayload::NativeClosure(nc) => (nc.func)(&[handler_inst.clone()]),
                PyObjectPayload::NativeFunction(nf) => (nf.func)(&[handler_inst.clone()]),
                _ => Ok(PyObject::none()),
            };
            if result.is_err() {
                write_error_response(stream, 500, "Internal Server Error");
                return;
            }
        }
        None => {
            write_error_response(stream, 501, &format!("Method {} not implemented", method));
            return;
        }
    }

    let response_data = wfile_buf.lock().unwrap();
    if !response_data.is_empty() {
        let _ = stream.write_all(&response_data);
        let _ = stream.flush();
    }
}
