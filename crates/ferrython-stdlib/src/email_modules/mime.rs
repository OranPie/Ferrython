use super::*;

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
    make_module(
        "email.mime.text",
        vec![("MIMEText", make_builtin(mime_text_constructor))],
    )
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
    make_module(
        "email.mime.multipart",
        vec![("MIMEMultipart", make_builtin(mime_multipart_constructor))],
    )
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
    make_module(
        "email.mime.base",
        vec![("MIMEBase", make_builtin(mime_base_constructor))],
    )
}

// ── email.mime.application module ──────────────────────────────────────

fn mime_application_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "MIMEApplication() missing required argument: 'data'",
        ));
    }
    let data = match &args[0].payload {
        PyObjectPayload::Bytes(b) => (**b).clone(),
        PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
        _ => args[0].py_to_string().into_bytes(),
    };
    let subtype = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        "octet-stream".to_string()
    };
    let ct = format!("application/{}", subtype);
    // Base64-encode the data
    let encoded = base64_encode(&data);
    let msg = build_message_instance(
        Some(&ct),
        Some(PyObject::str_val(CompactString::from(encoded))),
    );
    // Set Content-Transfer-Encoding header
    if let PyObjectPayload::Instance(ref inst) = msg.payload {
        let attrs = inst.attrs.read();
        if let Some(setitem) = attrs.get("__setitem__") {
            if let PyObjectPayload::NativeClosure(nc) = &setitem.payload {
                let _ = (nc.func)(&[
                    PyObject::str_val(CompactString::from("Content-Transfer-Encoding")),
                    PyObject::str_val(CompactString::from("base64")),
                ]);
            }
        }
    }
    Ok(msg)
}

fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

pub fn create_email_mime_application_module() -> PyObjectRef {
    make_module(
        "email.mime.application",
        vec![(
            "MIMEApplication",
            make_builtin(mime_application_constructor),
        )],
    )
}

// ── email.mime.image module ────────────────────────────────────────────

fn mime_image_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "MIMEImage() missing required argument: 'imagedata'",
        ));
    }
    let data = match &args[0].payload {
        PyObjectPayload::Bytes(b) => (**b).clone(),
        _ => args[0].py_to_string().into_bytes(),
    };
    let subtype = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        // Auto-detect from data magic bytes
        if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
            "png".to_string()
        } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
            "jpeg".to_string()
        } else if data.starts_with(b"GIF8") {
            "gif".to_string()
        } else {
            "octet-stream".to_string()
        }
    };
    let ct = format!("image/{}", subtype);
    let encoded = base64_encode(&data);
    let msg = build_message_instance(
        Some(&ct),
        Some(PyObject::str_val(CompactString::from(encoded))),
    );
    if let PyObjectPayload::Instance(ref inst) = msg.payload {
        let attrs = inst.attrs.read();
        if let Some(setitem) = attrs.get("__setitem__") {
            if let PyObjectPayload::NativeClosure(nc) = &setitem.payload {
                let _ = (nc.func)(&[
                    PyObject::str_val(CompactString::from("Content-Transfer-Encoding")),
                    PyObject::str_val(CompactString::from("base64")),
                ]);
            }
        }
    }
    Ok(msg)
}

pub fn create_email_mime_image_module() -> PyObjectRef {
    make_module(
        "email.mime.image",
        vec![("MIMEImage", make_builtin(mime_image_constructor))],
    )
}

// ── email.mime package ─────────────────────────────────────────────────

pub fn create_email_mime_module() -> PyObjectRef {
    make_module(
        "email.mime",
        vec![
            ("text", create_email_mime_text_module()),
            ("multipart", create_email_mime_multipart_module()),
            ("base", create_email_mime_base_module()),
            ("application", create_email_mime_application_module()),
            ("image", create_email_mime_image_module()),
        ],
    )
}
