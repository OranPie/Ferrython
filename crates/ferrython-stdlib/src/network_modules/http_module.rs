//! HTTP, urllib, and SSL stdlib modules.

use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, NativeClosureData, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
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
mod http_client;
mod imaplib;
mod poplib;
mod smtplib;
mod socketserver;
mod ssl;
mod urllib_parse;
mod urllib_request;
mod xmlrpc;

pub use cgi::create_cgi_module;
pub use cookiejar::create_http_cookiejar_module;
pub use cookies::create_http_cookies_module;
pub use ftplib::create_ftplib_module;
pub use http_client::create_http_client_module;
pub use imaplib::create_imaplib_module;
pub use poplib::create_poplib_module;
pub use smtplib::create_smtplib_module;
pub use socketserver::create_socketserver_module;
pub use ssl::create_ssl_module;
pub use urllib_parse::create_urllib_parse_module;
pub use urllib_request::create_urllib_module;
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
// http module
// ════════════════════════════════════════════════════════════════════════

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

    let http_connection_cls = http_client::make_http_connection_class(80, "HTTPConnection", false);

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
