use super::*;
use crate::testing_modules::logging;

pub(super) fn register_log_assertions(tc_ns: &mut IndexMap<CompactString, PyObjectRef>) {
    // assertLogs(logger=None, level='INFO') — context manager
    tc_ns.insert(
        CompactString::from("assertLogs"),
        PyObject::native_closure("assertLogs", |args: &[PyObjectRef]| {
            let logger_name =
                if !args.is_empty() && !matches!(args[0].payload, PyObjectPayload::None) {
                    args[0].py_to_string()
                } else {
                    "root".to_string()
                };
            let level_str = if args.len() > 1 && !matches!(args[1].payload, PyObjectPayload::None) {
                args[1].py_to_string()
            } else {
                "INFO".to_string()
            };
            let level_num: i64 = match level_str.as_str() {
                "DEBUG" => 10,
                "INFO" => 20,
                "WARNING" => 30,
                "ERROR" => 40,
                "CRITICAL" => 50,
                _ => level_str.parse().unwrap_or(20),
            };

            // Shared state: captured records and output lines
            let records: Rc<PyCell<Vec<PyObjectRef>>> = Rc::new(PyCell::new(vec![]));
            let output: Rc<PyCell<Vec<PyObjectRef>>> = Rc::new(PyCell::new(vec![]));

            let cls = PyObject::class(
                CompactString::from("_AssertLogsContext"),
                vec![],
                IndexMap::new(),
            );
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("records"), PyObject::list(vec![]));
                w.insert(CompactString::from("output"), PyObject::list(vec![]));

                // Build a capturing handler
                let _recs_enter = records.clone();
                let _outs_enter = output.clone();
                let ln = logger_name.clone();
                let lnum = level_num;

                // __enter__: install capturing handler on the logger
                let recs_for_handler = records.clone();
                let outs_for_handler = output.clone();
                let handler_cls = PyObject::class(
                    CompactString::from("_CapturingHandler"),
                    vec![],
                    IndexMap::new(),
                );
                let handler_inst = PyObject::instance(handler_cls);
                if let PyObjectPayload::Instance(ref hd) = handler_inst.payload {
                    let mut ha = hd.attrs.write();
                    ha.insert(CompactString::from("level"), PyObject::int(0));
                    let rfh = recs_for_handler.clone();
                    let ofh = outs_for_handler.clone();
                    ha.insert(
                        CompactString::from("emit"),
                        PyObject::native_closure(
                            "_CapturingHandler.emit",
                            move |args: &[PyObjectRef]| {
                                let record = if args.len() >= 2 {
                                    &args[1]
                                } else if !args.is_empty() {
                                    &args[0]
                                } else {
                                    return Ok(PyObject::none());
                                };
                                rfh.write().push(record.clone());
                                let msg = record
                                    .get_attr("message")
                                    .or_else(|| record.get_attr("msg"))
                                    .map(|m| m.py_to_string())
                                    .unwrap_or_default();
                                let levelname = record
                                    .get_attr("levelname")
                                    .map(|l| l.py_to_string())
                                    .unwrap_or_else(|| "INFO".to_string());
                                let name = record
                                    .get_attr("name")
                                    .map(|n| n.py_to_string())
                                    .unwrap_or_else(|| "root".to_string());
                                let line = format!("{}:{}:{}", levelname, name, msg);
                                ofh.write()
                                    .push(PyObject::str_val(CompactString::from(line)));
                                Ok(PyObject::none())
                            },
                        ),
                    );
                    ha.insert(
                        CompactString::from("setLevel"),
                        make_builtin(|_| Ok(PyObject::none())),
                    );
                    ha.insert(
                        CompactString::from("setFormatter"),
                        make_builtin(|_| Ok(PyObject::none())),
                    );
                }

                let handler_for_enter = handler_inst.clone();
                let handler_for_exit = handler_inst;
                let inst_ref = d.attrs.clone();
                let ln_enter = ln.clone();

                w.insert(
                    CompactString::from("__enter__"),
                    PyObject::native_closure("__enter__", move |args: &[PyObjectRef]| {
                        // self is args[0] when called via context manager
                        let ctx = if !args.is_empty() {
                            args[0].clone()
                        } else {
                            return Ok(PyObject::none());
                        };
                        // Add handler to the target logger
                        let logger = logging::logging_get_logger(&[PyObject::str_val(
                            CompactString::from(ln_enter.as_str()),
                        )])?;
                        if let Some(add_handler) = logger.get_attr("addHandler") {
                            match &add_handler.payload {
                                PyObjectPayload::NativeClosure(nc) => {
                                    let _ = (nc.func)(&[handler_for_enter.clone()]);
                                }
                                _ => {}
                            }
                        }
                        // Lower logger level
                        if let Some(set_level) = logger.get_attr("setLevel") {
                            match &set_level.payload {
                                PyObjectPayload::NativeClosure(nc) => {
                                    let _ = (nc.func)(&[PyObject::int(lnum)]);
                                }
                                _ => {}
                            }
                        }
                        Ok(ctx)
                    }),
                );

                let ln_exit = ln;
                let recs_exit = records.clone();
                let outs_exit = output.clone();
                let inst_exit = inst_ref;
                w.insert(
                    CompactString::from("__exit__"),
                    PyObject::native_closure("__exit__", move |args: &[PyObjectRef]| {
                        // Remove the handler
                        let logger = logging::logging_get_logger(&[PyObject::str_val(
                            CompactString::from(ln_exit.as_str()),
                        )])?;
                        if let Some(rm_handler) = logger.get_attr("removeHandler") {
                            match &rm_handler.payload {
                                PyObjectPayload::NativeClosure(nc) => {
                                    let _ = (nc.func)(&[handler_for_exit.clone()]);
                                }
                                _ => {}
                            }
                        }
                        // Update .records and .output on context
                        let recs = recs_exit.read().clone();
                        let outs = outs_exit.read().clone();
                        {
                            let mut attrs = inst_exit.write();
                            attrs.insert(
                                CompactString::from("records"),
                                PyObject::list(recs.clone()),
                            );
                            attrs.insert(
                                CompactString::from("output"),
                                PyObject::list(outs.clone()),
                            );
                        }
                        // If no exc and no records, raise assertion error
                        let has_exc = if args.len() > 1 {
                            !matches!(args[1].payload, PyObjectPayload::None)
                        } else {
                            false
                        };
                        if !has_exc && recs.is_empty() {
                            return Err(PyException::assertion_error(format!(
                                "no logs of level INFO or above triggered on {}",
                                ln_exit
                            )));
                        }
                        Ok(PyObject::bool_val(false))
                    }),
                );
            }
            Ok(inst)
        }),
    );

    // assertRaisesRegex(exc_type, regex) — context manager
    tc_ns.insert(
        CompactString::from("assertRaisesRegex"),
        PyObject::native_closure("assertRaisesRegex", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "assertRaisesRegex requires exc_type and regex",
                ));
            }
            let exc_type_name = match &args[0].payload {
                PyObjectPayload::Class(cd) => cd.name.clone(),
                PyObjectPayload::Str(s) => s.to_compact_string(),
                _ => CompactString::from(args[0].py_to_string()),
            };
            let pattern = args[1].py_to_string();
            let cls = PyObject::class(
                CompactString::from("_AssertRaisesRegexContext"),
                vec![],
                IndexMap::new(),
            );
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(
                    CompactString::from("expected"),
                    PyObject::str_val(exc_type_name.clone()),
                );
                w.insert(
                    CompactString::from("__enter__"),
                    PyObject::native_closure("__enter__", |_: &[PyObjectRef]| Ok(PyObject::none())),
                );
                let etype = exc_type_name;
                let pat = pattern;
                w.insert(
                    CompactString::from("__exit__"),
                    PyObject::native_closure("__exit__", move |args: &[PyObjectRef]| {
                        let has_exc = if args.is_empty() {
                            false
                        } else {
                            !matches!(args[0].payload, PyObjectPayload::None)
                        };
                        if !has_exc {
                            return Err(PyException::assertion_error(format!(
                                "{} not raised",
                                etype
                            )));
                        }
                        // Check regex against exception message
                        let exc_msg = if args.len() > 1 {
                            args[1].py_to_string()
                        } else {
                            String::new()
                        };
                        if let Ok(re) = regex::Regex::new(&pat) {
                            if re.find(&exc_msg).is_none() {
                                return Err(PyException::assertion_error(format!(
                                    "\"{}\" does not match \"{}\"",
                                    pat, exc_msg
                                )));
                            }
                        }
                        Ok(PyObject::bool_val(true))
                    }),
                );
            }
            Ok(inst)
        }),
    );
}
