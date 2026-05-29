use super::reader::{csv_parse_line, extract_csv_dialect, logical_records, text_to_physical_lines};
use super::writer::{
    csv_format_field_for_dict, csv_format_string_field_for_dict, write_text_to_file,
};
use super::*;

fn hashable_key_repr(key: &HashableKey) -> String {
    match key {
        HashableKey::None => "None".to_string(),
        HashableKey::Bool(value) => value.to_string(),
        HashableKey::Int(value) => value.to_string(),
        HashableKey::Float(value) => value.to_string(),
        HashableKey::Str(value) => PyObject::str_val(value.to_compact_string()).repr(),
        _ => key.to_object().repr(),
    }
}

fn is_kwargs_dict(obj: &PyObjectRef) -> bool {
    if let PyObjectPayload::Dict(map) = &obj.payload {
        let r = map.read();
        r.keys().all(|key| match key {
            HashableKey::Str(s) => matches!(
                s.as_str(),
                "fieldnames"
                    | "restkey"
                    | "restval"
                    | "dialect"
                    | "delimiter"
                    | "quotechar"
                    | "escapechar"
                    | "doublequote"
                    | "skipinitialspace"
                    | "lineterminator"
                    | "quoting"
                    | "strict"
            ),
            _ => false,
        })
    } else {
        false
    }
}

fn dict_reader_skip(args: &[PyObjectRef]) -> usize {
    if args.len() > 1 && !is_kwargs_dict(&args[1]) {
        2
    } else {
        1
    }
}

pub(super) fn csv_dict_reader(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "csv.DictReader requires an iterable",
        ));
    }
    let lines = if let PyObjectPayload::Instance(inst) = &args[0].payload {
        let attrs = inst.attrs.read();
        // Try getvalue() for StringIO, then read(), then to_list
        if let Some(getvalue) = attrs.get("getvalue") {
            let text = match &getvalue.payload {
                PyObjectPayload::NativeClosure(nc) => (nc.func)(&[])?,
                PyObjectPayload::NativeFunction(nf) => (nf.func)(&[])?,
                _ => {
                    drop(attrs);
                    return Ok(PyObject::list(args[0].to_list().unwrap_or_default()));
                }
            };
            drop(attrs);
            text_to_physical_lines(&text.py_to_string())
        } else if let Some(read_fn) = attrs.get("read") {
            let text = match &read_fn.payload {
                PyObjectPayload::NativeClosure(nc) => (nc.func)(&[])?,
                PyObjectPayload::NativeFunction(nf) => (nf.func)(&[])?,
                _ => {
                    drop(attrs);
                    return Err(PyException::type_error(
                        "csv.DictReader requires an iterable",
                    ));
                }
            };
            drop(attrs);
            text_to_physical_lines(&text.py_to_string())
        } else {
            drop(attrs);
            args[0].to_list()?
        }
    } else {
        args[0].to_list()?
    };
    let dialect = extract_csv_dialect(args, dict_reader_skip(args))?;
    let records = logical_records(lines, &dialect)?;
    if records.is_empty() {
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::List {
                items: vec![],
                index: 0,
            }),
        ))));
    }
    let kwargs = args
        .last()
        .filter(|arg| is_kwargs_dict(arg))
        .and_then(|arg| {
            if let PyObjectPayload::Dict(map) = &arg.payload {
                Some(map.read())
            } else {
                None
            }
        });
    let fieldnames_arg = if args.len() >= 2 && !is_kwargs_dict(&args[1]) {
        Some(args[1].clone())
    } else {
        kwargs.as_ref().and_then(|kw| {
            kw.get(&HashableKey::str_key(CompactString::from("fieldnames")))
                .cloned()
        })
    };
    let restkey = kwargs
        .as_ref()
        .and_then(|kw| {
            kw.get(&HashableKey::str_key(CompactString::from("restkey")))
                .cloned()
        })
        .filter(|v| !matches!(&v.payload, PyObjectPayload::None));
    let restval = kwargs
        .as_ref()
        .and_then(|kw| {
            kw.get(&HashableKey::str_key(CompactString::from("restval")))
                .cloned()
        })
        .unwrap_or_else(PyObject::none);
    drop(kwargs);

    // Optional fieldnames as second arg
    let fieldnames: Vec<String> = if let Some(fnames) = fieldnames_arg {
        if matches!(&fnames.payload, PyObjectPayload::None) {
            Vec::new()
        } else {
            fnames.to_list()?.iter().map(|f| f.py_to_string()).collect()
        }
    } else {
        // First row is header
        let header = &records[0];
        csv_parse_line(header, &dialect)?
            .into_iter()
            .map(|f| f.to_string())
            .collect()
    };
    let data_start = if fieldnames.is_empty() {
        1
    } else if args.len() >= 2 && !is_kwargs_dict(&args[1]) {
        0
    } else if args
        .last()
        .and_then(|arg| {
            if is_kwargs_dict(arg) {
                if let PyObjectPayload::Dict(map) = &arg.payload {
                    map.read()
                        .get(&HashableKey::str_key(CompactString::from("fieldnames")))
                        .cloned()
                } else {
                    None
                }
            } else {
                None
            }
        })
        .is_some()
    {
        0
    } else {
        1
    };
    let mut rows = Vec::new();
    for s in &records[data_start..] {
        if s.is_empty() {
            continue;
        }
        let values = csv_parse_line(s, &dialect)?;
        let mut map = IndexMap::new();
        for (i, name) in fieldnames.iter().enumerate() {
            let val = values
                .get(i)
                .map(|v| PyObject::str_val(CompactString::from(v.as_str())))
                .unwrap_or_else(|| restval.clone());
            map.insert(
                HashableKey::str_key(CompactString::from(name.as_str())),
                val,
            );
        }
        if values.len() > fieldnames.len() {
            let extras: Vec<PyObjectRef> = values[fieldnames.len()..]
                .iter()
                .map(|v| PyObject::str_val(CompactString::from(v.as_str())))
                .collect();
            let key = restkey
                .as_ref()
                .map(|v| HashableKey::str_key(CompactString::from(v.py_to_string())))
                .unwrap_or(HashableKey::None);
            map.insert(key, PyObject::list(extras));
        }
        rows.push(PyObject::dict(map));
    }
    let fieldnames_list: Vec<PyObjectRef> = fieldnames
        .iter()
        .map(|name| PyObject::str_val(CompactString::from(name.as_str())))
        .collect();
    let shared_rows = Arc::new(rows);
    let iter_index = Arc::new(Mutex::new(0usize));
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("fieldnames"),
        PyObject::list(fieldnames_list),
    );

    let rows_for_iter = shared_rows.clone();
    attrs.insert(
        CompactString::from("__iter__"),
        PyObject::native_closure("DictReader.__iter__", move |_| {
            Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(IteratorData::List {
                    items: rows_for_iter.to_vec(),
                    index: 0,
                }),
            ))))
        }),
    );

    let rows_for_next = shared_rows.clone();
    let idx_for_next = iter_index.clone();
    attrs.insert(
        CompactString::from("__next__"),
        PyObject::native_closure("DictReader.__next__", move |_| {
            let mut idx = idx_for_next.lock().unwrap();
            if *idx < rows_for_next.len() {
                let row = rows_for_next[*idx].clone();
                *idx += 1;
                Ok(row)
            } else {
                Err(PyException::stop_iteration())
            }
        }),
    );

    Ok(PyObject::instance_with_attrs(
        PyObject::class(
            CompactString::from("csv_DictReader"),
            vec![],
            IndexMap::new(),
        ),
        attrs,
    ))
}

pub(super) fn csv_dict_writer(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "csv.DictWriter requires fileobj and fieldnames",
        ));
    }
    // Extract fieldnames: either positional arg[1] or kwarg "fieldnames"
    let fieldnames: Vec<String> = if args.len() >= 2 {
        // Check if args[1] is a kwargs dict containing "fieldnames"
        if let PyObjectPayload::Dict(map) = &args[1].payload {
            let r = map.read();
            if let Some(fnames) = r.get(&HashableKey::str_key(CompactString::from("fieldnames"))) {
                fnames.to_list()?.iter().map(|f| f.py_to_string()).collect()
            } else {
                // It's a plain list
                args[1]
                    .to_list()?
                    .iter()
                    .map(|f| f.py_to_string())
                    .collect()
            }
        } else {
            args[1]
                .to_list()?
                .iter()
                .map(|f| f.py_to_string())
                .collect()
        }
    } else {
        return Err(PyException::type_error(
            "csv.DictWriter requires fileobj and fieldnames",
        ));
    };
    let extrasaction = if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(kw) = &last.payload {
            kw.read()
                .get(&HashableKey::str_key(CompactString::from("extrasaction")))
                .map(|v| v.py_to_string().to_ascii_lowercase())
                .unwrap_or_else(|| "raise".to_string())
        } else {
            "raise".to_string()
        }
    } else {
        "raise".to_string()
    };
    if !matches!(extrasaction.as_str(), "raise" | "ignore") {
        return Err(PyException::value_error(
            "extrasaction must be 'raise' or 'ignore'",
        ));
    }
    let dialect = extract_csv_dialect(args, 2)?;
    let delimiter = dialect.delimiter.to_string();
    let cls = PyObject::class(
        CompactString::from("csv_DictWriter"),
        vec![],
        IndexMap::new(),
    );
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(
            CompactString::from("__csv_dictwriter__"),
            PyObject::bool_val(true),
        );
        attrs.insert(CompactString::from("_fileobj"), args[0].clone());
        let fieldnames_list: Vec<PyObjectRef> = fieldnames
            .iter()
            .map(|n| PyObject::str_val(CompactString::from(n.as_str())))
            .collect();
        attrs.insert(
            CompactString::from("_fieldnames"),
            PyObject::list(fieldnames_list.clone()),
        );
        attrs.insert(
            CompactString::from("fieldnames"),
            PyObject::list(fieldnames_list),
        );
        attrs.insert(
            CompactString::from("_extrasaction"),
            PyObject::str_val(CompactString::from(extrasaction.as_str())),
        );
        let fnames_owned: Vec<String> = fieldnames.clone();

        // writerow(rowdict) — formats row as CSV and writes to fileobj
        let self_ref = inst.clone();
        let fnames_for_row = fnames_owned.clone();
        let extra_row_action = extrasaction.clone();
        let delim_row = delimiter.clone();
        let dialect_row = dialect.clone();
        attrs.insert(
            CompactString::from("writerow"),
            PyObject::native_closure("DictWriter.writerow", {
                let self_ref = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("DictWriter.writerow", args, 1)?;
                    let row = &args[0];
                    if extra_row_action == "raise" {
                        if let PyObjectPayload::Dict(map) = &row.payload {
                            let allowed: std::collections::HashSet<&str> =
                                fnames_for_row.iter().map(|s| s.as_str()).collect();
                            let extras: Vec<String> = map
                                .read()
                                .keys()
                                .filter(|key| {
                                    !matches!(key, HashableKey::Str(s) if allowed.contains(s.as_str()))
                                })
                                .map(hashable_key_repr)
                                .collect();
                            if !extras.is_empty() {
                                return Err(PyException::value_error(format!(
                                    "dict contains fields not in fieldnames: {}",
                                    extras.join(", ")
                                )));
                            }
                        }
                    }
                    let mut fields = Vec::new();
                    for fname in &fnames_for_row {
                        let key = HashableKey::str_key(CompactString::from(fname.as_str()));
                        let val = if let PyObjectPayload::Dict(map) = &row.payload {
                            map.read().get(&key).cloned().unwrap_or_else(PyObject::none)
                        } else if let Some(v) = row.get_attr(fname) {
                            v
                        } else {
                            PyObject::none()
                        };
                        fields.push(csv_format_field_for_dict(&val, &dialect_row)?);
                    }
                    let line = format!("{}\r\n", fields.join(&delim_row));
                    write_to_fileobj(&self_ref, &line)
                }
            }),
        );

        // writerows(rows)
        let fnames_for_rows = fnames_owned.clone();
        let extra_rows_action = extrasaction.clone();
        let delim_rows = delimiter.clone();
        let dialect_rows = dialect.clone();
        attrs.insert(
            CompactString::from("writerows"),
            PyObject::native_closure("DictWriter.writerows", {
                let self_ref = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("DictWriter.writerows", args, 1)?;
                    let rows = args[0].to_list()?;
                    for row in &rows {
                        if extra_rows_action == "raise" {
                            if let PyObjectPayload::Dict(map) = &row.payload {
                                let allowed: std::collections::HashSet<&str> =
                                    fnames_for_rows.iter().map(|s| s.as_str()).collect();
                                let extras: Vec<String> = map
                                    .read()
                                    .keys()
                                    .filter(|key| {
                                        !matches!(key, HashableKey::Str(s) if allowed.contains(s.as_str()))
                                    })
                                    .map(hashable_key_repr)
                                    .collect();
                                if !extras.is_empty() {
                                    return Err(PyException::value_error(format!(
                                        "dict contains fields not in fieldnames: {}",
                                        extras.join(", ")
                                    )));
                                }
                            }
                        }
                        let mut fields = Vec::new();
                        for fname in &fnames_for_rows {
                            let key = HashableKey::str_key(CompactString::from(fname.as_str()));
                            let val = if let PyObjectPayload::Dict(map) = &row.payload {
                                map.read().get(&key).cloned().unwrap_or_else(PyObject::none)
                            } else {
                                PyObject::none()
                            };
                            fields.push(csv_format_field_for_dict(&val, &dialect_rows)?);
                        }
                        let line = format!("{}\r\n", fields.join(&delim_rows));
                        write_to_fileobj(&self_ref, &line)?;
                    }
                    Ok(PyObject::none())
                }
            }),
        );

        // writeheader() — writes fieldnames as CSV header line
        let delim_hdr = delimiter.clone();
        let dialect_hdr = dialect.clone();
        attrs.insert(
            CompactString::from("writeheader"),
            PyObject::native_closure("DictWriter.writeheader", {
                let self_ref = self_ref.clone();
                let fnames = fnames_owned.clone();
                move |_args: &[PyObjectRef]| {
                    let escaped: Vec<String> = fnames
                        .iter()
                        .map(|f| csv_format_string_field_for_dict(f, &dialect_hdr))
                        .collect::<PyResult<Vec<_>>>()?;
                    let line = format!("{}\r\n", escaped.join(&delim_hdr));
                    write_to_fileobj(&self_ref, &line)
                }
            }),
        );
    }
    Ok(inst)
}

/// Escape a CSV field: quote if contains comma, quote, or newline.
#[allow(dead_code)]
fn csv_escape_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Write a string to a DictWriter's fileobj via its write() method.
pub(super) fn write_to_fileobj(writer_inst: &PyObjectRef, text: &str) -> PyResult<PyObjectRef> {
    if let Some(fileobj) = writer_inst.get_attr("_fileobj") {
        return write_text_to_file(&fileobj, text);
    }
    Ok(PyObject::none())
}

#[allow(dead_code)]
fn write_to_fileobj_direct(writer_inst: &PyObjectRef, text: &str) -> PyResult<PyObjectRef> {
    if let Some(fileobj) = writer_inst.get_attr("_fileobj") {
        if let Some(write_fn) = fileobj.get_attr("write") {
            if let PyObjectPayload::NativeClosure(nc) = &write_fn.payload {
                let arg = PyObject::str_val(CompactString::from(text));
                return (nc.func)(&[arg]);
            }
            if let PyObjectPayload::NativeFunction(nf) = &write_fn.payload {
                let arg = PyObject::str_val(CompactString::from(text));
                return (nf.func)(&[arg]);
            }
        }
        // Fallback: if fileobj is StringIO, try direct buffer append
        if let PyObjectPayload::Instance(data) = &fileobj.payload {
            let mut attrs = data.attrs.write();
            if let Some(buf) = attrs.get("__buffer__") {
                if let PyObjectPayload::Str(s) = &buf.payload {
                    let mut new_s = s.to_string();
                    new_s.push_str(text);
                    attrs.insert(
                        CompactString::from("__buffer__"),
                        PyObject::str_val(CompactString::from(new_s.as_str())),
                    );
                    return Ok(PyObject::int(text.len() as i64));
                }
            }
        }
    }
    Ok(PyObject::none())
}
