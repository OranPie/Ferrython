//! Serialization stdlib modules (json, csv, base64, struct)

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, IteratorData,
    make_module, make_builtin, check_args, check_args_min,
};
use ferrython_core::types::HashableKey;
use std::sync::{Arc, Mutex};

pub fn create_json_module() -> PyObjectRef {
    make_module("json", vec![
        ("dumps", make_builtin(json_dumps)),
        ("loads", make_builtin(json_loads)),
        ("JSONEncoder", make_builtin(json_encoder_ctor)),
        ("JSONDecoder", make_builtin(json_decoder_ctor)),
    ])
}

fn json_encoder_ctor(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let cls = PyObject::class(CompactString::from("JSONEncoder"), vec![], indexmap::IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("encode"), PyObject::native_closure("encode", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("JSONEncoder.encode() missing argument"));
            }
            let s = py_to_json(&args[0])?;
            Ok(PyObject::str_val(CompactString::from(s)))
        }));
    }
    Ok(inst)
}

fn json_decoder_ctor(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let cls = PyObject::class(CompactString::from("JSONDecoder"), vec![], indexmap::IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("decode"), PyObject::native_closure("decode", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("JSONDecoder.decode() missing argument"));
            }
            let s = match &args[0].payload {
                PyObjectPayload::Str(s) => s.to_string(),
                _ => return Err(PyException::type_error("JSONDecoder.decode requires a string")),
            };
            parse_json_value(&s, &mut 0)
        }));
    }
    Ok(inst)
}

fn json_dumps(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("json.dumps() missing 1 required positional argument: 'obj'"));
    }
    // kwargs may be passed as a trailing dict by the VM
    let mut indent: Option<usize> = None;
    let mut sort_keys = false;
    let mut item_sep = ", ".to_string();
    let mut kv_sep = ": ".to_string();
    let mut default_fn: Option<PyObjectRef> = None;
    if args.len() > 1 {
        if let PyObjectPayload::Dict(kw_map) = &args[args.len() - 1].payload {
            let r = kw_map.read();
            if let Some(ind) = r.get(&HashableKey::Str(CompactString::from("indent"))) {
                indent = match &ind.payload {
                    PyObjectPayload::Int(n) => Some(n.to_i64().unwrap_or(2) as usize),
                    PyObjectPayload::None => None,
                    _ => None,
                };
            }
            if let Some(sk) = r.get(&HashableKey::Str(CompactString::from("sort_keys"))) {
                sort_keys = sk.is_truthy();
            }
            if let Some(seps) = r.get(&HashableKey::Str(CompactString::from("separators"))) {
                if let PyObjectPayload::Tuple(parts) = &seps.payload {
                    if parts.len() == 2 {
                        item_sep = parts[0].py_to_string();
                        kv_sep = parts[1].py_to_string();
                    }
                }
            }
            if let Some(def) = r.get(&HashableKey::Str(CompactString::from("default"))) {
                default_fn = Some(def.clone());
            }
            // cls=CustomEncoder: extract its `default` method as the default_fn
            if default_fn.is_none() {
                if let Some(cls) = r.get(&HashableKey::Str(CompactString::from("cls"))) {
                    if let Some(default_method) = cls.get_attr("default") {
                        default_fn = Some(default_method);
                    }
                }
            }
        } else {
            // Positional indent arg
            match &args[1].payload {
                PyObjectPayload::Int(n) => indent = Some(n.to_i64().unwrap_or(2) as usize),
                _ => {}
            }
        }
    }
    let obj = if sort_keys {
        sort_dict_keys_recursive(&args[0])
    } else {
        args[0].clone()
    };
    let s = if let Some(indent_size) = indent {
        py_to_json_pretty(&obj, 0, indent_size, default_fn.as_ref())?
    } else {
        py_to_json_sep(&obj, &item_sep, &kv_sep, default_fn.as_ref())?
    };
    Ok(PyObject::str_val(CompactString::from(s)))
}

/// Recursively sort dictionary keys for sort_keys=True in json.dumps
fn sort_dict_keys_recursive(obj: &PyObjectRef) -> PyObjectRef {
    match &obj.payload {
        PyObjectPayload::Dict(map) => {
            let r = map.read();
            let mut entries: Vec<_> = r.iter()
                .map(|(k, v)| (k.clone(), sort_dict_keys_recursive(v)))
                .collect();
            entries.sort_by(|(a, _), (b, _)| {
                let a_str = match a { HashableKey::Str(s) => s.to_string(), HashableKey::Int(n) => n.to_string(), _ => format!("{:?}", a) };
                let b_str = match b { HashableKey::Str(s) => s.to_string(), HashableKey::Int(n) => n.to_string(), _ => format!("{:?}", b) };
                a_str.cmp(&b_str)
            });
            drop(r);
            let new_dict = PyObject::dict_from_pairs(vec![]);
            if let PyObjectPayload::Dict(new_map) = &new_dict.payload {
                let mut w = new_map.write();
                for (k, v) in entries {
                    w.insert(k, v);
                }
            }
            new_dict
        }
        PyObjectPayload::List(items) => {
            let r = items.read();
            let sorted: Vec<_> = r.iter().map(|i| sort_dict_keys_recursive(i)).collect();
            drop(r);
            PyObject::list(sorted)
        }
        PyObjectPayload::Tuple(items) => {
            let sorted: Vec<_> = items.iter().map(|i| sort_dict_keys_recursive(i)).collect();
            PyObject::tuple(sorted)
        }
        _ => obj.clone(),
    }
}

fn py_to_json_pretty(obj: &PyObjectRef, depth: usize, indent: usize, default: Option<&PyObjectRef>) -> PyResult<String> {
    let pad = " ".repeat(indent * (depth + 1));
    let pad_close = " ".repeat(indent * depth);
    match &obj.payload {
        PyObjectPayload::None => Ok("null".into()),
        PyObjectPayload::Bool(b) => Ok(if *b { "true" } else { "false" }.into()),
        PyObjectPayload::Int(n) => Ok(n.to_string()),
        PyObjectPayload::Float(f) => {
            if f.is_nan() { return Err(PyException::value_error("NaN is not JSON serializable")); }
            if f.is_infinite() { return Err(PyException::value_error("Infinity is not JSON serializable")); }
            Ok(format!("{}", f))
        }
        PyObjectPayload::Str(s) => Ok(format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t"))),
        PyObjectPayload::List(items) => {
            let r = items.read();
            if r.is_empty() { return Ok("[]".into()); }
            let parts: Result<Vec<String>, _> = r.iter().map(|i| py_to_json_pretty(i, depth + 1, indent, default)).collect();
            Ok(format!("[\n{}{}\n{}]", pad, parts?.join(&format!(",\n{}", pad)), pad_close))
        }
        PyObjectPayload::Tuple(items) => {
            if items.is_empty() { return Ok("[]".into()); }
            let parts: Result<Vec<String>, _> = items.iter().map(|i| py_to_json_pretty(i, depth + 1, indent, default)).collect();
            Ok(format!("[\n{}{}\n{}]", pad, parts?.join(&format!(",\n{}", pad)), pad_close))
        }
        PyObjectPayload::Dict(map) => {
            let r = map.read();
            if r.is_empty() { return Ok("{}".into()); }
            let parts: Result<Vec<String>, _> = r.iter().map(|(k, v)| {
                let key_str = match k {
                    HashableKey::Str(s) => format!("\"{}\"", s),
                    HashableKey::Int(n) => format!("\"{}\"", n),
                    _ => return Err(PyException::type_error("keys must be str")),
                };
                let val_str = py_to_json_pretty(v, depth + 1, indent, default)?;
                Ok(format!("{}: {}", key_str, val_str))
            }).collect();
            Ok(format!("{{\n{}{}\n{}}}", pad, parts?.join(&format!(",\n{}", pad)), pad_close))
        }
        PyObjectPayload::Set(map) => {
            let r = map.read();
            let items: Vec<PyObjectRef> = r.keys().map(|k| k.to_object()).collect();
            let list = PyObject::list(items);
            py_to_json_pretty(&list, depth, indent, default)
        }
        _ => json_serialize_fallback(obj, default, |o, d| py_to_json_pretty(o, depth, indent, d)),
    }
}

fn py_to_json(obj: &PyObjectRef) -> PyResult<String> {
    py_to_json_sep(obj, ", ", ": ", None)
}

fn py_to_json_sep(obj: &PyObjectRef, item_sep: &str, kv_sep: &str, default: Option<&PyObjectRef>) -> PyResult<String> {
    match &obj.payload {
        PyObjectPayload::None => Ok("null".into()),
        PyObjectPayload::Bool(b) => Ok(if *b { "true" } else { "false" }.into()),
        PyObjectPayload::Int(n) => Ok(n.to_string()),
        PyObjectPayload::Float(f) => {
            if f.is_nan() { return Err(PyException::value_error("NaN is not JSON serializable")); }
            if f.is_infinite() { return Err(PyException::value_error("Infinity is not JSON serializable")); }
            Ok(format!("{}", f))
        }
        PyObjectPayload::Str(s) => Ok(format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t"))),
        PyObjectPayload::List(items) => {
            let r = items.read();
            let parts: Result<Vec<String>, _> = r.iter().map(|i| py_to_json_sep(i, item_sep, kv_sep, default)).collect();
            Ok(format!("[{}]", parts?.join(item_sep)))
        }
        PyObjectPayload::Tuple(items) => {
            let parts: Result<Vec<String>, _> = items.iter().map(|i| py_to_json_sep(i, item_sep, kv_sep, default)).collect();
            Ok(format!("[{}]", parts?.join(item_sep)))
        }
        PyObjectPayload::Dict(map) => {
            let r = map.read();
            let parts: Result<Vec<String>, _> = r.iter().map(|(k, v)| {
                let key_str = match k {
                    HashableKey::Str(s) => format!("\"{}\"", s),
                    HashableKey::Int(n) => format!("\"{}\"", n),
                    _ => return Err(PyException::type_error("keys must be str")),
                };
                let val_str = py_to_json_sep(v, item_sep, kv_sep, default)?;
                Ok(format!("{}{}{}", key_str, kv_sep, val_str))
            }).collect();
            Ok(format!("{{{}}}", parts?.join(item_sep)))
        }
        PyObjectPayload::InstanceDict(attrs) => {
            let r = attrs.read();
            let parts: Result<Vec<String>, _> = r.iter().map(|(k, v)| {
                let key_str = format!("\"{}\"", k);
                let val_str = py_to_json_sep(v, item_sep, kv_sep, default)?;
                Ok(format!("{}{}{}", key_str, kv_sep, val_str))
            }).collect();
            Ok(format!("{{{}}}", parts?.join(item_sep)))
        }
        PyObjectPayload::Set(map) => {
            // Sets serialize as JSON arrays (common pattern used by custom encoders)
            let r = map.read();
            let mut items: Vec<PyObjectRef> = r.keys().map(|k| k.to_object()).collect();
            let list = PyObject::list(items);
            py_to_json_sep(&list, item_sep, kv_sep, default)
        }
        _ => json_serialize_fallback(obj, default, |o, d| py_to_json_sep(o, item_sep, kv_sep, d)),
    }
}

/// Handle non-primitive objects in JSON serialization:
/// 1. Instance objects → serialize their attrs as a JSON object
/// 2. If a `default` callable is provided, call it and re-serialize the result
/// 3. Otherwise, raise TypeError
fn json_serialize_fallback<F>(
    obj: &PyObjectRef,
    default: Option<&PyObjectRef>,
    recurse: F,
) -> PyResult<String>
where
    F: Fn(&PyObjectRef, Option<&PyObjectRef>) -> PyResult<String>,
{
    // Try serializing Instance attrs as a JSON object
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        let attrs = inst.attrs.read();
        // Filter out internal/dunder attrs
        let public_attrs: Vec<_> = attrs.iter()
            .filter(|(k, _)| !k.starts_with('_'))
            .collect();
        if !public_attrs.is_empty() {
            let parts: Result<Vec<String>, _> = public_attrs.iter().map(|(k, v)| {
                let val_str = recurse(v, default)?;
                Ok(format!("\"{}\": {}", k, val_str))
            }).collect();
            return Ok(format!("{{{}}}", parts?.join(", ")));
        }
    }

    // Try calling the default function if provided
    if let Some(def) = default {
        if let Some(result) = try_call_default(def, obj)? {
            return recurse(&result, None);
        }
    }

    Err(PyException::type_error(format!(
        "Object of type {} is not JSON serializable", obj.type_name()
    )))
}

/// Try to call a default callable (NativeFunction or NativeClosure)
fn try_call_default(default: &PyObjectRef, obj: &PyObjectRef) -> PyResult<Option<PyObjectRef>> {
    match &default.payload {
        PyObjectPayload::NativeFunction { func, .. } => Ok(Some(func(&[obj.clone()])?)),
        PyObjectPayload::NativeClosure { func, .. } => Ok(Some(func(&[obj.clone()])?)),
        _ => Ok(None), // User-defined functions need VM context; fall through
    }
}

fn json_loads(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("json.loads", args, 1)?;
    let s = match &args[0].payload {
        PyObjectPayload::Str(s) => s.to_string(),
        _ => return Err(PyException::type_error("json.loads requires a string")),
    };
    parse_json_value(&s, &mut 0)
}

fn parse_json_value(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    skip_ws(s, pos);
    if *pos >= s.len() { return Err(PyException::value_error("Unexpected end of JSON")); }
    let ch = s.as_bytes()[*pos] as char;
    match ch {
        '"' => parse_json_string(s, pos),
        't' | 'f' => parse_json_bool(s, pos),
        'n' => parse_json_null(s, pos),
        '[' => parse_json_array(s, pos),
        '{' => parse_json_object(s, pos),
        _ => parse_json_number(s, pos),
    }
}

fn skip_ws(s: &str, pos: &mut usize) {
    while *pos < s.len() && s.as_bytes()[*pos].is_ascii_whitespace() { *pos += 1; }
}

fn parse_json_string(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    *pos += 1; // skip "
    let mut result = String::new();
    while *pos < s.len() {
        let ch = s.as_bytes()[*pos] as char;
        if ch == '"' { *pos += 1; return Ok(PyObject::str_val(CompactString::from(result))); }
        if ch == '\\' {
            *pos += 1;
            if *pos >= s.len() { break; }
            let esc = s.as_bytes()[*pos] as char;
            match esc {
                'n' => result.push('\n'),
                't' => result.push('\t'),
                'r' => result.push('\r'),
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                '/' => result.push('/'),
                _ => { result.push('\\'); result.push(esc); }
            }
        } else {
            result.push(ch);
        }
        *pos += 1;
    }
    Err(PyException::value_error("Unterminated string"))
}

fn parse_json_bool(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    if s[*pos..].starts_with("true") { *pos += 4; return Ok(PyObject::bool_val(true)); }
    if s[*pos..].starts_with("false") { *pos += 5; return Ok(PyObject::bool_val(false)); }
    Err(PyException::value_error("Invalid JSON"))
}

fn parse_json_null(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    if s[*pos..].starts_with("null") { *pos += 4; return Ok(PyObject::none()); }
    Err(PyException::value_error("Invalid JSON"))
}

fn parse_json_number(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    let start = *pos;
    let mut is_float = false;
    if *pos < s.len() && s.as_bytes()[*pos] == b'-' { *pos += 1; }
    while *pos < s.len() && s.as_bytes()[*pos].is_ascii_digit() { *pos += 1; }
    if *pos < s.len() && s.as_bytes()[*pos] == b'.' {
        is_float = true; *pos += 1;
        while *pos < s.len() && s.as_bytes()[*pos].is_ascii_digit() { *pos += 1; }
    }
    if *pos < s.len() && (s.as_bytes()[*pos] == b'e' || s.as_bytes()[*pos] == b'E') {
        is_float = true; *pos += 1;
        if *pos < s.len() && (s.as_bytes()[*pos] == b'+' || s.as_bytes()[*pos] == b'-') { *pos += 1; }
        while *pos < s.len() && s.as_bytes()[*pos].is_ascii_digit() { *pos += 1; }
    }
    let num_str = &s[start..*pos];
    if is_float {
        let f: f64 = num_str.parse().map_err(|_| PyException::value_error("Invalid number"))?;
        Ok(PyObject::float(f))
    } else {
        let i: i64 = num_str.parse().map_err(|_| PyException::value_error("Invalid number"))?;
        Ok(PyObject::int(i))
    }
}

fn parse_json_array(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    *pos += 1; // skip [
    let mut items = Vec::new();
    skip_ws(s, pos);
    if *pos < s.len() && s.as_bytes()[*pos] == b']' { *pos += 1; return Ok(PyObject::list(items)); }
    loop {
        items.push(parse_json_value(s, pos)?);
        skip_ws(s, pos);
        if *pos >= s.len() { break; }
        if s.as_bytes()[*pos] == b']' { *pos += 1; return Ok(PyObject::list(items)); }
        if s.as_bytes()[*pos] == b',' { *pos += 1; } else { break; }
    }
    Err(PyException::value_error("Invalid JSON array"))
}

fn parse_json_object(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    *pos += 1; // skip {
    let pairs: Vec<(PyObjectRef, PyObjectRef)> = Vec::new();
    let dict = PyObject::dict_from_pairs(pairs);
    skip_ws(s, pos);
    if *pos < s.len() && s.as_bytes()[*pos] == b'}' { *pos += 1; return Ok(dict); }
    loop {
        skip_ws(s, pos);
        let key = parse_json_string(s, pos)?;
        skip_ws(s, pos);
        if *pos >= s.len() || s.as_bytes()[*pos] != b':' { return Err(PyException::value_error("Expected ':'")); }
        *pos += 1;
        let value = parse_json_value(s, pos)?;
        let hk = HashableKey::Str(CompactString::from(key.py_to_string()));
        match &dict.payload {
            PyObjectPayload::Dict(map) => { map.write().insert(hk, value); }
            _ => unreachable!(),
        }
        skip_ws(s, pos);
        if *pos >= s.len() { break; }
        if s.as_bytes()[*pos] == b'}' { *pos += 1; return Ok(dict); }
        if s.as_bytes()[*pos] == b',' { *pos += 1; } else { break; }
    }
    Err(PyException::value_error("Invalid JSON object"))
}

// ── time module ──


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
        attrs.insert(CompactString::from("_fieldnames"), PyObject::list(
            fieldnames.iter().map(|n| PyObject::str_val(CompactString::from(n.as_str()))).collect()
        ));
        attrs.insert(CompactString::from("fieldnames"), PyObject::list(
            fieldnames.iter().map(|n| PyObject::str_val(CompactString::from(n.as_str()))).collect()
        ));
        attrs.insert(CompactString::from("_rows"), PyObject::list(vec![]));
    }
    Ok(inst)
}

// ── shutil module (basic) ──


pub fn create_base64_module() -> PyObjectRef {
    make_module("base64", vec![
        ("b64encode", make_builtin(base64_encode)),
        ("b64decode", make_builtin(base64_decode)),
        ("b16encode", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("b16encode requires data")); }
            let data = extract_bytes(&args[0])?;
            let hex: String = data.iter().map(|b| format!("{:02X}", b)).collect();
            Ok(PyObject::bytes(hex.into_bytes()))
        })),
        ("b16decode", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("b16decode requires data")); }
            let s = args[0].py_to_string();
            let upper = s.to_uppercase();
            let bytes: Vec<u8> = (0..upper.len())
                .step_by(2)
                .filter_map(|i| {
                    if i + 2 <= upper.len() {
                        u8::from_str_radix(&upper[i..i+2], 16).ok()
                    } else {
                        None
                    }
                })
                .collect();
            Ok(PyObject::bytes(bytes))
        })),
        ("b32encode", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("b32encode requires data")); }
            let data = extract_bytes(&args[0])?;
            const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
            let mut result = Vec::new();
            let chunks = data.chunks(5);
            for chunk in chunks {
                let mut buf = [0u8; 5];
                buf[..chunk.len()].copy_from_slice(chunk);
                let b = buf;
                // 5 bytes = 40 bits -> 8 base32 chars
                let indices = [
                    (b[0] >> 3) & 0x1F,
                    ((b[0] & 0x07) << 2) | ((b[1] >> 6) & 0x03),
                    (b[1] >> 1) & 0x1F,
                    ((b[1] & 0x01) << 4) | ((b[2] >> 4) & 0x0F),
                    ((b[2] & 0x0F) << 1) | ((b[3] >> 7) & 0x01),
                    (b[3] >> 2) & 0x1F,
                    ((b[3] & 0x03) << 3) | ((b[4] >> 5) & 0x07),
                    b[4] & 0x1F,
                ];
                let num_chars = match chunk.len() {
                    1 => 2, 2 => 4, 3 => 5, 4 => 7, 5 => 8, _ => 0,
                };
                let padding = 8 - num_chars;
                for i in 0..num_chars {
                    result.push(ALPHABET[indices[i] as usize]);
                }
                for _ in 0..padding {
                    result.push(b'=');
                }
            }
            Ok(PyObject::bytes(result))
        })),
        ("b32decode", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("b32decode requires data")); }
            let input_bytes = extract_bytes(&args[0])?;
            let input: Vec<u8> = input_bytes.iter().copied()
                .filter(|&b| b != b'\n' && b != b'\r')
                .collect();
            fn decode_b32(c: u8) -> u8 {
                match c {
                    b'A'..=b'Z' => c - b'A',
                    b'a'..=b'z' => c - b'a',
                    b'2'..=b'7' => c - b'2' + 26,
                    _ => 0,
                }
            }
            let mut result = Vec::new();
            for chunk in input.chunks(8) {
                let pad_count = chunk.iter().filter(|&&b| b == b'=').count();
                let mut vals = [0u8; 8];
                for (i, &b) in chunk.iter().enumerate() {
                    vals[i] = decode_b32(b);
                }
                let n = ((vals[0] as u64) << 35) | ((vals[1] as u64) << 30)
                    | ((vals[2] as u64) << 25) | ((vals[3] as u64) << 20)
                    | ((vals[4] as u64) << 15) | ((vals[5] as u64) << 10)
                    | ((vals[6] as u64) << 5) | (vals[7] as u64);
                let out_bytes = match pad_count {
                    6 => 1, 4 => 2, 3 => 3, 1 => 4, 0 => 5, _ => 0,
                };
                if out_bytes >= 1 { result.push((n >> 32) as u8); }
                if out_bytes >= 2 { result.push((n >> 24) as u8); }
                if out_bytes >= 3 { result.push((n >> 16) as u8); }
                if out_bytes >= 4 { result.push((n >> 8) as u8); }
                if out_bytes >= 5 { result.push(n as u8); }
            }
            Ok(PyObject::bytes(result))
        })),
        ("urlsafe_b64encode", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("urlsafe_b64encode requires data")); }
            let encoded = base64_encode(args)?;
            let bytes = extract_bytes(&encoded)?;
            let safe: Vec<u8> = bytes.iter().map(|&b| match b {
                b'+' => b'-',
                b'/' => b'_',
                _ => b,
            }).collect();
            Ok(PyObject::bytes(safe))
        })),
        ("urlsafe_b64decode", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("urlsafe_b64decode requires data")); }
            let input_bytes = extract_bytes(&args[0])?;
            let standard: Vec<u8> = input_bytes.iter().map(|&b| match b {
                b'-' => b'+',
                b'_' => b'/',
                _ => b,
            }).collect();
            let standard_obj = PyObject::bytes(standard);
            base64_decode(&[standard_obj])
        })),
        ("encodebytes", make_builtin(base64_encode)),
        ("decodebytes", make_builtin(base64_decode)),
    ])
}

pub(crate) fn extract_bytes(obj: &PyObjectRef) -> PyResult<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Ok(b.clone()),
        PyObjectPayload::Str(s) => Ok(s.as_bytes().to_vec()),
        _ => Err(PyException::type_error("expected bytes-like object")),
    }
}

fn base64_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("b64encode requires data")); }
    let data = extract_bytes(&args[0])?;
    // Simple base64 encoding
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = Vec::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((n >> 18) & 63) as usize]);
        result.push(CHARS[((n >> 12) & 63) as usize]);
        if chunk.len() > 1 { result.push(CHARS[((n >> 6) & 63) as usize]); } else { result.push(b'='); }
        if chunk.len() > 2 { result.push(CHARS[(n & 63) as usize]); } else { result.push(b'='); }
    }
    Ok(PyObject::bytes(result))
}

fn base64_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("b64decode requires data")); }
    let input_bytes = extract_bytes(&args[0])?;
    let input: Vec<u8> = input_bytes.iter().copied().filter(|&b| b != b'\n' && b != b'\r').collect();
    fn decode_char(c: u8) -> u32 {
        match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' => 62,
            b'/' => 63,
            _ => 0,
        }
    }
    let mut result = Vec::new();
    for chunk in input.chunks(4) {
        if chunk.len() < 4 { break; }
        let n = (decode_char(chunk[0]) << 18) | (decode_char(chunk[1]) << 12) | (decode_char(chunk[2]) << 6) | decode_char(chunk[3]);
        result.push((n >> 16) as u8);
        if chunk[2] != b'=' { result.push((n >> 8) as u8); }
        if chunk[3] != b'=' { result.push(n as u8); }
    }
    Ok(PyObject::bytes(result))
}

// ── pprint module ──


pub fn create_struct_module() -> PyObjectRef {
    make_module("struct", vec![
        ("pack", make_builtin(struct_pack)),
        ("unpack", make_builtin(struct_unpack)),
        ("calcsize", make_builtin(struct_calcsize)),
        ("Struct", make_builtin(struct_struct_ctor)),
    ])
}

fn struct_struct_ctor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("Struct() requires a format string"));
    }
    let fmt_str = args[0].py_to_string();
    let cls = PyObject::class(CompactString::from("Struct"), vec![], indexmap::IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("format"), PyObject::str_val(CompactString::from(&fmt_str)));
        // Compute size
        let size_obj = struct_calcsize(&[PyObject::str_val(CompactString::from(&fmt_str))])?;
        w.insert(CompactString::from("size"), size_obj);
        let fmt_for_pack = fmt_str.clone();
        w.insert(CompactString::from("pack"), PyObject::native_closure("pack", move |args| {
            let mut full_args = vec![PyObject::str_val(CompactString::from(&fmt_for_pack))];
            full_args.extend_from_slice(args);
            struct_pack(&full_args)
        }));
        let fmt_for_unpack = fmt_str;
        w.insert(CompactString::from("unpack"), PyObject::native_closure("unpack", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("Struct.unpack() requires a buffer"));
            }
            struct_unpack(&[PyObject::str_val(CompactString::from(&fmt_for_unpack)), args[0].clone()])
        }));
    }
    Ok(inst)
}

fn struct_calcsize(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("calcsize requires format string")); }
    let fmt = args[0].py_to_string();
    let mut size = 0usize;
    let mut chars = fmt.chars().peekable();
    // Skip byte order
    if let Some(&c) = chars.peek() {
        if "<>!=@".contains(c) { chars.next(); }
    }
    while let Some(c) = chars.next() {
        let count = if c.is_ascii_digit() {
            let mut n = (c as u8 - b'0') as usize;
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() { n = n * 10 + (d as u8 - b'0') as usize; chars.next(); } else { break; }
            }
            let fc = chars.next().unwrap_or('x');
            size += n * format_char_size(fc);
            continue;
        } else { 1 };
        size += count * format_char_size(c);
    }
    Ok(PyObject::int(size as i64))
}

fn format_char_size(c: char) -> usize {
    match c {
        'x' | 'c' | 'b' | 'B' | '?' => 1,
        'h' | 'H' => 2,
        'i' | 'I' | 'l' | 'L' | 'f' => 4,
        'q' | 'Q' | 'd' => 8,
        'n' | 'N' | 'P' => std::mem::size_of::<usize>(),
        's' | 'p' => 1,
        _ => 0,
    }
}

fn struct_pack(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("pack requires format string")); }
    let fmt = args[0].py_to_string();
    let mut result = Vec::new();
    let mut arg_idx = 1;
    let mut chars = fmt.chars().peekable();
    let little_endian = match chars.peek() {
        Some('<') => { chars.next(); true }
        Some('>') | Some('!') => { chars.next(); false }
        Some('=') | Some('@') => { chars.next(); cfg!(target_endian = "little") }
        _ => cfg!(target_endian = "little"),
    };
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() { continue; } // count handling simplified
        match c {
            'b' | 'B' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[arg_idx].to_int()? as u8;
                result.push(val);
                arg_idx += 1;
            }
            'h' | 'H' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[arg_idx].to_int()? as u16;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                arg_idx += 1;
            }
            'i' | 'I' | 'l' | 'L' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[arg_idx].to_int()? as u32;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                arg_idx += 1;
            }
            'q' | 'Q' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[arg_idx].to_int()? as u64;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                arg_idx += 1;
            }
            'f' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[arg_idx].to_float()? as f32;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                arg_idx += 1;
            }
            'd' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                let val = args[arg_idx].to_float()?;
                let bytes = if little_endian { val.to_le_bytes() } else { val.to_be_bytes() };
                result.extend_from_slice(&bytes);
                arg_idx += 1;
            }
            '?' => {
                if arg_idx >= args.len() { return Err(PyException::type_error("not enough args")); }
                result.push(if args[arg_idx].is_truthy() { 1 } else { 0 });
                arg_idx += 1;
            }
            'x' => result.push(0),
            _ => {}
        }
    }
    Ok(PyObject::bytes(result))
}

fn struct_unpack(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("unpack requires format string and bytes")); }
    let fmt = args[0].py_to_string();
    let data = match &args[1].payload {
        PyObjectPayload::Bytes(b) => b.clone(),
        _ => return Err(PyException::type_error("unpack requires bytes argument")),
    };
    let mut result = Vec::new();
    let mut offset = 0;
    let mut chars = fmt.chars().peekable();
    let little_endian = match chars.peek() {
        Some('<') => { chars.next(); true }
        Some('>') | Some('!') => { chars.next(); false }
        Some('=') | Some('@') => { chars.next(); cfg!(target_endian = "little") }
        _ => cfg!(target_endian = "little"),
    };
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() { continue; }
        match c {
            'b' => {
                if offset >= data.len() { break; }
                result.push(PyObject::int(data[offset] as i8 as i64));
                offset += 1;
            }
            'B' => {
                if offset >= data.len() { break; }
                result.push(PyObject::int(data[offset] as i64));
                offset += 1;
            }
            'h' => {
                if offset + 2 > data.len() { break; }
                let bytes: [u8; 2] = [data[offset], data[offset + 1]];
                let val = if little_endian { i16::from_le_bytes(bytes) } else { i16::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                offset += 2;
            }
            'H' => {
                if offset + 2 > data.len() { break; }
                let bytes: [u8; 2] = [data[offset], data[offset + 1]];
                let val = if little_endian { u16::from_le_bytes(bytes) } else { u16::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                offset += 2;
            }
            'i' | 'l' => {
                if offset + 4 > data.len() { break; }
                let bytes: [u8; 4] = [data[offset], data[offset+1], data[offset+2], data[offset+3]];
                let val = if little_endian { i32::from_le_bytes(bytes) } else { i32::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                offset += 4;
            }
            'I' | 'L' => {
                if offset + 4 > data.len() { break; }
                let bytes: [u8; 4] = [data[offset], data[offset+1], data[offset+2], data[offset+3]];
                let val = if little_endian { u32::from_le_bytes(bytes) } else { u32::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                offset += 4;
            }
            'q' => {
                if offset + 8 > data.len() { break; }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[offset..offset+8]);
                let val = if little_endian { i64::from_le_bytes(bytes) } else { i64::from_be_bytes(bytes) };
                result.push(PyObject::int(val));
                offset += 8;
            }
            'Q' => {
                if offset + 8 > data.len() { break; }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[offset..offset+8]);
                let val = if little_endian { u64::from_le_bytes(bytes) } else { u64::from_be_bytes(bytes) };
                result.push(PyObject::int(val as i64));
                offset += 8;
            }
            'f' => {
                if offset + 4 > data.len() { break; }
                let bytes: [u8; 4] = [data[offset], data[offset+1], data[offset+2], data[offset+3]];
                let val = if little_endian { f32::from_le_bytes(bytes) } else { f32::from_be_bytes(bytes) };
                result.push(PyObject::float(val as f64));
                offset += 4;
            }
            'd' => {
                if offset + 8 > data.len() { break; }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[offset..offset+8]);
                let val = if little_endian { f64::from_le_bytes(bytes) } else { f64::from_be_bytes(bytes) };
                result.push(PyObject::float(val));
                offset += 8;
            }
            '?' => {
                if offset >= data.len() { break; }
                result.push(PyObject::bool_val(data[offset] != 0));
                offset += 1;
            }
            'x' => { offset += 1; }
            _ => {}
        }
    }
    Ok(PyObject::tuple(result))
}

// ── pickle module ──

// Marker bytes for our simplified pickle format
const PICKLE_NONE: u8 = b'N';
const PICKLE_TRUE: u8 = b'T';
const PICKLE_FALSE: u8 = b'F';
const PICKLE_INT: u8 = b'I';
const PICKLE_FLOAT: u8 = b'D';
const PICKLE_STR: u8 = b'S';
const PICKLE_BYTES: u8 = b'B';
const PICKLE_LIST: u8 = b'L';
const PICKLE_TUPLE: u8 = b'U';
const PICKLE_DICT: u8 = b'd';
const PICKLE_STOP: u8 = b'.';

pub fn create_pickle_module() -> PyObjectRef {
    make_module("pickle", vec![
        ("dumps", make_builtin(pickle_dumps)),
        ("loads", make_builtin(pickle_loads)),
        ("dump", make_builtin(pickle_dump)),
        ("load", make_builtin(pickle_load)),
        ("HIGHEST_PROTOCOL", PyObject::int(5)),
        ("DEFAULT_PROTOCOL", PyObject::int(4)),
        ("PicklingError", PyObject::str_val(CompactString::from("PicklingError"))),
        ("UnpicklingError", PyObject::str_val(CompactString::from("UnpicklingError"))),
    ])
}

fn pickle_serialize(obj: &PyObjectRef, buf: &mut Vec<u8>) -> PyResult<()> {
    match &obj.payload {
        PyObjectPayload::None => buf.push(PICKLE_NONE),
        PyObjectPayload::Bool(b) => buf.push(if *b { PICKLE_TRUE } else { PICKLE_FALSE }),
        PyObjectPayload::Int(n) => {
            buf.push(PICKLE_INT);
            let s = n.to_string();
            let bytes = s.as_bytes();
            let len = bytes.len() as u32;
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(bytes);
        }
        PyObjectPayload::Float(f) => {
            buf.push(PICKLE_FLOAT);
            buf.extend_from_slice(&f.to_le_bytes());
        }
        PyObjectPayload::Str(s) => {
            buf.push(PICKLE_STR);
            let bytes = s.as_bytes();
            let len = bytes.len() as u32;
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(bytes);
        }
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
            buf.push(PICKLE_BYTES);
            let len = b.len() as u32;
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(b);
        }
        PyObjectPayload::List(items) => {
            let items = items.read();
            let count = items.len() as u32;
            for item in items.iter() {
                pickle_serialize(item, buf)?;
            }
            buf.push(PICKLE_LIST);
            buf.extend_from_slice(&count.to_le_bytes());
        }
        PyObjectPayload::Tuple(items) => {
            let count = items.len() as u32;
            for item in items.iter() {
                pickle_serialize(item, buf)?;
            }
            buf.push(PICKLE_TUPLE);
            buf.extend_from_slice(&count.to_le_bytes());
        }
        PyObjectPayload::Dict(map) => {
            let map = map.read();
            let count = map.len() as u32;
            for (k, v) in map.iter() {
                let key_obj = match k {
                    HashableKey::Str(s) => PyObject::str_val(s.clone()),
                    HashableKey::Int(n) => PyObject::int(n.to_i64().unwrap_or(0)),
                    HashableKey::Float(f) => PyObject::float(f.0),
                    HashableKey::Bool(b) => PyObject::bool_val(*b),
                    _ => PyObject::str_val(CompactString::from(format!("{:?}", k))),
                };
                pickle_serialize(&key_obj, buf)?;
                pickle_serialize(v, buf)?;
            }
            buf.push(PICKLE_DICT);
            buf.extend_from_slice(&count.to_le_bytes());
        }
        _ => {
            return Err(PyException::runtime_error(
                format!("PicklingError: can't pickle object of type {}", obj.type_name()),
            ));
        }
    }
    Ok(())
}

fn pickle_dumps(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "pickle.dumps() missing 1 required positional argument: 'obj'",
        ));
    }
    // args[0] = obj, args[1] = protocol (ignored)
    let mut buf = Vec::new();
    // Protocol header
    buf.push(0x80);
    buf.push(4); // protocol version
    pickle_serialize(&args[0], &mut buf)?;
    buf.push(PICKLE_STOP);
    Ok(PyObject::bytes(buf))
}

fn pickle_deserialize(data: &[u8], pos: &mut usize) -> PyResult<PyObjectRef> {
    if *pos >= data.len() {
        return Err(PyException::runtime_error("UnpicklingError: unexpected end of data"));
    }
    let marker = data[*pos];
    *pos += 1;
    match marker {
        PICKLE_NONE => Ok(PyObject::none()),
        PICKLE_TRUE => Ok(PyObject::bool_val(true)),
        PICKLE_FALSE => Ok(PyObject::bool_val(false)),
        PICKLE_INT => {
            if *pos + 4 > data.len() {
                return Err(PyException::runtime_error("UnpicklingError: truncated int"));
            }
            let len = u32::from_le_bytes([data[*pos], data[*pos+1], data[*pos+2], data[*pos+3]]) as usize;
            *pos += 4;
            if *pos + len > data.len() {
                return Err(PyException::runtime_error("UnpicklingError: truncated int data"));
            }
            let s = std::str::from_utf8(&data[*pos..*pos+len])
                .map_err(|_| PyException::runtime_error("UnpicklingError: invalid int encoding"))?;
            *pos += len;
            let val: i64 = s.parse()
                .map_err(|_| PyException::runtime_error("UnpicklingError: invalid int value"))?;
            Ok(PyObject::int(val))
        }
        PICKLE_FLOAT => {
            if *pos + 8 > data.len() {
                return Err(PyException::runtime_error("UnpicklingError: truncated float"));
            }
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&data[*pos..*pos+8]);
            *pos += 8;
            Ok(PyObject::float(f64::from_le_bytes(bytes)))
        }
        PICKLE_STR => {
            if *pos + 4 > data.len() {
                return Err(PyException::runtime_error("UnpicklingError: truncated str length"));
            }
            let len = u32::from_le_bytes([data[*pos], data[*pos+1], data[*pos+2], data[*pos+3]]) as usize;
            *pos += 4;
            if *pos + len > data.len() {
                return Err(PyException::runtime_error("UnpicklingError: truncated str data"));
            }
            let s = std::str::from_utf8(&data[*pos..*pos+len])
                .map_err(|_| PyException::runtime_error("UnpicklingError: invalid utf-8 in str"))?;
            *pos += len;
            Ok(PyObject::str_val(CompactString::from(s)))
        }
        PICKLE_BYTES => {
            if *pos + 4 > data.len() {
                return Err(PyException::runtime_error("UnpicklingError: truncated bytes length"));
            }
            let len = u32::from_le_bytes([data[*pos], data[*pos+1], data[*pos+2], data[*pos+3]]) as usize;
            *pos += 4;
            if *pos + len > data.len() {
                return Err(PyException::runtime_error("UnpicklingError: truncated bytes data"));
            }
            let b = data[*pos..*pos+len].to_vec();
            *pos += len;
            Ok(PyObject::bytes(b))
        }
        PICKLE_LIST => {
            if *pos + 4 > data.len() {
                return Err(PyException::runtime_error("UnpicklingError: truncated list count"));
            }
            let count = u32::from_le_bytes([data[*pos], data[*pos+1], data[*pos+2], data[*pos+3]]) as usize;
            *pos += 4;
            // Elements were serialized before the marker, so we need to
            // re-parse from the start. Use a stack-based approach instead.
            // This branch is reached after elements are already on the stack,
            // but since we parse linearly, we handle it by collecting from
            // recursive calls.
            // Actually, the format serializes children first then marker.
            // So we need a stack-based parser.
            // Let's stash count for now and note that the caller should use
            // the stack-based pickle_loads_stack.
            // This won't be reached — we use stack-based deserialization.
            let _ = count;
            Err(PyException::runtime_error("UnpicklingError: internal error"))
        }
        _ => Err(PyException::runtime_error(
            format!("UnpicklingError: unknown marker byte 0x{:02x}", marker),
        )),
    }
}

fn pickle_loads_stack(data: &[u8]) -> PyResult<PyObjectRef> {
    let mut pos = 0;
    // Skip protocol header if present
    if pos < data.len() && data[pos] == 0x80 {
        pos += 2; // skip 0x80 + version byte
    }

    let mut stack: Vec<PyObjectRef> = Vec::new();

    while pos < data.len() {
        let marker = data[pos];
        pos += 1;
        match marker {
            PICKLE_STOP => break,
            PICKLE_NONE => stack.push(PyObject::none()),
            PICKLE_TRUE => stack.push(PyObject::bool_val(true)),
            PICKLE_FALSE => stack.push(PyObject::bool_val(false)),
            PICKLE_INT => {
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated int"));
                }
                let len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated int data"));
                }
                let s = std::str::from_utf8(&data[pos..pos+len])
                    .map_err(|_| PyException::runtime_error("UnpicklingError: invalid int encoding"))?;
                pos += len;
                let val: i64 = s.parse()
                    .map_err(|_| PyException::runtime_error("UnpicklingError: invalid int value"))?;
                stack.push(PyObject::int(val));
            }
            PICKLE_FLOAT => {
                if pos + 8 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated float"));
                }
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&data[pos..pos+8]);
                pos += 8;
                stack.push(PyObject::float(f64::from_le_bytes(bytes)));
            }
            PICKLE_STR => {
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated str"));
                }
                let len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated str data"));
                }
                let s = std::str::from_utf8(&data[pos..pos+len])
                    .map_err(|_| PyException::runtime_error("UnpicklingError: invalid utf-8"))?;
                pos += len;
                stack.push(PyObject::str_val(CompactString::from(s)));
            }
            PICKLE_BYTES => {
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated bytes"));
                }
                let len = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                pos += 4;
                if pos + len > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated bytes data"));
                }
                let b = data[pos..pos+len].to_vec();
                pos += len;
                stack.push(PyObject::bytes(b));
            }
            PICKLE_LIST => {
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated list count"));
                }
                let count = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                pos += 4;
                if stack.len() < count {
                    return Err(PyException::runtime_error("UnpicklingError: stack underflow for list"));
                }
                let items = stack.split_off(stack.len() - count);
                stack.push(PyObject::list(items));
            }
            PICKLE_TUPLE => {
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated tuple count"));
                }
                let count = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                pos += 4;
                if stack.len() < count {
                    return Err(PyException::runtime_error("UnpicklingError: stack underflow for tuple"));
                }
                let items = stack.split_off(stack.len() - count);
                stack.push(PyObject::tuple(items));
            }
            PICKLE_DICT => {
                if pos + 4 > data.len() {
                    return Err(PyException::runtime_error("UnpicklingError: truncated dict count"));
                }
                let count = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]) as usize;
                pos += 4;
                let pair_count = count * 2;
                if stack.len() < pair_count {
                    return Err(PyException::runtime_error("UnpicklingError: stack underflow for dict"));
                }
                let kv_items = stack.split_off(stack.len() - pair_count);
                let mut pairs = Vec::new();
                for chunk in kv_items.chunks_exact(2) {
                    pairs.push((chunk[0].clone(), chunk[1].clone()));
                }
                stack.push(PyObject::dict_from_pairs(pairs));
            }
            _ => {
                return Err(PyException::runtime_error(
                    format!("UnpicklingError: unknown marker byte 0x{:02x}", marker),
                ));
            }
        }
    }

    stack.pop().ok_or_else(|| PyException::runtime_error("UnpicklingError: empty pickle data"))
}

fn pickle_loads(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "pickle.loads() missing 1 required positional argument: 'data'",
        ));
    }
    let data = extract_bytes(&args[0])?;
    pickle_loads_stack(&data)
}

fn pickle_dump(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "pickle.dump() missing required arguments: 'obj' and 'file'",
        ));
    }
    let data = pickle_dumps(&args[0..1])?;
    let data_bytes = extract_bytes(&data)?;
    // Write to file — expects a file-like object with a write() method
    if let Some(write_fn) = args[1].get_attr("write") {
        let _ = write_fn; // stub: actual call dispatch not available without VM
    }
    // Fallback: if the file arg is a string path, write directly
    if let PyObjectPayload::Str(path) = &args[1].payload {
        std::fs::write(path.as_str(), &data_bytes)
            .map_err(|e| PyException::runtime_error(format!("pickle.dump: {}", e)))?;
    }
    Ok(PyObject::none())
}

fn pickle_load(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "pickle.load() missing 1 required positional argument: 'file'",
        ));
    }
    // If file arg is a string path, read directly
    if let PyObjectPayload::Str(path) = &args[0].payload {
        let data = std::fs::read(path.as_str())
            .map_err(|e| PyException::runtime_error(format!("pickle.load: {}", e)))?;
        return pickle_loads_stack(&data);
    }
    Err(PyException::runtime_error(
        "pickle.load: expected a file path or file-like object",
    ))
}

// ── textwrap module ──


// ── binascii module ──

pub fn create_binascii_module() -> PyObjectRef {
    let hexlify_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args("hexlify", args, 1)?;
        let data = extract_bytes(&args[0])?;
        let hex: String = data.iter().map(|b| format!("{:02x}", b)).collect();
        Ok(PyObject::bytes(hex.into_bytes()))
    });

    let unhexlify_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args("unhexlify", args, 1)?;
        let hex_str = match &args[0].payload {
            PyObjectPayload::Bytes(b) => String::from_utf8_lossy(b).to_string(),
            PyObjectPayload::Str(s) => s.to_string(),
            _ => args[0].py_to_string(),
        };
        let hex_str = hex_str.trim();
        if hex_str.len() % 2 != 0 {
            return Err(PyException::value_error("Odd-length string"));
        }
        let mut result = Vec::with_capacity(hex_str.len() / 2);
        for i in (0..hex_str.len()).step_by(2) {
            let byte = u8::from_str_radix(&hex_str[i..i+2], 16)
                .map_err(|_| PyException::value_error("Non-hexadecimal digit found"))?;
            result.push(byte);
        }
        Ok(PyObject::bytes(result))
    });

    let crc32_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("crc32", args, 1)?;
        let data = extract_bytes(&args[0])?;
        let mut crc: u32 = if args.len() > 1 {
            match &args[1].payload {
                PyObjectPayload::Int(n) => n.to_i64().unwrap_or(0) as u32,
                _ => 0,
            }
        } else { 0 };
        crc = !crc;
        for &byte in &data {
            crc ^= byte as u32;
            for _ in 0..8 {
                if crc & 1 != 0 { crc = (crc >> 1) ^ 0xEDB88320; }
                else { crc >>= 1; }
            }
        }
        Ok(PyObject::int(!crc as i64))
    });

    let b2a_base64_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("b2a_base64", args, 1)?;
        let data = extract_bytes(&args[0])?;
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut result = Vec::new();
        for chunk in data.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
            let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
            let n = (b0 << 16) | (b1 << 8) | b2;
            result.push(CHARS[((n >> 18) & 63) as usize]);
            result.push(CHARS[((n >> 12) & 63) as usize]);
            if chunk.len() > 1 { result.push(CHARS[((n >> 6) & 63) as usize]); } else { result.push(b'='); }
            if chunk.len() > 2 { result.push(CHARS[(n & 63) as usize]); } else { result.push(b'='); }
        }
        result.push(b'\n');
        Ok(PyObject::bytes(result))
    });

    let a2b_base64_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args("a2b_base64", args, 1)?;
        let input_str = match &args[0].payload {
            PyObjectPayload::Bytes(b) => String::from_utf8_lossy(b).to_string(),
            _ => args[0].py_to_string(),
        };
        let input: Vec<u8> = input_str.bytes().filter(|&b| b != b'\n' && b != b'\r').collect();
        fn decode_char(c: u8) -> u32 {
            match c {
                b'A'..=b'Z' => (c - b'A') as u32, b'a'..=b'z' => (c - b'a' + 26) as u32,
                b'0'..=b'9' => (c - b'0' + 52) as u32, b'+' => 62, b'/' => 63, _ => 0,
            }
        }
        let mut result = Vec::new();
        for chunk in input.chunks(4) {
            if chunk.len() < 4 { break; }
            let n = (decode_char(chunk[0]) << 18) | (decode_char(chunk[1]) << 12) | (decode_char(chunk[2]) << 6) | decode_char(chunk[3]);
            result.push((n >> 16) as u8);
            if chunk[2] != b'=' { result.push((n >> 8) as u8); }
            if chunk[3] != b'=' { result.push(n as u8); }
        }
        Ok(PyObject::bytes(result))
    });

    make_module("binascii", vec![
        ("hexlify", hexlify_fn),
        ("b2a_hex", make_builtin(|args: &[PyObjectRef]| {
            let data = extract_bytes(&args[0])?;
            Ok(PyObject::bytes(data.iter().map(|b| format!("{:02x}", b)).collect::<String>().into_bytes()))
        })),
        ("unhexlify", unhexlify_fn),
        ("a2b_hex", make_builtin(|args: &[PyObjectRef]| {
            let hex_str = match &args[0].payload {
                PyObjectPayload::Bytes(b) => String::from_utf8_lossy(b).to_string(),
                _ => args[0].py_to_string(),
            };
            let hex_str = hex_str.trim();
            if hex_str.len() % 2 != 0 { return Err(PyException::value_error("Odd-length string")); }
            let mut result = Vec::with_capacity(hex_str.len() / 2);
            for i in (0..hex_str.len()).step_by(2) {
                result.push(u8::from_str_radix(&hex_str[i..i+2], 16)
                    .map_err(|_| PyException::value_error("Non-hexadecimal digit found"))?);
            }
            Ok(PyObject::bytes(result))
        })),
        ("crc32", crc32_fn),
        ("b2a_base64", b2a_base64_fn),
        ("a2b_base64", a2b_base64_fn),
    ])
}
