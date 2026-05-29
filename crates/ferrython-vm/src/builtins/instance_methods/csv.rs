use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;

fn csv_field_to_string(item: &PyObjectRef) -> String {
    match &item.payload {
        PyObjectPayload::None => String::new(),
        _ => item.py_to_string(),
    }
}

fn csv_quote_row(items: &[PyObjectRef]) -> Vec<String> {
    let single_empty = items.len() == 1 && matches!(&items[0].payload, PyObjectPayload::None);
    items
        .iter()
        .map(|item| {
            if single_empty {
                return "\"\"".to_string();
            }
            let s = csv_field_to_string(item);
            if s.contains(',') || s.contains('"') || s.contains('\n') {
                format!("\"{}\"", s.replace('"', "\"\""))
            } else {
                s
            }
        })
        .collect()
}

pub(crate) fn call_csv_writer_method(
    inst: &ferrython_core::object::InstanceData,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    let attrs = inst.attrs.read();
    let fileobj = attrs
        .get("_fileobj")
        .cloned()
        .unwrap_or_else(PyObject::none);
    let rows = attrs
        .get("_rows")
        .cloned()
        .unwrap_or_else(|| PyObject::list(vec![]));
    drop(attrs);

    match method {
        "writerow" => {
            if args.is_empty() {
                return Err(PyException::type_error("writerow() requires a sequence"));
            }
            let items = args[0].to_list()?;
            let fields = csv_quote_row(&items);
            let line = format!("{}\r\n", fields.join(","));
            // Write to the file object's write method or accumulate in _rows
            if let PyObjectPayload::Instance(fobj_inst) = &fileobj.payload {
                // StringIO write
                if fobj_inst.attrs.read().contains_key("__stringio__") {
                    let mut fobj_attrs = fobj_inst.attrs.write();
                    if let Some(buf) = fobj_attrs.get("_buffer") {
                        let existing = buf.py_to_string();
                        fobj_attrs.insert(
                            CompactString::from("_buffer"),
                            PyObject::str_val(CompactString::from(format!("{}{}", existing, line))),
                        );
                    }
                }
            }
            // Also store in _rows
            if let PyObjectPayload::List(row_list) = &rows.payload {
                row_list
                    .write()
                    .push(PyObject::str_val(CompactString::from(&line)));
            }
            Ok(PyObject::none())
        }
        "writerows" => {
            if args.is_empty() {
                return Err(PyException::type_error("writerows() requires an iterable"));
            }
            let rows_list = args[0].to_list()?;
            for row in rows_list {
                // Recursively call writerow
                call_csv_writer_method(inst, "writerow", &[row])?;
            }
            Ok(PyObject::none())
        }
        _ => Err(PyException::attribute_error(format!(
            "'csv.writer' object has no attribute '{}'",
            method
        ))),
    }
}

pub(crate) fn call_csv_dictwriter_method(
    inst: &ferrython_core::object::InstanceData,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    let attrs = inst.attrs.read();
    if let Some(callable) = attrs.get(method).cloned() {
        drop(attrs);
        return match &callable.payload {
            PyObjectPayload::NativeClosure(nc) => (nc.func)(args),
            PyObjectPayload::NativeFunction(nf) => (nf.func)(args),
            _ => Err(PyException::attribute_error(format!(
                "'csv.DictWriter' object has no callable attribute '{}'",
                method
            ))),
        };
    }
    let fileobj = attrs
        .get("_fileobj")
        .cloned()
        .unwrap_or_else(PyObject::none);
    let fieldnames = attrs
        .get("_fieldnames")
        .cloned()
        .unwrap_or_else(|| PyObject::list(vec![]));
    drop(attrs);

    let field_list = fieldnames.to_list()?;
    let names: Vec<String> = field_list.iter().map(|f| f.py_to_string()).collect();

    match method {
        "writeheader" => {
            let line = format!("{}\r\n", names.join(","));
            if let PyObjectPayload::Instance(fobj_inst) = &fileobj.payload {
                if fobj_inst.attrs.read().contains_key("__stringio__") {
                    let mut fobj_attrs = fobj_inst.attrs.write();
                    if let Some(buf) = fobj_attrs.get("_buffer") {
                        let existing = buf.py_to_string();
                        fobj_attrs.insert(
                            CompactString::from("_buffer"),
                            PyObject::str_val(CompactString::from(format!("{}{}", existing, line))),
                        );
                    }
                }
            }
            Ok(PyObject::none())
        }
        "writerow" => {
            if args.is_empty() {
                return Err(PyException::type_error("writerow() requires a dict"));
            }
            let row_dict = &args[0];
            let mut fields = Vec::new();
            for name in &names {
                // Dict key lookup first (avoids clashing with dict method names like "pop")
                let val = if let PyObjectPayload::Dict(map) = &row_dict.payload {
                    map.read()
                        .get(&HashableKey::str_key(CompactString::from(name.as_str())))
                        .cloned()
                        .unwrap_or_else(PyObject::none)
                } else if let Some(v) = row_dict.get_attr(name) {
                    v
                } else {
                    PyObject::none()
                };
                let s = csv_field_to_string(&val);
                if s.contains(',') || s.contains('"') || s.contains('\n') {
                    fields.push(format!("\"{}\"", s.replace('"', "\"\"")));
                } else {
                    fields.push(s);
                }
            }
            let line = format!("{}\r\n", fields.join(","));
            if let PyObjectPayload::Instance(fobj_inst) = &fileobj.payload {
                if fobj_inst.attrs.read().contains_key("__stringio__") {
                    let mut fobj_attrs = fobj_inst.attrs.write();
                    if let Some(buf) = fobj_attrs.get("_buffer") {
                        let existing = buf.py_to_string();
                        fobj_attrs.insert(
                            CompactString::from("_buffer"),
                            PyObject::str_val(CompactString::from(format!("{}{}", existing, line))),
                        );
                    }
                }
            }
            Ok(PyObject::none())
        }
        "writerows" => {
            if args.is_empty() {
                return Err(PyException::type_error("writerows() requires an iterable"));
            }
            let rows = args[0].to_list()?;
            for row in rows.iter() {
                call_csv_dictwriter_method(inst, "writerow", &[row.clone()])?;
            }
            Ok(PyObject::none())
        }
        _ => Err(PyException::attribute_error(format!(
            "'csv.DictWriter' object has no attribute '{}'",
            method
        ))),
    }
}
