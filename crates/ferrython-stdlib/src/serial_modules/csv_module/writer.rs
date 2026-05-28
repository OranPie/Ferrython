use super::dialect::DIALECT_REGISTRY;
use super::*;

/// Quote a CSV field according to quoting mode.
/// QUOTE_MINIMAL=0, QUOTE_ALL=1, QUOTE_NONNUMERIC=2, QUOTE_NONE=3
pub(super) fn csv_quote_field(s: &str, quoting: i64, quotechar: char, delimiter: &str) -> String {
    let qc = quotechar.to_string();
    let escaped = s.replace(&qc, &format!("{qc}{qc}"));
    match quoting {
        1 => format!("{qc}{escaped}{qc}"), // QUOTE_ALL
        2 => {
            // QUOTE_NONNUMERIC: quote if not a number
            if s.parse::<f64>().is_ok() {
                s.to_string()
            } else {
                format!("{qc}{escaped}{qc}")
            }
        }
        3 => s.to_string(), // QUOTE_NONE
        _ => {
            // QUOTE_MINIMAL: only quote if contains delimiter, quotechar, or newline
            if s.contains(delimiter)
                || s.contains(quotechar)
                || s.contains('\n')
                || s.contains('\r')
            {
                format!("{qc}{escaped}{qc}")
            } else {
                s.to_string()
            }
        }
    }
}

pub(super) fn csv_writer(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("csv.writer requires a file object"));
    }
    let fileobj = args[0].clone();
    let mut delimiter = ",".to_string();
    let mut quoting: i64 = 0; // QUOTE_MINIMAL
    let mut quotechar = '"';
    if args.len() > 1 {
        if let PyObjectPayload::Dict(kw) = &args[args.len() - 1].payload {
            let r = kw.read();
            // Check for dialect name first
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("dialect"))) {
                let name = v.py_to_string();
                if let Ok(reg) = DIALECT_REGISTRY.lock() {
                    if let Some(entry) = reg.get(&name) {
                        delimiter = entry.delimiter.to_string();
                        quotechar = entry.quotechar;
                    }
                }
            }
            // Individual overrides take precedence
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("delimiter"))) {
                delimiter = v.py_to_string();
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("quoting"))) {
                if let PyObjectPayload::Int(n) = &v.payload {
                    quoting = n.to_i64().unwrap_or(0);
                }
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("quotechar"))) {
                let s = v.py_to_string();
                quotechar = s.chars().next().unwrap_or('"');
            }
        }
    }

    let cls = PyObject::class(CompactString::from("csv_writer"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(
            CompactString::from("__csv_writer__"),
            PyObject::bool_val(true),
        );
        attrs.insert(CompactString::from("_fileobj"), fileobj.clone());

        // writerow(row) — format and write a single row
        let fo = fileobj.clone();
        let delim = delimiter.clone();
        let qt = quoting;
        let qc = quotechar;
        attrs.insert(
            CompactString::from("writerow"),
            PyObject::native_closure("csv_writer.writerow", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("writerow requires a sequence"));
                }
                let items = a[0].to_list()?;
                let fields: Vec<String> = items
                    .iter()
                    .map(|item| {
                        let s = item.py_to_string();
                        csv_quote_field(&s, qt, qc, &delim)
                    })
                    .collect();
                let line = format!("{}\r\n", fields.join(&delim));
                // Write to fileobj via its write() method
                if let Some(write_fn) = fo.get_attr("write") {
                    match &write_fn.payload {
                        PyObjectPayload::NativeFunction(nf) => {
                            (nf.func)(&[PyObject::str_val(CompactString::from(&line))])?;
                        }
                        PyObjectPayload::NativeClosure(nc) => {
                            (nc.func)(&[PyObject::str_val(CompactString::from(&line))])?;
                        }
                        _ => {}
                    }
                }
                Ok(PyObject::none())
            }),
        );

        // writerows(rows) — write multiple rows
        let fo2 = fileobj;
        let delim2 = delimiter;
        let qt2 = quoting;
        let qc2 = quotechar;
        attrs.insert(
            CompactString::from("writerows"),
            PyObject::native_closure("csv_writer.writerows", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("writerows requires an iterable"));
                }
                let rows = a[0].to_list()?;
                for row in &rows {
                    let items = row.to_list()?;
                    let fields: Vec<String> = items
                        .iter()
                        .map(|item| {
                            let s = item.py_to_string();
                            csv_quote_field(&s, qt2, qc2, &delim2)
                        })
                        .collect();
                    let line = format!("{}\r\n", fields.join(&delim2));
                    if let Some(write_fn) = fo2.get_attr("write") {
                        match &write_fn.payload {
                            PyObjectPayload::NativeFunction(nf) => {
                                (nf.func)(&[PyObject::str_val(CompactString::from(&line))])?;
                            }
                            PyObjectPayload::NativeClosure(nc) => {
                                (nc.func)(&[PyObject::str_val(CompactString::from(&line))])?;
                            }
                            _ => {}
                        }
                    }
                }
                Ok(PyObject::none())
            }),
        );
    }
    Ok(inst)
}
