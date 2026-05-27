//! Bytes and bytearray method dispatch.

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::helpers::{
    consume_bytearray_export, index_to_i64, index_to_usize_repeat,
};
use ferrython_core::object::{
    check_args_min, checked_repeat_len, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;

fn bytes_like_data(obj: &PyObjectRef) -> Option<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Some((**b).clone()),
        PyObjectPayload::Instance(inst) => {
            if inst.attrs.read().contains_key("__memoryview__") {
                if let Some(base) = inst.attrs.read().get("obj").cloned() {
                    return bytes_like_data(&base);
                }
            }
            if let Some(value) = inst.attrs.read().get("__builtin_value__").cloned() {
                return bytes_like_data(&value);
            }
            None
        }
        _ => None,
    }
}

fn bytes_fill_arg(obj: &PyObjectRef) -> PyResult<u8> {
    let data = bytes_like_data(obj)
        .ok_or_else(|| PyException::type_error("fill character must be a bytes-like object"))?;
    if data.len() != 1 {
        return Err(PyException::type_error(
            "fill character must be exactly one byte",
        ));
    }
    Ok(data[0])
}

fn bytes_search_bounds(
    len: usize,
    args: &[PyObjectRef],
    method: &str,
) -> PyResult<(usize, usize, bool)> {
    if args.len() > 3 {
        return Err(PyException::type_error(format!(
            "{}() takes at most 3 arguments ({} given)",
            method,
            args.len()
        )));
    }
    let len_i = len as i64;
    let raw_start = if args.len() > 1 && !matches!(args[1].payload, PyObjectPayload::None) {
        Some(args[1].to_int()?)
    } else {
        None
    };
    let raw_stop = if args.len() > 2 && !matches!(args[2].payload, PyObjectPayload::None) {
        Some(args[2].to_int()?)
    } else {
        None
    };
    let normalize = |idx: i64| -> usize {
        let bounded = if idx < 0 {
            (len_i + idx).max(0)
        } else {
            idx.min(len_i)
        };
        bounded as usize
    };
    let start = raw_start.map(normalize).unwrap_or(0);
    let stop = raw_stop.map(normalize).unwrap_or(len);
    let start_beyond = raw_start.is_some_and(|idx| idx > len_i);
    Ok((start, stop, start_beyond))
}

fn bytes_search_subslice<'a>(
    b: &'a [u8],
    args: &[PyObjectRef],
    method: &str,
) -> PyResult<(usize, &'a [u8], bool)> {
    let (start, stop, start_beyond) = bytes_search_bounds(b.len(), args, method)?;
    let empty_match = !start_beyond && start <= stop;
    if start < stop {
        Ok((start, &b[start..stop], empty_match))
    } else {
        Ok((start, &b[0..0], empty_match))
    }
}

fn bytes_int_arg(obj: &PyObjectRef) -> PyResult<Option<u8>> {
    let value = match &obj.payload {
        PyObjectPayload::Int(n) => n
            .to_i64()
            .ok_or_else(|| PyException::value_error("byte must be in range(0, 256)"))?,
        _ => {
            let Some(value) = obj.as_int() else {
                return Ok(None);
            };
            value
        }
    };
    if !(0..=255).contains(&value) {
        return Err(PyException::value_error("byte must be in range(0, 256)"));
    }
    Ok(Some(value as u8))
}

fn bytes_find_slice(haystack: &[u8], needle: &[u8], empty_match: bool) -> Option<usize> {
    if needle.is_empty() {
        empty_match.then_some(0)
    } else {
        haystack.windows(needle.len()).position(|w| w == needle)
    }
}

fn bytes_rfind_slice(haystack: &[u8], needle: &[u8], empty_match: bool) -> Option<usize> {
    if needle.is_empty() {
        empty_match.then_some(haystack.len())
    } else {
        haystack.windows(needle.len()).rposition(|w| w == needle)
    }
}

// ── UTF-16/32 decode helpers ──────────────────────────────────────

fn decode_utf16_le_bytes(b: &[u8]) -> PyResult<PyObjectRef> {
    if b.len() % 2 != 0 {
        return Err(PyException::value_error("utf-16-le: truncated data"));
    }
    let u16s: Vec<u16> = b
        .chunks(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let s = String::from_utf16(&u16s).map_err(|_| PyException::value_error("invalid utf-16-le"))?;
    Ok(PyObject::str_val(CompactString::from(s)))
}

fn decode_utf16_be_bytes(b: &[u8]) -> PyResult<PyObjectRef> {
    if b.len() % 2 != 0 {
        return Err(PyException::value_error("utf-16-be: truncated data"));
    }
    let u16s: Vec<u16> = b
        .chunks(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    let s = String::from_utf16(&u16s).map_err(|_| PyException::value_error("invalid utf-16-be"))?;
    Ok(PyObject::str_val(CompactString::from(s)))
}

fn decode_utf32_le_bytes(b: &[u8]) -> PyResult<PyObjectRef> {
    if b.len() % 4 != 0 {
        return Err(PyException::value_error("utf-32-le: truncated data"));
    }
    let s: Result<String, _> = b
        .chunks(4)
        .map(|c| {
            let cp = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
            char::from_u32(cp)
                .ok_or_else(|| PyException::value_error("invalid utf-32-le codepoint"))
        })
        .collect();
    Ok(PyObject::str_val(CompactString::from(s?)))
}

fn decode_utf32_be_bytes(b: &[u8]) -> PyResult<PyObjectRef> {
    if b.len() % 4 != 0 {
        return Err(PyException::value_error("utf-32-be: truncated data"));
    }
    let s: Result<String, _> = b
        .chunks(4)
        .map(|c| {
            let cp = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
            char::from_u32(cp)
                .ok_or_else(|| PyException::value_error("invalid utf-32-be codepoint"))
        })
        .collect();
    Ok(PyObject::str_val(CompactString::from(s?)))
}

pub(crate) fn call_bytes_method(
    b: &[u8],
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match method {
        "__getitem__" => {
            check_args_min("bytes.__getitem__", args, 1)?;
            let idx = index_to_i64(&args[0]).map_err(|e| {
                if e.kind == ExceptionKind::TypeError {
                    PyException::type_error("byte indices must be integers or slices")
                } else {
                    e
                }
            })?;
            let len = b.len() as i64;
            let actual = if idx < 0 { len + idx } else { idx };
            if actual < 0 || actual >= len {
                return Err(PyException::index_error("index out of range"));
            }
            Ok(PyObject::int(b[actual as usize] as i64))
        }
        "__mul__" | "__rmul__" => {
            check_args_min("bytes.__mul__", args, 1)?;
            let n = index_to_usize_repeat(&args[0])?;
            let size = checked_repeat_len(b.len(), n, "bytes repeat")?;
            let mut result = Vec::with_capacity(size);
            for _ in 0..n {
                result.extend_from_slice(b);
            }
            Ok(PyObject::bytes(result))
        }
        "__rmod__" => Ok(PyObject::not_implemented()),
        "decode" => {
            let encoding = if !args.is_empty() {
                args[0].py_to_string().to_lowercase()
            } else {
                "utf-8".to_string()
            };
            let errors = if args.len() > 1 {
                args[1].py_to_string()
            } else {
                "strict".to_string()
            };
            match encoding.as_str() {
                "utf-8" | "utf8" => match errors.as_str() {
                    "strict" => match std::str::from_utf8(b) {
                        Ok(s) => Ok(PyObject::str_val(CompactString::from(s))),
                        Err(e) => Err(PyException::new(
                            ExceptionKind::UnicodeDecodeError,
                            format!(
                                "'utf-8' codec can't decode byte 0x{:02x} in position {}",
                                b[e.valid_up_to()],
                                e.valid_up_to()
                            ),
                        )),
                    },
                    "ignore" => {
                        let s: String = b
                            .iter()
                            .filter(|&&x| x < 0x80)
                            .map(|&x| x as char)
                            .collect();
                        Ok(PyObject::str_val(CompactString::from(s)))
                    }
                    "replace" | _ => Ok(PyObject::str_val(CompactString::from(
                        String::from_utf8_lossy(b),
                    ))),
                },
                "ascii" | "us-ascii" | "us_ascii" | "iso646-us" | "iso_646.irv_1991" => {
                    match errors.as_str() {
                        "strict" => {
                            for (i, &byte) in b.iter().enumerate() {
                                if byte > 127 {
                                    return Err(PyException::new(
                                        ExceptionKind::UnicodeDecodeError,
                                        format!("'ascii' codec can't decode byte 0x{:02x} in position {}", byte, i),
                                    ));
                                }
                            }
                            Ok(PyObject::str_val(CompactString::from(
                                String::from_utf8_lossy(b),
                            )))
                        }
                        "ignore" => {
                            let s: String =
                                b.iter().filter(|&&x| x < 128).map(|&x| x as char).collect();
                            Ok(PyObject::str_val(CompactString::from(s)))
                        }
                        "surrogateescape" => {
                            // Rust String can't hold lone surrogates, so we use a
                            // lossless fallback: bytes >= 128 map to U+0080..U+00FF.
                            // The matching encode side must reverse this mapping.
                            let s: String = b.iter().map(|&x| x as char).collect();
                            Ok(PyObject::str_val(CompactString::from(s)))
                        }
                        "replace" | _ => {
                            let s: String = b
                                .iter()
                                .map(|&x| if x < 128 { x as char } else { '\u{FFFD}' })
                                .collect();
                            Ok(PyObject::str_val(CompactString::from(s)))
                        }
                    }
                }
                "latin-1" | "latin1" | "iso-8859-1" | "iso8859-1" => {
                    let s: String = b.iter().map(|&x| x as char).collect();
                    Ok(PyObject::str_val(CompactString::from(s)))
                }
                "utf-16" | "utf16" => {
                    // Auto-detect BOM
                    if b.len() >= 2 && b[0] == 0xFF && b[1] == 0xFE {
                        decode_utf16_le_bytes(&b[2..])
                    } else if b.len() >= 2 && b[0] == 0xFE && b[1] == 0xFF {
                        decode_utf16_be_bytes(&b[2..])
                    } else {
                        decode_utf16_le_bytes(b)
                    }
                }
                "utf-16-le" | "utf16-le" | "utf-16le" | "utf16le" => decode_utf16_le_bytes(b),
                "utf-16-be" | "utf16-be" | "utf-16be" | "utf16be" => decode_utf16_be_bytes(b),
                "utf-32" | "utf32" => {
                    if b.len() >= 4 && b[..4] == [0xFF, 0xFE, 0x00, 0x00] {
                        decode_utf32_le_bytes(&b[4..])
                    } else if b.len() >= 4 && b[..4] == [0x00, 0x00, 0xFE, 0xFF] {
                        decode_utf32_be_bytes(&b[4..])
                    } else {
                        decode_utf32_le_bytes(b)
                    }
                }
                "utf-32-le" | "utf32-le" | "utf-32le" | "utf32le" => decode_utf32_le_bytes(b),
                "utf-32-be" | "utf32-be" | "utf-32be" | "utf32be" => decode_utf32_be_bytes(b),
                "cp1252" | "windows-1252" | "windows1252" => {
                    let s: String = b
                        .iter()
                        .map(|&byte| {
                            if byte < 0x80 || byte >= 0xA0 {
                                return byte as char;
                            }
                            match byte {
                                0x80 => '\u{20AC}',
                                0x82 => '\u{201A}',
                                0x83 => '\u{0192}',
                                0x84 => '\u{201E}',
                                0x85 => '\u{2026}',
                                0x86 => '\u{2020}',
                                0x87 => '\u{2021}',
                                0x88 => '\u{02C6}',
                                0x89 => '\u{2030}',
                                0x8A => '\u{0160}',
                                0x8B => '\u{2039}',
                                0x8C => '\u{0152}',
                                0x8E => '\u{017D}',
                                0x91 => '\u{2018}',
                                0x92 => '\u{2019}',
                                0x93 => '\u{201C}',
                                0x94 => '\u{201D}',
                                0x95 => '\u{2022}',
                                0x96 => '\u{2013}',
                                0x97 => '\u{2014}',
                                0x98 => '\u{02DC}',
                                0x99 => '\u{2122}',
                                0x9A => '\u{0161}',
                                0x9B => '\u{203A}',
                                0x9C => '\u{0153}',
                                0x9E => '\u{017E}',
                                0x9F => '\u{0178}',
                                _ => '\u{FFFD}',
                            }
                        })
                        .collect();
                    Ok(PyObject::str_val(CompactString::from(s)))
                }
                "punycode" => crate::builtins::string_methods::punycode_decode_bytes(b),
                "idna" => {
                    let s = std::str::from_utf8(b)
                        .map_err(|_| PyException::value_error("idna: invalid bytes"))?;
                    Ok(PyObject::str_val(CompactString::from(s.to_string())))
                }
                _ => Err(PyException::new(
                    ExceptionKind::LookupError,
                    format!("unknown encoding: {}", encoding),
                )),
            }
        }
        "hex" => {
            // bytes.hex([sep[, bytes_per_sep]])
            let hex_str = hex::encode(b);
            if args.is_empty() {
                Ok(PyObject::str_val(CompactString::from(hex_str)))
            } else {
                let sep = match &args[0].payload {
                    PyObjectPayload::Str(s) => {
                        let mut chars = s.chars();
                        let Some(ch) = chars.next() else {
                            return Err(PyException::value_error("sep must be length 1"));
                        };
                        if chars.next().is_some() || !ch.is_ascii() {
                            return Err(PyException::value_error("sep must be ASCII and length 1"));
                        }
                        ch.to_string()
                    }
                    PyObjectPayload::Bytes(sep) | PyObjectPayload::ByteArray(sep) => {
                        if sep.len() != 1 || !sep[0].is_ascii() {
                            return Err(PyException::value_error("sep must be ASCII and length 1"));
                        }
                        (sep[0] as char).to_string()
                    }
                    _ => return Err(PyException::type_error("sep must be str or bytes")),
                };
                let group = if args.len() > 1 { args[1].to_int()? } else { 1 };
                if group == 0 || b.is_empty() {
                    return Ok(PyObject::str_val(CompactString::from(hex_str)));
                }
                let group_abs = group.unsigned_abs() as usize;
                let mut result = String::new();
                for i in 0..b.len() {
                    if i > 0 {
                        let insert = if group > 0 {
                            (b.len() - i) % group_abs == 0
                        } else {
                            i % group_abs == 0
                        };
                        if insert {
                            result.push_str(&sep);
                        }
                    }
                    result.push_str(&hex_str[i * 2..i * 2 + 2]);
                }
                Ok(PyObject::str_val(CompactString::from(result)))
            }
        }
        "count" => {
            if args.is_empty() {
                return Err(PyException::type_error("count requires an argument"));
            }
            let (_, slice, empty_match) = bytes_search_subslice(b, args, "count")?;
            if let Some(byte) = bytes_int_arg(&args[0])? {
                return Ok(PyObject::int(
                    slice.iter().filter(|&&x| x == byte).count() as i64
                ));
            }
            match &args[0].payload {
                PyObjectPayload::Bytes(needle) | PyObjectPayload::ByteArray(needle) => {
                    if needle.is_empty() {
                        Ok(PyObject::int(if empty_match {
                            slice.len() as i64 + 1
                        } else {
                            0
                        }))
                    } else {
                        let mut count = 0i64;
                        let mut start = 0;
                        while start + needle.len() <= slice.len() {
                            if &slice[start..start + needle.len()] == needle.as_slice() {
                                count += 1;
                                start += needle.len();
                            } else {
                                start += 1;
                            }
                        }
                        Ok(PyObject::int(count))
                    }
                }
                _ => Err(PyException::type_error("a bytes-like object is required")),
            }
        }
        "find" => {
            if args.is_empty() {
                return Err(PyException::type_error("find requires an argument"));
            }
            let (base, slice, empty_match) = bytes_search_subslice(b, args, "find")?;
            if let PyObjectPayload::Bytes(needle) | PyObjectPayload::ByteArray(needle) =
                &args[0].payload
            {
                let pos = bytes_find_slice(slice, needle.as_slice(), empty_match);
                Ok(PyObject::int(pos.map(|p| (base + p) as i64).unwrap_or(-1)))
            } else if let Some(byte) = bytes_int_arg(&args[0])? {
                Ok(PyObject::int(
                    slice
                        .iter()
                        .position(|&x| x == byte)
                        .map(|p| (base + p) as i64)
                        .unwrap_or(-1),
                ))
            } else {
                Err(PyException::type_error(format!(
                    "startswith first arg must be bytes or a tuple of bytes, not {}",
                    args[0].type_name()
                )))
            }
        }
        "startswith" => {
            if args.is_empty() {
                return Err(PyException::type_error("startswith requires an argument"));
            }
            if let PyObjectPayload::Bytes(prefix) | PyObjectPayload::ByteArray(prefix) =
                &args[0].payload
            {
                let (_, slice, empty_match) = bytes_search_subslice(b, args, "startswith")?;
                Ok(PyObject::bool_val(if prefix.is_empty() {
                    empty_match
                } else {
                    slice.starts_with(prefix)
                }))
            } else {
                Err(PyException::type_error(format!(
                    "endswith first arg must be bytes or a tuple of bytes, not {}",
                    args[0].type_name()
                )))
            }
        }
        "endswith" => {
            if args.is_empty() {
                return Err(PyException::type_error("endswith requires an argument"));
            }
            if let PyObjectPayload::Bytes(suffix) | PyObjectPayload::ByteArray(suffix) =
                &args[0].payload
            {
                let (_, slice, empty_match) = bytes_search_subslice(b, args, "endswith")?;
                Ok(PyObject::bool_val(if suffix.is_empty() {
                    empty_match
                } else {
                    slice.ends_with(suffix)
                }))
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "upper" => Ok(PyObject::bytes(b.to_ascii_uppercase())),
        "lower" => Ok(PyObject::bytes(b.to_ascii_lowercase())),
        "strip" => {
            let chars = if args.is_empty() || matches!(args[0].payload, PyObjectPayload::None) {
                None
            } else {
                Some(bytes_like_data(&args[0]).ok_or_else(|| {
                    PyException::type_error("a bytes-like object is required, not str")
                })?)
            };
            let stripped = b
                .iter()
                .copied()
                .skip_while(|c| {
                    chars
                        .as_ref()
                        .map(|bytes| bytes.contains(c))
                        .unwrap_or_else(|| c.is_ascii_whitespace())
                })
                .collect::<Vec<u8>>();
            let stripped: Vec<u8> = stripped
                .into_iter()
                .rev()
                .skip_while(|c| {
                    chars
                        .as_ref()
                        .map(|bytes| bytes.contains(c))
                        .unwrap_or_else(|| c.is_ascii_whitespace())
                })
                .collect::<Vec<u8>>()
                .into_iter()
                .rev()
                .collect();
            Ok(PyObject::bytes(stripped))
        }
        "split" => {
            if args.is_empty() {
                // Split on whitespace
                let parts: Vec<PyObjectRef> = String::from_utf8_lossy(b)
                    .split_whitespace()
                    .map(|s| PyObject::bytes(s.as_bytes().to_vec()))
                    .collect();
                Ok(PyObject::list(parts))
            } else if let PyObjectPayload::Bytes(sep) | PyObjectPayload::ByteArray(sep) =
                &args[0].payload
            {
                if sep.is_empty() {
                    return Err(PyException::value_error("empty separator"));
                }
                let mut parts = Vec::new();
                let mut start = 0;
                while start <= b.len() {
                    if let Some(pos) = b[start..]
                        .windows(sep.len())
                        .position(|w| w == sep.as_slice())
                    {
                        parts.push(PyObject::bytes(b[start..start + pos].to_vec()));
                        start = start + pos + sep.len();
                    } else {
                        parts.push(PyObject::bytes(b[start..].to_vec()));
                        break;
                    }
                }
                Ok(PyObject::list(parts))
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "join" => {
            if args.len() != 1 {
                return Err(PyException::type_error("join requires an argument"));
            }
            // Extract items from list, tuple, or other sequence types.
            // VM dispatch normalizes lazy iterables to a list before reaching this helper.
            let items: Vec<PyObjectRef> = match &args[0].payload {
                PyObjectPayload::List(items) => items.read().clone(),
                PyObjectPayload::Tuple(items) => (**items).clone(),
                PyObjectPayload::FrozenSet(items) => items.values().cloned().collect(),
                PyObjectPayload::Set(items) => items.read().values().cloned().collect(),
                _ => return Err(PyException::type_error("can only join an iterable")),
            };
            let mut result = Vec::new();
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    result.extend_from_slice(b);
                }
                match &item.payload {
                    PyObjectPayload::Bytes(ib) => result.extend_from_slice(ib),
                    PyObjectPayload::ByteArray(ib) => result.extend_from_slice(ib),
                    PyObjectPayload::Instance(_) => {
                        if let Some(data) = bytes_like_data(item) {
                            result.extend_from_slice(&data);
                        } else {
                            return Err(PyException::type_error(
                                "sequence item: expected a bytes-like object",
                            ));
                        }
                    }
                    _ => {
                        return Err(PyException::type_error(
                            "sequence item: expected a bytes-like object",
                        ))
                    }
                }
            }
            Ok(PyObject::bytes(result))
        }
        "replace" => {
            if args.len() < 2 {
                return Err(PyException::type_error("replace requires 2 arguments"));
            }
            if let (
                PyObjectPayload::Bytes(old) | PyObjectPayload::ByteArray(old),
                PyObjectPayload::Bytes(new) | PyObjectPayload::ByteArray(new),
            ) = (&args[0].payload, &args[1].payload)
            {
                let s = String::from_utf8_lossy(b);
                let old_s = String::from_utf8_lossy(old);
                let new_s = String::from_utf8_lossy(new);
                Ok(PyObject::bytes(
                    s.replace(old_s.as_ref(), new_s.as_ref()).into_bytes(),
                ))
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "isdigit" => Ok(PyObject::bool_val(
            !b.is_empty() && b.iter().all(|c| c.is_ascii_digit()),
        )),
        "isalpha" => Ok(PyObject::bool_val(
            !b.is_empty() && b.iter().all(|c| c.is_ascii_alphabetic()),
        )),
        "isalnum" => Ok(PyObject::bool_val(
            !b.is_empty() && b.iter().all(|c| c.is_ascii_alphanumeric()),
        )),
        "isspace" => Ok(PyObject::bool_val(
            !b.is_empty() && b.iter().all(|c| c.is_ascii_whitespace()),
        )),
        "islower" => Ok(PyObject::bool_val(
            b.iter().any(|c| c.is_ascii_lowercase()) && b.iter().all(|c| !c.is_ascii_uppercase()),
        )),
        "isupper" => Ok(PyObject::bool_val(
            b.iter().any(|c| c.is_ascii_uppercase()) && b.iter().all(|c| !c.is_ascii_lowercase()),
        )),
        "istitle" => {
            let s = String::from_utf8_lossy(b);
            let mut prev_cased = false;
            let mut found_cased = false;
            let mut is_title = true;
            for c in s.chars() {
                if c.is_uppercase() {
                    if prev_cased {
                        is_title = false;
                        break;
                    }
                    prev_cased = true;
                    found_cased = true;
                } else if c.is_lowercase() {
                    if !prev_cased {
                        is_title = false;
                        break;
                    }
                    prev_cased = true;
                    found_cased = true;
                } else {
                    prev_cased = false;
                }
            }
            Ok(PyObject::bool_val(found_cased && is_title))
        }
        "swapcase" => Ok(PyObject::bytes(
            b.iter()
                .map(|&c| {
                    if c.is_ascii_lowercase() {
                        c.to_ascii_uppercase()
                    } else if c.is_ascii_uppercase() {
                        c.to_ascii_lowercase()
                    } else {
                        c
                    }
                })
                .collect(),
        )),
        "title" => {
            let mut result = Vec::with_capacity(b.len());
            let mut prev_alpha = false;
            for &c in b {
                if c.is_ascii_alphabetic() {
                    if !prev_alpha {
                        result.push(c.to_ascii_uppercase());
                    } else {
                        result.push(c.to_ascii_lowercase());
                    }
                    prev_alpha = true;
                } else {
                    result.push(c);
                    prev_alpha = false;
                }
            }
            Ok(PyObject::bytes(result))
        }
        "capitalize" => {
            if b.is_empty() {
                return Ok(PyObject::bytes(vec![]));
            }
            let mut result = vec![b[0].to_ascii_uppercase()];
            result.extend(b[1..].iter().map(|c| c.to_ascii_lowercase()));
            Ok(PyObject::bytes(result))
        }
        "center" => {
            if args.is_empty() {
                return Err(PyException::type_error("center requires width argument"));
            }
            let width = args[0].to_int()? as usize;
            let fill = if args.len() > 1 {
                bytes_fill_arg(&args[1])?
            } else {
                b' '
            };
            if b.len() >= width {
                return Ok(PyObject::bytes(b.to_vec()));
            }
            let pad = width - b.len();
            let left = pad / 2;
            let right = pad - left;
            let mut result = vec![fill; left];
            result.extend_from_slice(b);
            result.extend(vec![fill; right]);
            Ok(PyObject::bytes(result))
        }
        "ljust" => {
            if args.is_empty() {
                return Err(PyException::type_error("ljust requires width argument"));
            }
            let width = args[0].to_int()? as usize;
            let fill = if args.len() > 1 {
                bytes_fill_arg(&args[1])?
            } else {
                b' '
            };
            if b.len() >= width {
                return Ok(PyObject::bytes(b.to_vec()));
            }
            let mut result = b.to_vec();
            result.extend(vec![fill; width - b.len()]);
            Ok(PyObject::bytes(result))
        }
        "rjust" => {
            if args.is_empty() {
                return Err(PyException::type_error("rjust requires width argument"));
            }
            let width = args[0].to_int()? as usize;
            let fill = if args.len() > 1 {
                bytes_fill_arg(&args[1])?
            } else {
                b' '
            };
            if b.len() >= width {
                return Ok(PyObject::bytes(b.to_vec()));
            }
            let mut result = vec![fill; width - b.len()];
            result.extend_from_slice(b);
            Ok(PyObject::bytes(result))
        }
        "lstrip" => {
            let chars = if args.is_empty() || matches!(args[0].payload, PyObjectPayload::None) {
                None
            } else {
                Some(bytes_like_data(&args[0]).ok_or_else(|| {
                    PyException::type_error("a bytes-like object is required, not str")
                })?)
            };
            let stripped: Vec<u8> = b
                .iter()
                .copied()
                .skip_while(|c| {
                    chars
                        .as_ref()
                        .map(|bytes| bytes.contains(c))
                        .unwrap_or_else(|| c.is_ascii_whitespace())
                })
                .collect();
            Ok(PyObject::bytes(stripped))
        }
        "rstrip" => {
            let mut result = b.to_vec();
            let chars = if args.is_empty() || matches!(args[0].payload, PyObjectPayload::None) {
                None
            } else {
                Some(bytes_like_data(&args[0]).ok_or_else(|| {
                    PyException::type_error("a bytes-like object is required, not str")
                })?)
            };
            while result.last().map_or(false, |c| {
                chars
                    .as_ref()
                    .map(|bytes| bytes.contains(c))
                    .unwrap_or_else(|| c.is_ascii_whitespace())
            }) {
                result.pop();
            }
            Ok(PyObject::bytes(result))
        }
        "rfind" => {
            if args.is_empty() {
                return Err(PyException::type_error("rfind requires an argument"));
            }
            let (base, slice, empty_match) = bytes_search_subslice(b, args, "rfind")?;
            if let PyObjectPayload::Bytes(needle) | PyObjectPayload::ByteArray(needle) =
                &args[0].payload
            {
                let pos = bytes_rfind_slice(slice, needle.as_slice(), empty_match);
                Ok(PyObject::int(pos.map(|p| (base + p) as i64).unwrap_or(-1)))
            } else if let Some(byte) = bytes_int_arg(&args[0])? {
                Ok(PyObject::int(
                    slice
                        .iter()
                        .rposition(|&x| x == byte)
                        .map(|p| (base + p) as i64)
                        .unwrap_or(-1),
                ))
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "index" => {
            if args.is_empty() {
                return Err(PyException::type_error("index requires an argument"));
            }
            let (base, slice, empty_match) = bytes_search_subslice(b, args, "index")?;
            if let Some(byte_val) = bytes_int_arg(&args[0])? {
                // int arg: search for single byte value
                match slice.iter().position(|&x| x == byte_val) {
                    Some(p) => Ok(PyObject::int((base + p) as i64)),
                    None => Err(PyException::value_error("subsection not found")),
                }
            } else if let PyObjectPayload::Bytes(needle) | PyObjectPayload::ByteArray(needle) =
                &args[0].payload
            {
                let pos = bytes_find_slice(slice, needle.as_slice(), empty_match);
                match pos {
                    Some(p) => Ok(PyObject::int((base + p) as i64)),
                    None => Err(PyException::value_error("subsection not found")),
                }
            } else {
                Err(PyException::type_error(
                    "a bytes-like object or int is required",
                ))
            }
        }
        "rindex" => {
            if args.is_empty() {
                return Err(PyException::type_error("rindex requires an argument"));
            }
            let (base, slice, empty_match) = bytes_search_subslice(b, args, "rindex")?;
            if let PyObjectPayload::Bytes(needle) | PyObjectPayload::ByteArray(needle) =
                &args[0].payload
            {
                let pos = bytes_rfind_slice(slice, needle.as_slice(), empty_match);
                match pos {
                    Some(p) => Ok(PyObject::int((base + p) as i64)),
                    None => Err(PyException::value_error("subsection not found")),
                }
            } else if let Some(byte) = bytes_int_arg(&args[0])? {
                match slice.iter().rposition(|&x| x == byte) {
                    Some(p) => Ok(PyObject::int((base + p) as i64)),
                    None => Err(PyException::value_error("subsection not found")),
                }
            } else {
                Err(PyException::type_error(
                    "a bytes-like object or int is required",
                ))
            }
        }
        "zfill" => {
            if args.is_empty() {
                return Err(PyException::type_error("zfill requires width argument"));
            }
            let width = args[0].to_int()? as usize;
            if b.len() >= width {
                return Ok(PyObject::bytes(b.to_vec()));
            }
            let pad = width - b.len();
            let mut result = vec![b'0'; pad];
            result.extend_from_slice(b);
            Ok(PyObject::bytes(result))
        }
        "expandtabs" => {
            let tabsize = if !args.is_empty() {
                args[0].to_int()? as usize
            } else {
                8
            };
            let mut result = Vec::new();
            let mut col = 0;
            for &byte in b {
                if byte == b'\t' {
                    let spaces = tabsize - (col % tabsize);
                    result.extend(std::iter::repeat(b' ').take(spaces));
                    col += spaces;
                } else if byte == b'\n' || byte == b'\r' {
                    result.push(byte);
                    col = 0;
                } else {
                    result.push(byte);
                    col += 1;
                }
            }
            Ok(PyObject::bytes(result))
        }
        "isascii" => Ok(PyObject::bool_val(b.iter().all(|c| c.is_ascii()))),
        "partition" => {
            if args.is_empty() {
                return Err(PyException::type_error("partition requires an argument"));
            }
            if let PyObjectPayload::Bytes(sep) | PyObjectPayload::ByteArray(sep) = &args[0].payload
            {
                if sep.is_empty() {
                    return Err(PyException::value_error("empty separator"));
                }
                if let Some(pos) = bytes_find_slice(b, sep.as_slice(), true) {
                    Ok(PyObject::tuple(vec![
                        PyObject::bytes(b[..pos].to_vec()),
                        PyObject::bytes((**sep).clone()),
                        PyObject::bytes(b[pos + sep.len()..].to_vec()),
                    ]))
                } else {
                    Ok(PyObject::tuple(vec![
                        PyObject::bytes(b.to_vec()),
                        PyObject::bytes(vec![]),
                        PyObject::bytes(vec![]),
                    ]))
                }
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "rpartition" => {
            if args.is_empty() {
                return Err(PyException::type_error("rpartition requires an argument"));
            }
            if let PyObjectPayload::Bytes(sep) | PyObjectPayload::ByteArray(sep) = &args[0].payload
            {
                if sep.is_empty() {
                    return Err(PyException::value_error("empty separator"));
                }
                if let Some(pos) = bytes_rfind_slice(b, sep.as_slice(), true) {
                    Ok(PyObject::tuple(vec![
                        PyObject::bytes(b[..pos].to_vec()),
                        PyObject::bytes((**sep).clone()),
                        PyObject::bytes(b[pos + sep.len()..].to_vec()),
                    ]))
                } else {
                    Ok(PyObject::tuple(vec![
                        PyObject::bytes(vec![]),
                        PyObject::bytes(vec![]),
                        PyObject::bytes(b.to_vec()),
                    ]))
                }
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "removeprefix" => {
            if args.is_empty() {
                return Err(PyException::type_error("removeprefix requires an argument"));
            }
            if let PyObjectPayload::Bytes(prefix) | PyObjectPayload::ByteArray(prefix) =
                &args[0].payload
            {
                if b.starts_with(prefix) {
                    Ok(PyObject::bytes(b[prefix.len()..].to_vec()))
                } else {
                    Ok(PyObject::bytes(b.to_vec()))
                }
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "removesuffix" => {
            if args.is_empty() {
                return Err(PyException::type_error("removesuffix requires an argument"));
            }
            if let PyObjectPayload::Bytes(suffix) | PyObjectPayload::ByteArray(suffix) =
                &args[0].payload
            {
                if b.ends_with(suffix) {
                    Ok(PyObject::bytes(b[..b.len() - suffix.len()].to_vec()))
                } else {
                    Ok(PyObject::bytes(b.to_vec()))
                }
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "rsplit" => {
            if args.is_empty() {
                let parts: Vec<PyObjectRef> = String::from_utf8_lossy(b)
                    .split_whitespace()
                    .rev()
                    .map(|s| PyObject::bytes(s.as_bytes().to_vec()))
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                Ok(PyObject::list(parts))
            } else if let PyObjectPayload::Bytes(sep) | PyObjectPayload::ByteArray(sep) =
                &args[0].payload
            {
                let max_split = if args.len() > 1 {
                    args[1].to_int().unwrap_or(-1)
                } else {
                    -1
                };
                let s = String::from_utf8_lossy(b);
                let sep_s = String::from_utf8_lossy(sep);
                let parts: Vec<&str> = if max_split < 0 {
                    s.rsplitn(usize::MAX, sep_s.as_ref()).collect()
                } else {
                    s.rsplitn(max_split as usize + 1, sep_s.as_ref()).collect()
                };
                let result: Vec<PyObjectRef> = parts
                    .into_iter()
                    .rev()
                    .map(|p| PyObject::bytes(p.as_bytes().to_vec()))
                    .collect();
                Ok(PyObject::list(result))
            } else {
                Err(PyException::type_error("a bytes-like object is required"))
            }
        }
        "splitlines" => {
            let s = String::from_utf8_lossy(b);
            let keep_ends = !args.is_empty() && args[0].is_truthy();
            let parts: Vec<PyObjectRef> = if keep_ends {
                s.split_inclusive('\n')
                    .map(|l| PyObject::bytes(l.as_bytes().to_vec()))
                    .collect()
            } else {
                s.lines()
                    .map(|l| PyObject::bytes(l.as_bytes().to_vec()))
                    .collect()
            };
            Ok(PyObject::list(parts))
        }
        "translate" => {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "translate requires a table argument",
                ));
            }
            let kwargs_delete = args.last().and_then(|last| {
                if let PyObjectPayload::Dict(map) = &last.payload {
                    map.read()
                        .get(&HashableKey::str_key(CompactString::from("delete")))
                        .cloned()
                } else {
                    None
                }
            });
            let positional_len = if matches!(
                args.last().map(|arg| &arg.payload),
                Some(PyObjectPayload::Dict(_))
            ) {
                args.len() - 1
            } else {
                args.len()
            };
            let table = match &args[0].payload {
                PyObjectPayload::Bytes(t) | PyObjectPayload::ByteArray(t) => (**t).clone(),
                PyObjectPayload::None => vec![],
                _ => {
                    return Err(PyException::type_error(
                        "a bytes-like object or None is required",
                    ))
                }
            };
            if !table.is_empty() && table.len() != 256 {
                return Err(PyException::value_error(
                    "translation table must be 256 characters long",
                ));
            }
            let delete_obj = if positional_len > 1 {
                Some(args[1].clone())
            } else {
                kwargs_delete
            };
            let delete: Vec<u8> = if let Some(obj) = delete_obj {
                bytes_like_data(&obj)
                    .ok_or_else(|| PyException::type_error("delete must be a bytes-like object"))?
            } else {
                vec![]
            };
            let mut result = Vec::with_capacity(b.len());
            for &byte in b.iter() {
                if delete.contains(&byte) {
                    continue;
                }
                if table.len() == 256 {
                    result.push(table[byte as usize]);
                } else {
                    result.push(byte);
                }
            }
            Ok(PyObject::bytes(result))
        }
        "tobytes" => {
            // memoryview.tobytes() / bytes.tobytes() — return a copy
            Ok(PyObject::bytes(b.to_vec()))
        }
        "tolist" => {
            // memoryview.tolist() — return list of ints
            let items: Vec<PyObjectRef> =
                b.iter().map(|&byte| PyObject::int(byte as i64)).collect();
            Ok(PyObject::list(items))
        }
        "release" => {
            // memoryview.release() — no-op for our impl
            Ok(PyObject::none())
        }
        _ => Err(PyException::attribute_error(format!(
            "'bytes' object has no attribute '{}'",
            method
        ))),
    }
}

// Hex encoding helper (avoid external dep)
mod hex {
    pub fn encode(data: &[u8]) -> String {
        data.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

/// Bytearray-specific method dispatch (mutable operations + delegates immutable ones to call_bytes_method).
pub(crate) fn call_bytearray_method(
    receiver: &PyObjectRef,
    b: &[u8],
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match method {
        "__mul__" | "__rmul__" => {
            check_args_min("bytearray.__mul__", args, 1)?;
            let n = index_to_usize_repeat(&args[0])?;
            let size = checked_repeat_len(b.len(), n, "bytearray repeat")?;
            let mut result = Vec::with_capacity(size);
            for _ in 0..n {
                result.extend_from_slice(b);
            }
            Ok(PyObject::bytearray(result))
        }
        "append" => {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "append() takes exactly one argument",
                ));
            }
            let byte_val = args[0].to_int()? as u8;
            // Safety: single-threaded access, Vec is owned inside Arc<PyObject>
            unsafe {
                let _ptr = b as *const [u8] as *const Vec<u8>;
                // Go from slice ptr back to Vec ptr (payload stores Vec<u8>)
                let vec_ptr = &receiver.payload as *const PyObjectPayload;
                if let PyObjectPayload::ByteArray(ref v) = *vec_ptr {
                    let vp = &**v as *const Vec<u8> as *mut Vec<u8>;
                    (*vp).push(byte_val);
                }
            }
            Ok(PyObject::none())
        }
        "extend" => {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "extend() takes exactly one argument",
                ));
            }
            if consume_bytearray_export(receiver) {
                return Err(PyException::new(
                    ExceptionKind::BufferError,
                    "Existing exports of data: object cannot be re-sized",
                ));
            }
            let new_bytes: Vec<u8> = match &args[0].payload {
                PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => (**b).clone(),
                PyObjectPayload::List(items) => items
                    .read()
                    .iter()
                    .map(|i| i.to_int().unwrap_or(0) as u8)
                    .collect(),
                _ => args[0]
                    .to_list()?
                    .iter()
                    .map(|i| i.to_int().unwrap_or(0) as u8)
                    .collect(),
            };
            unsafe {
                let vec_ptr = &receiver.payload as *const PyObjectPayload;
                if let PyObjectPayload::ByteArray(ref v) = *vec_ptr {
                    let vp = &**v as *const Vec<u8> as *mut Vec<u8>;
                    (*vp).extend_from_slice(&new_bytes);
                }
            }
            Ok(PyObject::none())
        }
        "pop" => {
            unsafe {
                let vec_ptr = &receiver.payload as *const PyObjectPayload;
                if let PyObjectPayload::ByteArray(ref v) = *vec_ptr {
                    let vp = &**v as *const Vec<u8> as *mut Vec<u8>;
                    if let Some(idx) = if args.is_empty() {
                        Some((*vp).len().wrapping_sub(1))
                    } else {
                        Some(args[0].to_int()? as usize)
                    } {
                        if idx < (*vp).len() {
                            let val = (*vp).remove(idx);
                            return Ok(PyObject::int(val as i64));
                        }
                    }
                    return Err(PyException::index_error("pop index out of range"));
                }
            }
            Err(PyException::index_error("pop from empty bytearray"))
        }
        "insert" => {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "insert() takes exactly 2 arguments",
                ));
            }
            let idx = index_to_i64(&args[0])?;
            let byte_val = args[1].to_int()? as u8;
            unsafe {
                let vec_ptr = &receiver.payload as *const PyObjectPayload;
                if let PyObjectPayload::ByteArray(ref v) = *vec_ptr {
                    let vp = &**v as *const Vec<u8> as *mut Vec<u8>;
                    let len = (*vp).len() as i64;
                    let actual = if idx < 0 {
                        (len + idx).max(0) as usize
                    } else {
                        (idx as usize).min((*vp).len())
                    };
                    (*vp).insert(actual, byte_val);
                }
            }
            Ok(PyObject::none())
        }
        "clear" => {
            unsafe {
                let vec_ptr = &receiver.payload as *const PyObjectPayload;
                if let PyObjectPayload::ByteArray(ref v) = *vec_ptr {
                    let vp = &**v as *const Vec<u8> as *mut Vec<u8>;
                    (*vp).clear();
                }
            }
            Ok(PyObject::none())
        }
        "reverse" => {
            unsafe {
                let vec_ptr = &receiver.payload as *const PyObjectPayload;
                if let PyObjectPayload::ByteArray(ref v) = *vec_ptr {
                    let vp = &**v as *const Vec<u8> as *mut Vec<u8>;
                    (*vp).reverse();
                }
            }
            Ok(PyObject::none())
        }
        "copy" => Ok(PyObject::bytearray(b.to_vec())),
        "__getitem__" => {
            check_args_min("bytearray.__getitem__", args, 1)?;
            let idx = index_to_i64(&args[0]).map_err(|e| {
                if e.kind == ExceptionKind::TypeError {
                    PyException::type_error("bytearray indices must be integers or slices")
                } else {
                    e
                }
            })?;
            let len = b.len() as i64;
            let actual = if idx < 0 { len + idx } else { idx };
            if actual < 0 || actual >= len {
                return Err(PyException::index_error("bytearray index out of range"));
            }
            Ok(PyObject::int(b[actual as usize] as i64))
        }
        "join" => {
            let result = call_bytes_method(b, "join", args)?;
            if let PyObjectPayload::Bytes(data) = &result.payload {
                Ok(PyObject::bytearray((**data).clone()))
            } else {
                Ok(result)
            }
        }
        "__setitem__" => {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "__setitem__() takes exactly 2 arguments",
                ));
            }
            let idx = index_to_i64(&args[0]).map_err(|e| {
                if e.kind == ExceptionKind::TypeError {
                    PyException::type_error("bytearray indices must be integers or slices")
                } else {
                    e
                }
            })?;
            let byte_val = args[1].to_int()? as u8;
            let len = b.len() as i64;
            let actual = if idx < 0 { len + idx } else { idx };
            if actual < 0 || actual >= len {
                return Err(PyException::index_error("bytearray index out of range"));
            }
            unsafe {
                let vec_ptr = &receiver.payload as *const PyObjectPayload;
                if let PyObjectPayload::ByteArray(ref v) = *vec_ptr {
                    let data_ptr = v.as_ptr() as *mut u8;
                    *data_ptr.add(actual as usize) = byte_val;
                }
            }
            Ok(PyObject::none())
        }
        // Delegate immutable methods to bytes
        _ => call_bytes_method(b, method, args),
    }
}
