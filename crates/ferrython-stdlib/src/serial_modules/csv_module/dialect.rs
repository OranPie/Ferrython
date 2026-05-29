use super::*;

/// Global dialect registry.
pub(super) static DIALECT_REGISTRY: std::sync::LazyLock<Mutex<HashMap<String, CsvDialectEntry>>> =
    std::sync::LazyLock::new(|| {
        let mut m = HashMap::new();
        m.insert(
            "excel".into(),
            CsvDialectEntry {
                delimiter: ',',
                quotechar: Some('"'),
                escapechar: None,
                doublequote: true,
                lineterminator: "\r\n".into(),
                quoting: 0,
                skipinitialspace: false,
                strict: false,
            },
        );
        m.insert(
            "excel-tab".into(),
            CsvDialectEntry {
                delimiter: '\t',
                quotechar: Some('"'),
                escapechar: None,
                doublequote: true,
                lineterminator: "\r\n".into(),
                quoting: 0,
                skipinitialspace: false,
                strict: false,
            },
        );
        m.insert(
            "unix".into(),
            CsvDialectEntry {
                delimiter: ',',
                quotechar: Some('"'),
                escapechar: None,
                doublequote: true,
                lineterminator: "\n".into(),
                quoting: 1,
                skipinitialspace: false,
                strict: false,
            },
        );
        Mutex::new(m)
    });

#[derive(Clone)]
pub(super) struct CsvDialectEntry {
    pub(super) delimiter: char,
    pub(super) quotechar: Option<char>,
    pub(super) escapechar: Option<char>,
    pub(super) doublequote: bool,
    pub(super) lineterminator: String,
    pub(super) quoting: i64,
    pub(super) skipinitialspace: bool,
    pub(super) strict: bool,
}

pub(super) fn csv_error(message: impl Into<String>) -> PyException {
    PyException::new(ExceptionKind::CsvError, message.into())
}

pub(super) fn validate_dialect_entry(entry: &CsvDialectEntry) -> Result<(), String> {
    if !matches!(entry.quoting, 0..=3) {
        return Err("bad \"quoting\" value".to_string());
    }
    if entry.quotechar.is_none() && entry.quoting != 3 {
        return Err("quotechar must be set if quoting enabled".to_string());
    }
    if entry.delimiter == '\0' {
        return Err("line contains NUL".to_string());
    }
    if entry.quotechar == Some('\0') {
        return Err("line contains NUL".to_string());
    }
    if entry.escapechar == Some('\0') {
        return Err("line contains NUL".to_string());
    }
    Ok(())
}

fn dialect_attr(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    obj.get_attr(name).or_else(|| match &obj.payload {
        PyObjectPayload::Instance(inst) => inst.attrs.read().get(name).cloned(),
        PyObjectPayload::Class(cls) => cls.namespace.read().get(name).cloned(),
        _ => None,
    })
}

fn one_char_value(value: &PyObjectRef, name: &str, allow_none: bool) -> PyResult<Option<char>> {
    match &value.payload {
        PyObjectPayload::None if allow_none => Ok(None),
        PyObjectPayload::None => Err(PyException::type_error(format!(
            "\"{}\" must be a 1-character string",
            name
        ))),
        PyObjectPayload::Str(s) => {
            let mut chars = s.chars();
            match (chars.next(), chars.next()) {
                (Some(ch), None) => Ok(Some(ch)),
                _ => Err(csv_error(format!(
                    "\"{}\" must be a 1-character string",
                    name
                ))),
            }
        }
        _ => Err(csv_error(format!(
            "\"{}\" must be string, not {}",
            name,
            value.type_name()
        ))),
    }
}

fn apply_dialect_obj(entry: &mut CsvDialectEntry, obj: &PyObjectRef) -> PyResult<()> {
    if let Some(v) = dialect_attr(obj, "delimiter") {
        entry.delimiter = one_char_value(&v, "delimiter", false)?.unwrap();
    }
    if let Some(v) = dialect_attr(obj, "quoting") {
        entry.quoting = v
            .as_int()
            .ok_or_else(|| csv_error("\"quoting\" must be an integer"))?;
    }
    if let Some(v) = dialect_attr(obj, "quotechar") {
        entry.quotechar = one_char_value(&v, "quotechar", true)?;
        if entry.quotechar.is_none() && dialect_attr(obj, "quoting").is_none() {
            entry.quoting = 3;
        }
    }
    if let Some(v) = dialect_attr(obj, "escapechar") {
        entry.escapechar = one_char_value(&v, "escapechar", true)?;
    }
    if let Some(v) = dialect_attr(obj, "doublequote") {
        entry.doublequote = v.is_truthy();
    }
    if let Some(v) = dialect_attr(obj, "lineterminator") {
        if !matches!(v.payload, PyObjectPayload::Str(_)) {
            return Err(csv_error("\"lineterminator\" must be a string"));
        }
        entry.lineterminator = v.py_to_string();
    }
    if let Some(v) = dialect_attr(obj, "skipinitialspace") {
        entry.skipinitialspace = v.is_truthy();
    }
    if let Some(v) = dialect_attr(obj, "strict") {
        entry.strict = v.is_truthy();
    }
    validate_dialect_entry(entry).map_err(csv_error)
}

pub(super) fn csv_dialect_init(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let Some(obj) = args.first() else {
        return Err(PyException::type_error("Dialect() missing self"));
    };
    let mut entry = CsvDialectEntry {
        delimiter: ',',
        quotechar: Some('"'),
        escapechar: None,
        doublequote: true,
        lineterminator: "\r\n".into(),
        quoting: 0,
        skipinitialspace: false,
        strict: false,
    };
    apply_dialect_obj(&mut entry, obj)?;
    Ok(PyObject::none())
}

/// csv.register_dialect(name, [dialect], **kwargs)
pub(super) fn csv_register_dialect(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("register_dialect requires a name"));
    }
    if !matches!(args[0].payload, PyObjectPayload::Str(_)) {
        return Err(PyException::type_error("dialect name must be a string"));
    }
    if args.len() > 3 {
        return Err(PyException::type_error(
            "register_dialect expected at most 2 arguments",
        ));
    }
    let name = args[0].py_to_string();
    let mut entry = CsvDialectEntry {
        delimiter: ',',
        quotechar: Some('"'),
        escapechar: None,
        doublequote: true,
        lineterminator: "\r\n".into(),
        quoting: 0,
        skipinitialspace: false,
        strict: false,
    };
    // Extract kwargs from trailing dict
    for arg in args.iter().skip(1) {
        if let PyObjectPayload::Dict(kw) = &arg.payload {
            let r = kw.read();
            for key in r.keys() {
                if let HashableKey::Str(s) = key {
                    if !matches!(
                        s.as_str(),
                        "delimiter"
                            | "quotechar"
                            | "escapechar"
                            | "doublequote"
                            | "lineterminator"
                            | "quoting"
                            | "skipinitialspace"
                            | "strict"
                    ) {
                        return Err(PyException::type_error(format!(
                            "'{}' is an invalid keyword argument for this function",
                            s
                        )));
                    }
                }
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("delimiter"))) {
                entry.delimiter = one_char_value(v, "delimiter", false)?.unwrap();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("quoting"))) {
                entry.quoting = v
                    .as_int()
                    .ok_or_else(|| PyException::type_error("\"quoting\" must be an integer"))?;
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("quotechar"))) {
                entry.quotechar = one_char_value(v, "quotechar", true)?;
                if entry.quotechar.is_none()
                    && r.get(&HashableKey::str_key(CompactString::from("quoting")))
                        .is_none()
                {
                    entry.quoting = 3;
                }
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("escapechar"))) {
                entry.escapechar = one_char_value(v, "escapechar", true)?;
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("doublequote"))) {
                entry.doublequote = v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("lineterminator"))) {
                if !matches!(v.payload, PyObjectPayload::Str(_)) {
                    return Err(csv_error("\"lineterminator\" must be a string"));
                }
                entry.lineterminator = v.py_to_string();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from(
                "skipinitialspace",
            ))) {
                entry.skipinitialspace = v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("strict"))) {
                entry.strict = v.is_truthy();
            }
        } else if matches!(
            arg.payload,
            PyObjectPayload::Instance(_) | PyObjectPayload::Class(_)
        ) {
            apply_dialect_obj(&mut entry, arg)?;
        } else {
            return Err(PyException::type_error(
                "dialect must be a Dialect subclass",
            ));
        }
    }
    validate_dialect_entry(&entry).map_err(csv_error)?;
    DIALECT_REGISTRY.lock().unwrap().insert(name, entry);
    Ok(PyObject::none())
}

pub(super) fn csv_unregister_dialect(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "unregister_dialect requires a name",
        ));
    }
    if !matches!(args[0].payload, PyObjectPayload::Str(_)) {
        return Err(csv_error("unknown dialect"));
    }
    let name = args[0].py_to_string();
    let mut reg = DIALECT_REGISTRY.lock().unwrap();
    if reg.remove(&name).is_none() {
        return Err(csv_error(format!("unknown dialect: '{}'", name)));
    }
    Ok(PyObject::none())
}

pub(super) fn csv_get_dialect(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("get_dialect requires a name"));
    }
    if !matches!(args[0].payload, PyObjectPayload::Str(_)) {
        return Err(csv_error("unknown dialect"));
    }
    let name = args[0].py_to_string();
    let reg = DIALECT_REGISTRY.lock().unwrap();
    if let Some(entry) = reg.get(&name) {
        Ok(make_dialect_obj(entry))
    } else {
        Err(csv_error(format!("unknown dialect: '{}'", name)))
    }
}

pub(super) fn csv_list_dialects(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if !args.is_empty() {
        return Err(PyException::type_error("list_dialects takes no arguments"));
    }
    let reg = DIALECT_REGISTRY.lock().unwrap();
    let names: Vec<_> = reg
        .keys()
        .map(|k| PyObject::str_val(CompactString::from(k.as_str())))
        .collect();
    Ok(PyObject::list(names))
}

fn readonly_dialect_attr(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Err(PyException::attribute_error("attribute is read-only"))
}

pub(super) fn make_dialect_obj(entry: &CsvDialectEntry) -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__setattr__"),
        make_builtin(readonly_dialect_attr),
    );
    ns.insert(
        CompactString::from("__delattr__"),
        make_builtin(readonly_dialect_attr),
    );
    let cls = PyObject::class(CompactString::from("Dialect"), vec![], ns);
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(
            CompactString::from("delimiter"),
            PyObject::str_val(CompactString::from(entry.delimiter.to_string().as_str())),
        );
        w.insert(
            CompactString::from("quotechar"),
            if let Some(quotechar) = entry.quotechar {
                PyObject::str_val(CompactString::from(quotechar.to_string().as_str()))
            } else {
                PyObject::none()
            },
        );
        w.insert(
            CompactString::from("doublequote"),
            PyObject::bool_val(entry.doublequote),
        );
        w.insert(
            CompactString::from("lineterminator"),
            PyObject::str_val(CompactString::from(entry.lineterminator.as_str())),
        );
        w.insert(CompactString::from("quoting"), PyObject::int(entry.quoting));
        w.insert(
            CompactString::from("skipinitialspace"),
            PyObject::bool_val(entry.skipinitialspace),
        );
        w.insert(
            CompactString::from("strict"),
            PyObject::bool_val(entry.strict),
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
        quotechar: Some(quotechar),
        escapechar: None,
        doublequote: true,
        lineterminator: "\r\n".into(),
        quoting: 0,
        skipinitialspace: false,
        strict: false,
    };
    Ok(make_dialect_obj(&entry))
}
