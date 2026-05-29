use super::*;

pub(super) fn create_environ_object() -> PyObjectRef {
    let initial_pairs: Vec<(PyObjectRef, PyObjectRef)> = std::env::vars()
        .map(|(k, v)| {
            (
                PyObject::str_val(CompactString::from(k)),
                PyObject::str_val(CompactString::from(v)),
            )
        })
        .collect();
    let data = PyObject::dict_from_pairs(initial_pairs);
    let data_ref = data.clone();
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("_data"), data.clone());

    attrs.insert(
        CompactString::from("__getitem__"),
        PyObject::native_closure("__getitem__", move |args| {
            let key_str = args
                .last()
                .ok_or_else(|| PyException::key_error("key required"))?
                .py_to_string();
            match std::env::var(&key_str) {
                Ok(val) => Ok(PyObject::str_val(CompactString::from(val))),
                Err(_) => Err(PyException::key_error(format!("'{}'", key_str))),
            }
        }),
    );
    let d2 = data_ref.clone();
    attrs.insert(
        CompactString::from("__setitem__"),
        PyObject::native_closure("__setitem__", move |args| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "__setitem__ requires key and value",
                ));
            }
            let val_str = args[args.len() - 1].py_to_string();
            let key_str = args[args.len() - 2].py_to_string();
            unsafe {
                std::env::set_var(&key_str, &val_str);
            }
            if let PyObjectPayload::Dict(dd) = &d2.payload {
                dd.write().insert(
                    HashableKey::str_key(CompactString::from(&key_str)),
                    PyObject::str_val(CompactString::from(&val_str)),
                );
            }
            Ok(PyObject::none())
        }),
    );
    let d3 = data_ref.clone();
    attrs.insert(
        CompactString::from("__delitem__"),
        PyObject::native_closure("__delitem__", move |args| {
            let key_str = args
                .last()
                .ok_or_else(|| PyException::key_error("key required"))?
                .py_to_string();
            unsafe {
                std::env::remove_var(&key_str);
            }
            if let PyObjectPayload::Dict(dd) = &d3.payload {
                dd.write()
                    .swap_remove(&HashableKey::str_key(CompactString::from(&key_str)));
            }
            Ok(PyObject::none())
        }),
    );
    attrs.insert(
        CompactString::from("__contains__"),
        PyObject::native_closure("__contains__", move |args| {
            let key_str = args.last().map(|a| a.py_to_string()).unwrap_or_default();
            Ok(PyObject::bool_val(std::env::var(&key_str).is_ok()))
        }),
    );
    attrs.insert(
        CompactString::from("get"),
        PyObject::native_closure("get", move |args| {
            let real_args =
                if args.len() > 1 && matches!(&args[0].payload, PyObjectPayload::Module(_)) {
                    &args[1..]
                } else {
                    args
                };
            if real_args.is_empty() {
                return Ok(PyObject::none());
            }
            let key_str = real_args[0].py_to_string();
            match std::env::var(&key_str) {
                Ok(val) => Ok(PyObject::str_val(CompactString::from(val))),
                Err(_) => Ok(real_args.get(1).cloned().unwrap_or_else(PyObject::none)),
            }
        }),
    );
    let d4 = data_ref.clone();
    attrs.insert(
        CompactString::from("pop"),
        PyObject::native_closure("pop", move |args| {
            let real_args =
                if args.len() > 1 && matches!(&args[0].payload, PyObjectPayload::Module(_)) {
                    &args[1..]
                } else {
                    args
                };
            if real_args.is_empty() {
                return Err(PyException::type_error("pop expected at least 1 argument"));
            }
            if real_args.len() > 2 {
                return Err(PyException::type_error("pop expected at most 2 arguments"));
            }

            let key_str = real_args[0].py_to_string();
            match std::env::var(&key_str) {
                Ok(val) => {
                    unsafe {
                        std::env::remove_var(&key_str);
                    }
                    if let PyObjectPayload::Dict(dd) = &d4.payload {
                        dd.write()
                            .swap_remove(&HashableKey::str_key(CompactString::from(&key_str)));
                    }
                    Ok(PyObject::str_val(CompactString::from(val)))
                }
                Err(_) => real_args
                    .get(1)
                    .cloned()
                    .ok_or_else(|| PyException::key_error(format!("'{}'", key_str))),
            }
        }),
    );
    attrs.insert(
        CompactString::from("keys"),
        PyObject::native_closure("keys", move |_| {
            let keys: Vec<PyObjectRef> = std::env::vars()
                .map(|(k, _)| PyObject::str_val(CompactString::from(k)))
                .collect();
            Ok(PyObject::list(keys))
        }),
    );
    attrs.insert(
        CompactString::from("values"),
        PyObject::native_closure("values", move |_| {
            let vals: Vec<PyObjectRef> = std::env::vars()
                .map(|(_, v)| PyObject::str_val(CompactString::from(v)))
                .collect();
            Ok(PyObject::list(vals))
        }),
    );
    attrs.insert(
        CompactString::from("items"),
        PyObject::native_closure("items", move |_| {
            let items: Vec<PyObjectRef> = std::env::vars()
                .map(|(k, v)| {
                    PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from(k)),
                        PyObject::str_val(CompactString::from(v)),
                    ])
                })
                .collect();
            Ok(PyObject::list(items))
        }),
    );
    attrs.insert(
        CompactString::from("copy"),
        PyObject::native_closure("copy", move |_| {
            let pairs: Vec<(PyObjectRef, PyObjectRef)> = std::env::vars()
                .map(|(k, v)| {
                    (
                        PyObject::str_val(CompactString::from(k)),
                        PyObject::str_val(CompactString::from(v)),
                    )
                })
                .collect();
            Ok(PyObject::dict_from_pairs(pairs))
        }),
    );
    attrs.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("__repr__", move |_| {
            Ok(PyObject::str_val(CompactString::from("environ({...})")))
        }),
    );
    PyObject::module_with_attrs(CompactString::from("_Environ"), attrs)
}
