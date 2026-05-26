use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

// ── cgi module ──

pub fn create_cgi_module() -> PyObjectRef {
    make_module(
        "cgi",
        vec![
            (
                "parse_header",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error("parse_header requires a string"));
                    }
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
                                    HashableKey::str_key(CompactString::from(&k)),
                                    PyObject::str_val(CompactString::from(v)),
                                );
                            }
                        }
                    }
                    Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from(main_type)),
                        PyObject::dict(params),
                    ]))
                }),
            ),
            (
                "escape",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error("escape requires a string"));
                    }
                    let s = args[0].py_to_string();
                    let escaped = s
                        .replace('&', "&amp;")
                        .replace('<', "&lt;")
                        .replace('>', "&gt;")
                        .replace('"', "&quot;");
                    Ok(PyObject::str_val(CompactString::from(escaped)))
                }),
            ),
            (
                "FieldStorage",
                make_builtin(|_| Err(PyException::not_implemented_error("cgi.FieldStorage"))),
            ),
            (
                "parse_qs",
                make_builtin(|_| {
                    Err(PyException::not_implemented_error(
                        "cgi.parse_qs (use urllib.parse.parse_qs)",
                    ))
                }),
            ),
        ],
    )
}
