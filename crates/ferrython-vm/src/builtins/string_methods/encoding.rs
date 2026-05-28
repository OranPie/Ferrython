use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectRef};

pub(super) fn str_encode(s: &str, encoding: &str, errors: &str) -> PyResult<PyObjectRef> {
    match encoding {
        "utf-8" | "utf8" => Ok(PyObject::bytes(s.as_bytes().to_vec())),
        "ascii" | "us-ascii" | "us_ascii" => encode_ascii(s, errors),
        "latin-1" | "latin1" | "iso-8859-1" | "iso8859-1" => encode_latin1(s, errors),
        "utf-16" | "utf16" => {
            let mut bytes = vec![0xFF_u8, 0xFE];
            for unit in s.encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            Ok(PyObject::bytes(bytes))
        }
        "utf-16-le" | "utf16-le" | "utf-16le" | "utf16le" => {
            let bytes: Vec<u8> = s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
            Ok(PyObject::bytes(bytes))
        }
        "utf-16-be" | "utf16-be" | "utf-16be" | "utf16be" => {
            let bytes: Vec<u8> = s.encode_utf16().flat_map(|c| c.to_be_bytes()).collect();
            Ok(PyObject::bytes(bytes))
        }
        "utf-32" | "utf32" => {
            let mut bytes = vec![0xFF_u8, 0xFE, 0x00, 0x00];
            for ch in s.chars() {
                bytes.extend_from_slice(&(ch as u32).to_le_bytes());
            }
            Ok(PyObject::bytes(bytes))
        }
        "utf-32-le" | "utf32-le" | "utf-32le" | "utf32le" => {
            let bytes: Vec<u8> = s.chars().flat_map(|c| (c as u32).to_le_bytes()).collect();
            Ok(PyObject::bytes(bytes))
        }
        "utf-32-be" | "utf32-be" | "utf-32be" | "utf32be" => {
            let bytes: Vec<u8> = s.chars().flat_map(|c| (c as u32).to_be_bytes()).collect();
            Ok(PyObject::bytes(bytes))
        }
        "cp1252" | "windows-1252" | "windows1252" => encode_cp1252(s, errors),
        "punycode" => super::punycode_encode_str(s),
        "idna" => Ok(PyObject::bytes(s.to_ascii_lowercase().into_bytes())),
        _ => Err(PyException::value_error(format!(
            "unknown encoding: {}",
            encoding
        ))),
    }
}

fn encode_ascii(s: &str, errors: &str) -> PyResult<PyObjectRef> {
    let mut result = Vec::new();
    for ch in s.chars() {
        if ch.is_ascii() {
            result.push(ch as u8);
        } else if errors == "surrogateescape" && (ch as u32) >= 0x80 && (ch as u32) <= 0xFF {
            result.push(ch as u8);
        } else {
            match errors {
                "ignore" => {}
                "replace" => result.push(b'?'),
                "xmlcharrefreplace" => {
                    result.extend_from_slice(format!("&#{};", ch as u32).as_bytes());
                }
                _ => {
                    return Err(PyException::new(
                        ExceptionKind::UnicodeEncodeError,
                        format!(
                            "'ascii' codec can't encode character '\\u{:04x}' in position",
                            ch as u32
                        ),
                    ));
                }
            }
        }
    }
    Ok(PyObject::bytes(result))
}

fn encode_latin1(s: &str, errors: &str) -> PyResult<PyObjectRef> {
    let mut result = Vec::new();
    for ch in s.chars() {
        if (ch as u32) <= 0xFF {
            result.push(ch as u8);
        } else {
            match errors {
                "ignore" => {}
                "replace" => result.push(b'?'),
                _ => {
                    return Err(PyException::new(
                        ExceptionKind::UnicodeEncodeError,
                        format!(
                            "'latin-1' codec can't encode character '\\u{:04x}'",
                            ch as u32
                        ),
                    ));
                }
            }
        }
    }
    Ok(PyObject::bytes(result))
}

fn encode_cp1252(s: &str, errors: &str) -> PyResult<PyObjectRef> {
    let mut result = Vec::new();
    for ch in s.chars() {
        let u = ch as u32;
        if u < 0x80 || (0xA0..=0xFF).contains(&u) {
            result.push(u as u8);
        } else {
            let byte = match u {
                0x20AC => Some(0x80u8),
                0x201A => Some(0x82),
                0x0192 => Some(0x83),
                0x201E => Some(0x84),
                0x2026 => Some(0x85),
                0x2020 => Some(0x86),
                0x2021 => Some(0x87),
                0x02C6 => Some(0x88),
                0x2030 => Some(0x89),
                0x0160 => Some(0x8A),
                0x2039 => Some(0x8B),
                0x0152 => Some(0x8C),
                0x017D => Some(0x8E),
                0x2018 => Some(0x91),
                0x2019 => Some(0x92),
                0x201C => Some(0x93),
                0x201D => Some(0x94),
                0x2022 => Some(0x95),
                0x2013 => Some(0x96),
                0x2014 => Some(0x97),
                0x02DC => Some(0x98),
                0x2122 => Some(0x99),
                0x0161 => Some(0x9A),
                0x203A => Some(0x9B),
                0x0153 => Some(0x9C),
                0x017E => Some(0x9E),
                0x0178 => Some(0x9F),
                _ => None,
            };
            match byte {
                Some(b) => result.push(b),
                None => match errors {
                    "ignore" => {}
                    "replace" => result.push(b'?'),
                    _ => {
                        return Err(PyException::new(
                            ExceptionKind::UnicodeEncodeError,
                            format!("'cp1252' codec can't encode character '\\u{:04x}'", u),
                        ));
                    }
                },
            }
        }
    }
    Ok(PyObject::bytes(result))
}
