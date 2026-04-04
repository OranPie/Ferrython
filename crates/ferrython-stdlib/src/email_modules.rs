//! Email stdlib modules: email, email.message, email.mime.*, email.utils

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin,
};
use indexmap::IndexMap;
use std::sync::{Arc, Mutex};

// ── Helper: build a Message instance ───────────────────────────────────

fn build_message_instance(
    content_type: Option<&str>,
    payload: Option<PyObjectRef>,
) -> PyObjectRef {
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
    attrs.insert(CompactString::from("__email_message__"), PyObject::bool_val(true));

    // __getitem__(key)
    {
        let h = headers.clone();
        attrs.insert(CompactString::from("__getitem__"),
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
            }));
    }

    // __setitem__(key, val)
    {
        let h = headers.clone();
        attrs.insert(CompactString::from("__setitem__"),
            PyObject::native_closure("__setitem__", move |args| {
                if args.len() < 2 {
                    return Err(PyException::type_error("__setitem__() requires key and value"));
                }
                let key = CompactString::from(args[0].py_to_string());
                let val = args[1].clone();
                h.lock().unwrap().insert(key, val);
                Ok(PyObject::none())
            }));
    }

    // __contains__(key)
    {
        let h = headers.clone();
        attrs.insert(CompactString::from("__contains__"),
            PyObject::native_closure("__contains__", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("__contains__() requires a key"));
                }
                let key = args[0].py_to_string();
                let guard = h.lock().unwrap();
                Ok(PyObject::bool_val(guard.contains_key(key.as_str())))
            }));
    }

    // keys()
    {
        let h = headers.clone();
        attrs.insert(CompactString::from("keys"),
            PyObject::native_closure("keys", move |_args| {
                let guard = h.lock().unwrap();
                let keys: Vec<PyObjectRef> = guard
                    .keys()
                    .map(|k| PyObject::str_val(k.clone()))
                    .collect();
                Ok(PyObject::list(keys))
            }));
    }

    // values()
    {
        let h = headers.clone();
        attrs.insert(CompactString::from("values"),
            PyObject::native_closure("values", move |_args| {
                let guard = h.lock().unwrap();
                let vals: Vec<PyObjectRef> = guard.values().cloned().collect();
                Ok(PyObject::list(vals))
            }));
    }

    // items()
    {
        let h = headers.clone();
        attrs.insert(CompactString::from("items"),
            PyObject::native_closure("items", move |_args| {
                let guard = h.lock().unwrap();
                let items: Vec<PyObjectRef> = guard
                    .iter()
                    .map(|(k, v)| PyObject::tuple(vec![
                        PyObject::str_val(k.clone()),
                        v.clone(),
                    ]))
                    .collect();
                Ok(PyObject::list(items))
            }));
    }

    // get(key, default=None)
    {
        let h = headers.clone();
        attrs.insert(CompactString::from("get"),
            PyObject::native_closure("get", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("get() requires a key"));
                }
                let key = args[0].py_to_string();
                let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
                let guard = h.lock().unwrap();
                Ok(guard.get(key.as_str()).cloned().unwrap_or(default))
            }));
    }

    // get_payload()
    {
        let p = payload_cell.clone();
        attrs.insert(CompactString::from("get_payload"),
            PyObject::native_closure("get_payload", move |_args| {
                Ok(p.lock().unwrap().clone())
            }));
    }

    // set_payload(payload)
    {
        let p = payload_cell.clone();
        attrs.insert(CompactString::from("set_payload"),
            PyObject::native_closure("set_payload", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("set_payload() requires an argument"));
                }
                *p.lock().unwrap() = args[0].clone();
                Ok(PyObject::none())
            }));
    }

    // get_content_type()
    {
        let h = headers.clone();
        attrs.insert(CompactString::from("get_content_type"),
            PyObject::native_closure("get_content_type", move |_args| {
                let guard = h.lock().unwrap();
                match guard.get("Content-Type") {
                    Some(v) => Ok(v.clone()),
                    None => Ok(PyObject::str_val(CompactString::from("text/plain"))),
                }
            }));
    }

    // get_charset()
    {
        let h = headers.clone();
        attrs.insert(CompactString::from("get_charset"),
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
            }));
    }

    // as_string()
    {
        let h = headers.clone();
        let p = payload_cell.clone();
        attrs.insert(CompactString::from("as_string"),
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
            }));
    }

    // is_multipart()
    {
        let p = payload_cell.clone();
        attrs.insert(CompactString::from("is_multipart"),
            PyObject::native_closure("is_multipart", move |_args| {
                let payload = p.lock().unwrap();
                Ok(PyObject::bool_val(matches!(payload.payload, PyObjectPayload::List(_))))
            }));
    }

    // attach(part) — for multipart messages
    {
        let p = payload_cell.clone();
        attrs.insert(CompactString::from("attach"),
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
            }));
    }

    let cls = PyObject::class(CompactString::from("Message"), vec![], IndexMap::new());
    PyObject::instance_with_attrs(cls, attrs)
}

// ── email.message module ───────────────────────────────────────────────

fn email_message_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let _ = args;
    Ok(build_message_instance(None, None))
}

pub fn create_email_message_module() -> PyObjectRef {
    make_module("email.message", vec![
        ("Message", make_builtin(email_message_constructor)),
    ])
}

// ── email.mime.text module ─────────────────────────────────────────────

fn mime_text_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "MIMEText() missing required argument: 'text'",
        ));
    }
    let text = args[0].py_to_string();
    let subtype = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        "plain".to_string()
    };
    let charset = if args.len() > 2 {
        args[2].py_to_string()
    } else {
        "utf-8".to_string()
    };
    let ct = format!("text/{}; charset=\"{}\"", subtype, charset);
    Ok(build_message_instance(
        Some(&ct),
        Some(PyObject::str_val(CompactString::from(text))),
    ))
}

pub fn create_email_mime_text_module() -> PyObjectRef {
    make_module("email.mime.text", vec![
        ("MIMEText", make_builtin(mime_text_constructor)),
    ])
}

// ── email.mime.multipart module ────────────────────────────────────────

fn mime_multipart_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let subtype = if !args.is_empty() {
        args[0].py_to_string()
    } else {
        "mixed".to_string()
    };
    let ct = format!("multipart/{}", subtype);
    Ok(build_message_instance(
        Some(&ct),
        Some(PyObject::list(vec![])),
    ))
}

pub fn create_email_mime_multipart_module() -> PyObjectRef {
    make_module("email.mime.multipart", vec![
        ("MIMEMultipart", make_builtin(mime_multipart_constructor)),
    ])
}

// ── email.mime.base module ─────────────────────────────────────────────

fn mime_base_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "MIMEBase() requires maintype and subtype",
        ));
    }
    let maintype = args[0].py_to_string();
    let subtype = args[1].py_to_string();
    let ct = format!("{}/{}", maintype, subtype);
    Ok(build_message_instance(Some(&ct), None))
}

pub fn create_email_mime_base_module() -> PyObjectRef {
    make_module("email.mime.base", vec![
        ("MIMEBase", make_builtin(mime_base_constructor)),
    ])
}

// ── email.mime package ─────────────────────────────────────────────────

pub fn create_email_mime_module() -> PyObjectRef {
    make_module("email.mime", vec![
        ("text", create_email_mime_text_module()),
        ("multipart", create_email_mime_multipart_module()),
        ("base", create_email_mime_base_module()),
    ])
}

// ── email.utils module ─────────────────────────────────────────────────

fn email_formatdate(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let _ = args;
    // Return a fixed RFC 2822 formatted date string
    Ok(PyObject::str_val(CompactString::from("Thu, 01 Jan 1970 00:00:00 +0000")))
}

fn email_parsedate(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("parsedate() requires a date string"));
    }
    let _ = args[0].py_to_string();
    // Return a 9-tuple stub (year, month, day, hour, min, sec, weekday, julian, tz)
    Ok(PyObject::tuple(vec![
        PyObject::int(1970), PyObject::int(1), PyObject::int(1),
        PyObject::int(0), PyObject::int(0), PyObject::int(0),
        PyObject::int(3), PyObject::int(1), PyObject::int(0),
    ]))
}

fn email_formataddr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("formataddr() requires a (name, addr) pair"));
    }
    // Expect a tuple (name, addr)
    let pair = &args[0];
    let (name, addr) = match &pair.payload {
        PyObjectPayload::Tuple(items) if items.len() >= 2 => {
            (items[0].py_to_string(), items[1].py_to_string())
        }
        _ => {
            return Err(PyException::type_error("formataddr() argument must be a (name, addr) tuple"));
        }
    };
    if name.is_empty() {
        Ok(PyObject::str_val(CompactString::from(addr)))
    } else {
        Ok(PyObject::str_val(CompactString::from(format!("{} <{}>", name, addr))))
    }
}

fn email_parseaddr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("parseaddr() requires an address string"));
    }
    let addr_str = args[0].py_to_string();
    // Simple parsing: "Name <email>" or just "email"
    if let Some(lt) = addr_str.find('<') {
        if let Some(gt) = addr_str.find('>') {
            let name = addr_str[..lt].trim().to_string();
            let email = addr_str[lt+1..gt].trim().to_string();
            return Ok(PyObject::tuple(vec![
                PyObject::str_val(CompactString::from(name)),
                PyObject::str_val(CompactString::from(email)),
            ]));
        }
    }
    Ok(PyObject::tuple(vec![
        PyObject::str_val(CompactString::from("")),
        PyObject::str_val(CompactString::from(addr_str)),
    ]))
}

fn email_make_msgid(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let _ = args;
    // Generate a simple unique-ish message ID
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let msgid = format!("<{}.ferrython@localhost>", ts);
    Ok(PyObject::str_val(CompactString::from(msgid)))
}

pub fn create_email_utils_module() -> PyObjectRef {
    make_module("email.utils", vec![
        ("formatdate", make_builtin(email_formatdate)),
        ("parsedate", make_builtin(email_parsedate)),
        ("formataddr", make_builtin(email_formataddr)),
        ("parseaddr", make_builtin(email_parseaddr)),
        ("make_msgid", make_builtin(email_make_msgid)),
    ])
}

// ── email top-level package ────────────────────────────────────────────

pub fn create_email_module() -> PyObjectRef {
    make_module("email", vec![
        ("message", create_email_message_module()),
        ("mime", create_email_mime_module()),
        ("utils", create_email_utils_module()),
    ])
}
