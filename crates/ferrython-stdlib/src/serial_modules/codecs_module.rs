use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    check_args, check_args_min, make_builtin, make_module, PyCell, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;
use std::rc::Rc;
use std::sync::Arc;

use super::base64_module::extract_bytes;

// ── codecs helpers ─────────────────────────────────────────────────

fn normalize_encoding(enc: &str) -> String {
    enc.to_lowercase().replace('-', "_")
}

fn rot13(c: char) -> char {
    match c {
        'a'..='m' | 'A'..='M' => (c as u8 + 13) as char,
        'n'..='z' | 'N'..='Z' => (c as u8 - 13) as char,
        _ => c,
    }
}

fn punycode_digit(d: u32) -> u8 {
    if d < 26 {
        b'a' + d as u8
    } else {
        b'0' + (d as u8 - 26)
    }
}

fn punycode_adapt(delta: u32, numpoints: u32, firsttime: bool) -> u32 {
    let mut d = if firsttime { delta / 700 } else { delta / 2 };
    d += d / numpoints;
    let mut k = 0u32;
    while d > 455 {
        d /= 35;
        k += 36;
    }
    k + (36 * d) / (d + 38)
}

fn cp1252_encode(c: char) -> Result<u8, String> {
    let u = c as u32;
    if u < 0x80 || (0xA0..=0xFF).contains(&u) {
        return Ok(u as u8);
    }
    // Windows-1252 special range 0x80-0x9F
    match u {
        0x20AC => Ok(0x80),
        0x201A => Ok(0x82),
        0x0192 => Ok(0x83),
        0x201E => Ok(0x84),
        0x2026 => Ok(0x85),
        0x2020 => Ok(0x86),
        0x2021 => Ok(0x87),
        0x02C6 => Ok(0x88),
        0x2030 => Ok(0x89),
        0x0160 => Ok(0x8A),
        0x2039 => Ok(0x8B),
        0x0152 => Ok(0x8C),
        0x017D => Ok(0x8E),
        0x2018 => Ok(0x91),
        0x2019 => Ok(0x92),
        0x201C => Ok(0x93),
        0x201D => Ok(0x94),
        0x2022 => Ok(0x95),
        0x2013 => Ok(0x96),
        0x2014 => Ok(0x97),
        0x02DC => Ok(0x98),
        0x2122 => Ok(0x99),
        0x0161 => Ok(0x9A),
        0x203A => Ok(0x9B),
        0x0153 => Ok(0x9C),
        0x017E => Ok(0x9E),
        0x0178 => Ok(0x9F),
        _ => Err(format!(
            "'cp1252' codec can't encode character '\\u{:04x}'",
            u
        )),
    }
}

fn cp1252_decode(b: u8) -> char {
    if b < 0x80 || b >= 0xA0 {
        return b as char;
    }
    match b {
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
        _ => '\u{FFFD}', // undefined bytes → replacement char
    }
}

fn decode_utf16_with_bom(bytes: &[u8]) -> PyResult<PyObjectRef> {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        decode_utf16_le(&bytes[2..])
    } else if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        decode_utf16_be(&bytes[2..])
    } else {
        decode_utf16_le(bytes) // default to LE like CPython on little-endian
    }
}

fn decode_utf16_le(bytes: &[u8]) -> PyResult<PyObjectRef> {
    if bytes.len() % 2 != 0 {
        return Err(PyException::value_error("utf-16-le: truncated data"));
    }
    let u16s: Vec<u16> = bytes
        .chunks(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let s = String::from_utf16(&u16s).map_err(|_| PyException::value_error("invalid utf-16-le"))?;
    Ok(PyObject::str_val(CompactString::from(s)))
}

fn decode_utf16_be(bytes: &[u8]) -> PyResult<PyObjectRef> {
    if bytes.len() % 2 != 0 {
        return Err(PyException::value_error("utf-16-be: truncated data"));
    }
    let u16s: Vec<u16> = bytes
        .chunks(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    let s = String::from_utf16(&u16s).map_err(|_| PyException::value_error("invalid utf-16-be"))?;
    Ok(PyObject::str_val(CompactString::from(s)))
}

fn decode_utf32_with_bom(bytes: &[u8]) -> PyResult<PyObjectRef> {
    if bytes.len() >= 4 && bytes[..4] == [0xFF, 0xFE, 0x00, 0x00] {
        decode_utf32_le(&bytes[4..])
    } else if bytes.len() >= 4 && bytes[..4] == [0x00, 0x00, 0xFE, 0xFF] {
        decode_utf32_be(&bytes[4..])
    } else {
        decode_utf32_le(bytes)
    }
}

fn decode_utf32_le(bytes: &[u8]) -> PyResult<PyObjectRef> {
    if bytes.len() % 4 != 0 {
        return Err(PyException::value_error("utf-32-le: truncated data"));
    }
    let s: Result<String, _> = bytes
        .chunks(4)
        .map(|c| {
            let cp = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
            char::from_u32(cp)
                .ok_or_else(|| PyException::value_error("invalid utf-32-le codepoint"))
        })
        .collect();
    Ok(PyObject::str_val(CompactString::from(s?)))
}

fn decode_utf32_be(bytes: &[u8]) -> PyResult<PyObjectRef> {
    if bytes.len() % 4 != 0 {
        return Err(PyException::value_error("utf-32-be: truncated data"));
    }
    let s: Result<String, _> = bytes
        .chunks(4)
        .map(|c| {
            let cp = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
            char::from_u32(cp)
                .ok_or_else(|| PyException::value_error("invalid utf-32-be codepoint"))
        })
        .collect();
    Ok(PyObject::str_val(CompactString::from(s?)))
}

fn backslashreplace_char(c: char) -> String {
    let cp = c as u32;
    if cp <= 0xFF {
        format!("\\x{:02x}", cp)
    } else if cp <= 0xFFFF {
        format!("\\u{:04x}", cp)
    } else {
        format!("\\U{:08x}", cp)
    }
}

fn xmlcharrefreplace_char(c: char) -> String {
    format!("&#{};", c as u32)
}

fn resolve_encoding(norm: &str) -> &str {
    match norm {
        "utf_8" | "utf8" => "utf_8",
        "ascii" | "us_ascii" => "ascii",
        "latin_1" | "latin1" | "iso_8859_1" | "iso8859_1" | "8859" | "cp819" | "l1" => "latin_1",
        "utf_16" | "utf16" => "utf_16",
        "utf_16_le" | "utf16_le" | "utf_16le" => "utf_16_le",
        "utf_16_be" | "utf16_be" | "utf_16be" => "utf_16_be",
        "utf_32" | "utf32" => "utf_32",
        "utf_32_le" | "utf32_le" => "utf_32_le",
        "utf_32_be" | "utf32_be" => "utf_32_be",
        "cp1252" | "windows_1252" => "cp1252",
        "rot_13" | "rot13" => "rot_13",
        "punycode" => "punycode",
        "idna" => "idna",
        // ISO-8859 family
        "iso8859_2" | "iso_8859_2" | "latin2" | "l2" => "iso8859_2",
        "iso8859_3" | "iso_8859_3" | "latin3" | "l3" => "iso8859_3",
        "iso8859_4" | "iso_8859_4" | "latin4" | "l4" => "iso8859_4",
        "iso8859_5" | "iso_8859_5" | "cyrillic" => "iso8859_5",
        "iso8859_6" | "iso_8859_6" | "arabic" => "iso8859_6",
        "iso8859_7" | "iso_8859_7" | "greek" => "iso8859_7",
        "iso8859_8" | "iso_8859_8" | "hebrew" => "iso8859_8",
        "iso8859_9" | "iso_8859_9" | "latin5" | "l5" => "iso8859_9",
        "iso8859_10" | "iso_8859_10" | "latin6" | "l6" => "iso8859_10",
        "iso8859_11" | "iso_8859_11" | "thai" => "iso8859_11",
        "iso8859_13" | "iso_8859_13" | "latin7" | "l7" => "iso8859_13",
        "iso8859_14" | "iso_8859_14" | "latin8" | "l8" => "iso8859_14",
        "iso8859_15" | "iso_8859_15" | "latin9" | "l9" => "iso8859_15",
        "iso8859_16" | "iso_8859_16" | "latin10" | "l10" => "iso8859_16",
        // Windows code pages
        "cp437" => "cp437",
        "cp850" => "cp850",
        "cp866" => "cp866",
        "cp874" | "windows_874" => "cp874",
        "cp932" | "ms932" | "mskanji" | "ms_kanji" => "cp932",
        "cp949" | "ms949" | "uhc" => "cp949",
        "cp950" | "ms950" => "cp950",
        "cp1250" | "windows_1250" => "cp1250",
        "cp1251" | "windows_1251" => "cp1251",
        "cp1253" | "windows_1253" => "cp1253",
        "cp1254" | "windows_1254" => "cp1254",
        "cp1255" | "windows_1255" => "cp1255",
        "cp1256" | "windows_1256" => "cp1256",
        "cp1257" | "windows_1257" => "cp1257",
        "cp1258" | "windows_1258" => "cp1258",
        // CJK encodings
        "big5" | "big5_tw" | "csbig5" => "big5",
        "big5hkscs" | "big5_hkscs" => "big5hkscs",
        "euc_jp" | "eucjp" | "ujis" | "u_jis" => "euc_jp",
        "euc_kr" | "euckr" | "korean" => "euc_kr",
        "euc_cn" | "gb2312" | "chinese" | "csiso58gb231280" => "gb2312",
        "gbk" | "cp936" | "ms936" => "gbk",
        "gb18030" => "gb18030",
        "hz" | "hzgb" | "hz_gb" | "hz_gb_2312" => "hz",
        "shift_jis" | "shiftjis" | "sjis" | "s_jis" | "csshiftjis" => "shift_jis",
        "shift_jis_2004" | "shiftjis2004" | "sjis_2004" => "shift_jis_2004",
        "shift_jisx0213" | "shiftjisx0213" | "sjisx0213" => "shift_jisx0213",
        "iso2022_jp" | "iso_2022_jp" | "csiso2022jp" => "iso2022_jp",
        "iso2022_jp_2" | "iso_2022_jp_2" => "iso2022_jp_2",
        "iso2022_kr" | "iso_2022_kr" | "csiso2022kr" => "iso2022_kr",
        "iso2022_cn" | "iso_2022_cn" => "iso2022_cn",
        // Russian/Ukrainian
        "koi8_r" | "koi8r" => "koi8_r",
        "koi8_u" | "koi8u" => "koi8_u",
        "koi8_t" => "koi8_t",
        // Mac encodings
        "mac_roman" | "macroman" | "macintosh" => "mac_roman",
        "mac_cyrillic" | "maccyrillic" => "mac_cyrillic",
        "mac_greek" | "macgreek" => "mac_greek",
        "mac_latin2" | "maclatin2" | "maccentraleurope" => "mac_latin2",
        // Other
        "johab" => "johab",
        "tis_620" | "tis620" => "tis_620",
        "viscii" => "viscii",
        other => other,
    }
}

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
            (
                "open",
                make_builtin(|args: &[PyObjectRef]| {
                    check_args_min("codecs.open", args, 1)?;
                    let filename = args[0].py_to_string();
                    let mode =
                        if args.len() > 1 && !matches!(args[1].payload, PyObjectPayload::Dict(_)) {
                            args[1].py_to_string()
                        } else {
                            "r".to_string()
                        };
                    let _encoding =
                        if args.len() > 2 && !matches!(args[2].payload, PyObjectPayload::Dict(_)) {
                            args[2].py_to_string()
                        } else {
                            "utf-8".to_string()
                        };
                    // Delegate to Rust file I/O — codecs.open is just open() with encoding
                    if mode.contains('w') {
                        // Verify file is creatable
                        let _ = std::fs::File::create(&filename)
                            .map_err(|e| PyException::os_error(format!("{}: {}", e, filename)))?;
                        let mut attrs = IndexMap::new();
                        let path = filename.clone();
                        let buf = Rc::new(PyCell::new(String::new()));
                        let buf_w = buf.clone();
                        let buf_r = buf.clone();
                        let path_w = path.clone();
                        attrs.insert(
                            CompactString::from("write"),
                            PyObject::native_closure("write", move |wargs: &[PyObjectRef]| {
                                if let Some(s) = wargs.first() {
                                    buf_w.write().push_str(&s.py_to_string());
                                }
                                Ok(PyObject::none())
                            }),
                        );
                        attrs.insert(
                            CompactString::from("flush"),
                            PyObject::native_closure("flush", move |_| {
                                let content = buf_r.read().clone();
                                std::fs::write(&path_w, content.as_bytes())
                                    .map_err(|e| PyException::os_error(e.to_string()))?;
                                Ok(PyObject::none())
                            }),
                        );
                        let path_c = path.clone();
                        let buf_c = buf.clone();
                        attrs.insert(
                            CompactString::from("close"),
                            PyObject::native_closure("close", move |_| {
                                let content = buf_c.read().clone();
                                std::fs::write(&path_c, content.as_bytes())
                                    .map_err(|e| PyException::os_error(e.to_string()))?;
                                Ok(PyObject::none())
                            }),
                        );
                        attrs.insert(
                            CompactString::from("__enter__"),
                            PyObject::native_function("__enter__", |a: &[PyObjectRef]| {
                                Ok(if !a.is_empty() {
                                    a[0].clone()
                                } else {
                                    PyObject::none()
                                })
                            }),
                        );
                        let path_e = path.clone();
                        let buf_e = buf.clone();
                        attrs.insert(
                            CompactString::from("__exit__"),
                            PyObject::native_closure("__exit__", move |_| {
                                let content = buf_e.read().clone();
                                let _ = std::fs::write(&path_e, content.as_bytes());
                                Ok(PyObject::bool_val(false))
                            }),
                        );
                        Ok(PyObject::module_with_attrs(
                            CompactString::from("TextIOWrapper"),
                            attrs,
                        ))
                    } else {
                        // Read mode
                        let content = std::fs::read_to_string(&filename)
                            .map_err(|e| PyException::os_error(format!("{}: {}", e, filename)))?;
                        let content_arc = Arc::new(content);
                        let c1 = content_arc.clone();
                        let c2 = content_arc.clone();
                        let mut attrs = IndexMap::new();
                        attrs.insert(
                            CompactString::from("read"),
                            PyObject::native_closure("read", move |_| {
                                Ok(PyObject::str_val(CompactString::from(c1.as_str())))
                            }),
                        );
                        attrs.insert(
                            CompactString::from("readlines"),
                            PyObject::native_closure("readlines", move |_| {
                                let lines: Vec<PyObjectRef> = c2
                                    .lines()
                                    .map(|l| {
                                        PyObject::str_val(CompactString::from(format!("{}\n", l)))
                                    })
                                    .collect();
                                Ok(PyObject::list(lines))
                            }),
                        );
                        attrs.insert(
                            CompactString::from("close"),
                            PyObject::native_function("close", |_: &[PyObjectRef]| {
                                Ok(PyObject::none())
                            }),
                        );
                        attrs.insert(
                            CompactString::from("__enter__"),
                            PyObject::native_function("__enter__", |a: &[PyObjectRef]| {
                                Ok(if !a.is_empty() {
                                    a[0].clone()
                                } else {
                                    PyObject::none()
                                })
                            }),
                        );
                        attrs.insert(
                            CompactString::from("__exit__"),
                            PyObject::native_function("__exit__", |_: &[PyObjectRef]| {
                                Ok(PyObject::bool_val(false))
                            }),
                        );
                        Ok(PyObject::module_with_attrs(
                            CompactString::from("TextIOWrapper"),
                            attrs,
                        ))
                    }
                }),
            ),
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
