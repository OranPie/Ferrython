//! Logging, testing, and debugging stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
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

use std::collections::HashMap;

/// Global logger registry: maps logger names to their PyObjectRef.
/// Thread-local so each test / interpreter session gets its own registry.
thread_local! {
    static LOGGER_REGISTRY: std::cell::RefCell<HashMap<String, PyObjectRef>> =
        std::cell::RefCell::new(HashMap::new());
}

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
            let inst_for_level = inst.clone();
            attrs.insert(CompactString::from("setLevel"), PyObject::native_closure(
                "setLevel", move |args: &[PyObjectRef]| {
                    if let Some(v) = args.first() {
                        if let Some(n) = v.as_int() {
                            *lr.write() = n;
                            // Also update the instance attribute so handler.level is visible
                            if let PyObjectPayload::Instance(ref d) = inst_for_level.payload {
                                d.attrs.write().insert(CompactString::from("level"), PyObject::int(n));
                            }
                        }
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

                    // Write to stream via its write() method
                    let line = format!("{}\n", formatted);
                    if let Some(write_fn) = stream2.get_attr("write") {
                        let line_obj = PyObject::str_val(CompactString::from(&line));
                        match &write_fn.payload {
                            PyObjectPayload::NativeClosure { func, .. } => {
                                let _ = func(&[line_obj]);
                            }
                            PyObjectPayload::NativeFunction { func, .. } => {
                                let _ = func(&[line_obj]);
                            }
                            _ => { eprintln!("{}", formatted); }
                        }
                    } else {
                        eprintln!("{}", formatted);
                    }
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
            let fs = fmt_str.clone();
            attrs.insert(CompactString::from("_fmt"), PyObject::str_val(fmt_str));
            // format(record) — apply %(key)s substitution from record attrs
            attrs.insert(CompactString::from("format"), PyObject::native_closure(
                "Formatter.format", move |args: &[PyObjectRef]| {
                    let record = if args.len() >= 1 { &args[0] } else {
                        return Ok(PyObject::str_val(CompactString::from("")));
                    };
                    let result = fs.to_string();
                    // Apply %(key)s, %(key)d style substitutions
                    let mut i = 0;
                    let bytes = result.as_bytes().to_vec();
                    let mut output = String::new();
                    while i < bytes.len() {
                        if i + 1 < bytes.len() && bytes[i] == b'%' && bytes[i+1] == b'(' {
                            // Find closing )s or )d
                            if let Some(close) = bytes[i+2..].iter().position(|&b| b == b')') {
                                let key = std::str::from_utf8(&bytes[i+2..i+2+close]).unwrap_or("");
                                let spec_idx = i + 2 + close + 1;
                                if spec_idx < bytes.len() {
                                    let val = if let Some(attr) = record.get_attr(key) {
                                        attr.py_to_string()
                                    } else {
                                        format!("%({})s", key)
                                    };
                                    output.push_str(&val);
                                    i = spec_idx + 1; // skip the format char (s, d, f, etc.)
                                    continue;
                                }
                            }
                        }
                        output.push(bytes[i] as char);
                        i += 1;
                    }
                    Ok(PyObject::str_val(CompactString::from(output)))
                }));
        }
        Ok(inst)
    });

    // Handler base class — proper class with __init__ for subclassing
    let handler_cls = {
        let mut ns = IndexMap::new();
        // __init__: set default level and formatter on self
        ns.insert(CompactString::from("__init__"), PyObject::native_closure(
            "Handler.__init__", move |args: &[PyObjectRef]| {
                if let Some(self_obj) = args.first() {
                    if let PyObjectPayload::Instance(ref inst_data) = self_obj.payload {
                        let mut attrs = inst_data.attrs.write();
                        attrs.insert(CompactString::from("level"), PyObject::int(0));
                        attrs.insert(CompactString::from("formatter"), PyObject::none());
                    }
                }
                Ok(PyObject::none())
            }));
        // setLevel(self, level) — class-level method
        ns.insert(CompactString::from("setLevel"), PyObject::native_closure(
            "Handler.setLevel", move |args: &[PyObjectRef]| {
                if args.len() >= 2 {
                    if let PyObjectPayload::Instance(ref inst_data) = args[0].payload {
                        let mut attrs = inst_data.attrs.write();
                        attrs.insert(CompactString::from("level"), args[1].clone());
                    }
                }
                Ok(PyObject::none())
            }));
        // setFormatter(self, fmt) — class-level method, stores on self.formatter
        ns.insert(CompactString::from("setFormatter"), PyObject::native_closure(
            "Handler.setFormatter", move |args: &[PyObjectRef]| {
                if args.len() >= 2 {
                    if let PyObjectPayload::Instance(ref inst_data) = args[0].payload {
                        let mut attrs = inst_data.attrs.write();
                        attrs.insert(CompactString::from("formatter"), args[1].clone());
                    }
                }
                Ok(PyObject::none())
            }));
        // format(self, record) — class-level method
        ns.insert(CompactString::from("format"), PyObject::native_closure(
            "Handler.format", move |args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Ok(PyObject::str_val(CompactString::from("")));
                }
                let self_obj = &args[0];
                let record = &args[1];
                if let Some(formatter) = self_obj.get_attr("formatter") {
                    if !matches!(formatter.payload, PyObjectPayload::None) {
                        if let Some(fmt_fn) = formatter.get_attr("format") {
                            if let PyObjectPayload::NativeClosure { func, .. } = &fmt_fn.payload {
                                return func(&[record.clone()]);
                            }
                        }
                    }
                }
                if let Some(msg) = record.get_attr("message") {
                    return Ok(msg);
                }
                Ok(PyObject::str_val(CompactString::from(record.py_to_string())))
            }));
        PyObject::class(CompactString::from("Handler"), vec![], ns)
    };
    let handler_fn = handler_cls.clone();

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

    // Return cached logger if it already exists
    {
        let found = LOGGER_REGISTRY.with(|reg| {
            reg.borrow().get(logger_name.as_str()).cloned()
        });
        if let Some(existing) = found {
            return Ok(existing);
        }
    }

    let mut ns = IndexMap::new();
    ns.insert(CompactString::from("name"), PyObject::str_val(logger_name.clone()));
    ns.insert(CompactString::from("propagate"), PyObject::bool_val(true));
    let root_level = ROOT_LEVEL.load(std::sync::atomic::Ordering::Relaxed);
    let is_root = logger_name.as_str() == "root";
    // CPython: named loggers start at level=0 (NOTSET); root logger defaults to WARNING(30)
    let initial_level: i64 = if is_root { if root_level > 0 { root_level } else { 30 } } else { 0 };
    // Effective level: non-root loggers use 0 (NOTSET) to trigger parent chain walk at log time
    let effective = initial_level;
    let effective_level: Arc<RwLock<i64>> = Arc::new(RwLock::new(effective));
    ns.insert(CompactString::from("level"), PyObject::int(initial_level));
    let handlers_list = PyObject::list(vec![]);
    ns.insert(CompactString::from("handlers"), handlers_list.clone());

    // Create log methods that capture the shared handlers list and effective level
    let make_log_method = |level: i64, level_name: &'static str, handlers: PyObjectRef, name: CompactString, eff_level: Arc<RwLock<i64>>| -> PyObjectRef {
        PyObject::native_closure(level_name, move |args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::none()); }
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
                                    if n > 0 { current_level = n; return; }
                                }
                            }
                        }
                    }
                });
                if current_level == 0 {
                    // Fall back to root level
                    current_level = ROOT_LEVEL.load(std::sync::atomic::Ordering::Relaxed);
                    if current_level == 0 { current_level = 30; }
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

            // Dispatch to handlers via shared list, then propagate to parents
            let mut any_handler_found = false;

            // Helper: emit record to a handler list
            fn emit_to_handlers(handlers_obj: &PyObjectRef, record: &PyObjectRef, level: i64) -> bool {
                if let PyObjectPayload::List(items) = &handlers_obj.payload {
                    let items_r = items.read();
                    if items_r.is_empty() { return false; }
                    for handler in items_r.iter() {
                        if let Some(handler_level) = handler.get_attr("level") {
                            if let Some(hl) = handler_level.as_int() {
                                if hl > 0 && level < hl { continue; }
                            }
                        }
                        if let Some(emit_fn) = handler.get_attr("emit") {
                            match &emit_fn.payload {
                                PyObjectPayload::NativeFunction { func, .. } => {
                                    let _ = func(&[handler.clone(), record.clone()]);
                                }
                                PyObjectPayload::NativeClosure { func, .. } => {
                                    let _ = func(&[handler.clone(), record.clone()]);
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
                    }
                }
                // Also propagate to root logger
                if let Some(root) = reg.get("root") {
                    if let Some(root_handlers) = root.get_attr("handlers") {
                        if emit_to_handlers(&root_handlers, &record, level) {
                            any_handler_found = true;
                        }
                    }
                }
            });
            // Last-resort: only print to stderr if no handlers registered at all
            if !any_handler_found {
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
    // exception() — logs at ERROR level (same as error(), exc_info implied)
    ns.insert(CompactString::from("exception"), make_log_method(40, "ERROR", handlers_list.clone(), logger_name.clone(), effective_level.clone()));
    // log(level, msg, *args) — generic log method
    {
        let hl_log = handlers_list.clone();
        let name_log = logger_name.clone();
        let el_log = effective_level.clone();
        ns.insert(CompactString::from("log"), PyObject::native_closure(
            "log", move |args: &[PyObjectRef]| {
                if args.len() < 2 { return Ok(PyObject::none()); }
                let level = args[0].as_int().unwrap_or(20);
                let eff = *el_log.read();
                if eff > 0 && level < eff { return Ok(PyObject::none()); }
                let msg = args[1].py_to_string();
                let level_name = match level {
                    10 => "DEBUG", 20 => "INFO", 30 => "WARNING",
                    40 => "ERROR", 50 => "CRITICAL", _ => "UNKNOWN",
                };
                let record_attrs = IndexMap::from([
                    (CompactString::from("message"), PyObject::str_val(CompactString::from(&msg))),
                    (CompactString::from("msg"), PyObject::str_val(CompactString::from(&msg))),
                    (CompactString::from("levelname"), PyObject::str_val(CompactString::from(level_name))),
                    (CompactString::from("levelno"), PyObject::int(level)),
                    (CompactString::from("name"), PyObject::str_val(name_log.clone())),
                ]);
                let record_cls = PyObject::class(CompactString::from("LogRecord"), vec![], IndexMap::new());
                let record = PyObject::instance_with_attrs(record_cls, record_attrs);
                if let PyObjectPayload::List(items) = &hl_log.payload {
                    let r = items.read();
                    if r.is_empty() {
                        eprintln!("{}: {}", level_name, msg);
                    } else {
                        for handler in r.iter() {
                            if let Some(emit) = handler.get_attr("emit") {
                                if let PyObjectPayload::NativeClosure { func, .. } = &emit.payload {
                                    let _ = func(&[record.clone()]);
                                }
                            }
                        }
                    }
                }
                Ok(PyObject::none())
            }
        ));
    }

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
    // Register in thread-local logger registry
    LOGGER_REGISTRY.with(|reg| {
        reg.borrow_mut().insert(logger_name.to_string(), inst.clone());
    });
    Ok(inst)
}

// ── unittest module ──

/// Helper: extract optional message from args at given index.
#[allow(dead_code)]
fn assert_msg(args: &[PyObjectRef], idx: usize) -> String {
    if args.len() > idx {
        args[idx].py_to_string()
    } else {
        String::new()
    }
}

#[allow(dead_code)]
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
        ("TextTestRunner", make_builtin(|_| {
            // TextTestRunner() returns an object with a run(suite) method.
            // run() returns a TestResult with wasSuccessful(), failures, errors.
            let mut runner_attrs = IndexMap::new();
            runner_attrs.insert(CompactString::from("run"), PyObject::native_closure(
                "run", |_args| {
                    // Build a TestResult object
                    let mut res_attrs = IndexMap::new();
                    let failures = Arc::new(RwLock::new(Vec::<PyObjectRef>::new()));
                    let errors = Arc::new(RwLock::new(Vec::<PyObjectRef>::new()));
                    let _tests_run = Arc::new(std::sync::atomic::AtomicI64::new(0));

                    let f = failures.clone();
                    res_attrs.insert(CompactString::from("failures"), PyObject::list(vec![]));
                    let e = errors.clone();
                    res_attrs.insert(CompactString::from("errors"), PyObject::list(vec![]));
                    res_attrs.insert(CompactString::from("skipped"), PyObject::list(vec![]));
                    res_attrs.insert(CompactString::from("expectedFailures"), PyObject::list(vec![]));
                    res_attrs.insert(CompactString::from("unexpectedSuccesses"), PyObject::list(vec![]));
                    res_attrs.insert(CompactString::from("testsRun"), PyObject::int(0));

                    let f2 = failures.clone();
                    let e2 = errors.clone();
                    res_attrs.insert(CompactString::from("wasSuccessful"), PyObject::native_closure(
                        "wasSuccessful", move |_| {
                            Ok(PyObject::bool_val(f2.read().is_empty() && e2.read().is_empty()))
                        }
                    ));
                    res_attrs.insert(CompactString::from("addFailure"), PyObject::native_closure(
                        "addFailure", move |args| {
                            if !args.is_empty() { f.write().push(args[0].clone()); }
                            Ok(PyObject::none())
                        }
                    ));
                    res_attrs.insert(CompactString::from("addError"), PyObject::native_closure(
                        "addError", move |args| {
                            if !args.is_empty() { e.write().push(args[0].clone()); }
                            Ok(PyObject::none())
                        }
                    ));
                    Ok(PyObject::module_with_attrs(CompactString::from("TestResult"), res_attrs))
                }
            ));
            Ok(PyObject::module_with_attrs(CompactString::from("TextTestRunner"), runner_attrs))
        })),
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
///
/// Design: return_value is stored directly in the instance dict so that
/// `mock.return_value = X` (a normal STORE_ATTR) updates it in-place.
/// The __call__ closure reads from the instance's attrs dict at call time
/// via a shared Arc<RwLock<IndexMap>> reference to the instance data.
fn build_mock_instance(name: &str, kwargs: &IndexMap<HashableKey, PyObjectRef>) -> PyObjectRef {
    let cls = PyObject::class(CompactString::from(name), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        let attrs_ref = d.attrs.clone(); // shared ref for closures to read live attrs

        // Shared mutable state via Arc
        let call_count: Arc<RwLock<i64>> = Arc::new(RwLock::new(0));
        let call_args_list: Arc<RwLock<Vec<PyObjectRef>>> = Arc::new(RwLock::new(vec![]));
        let children: Arc<RwLock<IndexMap<String, PyObjectRef>>> = Arc::new(RwLock::new(IndexMap::new()));
        let mock_name = CompactString::from(name);

        // Store return_value directly as a plain value (not a closure) so STORE_ATTR overwrites it
        let init_rv = kwargs.get(&HashableKey::Str(CompactString::from("return_value")))
            .cloned().unwrap_or_else(PyObject::none);
        w.insert(CompactString::from("return_value"), init_rv);

        // Store side_effect if provided
        let init_se = kwargs.get(&HashableKey::Str(CompactString::from("side_effect")))
            .cloned().unwrap_or_else(PyObject::none);
        w.insert(CompactString::from("side_effect"), init_se);

        // __call__ — tracks calls, checks side_effect, reads return_value from live instance attrs
        let cc3 = call_count.clone();
        let cal2 = call_args_list.clone();
        let attrs_call = attrs_ref.clone();
        w.insert(CompactString::from("__call__"), PyObject::native_closure(
            "Mock.__call__", move |args: &[PyObjectRef]| {
                *cc3.write() += 1;
                cal2.write().push(PyObject::tuple(args.to_vec()));

                // Check side_effect first
                let se = attrs_call.read().get("side_effect").cloned();
                if let Some(ref effect) = se {
                    if !matches!(effect.payload, PyObjectPayload::None) {
                        // If it's an exception instance, raise it
                        if let Some(exc_type) = effect.get_attr("__class__") {
                            let type_name = exc_type.get_attr("__name__")
                                .map(|n| n.py_to_string())
                                .unwrap_or_default();
                            // Check if it's an exception type/instance
                            if type_name.ends_with("Error") || type_name.ends_with("Exception")
                                || type_name == "KeyboardInterrupt" || type_name == "SystemExit"
                                || type_name == "StopIteration" || type_name == "GeneratorExit"
                            {
                                let msg = effect.get_attr("args")
                                    .and_then(|a| a.get_item(&PyObject::int(0)).ok())
                                    .map(|s| s.py_to_string())
                                    .unwrap_or_default();
                                let kind = match type_name.as_str() {
                                    "ValueError" => ExceptionKind::ValueError,
                                    "TypeError" => ExceptionKind::TypeError,
                                    "KeyError" => ExceptionKind::KeyError,
                                    "IndexError" => ExceptionKind::IndexError,
                                    "AttributeError" => ExceptionKind::AttributeError,
                                    "RuntimeError" => ExceptionKind::RuntimeError,
                                    "OSError" | "IOError" => ExceptionKind::OSError,
                                    "FileNotFoundError" => ExceptionKind::FileNotFoundError,
                                    "PermissionError" => ExceptionKind::PermissionError,
                                    "NotImplementedError" => ExceptionKind::NotImplementedError,
                                    "StopIteration" => ExceptionKind::StopIteration,
                                    "AssertionError" => ExceptionKind::AssertionError,
                                    "ImportError" => ExceptionKind::ImportError,
                                    "NameError" => ExceptionKind::NameError,
                                    _ => ExceptionKind::RuntimeError,
                                };
                                return Err(PyException::new(kind, msg));
                            }
                        }
                        // If it's a callable, call it
                        // (handled at VM level if it's a Function)
                    }
                }

                // Read return_value from live instance attrs (may have been updated via STORE_ATTR)
                let rv = attrs_call.read()
                    .get("return_value")
                    .cloned()
                    .unwrap_or_else(PyObject::none);
                Ok(rv)
            }
        ));

        // __getattr__ — create child mocks for unknown attributes, route properties
        let children2 = children.clone();
        let mn = mock_name.clone();
        let cc_ga = call_count.clone();
        let cal_ga = call_args_list.clone();
        w.insert(CompactString::from("__getattr__"), PyObject::native_closure(
            "Mock.__getattr__", move |args: &[PyObjectRef]| {
                let attr_name = if !args.is_empty() { args[0].py_to_string() } else { return Ok(PyObject::none()); };
                // Don't intercept dunder methods
                if attr_name.starts_with("__") && attr_name.ends_with("__") {
                    return Err(PyException::attribute_error(format!(
                        "'{}' object has no attribute '{}'", mn, attr_name)));
                }
                // Route mock-specific dynamic properties
                match attr_name.as_str() {
                    "call_count" => return Ok(PyObject::int(*cc_ga.read())),
                    "call_args_list" => return Ok(PyObject::list(cal_ga.read().clone())),
                    "called" => return Ok(PyObject::bool_val(*cc_ga.read() > 0)),
                    _ => {}
                }
                let mut cache = children2.write();
                if let Some(child) = cache.get(&attr_name) {
                    return Ok(child.clone());
                }
                // Create new child mock
                let child = build_mock_instance("MagicMock", &IndexMap::new());
                cache.insert(attr_name, child.clone());
                Ok(child)
            }
        ));

        // assert_called()
        let cc_ac = call_count.clone();
        w.insert(CompactString::from("assert_called"), PyObject::native_closure(
            "Mock.assert_called", move |_: &[PyObjectRef]| {
                if *cc_ac.read() == 0 {
                    return Err(PyException::assertion_error("Expected mock to have been called."));
                }
                Ok(PyObject::none())
            }
        ));

        // assert_called_once()
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

        // assert_called_with()
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

        // reset_mock()
        let cc_rm = call_count.clone();
        let cal_rm = call_args_list.clone();
        let ch_rm = children.clone();
        let attrs_rm = attrs_ref.clone();
        w.insert(CompactString::from("reset_mock"), PyObject::native_closure(
            "Mock.reset_mock", move |_: &[PyObjectRef]| {
                *cc_rm.write() = 0;
                cal_rm.write().clear();
                attrs_rm.write().insert(CompactString::from("return_value"), PyObject::none());
                ch_rm.write().clear();
                Ok(PyObject::none())
            }
        ));

        // MagicMock gets default magic methods
        if name == "MagicMock" {
            w.insert(CompactString::from("__len__"), PyObject::native_closure("__len__", |_| Ok(PyObject::int(0))));
            w.insert(CompactString::from("__bool__"), PyObject::native_closure("__bool__", |_| Ok(PyObject::bool_val(true))));
            w.insert(CompactString::from("__iter__"), PyObject::native_closure("__iter__", |_| Ok(PyObject::list(vec![]).get_iter().unwrap_or_else(|_| PyObject::none()))));
            w.insert(CompactString::from("__contains__"), PyObject::native_closure("__contains__", |_| Ok(PyObject::bool_val(false))));
            w.insert(CompactString::from("__int__"), PyObject::native_closure("__int__", |_| Ok(PyObject::int(1))));
            w.insert(CompactString::from("__float__"), PyObject::native_closure("__float__", |_| Ok(PyObject::float(1.0))));
            w.insert(CompactString::from("__str__"), PyObject::native_closure("__str__", |_| Ok(PyObject::str_val(CompactString::from("MagicMock")))));
            w.insert(CompactString::from("__repr__"), PyObject::native_closure("__repr__", |_| Ok(PyObject::str_val(CompactString::from("<MagicMock>")))));
            w.insert(CompactString::from("__enter__"), PyObject::native_closure("__enter__", |args: &[PyObjectRef]| {
                Ok(if !args.is_empty() { args[0].clone() } else { PyObject::none() })
            }));
            w.insert(CompactString::from("__exit__"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::bool_val(false))));
        }
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
    let sentinel_cls = PyObject::class(CompactString::from("_Sentinel"), vec![], IndexMap::new());
    let sentinel = PyObject::instance(sentinel_cls);
    if let PyObjectPayload::Instance(ref d) = sentinel.payload {
        let sentinel_cache: Arc<RwLock<IndexMap<String, PyObjectRef>>> = Arc::new(RwLock::new(IndexMap::new()));
        let sc = sentinel_cache;
        d.attrs.write().insert(CompactString::from("__getattr__"), PyObject::native_closure(
            "_Sentinel.__getattr__", move |args: &[PyObjectRef]| {
                let name = if !args.is_empty() { args[0].py_to_string() } else { return Ok(PyObject::none()); };
                if name.starts_with("__") && name.ends_with("__") {
                    return Err(PyException::attribute_error(format!("_Sentinel has no attribute '{}'", name)));
                }
                let mut cache = sc.write();
                if let Some(obj) = cache.get(&name) {
                    return Ok(obj.clone());
                }
                let cls = PyObject::class(CompactString::from("_SentinelObject"), vec![], IndexMap::new());
                let obj = PyObject::instance(cls);
                if let PyObjectPayload::Instance(ref d) = obj.payload {
                    let n = name.clone();
                    d.attrs.write().insert(CompactString::from("name"), PyObject::str_val(CompactString::from(n.as_str())));
                    let n2 = name.clone();
                    d.attrs.write().insert(CompactString::from("__repr__"), PyObject::native_closure(
                        "__repr__", move |_| Ok(PyObject::str_val(CompactString::from(format!("sentinel.{}", n2))))
                    ));
                }
                cache.insert(name, obj.clone());
                Ok(obj)
            }
        ));
    }

    // call — call record
    let call_fn = make_builtin(|args: &[PyObjectRef]| {
        Ok(PyObject::tuple(args.to_vec()))
    });

    // ANY — matches anything
    let any_cls = PyObject::class(CompactString::from("_ANY"), vec![], IndexMap::new());
    let any_obj = PyObject::instance(any_cls);
    if let PyObjectPayload::Instance(ref d) = any_obj.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("__eq__"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::bool_val(true))));
        w.insert(CompactString::from("__ne__"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::bool_val(false))));
        w.insert(CompactString::from("__repr__"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::str_val(CompactString::from("ANY")))));
    }

    // patch.object(target, attribute, new=DEFAULT, **kwargs) — context manager
    let patch_object_fn = make_builtin(|args: &[PyObjectRef]| {
        // args: target_obj, attribute_name, [new], **kwargs
        if args.len() < 2 {
            return Err(PyException::type_error("patch.object requires at least 2 arguments".to_string()));
        }
        let target = args[0].clone();
        let attr_name = args[1].py_to_string();
        let kwargs = extract_mock_kwargs(&args[2..]);
        let rv_key = HashableKey::Str(CompactString::from("return_value"));
        // Build replacement value
        let replacement = if let Some(rv) = kwargs.get(&rv_key) {
            build_mock_instance("MagicMock", &kwargs)
        } else if args.len() >= 3 {
            args[2].clone()
        } else {
            build_mock_instance("MagicMock", &kwargs)
        };

        let cls = PyObject::class(CompactString::from("_patch_object"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            let target_enter = target.clone();
            let attr_enter = attr_name.clone();
            let repl_enter = replacement.clone();
            let saved: Arc<RwLock<Option<PyObjectRef>>> = Arc::new(RwLock::new(None));
            let saved_for_exit = saved.clone();
            let target_exit = target.clone();
            let attr_exit = attr_name.clone();

            w.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "patch.object.__enter__", move |_: &[PyObjectRef]| {
                    // Save old value
                    let old = target_enter.get_attr(&attr_enter);
                    *saved.write() = old;
                    // Set new value
                    if let PyObjectPayload::Instance(ref d) = target_enter.payload {
                        d.attrs.write().insert(CompactString::from(attr_enter.as_str()), repl_enter.clone());
                    }
                    Ok(repl_enter.clone())
                }
            ));
            w.insert(CompactString::from("__exit__"), PyObject::native_closure(
                "patch.object.__exit__", move |_: &[PyObjectRef]| {
                    // Restore old value
                    if let PyObjectPayload::Instance(ref d) = target_exit.payload {
                        let old = saved_for_exit.read().clone();
                        if let Some(old_val) = old {
                            d.attrs.write().insert(CompactString::from(attr_exit.as_str()), old_val);
                        } else {
                            d.attrs.write().shift_remove(&CompactString::from(attr_exit.as_str()));
                        }
                    }
                    Ok(PyObject::bool_val(false))
                }
            ));
        }
        Ok(inst)
    });

    // patch.dict — context manager for dict patching
    let patch_dict_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("patch.dict requires at least 1 argument".to_string()));
        }
        let cls = PyObject::class(CompactString::from("_patch_dict"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("__enter__"), make_builtin(|args: &[PyObjectRef]| {
                if !args.is_empty() { Ok(args[0].clone()) } else { Ok(PyObject::none()) }
            }));
            w.insert(CompactString::from("__exit__"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::bool_val(false))));
        }
        Ok(inst)
    });

    // Make patch a callable object with .object and .dict attributes
    let patch_cls = PyObject::class(CompactString::from("_patcher"), vec![], IndexMap::new());
    let patch_obj = PyObject::instance(patch_cls);
    if let PyObjectPayload::Instance(ref d) = patch_obj.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("__call__"), patch_fn);
        w.insert(CompactString::from("object"), patch_object_fn);
        w.insert(CompactString::from("dict"), patch_dict_fn);
    }

    make_module("unittest.mock", vec![
        ("Mock", make_mock("Mock")),
        ("MagicMock", make_mock("MagicMock")),
        ("patch", patch_obj),
        ("sentinel", sentinel),
        ("call", call_fn),
        ("ANY", any_obj),
        ("DEFAULT", PyObject::str_val(CompactString::from("DEFAULT"))),
        ("PropertyMock", make_mock("PropertyMock")),
    ])
}

// ── doctest module (replaced by pure Python stdlib/Lib/doctest.py) ──

#[allow(dead_code)]
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
        eprintln!("(Pdb) > <stdin>: breakpoint");
        Ok(PyObject::none())
    });

    let pm_fn = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::none())
    });

    let run_fn = make_builtin(|args: &[PyObjectRef]| {
        let _ = args;
        Ok(PyObject::none())
    });

    let runeval_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("runeval requires an expression"));
        }
        Ok(PyObject::none())
    });

    let runcall_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("runcall requires a function"));
        }
        // Call the function with remaining args
        let func = &args[0];
        let call_args = if args.len() > 1 { &args[1..] } else { &[] };
        match &func.payload {
            PyObjectPayload::NativeFunction { func: f, .. } => f(call_args),
            PyObjectPayload::NativeClosure { func: f, .. } => f(call_args),
            _ => Ok(PyObject::none()),
        }
    });

    // Breakpoint class
    let bp_cls = PyObject::class(CompactString::from("Breakpoint"), vec![], IndexMap::new());
    if let PyObjectPayload::Class(ref cd) = bp_cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(CompactString::from("bpbynumber"), PyObject::list(vec![PyObject::none()]));
        ns.insert(CompactString::from("bplist"), PyObject::dict(IndexMap::new()));
        let bp_init = make_builtin(|args: &[PyObjectRef]| {
            if args.len() < 3 {
                return Err(PyException::type_error("Breakpoint() requires file and line"));
            }
            let inst = &args[0];
            let file = args[1].py_to_string();
            let line = args[2].to_int().unwrap_or(0);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("file"), PyObject::str_val(CompactString::from(&file)));
                w.insert(CompactString::from("line"), PyObject::int(line));
                w.insert(CompactString::from("enabled"), PyObject::bool_val(true));
                w.insert(CompactString::from("temporary"), PyObject::bool_val(false));
                w.insert(CompactString::from("cond"), PyObject::none());
                w.insert(CompactString::from("hits"), PyObject::int(0));
                static BP_NUM: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);
                let num = BP_NUM.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                w.insert(CompactString::from("number"), PyObject::int(num));
                w.insert(CompactString::from("enable"), make_builtin(|_| {
                    Ok(PyObject::none())
                }));
                w.insert(CompactString::from("disable"), make_builtin(|_| {
                    Ok(PyObject::none())
                }));
            }
            Ok(PyObject::none())
        });
        ns.insert(CompactString::from("__init__"), bp_init);
        ns.insert(CompactString::from("clearBreakpoints"), make_builtin(|_| Ok(PyObject::none())));
    }

    // Bdb class
    let bdb_cls = PyObject::class(CompactString::from("Bdb"), vec![], IndexMap::new());
    if let PyObjectPayload::Class(ref cd) = bdb_cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(CompactString::from("set_break"), make_builtin(|_| Ok(PyObject::none())));
        ns.insert(CompactString::from("clear_break"), make_builtin(|_| Ok(PyObject::none())));
        ns.insert(CompactString::from("clear_all_breaks"), make_builtin(|_| Ok(PyObject::none())));
        ns.insert(CompactString::from("set_step"), make_builtin(|_| Ok(PyObject::none())));
        ns.insert(CompactString::from("set_next"), make_builtin(|_| Ok(PyObject::none())));
        ns.insert(CompactString::from("set_return"), make_builtin(|_| Ok(PyObject::none())));
        ns.insert(CompactString::from("set_continue"), make_builtin(|_| Ok(PyObject::none())));
        ns.insert(CompactString::from("set_quit"), make_builtin(|_| Ok(PyObject::none())));
        ns.insert(CompactString::from("get_all_breaks"), make_builtin(|_| Ok(PyObject::dict(IndexMap::new()))));
    }

    // Pdb class
    let pdb_cls = PyObject::class(CompactString::from("Pdb"), vec![bdb_cls.clone()], IndexMap::new());
    if let PyObjectPayload::Class(ref cd) = pdb_cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(CompactString::from("prompt"), PyObject::str_val(CompactString::from("(Pdb) ")));
        ns.insert(CompactString::from("set_trace"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
        ns.insert(CompactString::from("run"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
        ns.insert(CompactString::from("set_break"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
        ns.insert(CompactString::from("clear_all_breaks"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
        ns.insert(CompactString::from("get_all_breaks"), make_builtin(|_| Ok(PyObject::dict(IndexMap::new()))));
    }

    make_module("pdb", vec![
        ("set_trace", set_trace_fn),
        ("pm", pm_fn),
        ("run", run_fn),
        ("runeval", runeval_fn),
        ("runcall", runcall_fn),
        ("post_mortem", make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none()))),
        ("Pdb", pdb_cls),
        ("Bdb", bdb_cls),
        ("Breakpoint", bp_cls),
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
                    let sort_key = if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None) {
                        args[0].py_to_string()
                    } else {
                        "cumulative".to_string()
                    };
                    let st = stats4.read();
                    let total: f64 = st.iter().map(|(_, _, t)| t).sum();
                    let ncalls: i64 = st.iter().map(|(_, n, _)| n).sum();
                    let mut lines = Vec::new();
                    lines.push(format!("         {} function calls in {:.3} seconds", ncalls.max(1), total));
                    lines.push(String::new());
                    lines.push(format!("   Ordered by: {}", sort_key));
                    lines.push(String::new());
                    lines.push("   ncalls  tottime  percall  cumtime  percall filename:lineno(function)".to_string());
                    for (name, calls, time) in st.iter() {
                        let percall = if *calls > 0 { time / *calls as f64 } else { 0.0 };
                        lines.push(format!("   {:>5}    {:.3}    {:.3}    {:.3}    {:.3} <string>:1({})", calls, time, percall, time, percall, name));
                    }
                    let output = lines.join("\n") + "\n";
                    // Check for stream= kwarg (may be passed as last positional arg)
                    let mut wrote_to_stream = false;
                    for arg in args.iter() {
                        if let Some(_write) = arg.get_attr("write") {
                            // Looks like a stream — write to it via deferred call
                            if let PyObjectPayload::Instance(ref d) = arg.payload {
                                // Try StringIO-like direct write
                                if let Some(buf_ref) = d.attrs.read().get(&CompactString::from("_buffer")) {
                                    if let PyObjectPayload::List(items) = &buf_ref.payload {
                                        items.write().push(PyObject::str_val(CompactString::from(output.as_str())));
                                        wrote_to_stream = true;
                                        break;
                                    }
                                }
                            }
                            // Fallback: call the write method
                            match &_write.payload {
                                PyObjectPayload::NativeFunction { func, .. } => {
                                    let _ = func(&[PyObject::str_val(CompactString::from(output.as_str()))]);
                                    wrote_to_stream = true;
                                    break;
                                }
                                PyObjectPayload::NativeClosure { func, .. } => {
                                    let _ = func(&[PyObject::str_val(CompactString::from(output.as_str()))]);
                                    wrote_to_stream = true;
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    if !wrote_to_stream {
                        eprint!("{}", output);
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
        ("describe", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
            let obj = &args[0];
            let name = obj.get_attr("__name__")
                .map(|n| n.py_to_string())
                .unwrap_or_else(|| obj.type_name().to_string());
            let desc = match &obj.payload {
                PyObjectPayload::Module(_) => format!("module {}", name),
                PyObjectPayload::Class(_) => format!("class {}", name),
                PyObjectPayload::Function(_) | PyObjectPayload::NativeFunction { .. }
                | PyObjectPayload::NativeClosure { .. } => format!("function {}", name),
                PyObjectPayload::BoundMethod { .. } => format!("method {}", name),
                _ => obj.type_name().to_string(),
            };
            Ok(PyObject::str_val(CompactString::from(desc)))
        })),
        ("Helper", make_builtin(pydoc_help)),
    ])
}

// ─── logging.handlers submodule ─────────────────────────────────────────────

pub fn create_logging_handlers_module() -> PyObjectRef {
    let make_handler_class = |name: &str| -> PyObjectRef {
        let class_name = CompactString::from(name);
        let cn = class_name.clone();
        let cls = PyObject::class(class_name, vec![], IndexMap::new());
        let _cls_ret = cls.clone();
        let factory = PyObject::native_closure(name, move |args: &[PyObjectRef]| {
            let inst = PyObject::instance(cls.clone());
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut attrs = d.attrs.write();
                attrs.insert(CompactString::from("level"), PyObject::int(0));
                let cn2 = cn.clone();
                attrs.insert(CompactString::from("setLevel"), PyObject::native_function("setLevel", |_| Ok(PyObject::none())));
                attrs.insert(CompactString::from("setFormatter"), PyObject::native_function("setFormatter", |_| Ok(PyObject::none())));
                attrs.insert(CompactString::from("emit"), PyObject::native_function("emit", |_| Ok(PyObject::none())));
                attrs.insert(CompactString::from("close"), PyObject::native_function("close", |_| Ok(PyObject::none())));
                attrs.insert(CompactString::from("flush"), PyObject::native_function("flush", |_| Ok(PyObject::none())));
                // Store constructor args
                for (i, arg) in args.iter().enumerate() {
                    attrs.insert(CompactString::from(format!("_arg{}", i)), arg.clone());
                }
                let _ = cn2;
            }
            Ok(inst)
        });
        factory
    };

    make_module("logging.handlers", vec![
        ("RotatingFileHandler", make_handler_class("RotatingFileHandler")),
        ("TimedRotatingFileHandler", make_handler_class("TimedRotatingFileHandler")),
        ("SocketHandler", make_handler_class("SocketHandler")),
        ("DatagramHandler", make_handler_class("DatagramHandler")),
        ("SysLogHandler", make_handler_class("SysLogHandler")),
        ("NTEventLogHandler", make_handler_class("NTEventLogHandler")),
        ("SMTPHandler", make_handler_class("SMTPHandler")),
        ("MemoryHandler", make_handler_class("MemoryHandler")),
        ("HTTPHandler", make_handler_class("HTTPHandler")),
        ("QueueHandler", make_handler_class("QueueHandler")),
        ("QueueListener", make_handler_class("QueueListener")),
        ("WatchedFileHandler", make_handler_class("WatchedFileHandler")),
        ("BufferingHandler", make_handler_class("BufferingHandler")),
        ("BaseRotatingHandler", make_handler_class("BaseRotatingHandler")),
    ])
}

// ─── logging.config submodule ───────────────────────────────────────────────

pub fn create_logging_config_module() -> PyObjectRef {
    make_module("logging.config", vec![
        ("dictConfig", PyObject::native_function("dictConfig", |_args| {
            Ok(PyObject::none())
        })),
        ("fileConfig", PyObject::native_function("fileConfig", |_args| {
            Ok(PyObject::none())
        })),
        ("listen", PyObject::native_function("listen", |_args| {
            Ok(PyObject::none())
        })),
        ("stopListening", PyObject::native_function("stopListening", |_args| {
            Ok(PyObject::none())
        })),
    ])
}
