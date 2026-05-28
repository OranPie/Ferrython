use super::dialect::DIALECT_REGISTRY;
use super::reader::{csv_parse_line, CsvDialect};
use super::writer::csv_quote_field;
use super::*;

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
            text.py_to_string()
                .lines()
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
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::List {
                items: vec![],
                index: 0,
            }),
        ))));
    }
    // Optional fieldnames as second arg
    let fieldnames: Vec<String> =
        if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
            args[1]
                .to_list()?
                .iter()
                .map(|f| f.py_to_string())
                .collect()
        } else {
            // First row is header
            csv_parse_line(&lines[0].py_to_string(), &CsvDialect::default())
                .into_iter()
                .map(|f| f.trim().to_string())
                .collect()
        };
    let data_start = if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
        0
    } else {
        1
    };
    let mut rows = Vec::new();
    for line in &lines[data_start..] {
        let s = line.py_to_string();
        if s.trim().is_empty() {
            continue;
        }
        let values = csv_parse_line(&s, &CsvDialect::default());
        let mut map = IndexMap::new();
        for (i, name) in fieldnames.iter().enumerate() {
            let val = values
                .get(i)
                .map(|v| v.trim().to_string())
                .unwrap_or_default();
            map.insert(
                HashableKey::str_key(CompactString::from(name.as_str())),
                PyObject::str_val(CompactString::from(&val)),
            );
        }
        rows.push(PyObject::dict(map));
    }
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
        PyCell::new(IteratorData::List {
            items: rows,
            index: 0,
        }),
    ))))
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
    // Extract dialect params from trailing kwargs dict
    let mut delimiter = ",".to_string();
    let mut quoting: i64 = 0;
    let mut quotechar = '"';
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(kw) = &last.payload {
            let r = kw.read();
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("dialect"))) {
                let name = v.py_to_string();
                if let Ok(reg) = DIALECT_REGISTRY.lock() {
                    if let Some(entry) = reg.get(&name) {
                        delimiter = entry.delimiter.to_string();
                        quotechar = entry.quotechar;
                    }
                }
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("delimiter"))) {
                delimiter = v.py_to_string();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("quoting"))) {
                if let Some(n) = v.as_int() {
                    quoting = n;
                }
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("quotechar"))) {
                let s = v.py_to_string();
                quotechar = s.chars().next().unwrap_or('"');
            }
        }
    }
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
        let fnames_owned: Vec<String> = fieldnames.clone();

        // writerow(rowdict) — formats row as CSV and writes to fileobj
        let self_ref = inst.clone();
        let fnames_for_row = fnames_owned.clone();
        let delim_row = delimiter.clone();
        let qt_row = quoting;
        let qc_row = quotechar;
        attrs.insert(
            CompactString::from("writerow"),
            PyObject::native_closure("DictWriter.writerow", {
                let self_ref = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("DictWriter.writerow", args, 1)?;
                    let row = &args[0];
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
                        fields.push(csv_quote_field(
                            &val.py_to_string(),
                            qt_row,
                            qc_row,
                            &delim_row,
                        ));
                    }
                    let line = format!("{}\r\n", fields.join(&delim_row));
                    write_to_fileobj(&self_ref, &line)?;
                    Ok(PyObject::none())
                }
            }),
        );

        // writerows(rows)
        let fnames_for_rows = fnames_owned.clone();
        let delim_rows = delimiter.clone();
        let qt_rows = quoting;
        let qc_rows = quotechar;
        attrs.insert(
            CompactString::from("writerows"),
            PyObject::native_closure("DictWriter.writerows", {
                let self_ref = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("DictWriter.writerows", args, 1)?;
                    let rows = args[0].to_list()?;
                    for row in &rows {
                        let mut fields = Vec::new();
                        for fname in &fnames_for_rows {
                            let key = HashableKey::str_key(CompactString::from(fname.as_str()));
                            let val = if let PyObjectPayload::Dict(map) = &row.payload {
                                map.read().get(&key).cloned().unwrap_or_else(PyObject::none)
                            } else {
                                PyObject::none()
                            };
                            fields.push(csv_quote_field(
                                &val.py_to_string(),
                                qt_rows,
                                qc_rows,
                                &delim_rows,
                            ));
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
        let qt_hdr = quoting;
        let qc_hdr = quotechar;
        attrs.insert(
            CompactString::from("writeheader"),
            PyObject::native_closure("DictWriter.writeheader", {
                let self_ref = self_ref.clone();
                let fnames = fnames_owned.clone();
                move |_args: &[PyObjectRef]| {
                    let escaped: Vec<String> = fnames
                        .iter()
                        .map(|f| csv_quote_field(f, qt_hdr, qc_hdr, &delim_hdr))
                        .collect();
                    let line = format!("{}\r\n", escaped.join(&delim_hdr));
                    write_to_fileobj(&self_ref, &line)?;
                    Ok(PyObject::none())
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
pub(super) fn write_to_fileobj(writer_inst: &PyObjectRef, text: &str) -> PyResult<()> {
    if let Some(fileobj) = writer_inst.get_attr("_fileobj") {
        if let Some(write_fn) = fileobj.get_attr("write") {
            if let PyObjectPayload::NativeClosure(nc) = &write_fn.payload {
                let arg = PyObject::str_val(CompactString::from(text));
                (nc.func)(&[arg])?;
                return Ok(());
            }
            if let PyObjectPayload::NativeFunction(nf) = &write_fn.payload {
                let arg = PyObject::str_val(CompactString::from(text));
                (nf.func)(&[arg])?;
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
                    attrs.insert(
                        CompactString::from("__buffer__"),
                        PyObject::str_val(CompactString::from(new_s.as_str())),
                    );
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}
