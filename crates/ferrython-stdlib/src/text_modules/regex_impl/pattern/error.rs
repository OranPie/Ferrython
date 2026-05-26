use super::*;

pub(in crate::text_modules::regex_impl) fn group_count_from_pattern_obj(
    obj: &PyObjectRef,
) -> usize {
    obj.get_attr("groups")
        .and_then(|v| v.to_int().ok())
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(0)
}

pub(in crate::text_modules::regex_impl) fn groupindex_contains(
    obj: &PyObjectRef,
    name: &str,
) -> bool {
    obj.get_attr("groupindex")
        .and_then(|groupindex| {
            if let PyObjectPayload::MappingProxy(map) | PyObjectPayload::Dict(map) =
                &groupindex.payload
            {
                let key = HashableKey::str_key(CompactString::from(name));
                Some(map.read().contains_key(&key))
            } else {
                None
            }
        })
        .unwrap_or(false)
}

pub(in crate::text_modules::regex_impl) fn re_error_with_pattern(
    message: impl Into<String>,
    pos: Option<usize>,
    pattern: Option<PyObjectRef>,
) -> PyException {
    let msg = message.into();
    let (lineno, colno) = if let (Some(pos), Some(pattern_obj)) = (pos, pattern.as_ref()) {
        let text = extract_re_pattern(pattern_obj).unwrap_or_else(|_| pattern_obj.py_to_string());
        let before = text.chars().take(pos).collect::<String>();
        let line = before.chars().filter(|&ch| ch == '\n').count() + 1;
        let col = before
            .rsplit_once('\n')
            .map(|(_, tail)| tail.chars().count() + 1)
            .unwrap_or_else(|| before.chars().count() + 1);
        (line, col)
    } else {
        (0, 0)
    };
    let display = match pos {
        Some(pos) if lineno > 1 => format!(
            "{} at position {} (line {}, column {})",
            msg, pos, lineno, colno
        ),
        Some(pos) => format!("{} at position {}", msg, pos),
        None => msg.clone(),
    };
    let mut attrs = ferrython_core::object::FxAttrMap::default();
    attrs.insert(
        CompactString::from("msg"),
        PyObject::str_val(CompactString::from(msg)),
    );
    attrs.insert(
        CompactString::from("pos"),
        pos.map(|p| PyObject::int(p as i64))
            .unwrap_or_else(PyObject::none),
    );
    attrs.insert(
        CompactString::from("pattern"),
        pattern.unwrap_or_else(PyObject::none),
    );
    attrs.insert(CompactString::from("lineno"), PyObject::int(lineno as i64));
    attrs.insert(CompactString::from("colno"), PyObject::int(colno as i64));
    let original = PyObject::wrap(PyObjectPayload::ExceptionInstance(
        std::mem::ManuallyDrop::new(Box::new(
            ferrython_core::object::ExceptionInstanceData::new_attrs(
                ExceptionKind::ReError,
                CompactString::from(display.clone()),
                vec![PyObject::str_val(CompactString::from(display.clone()))],
                Some(Rc::new(PyCell::new(attrs))),
            ),
        )),
    ));
    PyException::with_original(
        ExceptionKind::ReError,
        CompactString::from(display),
        original,
    )
}

pub(in crate::text_modules::regex_impl) fn re_error(
    message: impl Into<String>,
    pos: Option<usize>,
) -> PyException {
    re_error_with_pattern(message, pos, None)
}

pub(in crate::text_modules::regex_impl) fn re_pattern_error(
    message: impl Into<String>,
    pos: Option<usize>,
    pattern_obj: &PyObjectRef,
) -> PyException {
    re_error_with_pattern(message, pos, Some(pattern_obj.clone()))
}

pub(in crate::text_modules::regex_impl) fn parse_decimal_limited(
    chars: &[char],
    limit: u64,
) -> Result<u64, ()> {
    let mut value = 0_u64;
    for &ch in chars {
        let digit = ch.to_digit(10).ok_or(())? as u64;
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(digit))
            .ok_or(())?;
        if value >= limit {
            return Err(());
        }
    }
    Ok(value)
}
