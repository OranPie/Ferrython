//! HTTP, urllib, and SSL stdlib modules.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, new_fx_hashkey_map, FxHashKeyMap, NativeClosureData, PyObject,
    PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod cgi;
mod cookiejar;
mod cookies;
mod ftplib;
mod imaplib;
mod poplib;
mod smtplib;
mod socketserver;
mod ssl;
mod xmlrpc;

pub use cgi::create_cgi_module;
pub use cookiejar::create_http_cookiejar_module;
pub use cookies::create_http_cookies_module;
pub use ftplib::create_ftplib_module;
pub use imaplib::create_imaplib_module;
pub use poplib::create_poplib_module;
pub use smtplib::create_smtplib_module;
pub use socketserver::create_socketserver_module;
pub use ssl::create_ssl_module;
pub use xmlrpc::create_xmlrpc_module;

struct ParsedUrl {
    scheme: String,
    host: String,
    port: u16,
    path: String,
    query: String,
    fragment: String,
    netloc: String,
    username: String,
    password: String,
}

fn parse_url_string(url: &str) -> ParsedUrl {
    let (scheme, rest) = if let Some(idx) = url.find("://") {
        (url[..idx].to_string(), &url[idx + 3..])
    } else {
        (String::new(), url)
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

    // Extract userinfo (username:password@)
    let (userinfo, host_part) = if let Some(idx) = host_port.rfind('@') {
        (&host_port[..idx], &host_port[idx + 1..])
    } else {
        ("", host_port)
    };

    let (username, password) = if !userinfo.is_empty() {
        if let Some(idx) = userinfo.find(':') {
            (userinfo[..idx].to_string(), userinfo[idx + 1..].to_string())
        } else {
            (userinfo.to_string(), String::new())
        }
    } else {
        (String::new(), String::new())
    };

    let (host, port) = if let Some(idx) = host_part.rfind(':') {
        let port_str = &host_part[idx + 1..];
        if let Ok(p) = port_str.parse::<u16>() {
            (host_part[..idx].to_string(), p)
        } else {
            (
                host_part.to_string(),
                if scheme == "https" { 443 } else { 80 },
            )
        }
    } else {
        (
            host_part.to_string(),
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
        username,
        password,
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
            if let Ok(val) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00"),
                16,
            ) {
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
            (
                "getproxies",
                make_builtin(|_args: &[PyObjectRef]| {
                    // Return proxy settings from environment variables
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
                    // Parse HTTP list header per RFC 2616 Section 2.1
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
                    // Parse key=value HTTP list
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

/// Build the HTTPResponse *class* (used by http.client and urllib).
fn make_http_response_class() -> PyObjectRef {
    let mut ns = IndexMap::new();

    // __init__(self, status=200, reason="", headers=None, body=b"", url="")
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

    // read(self, n=-1) → bytes
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

    // readline(self) → bytes
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

    // getheader(self, name, default=None) → str or default
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

    // getheaders() → list of (name, value) tuples
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

    // getcode(self) → int
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

    // geturl(self) → str
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

    // close(self) — no-op
    ns.insert(
        CompactString::from("close"),
        PyObject::native_closure("HTTPResponse.close", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );

    // __enter__(self) → self
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

    // __exit__(self, *args) → False
    ns.insert(
        CompactString::from("__exit__"),
        PyObject::native_closure("HTTPResponse.__exit__", |_args: &[PyObjectRef]| {
            Ok(PyObject::bool_val(false))
        }),
    );

    PyObject::class(CompactString::from("HTTPResponse"), vec![], ns)
}

/// Build an HTTPResponse *instance* with pre-populated data.
fn build_response_object(
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

    // Build headers dict
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

    // Extract URL, method, headers, and data from args
    let (url, method, headers, data) = if let Some(u) = args[0].get_attr("full_url") {
        // Request object
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
        // Plain string URL
        let url = args[0].py_to_string();
        // Check for data= kwarg (second arg or trailing dict)
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

    // add_header(key, value) — add a header to the request
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
            // Named result types
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
            // Internal constants used by some packages
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
    // Helper: convert a value to string, decoding bytes as UTF-8 (like CPython)
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
    // Accept both str and bytes (CPython does)
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

    // Build a ParseResult-like object with both named attributes and tuple behavior
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

/// urlunparse((scheme, netloc, path, params, query, fragment)) -> URL string
fn urllib_parse_urlunparse(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("urlunparse() requires 1 argument"));
    }
    let components = match &args[0].payload {
        PyObjectPayload::Tuple(items) => (**items).clone(),
        PyObjectPayload::List(items) => items.read().clone(),
        // Also handle ParseResult-like objects with scheme/netloc/path/etc
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
    // Treat None as empty string (requests passes None for missing components)
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

/// urlsplit(url) -> SplitResult (like urlparse but without params)
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
    let has_explicit_port2 = {
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
        if has_explicit_port2 {
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

    // __iter__ for tuple-like unpacking
    let iter_components = components.clone();
    attrs.insert(
        CompactString::from("__iter__"),
        PyObject::native_closure("__iter__", move |_| {
            Ok(PyObject::tuple(iter_components.clone()))
        }),
    );

    // __repr__
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

/// urlunsplit((scheme, netloc, path, query, fragment)) -> URL string
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

/// urldefrag(url) -> DefragResult(url_without_fragment, fragment)
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

    // If url is absolute, return it directly
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

/// Build the HTTPConnection *class* (proper class, supports isinstance/subclassing).
fn make_http_connection_class(default_port: u16, class_name: &str, is_https: bool) -> PyObjectRef {
    let mut ns = IndexMap::new();
    let https_flag = is_https;
    let def_port = default_port;

    // __init__(self, host, port=None, timeout=30)
    ns.insert(CompactString::from("__init__"), {
        let class_name_str = class_name.to_string();
        PyObject::native_closure(
            &format!("{}.__init__", class_name_str),
            move |args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Err(PyException::type_error(&format!(
                        "{}() requires a host argument",
                        class_name_str
                    )));
                }
                let self_obj = &args[0];
                let host = args[1].py_to_string();
                let port: u16 =
                    if args.len() > 2 && !matches!(&args[2].payload, PyObjectPayload::None) {
                        args[2].as_int().unwrap_or(def_port as i64) as u16
                    } else if let Some(idx) = host.rfind(':') {
                        host[idx + 1..].parse().unwrap_or(def_port)
                    } else {
                        def_port
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
                let timeout_secs: i64 =
                    if args.len() > 3 && !matches!(&args[3].payload, PyObjectPayload::None) {
                        args[3].as_int().unwrap_or(30)
                    } else {
                        30
                    };

                if let PyObjectPayload::Instance(ref d) = self_obj.payload {
                    let mut w = d.attrs.write();
                    w.insert(
                        CompactString::from("host"),
                        PyObject::str_val(CompactString::from(host_only.as_str())),
                    );
                    w.insert(CompactString::from("port"), PyObject::int(port as i64));
                    w.insert(CompactString::from("timeout"), PyObject::int(timeout_secs));
                    w.insert(CompactString::from("debuglevel"), PyObject::int(0));
                    w.insert(
                        CompactString::from("_https"),
                        PyObject::bool_val(https_flag),
                    );
                    w.insert(CompactString::from("_response_data"), PyObject::none());
                }
                Ok(PyObject::none())
            },
        )
    });

    // request(self, method, url, body=None, headers=None)
    ns.insert(
        CompactString::from("request"),
        PyObject::native_closure("HTTPConnection.request", |args: &[PyObjectRef]| {
            if args.len() < 3 {
                return Err(PyException::type_error(
                    "request() requires method and url arguments",
                ));
            }
            let self_obj = &args[0];
            let method = args[1].py_to_string();
            let url = args[2].py_to_string();
            let body = if args.len() > 3 && !matches!(&args[3].payload, PyObjectPayload::None) {
                Some(args[3].py_to_string())
            } else {
                None
            };

            let mut extra_headers = IndexMap::new();
            if args.len() > 4 {
                if let PyObjectPayload::Dict(d) = &args[4].payload {
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

            let host = self_obj
                .get_attr("host")
                .map(|h| h.py_to_string())
                .unwrap_or_default();
            let port = self_obj
                .get_attr("port")
                .and_then(|p| p.as_int())
                .unwrap_or(80) as u16;
            let timeout_secs = self_obj
                .get_attr("timeout")
                .and_then(|t| t.as_int())
                .unwrap_or(30) as u64;

            let addr = format!("{}:{}", host, port);
            let timeout = Duration::from_secs(timeout_secs);
            let socket_addr: std::net::SocketAddr = addr.parse().or_else(|_| {
                use std::net::ToSocketAddrs;
                addr.to_socket_addrs()
                    .map_err(|e| PyException::os_error(format!("HTTPConnection DNS: {}", e)))
                    .and_then(|mut addrs| {
                        addrs.next().ok_or_else(|| {
                            PyException::os_error(format!(
                                "HTTPConnection: could not resolve {}",
                                addr
                            ))
                        })
                    })
            })?;
            let mut stream = TcpStream::connect_timeout(&socket_addr, timeout)
                .map_err(|e| PyException::os_error(format!("HTTPConnection: {}", e)))?;
            stream.set_read_timeout(Some(timeout)).ok();
            stream.set_write_timeout(Some(timeout)).ok();

            let mut req = format!(
                "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
                method, url, host
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

            // Store response data on self
            if let PyObjectPayload::Instance(ref d) = self_obj.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("_response_data"), PyObject::bytes(raw));
            }
            Ok(PyObject::none())
        }),
    );

    // getresponse(self) → HTTPResponse instance
    ns.insert(
        CompactString::from("getresponse"),
        PyObject::native_closure("HTTPConnection.getresponse", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::runtime_error("no response available"));
            }
            let self_obj = &args[0];
            let raw_obj = self_obj
                .get_attr("_response_data")
                .ok_or_else(|| PyException::runtime_error("no response available"))?;
            let raw = match &raw_obj.payload {
                PyObjectPayload::Bytes(b) => (**b).clone(),
                PyObjectPayload::None => {
                    return Err(PyException::runtime_error("no response available"))
                }
                _ => vec![],
            };

            // Clear stored data
            if let PyObjectPayload::Instance(ref d) = self_obj.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("_response_data"), PyObject::none());
            }

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

            let host = self_obj
                .get_attr("host")
                .map(|h| h.py_to_string())
                .unwrap_or_default();
            let port = self_obj
                .get_attr("port")
                .and_then(|p| p.as_int())
                .unwrap_or(80);
            let url_str = format!("http://{}:{}/", host, port);
            Ok(build_response_object(&url_str, status_code, headers, body))
        }),
    );

    // connect(self) — no-op (connection is done lazily in request())
    ns.insert(
        CompactString::from("connect"),
        PyObject::native_closure("HTTPConnection.connect", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );

    // close(self)
    ns.insert(
        CompactString::from("close"),
        PyObject::native_closure("HTTPConnection.close", |args: &[PyObjectRef]| {
            if !args.is_empty() {
                if let PyObjectPayload::Instance(ref d) = args[0].payload {
                    let mut w = d.attrs.write();
                    w.insert(CompactString::from("_response_data"), PyObject::none());
                }
            }
            Ok(PyObject::none())
        }),
    );

    // set_debuglevel(self, level)
    ns.insert(
        CompactString::from("set_debuglevel"),
        PyObject::native_closure("HTTPConnection.set_debuglevel", |args: &[PyObjectRef]| {
            if args.len() >= 2 {
                if let PyObjectPayload::Instance(ref d) = args[0].payload {
                    let mut w = d.attrs.write();
                    w.insert(CompactString::from("debuglevel"), args[1].clone());
                }
            }
            Ok(PyObject::none())
        }),
    );

    // set_tunnel(self, host, port=None, headers=None)
    ns.insert(
        CompactString::from("set_tunnel"),
        PyObject::native_closure("HTTPConnection.set_tunnel", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );

    // putheader(self, header, *values)
    ns.insert(
        CompactString::from("putheader"),
        PyObject::native_closure("HTTPConnection.putheader", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );

    // putrequest(self, method, url, skip_host=False, skip_accept_encoding=False)
    ns.insert(
        CompactString::from("putrequest"),
        PyObject::native_closure("HTTPConnection.putrequest", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );

    // endheaders(self, message_body=None)
    ns.insert(
        CompactString::from("endheaders"),
        PyObject::native_closure("HTTPConnection.endheaders", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );

    // __enter__(self) → self
    ns.insert(
        CompactString::from("__enter__"),
        PyObject::native_closure("HTTPConnection.__enter__", |args: &[PyObjectRef]| {
            if !args.is_empty() {
                Ok(args[0].clone())
            } else {
                Ok(PyObject::none())
            }
        }),
    );

    // __exit__(self, *args) → False
    ns.insert(
        CompactString::from("__exit__"),
        PyObject::native_closure("HTTPConnection.__exit__", |args: &[PyObjectRef]| {
            // call close
            if !args.is_empty() {
                if let PyObjectPayload::Instance(ref d) = args[0].payload {
                    let mut w = d.attrs.write();
                    w.insert(CompactString::from("_response_data"), PyObject::none());
                }
            }
            Ok(PyObject::bool_val(false))
        }),
    );

    PyObject::class(CompactString::from(class_name), vec![], ns)
}

pub fn create_http_client_module() -> PyObjectRef {
    let http_connection_cls = make_http_connection_class(80, "HTTPConnection", false);
    let https_connection_cls = make_http_connection_class(443, "HTTPSConnection", true);
    let http_response_cls = make_http_response_class();

    let mut client_attrs = IndexMap::new();
    client_attrs.insert(CompactString::from("HTTPConnection"), http_connection_cls);
    client_attrs.insert(CompactString::from("HTTPSConnection"), https_connection_cls);
    // Status code constants
    client_attrs.insert(CompactString::from("OK"), PyObject::int(200));
    client_attrs.insert(CompactString::from("NOT_FOUND"), PyObject::int(404));
    client_attrs.insert(
        CompactString::from("INTERNAL_SERVER_ERROR"),
        PyObject::int(500),
    );
    // HTTPResponse class
    client_attrs.insert(CompactString::from("HTTPResponse"), http_response_cls);
    // Exception classes
    client_attrs.insert(
        CompactString::from("HTTPException"),
        PyObject::builtin_type(CompactString::from("HTTPException")),
    );
    client_attrs.insert(
        CompactString::from("RemoteDisconnected"),
        PyObject::builtin_type(CompactString::from("RemoteDisconnected")),
    );
    client_attrs.insert(
        CompactString::from("IncompleteRead"),
        PyObject::builtin_type(CompactString::from("IncompleteRead")),
    );
    client_attrs.insert(
        CompactString::from("ResponseNotReady"),
        PyObject::builtin_type(CompactString::from("ResponseNotReady")),
    );
    client_attrs.insert(
        CompactString::from("BadStatusLine"),
        PyObject::builtin_type(CompactString::from("BadStatusLine")),
    );
    client_attrs.insert(
        CompactString::from("CannotSendRequest"),
        PyObject::builtin_type(CompactString::from("CannotSendRequest")),
    );
    // HTTPMessage class
    client_attrs.insert(
        CompactString::from("HTTPMessage"),
        PyObject::class(CompactString::from("HTTPMessage"), vec![], IndexMap::new()),
    );
    PyObject::module_with_attrs(CompactString::from("http.client"), client_attrs)
}

/// Create an HTTPStatus member with value, name, phrase, and description.
fn make_http_status_member(code: i64, name: &str, phrase: &str, description: &str) -> PyObjectRef {
    let status_cls = PyObject::class(CompactString::from("HTTPStatus"), vec![], IndexMap::new());
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("value"), PyObject::int(code));
    attrs.insert(CompactString::from("_value_"), PyObject::int(code));
    attrs.insert(
        CompactString::from("name"),
        PyObject::str_val(CompactString::from(name)),
    );
    attrs.insert(
        CompactString::from("_name_"),
        PyObject::str_val(CompactString::from(name)),
    );
    attrs.insert(
        CompactString::from("phrase"),
        PyObject::str_val(CompactString::from(phrase)),
    );
    attrs.insert(
        CompactString::from("description"),
        PyObject::str_val(CompactString::from(description)),
    );

    // __eq__: compare by value (code)
    let eq_code = code;
    attrs.insert(
        CompactString::from("__eq__"),
        PyObject::wrap(PyObjectPayload::NativeClosure(Box::new(
            NativeClosureData {
                name: CompactString::from("__eq__"),
                pickle_args: None,
                func: std::rc::Rc::new(move |args: &[PyObjectRef]| {
                    let other = if args.len() > 1 { &args[1] } else { &args[0] };
                    if let Some(v) = other.as_int() {
                        Ok(PyObject::bool_val(v == eq_code))
                    } else {
                        Ok(PyObject::bool_val(false))
                    }
                }),
            },
        ))),
    );
    // __int__: return numeric code
    let int_code = code;
    attrs.insert(
        CompactString::from("__int__"),
        PyObject::wrap(PyObjectPayload::NativeClosure(Box::new(
            NativeClosureData {
                name: CompactString::from("__int__"),
                func: std::rc::Rc::new(move |_args: &[PyObjectRef]| Ok(PyObject::int(int_code))),
                pickle_args: None,
            },
        ))),
    );
    // __hash__
    let hash_code = code;
    attrs.insert(
        CompactString::from("__hash__"),
        PyObject::wrap(PyObjectPayload::NativeClosure(Box::new(
            NativeClosureData {
                name: CompactString::from("__hash__"),
                func: std::rc::Rc::new(move |_args: &[PyObjectRef]| Ok(PyObject::int(hash_code))),
                pickle_args: None,
            },
        ))),
    );
    // __repr__ / __str__
    let repr_s = CompactString::from(format!("<HTTPStatus.{}: {}>", name, code));
    let str_s = CompactString::from(format!("HTTPStatus.{}", name));
    attrs.insert(
        CompactString::from("__repr__"),
        PyObject::wrap(PyObjectPayload::NativeClosure(Box::new(
            NativeClosureData {
                name: CompactString::from("__repr__"),
                pickle_args: None,
                func: std::rc::Rc::new(move |_args: &[PyObjectRef]| {
                    Ok(PyObject::str_val(repr_s.clone()))
                }),
            },
        ))),
    );
    attrs.insert(
        CompactString::from("__str__"),
        PyObject::wrap(PyObjectPayload::NativeClosure(Box::new(
            NativeClosureData {
                name: CompactString::from("__str__"),
                pickle_args: None,
                func: std::rc::Rc::new(move |_args: &[PyObjectRef]| {
                    Ok(PyObject::str_val(str_s.clone()))
                }),
            },
        ))),
    );

    PyObject::instance_with_attrs(status_cls, attrs)
}

pub fn create_http_module() -> PyObjectRef {
    // Build HTTPStatus as a proper IntEnum-like type with .value, .name, .phrase
    let statuses: Vec<(i64, &str, &str, &str)> = vec![
        (100, "CONTINUE", "Continue", ""),
        (101, "SWITCHING_PROTOCOLS", "Switching Protocols", ""),
        (200, "OK", "OK", ""),
        (201, "CREATED", "Created", ""),
        (202, "ACCEPTED", "Accepted", ""),
        (204, "NO_CONTENT", "No Content", ""),
        (206, "PARTIAL_CONTENT", "Partial Content", ""),
        (301, "MOVED_PERMANENTLY", "Moved Permanently", ""),
        (302, "FOUND", "Found", ""),
        (304, "NOT_MODIFIED", "Not Modified", ""),
        (307, "TEMPORARY_REDIRECT", "Temporary Redirect", ""),
        (308, "PERMANENT_REDIRECT", "Permanent Redirect", ""),
        (400, "BAD_REQUEST", "Bad Request", ""),
        (401, "UNAUTHORIZED", "Unauthorized", ""),
        (403, "FORBIDDEN", "Forbidden", ""),
        (404, "NOT_FOUND", "Not Found", ""),
        (405, "METHOD_NOT_ALLOWED", "Method Not Allowed", ""),
        (408, "REQUEST_TIMEOUT", "Request Timeout", ""),
        (409, "CONFLICT", "Conflict", ""),
        (410, "GONE", "Gone", ""),
        (413, "CONTENT_TOO_LARGE", "Content Too Large", ""),
        (415, "UNSUPPORTED_MEDIA_TYPE", "Unsupported Media Type", ""),
        (422, "UNPROCESSABLE_ENTITY", "Unprocessable Entity", ""),
        (429, "TOO_MANY_REQUESTS", "Too Many Requests", ""),
        (500, "INTERNAL_SERVER_ERROR", "Internal Server Error", ""),
        (502, "BAD_GATEWAY", "Bad Gateway", ""),
        (503, "SERVICE_UNAVAILABLE", "Service Unavailable", ""),
        (504, "GATEWAY_TIMEOUT", "Gateway Timeout", ""),
    ];

    let mut status_attrs = IndexMap::new();
    for (code, name, phrase, desc) in &statuses {
        let member = make_http_status_member(*code, name, phrase, desc);
        status_attrs.insert(CompactString::from(*name), member);
    }
    let http_status = PyObject::module_with_attrs(CompactString::from("HTTPStatus"), status_attrs);

    let http_connection_cls = make_http_connection_class(80, "HTTPConnection", false);

    make_module(
        "http",
        vec![
            ("HTTPStatus", http_status),
            ("HTTPConnection", http_connection_cls),
            ("client", create_http_client_module()),
        ],
    )
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
    let body = if let Some(cl) = headers
        .get("Content-Length")
        .or_else(|| headers.get("content-length"))
    {
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
        let cls = PyObject::class(
            CompactString::from("BaseHTTPRequestHandler"),
            vec![],
            IndexMap::new(),
        );
        PyObject::instance(cls)
    };

    let wfile_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));

    if let PyObjectPayload::Instance(ref data) = inst.payload {
        let mut w = data.attrs.write();
        w.insert(
            CompactString::from("command"),
            PyObject::str_val(CompactString::from(req.method.as_str())),
        );
        w.insert(
            CompactString::from("path"),
            PyObject::str_val(CompactString::from(req.path.as_str())),
        );
        w.insert(
            CompactString::from("request_version"),
            PyObject::str_val(CompactString::from(req.version.as_str())),
        );

        // headers as a dict
        let mut hdr_map = IndexMap::new();
        for (k, v) in &req.headers {
            hdr_map.insert(
                HashableKey::str_key(CompactString::from(k.as_str())),
                PyObject::str_val(CompactString::from(v.as_str())),
            );
        }
        w.insert(CompactString::from("headers"), PyObject::dict(hdr_map));

        // rfile — a readable object wrapping the body
        let body_data = Arc::new(req.body.clone());
        let body_pos = Arc::new(Mutex::new(0usize));
        let bd = body_data.clone();
        let bp = body_pos.clone();
        w.insert(CompactString::from("rfile"), {
            let mut rfile_attrs = IndexMap::new();
            let bd2 = bd.clone();
            let bp2 = bp.clone();
            rfile_attrs.insert(
                CompactString::from("read"),
                PyObject::native_closure("rfile.read", move |args| {
                    let n = if !args.is_empty() {
                        args[0].as_int().unwrap_or(-1)
                    } else {
                        -1
                    };
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
            rfile_attrs.insert(
                CompactString::from("_bind_methods"),
                PyObject::bool_val(true),
            );
            PyObject::module_with_attrs(CompactString::from("rfile"), rfile_attrs)
        });

        // wfile — a writable buffer that accumulates the response body
        let wbuf = wfile_buf.clone();
        let mut wfile_attrs = IndexMap::new();
        let wbuf2 = wbuf.clone();
        wfile_attrs.insert(
            CompactString::from("write"),
            PyObject::native_closure("wfile.write", move |args| {
                if !args.is_empty() {
                    let data = match &args[0].payload {
                        PyObjectPayload::Bytes(b) => (**b).clone(),
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
        wfile_attrs.insert(
            CompactString::from("_bind_methods"),
            PyObject::bool_val(true),
        );
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
                let code = if !args.is_empty() {
                    args[0].as_int().unwrap_or(200) as u16
                } else {
                    200
                };
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
                let code = if !args.is_empty() {
                    args[0].as_int().unwrap_or(500) as u16
                } else {
                    500
                };
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
        for method_name in &[
            "do_GET",
            "do_POST",
            "do_PUT",
            "do_DELETE",
            "do_HEAD",
            "do_PATCH",
            "do_OPTIONS",
        ] {
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
        // log_request — no-op for now
        w.insert(
            CompactString::from("log_request"),
            PyObject::native_closure("log_request", |_args| Ok(PyObject::none())),
        );

        // responses — dict mapping status code to (shortmsg, longmsg)
        let mut responses_map = IndexMap::new();
        let status_data: Vec<(i64, &str, &str)> = vec![
            (100, "Continue", "Request received, please continue"),
            (200, "OK", "Request fulfilled, document follows"),
            (201, "Created", "Document created, URL follows"),
            (204, "No Content", "Request fulfilled, nothing follows"),
            (301, "Moved Permanently", "Object moved permanently"),
            (302, "Found", "Object moved temporarily"),
            (
                304,
                "Not Modified",
                "Document has not changed since given time",
            ),
            (
                400,
                "Bad Request",
                "Bad request syntax or unsupported method",
            ),
            (
                401,
                "Unauthorized",
                "No permission -- see authorization schemes",
            ),
            (
                403,
                "Forbidden",
                "Request forbidden -- authorization will not help",
            ),
            (404, "Not Found", "Nothing matches the given URI"),
            (
                405,
                "Method Not Allowed",
                "Specified method is invalid for this resource",
            ),
            (500, "Internal Server Error", "Server got itself in trouble"),
            (
                501,
                "Not Implemented",
                "Server does not support this operation",
            ),
            (502, "Bad Gateway", "Invalid responses from another gateway"),
            (
                503,
                "Service Unavailable",
                "The server cannot process the request due to load",
            ),
        ];
        for (code, short, long) in &status_data {
            responses_map.insert(
                ferrython_core::types::HashableKey::Int(ferrython_core::types::PyInt::Small(*code)),
                PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from(*short)),
                    PyObject::str_val(CompactString::from(*long)),
                ]),
            );
        }
        w.insert(
            CompactString::from("responses"),
            PyObject::dict(responses_map),
        );

        // server_version — identifies the server software
        w.insert(
            CompactString::from("server_version"),
            PyObject::str_val(CompactString::from("BaseHTTP/0.6")),
        );
        w.insert(
            CompactString::from("sys_version"),
            PyObject::str_val(CompactString::from("Ferrython/0.1")),
        );
        w.insert(
            CompactString::from("protocol_version"),
            PyObject::str_val(CompactString::from("HTTP/1.0")),
        );

        // version_string() — return server version string
        w.insert(
            CompactString::from("version_string"),
            PyObject::native_closure("version_string", |_args| {
                Ok(PyObject::str_val(CompactString::from(
                    "BaseHTTP/0.6 Ferrython/0.1",
                )))
            }),
        );

        // date_time_string(timestamp=None) — return HTTP date string
        w.insert(
            CompactString::from("date_time_string"),
            PyObject::native_closure("date_time_string", |args| {
                use std::time::{SystemTime, UNIX_EPOCH};
                let ts = if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None) {
                    args[0].as_int().unwrap_or(0) as u64
                } else {
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                };
                // Simple RFC 1123 date
                let secs = ts;
                let sec = secs % 60;
                let mins = secs / 60;
                let min = mins % 60;
                let hrs = mins / 60;
                let hour = hrs % 24;
                let mut days = (hrs / 24) as i64;
                let mut year = 1970i64;
                loop {
                    let dy = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                        366
                    } else {
                        365
                    };
                    if days < dy {
                        break;
                    }
                    days -= dy;
                    year += 1;
                }
                let month_days = [
                    31,
                    if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                        29
                    } else {
                        28
                    },
                    31,
                    30,
                    31,
                    30,
                    31,
                    31,
                    30,
                    31,
                    30,
                    31,
                ];
                let mut month = 0;
                while month < 12 && days >= month_days[month] {
                    days -= month_days[month];
                    month += 1;
                }
                let day = days + 1;
                let weekdays = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
                let months = [
                    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov",
                    "Dec",
                ];
                let wday = ((ts / 86400 + 3) % 7) as usize; // Jan 1 1970 was Thursday
                let formatted = format!(
                    "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
                    weekdays[wday], day, months[month], year, hour, min, sec
                );
                Ok(PyObject::str_val(CompactString::from(formatted)))
            }),
        );

        // translate_path(path) — translate URL path to filesystem path
        w.insert(
            CompactString::from("translate_path"),
            PyObject::native_closure("translate_path", |args| {
                let request_path = if !args.is_empty() {
                    args[0].py_to_string()
                } else {
                    "/".to_string()
                };
                let path = if let Some(idx) = request_path.find('?') {
                    &request_path[..idx]
                } else {
                    request_path.as_str()
                };
                let decoded = percent_decode(path);
                let rel_path = decoded.trim_start_matches('/');
                if rel_path.is_empty() {
                    Ok(PyObject::str_val(CompactString::from(".")))
                } else {
                    Ok(PyObject::str_val(CompactString::from(rel_path)))
                }
            }),
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
                let mut body =
                    String::from("<html><head><title>Directory listing</title></head><body>\n");
                body.push_str(&format!(
                    "<h1>Directory listing for /{}</h1>\n<hr><ul>\n",
                    rel_path
                ));
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
                    let h = if h.is_empty() {
                        "0.0.0.0".to_string()
                    } else {
                        h
                    };
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
            PyObject::class(
                CompactString::from("BaseHTTPRequestHandler"),
                vec![],
                IndexMap::new(),
            )
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
            w.insert(
                CompactString::from("server_name"),
                PyObject::str_val(CompactString::from(host.as_str())),
            );
            w.insert(
                CompactString::from("server_port"),
                PyObject::int(port as i64),
            );

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
                            let guard = ss
                                .lock()
                                .map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;
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
                        let guard = ss
                            .lock()
                            .map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;
                        match &guard.listener {
                            Some(l) => l
                                .try_clone()
                                .map_err(|e| PyException::os_error(format!("try_clone: {}", e)))?,
                            None => {
                                return Err(PyException::runtime_error("server is closed"));
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
                        Err(e) => Err(PyException::os_error(format!("accept: {}", e))),
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
                    let mut guard = ss
                        .lock()
                        .map_err(|e| PyException::runtime_error(format!("lock: {}", e)))?;
                    guard.listener = None;
                    Ok(PyObject::none())
                }),
            );

            // ── socket attribute (for fileno() etc.) ──
            w.insert(CompactString::from("socket"), PyObject::none());
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
        let dummy_cls = PyObject::class(
            CompactString::from("BaseHTTPRequestHandler"),
            vec![],
            IndexMap::new(),
        );
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
        let cls = PyObject::class(
            CompactString::from("SimpleHTTPRequestHandler"),
            vec![],
            IndexMap::new(),
        );
        let (inst, wbuf) = build_handler_instance(&req, &cls);

        // Override do_GET and do_HEAD with file-serving implementations
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(
                CompactString::from("do_GET"),
                simple_handler_do_get(wbuf.clone(), false),
            );
            w.insert(
                CompactString::from("do_HEAD"),
                simple_handler_do_get(wbuf.clone(), true),
            );

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
        // CGIHTTPRequestHandler — stub that inherits from SimpleHTTPRequestHandler
        ("CGIHTTPRequestHandler", make_builtin(|args: &[PyObjectRef]| {
            let req = HttpRequest {
                method: "GET".to_string(),
                path: "/".to_string(),
                version: "HTTP/1.1".to_string(),
                headers: IndexMap::new(),
                body: Vec::new(),
            };
            let cls = PyObject::class(CompactString::from("CGIHTTPRequestHandler"), vec![], IndexMap::new());
            let (inst, wbuf) = build_handler_instance(&req, &cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("do_GET"), simple_handler_do_get(wbuf.clone(), false));
                w.insert(CompactString::from("do_HEAD"), simple_handler_do_get(wbuf.clone(), true));
                w.insert(CompactString::from("cgi_directories"), PyObject::list(vec![
                    PyObject::str_val(CompactString::from("/cgi-bin")),
                    PyObject::str_val(CompactString::from("/htbin")),
                ]));
                if args.len() > 1 {
                    w.insert(CompactString::from("client_address"), args[1].clone());
                }
            }
            Ok(inst)
        })),
        // DEFAULT_ERROR_MESSAGE constant
        ("DEFAULT_ERROR_MESSAGE", PyObject::str_val(CompactString::from(
            "<html><head><title>Error</title></head><body><h1>%(code)d %(message)s</h1></body></html>"
        ))),
        // DEFAULT_ERROR_CONTENT_TYPE
        ("DEFAULT_ERROR_CONTENT_TYPE", PyObject::str_val(CompactString::from("text/html;charset=utf-8"))),
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
            w.insert(
                CompactString::from("do_GET"),
                simple_handler_do_get(wfile_buf.clone(), false),
            );
            w.insert(
                CompactString::from("do_HEAD"),
                simple_handler_do_get(wfile_buf.clone(), true),
            );
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
                PyObjectPayload::NativeClosure(nc) => (nc.func)(&[handler_inst.clone()]),
                PyObjectPayload::NativeFunction(nf) => (nf.func)(&[handler_inst.clone()]),
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
