use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, IteratorData,
    make_module, make_builtin, check_args_min,
};
use ferrython_core::types::HashableKey;
use std::sync::{Arc, Mutex};

pub fn create_csv_module() -> PyObjectRef {
    make_module("csv", vec![
        ("reader", make_builtin(csv_reader)),
        ("writer", make_builtin(csv_writer)),
        ("DictReader", make_builtin(csv_dict_reader)),
        ("DictWriter", make_builtin(csv_dict_writer)),
        ("QUOTE_ALL", PyObject::int(1)),
        ("QUOTE_MINIMAL", PyObject::int(0)),
        ("QUOTE_NONNUMERIC", PyObject::int(2)),
        ("QUOTE_NONE", PyObject::int(3)),
    ])
}

fn csv_parse_line(s: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    current.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(ch);
            }
        } else if ch == '"' {
            in_quotes = true;
        } else if ch == ',' {
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
    // Try to get lines from the iterable
    let lines = match args[0].to_list() {
        Ok(items) => items,
        Err(_) => {
            // Handle StringIO-like objects: call getvalue() or read() to get text
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                let attrs = inst.attrs.read();
                if let Some(getvalue) = attrs.get("getvalue") {
                    let text = match &getvalue.payload {
                        PyObjectPayload::NativeClosure { func, .. } => func(&[])?,
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
                        PyObjectPayload::NativeClosure { func, .. } => func(&[])?,
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
        let fields: Vec<PyObjectRef> = csv_parse_line(&s)
            .into_iter()
            .map(|f| PyObject::str_val(CompactString::from(f.trim())))
            .collect();
        rows.push(PyObject::list(fields));
    }
    Ok(PyObject::list(rows))
}

fn csv_writer(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("csv.writer requires a file object"));
    }
    let fileobj = args[0].clone();
    let delimiter = if args.len() > 1 {
        // Check for trailing kwargs dict
        if let PyObjectPayload::Dict(kw) = &args[args.len()-1].payload {
            let r = kw.read();
            r.get(&HashableKey::Str(CompactString::from("delimiter")))
                .map(|v| v.py_to_string()).unwrap_or_else(|| ",".to_string())
        } else { ",".to_string() }
    } else { ",".to_string() };

    let cls = PyObject::class(CompactString::from("csv_writer"), vec![], indexmap::IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("__csv_writer__"), PyObject::bool_val(true));
        attrs.insert(CompactString::from("_fileobj"), fileobj.clone());

        // writerow(row) — format and write a single row
        let fo = fileobj.clone();
        let delim = delimiter.clone();
        attrs.insert(CompactString::from("writerow"), PyObject::native_closure(
            "csv_writer.writerow", move |a: &[PyObjectRef]| {
                if a.is_empty() { return Err(PyException::type_error("writerow requires a sequence")); }
                let items = a[0].to_list()?;
                let fields: Vec<String> = items.iter().map(|item| {
                    let s = item.py_to_string();
                    if s.contains(',') || s.contains('"') || s.contains('\n') {
                        format!("\"{}\"", s.replace('"', "\"\""))
                    } else {
                        s
                    }
                }).collect();
                let line = format!("{}\r\n", fields.join(&delim));
                // Write to fileobj via its write() method
                if let Some(write_fn) = fo.get_attr("write") {
                    match &write_fn.payload {
                        PyObjectPayload::NativeFunction { func, .. } => { func(&[PyObject::str_val(CompactString::from(&line))])?; }
                        PyObjectPayload::NativeClosure { func, .. } => { func(&[PyObject::str_val(CompactString::from(&line))])?; }
                        _ => {}
                    }
                }
                Ok(PyObject::none())
            }
        ));

        // writerows(rows) — write multiple rows
        let fo2 = fileobj;
        let delim2 = delimiter;
        attrs.insert(CompactString::from("writerows"), PyObject::native_closure(
            "csv_writer.writerows", move |a: &[PyObjectRef]| {
                if a.is_empty() { return Err(PyException::type_error("writerows requires an iterable")); }
                let rows = a[0].to_list()?;
                for row in &rows {
                    let items = row.to_list()?;
                    let fields: Vec<String> = items.iter().map(|item| {
                        let s = item.py_to_string();
                        if s.contains(',') || s.contains('"') || s.contains('\n') {
                            format!("\"{}\"", s.replace('"', "\"\""))
                        } else {
                            s
                        }
                    }).collect();
                    let line = format!("{}\r\n", fields.join(&delim2));
                    if let Some(write_fn) = fo2.get_attr("write") {
                        match &write_fn.payload {
                            PyObjectPayload::NativeFunction { func, .. } => { func(&[PyObject::str_val(CompactString::from(&line))])?; }
                            PyObjectPayload::NativeClosure { func, .. } => { func(&[PyObject::str_val(CompactString::from(&line))])?; }
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
                PyObjectPayload::NativeClosure { func, .. } => func(&[])?,
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
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items: vec![], index: 0 })))));
    }
    // Optional fieldnames as second arg
    let fieldnames: Vec<String> = if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
        args[1].to_list()?.iter().map(|f| f.py_to_string()).collect()
    } else {
        // First row is header
        csv_parse_line(&lines[0].py_to_string()).into_iter().map(|f| f.trim().to_string()).collect()
    };
    let data_start = if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) { 0 } else { 1 };
    let mut rows = Vec::new();
    for line in &lines[data_start..] {
        let s = line.py_to_string();
        if s.trim().is_empty() { continue; }
        let values = csv_parse_line(&s);
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
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(IteratorData::List { items: rows, index: 0 })))))
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
                        fields.push(csv_escape_field(&val.py_to_string()));
                    }
                    let line = format!("{}\r\n", fields.join(","));
                    write_to_fileobj(&self_ref, &line)?;
                    Ok(PyObject::none())
                }
            }
        ));

        // writerows(rows)
        let fnames_for_rows = fnames_owned.clone();
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
                            fields.push(csv_escape_field(&val.py_to_string()));
                        }
                        let line = format!("{}\r\n", fields.join(","));
                        write_to_fileobj(&self_ref, &line)?;
                    }
                    Ok(PyObject::none())
                }
            }
        ));

        // writeheader() — writes fieldnames as CSV header line
        attrs.insert(CompactString::from("writeheader"), PyObject::native_closure(
            "DictWriter.writeheader", {
                let self_ref = self_ref.clone();
                let fnames = fnames_owned.clone();
                move |_args: &[PyObjectRef]| {
                    let escaped: Vec<String> = fnames.iter()
                        .map(|f| csv_escape_field(f))
                        .collect();
                    let line = format!("{}\r\n", escaped.join(","));
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
            if let PyObjectPayload::NativeClosure { func, .. } = &write_fn.payload {
                let arg = PyObject::str_val(CompactString::from(text));
                func(&[arg])?;
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
