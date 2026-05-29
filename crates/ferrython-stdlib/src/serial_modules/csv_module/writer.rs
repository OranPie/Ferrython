use super::dialect::make_dialect_obj;
use super::reader::extract_csv_dialect;
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

pub(super) fn csv_field_needs_escape(s: &str, delimiter: &str, quotechar: char) -> bool {
    s.contains(delimiter) || s.contains(quotechar) || s.contains('\n') || s.contains('\r')
}

pub(super) fn csv_quote_row(
    items: &[PyObjectRef],
    quoting: i64,
    quotechar: char,
    delimiter: &str,
) -> PyResult<Vec<String>> {
    let single_empty = items.len() == 1 && matches!(&items[0].payload, PyObjectPayload::None);
    let mut fields = Vec::with_capacity(items.len());
    for item in items {
        fields.push({
            if single_empty {
                if quoting == 3 {
                    return Err(PyException::new(
                        ExceptionKind::CsvError,
                        "single empty field record must be quoted",
                    ));
                }
                format!("{quotechar}{quotechar}")
            } else {
                let s = csv_field_to_string(item);
                if quoting == 3 && csv_field_needs_escape(&s, delimiter, quotechar) {
                    return Err(PyException::new(
                        ExceptionKind::CsvError,
                        "need to escape, but no escapechar set",
                    ));
                }
                csv_quote_field(&s, quoting, quotechar, delimiter)
            }
        });
    }
    Ok(fields)
}

pub(super) fn csv_field_to_string(item: &PyObjectRef) -> String {
    match &item.payload {
        PyObjectPayload::None => String::new(),
        _ => item.py_to_string(),
    }
}

pub(super) fn csv_writer(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("csv.writer requires a file object"));
    }
    let fileobj = args[0].clone();
    let dialect = extract_csv_dialect(args, 1);
    let delimiter = dialect.delimiter.to_string();
    let quoting = dialect.quoting;
    let quotechar = dialect.quotechar;
    let lineterminator = dialect.lineterminator.clone();

    let cls = PyObject::class(CompactString::from("csv_writer"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(
            CompactString::from("__csv_writer__"),
            PyObject::bool_val(true),
        );
        attrs.insert(CompactString::from("_fileobj"), fileobj.clone());
        attrs.insert(
            CompactString::from("dialect"),
            make_dialect_obj(&dialect.to_entry()),
        );

        // writerow(row) — format and write a single row
        let fo = fileobj.clone();
        let delim = delimiter.clone();
        let line_term = lineterminator.clone();
        let qt = quoting;
        let qc = quotechar;
        attrs.insert(
            CompactString::from("writerow"),
            PyObject::native_closure("csv_writer.writerow", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("writerow requires a sequence"));
                }
                let items = a[0].to_list()?;
                let fields = csv_quote_row(&items, qt, qc, &delim)?;
                let line = format!("{}{}", fields.join(&delim), line_term);
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
        let line_term2 = lineterminator;
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
                    let fields = csv_quote_row(&items, qt2, qc2, &delim2)?;
                    let line = format!("{}{}", fields.join(&delim2), line_term2);
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
