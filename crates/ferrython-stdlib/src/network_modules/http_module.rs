//! HTTP, urllib, and SSL stdlib modules.

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
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

#[allow(dead_code)]
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

/// Like percent_encode but encodes spaces as '+' (application/x-www-form-urlencoded).
fn quote_plus_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            b' ' => result.push('+'),
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

fn build_http_request(parsed: &ParsedUrl, method: &str, headers: &IndexMap<String, String>, body: Option<&[u8]>) -> Vec<u8> {
    let full_path = if parsed.query.is_empty() {
        parsed.path.clone()
    } else {
        format!("{}?{}", parsed.path, parsed.query)
    };
    let mut req = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: ferrython/1.0\r\nAccept: */*\r\n",
        method, full_path, parsed.host
    );
    for (k, v) in headers {
        req.push_str(&format!("{}: {}\r\n", k, v));
    }
    if let Some(data) = body {
        if !headers.contains_key("Content-Length") {
            req.push_str(&format!("Content-Length: {}\r\n", data.len()));
        }
        if !headers.contains_key("Content-Type") {
            req.push_str("Content-Type: application/x-www-form-urlencoded\r\n");
        }
    }
    req.push_str("\r\n");
    let mut bytes = req.into_bytes();
    if let Some(data) = body {
        bytes.extend_from_slice(data);
    }
    bytes
}

fn do_http_request(url: &str, method: &str, headers: &IndexMap<String, String>, data: Option<&[u8]>) -> PyResult<(u16, IndexMap<String, String>, Vec<u8>)> {
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

    let request = build_http_request(&parsed, method, headers, data);
    stream
        .write_all(&request)
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

    // Extract URL, method, headers, and data from args
    let (url, method, headers, data) = if let Some(u) = args[0].get_attr("full_url") {
        // Request object
        let url = u.py_to_string();
        let method = args[0].get_attr("method")
            .map(|m| m.py_to_string())
            .unwrap_or_else(|| "GET".to_string());
        let data_bytes = args[0].get_attr("data").and_then(|d| {
            match &d.payload {
                PyObjectPayload::Bytes(b) => Some(b.clone()),
                PyObjectPayload::None => None,
                _ => Some(d.py_to_string().into_bytes()),
            }
        });
        let mut hdrs = IndexMap::new();
        if let Some(hdr_obj) = args[0].get_attr("headers") {
            if let PyObjectPayload::Dict(map) = &hdr_obj.payload {
                for (k, v) in map.read().iter() {
                    if let HashableKey::Str(key) = k {
                        hdrs.insert(key.to_string(), v.py_to_string());
                    }
                }
            }
        }
        (url, method, hdrs, data_bytes)
    } else {
        // Plain string URL
        let url = args[0].py_to_string();
        // Check for data= kwarg (second arg or trailing dict)
        let data_bytes = if args.len() > 1 && !matches!(&args[1].payload, PyObjectPayload::None) {
            match &args[1].payload {
                PyObjectPayload::Bytes(b) => Some(b.clone()),
                _ => Some(args[1].py_to_string().into_bytes()),
            }
        } else {
            None
        };
        let method = if data_bytes.is_some() { "POST" } else { "GET" };
        (url, method.to_string(), IndexMap::new(), data_bytes)
    };

    let (status, resp_headers, body) = do_http_request(
        &url, &method, &headers, data.as_deref()
    )?;
    Ok(build_response_object(&url, status, resp_headers, body))
}

fn urllib_request_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "Request() requires a url argument",
        ));
    }
    let url = args[0].py_to_string();

    // Parse data= (2nd arg) and method= (kwarg or 3rd arg)
    let data = if args.len() > 1 && !matches!(&args[1].payload, PyObjectPayload::None) {
        args[1].clone()
    } else {
        PyObject::none()
    };

    // Extract headers and method from kwargs dict if present
    let mut extra_headers = IndexMap::new();
    let mut method = if matches!(&data.payload, PyObjectPayload::None) {
        "GET".to_string()
    } else {
        "POST".to_string()
    };

    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(kw) = &last.payload {
            let r = kw.read();
            if let Some(m) = r.get(&HashableKey::Str(CompactString::from("method"))) {
                method = m.py_to_string();
            }
            if let Some(h) = r.get(&HashableKey::Str(CompactString::from("headers"))) {
                if let PyObjectPayload::Dict(hm) = &h.payload {
                    for (k, v) in hm.read().iter() {
                        if let HashableKey::Str(key) = k {
                            extra_headers.insert(
                                HashableKey::Str(key.clone()),
                                v.clone(),
                            );
                        }
                    }
                }
            }
        }
    }

    let parsed = parse_url_string(&url);
    let headers_dict = PyObject::dict(extra_headers);

    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("full_url"), PyObject::str_val(CompactString::from(url.as_str())));
    attrs.insert(CompactString::from("host"), PyObject::str_val(CompactString::from(parsed.host.as_str())));
    attrs.insert(CompactString::from("type"), PyObject::str_val(CompactString::from(parsed.scheme.as_str())));
    attrs.insert(CompactString::from("method"), PyObject::str_val(CompactString::from(method)));
    attrs.insert(CompactString::from("data"), data);
    attrs.insert(CompactString::from("headers"), headers_dict);

    // add_header(key, value) — add a header to the request
    let req_attrs = Arc::new(Mutex::new(attrs.clone()));
    let ra = req_attrs.clone();
    attrs.insert(CompactString::from("add_header"), PyObject::native_closure("add_header", move |a: &[PyObjectRef]| {
        if a.len() < 2 { return Err(PyException::type_error("add_header(key, value)")); }
        let key = a[0].py_to_string();
        let val = a[1].py_to_string();
        let mut locked = ra.lock().unwrap();
        if let Some(hdr) = locked.get_mut("headers") {
            if let PyObjectPayload::Dict(map) = &hdr.payload {
                map.write().insert(HashableKey::Str(CompactString::from(key)), PyObject::str_val(CompactString::from(val)));
            }
        }
        Ok(PyObject::none())
    }));

    Ok(PyObject::module_with_attrs(CompactString::from("urllib.request.Request"), attrs))
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
                    quote_plus_encode(&ks),
                    quote_plus_encode(&v.py_to_string())
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
                            quote_plus_encode(&pair[0].py_to_string()),
                            quote_plus_encode(&pair[1].py_to_string())
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

    let raw_path = if url.starts_with('/') {
        return Ok(PyObject::str_val(CompactString::from(
            format!("{}://{}{}", bp.scheme, bp.netloc, normalize_path(&url))
        )));
    } else if url.starts_with("//") {
        return Ok(PyObject::str_val(CompactString::from(
            format!("{}:{}", bp.scheme, url)
        )));
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
            "." | "" => { if segments.is_empty() { segments.push(""); } }
            ".." => { if segments.len() > 1 { segments.pop(); } }
            _ => segments.push(seg),
        }
    }
    let result = segments.join("/");
    if result.is_empty() { "/".to_string() } else { result }
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
                // HTTPResponse class
                client_attrs.insert(
                    CompactString::from("HTTPResponse"),
                    PyObject::class(CompactString::from("HTTPResponse"), vec![], IndexMap::new()),
                );
                // Common exception classes
                client_attrs.insert(
                    CompactString::from("HTTPException"),
                    PyObject::builtin_type(CompactString::from("HTTPException")),
                );
                client_attrs.insert(
                    CompactString::from("RemoteDisconnected"),
                    PyObject::builtin_type(CompactString::from("RemoteDisconnected")),
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

    // Extract timeout kwarg (positional arg 2 or keyword)
    let timeout_secs: u64 = if args.len() > 2 {
        args[2].as_int().unwrap_or(30) as u64
    } else {
        30
    };

    let conn_state: Arc<Mutex<HttpConnState>> =
        Arc::new(Mutex::new(HttpConnState {
            host: host_only,
            port,
            timeout_secs,
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
            let timeout = Duration::from_secs(guard.timeout_secs);
            let socket_addr: std::net::SocketAddr = addr.parse()
                .or_else(|_| {
                    // DNS resolution
                    use std::net::ToSocketAddrs;
                    addr.to_socket_addrs()
                        .map_err(|e| PyException::os_error(format!("HTTPConnection DNS: {}", e)))
                        .and_then(|mut addrs| addrs.next().ok_or_else(|| {
                            PyException::os_error(format!("HTTPConnection: could not resolve {}", addr))
                        }))
                })?;
            let mut stream = TcpStream::connect_timeout(&socket_addr, timeout)
                .map_err(|e| PyException::os_error(format!("HTTPConnection: {}", e)))?;
            stream.set_read_timeout(Some(timeout)).ok();
            stream.set_write_timeout(Some(timeout)).ok();

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
    timeout_secs: u64,
    stream: Option<TcpStream>,
    response_data: Option<Vec<u8>>,
}

// ── http.server module ──

/// Map common extensions to MIME types.
fn guess_content_type(path: &str) -> &'static str {
    if let Some(ext) = path.rsplit('.').next() {
        match ext.to_ascii_lowercase().as_str() {
            "html" | "htm" => "text/html; charset=utf-8",
            "css" => "text/css",
            "js" => "application/javascript",
            "json" => "application/json",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            "ico" => "image/x-icon",
            "txt" => "text/plain; charset=utf-8",
            "xml" => "application/xml",
            "pdf" => "application/pdf",
            "wasm" => "application/wasm",
            "zip" => "application/zip",
            "gz" | "tgz" => "application/gzip",
            "tar" => "application/x-tar",
            "mp3" => "audio/mpeg",
            "mp4" => "video/mp4",
            "webp" => "image/webp",
            "woff" => "font/woff",
            "woff2" => "font/woff2",
            _ => "application/octet-stream",
        }
    } else {
        "application/octet-stream"
    }
}

/// HTTP reason phrase for a status code.
fn http_status_reason(code: u16) -> &'static str {
    match code {
        100 => "Continue",
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        _ => "Unknown",
    }
}

/// Parsed HTTP request data shared between Rust and the Python handler object.
struct HttpRequest {
    method: String,
    path: String,
    version: String,
    headers: IndexMap<String, String>,
    body: Vec<u8>,
}

/// Parse an HTTP/1.x request from a buffered reader.
fn parse_http_request(reader: &mut BufReader<&mut TcpStream>) -> Option<HttpRequest> {
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).ok()? == 0 {
        return None;
    }
    let request_line = request_line.trim_end().to_string();
    let parts: Vec<&str> = request_line.splitn(3, ' ').collect();
    if parts.len() < 2 {
        return None;
    }
    let method = parts[0].to_string();
    let path = parts[1].to_string();
    let version = if parts.len() > 2 {
        parts[2].to_string()
    } else {
        "HTTP/1.0".to_string()
    };

    // Parse headers
    let mut headers = IndexMap::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(idx) = trimmed.find(':') {
            let key = trimmed[..idx].trim().to_string();
            let val = trimmed[idx + 1..].trim().to_string();
            headers.insert(key, val);
        }
    }

    // Read body if Content-Length is present
    let body = if let Some(cl) = headers.get("Content-Length").or_else(|| headers.get("content-length")) {
        if let Ok(len) = cl.parse::<usize>() {
            let mut buf = vec![0u8; len];
            reader.read_exact(&mut buf).ok()?;
            buf
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    Some(HttpRequest {
        method,
        path,
        version,
        headers,
        body,
    })
}

/// Write an HTTP error response directly on the stream.
fn write_error_response(stream: &mut TcpStream, code: u16, message: &str) {
    let reason = http_status_reason(code);
    let body = format!(
        "<html><head><title>Error {code}</title></head>\
         <body><h1>Error {code}: {reason}</h1><p>{message}</p></body></html>"
    );
    let response = format!(
        "HTTP/1.1 {code} {reason}\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

/// Build a Python handler instance for one request, populating command/path/headers/etc.
/// Returns the instance and the response buffer it will write into.
fn build_handler_instance(
    req: &HttpRequest,
    handler_cls: &PyObjectRef,
) -> (PyObjectRef, Arc<Mutex<Vec<u8>>>) {
    let inst = if let PyObjectPayload::Class(_) = &handler_cls.payload {
        PyObject::instance(handler_cls.clone())
    } else {
        let cls = PyObject::class(CompactString::from("BaseHTTPRequestHandler"), vec![], IndexMap::new());
        PyObject::instance(cls)
    };

    let wfile_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));

    if let PyObjectPayload::Instance(ref data) = inst.payload {
        let mut w = data.attrs.write();
        w.insert(CompactString::from("command"), PyObject::str_val(CompactString::from(req.method.as_str())));
        w.insert(CompactString::from("path"), PyObject::str_val(CompactString::from(req.path.as_str())));
        w.insert(CompactString::from("request_version"), PyObject::str_val(CompactString::from(req.version.as_str())));

        // headers as a dict
        let mut hdr_map = IndexMap::new();
        for (k, v) in &req.headers {
            hdr_map.insert(
                HashableKey::Str(CompactString::from(k.as_str())),
                PyObject::str_val(CompactString::from(v.as_str())),
            );
        }
        w.insert(CompactString::from("headers"), PyObject::dict(hdr_map));

        // rfile — a readable object wrapping the body
        let body_data = Arc::new(req.body.clone());
        let body_pos = Arc::new(Mutex::new(0usize));
        let bd = body_data.clone();
        let bp = body_pos.clone();
        w.insert(
            CompactString::from("rfile"),
            {
                let mut rfile_attrs = IndexMap::new();
                let bd2 = bd.clone();
                let bp2 = bp.clone();
                rfile_attrs.insert(
                    CompactString::from("read"),
                    PyObject::native_closure("rfile.read", move |args| {
                        let n = if !args.is_empty() { args[0].as_int().unwrap_or(-1) } else { -1 };
                        let mut p = bp2.lock().unwrap();
                        let remaining = &bd2[*p..];
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
                rfile_attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));
                PyObject::module_with_attrs(CompactString::from("rfile"), rfile_attrs)
            },
        );

        // wfile — a writable buffer that accumulates the response body
        let wbuf = wfile_buf.clone();
        let mut wfile_attrs = IndexMap::new();
        let wbuf2 = wbuf.clone();
        wfile_attrs.insert(
            CompactString::from("write"),
            PyObject::native_closure("wfile.write", move |args| {
                if !args.is_empty() {
                    let data = match &args[0].payload {
                        PyObjectPayload::Bytes(b) => b.clone(),
                        PyObjectPayload::Str(s) => s.as_bytes().to_vec(),
                        _ => args[0].py_to_string().into_bytes(),
                    };
                    wbuf2.lock().unwrap().extend_from_slice(&data);
                    Ok(PyObject::int(data.len() as i64))
                } else {
                    Ok(PyObject::int(0))
                }
            }),
        );
        let wbuf3 = wbuf.clone();
        wfile_attrs.insert(
            CompactString::from("flush"),
            PyObject::native_closure("wfile.flush", move |_args| {
                let _ = &wbuf3; // keep reference alive
                Ok(PyObject::none())
            }),
        );
        wfile_attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));
        w.insert(
            CompactString::from("wfile"),
            PyObject::module_with_attrs(CompactString::from("wfile"), wfile_attrs),
        );

        // _response_headers accumulates header lines before end_headers flushes them
        let resp_headers: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let resp_status: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

        // send_response(code, message=None)
        let rs = resp_status.clone();
        let wb = wfile_buf.clone();
        w.insert(
            CompactString::from("send_response"),
            PyObject::native_closure("send_response", move |args| {
                let code = if !args.is_empty() { args[0].as_int().unwrap_or(200) as u16 } else { 200 };
                let message = if args.len() > 1 {
                    args[1].py_to_string()
                } else {
                    http_status_reason(code).to_string()
                };
                let line = format!("HTTP/1.1 {} {}\r\n", code, message);
                *rs.lock().unwrap() = Some(line.clone());
                wb.lock().unwrap().extend_from_slice(line.as_bytes());
                Ok(PyObject::none())
            }),
        );

        // send_header(keyword, value)
        let rh = resp_headers.clone();
        let wb = wfile_buf.clone();
        w.insert(
            CompactString::from("send_header"),
            PyObject::native_closure("send_header", move |args| {
                if args.len() >= 2 {
                    let key = args[0].py_to_string();
                    let val = args[1].py_to_string();
                    let line = format!("{}: {}\r\n", key, val);
                    rh.lock().unwrap().push(line.clone());
                    wb.lock().unwrap().extend_from_slice(line.as_bytes());
                }
                Ok(PyObject::none())
            }),
        );

        // end_headers()
        let wb = wfile_buf.clone();
        w.insert(
            CompactString::from("end_headers"),
            PyObject::native_closure("end_headers", move |_args| {
                wb.lock().unwrap().extend_from_slice(b"\r\n");
                Ok(PyObject::none())
            }),
        );

        // send_error(code, message=None)
        let wb = wfile_buf.clone();
        w.insert(
            CompactString::from("send_error"),
            PyObject::native_closure("send_error", move |args| {
                let code = if !args.is_empty() { args[0].as_int().unwrap_or(500) as u16 } else { 500 };
                let message = if args.len() > 1 {
                    args[1].py_to_string()
                } else {
                    http_status_reason(code).to_string()
                };
                let reason = http_status_reason(code);
                let body = format!(
                    "<html><head><title>Error {code}</title></head>\
                     <body><h1>Error {code}: {reason}</h1><p>{message}</p></body></html>"
                );
                let resp = format!(
                    "HTTP/1.1 {code} {reason}\r\n\
                     Content-Type: text/html; charset=utf-8\r\n\
                     Content-Length: {}\r\n\
                     Connection: close\r\n\r\n{body}",
                    body.len()
                );
                wb.lock().unwrap().clear();
                wb.lock().unwrap().extend_from_slice(resp.as_bytes());
                Ok(PyObject::none())
            }),
        );

        // Stub do_GET / do_POST / do_PUT / do_DELETE / do_HEAD — return 501
        for method_name in &["do_GET", "do_POST", "do_PUT", "do_DELETE", "do_HEAD", "do_PATCH", "do_OPTIONS"] {
            let wb = wfile_buf.clone();
            let mname = *method_name;
            w.insert(
                CompactString::from(mname),
                PyObject::native_closure(mname, move |_args| {
                    let body = format!(
                        "<html><body><h1>501 Not Implemented</h1><p>{} not implemented</p></body></html>",
                        mname
                    );
                    let resp = format!(
                        "HTTP/1.1 501 Not Implemented\r\n\
                         Content-Type: text/html; charset=utf-8\r\n\
                         Content-Length: {}\r\n\
                         Connection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    wb.lock().unwrap().clear();
                    wb.lock().unwrap().extend_from_slice(resp.as_bytes());
                    Ok(PyObject::none())
                }),
            );
        }

        // log_message — no-op for now
        w.insert(
            CompactString::from("log_message"),
            PyObject::native_closure("log_message", |_args| Ok(PyObject::none())),
        );
    }

    (inst, wfile_buf)
}

/// Build a SimpleHTTPRequestHandler-style do_GET that serves files from cwd.
fn simple_handler_do_get(wfile_buf: Arc<Mutex<Vec<u8>>>, head_only: bool) -> PyObjectRef {
    let name = if head_only { "do_HEAD" } else { "do_GET" };
    PyObject::native_closure(name, move |args| {
        // args[0] is `self` (the handler instance) when called as a bound method,
        // but in our native_closure pattern it might not be passed.
        // We look for the `path` attribute on self if available.
        let request_path = if !args.is_empty() {
            if let Some(p) = args[0].get_attr("path") {
                p.py_to_string()
            } else {
                args[0].py_to_string()
            }
        } else {
            "/".to_string()
        };

        // Strip query string
        let fs_path = if let Some(idx) = request_path.find('?') {
            &request_path[..idx]
        } else {
            request_path.as_str()
        };

        // Decode percent-encoding and normalise
        let decoded = percent_decode(fs_path);
        let rel_path = decoded.trim_start_matches('/');
        let target = if rel_path.is_empty() {
            std::path::PathBuf::from(".")
        } else {
            std::path::PathBuf::from(rel_path)
        };

        let mut buf = wfile_buf.lock().unwrap();
        buf.clear();

        if target.is_dir() {
            // Try index.html first
            let index = target.join("index.html");
            if index.is_file() {
                match std::fs::read(&index) {
                    Ok(contents) => {
                        let header = format!(
                            "HTTP/1.1 200 OK\r\n\
                             Content-Type: text/html; charset=utf-8\r\n\
                             Content-Length: {}\r\n\
                             Connection: close\r\n\r\n",
                            contents.len()
                        );
                        buf.extend_from_slice(header.as_bytes());
                        if !head_only {
                            buf.extend_from_slice(&contents);
                        }
                    }
                    Err(_) => {
                        let body = b"<html><body><h1>500 Internal Server Error</h1></body></html>";
                        let header = format!(
                            "HTTP/1.1 500 Internal Server Error\r\n\
                             Content-Type: text/html; charset=utf-8\r\n\
                             Content-Length: {}\r\n\
                             Connection: close\r\n\r\n",
                            body.len()
                        );
                        buf.extend_from_slice(header.as_bytes());
                        if !head_only {
                            buf.extend_from_slice(body);
                        }
                    }
                }
            } else {
                // Directory listing
                let mut body = String::from("<html><head><title>Directory listing</title></head><body>\n");
                body.push_str(&format!("<h1>Directory listing for /{}</h1>\n<hr><ul>\n", rel_path));
                if let Ok(entries) = std::fs::read_dir(&target) {
                    let mut names: Vec<String> = entries
                        .filter_map(|e| e.ok())
                        .map(|e| {
                            let name = e.file_name().to_string_lossy().to_string();
                            if e.path().is_dir() {
                                format!("{}/", name)
                            } else {
                                name
                            }
                        })
                        .collect();
                    names.sort();
                    for name in &names {
                        let href = if rel_path.is_empty() {
                            format!("/{}", name)
                        } else {
                            format!("/{}/{}", rel_path, name)
                        };
                        body.push_str(&format!("<li><a href=\"{}\">{}</a></li>\n", href, name));
                    }
                }
                body.push_str("</ul><hr></body></html>\n");
                let body_bytes = body.as_bytes();
                let header = format!(
                    "HTTP/1.1 200 OK\r\n\
                     Content-Type: text/html; charset=utf-8\r\n\
                     Content-Length: {}\r\n\
                     Connection: close\r\n\r\n",
                    body_bytes.len()
                );
                buf.extend_from_slice(header.as_bytes());
                if !head_only {
                    buf.extend_from_slice(body_bytes);
                }
            }
        } else if target.is_file() {
            match std::fs::read(&target) {
                Ok(contents) => {
                    let ctype = guess_content_type(target.to_str().unwrap_or(""));
                    let header = format!(
                        "HTTP/1.1 200 OK\r\n\
                         Content-Type: {}\r\n\
                         Content-Length: {}\r\n\
                         Connection: close\r\n\r\n",
                        ctype,
                        contents.len()
                    );
                    buf.extend_from_slice(header.as_bytes());
                    if !head_only {
                        buf.extend_from_slice(&contents);
                    }
                }
                Err(_) => {
                    let body = b"<html><body><h1>403 Forbidden</h1></body></html>";
                    let header = format!(
                        "HTTP/1.1 403 Forbidden\r\n\
                         Content-Type: text/html; charset=utf-8\r\n\
                         Content-Length: {}\r\n\
                         Connection: close\r\n\r\n",
                        body.len()
                    );
                    buf.extend_from_slice(header.as_bytes());
                    if !head_only {
                        buf.extend_from_slice(body);
                    }
                }
            }
        } else {
            let body = b"<html><body><h1>404 Not Found</h1></body></html>";
            let header = format!(
                "HTTP/1.1 404 Not Found\r\n\
                 Content-Type: text/html; charset=utf-8\r\n\
                 Content-Length: {}\r\n\
                 Connection: close\r\n\r\n",
                body.len()
            );
            buf.extend_from_slice(header.as_bytes());
            if !head_only {
                buf.extend_from_slice(body);
            }
        }

        Ok(PyObject::none())
    })
}

/// State for a running HTTPServer.
#[allow(dead_code)]
struct HttpServerState {
    listener: Option<TcpListener>,
    host: String,
    port: u16,
}

pub fn create_http_server_module() -> PyObjectRef {
    // ── HTTPServer(server_address, RequestHandlerClass) ──
    let http_server_fn = make_builtin(|args: &[PyObjectRef]| {
        // server_address is a (host, port) tuple
        let (host, port) = if !args.is_empty() {
            let addr_obj = &args[0];
            match &addr_obj.payload {
                PyObjectPayload::Tuple(items) if items.len() >= 2 => {
                    let h = items[0].py_to_string();
                    let p = items[1].as_int().unwrap_or(8000) as u16;
                    let h = if h.is_empty() { "0.0.0.0".to_string() } else { h };
                    (h, p)
                }
                _ => {
                    let s = addr_obj.py_to_string();
                    if let Some(idx) = s.rfind(':') {
                        let port = s[idx + 1..].parse::<u16>().unwrap_or(8000);
                        (s[..idx].to_string(), port)
                    } else {
                        ("0.0.0.0".to_string(), 8000)
                    }
                }
            }
        } else {
            ("0.0.0.0".to_string(), 8000)
        };

        // Capture the handler class (second argument) if provided
        let handler_cls = if args.len() > 1 {
            args[1].clone()
        } else {
            PyObject::class(CompactString::from("BaseHTTPRequestHandler"), vec![], IndexMap::new())
        };

        let addr_str = format!("{}:{}", host, port);
        let listener = TcpListener::bind(&addr_str)
            .map_err(|e| PyException::os_error(format!("HTTPServer bind({}): {}", addr_str, e)))?;

        // Allow non-blocking accept for shutdown support
        listener
            .set_nonblocking(false)
            .map_err(|e| PyException::os_error(format!("set_nonblocking: {}", e)))?;

        let server_state = Arc::new(Mutex::new(HttpServerState {
            listener: Some(listener),
            host: host.clone(),
            port,
        }));

        let running = Arc::new(AtomicBool::new(false));

        let cls = PyObject::class(CompactString::from("HTTPServer"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();

            // server_address attribute — (host, port) tuple
            w.insert(
                CompactString::from("server_address"),
                PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from(host.as_str())),
                    PyObject::int(port as i64),
                ]),
            );

            // server_name / server_port for compatibility
            w.insert(CompactString::from("server_name"), PyObject::str_val(CompactString::from(host.as_str())));
            w.insert(CompactString::from("server_port"), PyObject::int(port as i64));

            // ── serve_forever(poll_interval=0.5) ──
            let ss = server_state.clone();
            let r = running.clone();
            let hcls = handler_cls.clone();
            w.insert(
                CompactString::from("serve_forever"),
                PyObject::native_closure("serve_forever", move |_args| {
                    r.store(true, Ordering::SeqCst);
                    loop {
                        if !r.load(Ordering::SeqCst) {
                            break;
                        }
                        let listener_clone = {
                            let guard = ss.lock().map_err(|e| {
                                PyException::runtime_error(format!("lock: {}", e))
                            })?;
                            match &guard.listener {
                                Some(l) => l.try_clone().map_err(|e| {
                                    PyException::os_error(format!("try_clone: {}", e))
                                })?,
                                None => return Ok(PyObject::none()),
                            }
                        };

                        // Set a short timeout so we can check the running flag periodically
                        let _ = listener_clone.set_nonblocking(true);

                        match listener_clone.accept() {
                            Ok((mut stream, _addr)) => {
                                let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
                                let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
                                handle_one_connection(&mut stream, &hcls);
                            }
                            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(Duration::from_millis(100));
                                continue;
                            }
                            Err(_) => {
                                std::thread::sleep(Duration::from_millis(50));
                                continue;
                            }
                        }
                    }
                    Ok(PyObject::none())
                }),
            );

            // ── handle_request() ── handle exactly one request
            let ss = server_state.clone();
            let hcls = handler_cls.clone();
            w.insert(
                CompactString::from("handle_request"),
                PyObject::native_closure("handle_request", move |_args| {
                    let listener_clone = {
                        let guard = ss.lock().map_err(|e| {
                            PyException::runtime_error(format!("lock: {}", e))
                        })?;
                        match &guard.listener {
                            Some(l) => l.try_clone().map_err(|e| {
                                PyException::os_error(format!("try_clone: {}", e))
                            })?,
                            None => {
                                return Err(PyException::runtime_error(
                                    "server is closed",
                                ));
                            }
                        }
                    };
                    match listener_clone.accept() {
                        Ok((mut stream, _addr)) => {
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
                            let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
                            handle_one_connection(&mut stream, &hcls);
                            Ok(PyObject::none())
                        }
                        Err(e) => Err(PyException::os_error(format!(
                            "accept: {}",
                            e
                        ))),
                    }
                }),
            );

            // ── shutdown() ── signal serve_forever to stop
            let r = running.clone();
            w.insert(
                CompactString::from("shutdown"),
                PyObject::native_closure("shutdown", move |_args| {
                    r.store(false, Ordering::SeqCst);
                    Ok(PyObject::none())
                }),
            );

            // ── server_close() ── drop the listener
            let ss = server_state.clone();
            w.insert(
                CompactString::from("server_close"),
                PyObject::native_closure("server_close", move |_args| {
                    let mut guard = ss.lock().map_err(|e| {
                        PyException::runtime_error(format!("lock: {}", e))
                    })?;
                    guard.listener = None;
                    Ok(PyObject::none())
                }),
            );

            // ── socket attribute (for fileno() etc.) ──
            w.insert(
                CompactString::from("socket"),
                PyObject::none(),
            );
        }
        Ok(inst)
    });

    // ── BaseHTTPRequestHandler ──
    // A callable class: calling it with (request, client_address, server) returns a handler instance
    let base_handler_fn = make_builtin(|args: &[PyObjectRef]| {
        // When used as a constructor, we just build a handler with empty request
        let req = HttpRequest {
            method: String::new(),
            path: String::new(),
            version: "HTTP/1.1".to_string(),
            headers: IndexMap::new(),
            body: Vec::new(),
        };
        let dummy_cls = PyObject::class(CompactString::from("BaseHTTPRequestHandler"), vec![], IndexMap::new());
        let (inst, _wbuf) = build_handler_instance(&req, &dummy_cls);

        // If client_address was provided, store it
        if args.len() > 1 {
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("client_address"), args[1].clone());
            }
        }
        if args.len() > 2 {
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("server"), args[2].clone());
            }
        }

        Ok(inst)
    });

    // ── SimpleHTTPRequestHandler ──
    let simple_handler_fn = make_builtin(|args: &[PyObjectRef]| {
        let req = HttpRequest {
            method: "GET".to_string(),
            path: "/".to_string(),
            version: "HTTP/1.1".to_string(),
            headers: IndexMap::new(),
            body: Vec::new(),
        };
        let cls = PyObject::class(CompactString::from("SimpleHTTPRequestHandler"), vec![], IndexMap::new());
        let (inst, wbuf) = build_handler_instance(&req, &cls);

        // Override do_GET and do_HEAD with file-serving implementations
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("do_GET"), simple_handler_do_get(wbuf.clone(), false));
            w.insert(CompactString::from("do_HEAD"), simple_handler_do_get(wbuf.clone(), true));

            if args.len() > 1 {
                w.insert(CompactString::from("client_address"), args[1].clone());
            }
            if args.len() > 2 {
                w.insert(CompactString::from("server"), args[2].clone());
            }
        }

        Ok(inst)
    });

    make_module("http.server", vec![
        ("HTTPServer", http_server_fn),
        ("BaseHTTPRequestHandler", base_handler_fn),
        ("SimpleHTTPRequestHandler", simple_handler_fn),
    ])
}

/// Handle one HTTP connection: parse request, dispatch to handler, write response.
fn handle_one_connection(stream: &mut TcpStream, handler_cls: &PyObjectRef) {
    let req = {
        let mut reader = BufReader::new(&mut *stream);
        match parse_http_request(&mut reader) {
            Some(r) => r,
            None => return,
        }
    };

    let method = req.method.clone();
    let (handler_inst, wfile_buf) = build_handler_instance(&req, handler_cls);

    // If the handler_cls is a SimpleHTTPRequestHandler (has file-serving do_GET),
    // attach the file-serving handlers.  Otherwise check if the class defines
    // custom do_METHOD handlers and copy them onto the instance.
    let is_simple = handler_cls
        .py_to_string()
        .contains("SimpleHTTPRequestHandler");

    if is_simple {
        if let PyObjectPayload::Instance(ref d) = handler_inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("do_GET"), simple_handler_do_get(wfile_buf.clone(), false));
            w.insert(CompactString::from("do_HEAD"), simple_handler_do_get(wfile_buf.clone(), true));
        }
    }

    // Look up the class for custom do_METHOD overrides
    if let PyObjectPayload::Class(ref cd) = handler_cls.payload {
        let ns = cd.namespace.read();
        for key in ns.keys() {
            if key.starts_with("do_") {
                if let Some(func) = ns.get(key) {
                    if let PyObjectPayload::Instance(ref d) = handler_inst.payload {
                        d.attrs.write().insert(key.clone(), func.clone());
                    }
                }
            }
        }
    }

    // Dispatch to the appropriate do_METHOD
    let do_method_name = format!("do_{}", method);
    let handler_method = if let PyObjectPayload::Instance(ref d) = handler_inst.payload {
        d.attrs.read().get(do_method_name.as_str()).cloned()
    } else {
        None
    };

    match handler_method {
        Some(func) => {
            // Call the handler method, passing the handler instance as self
            let result = match &func.payload {
                PyObjectPayload::NativeClosure { func: f, .. } => f(&[handler_inst.clone()]),
                PyObjectPayload::NativeFunction { func: f, .. } => f(&[handler_inst.clone()]),
                _ => Ok(PyObject::none()),
            };
            if let Err(_) = result {
                write_error_response(stream, 500, "Internal Server Error");
                return;
            }
        }
        None => {
            write_error_response(stream, 501, &format!("Method {} not implemented", method));
            return;
        }
    }

    // Write the accumulated response to the stream
    let response_data = wfile_buf.lock().unwrap();
    if !response_data.is_empty() {
        let _ = stream.write_all(&response_data);
        let _ = stream.flush();
    }
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

// ── http.cookies module ──

pub fn create_http_cookies_module() -> PyObjectRef {
    // Morsel class — represents a single cookie key/value with attributes
    let morsel_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("Morsel"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("key"), PyObject::str_val(CompactString::from("")));
            w.insert(CompactString::from("value"), PyObject::str_val(CompactString::from("")));
            w.insert(CompactString::from("coded_value"), PyObject::str_val(CompactString::from("")));

            let attrs: Arc<Mutex<IndexMap<CompactString, PyObjectRef>>> = Arc::new(Mutex::new({
                let mut m = IndexMap::new();
                for key in &["expires", "path", "comment", "domain", "max-age", "secure", "httponly", "version", "samesite"] {
                    m.insert(CompactString::from(*key), PyObject::str_val(CompactString::from("")));
                }
                m
            }));

            let a = attrs.clone();
            w.insert(CompactString::from("__setitem__"), PyObject::native_closure(
                "Morsel.__setitem__", move |args: &[PyObjectRef]| {
                    if args.len() >= 2 {
                        let key = CompactString::from(args[0].py_to_string().to_lowercase());
                        a.lock().unwrap().insert(key, args[1].clone());
                    }
                    Ok(PyObject::none())
                }
            ));
            let a2 = attrs.clone();
            w.insert(CompactString::from("__getitem__"), PyObject::native_closure(
                "Morsel.__getitem__", move |args: &[PyObjectRef]| {
                    if let Some(key) = args.first() {
                        let k = CompactString::from(key.py_to_string().to_lowercase());
                        if let Some(val) = a2.lock().unwrap().get(&k) {
                            return Ok(val.clone());
                        }
                    }
                    Ok(PyObject::str_val(CompactString::from("")))
                }
            ));
            let inst2 = inst.clone();
            w.insert(CompactString::from("set"), PyObject::native_closure(
                "Morsel.set", move |args: &[PyObjectRef]| {
                    if args.len() >= 3 {
                        if let PyObjectPayload::Instance(ref d) = inst2.payload {
                            let mut w = d.attrs.write();
                            w.insert(CompactString::from("key"), args[0].clone());
                            w.insert(CompactString::from("value"), args[1].clone());
                            w.insert(CompactString::from("coded_value"), args[2].clone());
                        }
                    }
                    Ok(PyObject::none())
                }
            ));
            let inst3 = inst.clone();
            let a3 = attrs.clone();
            w.insert(CompactString::from("OutputString"), PyObject::native_closure(
                "Morsel.OutputString", move |_args: &[PyObjectRef]| {
                    let key = if let PyObjectPayload::Instance(ref d) = inst3.payload {
                        d.attrs.read().get(&CompactString::from("key")).map(|k| k.py_to_string()).unwrap_or_default()
                    } else { String::new() };
                    let coded = if let PyObjectPayload::Instance(ref d) = inst3.payload {
                        d.attrs.read().get(&CompactString::from("coded_value")).map(|v| v.py_to_string()).unwrap_or_default()
                    } else { String::new() };
                    let mut parts = vec![format!("{}={}", key, coded)];
                    let attrs = a3.lock().unwrap();
                    for (k, v) in attrs.iter() {
                        let vs = v.py_to_string();
                        if !vs.is_empty() {
                            parts.push(format!("{}={}", k, vs));
                        }
                    }
                    Ok(PyObject::str_val(CompactString::from(parts.join("; "))))
                }
            ));
        }
        Ok(inst)
    });

    // SimpleCookie class — dict-like cookie container
    let simple_cookie_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("SimpleCookie"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            let cookies: Arc<Mutex<IndexMap<CompactString, PyObjectRef>>> = Arc::new(Mutex::new(IndexMap::new()));

            let c = cookies.clone();
            w.insert(CompactString::from("__setitem__"), PyObject::native_closure(
                "SimpleCookie.__setitem__", move |args: &[PyObjectRef]| {
                    if args.len() >= 2 {
                        let key = CompactString::from(args[0].py_to_string());
                        // Create a Morsel for the value
                        let morsel_cls = PyObject::class(CompactString::from("Morsel"), vec![], IndexMap::new());
                        let morsel = PyObject::instance(morsel_cls);
                        if let PyObjectPayload::Instance(ref md) = morsel.payload {
                            let mut mw = md.attrs.write();
                            mw.insert(CompactString::from("key"), args[0].clone());
                            mw.insert(CompactString::from("value"), args[1].clone());
                            mw.insert(CompactString::from("coded_value"), args[1].clone());
                        }
                        c.lock().unwrap().insert(key, morsel);
                    }
                    Ok(PyObject::none())
                }
            ));
            let c2 = cookies.clone();
            w.insert(CompactString::from("__getitem__"), PyObject::native_closure(
                "SimpleCookie.__getitem__", move |args: &[PyObjectRef]| {
                    if let Some(key) = args.first() {
                        let k = CompactString::from(key.py_to_string());
                        if let Some(val) = c2.lock().unwrap().get(&k) {
                            return Ok(val.clone());
                        }
                    }
                    Err(PyException::key_error("cookie not found"))
                }
            ));
            let c3 = cookies.clone();
            w.insert(CompactString::from("output"), PyObject::native_closure(
                "SimpleCookie.output", move |_args: &[PyObjectRef]| {
                    let cs = c3.lock().unwrap();
                    let mut lines = Vec::new();
                    for (k, _morsel) in cs.iter() {
                        lines.push(format!("Set-Cookie: {}", k));
                    }
                    Ok(PyObject::str_val(CompactString::from(lines.join("\r\n"))))
                }
            ));
            let c4 = cookies.clone();
            w.insert(CompactString::from("load"), PyObject::native_closure(
                "SimpleCookie.load", move |args: &[PyObjectRef]| {
                    if let Some(raw) = args.first() {
                        let raw_str = raw.py_to_string();
                        // Parse "key=value; key2=value2" format
                        for pair in raw_str.split(';') {
                            let pair = pair.trim();
                            if let Some(eq) = pair.find('=') {
                                let key = CompactString::from(pair[..eq].trim());
                                let value = pair[eq+1..].trim().to_string();
                                let morsel_cls = PyObject::class(CompactString::from("Morsel"), vec![], IndexMap::new());
                                let morsel = PyObject::instance(morsel_cls);
                                if let PyObjectPayload::Instance(ref md) = morsel.payload {
                                    let mut mw = md.attrs.write();
                                    mw.insert(CompactString::from("key"), PyObject::str_val(key.clone()));
                                    mw.insert(CompactString::from("value"), PyObject::str_val(CompactString::from(&value)));
                                    mw.insert(CompactString::from("coded_value"), PyObject::str_val(CompactString::from(&value)));
                                }
                                c4.lock().unwrap().insert(key, morsel);
                            }
                        }
                    }
                    Ok(PyObject::none())
                }
            ));
            let c5 = cookies.clone();
            w.insert(CompactString::from("keys"), PyObject::native_closure(
                "SimpleCookie.keys", move |_args: &[PyObjectRef]| {
                    let cs = c5.lock().unwrap();
                    let keys: Vec<PyObjectRef> = cs.keys().map(|k| PyObject::str_val(k.clone())).collect();
                    Ok(PyObject::list(keys))
                }
            ));
            let c6 = cookies.clone();
            w.insert(CompactString::from("values"), PyObject::native_closure(
                "SimpleCookie.values", move |_args: &[PyObjectRef]| {
                    let cs = c6.lock().unwrap();
                    let vals: Vec<PyObjectRef> = cs.values().cloned().collect();
                    Ok(PyObject::list(vals))
                }
            ));
            let c7 = cookies.clone();
            w.insert(CompactString::from("items"), PyObject::native_closure(
                "SimpleCookie.items", move |_args: &[PyObjectRef]| {
                    let cs = c7.lock().unwrap();
                    let items: Vec<PyObjectRef> = cs.iter().map(|(k, v)| {
                        PyObject::tuple(vec![PyObject::str_val(k.clone()), v.clone()])
                    }).collect();
                    Ok(PyObject::list(items))
                }
            ));
        }
        Ok(inst)
    });

    // CookieError exception
    let cookie_error = PyObject::class(CompactString::from("CookieError"), vec![], IndexMap::new());

    make_module("http.cookies", vec![
        ("Morsel", morsel_fn),
        ("SimpleCookie", simple_cookie_fn.clone()),
        ("BaseCookie", simple_cookie_fn),
        ("CookieError", cookie_error),
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

// ── smtplib module ──

pub fn create_smtplib_module() -> PyObjectRef {
    make_module("smtplib", vec![
        ("SMTP", make_builtin(|args: &[PyObjectRef]| {
            let host = if !args.is_empty() { args[0].py_to_string() } else { "localhost".to_string() };
            let port = if args.len() > 1 { args[1].as_int().unwrap_or(25) } else { 25 };
            let cls = PyObject::class(CompactString::from("SMTP"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                attrs.insert(CompactString::from("host"), PyObject::str_val(CompactString::from(host)));
                attrs.insert(CompactString::from("port"), PyObject::int(port));
                attrs.insert(CompactString::from("ehlo"), make_builtin(|_| Ok(PyObject::tuple(vec![PyObject::int(250), PyObject::str_val(CompactString::from("OK"))])))); 
                attrs.insert(CompactString::from("login"), make_builtin(|_| Ok(PyObject::none())));
                attrs.insert(CompactString::from("sendmail"), make_builtin(|_| Ok(PyObject::dict(IndexMap::new()))));
                attrs.insert(CompactString::from("send_message"), make_builtin(|_| Ok(PyObject::dict(IndexMap::new()))));
                attrs.insert(CompactString::from("quit"), make_builtin(|_| Ok(PyObject::none())));
                attrs.insert(CompactString::from("close"), make_builtin(|_| Ok(PyObject::none())));
                attrs.insert(CompactString::from("starttls"), make_builtin(|_| Ok(PyObject::none())));
            }
            Ok(inst)
        })),
        ("SMTP_SSL", make_builtin(|_args: &[PyObjectRef]| {
            Err(PyException::runtime_error("smtplib.SMTP_SSL: not connected (stub)"))
        })),
        ("SMTPException", PyObject::class(CompactString::from("SMTPException"), vec![], IndexMap::new())),
        ("SMTPAuthenticationError", PyObject::class(CompactString::from("SMTPAuthenticationError"), vec![], IndexMap::new())),
        ("SMTP_PORT", PyObject::int(25)),
        ("SMTP_SSL_PORT", PyObject::int(465)),
    ])
}

// ── ftplib module ──

pub fn create_ftplib_module() -> PyObjectRef {
    make_module("ftplib", vec![
        ("FTP", make_builtin(|args: &[PyObjectRef]| {
            let host = if !args.is_empty() { args[0].py_to_string() } else { String::new() };
            let cls = PyObject::class(CompactString::from("FTP"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                attrs.insert(CompactString::from("host"), PyObject::str_val(CompactString::from(host)));
                attrs.insert(CompactString::from("connect"), make_builtin(|_| Ok(PyObject::str_val(CompactString::from("220 FTP ready (stub)"))))); 
                attrs.insert(CompactString::from("login"), make_builtin(|_| Ok(PyObject::str_val(CompactString::from("230 Login OK")))));
                attrs.insert(CompactString::from("cwd"), make_builtin(|_| Ok(PyObject::str_val(CompactString::from("250 OK")))));
                attrs.insert(CompactString::from("pwd"), make_builtin(|_| Ok(PyObject::str_val(CompactString::from("/")))));
                attrs.insert(CompactString::from("nlst"), make_builtin(|_| Ok(PyObject::list(vec![]))));
                attrs.insert(CompactString::from("dir"), make_builtin(|_| Ok(PyObject::none())));
                attrs.insert(CompactString::from("quit"), make_builtin(|_| Ok(PyObject::str_val(CompactString::from("221 Bye")))));
                attrs.insert(CompactString::from("close"), make_builtin(|_| Ok(PyObject::none())));
            }
            Ok(inst)
        })),
        ("FTP_TLS", make_builtin(|_| {
            Err(PyException::not_implemented_error("ftplib.FTP_TLS"))
        })),
        ("error_reply", PyObject::class(CompactString::from("error_reply"), vec![], IndexMap::new())),
        ("error_perm", PyObject::class(CompactString::from("error_perm"), vec![], IndexMap::new())),
    ])
}

// ── imaplib module ──

pub fn create_imaplib_module() -> PyObjectRef {
    make_module("imaplib", vec![
        ("IMAP4", make_builtin(|_args: &[PyObjectRef]| {
            Err(PyException::runtime_error("imaplib.IMAP4: connection required (stub)"))
        })),
        ("IMAP4_SSL", make_builtin(|_args: &[PyObjectRef]| {
            Err(PyException::runtime_error("imaplib.IMAP4_SSL: connection required (stub)"))
        })),
        ("IMAP4_PORT", PyObject::int(143)),
        ("IMAP4_SSL_PORT", PyObject::int(993)),
    ])
}

// ── poplib module ──

pub fn create_poplib_module() -> PyObjectRef {
    make_module("poplib", vec![
        ("POP3", make_builtin(|_args: &[PyObjectRef]| {
            Err(PyException::runtime_error("poplib.POP3: connection required (stub)"))
        })),
        ("POP3_SSL", make_builtin(|_args: &[PyObjectRef]| {
            Err(PyException::runtime_error("poplib.POP3_SSL: connection required (stub)"))
        })),
        ("POP3_PORT", PyObject::int(110)),
        ("POP3_SSL_PORT", PyObject::int(995)),
    ])
}

// ── cgi module ──

pub fn create_cgi_module() -> PyObjectRef {
    make_module("cgi", vec![
        ("parse_header", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("parse_header requires a string")); }
            let line = args[0].py_to_string();
            let parts: Vec<&str> = line.splitn(2, ';').collect();
            let main_type = parts[0].trim().to_string();
            let mut params = IndexMap::new();
            if parts.len() > 1 {
                for param in parts[1].split(';') {
                    let kv: Vec<&str> = param.splitn(2, '=').collect();
                    if kv.len() == 2 {
                        let k = kv[0].trim().to_string();
                        let v = kv[1].trim().trim_matches('"').to_string();
                        params.insert(
                            HashableKey::Str(CompactString::from(&k)),
                            PyObject::str_val(CompactString::from(v)),
                        );
                    }
                }
            }
            Ok(PyObject::tuple(vec![
                PyObject::str_val(CompactString::from(main_type)),
                PyObject::dict(params),
            ]))
        })),
        ("escape", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("escape requires a string")); }
            let s = args[0].py_to_string();
            let escaped = s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
            Ok(PyObject::str_val(CompactString::from(escaped)))
        })),
        ("FieldStorage", make_builtin(|_| {
            Err(PyException::not_implemented_error("cgi.FieldStorage"))
        })),
        ("parse_qs", make_builtin(|_| {
            Err(PyException::not_implemented_error("cgi.parse_qs (use urllib.parse.parse_qs)"))
        })),
    ])
}

/// xmlrpc module — minimal stub for client/server XML-RPC
pub fn create_xmlrpc_module() -> PyObjectRef {
    let server_proxy = PyObject::native_closure("ServerProxy", move |args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("ServerProxy requires a URL argument"));
        }
        let url = args[0].py_to_string();
        let cls = PyObject::class(CompactString::from("ServerProxy"), vec![], IndexMap::new());
        let mut iattrs = IndexMap::new();
        iattrs.insert(CompactString::from("_url"), PyObject::str_val(CompactString::from(url.as_str())));
        Ok(PyObject::instance_with_attrs(cls, iattrs))
    });
    make_module("xmlrpc", vec![
        ("client", {
            make_module("xmlrpc.client", vec![
                ("ServerProxy", server_proxy),
                ("Fault", make_builtin(|args: &[PyObjectRef]| {
                    let msg = if !args.is_empty() { args[0].py_to_string() } else { "XML-RPC Fault".to_string() };
                    Err(PyException::runtime_error(msg))
                })),
                ("ProtocolError", make_builtin(|args: &[PyObjectRef]| {
                    let msg = if !args.is_empty() { args[0].py_to_string() } else { "Protocol Error".to_string() };
                    Err(PyException::runtime_error(msg))
                })),
            ])
        }),
        ("server", make_module("xmlrpc.server", vec![])),
    ])
}
