use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

fn current_thread_key() -> CompactString {
    CompactString::from(format!("{:?}", std::thread::current().id()))
}

// ── contextvars module ──

pub fn create_contextvars_module() -> PyObjectRef {
    // Create a shared ContextVar class so isinstance() works
    let context_var_class =
        PyObject::class(CompactString::from("ContextVar"), vec![], IndexMap::new());
    let cv_cls = context_var_class.clone();

    // __new__ receives (cls, name, ...) — create a properly-typed instance
    let cv_new = PyObject::native_closure("ContextVar.__new__", move |args: &[PyObjectRef]| {
        // args[0] = cls, args[1..] = user args
        let user_args = if args.len() > 1 { &args[1..] } else { &[] };
        if user_args.is_empty() {
            return Err(PyException::type_error("ContextVar() requires a name"));
        }
        let name = user_args[0].py_to_string();
        let default_val = if user_args.len() > 1 {
            if let PyObjectPayload::Dict(kw) = &user_args[user_args.len() - 1].payload {
                kw.read()
                    .get(&HashableKey::str_key(CompactString::from("default")))
                    .cloned()
            } else {
                Some(user_args[1].clone())
            }
        } else {
            None
        };

        let inst = PyObject::instance(cv_cls.clone());
        if let PyObjectPayload::Instance(ref data) = inst.payload {
            let mut attrs = data.attrs.write();
            attrs.insert(
                CompactString::from("name"),
                PyObject::str_val(CompactString::from(&name)),
            );
            let values: Rc<PyCell<IndexMap<CompactString, PyObjectRef>>> =
                Rc::new(PyCell::new(IndexMap::new()));

            let v = values.clone();
            let default_for_get = default_val.clone();
            attrs.insert(
                CompactString::from("get"),
                PyObject::native_closure("ContextVar.get", move |a: &[PyObjectRef]| {
                    let thread_key = current_thread_key();
                    if let Some(val) = v.read().get(&thread_key) {
                        Ok(val.clone())
                    } else if !a.is_empty() {
                        Ok(a[0].clone())
                    } else if let Some(default) = default_for_get.as_ref() {
                        Ok(default.clone())
                    } else {
                        Err(PyException::lookup_error("ContextVar has no value"))
                    }
                }),
            );

            let v = values.clone();
            let name_clone = name.clone();
            attrs.insert(
                CompactString::from("set"),
                PyObject::native_closure("ContextVar.set", move |a: &[PyObjectRef]| {
                    if a.is_empty() {
                        return Err(PyException::type_error("set() requires a value"));
                    }
                    let thread_key = current_thread_key();
                    let old = v.read().get(&thread_key).cloned();
                    v.write().insert(thread_key.clone(), a[0].clone());
                    let v_restore = v.clone();
                    let restore_thread_key = thread_key.clone();
                    let token_cls =
                        PyObject::class(CompactString::from("Token"), vec![], IndexMap::new());
                    let token = PyObject::instance(token_cls);
                    if let PyObjectPayload::Instance(ref td) = token.payload {
                        let mut ta = td.attrs.write();
                        ta.insert(
                            CompactString::from("old_value"),
                            old.clone().unwrap_or_else(PyObject::none),
                        );
                        ta.insert(
                            CompactString::from("_had_value"),
                            PyObject::bool_val(old.is_some()),
                        );
                        ta.insert(
                            CompactString::from("_thread_key"),
                            PyObject::str_val(thread_key.clone()),
                        );
                        ta.insert(
                            CompactString::from("var"),
                            PyObject::str_val(CompactString::from(name_clone.as_str())),
                        );
                        let old_clone = old;
                        ta.insert(
                            CompactString::from("_restore"),
                            PyObject::native_closure("Token._restore", move |_| {
                                if let Some(old) = old_clone.clone() {
                                    v_restore.write().insert(restore_thread_key.clone(), old);
                                } else {
                                    v_restore.write().shift_remove(&restore_thread_key);
                                }
                                Ok(PyObject::none())
                            }),
                        );
                    }
                    Ok(token)
                }),
            );

            let v = values.clone();
            attrs.insert(
                CompactString::from("reset"),
                PyObject::native_closure("ContextVar.reset", move |a: &[PyObjectRef]| {
                    if a.is_empty() {
                        return Err(PyException::type_error("reset() requires a token"));
                    }
                    let token = &a[0];
                    if let Some(restore_fn) = token.get_attr("_restore") {
                        if let PyObjectPayload::NativeClosure(nc) = &restore_fn.payload {
                            return (nc.func)(&[]);
                        }
                    }
                    let had_value = token
                        .get_attr("_had_value")
                        .and_then(|v| match v.payload {
                            PyObjectPayload::Bool(value) => Some(value),
                            _ => None,
                        })
                        .unwrap_or(true);
                    let thread_key = token
                        .get_attr("_thread_key")
                        .and_then(|v| v.as_str().map(CompactString::from))
                        .unwrap_or_else(current_thread_key);
                    if let Some(old) = token.get_attr("old_value") {
                        if had_value {
                            v.write().insert(thread_key, old);
                        } else {
                            v.write().shift_remove(&thread_key);
                        }
                    }
                    Ok(PyObject::none())
                }),
            );
        }
        Ok(inst)
    });

    if let PyObjectPayload::Class(ref cd) = context_var_class.payload {
        cd.namespace
            .write()
            .insert(CompactString::from("__new__"), cv_new);
    }

    make_module(
        "contextvars",
        vec![
            ("ContextVar", context_var_class),
            (
                "Context",
                make_builtin(|_| {
                    let cls =
                        PyObject::class(CompactString::from("Context"), vec![], IndexMap::new());
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref data) = inst.payload {
                        let mut attrs = data.attrs.write();
                        attrs.insert(
                            CompactString::from("run"),
                            make_builtin(|args: &[PyObjectRef]| {
                                if args.is_empty() {
                                    return Err(PyException::type_error(
                                        "Context.run() requires a callable",
                                    ));
                                }
                                let callable = &args[0];
                                let call_args: Vec<PyObjectRef> = args[1..].to_vec();
                                match &callable.payload {
                                    PyObjectPayload::NativeFunction(nf) => (nf.func)(&call_args),
                                    PyObjectPayload::NativeClosure(nc) => (nc.func)(&call_args),
                                    _ => {
                                        ferrython_core::error::request_vm_call(
                                            callable.clone(),
                                            call_args,
                                        );
                                        Ok(PyObject::none())
                                    }
                                }
                            }),
                        );
                        attrs.insert(
                            CompactString::from("copy"),
                            make_builtin(|_| {
                                let cls = PyObject::class(
                                    CompactString::from("Context"),
                                    vec![],
                                    IndexMap::new(),
                                );
                                let copy_inst = PyObject::instance(cls);
                                if let PyObjectPayload::Instance(ref d) = copy_inst.payload {
                                    let mut a = d.attrs.write();
                                    a.insert(
                                        CompactString::from("__len__"),
                                        make_builtin(|_| Ok(PyObject::int(0))),
                                    );
                                }
                                Ok(copy_inst)
                            }),
                        );
                    }
                    Ok(inst)
                }),
            ),
            (
                "copy_context",
                make_builtin(|_| {
                    let cls =
                        PyObject::class(CompactString::from("Context"), vec![], IndexMap::new());
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref data) = inst.payload {
                        let mut attrs = data.attrs.write();
                        attrs.insert(
                            CompactString::from("run"),
                            make_builtin(|args: &[PyObjectRef]| {
                                if args.is_empty() {
                                    return Err(PyException::type_error(
                                        "Context.run() requires a callable",
                                    ));
                                }
                                let callable = &args[0];
                                let call_args: Vec<PyObjectRef> = args[1..].to_vec();
                                match &callable.payload {
                                    PyObjectPayload::NativeFunction(nf) => (nf.func)(&call_args),
                                    PyObjectPayload::NativeClosure(nc) => (nc.func)(&call_args),
                                    _ => {
                                        ferrython_core::error::request_vm_call(
                                            callable.clone(),
                                            call_args,
                                        );
                                        Ok(PyObject::none())
                                    }
                                }
                            }),
                        );
                        attrs.insert(
                            CompactString::from("copy"),
                            make_builtin(|_| {
                                let cls = PyObject::class(
                                    CompactString::from("Context"),
                                    vec![],
                                    IndexMap::new(),
                                );
                                let copy_inst = PyObject::instance(cls);
                                if let PyObjectPayload::Instance(ref d) = copy_inst.payload {
                                    let mut a = d.attrs.write();
                                    a.insert(
                                        CompactString::from("__len__"),
                                        make_builtin(|_| Ok(PyObject::int(0))),
                                    );
                                }
                                Ok(copy_inst)
                            }),
                        );
                        attrs.insert(
                            CompactString::from("__len__"),
                            make_builtin(|_| Ok(PyObject::int(0))),
                        );
                    }
                    Ok(inst)
                }),
            ),
            (
                "Token",
                PyObject::class(CompactString::from("Token"), vec![], IndexMap::new()),
            ),
        ],
    )
}
