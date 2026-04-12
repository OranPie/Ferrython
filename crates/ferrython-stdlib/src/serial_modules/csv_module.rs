use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, IteratorData, NativeClosureData,
    make_module, make_builtin, check_args_min,
};
use ferrython_core::types::HashableKey;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

/// Global dialect registry.
static DIALECT_REGISTRY: std::sync::LazyLock<Mutex<HashMap<String, CsvDialectEntry>>> =
    std::sync::LazyLock::new(|| {
        let mut m = HashMap::new();
        m.insert("excel".into(), CsvDialectEntry { delimiter: ',', quotechar: '"', escapechar: None, doublequote: true, lineterminator: "\r\n".into() });
        m.insert("excel-tab".into(), CsvDialectEntry { delimiter: '\t', quotechar: '"', escapechar: None, doublequote: true, lineterminator: "\r\n".into() });
        m.insert("unix".into(), CsvDialectEntry { delimiter: ',', quotechar: '"', escapechar: None, doublequote: true, lineterminator: "\n".into() });
        Mutex::new(m)
    });

#[derive(Clone)]
struct CsvDialectEntry {
    delimiter: char,
    quotechar: char,
    escapechar: Option<char>,
    doublequote: bool,
    lineterminator: String,
}

pub fn create_csv_module() -> PyObjectRef {
    make_module("csv", vec![
        ("reader", make_builtin(csv_reader)),
        ("writer", make_builtin(csv_writer)),
        ("DictReader", make_builtin(csv_dict_reader)),
        ("DictWriter", make_builtin(csv_dict_writer)),
        ("register_dialect", make_builtin(csv_register_dialect)),
        ("unregister_dialect", make_builtin(csv_unregister_dialect)),
        ("get_dialect", make_builtin(csv_get_dialect)),
        ("list_dialects", make_builtin(csv_list_dialects)),
        ("Sniffer", make_builtin(csv_sniffer_ctor)),
        ("field_size_limit", make_builtin(|args: &[PyObjectRef]| {
            // field_size_limit([new_limit]) — get/set maximum field size
            static FIELD_SIZE_LIMIT: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(131072);
            let old = FIELD_SIZE_LIMIT.load(std::sync::atomic::Ordering::Relaxed);
            if let Some(n) = args.first().and_then(|a| a.as_int()) {
                FIELD_SIZE_LIMIT.store(n, std::sync::atomic::Ordering::Relaxed);
            }
            Ok(PyObject::int(old))
        })),
        ("Error", PyObject::builtin_type(CompactString::from("Error"))),
        ("QUOTE_ALL", PyObject::int(1)),
        ("QUOTE_MINIMAL", PyObject::int(0)),
        ("QUOTE_NONNUMERIC", PyObject::int(2)),
        ("QUOTE_NONE", PyObject::int(3)),
    ])
}

/// csv.register_dialect(name, [dialect], **kwargs)
fn csv_register_dialect(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("register_dialect requires a name")); }
    let name = args[0].py_to_string();
    let mut entry = CsvDialectEntry { delimiter: ',', quotechar: '"', escapechar: None, doublequote: true, lineterminator: "\r\n".into() };
    // Extract kwargs from trailing dict
    for arg in args.iter().skip(1) {
        if let PyObjectPayload::Dict(kw) = &arg.payload {
            let r = kw.read();
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("delimiter"))) {
                if let Some(c) = v.py_to_string().chars().next() { entry.delimiter = c; }
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("quotechar"))) {
                if let Some(c) = v.py_to_string().chars().next() { entry.quotechar = c; }
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("escapechar"))) {
                entry.escapechar = v.py_to_string().chars().next();
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("doublequote"))) {
                entry.doublequote = v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("lineterminator"))) {
                entry.lineterminator = v.py_to_string();
            }
        } else if let PyObjectPayload::Instance(inst) = &arg.payload {
            let r = inst.attrs.read();
            if let Some(v) = r.get("delimiter") { if let Some(c) = v.py_to_string().chars().next() { entry.delimiter = c; } }
            if let Some(v) = r.get("quotechar") { if let Some(c) = v.py_to_string().chars().next() { entry.quotechar = c; } }
        }
    }
    DIALECT_REGISTRY.lock().unwrap().insert(name, entry);
    Ok(PyObject::none())
}

fn csv_unregister_dialect(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("unregister_dialect requires a name")); }
    let name = args[0].py_to_string();
    let mut reg = DIALECT_REGISTRY.lock().unwrap();
    if reg.remove(&name).is_none() {
        return Err(PyException::runtime_error(format!("unknown dialect: '{}'", name)));
    }
    Ok(PyObject::none())
}

fn csv_get_dialect(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("get_dialect requires a name")); }
    let name = args[0].py_to_string();
    let reg = DIALECT_REGISTRY.lock().unwrap();
    if let Some(entry) = reg.get(&name) {
        Ok(make_dialect_obj(entry))
    } else {
        Err(PyException::runtime_error(format!("unknown dialect: '{}'", name)))
    }
}

fn csv_list_dialects(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let reg = DIALECT_REGISTRY.lock().unwrap();
    let names: Vec<_> = reg.keys().map(|k| PyObject::str_val(CompactString::from(k.as_str()))).collect();
    Ok(PyObject::list(names))
}

fn make_dialect_obj(entry: &CsvDialectEntry) -> PyObjectRef {
    let cls = PyObject::class(CompactString::from("Dialect"), vec![], indexmap::IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("delimiter"), PyObject::str_val(CompactString::from(entry.delimiter.to_string().as_str())));
        w.insert(CompactString::from("quotechar"), PyObject::str_val(CompactString::from(entry.quotechar.to_string().as_str())));
        w.insert(CompactString::from("doublequote"), PyObject::bool_val(entry.doublequote));
        w.insert(CompactString::from("lineterminator"), PyObject::str_val(CompactString::from(entry.lineterminator.as_str())));
        if let Some(esc) = entry.escapechar {
            w.insert(CompactString::from("escapechar"), PyObject::str_val(CompactString::from(esc.to_string().as_str())));
        } else {
            w.insert(CompactString::from("escapechar"), PyObject::none());
        }
    }
    inst
}

/// csv.Sniffer() — returns a Sniffer instance with sniff() and has_header() methods
fn csv_sniffer_ctor(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let cls = PyObject::class(CompactString::from("Sniffer"), vec![], indexmap::IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("sniff"), PyObject::native_closure("Sniffer.sniff", move |args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("sniff() requires sample text")); }
            let sample = args[0].py_to_string();
            let delimiters = if args.len() > 1 { Some(args[1].py_to_string()) } else { None };
            sniff_dialect(&sample, delimiters.as_deref())
        }));
        w.insert(CompactString::from("has_header"), PyObject::native_closure("Sniffer.has_header", move |args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("has_header() requires sample text")); }
            // Simple heuristic: check if first line looks like a header (non-numeric)
            let sample = args[0].py_to_string();
            if let Some(first_line) = sample.lines().next() {
                let has = first_line.split(',').all(|f| {
                    let trimmed = f.trim().trim_matches('"');
                    trimmed.parse::<f64>().is_err()
                });
                Ok(PyObject::bool_val(has))
            } else {
                Ok(PyObject::bool_val(false))
            }
        }));
    }
    Ok(inst)
}

/// Sniff a CSV sample to detect delimiter and quotechar.
fn sniff_dialect(sample: &str, delimiters: Option<&str>) -> PyResult<PyObjectRef> {
    let candidates = delimiters.unwrap_or(",\t;:|");
    // Count occurrences of each candidate delimiter
    let mut best_delim = ',';
    let mut best_count = 0usize;
    for delim in candidates.chars() {
        let count: usize = sample.lines().map(|line| line.matches(delim).count()).sum();
        if count > best_count {
            best_count = count;
            best_delim = delim;
        }
    }
    // Detect quotechar
    let quotechar = if sample.contains('"') { '"' } else if sample.contains('\'') { '\'' } else { '"' };
    let entry = CsvDialectEntry {
        delimiter: best_delim,
        quotechar,
        escapechar: None,
        doublequote: true,
        lineterminator: "\r\n".into(),
    };
    Ok(make_dialect_obj(&entry))
}

/// CSV dialect settings extracted from kwargs.
struct CsvDialect {
    delimiter: char,
    quotechar: char,
    escapechar: Option<char>,
    doublequote: bool,
}

impl Default for CsvDialect {
    fn default() -> Self {
        Self { delimiter: ',', quotechar: '"', escapechar: None, doublequote: true }
    }
}

/// Extract CSV dialect parameters from kwargs dict (trailing dict arg).
fn extract_csv_dialect(args: &[PyObjectRef], skip: usize) -> CsvDialect {
    let mut d = CsvDialect::default();
    for arg in args.iter().skip(skip) {
        // Check for dialect name string → look up registry
        if let PyObjectPayload::Str(name) = &arg.payload {
            if let Ok(reg) = DIALECT_REGISTRY.lock() {
                if let Some(entry) = reg.get(name.as_str()) {
                    d.delimiter = entry.delimiter;
                    d.quotechar = entry.quotechar;
                    d.escapechar = entry.escapechar;
                    d.doublequote = entry.doublequote;
                    continue;
                }
            }
        }
        // Check for kwargs dict
        if let PyObjectPayload::Dict(kw) = &arg.payload {
            let r = kw.read();
            if let Some(dialect_name) = r.get(&HashableKey::Str(CompactString::from("dialect"))) {
                let name = dialect_name.py_to_string();
                if let Ok(reg) = DIALECT_REGISTRY.lock() {
                    if let Some(entry) = reg.get(&name) {
                        d.delimiter = entry.delimiter;
                        d.quotechar = entry.quotechar;
                        d.escapechar = entry.escapechar;
                        d.doublequote = entry.doublequote;
                    }
                }
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("delimiter"))) {
                let s = v.py_to_string();
                if let Some(c) = s.chars().next() { d.delimiter = c; }
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("quotechar"))) {
                let s = v.py_to_string();
                if let Some(c) = s.chars().next() { d.quotechar = c; }
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("escapechar"))) {
                let s = v.py_to_string();
                d.escapechar = s.chars().next();
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("doublequote"))) {
                d.doublequote = v.is_truthy();
            }
            break;
        }
    }
    d
}

fn csv_parse_line(s: &str, dialect: &CsvDialect) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == dialect.quotechar {
                if dialect.doublequote && chars.peek() == Some(&dialect.quotechar) {
                    current.push(dialect.quotechar);
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else if dialect.escapechar == Some(ch) {
                // Escape char inside quotes: next char is literal
                if let Some(next) = chars.next() { current.push(next); }
            } else {
                current.push(ch);
            }
        } else if ch == dialect.quotechar {
            in_quotes = true;
        } else if dialect.escapechar == Some(ch) && !in_quotes {
            // Escape char outside quotes: next char is literal
            if let Some(next) = chars.next() { current.push(next); }
        } else if ch == dialect.delimiter {
            fields.push(current.clone());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    fields.push(current);
    fields
}

fn csv_reader(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("csv.reader requires an iterable"));
    }
    let dialect = extract_csv_dialect(args, 1);
    // Try to get lines from the iterable
    let lines = match args[0].to_list() {
        Ok(items) => items,
        Err(_) => {
            // Handle StringIO-like objects: call getvalue() or read() to get text
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                let attrs = inst.attrs.read();
                if let Some(getvalue) = attrs.get("getvalue") {
                    let text = match &getvalue.payload {
                        PyObjectPayload::NativeClosure(nc) => (nc.func)(&[])?,
                        PyObjectPayload::NativeFunction { func, .. } => func(&[])?,
                        _ => return Err(PyException::type_error("csv.reader requires an iterable")),
                    };
                    drop(attrs);
                    text.py_to_string().lines()
                        .filter(|l| !l.is_empty())
                        .map(|l| PyObject::str_val(CompactString::from(l)))
                        .collect()
                } else if let Some(read_fn) = attrs.get("read") {
                    let text = match &read_fn.payload {
                        PyObjectPayload::NativeClosure(nc) => (nc.func)(&[])?,
                        PyObjectPayload::NativeFunction { func, .. } => func(&[])?,
                        _ => return Err(PyException::type_error("csv.reader requires an iterable")),
                    };
                    drop(attrs);
                    text.py_to_string().lines()
                        .filter(|l| !l.is_empty())
                        .map(|l| PyObject::str_val(CompactString::from(l)))
                        .collect()
                } else {
                    drop(attrs);
                    return Err(PyException::type_error("csv.reader requires an iterable"));
                }
            } else {
                return Err(PyException::type_error("csv.reader requires an iterable"));
            }
        }
    };
    let mut rows = Vec::new();
    for line in &lines {
        let s = line.py_to_string();
        if s.trim().is_empty() { continue; }
        let fields: Vec<PyObjectRef> = csv_parse_line(&s, &dialect)
            .into_iter()
            .map(|f| PyObject::str_val(CompactString::from(f.trim())))
            .collect();
        rows.push(PyObject::list(fields));
    }
    // Build a csv_reader instance that supports both iteration (next()) and
    // list-like access (len(), []) for backward compatibility.
    let line_count = rows.len() as i64;
    let shared_rows = Arc::new(rows);
    let iter_index = Arc::new(Mutex::new(0usize));

    let mut attrs = indexmap::IndexMap::new();
    attrs.insert(CompactString::from("line_num"), PyObject::int(line_count));

    // __len__ for len(reader)
    let rows_ref = shared_rows.clone();
    attrs.insert(CompactString::from("__len__"), PyObject::wrap(PyObjectPayload::NativeClosure(Box::new(NativeClosureData {
        name: CompactString::from("__len__"),
        func: std::rc::Rc::new(move |_args| Ok(PyObject::int(rows_ref.len() as i64))),
    }))));

    // __getitem__ for reader[i]
    let rows_ref = shared_rows.clone();
    attrs.insert(CompactString::from("__getitem__"), PyObject::wrap(PyObjectPayload::NativeClosure(Box::new(NativeClosureData {
        name: CompactString::from("__getitem__"),
        func: std::rc::Rc::new(move |args| {
            let idx = if args.is_empty() {
                return Err(PyException::type_error("__getitem__ requires an index"));
            } else {
                args[0].to_int().unwrap_or(0)
            };
            let len = rows_ref.len() as i64;
            let real_idx = if idx < 0 { (len + idx) as usize } else { idx as usize };
            if real_idx < rows_ref.len() {
                Ok(rows_ref[real_idx].clone())
            } else {
                Err(PyException::index_error("list index out of range"))
            }
        }),
    }))));

    // __iter__ returns self (the iterator facade)
    let _idx_ref = iter_index.clone();
    let _rows_ref = shared_rows.clone();
    // We create a closure-based iterator that the VM can iterate
    attrs.insert(CompactString::from("__iter__"), {
        // Build a proper Iterator payload for the VM's for-loop
        let rows_vec = shared_rows.to_vec();
        let iter_obj = PyObject::wrap(PyObjectPayload::Iterator(Arc::new(parking_lot::Mutex::new(
            IteratorData::List { items: rows_vec, index: 0 },
        ))));
        let it = iter_obj.clone();
        PyObject::wrap(PyObjectPayload::NativeClosure(Box::new(NativeClosureData {
            name: CompactString::from("__iter__"),
            func: std::rc::Rc::new(move |_args| Ok(it.clone())),
        })))
    });

    // __next__ for next(reader)
    let rows_ref = shared_rows.clone();
    let idx_ref = iter_index.clone();
    attrs.insert(CompactString::from("__next__"), PyObject::wrap(PyObjectPayload::NativeClosure(Box::new(NativeClosureData {
        name: CompactString::from("__next__"),
        func: std::rc::Rc::new(move |_args| {
            let mut idx = idx_ref.lock().unwrap();
            if *idx < rows_ref.len() {
                let val = rows_ref[*idx].clone();
                *idx += 1;
                Ok(val)
            } else {
                Err(PyException::stop_iteration())
            }
        }),
    }))));

    Ok(PyObject::instance_with_attrs(
        PyObject::class(CompactString::from("csv_reader"), vec![], indexmap::IndexMap::new()),
        attrs,
    ))
}

/// Quote a CSV field according to quoting mode.
/// QUOTE_MINIMAL=0, QUOTE_ALL=1, QUOTE_NONNUMERIC=2, QUOTE_NONE=3
fn csv_quote_field(s: &str, quoting: i64, quotechar: char, delimiter: &str) -> String {
    let qc = quotechar.to_string();
    let escaped = s.replace(&qc, &format!("{qc}{qc}"));
    match quoting {
        1 => format!("{qc}{escaped}{qc}"), // QUOTE_ALL
        2 => {
            // QUOTE_NONNUMERIC: quote if not a number
            if s.parse::<f64>().is_ok() { s.to_string() }
            else { format!("{qc}{escaped}{qc}") }
        }
        3 => s.to_string(), // QUOTE_NONE
        _ => {
            // QUOTE_MINIMAL: only quote if contains delimiter, quotechar, or newline
            if s.contains(delimiter) || s.contains(quotechar) || s.contains('\n') || s.contains('\r') {
                format!("{qc}{escaped}{qc}")
            } else {
                s.to_string()
            }
        }
    }
}

fn csv_writer(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("csv.writer requires a file object"));
    }
    let fileobj = args[0].clone();
    let mut delimiter = ",".to_string();
    let mut quoting: i64 = 0; // QUOTE_MINIMAL
    let mut quotechar = '"';
    if args.len() > 1 {
        if let PyObjectPayload::Dict(kw) = &args[args.len()-1].payload {
            let r = kw.read();
            // Check for dialect name first
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("dialect"))) {
                let name = v.py_to_string();
                if let Ok(reg) = DIALECT_REGISTRY.lock() {
                    if let Some(entry) = reg.get(&name) {
                        delimiter = entry.delimiter.to_string();
                        quotechar = entry.quotechar;
                    }
                }
            }
            // Individual overrides take precedence
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("delimiter"))) {
                delimiter = v.py_to_string();
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("quoting"))) {
                if let PyObjectPayload::Int(n) = &v.payload {
                    quoting = n.to_i64().unwrap_or(0);
                }
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("quotechar"))) {
                let s = v.py_to_string();
                quotechar = s.chars().next().unwrap_or('"');
            }
        }
    }

    let cls = PyObject::class(CompactString::from("csv_writer"), vec![], indexmap::IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("__csv_writer__"), PyObject::bool_val(true));
        attrs.insert(CompactString::from("_fileobj"), fileobj.clone());

        // writerow(row) — format and write a single row
        let fo = fileobj.clone();
        let delim = delimiter.clone();
        let qt = quoting;
        let qc = quotechar;
        attrs.insert(CompactString::from("writerow"), PyObject::native_closure(
            "csv_writer.writerow", move |a: &[PyObjectRef]| {
                if a.is_empty() { return Err(PyException::type_error("writerow requires a sequence")); }
                let items = a[0].to_list()?;
                let fields: Vec<String> = items.iter().map(|item| {
                    let s = item.py_to_string();
                    csv_quote_field(&s, qt, qc, &delim)
                }).collect();
                let line = format!("{}\r\n", fields.join(&delim));
                // Write to fileobj via its write() method
                if let Some(write_fn) = fo.get_attr("write") {
                    match &write_fn.payload {
                        PyObjectPayload::NativeFunction { func, .. } => { func(&[PyObject::str_val(CompactString::from(&line))])?; }
                        PyObjectPayload::NativeClosure(nc) => { (nc.func)(&[PyObject::str_val(CompactString::from(&line))])?; }
                        _ => {}
                    }
                }
                Ok(PyObject::none())
            }
        ));

        // writerows(rows) — write multiple rows
        let fo2 = fileobj;
        let delim2 = delimiter;
        let qt2 = quoting;
        let qc2 = quotechar;
        attrs.insert(CompactString::from("writerows"), PyObject::native_closure(
            "csv_writer.writerows", move |a: &[PyObjectRef]| {
                if a.is_empty() { return Err(PyException::type_error("writerows requires an iterable")); }
                let rows = a[0].to_list()?;
                for row in &rows {
                    let items = row.to_list()?;
                    let fields: Vec<String> = items.iter().map(|item| {
                        let s = item.py_to_string();
                        csv_quote_field(&s, qt2, qc2, &delim2)
                    }).collect();
                    let line = format!("{}\r\n", fields.join(&delim2));
                    if let Some(write_fn) = fo2.get_attr("write") {
                        match &write_fn.payload {
                            PyObjectPayload::NativeFunction { func, .. } => { func(&[PyObject::str_val(CompactString::from(&line))])?; }
                            PyObjectPayload::NativeClosure(nc) => { (nc.func)(&[PyObject::str_val(CompactString::from(&line))])?; }
                            _ => {}
                        }
                    }
                }
                Ok(PyObject::none())
            }
        ));
    }
    Ok(inst)
}

fn csv_dict_reader(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("csv.DictReader requires an iterable"));
    }
    let lines = if let PyObjectPayload::Instance(inst) = &args[0].payload {
        let attrs = inst.attrs.read();
        // Try getvalue() for StringIO, then read(), then to_list
        if let Some(getvalue) = attrs.get("getvalue") {
            let text = match &getvalue.payload {
                PyObjectPayload::NativeClosure(nc) => (nc.func)(&[])?,
                PyObjectPayload::NativeFunction { func, .. } => func(&[])?,
                _ => { drop(attrs); return Ok(PyObject::list(args[0].to_list().unwrap_or_default())); }
            };
            drop(attrs);
            text.py_to_string().lines()
                .filter(|l| !l.is_empty())
                .map(|l| PyObject::str_val(CompactString::from(l)))
                .collect()
        } else {
            drop(attrs);
            args[0].to_list()?
        }
    } else {
        args[0].to_list()?
    };
    if lines.is_empty() {
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(parking_lot::Mutex::new(IteratorData::List { items: vec![], index: 0 })))));
    }
    // Optional fieldnames as second arg
    let fieldnames: Vec<String> = if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
        args[1].to_list()?.iter().map(|f| f.py_to_string()).collect()
    } else {
        // First row is header
        csv_parse_line(&lines[0].py_to_string(), &CsvDialect::default()).into_iter().map(|f| f.trim().to_string()).collect()
    };
    let data_start = if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) { 0 } else { 1 };
    let mut rows = Vec::new();
    for line in &lines[data_start..] {
        let s = line.py_to_string();
        if s.trim().is_empty() { continue; }
        let values = csv_parse_line(&s, &CsvDialect::default());
        let mut map = indexmap::IndexMap::new();
        for (i, name) in fieldnames.iter().enumerate() {
            let val = values.get(i).map(|v| v.trim().to_string()).unwrap_or_default();
            map.insert(
                HashableKey::Str(CompactString::from(name.as_str())),
                PyObject::str_val(CompactString::from(&val)),
            );
        }
        rows.push(PyObject::dict(map));
    }
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(parking_lot::Mutex::new(IteratorData::List { items: rows, index: 0 })))))
}

fn csv_dict_writer(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("csv.DictWriter requires fileobj and fieldnames"));
    }
    // Extract fieldnames: either positional arg[1] or kwarg "fieldnames"
    let fieldnames: Vec<String> = if args.len() >= 2 {
        // Check if args[1] is a kwargs dict containing "fieldnames"
        if let PyObjectPayload::Dict(map) = &args[1].payload {
            let r = map.read();
            if let Some(fnames) = r.get(&HashableKey::Str(CompactString::from("fieldnames"))) {
                fnames.to_list()?.iter().map(|f| f.py_to_string()).collect()
            } else {
                // It's a plain list
                args[1].to_list()?.iter().map(|f| f.py_to_string()).collect()
            }
        } else {
            args[1].to_list()?.iter().map(|f| f.py_to_string()).collect()
        }
    } else {
        return Err(PyException::type_error("csv.DictWriter requires fileobj and fieldnames"));
    };
    // Extract dialect params from trailing kwargs dict
    let mut delimiter = ",".to_string();
    let mut quoting: i64 = 0;
    let mut quotechar = '"';
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(kw) = &last.payload {
            let r = kw.read();
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("dialect"))) {
                let name = v.py_to_string();
                if let Ok(reg) = DIALECT_REGISTRY.lock() {
                    if let Some(entry) = reg.get(&name) {
                        delimiter = entry.delimiter.to_string();
                        quotechar = entry.quotechar;
                    }
                }
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("delimiter"))) {
                delimiter = v.py_to_string();
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("quoting"))) {
                if let Some(n) = v.as_int() { quoting = n; }
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("quotechar"))) {
                let s = v.py_to_string();
                quotechar = s.chars().next().unwrap_or('"');
            }
        }
    }
    let cls = PyObject::class(CompactString::from("csv_DictWriter"), vec![], indexmap::IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("__csv_dictwriter__"), PyObject::bool_val(true));
        attrs.insert(CompactString::from("_fileobj"), args[0].clone());
        let fieldnames_list: Vec<PyObjectRef> = fieldnames.iter()
            .map(|n| PyObject::str_val(CompactString::from(n.as_str()))).collect();
        attrs.insert(CompactString::from("_fieldnames"), PyObject::list(fieldnames_list.clone()));
        attrs.insert(CompactString::from("fieldnames"), PyObject::list(fieldnames_list));
        let fnames_owned: Vec<String> = fieldnames.clone();

        // writerow(rowdict) — formats row as CSV and writes to fileobj
        let self_ref = inst.clone();
        let fnames_for_row = fnames_owned.clone();
        let delim_row = delimiter.clone();
        let qt_row = quoting;
        let qc_row = quotechar;
        attrs.insert(CompactString::from("writerow"), PyObject::native_closure(
            "DictWriter.writerow", {
                let self_ref = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("DictWriter.writerow", args, 1)?;
                    let row = &args[0];
                    let mut fields = Vec::new();
                    for fname in &fnames_for_row {
                        let key = HashableKey::Str(CompactString::from(fname.as_str()));
                        let val = if let PyObjectPayload::Dict(map) = &row.payload {
                            map.read().get(&key).cloned()
                                .unwrap_or_else(PyObject::none)
                        } else if let Some(v) = row.get_attr(fname) {
                            v
                        } else {
                            PyObject::none()
                        };
                        fields.push(csv_quote_field(&val.py_to_string(), qt_row, qc_row, &delim_row));
                    }
                    let line = format!("{}\r\n", fields.join(&delim_row));
                    write_to_fileobj(&self_ref, &line)?;
                    Ok(PyObject::none())
                }
            }
        ));

        // writerows(rows)
        let fnames_for_rows = fnames_owned.clone();
        let delim_rows = delimiter.clone();
        let qt_rows = quoting;
        let qc_rows = quotechar;
        attrs.insert(CompactString::from("writerows"), PyObject::native_closure(
            "DictWriter.writerows", {
                let self_ref = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("DictWriter.writerows", args, 1)?;
                    let rows = args[0].to_list()?;
                    for row in &rows {
                        let mut fields = Vec::new();
                        for fname in &fnames_for_rows {
                            let key = HashableKey::Str(CompactString::from(fname.as_str()));
                            let val = if let PyObjectPayload::Dict(map) = &row.payload {
                                map.read().get(&key).cloned()
                                    .unwrap_or_else(PyObject::none)
                            } else {
                                PyObject::none()
                            };
                            fields.push(csv_quote_field(&val.py_to_string(), qt_rows, qc_rows, &delim_rows));
                        }
                        let line = format!("{}\r\n", fields.join(&delim_rows));
                        write_to_fileobj(&self_ref, &line)?;
                    }
                    Ok(PyObject::none())
                }
            }
        ));

        // writeheader() — writes fieldnames as CSV header line
        let delim_hdr = delimiter.clone();
        let qt_hdr = quoting;
        let qc_hdr = quotechar;
        attrs.insert(CompactString::from("writeheader"), PyObject::native_closure(
            "DictWriter.writeheader", {
                let self_ref = self_ref.clone();
                let fnames = fnames_owned.clone();
                move |_args: &[PyObjectRef]| {
                    let escaped: Vec<String> = fnames.iter()
                        .map(|f| csv_quote_field(f, qt_hdr, qc_hdr, &delim_hdr))
                        .collect();
                    let line = format!("{}\r\n", escaped.join(&delim_hdr));
                    write_to_fileobj(&self_ref, &line)?;
                    Ok(PyObject::none())
                }
            }
        ));
    }
    Ok(inst)
}

/// Escape a CSV field: quote if contains comma, quote, or newline.
fn csv_escape_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Write a string to a DictWriter's fileobj via its write() method.
fn write_to_fileobj(writer_inst: &PyObjectRef, text: &str) -> PyResult<()> {
    if let Some(fileobj) = writer_inst.get_attr("_fileobj") {
        if let Some(write_fn) = fileobj.get_attr("write") {
            if let PyObjectPayload::NativeClosure(nc) = &write_fn.payload {
                let arg = PyObject::str_val(CompactString::from(text));
                (nc.func)(&[arg])?;
                return Ok(());
            }
            if let PyObjectPayload::NativeFunction { func, .. } = &write_fn.payload {
                let arg = PyObject::str_val(CompactString::from(text));
                func(&[arg])?;
                return Ok(());
            }
        }
        // Fallback: if fileobj is StringIO, try direct buffer append
        if let PyObjectPayload::Instance(data) = &fileobj.payload {
            let mut attrs = data.attrs.write();
            if let Some(buf) = attrs.get("__buffer__") {
                if let PyObjectPayload::Str(s) = &buf.payload {
                    let mut new_s = s.to_string();
                    new_s.push_str(text);
                    attrs.insert(CompactString::from("__buffer__"),
                        PyObject::str_val(CompactString::from(new_s.as_str())));
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}
