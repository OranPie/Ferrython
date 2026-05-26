use super::*;

pub fn create_string_module() -> PyObjectRef {
    make_module("string", vec![
        ("ascii_lowercase", PyObject::str_val(CompactString::from("abcdefghijklmnopqrstuvwxyz"))),
        ("ascii_uppercase", PyObject::str_val(CompactString::from("ABCDEFGHIJKLMNOPQRSTUVWXYZ"))),
        ("ascii_letters", PyObject::str_val(CompactString::from("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"))),
        ("digits", PyObject::str_val(CompactString::from("0123456789"))),
        ("hexdigits", PyObject::str_val(CompactString::from("0123456789abcdefABCDEF"))),
        ("octdigits", PyObject::str_val(CompactString::from("01234567"))),
        ("punctuation", PyObject::str_val(CompactString::from("!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~"))),
        ("whitespace", PyObject::str_val(CompactString::from(" \t\n\r\x0b\x0c"))),
        ("printable", PyObject::str_val(CompactString::from("0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~ \t\n\r\x0b\x0c"))),
        ("Template", PyObject::native_function("string.Template", template_new)),
        ("Formatter", create_formatter_class()),
        ("capwords", make_builtin(string_capwords)),
    ])
}

fn string_capwords(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("capwords() requires a string"));
    }
    let s = args[0].py_to_string();
    let sep = if args.len() > 1 {
        Some(args[1].py_to_string())
    } else {
        None
    };
    let result: String = match sep {
        Some(ref sep_str) => s
            .split(sep_str.as_str())
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                }
            })
            .collect::<Vec<_>>()
            .join(sep_str),
        None => s
            .split_whitespace()
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    };
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn create_formatter_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("format"),
        make_builtin(formatter_format),
    );
    ns.insert(
        CompactString::from("vformat"),
        make_builtin(formatter_format),
    );
    PyObject::class(CompactString::from("Formatter"), vec![], ns)
}

fn formatter_format(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // args[0] = self (Formatter instance), args[1] = format_string, rest = positional/kwargs
    if args.len() < 2 {
        return Err(PyException::type_error("format() requires a format string"));
    }
    let fmt_str = args[1].py_to_string();
    let pos_args = if args.len() > 2 {
        &args[2..]
    } else {
        &[] as &[PyObjectRef]
    };
    // Check if last arg is a kwargs dict
    let (pos_args_final, kwargs) = if let Some(last) = pos_args.last() {
        if let PyObjectPayload::Dict(map) = &last.payload {
            (&pos_args[..pos_args.len() - 1], Some(map.read().clone()))
        } else {
            (pos_args, None)
        }
    } else {
        (pos_args, None)
    };
    let result = format_string_impl(&fmt_str, pos_args_final, &kwargs)?;
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn format_string_impl(
    fmt: &str,
    pos_args: &[PyObjectRef],
    kwargs: &Option<FxHashKeyMap>,
) -> PyResult<String> {
    let mut result = String::new();
    let mut auto_idx = 0usize;
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            if i + 1 < chars.len() && chars[i + 1] == '{' {
                result.push('{');
                i += 2;
                continue;
            }
            i += 1;
            let start = i;
            let mut depth = 1;
            while i < chars.len() && depth > 0 {
                if chars[i] == '{' {
                    depth += 1;
                }
                if chars[i] == '}' {
                    depth -= 1;
                }
                if depth > 0 {
                    i += 1;
                }
            }
            let field: String = chars[start..i].iter().collect();
            i += 1; // skip }
                    // Parse field_name:format_spec
            let (field_name, _format_spec) = if let Some(colon) = field.find(':') {
                (&field[..colon], &field[colon + 1..])
            } else {
                (field.as_str(), "")
            };
            let value = if field_name.is_empty() {
                if auto_idx < pos_args.len() {
                    let v = pos_args[auto_idx].clone();
                    auto_idx += 1;
                    v
                } else {
                    return Err(PyException::index_error("Replacement index out of range"));
                }
            } else if let Ok(idx) = field_name.parse::<usize>() {
                if idx < pos_args.len() {
                    pos_args[idx].clone()
                } else {
                    return Err(PyException::index_error("Replacement index out of range"));
                }
            } else if let Some(ref kw) = kwargs {
                kw.get(&HashableKey::str_key(CompactString::from(field_name)))
                    .cloned()
                    .ok_or_else(|| PyException::key_error(format!("'{}'", field_name)))?
            } else {
                return Err(PyException::key_error(format!("'{}'", field_name)));
            };
            result.push_str(&value.py_to_string());
        } else if chars[i] == '}' {
            if i + 1 < chars.len() && chars[i + 1] == '}' {
                result.push('}');
                i += 2;
                continue;
            }
            result.push('}');
            i += 1;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    Ok(result)
}

fn template_substitute(template: &str, kwargs: &FxHashKeyMap, safe: bool) -> PyResult<String> {
    let mut result = String::new();
    let chars: Vec<char> = template.chars().collect();
    let len = chars.len();
    let mut i = 0;
    while i < len {
        if chars[i] == '$' && i + 1 < len {
            if chars[i + 1] == '$' {
                // Escaped $$
                result.push('$');
                i += 2;
            } else if chars[i + 1] == '{' {
                // ${name} form
                let start = i + 2;
                if let Some(end_pos) = chars[start..].iter().position(|&c| c == '}') {
                    let name: String = chars[start..start + end_pos].iter().collect();
                    let key = HashableKey::str_key(CompactString::from(&name));
                    if let Some(val) = kwargs.get(&key) {
                        result.push_str(&val.py_to_string());
                    } else if safe {
                        result.push_str(&format!("${{{}}}", name));
                    } else {
                        return Err(PyException::key_error(format!("'{}'", name)));
                    }
                    i = start + end_pos + 1;
                } else {
                    result.push('$');
                    i += 1;
                }
            } else if chars[i + 1].is_alphanumeric() || chars[i + 1] == '_' {
                // $name form
                let start = i + 1;
                let mut end = start;
                while end < len && (chars[end].is_alphanumeric() || chars[end] == '_') {
                    end += 1;
                }
                let name: String = chars[start..end].iter().collect();
                let key = HashableKey::str_key(CompactString::from(&name));
                if let Some(val) = kwargs.get(&key) {
                    result.push_str(&val.py_to_string());
                } else if safe {
                    result.push('$');
                    result.push_str(&name);
                } else {
                    return Err(PyException::key_error(format!("'{}'", name)));
                }
                i = end;
            } else {
                result.push('$');
                i += 1;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    Ok(result)
}

fn extract_kwargs_dict(args: &[PyObjectRef]) -> FxHashKeyMap {
    for arg in args.iter().rev() {
        if let PyObjectPayload::Dict(d) = &arg.payload {
            return d.read().clone();
        }
    }
    new_fx_hashkey_map()
}

fn template_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "Template() requires a template string",
        ));
    }
    let tmpl_str = args[0].py_to_string();
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("template"),
        PyObject::str_val(CompactString::from(tmpl_str)),
    );
    attrs.insert(
        CompactString::from("substitute"),
        PyObject::native_function("Template.substitute", template_substitute_method),
    );
    attrs.insert(
        CompactString::from("safe_substitute"),
        PyObject::native_function("Template.safe_substitute", template_safe_substitute_method),
    );
    attrs.insert(
        CompactString::from("_bind_methods"),
        PyObject::bool_val(true),
    );
    Ok(PyObject::module_with_attrs(
        CompactString::from("Template"),
        attrs,
    ))
}

fn template_substitute_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("substitute() needs self"));
    }
    let self_obj = &args[0];
    let tmpl = self_obj
        .get_attr("template")
        .ok_or(PyException::attribute_error("template"))?
        .py_to_string();
    let kwargs = extract_kwargs_dict(&args[1..]);
    let result = template_substitute(&tmpl, &kwargs, false)?;
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn template_safe_substitute_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("safe_substitute() needs self"));
    }
    let self_obj = &args[0];
    let tmpl = self_obj
        .get_attr("template")
        .ok_or(PyException::attribute_error("template"))?
        .py_to_string();
    let kwargs = extract_kwargs_dict(&args[1..]);
    let result = template_substitute(&tmpl, &kwargs, true)?;
    Ok(PyObject::str_val(CompactString::from(result)))
}
