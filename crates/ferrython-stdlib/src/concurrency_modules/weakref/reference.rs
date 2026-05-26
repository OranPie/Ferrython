use super::*;

mod proxy;
mod weak_method;

pub(super) use proxy::make_proxy_fn;
pub(super) use weak_method::make_weak_method_fn;

fn is_weak_method_ref(obj: &PyObjectRef) -> bool {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        inst.attrs.read().contains_key("__weakmethod__")
    } else {
        false
    }
}

fn weak_method_parts(obj: &PyObjectRef) -> PyResult<Option<(PyObjectRef, PyObjectRef)>> {
    let Some(call) = obj.get_attr("__call__") else {
        return Ok(None);
    };
    let bound = call_callable(&call, &[])?;
    if matches!(&bound.payload, PyObjectPayload::None) {
        return Ok(None);
    }
    if let PyObjectPayload::BoundMethod { receiver, method } = &bound.payload {
        Ok(Some((receiver.clone(), method.clone())))
    } else {
        Ok(None)
    }
}

fn compare_weak_methods(
    this: &PyObjectRef,
    other: &PyObjectRef,
    op: CompareOp,
) -> PyResult<PyObjectRef> {
    let eq = match (weak_method_parts(this)?, weak_method_parts(other)?) {
        (Some((this_receiver, this_func)), Some((other_receiver, other_func))) => {
            PyObjectRef::ptr_eq(&this_func, &other_func)
                && this_receiver
                    .compare(&other_receiver, CompareOp::Eq)?
                    .is_truthy()
        }
        _ => false,
    };
    Ok(PyObject::bool_val(if matches!(op, CompareOp::Eq) {
        eq
    } else {
        !eq
    }))
}

fn weak_ref_call(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if let Some(this) = args.first() {
        if let PyObjectPayload::Instance(inst) = &this.payload {
            if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
                return call_callable(&target_fn, &[]);
            }
        }
    }
    Ok(PyObject::none())
}

fn weak_ref_init(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let marker = HashableKey::str_key(CompactString::from("__weakref_ref_kwargs__"));
    let (args, has_kwargs_marker) =
        if let Some(PyObjectPayload::Dict(map)) = args.last().map(|arg| &arg.payload) {
            let has_marker = map.read().contains_key(&marker);
            if has_marker {
                (&args[..args.len() - 1], true)
            } else {
                (args, false)
            }
        } else {
            (args, false)
        };
    if has_kwargs_marker {
        return Err(PyException::type_error("ref() takes no keyword arguments"));
    }
    if args.len() > 3 {
        return Err(PyException::type_error(format!(
            "__init__() takes at most 2 arguments ({} given)",
            args.len().saturating_sub(1)
        )));
    }
    Ok(PyObject::none())
}

fn weak_ref_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "ref.__new__ requires type and object",
        ));
    }
    let marker = HashableKey::str_key(CompactString::from("__weakref_ref_kwargs__"));
    let (args, has_kwargs_marker) =
        if let Some(PyObjectPayload::Dict(map)) = args.last().map(|arg| &arg.payload) {
            let has_marker = map.read().contains_key(&marker);
            if has_marker {
                (&args[..args.len() - 1], true)
            } else {
                (args, false)
            }
        } else {
            (args, false)
        };
    if args.len() > 3 {
        return Err(PyException::type_error(format!(
            "ref() takes at most 2 arguments ({} given)",
            args.len() - 1
        )));
    }
    let cls = args[0].clone();
    if has_kwargs_marker {
        if let PyObjectPayload::Class(cd) = &cls.payload {
            if cd.name.as_str() == "weakref" {
                return Err(PyException::type_error("ref() takes no keyword arguments"));
            }
        }
    }
    let target = args[1].clone();
    let callback = args.get(2).cloned().unwrap_or_else(PyObject::none);
    let callback = if matches!(callback.payload, PyObjectPayload::None) {
        None
    } else {
        Some(callback)
    };
    if callback.is_none() {
        if let PyObjectPayload::Class(cd) = &cls.payload {
            if cd.name.as_str() == "weakref" {
                if let Some(existing) =
                    PyObjectRef::find_shared_weak_object(&target, WeakObjectKind::Ref)
                {
                    if let PyObjectPayload::Instance(inst) = &existing.payload {
                        if let PyObjectPayload::Class(cd) = &inst.class.payload {
                            if cd.name.as_str() == "weakref" {
                                return Ok(existing);
                            }
                        }
                    }
                }
            }
        }
    }
    let weak: PyWeakRef = PyObjectRef::downgrade(&target);
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(
            CompactString::from("__weakref_ref__"),
            PyObject::bool_val(true),
        );
        attrs.insert(
            CompactString::from("__weakref_callback__"),
            callback.clone().unwrap_or_else(PyObject::none),
        );
        let target_weak = weak.clone();
        attrs.insert(
            CompactString::from("__weakref_target__"),
            PyObject::native_closure("weakref.__target__", move |_| {
                Ok(upgrade_or_none(&target_weak))
            }),
        );
        let repr_weak = weak.clone();
        attrs.insert(
            CompactString::from("__repr__"),
            PyObject::native_closure("weakref.__repr__", move |_| {
                if repr_weak.upgrade().is_some() {
                    Ok(PyObject::str_val(CompactString::from("<weakref (alive)>")))
                } else {
                    Ok(PyObject::str_val(CompactString::from("<weakref (dead)>")))
                }
            }),
        );
        let bool_weak = weak.clone();
        attrs.insert(
            CompactString::from("__bool__"),
            PyObject::native_closure("weakref.__bool__", move |_| {
                Ok(PyObject::bool_val(bool_weak.upgrade().is_some()))
            }),
        );
    }
    PyObjectRef::register_weak_object(&target, &inst, callback, WeakObjectKind::Ref);
    Ok(inst)
}

pub(super) fn configure_reference_type(reference_type: &PyObjectRef) {
    let mut reference_namespace = IndexMap::new();
    reference_namespace.insert(
        CompactString::from("__new__"),
        PyObject::native_function("weakref.__new__", weak_ref_new),
    );
    reference_namespace.insert(
        CompactString::from("__init__"),
        PyObject::native_function("weakref.__init__", weak_ref_init),
    );
    reference_namespace.insert(
        CompactString::from("__call__"),
        PyObject::native_function("weakref.__call__", weak_ref_call),
    );
    reference_namespace.insert(
        CompactString::from("__eq__"),
        PyObject::native_closure("weakref.__eq__", |args| {
            if let (Some(this), Some(other)) = (args.first(), args.get(1)) {
                if PyObjectRef::ptr_eq(this, other) {
                    return Ok(PyObject::bool_val(true));
                }
                if is_weak_method_ref(this) || is_weak_method_ref(other) {
                    if !is_weak_method_ref(this) || !is_weak_method_ref(other) {
                        return Ok(PyObject::bool_val(false));
                    }
                    return compare_weak_methods(this, other, CompareOp::Eq);
                }
                let this_obj = weak_ref_call(&[this.clone()])?;
                let other_obj = weak_ref_call(&[other.clone()])?;
                if matches!(&this_obj.payload, PyObjectPayload::None)
                    || matches!(&other_obj.payload, PyObjectPayload::None)
                {
                    return Ok(PyObject::bool_val(false));
                }
                return this_obj.compare(&other_obj, CompareOp::Eq);
            }
            Ok(PyObject::bool_val(false))
        }),
    );
    reference_namespace.insert(
        CompactString::from("__ne__"),
        PyObject::native_closure("weakref.__ne__", |args| {
            if let (Some(this), Some(other)) = (args.first(), args.get(1)) {
                if PyObjectRef::ptr_eq(this, other) {
                    return Ok(PyObject::bool_val(false));
                }
                if is_weak_method_ref(this) || is_weak_method_ref(other) {
                    if !is_weak_method_ref(this) || !is_weak_method_ref(other) {
                        return Ok(PyObject::bool_val(true));
                    }
                    return compare_weak_methods(this, other, CompareOp::Ne);
                }
                let this_obj = weak_ref_call(&[this.clone()])?;
                let other_obj = weak_ref_call(&[other.clone()])?;
                if matches!(&this_obj.payload, PyObjectPayload::None)
                    || matches!(&other_obj.payload, PyObjectPayload::None)
                {
                    return Ok(PyObject::bool_val(true));
                }
                return this_obj.compare(&other_obj, CompareOp::Ne);
            }
            Ok(PyObject::bool_val(true))
        }),
    );
    if let PyObjectPayload::Class(cd) = &reference_type.payload {
        cd.namespace.write().extend(reference_namespace);
        cd.has_custom_new.set(true);
        cd.is_simple_class.set(false);
        cd.method_cache.write().clear();
    }
}
