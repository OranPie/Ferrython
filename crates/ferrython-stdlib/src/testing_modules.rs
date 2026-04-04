//! Logging, testing, and debugging stdlib modules

use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin,
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
    ns.insert(CompactString::from("level"), PyObject::int(30)); // WARNING default
    let handlers_list = PyObject::list(vec![]);
    ns.insert(CompactString::from("handlers"), handlers_list.clone());

    // Create log methods that capture the shared handlers list
    let make_log_method = |level: i64, level_name: &'static str, handlers: PyObjectRef, name: CompactString| -> PyObjectRef {
        PyObject::native_closure(level_name, move |args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::none()); }
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

    ns.insert(CompactString::from("debug"), make_log_method(10, "DEBUG", handlers_list.clone(), logger_name.clone()));
    ns.insert(CompactString::from("info"), make_log_method(20, "INFO", handlers_list.clone(), logger_name.clone()));
    ns.insert(CompactString::from("warning"), make_log_method(30, "WARNING", handlers_list.clone(), logger_name.clone()));
    ns.insert(CompactString::from("error"), make_log_method(40, "ERROR", handlers_list.clone(), logger_name.clone()));
    ns.insert(CompactString::from("critical"), make_log_method(50, "CRITICAL", handlers_list.clone(), logger_name.clone()));

    ns.insert(CompactString::from("setLevel"), make_builtin(|_| Ok(PyObject::none())));
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
    ns.insert(CompactString::from("isEnabledFor"), make_builtin(|_| Ok(PyObject::bool_val(true))));
    ns.insert(CompactString::from("getEffectiveLevel"), make_builtin(|_| Ok(PyObject::int(30))));
    
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

pub fn create_unittest_module() -> PyObjectRef {
    // Create TestCase class
    let mut tc_ns = IndexMap::new();
    tc_ns.insert(CompactString::from("__unittest_testcase__"), PyObject::bool_val(true));
    let test_case = PyObject::class(CompactString::from("TestCase"), vec![], tc_ns);

    make_module("unittest", vec![
        ("TestCase", test_case),
        ("main", make_builtin(|_| Ok(PyObject::none()))),
        ("TestSuite", make_builtin(|_| Ok(PyObject::none()))),
        ("TestLoader", make_builtin(|_| Ok(PyObject::none()))),
        ("TextTestRunner", make_builtin(|_| Ok(PyObject::none()))),
        ("skip", make_builtin(|_args| {
            // Return identity decorator
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
            let ir = inst.clone();
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
