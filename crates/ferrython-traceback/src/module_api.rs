//! Python `traceback` module API — Rust implementation.
//!
//! Provides `traceback.format_exception()`, `traceback.format_tb()`,
//! `traceback.extract_tb()`, `traceback.print_exception()`, etc.
//! These work with real traceback objects created by the VM.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult, TracebackEntry};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin, check_args_min,
};
use indexmap::IndexMap;

use crate::source_cache::SourceCache;

/// Create the `traceback` module with Rust-backed functions.
pub fn create_traceback_module() -> PyObjectRef {
    make_module("traceback", vec![
        ("format_exc", make_builtin(traceback_format_exc)),
        ("format_exception", make_builtin(traceback_format_exception)),
        ("format_tb", make_builtin(traceback_format_tb)),
        ("format_stack", make_builtin(traceback_format_stack)),
        ("extract_tb", make_builtin(traceback_extract_tb)),
        ("print_exc", make_builtin(traceback_print_exc)),
        ("print_exception", make_builtin(traceback_print_exception)),
        ("format_exception_only", make_builtin(traceback_format_exception_only)),
        ("print_tb", make_builtin(traceback_print_tb)),
        ("TracebackException", create_traceback_exception_class()),
        ("FrameSummary", make_builtin(frame_summary_cls)),
        ("StackSummary", make_builtin(stack_summary_cls)),
        ("linecache", make_linecache_module()),
    ])
}

// ── traceback.format_exc() ──────────────────────────────────────────────

/// `traceback.format_exc(limit=None, chain=True)` — format current exception.
/// Returns the formatted traceback string, or "NoneType: None" if no exception.
fn traceback_format_exc(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Get current exception info from thread-local
    let exc_info = ferrython_core::error::get_thread_exc_info();
    if let Some((kind, message, tb_entries)) = exc_info {
        let exc = PyException {
            kind, message, original: None,
            traceback: tb_entries, cause: None, context: None, value: None, os_error_info: None,
        };
        Ok(PyObject::str_val(CompactString::from(crate::format_traceback(&exc))))
    } else {
        Ok(PyObject::str_val(CompactString::from("NoneType: None")))
    }
}

// ── traceback.format_exception(etype, value, tb) ────────────────────────

/// `traceback.format_exception(etype, value, tb, limit=None, chain=True)`
/// Returns a list of strings, each ending with a newline.
fn traceback_format_exception(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("format_exception", args, 1)?;

    // If first arg is an exception instance, extract info from it
    let entries = extract_tb_from_arg(args.get(2).or(args.get(0)));
    let (kind_str, msg) = extract_exc_type_msg(args);

    let mut result = Vec::new();
    if !entries.is_empty() {
        result.push(PyObject::str_val(CompactString::from("Traceback (most recent call last):\n")));
        for entry in &entries {
            result.push(PyObject::str_val(CompactString::from(
                crate::formatting::format_entry(entry),
            )));
        }
    }
    let exc_line = if msg.is_empty() {
        format!("{}\n", kind_str)
    } else {
        format!("{}: {}\n", kind_str, msg)
    };
    result.push(PyObject::str_val(CompactString::from(exc_line)));
    Ok(PyObject::list(result))
}

// ── traceback.format_tb(tb, limit=None) ─────────────────────────────────

/// `traceback.format_tb(tb, limit=None)` — format traceback entries as list of strings.
fn traceback_format_tb(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("format_tb", args, 1)?;
    let entries = extract_tb_from_arg(Some(&args[0]));
    let result: Vec<PyObjectRef> = entries
        .iter()
        .map(|e| PyObject::str_val(CompactString::from(crate::formatting::format_entry(e))))
        .collect();
    Ok(PyObject::list(result))
}

// ── traceback.format_stack() ────────────────────────────────────────────

/// `traceback.format_stack(f=None, limit=None)` — format current stack.
/// Returns a list of strings. Since we don't have access to live frames here,
/// returns an empty list (the VM would need to populate frame info).
fn traceback_format_stack(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::list(vec![]))
}

// ── traceback.extract_tb(tb, limit=None) ────────────────────────────────

/// `traceback.extract_tb(tb, limit=None)` — extract traceback as list of FrameSummary.
fn traceback_extract_tb(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("extract_tb", args, 1)?;
    let entries = extract_tb_from_arg(Some(&args[0]));
    let result: Vec<PyObjectRef> = entries
        .iter()
        .map(|e| make_frame_summary(&e.filename, e.lineno, &e.function))
        .collect();
    Ok(PyObject::list(result))
}

// ── traceback.print_exc() ───────────────────────────────────────────────

/// `traceback.print_exc(limit=None, file=None, chain=True)` — print current exception.
fn traceback_print_exc(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let file_obj = extract_kwarg(args, "file");
    let exc_info = ferrython_core::error::get_thread_exc_info();
    if let Some((kind, message, tb_entries)) = exc_info {
        let exc = PyException {
            kind, message, original: None,
            traceback: tb_entries, cause: None, context: None, value: None, os_error_info: None,
        };
        let text = crate::format_traceback(&exc);
        write_to_file_or_stderr(&file_obj, &text);
    }
    Ok(PyObject::none())
}

// ── traceback.print_exception() ─────────────────────────────────────────

/// `traceback.print_exception(etype, value, tb, limit=None, file=None, chain=True)`
fn traceback_print_exception(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("print_exception", args, 1)?;
    let file_obj = extract_kwarg(args, "file");
    let entries = extract_tb_from_arg(args.get(2).or(args.get(0)));
    let (kind_str, msg) = extract_exc_type_msg(args);

    let mut text = String::new();
    if !entries.is_empty() {
        text.push_str("Traceback (most recent call last):\n");
        for entry in &entries {
            text.push_str(&crate::formatting::format_entry(entry));
        }
    }
    if msg.is_empty() {
        text.push_str(&kind_str);
        text.push('\n');
    } else {
        text.push_str(&format!("{}: {}\n", kind_str, msg));
    }
    write_to_file_or_stderr(&file_obj, &text);
    Ok(PyObject::none())
}

// ── traceback.format_exception_only(etype, value) ───────────────────────

/// `traceback.format_exception_only(etype, value)` — format just the exception line.
fn traceback_format_exception_only(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("format_exception_only", args, 1)?;
    let (kind_str, msg) = extract_exc_type_msg(args);
    let line = if msg.is_empty() {
        format!("{}\n", kind_str)
    } else {
        format!("{}: {}\n", kind_str, msg)
    };
    Ok(PyObject::list(vec![PyObject::str_val(CompactString::from(line))]))
}

// ── traceback.print_tb(tb, limit=None, file=None) ──────────────────────

/// `traceback.print_tb(tb, limit=None, file=None)` — print traceback entries.
fn traceback_print_tb(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("print_tb", args, 1)?;
    let file_obj = extract_kwarg(args, "file");
    let entries = extract_tb_from_arg(Some(&args[0]));
    let mut text = String::new();
    for entry in &entries {
        text.push_str(&crate::formatting::format_entry(entry));
    }
    write_to_file_or_stderr(&file_obj, &text);
    Ok(PyObject::none())
}

// ── TracebackException class ────────────────────────────────────────────

/// Create the TracebackException class with `format` and `format_exception_only`
/// as proper class methods so `TracebackException.format` resolves.
fn create_traceback_exception_class() -> PyObjectRef {
    let mut ns = IndexMap::new();

    // __init__(self, exc_type, exc_value, exc_tb, ...)
    ns.insert(CompactString::from("__init__"), make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("TracebackException.__init__ requires self"));
        }
        let self_obj = &args[0];
        let rest = &args[1..];
        let (kind_str, msg) = extract_exc_type_msg(rest);
        let entries = extract_tb_from_arg(rest.get(2));
        let formatted = if msg.is_empty() { kind_str.clone() } else { format!("{}: {}", kind_str, msg) };

        if let PyObjectPayload::Instance(inst) = &self_obj.payload {
            let mut attrs = inst.attrs.write();
            if !rest.is_empty() {
                attrs.insert(CompactString::from("exc_type"), rest[0].clone());
            }
            attrs.insert(CompactString::from("_str"), PyObject::str_val(CompactString::from(&formatted)));
            if rest.len() > 1 {
                attrs.insert(CompactString::from("exc_value"), rest[1].clone());
            }
            let fmt_lines: Vec<PyObjectRef> = entries.iter()
                .map(|e| PyObject::str_val(CompactString::from(crate::formatting::format_entry(e))))
                .collect();
            attrs.insert(CompactString::from("stack"), PyObject::list(fmt_lines));
        }
        Ok(PyObject::none())
    }));

    // format(self) → list of str
    ns.insert(CompactString::from("format"), make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("format requires self"));
        }
        let s = args[0].get_attr("_str").map(|v| v.py_to_string()).unwrap_or_default();
        Ok(PyObject::list(vec![PyObject::str_val(CompactString::from(&s))]))
    }));

    // format_exception_only(self) → list of str
    ns.insert(CompactString::from("format_exception_only"), make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("format_exception_only requires self"));
        }
        let s = args[0].get_attr("_str").map(|v| v.py_to_string()).unwrap_or_default();
        Ok(PyObject::list(vec![PyObject::str_val(CompactString::from(format!("{}\n", s)))]))
    }));

    // __str__(self)
    ns.insert(CompactString::from("__str__"), make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("__str__ requires self"));
        }
        let s = args[0].get_attr("_str").map(|v| v.py_to_string()).unwrap_or_default();
        Ok(PyObject::str_val(CompactString::from(&s)))
    }));

    PyObject::class(CompactString::from("TracebackException"), vec![], ns)
}

/// Legacy function form kept for direct calls (no longer in module table)
fn traceback_exception_cls(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("TracebackException", args, 1)?;
    let (kind_str, msg) = extract_exc_type_msg(args);
    let entries = extract_tb_from_arg(args.get(2));
    let formatted = if msg.is_empty() {
        kind_str.clone()
    } else {
        format!("{}: {}", kind_str, msg)
    };

    let cls = PyObject::builtin_type(CompactString::from("TracebackException"));
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("exc_type"), args[0].clone());
    attrs.insert(CompactString::from("_str"), PyObject::str_val(CompactString::from(&formatted)));
    if args.len() > 1 {
        attrs.insert(CompactString::from("exc_value"), args[1].clone());
    }

    // Store formatted lines
    let fmt_lines: Vec<PyObjectRef> = entries
        .iter()
        .map(|e| PyObject::str_val(CompactString::from(crate::formatting::format_entry(e))))
        .collect();
    attrs.insert(CompactString::from("stack"), PyObject::list(fmt_lines));

    // format() method
    let formatted_clone = formatted.clone();
    attrs.insert(CompactString::from("format"), PyObject::native_closure(
        "TracebackException.format",
        move |_| {
            Ok(PyObject::list(vec![
                PyObject::str_val(CompactString::from(&formatted_clone)),
            ]))
        },
    ));

    // __str__
    attrs.insert(CompactString::from("__str__"), PyObject::native_closure(
        "TracebackException.__str__",
        move |_| Ok(PyObject::str_val(CompactString::from(&formatted))),
    ));

    Ok(PyObject::instance_with_attrs(cls, attrs))
}

// ── FrameSummary class ──────────────────────────────────────────────────

/// `traceback.FrameSummary(filename, lineno, name, ...)`
fn frame_summary_cls(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("FrameSummary", args, 3)?;
    let filename = args[0].py_to_string();
    let lineno = match &args[1].payload {
        PyObjectPayload::Int(n) => n.to_i64().unwrap_or(0) as u32,
        _ => 0,
    };
    let name = args[2].py_to_string();
    Ok(make_frame_summary(&filename, lineno, &name))
}

/// `traceback.StackSummary.from_list(frame_list)` — create a StackSummary.
fn stack_summary_cls(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // StackSummary is essentially a list of FrameSummary objects
    if args.is_empty() {
        return Ok(PyObject::list(vec![]));
    }
    Ok(args[0].clone())
}

// ── linecache sub-module ────────────────────────────────────────────────

fn make_linecache_module() -> PyObjectRef {
    make_module("linecache", vec![
        ("getline", make_builtin(linecache_getline)),
        ("clearcache", make_builtin(linecache_clearcache)),
        ("checkcache", make_builtin(linecache_checkcache)),
    ])
}

fn linecache_getline(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("getline", args, 2)?;
    let filename = args[0].py_to_string();
    let lineno = match &args[1].payload {
        PyObjectPayload::Int(n) => n.to_i64().unwrap_or(0) as u32,
        _ => return Ok(PyObject::str_val(CompactString::from(""))),
    };
    match SourceCache::get_line(&filename, lineno) {
        Some(line) => Ok(PyObject::str_val(CompactString::from(line))),
        None => Ok(PyObject::str_val(CompactString::from(""))),
    }
}

fn linecache_clearcache(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    SourceCache::clear();
    Ok(PyObject::none())
}

fn linecache_checkcache(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if let Some(arg) = args.first() {
        SourceCache::invalidate(&arg.py_to_string());
    } else {
        SourceCache::clear();
    }
    Ok(PyObject::none())
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Create a FrameSummary instance with filename, lineno, name, line attributes.
fn make_frame_summary(filename: &str, lineno: u32, name: &str) -> PyObjectRef {
    let cls = PyObject::builtin_type(CompactString::from("FrameSummary"));
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("filename"), PyObject::str_val(CompactString::from(filename)));
    attrs.insert(CompactString::from("lineno"), PyObject::int(lineno as i64));
    attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(name)));

    // Source line (may be None)
    let line = SourceCache::get_line(filename, lineno);
    attrs.insert(CompactString::from("line"), match line {
        Some(l) => PyObject::str_val(CompactString::from(l.trim())),
        None => PyObject::none(),
    });
    attrs.insert(CompactString::from("locals"), PyObject::none());

    // __repr__
    let fname = filename.to_string();
    let flineno = lineno;
    let fname_disp = name.to_string();
    attrs.insert(CompactString::from("__repr__"), PyObject::native_closure(
        "FrameSummary.__repr__",
        move |_| {
            Ok(PyObject::str_val(CompactString::from(format!(
                "<FrameSummary file {}, line {} in {}>",
                fname, flineno, fname_disp,
            ))))
        },
    ));

    PyObject::instance_with_attrs(cls, attrs)
}

/// Walk a traceback chain object and extract TracebackEntry values.
fn extract_tb_from_arg(tb_arg: Option<&PyObjectRef>) -> Vec<TracebackEntry> {
    let Some(tb) = tb_arg else { return vec![]; };

    match &tb.payload {
        PyObjectPayload::None => vec![],
        PyObjectPayload::Instance(_inst) => {
            // Walk the tb_next chain
            let mut entries = Vec::new();
            let mut current = tb.clone();
            loop {
                let attrs = match &current.payload {
                    PyObjectPayload::Instance(inst) => inst.attrs.read().clone(),
                    _ => break,
                };
                let lineno = attrs.get("tb_lineno")
                    .and_then(|v| match &v.payload {
                        PyObjectPayload::Int(n) => Some(n.to_i64().unwrap_or(0) as u32),
                        _ => None,
                    })
                    .unwrap_or(0);
                let filename = attrs.get("tb_filename")
                    .map(|v| v.py_to_string())
                    .unwrap_or_else(|| "<unknown>".to_string());
                let function = attrs.get("tb_name")
                    .map(|v| v.py_to_string())
                    .unwrap_or_else(|| "<unknown>".to_string());

                entries.push(TracebackEntry { filename, function, lineno });

                match attrs.get("tb_next") {
                    Some(next) if !matches!(next.payload, PyObjectPayload::None) => {
                        current = next.clone();
                    }
                    _ => break,
                }
            }
            entries
        }
        // If passed a list of FrameSummary objects
        PyObjectPayload::List(items) => {
            let items = items.read();
            items.iter().filter_map(|item| {
                match &item.payload {
                    PyObjectPayload::Instance(inst) => {
                        let attrs = inst.attrs.read();
                        let filename = attrs.get("filename").map(|v| v.py_to_string())?;
                        let lineno = attrs.get("lineno").and_then(|v| match &v.payload {
                            PyObjectPayload::Int(n) => Some(n.to_i64().unwrap_or(0) as u32),
                            _ => None,
                        })?;
                        let function = attrs.get("name").map(|v| v.py_to_string())?;
                        Some(TracebackEntry { filename, function, lineno })
                    }
                    _ => None,
                }
            }).collect()
        }
        _ => vec![],
    }
}

/// Extract exception type name and message from args.
fn extract_exc_type_msg(args: &[PyObjectRef]) -> (String, String) {
    if args.is_empty() {
        return ("Exception".to_string(), String::new());
    }

    let first = &args[0];
    match &first.payload {
        // Exception type object
        PyObjectPayload::ExceptionType(kind) => {
            let msg = args.get(1)
                .map(|v| v.py_to_string())
                .unwrap_or_default();
            (format!("{}", kind), msg)
        }
        // Exception instance
        PyObjectPayload::ExceptionInstance { kind, message, .. } => {
            (format!("{}", kind), message.to_string())
        }
        // Instance (user-defined exception)
        PyObjectPayload::Instance(inst) => {
            let attrs = inst.attrs.read();
            let cls_name = match &inst.class.payload {
                PyObjectPayload::Class(cd) => cd.name.to_string(),
                PyObjectPayload::BuiltinType(name) => name.to_string(),
                PyObjectPayload::ExceptionType(kind) => format!("{}", kind),
                _ => "Exception".to_string(),
            };
            let msg = attrs.get("args")
                .map(|v| v.py_to_string())
                .or_else(|| args.get(1).map(|v| v.py_to_string()))
                .unwrap_or_default();
            (cls_name, msg)
        }
        // String (simple case)
        PyObjectPayload::Str(s) => {
            let msg = args.get(1)
                .map(|v| v.py_to_string())
                .unwrap_or_default();
            (s.to_string(), msg)
        }
        _ => {
            let type_name = first.py_to_string();
            let msg = args.get(1)
                .map(|v| v.py_to_string())
                .unwrap_or_default();
            (type_name, msg)
        }
    }
}

/// Extract a keyword argument from the trailing kwargs dict.
fn extract_kwarg(args: &[PyObjectRef], key: &str) -> Option<PyObjectRef> {
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(map) = &last.payload {
            let r = map.read();
            for (k, v) in r.iter() {
                if k.to_object().py_to_string() == key {
                    return Some(v.clone());
                }
            }
        }
    }
    // Also check positional args that might be "file" by convention
    None
}

/// Write text to a file object (calling its write() method) or to stderr.
fn write_to_file_or_stderr(file_obj: &Option<PyObjectRef>, text: &str) {
    if let Some(ref fobj) = file_obj {
        if matches!(fobj.payload, PyObjectPayload::None) {
            eprint!("{}", text);
            return;
        }
        // Try to call write() on the file object
        if let Some(write_fn) = fobj.get_attr("write") {
            let text_obj = PyObject::str_val(CompactString::from(text));
            match &write_fn.payload {
                PyObjectPayload::NativeClosure { func, .. } => {
                    let _ = func(&[text_obj]);
                    return;
                }
                PyObjectPayload::NativeFunction { func, .. } => {
                    let _ = func(&[text_obj]);
                    return;
                }
                PyObjectPayload::BoundMethod { receiver, method } => {
                    match &method.payload {
                        PyObjectPayload::NativeClosure { func, .. } => {
                            let _ = func(&[text_obj]);
                            return;
                        }
                        PyObjectPayload::NativeFunction { func, .. } => {
                            let _ = func(&[receiver.clone(), text_obj]);
                            return;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        eprint!("{}", text);
    } else {
        eprint!("{}", text);
    }
}