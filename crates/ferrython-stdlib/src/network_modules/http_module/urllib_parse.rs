use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, new_fx_hashkey_map, FxHashKeyMap, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use super::{parse_url_string, percent_decode, quote_plus_encode};

pub fn create_urllib_parse_module() -> PyObjectRef {
    make_module(
        "urllib.parse",
        vec![
            ("urlencode", make_builtin(urllib_parse_urlencode)),
            ("quote", make_builtin(urllib_parse_quote)),
            ("quote_plus", make_builtin(urllib_parse_quote_plus)),
            (
                "quote_from_bytes",
                make_builtin(urllib_parse_quote_from_bytes),
            ),
            ("unquote", make_builtin(urllib_parse_unquote)),
            ("unquote_plus", make_builtin(urllib_parse_unquote_plus)),
            (
                "unquote_to_bytes",
                make_builtin(urllib_parse_unquote_to_bytes),
            ),
            ("urlparse", make_builtin(urllib_parse_urlparse)),
            ("urlunparse", make_builtin(urllib_parse_urlunparse)),
            ("urlsplit", make_builtin(urllib_parse_urlsplit)),
            ("urlunsplit", make_builtin(urllib_parse_urlunsplit)),
            ("urldefrag", make_builtin(urllib_parse_urldefrag)),
            ("urljoin", make_builtin(urllib_parse_urljoin)),
            ("parse_qs", make_builtin(urllib_parse_parse_qs)),
            ("parse_qsl", make_builtin(urllib_parse_parse_qsl)),
            (
                "uses_relative",
                PyObject::list(
                    vec![
                        "ftp", "http", "gopher", "nntp", "telnet", "file", "https", "shttp",
                        "snews", "prospero", "rtsp", "rtspu", "svn", "svn+ssh", "sftp", "nfs",
                        "git", "git+ssh",
                    ]
                    .into_iter()
                    .map(|s| PyObject::str_val(CompactString::from(s)))
                    .collect(),
                ),
            ),
            (
                "uses_netloc",
                PyObject::list(
                    vec![
                        "ftp", "http", "gopher", "nntp", "telnet", "file", "https", "shttp",
                        "snews", "prospero", "rtsp", "rtspu", "svn", "svn+ssh", "sftp", "nfs",
                        "git", "git+ssh", "ssh",
                    ]
                    .into_iter()
                    .map(|s| PyObject::str_val(CompactString::from(s)))
                    .collect(),
                ),
            ),
            (
                "ParseResult",
                PyObject::class(CompactString::from("ParseResult"), vec![], IndexMap::new()),
            ),
            (
                "SplitResult",
                PyObject::class(CompactString::from("SplitResult"), vec![], IndexMap::new()),
            ),
            (
                "DefragResult",
                PyObject::class(CompactString::from("DefragResult"), vec![], IndexMap::new()),
            ),
            (
                "SplitResultBytes",
                PyObject::class(
                    CompactString::from("SplitResultBytes"),
                    vec![],
                    IndexMap::new(),
                ),
            ),
            (
                "ParseResultBytes",
                PyObject::class(
                    CompactString::from("ParseResultBytes"),
                    vec![],
                    IndexMap::new(),
                ),
            ),
            (
                "scheme_chars",
                PyObject::str_val(CompactString::from(
                    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+-.",
                )),
            ),
            ("MAX_CACHE_SIZE", PyObject::int(20)),
        ],
    )
}

fn urllib_parse_urlencode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "urlencode() requires a mapping argument",
        ));
    }
    let val_to_str = |v: &PyObjectRef| -> String {
        match &v.payload {
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                String::from_utf8_lossy(b).into_owned()
            }
            _ => v.py_to_string(),
        }
    };
    let mut pairs = Vec::new();
    match &args[0].payload {
        PyObjectPayload::Dict(d) => {
            let d = d.read();
            for (k, v) in d.iter() {
                let ks = match k {
                    HashableKey::Str(s) => s.to_string(),
                    HashableKey::Int(i) => i.to_string(),
                    _ => format!("{:?}", k),
                };
                pairs.push(format!(
                    "{}={}",
                    quote_plus_encode(&ks),
                    quote_plus_encode(&val_to_str(&v))
                ));
            }
        }
        PyObjectPayload::List(items) => {
            let items = items.read();
            for item in items.iter() {
                if let PyObjectPayload::Tuple(pair) = &item.payload {
                    if pair.len() >= 2 {
                        pairs.push(format!(
                            "{}={}",
                            quote_plus_encode(&val_to_str(&pair[0])),
                            quote_plus_encode(&val_to_str(&pair[1]))
                        ));
                    }
                }
            }
        }
        _ => {
            return Err(PyException::type_error(
                "urlencode requires a mapping or sequence",
            ))
        }
    }
    Ok(PyObject::str_val(CompactString::from(pairs.join("&"))))
}

fn urllib_parse_quote(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "quote() requires a string argument",
        ));
    }
    let s = match &args[0].payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
            String::from_utf8_lossy(b).into_owned()
        }
        _ => args[0].py_to_string(),
    };
    let safe = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        "/".to_string()
    };
    let mut result = String::with_capacity(s.len());
    for b in s.bytes() {
        if (b as char).is_ascii_alphanumeric()
            || b == b'-'
            || b == b'_'
            || b == b'.'
            || b == b'~'
            || safe.as_bytes().contains(&b)
        {
            result.push(b as char);
        } else {
            result.push_str(&format!("%{:02X}", b));
        }
    }
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn urllib_parse_quote_plus(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "quote_plus() requires a string argument",
        ));
    }
    let s = args[0].py_to_string();
    let safe = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        String::new()
    };
    let mut result = String::with_capacity(s.len());
    for b in s.bytes() {
        if b == b' ' {
            result.push('+');
        } else if (b as char).is_ascii_alphanumeric()
            || b == b'-'
            || b == b'_'
            || b == b'.'
            || b == b'~'
            || safe.as_bytes().contains(&b)
        {
            result.push(b as char);
        } else {
            result.push_str(&format!("%{:02X}", b));
        }
    }
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn urllib_parse_quote_from_bytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "quote_from_bytes() requires a bytes argument",
        ));
    }
    let data = match &args[0].payload {
        PyObjectPayload::Bytes(b) => (**b).clone(),
        PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
        _ => return Err(PyException::type_error("quote_from_bytes: expected bytes")),
    };
    let safe = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        "/".to_string()
    };
    let mut result = String::with_capacity(data.len());
    for b in &data {
        if (*b as char).is_ascii_alphanumeric()
            || *b == b'-'
            || *b == b'_'
            || *b == b'.'
            || *b == b'~'
            || safe.as_bytes().contains(b)
        {
            result.push(*b as char);
        } else {
            result.push_str(&format!("%{:02X}", b));
        }
    }
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn urllib_parse_unquote_to_bytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "unquote_to_bytes() requires a string argument",
        ));
    }
    let s = args[0].py_to_string();
    let decoded = percent_decode(&s);
    Ok(PyObject::bytes(decoded.into_bytes()))
}

fn urllib_parse_unquote(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "unquote() requires a string argument",
        ));
    }
    let s = args[0].py_to_string();
    Ok(PyObject::str_val(CompactString::from(percent_decode(&s))))
}

fn urllib_parse_unquote_plus(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "unquote_plus() requires a string argument",
        ));
    }
    let s = args[0].py_to_string().replace('+', " ");
    Ok(PyObject::str_val(CompactString::from(percent_decode(&s))))
}

fn urllib_parse_urlparse(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "urlparse() requires a string argument",
        ));
    }
    let url = args[0].py_to_string();
    let p = parse_url_string(&url);

    let scheme = PyObject::str_val(CompactString::from(&p.scheme));
    let netloc = PyObject::str_val(CompactString::from(&p.netloc));
    let path = PyObject::str_val(CompactString::from(&p.path));
    let params = PyObject::str_val(CompactString::from(""));
    let query = PyObject::str_val(CompactString::from(&p.query));
    let fragment = PyObject::str_val(CompactString::from(&p.fragment));

    let components = vec![
        scheme.clone(),
        netloc.clone(),
        path.clone(),
        params.clone(),
        query.clone(),
        fragment.clone(),
    ];

    let cls = PyObject::class(CompactString::from("ParseResult"), vec![], IndexMap::new());
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("scheme"), scheme);
    attrs.insert(CompactString::from("netloc"), netloc);
    attrs.insert(CompactString::from("path"), path);
    attrs.insert(CompactString::from("params"), params);
    attrs.insert(CompactString::from("query"), query);
    attrs.insert(CompactString::from("fragment"), fragment);
    attrs.insert(
        CompactString::from("hostname"),
        if p.host.is_empty() {
            PyObject::none()
        } else {
            PyObject::str_val(CompactString::from(p.host.to_lowercase()))
        },
    );
    let has_explicit_port = {
        let hp = if p.netloc.contains('@') {
            p.netloc.rsplit('@').next().unwrap_or(&p.netloc)
        } else {
            &p.netloc
        };
        hp.contains(':')
            && hp
                .rsplit(':')
                .next()
                .and_then(|s| s.parse::<u16>().ok())
                .is_some()
    };
    attrs.insert(
        CompactString::from("port"),
        if has_explicit_port {
            PyObject::int(p.port as i64)
        } else {
            PyObject::none()
        },
    );
    attrs.insert(
        CompactString::from("username"),
        if p.username.is_empty() {
            PyObject::none()
        } else {
            PyObject::str_val(CompactString::from(&p.username))
        },
    );
    attrs.insert(
        CompactString::from("password"),
        if p.password.is_empty() {
            PyObject::none()
        } else {
            PyObject::str_val(CompactString::from(&p.password))
        },
    );

    let url_c = url.clone();
    attrs.insert(
        CompactString::from("geturl"),
        PyObject::native_closure("geturl", move |_args| {
            Ok(PyObject::str_val(CompactString::from(url_c.as_str())))
        }),
    );

    let iter_components = components.clone();
    attrs.insert(
        CompactString::from("__iter__"),
        PyObject::native_closure("__iter__", move |_args| {
            Ok(PyObject::tuple(iter_components.clone()))
        }),
    );

    let idx_components = components.clone();
    attrs.insert(
        CompactString::from("__getitem__"),
        PyObject::native_closure("__getitem__", move |args| {
            let idx = if !args.is_empty() {
                args[0].as_int().unwrap_or(0)
            } else {
                0
            };
            let i = if idx < 0 {
                (6 + idx) as usize
            } else {
                idx as usize
            };
            idx_components
                .get(i)
                .cloned()
                .ok_or_else(|| PyException::index_error("tuple index out of range"))
        }),
    );

    attrs.insert(
        CompactString::from("__len__"),
        PyObject::native_closure("__len__", move |_args| Ok(PyObject::int(6))),
    );

    let repr_components = components;
    attrs.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("__repr__", move |_args| {
            let parts: Vec<String> = repr_components
                .iter()
                .map(|c| format!("'{}'", c.py_to_string()))
                .collect();
            Ok(PyObject::str_val(CompactString::from(format!(
                "ParseResult(scheme={}, netloc={}, path={}, params={}, query={}, fragment={})",
                parts[0], parts[1], parts[2], parts[3], parts[4], parts[5]
            ))))
        }),
    );

    Ok(PyObject::instance_with_attrs(cls, attrs))
}

fn urllib_parse_urlunparse(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("urlunparse() requires 1 argument"));
    }
    let components = match &args[0].payload {
        PyObjectPayload::Tuple(items) => (**items).clone(),
        PyObjectPayload::List(items) => items.read().clone(),
        PyObjectPayload::Instance(_) => {
            let mut parts = Vec::new();
            for attr in &["scheme", "netloc", "path", "params", "query", "fragment"] {
                parts.push(
                    args[0]
                        .get_attr(attr)
                        .unwrap_or_else(|| PyObject::str_val(CompactString::from(""))),
                );
            }
            parts
        }
        _ => {
            return Err(PyException::type_error(
                "urlunparse requires a tuple/list/ParseResult",
            ))
        }
    };
    if components.len() < 6 {
        return Err(PyException::type_error("urlunparse requires 6 components"));
    }
    let to_str = |obj: &PyObjectRef| -> String {
        if matches!(&obj.payload, PyObjectPayload::None) {
            String::new()
        } else {
            obj.py_to_string()
        }
    };
    let scheme = to_str(&components[0]);
    let netloc = to_str(&components[1]);
    let path = to_str(&components[2]);
    let params = to_str(&components[3]);
    let query = to_str(&components[4]);
    let fragment = to_str(&components[5]);

    let mut url = String::new();
    if !scheme.is_empty() {
        url.push_str(&scheme);
        url.push_str("://");
    }
    url.push_str(&netloc);
    if !path.is_empty() {
        if !path.starts_with('/') && !netloc.is_empty() {
            url.push('/');
        }
        url.push_str(&path);
    }
    if !params.is_empty() {
        url.push(';');
        url.push_str(&params);
    }
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query);
    }
    if !fragment.is_empty() {
        url.push('#');
        url.push_str(&fragment);
    }
    Ok(PyObject::str_val(CompactString::from(url)))
}

fn urllib_parse_urlsplit(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("urlsplit() requires 1 argument"));
    }
    let url = args[0].py_to_string();
    let p = parse_url_string(&url);

    let scheme = PyObject::str_val(CompactString::from(&p.scheme));
    let netloc = PyObject::str_val(CompactString::from(&p.netloc));
    let path = PyObject::str_val(CompactString::from(&p.path));
    let query = PyObject::str_val(CompactString::from(&p.query));
    let fragment = PyObject::str_val(CompactString::from(&p.fragment));

    let components = vec![
        scheme.clone(),
        netloc.clone(),
        path.clone(),
        query.clone(),
        fragment.clone(),
    ];

    let cls = PyObject::class(CompactString::from("SplitResult"), vec![], IndexMap::new());
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("scheme"), scheme);
    attrs.insert(CompactString::from("netloc"), netloc);
    attrs.insert(CompactString::from("path"), path);
    attrs.insert(CompactString::from("query"), query);
    attrs.insert(CompactString::from("fragment"), fragment);
    attrs.insert(
        CompactString::from("hostname"),
        if p.host.is_empty() {
            PyObject::none()
        } else {
            PyObject::str_val(CompactString::from(p.host.to_lowercase()))
        },
    );
    let has_explicit_port = {
        let hp = if p.netloc.contains('@') {
            p.netloc.rsplit('@').next().unwrap_or(&p.netloc)
        } else {
            &p.netloc
        };
        hp.contains(':')
            && hp
                .rsplit(':')
                .next()
                .and_then(|s| s.parse::<u16>().ok())
                .is_some()
    };
    attrs.insert(
        CompactString::from("port"),
        if has_explicit_port {
            PyObject::int(p.port as i64)
        } else {
            PyObject::none()
        },
    );
    attrs.insert(
        CompactString::from("username"),
        if p.username.is_empty() {
            PyObject::none()
        } else {
            PyObject::str_val(CompactString::from(&p.username))
        },
    );
    attrs.insert(
        CompactString::from("password"),
        if p.password.is_empty() {
            PyObject::none()
        } else {
            PyObject::str_val(CompactString::from(&p.password))
        },
    );

    let url_c = url.clone();
    attrs.insert(
        CompactString::from("geturl"),
        PyObject::native_closure("geturl", move |_| {
            Ok(PyObject::str_val(CompactString::from(url_c.as_str())))
        }),
    );

    let idx_components = components.clone();
    attrs.insert(
        CompactString::from("__getitem__"),
        PyObject::native_closure("__getitem__", move |args| {
            let idx = if !args.is_empty() {
                args[0].as_int().unwrap_or(0)
            } else {
                0
            };
            let i = if idx < 0 {
                (5 + idx) as usize
            } else {
                idx as usize
            };
            idx_components
                .get(i)
                .cloned()
                .ok_or_else(|| PyException::index_error("tuple index out of range"))
        }),
    );

    attrs.insert(
        CompactString::from("__len__"),
        PyObject::native_closure("__len__", |_| Ok(PyObject::int(5))),
    );

    let iter_components = components.clone();
    attrs.insert(
        CompactString::from("__iter__"),
        PyObject::native_closure("__iter__", move |_| {
            Ok(PyObject::tuple(iter_components.clone()))
        }),
    );

    let repr_components = components;
    attrs.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("__repr__", move |_| {
            let parts: Vec<String> = repr_components
                .iter()
                .map(|c| format!("'{}'", c.py_to_string()))
                .collect();
            Ok(PyObject::str_val(CompactString::from(format!(
                "SplitResult(scheme={}, netloc={}, path={}, query={}, fragment={})",
                parts[0], parts[1], parts[2], parts[3], parts[4]
            ))))
        }),
    );

    Ok(PyObject::instance_with_attrs(cls, attrs))
}

fn urllib_parse_urlunsplit(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("urlunsplit() requires 1 argument"));
    }
    let components = match &args[0].payload {
        PyObjectPayload::Tuple(items) => (**items).clone(),
        PyObjectPayload::List(items) => items.read().clone(),
        _ => return Err(PyException::type_error("urlunsplit requires a tuple/list")),
    };
    if components.len() < 5 {
        return Err(PyException::type_error("urlunsplit requires 5 components"));
    }
    let to_str = |obj: &PyObjectRef| -> String {
        if matches!(&obj.payload, PyObjectPayload::None) {
            String::new()
        } else {
            obj.py_to_string()
        }
    };
    let scheme = to_str(&components[0]);
    let netloc = to_str(&components[1]);
    let path = to_str(&components[2]);
    let query = to_str(&components[3]);
    let fragment = to_str(&components[4]);

    let mut url = String::new();
    if !scheme.is_empty() {
        url.push_str(&scheme);
        url.push_str("://");
    }
    url.push_str(&netloc);
    if !path.is_empty() {
        if !path.starts_with('/') && !netloc.is_empty() {
            url.push('/');
        }
        url.push_str(&path);
    }
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query);
    }
    if !fragment.is_empty() {
        url.push('#');
        url.push_str(&fragment);
    }
    Ok(PyObject::str_val(CompactString::from(url)))
}

fn urllib_parse_urldefrag(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("urldefrag() requires 1 argument"));
    }
    let url = args[0].py_to_string();
    let (base, frag) = if let Some(idx) = url.find('#') {
        (&url[..idx], &url[idx + 1..])
    } else {
        (url.as_str(), "")
    };
    let cls = PyObject::class(CompactString::from("DefragResult"), vec![], IndexMap::new());
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("url"),
        PyObject::str_val(CompactString::from(base)),
    );
    attrs.insert(
        CompactString::from("fragment"),
        PyObject::str_val(CompactString::from(frag)),
    );
    let base_c = base.to_string();
    let frag_c = frag.to_string();
    let components = vec![
        PyObject::str_val(CompactString::from(&base_c)),
        PyObject::str_val(CompactString::from(&frag_c)),
    ];
    let idx_c = components.clone();
    attrs.insert(
        CompactString::from("__getitem__"),
        PyObject::native_closure("__getitem__", move |args| {
            let idx = if !args.is_empty() {
                args[0].as_int().unwrap_or(0)
            } else {
                0
            };
            let i = if idx < 0 {
                (2 + idx) as usize
            } else {
                idx as usize
            };
            idx_c
                .get(i)
                .cloned()
                .ok_or_else(|| PyException::index_error("tuple index out of range"))
        }),
    );
    attrs.insert(
        CompactString::from("__len__"),
        PyObject::native_closure("__len__", |_| Ok(PyObject::int(2))),
    );
    Ok(PyObject::instance_with_attrs(cls, attrs))
}

fn urllib_parse_urljoin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("urljoin() requires 2 arguments"));
    }
    let base = args[0].py_to_string();
    let url = args[1].py_to_string();

    if url.contains("://") {
        return Ok(PyObject::str_val(CompactString::from(url)));
    }

    let bp = parse_url_string(&base);

    let raw_path = if url.starts_with('/') {
        return Ok(PyObject::str_val(CompactString::from(format!(
            "{}://{}{}",
            bp.scheme,
            bp.netloc,
            normalize_path(&url)
        ))));
    } else if url.starts_with("//") {
        return Ok(PyObject::str_val(CompactString::from(format!(
            "{}:{}",
            bp.scheme, url
        ))));
    } else if url.is_empty() {
        return Ok(PyObject::str_val(CompactString::from(base)));
    } else {
        let base_dir = if let Some(idx) = bp.path.rfind('/') {
            &bp.path[..=idx]
        } else {
            "/"
        };
        format!("{}{}", base_dir, url)
    };

    let result = format!("{}://{}{}", bp.scheme, bp.netloc, normalize_path(&raw_path));
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn normalize_path(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "." | "" => {
                if segments.is_empty() {
                    segments.push("");
                }
            }
            ".." => {
                if segments.len() > 1 {
                    segments.pop();
                }
            }
            _ => segments.push(seg),
        }
    }
    let result = segments.join("/");
    if result.is_empty() {
        "/".to_string()
    } else {
        result
    }
}

fn urllib_parse_parse_qs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "parse_qs() requires a string argument",
        ));
    }
    let qs = args[0].py_to_string();
    let mut result: FxHashKeyMap = new_fx_hashkey_map();

    if qs.is_empty() {
        return Ok(PyObject::dict(result));
    }

    for pair in qs.split('&') {
        let parts: Vec<&str> = pair.splitn(2, '=').collect();
        let key = percent_decode(parts[0]);
        let val = if parts.len() > 1 {
            percent_decode(parts[1])
        } else {
            String::new()
        };
        let hk = HashableKey::str_key(CompactString::from(key.as_str()));
        let entry = result
            .entry(hk.clone())
            .or_insert_with(|| PyObject::list(vec![]));
        if let PyObjectPayload::List(items) = &entry.payload {
            items
                .write()
                .push(PyObject::str_val(CompactString::from(val.as_str())));
        }
    }

    Ok(PyObject::dict(result))
}

fn urllib_parse_parse_qsl(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "parse_qsl() requires a string argument",
        ));
    }
    let qs = args[0].py_to_string();
    let mut result = Vec::new();

    if qs.is_empty() {
        return Ok(PyObject::list(result));
    }

    for pair in qs.split('&') {
        let parts: Vec<&str> = pair.splitn(2, '=').collect();
        let key = percent_decode(parts[0]);
        let val = if parts.len() > 1 {
            percent_decode(parts[1])
        } else {
            String::new()
        };
        result.push(PyObject::tuple(vec![
            PyObject::str_val(CompactString::from(key)),
            PyObject::str_val(CompactString::from(val)),
        ]));
    }

    Ok(PyObject::list(result))
}
