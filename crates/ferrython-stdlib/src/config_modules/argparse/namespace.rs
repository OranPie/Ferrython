use super::*;

// ── argparse module (basic) ──

fn argparse_is_keyword(name: &str) -> bool {
    matches!(
        name,
        "False"
            | "None"
            | "True"
            | "and"
            | "as"
            | "assert"
            | "async"
            | "await"
            | "break"
            | "class"
            | "continue"
            | "def"
            | "del"
            | "elif"
            | "else"
            | "except"
            | "finally"
            | "for"
            | "from"
            | "global"
            | "if"
            | "import"
            | "in"
            | "is"
            | "lambda"
            | "nonlocal"
            | "not"
            | "or"
            | "pass"
            | "raise"
            | "return"
            | "try"
            | "while"
            | "with"
            | "yield"
    )
}

fn argparse_is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_alphabetic())
        && chars.all(|c| c == '_' || c.is_alphanumeric())
        && !argparse_is_keyword(name)
}

fn argparse_namespace_repr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::str_val(CompactString::from("Namespace()")));
    }
    let PyObjectPayload::Instance(inst) = &args[0].payload else {
        return Ok(PyObject::str_val(CompactString::from("Namespace()")));
    };
    let attrs = inst.attrs.read();
    let mut entries: Vec<(String, PyObjectRef)> = attrs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut valid_parts = Vec::new();
    let mut invalid_parts = Vec::new();
    for (name, value) in entries {
        if argparse_is_identifier(&name) {
            valid_parts.push(format!("{}={}", name, value.repr()));
        } else {
            let key = PyObject::str_val(CompactString::from(name.as_str())).repr();
            invalid_parts.push(format!("{}: {}", key, value.repr()));
        }
    }

    if !invalid_parts.is_empty() {
        valid_parts.push(format!("**{{{}}}", invalid_parts.join(", ")));
    }
    Ok(PyObject::str_val(CompactString::from(format!(
        "Namespace({})",
        valid_parts.join(", ")
    ))))
}

fn argparse_namespace_eq(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::not_implemented());
    }
    let (PyObjectPayload::Instance(left), PyObjectPayload::Instance(right)) =
        (&args[0].payload, &args[1].payload)
    else {
        return Ok(PyObject::not_implemented());
    };
    if right.class.get_attr("__argparse_namespace__").is_none() {
        return Ok(PyObject::not_implemented());
    }
    let left_attrs = left.attrs.read();
    let right_attrs = right.attrs.read();
    if left_attrs.len() != right_attrs.len() {
        return Ok(PyObject::bool_val(false));
    }
    for (name, value) in left_attrs.iter() {
        let Some(other_value) = right_attrs.get(name.as_str()) else {
            return Ok(PyObject::bool_val(false));
        };
        let eq = value.compare(other_value, CompareOp::Eq)?;
        if !eq.is_truthy() {
            return Ok(PyObject::bool_val(false));
        }
    }
    Ok(PyObject::bool_val(true))
}

fn argparse_namespace_ne(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let eq = argparse_namespace_eq(args)?;
    if matches!(eq.payload, PyObjectPayload::NotImplemented) {
        Ok(eq)
    } else {
        Ok(PyObject::bool_val(!eq.is_truthy()))
    }
}

fn argparse_namespace_contains(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("Namespace.__contains__", args, 2)?;
    let key = args[1].py_to_string();
    let contains = if let PyObjectPayload::Instance(inst) = &args[0].payload {
        inst.attrs.read().contains_key(key.as_str())
    } else {
        false
    };
    Ok(PyObject::bool_val(contains))
}

fn argparse_namespace_init(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("Namespace.__init__", args, 1)?;
    let Some(last) = args.last() else {
        return Ok(PyObject::none());
    };
    if let PyObjectPayload::Dict(kw_map) = &last.payload {
        if let PyObjectPayload::Instance(ref id) = args[0].payload {
            let mut attrs = id.attrs.write();
            let r = kw_map.read();
            for (k, v) in r.iter() {
                if let HashableKey::Str(ks) = k {
                    attrs.insert(ks.to_compact_string(), v.clone());
                }
            }
        }
    }
    Ok(PyObject::none())
}

pub(super) fn create_argparse_namespace_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__argparse_namespace__"),
        PyObject::bool_val(true),
    );
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_function("Namespace.__init__", argparse_namespace_init),
    );
    ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_function("Namespace.__repr__", argparse_namespace_repr),
    );
    ns.insert(
        CompactString::from("__eq__"),
        PyObject::native_function("Namespace.__eq__", argparse_namespace_eq),
    );
    ns.insert(
        CompactString::from("__ne__"),
        PyObject::native_function("Namespace.__ne__", argparse_namespace_ne),
    );
    ns.insert(
        CompactString::from("__contains__"),
        PyObject::native_function("Namespace.__contains__", argparse_namespace_contains),
    );
    PyObject::class(CompactString::from("Namespace"), vec![], ns)
}
