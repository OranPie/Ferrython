use super::*;

// ── pprint module ──

fn pformat_value(
    obj: &PyObjectRef,
    indent: usize,
    width: usize,
    depth: Option<usize>,
    current_depth: usize,
) -> String {
    if let Some(max_d) = depth {
        if current_depth > max_d {
            return "...".to_string();
        }
    }
    let prefix = " ".repeat(indent * current_depth);
    let inner_prefix = " ".repeat(indent * (current_depth + 1));

    match &obj.payload {
        PyObjectPayload::Dict(map) => {
            let r = map.read();
            if r.is_empty() {
                return "{}".to_string();
            }
            let mut entries: Vec<String> = Vec::new();
            for (k, v) in r.iter() {
                let ks = match k {
                    HashableKey::Str(s) => format!("'{}'", s),
                    HashableKey::Int(i) => i.to_string(),
                    HashableKey::Float(f) => format!("{}", f),
                    HashableKey::Bool(b) => {
                        if *b {
                            "True".to_string()
                        } else {
                            "False".to_string()
                        }
                    }
                    HashableKey::None => "None".to_string(),
                    HashableKey::Tuple(t) => format!(
                        "({})",
                        t.iter()
                            .map(|x| match x {
                                HashableKey::Str(s) => format!("'{}'", s),
                                HashableKey::Int(i) => i.to_string(),
                                _ => "...".to_string(),
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    HashableKey::FrozenSet(_) => "frozenset(...)".to_string(),
                    HashableKey::Range(_) => "range(...)".to_string(),
                    HashableKey::Bytes(_)
                    | HashableKey::Identity(_, _)
                    | HashableKey::Custom { .. } => "...".to_string(),
                };
                let vs = pformat_value(v, indent, width, depth, current_depth + 1);
                entries.push(format!("{}: {}", ks, vs));
            }
            let oneline = format!("{{{}}}", entries.join(", "));
            if oneline.len() + prefix.len() <= width {
                return oneline;
            }
            let mut s = String::from("{\n");
            for (i, e) in entries.iter().enumerate() {
                s.push_str(&inner_prefix);
                s.push_str(e);
                if i < entries.len() - 1 {
                    s.push(',');
                }
                s.push('\n');
            }
            s.push_str(&prefix);
            s.push('}');
            s
        }
        PyObjectPayload::List(items) => {
            let r = items.read();
            if r.is_empty() {
                return "[]".to_string();
            }
            let oneline = format!(
                "[{}]",
                r.iter()
                    .map(|v| pformat_value(v, indent, width, depth, current_depth + 1))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if oneline.len() + prefix.len() <= width {
                return oneline;
            }
            let mut s = String::from("[\n");
            for (i, v) in r.iter().enumerate() {
                s.push_str(&inner_prefix);
                s.push_str(&pformat_value(v, indent, width, depth, current_depth + 1));
                if i < r.len() - 1 {
                    s.push(',');
                }
                s.push('\n');
            }
            s.push_str(&prefix);
            s.push(']');
            s
        }
        PyObjectPayload::Tuple(items) => {
            if items.is_empty() {
                return "()".to_string();
            }
            if items.len() == 1 {
                return format!(
                    "({},)",
                    pformat_value(&items[0], indent, width, depth, current_depth + 1)
                );
            }
            let oneline = format!(
                "({})",
                items
                    .iter()
                    .map(|v| pformat_value(v, indent, width, depth, current_depth + 1))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if oneline.len() + prefix.len() <= width {
                return oneline;
            }
            let mut s = String::from("(\n");
            for (i, v) in items.iter().enumerate() {
                s.push_str(&inner_prefix);
                s.push_str(&pformat_value(v, indent, width, depth, current_depth + 1));
                if i < items.len() - 1 {
                    s.push(',');
                }
                s.push('\n');
            }
            s.push_str(&prefix);
            s.push(')');
            s
        }
        PyObjectPayload::Set(items) => {
            let r = items.read();
            if r.is_empty() {
                return "set()".to_string();
            }
            let elems: Vec<String> = r
                .iter()
                .map(|(k, _)| match k {
                    HashableKey::Str(s) => format!("'{}'", s),
                    HashableKey::Int(i) => i.to_string(),
                    HashableKey::Float(f) => f.0.to_string(),
                    HashableKey::Bool(b) => {
                        if *b {
                            "True".to_string()
                        } else {
                            "False".to_string()
                        }
                    }
                    HashableKey::None => "None".to_string(),
                    _ => format!("{:?}", k),
                })
                .collect();
            format!("{{{}}}", elems.join(", "))
        }
        _ => {
            // For strings, add quotes (like repr)
            if let PyObjectPayload::Str(s) = &obj.payload {
                return format!("'{}'", s);
            }
            obj.py_to_string()
        }
    }
}

/// Check if an object is "readable" by Python's eval (i.e., its repr can be round-tripped).
/// Objects with custom classes, functions, or non-standard types are not readable.
fn pprint_is_readable_impl(obj: &PyObjectRef, seen: &mut Vec<usize>) -> bool {
    let id = PyObjectRef::as_ptr(obj) as usize;
    if seen.contains(&id) {
        return false; // circular reference
    }
    match &obj.payload {
        PyObjectPayload::None
        | PyObjectPayload::Bool(_)
        | PyObjectPayload::Int(_)
        | PyObjectPayload::Float(_)
        | PyObjectPayload::Complex { .. }
        | PyObjectPayload::Str(_)
        | PyObjectPayload::Bytes(_)
        | PyObjectPayload::ByteArray(_)
        | PyObjectPayload::Ellipsis => true,
        PyObjectPayload::List(items) => {
            seen.push(id);
            let r = items.read();
            let result = r.iter().all(|v| pprint_is_readable_impl(v, seen));
            seen.pop();
            result
        }
        PyObjectPayload::Tuple(items) => {
            seen.push(id);
            let result = items.iter().all(|v| pprint_is_readable_impl(v, seen));
            seen.pop();
            result
        }
        PyObjectPayload::Dict(map) => {
            seen.push(id);
            let r = map.read();
            let result = r.values().all(|v| pprint_is_readable_impl(v, seen));
            seen.pop();
            result
        }
        PyObjectPayload::Set(items) => {
            seen.push(id);
            let _ = items.read();
            seen.pop();
            true
        }
        PyObjectPayload::FrozenSet(_) => true,
        _ => false,
    }
}

/// Check if an object contains circular references.
fn pprint_is_recursive_impl(obj: &PyObjectRef, seen: &mut Vec<usize>) -> bool {
    let id = PyObjectRef::as_ptr(obj) as usize;
    if seen.contains(&id) {
        return true; // found circular reference
    }
    match &obj.payload {
        PyObjectPayload::List(items) => {
            seen.push(id);
            let r = items.read();
            let result = r.iter().any(|v| pprint_is_recursive_impl(v, seen));
            seen.pop();
            result
        }
        PyObjectPayload::Tuple(items) => {
            seen.push(id);
            let result = items.iter().any(|v| pprint_is_recursive_impl(v, seen));
            seen.pop();
            result
        }
        PyObjectPayload::Dict(map) => {
            seen.push(id);
            let r = map.read();
            let result = r.values().any(|v| pprint_is_recursive_impl(v, seen));
            seen.pop();
            result
        }
        _ => false,
    }
}

fn pprint_isreadable(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::bool_val(true));
    }
    let mut seen = Vec::new();
    Ok(PyObject::bool_val(pprint_is_readable_impl(
        &args[0], &mut seen,
    )))
}

fn pprint_isrecursive(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::bool_val(false));
    }
    let mut seen = Vec::new();
    Ok(PyObject::bool_val(pprint_is_recursive_impl(
        &args[0], &mut seen,
    )))
}

fn pprint_saferepr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::str_val(CompactString::from("")));
    }
    let obj = &args[0];
    let repr = match &obj.payload {
        PyObjectPayload::Str(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
        PyObjectPayload::Bytes(b) => {
            let mut r = String::from("b'");
            for &byte in b.iter() {
                if byte == b'\\' {
                    r.push_str("\\\\");
                } else if byte == b'\'' {
                    r.push_str("\\'");
                } else if byte >= 0x20 && byte < 0x7F {
                    r.push(byte as char);
                } else {
                    r.push_str(&format!("\\x{:02x}", byte));
                }
            }
            r.push('\'');
            r
        }
        _ => pformat_value(obj, 1, 80, None, 0),
    };
    Ok(PyObject::str_val(CompactString::from(repr)))
}

pub fn create_pprint_module() -> PyObjectRef {
    let pretty_printer = make_pretty_printer_class();
    make_module(
        "pprint",
        vec![
            (
                "pprint",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Ok(PyObject::none());
                    }
                    // Parse kwargs: stream, indent, width, depth
                    let mut indent = 1usize;
                    let mut width = 80usize;
                    let mut depth: Option<usize> = None;
                    let mut stream_obj: Option<PyObjectRef> = None;
                    if let Some(last) = args.last() {
                        if let PyObjectPayload::Dict(kw) = &last.payload {
                            let r = kw.read();
                            if let Some(s) =
                                r.get(&HashableKey::str_key(CompactString::from("stream")))
                            {
                                if !matches!(s.payload, PyObjectPayload::None) {
                                    stream_obj = Some(s.clone());
                                }
                            }
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("indent")))
                            {
                                indent = v.as_int().unwrap_or(1) as usize;
                            }
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("width")))
                            {
                                width = v.as_int().unwrap_or(80) as usize;
                            }
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("depth")))
                            {
                                depth = v.as_int().map(|d| d as usize);
                            }
                        }
                    }
                    let text = pformat_value(&args[0], indent, width, depth, 0);
                    if let Some(stream) = stream_obj {
                        if let Some(write_fn) = stream.get_attr("write") {
                            let line = format!("{}\n", text);
                            let text_arg = PyObject::str_val(CompactString::from(&line));
                            match &write_fn.payload {
                                PyObjectPayload::NativeFunction(nf) => {
                                    let _ = (nf.func)(&[text_arg]);
                                }
                                PyObjectPayload::NativeClosure(nc) => {
                                    let _ = (nc.func)(&[text_arg]);
                                }
                                _ => {
                                    println!("{}", text);
                                }
                            }
                        } else {
                            println!("{}", text);
                        }
                    } else {
                        println!("{}", text);
                    }
                    Ok(PyObject::none())
                }),
            ),
            (
                "pformat",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Ok(PyObject::str_val(CompactString::from("")));
                    }
                    let mut indent = 1usize;
                    let mut width = 80usize;
                    let mut depth: Option<usize> = None;
                    if args.len() > 1 {
                        indent = args[1].as_int().unwrap_or(1) as usize;
                    }
                    if args.len() > 2 {
                        width = args[2].as_int().unwrap_or(80) as usize;
                    }
                    if args.len() > 3 {
                        depth = args[3].as_int().map(|d| d as usize);
                    }
                    let text = pformat_value(&args[0], indent, width, depth, 0);
                    Ok(PyObject::str_val(CompactString::from(text)))
                }),
            ),
            ("PrettyPrinter", pretty_printer),
            ("isreadable", make_builtin(pprint_isreadable)),
            ("isrecursive", make_builtin(pprint_isrecursive)),
            ("saferepr", make_builtin(pprint_saferepr)),
        ],
    )
}

fn make_pretty_printer_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        make_builtin(pretty_printer_init),
    );
    ns.insert(
        CompactString::from("pformat"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::str_val(CompactString::from("")));
            }
            let (indent, width, depth) = pretty_printer_settings(&args[0]);
            Ok(PyObject::str_val(CompactString::from(pformat_value(
                &args[1], indent, width, depth, 0,
            ))))
        }),
    );
    ns.insert(
        CompactString::from("pprint"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::none());
            }
            let (indent, width, depth) = pretty_printer_settings(&args[0]);
            println!("{}", pformat_value(&args[1], indent, width, depth, 0));
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("isreadable"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(true));
            }
            pprint_isreadable(&args[1..])
        }),
    );
    ns.insert(
        CompactString::from("isrecursive"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            pprint_isrecursive(&args[1..])
        }),
    );
    PyObject::class(CompactString::from("PrettyPrinter"), vec![], ns)
}

fn pretty_printer_init(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "PrettyPrinter.__init__ requires self",
        ));
    }
    let has_kwargs = args
        .last()
        .is_some_and(|obj| matches!(&obj.payload, PyObjectPayload::Dict(_)));
    let pos_len = if has_kwargs {
        args.len() - 1
    } else {
        args.len()
    };
    if pos_len > 5 {
        return Err(PyException::type_error(
            "PrettyPrinter() takes at most 4 arguments",
        ));
    }
    let mut indent = args.get(1).and_then(|v| v.as_int()).unwrap_or(1);
    let mut width = args.get(2).and_then(|v| v.as_int()).unwrap_or(80);
    let mut depth = args.get(3).and_then(|v| v.as_int());
    if has_kwargs {
        if let PyObjectPayload::Dict(kw) = &args.last().unwrap().payload {
            let r = kw.read();
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("indent"))) {
                indent = v.as_int().unwrap_or(indent);
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("width"))) {
                width = v.as_int().unwrap_or(width);
            }
            if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("depth"))) {
                depth = v.as_int();
            }
        }
    }
    if indent < 0 {
        return Err(PyException::value_error("indent must be >= 0"));
    }
    if width <= 0 {
        return Err(PyException::value_error("width must be > 0"));
    }
    if depth.is_some_and(|d| d <= 0) {
        return Err(PyException::value_error("depth must be > 0"));
    }
    if let PyObjectPayload::Instance(inst) = &args[0].payload {
        let mut attrs = inst.attrs.write();
        attrs.insert(CompactString::from("_indent"), PyObject::int(indent));
        attrs.insert(CompactString::from("_width"), PyObject::int(width));
        attrs.insert(
            CompactString::from("_depth"),
            depth.map(PyObject::int).unwrap_or_else(PyObject::none),
        );
    }
    Ok(PyObject::none())
}

fn pretty_printer_settings(obj: &PyObjectRef) -> (usize, usize, Option<usize>) {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        let attrs = inst.attrs.read();
        let indent = attrs
            .get("_indent")
            .and_then(|v| v.as_int())
            .unwrap_or(1)
            .max(0) as usize;
        let width = attrs
            .get("_width")
            .and_then(|v| v.as_int())
            .unwrap_or(80)
            .max(1) as usize;
        let depth = attrs
            .get("_depth")
            .and_then(|v| v.as_int())
            .map(|d| d.max(1) as usize);
        (indent, width, depth)
    } else {
        (1, 80, None)
    }
}
