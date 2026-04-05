//! HTTP, urllib, and SSL stdlib modules.

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct ParsedUrl {
    scheme: String,
    host: String,
    port: u16,
    path: String,
    query: String,
    fragment: String,
    netloc: String,
}

fn parse_url_string(url: &str) -> ParsedUrl {
    let (scheme, rest) = if let Some(idx) = url.find("://") {
        (url[..idx].to_string(), &url[idx + 3..])
    } else {
        ("http".to_string(), url)
    };

    let (rest2, fragment) = if let Some(idx) = rest.find('#') {
        (&rest[..idx], rest[idx + 1..].to_string())
    } else {
        (rest, String::new())
    };

    let (rest3, query) = if let Some(idx) = rest2.find('?') {
        (&rest2[..idx], rest2[idx + 1..].to_string())
    } else {
        (rest2, String::new())
    };

    let (host_port, path) = if let Some(idx) = rest3.find('/') {
        (&rest3[..idx], rest3[idx..].to_string())
    } else {
        (rest3, "/".to_string())
    };

    let netloc = host_port.to_string();
    let (host, port) = if let Some(idx) = host_port.rfind(':') {
        let port_str = &host_port[idx + 1..];
        if let Ok(p) = port_str.parse::<u16>() {
            (host_port[..idx].to_string(), p)
        } else {
            (
                host_port.to_string(),
                if scheme == "https" { 443 } else { 80 },
            )
        }
    } else {
        (
            host_port.to_string(),
            if scheme == "https" { 443 } else { 80 },
        )
    };

    ParsedUrl {
        scheme,
        host,
        port,
        path,
        query,
        fragment,
        netloc,
    }
}

fn percent_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", b));
            }
        }
    }
    result
}

fn percent_decode(s: &str) -> String {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(val) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00"), 16)
            {
                result.push(val);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).to_string()
}

// ════════════════════════════════════════════════════════════════════════
// urllib module (urllib.request)
// ════════════════════════════════════════════════════════════════════════

pub fn create_urllib_module() -> PyObjectRef {
    make_module(
        "urllib.request",
        vec![
            ("urlopen", make_builtin(urllib_urlopen)),
            ("Request", make_builtin(urllib_request_constructor)),
        ],
    )
}

fn build_http_get(parsed: &ParsedUrl) -> String {
    let full_path = if parsed.query.is_empty() {
        parsed.path.clone()
    } else {
        format!("{}?{}", parsed.path, parsed.query)
    };
    format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: ferrython/1.0\r\nAccept: */*\r\n\r\n",
        full_path, parsed.host
    )
}

fn do_http_request(url: &str) -> PyResult<(u16, IndexMap<String, String>, Vec<u8>)> {
    let parsed = parse_url_string(url);
    if parsed.scheme == "https" {
        return Err(PyException::os_error(
            "HTTPS is not supported (no TLS available)",
        ));
    }

    let addr = format!("{}:{}", parsed.host, parsed.port);
    let mut stream = TcpStream::connect(&addr)
        .map_err(|e| PyException::os_error(format!("urlopen: {}", e)))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .ok();

    let request = build_http_get(&parsed);
    stream
        .write_all(request.as_bytes())
        .map_err(|e| PyException::os_error(format!("urlopen write: {}", e)))?;

    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| PyException::os_error(format!("urlopen read: {}", e)))?;

    // Parse HTTP response
    let raw_str = String::from_utf8_lossy(&raw);
    let header_end = raw_str.find("\r\n\r\n").unwrap_or(raw_str.len());
    let header_section = &raw_str[..header_end];
    let body_start = if header_end + 4 <= raw.len() {
        header_end + 4
    } else {
        raw.len()
    };
    let body = raw[body_start..].to_vec();

    let mut lines = header_section.lines();
    let status_line = lines.next().unwrap_or("HTTP/1.1 200 OK");
    let status_code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);

    let mut headers = IndexMap::new();
    for line in lines {
        if let Some(idx) = line.find(':') {
            let key = line[..idx].trim().to_string();
            let val = line[idx + 1..].trim().to_string();
            headers.insert(key, val);
        }
    }

    Ok((status_code, headers, body))
}

fn build_response_object(
    url: &str,
    status: u16,
    headers: IndexMap<String, String>,
    body: Vec<u8>,
) -> PyObjectRef {
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("__urllib_response__"),
        PyObject::bool_val(true),
    );
    attrs.insert(
        CompactString::from("url"),
        PyObject::str_val(CompactString::from(url)),
    );
    attrs.insert(CompactString::from("status"), PyObject::int(status as i64));
    attrs.insert(CompactString::from("code"), PyObject::int(status as i64));
    attrs.insert(
        CompactString::from("reason"),
        PyObject::str_val(CompactString::from(http_reason(status))),
    );

    // Build headers dict
    let mut hdr_map = IndexMap::new();
    for (k, v) in &headers {
        hdr_map.insert(
            HashableKey::Str(CompactString::from(k.as_str())),
            PyObject::str_val(CompactString::from(v.as_str())),
        );
    }
    attrs.insert(CompactString::from("headers"), PyObject::dict(hdr_map));

    let body_arc = Arc::new(body);
    let body_pos = Arc::new(Mutex::new(0usize));

    // read(n=-1) → bytes
    let b = body_arc.clone();
    let pos = body_pos.clone();
    attrs.insert(
        CompactString::from("read"),
        PyObject::native_closure("read", move |args| {
            let n = if !args.is_empty() {
                args[0].as_int().unwrap_or(-1)
            } else {
                -1
            };
            let mut p = pos.lock().unwrap();
            let remaining = &b[*p..];
            let chunk = if n < 0 {
                remaining.to_vec()
            } else {
                let end = std::cmp::min(n as usize, remaining.len());
                remaining[..end].to_vec()
            };
            *p += chunk.len();
            Ok(PyObject::bytes(chunk))
        }),
    );

    // readline() → bytes
    let b = body_arc.clone();
    let pos = body_pos.clone();
    attrs.insert(
        CompactString::from("readline"),
        PyObject::native_closure("readline", move |_args| {
            let mut p = pos.lock().unwrap();
            let remaining = &b[*p..];
            let end = remaining
                .iter()
                .position(|&c| c == b'\n')
                .map(|i| i + 1)
                .unwrap_or(remaining.len());
            let line = remaining[..end].to_vec();
            *p += line.len();
            Ok(PyObject::bytes(line))
        }),
    );

    // getcode() → int
    let sc = status;
    attrs.insert(
        CompactString::from("getcode"),
        PyObject::native_closure("getcode", move |_args| Ok(PyObject::int(sc as i64))),
    );

    // geturl() → str
    let u = url.to_string();
    attrs.insert(
        CompactString::from("geturl"),
        PyObject::native_closure("geturl", move |_args| {
            Ok(PyObject::str_val(CompactString::from(u.as_str())))
        }),
    );

    // close() — no-op
    attrs.insert(
        CompactString::from("close"),
        PyObject::native_closure("close", |_args| Ok(PyObject::none())),
    );

    // __enter__ / __exit__
    attrs.insert(
        CompactString::from("__enter__"),
        PyObject::native_function("__enter__", |args| {
            if !args.is_empty() {
                Ok(args[0].clone())
            } else {
                Ok(PyObject::none())
            }
        }),
    );
    attrs.insert(
        CompactString::from("__exit__"),
        PyObject::native_closure("__exit__", |_args| Ok(PyObject::bool_val(false))),
    );

    attrs.insert(
        CompactString::from("_bind_methods"),
        PyObject::bool_val(true),
    );

    PyObject::module_with_attrs(CompactString::from("http.client.HTTPResponse"), attrs)
}

fn urllib_urlopen(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "urlopen() requires a url argument",
        ));
    }
    // Accept a string URL or a Request object
    let url = if let Some(u) = args[0].get_attr("full_url") {
        u.py_to_string()
    } else {
        args[0].py_to_string()
    };

    let (status, headers, body) = do_http_request(&url)?;
    Ok(build_response_object(&url, status, headers, body))
}

fn urllib_request_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "Request() requires a url argument",
        ));
    }
    let url = args[0].py_to_string();
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("full_url"),
        PyObject::str_val(CompactString::from(url.as_str())),
    );
    attrs.insert(
        CompactString::from("host"),
        PyObject::str_val(CompactString::from(
            parse_url_string(&url).host.as_str(),
        )),
    );
    attrs.insert(
        CompactString::from("type"),
        PyObject::str_val(CompactString::from(
            parse_url_string(&url).scheme.as_str(),
        )),
    );
    attrs.insert(
        CompactString::from("method"),
        PyObject::str_val(CompactString::from("GET")),
    );
    attrs.insert(
        CompactString::from("headers"),
        PyObject::dict(IndexMap::new()),
    );
    attrs.insert(
        CompactString::from("add_header"),
        PyObject::native_closure("add_header", |_args| Ok(PyObject::none())),
    );
    Ok(PyObject::module_with_attrs(
        CompactString::from("urllib.request.Request"),
        attrs,
    ))
}

// ════════════════════════════════════════════════════════════════════════
// urllib.parse module
// ════════════════════════════════════════════════════════════════════════

pub fn create_urllib_parse_module() -> PyObjectRef {
    make_module(
        "urllib.parse",
        vec![
            ("urlencode", make_builtin(urllib_parse_urlencode)),
            ("quote", make_builtin(urllib_parse_quote)),
            ("quote_plus", make_builtin(urllib_parse_quote_plus)),
            ("unquote", make_builtin(urllib_parse_unquote)),
            ("unquote_plus", make_builtin(urllib_parse_unquote_plus)),
            ("urlparse", make_builtin(urllib_parse_urlparse)),
            ("urljoin", make_builtin(urllib_parse_urljoin)),
            ("parse_qs", make_builtin(urllib_parse_parse_qs)),
            ("parse_qsl", make_builtin(urllib_parse_parse_qsl)),
        ],
    )
}

fn urllib_parse_urlencode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "urlencode() requires a mapping argument",
        ));
    }
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
                    percent_encode(&ks),
                    percent_encode(&v.py_to_string())
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
                            percent_encode(&pair[0].py_to_string()),
                            percent_encode(&pair[1].py_to_string())
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
    let s = args[0].py_to_string();
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

    // Build a ParseResult-like object with both named attributes and tuple behavior
    let scheme = PyObject::str_val(CompactString::from(&p.scheme));
    let netloc = PyObject::str_val(CompactString::from(&p.netloc));
    let path = PyObject::str_val(CompactString::from(&p.path));
    let params = PyObject::str_val(CompactString::from(""));
    let query = PyObject::str_val(CompactString::from(&p.query));
    let fragment = PyObject::str_val(CompactString::from(&p.fragment));

    let components = vec![
        scheme.clone(), netloc.clone(), path.clone(),
        params.clone(), query.clone(), fragment.clone(),
    ];

    let cls = PyObject::class(CompactString::from("ParseResult"), vec![], IndexMap::new());
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("scheme"), scheme);
    attrs.insert(CompactString::from("netloc"), netloc);
    attrs.insert(CompactString::from("path"), path);
    attrs.insert(CompactString::from("params"), params);
    attrs.insert(CompactString::from("query"), query);
    attrs.insert(CompactString::from("fragment"), fragment);
    attrs.insert(CompactString::from("hostname"), PyObject::str_val(CompactString::from(&p.host)));
    attrs.insert(CompactString::from("port"), PyObject::int(p.port as i64));

    // geturl()
    let url_c = url.clone();
    attrs.insert(
        CompactString::from("geturl"),
        PyObject::native_closure("geturl", move |_args| {
            Ok(PyObject::str_val(CompactString::from(url_c.as_str())))
        }),
    );

    // __iter__ for tuple-like unpacking
    let iter_components = components.clone();
    attrs.insert(
        CompactString::from("__iter__"),
        PyObject::native_closure("__iter__", move |_args| {
            Ok(PyObject::tuple(iter_components.clone()))
        }),
    );

    // __getitem__ for indexing
    let idx_components = components.clone();
    attrs.insert(
        CompactString::from("__getitem__"),
        PyObject::native_closure("__getitem__", move |args| {
            let idx = if !args.is_empty() { args[0].as_int().unwrap_or(0) } else { 0 };
            let i = if idx < 0 { (6 + idx) as usize } else { idx as usize };
            idx_components.get(i).cloned().ok_or_else(|| {
                PyException::index_error("tuple index out of range")
            })
        }),
    );

    // __len__
    attrs.insert(
        CompactString::from("__len__"),
        PyObject::native_closure("__len__", move |_args| Ok(PyObject::int(6))),
    );

    // __repr__
    let repr_components = components;
    attrs.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("__repr__", move |_args| {
            let parts: Vec<String> = repr_components.iter().map(|c| format!("'{}'", c.py_to_string())).collect();
            Ok(PyObject::str_val(CompactString::from(format!(
                "ParseResult(scheme={}, netloc={}, path={}, params={}, query={}, fragment={})",
                parts[0], parts[1], parts[2], parts[3], parts[4], parts[5]
            ))))
        }),
    );

    Ok(PyObject::instance_with_attrs(cls, attrs))
}

fn urllib_parse_urljoin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "urljoin() requires 2 arguments",
        ));
    }
    let base = args[0].py_to_string();
    let url = args[1].py_to_string();

    // If url is absolute, return it directly
    if url.contains("://") {
        return Ok(PyObject::str_val(CompactString::from(url)));
    }

    let bp = parse_url_string(&base);

    let result = if url.starts_with('/') {
        format!("{}://{}{}", bp.scheme, bp.netloc, url)
    } else if url.starts_with("//") {
        format!("{}:{}", bp.scheme, url)
    } else if url.is_empty() {
        base
    } else {
        // Relative path — resolve against base path
        let base_dir = if let Some(idx) = bp.path.rfind('/') {
            &bp.path[..=idx]
        } else {
            "/"
        };
        format!("{}://{}{}{}", bp.scheme, bp.netloc, base_dir, url)
    };

    Ok(PyObject::str_val(CompactString::from(result)))
}

fn urllib_parse_parse_qs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "parse_qs() requires a string argument",
        ));
    }
    let qs = args[0].py_to_string();
    let mut result: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();

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
        let hk = HashableKey::Str(CompactString::from(key.as_str()));
        let entry = result
            .entry(hk.clone())
            .or_insert_with(|| PyObject::list(vec![]));
        // Append to the list
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

// ════════════════════════════════════════════════════════════════════════
// http module
// ════════════════════════════════════════════════════════════════════════

fn http_reason(code: u16) -> &'static str {
    match code {
        100 => "Continue",
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}

pub fn create_http_module() -> PyObjectRef {
    // Build HTTPStatus as an object with named constants
    let mut status_attrs = IndexMap::new();
    let statuses: Vec<(i64, &str)> = vec![
        (100, "CONTINUE"),
        (200, "OK"),
        (201, "CREATED"),
        (204, "NO_CONTENT"),
        (301, "MOVED_PERMANENTLY"),
        (302, "FOUND"),
        (304, "NOT_MODIFIED"),
        (400, "BAD_REQUEST"),
        (401, "UNAUTHORIZED"),
        (403, "FORBIDDEN"),
        (404, "NOT_FOUND"),
        (405, "METHOD_NOT_ALLOWED"),
        (408, "REQUEST_TIMEOUT"),
        (500, "INTERNAL_SERVER_ERROR"),
        (502, "BAD_GATEWAY"),
        (503, "SERVICE_UNAVAILABLE"),
        (504, "GATEWAY_TIMEOUT"),
    ];
    for (code, name) in &statuses {
        status_attrs.insert(CompactString::from(*name), PyObject::int(*code));
    }
    let http_status = PyObject::module_with_attrs(CompactString::from("HTTPStatus"), status_attrs);

    // HTTPConnection class
    let http_connection_fn = make_builtin(http_connection_constructor);

    make_module(
        "http",
        vec![
            ("HTTPStatus", http_status),
            ("HTTPConnection", http_connection_fn.clone()),
            // http.client sub-attributes
            ("client", {
                let mut client_attrs = IndexMap::new();
                client_attrs.insert(
                    CompactString::from("HTTPConnection"),
                    http_connection_fn,
                );
                client_attrs.insert(
                    CompactString::from("HTTPSConnection"),
                    make_builtin(|_args| {
                        Err(PyException::os_error(
                            "HTTPS is not supported (no TLS available)",
                        ))
                    }),
                );
                // Status code constants on client module
                client_attrs.insert(CompactString::from("OK"), PyObject::int(200));
                client_attrs.insert(CompactString::from("NOT_FOUND"), PyObject::int(404));
                client_attrs.insert(
                    CompactString::from("INTERNAL_SERVER_ERROR"),
                    PyObject::int(500),
                );
                PyObject::module_with_attrs(CompactString::from("http.client"), client_attrs)
            }),
            // Common status codes at top level
            ("OK", PyObject::int(200)),
            ("CREATED", PyObject::int(201)),
            ("NO_CONTENT", PyObject::int(204)),
            ("MOVED_PERMANENTLY", PyObject::int(301)),
            ("FOUND", PyObject::int(302)),
            ("NOT_MODIFIED", PyObject::int(304)),
            ("BAD_REQUEST", PyObject::int(400)),
            ("UNAUTHORIZED", PyObject::int(401)),
            ("FORBIDDEN", PyObject::int(403)),
            ("NOT_FOUND", PyObject::int(404)),
            ("INTERNAL_SERVER_ERROR", PyObject::int(500)),
            ("BAD_GATEWAY", PyObject::int(502)),
            ("SERVICE_UNAVAILABLE", PyObject::int(503)),
        ],
    )
}

fn http_connection_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "HTTPConnection() requires a host argument",
        ));
    }
    let host = args[0].py_to_string();
    let port: u16 = if args.len() > 1 {
        args[1].as_int().unwrap_or(80) as u16
    } else {
        // Check if host contains port
        if let Some(idx) = host.rfind(':') {
            host[idx + 1..].parse().unwrap_or(80)
        } else {
            80
        }
    };
    let host_only = if let Some(idx) = host.rfind(':') {
        if host[idx + 1..].parse::<u16>().is_ok() {
            host[..idx].to_string()
        } else {
            host.clone()
        }
    } else {
        host.clone()
    };

    let conn_state: Arc<Mutex<HttpConnState>> =
        Arc::new(Mutex::new(HttpConnState {
            host: host_only,
            port,
            stream: None,
            response_data: None,
        }));

    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("host"),
        PyObject::str_val(CompactString::from(host.as_str())),
    );
    attrs.insert(CompactString::from("port"), PyObject::int(port as i64));

    // request(method, url, body=None, headers=None)
    let cs = conn_state.clone();
    attrs.insert(
        CompactString::from("request"),
        PyObject::native_closure("request", move |args| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "request() requires method and url arguments",
                ));
            }
            let method = args[0].py_to_string();
            let url = args[1].py_to_string();
            let body = if args.len() > 2 && !matches!(&args[2].payload, PyObjectPayload::None) {
                Some(args[2].py_to_string())
            } else {
                None
            };

            let mut extra_headers = IndexMap::new();
            if args.len() > 3 {
                if let PyObjectPayload::Dict(d) = &args[3].payload {
                    let d = d.read();
                    for (k, v) in d.iter() {
                        let ks = match k {
                            HashableKey::Str(s) => s.to_string(),
                            HashableKey::Int(i) => i.to_string(),
                            _ => format!("{:?}", k),
                        };
                        extra_headers.insert(ks, v.py_to_string());
                    }
                }
            }

            let mut guard = cs
                .lock()
                .map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;

            let addr = format!("{}:{}", guard.host, guard.port);
            let mut stream = TcpStream::connect(&addr)
                .map_err(|e| PyException::os_error(format!("HTTPConnection: {}", e)))?;
            stream.set_read_timeout(Some(Duration::from_secs(30))).ok();

            let mut req = format!(
                "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
                method, url, guard.host
            );
            for (k, v) in &extra_headers {
                req.push_str(&format!("{}: {}\r\n", k, v));
            }
            if let Some(ref b) = body {
                req.push_str(&format!("Content-Length: {}\r\n", b.len()));
            }
            req.push_str("\r\n");
            if let Some(ref b) = body {
                req.push_str(b);
            }

            stream
                .write_all(req.as_bytes())
                .map_err(|e| PyException::os_error(format!("HTTPConnection write: {}", e)))?;

            let mut raw = Vec::new();
            stream
                .read_to_end(&mut raw)
                .map_err(|e| PyException::os_error(format!("HTTPConnection read: {}", e)))?;

            guard.response_data = Some(raw);
            guard.stream = None;
            Ok(PyObject::none())
        }),
    );

    // getresponse() → response object
    let cs = conn_state.clone();
    attrs.insert(
        CompactString::from("getresponse"),
        PyObject::native_closure("getresponse", move |_args| {
            let mut guard = cs
                .lock()
                .map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;
            let raw = guard
                .response_data
                .take()
                .ok_or_else(|| PyException::runtime_error("no response available"))?;

            let raw_str = String::from_utf8_lossy(&raw);
            let header_end = raw_str.find("\r\n\r\n").unwrap_or(raw_str.len());
            let header_section = &raw_str[..header_end];
            let body_start = if header_end + 4 <= raw.len() {
                header_end + 4
            } else {
                raw.len()
            };
            let body = raw[body_start..].to_vec();

            let mut lines = header_section.lines();
            let status_line = lines.next().unwrap_or("HTTP/1.1 200 OK");
            let status_code: u16 = status_line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(200);

            let mut headers = IndexMap::new();
            for line in lines {
                if let Some(idx) = line.find(':') {
                    let key = line[..idx].trim().to_string();
                    let val = line[idx + 1..].trim().to_string();
                    headers.insert(key, val);
                }
            }

            let url_str = format!("http://{}:{}/", guard.host, guard.port);
            Ok(build_response_object(&url_str, status_code, headers, body))
        }),
    );

    // close()
    let cs = conn_state.clone();
    attrs.insert(
        CompactString::from("close"),
        PyObject::native_closure("close", move |_args| {
            let mut guard = cs
                .lock()
                .map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;
            guard.stream = None;
            guard.response_data = None;
            Ok(PyObject::none())
        }),
    );

    attrs.insert(
        CompactString::from("_bind_methods"),
        PyObject::bool_val(true),
    );

    Ok(PyObject::module_with_attrs(CompactString::from("http.client.HTTPConnection"), attrs))
}

struct HttpConnState {
    host: String,
    port: u16,
    stream: Option<TcpStream>,
    response_data: Option<Vec<u8>>,
}

// ── http.server module ──

pub fn create_http_server_module() -> PyObjectRef {
    let http_server_fn = make_builtin(|args: &[PyObjectRef]| {
        let addr = if !args.is_empty() {
            args[0].py_to_string()
        } else {
            "0.0.0.0:8000".to_string()
        };
        let cls = PyObject::class(CompactString::from("HTTPServer"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("server_address"), PyObject::str_val(CompactString::from(addr.as_str())));
            w.insert(CompactString::from("serve_forever"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("handle_request"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("shutdown"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("server_close"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
        }
        Ok(inst)
    });

    let base_handler_cls = PyObject::class(
        CompactString::from("BaseHTTPRequestHandler"), vec![], IndexMap::new(),
    );

    let simple_handler_cls = PyObject::class(
        CompactString::from("SimpleHTTPRequestHandler"), vec![], IndexMap::new(),
    );

    make_module("http.server", vec![
        ("HTTPServer", http_server_fn),
        ("BaseHTTPRequestHandler", base_handler_cls),
        ("SimpleHTTPRequestHandler", simple_handler_cls),
    ])
}

// ── http.cookiejar module ──

pub fn create_http_cookiejar_module() -> PyObjectRef {
    let cookiejar_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("CookieJar"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            let cookies: Arc<Mutex<Vec<PyObjectRef>>> = Arc::new(Mutex::new(vec![]));

            let c = cookies.clone();
            w.insert(CompactString::from("set_cookie"), PyObject::native_closure(
                "CookieJar.set_cookie", move |args: &[PyObjectRef]| {
                    if !args.is_empty() {
                        c.lock().unwrap().push(args[0].clone());
                    }
                    Ok(PyObject::none())
                }
            ));

            let c2 = cookies.clone();
            w.insert(CompactString::from("clear"), PyObject::native_closure(
                "CookieJar.clear", move |_args: &[PyObjectRef]| {
                    c2.lock().unwrap().clear();
                    Ok(PyObject::none())
                }
            ));

            let c3 = cookies.clone();
            w.insert(CompactString::from("__len__"), PyObject::native_closure(
                "CookieJar.__len__", move |_args: &[PyObjectRef]| {
                    Ok(PyObject::int(c3.lock().unwrap().len() as i64))
                }
            ));

            let c4 = cookies.clone();
            w.insert(CompactString::from("__iter__"), PyObject::native_closure(
                "CookieJar.__iter__", move |_args: &[PyObjectRef]| {
                    Ok(PyObject::list(c4.lock().unwrap().clone()))
                }
            ));
        }
        Ok(inst)
    });

    make_module("http.cookiejar", vec![
        ("CookieJar", cookiejar_fn),
        ("FileCookieJar", make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none()))),
        ("MozillaCookieJar", make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none()))),
    ])
}

// ── ssl module ──

pub fn create_ssl_module() -> PyObjectRef {
    let ssl_context_fn = make_builtin(|args: &[PyObjectRef]| {
        let protocol = if !args.is_empty() {
            args[0].to_int().unwrap_or(2)
        } else {
            2
        };
        let cls = PyObject::class(CompactString::from("SSLContext"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("protocol"), PyObject::int(protocol));

            w.insert(CompactString::from("wrap_socket"), make_builtin(|args: &[PyObjectRef]| {
                if !args.is_empty() {
                    Ok(args[0].clone())
                } else {
                    Ok(PyObject::none())
                }
            }));

            w.insert(CompactString::from("load_cert_chain"), make_builtin(|_args: &[PyObjectRef]| {
                Ok(PyObject::none())
            }));

            w.insert(CompactString::from("load_verify_locations"), make_builtin(|_args: &[PyObjectRef]| {
                Ok(PyObject::none())
            }));

            w.insert(CompactString::from("set_ciphers"), make_builtin(|_args: &[PyObjectRef]| {
                Ok(PyObject::none())
            }));

            w.insert(CompactString::from("set_default_verify_paths"), make_builtin(|_args: &[PyObjectRef]| {
                Ok(PyObject::none())
            }));

            w.insert(CompactString::from("check_hostname"), PyObject::bool_val(true));
            w.insert(CompactString::from("verify_mode"), PyObject::int(2));
        }
        Ok(inst)
    });

    let create_default_context_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("SSLContext"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("protocol"), PyObject::int(2));
            w.insert(CompactString::from("check_hostname"), PyObject::bool_val(true));
            w.insert(CompactString::from("verify_mode"), PyObject::int(2));
            w.insert(CompactString::from("wrap_socket"), make_builtin(|args: &[PyObjectRef]| {
                if !args.is_empty() { Ok(args[0].clone()) } else { Ok(PyObject::none()) }
            }));
            w.insert(CompactString::from("load_cert_chain"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("load_verify_locations"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("set_ciphers"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
            w.insert(CompactString::from("set_default_verify_paths"), make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none())));
        }
        Ok(inst)
    });

    make_module("ssl", vec![
        ("SSLContext", ssl_context_fn),
        ("create_default_context", create_default_context_fn),
        ("SSLError", PyObject::exception_type(ExceptionKind::OSError)),
        ("SSLCertVerificationError", PyObject::exception_type(ExceptionKind::OSError)),
        ("PROTOCOL_TLS", PyObject::int(2)),
        ("PROTOCOL_TLS_CLIENT", PyObject::int(16)),
        ("PROTOCOL_TLS_SERVER", PyObject::int(17)),
        ("PROTOCOL_SSLv23", PyObject::int(2)),
        ("CERT_NONE", PyObject::int(0)),
        ("CERT_OPTIONAL", PyObject::int(1)),
        ("CERT_REQUIRED", PyObject::int(2)),
        ("OP_NO_SSLv2", PyObject::int(0x01000000)),
        ("OP_NO_SSLv3", PyObject::int(0x02000000)),
        ("OP_NO_TLSv1", PyObject::int(0x04000000)),
        ("HAS_SNI", PyObject::bool_val(true)),
        ("HAS_ECDH", PyObject::bool_val(true)),
        ("HAS_NPN", PyObject::bool_val(false)),
        ("HAS_ALPN", PyObject::bool_val(true)),
        ("OPENSSL_VERSION", PyObject::str_val(CompactString::from("OpenSSL 3.0.0 (stub)"))),
        ("OPENSSL_VERSION_NUMBER", PyObject::int(0x30000000)),
    ])
}
