use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

// ── http.cookiejar module ──

/// Helper: get the `_cookies` list from a CookieJar instance (self).
/// Falls back to an empty vec if not found.
fn cookiejar_get_cookies(self_obj: &PyObjectRef) -> Vec<PyObjectRef> {
    if let Some(cookies) = self_obj.get_attr("_cookies") {
        if let PyObjectPayload::List(ref items) = cookies.payload {
            return items.read().clone();
        }
    }
    vec![]
}

/// Helper: mutate the `_cookies` list on a CookieJar instance via a closure.
fn cookiejar_with_cookies_mut<F>(self_obj: &PyObjectRef, f: F)
where
    F: FnOnce(&mut Vec<PyObjectRef>),
{
    if let Some(cookies) = self_obj.get_attr("_cookies") {
        if let PyObjectPayload::List(ref items) = cookies.payload {
            f(&mut items.write());
        }
    }
}

/// Build a Cookie instance with the given attributes.
fn make_cookie_instance(cookie_cls: &PyObjectRef, attrs: Vec<(&str, PyObjectRef)>) -> PyObjectRef {
    let inst = PyObject::instance(cookie_cls.clone());
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        for (k, v) in attrs {
            w.insert(CompactString::from(k), v);
        }
    }
    inst
}

pub fn create_http_cookiejar_module() -> PyObjectRef {
    // ── Cookie class ──
    let mut cookie_ns = IndexMap::new();

    // Cookie.__init__(self, version, name, value, port, port_specified, domain,
    //     domain_specified, domain_initial_dot, path, path_specified, secure,
    //     expires, discard, comment, comment_url, rest, rfc2109=False)
    cookie_ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("Cookie.__init__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            let self_obj = &args[0];
            if let PyObjectPayload::Instance(ref d) = self_obj.payload {
                let mut w = d.attrs.write();
                let field_names = [
                    "version",
                    "name",
                    "value",
                    "port",
                    "port_specified",
                    "domain",
                    "domain_specified",
                    "domain_initial_dot",
                    "path",
                    "path_specified",
                    "secure",
                    "expires",
                    "discard",
                    "comment",
                    "comment_url",
                    "rest",
                    "rfc2109",
                ];
                // Handle kwargs dict as last positional arg
                let kwargs = args.last().and_then(|a| {
                    if let PyObjectPayload::Dict(ref map) = a.payload {
                        Some(map.read().clone())
                    } else {
                        None
                    }
                });
                let positional_end = if kwargs.is_some() {
                    args.len() - 1
                } else {
                    args.len()
                };
                for (i, name) in field_names.iter().enumerate() {
                    let idx = i + 1; // skip self
                    let val = if idx < positional_end {
                        args[idx].clone()
                    } else if let Some(ref kw) = kwargs {
                        kw.get(&HashableKey::str_key(CompactString::from(*name)))
                            .cloned()
                            .unwrap_or_else(|| {
                                if *name == "rfc2109" {
                                    PyObject::bool_val(false)
                                } else {
                                    PyObject::none()
                                }
                            })
                    } else if *name == "rfc2109" {
                        PyObject::bool_val(false)
                    } else {
                        PyObject::none()
                    };
                    w.insert(CompactString::from(*name), val);
                }
            }
            Ok(PyObject::none())
        }),
    );

    cookie_ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("Cookie.__repr__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::str_val(CompactString::from("Cookie()")));
            }
            let name = args[0]
                .get_attr("name")
                .map(|v| v.py_to_string())
                .unwrap_or_default();
            let value = args[0]
                .get_attr("value")
                .map(|v| v.py_to_string())
                .unwrap_or_default();
            Ok(PyObject::str_val(CompactString::from(format!(
                "<Cookie {}={}>",
                name, value
            ))))
        }),
    );

    let cookie_cls = PyObject::class(CompactString::from("Cookie"), vec![], cookie_ns);

    // ── CookieJar class (proper class, inheritable) ──
    let mut ns = IndexMap::new();

    // __init__: set up _cookies storage on the instance
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("CookieJar.__init__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            let self_obj = &args[0];
            if let PyObjectPayload::Instance(ref d) = self_obj.payload {
                let mut w = d.attrs.write();
                if !w.contains_key("_cookies") {
                    w.insert(CompactString::from("_cookies"), PyObject::list(vec![]));
                }
            }
            Ok(PyObject::none())
        }),
    );

    // __iter__: yield cookie objects
    ns.insert(
        CompactString::from("__iter__"),
        PyObject::native_closure("CookieJar.__iter__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::list(vec![]));
            }
            Ok(PyObject::list(cookiejar_get_cookies(&args[0])))
        }),
    );

    // __len__
    ns.insert(
        CompactString::from("__len__"),
        PyObject::native_closure("CookieJar.__len__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::int(0));
            }
            Ok(PyObject::int(cookiejar_get_cookies(&args[0]).len() as i64))
        }),
    );

    // __bool__
    ns.insert(
        CompactString::from("__bool__"),
        PyObject::native_closure("CookieJar.__bool__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::bool_val(false));
            }
            Ok(PyObject::bool_val(
                !cookiejar_get_cookies(&args[0]).is_empty(),
            ))
        }),
    );

    // __contains__(self, name)
    ns.insert(
        CompactString::from("__contains__"),
        PyObject::native_closure("CookieJar.__contains__", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let name = args[1].py_to_string();
            let found = cookiejar_get_cookies(&args[0]).iter().any(|c| {
                c.get_attr("name")
                    .map(|n| n.py_to_string() == name)
                    .unwrap_or(false)
            });
            Ok(PyObject::bool_val(found))
        }),
    );

    // __setitem__(self, name, value): set a cookie by name
    ns.insert(
        CompactString::from("__setitem__"),
        PyObject::native_closure("CookieJar.__setitem__", |args: &[PyObjectRef]| {
            if args.len() < 3 {
                return Err(PyException::type_error(
                    "__setitem__ requires name and value",
                ));
            }
            let self_obj = &args[0];
            let name_str = args[1].py_to_string();
            let value = args[2].clone();
            // Remove existing cookie with this name
            cookiejar_with_cookies_mut(self_obj, |cookies| {
                cookies.retain(|c| {
                    c.get_attr("name")
                        .map(|n| n.py_to_string() != name_str)
                        .unwrap_or(true)
                });
            });
            // Create a minimal Cookie and add it
            let cookie_cls =
                PyObject::class(CompactString::from("Cookie"), vec![], IndexMap::new());
            let cookie = make_cookie_instance(
                &cookie_cls,
                vec![
                    (
                        "name",
                        PyObject::str_val(CompactString::from(name_str.as_str())),
                    ),
                    ("value", value),
                    ("domain", PyObject::str_val(CompactString::from(""))),
                    ("path", PyObject::str_val(CompactString::from("/"))),
                ],
            );
            cookiejar_with_cookies_mut(self_obj, |cookies| {
                cookies.push(cookie);
            });
            Ok(PyObject::none())
        }),
    );

    // __getitem__(self, name): get cookie value by name
    ns.insert(
        CompactString::from("__getitem__"),
        PyObject::native_closure("CookieJar.__getitem__", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("__getitem__ requires a key"));
            }
            let name = args[1].py_to_string();
            for c in cookiejar_get_cookies(&args[0]) {
                if c.get_attr("name")
                    .map(|n| n.py_to_string() == name)
                    .unwrap_or(false)
                {
                    return Ok(c.get_attr("value").unwrap_or_else(PyObject::none));
                }
            }
            Err(PyException::key_error(&name))
        }),
    );

    // __delitem__(self, name)
    ns.insert(
        CompactString::from("__delitem__"),
        PyObject::native_closure("CookieJar.__delitem__", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("__delitem__ requires a key"));
            }
            let name = args[1].py_to_string();
            cookiejar_with_cookies_mut(&args[0], |cookies| {
                cookies.retain(|c| {
                    c.get_attr("name")
                        .map(|n| n.py_to_string() != name)
                        .unwrap_or(true)
                });
            });
            Ok(PyObject::none())
        }),
    );

    // set_cookie(self, cookie, *args, **kwargs)
    ns.insert(
        CompactString::from("set_cookie"),
        PyObject::native_closure("CookieJar.set_cookie", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::none());
            }
            let self_obj = &args[0];
            let cookie = args[1].clone();
            cookiejar_with_cookies_mut(self_obj, |cookies| {
                cookies.push(cookie);
            });
            Ok(PyObject::none())
        }),
    );

    // clear(self)
    ns.insert(
        CompactString::from("clear"),
        PyObject::native_closure("CookieJar.clear", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            cookiejar_with_cookies_mut(&args[0], |cookies| cookies.clear());
            Ok(PyObject::none())
        }),
    );

    // copy(self): return a new CookieJar with the same cookies
    ns.insert(
        CompactString::from("copy"),
        PyObject::native_closure("CookieJar.copy", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            let cookies = cookiejar_get_cookies(&args[0]);
            // Get the class of self for proper subclass support
            let cls = if let PyObjectPayload::Instance(ref d) = args[0].payload {
                d.class.clone()
            } else {
                PyObject::class(CompactString::from("CookieJar"), vec![], IndexMap::new())
            };
            let new_inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = new_inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("_cookies"), PyObject::list(cookies));
            }
            Ok(new_inst)
        }),
    );

    // keys(self): list of cookie names
    ns.insert(
        CompactString::from("keys"),
        PyObject::native_closure("CookieJar.keys", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::list(vec![]));
            }
            let names: Vec<PyObjectRef> = cookiejar_get_cookies(&args[0])
                .iter()
                .map(|c| c.get_attr("name").unwrap_or_else(PyObject::none))
                .collect();
            Ok(PyObject::list(names))
        }),
    );

    // values(self): list of cookie values
    ns.insert(
        CompactString::from("values"),
        PyObject::native_closure("CookieJar.values", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::list(vec![]));
            }
            let values: Vec<PyObjectRef> = cookiejar_get_cookies(&args[0])
                .iter()
                .map(|c| c.get_attr("value").unwrap_or_else(PyObject::none))
                .collect();
            Ok(PyObject::list(values))
        }),
    );

    // items(self): list of (name, value) tuples
    ns.insert(
        CompactString::from("items"),
        PyObject::native_closure("CookieJar.items", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::list(vec![]));
            }
            let items: Vec<PyObjectRef> = cookiejar_get_cookies(&args[0])
                .iter()
                .map(|c| {
                    PyObject::tuple(vec![
                        c.get_attr("name").unwrap_or_else(PyObject::none),
                        c.get_attr("value").unwrap_or_else(PyObject::none),
                    ])
                })
                .collect();
            Ok(PyObject::list(items))
        }),
    );

    // get(self, name, default=None, domain=None, path=None)
    ns.insert(
        CompactString::from("get"),
        PyObject::native_closure("CookieJar.get", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::none());
            }
            let name = args[1].py_to_string();
            let default = if args.len() > 2 {
                args[2].clone()
            } else {
                PyObject::none()
            };
            for c in cookiejar_get_cookies(&args[0]) {
                if c.get_attr("name")
                    .map(|n| n.py_to_string() == name)
                    .unwrap_or(false)
                {
                    return Ok(c.get_attr("value").unwrap_or_else(PyObject::none));
                }
            }
            Ok(default)
        }),
    );

    // set(self, name, value, **kwargs)
    let cookie_cls_for_set = cookie_cls.clone();
    ns.insert(
        CompactString::from("set"),
        PyObject::native_closure("CookieJar.set", move |args: &[PyObjectRef]| {
            if args.len() < 3 {
                return Ok(PyObject::none());
            }
            let self_obj = &args[0];
            let name_str = args[1].py_to_string();
            let value = args[2].clone();
            let domain = if args.len() > 3 {
                args[3].py_to_string()
            } else {
                String::new()
            };
            let path = if args.len() > 4 {
                args[4].py_to_string()
            } else {
                String::from("/")
            };
            // Remove existing
            cookiejar_with_cookies_mut(self_obj, |cookies| {
                cookies.retain(|c| {
                    c.get_attr("name")
                        .map(|n| n.py_to_string() != name_str)
                        .unwrap_or(true)
                });
            });
            let cookie = make_cookie_instance(
                &cookie_cls_for_set,
                vec![
                    (
                        "name",
                        PyObject::str_val(CompactString::from(name_str.as_str())),
                    ),
                    ("value", value),
                    (
                        "domain",
                        PyObject::str_val(CompactString::from(domain.as_str())),
                    ),
                    (
                        "path",
                        PyObject::str_val(CompactString::from(path.as_str())),
                    ),
                ],
            );
            cookiejar_with_cookies_mut(self_obj, |cookies| {
                cookies.push(cookie);
            });
            Ok(PyObject::none())
        }),
    );

    // update(self, other): merge cookies from another jar or dict
    ns.insert(
        CompactString::from("update"),
        PyObject::native_closure("CookieJar.update", |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::none());
            }
            let self_obj = &args[0];
            let other = &args[1];
            // If other has _cookies attr, it's a jar-like object
            let other_cookies = cookiejar_get_cookies(other);
            if !other_cookies.is_empty() {
                cookiejar_with_cookies_mut(self_obj, |cookies| {
                    for c in other_cookies {
                        cookies.push(c);
                    }
                });
            }
            Ok(PyObject::none())
        }),
    );

    // _cookies_for_request(self, request) — stub for compatibility
    ns.insert(
        CompactString::from("_cookies_for_request"),
        PyObject::native_closure(
            "CookieJar._cookies_for_request",
            |_args: &[PyObjectRef]| Ok(PyObject::list(vec![])),
        ),
    );

    // extract_cookies(self, response, request) — stub for compatibility
    ns.insert(
        CompactString::from("extract_cookies"),
        PyObject::native_closure("CookieJar.extract_cookies", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );

    // add_cookie_header(self, request) — stub for compatibility
    ns.insert(
        CompactString::from("add_cookie_header"),
        PyObject::native_closure("CookieJar.add_cookie_header", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );

    // make_cookies(self, response, request) — stub
    ns.insert(
        CompactString::from("make_cookies"),
        PyObject::native_closure("CookieJar.make_cookies", |_args: &[PyObjectRef]| {
            Ok(PyObject::list(vec![]))
        }),
    );

    // set_policy(self, policy) — stub
    ns.insert(
        CompactString::from("set_policy"),
        PyObject::native_closure("CookieJar.set_policy", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );

    // __repr__
    ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("CookieJar.__repr__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::str_val(CompactString::from("<CookieJar[]>")));
            }
            let cookies = cookiejar_get_cookies(&args[0]);
            let inner: Vec<String> = cookies
                .iter()
                .map(|c| {
                    let n = c
                        .get_attr("name")
                        .map(|v| v.py_to_string())
                        .unwrap_or_default();
                    let v = c
                        .get_attr("value")
                        .map(|v| v.py_to_string())
                        .unwrap_or_default();
                    format!("<Cookie {}={}>", n, v)
                })
                .collect();
            Ok(PyObject::str_val(CompactString::from(format!(
                "<CookieJar[{}]>",
                inner.join(", ")
            ))))
        }),
    );

    let cookiejar_cls = PyObject::class(CompactString::from("CookieJar"), vec![], ns);

    // FileCookieJar: subclass of CookieJar
    let mut file_ns = IndexMap::new();
    file_ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("FileCookieJar.__init__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            if let PyObjectPayload::Instance(ref d) = args[0].payload {
                let mut w = d.attrs.write();
                if !w.contains_key("_cookies") {
                    w.insert(CompactString::from("_cookies"), PyObject::list(vec![]));
                }
                let filename = if args.len() > 1 {
                    args[1].clone()
                } else {
                    PyObject::none()
                };
                w.insert(CompactString::from("filename"), filename);
            }
            Ok(PyObject::none())
        }),
    );
    file_ns.insert(
        CompactString::from("save"),
        PyObject::native_closure("FileCookieJar.save", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );
    file_ns.insert(
        CompactString::from("load"),
        PyObject::native_closure("FileCookieJar.load", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );
    file_ns.insert(
        CompactString::from("revert"),
        PyObject::native_closure("FileCookieJar.revert", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );
    let file_cookiejar_cls = PyObject::class(
        CompactString::from("FileCookieJar"),
        vec![cookiejar_cls.clone()],
        file_ns,
    );

    // MozillaCookieJar: subclass of FileCookieJar
    let mozilla_cookiejar_cls = PyObject::class(
        CompactString::from("MozillaCookieJar"),
        vec![file_cookiejar_cls.clone()],
        IndexMap::new(),
    );

    // DefaultCookiePolicy: stub
    let mut policy_ns = IndexMap::new();
    policy_ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("DefaultCookiePolicy.__init__", |_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }),
    );
    let policy_cls = PyObject::class(
        CompactString::from("DefaultCookiePolicy"),
        vec![],
        policy_ns,
    );

    make_module(
        "http.cookiejar",
        vec![
            ("CookieJar", cookiejar_cls),
            ("FileCookieJar", file_cookiejar_cls),
            ("MozillaCookieJar", mozilla_cookiejar_cls),
            ("Cookie", cookie_cls),
            ("DefaultCookiePolicy", policy_cls),
        ],
    )
}
