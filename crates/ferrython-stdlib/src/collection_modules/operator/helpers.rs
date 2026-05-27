use super::*;

pub(super) fn call_dunder(
    obj: &PyObjectRef,
    name: &str,
    args: &[PyObjectRef],
) -> PyResult<Option<PyObjectRef>> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
            if let PyObjectPayload::NativeClosure(ref nc) = target_fn.payload {
                let referent = (nc.func)(&[])?;
                return call_dunder(&referent, name, args);
            }
        }
    }
    if let Some(method) = obj.get_attr(name) {
        let result = ferrython_core::object::call_callable(&method, args)?;
        if matches!(&result.payload, PyObjectPayload::NotImplemented) {
            Ok(None)
        } else {
            Ok(Some(result))
        }
    } else {
        Ok(None)
    }
}

pub(super) fn call_inplace_dunder(
    obj: &PyObjectRef,
    arg: &PyObjectRef,
    inplace_name: &str,
    fallback_name: &str,
) -> PyResult<Option<PyObjectRef>> {
    if let Some(result) = call_dunder(obj, inplace_name, &[arg.clone()])? {
        return Ok(Some(result));
    }
    call_dunder(obj, fallback_name, &[arg.clone()])
}

pub(super) fn operator_call_iterator_dunder(
    receiver: &PyObjectRef,
    method: &PyObjectRef,
) -> PyResult<PyObjectRef> {
    if matches!(&receiver.payload, PyObjectPayload::Module(_))
        && matches!(
            &method.payload,
            PyObjectPayload::NativeFunction(_) | PyObjectPayload::NativeClosure(_)
        )
    {
        ferrython_core::object::call_callable(method, &[receiver.clone()])
    } else {
        ferrython_core::object::call_callable(method, &[])
    }
}

pub(super) fn operator_iterator_from(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    if matches!(&obj.payload, PyObjectPayload::Module(_)) {
        if let Some(iter_method) = obj.get_attr("__iter__") {
            let iter = operator_call_iterator_dunder(obj, &iter_method)?;
            if iter.get_attr("__next__").is_some() {
                return Ok(iter);
            }
            return Err(PyException::type_error(format!(
                "iter() returned non-iterator of type '{}'",
                obj.type_name()
            )));
        }
        if obj.get_attr("__next__").is_some() {
            return Ok(obj.clone());
        }
        return Err(PyException::type_error(format!(
            "'{}' object is not iterable",
            obj.type_name()
        )));
    }
    obj.get_iter()
}

pub(super) fn operator_next_from_iter(iter: &PyObjectRef) -> PyResult<Option<PyObjectRef>> {
    let next = iter.get_attr("__next__").ok_or_else(|| {
        PyException::type_error(format!("'{}' object is not an iterator", iter.type_name()))
    })?;
    match operator_call_iterator_dunder(iter, &next) {
        Ok(value) => Ok(Some(value)),
        Err(err) if err.kind == ExceptionKind::StopIteration => Ok(None),
        Err(err) => Err(err),
    }
}

pub(super) fn operator_index_of(seq: &PyObjectRef, target: &PyObjectRef) -> PyResult<i64> {
    match &seq.payload {
        PyObjectPayload::List(items) => {
            for (i, item) in items.read().iter().enumerate() {
                if item.compare(target, CompareOp::Eq)?.is_truthy() {
                    return Ok(i as i64);
                }
            }
        }
        PyObjectPayload::Tuple(items) => {
            for (i, item) in items.iter().enumerate() {
                if item.compare(target, CompareOp::Eq)?.is_truthy() {
                    return Ok(i as i64);
                }
            }
        }
        _ => {
            let iter = operator_iterator_from(seq)?;
            let mut index = 0i64;
            while let Some(item) = operator_next_from_iter(&iter)? {
                if item.compare(target, CompareOp::Eq)?.is_truthy() {
                    return Ok(index);
                }
                index += 1;
            }
        }
    }
    Err(PyException::value_error(
        "sequence.index(x): x not in sequence",
    ))
}

pub(super) fn operator_count_of(seq: &PyObjectRef, target: &PyObjectRef) -> PyResult<i64> {
    match &seq.payload {
        PyObjectPayload::List(items) => {
            let mut count = 0i64;
            for item in items.read().iter() {
                if item.compare(target, CompareOp::Eq)?.is_truthy() {
                    count += 1;
                }
            }
            Ok(count)
        }
        PyObjectPayload::Tuple(items) => {
            let mut count = 0i64;
            for item in items.iter() {
                if item.compare(target, CompareOp::Eq)?.is_truthy() {
                    count += 1;
                }
            }
            Ok(count)
        }
        _ => {
            let iter = operator_iterator_from(seq)?;
            let mut count = 0i64;
            while let Some(item) = operator_next_from_iter(&iter)? {
                if item.compare(target, CompareOp::Eq)?.is_truthy() {
                    count += 1;
                }
            }
            Ok(count)
        }
    }
}

pub(super) fn builtin_index_value(obj: &PyObjectRef) -> Option<PyInt> {
    match &obj.payload {
        PyObjectPayload::Int(n) => Some(n.clone()),
        PyObjectPayload::Bool(b) => Some(PyInt::Small(if *b { 1 } else { 0 })),
        PyObjectPayload::Instance(inst) => inst
            .attrs
            .read()
            .get("__builtin_value__")
            .and_then(builtin_index_value),
        _ => None,
    }
}

pub(super) fn object_index_result(result: PyObjectRef) -> PyResult<PyObjectRef> {
    match &result.payload {
        PyObjectPayload::Int(n) => Ok(n.to_object()),
        PyObjectPayload::Bool(b) => {
            emit_deprecation_warning(
                "__index__ returned non-int (type bool). The ability to return an instance of a strict subclass of int is deprecated.",
            );
            Ok(PyObject::int(if *b { 1 } else { 0 }))
        }
        PyObjectPayload::Instance(inst) => {
            if let Some(value) = inst.attrs.read().get("__builtin_value__").cloned() {
                if let Some(index) = builtin_index_value(&value) {
                    emit_deprecation_warning(
                        "__index__ returned non-int (type int). The ability to return an instance of a strict subclass of int is deprecated.",
                    );
                    return Ok(index.to_object());
                }
            }
            Err(PyException::type_error(format!(
                "__index__ returned non-int (type {})",
                result.type_name()
            )))
        }
        _ => Err(PyException::type_error(format!(
            "__index__ returned non-int (type {})",
            result.type_name()
        ))),
    }
}
