use super::*;

pub(crate) fn make_weak_method_fn(reference_type: &PyObjectRef) -> PyObjectRef {
    let reference_type = reference_type.clone();
    PyObject::native_closure("weakref.WeakMethod", move |args| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "WeakMethod requires at least 1 argument",
            ));
        }
        if args.len() > 2 {
            return Err(PyException::type_error(
                "WeakMethod expected at most 2 arguments",
            ));
        }
        let method = args[0].clone();
        let (receiver, func) = match &method.payload {
            PyObjectPayload::BoundMethod { receiver, method } => (receiver.clone(), method.clone()),
            PyObjectPayload::BuiltinBoundMethod(bbm) => (
                bbm.receiver.clone(),
                PyObject::str_val(bbm.method_name.clone()),
            ),
            _ => {
                let receiver = method.get_attr("__self__").ok_or_else(|| {
                    PyException::type_error("argument should be a bound method, not other callable")
                })?;
                let func = method.get_attr("__func__").ok_or_else(|| {
                    PyException::type_error("argument should be a bound method, not other callable")
                })?;
                (receiver, func)
            }
        };
        let callback = args.get(1).cloned().unwrap_or_else(PyObject::none);
        let callback = if matches!(callback.payload, PyObjectPayload::None) {
            None
        } else {
            Some(callback)
        };
        let weak_receiver = PyObjectRef::downgrade(&receiver);
        let weak_func = PyObjectRef::downgrade(&func);
        let cls = PyObject::class(
            CompactString::from("WeakMethod"),
            vec![reference_type.clone()],
            IndexMap::new(),
        );
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(
                CompactString::from("__weakref_ref__"),
                PyObject::bool_val(true),
            );
            w.insert(
                CompactString::from("__weakmethod__"),
                PyObject::bool_val(true),
            );
            w.insert(
                CompactString::from("__weakref_callback__"),
                callback.clone().unwrap_or_else(PyObject::none),
            );
            let call_receiver = weak_receiver.clone();
            let call_func = weak_func.clone();
            w.insert(
                CompactString::from("__call__"),
                PyObject::native_closure("WeakMethod.__call__", move |_| {
                    let Some(receiver) = call_receiver.upgrade() else {
                        return Ok(PyObject::none());
                    };
                    let Some(func) = call_func.upgrade() else {
                        return Ok(PyObject::none());
                    };
                    Ok(PyObject::wrap(PyObjectPayload::BoundMethod {
                        receiver,
                        method: func,
                    }))
                }),
            );
            let bool_receiver = weak_receiver.clone();
            let bool_func = weak_func.clone();
            w.insert(
                CompactString::from("__bool__"),
                PyObject::native_closure("WeakMethod.__bool__", move |_| {
                    Ok(PyObject::bool_val(
                        bool_receiver.upgrade().is_some() && bool_func.upgrade().is_some(),
                    ))
                }),
            );
        }
        if let Some(callback) = callback {
            let fired = Rc::new(Cell::new(false));
            let cb1 = callback.clone();
            let weak_method = inst.clone();
            let fired1 = fired.clone();
            let callback_wrapper = PyObject::native_closure("WeakMethod.callback", move |_| {
                if !fired1.replace(true) {
                    call_callable(&cb1, &[weak_method.clone()])?;
                }
                Ok(PyObject::none())
            });
            PyObjectRef::register_weak_object(
                &receiver,
                &inst,
                Some(callback_wrapper.clone()),
                WeakObjectKind::Ref,
            );
            let cb2 = callback;
            let weak_method = inst.clone();
            let fired2 = fired;
            let callback_wrapper = PyObject::native_closure("WeakMethod.callback", move |_| {
                if !fired2.replace(true) {
                    call_callable(&cb2, &[weak_method.clone()])?;
                }
                Ok(PyObject::none())
            });
            PyObjectRef::register_weak_object(
                &func,
                &inst,
                Some(callback_wrapper),
                WeakObjectKind::Ref,
            );
        }
        Ok(inst)
    })
}
