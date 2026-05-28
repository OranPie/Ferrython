use super::*;

/// Global dialect registry.
pub(super) static DIALECT_REGISTRY: std::sync::LazyLock<Mutex<HashMap<String, CsvDialectEntry>>> =
    std::sync::LazyLock::new(|| {
        let mut m = HashMap::new();
        m.insert(
            "excel".into(),
            CsvDialectEntry {
                delimiter: ',',
                quotechar: '"',
                escapechar: None,
                doublequote: true,
                lineterminator: "\r\n".into(),
            },
        );
        m.insert(
            "excel-tab".into(),
            CsvDialectEntry {
                delimiter: '\t',
                quotechar: '"',
                escapechar: None,
                doublequote: true,
                lineterminator: "\r\n".into(),
            },
        );
        m.insert(
            "unix".into(),
            CsvDialectEntry {
                delimiter: ',',
                quotechar: '"',
                escapechar: None,
                doublequote: true,
                lineterminator: "\n".into(),
            },
        );
        Mutex::new(m)
    });

#[derive(Clone)]
pub(super) struct CsvDialectEntry {
    pub(super) delimiter: char,
    pub(super) quotechar: char,
    pub(super) escapechar: Option<char>,
    pub(super) doublequote: bool,
    pub(super) lineterminator: String,
}

/// csv.register_dialect(name, [dialect], **kwargs)
pub(super) fn csv_register_dialect(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("register_dialect requires a name"));
    }
    let name = args[0].py_to_string();
    let mut entry = CsvDialectEntry {
        delimiter: ',',
        quotechar: '"',
        escapechar: None,
        doublequote: true,
        lineterminator: "\r\n".into(),
    };
    // Extract kwargs from trailing dict
    for arg in args.iter().skip(1) {
        if let PyObjectPayload::Dict(kw) = &arg.payload {
            let r = kw.read();
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("delimiter"))) {
                if let Some(c) = v.py_to_string().chars().next() {
                    entry.delimiter = c;
                }
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("quotechar"))) {
                if let Some(c) = v.py_to_string().chars().next() {
                    entry.quotechar = c;
                }
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("escapechar"))) {
                entry.escapechar = v.py_to_string().chars().next();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("doublequote"))) {
                entry.doublequote = v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("lineterminator"))) {
                entry.lineterminator = v.py_to_string();
            }
        } else if let PyObjectPayload::Instance(inst) = &arg.payload {
            let r = inst.attrs.read();
            if let Some(v) = r.get("delimiter") {
                if let Some(c) = v.py_to_string().chars().next() {
                    entry.delimiter = c;
                }
            }
            if let Some(v) = r.get("quotechar") {
                if let Some(c) = v.py_to_string().chars().next() {
                    entry.quotechar = c;
                }
            }
        }
    }
    DIALECT_REGISTRY.lock().unwrap().insert(name, entry);
    Ok(PyObject::none())
}

pub(super) fn csv_unregister_dialect(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "unregister_dialect requires a name",
        ));
    }
    let name = args[0].py_to_string();
    let mut reg = DIALECT_REGISTRY.lock().unwrap();
    if reg.remove(&name).is_none() {
        return Err(PyException::runtime_error(format!(
            "unknown dialect: '{}'",
            name
        )));
    }
    Ok(PyObject::none())
}

pub(super) fn csv_get_dialect(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("get_dialect requires a name"));
    }
    let name = args[0].py_to_string();
    let reg = DIALECT_REGISTRY.lock().unwrap();
    if let Some(entry) = reg.get(&name) {
        Ok(make_dialect_obj(entry))
    } else {
        Err(PyException::runtime_error(format!(
            "unknown dialect: '{}'",
            name
        )))
    }
}

pub(super) fn csv_list_dialects(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let reg = DIALECT_REGISTRY.lock().unwrap();
    let names: Vec<_> = reg
        .keys()
        .map(|k| PyObject::str_val(CompactString::from(k.as_str())))
        .collect();
    Ok(PyObject::list(names))
}

pub(super) fn make_dialect_obj(entry: &CsvDialectEntry) -> PyObjectRef {
    let cls = PyObject::class(CompactString::from("Dialect"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(
            CompactString::from("delimiter"),
            PyObject::str_val(CompactString::from(entry.delimiter.to_string().as_str())),
        );
        w.insert(
            CompactString::from("quotechar"),
            PyObject::str_val(CompactString::from(entry.quotechar.to_string().as_str())),
        );
        w.insert(
            CompactString::from("doublequote"),
            PyObject::bool_val(entry.doublequote),
        );
        w.insert(
            CompactString::from("lineterminator"),
            PyObject::str_val(CompactString::from(entry.lineterminator.as_str())),
        );
        if let Some(esc) = entry.escapechar {
            w.insert(
                CompactString::from("escapechar"),
                PyObject::str_val(CompactString::from(esc.to_string().as_str())),
            );
        } else {
            w.insert(CompactString::from("escapechar"), PyObject::none());
        }
    }
    inst
}

/// csv.Sniffer() — returns a Sniffer instance with sniff() and has_header() methods
pub(super) fn csv_sniffer_ctor(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let cls = PyObject::class(CompactString::from("Sniffer"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(
            CompactString::from("sniff"),
            PyObject::native_closure("Sniffer.sniff", move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Err(PyException::type_error("sniff() requires sample text"));
                }
                let sample = args[0].py_to_string();
                let delimiters = if args.len() > 1 {
                    Some(args[1].py_to_string())
                } else {
                    None
                };
                sniff_dialect(&sample, delimiters.as_deref())
            }),
        );
        w.insert(
            CompactString::from("has_header"),
            PyObject::native_closure("Sniffer.has_header", move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Err(PyException::type_error("has_header() requires sample text"));
                }
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
            }),
        );
    }
    Ok(inst)
}

/// Sniff a CSV sample to detect delimiter and quotechar.
pub(super) fn sniff_dialect(sample: &str, delimiters: Option<&str>) -> PyResult<PyObjectRef> {
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
    let quotechar = if sample.contains('"') {
        '"'
    } else if sample.contains('\'') {
        '\''
    } else {
        '"'
    };
    let entry = CsvDialectEntry {
        delimiter: best_delim,
        quotechar,
        escapechar: None,
        doublequote: true,
        lineterminator: "\r\n".into(),
    };
    Ok(make_dialect_obj(&entry))
}
