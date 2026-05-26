use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef};

// ── quopri module ──

pub fn create_quopri_module() -> PyObjectRef {
    make_module(
        "quopri",
        vec![
            (
                "encode",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error("encode requires input"));
                    }
                    let data = args[0].py_to_string();
                    let mut encoded = String::new();
                    for b in data.bytes() {
                        if (b == b'\t' || b == b' ' || (b >= 33 && b <= 126)) && b != b'=' {
                            encoded.push(b as char);
                        } else {
                            encoded.push_str(&format!("={:02X}", b));
                        }
                    }
                    Ok(PyObject::str_val(CompactString::from(encoded)))
                }),
            ),
            (
                "decode",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error("decode requires input"));
                    }
                    let data = args[0].py_to_string();
                    let mut decoded = Vec::new();
                    let bytes = data.as_bytes();
                    let mut i = 0;
                    while i < bytes.len() {
                        if bytes[i] == b'=' && i + 2 < bytes.len() {
                            if let Ok(val) = u8::from_str_radix(
                                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00"),
                                16,
                            ) {
                                decoded.push(val);
                                i += 3;
                                continue;
                            }
                        }
                        decoded.push(bytes[i]);
                        i += 1;
                    }
                    Ok(PyObject::str_val(CompactString::from(
                        String::from_utf8_lossy(&decoded).to_string(),
                    )))
                }),
            ),
            (
                "encodestring",
                make_builtin(|args: &[PyObjectRef]| {
                    // Alias for encode
                    if args.is_empty() {
                        return Err(PyException::type_error("encodestring requires input"));
                    }
                    let data = args[0].py_to_string();
                    let mut encoded = String::new();
                    for b in data.bytes() {
                        if (b == b'\t' || b == b' ' || (b >= 33 && b <= 126)) && b != b'=' {
                            encoded.push(b as char);
                        } else {
                            encoded.push_str(&format!("={:02X}", b));
                        }
                    }
                    Ok(PyObject::str_val(CompactString::from(encoded)))
                }),
            ),
            (
                "decodestring",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error("decodestring requires input"));
                    }
                    let data = args[0].py_to_string();
                    let mut decoded = Vec::new();
                    let bytes = data.as_bytes();
                    let mut i = 0;
                    while i < bytes.len() {
                        if bytes[i] == b'=' && i + 2 < bytes.len() {
                            if let Ok(val) = u8::from_str_radix(
                                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00"),
                                16,
                            ) {
                                decoded.push(val);
                                i += 3;
                                continue;
                            }
                        }
                        decoded.push(bytes[i]);
                        i += 1;
                    }
                    Ok(PyObject::str_val(CompactString::from(
                        String::from_utf8_lossy(&decoded).to_string(),
                    )))
                }),
            ),
        ],
    )
}
