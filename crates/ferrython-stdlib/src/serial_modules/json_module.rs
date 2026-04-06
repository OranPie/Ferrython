use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin,
};
use ferrython_core::types::HashableKey;
use std::cell::Cell;

thread_local! {
    static ENSURE_ASCII: Cell<bool> = const { Cell::new(true) };
}

/// Escape a string for JSON, respecting the ensure_ascii setting.
fn json_escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if !c.is_ascii() && ENSURE_ASCII.with(|f| f.get()) => {
                // Escape non-ASCII to \uXXXX (or surrogate pairs for > U+FFFF)
                let cp = c as u32;
                if cp <= 0xFFFF {
                    out.push_str(&format!("\\u{:04x}", cp));
                } else {
                    // Surrogate pair for supplementary characters
                    let cp = cp - 0x10000;
                    let hi = 0xD800 + (cp >> 10);
                    let lo = 0xDC00 + (cp & 0x3FF);
                    out.push_str(&format!("\\u{:04x}\\u{:04x}", hi, lo));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn create_json_module() -> PyObjectRef {
    make_module("json", vec![
        ("dumps", PyObject::native_function("json.dumps", json_dumps)),
        ("loads", make_builtin(json_loads)),
        ("dump", PyObject::native_function("json.dump", json_dump)),
        ("load", make_builtin(json_load)),
        ("JSONEncoder", make_builtin(json_encoder_ctor)),
        ("JSONDecoder", make_builtin(json_decoder_ctor)),
        ("JSONDecodeError", PyObject::class(
            CompactString::from("JSONDecodeError"),
            vec![],
            indexmap::IndexMap::new(),
        )),
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

pub fn json_dumps(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("json.dumps() missing 1 required positional argument: 'obj'"));
    }
    // kwargs may be passed as a trailing dict by the VM
    let mut indent: Option<usize> = None;
    let mut sort_keys = false;
    let mut item_sep = ", ".to_string();
    let mut kv_sep = ": ".to_string();
    let mut default_fn: Option<PyObjectRef> = None;
    let mut ensure_ascii = true;
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
            if let Some(ea) = r.get(&HashableKey::Str(CompactString::from("ensure_ascii"))) {
                ensure_ascii = ea.is_truthy();
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
            // cls=CustomEncoder: create an instance and bind its `default` method
            if default_fn.is_none() {
                if let Some(cls) = r.get(&HashableKey::Str(CompactString::from("cls"))) {
                    let encoder_inst = PyObject::instance(cls.clone());
                    if let Some(default_method) = cls.get_attr("default") {
                        default_fn = Some(PyObject::wrap(PyObjectPayload::BoundMethod {
                            receiver: encoder_inst,
                            method: default_method,
                        }));
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
    ENSURE_ASCII.with(|f| f.set(ensure_ascii));
    let s = if let Some(indent_size) = indent {
        py_to_json_pretty(&obj, 0, indent_size, default_fn.as_ref())?
    } else {
        py_to_json_sep(&obj, &item_sep, &kv_sep, default_fn.as_ref())?
    };
    ENSURE_ASCII.with(|f| f.set(true)); // restore default
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
        PyObjectPayload::Str(s) => Ok(json_escape_string(s)),
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
                    HashableKey::Str(s) => json_escape_string(s),
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
        PyObjectPayload::Str(s) => Ok(json_escape_string(s)),
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
                    HashableKey::Str(s) => json_escape_string(s),
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
                let key_str = json_escape_string(k);
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
/// 1. If a `default` callable is provided, call it and re-serialize the result
/// 2. Otherwise, raise TypeError (matching CPython behavior — unknown types are NOT auto-serialized)
fn json_serialize_fallback<F>(
    obj: &PyObjectRef,
    default: Option<&PyObjectRef>,
    recurse: F,
) -> PyResult<String>
where
    F: Fn(&PyObjectRef, Option<&PyObjectRef>) -> PyResult<String>,
{
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

/// Try to call a default callable (NativeFunction, NativeClosure, or BoundMethod)
fn try_call_default(default: &PyObjectRef, obj: &PyObjectRef) -> PyResult<Option<PyObjectRef>> {
    match &default.payload {
        PyObjectPayload::NativeFunction { func, .. } => Ok(Some(func(&[obj.clone()])?)),
        PyObjectPayload::NativeClosure { func, .. } => Ok(Some(func(&[obj.clone()])?)),
        PyObjectPayload::BoundMethod { receiver, method } => {
            // Call method(self, obj) — dispatch based on method type
            match &method.payload {
                PyObjectPayload::NativeFunction { func, .. } => Ok(Some(func(&[receiver.clone(), obj.clone()])?)),
                PyObjectPayload::NativeClosure { func, .. } => Ok(Some(func(&[receiver.clone(), obj.clone()])?)),
                PyObjectPayload::Function(_) => {
                    // Python function — we need the VM. Use request_vm_call.
                    ferrython_core::error::request_vm_call(method.clone(), vec![receiver.clone(), obj.clone()]);
                    Ok(None) // signal that we need VM callback
                }
                _ => Ok(None),
            }
        }
        PyObjectPayload::Function(_) => {
            ferrython_core::error::request_vm_call(default.clone(), vec![obj.clone()]);
            Ok(None)
        }
        _ => Ok(None),
    }
}

fn json_loads(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("json.loads requires a string argument"));
    }
    let s = match &args[0].payload {
        PyObjectPayload::Str(s) => s.to_string(),
        PyObjectPayload::Bytes(b) => String::from_utf8_lossy(b).to_string(),
        _ => return Err(PyException::type_error("json.loads requires a string")),
    };
    
    // Extract kwargs if present
    let kwargs = args.last().and_then(|a| {
        if let PyObjectPayload::Dict(d) = &a.payload { Some(d.read().clone()) } else { None }
    });
    let object_hook = kwargs.as_ref().and_then(|kw| {
        kw.get(&HashableKey::Str(CompactString::from("object_hook"))).cloned()
    }).filter(|v| !matches!(&v.payload, PyObjectPayload::None));
    let parse_float = kwargs.as_ref().and_then(|kw| {
        kw.get(&HashableKey::Str(CompactString::from("parse_float"))).cloned()
    }).filter(|v| !matches!(&v.payload, PyObjectPayload::None));
    let parse_int = kwargs.as_ref().and_then(|kw| {
        kw.get(&HashableKey::Str(CompactString::from("parse_int"))).cloned()
    }).filter(|v| !matches!(&v.payload, PyObjectPayload::None));
    
    let result = parse_json_value(&s, &mut 0)?;
    
    // Apply hooks if provided
    if object_hook.is_some() || parse_float.is_some() || parse_int.is_some() {
        apply_json_hooks(&result, &object_hook, &parse_float, &parse_int)
    } else {
        Ok(result)
    }
}

fn apply_json_hooks(
    value: &PyObjectRef,
    object_hook: &Option<PyObjectRef>,
    parse_float: &Option<PyObjectRef>,
    parse_int: &Option<PyObjectRef>,
) -> PyResult<PyObjectRef> {
    match &value.payload {
        PyObjectPayload::Dict(d) => {
            // Recursively apply hooks to values
            let rd = d.read();
            let mut new_map = indexmap::IndexMap::new();
            for (k, v) in rd.iter() {
                let new_v = apply_json_hooks(v, object_hook, parse_float, parse_int)?;
                new_map.insert(k.clone(), new_v);
            }
            let new_dict = PyObject::dict(new_map);
            // Apply object_hook to the dict
            if let Some(hook) = object_hook {
                try_call_default(hook, &new_dict).map(|r| r.unwrap_or(new_dict))
            } else {
                Ok(new_dict)
            }
        }
        PyObjectPayload::List(items) => {
            let r = items.read();
            let new_items: Vec<PyObjectRef> = r.iter()
                .map(|item| apply_json_hooks(item, object_hook, parse_float, parse_int))
                .collect::<PyResult<Vec<_>>>()?;
            Ok(PyObject::list(new_items))
        }
        PyObjectPayload::Float(_) => {
            if let Some(pf) = parse_float {
                let s = PyObject::str_val(CompactString::from(value.py_to_string()));
                try_call_default(pf, &s).map(|r| r.unwrap_or_else(|| value.clone()))
            } else {
                Ok(value.clone())
            }
        }
        PyObjectPayload::Int(_) => {
            if let Some(pi) = parse_int {
                let s = PyObject::str_val(CompactString::from(value.py_to_string()));
                try_call_default(pi, &s).map(|r| r.unwrap_or_else(|| value.clone()))
            } else {
                Ok(value.clone())
            }
        }
        _ => Ok(value.clone()),
    }
}

/// json.dump(obj, fp, **kwargs) — serialize obj as JSON and write to fp.write()
fn json_dump(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "json.dump() missing required arguments: 'obj' and 'fp'",
        ));
    }
    // Reuse json_dumps for serialization: pass obj + remaining kwargs
    let mut dump_args = vec![args[0].clone()];
    if args.len() > 2 {
        dump_args.push(args[2].clone());
    }
    let json_str = json_dumps(&dump_args)?;
    // Call fp.write(json_str)
    let fp = &args[1];
    if let Some(write_fn) = fp.get_attr("write") {
        match &write_fn.payload {
            PyObjectPayload::NativeFunction { func, .. } => { func(&[fp.clone(), json_str])?; }
            PyObjectPayload::NativeClosure { func, .. } => { func(&[json_str])?; }
            _ => {} // user-defined write — best-effort
        }
    }
    Ok(PyObject::none())
}

/// json.load(fp) — read JSON from fp.read() and deserialize
fn json_load(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "json.load() missing required argument: 'fp'",
        ));
    }
    let fp = &args[0];
    if let Some(read_fn) = fp.get_attr("read") {
        let data = match &read_fn.payload {
            PyObjectPayload::NativeFunction { func, .. } => func(&[fp.clone()])?,
            PyObjectPayload::NativeClosure { func, .. } => func(&[])?,
            _ => return Err(PyException::type_error("fp.read() is not callable")),
        };
        let s = match &data.payload {
            PyObjectPayload::Str(s) => s.to_string(),
            _ => return Err(PyException::type_error("fp.read() must return a string")),
        };
        return parse_json_value(&s, &mut 0);
    }
    Err(PyException::attribute_error("'fp' object has no attribute 'read'"))
}

fn parse_json_value(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    skip_ws(s, pos);
    if *pos >= s.len() { return Err(PyException::json_decode_error("Unexpected end of JSON")); }
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
                'b' => result.push('\u{0008}'),
                'f' => result.push('\u{000C}'),
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                '/' => result.push('/'),
                'u' => {
                    // Parse \uXXXX unicode escape (and surrogate pairs)
                    if *pos + 4 >= s.len() {
                        return Err(PyException::json_decode_error("Incomplete \\uXXXX escape"));
                    }
                    let hex = &s[*pos + 1..*pos + 5];
                    let cp = u32::from_str_radix(hex, 16).map_err(|_|
                        PyException::json_decode_error("Invalid \\uXXXX escape"))?;
                    *pos += 4; // skip 4 hex digits (loop will +1 more)
                    // Handle UTF-16 surrogate pairs: \uD800-\uDBFF followed by \uDC00-\uDFFF
                    if (0xD800..=0xDBFF).contains(&cp) {
                        // High surrogate — expect \uDCxx low surrogate
                        if *pos + 6 < s.len() && s.as_bytes()[*pos + 1] == b'\\' && s.as_bytes()[*pos + 2] == b'u' {
                            let lo_hex = &s[*pos + 3..*pos + 7];
                            if let Ok(lo) = u32::from_str_radix(lo_hex, 16) {
                                if (0xDC00..=0xDFFF).contains(&lo) {
                                    let combined = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                    if let Some(c) = char::from_u32(combined) {
                                        result.push(c);
                                    }
                                    *pos += 6; // skip \uXXXX of low surrogate
                                } else {
                                    result.push(char::REPLACEMENT_CHARACTER);
                                }
                            } else {
                                result.push(char::REPLACEMENT_CHARACTER);
                            }
                        } else {
                            result.push(char::REPLACEMENT_CHARACTER);
                        }
                    } else if let Some(c) = char::from_u32(cp) {
                        result.push(c);
                    } else {
                        result.push(char::REPLACEMENT_CHARACTER);
                    }
                }
                _ => { result.push('\\'); result.push(esc); }
            }
        } else {
            result.push(ch);
        }
        *pos += 1;
    }
    Err(PyException::json_decode_error("Unterminated string"))
}

fn parse_json_bool(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    if s[*pos..].starts_with("true") { *pos += 4; return Ok(PyObject::bool_val(true)); }
    if s[*pos..].starts_with("false") { *pos += 5; return Ok(PyObject::bool_val(false)); }
    Err(PyException::json_decode_error("Invalid JSON"))
}

fn parse_json_null(s: &str, pos: &mut usize) -> PyResult<PyObjectRef> {
    if s[*pos..].starts_with("null") { *pos += 4; return Ok(PyObject::none()); }
    Err(PyException::json_decode_error("Invalid JSON"))
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
        let f: f64 = num_str.parse().map_err(|_| PyException::json_decode_error("Invalid number"))?;
        Ok(PyObject::float(f))
    } else {
        let i: i64 = num_str.parse().map_err(|_| PyException::json_decode_error("Invalid number"))?;
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
    Err(PyException::json_decode_error("Invalid JSON array"))
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
        if *pos >= s.len() || s.as_bytes()[*pos] != b':' { return Err(PyException::json_decode_error("Expected ':'")); }
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
    Err(PyException::json_decode_error("Invalid JSON object"))
}
