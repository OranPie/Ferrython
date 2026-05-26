use super::*;

// ── encodings module ──

pub fn create_encodings_module() -> PyObjectRef {
    // The encodings module provides codec registration and lookup.
    // In CPython this is a package (encodings/__init__.py) with sub-modules for each codec.
    // We provide a minimal stub that covers common use cases.

    let search_function = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Ok(PyObject::none());
        }
        let name = args[0].py_to_string().to_lowercase().replace('-', "_");
        match name.as_str() {
            "utf_8" | "utf8" | "utf_8_sig" => {
                Ok(PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from("utf-8")),
                    PyObject::none(), // encode
                    PyObject::none(), // decode
                    PyObject::none(), // streamreader
                    PyObject::none(), // streamwriter
                ]))
            }
            "ascii" | "us_ascii" => Ok(PyObject::tuple(vec![
                PyObject::str_val(CompactString::from("ascii")),
                PyObject::none(),
                PyObject::none(),
                PyObject::none(),
                PyObject::none(),
            ])),
            "latin_1" | "iso8859_1" | "latin1" | "iso_8859_1" => Ok(PyObject::tuple(vec![
                PyObject::str_val(CompactString::from("latin-1")),
                PyObject::none(),
                PyObject::none(),
                PyObject::none(),
                PyObject::none(),
            ])),
            _ => Ok(PyObject::none()),
        }
    });

    let normalize_encoding = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Ok(PyObject::str_val(CompactString::from("")));
        }
        let name = args[0]
            .py_to_string()
            .to_lowercase()
            .replace('-', "_")
            .replace(' ', "_");
        Ok(PyObject::str_val(CompactString::from(name)))
    });

    make_module(
        "encodings",
        vec![
            ("search_function", search_function),
            ("normalize_encoding", normalize_encoding),
            // Sub-module aliases
            ("utf_8", make_builtin(|_| Ok(PyObject::none()))),
            ("ascii", make_builtin(|_| Ok(PyObject::none()))),
            ("latin_1", make_builtin(|_| Ok(PyObject::none()))),
        ],
    )
}

pub fn create_encodings_aliases_module() -> PyObjectRef {
    let mut aliases = IndexMap::new();
    let alias_pairs = [
        ("646", "ascii"),
        ("ansi_x3.4_1968", "ascii"),
        ("ansi_x3_4_1968", "ascii"),
        ("ascii", "ascii"),
        ("cp367", "ascii"),
        ("csascii", "ascii"),
        ("ibm367", "ascii"),
        ("iso646_us", "ascii"),
        ("iso_646.irv_1991", "ascii"),
        ("iso_ir_6", "ascii"),
        ("us", "ascii"),
        ("us_ascii", "ascii"),
        ("utf_8", "utf_8"),
        ("utf8", "utf_8"),
        ("utf", "utf_8"),
        ("cp65001", "utf_8"),
        ("utf_8_sig", "utf_8_sig"),
        ("latin_1", "iso8859_1"),
        ("latin1", "iso8859_1"),
        ("iso_8859_1", "iso8859_1"),
        ("iso8859_1", "iso8859_1"),
        ("8859", "iso8859_1"),
        ("cp819", "iso8859_1"),
        ("iso_8859_1_1987", "iso8859_1"),
        ("l1", "iso8859_1"),
        ("utf_16", "utf_16"),
        ("utf16", "utf_16"),
        ("utf_16_le", "utf_16_le"),
        ("utf_16_be", "utf_16_be"),
        ("utf_32", "utf_32"),
        ("utf_32_le", "utf_32_le"),
        ("utf_32_be", "utf_32_be"),
        ("cp1252", "cp1252"),
        ("windows_1252", "cp1252"),
        ("cp437", "cp437"),
        ("ibm437", "cp437"),
        ("shift_jis", "shift_jis"),
        ("shiftjis", "shift_jis"),
        ("csshiftjis", "shift_jis"),
        ("euc_jp", "euc_jp"),
        ("eucjp", "euc_jp"),
        ("euc_kr", "euc_kr"),
        ("euckr", "euc_kr"),
        ("gb2312", "gb2312"),
        ("gbk", "gbk"),
        ("gb18030", "gb18030"),
        ("big5", "big5"),
        ("big5hkscs", "big5hkscs"),
        ("cp949", "cp949"),
        ("uhc", "cp949"),
        ("iso8859_2", "iso8859_2"),
        ("latin2", "iso8859_2"),
        ("l2", "iso8859_2"),
        ("iso8859_15", "iso8859_15"),
        ("latin9", "iso8859_15"),
        ("koi8_r", "koi8_r"),
        ("koi8_u", "koi8_u"),
        ("mac_roman", "mac_roman"),
        ("macintosh", "mac_roman"),
        ("idna", "idna"),
    ];
    for (alias, codec) in &alias_pairs {
        aliases.insert(
            HashableKey::str_key(CompactString::from(*alias)),
            PyObject::str_val(CompactString::from(*codec)),
        );
    }
    make_module(
        "encodings.aliases",
        vec![("aliases", PyObject::dict(aliases))],
    )
}

pub fn create_encodings_idna_module() -> PyObjectRef {
    make_module(
        "encodings.idna",
        vec![
            ("name", PyObject::str_val(CompactString::from("idna"))),
            (
                "encode",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error("encode() requires input"));
                    }
                    let s = args[0].py_to_string();
                    // Simple IDNA encoding: just lowercase ASCII
                    let encoded = s.to_ascii_lowercase();
                    Ok(PyObject::tuple(vec![
                        PyObject::bytes(encoded.into_bytes()),
                        PyObject::int(s.len() as i64),
                    ]))
                }),
            ),
            (
                "decode",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error("decode() requires input"));
                    }
                    let s = args[0].py_to_string();
                    Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from(&s)),
                        PyObject::int(s.len() as i64),
                    ]))
                }),
            ),
            (
                "IncrementalEncoder",
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
            ),
            (
                "IncrementalDecoder",
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
            ),
        ],
    )
}

pub fn create_multibytecodec_module() -> PyObjectRef {
    let mb_inc_decoder = PyObject::class(
        CompactString::from("MultibyteIncrementalDecoder"),
        vec![],
        IndexMap::new(),
    );
    if let PyObjectPayload::Class(ref cd) = mb_inc_decoder.payload {
        cd.namespace.write().insert(
            CompactString::from("__init__"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Err(PyException::type_error("requires self"));
                }
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    let errors = if args.len() > 1 {
                        args[1].py_to_string()
                    } else {
                        "strict".to_string()
                    };
                    inst.attrs.write().insert(
                        CompactString::from("errors"),
                        PyObject::str_val(CompactString::from(errors)),
                    );
                }
                Ok(PyObject::none())
            }),
        );
        cd.namespace.write().insert(
            CompactString::from("decode"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Err(PyException::type_error("decode() requires input"));
                }
                let input = args[1].py_to_string();
                Ok(PyObject::str_val(CompactString::from(input)))
            }),
        );
        cd.namespace.write().insert(
            CompactString::from("reset"),
            make_builtin(|_| Ok(PyObject::none())),
        );
    }

    let mb_inc_encoder = PyObject::class(
        CompactString::from("MultibyteIncrementalEncoder"),
        vec![],
        IndexMap::new(),
    );
    if let PyObjectPayload::Class(ref cd) = mb_inc_encoder.payload {
        cd.namespace.write().insert(
            CompactString::from("__init__"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Err(PyException::type_error("requires self"));
                }
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    let errors = if args.len() > 1 {
                        args[1].py_to_string()
                    } else {
                        "strict".to_string()
                    };
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
            make_builtin(|args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Err(PyException::type_error("encode() requires input"));
                }
                let input = args[1].py_to_string();
                Ok(PyObject::bytes(input.into_bytes()))
            }),
        );
        cd.namespace.write().insert(
            CompactString::from("reset"),
            make_builtin(|_| Ok(PyObject::none())),
        );
    }

    let mb_stream_reader = PyObject::class(
        CompactString::from("MultibyteStreamReader"),
        vec![],
        IndexMap::new(),
    );
    let mb_stream_writer = PyObject::class(
        CompactString::from("MultibyteStreamWriter"),
        vec![],
        IndexMap::new(),
    );

    make_module(
        "_multibytecodec",
        vec![
            ("MultibyteIncrementalDecoder", mb_inc_decoder),
            ("MultibyteIncrementalEncoder", mb_inc_encoder),
            ("MultibyteStreamReader", mb_stream_reader),
            ("MultibyteStreamWriter", mb_stream_writer),
            ("__create_codec", make_builtin(|_| Ok(PyObject::none()))),
        ],
    )
}

/// Generic encodings.* codec submodule — provides IncrementalDecoder/Encoder classes
/// that handle encode/decode via the codecs module infrastructure.
pub fn create_encodings_codec_module(module_name: &str) -> PyObjectRef {
    let codec_name = module_name
        .strip_prefix("encodings.")
        .unwrap_or(module_name);
    let codec_name_cs = CompactString::from(codec_name);

    // IncrementalDecoder class for this encoding
    let inc_decoder = PyObject::class(
        CompactString::from("IncrementalDecoder"),
        vec![],
        IndexMap::new(),
    );
    if let PyObjectPayload::Class(ref cd) = inc_decoder.payload {
        let cn = codec_name_cs.clone();
        cd.namespace.write().insert(
            CompactString::from("__init__"),
            PyObject::native_closure(
                "IncrementalDecoder.__init__",
                move |args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error("requires self"));
                    }
                    if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                        let errors = if args.len() > 1 {
                            args[1].py_to_string()
                        } else {
                            "strict".to_string()
                        };
                        inst.attrs.write().insert(
                            CompactString::from("errors"),
                            PyObject::str_val(CompactString::from(errors)),
                        );
                        inst.attrs.write().insert(
                            CompactString::from("_encoding"),
                            PyObject::str_val(cn.clone()),
                        );
                    }
                    Ok(PyObject::none())
                },
            ),
        );
        cd.namespace.write().insert(
            CompactString::from("decode"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Err(PyException::type_error("decode() requires input"));
                }
                // Simple passthrough for UTF-8 compatible encodings
                let input = args[1].py_to_string();
                Ok(PyObject::str_val(CompactString::from(input)))
            }),
        );
        cd.namespace.write().insert(
            CompactString::from("reset"),
            make_builtin(|_| Ok(PyObject::none())),
        );
        cd.namespace.write().insert(
            CompactString::from("getstate"),
            make_builtin(|_| {
                Ok(PyObject::tuple(vec![
                    PyObject::bytes(vec![]),
                    PyObject::int(0),
                ]))
            }),
        );
        cd.namespace.write().insert(
            CompactString::from("setstate"),
            make_builtin(|_| Ok(PyObject::none())),
        );
    }

    // IncrementalEncoder class for this encoding
    let inc_encoder = PyObject::class(
        CompactString::from("IncrementalEncoder"),
        vec![],
        IndexMap::new(),
    );
    if let PyObjectPayload::Class(ref cd) = inc_encoder.payload {
        let cn = codec_name_cs.clone();
        cd.namespace.write().insert(
            CompactString::from("__init__"),
            PyObject::native_closure(
                "IncrementalEncoder.__init__",
                move |args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error("requires self"));
                    }
                    if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                        let errors = if args.len() > 1 {
                            args[1].py_to_string()
                        } else {
                            "strict".to_string()
                        };
                        inst.attrs.write().insert(
                            CompactString::from("errors"),
                            PyObject::str_val(CompactString::from(errors)),
                        );
                        inst.attrs.write().insert(
                            CompactString::from("_encoding"),
                            PyObject::str_val(cn.clone()),
                        );
                    }
                    Ok(PyObject::none())
                },
            ),
        );
        cd.namespace.write().insert(
            CompactString::from("encode"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Err(PyException::type_error("encode() requires input"));
                }
                let input = args[1].py_to_string();
                Ok(PyObject::bytes(input.into_bytes()))
            }),
        );
        cd.namespace.write().insert(
            CompactString::from("reset"),
            make_builtin(|_| Ok(PyObject::none())),
        );
        cd.namespace.write().insert(
            CompactString::from("getstate"),
            make_builtin(|_| Ok(PyObject::int(0))),
        );
        cd.namespace.write().insert(
            CompactString::from("setstate"),
            make_builtin(|_| Ok(PyObject::none())),
        );
    }

    // getregentry() — returns a CodecInfo-like tuple
    let cn_entry = CompactString::from(codec_name);
    let getregentry = PyObject::native_closure("getregentry", move |_args: &[PyObjectRef]| {
        Ok(PyObject::tuple(vec![
            PyObject::str_val(cn_entry.clone()),
            PyObject::none(), // encode fn
            PyObject::none(), // decode fn
            PyObject::none(), // stream_reader
            PyObject::none(), // stream_writer
        ]))
    });

    make_module(
        module_name,
        vec![
            ("IncrementalDecoder", inc_decoder),
            ("IncrementalEncoder", inc_encoder),
            ("getregentry", getregentry),
            ("name", PyObject::str_val(CompactString::from(codec_name))),
        ],
    )
}

/// _string module — C accelerator for str.format_map internals
pub fn create_string_internal_module() -> PyObjectRef {
    make_module(
        "_string",
        vec![
            (
                "formatter_field_name_split",
                make_builtin(|args| {
                    // formatter_field_name_split(field_name) → (first, rest_iterator)
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "formatter_field_name_split requires 1 argument",
                        ));
                    }
                    let s = args[0].py_to_string();
                    // Split on first '.' or '['
                    let first_end = s.find(|c: char| c == '.' || c == '[').unwrap_or(s.len());
                    let first = &s[..first_end];
                    let first_val =
                        if first.chars().all(|c| c.is_ascii_digit()) && !first.is_empty() {
                            PyObject::int(first.parse::<i64>().unwrap_or(0))
                        } else {
                            PyObject::str_val(CompactString::from(first))
                        };
                    // Rest as list of (is_attr, value) tuples
                    let mut rest = Vec::new();
                    let mut pos = first_end;
                    let chars: Vec<char> = s.chars().collect();
                    while pos < chars.len() {
                        if chars[pos] == '.' {
                            pos += 1;
                            let start = pos;
                            while pos < chars.len() && chars[pos] != '.' && chars[pos] != '[' {
                                pos += 1;
                            }
                            let attr_name: String = chars[start..pos].iter().collect();
                            rest.push(PyObject::tuple(vec![
                                PyObject::bool_val(true),
                                PyObject::str_val(CompactString::from(attr_name)),
                            ]));
                        } else if chars[pos] == '[' {
                            pos += 1;
                            let start = pos;
                            while pos < chars.len() && chars[pos] != ']' {
                                pos += 1;
                            }
                            let idx_str: String = chars[start..pos].iter().collect();
                            let idx_val = if idx_str.chars().all(|c| c.is_ascii_digit())
                                && !idx_str.is_empty()
                            {
                                PyObject::int(idx_str.parse::<i64>().unwrap_or(0))
                            } else {
                                PyObject::str_val(CompactString::from(idx_str))
                            };
                            rest.push(PyObject::tuple(vec![PyObject::bool_val(false), idx_val]));
                            if pos < chars.len() {
                                pos += 1; // skip ']'
                            }
                        } else {
                            pos += 1;
                        }
                    }
                    let rest_iter = PyObject::list(rest);
                    Ok(PyObject::tuple(vec![first_val, rest_iter]))
                }),
            ),
            (
                "formatter_parser",
                make_builtin(|args| {
                    // formatter_parser(format_string) → iterator of
                    //   (literal_text, field_name, format_spec, conversion) tuples
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "formatter_parser requires 1 argument",
                        ));
                    }
                    let s = args[0].py_to_string();
                    let mut result = Vec::new();
                    let mut pos = 0;
                    let chars: Vec<char> = s.chars().collect();
                    while pos < chars.len() {
                        if chars[pos] == '{' {
                            if pos + 1 < chars.len() && chars[pos + 1] == '{' {
                                result.push(PyObject::tuple(vec![
                                    PyObject::str_val(CompactString::from("{")),
                                    PyObject::none(),
                                    PyObject::none(),
                                    PyObject::none(),
                                ]));
                                pos += 2;
                                continue;
                            }
                            let start = pos + 1;
                            let mut depth = 1;
                            pos += 1;
                            while pos < chars.len() && depth > 0 {
                                if chars[pos] == '{' {
                                    depth += 1;
                                }
                                if chars[pos] == '}' {
                                    depth -= 1;
                                }
                                if depth > 0 {
                                    pos += 1;
                                }
                            }
                            let field: String = chars[start..pos].iter().collect();
                            pos += 1; // skip '}'
                                      // Parse field_name!conversion:format_spec
                            let (field_name, conversion, format_spec) = {
                                let mut fname = field.as_str();
                                let mut conv = PyObject::none();
                                let mut fspec = CompactString::from("");
                                if let Some(i) = fname.find(':') {
                                    fspec = CompactString::from(&fname[i + 1..]);
                                    fname = &fname[..i];
                                }
                                // re-check fname for !conversion
                                let fname_str;
                                if let Some(i) = fname.find('!') {
                                    conv = PyObject::str_val(CompactString::from(&fname[i + 1..]));
                                    fname_str = fname[..i].to_string();
                                } else {
                                    fname_str = fname.to_string();
                                }
                                (fname_str, conv, fspec)
                            };
                            result.push(PyObject::tuple(vec![
                                PyObject::str_val(CompactString::from("")),
                                PyObject::str_val(CompactString::from(field_name)),
                                PyObject::str_val(format_spec),
                                conversion,
                            ]));
                        } else if chars[pos] == '}'
                            && pos + 1 < chars.len()
                            && chars[pos + 1] == '}'
                        {
                            result.push(PyObject::tuple(vec![
                                PyObject::str_val(CompactString::from("}")),
                                PyObject::none(),
                                PyObject::none(),
                                PyObject::none(),
                            ]));
                            pos += 2;
                        } else {
                            let start = pos;
                            while pos < chars.len() && chars[pos] != '{' && chars[pos] != '}' {
                                pos += 1;
                            }
                            let literal: String = chars[start..pos].iter().collect();
                            if !literal.is_empty() {
                                result.push(PyObject::tuple(vec![
                                    PyObject::str_val(CompactString::from(literal)),
                                    PyObject::none(),
                                    PyObject::none(),
                                    PyObject::none(),
                                ]));
                            }
                        }
                    }
                    Ok(PyObject::list(result))
                }),
            ),
        ],
    )
}
