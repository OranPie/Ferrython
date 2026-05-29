use super::dialect::make_dialect_obj;
use super::reader::extract_csv_dialect;
use super::*;
use ferrython_core::object::call_callable;

pub(super) fn csv_field_needs_escape(
    s: &str,
    delimiter: &str,
    quotechar: Option<char>,
    lineterminator: &str,
) -> bool {
    s.contains(delimiter)
        || quotechar.is_some_and(|ch| s.contains(ch))
        || s.contains('\n')
        || s.contains('\r')
        || lineterminator.chars().any(|ch| s.contains(ch))
}

fn csv_escape_unquoted(
    s: &str,
    delimiter: &str,
    quotechar: Option<char>,
    escapechar: Option<char>,
    lineterminator: &str,
) -> PyResult<String> {
    let Some(esc) = escapechar else {
        return Err(PyException::new(
            ExceptionKind::CsvError,
            "need to escape, but no escapechar set",
        ));
    };
    let mut out = String::new();
    for ch in s.chars() {
        if delimiter.contains(ch)
            || Some(ch) == quotechar
            || ch == '\n'
            || ch == '\r'
            || lineterminator.contains(ch)
            || ch == esc
        {
            out.push(esc);
        }
        out.push(ch);
    }
    Ok(out)
}

fn csv_format_field(
    s: &str,
    quoting: i64,
    quotechar: Option<char>,
    delimiter: &str,
    escapechar: Option<char>,
    doublequote: bool,
    lineterminator: &str,
    quote_nonnumeric: bool,
) -> PyResult<String> {
    if quoting == 0 && !doublequote && quotechar.is_some() && escapechar.is_some() {
        let requires_outer_quotes = s.contains(delimiter)
            || s.contains('\n')
            || s.contains('\r')
            || lineterminator.chars().any(|ch| s.contains(ch));
        if !requires_outer_quotes && quotechar.is_some_and(|ch| s.contains(ch)) {
            return csv_escape_unquoted(s, delimiter, quotechar, escapechar, lineterminator);
        }
    }
    if quoting == 3 {
        if csv_field_needs_escape(s, delimiter, quotechar, lineterminator) {
            return csv_escape_unquoted(s, delimiter, quotechar, escapechar, lineterminator);
        }
        return Ok(s.to_string());
    }
    let should_quote = match quoting {
        1 => true,
        2 => quote_nonnumeric,
        _ => csv_field_needs_escape(s, delimiter, quotechar, lineterminator),
    };
    if !should_quote {
        return Ok(s.to_string());
    }
    let Some(quotechar) = quotechar else {
        return Err(PyException::type_error(
            "quotechar must be set if quoting enabled",
        ));
    };
    let mut body = String::new();
    for ch in s.chars() {
        if ch == quotechar {
            if doublequote {
                body.push(quotechar);
                body.push(quotechar);
            } else if let Some(esc) = escapechar {
                body.push(esc);
                body.push(ch);
            } else {
                return Err(PyException::new(
                    ExceptionKind::CsvError,
                    "need to escape, but no escapechar set",
                ));
            }
        } else if Some(ch) == escapechar {
            body.push(ch);
            body.push(ch);
        } else {
            body.push(ch);
        }
    }
    Ok(format!("{quotechar}{body}{quotechar}"))
}

pub(super) fn csv_format_field_for_dict(
    item: &PyObjectRef,
    dialect: &super::reader::CsvDialect,
) -> PyResult<String> {
    let s = csv_field_to_string(item);
    csv_format_field(
        &s,
        dialect.quoting,
        dialect.quotechar,
        &dialect.delimiter.to_string(),
        dialect.escapechar,
        dialect.doublequote,
        &dialect.lineterminator,
        dialect.quoting == 2 && !is_csv_numeric_field(item),
    )
}

pub(super) fn csv_format_string_field_for_dict(
    s: &str,
    dialect: &super::reader::CsvDialect,
) -> PyResult<String> {
    csv_format_field(
        s,
        dialect.quoting,
        dialect.quotechar,
        &dialect.delimiter.to_string(),
        dialect.escapechar,
        dialect.doublequote,
        &dialect.lineterminator,
        dialect.quoting == 2,
    )
}

pub(super) fn csv_quote_row(
    items: &[PyObjectRef],
    dialect: &super::reader::CsvDialect,
) -> PyResult<Vec<String>> {
    let single_empty = items.len() == 1 && matches!(&items[0].payload, PyObjectPayload::None);
    let mut fields = Vec::with_capacity(items.len());
    for item in items {
        fields.push({
            if single_empty {
                if dialect.quoting == 3 {
                    return Err(PyException::new(
                        ExceptionKind::CsvError,
                        "single empty field record must be quoted",
                    ));
                }
                let quotechar = dialect.quotechar.ok_or_else(|| {
                    PyException::type_error("quotechar must be set if quoting enabled")
                })?;
                format!("{}{}", quotechar, quotechar)
            } else {
                let s = csv_field_to_string_checked(item)?;
                csv_format_field(
                    &s,
                    dialect.quoting,
                    dialect.quotechar,
                    &dialect.delimiter.to_string(),
                    dialect.escapechar,
                    dialect.doublequote,
                    &dialect.lineterminator,
                    dialect.quoting == 2 && !is_csv_numeric_field(item),
                )?
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

fn csv_field_to_string_checked(item: &PyObjectRef) -> PyResult<String> {
    match &item.payload {
        PyObjectPayload::None => Ok(String::new()),
        PyObjectPayload::Instance(_) => {
            if let Some(str_fn) = item.get_attr("__str__") {
                let result = call_callable(&str_fn, &[])?;
                return Ok(result.py_to_string());
            }
            Ok(item.py_to_string())
        }
        _ => Ok(item.py_to_string()),
    }
}

fn is_csv_numeric_field(item: &PyObjectRef) -> bool {
    matches!(
        item.payload,
        PyObjectPayload::Int(_) | PyObjectPayload::Float(_) | PyObjectPayload::Bool(_)
    )
}

fn resolve_write_method(fileobj: &PyObjectRef) -> PyResult<PyObjectRef> {
    let Some(write_fn) = fileobj.get_attr("write") else {
        return Err(PyException::type_error(
            "argument 1 must have a \"write\" method",
        ));
    };
    if let PyObjectPayload::Property(prop) = &write_fn.payload {
        if let Some(getter) = prop.fget.as_ref() {
            return call_callable(getter, std::slice::from_ref(fileobj));
        }
        return Err(PyException::attribute_error("unreadable attribute"));
    }
    Ok(write_fn)
}

pub(super) fn csv_writer(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("csv.writer requires a file object"));
    }
    let fileobj = args[0].clone();
    resolve_write_method(&fileobj)?;
    let dialect = extract_csv_dialect(args, 1)?;
    let delimiter = dialect.delimiter.to_string();
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
        let dialect_row = dialect.clone();
        attrs.insert(
            CompactString::from("writerow"),
            PyObject::native_closure("csv_writer.writerow", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("writerow requires a sequence"));
                }
                if matches!(&a[0].payload, PyObjectPayload::None) {
                    return Err(PyException::new(
                        ExceptionKind::CsvError,
                        "iterable expected, not NoneType",
                    ));
                }
                let items = a[0].to_list()?;
                let fields = csv_quote_row(&items, &dialect_row)?;
                let line = format!("{}{}", fields.join(&delim), line_term);
                write_text_to_file(&fo, &line)
            }),
        );

        // writerows(rows) — write multiple rows
        let fo2 = fileobj;
        let delim2 = delimiter;
        let line_term2 = lineterminator;
        let dialect_rows = dialect.clone();
        attrs.insert(
            CompactString::from("writerows"),
            PyObject::native_closure("csv_writer.writerows", move |a: &[PyObjectRef]| {
                if a.is_empty() {
                    return Err(PyException::type_error("writerows requires an iterable"));
                }
                let rows = a[0].to_list()?;
                for row in &rows {
                    let items = row.to_list()?;
                    let fields = csv_quote_row(&items, &dialect_rows)?;
                    let line = format!("{}{}", fields.join(&delim2), line_term2);
                    write_text_to_file(&fo2, &line)?;
                }
                Ok(PyObject::none())
            }),
        );
    }
    Ok(inst)
}

pub(super) fn write_text_to_file(fileobj: &PyObjectRef, line: &str) -> PyResult<PyObjectRef> {
    let write_fn = resolve_write_method(fileobj)?;
    let text = PyObject::str_val(CompactString::from(line));
    match &write_fn.payload {
        PyObjectPayload::NativeFunction(nf) => {
            if fileobj.get_attr("_bind_methods").is_some() {
                (nf.func)(&[fileobj.clone(), text])
            } else {
                (nf.func)(&[text])
            }
        }
        PyObjectPayload::NativeClosure(nc) => (nc.func)(&[text]),
        _ => call_callable(&write_fn, &[text]),
    }
}
