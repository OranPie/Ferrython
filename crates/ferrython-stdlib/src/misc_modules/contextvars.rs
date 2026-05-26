use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

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
            let value: Rc<PyCell<Option<PyObjectRef>>> = Rc::new(PyCell::new(default_val.clone()));

            let v = value.clone();
            attrs.insert(
                CompactString::from("get"),
                PyObject::native_closure("ContextVar.get", move |a: &[PyObjectRef]| {
                    if let Some(val) = v.read().as_ref() {
                        Ok(val.clone())
                    } else if !a.is_empty() {
                        Ok(a[0].clone())
                    } else {
                        Err(PyException::runtime_error("ContextVar has no value"))
                    }
                }),
            );

            let v = value.clone();
            let name_clone = name.clone();
            attrs.insert(
                CompactString::from("set"),
                PyObject::native_closure("ContextVar.set", move |a: &[PyObjectRef]| {
                    if a.is_empty() {
                        return Err(PyException::type_error("set() requires a value"));
                    }
                    let old = v.read().clone();
                    *v.write() = Some(a[0].clone());
                    let v_restore = v.clone();
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
                            CompactString::from("var"),
                            PyObject::str_val(CompactString::from(name_clone.as_str())),
                        );
                        let old_clone = old;
                        ta.insert(
                            CompactString::from("_restore"),
                            PyObject::native_closure("Token._restore", move |_| {
                                *v_restore.write() = old_clone.clone();
                                Ok(PyObject::none())
                            }),
                        );
                    }
                    Ok(token)
                }),
            );

            let v = value.clone();
            let default_clone = default_val.clone();
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
                    if let Some(old) = token.get_attr("old_value") {
                        if matches!(&old.payload, PyObjectPayload::None) {
                            *v.write() = default_clone.clone();
                        } else {
                            *v.write() = Some(old);
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
