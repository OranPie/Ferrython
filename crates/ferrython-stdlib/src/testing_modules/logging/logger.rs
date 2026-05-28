use super::*;

pub(super) fn logging_log(level: i64, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::none());
    }
    // Check global disable threshold
    let disable_level = DISABLE_LEVEL.load(std::sync::atomic::Ordering::Relaxed);
    if disable_level > 0 && level <= disable_level {
        return Ok(PyObject::none());
    }
    // Respect root logger level from basicConfig
    let root_level = ROOT_LEVEL.load(std::sync::atomic::Ordering::Relaxed);
    if root_level > 0 && level < root_level {
        return Ok(PyObject::none());
    }
    let level_name = match level {
        10 => "DEBUG",
        20 => "INFO",
        30 => "WARNING",
        40 => "ERROR",
        50 => "CRITICAL",
        _ => "UNKNOWN",
    };
    let msg_fmt = args[0].py_to_string();
    let msg = if args.len() > 1 {
        apply_percent_format(&msg_fmt, &args[1..])
    } else {
        msg_fmt
    };

    // Dispatch through the root logger's handlers if any are registered
    let mut dispatched = false;
    LOGGER_REGISTRY.with(|reg| {
        let reg = reg.borrow();
        if let Some(root) = reg.get("root") {
            if let Some(handlers) = root.get_attr("handlers") {
                if let PyObjectPayload::List(items) = &handlers.payload {
                    let r = items.read();
                    if !r.is_empty() {
                        // Build a LogRecord
                        let rec_cls = PyObject::class(
                            CompactString::from("LogRecord"),
                            vec![],
                            IndexMap::new(),
                        );
                        let mut rec_attrs = IndexMap::new();
                        rec_attrs.insert(
                            CompactString::from("levelname"),
                            PyObject::str_val(CompactString::from(level_name)),
                        );
                        rec_attrs.insert(CompactString::from("levelno"), PyObject::int(level));
                        rec_attrs.insert(
                            CompactString::from("name"),
                            PyObject::str_val(CompactString::from("root")),
                        );
                        rec_attrs.insert(
                            CompactString::from("message"),
                            PyObject::str_val(CompactString::from(msg.as_str())),
                        );
                        rec_attrs.insert(
                            CompactString::from("msg"),
                            PyObject::str_val(CompactString::from(msg.as_str())),
                        );
                        let record = PyObject::instance_with_attrs(rec_cls, rec_attrs);

                        for handler in r.iter() {
                            if let Some(emit_fn) = handler.get_attr("emit") {
                                match &emit_fn.payload {
                                    PyObjectPayload::NativeFunction(nf) => {
                                        let _ = (nf.func)(&[record.clone()]);
                                    }
                                    PyObjectPayload::NativeClosure(nc) => {
                                        let _ = (nc.func)(&[record.clone()]);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        dispatched = true;
                    }
                }
            }
        }
    });

    if !dispatched {
        let formatted = format_log_message(root_format(), level_name, "root", &msg);
        eprintln!("{}", formatted);
    }
    Ok(PyObject::none())
}

pub(crate) fn logging_get_logger(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let logger_name = if args.is_empty() || matches!(&args[0].payload, PyObjectPayload::None) {
        CompactString::from("root")
    } else {
        CompactString::from(args[0].py_to_string())
    };

    // Return cached logger if it already exists
    {
        let found = LOGGER_REGISTRY.with(|reg| reg.borrow().get(logger_name.as_str()).cloned());
        if let Some(existing) = found {
            return Ok(existing);
        }
    }

    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("name"),
        PyObject::str_val(logger_name.clone()),
    );
    ns.insert(CompactString::from("propagate"), PyObject::bool_val(true));
    let root_level = ROOT_LEVEL.load(std::sync::atomic::Ordering::Relaxed);
    let is_root = logger_name.as_str() == "root";
    // CPython: named loggers start at level=0 (NOTSET); root logger defaults to WARNING(30)
    let initial_level: i64 = if is_root {
        if root_level > 0 {
            root_level
        } else {
            30
        }
    } else {
        0
    };
    // Effective level: non-root loggers use 0 (NOTSET) to trigger parent chain walk at log time
    let effective = initial_level;
    let effective_level: Rc<PyCell<i64>> = Rc::new(PyCell::new(effective));
    ns.insert(CompactString::from("level"), PyObject::int(initial_level));
    let handlers_list = PyObject::list(vec![]);
    ns.insert(CompactString::from("handlers"), handlers_list.clone());

    // Create log methods that capture the shared handlers list and effective level
    let make_log_method = |level: i64,
                           level_name: &'static str,
                           handlers: PyObjectRef,
                           name: CompactString,
                           eff_level: Rc<PyCell<i64>>|
     -> PyObjectRef {
        PyObject::native_closure(level_name, move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            // Check global disable threshold first
            let disable_level = DISABLE_LEVEL.load(std::sync::atomic::Ordering::Relaxed);
            if disable_level > 0 && level <= disable_level {
                return Ok(PyObject::none());
            }
            // Filter: only emit if message level >= logger's effective level
            // If own level is NOTSET (0), walk parent chain to find effective level
            let mut current_level = *eff_level.read();
            if current_level == 0 {
                LOGGER_REGISTRY.with(|reg| {
                    let reg = reg.borrow();
                    let mut cur = name.to_string();
                    while let Some(dot) = cur.rfind('.') {
                        cur.truncate(dot);
                        if let Some(parent) = reg.get(&cur) {
                            if let Some(plvl) = parent.get_attr("level") {
                                if let Some(n) = plvl.as_int() {
                                    if n > 0 {
                                        current_level = n;
                                        return;
                                    }
                                }
                            }
                        }
                    }
                });
                if current_level == 0 {
                    // Fall back to root level
                    current_level = ROOT_LEVEL.load(std::sync::atomic::Ordering::Relaxed);
                    if current_level == 0 {
                        current_level = 30;
                    }
                }
            }
            if current_level > 0 && level < current_level {
                return Ok(PyObject::none());
            }
            let msg_fmt = args[0].py_to_string();
            let msg = if args.len() > 1 {
                apply_percent_format(&msg_fmt, &args[1..])
            } else {
                msg_fmt
            };

            // Create a LogRecord-like instance
            let rec_cls =
                PyObject::class(CompactString::from("LogRecord"), vec![], IndexMap::new());
            let record = PyObject::instance(rec_cls);
            if let PyObjectPayload::Instance(ref rd) = record.payload {
                let mut ra = rd.attrs.write();
                ra.insert(
                    CompactString::from("levelname"),
                    PyObject::str_val(CompactString::from(level_name)),
                );
                ra.insert(CompactString::from("levelno"), PyObject::int(level));
                ra.insert(CompactString::from("name"), PyObject::str_val(name.clone()));
                ra.insert(
                    CompactString::from("message"),
                    PyObject::str_val(CompactString::from(msg.clone())),
                );
                ra.insert(
                    CompactString::from("msg"),
                    PyObject::str_val(CompactString::from(msg.clone())),
                );
                ra.insert(CompactString::from("args"), PyObject::none());
                ra.insert(
                    CompactString::from("asctime"),
                    PyObject::str_val(CompactString::from(current_asctime(None))),
                );
                ra.insert(CompactString::from("lineno"), PyObject::int(0));
                ra.insert(
                    CompactString::from("filename"),
                    PyObject::str_val(CompactString::from("")),
                );
                ra.insert(
                    CompactString::from("funcName"),
                    PyObject::str_val(CompactString::from("")),
                );
                ra.insert(
                    CompactString::from("pathname"),
                    PyObject::str_val(CompactString::from("")),
                );
                ra.insert(
                    CompactString::from("module"),
                    PyObject::str_val(CompactString::from("")),
                );
                let created = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64();
                ra.insert(CompactString::from("created"), PyObject::float(created));
                let msg_clone = msg.clone();
                ra.insert(
                    CompactString::from("getMessage"),
                    PyObject::native_closure("LogRecord.getMessage", move |_args| {
                        Ok(PyObject::str_val(CompactString::from(msg_clone.clone())))
                    }),
                );
            }

            // Dispatch to handlers via shared list, then propagate to parents
            let mut any_handler_found = false;

            // Helper: emit record to a handler list
            fn emit_to_handlers(
                handlers_obj: &PyObjectRef,
                record: &PyObjectRef,
                level: i64,
            ) -> bool {
                if let PyObjectPayload::List(items) = &handlers_obj.payload {
                    let items_r = items.read();
                    if items_r.is_empty() {
                        return false;
                    }
                    for handler in items_r.iter() {
                        if let Some(handler_level) = handler.get_attr("level") {
                            if let Some(hl) = handler_level.as_int() {
                                if hl > 0 && level < hl {
                                    continue;
                                }
                            }
                        }
                        if let Some(emit_fn) = handler.get_attr("emit") {
                            match &emit_fn.payload {
                                PyObjectPayload::NativeFunction(nf) => {
                                    let _ = (nf.func)(&[handler.clone(), record.clone()]);
                                }
                                PyObjectPayload::NativeClosure(nc) => {
                                    let _ = (nc.func)(&[handler.clone(), record.clone()]);
                                }
                                _ => {
                                    ferrython_core::error::request_vm_call(
                                        emit_fn.clone(),
                                        vec![record.clone()],
                                    );
                                }
                            }
                        }
                    }
                    true
                } else {
                    false
                }
            }

            // Emit to own handlers
            if emit_to_handlers(&handlers, &record, level) {
                any_handler_found = true;
            }

            // Propagate to parent loggers by walking the name hierarchy
            // Only propagate if the logger's propagate attribute is True
            let should_propagate = LOGGER_REGISTRY.with(|reg| {
                let reg = reg.borrow();
                if let Some(this_logger) = reg.get(name.as_str()) {
                    this_logger
                        .get_attr("propagate")
                        .map(|p| p.is_truthy())
                        .unwrap_or(true)
                } else {
                    true
                }
            });
            if should_propagate {
                LOGGER_REGISTRY.with(|reg| {
                    let reg = reg.borrow();
                    let mut current_name = name.to_string();
                    while let Some(dot_pos) = current_name.rfind('.') {
                        current_name.truncate(dot_pos);
                        if let Some(parent) = reg.get(&current_name) {
                            if let Some(parent_handlers) = parent.get_attr("handlers") {
                                if emit_to_handlers(&parent_handlers, &record, level) {
                                    any_handler_found = true;
                                }
                            }
                            // Check parent's propagate for further walking
                            let parent_propagate = parent
                                .get_attr("propagate")
                                .map(|p| p.is_truthy())
                                .unwrap_or(true);
                            if !parent_propagate {
                                break;
                            }
                        }
                    }
                    // Also propagate to root logger if we haven't stopped
                    if current_name != "root" {
                        if let Some(root) = reg.get("root") {
                            if let Some(root_handlers) = root.get_attr("handlers") {
                                if emit_to_handlers(&root_handlers, &record, level) {
                                    any_handler_found = true;
                                }
                            }
                        }
                    }
                });
            }
            // Last-resort: only print to stderr if no handlers registered at all
            if !any_handler_found {
                eprintln!("{}:{}:{}", level_name, name, msg);
            }
            Ok(PyObject::none())
        })
    };

    ns.insert(
        CompactString::from("debug"),
        make_log_method(
            10,
            "DEBUG",
            handlers_list.clone(),
            logger_name.clone(),
            effective_level.clone(),
        ),
    );
    ns.insert(
        CompactString::from("info"),
        make_log_method(
            20,
            "INFO",
            handlers_list.clone(),
            logger_name.clone(),
            effective_level.clone(),
        ),
    );
    ns.insert(
        CompactString::from("warning"),
        make_log_method(
            30,
            "WARNING",
            handlers_list.clone(),
            logger_name.clone(),
            effective_level.clone(),
        ),
    );
    ns.insert(
        CompactString::from("error"),
        make_log_method(
            40,
            "ERROR",
            handlers_list.clone(),
            logger_name.clone(),
            effective_level.clone(),
        ),
    );
    ns.insert(
        CompactString::from("critical"),
        make_log_method(
            50,
            "CRITICAL",
            handlers_list.clone(),
            logger_name.clone(),
            effective_level.clone(),
        ),
    );
    // exception() — logs at ERROR level (same as error(), exc_info implied)
    ns.insert(
        CompactString::from("exception"),
        make_log_method(
            40,
            "ERROR",
            handlers_list.clone(),
            logger_name.clone(),
            effective_level.clone(),
        ),
    );
    // log(level, msg, *args) — generic log method
    {
        let hl_log = handlers_list.clone();
        let name_log = logger_name.clone();
        let el_log = effective_level.clone();
        ns.insert(
            CompactString::from("log"),
            PyObject::native_closure("log", move |args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Ok(PyObject::none());
                }
                let level = args[0].as_int().unwrap_or(20);
                let eff = *el_log.read();
                if eff > 0 && level < eff {
                    return Ok(PyObject::none());
                }
                let msg = args[1].py_to_string();
                let level_name = match level {
                    10 => "DEBUG",
                    20 => "INFO",
                    30 => "WARNING",
                    40 => "ERROR",
                    50 => "CRITICAL",
                    _ => "UNKNOWN",
                };
                let msg_for_getmsg = msg.clone();
                let mut record_attrs = IndexMap::new();
                record_attrs.insert(
                    CompactString::from("message"),
                    PyObject::str_val(CompactString::from(&msg)),
                );
                record_attrs.insert(
                    CompactString::from("msg"),
                    PyObject::str_val(CompactString::from(&msg)),
                );
                record_attrs.insert(
                    CompactString::from("levelname"),
                    PyObject::str_val(CompactString::from(level_name)),
                );
                record_attrs.insert(CompactString::from("levelno"), PyObject::int(level));
                record_attrs.insert(
                    CompactString::from("name"),
                    PyObject::str_val(name_log.clone()),
                );
                record_attrs.insert(CompactString::from("args"), PyObject::none());
                record_attrs.insert(
                    CompactString::from("asctime"),
                    PyObject::str_val(CompactString::from(current_asctime(None))),
                );
                record_attrs.insert(CompactString::from("lineno"), PyObject::int(0));
                record_attrs.insert(
                    CompactString::from("filename"),
                    PyObject::str_val(CompactString::from("")),
                );
                record_attrs.insert(
                    CompactString::from("funcName"),
                    PyObject::str_val(CompactString::from("")),
                );
                record_attrs.insert(
                    CompactString::from("pathname"),
                    PyObject::str_val(CompactString::from("")),
                );
                record_attrs.insert(
                    CompactString::from("module"),
                    PyObject::str_val(CompactString::from("")),
                );
                let created = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64();
                record_attrs.insert(CompactString::from("created"), PyObject::float(created));
                record_attrs.insert(
                    CompactString::from("getMessage"),
                    PyObject::native_closure("LogRecord.getMessage", move |_args| {
                        Ok(PyObject::str_val(CompactString::from(
                            msg_for_getmsg.clone(),
                        )))
                    }),
                );
                let record_cls =
                    PyObject::class(CompactString::from("LogRecord"), vec![], IndexMap::new());
                let record = PyObject::instance_with_attrs(record_cls, record_attrs);
                if let PyObjectPayload::List(items) = &hl_log.payload {
                    let r = items.read();
                    if r.is_empty() {
                        eprintln!("{}: {}", level_name, msg);
                    } else {
                        for handler in r.iter() {
                            if let Some(emit) = handler.get_attr("emit") {
                                if let PyObjectPayload::NativeClosure(nc) = &emit.payload {
                                    let _ = (nc.func)(&[record.clone()]);
                                }
                            }
                        }
                    }
                }
                Ok(PyObject::none())
            }),
        );
    }

    // setLevel — placeholder (patched after instance creation to update .level attr)
    let el = effective_level.clone();
    ns.insert(
        CompactString::from("setLevel"),
        PyObject::native_closure("setLevel", move |args: &[PyObjectRef]| {
            if let Some(v) = args.first() {
                if let Some(n) = v.as_int() {
                    *el.write() = n;
                }
            }
            Ok(PyObject::none())
        }),
    );
    // addHandler — push to shared handlers list
    let hl = handlers_list.clone();
    ns.insert(
        CompactString::from("addHandler"),
        PyObject::native_closure("addHandler", move |args: &[PyObjectRef]| {
            if !args.is_empty() {
                if let PyObjectPayload::List(items) = &hl.payload {
                    items.write().push(args[0].clone());
                }
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(CompactString::from("removeHandler"), {
        let hl_rm = handlers_list.clone();
        PyObject::native_closure("removeHandler", move |args: &[PyObjectRef]| {
            if !args.is_empty() {
                if let PyObjectPayload::List(items) = &hl_rm.payload {
                    let mut w = items.write();
                    // Remove by identity (pointer equality)
                    let target = &args[0];
                    w.retain(|h| !std::ptr::eq(h.as_ref(), target.as_ref()));
                }
            }
            Ok(PyObject::none())
        })
    });
    let hl2 = handlers_list.clone();
    ns.insert(
        CompactString::from("hasHandlers"),
        PyObject::native_closure("hasHandlers", move |_: &[PyObjectRef]| {
            if let PyObjectPayload::List(items) = &hl2.payload {
                return Ok(PyObject::bool_val(!items.read().is_empty()));
            }
            Ok(PyObject::bool_val(false))
        }),
    );
    let el2 = effective_level.clone();
    let name_for_enabled = logger_name.clone();
    ns.insert(
        CompactString::from("isEnabledFor"),
        PyObject::native_closure("isEnabledFor", move |args: &[PyObjectRef]| {
            if let Some(v) = args.first() {
                if let Some(n) = v.as_int() {
                    // Check disable threshold first
                    let disable_level = DISABLE_LEVEL.load(std::sync::atomic::Ordering::Relaxed);
                    if disable_level > 0 && n <= disable_level {
                        return Ok(PyObject::bool_val(false));
                    }
                    // Get effective level (walk parents if NOTSET)
                    let mut current = *el2.read();
                    if current == 0 {
                        LOGGER_REGISTRY.with(|reg| {
                            let reg = reg.borrow();
                            let mut cur = name_for_enabled.to_string();
                            while let Some(dot) = cur.rfind('.') {
                                cur.truncate(dot);
                                if let Some(parent) = reg.get(&cur) {
                                    if let Some(plvl) = parent.get_attr("level") {
                                        if let Some(pn) = plvl.as_int() {
                                            if pn > 0 {
                                                current = pn;
                                                return;
                                            }
                                        }
                                    }
                                }
                            }
                            // Check root logger
                            if let Some(root) = reg.get("root") {
                                if let Some(rlvl) = root.get_attr("level") {
                                    if let Some(rn) = rlvl.as_int() {
                                        if rn > 0 {
                                            current = rn;
                                            return;
                                        }
                                    }
                                }
                            }
                        });
                        if current == 0 {
                            current = ROOT_LEVEL.load(std::sync::atomic::Ordering::Relaxed);
                            if current == 0 {
                                current = 30;
                            }
                        }
                    }
                    return Ok(PyObject::bool_val(n >= current));
                }
            }
            Ok(PyObject::bool_val(true))
        }),
    );
    let el3 = effective_level.clone();
    let name_for_eff = logger_name.clone();
    ns.insert(
        CompactString::from("getEffectiveLevel"),
        PyObject::native_closure("getEffectiveLevel", move |_: &[PyObjectRef]| {
            let mut current = *el3.read();
            if current == 0 {
                LOGGER_REGISTRY.with(|reg| {
                    let reg = reg.borrow();
                    let mut cur = name_for_eff.to_string();
                    while let Some(dot) = cur.rfind('.') {
                        cur.truncate(dot);
                        if let Some(parent) = reg.get(&cur) {
                            if let Some(plvl) = parent.get_attr("level") {
                                if let Some(pn) = plvl.as_int() {
                                    if pn > 0 {
                                        current = pn;
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    // Check root logger
                    if let Some(root) = reg.get("root") {
                        if let Some(rlvl) = root.get_attr("level") {
                            if let Some(rn) = rlvl.as_int() {
                                if rn > 0 {
                                    current = rn;
                                    return;
                                }
                            }
                        }
                    }
                });
                if current == 0 {
                    current = ROOT_LEVEL.load(std::sync::atomic::Ordering::Relaxed);
                    if current == 0 {
                        current = 30;
                    }
                }
            }
            Ok(PyObject::int(current))
        }),
    );
    // parent — reference to parent logger (None for root, else the parent)
    {
        let parent_name = if logger_name.as_str() == "root" {
            None
        } else if let Some(dot) = logger_name.rfind('.') {
            Some(CompactString::from(&logger_name.as_str()[..dot]))
        } else {
            Some(CompactString::from("root"))
        };
        if let Some(pn) = parent_name {
            let parent = LOGGER_REGISTRY.with(|reg| reg.borrow().get(pn.as_str()).cloned());
            ns.insert(
                CompactString::from("parent"),
                parent.unwrap_or_else(PyObject::none),
            );
        } else {
            ns.insert(CompactString::from("parent"), PyObject::none());
        }
    }
    // getChild(suffix) — return a child logger
    {
        let name_for_child = logger_name.clone();
        ns.insert(
            CompactString::from("getChild"),
            PyObject::native_closure("getChild", move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Err(PyException::type_error("getChild() requires a suffix"));
                }
                let suffix = args[0].py_to_string();
                let child_name = if name_for_child.as_str() == "root" {
                    suffix
                } else {
                    format!("{}.{}", name_for_child, suffix)
                };
                logging_get_logger(&[PyObject::str_val(CompactString::from(child_name))])
            }),
        );
    }
    // addFilter / removeFilter — manage filter list on logger
    let filters_list = PyObject::list(vec![]);
    {
        let fl = filters_list.clone();
        ns.insert(
            CompactString::from("addFilter"),
            PyObject::native_closure("addFilter", move |args: &[PyObjectRef]| {
                if !args.is_empty() {
                    if let PyObjectPayload::List(items) = &fl.payload {
                        items.write().push(args[0].clone());
                    }
                }
                Ok(PyObject::none())
            }),
        );
    }
    {
        let fl = filters_list.clone();
        ns.insert(
            CompactString::from("removeFilter"),
            PyObject::native_closure("removeFilter", move |args: &[PyObjectRef]| {
                if !args.is_empty() {
                    if let PyObjectPayload::List(items) = &fl.payload {
                        let target = &args[0];
                        items
                            .write()
                            .retain(|h| !std::ptr::eq(h.as_ref(), target.as_ref()));
                    }
                }
                Ok(PyObject::none())
            }),
        );
    }
    ns.insert(CompactString::from("filters"), filters_list);
    // manager attribute (stub for compatibility)
    ns.insert(CompactString::from("manager"), PyObject::none());
    ns.insert(CompactString::from("disabled"), PyObject::bool_val(false));

    let cls = PyObject::class(CompactString::from("Logger"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        for (k, v) in ns {
            attrs.insert(k, v);
        }
    }
    // Patch setLevel to also update the visible .level attribute
    {
        let el_patch = effective_level.clone();
        let inst_ref = inst.clone();
        let set_level_fn = PyObject::native_closure("setLevel", move |args: &[PyObjectRef]| {
            if let Some(v) = args.first() {
                if let Some(n) = v.as_int() {
                    *el_patch.write() = n;
                    if let PyObjectPayload::Instance(ref data) = inst_ref.payload {
                        data.attrs
                            .write()
                            .insert(CompactString::from("level"), PyObject::int(n));
                    }
                }
            }
            Ok(PyObject::none())
        });
        if let PyObjectPayload::Instance(inst_data) = &inst.payload {
            inst_data
                .attrs
                .write()
                .insert(CompactString::from("setLevel"), set_level_fn);
        }
    }
    // Register in thread-local logger registry
    LOGGER_REGISTRY.with(|reg| {
        reg.borrow_mut()
            .insert(logger_name.to_string(), inst.clone());
    });
    Ok(inst)
}
