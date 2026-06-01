use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, new_fx_hashkey_map, FxHashKeyMap, PropertyData, PyCell, PyObject,
    PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::cell::Cell;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UrlKind {
    Str,
    Bytes,
}

#[derive(Clone, Debug)]
struct UrlParts {
    scheme: String,
    netloc: String,
    path: String,
    params: String,
    query: String,
    fragment: String,
    username: Option<String>,
    password: Option<String>,
    hostname: Option<String>,
    port_text: Option<String>,
}

#[derive(Clone, Debug)]
enum PortState {
    Missing,
    Value(i64),
    Error(String),
}

fn str_obj(value: impl AsRef<str>) -> PyObjectRef {
    PyObject::str_val(CompactString::from(value.as_ref()))
}

fn bytes_obj(value: impl AsRef<str>) -> PyObjectRef {
    PyObject::bytes(value.as_ref().as_bytes().to_vec())
}

fn component_obj(kind: UrlKind, value: impl AsRef<str>) -> PyObjectRef {
    match kind {
        UrlKind::Str => str_obj(value),
        UrlKind::Bytes => bytes_obj(value),
    }
}

fn object_kind(obj: &PyObjectRef) -> PyResult<UrlKind> {
    match &obj.payload {
        PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_) => Ok(UrlKind::Bytes),
        PyObjectPayload::Str(_) => Ok(UrlKind::Str),
        _ => Ok(UrlKind::Str),
    }
}

fn object_text(obj: &PyObjectRef) -> String {
    match &obj.payload {
        PyObjectPayload::Str(s) => s.to_string(),
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
            String::from_utf8_lossy(b).into_owned()
        }
        PyObjectPayload::None => String::new(),
        _ => obj.py_to_string(),
    }
}

fn component_text(obj: &PyObjectRef) -> String {
    object_text(obj)
}

fn ensure_url_arg_kind(base: UrlKind, obj: &PyObjectRef) -> PyResult<()> {
    match (&obj.payload, base) {
        (PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_), UrlKind::Str) => Err(
            PyException::type_error("Cannot mix str and non-str arguments"),
        ),
        (PyObjectPayload::Str(s), UrlKind::Bytes) if !s.is_empty() => Err(PyException::type_error(
            "Cannot mix str and non-str arguments",
        )),
        _ => Ok(()),
    }
}

fn result_property(getter: fn(&[PyObjectRef]) -> PyResult<PyObjectRef>) -> PyObjectRef {
    PyObjectRef::new(PyObject {
        payload: PyObjectPayload::Property(Box::new(PropertyData {
            fget: Some(PyObject::native_function(
                "urllib.parse.result_property",
                getter,
            )),
            fset: None,
            fdel: None,
            doc: PyCell::new(None),
            doc_from_getter: Cell::new(false),
        })),
    })
}

fn result_names(names: &[&str]) -> Vec<PyObjectRef> {
    names.iter().map(|name| str_obj(*name)).collect()
}

fn string_tuple(items: &[&str]) -> PyObjectRef {
    PyObject::tuple(items.iter().map(|item| str_obj(*item)).collect())
}

fn make_result_class(name: &str, fields: &[&str]) -> PyObjectRef {
    let mut ns = IndexMap::new();
    let init_name = format!("{name}.__init__");
    let len_name = format!("{name}.__len__");
    let iter_name = format!("{name}.__iter__");
    let getitem_name = format!("{name}.__getitem__");
    let eq_name = format!("{name}.__eq__");
    let encode_name = format!("{name}.encode");
    let decode_name = format!("{name}.decode");
    let geturl_name = format!("{name}.geturl");
    let repr_name = format!("{name}.__repr__");
    ns.insert(CompactString::from("_fields"), string_tuple(fields));
    ns.insert(CompactString::from("__module__"), str_obj("urllib.parse"));
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_function(&init_name, result_init),
    );
    ns.insert(
        CompactString::from("__len__"),
        PyObject::native_closure(&len_name, |args| {
            if args.is_empty() {
                return Ok(PyObject::int(0));
            }
            if let Some(fields) = args[0].get_attr("_fields") {
                return Ok(PyObject::int(fields.py_len()? as i64));
            }
            Ok(PyObject::int(0))
        }),
    );
    ns.insert(
        CompactString::from("__iter__"),
        PyObject::native_closure(&iter_name, |args| {
            if args.is_empty() {
                return Err(PyException::type_error("__iter__ requires self"));
            }
            result_tuple_from_instance(&args[0])
        }),
    );
    ns.insert(
        CompactString::from("__getitem__"),
        PyObject::native_closure(&getitem_name, |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("__getitem__ requires index"));
            }
            let tuple = result_tuple_from_instance(&args[0])?;
            tuple.get_item(&args[1])
        }),
    );
    ns.insert(
        CompactString::from("__eq__"),
        PyObject::native_function(&eq_name, result_eq),
    );
    ns.insert(
        CompactString::from("encode"),
        PyObject::native_function(&encode_name, result_encode),
    );
    ns.insert(
        CompactString::from("decode"),
        PyObject::native_function(&decode_name, result_decode),
    );
    ns.insert(
        CompactString::from("geturl"),
        PyObject::native_function(&geturl_name, result_geturl),
    );
    ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_function(&repr_name, result_repr),
    );
    ns.insert(
        CompactString::from("hostname"),
        result_property(result_hostname),
    );
    ns.insert(
        CompactString::from("username"),
        result_property(result_username),
    );
    ns.insert(
        CompactString::from("password"),
        result_property(result_password),
    );
    ns.insert(CompactString::from("port"), result_property(result_port));
    PyObject::class(CompactString::from(name), vec![], ns)
}

fn link_result_pairs(str_cls: &PyObjectRef, bytes_cls: &PyObjectRef) {
    if let PyObjectPayload::Class(cd) = &str_cls.payload {
        cd.namespace.write().insert(
            CompactString::from("_encoded_counterpart"),
            bytes_cls.clone(),
        );
        cd.invalidate_cache();
    }
    if let PyObjectPayload::Class(cd) = &bytes_cls.payload {
        cd.namespace
            .write()
            .insert(CompactString::from("_decoded_counterpart"), str_cls.clone());
        cd.invalidate_cache();
    }
}

fn result_tuple_from_instance(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    let fields = obj
        .get_attr("_fields")
        .ok_or_else(|| PyException::attribute_error("_fields"))?;
    let names = fields.to_list()?;
    let mut values = Vec::with_capacity(names.len());
    for name in names {
        values.push(
            obj.get_attr(&name.py_to_string())
                .unwrap_or_else(|| PyObject::str_val(CompactString::from(""))),
        );
    }
    Ok(PyObject::tuple(values))
}

fn result_kind(obj: &PyObjectRef) -> UrlKind {
    if let Some(kind) = obj.get_attr("_kind") {
        if kind.py_to_string() == "bytes" {
            return UrlKind::Bytes;
        }
    }
    UrlKind::Str
}

fn result_init(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("__init__ requires self"));
    }
    let fields = args[0]
        .get_attr("_fields")
        .ok_or_else(|| PyException::attribute_error("_fields"))?;
    let names = fields.to_list()?;
    let values = &args[1..];
    if values.len() != names.len() {
        return Err(PyException::type_error(format!(
            "expected {} arguments, got {}",
            names.len(),
            values.len()
        )));
    }
    let kind = values
        .iter()
        .find_map(|value| match &value.payload {
            PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_) => Some(UrlKind::Bytes),
            _ => None,
        })
        .unwrap_or(UrlKind::Str);
    if let PyObjectPayload::Instance(inst) = &args[0].payload {
        let mut attrs = inst.attrs.write();
        attrs.insert(
            CompactString::from("_kind"),
            str_obj(if kind == UrlKind::Bytes {
                "bytes"
            } else {
                "str"
            }),
        );
        for (field, value) in names.into_iter().zip(values.iter().cloned()) {
            attrs.insert(CompactString::from(field.py_to_string()), value);
        }
    }
    Ok(PyObject::none())
}

fn result_field_strings(obj: &PyObjectRef) -> PyResult<Vec<String>> {
    result_tuple_from_instance(obj)?
        .to_list()?
        .into_iter()
        .map(|value| Ok(component_text(&value)))
        .collect()
}

fn result_eq(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    let left = result_tuple_from_instance(&args[0])?;
    Ok(left.compare(&args[1], ferrython_core::object::CompareOp::Eq)?)
}

fn result_attr_text(obj: &PyObjectRef, name: &str) -> Option<String> {
    obj.get_attr(name).map(|value| component_text(&value))
}

fn result_hostname(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::none());
    }
    let kind = result_kind(&args[0]);
    let host = result_attr_text(&args[0], "_hostname")
        .or_else(|| parse_netloc_info(&result_attr_text(&args[0], "netloc").unwrap_or_default()).2);
    Ok(match host {
        Some(host) => component_obj(kind, host),
        None => PyObject::none(),
    })
}

fn result_username(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::none());
    }
    let kind = result_kind(&args[0]);
    let value = result_attr_text(&args[0], "_username")
        .or_else(|| parse_netloc_info(&result_attr_text(&args[0], "netloc").unwrap_or_default()).0);
    Ok(match value {
        Some(value) => component_obj(kind, value),
        None => PyObject::none(),
    })
}

fn result_password(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::none());
    }
    let kind = result_kind(&args[0]);
    let value = result_attr_text(&args[0], "_password")
        .or_else(|| parse_netloc_info(&result_attr_text(&args[0], "netloc").unwrap_or_default()).1);
    Ok(match value {
        Some(value) => component_obj(kind, value),
        None => PyObject::none(),
    })
}

fn result_port(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::none());
    }
    let port_text = result_attr_text(&args[0], "_port_text")
        .or_else(|| parse_netloc_info(&result_attr_text(&args[0], "netloc").unwrap_or_default()).3);
    match port_state(port_text.as_deref()) {
        PortState::Missing => Ok(PyObject::none()),
        PortState::Value(port) => Ok(PyObject::int(port)),
        PortState::Error(value) => Err(PyException::value_error(format!(
            "{}",
            if value.chars().all(|c| c.is_ascii_digit()) {
                format!("Port out of range 0-65535")
            } else {
                format!("Port could not be cast to integer value as '{}'", value)
            }
        ))),
    }
}

fn result_encode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("encode requires self"));
    }
    let bytes_cls = args[0]
        .get_attr("_encoded_counterpart")
        .unwrap_or_else(|| make_result_class("ResultBytes", &[]));
    let values = result_field_strings(&args[0])?;
    let dummy = UrlParts {
        scheme: String::new(),
        netloc: String::new(),
        path: String::new(),
        params: String::new(),
        query: String::new(),
        fragment: String::new(),
        username: None,
        password: None,
        hostname: None,
        port_text: None,
    };
    Ok(make_url_result_instance(
        bytes_cls,
        UrlKind::Bytes,
        values,
        &dummy,
    ))
}

fn result_decode(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("decode requires self"));
    }
    let str_cls = args[0]
        .get_attr("_decoded_counterpart")
        .unwrap_or_else(|| make_result_class("Result", &[]));
    let values = result_field_strings(&args[0])?;
    let dummy = UrlParts {
        scheme: String::new(),
        netloc: String::new(),
        path: String::new(),
        params: String::new(),
        query: String::new(),
        fragment: String::new(),
        username: None,
        password: None,
        hostname: None,
        port_text: None,
    };
    Ok(make_url_result_instance(
        str_cls,
        UrlKind::Str,
        values,
        &dummy,
    ))
}

fn result_geturl(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("geturl requires self"));
    }
    let values = result_field_strings(&args[0])?;
    let url = match values.len() {
        2 => {
            if values[1].is_empty() {
                values[0].clone()
            } else {
                format!("{}#{}", values[0], values[1])
            }
        }
        5 => assemble_url(
            &values[0], &values[1], &values[2], "", &values[3], &values[4],
        ),
        6 => assemble_url(
            &values[0], &values[1], &values[2], &values[3], &values[4], &values[5],
        ),
        _ => values.join(""),
    };
    Ok(component_obj(result_kind(&args[0]), url))
}

fn result_repr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(str_obj("Result()"));
    }
    let type_name = args[0].type_name();
    let fields = args[0]
        .get_attr("_fields")
        .unwrap_or_else(|| PyObject::tuple(vec![]));
    let names = fields.to_list()?;
    let values = result_field_strings(&args[0])?;
    let parts = names
        .iter()
        .zip(values.iter())
        .map(|(name, value)| format!("{}='{}'", name.py_to_string(), value))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(PyObject::str_val(CompactString::from(format!(
        "{type_name}({parts})"
    ))))
}

fn make_result_instance(cls: PyObjectRef, values: Vec<PyObjectRef>) -> PyObjectRef {
    let mut attrs = IndexMap::new();
    let fields = cls
        .get_attr("_fields")
        .and_then(|fields| fields.to_list().ok())
        .unwrap_or_default();
    for (field, value) in fields.into_iter().zip(values.iter().cloned()) {
        attrs.insert(CompactString::from(field.py_to_string()), value);
    }
    PyObject::instance_with_attrs(cls, attrs)
}

fn make_url_result_instance(
    cls: PyObjectRef,
    kind: UrlKind,
    values: Vec<String>,
    parts: &UrlParts,
) -> PyObjectRef {
    let mut objects = values
        .into_iter()
        .map(|value| component_obj(kind, value))
        .collect::<Vec<_>>();
    let inst = make_result_instance(cls, objects.drain(..).collect());
    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(
            CompactString::from("_kind"),
            str_obj(if kind == UrlKind::Bytes {
                "bytes"
            } else {
                "str"
            }),
        );
        if let Some(username) = &parts.username {
            attrs.insert(
                CompactString::from("_username"),
                component_obj(kind, username),
            );
        }
        if let Some(password) = &parts.password {
            attrs.insert(
                CompactString::from("_password"),
                component_obj(kind, password),
            );
        }
        if let Some(hostname) = &parts.hostname {
            attrs.insert(
                CompactString::from("_hostname"),
                component_obj(kind, hostname),
            );
        }
        if let Some(port_text) = &parts.port_text {
            attrs.insert(CompactString::from("_port_text"), str_obj(port_text));
        }
    }
    inst
}

fn assemble_url(
    scheme: &str,
    netloc: &str,
    path: &str,
    params: &str,
    query: &str,
    fragment: &str,
) -> String {
    let mut url = String::from(path);
    if !params.is_empty() {
        url.push(';');
        url.push_str(params);
    }
    if !netloc.is_empty()
        || (!scheme.is_empty() && uses_netloc_scheme(scheme) && !url.starts_with("//"))
    {
        if !url.is_empty() && !url.starts_with('/') {
            url.insert(0, '/');
        }
        url = format!("//{}{}", netloc, url);
    } else if url.starts_with("//") {
        url.insert_str(0, "//");
    }
    if !scheme.is_empty() {
        url = format!("{scheme}:{url}");
    }
    if !query.is_empty() {
        url.push('?');
        url.push_str(query);
    }
    if !fragment.is_empty() {
        url.push('#');
        url.push_str(fragment);
    }
    url
}

fn uses_netloc_scheme(scheme: &str) -> bool {
    matches!(
        scheme.to_ascii_lowercase().as_str(),
        "ftp"
            | "http"
            | "gopher"
            | "nntp"
            | "telnet"
            | "file"
            | "https"
            | "shttp"
            | "snews"
            | "prospero"
            | "rtsp"
            | "rtspu"
            | "svn"
            | "svn+ssh"
            | "sftp"
            | "nfs"
            | "git"
            | "git+ssh"
            | "ssh"
            | "ws"
            | "wss"
    )
}

fn quote_bytes(data: &[u8], safe: &[u8], plus_for_space: bool) -> String {
    let mut result = String::with_capacity(data.len());
    for &b in data {
        if plus_for_space && b == b' ' {
            result.push('+');
        } else if (b as char).is_ascii_alphanumeric()
            || matches!(b, b'-' | b'_' | b'.' | b'~')
            || safe.contains(&b)
        {
            result.push(b as char);
        } else {
            result.push_str(&format!("%{:02X}", b));
        }
    }
    result
}

fn quote_encode(s: &str, safe: &str) -> String {
    quote_bytes(s.as_bytes(), safe.as_bytes(), false)
}

fn quote_plus_encode_with_safe(s: &str, safe: &str) -> String {
    quote_bytes(s.as_bytes(), safe.as_bytes(), true)
}

fn percent_decode_bytes(s: &str) -> Vec<u8> {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    result.push(value);
                    i += 3;
                    continue;
                }
            }
        }
        if let Some(ch) = s[i..].chars().next() {
            let mut buf = [0u8; 4];
            result.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    result
}

fn decode_percent_component(s: &str, encoding: &str, errors: &str) -> String {
    match encoding.to_ascii_lowercase().as_str() {
        "latin-1" | "latin1" | "iso-8859-1" => decode_percent_mixed_latin1(s),
        "ascii" => decode_percent_mixed_ascii(s, errors),
        _ => String::from_utf8_lossy(&percent_decode_form_bytes(s)).into_owned(),
    }
}

fn decode_percent_mixed_latin1(s: &str) -> String {
    let mut result = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    result.push(value as char);
                    i += 3;
                    continue;
                }
            }
        }
        if let Some(ch) = s[i..].chars().next() {
            result.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    result
}

fn decode_percent_mixed_ascii(s: &str, errors: &str) -> String {
    let mut result = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    if value < 0x80 {
                        result.push(value as char);
                    } else if errors != "ignore" {
                        result.push('\u{fffd}');
                    }
                    i += 3;
                    continue;
                }
            }
        }
        if let Some(ch) = s[i..].chars().next() {
            result.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    result
}

fn percent_decode_form_bytes(s: &str) -> Vec<u8> {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    result.push(value);
                    i += 3;
                    continue;
                }
            }
        }
        if let Some(ch) = s[i..].chars().next() {
            if ch.is_ascii() {
                result.push(ch as u8);
            } else {
                result.extend_from_slice(ch.to_string().as_bytes());
            }
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    result
}

fn clean_url_input(url: &str) -> String {
    url.trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(*c, '\t' | '\n' | '\r') && *c >= ' ')
        .collect()
}

fn clean_scheme_input(scheme: &str) -> String {
    clean_url_input(scheme)
        .trim_matches(|c: char| c <= ' ')
        .to_string()
}

fn is_scheme_text(candidate: &str) -> bool {
    let mut chars = candidate.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn split_scheme(url: &str) -> (String, &str) {
    if let Some(idx) = url.find(':') {
        let candidate = &url[..idx];
        if is_scheme_text(candidate) {
            if candidate.eq_ignore_ascii_case("path")
                && url[idx + 1..].chars().all(|c| c.is_ascii_digit())
            {
                return (String::new(), url);
            }
            return (candidate.to_ascii_lowercase(), &url[idx + 1..]);
        }
    }
    (String::new(), url)
}

fn parse_netloc_info(
    netloc: &str,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    if netloc.is_empty() {
        return (None, None, None, None);
    }
    let host_port = netloc.rsplit('@').next().unwrap_or(netloc);
    let userinfo = netloc.rsplit_once('@').map(|(user, _)| user);
    let (username, password) = if let Some(userinfo) = userinfo {
        if let Some((user, pass)) = userinfo.rsplit_once(':') {
            (Some(user.to_string()), Some(pass.to_string()))
        } else {
            (Some(userinfo.to_string()), None)
        }
    } else {
        (None, None)
    };

    let (host, port_text) = if let Some(rest) = host_port.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            let host = rest[..end].to_string();
            let port_text = rest[end + 1..].strip_prefix(':').map(|s| s.to_string());
            (Some(normalize_hostname(&host)), port_text)
        } else {
            (Some(host_port.to_ascii_lowercase()), None)
        }
    } else if let Some((host, port)) = host_port.rsplit_once(':') {
        (Some(host.to_ascii_lowercase()), Some(port.to_string()))
    } else if host_port.is_empty() {
        (None, None)
    } else {
        (Some(host_port.to_ascii_lowercase()), None)
    };

    (username, password, host, port_text)
}

fn normalize_hostname(host: &str) -> String {
    if let Some((addr, zone)) = host.rsplit_once('%') {
        format!("{}%{}", addr.to_ascii_lowercase(), zone)
    } else {
        host.to_ascii_lowercase()
    }
}

fn port_state(port_text: Option<&str>) -> PortState {
    let Some(text) = port_text else {
        return PortState::Missing;
    };
    if text.is_empty() {
        return PortState::Missing;
    }
    if !text.chars().all(|c| c.is_ascii_digit()) {
        return PortState::Error(text.to_string());
    }
    match text.parse::<i64>() {
        Ok(port) if (0..=65535).contains(&port) => PortState::Value(port),
        Ok(_) => PortState::Error(text.to_string()),
        Err(_) => PortState::Error(text.to_string()),
    }
}

fn parse_url_parts(url: &str, default_scheme: &str, allow_fragments: bool) -> UrlParts {
    let url = clean_url_input(url);
    if has_forbidden_netloc_char_after_normalization(&url) {
        return UrlParts {
            scheme: "__invalid__".to_string(),
            netloc: String::new(),
            path: String::new(),
            params: String::new(),
            query: String::new(),
            fragment: String::new(),
            username: None,
            password: None,
            hostname: None,
            port_text: None,
        };
    }
    let (parsed_scheme, mut rest) = split_scheme(&url);
    let scheme = if parsed_scheme.is_empty() {
        clean_scheme_input(default_scheme).to_ascii_lowercase()
    } else {
        parsed_scheme
    };
    let mut fragment = String::new();
    if allow_fragments {
        if let Some(idx) = rest.find('#') {
            fragment = rest[idx + 1..].to_string();
            rest = &rest[..idx];
        }
    }
    let mut query = String::new();
    if let Some(idx) = rest.find('?') {
        query = rest[idx + 1..].to_string();
        rest = &rest[..idx];
    }

    let (netloc, path) = if let Some(after_slashes) = rest.strip_prefix("//") {
        if after_slashes.starts_with('/') {
            (String::new(), after_slashes.to_string())
        } else {
            let split_at = after_slashes
                .find(|c| matches!(c, '/' | '?' | '#'))
                .unwrap_or(after_slashes.len());
            (
                after_slashes[..split_at].to_string(),
                after_slashes[split_at..].to_string(),
            )
        }
    } else if scheme.is_empty() && rest.starts_with("////") {
        (String::new(), rest[2..].to_string())
    } else if scheme.is_empty() && rest.starts_with("///") {
        (String::new(), rest[2..].to_string())
    } else {
        (String::new(), rest.to_string())
    };
    let path = path;
    let (username, password, hostname, port_text) = parse_netloc_info(&netloc);
    if (netloc.contains('[') && !netloc.contains(']'))
        || (netloc.contains(']') && !netloc.contains('['))
    {
        return UrlParts {
            scheme: "__invalid__".to_string(),
            netloc,
            path,
            params: String::new(),
            query,
            fragment,
            username,
            password,
            hostname,
            port_text,
        };
    }
    UrlParts {
        scheme,
        netloc,
        path,
        params: String::new(),
        query,
        fragment,
        username,
        password,
        hostname,
        port_text,
    }
}

fn has_forbidden_netloc_char_after_normalization(url: &str) -> bool {
    if !url.contains("://") {
        return false;
    }
    url.chars()
        .any(|c| matches!(c, '\u{2100}' | '\u{FF03}' | '\u{FE13}'))
}

fn split_parse_params(mut parts: UrlParts) -> UrlParts {
    let segment_start = parts.path.rfind('/').map(|idx| idx + 1).unwrap_or(0);
    if let Some(offset) = parts.path[segment_start..].find(';') {
        let idx = segment_start + offset;
        parts.params = parts.path[idx + 1..].to_string();
        parts.path.truncate(idx);
    }
    parts
}

fn canonical_geturl_for_parts(parts: &UrlParts, include_params: bool) -> String {
    assemble_url(
        &parts.scheme,
        &parts.netloc,
        &parts.path,
        if include_params { &parts.params } else { "" },
        &parts.query,
        &parts.fragment,
    )
}

fn component_sequence_kind(components: &[PyObjectRef]) -> UrlKind {
    if components.iter().any(|obj| {
        matches!(
            &obj.payload,
            PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_)
        )
    }) {
        UrlKind::Bytes
    } else {
        UrlKind::Str
    }
}

fn reject_mixed_components(components: &[PyObjectRef]) -> PyResult<()> {
    let has_bytes = components.iter().any(|obj| {
        matches!(
            &obj.payload,
            PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_)
        )
    });
    let has_str = components
        .iter()
        .any(|obj| matches!(&obj.payload, PyObjectPayload::Str(s) if !s.is_empty()));
    if has_bytes && has_str {
        Err(PyException::type_error(
            "Cannot mix str and non-str arguments",
        ))
    } else {
        Ok(())
    }
}

fn deprecated_split_wrapper(
    name: &'static str,
    f: fn(&[PyObjectRef]) -> PyResult<PyObjectRef>,
) -> PyObjectRef {
    PyObject::native_closure(name, move |args| {
        let target = if name == "splitvalue" {
            "use urllib.parse.parse_qsl() instead"
        } else if name == "to_bytes" {
            ""
        } else {
            "use urllib.parse.urlparse() instead"
        };
        let message = if target.is_empty() {
            format!("urllib.parse.{name}() is deprecated as of 3.8")
        } else {
            format!("urllib.parse.{name}() is deprecated as of 3.8, {target}")
        };
        crate::introspection_modules::emit_deprecation_warning(&message);
        f(args)
    })
}

pub fn create_urllib_parse_module() -> PyObjectRef {
    let defrag_result = make_result_class("DefragResult", &["url", "fragment"]);
    let split_result = make_result_class(
        "SplitResult",
        &["scheme", "netloc", "path", "query", "fragment"],
    );
    let parse_result = make_result_class(
        "ParseResult",
        &["scheme", "netloc", "path", "params", "query", "fragment"],
    );
    let defrag_result_bytes = make_result_class("DefragResultBytes", &["url", "fragment"]);
    let split_result_bytes = make_result_class(
        "SplitResultBytes",
        &["scheme", "netloc", "path", "query", "fragment"],
    );
    let parse_result_bytes = make_result_class(
        "ParseResultBytes",
        &["scheme", "netloc", "path", "params", "query", "fragment"],
    );
    link_result_pairs(&defrag_result, &defrag_result_bytes);
    link_result_pairs(&split_result, &split_result_bytes);
    link_result_pairs(&parse_result, &parse_result_bytes);

    make_module(
        "urllib.parse",
        vec![
            (
                "__all__",
                PyObject::list(result_names(&[
                    "DefragResult",
                    "DefragResultBytes",
                    "ParseResult",
                    "ParseResultBytes",
                    "SplitResult",
                    "SplitResultBytes",
                ])),
            ),
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
            ("_splittype", make_builtin(urllib_parse_splittype)),
            ("_splithost", make_builtin(urllib_parse_splithost)),
            ("_splituser", make_builtin(urllib_parse_splituser)),
            ("_splitpasswd", make_builtin(urllib_parse_splitpasswd)),
            ("_splitport", make_builtin(urllib_parse_splitport)),
            ("_splitnport", make_builtin(urllib_parse_splitnport)),
            ("_splitquery", make_builtin(urllib_parse_splitquery)),
            ("_splittag", make_builtin(urllib_parse_splittag)),
            ("_splitattr", make_builtin(urllib_parse_splitattr)),
            ("_splitvalue", make_builtin(urllib_parse_splitvalue)),
            ("_to_bytes", make_builtin(urllib_parse_to_bytes)),
            ("unwrap", make_builtin(urllib_parse_unwrap)),
            (
                "splittype",
                deprecated_split_wrapper("splittype", urllib_parse_splittype),
            ),
            (
                "splithost",
                deprecated_split_wrapper("splithost", urllib_parse_splithost),
            ),
            (
                "splituser",
                deprecated_split_wrapper("splituser", urllib_parse_splituser),
            ),
            (
                "splitpasswd",
                deprecated_split_wrapper("splitpasswd", urllib_parse_splitpasswd),
            ),
            (
                "splitport",
                deprecated_split_wrapper("splitport", urllib_parse_splitport),
            ),
            (
                "splitnport",
                deprecated_split_wrapper("splitnport", urllib_parse_splitnport),
            ),
            (
                "splitquery",
                deprecated_split_wrapper("splitquery", urllib_parse_splitquery),
            ),
            (
                "splittag",
                deprecated_split_wrapper("splittag", urllib_parse_splittag),
            ),
            (
                "splitattr",
                deprecated_split_wrapper("splitattr", urllib_parse_splitattr),
            ),
            (
                "splitvalue",
                deprecated_split_wrapper("splitvalue", urllib_parse_splitvalue),
            ),
            (
                "to_bytes",
                deprecated_split_wrapper("to_bytes", urllib_parse_to_bytes),
            ),
            ("Quoter", make_builtin(urllib_parse_quoter)),
            (
                "ResultBase",
                PyObject::class(CompactString::from("ResultBase"), vec![], IndexMap::new()),
            ),
            (
                "_ALWAYS_SAFE",
                PyObject::bytes(
                    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_.-~".to_vec(),
                ),
            ),
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
                "uses_params",
                PyObject::list(
                    vec![
                        "ftp", "hdl", "prospero", "http", "imap", "https", "shttp", "rtsp",
                        "rtspu", "sip", "sips", "mms", "",
                    ]
                    .into_iter()
                    .map(|s| PyObject::str_val(CompactString::from(s)))
                    .collect(),
                ),
            ),
            (
                "uses_query",
                PyObject::list(
                    vec![
                        "http", "wais", "imap", "https", "shttp", "mms", "gopher", "rtsp", "rtspu",
                        "sip", "sips", "",
                    ]
                    .into_iter()
                    .map(|s| PyObject::str_val(CompactString::from(s)))
                    .collect(),
                ),
            ),
            (
                "uses_fragment",
                PyObject::list(
                    vec![
                        "ftp", "hdl", "http", "gopher", "news", "nntp", "wais", "https", "shttp",
                        "snews", "file", "prospero", "",
                    ]
                    .into_iter()
                    .map(|s| PyObject::str_val(CompactString::from(s)))
                    .collect(),
                ),
            ),
            (
                "non_hierarchical",
                PyObject::list(
                    vec![
                        "gopher", "hdl", "mailto", "news", "telnet", "wais", "imap", "snews",
                        "sip", "sips",
                    ]
                    .into_iter()
                    .map(|s| PyObject::str_val(CompactString::from(s)))
                    .collect(),
                ),
            ),
            ("ParseResult", parse_result),
            ("SplitResult", split_result),
            ("DefragResult", defrag_result),
            ("SplitResultBytes", split_result_bytes),
            ("ParseResultBytes", parse_result_bytes),
            ("DefragResultBytes", defrag_result_bytes),
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
    let (pos_args, kwargs) = split_kwargs(args);
    if pos_args.is_empty() {
        return Err(PyException::type_error(
            "urlencode() requires a mapping argument",
        ));
    }
    let doseq = pos_args.get(1).map(|v| v.is_truthy()).unwrap_or(false);
    let safe = kw_value(kwargs.as_ref(), "safe")
        .map(|v| object_text(&v))
        .unwrap_or_default();
    let quote_via_is_quote = kw_value(kwargs.as_ref(), "quote_via").is_some();
    let encode = |s: &str| {
        if quote_via_is_quote {
            quote_encode(s, &safe)
        } else {
            quote_plus_encode_with_safe(s, &safe)
        }
    };
    let val_to_str = |v: &PyObjectRef| -> String {
        match &v.payload {
            PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
                String::from_utf8_lossy(b).into_owned()
            }
            _ => v.py_to_string(),
        }
    };
    let mut pairs = Vec::new();
    match &pos_args[0].payload {
        PyObjectPayload::Dict(d) => {
            let d = d.read();
            for (k, v) in d.iter() {
                let ks = match k {
                    HashableKey::Str(s) => s.to_string(),
                    HashableKey::Int(i) => i.to_string(),
                    _ => format!("{:?}", k),
                };
                let direct_str_value = if let PyObjectPayload::Instance(inst) = &v.payload {
                    if let Some(method) =
                        ferrython_core::object::lookup_in_class_mro(&inst.class, "__str__")
                    {
                        ferrython_core::object::call_callable(&method, &[v.clone()])
                            .ok()
                            .map(|value| object_text(&value))
                    } else {
                        None
                    }
                } else {
                    None
                };
                if doseq {
                    if let Ok(values) = v.to_list() {
                        if !matches!(
                            &v.payload,
                            PyObjectPayload::Str(_) | PyObjectPayload::Bytes(_)
                        ) {
                            for item in values {
                                pairs.push(format!(
                                    "{}={}",
                                    encode(&ks),
                                    encode(&val_to_str(&item))
                                ));
                            }
                            continue;
                        }
                    }
                }
                let vs = direct_str_value.unwrap_or_else(|| val_to_str(&v));
                pairs.push(format!("{}={}", encode(&ks), encode(&vs)));
            }
        }
        PyObjectPayload::List(items) => {
            let items = items.read();
            for item in items.iter() {
                if let PyObjectPayload::Tuple(pair) = &item.payload {
                    if pair.len() >= 2 {
                        pairs.push(format!(
                            "{}={}",
                            encode(&val_to_str(&pair[0])),
                            encode(&val_to_str(&pair[1]))
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
    if matches!(
        &args[0].payload,
        PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_)
    ) && (kw_value(arg_kwargs(args), "encoding").is_some()
        || kw_value(arg_kwargs(args), "errors").is_some())
    {
        return Err(PyException::type_error(
            "quote() doesn't support 'encoding' or 'errors' for bytes",
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
    Ok(PyObject::str_val(CompactString::from(quote_encode(
        &s, &safe,
    ))))
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
    Ok(PyObject::str_val(CompactString::from(
        quote_plus_encode_with_safe(&s, &safe),
    )))
}

fn urllib_parse_quote_from_bytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "quote_from_bytes() requires a bytes argument",
        ));
    }
    let data = match &args[0].payload {
        PyObjectPayload::Bytes(b) => (**b).clone(),
        _ => return Err(PyException::type_error("quote_from_bytes: expected bytes")),
    };
    let safe = if args.len() > 1 {
        args[1].py_to_string()
    } else {
        "/".to_string()
    };
    let result = quote_bytes(&data, safe.as_bytes(), false);
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn urllib_parse_unquote_to_bytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "unquote_to_bytes() requires a string argument",
        ));
    }
    let s = object_text(&args[0]);
    Ok(PyObject::bytes(percent_decode_bytes(&s)))
}

fn urllib_parse_unquote(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "unquote() requires a string argument",
        ));
    }
    let s = object_text(&args[0]);
    let encoding = kw_value(arg_kwargs(args), "encoding")
        .map(|v| object_text(&v))
        .unwrap_or_else(|| "utf-8".to_string());
    let errors = kw_value(arg_kwargs(args), "errors")
        .map(|v| object_text(&v))
        .unwrap_or_else(|| "replace".to_string());
    Ok(str_obj(decode_percent_component(&s, &encoding, &errors)))
}

fn urllib_parse_unquote_plus(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "unquote_plus() requires a string argument",
        ));
    }
    let s = object_text(&args[0]).replace('+', " ");
    let encoding = kw_value(arg_kwargs(args), "encoding")
        .map(|v| object_text(&v))
        .unwrap_or_else(|| "utf-8".to_string());
    let errors = kw_value(arg_kwargs(args), "errors")
        .map(|v| object_text(&v))
        .unwrap_or_else(|| "replace".to_string());
    Ok(str_obj(decode_percent_component(&s, &encoding, &errors)))
}

fn urllib_parse_urlparse(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "urlparse() requires a string argument",
        ));
    }
    let (pos_args, kwargs) = split_kwargs(args);
    let kind = object_kind(&pos_args[0])?;
    if let Some(scheme) = pos_args.get(1) {
        ensure_url_arg_kind(kind, scheme)?;
    }
    if let Some(scheme) = kw_value(kwargs.as_ref(), "scheme") {
        ensure_url_arg_kind(kind, &scheme)?;
    }
    let default_scheme = kw_value(kwargs.as_ref(), "scheme")
        .map(|v| object_text(&v))
        .or_else(|| pos_args.get(1).map(object_text))
        .unwrap_or_default();
    let allow_fragments = pos_args
        .get(2)
        .map(|value| value.is_truthy())
        .or_else(|| kw_value(kwargs.as_ref(), "allow_fragments").map(|value| value.is_truthy()))
        .unwrap_or(true);
    let parts = split_parse_params(parse_url_parts(
        &object_text(&pos_args[0]),
        &default_scheme,
        allow_fragments,
    ));
    if parts.scheme == "__invalid__" {
        return Err(PyException::value_error(
            "netloc contains invalid characters",
        ));
    }
    let class = make_result_class(
        if kind == UrlKind::Bytes {
            "ParseResultBytes"
        } else {
            "ParseResult"
        },
        &["scheme", "netloc", "path", "params", "query", "fragment"],
    );
    Ok(make_url_result_instance(
        class,
        kind,
        vec![
            parts.scheme.clone(),
            parts.netloc.clone(),
            parts.path.clone(),
            parts.params.clone(),
            parts.query.clone(),
            parts.fragment.clone(),
        ],
        &parts,
    ))
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
    reject_mixed_components(&components)?;
    let kind = component_sequence_kind(&components);
    let to_str = |obj: &PyObjectRef| -> String { component_text(obj) };
    let scheme = to_str(&components[0]);
    let netloc = to_str(&components[1]);
    let path = to_str(&components[2]);
    let params = to_str(&components[3]);
    let query = to_str(&components[4]);
    let fragment = to_str(&components[5]);

    Ok(component_obj(
        kind,
        assemble_url(&scheme, &netloc, &path, &params, &query, &fragment),
    ))
}

fn urllib_parse_urlsplit(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("urlsplit() requires 1 argument"));
    }
    let (pos_args, kwargs) = split_kwargs(args);
    let kind = object_kind(&pos_args[0])?;
    if let Some(scheme) = pos_args.get(1) {
        ensure_url_arg_kind(kind, scheme)?;
    }
    if let Some(scheme) = kw_value(kwargs.as_ref(), "scheme") {
        ensure_url_arg_kind(kind, &scheme)?;
    }
    let default_scheme = kw_value(kwargs.as_ref(), "scheme")
        .map(|v| object_text(&v))
        .or_else(|| pos_args.get(1).map(object_text))
        .unwrap_or_default();
    let allow_fragments = pos_args
        .get(2)
        .map(|value| value.is_truthy())
        .or_else(|| kw_value(kwargs.as_ref(), "allow_fragments").map(|value| value.is_truthy()))
        .unwrap_or(true);
    let parts = parse_url_parts(&object_text(&pos_args[0]), &default_scheme, allow_fragments);
    if parts.scheme == "__invalid__" {
        return Err(PyException::value_error(
            "netloc contains invalid characters",
        ));
    }
    let class = make_result_class(
        if kind == UrlKind::Bytes {
            "SplitResultBytes"
        } else {
            "SplitResult"
        },
        &["scheme", "netloc", "path", "query", "fragment"],
    );
    Ok(make_url_result_instance(
        class,
        kind,
        vec![
            parts.scheme.clone(),
            parts.netloc.clone(),
            parts.path.clone(),
            parts.query.clone(),
            parts.fragment.clone(),
        ],
        &parts,
    ))
}

fn urllib_parse_urlunsplit(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("urlunsplit() requires 1 argument"));
    }
    let components = match &args[0].payload {
        PyObjectPayload::Tuple(items) => (**items).clone(),
        PyObjectPayload::List(items) => items.read().clone(),
        PyObjectPayload::Instance(_) => {
            let mut parts = Vec::new();
            for attr in &["scheme", "netloc", "path", "query", "fragment"] {
                parts.push(
                    args[0]
                        .get_attr(attr)
                        .unwrap_or_else(|| PyObject::str_val(CompactString::from(""))),
                );
            }
            parts
        }
        _ => return Err(PyException::type_error("urlunsplit requires a tuple/list")),
    };
    if components.len() < 5 {
        return Err(PyException::type_error("urlunsplit requires 5 components"));
    }
    reject_mixed_components(&components)?;
    let kind = component_sequence_kind(&components);
    let to_str = |obj: &PyObjectRef| -> String { component_text(obj) };
    let scheme = to_str(&components[0]);
    let netloc = to_str(&components[1]);
    let path = to_str(&components[2]);
    let query = to_str(&components[3]);
    let fragment = to_str(&components[4]);

    Ok(component_obj(
        kind,
        assemble_url(&scheme, &netloc, &path, "", &query, &fragment),
    ))
}

fn urllib_parse_urldefrag(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("urldefrag() requires 1 argument"));
    }
    let kind = object_kind(&args[0])?;
    let url = object_text(&args[0]);
    let (base, frag) = if let Some(idx) = url.find('#') {
        (&url[..idx], &url[idx + 1..])
    } else {
        (url.as_str(), "")
    };
    let cls = make_result_class(
        if kind == UrlKind::Bytes {
            "DefragResultBytes"
        } else {
            "DefragResult"
        },
        &["url", "fragment"],
    );
    let parts = UrlParts {
        scheme: String::new(),
        netloc: String::new(),
        path: String::new(),
        params: String::new(),
        query: String::new(),
        fragment: frag.to_string(),
        username: None,
        password: None,
        hostname: None,
        port_text: None,
    };
    Ok(make_url_result_instance(
        cls,
        kind,
        vec![base.to_string(), frag.to_string()],
        &parts,
    ))
}

fn urllib_parse_urljoin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("urljoin() requires 2 arguments"));
    }
    let kind = object_kind(&args[0])?;
    ensure_url_arg_kind(kind, &args[1])?;
    let base = object_text(&args[0]);
    let url = object_text(&args[1]);
    let (url_scheme, _) = split_scheme(&url);

    if !url_scheme.is_empty() && !url_scheme.eq_ignore_ascii_case(&split_scheme(&base).0) {
        return Ok(component_obj(kind, url));
    }

    let bp = split_parse_params(parse_url_parts(&base, "", true));

    let joined = if url.starts_with("//") {
        format!("{}:{}", bp.scheme, url)
    } else if !url_scheme.is_empty() && url_scheme != bp.scheme {
        url
    } else if !url_scheme.is_empty()
        && url_scheme == bp.scheme
        && !url.starts_with(&format!("{}://", bp.scheme))
    {
        let (_, rel_path) = split_scheme(&url);
        let mut p = bp.clone();
        let base_dir = join_base_dir(&bp);
        let rel_parts = parse_url_parts(rel_path, "", true);
        if rel_parts.path.is_empty() {
            p.path = bp.path.clone();
            p.params = bp.params.clone();
        } else {
            p.path = normalize_path(&format!("{}{}", base_dir, rel_parts.path));
            p.params.clear();
        }
        p.query = rel_parts.query;
        p.fragment = rel_parts.fragment;
        canonical_geturl_for_parts(&p, true)
    } else if url.starts_with('/') {
        let mut p = bp.clone();
        p.path = normalize_path(&url);
        p.params.clear();
        p.query.clear();
        p.fragment.clear();
        canonical_geturl_for_parts(&p, false)
    } else if url.is_empty() {
        base
    } else if url.starts_with('?') {
        let mut p = bp.clone();
        p.query = url[1..].to_string();
        p.fragment.clear();
        canonical_geturl_for_parts(&p, true)
    } else if url.starts_with('#') {
        let mut p = bp.clone();
        p.fragment = url[1..].to_string();
        canonical_geturl_for_parts(&p, true)
    } else {
        let base_dir = join_base_dir(&bp);
        let mut p = parse_url_parts(&url, &bp.scheme, true);
        if p.scheme == bp.scheme && p.netloc.is_empty() {
            p.netloc = bp.netloc.clone();
            p.path = normalize_path(&format!("{}{}", base_dir, p.path));
        }
        canonical_geturl_for_parts(&split_parse_params(p), true)
    };
    Ok(component_obj(kind, joined))
}

fn join_base_dir(parts: &UrlParts) -> &str {
    if let Some(idx) = parts.path.rfind('/') {
        &parts.path[..=idx]
    } else if !parts.scheme.is_empty() || !parts.netloc.is_empty() {
        "/"
    } else {
        ""
    }
}

fn normalize_path(path: &str) -> String {
    let trailing_slash = path.ends_with('/')
        || path.ends_with("/.")
        || path.ends_with("/..")
        || path == "."
        || path == "..";
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
    } else if trailing_slash && !result.ends_with('/') {
        format!("{}/", result)
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
    let (pos_args, kwargs) = split_kwargs(args);
    let kind = object_kind(&pos_args[0])?;
    let qs = object_text(&pos_args[0]);
    let keep_kw = kw_value(kwargs.as_ref(), "keep_blank_values");
    let keep_blank_values = pos_args
        .get(1)
        .or(keep_kw.as_ref())
        .map(|v| v.is_truthy())
        .unwrap_or(false);
    let separator = kw_value(kwargs.as_ref(), "separator")
        .map(|v| object_text(&v))
        .unwrap_or_else(|| "&".to_string());
    let encoding = kw_value(kwargs.as_ref(), "encoding")
        .map(|v| object_text(&v))
        .unwrap_or_else(|| "utf-8".to_string());
    let errors = kw_value(kwargs.as_ref(), "errors")
        .map(|v| object_text(&v))
        .unwrap_or_else(|| "replace".to_string());
    if let Some(limit) = kw_value(kwargs.as_ref(), "max_num_fields").and_then(|v| v.as_int()) {
        if qs.split(separator.as_str()).count() as i64 > limit {
            return Err(PyException::value_error("Max number of fields exceeded"));
        }
    }
    let mut result: FxHashKeyMap = new_fx_hashkey_map();

    if qs.is_empty() {
        return Ok(PyObject::dict(result));
    }

    for pair in qs.split(separator.as_str()) {
        if pair.is_empty() {
            continue;
        }
        let parts: Vec<&str> = pair.splitn(2, '=').collect();
        if parts.len() == 1 && !keep_blank_values {
            continue;
        }
        let key = decode_percent_component(&parts[0].replace('+', " "), &encoding, &errors);
        let val = if parts.len() > 1 {
            decode_percent_component(&parts[1].replace('+', " "), &encoding, &errors)
        } else {
            String::new()
        };
        if val.is_empty() && !keep_blank_values {
            continue;
        }
        let hk = match kind {
            UrlKind::Str => HashableKey::str_key(CompactString::from(key.as_str())),
            UrlKind::Bytes => HashableKey::Bytes(Box::new(key.as_bytes().to_vec())),
        };
        let entry = result
            .entry(hk.clone())
            .or_insert_with(|| PyObject::list(vec![]));
        if let PyObjectPayload::List(items) = &entry.payload {
            items.write().push(component_obj(kind, val.as_str()));
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
    let (pos_args, kwargs) = split_kwargs(args);
    let kind = object_kind(&pos_args[0])?;
    let qs = object_text(&pos_args[0]);
    let keep_kw = kw_value(kwargs.as_ref(), "keep_blank_values");
    let keep_blank_values = pos_args
        .get(1)
        .or(keep_kw.as_ref())
        .map(|v| v.is_truthy())
        .unwrap_or(false);
    let separator = kw_value(kwargs.as_ref(), "separator")
        .map(|v| object_text(&v))
        .unwrap_or_else(|| "&".to_string());
    let encoding = kw_value(kwargs.as_ref(), "encoding")
        .map(|v| object_text(&v))
        .unwrap_or_else(|| "utf-8".to_string());
    let errors = kw_value(kwargs.as_ref(), "errors")
        .map(|v| object_text(&v))
        .unwrap_or_else(|| "replace".to_string());
    let mut result = Vec::new();

    if qs.is_empty() {
        return Ok(PyObject::list(result));
    }

    for pair in qs.split(separator.as_str()) {
        if pair.is_empty() {
            continue;
        }
        let parts: Vec<&str> = pair.splitn(2, '=').collect();
        if parts.len() == 1 && !keep_blank_values {
            continue;
        }
        let key = decode_percent_component(&parts[0].replace('+', " "), &encoding, &errors);
        let val = if parts.len() > 1 {
            decode_percent_component(&parts[1].replace('+', " "), &encoding, &errors)
        } else {
            String::new()
        };
        if val.is_empty() && !keep_blank_values {
            continue;
        }
        result.push(PyObject::tuple(vec![
            component_obj(kind, key),
            component_obj(kind, val),
        ]));
    }

    Ok(PyObject::list(result))
}

fn split_kwargs(args: &[PyObjectRef]) -> (&[PyObjectRef], Option<PyObjectRef>) {
    if args.len() > 1
        && args
            .last()
            .is_some_and(|obj| matches!(&obj.payload, PyObjectPayload::Dict(_)))
    {
        (&args[..args.len() - 1], args.last().cloned())
    } else {
        (args, None)
    }
}

fn kw_value(kwargs: Option<&PyObjectRef>, name: &str) -> Option<PyObjectRef> {
    let PyObjectPayload::Dict(map) = &kwargs?.payload else {
        return None;
    };
    map.read()
        .get(&HashableKey::str_key(CompactString::from(name)))
        .cloned()
}

fn arg_kwargs(args: &[PyObjectRef]) -> Option<&PyObjectRef> {
    if args.len() <= 1 {
        return None;
    }
    args.last()
        .filter(|obj| matches!(&obj.payload, PyObjectPayload::Dict(_)))
}

fn none_or_str(value: Option<&str>) -> PyObjectRef {
    value.map(str_obj).unwrap_or_else(PyObject::none)
}

fn urllib_parse_splittype(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("splittype requires url"));
    }
    let url = args[0].py_to_string();
    if let Some(pos) = url.find(':') {
        let head = &url[..pos];
        if !head.is_empty() && !head.contains('/') {
            return Ok(PyObject::tuple(vec![
                str_obj(head.to_ascii_lowercase()),
                str_obj(&url[pos + 1..]),
            ]));
        }
    }
    Ok(PyObject::tuple(vec![PyObject::none(), str_obj(url)]))
}

fn urllib_parse_splithost(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("splithost requires url"));
    }
    let url = args[0].py_to_string();
    if let Some(rest) = url.strip_prefix("//") {
        let split_at = rest
            .find(|c| matches!(c, '/' | '#' | '?'))
            .unwrap_or(rest.len());
        let host = &rest[..split_at];
        let mut path = rest[split_at..].to_string();
        if !path.is_empty() && !path.starts_with('/') {
            path.insert(0, '/');
        }
        return Ok(PyObject::tuple(vec![str_obj(host), str_obj(path)]));
    }
    Ok(PyObject::tuple(vec![PyObject::none(), str_obj(url)]))
}

fn urllib_parse_splituser(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("splituser requires host"));
    }
    let host = args[0].py_to_string();
    if let Some(pos) = host.rfind('@') {
        Ok(PyObject::tuple(vec![
            str_obj(&host[..pos]),
            str_obj(&host[pos + 1..]),
        ]))
    } else {
        Ok(PyObject::tuple(vec![PyObject::none(), str_obj(host)]))
    }
}

fn urllib_parse_splitpasswd(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("splitpasswd requires user"));
    }
    let user = args[0].py_to_string();
    if let Some(pos) = user.find(':') {
        Ok(PyObject::tuple(vec![
            str_obj(&user[..pos]),
            str_obj(&user[pos + 1..]),
        ]))
    } else {
        Ok(PyObject::tuple(vec![str_obj(user), PyObject::none()]))
    }
}

fn split_port_parts(host: &str) -> (&str, Option<&str>) {
    if host.starts_with('[') {
        if let Some(end) = host.find(']') {
            if host[end + 1..].starts_with(':') {
                let port = &host[end + 2..];
                return (
                    &host[..=end],
                    if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() {
                        Some(port)
                    } else {
                        None
                    },
                );
            }
            return (host, None);
        }
    }
    if let Some(pos) = host.rfind(':') {
        let port = &host[pos + 1..];
        if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
            return (&host[..pos], Some(port));
        }
        if port.is_empty() {
            return (&host[..pos], None);
        }
    }
    (host, None)
}

fn urllib_parse_splitport(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("splitport requires host"));
    }
    let host = args[0].py_to_string();
    let (host_part, port) = split_port_parts(&host);
    Ok(PyObject::tuple(vec![str_obj(host_part), none_or_str(port)]))
}

fn urllib_parse_splitnport(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("splitnport requires host"));
    }
    let host = args[0].py_to_string();
    let defport = args.get(1).and_then(|v| v.as_int()).unwrap_or(-1);
    if let Some(pos) = host.rfind(':') {
        let port = &host[pos + 1..];
        if port.is_empty() {
            return Ok(PyObject::tuple(vec![
                str_obj(&host[..pos]),
                PyObject::int(defport),
            ]));
        }
        if port.chars().all(|c| c.is_ascii_digit()) {
            return Ok(PyObject::tuple(vec![
                str_obj(&host[..pos]),
                PyObject::int(port.parse::<i64>().unwrap_or(defport)),
            ]));
        }
        return Ok(PyObject::tuple(vec![
            str_obj(&host[..pos]),
            PyObject::none(),
        ]));
    }
    Ok(PyObject::tuple(vec![str_obj(host), PyObject::int(defport)]))
}

fn split_last(url: &str, marker: char) -> (String, PyObjectRef) {
    if let Some(pos) = url.rfind(marker) {
        (url[..pos].to_string(), str_obj(&url[pos + 1..]))
    } else {
        (url.to_string(), PyObject::none())
    }
}

fn urllib_parse_splitquery(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("splitquery requires url"));
    }
    let (left, right) = split_last(&args[0].py_to_string(), '?');
    Ok(PyObject::tuple(vec![str_obj(left), right]))
}

fn urllib_parse_splittag(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("splittag requires url"));
    }
    let (left, right) = split_last(&args[0].py_to_string(), '#');
    Ok(PyObject::tuple(vec![str_obj(left), right]))
}

fn urllib_parse_splitattr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("splitattr requires url"));
    }
    let url = args[0].py_to_string();
    let mut parts = url.split(';');
    let base = parts.next().unwrap_or("");
    Ok(PyObject::tuple(vec![
        str_obj(base),
        PyObject::list(parts.map(str_obj).collect()),
    ]))
}

fn urllib_parse_splitvalue(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("splitvalue requires attr"));
    }
    let attr = args[0].py_to_string();
    if let Some(pos) = attr.find('=') {
        Ok(PyObject::tuple(vec![
            str_obj(&attr[..pos]),
            str_obj(&attr[pos + 1..]),
        ]))
    } else {
        Ok(PyObject::tuple(vec![str_obj(attr), PyObject::none()]))
    }
}

fn urllib_parse_to_bytes(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("to_bytes requires url"));
    }
    let url = args[0].py_to_string();
    if !url.is_ascii() {
        return Err(PyException::new(
            ExceptionKind::UnicodeError,
            format!("URL {:?} contains non-ASCII characters", url),
        ));
    }
    Ok(str_obj(url))
}

fn urllib_parse_unwrap(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("unwrap requires url"));
    }
    let mut url = args[0].py_to_string().trim().to_string();
    if url.starts_with('<') && url.ends_with('>') {
        url = url[1..url.len() - 1].trim().to_string();
    }
    if let Some(rest) = url.strip_prefix("URL:") {
        url = rest.trim().to_string();
    }
    Ok(str_obj(url))
}

fn urllib_parse_quoter(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let safe = args.first().map(|v| v.py_to_string()).unwrap_or_default();
    let cls = PyObject::class(CompactString::from("Quoter"), vec![], IndexMap::new());
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("safe"), str_obj(safe));
    attrs.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("Quoter.__repr__", |_| Ok(str_obj("<Quoter {}>"))),
    );
    Ok(PyObject::instance_with_attrs(cls, attrs))
}
