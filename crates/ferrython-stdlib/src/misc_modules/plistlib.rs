use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

// ── plistlib module ──

pub fn create_plistlib_module() -> PyObjectRef {
    // plistlib.dumps(value, fmt=FMT_XML) — serialize to XML plist bytes
    let dumps_fn = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error(
                "plistlib.dumps() missing required argument: 'value'",
            ));
        }
        let xml = plist_serialize_xml(&args[0])?;
        let full = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n{}</plist>\n", xml);
        Ok(PyObject::bytes(full.into_bytes()))
    });

    // plistlib.loads(data) — parse XML plist bytes
    let loads_fn = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error(
                "plistlib.loads() missing required argument: 'data'",
            ));
        }
        let data = match &args[0].payload {
            PyObjectPayload::Bytes(b) => String::from_utf8_lossy(b).to_string(),
            PyObjectPayload::Str(s) => s.to_string(),
            _ => {
                return Err(PyException::type_error(
                    "plistlib.loads() argument must be bytes or str",
                ))
            }
        };
        plist_parse_xml(&data)
    });

    // plistlib.dump(value, fp) — serialize to file
    let dump_fn = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "plistlib.dump() requires 2 arguments: value and file",
            ));
        }
        let xml = plist_serialize_xml(&args[0])?;
        let full = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n{}</plist>\n", xml);
        // If file arg is a string path, write directly
        if let PyObjectPayload::Str(path) = &args[1].payload {
            std::fs::write(path.as_str(), full.as_bytes())
                .map_err(|e| PyException::runtime_error(format!("plistlib.dump: {}", e)))?;
        }
        Ok(PyObject::none())
    });

    // plistlib.load(fp) — parse from file
    let load_fn = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error(
                "plistlib.load() missing required argument: 'fp'",
            ));
        }
        if let PyObjectPayload::Str(path) = &args[0].payload {
            let data = std::fs::read_to_string(path.as_str())
                .map_err(|e| PyException::runtime_error(format!("plistlib.load: {}", e)))?;
            return plist_parse_xml(&data);
        }
        Err(PyException::runtime_error(
            "plistlib.load: expected file path or file-like object",
        ))
    });

    make_module(
        "plistlib",
        vec![
            ("loads", loads_fn),
            ("dumps", dumps_fn),
            ("load", load_fn),
            ("dump", dump_fn),
            ("FMT_XML", PyObject::int(1)),
            ("FMT_BINARY", PyObject::int(2)),
        ],
    )
}

/// Serialize a Python object to XML plist format string
fn plist_serialize_xml(obj: &PyObjectRef) -> PyResult<String> {
    plist_serialize_xml_indent(obj, 0)
}

fn plist_serialize_xml_indent(obj: &PyObjectRef, indent: usize) -> PyResult<String> {
    let pad = "\t".repeat(indent);
    match &obj.payload {
        PyObjectPayload::None => Ok(format!("{}<false/>\n", pad)),
        PyObjectPayload::Bool(b) => {
            Ok(format!("{}<{}/>\n", pad, if *b { "true" } else { "false" }))
        }
        PyObjectPayload::Int(n) => Ok(format!("{}<integer>{}</integer>\n", pad, n)),
        PyObjectPayload::Float(f) => Ok(format!("{}<real>{}</real>\n", pad, f)),
        PyObjectPayload::Str(s) => {
            let escaped = s
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            Ok(format!("{}<string>{}</string>\n", pad, escaped))
        }
        PyObjectPayload::Bytes(b) => {
            use std::fmt::Write;
            let mut encoded = String::new();
            // Simple base64 encoding
            let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut i = 0;
            while i + 2 < b.len() {
                let n = ((b[i] as u32) << 16) | ((b[i + 1] as u32) << 8) | (b[i + 2] as u32);
                let _ = write!(
                    encoded,
                    "{}{}{}{}",
                    table[(n >> 18) as usize & 63] as char,
                    table[(n >> 12) as usize & 63] as char,
                    table[(n >> 6) as usize & 63] as char,
                    table[n as usize & 63] as char
                );
                i += 3;
            }
            if i + 1 == b.len() {
                let n = (b[i] as u32) << 16;
                let _ = write!(
                    encoded,
                    "{}{}==",
                    table[(n >> 18) as usize & 63] as char,
                    table[(n >> 12) as usize & 63] as char
                );
            } else if i + 2 == b.len() {
                let n = ((b[i] as u32) << 16) | ((b[i + 1] as u32) << 8);
                let _ = write!(
                    encoded,
                    "{}{}{}=",
                    table[(n >> 18) as usize & 63] as char,
                    table[(n >> 12) as usize & 63] as char,
                    table[(n >> 6) as usize & 63] as char
                );
            }
            Ok(format!(
                "{}<data>\n{}{}\n{}</data>\n",
                pad, pad, encoded, pad
            ))
        }
        PyObjectPayload::List(items) => {
            let items_r = items.read();
            let mut out = format!("{}<array>\n", pad);
            for item in items_r.iter() {
                out.push_str(&plist_serialize_xml_indent(item, indent + 1)?);
            }
            out.push_str(&format!("{}</array>\n", pad));
            Ok(out)
        }
        PyObjectPayload::Tuple(items) => {
            let mut out = format!("{}<array>\n", pad);
            for item in items.iter() {
                out.push_str(&plist_serialize_xml_indent(item, indent + 1)?);
            }
            out.push_str(&format!("{}</array>\n", pad));
            Ok(out)
        }
        PyObjectPayload::Dict(map) => {
            let map_r = map.read();
            let mut out = format!("{}<dict>\n", pad);
            for (k, v) in map_r.iter() {
                let key_str = match k {
                    HashableKey::Str(s) => s.to_string(),
                    HashableKey::Int(i) => i.to_string(),
                    _ => format!("{:?}", k),
                };
                let escaped = key_str.replace('&', "&amp;").replace('<', "&lt;");
                out.push_str(&format!("{}\t<key>{}</key>\n", pad, escaped));
                out.push_str(&plist_serialize_xml_indent(v, indent + 1)?);
            }
            out.push_str(&format!("{}</dict>\n", pad));
            Ok(out)
        }
        _ => Ok(format!(
            "{}<string>{}</string>\n",
            pad,
            obj.py_to_string()
                .replace('&', "&amp;")
                .replace('<', "&lt;")
        )),
    }
}

/// Parse XML plist data into Python objects
fn plist_parse_xml(xml: &str) -> PyResult<PyObjectRef> {
    // Find content inside <plist ...> ... </plist>
    let content = if let Some(start) = xml.find("<plist") {
        if let Some(gt) = xml[start..].find('>') {
            let after = &xml[start + gt + 1..];
            if let Some(end) = after.rfind("</plist>") {
                after[..end].trim()
            } else {
                after.trim()
            }
        } else {
            xml.trim()
        }
    } else {
        xml.trim()
    };

    let (obj, _) = plist_parse_element(content, 0)?;
    Ok(obj)
}

/// Parse a single XML element, return (value, position_after_element)
fn plist_parse_element(xml: &str, pos: usize) -> PyResult<(PyObjectRef, usize)> {
    let s = &xml[pos..];
    let s = s.trim_start();
    let new_pos = xml.len() - s.len();

    if s.is_empty() {
        return Ok((PyObject::none(), xml.len()));
    }

    if !s.starts_with('<') {
        return Err(PyException::value_error("plistlib: expected XML element"));
    }

    // Self-closing tags
    if s.starts_with("<true/>") {
        return Ok((PyObject::bool_val(true), new_pos + 7));
    }
    if s.starts_with("<false/>") {
        return Ok((PyObject::bool_val(false), new_pos + 8));
    }

    // Find tag name
    let gt = s
        .find('>')
        .ok_or_else(|| PyException::value_error("plistlib: malformed XML"))?;
    let tag = &s[1..gt];

    if tag == "integer" {
        let end = s
            .find("</integer>")
            .ok_or_else(|| PyException::value_error("plistlib: unclosed <integer>"))?;
        let val: i64 = s[gt + 1..end].trim().parse().unwrap_or(0);
        return Ok((PyObject::int(val), new_pos + end + 10));
    }
    if tag == "real" {
        let end = s
            .find("</real>")
            .ok_or_else(|| PyException::value_error("plistlib: unclosed <real>"))?;
        let val: f64 = s[gt + 1..end].trim().parse().unwrap_or(0.0);
        return Ok((PyObject::float(val), new_pos + end + 7));
    }
    if tag == "string" {
        let end = s
            .find("</string>")
            .ok_or_else(|| PyException::value_error("plistlib: unclosed <string>"))?;
        let val = s[gt + 1..end]
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">");
        return Ok((
            PyObject::str_val(CompactString::from(val)),
            new_pos + end + 9,
        ));
    }
    if tag == "data" {
        let end = s
            .find("</data>")
            .ok_or_else(|| PyException::value_error("plistlib: unclosed <data>"))?;
        let b64: String = s[gt + 1..end]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let bytes = base64_decode(&b64);
        return Ok((PyObject::bytes(bytes), new_pos + end + 7));
    }
    if tag == "date" {
        let end = s
            .find("</date>")
            .ok_or_else(|| PyException::value_error("plistlib: unclosed <date>"))?;
        let val = &s[gt + 1..end];
        return Ok((
            PyObject::str_val(CompactString::from(val.trim())),
            new_pos + end + 7,
        ));
    }
    if tag == "key" {
        let end = s
            .find("</key>")
            .ok_or_else(|| PyException::value_error("plistlib: unclosed <key>"))?;
        let val = s[gt + 1..end]
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">");
        return Ok((
            PyObject::str_val(CompactString::from(val)),
            new_pos + end + 6,
        ));
    }
    if tag == "dict" {
        let end_tag = find_closing_tag(s, "dict")?;
        let inner = &s[gt + 1..end_tag];
        let mut map = IndexMap::new();
        let mut ipos = 0;
        while ipos < inner.len() {
            let rest = inner[ipos..].trim_start();
            if rest.is_empty() || rest.starts_with("</") {
                break;
            }
            ipos = inner.len() - rest.len();
            // Parse key
            let (key_obj, next) = plist_parse_element(inner, ipos)?;
            ipos = next;
            // Parse value
            let (val_obj, next2) = plist_parse_element(inner, ipos)?;
            ipos = next2;
            let key = CompactString::from(key_obj.py_to_string());
            map.insert(HashableKey::str_key(key), val_obj);
        }
        return Ok((PyObject::dict(map), new_pos + end_tag + 7));
    }
    if tag == "array" {
        let end_tag = find_closing_tag(s, "array")?;
        let inner = &s[gt + 1..end_tag];
        let mut items = Vec::new();
        let mut ipos = 0;
        while ipos < inner.len() {
            let rest = inner[ipos..].trim_start();
            if rest.is_empty() || rest.starts_with("</") {
                break;
            }
            ipos = inner.len() - rest.len();
            let (item, next) = plist_parse_element(inner, ipos)?;
            items.push(item);
            ipos = next;
        }
        return Ok((PyObject::list(items), new_pos + end_tag + 8));
    }

    // Unknown tag — skip it
    if let Some(close) = s.find(&format!("</{}>", tag)) {
        let val = &s[gt + 1..close];
        return Ok((
            PyObject::str_val(CompactString::from(val.trim())),
            new_pos + close + tag.len() + 3,
        ));
    }

    Ok((PyObject::none(), new_pos + gt + 1))
}

/// Find closing tag position for nested XML elements
fn find_closing_tag(s: &str, tag: &str) -> PyResult<usize> {
    let open_tag = format!("<{}", tag);
    let close_tag = format!("</{}>", tag);
    let mut depth = 0;
    let mut pos = 0;
    while pos < s.len() {
        if s[pos..].starts_with(&close_tag) {
            if depth == 1 {
                return Ok(pos);
            }
            depth -= 1;
            pos += close_tag.len();
        } else if s[pos..].starts_with(&open_tag) {
            depth += 1;
            pos += open_tag.len();
        } else {
            pos += 1;
        }
    }
    Err(PyException::value_error(format!(
        "plistlib: unclosed <{}>",
        tag
    )))
}

/// Simple base64 decoder
fn base64_decode(input: &str) -> Vec<u8> {
    let mut result = Vec::new();
    let mut buf: u32 = 0;
    let mut bits = 0;
    for c in input.bytes() {
        let val = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => continue,
        };
        buf = (buf << 6) | val as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            result.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    result
}
