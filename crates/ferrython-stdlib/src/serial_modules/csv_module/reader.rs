use super::dialect::DIALECT_REGISTRY;
use super::*;

/// CSV dialect settings extracted from kwargs.
pub(super) struct CsvDialect {
    delimiter: char,
    quotechar: char,
    escapechar: Option<char>,
    doublequote: bool,
}

impl Default for CsvDialect {
    fn default() -> Self {
        Self {
            delimiter: ',',
            quotechar: '"',
            escapechar: None,
            doublequote: true,
        }
    }
}

/// Extract CSV dialect parameters from kwargs dict (trailing dict arg).
pub(super) fn extract_csv_dialect(args: &[PyObjectRef], skip: usize) -> CsvDialect {
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
            if let Some(dialect_name) = r.get(&HashableKey::str_key(CompactString::from("dialect")))
            {
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
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("delimiter"))) {
                let s = v.py_to_string();
                if let Some(c) = s.chars().next() {
                    d.delimiter = c;
                }
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("quotechar"))) {
                let s = v.py_to_string();
                if let Some(c) = s.chars().next() {
                    d.quotechar = c;
                }
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("escapechar"))) {
                let s = v.py_to_string();
                d.escapechar = s.chars().next();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("doublequote"))) {
                d.doublequote = v.is_truthy();
            }
            break;
        }
    }
    d
}

pub(super) fn csv_parse_line(s: &str, dialect: &CsvDialect) -> Vec<String> {
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
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            } else {
                current.push(ch);
            }
        } else if ch == dialect.quotechar {
            in_quotes = true;
        } else if dialect.escapechar == Some(ch) && !in_quotes {
            // Escape char outside quotes: next char is literal
            if let Some(next) = chars.next() {
                current.push(next);
            }
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

pub(super) fn csv_reader(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
                        PyObjectPayload::NativeFunction(nf) => (nf.func)(&[])?,
                        _ => {
                            return Err(PyException::type_error("csv.reader requires an iterable"))
                        }
                    };
                    drop(attrs);
                    text.py_to_string()
                        .lines()
                        .filter(|l| !l.is_empty())
                        .map(|l| PyObject::str_val(CompactString::from(l)))
                        .collect()
                } else if let Some(read_fn) = attrs.get("read") {
                    let text = match &read_fn.payload {
                        PyObjectPayload::NativeClosure(nc) => (nc.func)(&[])?,
                        PyObjectPayload::NativeFunction(nf) => (nf.func)(&[])?,
                        _ => {
                            return Err(PyException::type_error("csv.reader requires an iterable"))
                        }
                    };
                    drop(attrs);
                    text.py_to_string()
                        .lines()
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
        if s.trim().is_empty() {
            continue;
        }
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

    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("line_num"), PyObject::int(line_count));

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
