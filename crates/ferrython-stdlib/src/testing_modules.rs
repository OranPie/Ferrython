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

// Global root logger config — basicConfig modifies this once
static ROOT_CONFIGURED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
static ROOT_LEVEL: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(30); // WARNING
static ROOT_FORMAT: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn root_format() -> &'static str {
    ROOT_FORMAT.get().map(|s| s.as_str()).unwrap_or("%(levelname)s:%(name)s:%(message)s")
}

fn format_log_message(fmt: &str, level_name: &str, name: &str, msg: &str) -> String {
    fmt.replace("%(levelname)s", level_name)
       .replace("%(name)s", name)
       .replace("%(message)s", msg)
       .replace("%(asctime)s", "")
       .replace("%(lineno)d", "0")
       .replace("%(filename)s", "")
       .replace("%(funcName)s", "")
       .replace("%(module)s", "")
       .replace("%(pathname)s", "")
}

/// Apply Python %-style formatting: "Hello %s" % ("world",) → "Hello world"
fn apply_percent_format(fmt: &str, args: &[PyObjectRef]) -> String {
    if args.is_empty() { return fmt.to_string(); }
    let mut result = String::with_capacity(fmt.len() + 32);
    let mut chars = fmt.chars().peekable();
    let mut arg_idx = 0;
    while let Some(ch) = chars.next() {
        if ch == '%' {
            if let Some(&next) = chars.peek() {
                match next {
                    's' => {
                        chars.next();
                        if arg_idx < args.len() {
                            result.push_str(&args[arg_idx].py_to_string());
                            arg_idx += 1;
                        } else {
                            result.push_str("%s");
                        }
                    }
                    'd' | 'i' => {
                        chars.next();
                        if arg_idx < args.len() {
                            result.push_str(&format!("{}", args[arg_idx].as_int().unwrap_or(0)));
                            arg_idx += 1;
                        } else {
                            result.push('%'); result.push(next);
                        }
                    }
                    'f' => {
                        chars.next();
                        if arg_idx < args.len() {
                            let val = args[arg_idx].to_float().unwrap_or(0.0);
                            result.push_str(&format!("{:.6}", val));
                            arg_idx += 1;
                        } else {
                            result.push_str("%f");
                        }
                    }
                    'r' => {
                        chars.next();
                        if arg_idx < args.len() {
                            result.push_str(&format!("'{}'", args[arg_idx].py_to_string()));
                            arg_idx += 1;
                        } else {
                            result.push_str("%r");
                        }
                    }
                    '.' => {
                        // Handle %.Nf format specifiers
                        chars.next();
                        let mut precision = String::new();
                        while let Some(&c) = chars.peek() {
                            if c.is_ascii_digit() {
                                precision.push(c);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        if let Some(&fmt_char) = chars.peek() {
                            chars.next();
                            if fmt_char == 'f' && arg_idx < args.len() {
                                let prec: usize = precision.parse().unwrap_or(6);
                                let val = args[arg_idx].to_float().unwrap_or(0.0);
                                result.push_str(&format!("{:.prec$}", val, prec = prec));
                                arg_idx += 1;
                            } else {
                                result.push('%');
                                result.push('.');
                                result.push_str(&precision);
                                result.push(fmt_char);
                            }
                        }
                    }
                    '%' => {
                        chars.next();
                        result.push('%');
                    }
                    _ => {
                        result.push('%');
                    }
                }
            } else {
                result.push('%');
            }
        } else {
            result.push(ch);
        }
    }
    result
}

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
            // mode: 'a' (append) by default, 'w' for truncate
            let mode = if args.len() > 1 {
                args[1].py_to_string()
            } else { "a".to_string() };
            attrs.insert(CompactString::from("baseFilename"), PyObject::str_val(filename.clone()));
            attrs.insert(CompactString::from("mode"), PyObject::str_val(CompactString::from(&mode)));
            attrs.insert(CompactString::from("level"), PyObject::int(0));
            attrs.insert(CompactString::from("formatter"), PyObject::none());

            // Shared formatter/level refs for closures
            let fmt_ref: Arc<RwLock<PyObjectRef>> = Arc::new(RwLock::new(PyObject::none()));
            let level_ref: Arc<RwLock<i64>> = Arc::new(RwLock::new(0));

            let lr = level_ref.clone();
            attrs.insert(CompactString::from("setLevel"), PyObject::native_closure(
                "setLevel", move |args: &[PyObjectRef]| {
                    if let Some(v) = args.first().and_then(|a| a.as_int()) {
                        *lr.write() = v;
                    }
                    Ok(PyObject::none())
                }
            ));
            let fr = fmt_ref.clone();
            attrs.insert(CompactString::from("setFormatter"), PyObject::native_closure(
                "setFormatter", move |args: &[PyObjectRef]| {
                    if let Some(v) = args.first() { *fr.write() = v.clone(); }
                    Ok(PyObject::none())
                }
            ));
            // emit(record) — write formatted message to file
            let fr2 = fmt_ref.clone();
            let fname = filename.clone();
            let fmode = mode.clone();
            attrs.insert(CompactString::from("emit"), PyObject::native_closure(
                "emit", move |args: &[PyObjectRef]| {
                    let record = if args.len() >= 2 { &args[1] } else if !args.is_empty() { &args[0] } else {
                        return Ok(PyObject::none());
                    };
                    let msg = if let Some(m) = record.get_attr("message") {
                        m.py_to_string()
                    } else if let Some(m) = record.get_attr("msg") {
                        m.py_to_string()
                    } else { record.py_to_string() };

                    // Apply formatter
                    let fmt = fr2.read().clone();
                    let formatted = if !matches!(&fmt.payload, PyObjectPayload::None) {
                        if let Some(fmt_str) = fmt.get_attr("_fmt") {
                            let fs = fmt_str.py_to_string();
                            let mut result = fs.clone();
                            result = result.replace("%(message)s", &msg);
                            let levelname = record.get_attr("levelname").map(|l| l.py_to_string()).unwrap_or_else(|| "INFO".to_string());
                            let name = record.get_attr("name").map(|n| n.py_to_string()).unwrap_or_else(|| "root".to_string());
                            result = result.replace("%(levelname)s", &levelname);
                            result = result.replace("%(name)s", &name);
                            result
                        } else { msg.clone() }
                    } else { msg.clone() };

                    // Write to file
                    use std::io::Write;
                    let line = format!("{}\n", formatted);
                    let result = if fmode == "w" {
                        std::fs::write(fname.as_str(), &line)
                    } else {
                        std::fs::OpenOptions::new()
                            .create(true).append(true)
                            .open(fname.as_str())
                            .and_then(|mut f| f.write_all(line.as_bytes()))
                    };
                    if let Err(e) = result {
                        eprintln!("FileHandler error: {}", e);
                    }
                    Ok(PyObject::none())
                }
            ));
            // close() — no-op (file is opened/closed per emit)
            attrs.insert(CompactString::from("close"), make_builtin(|_| Ok(PyObject::none())));
        }
        Ok(inst)
    });

    // RotatingFileHandler(filename, mode='a', maxBytes=0, backupCount=0)
    let rfh_cls = PyObject::class(CompactString::from("RotatingFileHandler"), vec![], IndexMap::new());
    let rfh_cls2 = rfh_cls.clone();
    let rotating_file_handler_fn = PyObject::native_closure("RotatingFileHandler", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(rfh_cls2.clone());
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let filename = if args.is_empty() { CompactString::from("") } else { CompactString::from(args[0].py_to_string()) };
            let max_bytes: i64 = if args.len() > 2 { args[2].as_int().unwrap_or(0) } else { 0 };
            let backup_count: i64 = if args.len() > 3 { args[3].as_int().unwrap_or(0) } else { 0 };

            attrs.insert(CompactString::from("baseFilename"), PyObject::str_val(filename.clone()));
            attrs.insert(CompactString::from("maxBytes"), PyObject::int(max_bytes));
            attrs.insert(CompactString::from("backupCount"), PyObject::int(backup_count));
            attrs.insert(CompactString::from("level"), PyObject::int(0));
            attrs.insert(CompactString::from("formatter"), PyObject::none());

            let fmt_ref: Arc<RwLock<PyObjectRef>> = Arc::new(RwLock::new(PyObject::none()));
            let fr = fmt_ref.clone();
            attrs.insert(CompactString::from("setLevel"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("setFormatter"), PyObject::native_closure(
                "setFormatter", move |args: &[PyObjectRef]| {
                    if let Some(v) = args.first() { *fr.write() = v.clone(); }
                    Ok(PyObject::none())
                }
            ));

            // emit with rotation
            let fr2 = fmt_ref.clone();
            let fname = filename.clone();
            attrs.insert(CompactString::from("emit"), PyObject::native_closure(
                "emit", move |args: &[PyObjectRef]| {
                    let record = if args.len() >= 2 { &args[1] } else if !args.is_empty() { &args[0] } else {
                        return Ok(PyObject::none());
                    };
                    let msg = record.get_attr("message")
                        .or_else(|| record.get_attr("msg"))
                        .map(|m| m.py_to_string())
                        .unwrap_or_else(|| record.py_to_string());
                    let fmt = fr2.read().clone();
                    let formatted = if !matches!(&fmt.payload, PyObjectPayload::None) {
                        if let Some(fmt_str) = fmt.get_attr("_fmt") {
                            let fs = fmt_str.py_to_string();
                            fs.replace("%(message)s", &msg)
                              .replace("%(levelname)s", &record.get_attr("levelname").map(|l| l.py_to_string()).unwrap_or_else(|| "INFO".to_string()))
                              .replace("%(name)s", &record.get_attr("name").map(|n| n.py_to_string()).unwrap_or_else(|| "root".to_string()))
                        } else { msg.clone() }
                    } else { msg.clone() };

                    // Check rotation
                    if max_bytes > 0 {
                        let current_size = std::fs::metadata(fname.as_str()).map(|m| m.len() as i64).unwrap_or(0);
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
                    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(fname.as_str()) {
                        let _ = f.write_all(line.as_bytes());
                    }
                    Ok(PyObject::none())
                }
            ));
            attrs.insert(CompactString::from("close"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("doRollover"), make_builtin(|_| Ok(PyObject::none())));
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
            let level = Arc::new(std::sync::atomic::AtomicI64::new(0));
            attrs.insert(CompactString::from("level"), PyObject::int(0));
            let lv = level.clone();
            attrs.insert(CompactString::from("setLevel"), PyObject::native_closure(
                "setLevel", move |args: &[PyObjectRef]| {
                    if let Some(v) = args.first().and_then(|a| a.as_int()) {
                        lv.store(v, std::sync::atomic::Ordering::Relaxed);
                    }
                    Ok(PyObject::none())
                }));
            let formatter_ref: Arc<RwLock<PyObjectRef>> = Arc::new(RwLock::new(PyObject::none()));
            let fr = formatter_ref.clone();
            attrs.insert(CompactString::from("setFormatter"), PyObject::native_closure(
                "setFormatter", move |args: &[PyObjectRef]| {
                    if let Some(v) = args.first() {
                        *fr.write() = v.clone();
                    }
                    Ok(PyObject::none())
                }));
            attrs.insert(CompactString::from("formatter"), PyObject::none());
        }
        Ok(inst)
    });

    // basicConfig(**kwargs) — configure root logger (once)
    let basic_config_fn = make_builtin(|args: &[PyObjectRef]| {
        // Only configure once per CPython semantics
        if ROOT_CONFIGURED.get().is_some() { return Ok(PyObject::none()); }
        
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw_map) = &last.payload {
                let r = kw_map.read();
                if let Some(level) = r.get(&HashableKey::Str(CompactString::from("level"))) {
                    if let Some(n) = level.as_int() {
                        ROOT_LEVEL.store(n, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                if let Some(format) = r.get(&HashableKey::Str(CompactString::from("format"))) {
                    let _ = ROOT_FORMAT.set(format.py_to_string().to_string());
                }
            }
        }
        let _ = ROOT_CONFIGURED.set(());
        Ok(PyObject::none())
    });

    // NullHandler — discards all log records
    let null_handler_cls = PyObject::class(CompactString::from("NullHandler"), vec![], IndexMap::new());
    let nh_cls = null_handler_cls.clone();
    let null_handler_fn = PyObject::native_closure("NullHandler", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(nh_cls.clone());
        if let PyObjectPayload::Instance(ref data) = inst.payload {
            let mut attrs = data.attrs.write();
            attrs.insert(CompactString::from("level"), PyObject::int(0));
            attrs.insert(CompactString::from("emit"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("handle"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("setLevel"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("setFormatter"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("createLock"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("acquire"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("release"), make_builtin(|_| Ok(PyObject::none())));
        }
        Ok(inst)
    });

    // getLevelName
    let get_level_name_fn = make_builtin(|args: &[PyObjectRef]| {
        if let Some(v) = args.first().and_then(|a| a.as_int()) {
            let name = match v {
                10 => "DEBUG", 20 => "INFO", 30 => "WARNING",
                40 => "ERROR", 50 => "CRITICAL", 0 => "NOTSET",
                _ => return Ok(PyObject::str_val(CompactString::from(format!("Level {}", v)))),
            };
            Ok(PyObject::str_val(CompactString::from(name)))
        } else if let Some(s) = args.first() {
            let name = s.py_to_string();
            let level = match name.as_ref() {
                "DEBUG" => 10, "INFO" => 20, "WARNING" => 30,
                "ERROR" => 40, "CRITICAL" => 50, "NOTSET" => 0,
                _ => return Err(PyException::value_error(format!("Unknown level: '{}'", name))),
            };
            Ok(PyObject::int(level))
        } else {
            Ok(PyObject::none())
        }
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
        ("getLevelName", get_level_name_fn),
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
        ("RotatingFileHandler", rotating_file_handler_fn),
        ("Formatter", formatter_fn),
        ("Handler", handler_fn),
        ("NullHandler", null_handler_fn),
        ("Logger", make_builtin(logging_get_logger)),
    ])
}

fn logging_log(level: i64, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Ok(PyObject::none()); }
    // Respect root logger level from basicConfig
    let root_level = ROOT_LEVEL.load(std::sync::atomic::Ordering::Relaxed);
    if root_level > 0 && level < root_level { return Ok(PyObject::none()); }
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
    let formatted = format_log_message(root_format(), level_name, "root", &msg);
    eprintln!("{}", formatted);
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
    let root_level = ROOT_LEVEL.load(std::sync::atomic::Ordering::Relaxed);
    let initial_level = if root_level > 0 { root_level } else { 30 };
    let effective_level: Arc<RwLock<i64>> = Arc::new(RwLock::new(initial_level));
    ns.insert(CompactString::from("level"), PyObject::int(initial_level));
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
            let msg_fmt = args[0].py_to_string();
            let msg = if args.len() > 1 {
                apply_percent_format(&msg_fmt, &args[1..])
            } else {
                msg_fmt
            };

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

    // setLevel — placeholder (patched after instance creation to update .level attr)
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
    // Patch setLevel to also update the visible .level attribute
    {
        let el_patch = effective_level.clone();
        let inst_ref = inst.clone();
        let set_level_fn = PyObject::native_closure("setLevel", move |args: &[PyObjectRef]| {
            if let Some(v) = args.first() {
                if let Some(n) = v.as_int() {
                    *el_patch.write() = n;
                    if let PyObjectPayload::Instance(ref data) = inst_ref.payload {
                        data.attrs.write().insert(CompactString::from("level"), PyObject::int(n));
                    }
                }
            }
            Ok(PyObject::none())
        });
        if let PyObjectPayload::Instance(inst_data) = &inst.payload {
            inst_data.attrs.write().insert(CompactString::from("setLevel"), set_level_fn);
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
        ("TestSuite", make_builtin(|args| {
            let tests: Vec<PyObjectRef> = if !args.is_empty() {
                args[0].to_list().unwrap_or_default()
            } else {
                vec![]
            };
            let test_list = Arc::new(RwLock::new(tests));
            let mut attrs = IndexMap::new();
            let tl = test_list.clone();
            attrs.insert(CompactString::from("_tests"), PyObject::list(tl.read().clone()));
            let tl = test_list.clone();
            attrs.insert(CompactString::from("addTest"), PyObject::native_closure("addTest", move |args| {
                if !args.is_empty() { tl.write().push(args[0].clone()); }
                Ok(PyObject::none())
            }));
            let tl = test_list.clone();
            attrs.insert(CompactString::from("__iter__"), PyObject::native_closure("__iter__", move |_| {
                Ok(PyObject::list(tl.read().clone()).get_iter()?)
            }));
            let tl = test_list.clone();
            attrs.insert(CompactString::from("__len__"), PyObject::native_closure("__len__", move |_| {
                Ok(PyObject::int(tl.read().len() as i64))
            }));
            let tl = test_list.clone();
            attrs.insert(CompactString::from("countTestCases"), PyObject::native_closure("countTestCases", move |_| {
                Ok(PyObject::int(tl.read().len() as i64))
            }));
            Ok(PyObject::module_with_attrs(CompactString::from("TestSuite"), attrs))
        })),
        ("TestLoader", make_builtin(|_| {
            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("loadTestsFromTestCase"), make_builtin(|args| {
                if args.is_empty() {
                    return Err(PyException::type_error("loadTestsFromTestCase() requires a TestCase class"));
                }
                let cls = &args[0];
                let mut tests = vec![];
                // Get test methods from the class namespace
                if let PyObjectPayload::Class(cls_data) = &cls.payload {
                    let ns = cls_data.namespace.read();
                    for (name, _) in ns.iter() {
                        if name.starts_with("test") {
                            tests.push(PyObject::str_val(CompactString::from(name.as_str())));
                        }
                    }
                }
                Ok(PyObject::list(tests))
            }));
            Ok(PyObject::module_with_attrs(CompactString::from("TestLoader"), attrs))
        })),
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

/// Create a Mock/MagicMock instance with proper dynamic attribute access,
/// return_value support, call tracking, and assertion methods.
fn build_mock_instance(name: &str, kwargs: &IndexMap<HashableKey, PyObjectRef>) -> PyObjectRef {
    let cls = PyObject::class(CompactString::from(name), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();

        // Shared state
        let call_count: Arc<RwLock<i64>> = Arc::new(RwLock::new(0));
        let call_args_list: Arc<RwLock<Vec<PyObjectRef>>> = Arc::new(RwLock::new(vec![]));
        // Extract return_value from kwargs if provided
        let init_rv = kwargs.get(&HashableKey::Str(CompactString::from("return_value")))
            .cloned().unwrap_or_else(PyObject::none);
        let return_value: Arc<RwLock<PyObjectRef>> = Arc::new(RwLock::new(init_rv));
        // Child mock cache for __getattr__ — dynamically created children
        let children: Arc<RwLock<IndexMap<String, PyObjectRef>>> = Arc::new(RwLock::new(IndexMap::new()));
        let mock_name = CompactString::from(name);

        // return_value property (readable)
        let rv = return_value.clone();
        w.insert(CompactString::from("return_value"), PyObject::native_closure(
            "Mock.return_value", move |_: &[PyObjectRef]| Ok(rv.read().clone())
        ));

        // call_count property
        let cc = call_count.clone();
        w.insert(CompactString::from("call_count"), PyObject::native_closure(
            "Mock.call_count", move |_: &[PyObjectRef]| Ok(PyObject::int(*cc.read()))
        ));

        // call_args_list property
        let cal = call_args_list.clone();
        w.insert(CompactString::from("call_args_list"), PyObject::native_closure(
            "Mock.call_args_list", move |_: &[PyObjectRef]| Ok(PyObject::list(cal.read().clone()))
        ));

        // called property
        w.insert(CompactString::from("called"), PyObject::native_closure(
            "Mock.called", {
                let cc2 = call_count.clone();
                move |_: &[PyObjectRef]| Ok(PyObject::bool_val(*cc2.read() > 0))
            }
        ));

        // side_effect support
        let side_effect: Arc<RwLock<Option<PyObjectRef>>> = Arc::new(RwLock::new(
            kwargs.get(&HashableKey::Str(CompactString::from("side_effect"))).cloned()
        ));
        let se = side_effect.clone();
        w.insert(CompactString::from("side_effect"), PyObject::native_closure(
            "Mock.side_effect", move |_: &[PyObjectRef]| {
                Ok(se.read().clone().unwrap_or_else(PyObject::none))
            }
        ));

        // __call__ — tracks calls and returns return_value (or raises side_effect)
        let cc3 = call_count.clone();
        let cal2 = call_args_list.clone();
        let rv2 = return_value.clone();
        let se2 = side_effect.clone();
        w.insert(CompactString::from("__call__"), PyObject::native_closure(
            "Mock.__call__", move |args: &[PyObjectRef]| {
                *cc3.write() += 1;
                cal2.write().push(PyObject::tuple(args.to_vec()));
                // Check side_effect
                if let Some(ref effect) = *se2.read() {
                    if let PyObjectPayload::List(items) = &effect.payload {
                        let idx = (*cc3.read() - 1) as usize;
                        let items_r = items.read();
                        if idx < items_r.len() {
                            return Ok(items_r[idx].clone());
                        }
                    }
                }
                Ok(rv2.read().clone())
            }
        ));

        // __getattr__ — dynamically create child mocks for unknown attributes
        let children2 = children.clone();
        let mn = mock_name.clone();
        w.insert(CompactString::from("__getattr__"), PyObject::native_closure(
            "Mock.__getattr__", move |args: &[PyObjectRef]| {
                let attr_name = if !args.is_empty() { args[0].py_to_string() } else { return Ok(PyObject::none()); };
                // Don't intercept dunder methods or known internals
                if attr_name.starts_with("__") && attr_name.ends_with("__") {
                    return Err(PyException::attribute_error(format!(
                        "'{}' object has no attribute '{}'", mn, attr_name)));
                }
                let mut cache = children2.write();
                if let Some(child) = cache.get(&attr_name) {
                    return Ok(child.clone());
                }
                // Create a new child mock
                let child = build_mock_instance("MagicMock", &IndexMap::new());
                cache.insert(attr_name, child.clone());
                Ok(child)
            }
        ));

        // __setattr__ — intercept return_value assignment and child mock setting
        let rv_set = return_value.clone();
        let children3 = children.clone();
        w.insert(CompactString::from("__setattr__"), PyObject::native_closure(
            "Mock.__setattr__", move |args: &[PyObjectRef]| {
                if args.len() < 2 { return Ok(PyObject::none()); }
                let attr_name = args[0].py_to_string();
                let value = args[1].clone();
                if attr_name == "return_value" {
                    *rv_set.write() = value;
                } else if attr_name == "side_effect" {
                    // Would need side_effect arc here too, but for simplicity store as child
                    children3.write().insert(attr_name, value);
                } else {
                    children3.write().insert(attr_name, value);
                }
                Ok(PyObject::none())
            }
        ));

        // assert_called() — raises AssertionError if not called
        let cc_ac = call_count.clone();
        w.insert(CompactString::from("assert_called"), PyObject::native_closure(
            "Mock.assert_called", move |_: &[PyObjectRef]| {
                if *cc_ac.read() == 0 {
                    return Err(PyException::assertion_error("Expected mock to have been called."));
                }
                Ok(PyObject::none())
            }
        ));

        // assert_called_once() — raises if call_count != 1
        let cc_aco = call_count.clone();
        w.insert(CompactString::from("assert_called_once"), PyObject::native_closure(
            "Mock.assert_called_once", move |_: &[PyObjectRef]| {
                let count = *cc_aco.read();
                if count != 1 {
                    return Err(PyException::assertion_error(format!(
                        "Expected mock to have been called once. Called {} times.", count)));
                }
                Ok(PyObject::none())
            }
        ));

        // assert_called_with(*args) — check last call matches
        let cal_acw = call_args_list.clone();
        w.insert(CompactString::from("assert_called_with"), PyObject::native_closure(
            "Mock.assert_called_with", move |args: &[PyObjectRef]| {
                let history = cal_acw.read();
                if history.is_empty() {
                    return Err(PyException::assertion_error("Expected mock to have been called."));
                }
                let last_call = history.last().unwrap();
                let expected = PyObject::tuple(args.to_vec());
                if last_call.py_to_string() != expected.py_to_string() {
                    return Err(PyException::assertion_error(format!(
                        "expected call: mock{}\nActual call: mock{}",
                        expected.py_to_string(), last_call.py_to_string())));
                }
                Ok(PyObject::none())
            }
        ));

        // assert_not_called()
        let cc_anc = call_count.clone();
        w.insert(CompactString::from("assert_not_called"), PyObject::native_closure(
            "Mock.assert_not_called", move |_: &[PyObjectRef]| {
                let count = *cc_anc.read();
                if count > 0 {
                    return Err(PyException::assertion_error(format!(
                        "Expected mock to not have been called. Called {} times.", count)));
                }
                Ok(PyObject::none())
            }
        ));

        // reset_mock() — clear all tracking state
        let cc_rm = call_count.clone();
        let cal_rm = call_args_list.clone();
        let rv_rm = return_value.clone();
        let ch_rm = children.clone();
        w.insert(CompactString::from("reset_mock"), PyObject::native_closure(
            "Mock.reset_mock", move |_: &[PyObjectRef]| {
                *cc_rm.write() = 0;
                cal_rm.write().clear();
                *rv_rm.write() = PyObject::none();
                ch_rm.write().clear();
                Ok(PyObject::none())
            }
        ));
    }
    inst
}

/// Extract kwargs dict from trailing argument (VM passes kwargs as last Dict arg)
fn extract_mock_kwargs(args: &[PyObjectRef]) -> IndexMap<HashableKey, PyObjectRef> {
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(kw_map) = &last.payload {
            return kw_map.read().clone();
        }
    }
    IndexMap::new()
}

pub fn create_unittest_mock_module() -> PyObjectRef {
    let make_mock = |name: &'static str| -> PyObjectRef {
        PyObject::native_closure(name, move |args: &[PyObjectRef]| {
            let kwargs = extract_mock_kwargs(args);
            Ok(build_mock_instance(name, &kwargs))
        })
    };

    // patch function — context manager that temporarily replaces a target attribute
    let patch_fn = make_builtin(|args: &[PyObjectRef]| {
        let target = if !args.is_empty() { args[0].py_to_string() } else { String::new() };
        let kwargs = extract_mock_kwargs(args);
        let cls = PyObject::class(CompactString::from("_patch"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("attribute"), PyObject::str_val(CompactString::from(target.as_str())));
            let mock_for_enter = build_mock_instance("MagicMock", &kwargs);
            let mfe = mock_for_enter.clone();
            w.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "patch.__enter__", move |_: &[PyObjectRef]| Ok(mfe.clone())
            ));
            w.insert(CompactString::from("__exit__"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::bool_val(false))));
            // As decorator: patch(target)(func) → wrapped func
            let mock_for_deco = mock_for_enter;
            w.insert(CompactString::from("__call__"), PyObject::native_closure(
                "patch.__call__", move |args: &[PyObjectRef]| {
                    if !args.is_empty() {
                        // Decorator mode: return the function unchanged (mock passed as extra arg)
                        Ok(args[0].clone())
                    } else {
                        Ok(mock_for_deco.clone())
                    }
                }
            ));
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
    // profile.run(statement) — execute and print simple timing
    let run_fn = make_builtin(|args: &[PyObjectRef]| {
        if !args.is_empty() {
            let stmt = args[0].py_to_string();
            eprintln!("         1 function calls in 0.000 seconds");
            eprintln!("");
            eprintln!("   Ordered by: standard name");
            eprintln!("");
            eprintln!("   ncalls  tottime  percall  cumtime  percall filename:lineno(function)");
            eprintln!("        1    0.000    0.000    0.000    0.000 <string>:1({})", stmt);
            eprintln!("        1    0.000    0.000    0.000    0.000 {{method 'disable' of '_lsprof.Profiler' objects}}");
        }
        Ok(PyObject::none())
    });

    let profile_cls_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("Profile"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            // Track timing state
            let stats: Arc<RwLock<Vec<(String, f64)>>> = Arc::new(RwLock::new(Vec::new()));
            let enabled: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));
            let start_time: Arc<RwLock<Option<std::time::Instant>>> = Arc::new(RwLock::new(None));

            let e = enabled.clone();
            let st = start_time.clone();
            w.insert(CompactString::from("enable"), PyObject::native_closure(
                "enable", move |_: &[PyObjectRef]| {
                    *e.write() = true;
                    *st.write() = Some(std::time::Instant::now());
                    Ok(PyObject::none())
                }
            ));
            let e2 = enabled.clone();
            let st2 = start_time.clone();
            let stats2 = stats.clone();
            w.insert(CompactString::from("disable"), PyObject::native_closure(
                "disable", move |_: &[PyObjectRef]| {
                    *e2.write() = false;
                    if let Some(start) = st2.read().as_ref() {
                        stats2.write().push(("profiling".to_string(), start.elapsed().as_secs_f64()));
                    }
                    Ok(PyObject::none())
                }
            ));
            // runcall(func, *args) — call func and profile it
            let stats3 = stats.clone();
            w.insert(CompactString::from("runcall"), PyObject::native_closure(
                "runcall", move |args: &[PyObjectRef]| {
                    if args.is_empty() { return Ok(PyObject::none()); }
                    let func = &args[0];
                    let func_args = if args.len() > 1 { &args[1..] } else { &[] };
                    let start = std::time::Instant::now();
                    let result = match &func.payload {
                        PyObjectPayload::NativeFunction { func: f, .. } => f(func_args)?,
                        PyObjectPayload::NativeClosure { func: f, .. } => f(func_args)?,
                        _ => PyObject::none(),
                    };
                    stats3.write().push(("runcall".to_string(), start.elapsed().as_secs_f64()));
                    Ok(result)
                }
            ));
            let stats4 = stats.clone();
            w.insert(CompactString::from("print_stats"), PyObject::native_closure(
                "print_stats", move |args: &[PyObjectRef]| {
                    let sort_key = if !args.is_empty() { args[0].py_to_string() } else { "cumulative".to_string() };
                    let st = stats4.read();
                    let total: f64 = st.iter().map(|(_, t)| t).sum();
                    eprintln!("         {} function calls in {:.3} seconds", st.len().max(1), total);
                    eprintln!("");
                    eprintln!("   Ordered by: {}", sort_key);
                    eprintln!("");
                    eprintln!("   ncalls  tottime  percall  cumtime  percall filename:lineno(function)");
                    for (name, time) in st.iter() {
                        eprintln!("        1    {:.3}    {:.3}    {:.3}    {:.3} <string>:1({})", time, time, time, time, name);
                    }
                    Ok(PyObject::none())
                }
            ));
            w.insert(CompactString::from("run"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
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
        if !args.is_empty() {
            let stmt = args[0].py_to_string();
            eprintln!("         1 function calls in 0.000 seconds");
            eprintln!("");
            eprintln!("   Ordered by: standard name");
            eprintln!("");
            eprintln!("   ncalls  tottime  percall  cumtime  percall filename:lineno(function)");
            eprintln!("        1    0.000    0.000    0.000    0.000 <string>:1({})", stmt);
        }
        Ok(PyObject::none())
    });

    let profile_cls_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("Profile"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            let stats: Arc<RwLock<Vec<(String, i64, f64)>>> = Arc::new(RwLock::new(Vec::new()));
            let enabled: Arc<RwLock<bool>> = Arc::new(RwLock::new(false));
            let start_time: Arc<RwLock<Option<std::time::Instant>>> = Arc::new(RwLock::new(None));

            let e = enabled.clone();
            let st = start_time.clone();
            w.insert(CompactString::from("enable"), PyObject::native_closure(
                "enable", move |_: &[PyObjectRef]| {
                    *e.write() = true;
                    *st.write() = Some(std::time::Instant::now());
                    Ok(PyObject::none())
                }
            ));
            let e2 = enabled.clone();
            let st2 = start_time.clone();
            let stats2 = stats.clone();
            w.insert(CompactString::from("disable"), PyObject::native_closure(
                "disable", move |_: &[PyObjectRef]| {
                    *e2.write() = false;
                    if let Some(start) = st2.read().as_ref() {
                        stats2.write().push(("profiling".to_string(), 1, start.elapsed().as_secs_f64()));
                    }
                    Ok(PyObject::none())
                }
            ));
            let stats3 = stats.clone();
            w.insert(CompactString::from("runcall"), PyObject::native_closure(
                "runcall", move |args: &[PyObjectRef]| {
                    if args.is_empty() { return Ok(PyObject::none()); }
                    let func = &args[0];
                    let func_args = if args.len() > 1 { &args[1..] } else { &[] };
                    let start = std::time::Instant::now();
                    let result = match &func.payload {
                        PyObjectPayload::NativeFunction { func: f, .. } => f(func_args)?,
                        PyObjectPayload::NativeClosure { func: f, .. } => f(func_args)?,
                        _ => PyObject::none(),
                    };
                    stats3.write().push(("runcall".to_string(), 1, start.elapsed().as_secs_f64()));
                    Ok(result)
                }
            ));
            let stats4 = stats.clone();
            w.insert(CompactString::from("print_stats"), PyObject::native_closure(
                "print_stats", move |args: &[PyObjectRef]| {
                    let sort_key = if !args.is_empty() { args[0].py_to_string() } else { "cumulative".to_string() };
                    let st = stats4.read();
                    let total: f64 = st.iter().map(|(_, _, t)| t).sum();
                    let ncalls: i64 = st.iter().map(|(_, n, _)| n).sum();
                    eprintln!("         {} function calls in {:.3} seconds", ncalls.max(1), total);
                    eprintln!("   Ordered by: {}", sort_key);
                    eprintln!("   ncalls  tottime  percall  cumtime  percall filename:lineno(function)");
                    for (name, calls, time) in st.iter() {
                        let percall = if *calls > 0 { time / *calls as f64 } else { 0.0 };
                        eprintln!("   {:>5}    {:.3}    {:.3}    {:.3}    {:.3} <string>:1({})", calls, time, percall, time, percall, name);
                    }
                    Ok(PyObject::none())
                }
            ));
            // getstats() — return stats as list of tuples
            let stats5 = stats.clone();
            w.insert(CompactString::from("getstats"), PyObject::native_closure(
                "getstats", move |_: &[PyObjectRef]| {
                    let st = stats5.read();
                    let items: Vec<PyObjectRef> = st.iter().map(|(name, calls, time)| {
                        PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from(name.as_str())),
                            PyObject::int(*calls),
                            PyObject::float(*time),
                            PyObject::float(*time),
                            PyObject::list(vec![]),
                        ])
                    }).collect();
                    Ok(PyObject::list(items))
                }
            ));
            w.insert(CompactString::from("run"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
        }
        Ok(inst)
    });

    make_module("cProfile", vec![
        ("run", run_fn),
        ("Profile", profile_cls_fn),
    ])
}

// ── timeit module ──

/// Call a callable (NativeFunction or NativeClosure) with no args
fn call_callable(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::NativeFunction { func, .. } => func(&[]),
        PyObjectPayload::NativeClosure { func, .. } => func(&[]),
        _ => Ok(PyObject::none()),
    }
}

/// Check if object is callable
fn is_callable(obj: &PyObjectRef) -> bool {
    matches!(&obj.payload,
        PyObjectPayload::NativeFunction { .. } |
        PyObjectPayload::NativeClosure { .. } |
        PyObjectPayload::Function(_) |
        PyObjectPayload::BoundMethod { .. }
    )
}

pub fn create_timeit_module() -> PyObjectRef {
    // timeit.default_timer — alias for time.perf_counter
    let default_timer = make_builtin(|_args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        Ok(PyObject::float(t))
    });

    // timeit.timeit(stmt='pass', setup='pass', timer=<default>, number=1000000, globals=None)
    // If stmt is callable, calls it `number` times and returns total elapsed seconds
    // If stmt is a string, can't execute without VM — returns estimated time
    let timeit_fn = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        use std::time::Instant;
        // Extract kwargs dict if last arg is dict
        let (positional, kwargs) = if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw_map) = &last.payload {
                (&args[..args.len()-1], Some(kw_map.read().clone()))
            } else { (args, None) }
        } else { (args, None) };

        // stmt from positional[0] or kwargs['stmt']
        let stmt = positional.first().cloned()
            .or_else(|| kwargs.as_ref().and_then(|kw| kw.get(&HashableKey::Str(CompactString::from("stmt"))).cloned()));
        // setup from positional[1] or kwargs['setup']
        let setup = if positional.len() > 1 { Some(positional[1].clone()) }
            else { kwargs.as_ref().and_then(|kw| kw.get(&HashableKey::Str(CompactString::from("setup"))).cloned()) };
        // number from positional[2] or kwargs['number']
        let number: i64 = if positional.len() > 2 { positional[2].as_int().unwrap_or(1_000_000) }
            else { kwargs.as_ref().and_then(|kw| kw.get(&HashableKey::Str(CompactString::from("number"))).and_then(|v| v.as_int())).unwrap_or(1_000_000) };

        // Run setup if callable
        if let Some(ref s) = setup {
            if is_callable(s) { let _ = call_callable(s); }
        }

        if let Some(ref s) = stmt {
            if is_callable(s) {
                // Actually call the function `number` times
                let start = Instant::now();
                for _ in 0..number {
                    let _ = call_callable(s);
                }
                return Ok(PyObject::float(start.elapsed().as_secs_f64()));
            }
        }

        // String stmt or no stmt — measure overhead of loop
        let start = Instant::now();
        for _ in 0..number {
            std::hint::black_box(0);
        }
        Ok(PyObject::float(start.elapsed().as_secs_f64()))
    });

    // timeit.repeat(stmt, setup, repeat=5, number=1000000)
    let repeat_fn = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        use std::time::Instant;
        let (positional, kwargs) = if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw_map) = &last.payload {
                (&args[..args.len()-1], Some(kw_map.read().clone()))
            } else { (args, None) }
        } else { (args, None) };

        let stmt = positional.first().cloned()
            .or_else(|| kwargs.as_ref().and_then(|kw| kw.get(&HashableKey::Str(CompactString::from("stmt"))).cloned()));
        let setup = if positional.len() > 1 { Some(positional[1].clone()) }
            else { kwargs.as_ref().and_then(|kw| kw.get(&HashableKey::Str(CompactString::from("setup"))).cloned()) };
        let repeat_count: i64 = if positional.len() > 2 { positional[2].as_int().unwrap_or(5) }
            else { kwargs.as_ref().and_then(|kw| kw.get(&HashableKey::Str(CompactString::from("repeat"))).and_then(|v| v.as_int())).unwrap_or(5) };
        let number: i64 = if positional.len() > 3 { positional[3].as_int().unwrap_or(1_000_000) }
            else { kwargs.as_ref().and_then(|kw| kw.get(&HashableKey::Str(CompactString::from("number"))).and_then(|v| v.as_int())).unwrap_or(1_000_000) };

        if let Some(ref s) = setup {
            if is_callable(s) { let _ = call_callable(s); }
        }

        let is_stmt_callable = stmt.as_ref().map(|s| is_callable(s)).unwrap_or(false);
        let mut results = Vec::new();
        for _ in 0..repeat_count {
            let start = Instant::now();
            if is_stmt_callable {
                for _ in 0..number {
                    let _ = call_callable(stmt.as_ref().unwrap());
                }
            } else {
                for _ in 0..number {
                    std::hint::black_box(0);
                }
            }
            results.push(PyObject::float(start.elapsed().as_secs_f64()));
        }
        Ok(PyObject::list(results))
    });

    // Timer class
    let timer_cls = PyObject::class(CompactString::from("Timer"), vec![], IndexMap::new());
    let tc = timer_cls.clone();
    let timer_fn = PyObject::native_closure("Timer", move |args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        let inst = PyObject::instance(tc.clone());
        if let PyObjectPayload::Instance(ref data) = inst.payload {
            let mut attrs = data.attrs.write();
            let stmt = args.first().cloned().unwrap_or_else(PyObject::none);
            let setup = args.get(1).cloned().unwrap_or_else(PyObject::none);
            attrs.insert(CompactString::from("stmt"), stmt.clone());
            attrs.insert(CompactString::from("setup"), setup.clone());

            // timeit(number=1000000)
            let stmt2 = stmt.clone();
            let setup2 = setup.clone();
            attrs.insert(CompactString::from("timeit"), PyObject::native_closure(
                "timeit", move |inner_args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
                    use std::time::Instant;
                    let number: i64 = if inner_args.is_empty() { 1_000_000 }
                        else if inner_args.len() == 1 { inner_args[0].as_int().unwrap_or(1_000_000) }
                        else { inner_args[1].as_int().unwrap_or(1_000_000) };

                    if is_callable(&setup2) { let _ = call_callable(&setup2); }

                    let start = Instant::now();
                    if is_callable(&stmt2) {
                        for _ in 0..number { let _ = call_callable(&stmt2); }
                    } else {
                        for _ in 0..number { std::hint::black_box(0); }
                    }
                    Ok(PyObject::float(start.elapsed().as_secs_f64()))
                }
            ));
            // repeat(repeat=5, number=1000000)
            let stmt3 = stmt.clone();
            let setup3 = setup.clone();
            attrs.insert(CompactString::from("repeat"), PyObject::native_closure(
                "repeat", move |inner_args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
                    use std::time::Instant;
                    let repeat_count: i64 = if inner_args.is_empty() { 5 }
                        else if inner_args.len() == 1 { inner_args[0].as_int().unwrap_or(5) }
                        else { inner_args[1].as_int().unwrap_or(5) };
                    let number: i64 = if inner_args.len() > 2 { inner_args[2].as_int().unwrap_or(1_000_000) }
                        else { 1_000_000 };

                    if is_callable(&setup3) { let _ = call_callable(&setup3); }

                    let mut results = Vec::new();
                    for _ in 0..repeat_count {
                        let start = Instant::now();
                        if is_callable(&stmt3) {
                            for _ in 0..number { let _ = call_callable(&stmt3); }
                        } else {
                            for _ in 0..number { std::hint::black_box(0); }
                        }
                        results.push(PyObject::float(start.elapsed().as_secs_f64()));
                    }
                    Ok(PyObject::list(results))
                }
            ));
            // autorange() — find a good number to run
            let stmt4 = stmt.clone();
            attrs.insert(CompactString::from("autorange"), PyObject::native_closure(
                "autorange", move |_: &[PyObjectRef]| -> PyResult<PyObjectRef> {
                    use std::time::Instant;
                    let mut number: i64 = 1;
                    loop {
                        let start = Instant::now();
                        if is_callable(&stmt4) {
                            for _ in 0..number { let _ = call_callable(&stmt4); }
                        } else {
                            for _ in 0..number { std::hint::black_box(0); }
                        }
                        let elapsed = start.elapsed().as_secs_f64();
                        if elapsed >= 0.2 {
                            return Ok(PyObject::tuple(vec![PyObject::int(number), PyObject::float(elapsed)]));
                        }
                        number *= 10;
                        if number > 1_000_000_000 { break; }
                    }
                    Ok(PyObject::tuple(vec![PyObject::int(number), PyObject::float(0.0)]))
                }
            ));
            // print_exc() — stub
            attrs.insert(CompactString::from("print_exc"), make_builtin(|_| Ok(PyObject::none())));
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

// ── faulthandler module ──

pub fn create_faulthandler_module() -> PyObjectRef {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ENABLED: AtomicBool = AtomicBool::new(false);

    let enable = PyObject::native_closure("faulthandler.enable", move |_args: &[PyObjectRef]| {
        ENABLED.store(true, Ordering::Relaxed);
        Ok(PyObject::none())
    });
    let disable = PyObject::native_closure("faulthandler.disable", move |_: &[PyObjectRef]| {
        ENABLED.store(false, Ordering::Relaxed);
        Ok(PyObject::none())
    });
    let is_enabled = PyObject::native_closure("faulthandler.is_enabled", move |_: &[PyObjectRef]| {
        Ok(PyObject::bool_val(ENABLED.load(Ordering::Relaxed)))
    });
    let dump_traceback = make_builtin(|_args: &[PyObjectRef]| {
        eprintln!("Current thread (main thread):");
        eprintln!("  File \"<unknown>\", line 0 in <module>");
        Ok(PyObject::none())
    });
    let register_fn = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::none())
    });
    let unregister_fn = make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none()));
    let dump_traceback_later = make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none()));
    let cancel_dump = make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none()));

    make_module("faulthandler", vec![
        ("enable", enable),
        ("disable", disable),
        ("is_enabled", is_enabled),
        ("dump_traceback", dump_traceback),
        ("dump_traceback_later", dump_traceback_later),
        ("cancel_dump_traceback_later", cancel_dump),
        ("register", register_fn),
        ("unregister", unregister_fn),
    ])
}

// ── tracemalloc module ──

pub fn create_tracemalloc_module() -> PyObjectRef {
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
    use parking_lot::RwLock;

    static TRACING: AtomicBool = AtomicBool::new(false);
    static NFRAME: AtomicI64 = AtomicI64::new(1);

    // Snapshot data: list of (filename, lineno, size) triples
    static ALLOCS: std::sync::LazyLock<RwLock<Vec<(String, i64, i64)>>> =
        std::sync::LazyLock::new(|| RwLock::new(Vec::new()));

    let start = PyObject::native_closure("tracemalloc.start", move |args: &[PyObjectRef]| {
        let nframe = if !args.is_empty() {
            args[0].as_int().unwrap_or(1).max(1)
        } else { 1 };
        NFRAME.store(nframe, Ordering::Relaxed);
        TRACING.store(true, Ordering::Relaxed);
        ALLOCS.write().clear();
        Ok(PyObject::none())
    });
    let stop = PyObject::native_closure("tracemalloc.stop", move |_: &[PyObjectRef]| {
        TRACING.store(false, Ordering::Relaxed);
        Ok(PyObject::none())
    });
    let is_tracing = PyObject::native_closure("tracemalloc.is_tracing", move |_: &[PyObjectRef]| {
        Ok(PyObject::bool_val(TRACING.load(Ordering::Relaxed)))
    });
    let get_traced_memory = PyObject::native_closure("tracemalloc.get_traced_memory", move |_: &[PyObjectRef]| {
        // Return (current, peak) in bytes — use process RSS as estimate
        let current = {
            #[cfg(target_os = "linux")]
            {
                std::fs::read_to_string("/proc/self/statm")
                    .ok()
                    .and_then(|s| s.split_whitespace().nth(1).and_then(|v| v.parse::<i64>().ok()))
                    .map(|pages| pages * 4096)
                    .unwrap_or(0)
            }
            #[cfg(not(target_os = "linux"))]
            { 0i64 }
        };
        Ok(PyObject::tuple(vec![PyObject::int(current), PyObject::int(current)]))
    });
    let get_tracemalloc_memory = make_builtin(|_: &[PyObjectRef]| {
        Ok(PyObject::int(0))
    });
    let take_snapshot = PyObject::native_closure("tracemalloc.take_snapshot", move |_: &[PyObjectRef]| {
        let allocs = ALLOCS.read().clone();
        let traces = PyObject::list(
            allocs.iter().map(|(f, l, s)| {
                PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from(f.as_str())),
                    PyObject::int(*l),
                    PyObject::int(*s),
                ])
            }).collect()
        );
        Ok(make_module("Snapshot", vec![
            ("traces", traces),
            ("statistics", make_builtin(|_: &[PyObjectRef]| Ok(PyObject::list(vec![])))),
            ("compare_to", make_builtin(|_: &[PyObjectRef]| Ok(PyObject::list(vec![])))),
            ("filter_traces", make_builtin(|_: &[PyObjectRef]| {
                Ok(make_module("_filtered", vec![
                    ("traces", PyObject::list(vec![])),
                    ("statistics", make_builtin(|_: &[PyObjectRef]| Ok(PyObject::list(vec![])))),
                ]))
            })),
        ]))
    });
    let get_object_traceback = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::none())
    });
    let clear_traces = PyObject::native_closure("tracemalloc.clear_traces", move |_: &[PyObjectRef]| {
        ALLOCS.write().clear();
        Ok(PyObject::none())
    });

    make_module("tracemalloc", vec![
        ("start", start),
        ("stop", stop),
        ("is_tracing", is_tracing),
        ("get_traced_memory", get_traced_memory),
        ("get_tracemalloc_memory", get_tracemalloc_memory),
        ("take_snapshot", take_snapshot),
        ("get_object_traceback", get_object_traceback),
        ("clear_traces", clear_traces),
    ])
}

// ── pydoc module ──

pub fn create_pydoc_module() -> PyObjectRef {
    fn pydoc_help(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            println!("Welcome to Ferrython help utility!");
            println!("Type help(object) to get help on an object.");
            return Ok(PyObject::none());
        }
        let obj = &args[0];
        match &obj.payload {
            PyObjectPayload::Str(s) => {
                println!("Help on topic '{}':", s);
                println!("  (No detailed help available)");
            }
            PyObjectPayload::BuiltinType(name) => {
                println!("Help on class {}:", name);
                println!("  Built-in type '{}'", name);
            }
            PyObjectPayload::Function(f) => {
                println!("Help on function {}:", f.name);
                if let Some(doc) = obj.get_attr("__doc__") {
                    if let PyObjectPayload::Str(s) = &doc.payload {
                        println!("  {}", s);
                    }
                }
            }
            PyObjectPayload::Class(cd) => {
                println!("Help on class {}:", cd.name);
                let ns = cd.namespace.read();
                if let Some(doc) = ns.get("__doc__") {
                    if let PyObjectPayload::Str(s) = &doc.payload { println!("  {}", s); }
                }
                println!("\n  Methods:");
                for (name, _) in ns.iter() {
                    if !name.starts_with('_') {
                        println!("    {}", name);
                    }
                }
            }
            PyObjectPayload::Module(entries) => {
                println!("Help on module:");
                let rd = entries.attrs.read();
                if let Some(doc) = rd.get("__doc__") {
                    if let PyObjectPayload::Str(s) = &doc.payload { println!("  {}", s); }
                }
                println!("\n  Contents:");
                for (name, _) in rd.iter() {
                    if !name.starts_with('_') {
                        println!("    {}", name);
                    }
                }
            }
            _ => {
                println!("Help on {} object:", obj.type_name());
                println!("  Type: {}", obj.type_name());
            }
        }
        Ok(PyObject::none())
    }

    fn render_doc(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Ok(PyObject::str_val(CompactString::from("")));
        }
        let obj = &args[0];
        let type_name = obj.type_name();
        Ok(PyObject::str_val(CompactString::from(format!("Help on {} object", type_name))))
    }

    fn getdoc(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Ok(PyObject::none());
        }
        if let Some(doc) = args[0].get_attr("__doc__") {
            if !matches!(&doc.payload, PyObjectPayload::None) {
                return Ok(doc);
            }
        }
        Ok(PyObject::none())
    }

    make_module("pydoc", vec![
        ("help", make_builtin(pydoc_help)),
        ("render_doc", make_builtin(render_doc)),
        ("getdoc", make_builtin(getdoc)),
        ("Helper", make_builtin(pydoc_help)),
    ])
}
