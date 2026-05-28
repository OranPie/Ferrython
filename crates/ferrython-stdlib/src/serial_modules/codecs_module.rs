use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    check_args, check_args_min, make_builtin, make_module, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;

use super::base64_module::extract_bytes;

mod helpers;
mod io;

use helpers::{
    backslashreplace_char, cp1252_decode, cp1252_encode, decode_utf16_be, decode_utf16_le,
    decode_utf16_with_bom, decode_utf32_be, decode_utf32_le, decode_utf32_with_bom,
    normalize_encoding, punycode_adapt, punycode_digit, resolve_encoding, rot13,
    xmlcharrefreplace_char,
};
use io::codecs_open;

// ── codecs module ──────────────────────────────────────────────────
pub fn create_codecs_module() -> PyObjectRef {
    // IncrementalDecoder base class
    let inc_decoder_cls = {
        let cls = PyObject::class(
            CompactString::from("IncrementalDecoder"),
            vec![],
            IndexMap::new(),
        );
        if let PyObjectPayload::Class(ref cd) = cls.payload {
            cd.namespace.write().insert(
                CompactString::from("__init__"),
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "IncrementalDecoder.__init__ requires self",
                        ));
                    }
                    let encoding = if args.len() > 1 {
                        args[1].py_to_string()
                    } else {
                        "strict".to_string()
                    };
                    if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                        inst.attrs.write().insert(
                            CompactString::from("errors"),
                            PyObject::str_val(CompactString::from(encoding)),
                        );
                    }
                    Ok(PyObject::none())
                }),
            );
            cd.namespace.write().insert(
                CompactString::from("decode"),
                make_builtin(|_args: &[PyObjectRef]| {
                    Err(PyException::not_implemented_error(
                        "IncrementalDecoder.decode() is abstract",
                    ))
                }),
            );
            cd.namespace.write().insert(
                CompactString::from("reset"),
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
            );
            cd.namespace.write().insert(
                CompactString::from("getstate"),
                make_builtin(|_args: &[PyObjectRef]| {
                    Ok(PyObject::tuple(vec![
                        PyObject::bytes(vec![]),
                        PyObject::int(0),
                    ]))
                }),
            );
            cd.namespace.write().insert(
                CompactString::from("setstate"),
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
            );
        }
        cls
    };

    // IncrementalEncoder base class
    let inc_encoder_cls = {
        let cls = PyObject::class(
            CompactString::from("IncrementalEncoder"),
            vec![],
            IndexMap::new(),
        );
        if let PyObjectPayload::Class(ref cd) = cls.payload {
            cd.namespace.write().insert(
                CompactString::from("__init__"),
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "IncrementalEncoder.__init__ requires self",
                        ));
                    }
                    let errors = if args.len() > 1 {
                        args[1].py_to_string()
                    } else {
                        "strict".to_string()
                    };
                    if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                        inst.attrs.write().insert(
                            CompactString::from("errors"),
                            PyObject::str_val(CompactString::from(errors)),
                        );
                    }
                    Ok(PyObject::none())
                }),
            );
            cd.namespace.write().insert(
                CompactString::from("encode"),
                make_builtin(|_args: &[PyObjectRef]| {
                    Err(PyException::not_implemented_error(
                        "IncrementalEncoder.encode() is abstract",
                    ))
                }),
            );
            cd.namespace.write().insert(
                CompactString::from("reset"),
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
            );
            cd.namespace.write().insert(
                CompactString::from("getstate"),
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::int(0))),
            );
            cd.namespace.write().insert(
                CompactString::from("setstate"),
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
            );
        }
        cls
    };

    // StreamReader / StreamWriter / CodecInfo base classes (stubs)
    let stream_reader_cls =
        PyObject::class(CompactString::from("StreamReader"), vec![], IndexMap::new());
    let stream_writer_cls =
        PyObject::class(CompactString::from("StreamWriter"), vec![], IndexMap::new());
    let codec_info_cls = PyObject::class(CompactString::from("CodecInfo"), vec![], IndexMap::new());

    make_module(
        "codecs",
        vec![
            ("encode", make_builtin(codecs_encode)),
            ("decode", make_builtin(codecs_decode)),
            ("lookup", make_builtin(codecs_lookup)),
            ("getencoder", make_builtin(codecs_getencoder)),
            ("getdecoder", make_builtin(codecs_getdecoder)),
            ("getincrementaldecoder", {
                let idc = inc_decoder_cls.clone();
                PyObject::native_closure("getincrementaldecoder", move |args: &[PyObjectRef]| {
                    check_args("codecs.getincrementaldecoder", args, 1)?;
                    let _encoding = args[0].py_to_string();
                    // Return the IncrementalDecoder class (simplified — always returns base class)
                    Ok(idc.clone())
                })
            }),
            ("getincrementalencoder", {
                let iec = inc_encoder_cls.clone();
                PyObject::native_closure("getincrementalencoder", move |args: &[PyObjectRef]| {
                    check_args("codecs.getincrementalencoder", args, 1)?;
                    let _encoding = args[0].py_to_string();
                    Ok(iec.clone())
                })
            }),
            ("getreader", {
                let sr = stream_reader_cls.clone();
                PyObject::native_closure("getreader", move |args: &[PyObjectRef]| {
                    check_args("codecs.getreader", args, 1)?;
                    Ok(sr.clone())
                })
            }),
            ("getwriter", {
                let sw = stream_writer_cls.clone();
                PyObject::native_closure("getwriter", move |args: &[PyObjectRef]| {
                    check_args("codecs.getwriter", args, 1)?;
                    Ok(sw.clone())
                })
            }),
            ("utf_8_encode", make_builtin(codecs_utf8_encode)),
            ("utf_8_decode", make_builtin(codecs_utf8_decode)),
            ("IncrementalDecoder", inc_decoder_cls),
            ("IncrementalEncoder", inc_encoder_cls),
            ("StreamReader", stream_reader_cls),
            ("StreamWriter", stream_writer_cls),
            ("CodecInfo", codec_info_cls),
            (
                "register",
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
            ),
            (
                "register_error",
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
            ),
            (
                "lookup_error",
                make_builtin(|args: &[PyObjectRef]| {
                    check_args("codecs.lookup_error", args, 1)?;
                    Err(PyException::new(
                        ExceptionKind::LookupError,
                        format!("unknown error handler name '{}'", args[0].py_to_string()),
                    ))
                }),
            ),
            ("open", make_builtin(codecs_open)),
            ("BOM", PyObject::bytes(vec![0xFF, 0xFE])),
            ("BOM_UTF8", PyObject::bytes(vec![0xEF, 0xBB, 0xBF])),
            ("BOM_UTF16", PyObject::bytes(vec![0xFF, 0xFE])),
            ("BOM_UTF16_LE", PyObject::bytes(vec![0xFF, 0xFE])),
            ("BOM_UTF16_BE", PyObject::bytes(vec![0xFE, 0xFF])),
            ("BOM_UTF32", PyObject::bytes(vec![0xFF, 0xFE, 0x00, 0x00])),
            (
                "BOM_UTF32_LE",
                PyObject::bytes(vec![0xFF, 0xFE, 0x00, 0x00]),
            ),
            (
                "BOM_UTF32_BE",
                PyObject::bytes(vec![0x00, 0x00, 0xFE, 0xFF]),
            ),
            // Error handlers (CPython exposes these as module-level functions)
            (
                "strict_errors",
                PyObject::native_function("codecs.strict_errors", |args: &[PyObjectRef]| {
                    let exc = if args.is_empty() {
                        PyException::runtime_error("strict_errors")
                    } else {
                        PyException::runtime_error(args[0].py_to_string())
                    };
                    Err(exc)
                }),
            ),
            (
                "ignore_errors",
                PyObject::native_function("codecs.ignore_errors", |args: &[PyObjectRef]| {
                    // Returns (replacement, position) tuple
                    let end = if !args.is_empty() {
                        args[0]
                            .get_attr("end")
                            .and_then(|v| v.to_int().ok())
                            .unwrap_or(0)
                    } else {
                        0
                    };
                    Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from("")),
                        PyObject::int(end),
                    ]))
                }),
            ),
            (
                "replace_errors",
                PyObject::native_function("codecs.replace_errors", |args: &[PyObjectRef]| {
                    let end = if !args.is_empty() {
                        args[0]
                            .get_attr("end")
                            .and_then(|v| v.to_int().ok())
                            .unwrap_or(0)
                    } else {
                        0
                    };
                    Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from("?")),
                        PyObject::int(end),
                    ]))
                }),
            ),
            (
                "xmlcharrefreplace_errors",
                PyObject::native_function(
                    "codecs.xmlcharrefreplace_errors",
                    |args: &[PyObjectRef]| {
                        // Replace unencodable characters with XML character references
                        let (obj_str, start, end) = if !args.is_empty() {
                            let exc = &args[0];
                            let o = exc
                                .get_attr("object")
                                .map(|v| v.py_to_string())
                                .unwrap_or_default();
                            let s = exc
                                .get_attr("start")
                                .and_then(|v| v.to_int().ok())
                                .unwrap_or(0) as usize;
                            let e = exc
                                .get_attr("end")
                                .and_then(|v| v.to_int().ok())
                                .unwrap_or(0) as usize;
                            (o, s, e)
                        } else {
                            (String::new(), 0, 0)
                        };
                        let chars: Vec<char> = obj_str.chars().collect();
                        let mut replacement = String::new();
                        for i in start..end.min(chars.len()) {
                            replacement.push_str(&format!("&#{};", chars[i] as u32));
                        }
                        Ok(PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from(replacement)),
                            PyObject::int(end as i64),
                        ]))
                    },
                ),
            ),
            (
                "backslashreplace_errors",
                PyObject::native_function(
                    "codecs.backslashreplace_errors",
                    |args: &[PyObjectRef]| {
                        let (obj_str, start, end) = if !args.is_empty() {
                            let exc = &args[0];
                            let o = exc
                                .get_attr("object")
                                .map(|v| v.py_to_string())
                                .unwrap_or_default();
                            let s = exc
                                .get_attr("start")
                                .and_then(|v| v.to_int().ok())
                                .unwrap_or(0) as usize;
                            let e = exc
                                .get_attr("end")
                                .and_then(|v| v.to_int().ok())
                                .unwrap_or(0) as usize;
                            (o, s, e)
                        } else {
                            (String::new(), 0, 0)
                        };
                        let chars: Vec<char> = obj_str.chars().collect();
                        let mut replacement = String::new();
                        for i in start..end.min(chars.len()) {
                            let c = chars[i] as u32;
                            if c <= 0xFF {
                                replacement.push_str(&format!("\\x{:02x}", c));
                            } else if c <= 0xFFFF {
                                replacement.push_str(&format!("\\u{:04x}", c));
                            } else {
                                replacement.push_str(&format!("\\U{:08x}", c));
                            }
                        }
                        Ok(PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from(replacement)),
                            PyObject::int(end as i64),
                        ]))
                    },
                ),
            ),
            (
                "namereplace_errors",
                PyObject::native_function("codecs.namereplace_errors", |args: &[PyObjectRef]| {
                    let (obj_str, start, end) = if !args.is_empty() {
                        let exc = &args[0];
                        let o = exc
                            .get_attr("object")
                            .map(|v| v.py_to_string())
                            .unwrap_or_default();
                        let s = exc
                            .get_attr("start")
                            .and_then(|v| v.to_int().ok())
                            .unwrap_or(0) as usize;
                        let e = exc
                            .get_attr("end")
                            .and_then(|v| v.to_int().ok())
                            .unwrap_or(0) as usize;
                        (o, s, e)
                    } else {
                        (String::new(), 0, 0)
                    };
                    let chars: Vec<char> = obj_str.chars().collect();
                    let mut replacement = String::new();
                    for i in start..end.min(chars.len()) {
                        replacement.push_str(&format!("\\N{{{:04X}}}", chars[i] as u32));
                    }
                    Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from(replacement)),
                        PyObject::int(end as i64),
                    ]))
                }),
            ),
        ],
    )
}

fn codecs_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("codecs.encode", args, 1)?;
    let s = args[0].py_to_string();
    let encoding = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        "utf-8".to_string()
    };
    let errors = if args.len() > 2 {
        args[2].py_to_string()
    } else {
        "strict".to_string()
    };
    let norm = normalize_encoding(&encoding);
    let enc = resolve_encoding(&norm);
    match enc {
        "utf_8" => Ok(PyObject::bytes(s.as_bytes().to_vec())),
        "ascii" => {
            let mut out = Vec::new();
            for (i, c) in s.chars().enumerate() {
                if c.is_ascii() {
                    out.push(c as u8);
                } else {
                    match errors.as_str() {
                        "ignore" => {}
                        "replace" => out.push(b'?'),
                        "backslashreplace" => {
                            out.extend_from_slice(backslashreplace_char(c).as_bytes())
                        }
                        "xmlcharrefreplace" => {
                            out.extend_from_slice(xmlcharrefreplace_char(c).as_bytes())
                        }
                        _ => {
                            return Err(PyException::value_error(format!(
                                "'ascii' codec can't encode character '\\u{:04x}' in position {}",
                                c as u32, i
                            )))
                        }
                    }
                }
            }
            Ok(PyObject::bytes(out))
        }
        "latin_1" => {
            let mut out = Vec::new();
            for (i, c) in s.chars().enumerate() {
                if (c as u32) <= 255 {
                    out.push(c as u8);
                } else {
                    match errors.as_str() {
                        "ignore" => {}
                        "replace" => out.push(b'?'),
                        "backslashreplace" => {
                            out.extend_from_slice(backslashreplace_char(c).as_bytes())
                        }
                        "xmlcharrefreplace" => {
                            out.extend_from_slice(xmlcharrefreplace_char(c).as_bytes())
                        }
                        _ => {
                            return Err(PyException::value_error(format!(
                                "'latin-1' codec can't encode character '\\u{:04x}' in position {}",
                                c as u32, i
                            )))
                        }
                    }
                }
            }
            Ok(PyObject::bytes(out))
        }
        "utf_16" => {
            let mut bytes = vec![0xFF, 0xFE]; // BOM (little-endian)
            for c in s.encode_utf16() {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
            Ok(PyObject::bytes(bytes))
        }
        "utf_16_le" => {
            let bytes: Vec<u8> = s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
            Ok(PyObject::bytes(bytes))
        }
        "utf_16_be" => {
            let bytes: Vec<u8> = s.encode_utf16().flat_map(|c| c.to_be_bytes()).collect();
            Ok(PyObject::bytes(bytes))
        }
        "utf_32" => {
            let mut bytes = vec![0xFF, 0xFE, 0x00, 0x00]; // BOM (little-endian)
            for c in s.chars() {
                bytes.extend_from_slice(&(c as u32).to_le_bytes());
            }
            Ok(PyObject::bytes(bytes))
        }
        "utf_32_le" => {
            let bytes: Vec<u8> = s.chars().flat_map(|c| (c as u32).to_le_bytes()).collect();
            Ok(PyObject::bytes(bytes))
        }
        "utf_32_be" => {
            let bytes: Vec<u8> = s.chars().flat_map(|c| (c as u32).to_be_bytes()).collect();
            Ok(PyObject::bytes(bytes))
        }
        "cp1252" => {
            let mut out = Vec::new();
            for (i, c) in s.chars().enumerate() {
                match cp1252_encode(c) {
                    Ok(b) => out.push(b),
                    Err(_) => match errors.as_str() {
                        "ignore" => {}
                        "replace" => out.push(b'?'),
                        "backslashreplace" => {
                            out.extend_from_slice(backslashreplace_char(c).as_bytes())
                        }
                        "xmlcharrefreplace" => {
                            out.extend_from_slice(xmlcharrefreplace_char(c).as_bytes())
                        }
                        _ => {
                            return Err(PyException::value_error(format!(
                                "'cp1252' codec can't encode character '\\u{:04x}' in position {}",
                                c as u32, i
                            )))
                        }
                    },
                }
            }
            Ok(PyObject::bytes(out))
        }
        "rot_13" => {
            let rotated: String = s.chars().map(|c| rot13(c)).collect();
            Ok(PyObject::str_val(CompactString::from(rotated)))
        }
        "punycode" => {
            // Simple punycode encoding (RFC 3492)
            let mut output = Vec::new();
            let mut basic: Vec<char> = Vec::new();
            let mut non_basic: Vec<char> = Vec::new();
            for ch in s.chars() {
                if ch.is_ascii() {
                    basic.push(ch);
                    output.push(ch as u8);
                } else {
                    non_basic.push(ch);
                }
            }
            if !basic.is_empty() && !non_basic.is_empty() {
                output.push(b'-');
            }
            // Simplified: for non-ASCII chars, use the Punycode delta encoding
            let mut n: u32 = 128;
            let mut delta: u32 = 0;
            let mut bias: u32 = 72;
            let mut h = basic.len() as u32;
            let b_len = h;
            let all_chars: Vec<u32> = s.chars().map(|c| c as u32).collect();
            let total = all_chars.len() as u32;
            while h < total {
                let m = *all_chars.iter().filter(|&&cp| cp >= n).min().unwrap_or(&n);
                delta = delta.wrapping_add((m - n).wrapping_mul(h + 1));
                n = m;
                for &cp in &all_chars {
                    if cp < n {
                        delta = delta.wrapping_add(1);
                    }
                    if cp == n {
                        let mut q = delta;
                        let mut k = 36u32;
                        loop {
                            let t = if k <= bias {
                                1
                            } else if k >= bias + 26 {
                                26
                            } else {
                                k - bias
                            };
                            if q < t {
                                break;
                            }
                            let digit = t + (q - t) % (36 - t);
                            output.push(punycode_digit(digit));
                            q = (q - t) / (36 - t);
                            k += 36;
                        }
                        output.push(punycode_digit(q));
                        bias = punycode_adapt(delta, h + 1, h == b_len);
                        delta = 0;
                        h += 1;
                    }
                }
                delta += 1;
                n += 1;
            }
            Ok(PyObject::bytes(output))
        }
        "idna" => {
            // Simple IDNA encoding: lowercase ASCII
            let encoded = s.to_ascii_lowercase();
            Ok(PyObject::bytes(encoded.into_bytes()))
        }
        _ => Err(PyException::value_error(format!(
            "unknown encoding: {}",
            encoding
        ))),
    }
}

fn codecs_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("codecs.decode", args, 1)?;
    let encoding = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        "utf-8".to_string()
    };
    let errors = if args.len() > 2 {
        args[2].py_to_string()
    } else {
        "strict".to_string()
    };
    let norm = normalize_encoding(&encoding);
    let enc = resolve_encoding(&norm);
    // rot_13 decode works on strings
    if enc == "rot_13" {
        let s = args[0].py_to_string();
        let rotated: String = s.chars().map(|c| rot13(c)).collect();
        return Ok(PyObject::str_val(CompactString::from(rotated)));
    }
    let bytes = extract_bytes(&args[0])?;
    match enc {
        "utf_8" => match String::from_utf8(bytes.clone()) {
            Ok(s) => Ok(PyObject::str_val(CompactString::from(s))),
            Err(_) => match errors.as_str() {
                "ignore" => {
                    let s: String = bytes
                        .iter()
                        .filter(|b| b.is_ascii())
                        .map(|&b| b as char)
                        .collect();
                    Ok(PyObject::str_val(CompactString::from(s)))
                }
                "replace" => {
                    let s = String::from_utf8_lossy(&bytes).to_string();
                    Ok(PyObject::str_val(CompactString::from(s)))
                }
                _ => Err(PyException::value_error("invalid utf-8")),
            },
        },
        "ascii" => {
            let mut out = String::new();
            for (i, &b) in bytes.iter().enumerate() {
                if b <= 127 {
                    out.push(b as char);
                } else {
                    match errors.as_str() {
                        "ignore" => {}
                        "replace" => out.push('\u{FFFD}'),
                        _ => {
                            return Err(PyException::value_error(format!(
                                "'ascii' codec can't decode byte 0x{:02x} in position {}",
                                b, i
                            )))
                        }
                    }
                }
            }
            Ok(PyObject::str_val(CompactString::from(out)))
        }
        "latin_1" => {
            let s: String = bytes.iter().map(|&b| b as char).collect();
            Ok(PyObject::str_val(CompactString::from(s)))
        }
        "utf_16" => decode_utf16_with_bom(&bytes),
        "utf_16_le" => decode_utf16_le(&bytes),
        "utf_16_be" => decode_utf16_be(&bytes),
        "utf_32" => decode_utf32_with_bom(&bytes),
        "utf_32_le" => decode_utf32_le(&bytes),
        "utf_32_be" => decode_utf32_be(&bytes),
        "cp1252" => {
            let s: String = bytes.iter().map(|&b| cp1252_decode(b)).collect();
            Ok(PyObject::str_val(CompactString::from(s)))
        }
        "punycode" => {
            // Punycode decode (RFC 3492)
            let input = std::str::from_utf8(&bytes)
                .map_err(|_| PyException::value_error("punycode: invalid input"))?;
            let (basic_part, encoded_part) = if let Some(pos) = input.rfind('-') {
                (&input[..pos], &input[pos + 1..])
            } else {
                ("", input.as_ref())
            };
            let mut output: Vec<u32> = basic_part.chars().map(|c| c as u32).collect();
            let mut n: u32 = 128;
            let mut i: u32 = 0;
            let mut bias: u32 = 72;
            let encoded_bytes = encoded_part.as_bytes();
            let mut idx = 0;
            while idx < encoded_bytes.len() {
                let oldi = i;
                let mut w: u32 = 1;
                let mut k: u32 = 36;
                loop {
                    if idx >= encoded_bytes.len() {
                        break;
                    }
                    let byte = encoded_bytes[idx];
                    idx += 1;
                    let digit = if byte >= b'a' && byte <= b'z' {
                        (byte - b'a') as u32
                    } else if byte >= b'A' && byte <= b'Z' {
                        (byte - b'A') as u32
                    } else if byte >= b'0' && byte <= b'9' {
                        (byte - b'0') as u32 + 26
                    } else {
                        return Err(PyException::value_error("punycode: bad input"));
                    };
                    i = i.wrapping_add(digit.wrapping_mul(w));
                    let t = if k <= bias {
                        1
                    } else if k >= bias + 26 {
                        26
                    } else {
                        k - bias
                    };
                    if digit < t {
                        break;
                    }
                    w = w.wrapping_mul(36 - t);
                    k += 36;
                }
                let out_len = output.len() as u32 + 1;
                bias = punycode_adapt(i.wrapping_sub(oldi), out_len, oldi == 0);
                n = n.wrapping_add(i / out_len);
                i %= out_len;
                output.insert(i as usize, n);
                i += 1;
            }
            let result: String = output.iter().filter_map(|&cp| char::from_u32(cp)).collect();
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "idna" => {
            let s = std::str::from_utf8(&bytes)
                .map_err(|_| PyException::value_error("idna: invalid bytes"))?;
            Ok(PyObject::str_val(CompactString::from(s.to_string())))
        }
        _ => Err(PyException::value_error(format!(
            "unknown encoding: {}",
            encoding
        ))),
    }
}

fn codecs_lookup(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("codecs.lookup", args, 1)?;
    let norm = normalize_encoding(&args[0].py_to_string());
    let enc = resolve_encoding(&norm);
    let known = matches!(
        enc,
        "utf_8"
            | "ascii"
            | "latin_1"
            | "utf_16"
            | "utf_16_le"
            | "utf_16_be"
            | "utf_32"
            | "utf_32_le"
            | "utf_32_be"
            | "cp1252"
            | "rot_13"
            | "iso8859_2"
            | "iso8859_3"
            | "iso8859_4"
            | "iso8859_5"
            | "iso8859_6"
            | "iso8859_7"
            | "iso8859_8"
            | "iso8859_9"
            | "iso8859_10"
            | "iso8859_11"
            | "iso8859_13"
            | "iso8859_14"
            | "iso8859_15"
            | "iso8859_16"
            | "cp437"
            | "cp850"
            | "cp866"
            | "cp874"
            | "cp932"
            | "cp949"
            | "cp950"
            | "cp1250"
            | "cp1251"
            | "cp1253"
            | "cp1254"
            | "cp1255"
            | "cp1256"
            | "cp1257"
            | "cp1258"
            | "big5"
            | "big5hkscs"
            | "euc_jp"
            | "euc_kr"
            | "euc_cn"
            | "gb2312"
            | "gbk"
            | "gb18030"
            | "hz"
            | "shift_jis"
            | "shift_jis_2004"
            | "shift_jisx0213"
            | "iso2022_jp"
            | "iso2022_jp_2"
            | "iso2022_kr"
            | "iso2022_cn"
            | "koi8_r"
            | "koi8_u"
            | "koi8_t"
            | "mac_roman"
            | "mac_cyrillic"
            | "mac_greek"
            | "mac_latin2"
            | "johab"
            | "tis_620"
            | "viscii"
    );
    if known {
        // Return a CodecInfo-like object with .name attribute (CPython compat)
        let display_name = enc.replace('_', "-");
        let cls = PyObject::class(CompactString::from("CodecInfo"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        attrs.insert(
            CompactString::from("name"),
            PyObject::str_val(CompactString::from(display_name.as_str())),
        );
        attrs.insert(CompactString::from("encode"), make_builtin(codecs_encode));
        attrs.insert(CompactString::from("decode"), make_builtin(codecs_decode));
        attrs.insert(CompactString::from("incrementalencoder"), PyObject::none());
        attrs.insert(CompactString::from("incrementaldecoder"), PyObject::none());
        attrs.insert(CompactString::from("streamreader"), PyObject::none());
        attrs.insert(CompactString::from("streamwriter"), PyObject::none());
        // Also support tuple-like indexing (CPython CodecInfo is a 4-tuple subclass)
        let enc_fn = make_builtin(codecs_encode);
        let dec_fn = make_builtin(codecs_decode);
        attrs.insert(
            CompactString::from("__getitem__"),
            PyObject::native_closure("CodecInfo.__getitem__", {
                let enc2 = enc_fn.clone();
                let dec2 = dec_fn.clone();
                let name2 = CompactString::from(display_name.as_str());
                move |gargs: &[PyObjectRef]| {
                    let idx = if !gargs.is_empty() {
                        gargs[0].as_int().unwrap_or(0)
                    } else {
                        0
                    };
                    match idx {
                        0 => Ok(PyObject::str_val(name2.clone())),
                        1 => Ok(enc2.clone()),
                        2 => Ok(dec2.clone()),
                        3 => Ok(PyObject::none()),
                        _ => Err(PyException::index_error("CodecInfo index out of range")),
                    }
                }
            }),
        );
        Ok(PyObject::instance_with_attrs(cls, attrs))
    } else {
        Err(PyException::lookup_error(format!(
            "unknown encoding: {}",
            norm
        )))
    }
}

fn codecs_getencoder(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("codecs.getencoder", args, 1)?;
    Ok(make_builtin(codecs_encode))
}

fn codecs_getdecoder(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("codecs.getdecoder", args, 1)?;
    Ok(make_builtin(codecs_decode))
}

fn codecs_utf8_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("codecs.utf_8_encode", args, 1)?;
    let s = args[0].py_to_string();
    let b = s.as_bytes().to_vec();
    let len = b.len() as i64;
    Ok(PyObject::tuple(vec![
        PyObject::bytes(b),
        PyObject::int(len),
    ]))
}

fn codecs_utf8_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("codecs.utf_8_decode", args, 1)?;
    let bytes = extract_bytes(&args[0])?;
    let s =
        String::from_utf8(bytes.clone()).map_err(|_| PyException::value_error("invalid utf-8"))?;
    let len = bytes.len() as i64;
    Ok(PyObject::tuple(vec![
        PyObject::str_val(CompactString::from(s)),
        PyObject::int(len),
    ]))
}
