//! HTTP, urllib, and SSL stdlib modules.

use compact_str::CompactString;
use ferrython_core::object::{
    make_module, NativeClosureData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;

mod cgi;
mod cookiejar;
mod cookies;
mod ftplib;
mod http_client;
mod http_server;
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
pub use http_server::create_http_server_module;
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
