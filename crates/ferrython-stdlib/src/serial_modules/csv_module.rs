use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, IteratorData,
    make_module, make_builtin, check_args_min,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
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
            // Handle StringIO-like objects: read the full text and split into lines
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if inst.attrs.read().contains_key("__stringio__") {
                    let attrs = inst.attrs.read();
                    let buf = attrs.get("_buffer").map(|b| b.py_to_string()).unwrap_or_default();
                    buf.lines()
                        .map(|l| PyObject::str_val(CompactString::from(l)))
                        .collect()
                } else {
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
    let cls = PyObject::class(CompactString::from("csv_writer"), vec![], indexmap::IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("__csv_writer__"), PyObject::bool_val(true));
        attrs.insert(CompactString::from("_fileobj"), args[0].clone());
        attrs.insert(CompactString::from("_rows"), PyObject::list(vec![]));
    }
    Ok(inst)
}

fn csv_dict_reader(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("csv.DictReader requires an iterable"));
    }
    let lines = if let PyObjectPayload::Instance(inst) = &args[0].payload {
        let attrs = inst.attrs.read();
        if attrs.contains_key("__stringio__") {
            let buf = attrs.get("_buffer").map(|b| b.py_to_string()).unwrap_or_default();
            drop(attrs);
            buf.lines().filter(|l| !l.is_empty()).map(|l| PyObject::str_val(CompactString::from(l))).collect()
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
        attrs.insert(CompactString::from("_rows"), PyObject::list(vec![]));

        // writerow(rowdict) — appends one row (dict) to internal buffer
        let self_ref = inst.clone();
        attrs.insert(CompactString::from("writerow"), PyObject::native_closure(
            "DictWriter.writerow", {
                let self_ref = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("DictWriter.writerow", args, 1)?;
                    if let Some(rows) = self_ref.get_attr("_rows") {
                        if let PyObjectPayload::List(items) = &rows.payload {
                            items.write().push(args[0].clone());
                        }
                    }
                    Ok(PyObject::none())
                }
            }
        ));

        // writerows(rows) — appends multiple rows
        attrs.insert(CompactString::from("writerows"), PyObject::native_closure(
            "DictWriter.writerows", {
                let self_ref = self_ref.clone();
                move |args: &[PyObjectRef]| {
                    check_args_min("DictWriter.writerows", args, 1)?;
                    let new_rows = args[0].to_list()?;
                    if let Some(rows) = self_ref.get_attr("_rows") {
                        if let PyObjectPayload::List(items) = &rows.payload {
                            let mut w = items.write();
                            for row in new_rows {
                                w.push(row);
                            }
                        }
                    }
                    Ok(PyObject::none())
                }
            }
        ));

        // writeheader() — writes fieldnames as first row
        attrs.insert(CompactString::from("writeheader"), PyObject::native_closure(
            "DictWriter.writeheader", {
                let self_ref = self_ref.clone();
                move |_args: &[PyObjectRef]| {
                    if let Some(fnames) = self_ref.get_attr("_fieldnames") {
                        if let Some(rows) = self_ref.get_attr("_rows") {
                            if let PyObjectPayload::List(items) = &rows.payload {
                                // Create a dict mapping fieldname->fieldname (header row)
                                let header_items: Vec<PyObjectRef> = if let Ok(fl) = fnames.to_list() {
                                    fl
                                } else {
                                    vec![]
                                };
                                let mut map = IndexMap::new();
                                for f in &header_items {
                                    let key = HashableKey::Str(CompactString::from(f.py_to_string()));
                                    map.insert(key, f.clone());
                                }
                                items.write().push(PyObject::dict(map));
                            }
                        }
                    }
                    Ok(PyObject::none())
                }
            }
        ));
    }
    Ok(inst)
}
