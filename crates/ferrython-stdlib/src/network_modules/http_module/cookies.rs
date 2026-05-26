use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;
use std::sync::{Arc, Mutex};

// ── http.cookies module ──

/// Create a Morsel instance with full __setitem__/__getitem__ for cookie attributes
fn make_full_morsel(key: PyObjectRef, value: PyObjectRef, coded_value: PyObjectRef) -> PyObjectRef {
    let cls = PyObject::class(CompactString::from("Morsel"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("key"), key);
        w.insert(CompactString::from("value"), value);
        w.insert(CompactString::from("coded_value"), coded_value);

        let attrs: Arc<Mutex<IndexMap<CompactString, PyObjectRef>>> = Arc::new(Mutex::new({
            let mut m = IndexMap::new();
            for k in &[
                "expires", "path", "comment", "domain", "max-age", "secure", "httponly", "version",
                "samesite",
            ] {
                m.insert(
                    CompactString::from(*k),
                    PyObject::str_val(CompactString::from("")),
                );
            }
            m
        }));

        let a = attrs.clone();
        w.insert(
            CompactString::from("__setitem__"),
            PyObject::native_closure("Morsel.__setitem__", move |args: &[PyObjectRef]| {
                if args.len() >= 2 {
                    let key = CompactString::from(args[0].py_to_string().to_lowercase());
                    a.lock().unwrap().insert(key, args[1].clone());
                }
                Ok(PyObject::none())
            }),
        );
        let a2 = attrs.clone();
        w.insert(
            CompactString::from("__getitem__"),
            PyObject::native_closure("Morsel.__getitem__", move |args: &[PyObjectRef]| {
                if let Some(key) = args.first() {
                    let k = CompactString::from(key.py_to_string().to_lowercase());
                    if let Some(val) = a2.lock().unwrap().get(&k) {
                        return Ok(val.clone());
                    }
                }
                Ok(PyObject::str_val(CompactString::from("")))
            }),
        );
    }
    inst
}

pub fn create_http_cookies_module() -> PyObjectRef {
    // Morsel class — represents a single cookie key/value with attributes
    let morsel_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("Morsel"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(
                CompactString::from("key"),
                PyObject::str_val(CompactString::from("")),
            );
            w.insert(
                CompactString::from("value"),
                PyObject::str_val(CompactString::from("")),
            );
            w.insert(
                CompactString::from("coded_value"),
                PyObject::str_val(CompactString::from("")),
            );

            let attrs: Arc<Mutex<IndexMap<CompactString, PyObjectRef>>> = Arc::new(Mutex::new({
                let mut m = IndexMap::new();
                for key in &[
                    "expires", "path", "comment", "domain", "max-age", "secure", "httponly",
                    "version", "samesite",
                ] {
                    m.insert(
                        CompactString::from(*key),
                        PyObject::str_val(CompactString::from("")),
                    );
                }
                m
            }));

            let a = attrs.clone();
            w.insert(
                CompactString::from("__setitem__"),
                PyObject::native_closure("Morsel.__setitem__", move |args: &[PyObjectRef]| {
                    if args.len() >= 2 {
                        let key = CompactString::from(args[0].py_to_string().to_lowercase());
                        a.lock().unwrap().insert(key, args[1].clone());
                    }
                    Ok(PyObject::none())
                }),
            );
            let a2 = attrs.clone();
            w.insert(
                CompactString::from("__getitem__"),
                PyObject::native_closure("Morsel.__getitem__", move |args: &[PyObjectRef]| {
                    if let Some(key) = args.first() {
                        let k = CompactString::from(key.py_to_string().to_lowercase());
                        if let Some(val) = a2.lock().unwrap().get(&k) {
                            return Ok(val.clone());
                        }
                    }
                    Ok(PyObject::str_val(CompactString::from("")))
                }),
            );
            let inst2 = inst.clone();
            w.insert(
                CompactString::from("set"),
                PyObject::native_closure("Morsel.set", move |args: &[PyObjectRef]| {
                    if args.len() >= 3 {
                        if let PyObjectPayload::Instance(ref d) = inst2.payload {
                            let mut w = d.attrs.write();
                            w.insert(CompactString::from("key"), args[0].clone());
                            w.insert(CompactString::from("value"), args[1].clone());
                            w.insert(CompactString::from("coded_value"), args[2].clone());
                        }
                    }
                    Ok(PyObject::none())
                }),
            );
            let inst3 = inst.clone();
            let a3 = attrs.clone();
            w.insert(
                CompactString::from("OutputString"),
                PyObject::native_closure("Morsel.OutputString", move |_args: &[PyObjectRef]| {
                    let key = if let PyObjectPayload::Instance(ref d) = inst3.payload {
                        d.attrs
                            .read()
                            .get(&CompactString::from("key"))
                            .map(|k| k.py_to_string())
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    let coded = if let PyObjectPayload::Instance(ref d) = inst3.payload {
                        d.attrs
                            .read()
                            .get(&CompactString::from("coded_value"))
                            .map(|v| v.py_to_string())
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    let mut parts = vec![format!("{}={}", key, coded)];
                    let attrs = a3.lock().unwrap();
                    for (k, v) in attrs.iter() {
                        let vs = v.py_to_string();
                        if !vs.is_empty() {
                            parts.push(format!("{}={}", k, vs));
                        }
                    }
                    Ok(PyObject::str_val(CompactString::from(parts.join("; "))))
                }),
            );
        }
        Ok(inst)
    });

    // SimpleCookie class — dict-like cookie container
    let simple_cookie_fn = make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("SimpleCookie"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            let cookies: Arc<Mutex<IndexMap<CompactString, PyObjectRef>>> =
                Arc::new(Mutex::new(IndexMap::new()));

            let c = cookies.clone();
            w.insert(
                CompactString::from("__setitem__"),
                PyObject::native_closure(
                    "SimpleCookie.__setitem__",
                    move |args: &[PyObjectRef]| {
                        if args.len() >= 2 {
                            let key = CompactString::from(args[0].py_to_string());
                            // Create a full Morsel with __setitem__/__getitem__ for cookie attrs
                            let morsel =
                                make_full_morsel(args[0].clone(), args[1].clone(), args[1].clone());
                            c.lock().unwrap().insert(key, morsel);
                        }
                        Ok(PyObject::none())
                    },
                ),
            );
            let c2 = cookies.clone();
            w.insert(
                CompactString::from("__getitem__"),
                PyObject::native_closure(
                    "SimpleCookie.__getitem__",
                    move |args: &[PyObjectRef]| {
                        if let Some(key) = args.first() {
                            let k = CompactString::from(key.py_to_string());
                            if let Some(val) = c2.lock().unwrap().get(&k) {
                                return Ok(val.clone());
                            }
                        }
                        Err(PyException::key_error("cookie not found"))
                    },
                ),
            );
            let c3 = cookies.clone();
            w.insert(
                CompactString::from("output"),
                PyObject::native_closure("SimpleCookie.output", move |_args: &[PyObjectRef]| {
                    let cs = c3.lock().unwrap();
                    let mut lines = Vec::new();
                    for (k, _morsel) in cs.iter() {
                        lines.push(format!("Set-Cookie: {}", k));
                    }
                    Ok(PyObject::str_val(CompactString::from(lines.join("\r\n"))))
                }),
            );
            let c4 = cookies.clone();
            w.insert(
                CompactString::from("load"),
                PyObject::native_closure("SimpleCookie.load", move |args: &[PyObjectRef]| {
                    if let Some(raw) = args.first() {
                        let raw_str = raw.py_to_string();
                        // Parse "key=value; key2=value2" format
                        for pair in raw_str.split(';') {
                            let pair = pair.trim();
                            if let Some(eq) = pair.find('=') {
                                let key = CompactString::from(pair[..eq].trim());
                                let value = pair[eq + 1..].trim().to_string();
                                let morsel = make_full_morsel(
                                    PyObject::str_val(key.clone()),
                                    PyObject::str_val(CompactString::from(&value)),
                                    PyObject::str_val(CompactString::from(&value)),
                                );
                                c4.lock().unwrap().insert(key, morsel);
                            }
                        }
                    }
                    Ok(PyObject::none())
                }),
            );
            let c5 = cookies.clone();
            w.insert(
                CompactString::from("keys"),
                PyObject::native_closure("SimpleCookie.keys", move |_args: &[PyObjectRef]| {
                    let cs = c5.lock().unwrap();
                    let keys: Vec<PyObjectRef> =
                        cs.keys().map(|k| PyObject::str_val(k.clone())).collect();
                    Ok(PyObject::list(keys))
                }),
            );
            let c6 = cookies.clone();
            w.insert(
                CompactString::from("values"),
                PyObject::native_closure("SimpleCookie.values", move |_args: &[PyObjectRef]| {
                    let cs = c6.lock().unwrap();
                    let vals: Vec<PyObjectRef> = cs.values().cloned().collect();
                    Ok(PyObject::list(vals))
                }),
            );
            let c7 = cookies.clone();
            w.insert(
                CompactString::from("items"),
                PyObject::native_closure("SimpleCookie.items", move |_args: &[PyObjectRef]| {
                    let cs = c7.lock().unwrap();
                    let items: Vec<PyObjectRef> = cs
                        .iter()
                        .map(|(k, v)| {
                            PyObject::tuple(vec![PyObject::str_val(k.clone()), v.clone()])
                        })
                        .collect();
                    Ok(PyObject::list(items))
                }),
            );
        }
        Ok(inst)
    });

    // CookieError exception
    let cookie_error = PyObject::class(CompactString::from("CookieError"), vec![], IndexMap::new());

    make_module(
        "http.cookies",
        vec![
            ("Morsel", morsel_fn),
            ("SimpleCookie", simple_cookie_fn.clone()),
            ("BaseCookie", simple_cookie_fn),
            ("CookieError", cookie_error),
        ],
    )
}
