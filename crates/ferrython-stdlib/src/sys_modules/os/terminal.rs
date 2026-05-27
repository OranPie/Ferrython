use super::*;

/// Create the os.terminal_size class (namedtuple-like).
pub fn make_terminal_size_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("terminal_size.__init__", |args| {
            // terminal_size((columns, lines))
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "terminal_size requires a (columns, lines) argument",
                ));
            }
            let seq = &args[1];
            let (cols, lines) = match &seq.payload {
                PyObjectPayload::Tuple(items) if items.len() >= 2 => {
                    let c = items[0].as_int().unwrap_or(80);
                    let l = items[1].as_int().unwrap_or(24);
                    (c, l)
                }
                _ => {
                    return Err(PyException::type_error(
                        "terminal_size requires a 2-item sequence",
                    ))
                }
            };
            if let PyObjectPayload::Instance(ref data) = args[0].payload {
                let mut attrs = data.attrs.write();
                attrs.insert(CompactString::from("columns"), PyObject::int(cols));
                attrs.insert(CompactString::from("lines"), PyObject::int(lines));
            }
            Ok(PyObject::none())
        }),
    );
    PyObject::class(CompactString::from("terminal_size"), vec![], ns)
}

/// Create a terminal_size instance with columns and lines.
pub fn make_terminal_size_instance(cols: i64, lines: i64) -> PyObjectRef {
    let cls = make_terminal_size_class();
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("columns"), PyObject::int(cols));
    attrs.insert(CompactString::from("lines"), PyObject::int(lines));
    // Support tuple-like indexing, iteration, length, and repr
    let c = cols;
    let l = lines;
    attrs.insert(
        CompactString::from("__getitem__"),
        PyObject::native_closure("terminal_size.__getitem__", move |args| {
            let idx = args.last().and_then(|a| a.as_int()).unwrap_or(0);
            match idx {
                0 => Ok(PyObject::int(c)),
                1 => Ok(PyObject::int(l)),
                _ => Err(PyException::index_error("tuple index out of range")),
            }
        }),
    );
    attrs.insert(
        CompactString::from("__len__"),
        PyObject::native_closure("terminal_size.__len__", |_| Ok(PyObject::int(2))),
    );
    attrs.insert(
        CompactString::from("__iter__"),
        PyObject::native_closure("terminal_size.__iter__", move |_| {
            Ok(PyObject::tuple(vec![PyObject::int(c), PyObject::int(l)]))
        }),
    );
    attrs.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("terminal_size.__repr__", move |_| {
            Ok(PyObject::str_val(CompactString::from(format!(
                "os.terminal_size(columns={}, lines={})",
                c, l
            ))))
        }),
    );
    PyObject::instance_with_attrs(cls, attrs)
}
