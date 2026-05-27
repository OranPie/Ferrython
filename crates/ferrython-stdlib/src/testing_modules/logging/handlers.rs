use super::*;

pub(super) fn create_stream_handler_fn() -> PyObjectRef {
    // StreamHandler class — creates handler instance with stream ref and format/emit
    let stream_handler_cls = PyObject::class(
        CompactString::from("StreamHandler"),
        vec![],
        IndexMap::new(),
    );
    let sh_cls = stream_handler_cls.clone();
    let stream_handler_fn =
        PyObject::native_closure("StreamHandler", move |args: &[PyObjectRef]| {
            let inst = PyObject::instance(sh_cls.clone());
            let stream = if args.is_empty() {
                PyObject::none()
            } else {
                args[0].clone()
            };
            // Shared state for formatter and level
            let formatter_ref: Rc<PyCell<PyObjectRef>> = Rc::new(PyCell::new(PyObject::none()));
            let level_ref: Rc<PyCell<i64>> = Rc::new(PyCell::new(0));

            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                let mut attrs = inst_data.attrs.write();
                attrs.insert(CompactString::from("stream"), stream.clone());
                attrs.insert(CompactString::from("level"), PyObject::int(0));
                attrs.insert(CompactString::from("formatter"), PyObject::none());

                let lr = level_ref.clone();
                let inst_for_level = inst.clone();
                attrs.insert(
                    CompactString::from("setLevel"),
                    PyObject::native_closure("setLevel", move |args: &[PyObjectRef]| {
                        if let Some(v) = args.first() {
                            if let Some(n) = v.as_int() {
                                *lr.write() = n;
                                // Also update the instance attribute so handler.level is visible
                                if let PyObjectPayload::Instance(ref d) = inst_for_level.payload {
                                    d.attrs
                                        .write()
                                        .insert(CompactString::from("level"), PyObject::int(n));
                                }
                            }
                        }
                        Ok(PyObject::none())
                    }),
                );
                let fr = formatter_ref.clone();
                attrs.insert(
                    CompactString::from("setFormatter"),
                    PyObject::native_closure("setFormatter", move |args: &[PyObjectRef]| {
                        if let Some(v) = args.first() {
                            *fr.write() = v.clone();
                        }
                        Ok(PyObject::none())
                    }),
                );
                // emit(record) — write formatted message to stream or stderr
                let fr2 = formatter_ref.clone();
                let stream2 = stream.clone();
                attrs.insert(
                    CompactString::from("emit"),
                    PyObject::native_closure("emit", move |args: &[PyObjectRef]| {
                        // args[0] may be handler (from logger dispatch) or record (direct call)
                        // Detect: if called with 2 args, args[0]=handler, args[1]=record
                        // If called with 1 arg, args[0]=record
                        let record = if args.len() >= 2 {
                            &args[1]
                        } else if !args.is_empty() {
                            &args[0]
                        } else {
                            return Ok(PyObject::none());
                        };

                        let msg = if let Some(m) = record.get_attr("message") {
                            m.py_to_string()
                        } else if let Some(m) = record.get_attr("msg") {
                            m.py_to_string()
                        } else {
                            record.py_to_string()
                        };

                        // Apply formatter if set
                        let fmt = fr2.read().clone();
                        let formatted = if !matches!(&fmt.payload, PyObjectPayload::None) {
                            if let Some(fmt_fn) = fmt.get_attr("format") {
                                // Use Formatter.format(record) for full field resolution
                                match &fmt_fn.payload {
                                    PyObjectPayload::NativeClosure(nc) => {
                                        (nc.func)(&[record.clone()])
                                            .map(|r| r.py_to_string())
                                            .unwrap_or_else(|_| msg.clone())
                                    }
                                    _ => msg.clone(),
                                }
                            } else if let Some(fmt_str) = fmt.get_attr("_fmt") {
                                let fs = fmt_str.py_to_string();
                                let mut result = fs.clone();
                                result = result.replace("%(message)s", &msg);
                                let levelname = if let Some(ln) = record.get_attr("levelname") {
                                    ln.py_to_string()
                                } else {
                                    "INFO".to_string()
                                };
                                let name = if let Some(n) = record.get_attr("name") {
                                    n.py_to_string()
                                } else {
                                    "root".to_string()
                                };
                                result = result.replace("%(levelname)s", &levelname);
                                result = result.replace("%(name)s", &name);
                                result = result.replace("%(asctime)s", &current_asctime(None));
                                result = result.replace(
                                    "%(lineno)d",
                                    &record
                                        .get_attr("lineno")
                                        .map(|l| l.py_to_string())
                                        .unwrap_or_else(|| "0".to_string()),
                                );
                                result = result.replace(
                                    "%(filename)s",
                                    &record
                                        .get_attr("filename")
                                        .map(|f| f.py_to_string())
                                        .unwrap_or_default(),
                                );
                                result = result.replace(
                                    "%(funcName)s",
                                    &record
                                        .get_attr("funcName")
                                        .map(|f| f.py_to_string())
                                        .unwrap_or_default(),
                                );
                                result = result.replace(
                                    "%(module)s",
                                    &record
                                        .get_attr("module")
                                        .map(|m| m.py_to_string())
                                        .unwrap_or_default(),
                                );
                                result = result.replace(
                                    "%(pathname)s",
                                    &record
                                        .get_attr("pathname")
                                        .map(|p| p.py_to_string())
                                        .unwrap_or_default(),
                                );
                                result
                            } else {
                                msg.clone()
                            }
                        } else {
                            msg.clone()
                        };

                        // Write to stream via its write() method
                        let line = format!("{}\n", formatted);
                        if let Some(write_fn) = stream2.get_attr("write") {
                            let line_obj = PyObject::str_val(CompactString::from(&line));
                            match &write_fn.payload {
                                PyObjectPayload::NativeClosure(nc) => {
                                    let _ = (nc.func)(&[line_obj]);
                                }
                                PyObjectPayload::NativeFunction(nf) => {
                                    let _ = (nf.func)(&[line_obj]);
                                }
                                _ => {
                                    eprintln!("{}", formatted);
                                }
                            }
                        } else {
                            eprintln!("{}", formatted);
                        }
                        Ok(PyObject::none())
                    }),
                );
            }
            Ok(inst)
        });
    stream_handler_fn
}

pub(super) fn create_file_handler_fn() -> PyObjectRef {
    // FileHandler class — handler that writes to file
    let file_handler_cls =
        PyObject::class(CompactString::from("FileHandler"), vec![], IndexMap::new());
    let fh_cls = file_handler_cls.clone();
    let file_handler_fn = PyObject::native_closure("FileHandler", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(fh_cls.clone());
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let filename = if args.is_empty() {
                CompactString::from("")
            } else {
                CompactString::from(args[0].py_to_string())
            };
            // mode: 'a' (append) by default, 'w' for truncate
            let mode = if args.len() > 1 {
                args[1].py_to_string()
            } else {
                "a".to_string()
            };
            attrs.insert(
                CompactString::from("baseFilename"),
                PyObject::str_val(filename.clone()),
            );
            attrs.insert(
                CompactString::from("mode"),
                PyObject::str_val(CompactString::from(&mode)),
            );
            attrs.insert(CompactString::from("level"), PyObject::int(0));
            attrs.insert(CompactString::from("formatter"), PyObject::none());

            // Shared formatter/level refs for closures
            let fmt_ref: Rc<PyCell<PyObjectRef>> = Rc::new(PyCell::new(PyObject::none()));
            let level_ref: Rc<PyCell<i64>> = Rc::new(PyCell::new(0));

            let lr = level_ref.clone();
            attrs.insert(
                CompactString::from("setLevel"),
                PyObject::native_closure("setLevel", move |args: &[PyObjectRef]| {
                    if let Some(v) = args.first().and_then(|a| a.as_int()) {
                        *lr.write() = v;
                    }
                    Ok(PyObject::none())
                }),
            );
            let fr = fmt_ref.clone();
            attrs.insert(
                CompactString::from("setFormatter"),
                PyObject::native_closure("setFormatter", move |args: &[PyObjectRef]| {
                    if let Some(v) = args.first() {
                        *fr.write() = v.clone();
                    }
                    Ok(PyObject::none())
                }),
            );
            // emit(record) — write formatted message to file
            let fr2 = fmt_ref.clone();
            let fname = filename.clone();
            let fmode = mode.clone();
            attrs.insert(
                CompactString::from("emit"),
                PyObject::native_closure("emit", move |args: &[PyObjectRef]| {
                    let record = if args.len() >= 2 {
                        &args[1]
                    } else if !args.is_empty() {
                        &args[0]
                    } else {
                        return Ok(PyObject::none());
                    };
                    let msg = if let Some(m) = record.get_attr("message") {
                        m.py_to_string()
                    } else if let Some(m) = record.get_attr("msg") {
                        m.py_to_string()
                    } else {
                        record.py_to_string()
                    };

                    // Apply formatter
                    let fmt = fr2.read().clone();
                    let formatted = if !matches!(&fmt.payload, PyObjectPayload::None) {
                        if let Some(fmt_fn) = fmt.get_attr("format") {
                            match &fmt_fn.payload {
                                PyObjectPayload::NativeClosure(nc) => (nc.func)(&[record.clone()])
                                    .map(|r| r.py_to_string())
                                    .unwrap_or_else(|_| msg.clone()),
                                _ => msg.clone(),
                            }
                        } else if let Some(fmt_str) = fmt.get_attr("_fmt") {
                            let fs = fmt_str.py_to_string();
                            let mut result = fs.clone();
                            result = result.replace("%(message)s", &msg);
                            let levelname = record
                                .get_attr("levelname")
                                .map(|l| l.py_to_string())
                                .unwrap_or_else(|| "INFO".to_string());
                            let name = record
                                .get_attr("name")
                                .map(|n| n.py_to_string())
                                .unwrap_or_else(|| "root".to_string());
                            result = result.replace("%(levelname)s", &levelname);
                            result = result.replace("%(name)s", &name);
                            result = result.replace("%(asctime)s", &current_asctime(None));
                            result = result.replace(
                                "%(lineno)d",
                                &record
                                    .get_attr("lineno")
                                    .map(|l| l.py_to_string())
                                    .unwrap_or_else(|| "0".to_string()),
                            );
                            result = result.replace(
                                "%(filename)s",
                                &record
                                    .get_attr("filename")
                                    .map(|f| f.py_to_string())
                                    .unwrap_or_default(),
                            );
                            result
                        } else {
                            msg.clone()
                        }
                    } else {
                        msg.clone()
                    };

                    // Write to file
                    use std::io::Write;
                    let line = format!("{}\n", formatted);
                    let result = if fmode == "w" {
                        std::fs::write(fname.as_str(), &line)
                    } else {
                        std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(fname.as_str())
                            .and_then(|mut f| f.write_all(line.as_bytes()))
                    };
                    if let Err(e) = result {
                        eprintln!("FileHandler error: {}", e);
                    }
                    Ok(PyObject::none())
                }),
            );
            // close() — no-op (file is opened/closed per emit)
            attrs.insert(
                CompactString::from("close"),
                make_builtin(|_| Ok(PyObject::none())),
            );
        }
        Ok(inst)
    });
    file_handler_fn
}

pub(super) fn create_rotating_file_handler_fn() -> PyObjectRef {
    // RotatingFileHandler(filename, mode='a', maxBytes=0, backupCount=0)
    let rfh_cls = PyObject::class(
        CompactString::from("RotatingFileHandler"),
        vec![],
        IndexMap::new(),
    );
    let rfh_cls2 = rfh_cls.clone();
    let rotating_file_handler_fn =
        PyObject::native_closure("RotatingFileHandler", move |args: &[PyObjectRef]| {
            let inst = PyObject::instance(rfh_cls2.clone());
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                let mut attrs = inst_data.attrs.write();
                let filename = if args.is_empty() {
                    CompactString::from("")
                } else {
                    CompactString::from(args[0].py_to_string())
                };
                let max_bytes: i64 = if args.len() > 2 {
                    args[2].as_int().unwrap_or(0)
                } else {
                    0
                };
                let backup_count: i64 = if args.len() > 3 {
                    args[3].as_int().unwrap_or(0)
                } else {
                    0
                };

                attrs.insert(
                    CompactString::from("baseFilename"),
                    PyObject::str_val(filename.clone()),
                );
                attrs.insert(CompactString::from("maxBytes"), PyObject::int(max_bytes));
                attrs.insert(
                    CompactString::from("backupCount"),
                    PyObject::int(backup_count),
                );
                attrs.insert(CompactString::from("level"), PyObject::int(0));
                attrs.insert(CompactString::from("formatter"), PyObject::none());

                let fmt_ref: Rc<PyCell<PyObjectRef>> = Rc::new(PyCell::new(PyObject::none()));
                let fr = fmt_ref.clone();
                attrs.insert(
                    CompactString::from("setLevel"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
                attrs.insert(
                    CompactString::from("setFormatter"),
                    PyObject::native_closure("setFormatter", move |args: &[PyObjectRef]| {
                        if let Some(v) = args.first() {
                            *fr.write() = v.clone();
                        }
                        Ok(PyObject::none())
                    }),
                );

                // emit with rotation
                let fr2 = fmt_ref.clone();
                let fname = filename.clone();
                attrs.insert(
                    CompactString::from("emit"),
                    PyObject::native_closure("emit", move |args: &[PyObjectRef]| {
                        let record = if args.len() >= 2 {
                            &args[1]
                        } else if !args.is_empty() {
                            &args[0]
                        } else {
                            return Ok(PyObject::none());
                        };
                        let msg = record
                            .get_attr("message")
                            .or_else(|| record.get_attr("msg"))
                            .map(|m| m.py_to_string())
                            .unwrap_or_else(|| record.py_to_string());
                        let fmt = fr2.read().clone();
                        let formatted = if !matches!(&fmt.payload, PyObjectPayload::None) {
                            if let Some(fmt_fn) = fmt.get_attr("format") {
                                match &fmt_fn.payload {
                                    PyObjectPayload::NativeClosure(nc) => {
                                        (nc.func)(&[record.clone()])
                                            .map(|r| r.py_to_string())
                                            .unwrap_or_else(|_| msg.clone())
                                    }
                                    _ => msg.clone(),
                                }
                            } else if let Some(fmt_str) = fmt.get_attr("_fmt") {
                                let fs = fmt_str.py_to_string();
                                fs.replace("%(message)s", &msg)
                                    .replace(
                                        "%(levelname)s",
                                        &record
                                            .get_attr("levelname")
                                            .map(|l| l.py_to_string())
                                            .unwrap_or_else(|| "INFO".to_string()),
                                    )
                                    .replace(
                                        "%(name)s",
                                        &record
                                            .get_attr("name")
                                            .map(|n| n.py_to_string())
                                            .unwrap_or_else(|| "root".to_string()),
                                    )
                                    .replace("%(asctime)s", &current_asctime(None))
                                    .replace(
                                        "%(lineno)d",
                                        &record
                                            .get_attr("lineno")
                                            .map(|l| l.py_to_string())
                                            .unwrap_or_else(|| "0".to_string()),
                                    )
                                    .replace(
                                        "%(filename)s",
                                        &record
                                            .get_attr("filename")
                                            .map(|f| f.py_to_string())
                                            .unwrap_or_default(),
                                    )
                            } else {
                                msg.clone()
                            }
                        } else {
                            msg.clone()
                        };

                        // Check rotation
                        if max_bytes > 0 {
                            let current_size = std::fs::metadata(fname.as_str())
                                .map(|m| m.len() as i64)
                                .unwrap_or(0);
                            if current_size + formatted.len() as i64 > max_bytes {
                                // Rotate: .log.N-1 -> .log.N, ... , .log -> .log.1
                                for i in (1..backup_count).rev() {
                                    let src = format!("{}.{}", fname, i);
                                    let dst = format!("{}.{}", fname, i + 1);
                                    let _ = std::fs::rename(&src, &dst);
                                }
                                if backup_count > 0 {
                                    let _ = std::fs::rename(fname.as_str(), format!("{}.1", fname));
                                } else {
                                    let _ = std::fs::write(fname.as_str(), "");
                                }
                            }
                        }
                        use std::io::Write;
                        let line = format!("{}\n", formatted);
                        if let Ok(mut f) = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(fname.as_str())
                        {
                            let _ = f.write_all(line.as_bytes());
                        }
                        Ok(PyObject::none())
                    }),
                );
                attrs.insert(
                    CompactString::from("close"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
                attrs.insert(
                    CompactString::from("doRollover"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
            }
            Ok(inst)
        });
    rotating_file_handler_fn
}

pub(super) fn create_formatter_fn() -> PyObjectRef {
    // Formatter(fmt, datefmt=None) — stores format string, has format(record) method
    let formatter_cls = PyObject::class(CompactString::from("Formatter"), vec![], IndexMap::new());
    let fmt_cls = formatter_cls.clone();
    let formatter_fn = PyObject::native_closure("Formatter", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(fmt_cls.clone());
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            // Handle both positional args and kwargs dict
            let (fmt_str, datefmt) = if !args.is_empty() {
                if let PyObjectPayload::Dict(kw_map) = &args[0].payload {
                    // kwargs passed as dict
                    let r = kw_map.read();
                    let f = r
                        .get(&HashableKey::str_key(CompactString::from("fmt")))
                        .map(|v| CompactString::from(v.py_to_string()))
                        .unwrap_or_else(|| {
                            CompactString::from("%(levelname)s:%(name)s:%(message)s")
                        });
                    let d = r
                        .get(&HashableKey::str_key(CompactString::from("datefmt")))
                        .and_then(|v| {
                            if matches!(v.payload, PyObjectPayload::None) {
                                None
                            } else {
                                Some(v.py_to_string())
                            }
                        });
                    (f, d)
                } else {
                    let f = CompactString::from(args[0].py_to_string());
                    let d = if args.len() > 1 && !matches!(args[1].payload, PyObjectPayload::None) {
                        // Second positional could also be a kwargs dict
                        if let PyObjectPayload::Dict(kw_map) = &args[1].payload {
                            let r = kw_map.read();
                            r.get(&HashableKey::str_key(CompactString::from("datefmt")))
                                .and_then(|v| {
                                    if matches!(v.payload, PyObjectPayload::None) {
                                        None
                                    } else {
                                        Some(v.py_to_string())
                                    }
                                })
                        } else {
                            Some(args[1].py_to_string())
                        }
                    } else {
                        None
                    };
                    (f, d)
                }
            } else {
                (
                    CompactString::from("%(levelname)s:%(name)s:%(message)s"),
                    None,
                )
            };
            let fs = fmt_str.clone();
            let df = datefmt.clone();
            attrs.insert(CompactString::from("_fmt"), PyObject::str_val(fmt_str));
            attrs.insert(
                CompactString::from("datefmt"),
                if let Some(ref d) = datefmt {
                    PyObject::str_val(CompactString::from(d.as_str()))
                } else {
                    PyObject::none()
                },
            );
            // format(record) — apply %(key)s substitution from record attrs
            attrs.insert(
                CompactString::from("format"),
                PyObject::native_closure("Formatter.format", move |args: &[PyObjectRef]| {
                    let record = if !args.is_empty() {
                        &args[0]
                    } else {
                        return Ok(PyObject::str_val(CompactString::from("")));
                    };
                    let result = fs.to_string();
                    // Apply %(key)s, %(key)d style substitutions
                    let mut i = 0;
                    let bytes = result.as_bytes().to_vec();
                    let mut output = String::new();
                    while i < bytes.len() {
                        if i + 1 < bytes.len() && bytes[i] == b'%' && bytes[i + 1] == b'(' {
                            if let Some(close) = bytes[i + 2..].iter().position(|&b| b == b')') {
                                let key =
                                    std::str::from_utf8(&bytes[i + 2..i + 2 + close]).unwrap_or("");
                                let spec_idx = i + 2 + close + 1;
                                if spec_idx < bytes.len() {
                                    let val = if key == "asctime" {
                                        // Compute asctime from record.created or current time
                                        current_asctime(df.as_deref())
                                    } else if let Some(attr) = record.get_attr(key) {
                                        attr.py_to_string()
                                    } else {
                                        format!("%({})s", key)
                                    };
                                    output.push_str(&val);
                                    i = spec_idx + 1;
                                    continue;
                                }
                            }
                        }
                        output.push(bytes[i] as char);
                        i += 1;
                    }
                    Ok(PyObject::str_val(CompactString::from(output)))
                }),
            );
        }
        Ok(inst)
    });
    formatter_fn
}

pub(super) fn create_handler_class() -> PyObjectRef {
    // Handler base class — proper class with __init__ for subclassing
    let handler_cls = {
        let mut ns = IndexMap::new();
        // __init__: set default level and formatter on self
        ns.insert(
            CompactString::from("__init__"),
            PyObject::native_closure("Handler.__init__", move |args: &[PyObjectRef]| {
                if let Some(self_obj) = args.first() {
                    if let PyObjectPayload::Instance(ref inst_data) = self_obj.payload {
                        let mut attrs = inst_data.attrs.write();
                        attrs.insert(CompactString::from("level"), PyObject::int(0));
                        attrs.insert(CompactString::from("formatter"), PyObject::none());
                    }
                }
                Ok(PyObject::none())
            }),
        );
        // setLevel(self, level) — class-level method
        ns.insert(
            CompactString::from("setLevel"),
            PyObject::native_closure("Handler.setLevel", move |args: &[PyObjectRef]| {
                if args.len() >= 2 {
                    if let PyObjectPayload::Instance(ref inst_data) = args[0].payload {
                        let mut attrs = inst_data.attrs.write();
                        attrs.insert(CompactString::from("level"), args[1].clone());
                    }
                }
                Ok(PyObject::none())
            }),
        );
        // setFormatter(self, fmt) — class-level method, stores on self.formatter
        ns.insert(
            CompactString::from("setFormatter"),
            PyObject::native_closure("Handler.setFormatter", move |args: &[PyObjectRef]| {
                if args.len() >= 2 {
                    if let PyObjectPayload::Instance(ref inst_data) = args[0].payload {
                        let mut attrs = inst_data.attrs.write();
                        attrs.insert(CompactString::from("formatter"), args[1].clone());
                    }
                }
                Ok(PyObject::none())
            }),
        );
        // format(self, record) — class-level method
        ns.insert(
            CompactString::from("format"),
            PyObject::native_closure("Handler.format", move |args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Ok(PyObject::str_val(CompactString::from("")));
                }
                let self_obj = &args[0];
                let record = &args[1];
                if let Some(formatter) = self_obj.get_attr("formatter") {
                    if !matches!(formatter.payload, PyObjectPayload::None) {
                        if let Some(fmt_fn) = formatter.get_attr("format") {
                            if let PyObjectPayload::NativeClosure(nc) = &fmt_fn.payload {
                                return (nc.func)(&[record.clone()]);
                            }
                        }
                    }
                }
                if let Some(msg) = record.get_attr("message") {
                    return Ok(msg);
                }
                Ok(PyObject::str_val(CompactString::from(
                    record.py_to_string(),
                )))
            }),
        );
        PyObject::class(CompactString::from("Handler"), vec![], ns)
    };
    let handler_fn = handler_cls.clone();
    handler_fn
}
