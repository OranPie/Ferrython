use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    call_callable, check_args, make_builtin, make_module, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

pub fn create_tomllib_module() -> PyObjectRef {
    make_module(
        "tomllib",
        vec![
            ("loads", make_builtin(tomllib_loads)),
            ("load", make_builtin(tomllib_load)),
            (
                "TOMLDecodeError",
                PyObject::class(
                    CompactString::from("TOMLDecodeError"),
                    vec![PyObject::exception_type(ExceptionKind::ValueError)],
                    IndexMap::new(),
                ),
            ),
            (
                "__all__",
                PyObject::list(vec![
                    PyObject::str_val(CompactString::from("loads")),
                    PyObject::str_val(CompactString::from("load")),
                    PyObject::str_val(CompactString::from("TOMLDecodeError")),
                ]),
            ),
        ],
    )
}

fn tomllib_load(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("tomllib.load", args, 1)?;
    let read = args[0]
        .get_attr("read")
        .ok_or_else(|| PyException::attribute_error("read"))?;
    let data = call_callable(&read, &[])?;
    parse_toml_text(&data.py_to_string())
}

fn tomllib_loads(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("tomllib.loads", args, 1)?;
    parse_toml_text(&args[0].py_to_string())
}

fn parse_toml_text(text: &str) -> PyResult<PyObjectRef> {
    let root = PyObject::dict(IndexMap::new());
    let mut current_path: Vec<String> = Vec::new();
    for line in text.lines() {
        let stripped = line.trim();
        if stripped.is_empty() || stripped.starts_with('#') {
            continue;
        }
        if stripped.starts_with("[[") && stripped.ends_with("]]") {
            let parts = split_toml_key(stripped[2..stripped.len() - 2].trim());
            if parts.is_empty() {
                return Err(toml_error("empty table name"));
            }
            let parent = ensure_toml_table(&root, &parts[..parts.len() - 1])?;
            let last = parts.last().unwrap();
            let list = ensure_array_table(&parent, last)?;
            let new_table = PyObject::dict(IndexMap::new());
            if let PyObjectPayload::List(items) = &list.payload {
                items.write().push(new_table.clone());
            }
            current_path = parts;
            continue;
        }
        if stripped.starts_with('[') && stripped.ends_with(']') {
            let parts = split_toml_key(stripped[1..stripped.len() - 1].trim());
            if parts.is_empty() {
                return Err(toml_error("empty table name"));
            }
            ensure_toml_table(&root, &parts)?;
            current_path = parts;
            continue;
        }
        let Some(eq_pos) = stripped.find('=') else {
            return Err(toml_error("expected key/value pair"));
        };
        let key = trim_toml_key(&stripped[..eq_pos]);
        let value_text = remove_toml_comment(stripped[eq_pos + 1..].trim());
        let current = ensure_toml_table(&root, &current_path)?;
        dict_set_str(&current, &key, parse_toml_value(value_text.trim())?)?;
    }
    Ok(root)
}

fn toml_error(message: &str) -> PyException {
    PyException::new(ExceptionKind::ValueError, message)
}

fn dict_set_str(dict: &PyObjectRef, key: &str, value: PyObjectRef) -> PyResult<()> {
    let PyObjectPayload::Dict(map) = &dict.payload else {
        return Err(PyException::type_error("expected dict"));
    };
    map.write()
        .insert(HashableKey::str_key(CompactString::from(key)), value);
    Ok(())
}

fn dict_get_str(dict: &PyObjectRef, key: &str) -> Option<PyObjectRef> {
    let PyObjectPayload::Dict(map) = &dict.payload else {
        return None;
    };
    map.read()
        .get(&HashableKey::str_key(CompactString::from(key)))
        .cloned()
}

fn ensure_toml_table(root: &PyObjectRef, path: &[String]) -> PyResult<PyObjectRef> {
    let mut current = root.clone();
    for part in path {
        let next = dict_get_str(&current, part).unwrap_or_else(|| {
            let obj = PyObject::dict(IndexMap::new());
            let _ = dict_set_str(&current, part, obj.clone());
            obj
        });
        if let PyObjectPayload::List(items) = &next.payload {
            if let Some(last) = items.read().last() {
                current = last.clone();
                continue;
            }
        }
        if !matches!(next.payload, PyObjectPayload::Dict(_)) {
            return Err(toml_error("table path is not a table"));
        }
        current = next;
    }
    Ok(current)
}

fn ensure_array_table(parent: &PyObjectRef, key: &str) -> PyResult<PyObjectRef> {
    if let Some(existing) = dict_get_str(parent, key) {
        if matches!(existing.payload, PyObjectPayload::List(_)) {
            return Ok(existing);
        }
        let list = PyObject::list(vec![existing]);
        dict_set_str(parent, key, list.clone())?;
        return Ok(list);
    }
    let list = PyObject::list(vec![]);
    dict_set_str(parent, key, list.clone())?;
    Ok(list)
}

fn split_toml_key(key: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote = '\0';
    for ch in key.chars() {
        if in_quotes {
            if ch == quote {
                in_quotes = false;
            } else {
                current.push(ch);
            }
        } else if ch == '"' || ch == '\'' {
            in_quotes = true;
            quote = ch;
        } else if ch == '.' {
            parts.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

fn trim_toml_key(key: &str) -> String {
    key.trim().trim_matches('"').trim_matches('\'').to_string()
}

fn remove_toml_comment(s: &str) -> String {
    let mut in_string = false;
    let mut quote = '\0';
    for (idx, ch) in s.char_indices() {
        if in_string {
            if ch == quote {
                in_string = false;
            }
        } else if ch == '"' || ch == '\'' {
            in_string = true;
            quote = ch;
        } else if ch == '#' {
            return s[..idx].trim_end().to_string();
        }
    }
    s.to_string()
}

fn parse_toml_value(s: &str) -> PyResult<PyObjectRef> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(PyObject::str_val(CompactString::new("")));
    }
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        let mut inner = s[1..s.len() - 1].to_string();
        if s.starts_with('"') {
            inner = inner
                .replace("\\n", "\n")
                .replace("\\t", "\t")
                .replace("\\\"", "\"")
                .replace("\\\\", "\\");
        }
        return Ok(PyObject::str_val(CompactString::from(inner)));
    }
    match s {
        "true" => return Ok(PyObject::bool_val(true)),
        "false" => return Ok(PyObject::bool_val(false)),
        "inf" | "+inf" => return Ok(PyObject::float(f64::INFINITY)),
        "-inf" => return Ok(PyObject::float(f64::NEG_INFINITY)),
        "nan" | "+nan" | "-nan" => return Ok(PyObject::float(f64::NAN)),
        _ => {}
    }
    if s.starts_with('[') && s.ends_with(']') {
        return parse_toml_array(s);
    }
    if s.starts_with('{') && s.ends_with('}') {
        return parse_toml_inline_table(s);
    }
    let clean = s.replace('_', "");
    if let Some(rest) = clean
        .strip_prefix("0x")
        .or_else(|| clean.strip_prefix("0X"))
    {
        if let Ok(value) = i64::from_str_radix(rest, 16) {
            return Ok(PyObject::int(value));
        }
    }
    if let Some(rest) = clean
        .strip_prefix("0o")
        .or_else(|| clean.strip_prefix("0O"))
    {
        if let Ok(value) = i64::from_str_radix(rest, 8) {
            return Ok(PyObject::int(value));
        }
    }
    if let Some(rest) = clean
        .strip_prefix("0b")
        .or_else(|| clean.strip_prefix("0B"))
    {
        if let Ok(value) = i64::from_str_radix(rest, 2) {
            return Ok(PyObject::int(value));
        }
    }
    if clean.contains('.') || clean.contains('e') || clean.contains('E') {
        if let Ok(value) = clean.parse::<f64>() {
            return Ok(PyObject::float(value));
        }
    }
    if let Ok(value) = clean.parse::<i64>() {
        return Ok(PyObject::int(value));
    }
    Ok(PyObject::str_val(CompactString::from(s)))
}

fn parse_toml_array(s: &str) -> PyResult<PyObjectRef> {
    let inner = s[1..s.len() - 1].trim();
    if inner.is_empty() {
        return Ok(PyObject::list(vec![]));
    }
    let mut values = Vec::new();
    for item in split_toml_array(inner) {
        if !item.trim().is_empty() {
            values.push(parse_toml_value(item.trim())?);
        }
    }
    Ok(PyObject::list(values))
}

fn parse_toml_inline_table(s: &str) -> PyResult<PyObjectRef> {
    let inner = s[1..s.len() - 1].trim();
    let dict = PyObject::dict(IndexMap::new());
    if inner.is_empty() {
        return Ok(dict);
    }
    for pair in split_toml_array(inner) {
        if let Some(eq) = pair.find('=') {
            let key = trim_toml_key(&pair[..eq]);
            let value = parse_toml_value(pair[eq + 1..].trim())?;
            dict_set_str(&dict, &key, value)?;
        }
    }
    Ok(dict)
}

fn split_toml_array(s: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut depth = 0i64;
    let mut in_string = false;
    let mut quote = '\0';
    for ch in s.chars() {
        if in_string {
            current.push(ch);
            if ch == quote {
                in_string = false;
            }
        } else if ch == '"' || ch == '\'' {
            in_string = true;
            quote = ch;
            current.push(ch);
        } else if ch == '[' || ch == '{' {
            depth += 1;
            current.push(ch);
        } else if ch == ']' || ch == '}' {
            depth -= 1;
            current.push(ch);
        } else if ch == ',' && depth == 0 {
            items.push(current);
            current = String::new();
        } else {
            current.push(ch);
        }
    }
    if !current.trim().is_empty() {
        items.push(current);
    }
    items
}
