//! Email stdlib modules: email, email.message, email.mime.*, email.utils

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;
use std::sync::{Arc, Mutex};

mod charset;
mod contentmanager;
mod errors;
mod message;
mod mime;
mod policy;
mod utils;

pub use charset::create_email_charset_module;
pub use contentmanager::create_email_contentmanager_module;
pub use errors::create_email_errors_module;
pub use message::create_email_message_module;
pub use mime::{
    create_email_mime_application_module, create_email_mime_base_module,
    create_email_mime_image_module, create_email_mime_module, create_email_mime_multipart_module,
    create_email_mime_text_module,
};
pub use policy::create_email_policy_module;
pub use utils::create_email_utils_module;

// ── Helper: build a Message instance ───────────────────────────────────

/// Serialize a message part by calling its _serialize closure (which captures headers/payload Arcs)
fn serialize_message_part(part: &PyObjectRef) -> String {
    if let PyObjectPayload::Instance(ref inst) = part.payload {
        let attrs = inst.attrs.read();
        if let Some(ser_fn) = attrs.get("_serialize") {
            if let PyObjectPayload::NativeClosure(nc) = &ser_fn.payload {
                if let Ok(result) = (nc.func)(&[]) {
                    return result.py_to_string();
                }
            }
        }
    }
    part.py_to_string()
}

fn build_message_instance(content_type: Option<&str>, payload: Option<PyObjectRef>) -> PyObjectRef {
    let headers: Arc<Mutex<IndexMap<CompactString, PyObjectRef>>> =
        Arc::new(Mutex::new(IndexMap::new()));
    let payload_cell: Arc<Mutex<PyObjectRef>> =
        Arc::new(Mutex::new(payload.unwrap_or_else(PyObject::none)));

    if let Some(ct) = content_type {
        headers.lock().unwrap().insert(
            CompactString::from("Content-Type"),
            PyObject::str_val(CompactString::from(ct)),
        );
    }

    let mut attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    attrs.insert(
        CompactString::from("__email_message__"),
        PyObject::bool_val(true),
    );

    // __getitem__(key)
    {
        let h = headers.clone();
        attrs.insert(
            CompactString::from("__getitem__"),
            PyObject::native_closure("__getitem__", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("__getitem__() requires a key"));
                }
                let key = args[0].py_to_string();
                let guard = h.lock().unwrap();
                match guard.get(key.as_str()) {
                    Some(v) => Ok(v.clone()),
                    None => Err(PyException::key_error(&key)),
                }
            }),
        );
    }

    // __setitem__(key, val)
    {
        let h = headers.clone();
        attrs.insert(
            CompactString::from("__setitem__"),
            PyObject::native_closure("__setitem__", move |args| {
                if args.len() < 2 {
                    return Err(PyException::type_error(
                        "__setitem__() requires key and value",
                    ));
                }
                let key = CompactString::from(args[0].py_to_string());
                let val = args[1].clone();
                h.lock().unwrap().insert(key, val);
                Ok(PyObject::none())
            }),
        );
    }

    // __contains__(key)
    {
        let h = headers.clone();
        attrs.insert(
            CompactString::from("__contains__"),
            PyObject::native_closure("__contains__", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("__contains__() requires a key"));
                }
                let key = args[0].py_to_string();
                let guard = h.lock().unwrap();
                Ok(PyObject::bool_val(guard.contains_key(key.as_str())))
            }),
        );
    }

    // keys()
    {
        let h = headers.clone();
        attrs.insert(
            CompactString::from("keys"),
            PyObject::native_closure("keys", move |_args| {
                let guard = h.lock().unwrap();
                let keys: Vec<PyObjectRef> =
                    guard.keys().map(|k| PyObject::str_val(k.clone())).collect();
                Ok(PyObject::list(keys))
            }),
        );
    }

    // values()
    {
        let h = headers.clone();
        attrs.insert(
            CompactString::from("values"),
            PyObject::native_closure("values", move |_args| {
                let guard = h.lock().unwrap();
                let vals: Vec<PyObjectRef> = guard.values().cloned().collect();
                Ok(PyObject::list(vals))
            }),
        );
    }

    // items()
    {
        let h = headers.clone();
        attrs.insert(
            CompactString::from("items"),
            PyObject::native_closure("items", move |_args| {
                let guard = h.lock().unwrap();
                let items: Vec<PyObjectRef> = guard
                    .iter()
                    .map(|(k, v)| PyObject::tuple(vec![PyObject::str_val(k.clone()), v.clone()]))
                    .collect();
                Ok(PyObject::list(items))
            }),
        );
    }

    // get(key, default=None)
    {
        let h = headers.clone();
        attrs.insert(
            CompactString::from("get"),
            PyObject::native_closure("get", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("get() requires a key"));
                }
                let key = args[0].py_to_string();
                let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
                let guard = h.lock().unwrap();
                Ok(guard.get(key.as_str()).cloned().unwrap_or(default))
            }),
        );
    }

    // get_payload()
    {
        let p = payload_cell.clone();
        attrs.insert(
            CompactString::from("get_payload"),
            PyObject::native_closure("get_payload", move |_args| Ok(p.lock().unwrap().clone())),
        );
    }

    // set_payload(payload)
    {
        let p = payload_cell.clone();
        attrs.insert(
            CompactString::from("set_payload"),
            PyObject::native_closure("set_payload", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "set_payload() requires an argument",
                    ));
                }
                *p.lock().unwrap() = args[0].clone();
                Ok(PyObject::none())
            }),
        );
    }

    // get_content_type()
    {
        let h = headers.clone();
        attrs.insert(
            CompactString::from("get_content_type"),
            PyObject::native_closure("get_content_type", move |_args| {
                let guard = h.lock().unwrap();
                match guard.get("Content-Type") {
                    Some(v) => {
                        let s = v.py_to_string();
                        let ct = s.split(';').next().unwrap_or("text/plain").trim();
                        Ok(PyObject::str_val(CompactString::from(ct)))
                    }
                    None => Ok(PyObject::str_val(CompactString::from("text/plain"))),
                }
            }),
        );
    }

    // get_content_maintype()
    {
        let h = headers.clone();
        attrs.insert(
            CompactString::from("get_content_maintype"),
            PyObject::native_closure("get_content_maintype", move |_args| {
                let guard = h.lock().unwrap();
                let ct = guard
                    .get("Content-Type")
                    .map(|v| v.py_to_string())
                    .unwrap_or_else(|| "text/plain".to_string());
                let main = ct
                    .split('/')
                    .next()
                    .unwrap_or("text")
                    .split(';')
                    .next()
                    .unwrap_or("text")
                    .trim();
                Ok(PyObject::str_val(CompactString::from(main)))
            }),
        );
    }

    // get_content_subtype()
    {
        let h = headers.clone();
        attrs.insert(
            CompactString::from("get_content_subtype"),
            PyObject::native_closure("get_content_subtype", move |_args| {
                let guard = h.lock().unwrap();
                let ct = guard
                    .get("Content-Type")
                    .map(|v| v.py_to_string())
                    .unwrap_or_else(|| "text/plain".to_string());
                let parts: Vec<&str> = ct.split('/').collect();
                let sub = if parts.len() > 1 {
                    parts[1].split(';').next().unwrap_or("plain").trim()
                } else {
                    "plain"
                };
                Ok(PyObject::str_val(CompactString::from(sub)))
            }),
        );
    }

    // get_charset()
    {
        let h = headers.clone();
        attrs.insert(
            CompactString::from("get_charset"),
            PyObject::native_closure("get_charset", move |_args| {
                let guard = h.lock().unwrap();
                if let Some(ct) = guard.get("Content-Type") {
                    let ct_str = ct.py_to_string();
                    if let Some(idx) = ct_str.find("charset=") {
                        let rest = &ct_str[idx + 8..];
                        let charset = rest.split(';').next().unwrap_or("").trim();
                        return Ok(PyObject::str_val(CompactString::from(charset)));
                    }
                }
                Ok(PyObject::none())
            }),
        );
    }

    // as_string()
    {
        let h = headers.clone();
        let p = payload_cell.clone();
        attrs.insert(
            CompactString::from("as_string"),
            PyObject::native_closure("as_string", move |_args| {
                let guard = h.lock().unwrap();
                let mut s = String::new();
                for (k, v) in guard.iter() {
                    s.push_str(k.as_str());
                    s.push_str(": ");
                    s.push_str(&v.py_to_string());
                    s.push('\n');
                }
                s.push('\n');
                let payload = p.lock().unwrap();
                if !matches!(payload.payload, PyObjectPayload::None) {
                    let ps = payload.py_to_string();
                    if ps != "None" {
                        s.push_str(&ps);
                    }
                }
                Ok(PyObject::str_val(CompactString::from(s)))
            }),
        );
    }

    // is_multipart()
    {
        let p = payload_cell.clone();
        attrs.insert(
            CompactString::from("is_multipart"),
            PyObject::native_closure("is_multipart", move |_args| {
                let payload = p.lock().unwrap();
                Ok(PyObject::bool_val(matches!(
                    payload.payload,
                    PyObjectPayload::List(_)
                )))
            }),
        );
    }

    // set_content(body) — EmailMessage API
    {
        let h = headers.clone();
        let p = payload_cell.clone();
        attrs.insert(
            CompactString::from("set_content"),
            PyObject::native_closure("set_content", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("set_content() requires body text"));
                }
                let body = args[0].py_to_string();
                *p.lock().unwrap() = PyObject::str_val(CompactString::from(body));
                h.lock()
                    .unwrap()
                    .entry(CompactString::from("Content-Type"))
                    .or_insert_with(|| {
                        PyObject::str_val(CompactString::from("text/plain; charset=\"utf-8\""))
                    });
                Ok(PyObject::none())
            }),
        );
    }

    // get_content() — EmailMessage API: return decoded text payload
    {
        let p = payload_cell.clone();
        attrs.insert(
            CompactString::from("get_content"),
            PyObject::native_closure("get_content", move |_args| {
                let payload = p.lock().unwrap();
                Ok(payload.clone())
            }),
        );
    }

    // __str__ — produces RFC 2822 formatted string
    {
        let h = headers.clone();
        let p = payload_cell.clone();
        attrs.insert(
            CompactString::from("__str__"),
            PyObject::native_closure("__str__", move |_args| {
                let guard = h.lock().unwrap();
                let payload = p.lock().unwrap();
                let is_multipart = matches!(payload.payload, PyObjectPayload::List(_));
                let boundary = if is_multipart {
                    format!(
                        "==============={}==",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .subsec_nanos()
                    )
                } else {
                    String::new()
                };

                let mut s = String::new();
                for (k, v) in guard.iter() {
                    if is_multipart && k == "Content-Type" {
                        s.push_str("Content-Type: ");
                        let ct = v.py_to_string();
                        if ct.contains("boundary=") {
                            s.push_str(&ct);
                        } else {
                            s.push_str(&ct);
                            s.push_str(&format!("; boundary=\"{}\"", boundary));
                        }
                        s.push('\n');
                    } else {
                        s.push_str(k.as_str());
                        s.push_str(": ");
                        s.push_str(&v.py_to_string());
                        s.push('\n');
                    }
                }
                s.push('\n');

                if is_multipart {
                    if let PyObjectPayload::List(items) = &payload.payload {
                        for part in items.read().iter() {
                            s.push_str(&format!("--{}\n", boundary));
                            // Serialize each part by extracting its headers and payload
                            s.push_str(&serialize_message_part(part));
                            s.push('\n');
                        }
                        s.push_str(&format!("--{}--\n", boundary));
                    }
                } else if !matches!(payload.payload, PyObjectPayload::None) {
                    let ps = payload.py_to_string();
                    if ps != "None" {
                        s.push_str(&ps);
                        s.push('\n');
                    }
                }
                Ok(PyObject::str_val(CompactString::from(s)))
            }),
        );
    }

    // attach(part) — for multipart messages
    {
        let p = payload_cell.clone();
        attrs.insert(
            CompactString::from("attach"),
            PyObject::native_closure("attach", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("attach() requires a message part"));
                }
                let mut payload = p.lock().unwrap();
                match &payload.payload {
                    PyObjectPayload::List(items) => {
                        let mut new_items = items.write().clone();
                        new_items.push(args[0].clone());
                        *payload = PyObject::list(new_items);
                    }
                    _ => {
                        *payload = PyObject::list(vec![args[0].clone()]);
                    }
                }
                Ok(PyObject::none())
            }),
        );
    }

    // _serialize() — internal method for parent __str__ to call on parts
    {
        let h = headers.clone();
        let p = payload_cell.clone();
        attrs.insert(
            CompactString::from("_serialize"),
            PyObject::native_closure("_serialize", move |_args| {
                let guard = h.lock().unwrap();
                let payload = p.lock().unwrap();
                let mut s = String::new();
                for (k, v) in guard.iter() {
                    s.push_str(k.as_str());
                    s.push_str(": ");
                    s.push_str(&v.py_to_string());
                    s.push('\n');
                }
                s.push('\n');
                if !matches!(payload.payload, PyObjectPayload::None) {
                    let ps = payload.py_to_string();
                    if ps != "None" {
                        s.push_str(&ps);
                    }
                }
                Ok(PyObject::str_val(CompactString::from(s)))
            }),
        );
    }

    let cls = PyObject::class(CompactString::from("Message"), vec![], IndexMap::new());
    PyObject::instance_with_attrs(cls, attrs)
}

// ── email top-level package ────────────────────────────────────────────

pub fn create_email_module() -> PyObjectRef {
    make_module(
        "email",
        vec![
            ("message", create_email_message_module()),
            ("mime", create_email_mime_module()),
            ("utils", create_email_utils_module()),
            ("errors", create_email_errors_module()),
            ("policy", create_email_policy_module()),
            ("contentmanager", create_email_contentmanager_module()),
            ("charset", create_email_charset_module()),
            (
                "message_from_string",
                PyObject::native_function("email.message_from_string", email_message_from_string),
            ),
            (
                "message_from_bytes",
                PyObject::native_function("email.message_from_bytes", email_message_from_bytes),
            ),
        ],
    )
}

fn email_message_from_string(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "message_from_string() requires a string argument",
        ));
    }
    let raw = args[0].py_to_string();

    // Split headers from body at first blank line
    let (header_part, body) = if let Some(idx) = raw.find("\n\n") {
        (&raw[..idx], raw[idx + 2..].to_string())
    } else if let Some(idx) = raw.find("\r\n\r\n") {
        (&raw[..idx], raw[idx + 4..].to_string())
    } else {
        (raw.as_str(), String::new())
    };

    // Parse headers (handle continuation lines)
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in header_part.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation of previous header
            if let Some(last) = headers.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
        } else if let Some(colon_pos) = line.find(':') {
            let key = line[..colon_pos].trim().to_string();
            let val = line[colon_pos + 1..].trim().to_string();
            headers.push((key, val));
        }
    }

    // Build message using existing build_message_instance
    let msg = build_message_instance(None, Some(PyObject::str_val(CompactString::from(&body))));

    // Set headers via __setitem__
    if let PyObjectPayload::Instance(ref inst) = msg.payload {
        let attrs = inst.attrs.read();
        if let Some(setitem) = attrs.get("__setitem__") {
            if let PyObjectPayload::NativeClosure(nc) = &setitem.payload {
                for (k, v) in &headers {
                    let _ = (nc.func)(&[
                        PyObject::str_val(CompactString::from(k.as_str())),
                        PyObject::str_val(CompactString::from(v.as_str())),
                    ]);
                }
            }
        }
    }

    Ok(msg)
}

fn email_message_from_bytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "message_from_bytes() requires a bytes argument",
        ));
    }
    // Convert bytes to string, then delegate to message_from_string
    let text = match &args[0].payload {
        PyObjectPayload::Bytes(b) => String::from_utf8_lossy(b).to_string(),
        _ => args[0].py_to_string(),
    };
    let str_arg = PyObject::str_val(CompactString::from(text));
    email_message_from_string(&[str_arg])
}
