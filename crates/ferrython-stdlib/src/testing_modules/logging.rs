use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::rc::Rc;

// ── logging module ──

// Global root logger config — basicConfig modifies this once
static ROOT_CONFIGURED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
static ROOT_LEVEL: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(30); // WARNING
static ROOT_FORMAT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
/// Global disable threshold: logging.disable(level) sets this; messages at or below are suppressed.
static DISABLE_LEVEL: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

// Global logger registry: maps logger names to their PyObjectRef.
// Thread-local so each test / interpreter session gets its own registry.
thread_local! {
    static LOGGER_REGISTRY: std::cell::RefCell<HashMap<String, PyObjectRef>> =
        std::cell::RefCell::new(HashMap::new());
}

fn root_format() -> &'static str {
    ROOT_FORMAT
        .get()
        .map(|s| s.as_str())
        .unwrap_or("%(levelname)s:%(name)s:%(message)s")
}

fn current_asctime(datefmt: Option<&str>) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    // Convert to broken-down time (UTC-based, simplified)
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    // Compute year/month/day from days since epoch (1970-01-01)
    let (year, month, day) = days_to_ymd(days as i64);
    if let Some(fmt) = datefmt {
        fmt.replace("%Y", &format!("{:04}", year))
            .replace("%m", &format!("{:02}", month))
            .replace("%d", &format!("{:02}", day))
            .replace("%H", &format!("{:02}", hours))
            .replace("%M", &format!("{:02}", minutes))
            .replace("%S", &format!("{:02}", seconds))
            .replace("%f", &format!("{:06}", millis * 1000))
    } else {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02},{:03}",
            year, month, day, hours, minutes, seconds, millis
        )
    }
}

fn days_to_ymd(mut days: i64) -> (i64, u32, u32) {
    let mut year = 1970i64;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u32;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, (days + 1) as u32)
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn format_log_message(fmt: &str, level_name: &str, name: &str, msg: &str) -> String {
    let asctime = current_asctime(None);
    fmt.replace("%(levelname)s", level_name)
        .replace("%(name)s", name)
        .replace("%(message)s", msg)
        .replace("%(asctime)s", &asctime)
        .replace("%(lineno)d", "0")
        .replace("%(filename)s", "")
        .replace("%(funcName)s", "")
        .replace("%(module)s", "")
        .replace("%(pathname)s", "")
}

/// Apply Python %-style formatting: "Hello %s" % ("world",) → "Hello world"
fn apply_percent_format(fmt: &str, args: &[PyObjectRef]) -> String {
    if args.is_empty() {
        return fmt.to_string();
    }
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
                            result.push('%');
                            result.push(next);
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

fn logging_log(level: i64, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn logging_get_logger(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
