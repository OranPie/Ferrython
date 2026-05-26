use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{parse_url_string, ParsedUrl};

pub fn create_urllib_module() -> PyObjectRef {
    make_module(
        "urllib.request",
        vec![
            ("urlopen", make_builtin(urllib_urlopen)),
            ("Request", make_builtin(urllib_request_constructor)),
            (
                "getproxies",
                make_builtin(|_args: &[PyObjectRef]| {
                    let mut proxies = IndexMap::new();
                    for (env_var, scheme) in &[
                        ("http_proxy", "http"),
                        ("https_proxy", "https"),
                        ("HTTP_PROXY", "http"),
                        ("HTTPS_PROXY", "https"),
                        ("ftp_proxy", "ftp"),
                        ("no_proxy", "no"),
                    ] {
                        if let Ok(val) = std::env::var(env_var) {
                            proxies.insert(
                                HashableKey::str_key(CompactString::from(*scheme)),
                                PyObject::str_val(CompactString::from(val)),
                            );
                        }
                    }
                    Ok(PyObject::dict(proxies))
                }),
            ),
            (
                "getproxies_environment",
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::dict(IndexMap::new()))),
            ),
            (
                "proxy_bypass",
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::bool_val(false))),
            ),
            (
                "proxy_bypass_environment",
                make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::bool_val(false))),
            ),
            (
                "pathname2url",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error("pathname2url requires 1 argument"));
                    }
                    Ok(args[0].clone())
                }),
            ),
            (
                "url2pathname",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error("url2pathname requires 1 argument"));
                    }
                    Ok(args[0].clone())
                }),
            ),
            (
                "parse_http_list",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.len() != 1 {
                        return Err(PyException::type_error(
                            "parse_http_list() takes 1 argument",
                        ));
                    }
                    let s = args[0].py_to_string();
                    let mut result = Vec::new();
                    let mut current = String::new();
                    let mut in_quote = false;
                    let mut escape = false;
                    for ch in s.chars() {
                        if escape {
                            current.push(ch);
                            escape = false;
                        } else if ch == '\\' && in_quote {
                            escape = true;
                            current.push(ch);
                        } else if ch == '"' {
                            in_quote = !in_quote;
                            current.push(ch);
                        } else if ch == ',' && !in_quote {
                            let trimmed = current.trim().to_string();
                            if !trimmed.is_empty() {
                                result.push(PyObject::str_val(CompactString::from(trimmed)));
                            }
                            current.clear();
                        } else {
                            current.push(ch);
                        }
                    }
                    let trimmed = current.trim().to_string();
                    if !trimmed.is_empty() {
                        result.push(PyObject::str_val(CompactString::from(trimmed)));
                    }
                    Ok(PyObject::list(result))
                }),
            ),
            (
                "parse_keqv_list",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.len() != 1 {
                        return Err(PyException::type_error(
                            "parse_keqv_list() takes 1 argument",
                        ));
                    }
                    let items = if let PyObjectPayload::List(ref ls) = args[0].payload {
                        ls.read().clone()
                    } else {
                        return Err(PyException::type_error("parse_keqv_list expects a list"));
                    };
                    let mut dict = IndexMap::new();
                    for item in &items {
                        let s = item.py_to_string();
                        if let Some(eq_pos) = s.find('=') {
                            let key = s[..eq_pos].trim().to_string();
                            let mut val = s[eq_pos + 1..].trim().to_string();
                            if val.starts_with('"') && val.ends_with('"') && val.len() >= 2 {
                                val = val[1..val.len() - 1].to_string();
                            }
                            dict.insert(
                                HashableKey::str_key(CompactString::from(key)),
                                PyObject::str_val(CompactString::from(val)),
                            );
                        } else {
                            dict.insert(
                                HashableKey::str_key(CompactString::from(s.trim())),
                                PyObject::none(),
                            );
                        }
                    }
                    Ok(PyObject::dict(dict))
                }),
            ),
        ],
    )
}

fn build_http_request(
    parsed: &ParsedUrl,
    method: &str,
    headers: &IndexMap<String, String>,
    body: Option<&[u8]>,
) -> Vec<u8> {
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

fn do_http_request(
    url: &str,
    method: &str,
    headers: &IndexMap<String, String>,
    data: Option<&[u8]>,
) -> PyResult<(u16, IndexMap<String, String>, Vec<u8>)> {
    let parsed = parse_url_string(url);
    if parsed.scheme == "https" {
        return Err(PyException::os_error(
            "HTTPS is not supported (no TLS available)",
        ));
    }

    let addr = format!("{}:{}", parsed.host, parsed.port);
    let mut stream =
        TcpStream::connect(&addr).map_err(|e| PyException::os_error(format!("urlopen: {}", e)))?;
    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();

    let request = build_http_request(&parsed, method, headers, data);
    stream
        .write_all(&request)
        .map_err(|e| PyException::os_error(format!("urlopen write: {}", e)))?;

    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| PyException::os_error(format!("urlopen read: {}", e)))?;

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

pub(super) fn make_http_response_class() -> PyObjectRef {
    let mut ns = IndexMap::new();

    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("HTTPResponse.__init__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            let self_obj = &args[0];
            if let PyObjectPayload::Instance(ref d) = self_obj.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("status"), PyObject::int(200));
                w.insert(CompactString::from("code"), PyObject::int(200));
                w.insert(
                    CompactString::from("reason"),
                    PyObject::str_val(CompactString::from("OK")),
                );
                w.insert(
                    CompactString::from("headers"),
                    PyObject::dict(IndexMap::new()),
                );
                w.insert(CompactString::from("_body"), PyObject::bytes(vec![]));
                w.insert(CompactString::from("_read_pos"), PyObject::int(0));
                w.insert(
                    CompactString::from("url"),
                    PyObject::str_val(CompactString::from("")),
                );
                w.insert(
                    CompactString::from("__urllib_response__"),
                    PyObject::bool_val(true),
                );
            }
            Ok(PyObject::none())
        }),
    );

    ns.insert(
        CompactString::from("read"),
        PyObject::native_closure("HTTPResponse.read", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::bytes(vec![]));
            }
            let self_obj = &args[0];
            let n = if args.len() > 1 {
                args[1].as_int().unwrap_or(-1)
            } else {
                -1
            };
            let body_bytes = self_obj
                .get_attr("_body")
                .map(|b| match &b.payload {
                    PyObjectPayload::Bytes(v) => (**v).clone(),
                    _ => vec![],
                })
                .unwrap_or_default();
            let pos = self_obj
                .get_attr("_read_pos")
                .and_then(|p| p.as_int())
                .unwrap_or(0) as usize;
            let pos = std::cmp::min(pos, body_bytes.len());
            let remaining = &body_bytes[pos..];
            let chunk = if n < 0 {
                remaining.to_vec()
            } else {
                let end = std::cmp::min(n as usize, remaining.len());
                remaining[..end].to_vec()
            };
            let new_pos = pos + chunk.len();
            if let PyObjectPayload::Instance(ref d) = self_obj.payload {
                let mut w = d.attrs.write();
                w.insert(
                    CompactString::from("_read_pos"),
                    PyObject::int(new_pos as i64),
                );
            }
            Ok(PyObject::bytes(chunk))
        }),
    );

    ns.insert(
        CompactString::from("readline"),
        PyObject::native_closure("HTTPResponse.readline", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::bytes(vec![]));
            }
            let self_obj = &args[0];
            let body_bytes = self_obj
                .get_attr("_body")
                .map(|b| match &b.payload {
                    PyObjectPayload::Bytes(v) => (**v).clone(),
                    _ => vec![],
                })
                .unwrap_or_default();
            let pos = self_obj
                .get_attr("_read_pos")
                .and_then(|p| p.as_int())
                .unwrap_or(0) as usize;
            let pos = std::cmp::min(pos, body_bytes.len());
            let remaining = &body_bytes[pos..];
            let end = remaining
                .iter()
                .position(|&c| c == b'\n')
                .map(|i| i + 1)
                .unwrap_or(remaining.len());
            let line = remaining[..end].to_vec();
            let new_pos = pos + line.len();
            if let PyObjectPayload::Instance(ref d) = self_obj.payload {
                let mut w = d.attrs.write();
                w.insert(
                    CompactString::from("_read_pos"),
                    PyObject::int(new_pos as i64),
                );
            }
            Ok(PyObject::bytes(line))
        }),
    );

    ns.insert(
        CompactString::from("getheader"),
        PyObject::native_closure("HTTPResponse.getheader", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::none());
            }
            let self_obj = &args[0];
            let name = args[1].py_to_string();
            let default = args.get(2).cloned().unwrap_or_else(PyObject::none);
            let headers = self_obj
                .get_attr("headers")
                .unwrap_or_else(|| PyObject::dict(IndexMap::new()));
            if let PyObjectPayload::Dict(d) = &headers.payload {
                let map = d.read();
                let name_lower = name.to_lowercase();
                for (k, v) in map.iter() {
                    let ks = match k {
                        HashableKey::Str(s) => s.to_string(),
                        _ => continue,
                    };
                    if ks.to_lowercase() == name_lower {
                        return Ok(v.clone());
                    }
                }
            }
            Ok(default)
        }),
    );

    ns.insert(
        CompactString::from("getheaders"),
        PyObject::native_closure("HTTPResponse.getheaders", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::list(vec![]));
            }
            let self_obj = &args[0];
            let headers = self_obj
                .get_attr("headers")
                .unwrap_or_else(|| PyObject::dict(IndexMap::new()));
            let mut result = Vec::new();
            if let PyObjectPayload::Dict(d) = &headers.payload {
                let map = d.read();
                for (k, v) in map.iter() {
                    let ks = match k {
                        HashableKey::Str(s) => PyObject::str_val(s.to_compact_string()),
                        _ => continue,
                    };
                    result.push(PyObject::tuple(vec![ks, v.clone()]));
                }
            }
            Ok(PyObject::list(result))
        }),
    );

    ns.insert(
        CompactString::from("getcode"),
        PyObject::native_closure("HTTPResponse.getcode", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::int(0));
            }
            Ok(args[0]
                .get_attr("status")
                .unwrap_or_else(|| PyObject::int(0)))
        }),
    );

    ns.insert(
        CompactString::from("geturl"),
        PyObject::native_closure("HTTPResponse.geturl", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::str_val(CompactString::from("")));
            }
            Ok(args[0]
                .get_attr("url")
                .unwrap_or_else(|| PyObject::str_val(CompactString::from(""))))
        }),
    );

    ns.insert(
        CompactString::from("close"),
        PyObject::native_closure("HTTPResponse.close", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );

    ns.insert(
        CompactString::from("__enter__"),
        PyObject::native_closure("HTTPResponse.__enter__", |args: &[PyObjectRef]| {
            if !args.is_empty() {
                Ok(args[0].clone())
            } else {
                Ok(PyObject::none())
            }
        }),
    );

    ns.insert(
        CompactString::from("__exit__"),
        PyObject::native_closure("HTTPResponse.__exit__", |_args: &[PyObjectRef]| {
            Ok(PyObject::bool_val(false))
        }),
    );

    PyObject::class(CompactString::from("HTTPResponse"), vec![], ns)
}

pub(super) fn build_response_object(
    url: &str,
    status: u16,
    headers: IndexMap<String, String>,
    body: Vec<u8>,
) -> PyObjectRef {
    let cls = make_http_response_class();

    let mut inst_attrs = IndexMap::new();
    inst_attrs.insert(
        CompactString::from("__urllib_response__"),
        PyObject::bool_val(true),
    );
    inst_attrs.insert(
        CompactString::from("url"),
        PyObject::str_val(CompactString::from(url)),
    );
    inst_attrs.insert(CompactString::from("status"), PyObject::int(status as i64));
    inst_attrs.insert(CompactString::from("code"), PyObject::int(status as i64));
    inst_attrs.insert(
        CompactString::from("reason"),
        PyObject::str_val(CompactString::from(http_reason(status))),
    );

    let mut hdr_map = IndexMap::new();
    for (k, v) in &headers {
        hdr_map.insert(
            HashableKey::str_key(CompactString::from(k.as_str())),
            PyObject::str_val(CompactString::from(v.as_str())),
        );
    }
    inst_attrs.insert(CompactString::from("headers"), PyObject::dict(hdr_map));
    inst_attrs.insert(CompactString::from("_body"), PyObject::bytes(body));
    inst_attrs.insert(CompactString::from("_read_pos"), PyObject::int(0));

    PyObject::instance_with_attrs(cls, inst_attrs)
}

fn urllib_urlopen(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("urlopen() requires a url argument"));
    }

    let (url, method, headers, data) = if let Some(u) = args[0].get_attr("full_url") {
        let url = u.py_to_string();
        let method = args[0]
            .get_attr("method")
            .map(|m| m.py_to_string())
            .unwrap_or_else(|| "GET".to_string());
        let data_bytes = args[0].get_attr("data").and_then(|d| match &d.payload {
            PyObjectPayload::Bytes(b) => Some((**b).clone()),
            PyObjectPayload::None => None,
            _ => Some(d.py_to_string().into_bytes()),
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
        let url = args[0].py_to_string();
        let data_bytes = if args.len() > 1 && !matches!(&args[1].payload, PyObjectPayload::None) {
            match &args[1].payload {
                PyObjectPayload::Bytes(b) => Some((**b).clone()),
                _ => Some(args[1].py_to_string().into_bytes()),
            }
        } else {
            None
        };
        let method = if data_bytes.is_some() { "POST" } else { "GET" };
        (url, method.to_string(), IndexMap::new(), data_bytes)
    };

    let (status, resp_headers, body) = do_http_request(&url, &method, &headers, data.as_deref())?;
    Ok(build_response_object(&url, status, resp_headers, body))
}

fn urllib_request_constructor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("Request() requires a url argument"));
    }
    let url = args[0].py_to_string();

    let data = if args.len() > 1 && !matches!(&args[1].payload, PyObjectPayload::None) {
        args[1].clone()
    } else {
        PyObject::none()
    };

    let mut extra_headers = IndexMap::new();
    let mut method = if matches!(&data.payload, PyObjectPayload::None) {
        "GET".to_string()
    } else {
        "POST".to_string()
    };

    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(kw) = &last.payload {
            let r = kw.read();
            if let Some(m) = r.get(&HashableKey::str_key(CompactString::from("method"))) {
                method = m.py_to_string();
            }
            if let Some(h) = r.get(&HashableKey::str_key(CompactString::from("headers"))) {
                if let PyObjectPayload::Dict(hm) = &h.payload {
                    for (k, v) in hm.read().iter() {
                        if let HashableKey::Str(key) = k {
                            extra_headers
                                .insert(HashableKey::str_key(key.to_compact_string()), v.clone());
                        }
                    }
                }
            }
        }
    }

    let parsed = parse_url_string(&url);
    let headers_dict = PyObject::dict(extra_headers);

    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("full_url"),
        PyObject::str_val(CompactString::from(url.as_str())),
    );
    attrs.insert(
        CompactString::from("host"),
        PyObject::str_val(CompactString::from(parsed.host.as_str())),
    );
    attrs.insert(
        CompactString::from("type"),
        PyObject::str_val(CompactString::from(parsed.scheme.as_str())),
    );
    attrs.insert(
        CompactString::from("method"),
        PyObject::str_val(CompactString::from(method)),
    );
    attrs.insert(CompactString::from("data"), data);
    attrs.insert(CompactString::from("headers"), headers_dict);

    let req_attrs = Arc::new(Mutex::new(attrs.clone()));
    let ra = req_attrs.clone();
    attrs.insert(
        CompactString::from("add_header"),
        PyObject::native_closure("add_header", move |a: &[PyObjectRef]| {
            if a.len() < 2 {
                return Err(PyException::type_error("add_header(key, value)"));
            }
            let key = a[0].py_to_string();
            let val = a[1].py_to_string();

            let mut locked = ra.lock().unwrap();
            if let Some(hdr) = locked.get_mut("headers") {
                if let PyObjectPayload::Dict(map) = &hdr.payload {
                    map.write().insert(
                        HashableKey::str_key(CompactString::from(key)),
                        PyObject::str_val(CompactString::from(val)),
                    );
                }
            }
            Ok(PyObject::none())
        }),
    );

    Ok(PyObject::module_with_attrs(
        CompactString::from("urllib.request.Request"),
        attrs,
    ))
}
