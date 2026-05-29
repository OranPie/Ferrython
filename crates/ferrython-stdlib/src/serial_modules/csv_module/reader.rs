use super::current_field_size_limit;
use super::dialect::{
    csv_error, make_dialect_obj, validate_dialect_entry, CsvDialectEntry, DIALECT_REGISTRY,
};
use super::*;

/// CSV dialect settings extracted from kwargs.
#[derive(Clone)]
pub(super) struct CsvDialect {
    pub(super) delimiter: char,
    pub(super) quotechar: Option<char>,
    pub(super) escapechar: Option<char>,
    pub(super) doublequote: bool,
    pub(super) lineterminator: String,
    pub(super) quoting: i64,
    pub(super) skipinitialspace: bool,
    pub(super) strict: bool,
}

impl Default for CsvDialect {
    fn default() -> Self {
        Self {
            delimiter: ',',
            quotechar: Some('"'),
            escapechar: None,
            doublequote: true,
            lineterminator: "\r\n".into(),
            quoting: 0,
            skipinitialspace: false,
            strict: false,
        }
    }
}

impl CsvDialect {
    fn apply_entry(&mut self, entry: &CsvDialectEntry) {
        self.delimiter = entry.delimiter;
        self.quotechar = entry.quotechar;
        self.escapechar = entry.escapechar;
        self.doublequote = entry.doublequote;
        self.lineterminator = entry.lineterminator.clone();
        self.quoting = entry.quoting;
        self.skipinitialspace = entry.skipinitialspace;
        self.strict = entry.strict;
    }

    pub(super) fn to_entry(&self) -> CsvDialectEntry {
        CsvDialectEntry {
            delimiter: self.delimiter,
            quotechar: self.quotechar,
            escapechar: self.escapechar,
            doublequote: self.doublequote,
            lineterminator: self.lineterminator.clone(),
            quoting: self.quoting,
            skipinitialspace: self.skipinitialspace,
            strict: self.strict,
        }
    }
}

pub(super) fn dialect_attr(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    if let Some(value) = obj.get_attr(name) {
        return Some(value);
    }
    match &obj.payload {
        PyObjectPayload::Instance(inst) => inst.attrs.read().get(name).cloned(),
        PyObjectPayload::Class(cls) => cls.namespace.read().get(name).cloned(),
        _ => None,
    }
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
                _ => Err(PyException::type_error(format!(
                    "\"{}\" must be a 1-character string",
                    name
                ))),
            }
        }
        _ => Err(PyException::type_error(format!(
            "\"{}\" must be string, not {}",
            name,
            value.type_name()
        ))),
    }
}

fn apply_dialect_attrs(dialect: &mut CsvDialect, obj: &PyObjectRef) -> PyResult<()> {
    if let Some(v) = dialect_attr(obj, "delimiter") {
        dialect.delimiter = one_char_value(&v, "delimiter", false)?.unwrap();
    }
    let has_quoting = dialect_attr(obj, "quoting").is_some();
    if let Some(v) = dialect_attr(obj, "quoting") {
        dialect.quoting = v
            .as_int()
            .ok_or_else(|| PyException::type_error("\"quoting\" must be an integer"))?;
    }
    if let Some(v) = dialect_attr(obj, "quotechar") {
        dialect.quotechar = one_char_value(&v, "quotechar", true)?;
        if dialect.quotechar.is_none() {
            if !has_quoting {
                dialect.quoting = 3;
            } else if dialect.quoting != 3 {
                return Err(PyException::type_error(
                    "quotechar must be set if quoting enabled",
                ));
            }
        }
    }
    if let Some(v) = dialect_attr(obj, "escapechar") {
        dialect.escapechar = one_char_value(&v, "escapechar", true)?;
    }
    if let Some(v) = dialect_attr(obj, "doublequote") {
        dialect.doublequote = v.is_truthy();
    }
    if let Some(v) = dialect_attr(obj, "lineterminator") {
        if !matches!(v.payload, PyObjectPayload::Str(_)) {
            return Err(PyException::type_error(
                "\"lineterminator\" must be a string",
            ));
        }
        dialect.lineterminator = v.py_to_string();
    }
    if let Some(v) = dialect_attr(obj, "skipinitialspace") {
        dialect.skipinitialspace = v.is_truthy();
    }
    if let Some(v) = dialect_attr(obj, "strict") {
        dialect.strict = v.is_truthy();
    }
    validate_dialect_entry(&dialect.to_entry()).map_err(PyException::type_error)?;
    Ok(())
}

fn validate_keyword_names(map: &ferrython_core::object::FxHashKeyMap) -> PyResult<()> {
    for key in map.keys() {
        if let HashableKey::Str(s) = key {
            if !matches!(
                s.as_str(),
                "dialect"
                    | "delimiter"
                    | "quotechar"
                    | "escapechar"
                    | "doublequote"
                    | "lineterminator"
                    | "quoting"
                    | "skipinitialspace"
                    | "strict"
                    | "fieldnames"
                    | "restkey"
                    | "restval"
                    | "extrasaction"
            ) {
                return Err(PyException::type_error(format!(
                    "'{}' is an invalid keyword argument for this function",
                    s
                )));
            }
        }
    }
    Ok(())
}

/// Extract CSV dialect parameters from kwargs dict (trailing dict arg).
pub(super) fn extract_csv_dialect(args: &[PyObjectRef], skip: usize) -> PyResult<CsvDialect> {
    let mut d = CsvDialect::default();
    for arg in args.iter().skip(skip) {
        // Check for dialect name string → look up registry
        if let PyObjectPayload::Str(name) = &arg.payload {
            if let Ok(reg) = DIALECT_REGISTRY.lock() {
                if let Some(entry) = reg.get(name.as_str()) {
                    d.apply_entry(entry);
                    continue;
                }
            }
            return Err(csv_error("unknown dialect"));
        }
        if matches!(
            &arg.payload,
            PyObjectPayload::Class(_) | PyObjectPayload::Instance(_)
        ) {
            apply_dialect_attrs(&mut d, arg)?;
            continue;
        }
        // Check for kwargs dict
        if let PyObjectPayload::Dict(kw) = &arg.payload {
            let r = kw.read();
            validate_keyword_names(&r)?;
            if let Some(dialect_name) = r.get(&HashableKey::str_key(CompactString::from("dialect")))
            {
                if let PyObjectPayload::Str(name) = &dialect_name.payload {
                    if let Ok(reg) = DIALECT_REGISTRY.lock() {
                        if let Some(entry) = reg.get(name.as_str()) {
                            d.apply_entry(entry);
                        }
                    }
                } else {
                    apply_dialect_attrs(&mut d, dialect_name)?;
                }
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("delimiter"))) {
                d.delimiter = one_char_value(v, "delimiter", false)?.unwrap();
            }
            let has_quoting = r
                .get(&HashableKey::str_key(CompactString::from("quoting")))
                .is_some();
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("quoting"))) {
                d.quoting = v
                    .as_int()
                    .ok_or_else(|| PyException::type_error("\"quoting\" must be an integer"))?;
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("quotechar"))) {
                d.quotechar = one_char_value(v, "quotechar", true)?;
                if d.quotechar.is_none() {
                    if !has_quoting {
                        d.quoting = 3;
                    } else if d.quoting != 3 {
                        return Err(PyException::type_error(
                            "quotechar must be set if quoting enabled",
                        ));
                    }
                }
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("escapechar"))) {
                d.escapechar = one_char_value(v, "escapechar", true)?;
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("doublequote"))) {
                d.doublequote = v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("lineterminator"))) {
                if !matches!(v.payload, PyObjectPayload::Str(_)) {
                    return Err(PyException::type_error(
                        "\"lineterminator\" must be a string",
                    ));
                }
                d.lineterminator = v.py_to_string();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from(
                "skipinitialspace",
            ))) {
                d.skipinitialspace = v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("strict"))) {
                d.strict = v.is_truthy();
            }
            break;
        }
    }
    validate_dialect_entry(&d.to_entry()).map_err(PyException::type_error)?;
    Ok(d)
}

fn csv_parse_line_parts(s: &str, dialect: &CsvDialect) -> PyResult<Vec<(String, bool)>> {
    if dialect.strict && s.contains('\0') {
        return Err(PyException::new(
            ExceptionKind::CsvError,
            "line contains NUL",
        ));
    }
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut at_field_start = true;
    let mut quote_just_closed = false;
    let mut field_quoted = false;
    let mut chars = s.chars().peekable();
    let field_limit = current_field_size_limit();
    let check_field_limit = |field: &str| -> PyResult<()> {
        if field_limit >= 0 && field.chars().count() as i64 > field_limit {
            Err(csv_error("field larger than field limit"))
        } else {
            Ok(())
        }
    };
    while let Some(ch) = chars.next() {
        if in_quotes {
            if Some(ch) == dialect.quotechar {
                if dialect.doublequote && Some(chars.peek().copied()) == Some(dialect.quotechar) {
                    current.push(ch);
                    chars.next();
                } else {
                    in_quotes = false;
                    quote_just_closed = true;
                }
            } else if dialect.escapechar == Some(ch) {
                // Escape char inside quotes: next char is literal
                if let Some(next) = chars.next() {
                    current.push(next);
                } else if dialect.strict {
                    return Err(PyException::new(
                        ExceptionKind::CsvError,
                        "unexpected end of data",
                    ));
                } else {
                    current.push('\n');
                }
            } else {
                current.push(ch);
            }
            at_field_start = false;
        } else if dialect.skipinitialspace && at_field_start && ch == ' ' {
            continue;
        } else if dialect.quoting != 3 && at_field_start && Some(ch) == dialect.quotechar {
            in_quotes = true;
            at_field_start = false;
            field_quoted = true;
        } else if dialect.escapechar == Some(ch) && !in_quotes {
            // Escape char outside quotes: next char is literal
            if let Some(next) = chars.next() {
                current.push(next);
            } else if dialect.strict {
                return Err(PyException::new(
                    ExceptionKind::CsvError,
                    "unexpected end of data",
                ));
            } else {
                current.push('\n');
            }
            quote_just_closed = false;
            at_field_start = false;
        } else if ch == dialect.delimiter {
            check_field_limit(&current)?;
            fields.push((current.clone(), field_quoted));
            current.clear();
            quote_just_closed = false;
            at_field_start = true;
            field_quoted = false;
        } else if ch == '\n' || ch == '\r' {
            return Err(PyException::new(
                ExceptionKind::CsvError,
                "new-line character seen in unquoted field",
            ));
        } else if quote_just_closed && dialect.strict {
            return Err(PyException::new(
                ExceptionKind::CsvError,
                "delimiter expected after closing quote",
            ));
        } else {
            current.push(ch);
            quote_just_closed = false;
            at_field_start = false;
        }
    }
    if in_quotes && dialect.strict {
        return Err(PyException::new(
            ExceptionKind::CsvError,
            "unexpected end of data",
        ));
    }
    check_field_limit(&current)?;
    fields.push((current, field_quoted));
    Ok(fields)
}

pub(super) fn csv_parse_line(s: &str, dialect: &CsvDialect) -> PyResult<Vec<String>> {
    Ok(csv_parse_line_parts(s, dialect)?
        .into_iter()
        .map(|(field, _)| field)
        .collect())
}

fn trim_record_ending(mut text: String) -> String {
    if text.ends_with("\r\n") {
        text.truncate(text.len() - 2);
    } else if text.ends_with('\n') || text.ends_with('\r') {
        text.pop();
    }
    text
}

fn record_needs_more(text: &str, dialect: &CsvDialect) -> bool {
    let mut in_quotes = false;
    let mut at_field_start = true;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_quotes {
            if dialect.escapechar == Some(ch) {
                let _ = chars.next();
            } else if Some(ch) == dialect.quotechar {
                if dialect.doublequote && Some(chars.peek().copied()) == Some(dialect.quotechar) {
                    let _ = chars.next();
                } else {
                    in_quotes = false;
                }
            }
        } else if dialect.skipinitialspace && at_field_start && ch == ' ' {
            continue;
        } else if dialect.quoting != 3 && at_field_start && Some(ch) == dialect.quotechar {
            in_quotes = true;
            at_field_start = false;
        } else if ch == dialect.delimiter || ch == '\n' || ch == '\r' {
            at_field_start = true;
        } else {
            at_field_start = false;
        }
    }
    in_quotes
}

pub(super) fn logical_records(
    lines: Vec<PyObjectRef>,
    dialect: &CsvDialect,
) -> PyResult<Vec<String>> {
    let mut records = Vec::new();
    let mut pending = String::new();
    for line in &lines {
        if matches!(
            &line.payload,
            PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_)
        ) {
            return Err(PyException::new(
                ExceptionKind::CsvError,
                "iterator should return strings, not bytes",
            ));
        }
        pending.push_str(&line.py_to_string());
        if record_needs_more(&pending, dialect) {
            if !pending.ends_with('\n') && !pending.ends_with('\r') {
                pending.push('\n');
            }
            continue;
        }
        records.push(trim_record_ending(std::mem::take(&mut pending)));
    }
    if !pending.is_empty() {
        records.push(trim_record_ending(pending));
    }
    Ok(records)
}

pub(super) fn text_to_physical_lines(text: &str) -> Vec<PyObjectRef> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            lines.push(PyObject::str_val(CompactString::from(&text[start..=i])));
            i += 1;
            start = i;
        } else if bytes[i] == b'\r' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                lines.push(PyObject::str_val(CompactString::from(&text[start..=i + 1])));
                i += 2;
            } else {
                lines.push(PyObject::str_val(CompactString::from(&text[start..=i])));
                i += 1;
            }
            start = i;
        } else {
            i += 1;
        }
    }
    if start < text.len() {
        lines.push(PyObject::str_val(CompactString::from(&text[start..])));
    }
    lines
}

fn field_to_object(field: String, quoted: bool, dialect: &CsvDialect) -> PyResult<PyObjectRef> {
    if dialect.quoting == 2 && !quoted && !field.is_empty() {
        let value = field.trim().parse::<f64>().map_err(|_| {
            PyException::value_error(format!("could not convert string to float: '{}'", field))
        })?;
        Ok(PyObject::float(value))
    } else {
        Ok(PyObject::str_val(CompactString::from(field)))
    }
}

pub(super) fn csv_reader(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("csv.reader requires an iterable"));
    }
    let dialect = extract_csv_dialect(args, 1)?;
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
                        PyObjectPayload::NativeFunction(nf) => (nf.func)(&[])?,
                        _ => {
                            return Err(PyException::type_error("csv.reader requires an iterable"))
                        }
                    };
                    drop(attrs);
                    text_to_physical_lines(&text.py_to_string())
                } else if let Some(read_fn) = attrs.get("read") {
                    let text = match &read_fn.payload {
                        PyObjectPayload::NativeClosure(nc) => (nc.func)(&[])?,
                        PyObjectPayload::NativeFunction(nf) => (nf.func)(&[])?,
                        _ => {
                            return Err(PyException::type_error("csv.reader requires an iterable"))
                        }
                    };
                    drop(attrs);
                    text_to_physical_lines(&text.py_to_string())
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
    for s in logical_records(lines, &dialect)? {
        if s.is_empty() {
            rows.push(PyObject::list(vec![]));
            continue;
        }
        let fields: Vec<PyObjectRef> = csv_parse_line_parts(&s, &dialect)?
            .into_iter()
            .map(|(f, quoted)| field_to_object(f, quoted, &dialect))
            .collect::<PyResult<Vec<_>>>()?;
        rows.push(PyObject::list(fields));
    }
    // Build a csv_reader instance that supports both iteration (next()) and
    // list-like access (len(), []) for backward compatibility.
    let line_count = rows.len() as i64;
    let shared_rows = Arc::new(rows);
    let iter_index = Arc::new(Mutex::new(0usize));

    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("line_num"), PyObject::int(line_count));
    attrs.insert(
        CompactString::from("dialect"),
        make_dialect_obj(&dialect.to_entry()),
    );

    // __len__ for len(reader)
    let rows_ref = shared_rows.clone();
    attrs.insert(
        CompactString::from("__len__"),
        PyObject::wrap(PyObjectPayload::NativeClosure(Box::new(
            NativeClosureData {
                name: CompactString::from("__len__"),
                func: std::rc::Rc::new(move |_args| Ok(PyObject::int(rows_ref.len() as i64))),
                pickle_args: None,
            },
        ))),
    );

    // __getitem__ for reader[i]
    let rows_ref = shared_rows.clone();
    attrs.insert(
        CompactString::from("__getitem__"),
        PyObject::wrap(PyObjectPayload::NativeClosure(Box::new(
            NativeClosureData {
                name: CompactString::from("__getitem__"),
                pickle_args: None,
                func: std::rc::Rc::new(move |args| {
                    let idx = if args.is_empty() {
                        return Err(PyException::type_error("__getitem__ requires an index"));
                    } else {
                        args[0].to_int().unwrap_or(0)
                    };
                    let len = rows_ref.len() as i64;
                    let real_idx = if idx < 0 {
                        (len + idx) as usize
                    } else {
                        idx as usize
                    };
                    if real_idx < rows_ref.len() {
                        Ok(rows_ref[real_idx].clone())
                    } else {
                        Err(PyException::index_error("list index out of range"))
                    }
                }),
            },
        ))),
    );

    // __iter__ returns self (the iterator facade)
    let _idx_ref = iter_index.clone();
    let _rows_ref = shared_rows.clone();
    // We create a closure-based iterator that the VM can iterate
    attrs.insert(CompactString::from("__iter__"), {
        // Build a proper Iterator payload for the VM's for-loop
        let rows_vec = shared_rows.to_vec();
        let iter_obj = PyObject::wrap(PyObjectPayload::Iterator(Rc::new(PyCell::new(
            IteratorData::List {
                items: rows_vec,
                index: 0,
            },
        ))));
        let it = iter_obj.clone();
        PyObject::wrap(PyObjectPayload::NativeClosure(Box::new(
            NativeClosureData {
                name: CompactString::from("__iter__"),
                func: std::rc::Rc::new(move |_args| Ok(it.clone())),
                pickle_args: None,
            },
        )))
    });

    // __next__ for next(reader)
    let rows_ref = shared_rows.clone();
    let idx_ref = iter_index.clone();
    attrs.insert(
        CompactString::from("__next__"),
        PyObject::wrap(PyObjectPayload::NativeClosure(Box::new(
            NativeClosureData {
                name: CompactString::from("__next__"),
                pickle_args: None,
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
            },
        ))),
    );

    Ok(PyObject::instance_with_attrs(
        PyObject::class(CompactString::from("csv_reader"), vec![], IndexMap::new()),
        attrs,
    ))
}
