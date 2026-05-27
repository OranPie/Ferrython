use super::*;

pub(super) fn create_pathlike_class() -> PyObjectRef {
    PyObject::class(CompactString::from("PathLike"), vec![], {
        let mut ns = IndexMap::new();
        ns.insert(
            CompactString::from("__fspath__"),
            make_builtin(|_args: &[PyObjectRef]| {
                Err(PyException::not_implemented_error(
                    "PathLike.__fspath__() is abstract",
                ))
            }),
        );
        ns.insert(
            CompactString::from("register"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.len() >= 2 {
                    Ok(args[1].clone())
                } else if args.len() == 1 {
                    Ok(args[0].clone())
                } else {
                    Ok(PyObject::none())
                }
            }),
        );
        ns
    })
}

pub(super) fn os_fspath(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("os.fspath", args, 1)?;
    match &args[0].payload {
        PyObjectPayload::Str(_) => Ok(args[0].clone()),
        PyObjectPayload::Bytes(_) => Ok(args[0].clone()),
        _ => {
            if let Some(method) = args[0].get_attr("__fspath__") {
                match &method.payload {
                    PyObjectPayload::NativeFunction(nf) => (nf.func)(&[args[0].clone()]),
                    PyObjectPayload::NativeClosure(nc) => (nc.func)(&[args[0].clone()]),
                    PyObjectPayload::Function(_) => Ok(PyObject::str_val(CompactString::from(
                        args[0].py_to_string(),
                    ))),
                    _ => Err(PyException::type_error(format!(
                        "expected str, bytes or os.PathLike object, not '{}'",
                        args[0].type_name()
                    ))),
                }
            } else {
                Err(PyException::type_error(format!(
                    "expected str, bytes or os.PathLike object, not '{}'",
                    args[0].type_name()
                )))
            }
        }
    }
}
