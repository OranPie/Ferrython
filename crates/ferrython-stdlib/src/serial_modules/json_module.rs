use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin, check_args,
};
use ferrython_core::types::HashableKey;

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
            let parts: Result<Vec<String>, PyException> = r.iter().map(|i| py_to_json_pretty(i, depth + 1, indent, default)).collect();
            Ok(format!("[\n{}{}\n{}]", pad, parts?.join(&format!(",\n{}", pad)), pad_close))
        }
        PyObjectPayload::Tuple(items) => {
            if items.is_empty() { return Ok("[]".into()); }
            let parts: Result<Vec<String>, PyException> = items.iter().map(|i| py_to_json_pretty(i, depth + 1, indent, default)).collect();
            Ok(format!("[\n{}{}\n{}]", pad, parts?.join(&format!(",\n{}", pad)), pad_close))
        }
        PyObjectPayload::Dict(map) => {
            let r = map.read();
            if r.is_empty() { return Ok("{}".into()); }
            let parts: Result<Vec<String>, PyException> = r.iter().map(|(k, v)| {
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
            let parts: Result<Vec<String>, PyException> = r.iter().map(|i| py_to_json_sep(i, item_sep, kv_sep, default)).collect();
            Ok(format!("[{}]", parts?.join(item_sep)))
        }
        PyObjectPayload::Tuple(items) => {
            let parts: Result<Vec<String>, PyException> = items.iter().map(|i| py_to_json_sep(i, item_sep, kv_sep, default)).collect();
            Ok(format!("[{}]", parts?.join(item_sep)))
        }
        PyObjectPayload::Dict(map) => {
            let r = map.read();
            let parts: Result<Vec<String>, PyException> = r.iter().map(|(k, v)| {
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
            let parts: Result<Vec<String>, PyException> = r.iter().map(|(k, v)| {
                let key_str = format!("\"{}\"", k);
                let val_str = py_to_json_sep(v, item_sep, kv_sep, default)?;
                Ok(format!("{}{}{}", key_str, kv_sep, val_str))
            }).collect();
            Ok(format!("{{{}}}", parts?.join(item_sep)))
        }
        PyObjectPayload::Set(map) => {
            // Sets serialize as JSON arrays (common pattern used by custom encoders)
            let r = map.read();
            let items: Vec<PyObjectRef> = r.keys().map(|k| k.to_object()).collect();
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
            let parts: Result<Vec<String>, PyException> = public_attrs.iter().map(|(k, v)| {
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
