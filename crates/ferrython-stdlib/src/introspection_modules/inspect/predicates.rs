use super::*;

pub(super) fn inspect_isfunction(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.isfunction", args, 1)?;
    Ok(PyObject::bool_val(matches!(
        &args[0].payload,
        PyObjectPayload::Function(_)
    )))
}

pub(super) fn inspect_isclass(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.isclass", args, 1)?;
    Ok(PyObject::bool_val(matches!(
        &args[0].payload,
        PyObjectPayload::Class(_) | PyObjectPayload::BuiltinType(_)
    )))
}

pub(super) fn inspect_ismethod(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.ismethod", args, 1)?;
    Ok(PyObject::bool_val(matches!(
        &args[0].payload,
        PyObjectPayload::BoundMethod { .. }
    )))
}

pub(super) fn inspect_ismodule(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.ismodule", args, 1)?;
    Ok(PyObject::bool_val(matches!(
        &args[0].payload,
        PyObjectPayload::Module(_)
    )))
}

pub(super) fn inspect_isbuiltin(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.isbuiltin", args, 1)?;
    Ok(PyObject::bool_val(matches!(
        &args[0].payload,
        PyObjectPayload::NativeFunction(_)
            | PyObjectPayload::BuiltinFunction(_)
            | PyObjectPayload::BuiltinType(_)
    )))
}

pub(super) fn inspect_isgenerator(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.isgenerator", args, 1)?;
    Ok(PyObject::bool_val(matches!(
        &args[0].payload,
        PyObjectPayload::Generator(_)
    )))
}

pub(super) fn inspect_isgeneratorfunction(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.isgeneratorfunction", args, 1)?;
    if let PyObjectPayload::Function(f) = &args[0].payload {
        Ok(PyObject::bool_val(
            f.code.flags.contains(CodeFlags::GENERATOR),
        ))
    } else {
        Ok(PyObject::bool_val(false))
    }
}

pub(super) fn inspect_iscoroutine(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.iscoroutine", args, 1)?;
    Ok(PyObject::bool_val(matches!(
        &args[0].payload,
        PyObjectPayload::Coroutine(_)
    )))
}

pub(super) fn inspect_iscoroutinefunction(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.iscoroutinefunction", args, 1)?;
    if let PyObjectPayload::Function(pf) = &args[0].payload {
        Ok(PyObject::bool_val(
            pf.code.flags.contains(CodeFlags::COROUTINE),
        ))
    } else {
        Ok(PyObject::bool_val(false))
    }
}

pub(super) fn inspect_isroutine(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.isroutine", args, 1)?;
    Ok(PyObject::bool_val(matches!(
        &args[0].payload,
        PyObjectPayload::Function(_)
            | PyObjectPayload::BoundMethod { .. }
            | PyObjectPayload::NativeFunction(_)
            | PyObjectPayload::NativeClosure(_)
            | PyObjectPayload::BuiltinBoundMethod(_)
            | PyObjectPayload::BuiltinFunction(_)
    )))
}

pub(super) fn inspect_isabstract(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.isabstract", args, 1)?;
    Ok(PyObject::bool_val(
        args[0].get_attr("__abstractmethods__").is_some(),
    ))
}

pub(super) fn inspect_isasyncgen(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.isasyncgen", args, 1)?;
    Ok(PyObject::bool_val(matches!(
        &args[0].payload,
        PyObjectPayload::AsyncGenerator(_)
    )))
}

pub(super) fn inspect_isasyncgenfunction(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.isasyncgenfunction", args, 1)?;
    if let PyObjectPayload::Function(pf) = &args[0].payload {
        Ok(PyObject::bool_val(
            pf.code.flags.contains(CodeFlags::ASYNC_GENERATOR),
        ))
    } else {
        Ok(PyObject::bool_val(false))
    }
}

pub(super) fn inspect_isawaitable(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.isawaitable", args, 1)?;
    Ok(PyObject::bool_val(matches!(
        &args[0].payload,
        PyObjectPayload::Coroutine(_) | PyObjectPayload::BuiltinAwaitable(_)
    )))
}

pub(super) fn inspect_isdatadescriptor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.isdatadescriptor", args, 1)?;
    Ok(PyObject::bool_val(
        args[0].get_attr("__get__").is_some() && args[0].get_attr("__set__").is_some(),
    ))
}
