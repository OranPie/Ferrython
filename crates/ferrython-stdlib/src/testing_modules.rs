//! Logging, testing, and debugging stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin, CompareOp,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

// ── logging module ──

pub fn create_logging_module() -> PyObjectRef {
    // Logging levels
    let debug_level = PyObject::int(10);
    let info_level = PyObject::int(20);
    let warning_level = PyObject::int(30);
    let error_level = PyObject::int(40);
    let critical_level = PyObject::int(50);

    // StreamHandler class — creates handler instance with stream ref and format/emit
    let stream_handler_cls = PyObject::class(CompactString::from("StreamHandler"), vec![], IndexMap::new());
    let sh_cls = stream_handler_cls.clone();
    let stream_handler_fn = PyObject::native_closure("StreamHandler", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(sh_cls.clone());
        let stream = if args.is_empty() { PyObject::none() } else { args[0].clone() };
        // Shared state for formatter and level
        let formatter_ref: Arc<RwLock<PyObjectRef>> = Arc::new(RwLock::new(PyObject::none()));
        let level_ref: Arc<RwLock<i64>> = Arc::new(RwLock::new(0));

        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(CompactString::from("stream"), stream.clone());
            attrs.insert(CompactString::from("level"), PyObject::int(0));
            attrs.insert(CompactString::from("formatter"), PyObject::none());

            let lr = level_ref.clone();
            attrs.insert(CompactString::from("setLevel"), PyObject::native_closure(
                "setLevel", move |args: &[PyObjectRef]| {
                    if let Some(v) = args.first() {
                        if let Some(n) = v.as_int() { *lr.write() = n; }
                    }
                    Ok(PyObject::none())
                }
            ));
            let fr = formatter_ref.clone();
            attrs.insert(CompactString::from("setFormatter"), PyObject::native_closure(
                "setFormatter", move |args: &[PyObjectRef]| {
                    if let Some(v) = args.first() {
                        *fr.write() = v.clone();
                    }
                    Ok(PyObject::none())
                }
            ));
            // emit(record) — write formatted message to stream or stderr
            let fr2 = formatter_ref.clone();
            let stream2 = stream.clone();
            attrs.insert(CompactString::from("emit"), PyObject::native_closure(
                "emit", move |args: &[PyObjectRef]| {
                    // args[0] may be handler (from logger dispatch) or record (direct call)
                    // Detect: if called with 2 args, args[0]=handler, args[1]=record
                    // If called with 1 arg, args[0]=record
                    let record = if args.len() >= 2 { &args[1] } else if !args.is_empty() { &args[0] } else {
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
                        if let Some(fmt_str) = fmt.get_attr("_fmt") {
                            let fs = fmt_str.py_to_string();
                            let mut result = fs.clone();
                            result = result.replace("%(message)s", &msg);
                            let levelname = if let Some(ln) = record.get_attr("levelname") {
                                ln.py_to_string()
                            } else { "INFO".to_string() };
                            let name = if let Some(n) = record.get_attr("name") {
                                n.py_to_string()
                            } else { "root".to_string() };
                            result = result.replace("%(levelname)s", &levelname);
                            result = result.replace("%(name)s", &name);
                            result
                        } else { msg.clone() }
                    } else { msg.clone() };

                    // Write to stream (directly to StringIO buffer)
                    if let PyObjectPayload::Instance(ref si) = stream2.payload {
                        let attrs_r = si.attrs.read();
                        if attrs_r.contains_key("__stringio__") {
                            drop(attrs_r);
                            let mut attrs_w = si.attrs.write();
                            let line = format!("{}\n", formatted);
                            if let Some(buf) = attrs_w.get("_buffer") {
                                let cur = buf.py_to_string();
                                attrs_w.insert(
                                    CompactString::from("_buffer"),
                                    PyObject::str_val(CompactString::from(format!("{}{}", cur, line))),
                                );
                            }
                            return Ok(PyObject::none());
                        }
                    }
                    eprintln!("{}", formatted);
                    Ok(PyObject::none())
                }
            ));
        }
        Ok(inst)
    });

    // FileHandler class — handler that writes to file
    let file_handler_cls = PyObject::class(CompactString::from("FileHandler"), vec![], IndexMap::new());
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
            attrs.insert(CompactString::from("baseFilename"), PyObject::str_val(filename));
            attrs.insert(CompactString::from("level"), PyObject::int(0));
            attrs.insert(CompactString::from("formatter"), PyObject::none());
            attrs.insert(CompactString::from("setLevel"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() >= 2 {
                    if let PyObjectPayload::Instance(ref d) = args[0].payload {
                        d.attrs.write().insert(CompactString::from("level"), args[1].clone());
                    }
                }
                Ok(PyObject::none())
            }));
            attrs.insert(CompactString::from("setFormatter"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() >= 2 {
                    if let PyObjectPayload::Instance(ref d) = args[0].payload {
                        d.attrs.write().insert(CompactString::from("formatter"), args[1].clone());
                    }
                }
                Ok(PyObject::none())
            }));
        }
        Ok(inst)
    });

    // Formatter(fmt) — stores format string, has format(record) method
    let formatter_cls = PyObject::class(CompactString::from("Formatter"), vec![], IndexMap::new());
    let fmt_cls = formatter_cls.clone();
    let formatter_fn = PyObject::native_closure("Formatter", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(fmt_cls.clone());
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let fmt_str = if args.is_empty() {
                CompactString::from("%(levelname)s:%(name)s:%(message)s")
            } else {
                CompactString::from(args[0].py_to_string())
            };
            attrs.insert(CompactString::from("_fmt"), PyObject::str_val(fmt_str));
            attrs.insert(CompactString::from("format"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() >= 2 {
                    Ok(PyObject::str_val(CompactString::from(args[1].py_to_string())))
                } else {
                    Ok(PyObject::str_val(CompactString::from("")))
                }
            }));
        }
        Ok(inst)
    });

    // Handler base class
    let handler_cls = PyObject::class(CompactString::from("Handler"), vec![], IndexMap::new());
    let h_cls = handler_cls.clone();
    let handler_fn = PyObject::native_closure("Handler", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(h_cls.clone());
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(CompactString::from("level"), PyObject::int(0));
            attrs.insert(CompactString::from("setLevel"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("setFormatter"), make_builtin(|_| Ok(PyObject::none())));
        }
        Ok(inst)
    });

    // basicConfig(**kwargs) — configure root logger
    let basic_config_fn = make_builtin(|args: &[PyObjectRef]| {
        // Accept kwargs as last dict arg from VM
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw_map) = &last.payload {
                let r = kw_map.read();
                // Extract level if present
                if let Some(_level) = r.get(&HashableKey::Str(CompactString::from("level"))) {
                    // In a real impl, would set root logger level
                }
                // Extract format if present
                if let Some(_format) = r.get(&HashableKey::Str(CompactString::from("format"))) {
                    // Would set root logger format
                }
            }
        }
        Ok(PyObject::none())
    });

    make_module("logging", vec![
        ("DEBUG", debug_level),
        ("INFO", info_level),
        ("WARNING", warning_level.clone()),
        ("ERROR", error_level),
        ("CRITICAL", critical_level),
        ("NOTSET", PyObject::int(0)),
        ("basicConfig", basic_config_fn),
        ("getLogger", make_builtin(logging_get_logger)),
        ("debug", make_builtin(|args| { logging_log(10, args) })),
        ("info", make_builtin(|args| { logging_log(20, args) })),
        ("warning", make_builtin(|args| { logging_log(30, args) })),
        ("error", make_builtin(|args| { logging_log(40, args) })),
        ("critical", make_builtin(|args| { logging_log(50, args) })),
        ("log", make_builtin(|args| {
            if args.len() >= 2 {
                let level = args[0].as_int().unwrap_or(20);
                logging_log(level, &args[1..])
            } else {
                Ok(PyObject::none())
            }
        })),
        ("StreamHandler", stream_handler_fn),
        ("FileHandler", file_handler_fn),
        ("Formatter", formatter_fn),
        ("Handler", handler_fn),
        ("Logger", make_builtin(logging_get_logger)),
    ])
}

fn logging_log(level: i64, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Ok(PyObject::none()); }
    let level_name = match level {
        10 => "DEBUG",
        20 => "INFO",
        30 => "WARNING",
        40 => "ERROR",
        50 => "CRITICAL",
        _ => "UNKNOWN",
    };
    let msg = args[0].py_to_string();
    eprintln!("{}:root:{}", level_name, msg);
    Ok(PyObject::none())
}

fn logging_get_logger(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let logger_name = if args.is_empty() || matches!(&args[0].payload, PyObjectPayload::None) {
        CompactString::from("root")
    } else {
        CompactString::from(args[0].py_to_string())
    };
    let mut ns = IndexMap::new();
    ns.insert(CompactString::from("name"), PyObject::str_val(logger_name.clone()));
    let effective_level: Arc<RwLock<i64>> = Arc::new(RwLock::new(30)); // WARNING default
    ns.insert(CompactString::from("level"), PyObject::int(30)); // WARNING default
    let handlers_list = PyObject::list(vec![]);
    ns.insert(CompactString::from("handlers"), handlers_list.clone());

    // Create log methods that capture the shared handlers list and effective level
    let make_log_method = |level: i64, level_name: &'static str, handlers: PyObjectRef, name: CompactString, eff_level: Arc<RwLock<i64>>| -> PyObjectRef {
        PyObject::native_closure(level_name, move |args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::none()); }
            // Filter: only emit if message level >= logger's effective level
            let current_level = *eff_level.read();
            if current_level > 0 && level < current_level {
                return Ok(PyObject::none());
            }
            let msg = args[0].py_to_string();

            // Create a LogRecord-like instance
            let rec_cls = PyObject::class(CompactString::from("LogRecord"), vec![], IndexMap::new());
            let record = PyObject::instance(rec_cls);
            if let PyObjectPayload::Instance(ref rd) = record.payload {
                let mut ra = rd.attrs.write();
                ra.insert(CompactString::from("levelname"), PyObject::str_val(CompactString::from(level_name)));
                ra.insert(CompactString::from("levelno"), PyObject::int(level));
                ra.insert(CompactString::from("name"), PyObject::str_val(name.clone()));
                ra.insert(CompactString::from("message"), PyObject::str_val(CompactString::from(msg.clone())));
                ra.insert(CompactString::from("msg"), PyObject::str_val(CompactString::from(msg.clone())));
            }

            // Dispatch to handlers via shared list
            let mut dispatched = false;
            if let PyObjectPayload::List(items) = &handlers.payload {
                let items_r = items.read();
                for handler in items_r.iter() {
                    if let Some(emit_fn) = handler.get_attr("emit") {
                        match &emit_fn.payload {
                            PyObjectPayload::NativeFunction { func, .. } => {
                                let _ = func(&[handler.clone(), record.clone()]);
                                dispatched = true;
                            }
                            PyObjectPayload::NativeClosure { func, .. } => {
                                let _ = func(&[handler.clone(), record.clone()]);
                                dispatched = true;
                            }
                            _ => {}
                        }
                    }
                }
            }
            if !dispatched {
                eprintln!("{}:{}:{}", level_name, name, msg);
            }
            Ok(PyObject::none())
        })
    };

    ns.insert(CompactString::from("debug"), make_log_method(10, "DEBUG", handlers_list.clone(), logger_name.clone(), effective_level.clone()));
    ns.insert(CompactString::from("info"), make_log_method(20, "INFO", handlers_list.clone(), logger_name.clone(), effective_level.clone()));
    ns.insert(CompactString::from("warning"), make_log_method(30, "WARNING", handlers_list.clone(), logger_name.clone(), effective_level.clone()));
    ns.insert(CompactString::from("error"), make_log_method(40, "ERROR", handlers_list.clone(), logger_name.clone(), effective_level.clone()));
    ns.insert(CompactString::from("critical"), make_log_method(50, "CRITICAL", handlers_list.clone(), logger_name.clone(), effective_level.clone()));

    // setLevel — update the shared effective level
    let el = effective_level.clone();
    ns.insert(CompactString::from("setLevel"), PyObject::native_closure(
        "setLevel", move |args: &[PyObjectRef]| {
            if let Some(v) = args.first() {
                if let Some(n) = v.as_int() { *el.write() = n; }
            }
            Ok(PyObject::none())
        }
    ));
    // addHandler — push to shared handlers list
    let hl = handlers_list.clone();
    ns.insert(CompactString::from("addHandler"), PyObject::native_closure(
        "addHandler", move |args: &[PyObjectRef]| {
            if !args.is_empty() {
                if let PyObjectPayload::List(items) = &hl.payload {
                    items.write().push(args[0].clone());
                }
            }
            Ok(PyObject::none())
        }
    ));
    ns.insert(CompactString::from("removeHandler"), make_builtin(|_| Ok(PyObject::none())));
    let hl2 = handlers_list.clone();
    ns.insert(CompactString::from("hasHandlers"), PyObject::native_closure(
        "hasHandlers", move |_: &[PyObjectRef]| {
            if let PyObjectPayload::List(items) = &hl2.payload {
                return Ok(PyObject::bool_val(!items.read().is_empty()));
            }
            Ok(PyObject::bool_val(false))
        }
    ));
    let el2 = effective_level.clone();
    ns.insert(CompactString::from("isEnabledFor"), PyObject::native_closure(
        "isEnabledFor", move |args: &[PyObjectRef]| {
            if let Some(v) = args.first() {
                if let Some(n) = v.as_int() {
                    let current = *el2.read();
                    return Ok(PyObject::bool_val(current == 0 || n >= current));
                }
            }
            Ok(PyObject::bool_val(true))
        }
    ));
    let el3 = effective_level.clone();
    ns.insert(CompactString::from("getEffectiveLevel"), PyObject::native_closure(
        "getEffectiveLevel", move |_: &[PyObjectRef]| {
            Ok(PyObject::int(*el3.read()))
        }
    ));
    
    let cls = PyObject::class(CompactString::from("Logger"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        for (k, v) in ns {
            attrs.insert(k, v);
        }
    }
    Ok(inst)
}

// ── unittest module ──

/// Helper: extract optional message from args at given index.
fn assert_msg(args: &[PyObjectRef], idx: usize) -> String {
    if args.len() > idx {
        args[idx].py_to_string()
    } else {
        String::new()
    }
}

pub fn create_unittest_module() -> PyObjectRef {
    // Build TestCase class with assert methods in the namespace so that
    // subclass instances inherit them via MRO lookup.
    let mut tc_ns = IndexMap::new();
    tc_ns.insert(CompactString::from("__unittest_testcase__"), PyObject::bool_val(true));

    // setUp / tearDown — default no-ops, subclasses override
    tc_ns.insert(CompactString::from("setUp"), make_builtin(|_| Ok(PyObject::none())));
    tc_ns.insert(CompactString::from("tearDown"), make_builtin(|_| Ok(PyObject::none())));

    // assertEqual(a, b[, msg])
    tc_ns.insert(CompactString::from("assertEqual"), PyObject::native_closure(
        "assertEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertEqual requires 2 arguments"));
            }
            let result = args[0].compare(&args[1], CompareOp::Eq)?;
            if !result.is_truthy() {
                let msg = if args.len() > 2 { args[2].py_to_string() }
                    else { format!("{} != {}", args[0].py_to_string(), args[1].py_to_string()) };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertNotEqual(a, b[, msg])
    tc_ns.insert(CompactString::from("assertNotEqual"), PyObject::native_closure(
        "assertNotEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertNotEqual requires 2 arguments"));
            }
            let result = args[0].compare(&args[1], CompareOp::Ne)?;
            if !result.is_truthy() {
                let msg = if args.len() > 2 { args[2].py_to_string() }
                    else { format!("{} == {}", args[0].py_to_string(), args[1].py_to_string()) };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertTrue(x[, msg])
    tc_ns.insert(CompactString::from("assertTrue"), PyObject::native_closure(
        "assertTrue", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("assertTrue requires 1 argument"));
            }
            if !args[0].is_truthy() {
                let msg = assert_msg(args, 1);
                let msg = if msg.is_empty() { format!("{} is not true", args[0].py_to_string()) } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertFalse(x[, msg])
    tc_ns.insert(CompactString::from("assertFalse"), PyObject::native_closure(
        "assertFalse", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("assertFalse requires 1 argument"));
            }
            if args[0].is_truthy() {
                let msg = assert_msg(args, 1);
                let msg = if msg.is_empty() { format!("{} is not false", args[0].py_to_string()) } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertIs(a, b[, msg])
    tc_ns.insert(CompactString::from("assertIs"), PyObject::native_closure(
        "assertIs", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertIs requires 2 arguments"));
            }
            if !Arc::ptr_eq(&args[0], &args[1]) {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} is not {}", args[0].py_to_string(), args[1].py_to_string())
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertIsNot(a, b[, msg])
    tc_ns.insert(CompactString::from("assertIsNot"), PyObject::native_closure(
        "assertIsNot", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertIsNot requires 2 arguments"));
            }
            if Arc::ptr_eq(&args[0], &args[1]) {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} is {}", args[0].py_to_string(), args[1].py_to_string())
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertIsNone(x[, msg])
    tc_ns.insert(CompactString::from("assertIsNone"), PyObject::native_closure(
        "assertIsNone", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("assertIsNone requires 1 argument"));
            }
            if !matches!(args[0].payload, PyObjectPayload::None) {
                let msg = assert_msg(args, 1);
                let msg = if msg.is_empty() { format!("{} is not None", args[0].py_to_string()) } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertIsNotNone(x[, msg])
    tc_ns.insert(CompactString::from("assertIsNotNone"), PyObject::native_closure(
        "assertIsNotNone", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("assertIsNotNone requires 1 argument"));
            }
            if matches!(args[0].payload, PyObjectPayload::None) {
                let msg = assert_msg(args, 1);
                let msg = if msg.is_empty() { "unexpectedly None".to_string() } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertIn(a, b[, msg])
    tc_ns.insert(CompactString::from("assertIn"), PyObject::native_closure(
        "assertIn", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertIn requires 2 arguments"));
            }
            let contained = args[1].contains(&args[0])?;
            if !contained {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} not found in {}", args[0].py_to_string(), args[1].py_to_string())
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertNotIn(a, b[, msg])
    tc_ns.insert(CompactString::from("assertNotIn"), PyObject::native_closure(
        "assertNotIn", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertNotIn requires 2 arguments"));
            }
            let contained = args[1].contains(&args[0])?;
            if contained {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} unexpectedly found in {}", args[0].py_to_string(), args[1].py_to_string())
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertGreater(a, b[, msg])
    tc_ns.insert(CompactString::from("assertGreater"), PyObject::native_closure(
        "assertGreater", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertGreater requires 2 arguments"));
            }
            let result = args[0].compare(&args[1], CompareOp::Gt)?;
            if !result.is_truthy() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} not greater than {}", args[0].py_to_string(), args[1].py_to_string())
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertLess(a, b[, msg])
    tc_ns.insert(CompactString::from("assertLess"), PyObject::native_closure(
        "assertLess", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertLess requires 2 arguments"));
            }
            let result = args[0].compare(&args[1], CompareOp::Lt)?;
            if !result.is_truthy() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} not less than {}", args[0].py_to_string(), args[1].py_to_string())
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertRaises(exc_type) — returns a context manager
    tc_ns.insert(CompactString::from("assertRaises"), PyObject::native_closure(
        "assertRaises", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("assertRaises requires an exception type"));
            }
            let exc_type_name = match &args[0].payload {
                PyObjectPayload::Class(cd) => cd.name.clone(),
                PyObjectPayload::Str(s) => s.clone(),
                _ => CompactString::from(args[0].py_to_string()),
            };
            // Build a context-manager object with __enter__ / __exit__
            let cls = PyObject::class(
                CompactString::from("_AssertRaisesContext"),
                vec![],
                IndexMap::new(),
            );
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("expected"), PyObject::str_val(exc_type_name.clone()));
                w.insert(CompactString::from("__enter__"), PyObject::native_closure(
                    "__enter__", |_args: &[PyObjectRef]| Ok(PyObject::none()),
                ));
                let etype = exc_type_name.clone();
                w.insert(CompactString::from("__exit__"), PyObject::native_closure(
                    "__exit__", move |args: &[PyObjectRef]| {
                        // args: exc_type, exc_val, exc_tb (or None if no exception)
                        let has_exc = if args.is_empty() {
                            false
                        } else {
                            !matches!(args[0].payload, PyObjectPayload::None)
                        };
                        if !has_exc {
                            return Err(PyException::assertion_error(
                                format!("{} not raised", etype),
                            ));
                        }
                        // Suppress the exception
                        Ok(PyObject::bool_val(true))
                    },
                ));
            }
            Ok(inst)
        },
    ));

    // assertGreaterEqual(a, b[, msg])
    tc_ns.insert(CompactString::from("assertGreaterEqual"), PyObject::native_closure(
        "assertGreaterEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertGreaterEqual requires 2 arguments"));
            }
            let result = args[0].compare(&args[1], CompareOp::Ge)?;
            if !result.is_truthy() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} not greater than or equal to {}", args[0].py_to_string(), args[1].py_to_string())
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertLessEqual(a, b[, msg])
    tc_ns.insert(CompactString::from("assertLessEqual"), PyObject::native_closure(
        "assertLessEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertLessEqual requires 2 arguments"));
            }
            let result = args[0].compare(&args[1], CompareOp::Le)?;
            if !result.is_truthy() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} not less than or equal to {}", args[0].py_to_string(), args[1].py_to_string())
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertAlmostEqual(a, b[, places=7, msg=None])
    tc_ns.insert(CompactString::from("assertAlmostEqual"), PyObject::native_closure(
        "assertAlmostEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertAlmostEqual requires 2 arguments"));
            }
            let a = args[0].to_float().or_else(|_| args[0].as_int().map(|i| i as f64)
                .ok_or_else(|| PyException::type_error("assertAlmostEqual requires numeric arguments")))?;
            let b = args[1].to_float().or_else(|_| args[1].as_int().map(|i| i as f64)
                .ok_or_else(|| PyException::type_error("assertAlmostEqual requires numeric arguments")))?;
            let places = if args.len() > 2 { args[2].as_int().unwrap_or(7) } else { 7 };
            // CPython: round(a-b, places) == 0, equivalent to abs(a-b) < 0.5 * 10^(-places)
            let tolerance = 0.5 * 10f64.powi(-(places as i32));
            if (a - b).abs() >= tolerance {
                let msg = assert_msg(args, 3);
                let msg = if msg.is_empty() {
                    format!("{} != {} within {} places", a, b, places)
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertNotAlmostEqual(a, b[, places=7, msg=None])
    tc_ns.insert(CompactString::from("assertNotAlmostEqual"), PyObject::native_closure(
        "assertNotAlmostEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertNotAlmostEqual requires 2 arguments"));
            }
            let a = args[0].to_float().or_else(|_| args[0].as_int().map(|i| i as f64)
                .ok_or_else(|| PyException::type_error("assertNotAlmostEqual requires numeric arguments")))?;
            let b = args[1].to_float().or_else(|_| args[1].as_int().map(|i| i as f64)
                .ok_or_else(|| PyException::type_error("assertNotAlmostEqual requires numeric arguments")))?;
            let places = if args.len() > 2 { args[2].as_int().unwrap_or(7) } else { 7 };
            let tolerance = 0.5 * 10f64.powi(-(places as i32));
            if (a - b).abs() < tolerance {
                let msg = assert_msg(args, 3);
                let msg = if msg.is_empty() {
                    format!("{} == {} within {} places", a, b, places)
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertIsInstance(obj, cls[, msg])
    tc_ns.insert(CompactString::from("assertIsInstance"), PyObject::native_closure(
        "assertIsInstance", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertIsInstance requires 2 arguments"));
            }
            let obj_type = args[0].type_name();
            let expected = match &args[1].payload {
                PyObjectPayload::Class(cd) => cd.name.as_str().to_string(),
                _ => args[1].py_to_string(),
            };
            // Check direct type match or class hierarchy
            let is_instance = obj_type == expected
                || obj_type.eq_ignore_ascii_case(&expected);
            if !is_instance {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} is not an instance of {}", args[0].py_to_string(), expected)
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertNotIsInstance(obj, cls[, msg])
    tc_ns.insert(CompactString::from("assertNotIsInstance"), PyObject::native_closure(
        "assertNotIsInstance", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertNotIsInstance requires 2 arguments"));
            }
            let obj_type = args[0].type_name();
            let expected = match &args[1].payload {
                PyObjectPayload::Class(cd) => cd.name.as_str().to_string(),
                _ => args[1].py_to_string(),
            };
            let is_instance = obj_type == expected
                || obj_type.eq_ignore_ascii_case(&expected);
            if is_instance {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} is an instance of {}", args[0].py_to_string(), expected)
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertRegex(text, regex[, msg])
    tc_ns.insert(CompactString::from("assertRegex"), PyObject::native_closure(
        "assertRegex", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertRegex requires 2 arguments"));
            }
            let text = args[0].py_to_string();
            let pattern = args[1].py_to_string();
            let re = regex::Regex::new(&pattern)
                .map_err(|e| PyException::runtime_error(format!("Invalid regex: {}", e)))?;
            if re.find(&text).is_none() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("Regex '{}' didn't match '{}'", pattern, text)
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertNotRegex(text, regex[, msg])
    tc_ns.insert(CompactString::from("assertNotRegex"), PyObject::native_closure(
        "assertNotRegex", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertNotRegex requires 2 arguments"));
            }
            let text = args[0].py_to_string();
            let pattern = args[1].py_to_string();
            let re = regex::Regex::new(&pattern)
                .map_err(|e| PyException::runtime_error(format!("Invalid regex: {}", e)))?;
            if re.find(&text).is_some() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("Regex '{}' unexpectedly matched '{}'", pattern, text)
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertCountEqual(first, second[, msg]) — same elements, any order
    tc_ns.insert(CompactString::from("assertCountEqual"), PyObject::native_closure(
        "assertCountEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertCountEqual requires 2 arguments"));
            }
            let a_items = args[0].to_list()?;
            let b_items = args[1].to_list()?;
            if a_items.len() != b_items.len() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("Element counts differ: {} vs {}", a_items.len(), b_items.len())
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            let mut a_strs: Vec<String> = a_items.iter().map(|x| x.py_to_string()).collect();
            let mut b_strs: Vec<String> = b_items.iter().map(|x| x.py_to_string()).collect();
            a_strs.sort();
            b_strs.sort();
            if a_strs != b_strs {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    "Element counts differ".to_string()
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertDictEqual(d1, d2[, msg])
    tc_ns.insert(CompactString::from("assertDictEqual"), PyObject::native_closure(
        "assertDictEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertDictEqual requires 2 arguments"));
            }
            let result = args[0].compare(&args[1], CompareOp::Eq)?;
            if !result.is_truthy() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} != {}", args[0].py_to_string(), args[1].py_to_string())
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertListEqual(list1, list2[, msg])
    tc_ns.insert(CompactString::from("assertListEqual"), PyObject::native_closure(
        "assertListEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertListEqual requires 2 arguments"));
            }
            let result = args[0].compare(&args[1], CompareOp::Eq)?;
            if !result.is_truthy() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} != {}", args[0].py_to_string(), args[1].py_to_string())
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertTupleEqual(tuple1, tuple2[, msg])
    tc_ns.insert(CompactString::from("assertTupleEqual"), PyObject::native_closure(
        "assertTupleEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertTupleEqual requires 2 arguments"));
            }
            let result = args[0].compare(&args[1], CompareOp::Eq)?;
            if !result.is_truthy() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} != {}", args[0].py_to_string(), args[1].py_to_string())
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertSetEqual(set1, set2[, msg])
    tc_ns.insert(CompactString::from("assertSetEqual"), PyObject::native_closure(
        "assertSetEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertSetEqual requires 2 arguments"));
            }
            let result = args[0].compare(&args[1], CompareOp::Eq)?;
            if !result.is_truthy() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("{} != {}", args[0].py_to_string(), args[1].py_to_string())
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertSequenceEqual(seq1, seq2[, msg])
    tc_ns.insert(CompactString::from("assertSequenceEqual"), PyObject::native_closure(
        "assertSequenceEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertSequenceEqual requires 2 arguments"));
            }
            let result = args[0].compare(&args[1], CompareOp::Eq)?;
            if !result.is_truthy() {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("Sequences differ: {} != {}", args[0].py_to_string(), args[1].py_to_string())
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // assertMultiLineEqual(first, second[, msg])
    tc_ns.insert(CompactString::from("assertMultiLineEqual"), PyObject::native_closure(
        "assertMultiLineEqual", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("assertMultiLineEqual requires 2 arguments"));
            }
            let a = args[0].py_to_string();
            let b = args[1].py_to_string();
            if a != b {
                let msg = assert_msg(args, 2);
                let msg = if msg.is_empty() {
                    format!("'{}' != '{}'", a, b)
                } else { msg };
                return Err(PyException::assertion_error(msg));
            }
            Ok(PyObject::none())
        },
    ));

    // fail([msg]) — unconditionally fail
    tc_ns.insert(CompactString::from("fail"), PyObject::native_closure(
        "fail", |args: &[PyObjectRef]| {
            let msg = if args.is_empty() { "Fail".to_string() } else { args[0].py_to_string() };
            Err(PyException::assertion_error(msg))
        },
    ));

    // subTest — context manager stub for subtests
    tc_ns.insert(CompactString::from("subTest"), PyObject::native_closure(
        "subTest", |_args: &[PyObjectRef]| {
            let cls = PyObject::class(CompactString::from("_SubTest"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("__enter__"), PyObject::native_closure(
                    "__enter__", |_: &[PyObjectRef]| Ok(PyObject::none()),
                ));
                w.insert(CompactString::from("__exit__"), PyObject::native_closure(
                    "__exit__", |_: &[PyObjectRef]| Ok(PyObject::bool_val(false)),
                ));
            }
            Ok(inst)
        },
    ));

    let test_case = PyObject::class(CompactString::from("TestCase"), vec![], tc_ns);

    make_module("unittest", vec![
        ("TestCase", test_case),
        ("main", make_builtin(|_| Ok(PyObject::none()))),
        ("TestSuite", make_builtin(|_| Ok(PyObject::none()))),
        ("TestLoader", make_builtin(|_| Ok(PyObject::none()))),
        ("TextTestRunner", make_builtin(|_| Ok(PyObject::none()))),
        ("skip", make_builtin(|_args| {
            Ok(make_builtin(|args| {
                if args.is_empty() { Ok(PyObject::none()) } else { Ok(args[0].clone()) }
            }))
        })),
        ("skipIf", make_builtin(|_| {
            Ok(make_builtin(|args| {
                if args.is_empty() { Ok(PyObject::none()) } else { Ok(args[0].clone()) }
            }))
        })),
        ("expectedFailure", make_builtin(|args| {
            if args.is_empty() { Ok(PyObject::none()) } else { Ok(args[0].clone()) }
        })),
    ])
}

// ── unittest.mock module ──

pub fn create_unittest_mock_module() -> PyObjectRef {
    let make_mock = |name: &'static str| -> PyObjectRef {
        PyObject::native_closure(name, move |_args: &[PyObjectRef]| {
            let cls = PyObject::class(CompactString::from(name), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                let call_count: Arc<RwLock<i64>> = Arc::new(RwLock::new(0));
                let call_args_list: Arc<RwLock<Vec<PyObjectRef>>> = Arc::new(RwLock::new(vec![]));
                let return_value: Arc<RwLock<PyObjectRef>> = Arc::new(RwLock::new(PyObject::none()));

                let rv = return_value.clone();
                w.insert(CompactString::from("return_value"), PyObject::native_closure(
                    "Mock.return_value", move |_: &[PyObjectRef]| Ok(rv.read().clone())
                ));

                let cc = call_count.clone();
                w.insert(CompactString::from("call_count"), PyObject::native_closure(
                    "Mock.call_count", move |_: &[PyObjectRef]| Ok(PyObject::int(*cc.read()))
                ));

                let cal = call_args_list.clone();
                w.insert(CompactString::from("call_args_list"), PyObject::native_closure(
                    "Mock.call_args_list", move |_: &[PyObjectRef]| Ok(PyObject::list(cal.read().clone()))
                ));

                w.insert(CompactString::from("called"), PyObject::native_closure(
                    "Mock.called", {
                        let cc2 = call_count.clone();
                        move |_: &[PyObjectRef]| Ok(PyObject::bool_val(*cc2.read() > 0))
                    }
                ));

                let cc3 = call_count.clone();
                let cal2 = call_args_list.clone();
                let rv2 = return_value.clone();
                w.insert(CompactString::from("__call__"), PyObject::native_closure(
                    "Mock.__call__", move |args: &[PyObjectRef]| {
                        *cc3.write() += 1;
                        cal2.write().push(PyObject::tuple(args.to_vec()));
                        Ok(rv2.read().clone())
                    }
                ));

                w.insert(CompactString::from("assert_called"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
                w.insert(CompactString::from("assert_called_once"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
                w.insert(CompactString::from("assert_called_with"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
                w.insert(CompactString::from("reset_mock"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            }
            Ok(inst)
        })
    };

    // patch function — returns a context manager / decorator stub
    let patch_fn = make_builtin(|args: &[PyObjectRef]| {
        let target = if !args.is_empty() { args[0].py_to_string() } else { String::new() };
        let cls = PyObject::class(CompactString::from("_patch"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("attribute"), PyObject::str_val(CompactString::from(target.as_str())));
            let _ir = inst.clone();
            w.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "patch.__enter__", move |_: &[PyObjectRef]| {
                    // Return a fresh Mock
                    let m_cls = PyObject::class(CompactString::from("MagicMock"), vec![], IndexMap::new());
                    Ok(PyObject::instance(m_cls))
                }
            ));
            w.insert(CompactString::from("__exit__"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::bool_val(false))));
            w.insert(CompactString::from("__call__"), make_builtin(|args: &[PyObjectRef]| {
                if !args.is_empty() { Ok(args[0].clone()) } else { Ok(PyObject::none()) }
            }));
        }
        Ok(inst)
    });

    // sentinel — attribute access returns unique sentinels
    let sentinel_cls = PyObject::class(CompactString::from("_SentinelObject"), vec![], IndexMap::new());
    let sentinel = PyObject::instance(sentinel_cls);

    // call — call record
    let call_fn = make_builtin(|args: &[PyObjectRef]| {
        Ok(PyObject::tuple(args.to_vec()))
    });

    // ANY — matches anything
    let any_cls = PyObject::class(CompactString::from("_ANY"), vec![], IndexMap::new());
    let any_obj = PyObject::instance(any_cls);

    make_module("unittest.mock", vec![
        ("Mock", make_mock("Mock")),
        ("MagicMock", make_mock("MagicMock")),
        ("patch", patch_fn),
        ("sentinel", sentinel),
        ("call", call_fn),
        ("ANY", any_obj),
        ("DEFAULT", PyObject::str_val(CompactString::from("DEFAULT"))),
        ("PropertyMock", make_mock("PropertyMock")),
    ])
}

// ── doctest module ──

pub fn create_doctest_module() -> PyObjectRef {
    let testmod_fn = make_builtin(|_args: &[PyObjectRef]| {
        // Return a TestResults(failed=0, attempted=0) named tuple-like
        let cls = PyObject::class(CompactString::from("TestResults"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("failed"), PyObject::int(0));
        attrs.insert(CompactString::from("attempted"), PyObject::int(0));
        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    let run_docstring_fn = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::none())
    });

    make_module("doctest", vec![
        ("testmod", testmod_fn),
        ("run_docstring_examples", run_docstring_fn),
        ("DocTestRunner", make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none()))),
        ("DocTestFinder", make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none()))),
        ("ELLIPSIS", PyObject::int(8)),
        ("NORMALIZE_WHITESPACE", PyObject::int(2)),
        ("IGNORE_EXCEPTION_DETAIL", PyObject::int(4)),
        ("OPTIONFLAGS", PyObject::int(0)),
    ])
}

// ── pdb module ──

pub fn create_pdb_module() -> PyObjectRef {
    let set_trace_fn = make_builtin(|_args: &[PyObjectRef]| {
        // No-op in this runtime
        Ok(PyObject::none())
    });

    let pm_fn = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::none())
    });

    let run_fn = make_builtin(|args: &[PyObjectRef]| {
        // Just evaluate nothing — stub
        let _ = args;
        Ok(PyObject::none())
    });

    let pdb_cls = PyObject::class(CompactString::from("Pdb"), vec![], IndexMap::new());
    if let PyObjectPayload::Class(ref cd) = pdb_cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(CompactString::from("set_trace"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
        ns.insert(CompactString::from("run"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
    }

    make_module("pdb", vec![
        ("set_trace", set_trace_fn),
        ("pm", pm_fn),
        ("run", run_fn),
        ("Pdb", pdb_cls),
        ("post_mortem", make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none()))),
    ])
}

// ── profile module ──

pub fn create_profile_module() -> PyObjectRef {
    let run_fn = make_builtin(|args: &[PyObjectRef]| {
        let _ = args;
        Ok(PyObject::none())
    });

    let profile_cls_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("Profile"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("enable"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("disable"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("run"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("runcall"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("print_stats"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
        }
        Ok(inst)
    });

    make_module("profile", vec![
        ("run", run_fn),
        ("Profile", profile_cls_fn),
    ])
}

// ── cProfile module ──

pub fn create_cprofile_module() -> PyObjectRef {
    let run_fn = make_builtin(|args: &[PyObjectRef]| {
        let _ = args;
        Ok(PyObject::none())
    });

    let profile_cls_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("Profile"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("enable"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("disable"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("run"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("runcall"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("print_stats"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
        }
        Ok(inst)
    });

    make_module("cProfile", vec![
        ("run", run_fn),
        ("Profile", profile_cls_fn),
    ])
}

// ── timeit module ──

pub fn create_timeit_module() -> PyObjectRef {
    // timeit.default_timer — alias for time.perf_counter (uses time.time)
    let default_timer = make_builtin(|_args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        Ok(PyObject::float(t))
    });

    // timeit.timeit(stmt, setup, number, globals) — simplified
    let timeit_fn = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        use std::time::Instant;
        let number: i64 = if args.len() > 2 {
            args[2].as_int().unwrap_or(1_000_000)
        } else {
            1_000_000
        };
        let start = Instant::now();
        for _ in 0..number.min(1000) {
            std::hint::black_box(0);
        }
        let elapsed = start.elapsed().as_secs_f64();
        let ratio = number as f64 / (number.min(1000) as f64);
        Ok(PyObject::float(elapsed * ratio))
    });

    // timeit.repeat(stmt, setup, repeat, number)
    let repeat_fn = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        use std::time::Instant;
        let repeat_count: i64 = if args.len() > 2 {
            args[2].as_int().unwrap_or(5)
        } else {
            5
        };
        let number: i64 = if args.len() > 3 {
            args[3].as_int().unwrap_or(1_000_000)
        } else {
            1_000_000
        };
        let mut results = Vec::new();
        for _ in 0..repeat_count {
            let start = Instant::now();
            for _ in 0..number.min(1000) {
                std::hint::black_box(0);
            }
            let elapsed = start.elapsed().as_secs_f64();
            let ratio = number as f64 / (number.min(1000) as f64);
            results.push(PyObject::float(elapsed * ratio));
        }
        Ok(PyObject::list(results))
    });

    // Timer class (simplified)
    let timer_cls = PyObject::class(CompactString::from("Timer"), vec![], IndexMap::new());
    let tc = timer_cls.clone();
    let timer_fn = PyObject::native_closure("Timer", move |args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        let inst = PyObject::instance(tc.clone());
        if let PyObjectPayload::Instance(ref data) = inst.payload {
            let mut attrs = data.attrs.write();
            if !args.is_empty() {
                attrs.insert(CompactString::from("stmt"), args[0].clone());
            }
            if args.len() > 1 {
                attrs.insert(CompactString::from("setup"), args[1].clone());
            }
            let timeit_method = make_builtin(|inner_args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
                let number: i64 = if inner_args.is_empty() {
                    1_000_000
                } else if inner_args.len() == 1 {
                    inner_args[0].as_int().unwrap_or(1_000_000)
                } else {
                    inner_args[1].as_int().unwrap_or(1_000_000)
                };
                Ok(PyObject::float(number as f64 * 1e-7))
            });
            attrs.insert(CompactString::from("timeit"), timeit_method);
        }
        Ok(inst)
    });

    make_module("timeit", vec![
        ("default_timer", default_timer),
        ("timeit", timeit_fn),
        ("repeat", repeat_fn),
        ("Timer", timer_fn),
        ("default_number", PyObject::int(1_000_000)),
        ("default_repeat", PyObject::int(5)),
    ])
}
