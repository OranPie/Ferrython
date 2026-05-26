use super::*;

fn sre_int_arg(args: &[PyObjectRef], index: usize, name: &str) -> PyResult<i64> {
    args.get(index)
        .ok_or_else(|| PyException::type_error(format!("{}() missing required argument", name)))?
        .to_int()
}

fn sre_ascii_tolower(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let code = sre_int_arg(args, 0, "ascii_tolower")?;
    let lowered = if (b'A' as i64..=b'Z' as i64).contains(&code) {
        code + 32
    } else {
        code
    };
    Ok(PyObject::int(lowered))
}

fn sre_unicode_tolower(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let code = sre_int_arg(args, 0, "unicode_tolower")?;
    let lowered = u32::try_from(code)
        .ok()
        .and_then(char::from_u32)
        .and_then(|ch| ch.to_lowercase().next())
        .map(|ch| ch as i64)
        .unwrap_or(code);
    Ok(PyObject::int(lowered))
}

fn sre_ascii_iscased(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let code = sre_int_arg(args, 0, "ascii_iscased")?;
    Ok(PyObject::bool_val(
        (b'A' as i64..=b'Z' as i64).contains(&code) || (b'a' as i64..=b'z' as i64).contains(&code),
    ))
}

fn sre_unicode_iscased(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let code = sre_int_arg(args, 0, "unicode_iscased")?;
    let iscased = u32::try_from(code)
        .ok()
        .and_then(char::from_u32)
        .map(|ch| {
            let original = ch.to_string();
            ch.to_lowercase().collect::<String>() != original
                || ch.to_uppercase().collect::<String>() != original
        })
        .unwrap_or(false);
    Ok(PyObject::bool_val(iscased))
}

fn sre_getcodesize(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::int(4))
}

fn sre_compile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 6 {
        return Err(PyException::type_error(
            "compile() missing required arguments",
        ));
    }
    if !matches!(args[4].payload, PyObjectPayload::Dict(_)) {
        return Err(PyException::type_error(format!(
            "compile() argument 'groupindex' must be dict, not {}",
            args[4].type_name()
        )));
    }
    let PyObjectPayload::List(code) = &args[2].payload else {
        return Err(PyException::type_error(format!(
            "compile() argument 'code' must be list, not {}",
            args[2].type_name()
        )));
    };
    for item in code.read().iter() {
        match item.to_int() {
            Ok(value) if (0..=u32::MAX as i64).contains(&value) => {}
            Ok(_) => {
                return Err(PyException::overflow_error(
                    "regular expression code size limit exceeded",
                ));
            }
            Err(exc) if matches!(exc.kind, ExceptionKind::OverflowError) => {
                return Err(PyException::overflow_error(
                    "regular expression code size limit exceeded",
                ));
            }
            Err(exc) => return Err(exc),
        }
    }
    Err(PyException::new(
        ExceptionKind::RuntimeError,
        CompactString::from("invalid SRE code"),
    ))
}

pub fn create_sre_module() -> PyObjectRef {
    make_module(
        "_sre",
        vec![
            ("MAGIC", PyObject::int(20171005)),
            ("CODESIZE", PyObject::int(4)),
            ("MAXREPEAT", PyObject::int(u32::MAX as i64)),
            ("MAXGROUPS", PyObject::int(2_147_483_647)),
            (
                "ascii_tolower",
                PyObject::native_function("_sre.ascii_tolower", sre_ascii_tolower),
            ),
            (
                "unicode_tolower",
                PyObject::native_function("_sre.unicode_tolower", sre_unicode_tolower),
            ),
            (
                "ascii_iscased",
                PyObject::native_function("_sre.ascii_iscased", sre_ascii_iscased),
            ),
            (
                "unicode_iscased",
                PyObject::native_function("_sre.unicode_iscased", sre_unicode_iscased),
            ),
            (
                "getcodesize",
                PyObject::native_function("_sre.getcodesize", sre_getcodesize),
            ),
            (
                "compile",
                PyObject::native_function("_sre.compile", sre_compile),
            ),
        ],
    )
}
