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

fn class_name(obj: &PyObjectRef) -> Option<String> {
    if let PyObjectPayload::Class(cls) = &obj.payload {
        Some(cls.name.to_string())
    } else {
        None
    }
}

fn dialect_target_class(obj: &PyObjectRef) -> Option<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::Instance(inst) => Some(inst.class.clone()),
        PyObjectPayload::Class(_) => Some(obj.clone()),
        _ => None,
    }
}

fn class_namespace_has(cls: &PyObjectRef, name: &str) -> bool {
    match &cls.payload {
        PyObjectPayload::Class(data) => data.namespace.read().contains_key(name),
        _ => false,
    }
}

fn requires_explicit_dialect_attrs(obj: &PyObjectRef) -> bool {
    let Some(cls) = dialect_target_class(obj) else {
        return false;
    };
    if class_name(&cls).as_deref() == Some("Dialect") {
        return true;
    }
    if let PyObjectPayload::Class(data) = &cls.payload {
        return data
            .bases
            .iter()
            .any(|base| class_name(base).as_deref() == Some("Dialect"));
    }
    false
}

fn validate_explicit_dialect_attrs(obj: &PyObjectRef) -> PyResult<()> {
    if !requires_explicit_dialect_attrs(obj) {
        return Ok(());
    }
    let Some(cls) = dialect_target_class(obj) else {
        return Ok(());
    };
    let missing = [
        "delimiter",
        "doublequote",
        "lineterminator",
        "quoting",
        "skipinitialspace",
    ]
    .iter()
    .any(|name| !class_namespace_has(&cls, name));
    if missing {
        return Err(csv_error("Dialect class missing required attribute"));
    }
    if let Some(quoting) = dialect_attr(obj, "quoting").and_then(|value| value.as_int()) {
        if quoting != 3 && !class_namespace_has(&cls, "quotechar") {
            return Err(csv_error(
                "Dialect class missing required quotechar attribute",
            ));
        }
    }
    Ok(())
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
    validate_explicit_dialect_attrs(obj)?;
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

fn unpickleable_dialect_attr(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Err(PyException::type_error("cannot pickle 'Dialect' instances"))
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
    ns.insert(
        CompactString::from("__copy__"),
        make_builtin(unpickleable_dialect_attr),
    );
    ns.insert(
        CompactString::from("__deepcopy__"),
        make_builtin(unpickleable_dialect_attr),
    );
    ns.insert(
        CompactString::from("__reduce__"),
        make_builtin(unpickleable_dialect_attr),
    );
    ns.insert(
        CompactString::from("__reduce_ex__"),
        make_builtin(unpickleable_dialect_attr),
    );
    let cls = PyObject::class(CompactString::from("Dialect"), vec![], ns);
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(
            CompactString::from("__csv_dialect__"),
            PyObject::bool_val(true),
        );
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
                Ok(PyObject::bool_val(has_header_sample(&sample)))
            }),
        );
    }
    Ok(inst)
}

/// Sniff a CSV sample to detect delimiter and quotechar.
pub(super) fn sniff_dialect(sample: &str, delimiters: Option<&str>) -> PyResult<PyObjectRef> {
    let candidates = delimiters.unwrap_or(",\t;:|+?");
    let mut best_delim = ',';
    let mut best_score = isize::MIN;
    for delim in candidates.chars() {
        let counts: Vec<usize> = sample
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.matches(delim).count())
            .collect();
        if counts.is_empty() {
            continue;
        }
        let total: usize = counts.iter().sum();
        let modal = counts
            .iter()
            .copied()
            .filter(|count| *count > 0)
            .max_by_key(|count| {
                counts
                    .iter()
                    .filter(|candidate| **candidate == *count)
                    .count()
            })
            .unwrap_or(0);
        let consistent = counts.iter().filter(|count| **count == modal).count();
        let score = (consistent as isize * 1000) + total as isize - (modal == 0) as isize * 10_000;
        if score > best_score {
            best_score = score;
            best_delim = delim;
        }
    }
    let quotechar = guess_quotechar(sample, best_delim);
    let doublequote = quotechar
        .map(|q| has_doublequote_pair(sample, q))
        .unwrap_or(false);
    let skipinitialspace = has_space_after_delimiter(sample, best_delim);
    let entry = CsvDialectEntry {
        delimiter: best_delim,
        quotechar,
        escapechar: None,
        doublequote,
        lineterminator: "\r\n".into(),
        quoting: 0,
        skipinitialspace,
        strict: false,
    };
    Ok(make_dialect_obj(&entry))
}

fn quote_delimiter_score(sample: &str, quote: char, delimiter: char) -> usize {
    let mut score = 0usize;
    for line in sample.lines() {
        let chars: Vec<char> = line.chars().collect();
        for (idx, ch) in chars.iter().enumerate() {
            if *ch != quote {
                continue;
            }
            let prev = idx.checked_sub(1).and_then(|i| chars.get(i)).copied();
            let next = chars.get(idx + 1).copied();
            if prev == Some(delimiter)
                || next == Some(delimiter)
                || prev.is_none()
                || next.is_none()
            {
                score += 1;
            }
        }
    }
    score
}

fn guess_quotechar(sample: &str, delimiter: char) -> Option<char> {
    let single = quote_delimiter_score(sample, '\'', delimiter);
    let double = quote_delimiter_score(sample, '"', delimiter);
    if single == 0 && double == 0 {
        Some('"')
    } else if single > double {
        Some('\'')
    } else {
        Some('"')
    }
}

fn has_doublequote_pair(sample: &str, quote: char) -> bool {
    let pair = format!("{quote}{quote}");
    sample.contains(&pair)
}

fn has_space_after_delimiter(sample: &str, delimiter: char) -> bool {
    let mut seen = 0usize;
    let mut spaced = 0usize;
    for line in sample.lines() {
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == delimiter {
                seen += 1;
                if chars.peek() == Some(&' ') {
                    spaced += 1;
                }
            }
        }
    }
    seen > 0 && spaced * 2 >= seen
}

fn has_header_sample(sample: &str) -> bool {
    let lines: Vec<&str> = sample
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if lines.len() < 2 {
        return false;
    }
    let Ok(dialect_obj) = sniff_dialect(sample, None) else {
        return false;
    };
    let delimiter = dialect_obj
        .get_attr("delimiter")
        .and_then(|value| value.py_to_string().chars().next())
        .unwrap_or(',');
    let quotechar = dialect_obj
        .get_attr("quotechar")
        .and_then(|value| value.py_to_string().chars().next());
    let rows: Vec<Vec<String>> = lines
        .iter()
        .take(21)
        .map(|line| split_sniffer_row(line, delimiter, quotechar))
        .collect();
    let Some(header) = rows.first() else {
        return false;
    };
    let columns = header.len();
    if columns == 0 {
        return false;
    }
    let mut column_types: Vec<Option<SnifferColumnType>> = vec![None; columns];
    let mut active = vec![true; columns];
    for row in rows.iter().skip(1) {
        if row.len() != columns {
            continue;
        }
        for (col, value) in row.iter().enumerate() {
            if !active[col] {
                continue;
            }
            let observed = sniffer_column_type(value);
            match &column_types[col] {
                None => column_types[col] = Some(observed),
                Some(existing) if *existing == observed => {}
                Some(_) => active[col] = false,
            }
        }
    }
    let mut votes = 0isize;
    for (col, kind) in column_types.iter().enumerate() {
        if !active[col] {
            continue;
        }
        let Some(kind) = kind else {
            continue;
        };
        let header_value = header[col].trim();
        match kind {
            SnifferColumnType::Numeric => {
                if header_value.parse::<f64>().is_err() {
                    votes += 1;
                } else {
                    votes -= 1;
                }
            }
            SnifferColumnType::TextLen(len) => {
                if header_value.len() != *len {
                    votes += 1;
                } else {
                    votes -= 1;
                }
            }
        }
    }
    votes > 0
}

#[derive(Clone, PartialEq, Eq)]
enum SnifferColumnType {
    Numeric,
    TextLen(usize),
}

fn sniffer_column_type(value: &str) -> SnifferColumnType {
    let trimmed = value.trim();
    if trimmed.parse::<f64>().is_ok() {
        SnifferColumnType::Numeric
    } else {
        SnifferColumnType::TextLen(trimmed.len())
    }
}

fn split_sniffer_row(line: &str, delimiter: char, quotechar: Option<char>) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_quotes {
            if Some(ch) == quotechar {
                if chars.peek().copied() == quotechar {
                    current.push(ch);
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(ch);
            }
        } else if current.trim().is_empty() && Some(ch) == quotechar {
            in_quotes = true;
        } else if ch == delimiter {
            fields.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    fields.push(current.trim().to_string());
    fields
}
