use super::*;

pub(super) fn inspect_getmro(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.getmro", args, 1)?;
    if let PyObjectPayload::Class(cd) = &args[0].payload {
        let mut mro = vec![args[0].clone()];
        mro.extend(cd.mro.iter().cloned());
        Ok(PyObject::tuple(mro))
    } else if let Some(mro) = args[0].get_attr("__mro__") {
        Ok(mro)
    } else {
        Ok(PyObject::tuple(vec![args[0].clone()]))
    }
}

pub(super) fn inspect_getargspec(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.getargspec", args, 1)?;
    if let PyObjectPayload::Function(pf) = &args[0].payload {
        let code = &pf.code;
        let ac = code.arg_count as usize;
        let has_varargs = code.flags.contains(CodeFlags::VARARGS);
        let has_varkw = code.flags.contains(CodeFlags::VARKEYWORDS);
        let mut positional = Vec::new();
        for i in 0..ac {
            if i < code.varnames.len() {
                positional.push(PyObject::str_val(code.varnames[i].clone()));
            }
        }
        let varargs = if has_varargs && ac < code.varnames.len() {
            PyObject::str_val(code.varnames[ac].clone())
        } else {
            PyObject::none()
        };
        let kw_start = ac + if has_varargs { 1 } else { 0 };
        let kwc = code.kwonlyarg_count as usize;
        let varkw = if has_varkw && kw_start + kwc < code.varnames.len() {
            PyObject::str_val(code.varnames[kw_start + kwc].clone())
        } else {
            PyObject::none()
        };
        let defaults_guard = pf.defaults.read();
        let defaults = if defaults_guard.is_empty() {
            PyObject::none()
        } else {
            PyObject::tuple(defaults_guard.clone())
        };
        Ok(PyObject::tuple(vec![
            PyObject::list(positional),
            varargs,
            varkw,
            defaults,
        ]))
    } else {
        Err(PyException::type_error("unsupported callable"))
    }
}

pub(super) fn inspect_classify_class_attrs(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("inspect.classify_class_attrs", args, 1)?;
    Ok(PyObject::list(vec![]))
}
