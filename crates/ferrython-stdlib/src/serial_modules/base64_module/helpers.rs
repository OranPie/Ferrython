use super::*;

pub(crate) fn extract_bytes(obj: &PyObjectRef) -> PyResult<Vec<u8>> {
    extract_bytes_like(obj, true, false, "bytes-like object")
}

pub(super) fn split_kwargs(args: &[PyObjectRef]) -> (&[PyObjectRef], Option<PyObjectRef>) {
    if args.len() > 1 && matches!(&args[args.len() - 1].payload, PyObjectPayload::Dict(_)) {
        (&args[..args.len() - 1], Some(args[args.len() - 1].clone()))
    } else {
        (args, None)
    }
}

pub(super) fn kw_arg(kwargs: Option<&PyObjectRef>, key: &str) -> Option<PyObjectRef> {
    let PyObjectPayload::Dict(map) = &kwargs?.payload else {
        return None;
    };
    map.read()
        .get(&HashableKey::str_key(CompactString::from(key)))
        .cloned()
}

pub(super) fn arg_or_kw(
    pos: &[PyObjectRef],
    kwargs: Option<&PyObjectRef>,
    idx: usize,
    key: &str,
) -> Option<PyObjectRef> {
    pos.get(idx).cloned().or_else(|| kw_arg(kwargs, key))
}

pub(super) fn bool_arg(
    pos: &[PyObjectRef],
    kwargs: Option<&PyObjectRef>,
    idx: usize,
    key: &str,
    default: bool,
) -> bool {
    arg_or_kw(pos, kwargs, idx, key)
        .map(|v| v.is_truthy())
        .unwrap_or(default)
}

pub(super) fn int_arg(
    pos: &[PyObjectRef],
    kwargs: Option<&PyObjectRef>,
    idx: usize,
    key: &str,
    default: i64,
) -> i64 {
    arg_or_kw(pos, kwargs, idx, key)
        .and_then(|v| v.as_int())
        .unwrap_or(default)
}

pub(super) fn is_none(obj: &PyObjectRef) -> bool {
    matches!(&obj.payload, PyObjectPayload::None)
}

fn extract_ascii_str(s: &str) -> PyResult<Vec<u8>> {
    if !s.is_ascii() {
        return Err(PyException::value_error(
            "string argument should contain only ASCII characters",
        ));
    }
    Ok(s.as_bytes().to_vec())
}

pub(in crate::serial_modules) fn extract_bytes_like(
    obj: &PyObjectRef,
    allow_str: bool,
    legacy_memoryview: bool,
    func_name: &str,
) -> PyResult<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Ok((**b).clone()),
        PyObjectPayload::Str(s) if allow_str => extract_ascii_str(s),
        PyObjectPayload::Str(_) => Err(PyException::type_error(format!(
            "{} expected a bytes-like object, not str",
            func_name
        ))),
        PyObjectPayload::Instance(_) if obj.get_attr("__memoryview__").is_some() => {
            if legacy_memoryview {
                let ndim = obj.get_attr("ndim").and_then(|v| v.as_int()).unwrap_or(1);
                let format = obj
                    .get_attr("format")
                    .map(|v| v.py_to_string())
                    .unwrap_or_else(|| "B".to_string());
                if ndim != 1 || !matches!(format.as_str(), "B" | "b" | "c") {
                    return Err(PyException::type_error(
                        "expected single-dimensional byte-oriented buffer",
                    ));
                }
            }
            if let Some(base) = obj.get_attr("obj") {
                extract_bytes_like(&base, false, false, func_name)
            } else {
                Err(PyException::type_error("expected bytes-like object"))
            }
        }
        PyObjectPayload::Instance(_) => {
            if let Some(data) = obj.get_attr("_data") {
                if let Some(typecode) = obj.get_attr("typecode") {
                    if let PyObjectPayload::List(items) = &data.payload {
                        return array_items_to_bytes(typecode.py_to_string().as_str(), items);
                    }
                }
            }
            Err(PyException::type_error("expected bytes-like object"))
        }
        _ => Err(PyException::type_error("expected bytes-like object")),
    }
}

fn array_items_to_bytes(typecode: &str, items: &PyCell<Vec<PyObjectRef>>) -> PyResult<Vec<u8>> {
    let r = items.read();
    let mut out = Vec::new();
    for item in r.iter() {
        let value = item.to_int()?;
        match typecode {
            "b" => out.push(value as i8 as u8),
            "B" => out.push(value as u8),
            "h" => out.extend_from_slice(&(value as i16).to_ne_bytes()),
            "H" => out.extend_from_slice(&(value as u16).to_ne_bytes()),
            "i" | "l" => out.extend_from_slice(&(value as i32).to_ne_bytes()),
            "I" | "L" => out.extend_from_slice(&(value as u32).to_ne_bytes()),
            "q" => out.extend_from_slice(&value.to_ne_bytes()),
            "Q" => out.extend_from_slice(&(value as u64).to_ne_bytes()),
            _ => out.push(value as u8),
        }
    }
    Ok(out)
}
