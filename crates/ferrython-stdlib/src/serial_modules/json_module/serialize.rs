use super::*;
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

pub fn json_dumps(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "json.dumps() missing 1 required positional argument: 'obj'",
        ));
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
            if let Some(ind) = r.get(&HashableKey::str_key(CompactString::from("indent"))) {
                indent = match &ind.payload {
                    PyObjectPayload::Int(n) => Some(n.to_i64().unwrap_or(2) as usize),
                    PyObjectPayload::None => None,
                    _ => None,
                };
            }
            if let Some(sk) = r.get(&HashableKey::str_key(CompactString::from("sort_keys"))) {
                sort_keys = sk.is_truthy();
            }
            if let Some(ea) = r.get(&HashableKey::str_key(CompactString::from("ensure_ascii"))) {
                ensure_ascii = ea.is_truthy();
            }
            if let Some(seps) = r.get(&HashableKey::str_key(CompactString::from("separators"))) {
                if let PyObjectPayload::Tuple(parts) = &seps.payload {
                    if parts.len() == 2 {
                        item_sep = parts[0].py_to_string();
                        kv_sep = parts[1].py_to_string();
                    }
                }
            }
            if let Some(def) = r.get(&HashableKey::str_key(CompactString::from("default"))) {
                default_fn = Some(def.clone());
            }
            // cls=CustomEncoder: create an instance and bind its `default` method
            if default_fn.is_none() {
                if let Some(cls) = r.get(&HashableKey::str_key(CompactString::from("cls"))) {
                    let encoder_inst = PyObject::instance(cls.clone());
                    if let Some(default_method) = cls.get_attr("default") {
                        match &default_method.payload {
                            // Native default method — can call directly
                            PyObjectPayload::NativeFunction(_)
                            | PyObjectPayload::NativeClosure(_) => {
                                default_fn = Some(PyObject::wrap(PyObjectPayload::BoundMethod {
                                    receiver: encoder_inst,
                                    method: default_method,
                                }));
                            }
                            // Python default method — pre-convert object tree then serialize
                            PyObjectPayload::Function(_) => {
                                let converted = pre_convert_for_json(&args[0]);
                                let obj_to_serialize = if sort_keys {
                                    sort_dict_keys_recursive(&converted)
                                } else {
                                    converted
                                };
                                ENSURE_ASCII.with(|f| f.set(ensure_ascii));
                                let s = if let Some(indent_size) = indent {
                                    py_to_json_pretty(&obj_to_serialize, 0, indent_size, None)?
                                } else {
                                    py_to_json_sep(&obj_to_serialize, &item_sep, &kv_sep, None)?
                                };
                                ENSURE_ASCII.with(|f| f.set(true));
                                return Ok(PyObject::str_val(CompactString::from(s)));
                            }
                            _ => {}
                        }
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

/// Recursively convert non-JSON-serializable types to serializable equivalents.
/// Handles: set/frozenset → list, bytes → str, Instance with __dict__ → dict.
/// Used when cls= is provided with a Python default method (which can't be called
/// from native code synchronously).
fn pre_convert_for_json(obj: &PyObjectRef) -> PyObjectRef {
    match &obj.payload {
        PyObjectPayload::Dict(map) => {
            let r = map.read();
            let entries: Vec<_> = r
                .iter()
                .map(|(k, v)| (k.clone(), pre_convert_for_json(v)))
                .collect();
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
            let converted: Vec<_> = r.iter().map(|i| pre_convert_for_json(i)).collect();
            drop(r);
            PyObject::list(converted)
        }
        PyObjectPayload::Tuple(items) => {
            let converted: Vec<_> = items.iter().map(|i| pre_convert_for_json(i)).collect();
            PyObject::list(converted)
        }
        PyObjectPayload::Set(set) => {
            let r = set.read();
            let items: Vec<_> = r
                .keys()
                .map(|k| pre_convert_for_json(&k.to_object()))
                .collect();
            drop(r);
            PyObject::list(items)
        }
        PyObjectPayload::FrozenSet(set) => {
            let items: Vec<_> = set
                .keys()
                .map(|k| pre_convert_for_json(&k.to_object()))
                .collect();
            PyObject::list(items)
        }
        PyObjectPayload::Bytes(b) => {
            let s = String::from_utf8_lossy(b).to_string();
            PyObject::str_val(CompactString::from(s))
        }
        PyObjectPayload::Instance(inst) => {
            let attrs = inst.attrs.read();
            if attrs.is_empty() {
                obj.clone()
            } else {
                let new_dict = PyObject::dict_from_pairs(vec![]);
                if let PyObjectPayload::Dict(new_map) = &new_dict.payload {
                    let mut w = new_map.write();
                    for (k, v) in attrs.iter() {
                        w.insert(HashableKey::str_key(k.clone()), pre_convert_for_json(v));
                    }
                }
                new_dict
            }
        }
        _ => obj.clone(),
    }
}

/// Recursively sort dictionary keys for sort_keys=True in json.dumps
fn sort_dict_keys_recursive(obj: &PyObjectRef) -> PyObjectRef {
    match &obj.payload {
        PyObjectPayload::Dict(map) => {
            let r = map.read();
            let mut entries: Vec<_> = r
                .iter()
                .map(|(k, v)| (k.clone(), sort_dict_keys_recursive(v)))
                .collect();
            entries.sort_by(|(a, _), (b, _)| {
                let a_str = match a {
                    HashableKey::Str(s) => s.to_string(),
                    HashableKey::Int(n) => n.to_string(),
                    _ => format!("{:?}", a),
                };
                let b_str = match b {
                    HashableKey::Str(s) => s.to_string(),
                    HashableKey::Int(n) => n.to_string(),
                    _ => format!("{:?}", b),
                };
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

fn py_to_json_pretty(
    obj: &PyObjectRef,
    depth: usize,
    indent: usize,
    default: Option<&PyObjectRef>,
) -> PyResult<String> {
    let pad = " ".repeat(indent * (depth + 1));
    let pad_close = " ".repeat(indent * depth);
    match &obj.payload {
        PyObjectPayload::None => Ok("null".into()),
        PyObjectPayload::Bool(b) => Ok(if *b { "true" } else { "false" }.into()),
        PyObjectPayload::Int(n) => Ok(n.to_string()),
        PyObjectPayload::Float(f) => {
            if f.is_nan() {
                return Err(PyException::value_error("NaN is not JSON serializable"));
            }
            if f.is_infinite() {
                return Err(PyException::value_error(
                    "Infinity is not JSON serializable",
                ));
            }
            Ok(format!("{}", f))
        }
        PyObjectPayload::Str(s) => Ok(json_escape_string(s)),
        PyObjectPayload::List(items) => {
            let r = items.read();
            if r.is_empty() {
                return Ok("[]".into());
            }
            let parts: Result<Vec<String>, PyException> = r
                .iter()
                .map(|i| py_to_json_pretty(i, depth + 1, indent, default))
                .collect();
            Ok(format!(
                "[\n{}{}\n{}]",
                pad,
                parts?.join(&format!(",\n{}", pad)),
                pad_close
            ))
        }
        PyObjectPayload::Tuple(items) => {
            if items.is_empty() {
                return Ok("[]".into());
            }
            let parts: Result<Vec<String>, PyException> = items
                .iter()
                .map(|i| py_to_json_pretty(i, depth + 1, indent, default))
                .collect();
            Ok(format!(
                "[\n{}{}\n{}]",
                pad,
                parts?.join(&format!(",\n{}", pad)),
                pad_close
            ))
        }
        PyObjectPayload::Dict(map) => {
            let r = map.read();
            if r.is_empty() {
                return Ok("{}".into());
            }
            let parts: Result<Vec<String>, PyException> = r
                .iter()
                .map(|(k, v)| {
                    let key_str = match k {
                        HashableKey::Str(s) => json_escape_string(s),
                        HashableKey::Int(n) => format!("\"{}\"", n),
                        _ => return Err(PyException::type_error("keys must be str")),
                    };
                    let val_str = py_to_json_pretty(v, depth + 1, indent, default)?;
                    Ok(format!("{}: {}", key_str, val_str))
                })
                .collect();
            Ok(format!(
                "{{\n{}{}\n{}}}",
                pad,
                parts?.join(&format!(",\n{}", pad)),
                pad_close
            ))
        }
        PyObjectPayload::Set(map) => {
            if default.is_some() {
                json_serialize_fallback(obj, default, |o, d| py_to_json_pretty(o, depth, indent, d))
            } else {
                let r = map.read();
                let items: Vec<PyObjectRef> = r.keys().map(|k| k.to_object()).collect();
                let list = PyObject::list(items);
                py_to_json_pretty(&list, depth, indent, default)
            }
        }
        PyObjectPayload::InstanceDict(attrs) => {
            let r = attrs.read();
            if r.is_empty() {
                return Ok("{}".into());
            }
            let parts: Result<Vec<String>, PyException> = r
                .iter()
                .map(|(k, v)| {
                    let key_str = json_escape_string(k);
                    let val_str = py_to_json_pretty(v, depth + 1, indent, default)?;
                    Ok(format!("{}: {}", key_str, val_str))
                })
                .collect();
            Ok(format!(
                "{{\n{}{}\n{}}}",
                pad,
                parts?.join(&format!(",\n{}", pad)),
                pad_close
            ))
        }
        _ => json_serialize_fallback(obj, default, |o, d| py_to_json_pretty(o, depth, indent, d)),
    }
}

pub(super) fn py_to_json(obj: &PyObjectRef) -> PyResult<String> {
    py_to_json_sep(obj, ", ", ": ", None)
}

fn py_to_json_sep(
    obj: &PyObjectRef,
    item_sep: &str,
    kv_sep: &str,
    default: Option<&PyObjectRef>,
) -> PyResult<String> {
    match &obj.payload {
        PyObjectPayload::None => Ok("null".into()),
        PyObjectPayload::Bool(b) => Ok(if *b { "true" } else { "false" }.into()),
        PyObjectPayload::Int(n) => Ok(n.to_string()),
        PyObjectPayload::Float(f) => {
            if f.is_nan() {
                return Err(PyException::value_error("NaN is not JSON serializable"));
            }
            if f.is_infinite() {
                return Err(PyException::value_error(
                    "Infinity is not JSON serializable",
                ));
            }
            Ok(format!("{}", f))
        }
        PyObjectPayload::Str(s) => Ok(json_escape_string(s)),
        PyObjectPayload::List(items) => {
            let r = items.read();
            let parts: Result<Vec<String>, PyException> = r
                .iter()
                .map(|i| py_to_json_sep(i, item_sep, kv_sep, default))
                .collect();
            Ok(format!("[{}]", parts?.join(item_sep)))
        }
        PyObjectPayload::Tuple(items) => {
            let parts: Result<Vec<String>, PyException> = items
                .iter()
                .map(|i| py_to_json_sep(i, item_sep, kv_sep, default))
                .collect();
            Ok(format!("[{}]", parts?.join(item_sep)))
        }
        PyObjectPayload::Dict(map) => {
            let r = map.read();
            let parts: Result<Vec<String>, PyException> = r
                .iter()
                .map(|(k, v)| {
                    let key_str = match k {
                        HashableKey::Str(s) => json_escape_string(s),
                        HashableKey::Int(n) => format!("\"{}\"", n),
                        _ => return Err(PyException::type_error("keys must be str")),
                    };
                    let val_str = py_to_json_sep(v, item_sep, kv_sep, default)?;
                    Ok(format!("{}{}{}", key_str, kv_sep, val_str))
                })
                .collect();
            Ok(format!("{{{}}}", parts?.join(item_sep)))
        }
        PyObjectPayload::InstanceDict(attrs) => {
            // InstanceDict is a dict representation — always serialize directly
            let r = attrs.read();
            let parts: Result<Vec<String>, PyException> = r
                .iter()
                .map(|(k, v)| {
                    let key_str = json_escape_string(k);
                    let val_str = py_to_json_sep(v, item_sep, kv_sep, default)?;
                    Ok(format!("{}{}{}", key_str, kv_sep, val_str))
                })
                .collect();
            Ok(format!("{{{}}}", parts?.join(item_sep)))
        }
        PyObjectPayload::Set(map) => {
            if default.is_some() {
                json_serialize_fallback(obj, default, |o, d| py_to_json_sep(o, item_sep, kv_sep, d))
            } else {
                let r = map.read();
                let items: Vec<PyObjectRef> = r.keys().map(|k| k.to_object()).collect();
                let list = PyObject::list(items);
                py_to_json_sep(&list, item_sep, kv_sep, default)
            }
        }
        _ => json_serialize_fallback(obj, default, |o, d| py_to_json_sep(o, item_sep, kv_sep, d)),
    }
}

/// Handle non-primitive objects in JSON serialization:
/// 1. If a `default` callable is provided, call it and re-serialize the result
/// 2. For Instance objects with a Python Function default, auto-serialize __dict__
/// 3. Otherwise, raise TypeError (matching CPython behavior)
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
        // Python Function can't be called synchronously from Rust —
        // auto-serialize Instance attrs as __dict__ (most common default pattern)
        if let Some(dict) = instance_to_dict(obj) {
            return recurse(&dict, default);
        }
    }

    Err(PyException::type_error(format!(
        "Object of type {} is not JSON serializable",
        obj.type_name()
    )))
}

/// Extract instance attrs as a Dict (equivalent to obj.__dict__)
fn instance_to_dict(obj: &PyObjectRef) -> Option<PyObjectRef> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        let attrs_r = inst.attrs.read();
        let mut map = IndexMap::new();
        for (k, v) in attrs_r.iter() {
            // Skip dunder attrs and callables
            let ks: &str = k.as_str();
            if ks.starts_with("__") && ks.ends_with("__") {
                continue;
            }
            map.insert(HashableKey::str_key(CompactString::from(ks)), v.clone());
        }
        Some(PyObject::dict(map))
    } else {
        None
    }
}

/// Try to call a default callable (NativeFunction, NativeClosure, or BoundMethod)
pub(super) fn try_call_default(
    default: &PyObjectRef,
    obj: &PyObjectRef,
) -> PyResult<Option<PyObjectRef>> {
    match &default.payload {
        PyObjectPayload::NativeFunction(nf) => Ok(Some((nf.func)(&[obj.clone()])?)),
        PyObjectPayload::NativeClosure(nc) => Ok(Some((nc.func)(&[obj.clone()])?)),
        PyObjectPayload::BoundMethod { receiver, method } => {
            // Call method(self, obj) — dispatch based on method type
            match &method.payload {
                PyObjectPayload::NativeFunction(nf) => {
                    Ok(Some((nf.func)(&[receiver.clone(), obj.clone()])?))
                }
                PyObjectPayload::NativeClosure(nc) => {
                    Ok(Some((nc.func)(&[receiver.clone(), obj.clone()])?))
                }
                PyObjectPayload::Function(_) => {
                    // Python function — we need the VM. Use request_vm_call.
                    ferrython_core::error::request_vm_call(
                        method.clone(),
                        vec![receiver.clone(), obj.clone()],
                    );
                    Ok(None) // signal that we need VM callback
                }
                _ => Ok(None),
            }
        }
        PyObjectPayload::Function(_) => {
            // Python function — can't call synchronously from Rust.
            // Caller handles via instance_to_dict fallback.
            Ok(None)
        }
        _ => Ok(None),
    }
}
