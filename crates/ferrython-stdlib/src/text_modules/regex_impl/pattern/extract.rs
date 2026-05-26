use super::*;

pub(in crate::text_modules::regex_impl) fn is_re_pattern_object(obj: &PyObjectRef) -> bool {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        let attrs = inst.attrs.read();
        if attrs.contains_key("__re_pattern__") {
            return true;
        }
        if !attrs.contains_key("_pattern_text") {
            return false;
        }
        drop(attrs);
        if let PyObjectPayload::Class(cd) = &inst.class.payload {
            return cd.name.as_str() == "Pattern";
        }
    }
    false
}

pub(in crate::text_modules::regex_impl) fn re_pattern_text_attr(
    obj: &PyObjectRef,
) -> Option<String> {
    if !is_re_pattern_object(obj) {
        return None;
    }
    obj.get_attr("_pattern_text").map(|v| v.py_to_string())
}

pub(in crate::text_modules::regex_impl) fn re_pattern_is_bytes(obj: &PyObjectRef) -> bool {
    if is_re_pattern_object(obj) {
        return obj
            .get_attr("_pattern_is_bytes")
            .map(|v| v.is_truthy())
            .unwrap_or(false);
    }
    extract_bytes_like(obj).is_some()
}

pub(in crate::text_modules::regex_impl) fn readonly_mapping(map: FxHashKeyMap) -> PyObjectRef {
    PyObject::wrap(PyObjectPayload::MappingProxy(Rc::new(PyCell::new(map))))
}

pub(in crate::text_modules::regex_impl) fn bytes_to_regex_text(bytes: &[u8]) -> String {
    bytes.iter().map(|&byte| byte as char).collect()
}

pub(in crate::text_modules::regex_impl) fn regex_text_to_bytes(text: &str) -> Vec<u8> {
    text.chars().map(|ch| ch as u32 as u8).collect()
}

pub(in crate::text_modules::regex_impl) fn py_re_text(text: &str, is_bytes: bool) -> PyObjectRef {
    if is_bytes {
        PyObject::bytes(regex_text_to_bytes(text))
    } else {
        PyObject::str_val(CompactString::from(text))
    }
}

pub(in crate::text_modules::regex_impl) fn extract_bytes_like(
    obj: &PyObjectRef,
) -> Option<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Bytes(bytes) | PyObjectPayload::ByteArray(bytes) => {
            Some((**bytes).clone())
        }
        PyObjectPayload::Instance(inst) => {
            let next = {
                let attrs = inst.attrs.read();
                if attrs
                    .get("__array__")
                    .map(|flag| flag.is_truthy())
                    .unwrap_or(false)
                {
                    if let Some(data) = attrs.get("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            let bytes: Vec<u8> = items
                                .read()
                                .iter()
                                .filter_map(|item| item.to_int().ok().map(|value| value as u8))
                                .collect();
                            return Some(bytes);
                        }
                    }
                }
                attrs.get("__builtin_value__").cloned().or_else(|| {
                    if attrs
                        .get("__memoryview__")
                        .map(|flag| flag.is_truthy())
                        .unwrap_or(false)
                    {
                        attrs.get("obj").cloned()
                    } else {
                        None
                    }
                })
            };
            next.and_then(|value| extract_bytes_like(&value))
        }
        _ => None,
    }
}

pub(in crate::text_modules::regex_impl) fn extract_str_like(obj: &PyObjectRef) -> Option<String> {
    match &obj.payload {
        PyObjectPayload::Str(_) => Some(obj.py_to_string()),
        PyObjectPayload::Instance(inst) => {
            let next = inst.attrs.read().get("__builtin_value__").cloned();
            next.and_then(|value| extract_str_like(&value))
        }
        _ => None,
    }
}

pub(in crate::text_modules::regex_impl) fn extract_re_subject(
    obj: &PyObjectRef,
) -> PyResult<(String, bool)> {
    if let Some(bytes) = extract_bytes_like(obj) {
        return Ok((bytes_to_regex_text(&bytes), true));
    }
    if let Some(text) = extract_str_like(obj) {
        return Ok((text, false));
    }
    Err(PyException::type_error(
        "expected string or bytes-like object",
    ))
}

pub(in crate::text_modules::regex_impl) fn ensure_re_compatible(
    pattern_obj: &PyObjectRef,
    subject_is_bytes: bool,
) -> PyResult<()> {
    if re_pattern_is_bytes(pattern_obj) != subject_is_bytes {
        return Err(PyException::type_error(
            "cannot use a string pattern on a bytes-like object",
        ));
    }
    Ok(())
}

pub(in crate::text_modules::regex_impl) fn extract_re_replacement(
    obj: &PyObjectRef,
    subject_is_bytes: bool,
) -> PyResult<String> {
    if subject_is_bytes {
        if let Some(bytes) = extract_bytes_like(obj) {
            return Ok(bytes_to_regex_text(&bytes));
        }
        return Err(PyException::type_error(
            "sequence item must be bytes-like object",
        ));
    }
    if let Some(text) = extract_str_like(obj) {
        return Ok(text);
    }
    Err(PyException::type_error(
        "sequence item must be str instance",
    ))
}

/// Extract regex pattern string from either a str, bytes, or compiled Pattern.
/// For bytes, decodes as Latin-1 to preserve all byte values as chars.
pub(in crate::text_modules::regex_impl) fn extract_re_pattern(
    obj: &PyObjectRef,
) -> PyResult<String> {
    if let Some(pattern) = re_pattern_text_attr(obj) {
        return Ok(pattern);
    }
    if let Some(bytes) = extract_bytes_like(obj) {
        return Ok(bytes_to_regex_text(&bytes));
    }
    if let Some(text) = extract_str_like(obj) {
        return Ok(text);
    }
    match &obj.payload {
        _ => Err(PyException::type_error(
            "first argument must be string or compiled pattern",
        )),
    }
}

pub(in crate::text_modules::regex_impl) fn extract_re_pattern_and_flags(
    obj: &PyObjectRef,
    supplied_flags: i64,
) -> PyResult<(String, i64)> {
    let pattern = extract_re_pattern(obj)?;
    if is_re_pattern_object(obj) {
        if supplied_flags != 0 {
            return Err(PyException::value_error(
                "cannot process flags argument with a compiled pattern",
            ));
        }
        let flags = obj
            .get_attr("flags")
            .and_then(|f| f.to_int().ok())
            .unwrap_or(0);
        Ok((pattern, flags))
    } else {
        Ok((pattern, supplied_flags))
    }
}
