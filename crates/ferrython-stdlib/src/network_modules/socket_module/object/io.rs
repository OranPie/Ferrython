use super::*;

pub(super) fn install_io_methods(
    attrs: &mut IndexMap<CompactString, PyObjectRef>,
    inner: &Arc<Mutex<SocketInner>>,
) {
    let st = inner.clone();
    attrs.insert(
        CompactString::from("send"),
        PyObject::native_closure("send", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("send() requires a data argument"));
            }
            let data = match &args[0].payload {
                PyObjectPayload::Bytes(b) => (**b).clone(),
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
                PyObjectPayload::Bytes(b) => (**b).clone(),
                PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                _ => return Err(PyException::type_error("a bytes-like object is required")),
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
            let stream_clone = {
                let guard = lock_inner(&st)?;
                if guard.closed {
                    return Err(PyException::os_error("[Errno 9] Bad file descriptor"));
                }
                if let Some(ref stream) = guard.tcp_stream {
                    Some(
                        stream
                            .try_clone()
                            .map_err(|e| PyException::os_error(format!("recv clone: {}", e)))?,
                    )
                } else if let Some(ref sock) = guard.udp_socket {
                    let mut buf = vec![0u8; bufsize as usize];
                    return match sock.recv(&mut buf) {
                        Ok(n) => {
                            buf.truncate(n);
                            Ok(PyObject::bytes(buf))
                        }
                        Err(e) => Err(PyException::os_error(format!("recv: {}", e))),
                    };
                } else {
                    None
                }
            };
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
                PyObjectPayload::Bytes(b) => (**b).clone(),
                PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                _ => return Err(PyException::type_error("a bytes-like object is required")),
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
                Err(PyException::os_error(
                    "sendto() on non-UDP socket without connection",
                ))
            }
        }),
    );

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
                    Err(e)
                        if e.kind() == std::io::ErrorKind::TimedOut
                            || e.kind() == std::io::ErrorKind::WouldBlock =>
                    {
                        Err(PyException::new(ExceptionKind::TimeoutError, "timed out"))
                    }
                    Err(e) => Err(PyException::os_error(format!("recvfrom: {}", e))),
                }
            } else {
                Err(PyException::os_error(
                    "recvfrom() requires a bound UDP socket",
                ))
            }
        }),
    );

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
                let cloned = stream
                    .try_clone()
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
                        let size = if !args.is_empty() {
                            args[0].as_int().unwrap_or(-1)
                        } else {
                            -1
                        };
                        let mut stream = is
                            .lock()
                            .map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;
                        if size < 0 {
                            let mut buf = Vec::new();
                            stream
                                .read_to_end(&mut buf)
                                .map_err(|e| PyException::os_error(format!("read: {}", e)))?;
                            Ok(PyObject::bytes(buf))
                        } else {
                            let mut buf = vec![0u8; size as usize];
                            let n = stream
                                .read(&mut buf)
                                .map_err(|e| PyException::os_error(format!("read: {}", e)))?;
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
                            let mut stream = is
                                .lock()
                                .map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;
                            let mut line = Vec::new();
                            let mut byte = [0u8; 1];
                            loop {
                                match stream.read(&mut byte) {
                                    Ok(0) => break,
                                    Ok(_) => {
                                        line.push(byte[0]);
                                        if byte[0] == b'\n' {
                                            break;
                                        }
                                    }
                                    Err(e)
                                        if e.kind() == std::io::ErrorKind::TimedOut
                                            || e.kind() == std::io::ErrorKind::WouldBlock =>
                                    {
                                        break
                                    }
                                    Err(e) => {
                                        return Err(PyException::os_error(format!(
                                            "readline: {}",
                                            e
                                        )))
                                    }
                                }
                            }
                            if mode.contains('b') {
                                Ok(PyObject::bytes(line))
                            } else {
                                Ok(PyObject::str_val(CompactString::from(
                                    String::from_utf8_lossy(&line).as_ref(),
                                )))
                            }
                        }
                    }),
                );
                let is = inner_stream.clone();
                file_attrs.insert(
                    CompactString::from("write"),
                    PyObject::native_closure("write", move |args| {
                        if args.is_empty() {
                            return Err(PyException::type_error("write() requires data"));
                        }
                        let data = match &args[0].payload {
                            PyObjectPayload::Bytes(b) => (**b).clone(),
                            PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                            _ => {
                                return Err(PyException::type_error(
                                    "a bytes-like object is required",
                                ))
                            }
                        };
                        let mut stream = is
                            .lock()
                            .map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;
                        let n = stream
                            .write(&data)
                            .map_err(|e| PyException::os_error(format!("write: {}", e)))?;
                        Ok(PyObject::int(n as i64))
                    }),
                );
                let is = inner_stream.clone();
                file_attrs.insert(
                    CompactString::from("flush"),
                    PyObject::native_closure("flush", move |_args| {
                        let mut stream = is
                            .lock()
                            .map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;
                        stream
                            .flush()
                            .map_err(|e| PyException::os_error(format!("flush: {}", e)))?;
                        Ok(PyObject::none())
                    }),
                );
                file_attrs.insert(
                    CompactString::from("close"),
                    PyObject::native_closure("close", move |_args| Ok(PyObject::none())),
                );
                file_attrs.insert(
                    CompactString::from("_bind_methods"),
                    PyObject::bool_val(true),
                );
                Ok(PyObject::module_with_attrs(
                    CompactString::from("socket.makefile"),
                    file_attrs,
                ))
            } else {
                Err(PyException::os_error(
                    "makefile() requires a connected TCP socket",
                ))
            }
        }),
    );
}
