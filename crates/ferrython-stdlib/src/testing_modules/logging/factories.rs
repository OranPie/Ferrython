use super::*;

pub(super) fn create_null_handler_fn() -> PyObjectRef {
    // NullHandler — discards all log records
    let null_handler_cls =
        PyObject::class(CompactString::from("NullHandler"), vec![], IndexMap::new());
    let nh_cls = null_handler_cls.clone();
    let null_handler_fn =
        PyObject::native_closure("NullHandler", move |_args: &[PyObjectRef]| {
            let inst = PyObject::instance(nh_cls.clone());
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                attrs.insert(CompactString::from("level"), PyObject::int(0));
                attrs.insert(
                    CompactString::from("emit"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
                attrs.insert(
                    CompactString::from("handle"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
                attrs.insert(
                    CompactString::from("setLevel"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
                attrs.insert(
                    CompactString::from("setFormatter"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
                attrs.insert(
                    CompactString::from("createLock"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
                attrs.insert(
                    CompactString::from("acquire"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
                attrs.insert(
                    CompactString::from("release"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
            }
            Ok(inst)
        });
    null_handler_fn
}

pub(super) fn create_get_level_name_fn() -> PyObjectRef {
    // getLevelName
    let get_level_name_fn = make_builtin(|args: &[PyObjectRef]| {
        if let Some(v) = args.first().and_then(|a| a.as_int()) {
            let name = match v {
                10 => "DEBUG",
                20 => "INFO",
                30 => "WARNING",
                40 => "ERROR",
                50 => "CRITICAL",
                0 => "NOTSET",
                _ => {
                    return Ok(PyObject::str_val(CompactString::from(format!(
                        "Level {}",
                        v
                    ))))
                }
            };
            Ok(PyObject::str_val(CompactString::from(name)))
        } else if let Some(s) = args.first() {
            let name = s.py_to_string();
            let level = match name.as_ref() {
                "DEBUG" => 10,
                "INFO" => 20,
                "WARNING" => 30,
                "ERROR" => 40,
                "CRITICAL" => 50,
                "NOTSET" => 0,
                _ => {
                    return Err(PyException::value_error(format!(
                        "Unknown level: '{}'",
                        name
                    )))
                }
            };
            Ok(PyObject::int(level))
        } else {
            Ok(PyObject::none())
        }
    });
    get_level_name_fn
}

pub(super) fn create_filter_fn() -> PyObjectRef {
    // Filter(name='') — filters records by logger name prefix
    let filter_fn = PyObject::native_closure("Filter", move |args: &[PyObjectRef]| {
        let name = if args.is_empty() {
            String::new()
        } else {
            args[0].py_to_string()
        };
        let cls = PyObject::class(CompactString::from("Filter"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(
                CompactString::from("name"),
                PyObject::str_val(CompactString::from(name.as_str())),
            );
            let filter_name = name.clone();
            w.insert(
                CompactString::from("filter"),
                PyObject::native_closure("Filter.filter", move |args: &[PyObjectRef]| {
                    if filter_name.is_empty() {
                        return Ok(PyObject::bool_val(true));
                    }
                    let record = if !args.is_empty() {
                        &args[0]
                    } else {
                        return Ok(PyObject::bool_val(true));
                    };
                    let rec_name = record
                        .get_attr("name")
                        .map(|n| n.py_to_string())
                        .unwrap_or_default();
                    let ok = rec_name == filter_name
                        || rec_name.starts_with(&format!("{}.", filter_name));
                    Ok(PyObject::bool_val(ok))
                }),
            );
        }
        Ok(inst)
    });
    filter_fn
}

pub(super) fn create_log_record_fn() -> PyObjectRef {
    // LogRecord(name, level, pathname, lineno, msg, args, exc_info)
    let log_record_fn = make_builtin(|args: &[PyObjectRef]| {
        let name = if !args.is_empty() {
            args[0].py_to_string()
        } else {
            "root".to_string()
        };
        let level = if args.len() > 1 {
            args[1].as_int().unwrap_or(20)
        } else {
            20
        };
        let pathname = if args.len() > 2 {
            args[2].py_to_string()
        } else {
            String::new()
        };
        let lineno = if args.len() > 3 {
            args[3].as_int().unwrap_or(0)
        } else {
            0
        };
        let msg = if args.len() > 4 {
            args[4].py_to_string()
        } else {
            String::new()
        };
        let level_name = match level {
            10 => "DEBUG",
            20 => "INFO",
            30 => "WARNING",
            40 => "ERROR",
            50 => "CRITICAL",
            _ => "UNKNOWN",
        };
        let cls = PyObject::class(CompactString::from("LogRecord"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        attrs.insert(
            CompactString::from("name"),
            PyObject::str_val(CompactString::from(name.as_str())),
        );
        attrs.insert(CompactString::from("levelno"), PyObject::int(level));
        attrs.insert(
            CompactString::from("levelname"),
            PyObject::str_val(CompactString::from(level_name)),
        );
        attrs.insert(
            CompactString::from("pathname"),
            PyObject::str_val(CompactString::from(pathname.as_str())),
        );
        attrs.insert(
            CompactString::from("filename"),
            PyObject::str_val(CompactString::from(pathname.as_str())),
        );
        attrs.insert(CompactString::from("lineno"), PyObject::int(lineno));
        attrs.insert(
            CompactString::from("msg"),
            PyObject::str_val(CompactString::from(msg.as_str())),
        );
        attrs.insert(
            CompactString::from("message"),
            PyObject::str_val(CompactString::from(msg.as_str())),
        );
        attrs.insert(
            CompactString::from("args"),
            if args.len() > 5 {
                args[5].clone()
            } else {
                PyObject::none()
            },
        );
        attrs.insert(
            CompactString::from("exc_info"),
            if args.len() > 6 {
                args[6].clone()
            } else {
                PyObject::none()
            },
        );
        attrs.insert(
            CompactString::from("funcName"),
            PyObject::str_val(CompactString::from("")),
        );
        attrs.insert(
            CompactString::from("module"),
            PyObject::str_val(CompactString::from("")),
        );
        let created = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        attrs.insert(CompactString::from("created"), PyObject::float(created));
        attrs.insert(
            CompactString::from("asctime"),
            PyObject::str_val(CompactString::from(current_asctime(None))),
        );
        attrs.insert(
            CompactString::from("msecs"),
            PyObject::float((created % 1.0) * 1000.0),
        );
        attrs.insert(CompactString::from("relativeCreated"), PyObject::float(0.0));
        attrs.insert(CompactString::from("thread"), PyObject::int(0));
        attrs.insert(
            CompactString::from("threadName"),
            PyObject::str_val(CompactString::from("MainThread")),
        );
        attrs.insert(
            CompactString::from("process"),
            PyObject::int(std::process::id() as i64),
        );
        attrs.insert(
            CompactString::from("processName"),
            PyObject::str_val(CompactString::from("MainProcess")),
        );
        let msg_clone = msg.clone();
        attrs.insert(
            CompactString::from("getMessage"),
            PyObject::native_closure("LogRecord.getMessage", move |_args| {
                Ok(PyObject::str_val(CompactString::from(msg_clone.clone())))
            }),
        );
        Ok(PyObject::instance_with_attrs(cls, attrs))
    });
    log_record_fn
}
