use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::rc::Rc;

mod factories;
mod format;
mod handlers;
mod logger;

use factories::{
    create_filter_fn, create_get_level_name_fn, create_log_record_fn, create_null_handler_fn,
};
use format::{apply_percent_format, current_asctime, format_log_message};
use handlers::{
    create_file_handler_fn, create_formatter_fn, create_handler_class,
    create_rotating_file_handler_fn, create_stream_handler_fn,
};
pub(super) use logger::logging_get_logger;
use logger::logging_log;

// ── logging module ──

// Global root logger config — basicConfig modifies this once
pub(super) static ROOT_CONFIGURED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
pub(super) static ROOT_LEVEL: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(30); // WARNING
pub(super) static ROOT_FORMAT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
/// Global disable threshold: logging.disable(level) sets this; messages at or below are suppressed.
pub(super) static DISABLE_LEVEL: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);

// Global logger registry: maps logger names to their PyObjectRef.
// Thread-local so each test / interpreter session gets its own registry.
thread_local! {
    static LOGGER_REGISTRY: std::cell::RefCell<HashMap<String, PyObjectRef>> =
        std::cell::RefCell::new(HashMap::new());
}

pub(super) fn root_format() -> &'static str {
    ROOT_FORMAT
        .get()
        .map(|s| s.as_str())
        .unwrap_or("%(levelname)s:%(name)s:%(message)s")
}

pub fn create_logging_module() -> PyObjectRef {
    // Logging levels
    let debug_level = PyObject::int(10);
    let info_level = PyObject::int(20);
    let warning_level = PyObject::int(30);
    let error_level = PyObject::int(40);
    let critical_level = PyObject::int(50);

    let stream_handler_fn = create_stream_handler_fn();
    let file_handler_fn = create_file_handler_fn();
    let rotating_file_handler_fn = create_rotating_file_handler_fn();
    let formatter_fn = create_formatter_fn();
    let handler_fn = create_handler_class();

    // basicConfig(**kwargs) — configure root logger (once)
    let basic_config_fn = make_builtin(|args: &[PyObjectRef]| {
        // Only configure once per CPython semantics
        if ROOT_CONFIGURED.get().is_some() {
            return Ok(PyObject::none());
        }

        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw_map) = &last.payload {
                let r = kw_map.read();
                if let Some(level) = r.get(&HashableKey::str_key(CompactString::from("level"))) {
                    if let Some(n) = level.as_int() {
                        ROOT_LEVEL.store(n, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                if let Some(format) = r.get(&HashableKey::str_key(CompactString::from("format"))) {
                    let _ = ROOT_FORMAT.set(format.py_to_string().to_string());
                }
                // filename= creates a FileHandler on the root logger
                if let Some(filename) =
                    r.get(&HashableKey::str_key(CompactString::from("filename")))
                {
                    let fname = filename.py_to_string();
                    let filemode = r
                        .get(&HashableKey::str_key(CompactString::from("filemode")))
                        .map(|v| v.py_to_string())
                        .unwrap_or_else(|| "a".to_string());
                    // Create a FileHandler and add it to the root logger
                    let fmt_ref: Rc<PyCell<PyObjectRef>> = Rc::new(PyCell::new(PyObject::none()));
                    // If format= was provided, build a Formatter and attach it
                    if let Some(format_val) =
                        r.get(&HashableKey::str_key(CompactString::from("format")))
                    {
                        let fs = format_val.py_to_string();
                        let fmt_cls = PyObject::class(
                            CompactString::from("Formatter"),
                            vec![],
                            IndexMap::new(),
                        );
                        let fmt_inst = PyObject::instance(fmt_cls);
                        if let PyObjectPayload::Instance(ref fd) = fmt_inst.payload {
                            fd.attrs.write().insert(
                                CompactString::from("_fmt"),
                                PyObject::str_val(CompactString::from(fs)),
                            );
                        }
                        *fmt_ref.write() = fmt_inst;
                    }
                    let fr2 = fmt_ref.clone();
                    let fname_c = CompactString::from(fname.clone());
                    let fmode_c = filemode.clone();
                    let fh_cls = PyObject::class(
                        CompactString::from("FileHandler"),
                        vec![],
                        IndexMap::new(),
                    );
                    let fh_inst = PyObject::instance(fh_cls);
                    if let PyObjectPayload::Instance(ref fd) = fh_inst.payload {
                        let mut attrs = fd.attrs.write();
                        attrs.insert(
                            CompactString::from("baseFilename"),
                            PyObject::str_val(fname_c.clone()),
                        );
                        attrs.insert(CompactString::from("level"), PyObject::int(0));
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
                                let fmt = fr2.read().clone();
                                let formatted = if !matches!(&fmt.payload, PyObjectPayload::None) {
                                    if let Some(fmt_str) = fmt.get_attr("_fmt") {
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
                                        result =
                                            result.replace("%(asctime)s", &current_asctime(None));
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
                                use std::io::Write;
                                let line = format!("{}\n", formatted);
                                let result = if fmode_c == "w" {
                                    std::fs::write(fname_c.as_str(), &line)
                                } else {
                                    std::fs::OpenOptions::new()
                                        .create(true)
                                        .append(true)
                                        .open(fname_c.as_str())
                                        .and_then(|mut f| f.write_all(line.as_bytes()))
                                };
                                if let Err(e) = result {
                                    eprintln!("FileHandler error: {}", e);
                                }
                                Ok(PyObject::none())
                            }),
                        );
                    }
                    // Add the handler to the root logger (create it if needed)
                    let root_exists = LOGGER_REGISTRY.with(|reg| reg.borrow().contains_key("root"));
                    if !root_exists {
                        // Create the root logger via logging_get_logger
                        let _ = logging_get_logger(&[]);
                    }
                    LOGGER_REGISTRY.with(|reg| {
                        let reg = reg.borrow();
                        if let Some(root) = reg.get("root") {
                            if let Some(handlers) = root.get_attr("handlers") {
                                if let PyObjectPayload::List(items) = &handlers.payload {
                                    items.write().push(fh_inst.clone());
                                }
                            }
                        }
                    });
                }
                // handlers= kwarg: add each handler to the root logger
                if let Some(handlers_val) =
                    r.get(&HashableKey::str_key(CompactString::from("handlers")))
                {
                    if let Ok(handler_list) = handlers_val.to_list() {
                        let root_exists =
                            LOGGER_REGISTRY.with(|reg| reg.borrow().contains_key("root"));
                        if !root_exists {
                            let _ = logging_get_logger(&[]);
                        }
                        LOGGER_REGISTRY.with(|reg| {
                            let reg = reg.borrow();
                            if let Some(root) = reg.get("root") {
                                if let Some(root_handlers) = root.get_attr("handlers") {
                                    if let PyObjectPayload::List(items) = &root_handlers.payload {
                                        let mut w = items.write();
                                        for h in handler_list {
                                            w.push(h);
                                        }
                                    }
                                }
                            }
                        });
                    }
                }
            }
        }
        let _ = ROOT_CONFIGURED.set(());
        Ok(PyObject::none())
    });

    let null_handler_fn = create_null_handler_fn();

    let get_level_name_fn = create_get_level_name_fn();

    let filter_fn = create_filter_fn();

    let log_record_fn = create_log_record_fn();

    // disable(level) — set the global disable level
    let disable_fn = make_builtin(|args: &[PyObjectRef]| {
        if let Some(n) = args.first().and_then(|a| a.as_int()) {
            DISABLE_LEVEL.store(n, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(PyObject::none())
    });

    make_module(
        "logging",
        vec![
            ("DEBUG", debug_level),
            ("INFO", info_level),
            ("WARNING", warning_level.clone()),
            ("WARN", warning_level),
            ("ERROR", error_level),
            ("CRITICAL", critical_level),
            ("FATAL", PyObject::int(50)),
            ("NOTSET", PyObject::int(0)),
            ("basicConfig", basic_config_fn),
            ("getLogger", make_builtin(logging_get_logger)),
            ("getLevelName", get_level_name_fn),
            ("debug", make_builtin(|args| logging_log(10, args))),
            ("info", make_builtin(|args| logging_log(20, args))),
            ("warning", make_builtin(|args| logging_log(30, args))),
            ("error", make_builtin(|args| logging_log(40, args))),
            ("critical", make_builtin(|args| logging_log(50, args))),
            (
                "log",
                make_builtin(|args| {
                    if args.len() >= 2 {
                        let level = args[0].as_int().unwrap_or(20);
                        logging_log(level, &args[1..])
                    } else {
                        Ok(PyObject::none())
                    }
                }),
            ),
            ("disable", disable_fn),
            ("StreamHandler", stream_handler_fn),
            ("FileHandler", file_handler_fn),
            ("RotatingFileHandler", rotating_file_handler_fn),
            ("Formatter", formatter_fn),
            ("Handler", handler_fn),
            ("NullHandler", null_handler_fn),
            ("Filter", filter_fn),
            ("LogRecord", log_record_fn),
            ("Logger", {
                // Logger is a class that, when called, creates logger instances
                let mut logger_ns = IndexMap::new();
                logger_ns.insert(
                    CompactString::from("__init__"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
                logger_ns.insert(
                    CompactString::from("__call__"),
                    make_builtin(logging_get_logger),
                );
                PyObject::class(CompactString::from("Logger"), vec![], logger_ns)
            }),
            ("root", make_builtin(|_| logging_get_logger(&[]))),
            ("addLevelName", make_builtin(|_args| Ok(PyObject::none()))),
            ("setLoggerClass", make_builtin(|_args| Ok(PyObject::none()))),
            ("Filterer", {
                let mut filterer_ns = IndexMap::new();
                filterer_ns.insert(CompactString::from("filters"), PyObject::list(vec![]));
                filterer_ns.insert(
                    CompactString::from("addFilter"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
                filterer_ns.insert(
                    CompactString::from("removeFilter"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
                filterer_ns.insert(
                    CompactString::from("filter"),
                    make_builtin(|_| Ok(PyObject::bool_val(true))),
                );
                PyObject::class(CompactString::from("Filterer"), vec![], filterer_ns)
            }),
            ("lastResort", PyObject::none()),
            ("raiseExceptions", PyObject::bool_val(true)),
            (
                "captureWarnings",
                make_builtin(|_args| Ok(PyObject::none())),
            ),
            ("shutdown", make_builtin(|_args| Ok(PyObject::none()))),
            (
                "makeLogRecord",
                make_builtin(|args| {
                    if !args.is_empty() {
                        Ok(args[0].clone())
                    } else {
                        Ok(PyObject::none())
                    }
                }),
            ),
        ],
    )
}
