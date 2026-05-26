use super::*;

pub(super) fn re_pattern_class() -> PyObjectRef {
    static RE_PATTERN_CLASS: OnceLock<PyObjectRef> = OnceLock::new();
    RE_PATTERN_CLASS
        .get_or_init(|| {
            let mut ns = IndexMap::new();
            ns.insert(
                CompactString::from("__module__"),
                PyObject::str_val(CompactString::from("re")),
            );
            ns.insert(
                CompactString::from("match"),
                PyObject::native_function("Pattern.match", compiled_match),
            );
            ns.insert(
                CompactString::from("search"),
                PyObject::native_function("Pattern.search", compiled_search),
            );
            ns.insert(
                CompactString::from("findall"),
                PyObject::native_function("Pattern.findall", compiled_findall),
            );
            ns.insert(
                CompactString::from("finditer"),
                PyObject::native_function("Pattern.finditer", compiled_finditer),
            );
            ns.insert(
                CompactString::from("sub"),
                PyObject::native_function("Pattern.sub", compiled_sub),
            );
            ns.insert(
                CompactString::from("split"),
                PyObject::native_function("Pattern.split", compiled_split),
            );
            ns.insert(
                CompactString::from("fullmatch"),
                PyObject::native_function("Pattern.fullmatch", compiled_fullmatch),
            );
            ns.insert(
                CompactString::from("subn"),
                PyObject::native_function("Pattern.subn", compiled_subn),
            );
            ns.insert(
                CompactString::from("scanner"),
                PyObject::native_function("Pattern.scanner", compiled_scanner),
            );
            ns.insert(
                CompactString::from("__repr__"),
                PyObject::native_closure("Pattern.__repr__", pattern_repr),
            );
            ns.insert(
                CompactString::from("__hash__"),
                PyObject::native_function("Pattern.__hash__", pattern_hash),
            );
            ns.insert(
                CompactString::from("__eq__"),
                PyObject::native_function("Pattern.__eq__", pattern_eq),
            );
            ns.insert(
                CompactString::from("__copy__"),
                PyObject::native_function("Pattern.__copy__", return_self),
            );
            ns.insert(
                CompactString::from("__deepcopy__"),
                PyObject::native_function("Pattern.__deepcopy__", return_self),
            );
            for name in ["__lt__", "__le__", "__gt__", "__ge__"] {
                ns.insert(
                    CompactString::from(name),
                    PyObject::native_function("Pattern.order", pattern_order_error),
                );
            }
            PyObject::class(CompactString::from("Pattern"), vec![], ns)
        })
        .clone()
}

fn return_self(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    args.first()
        .cloned()
        .ok_or_else(|| PyException::type_error("missing self"))
}

pub(super) fn re_scanner_class() -> PyObjectRef {
    static RE_SCANNER_CLASS: OnceLock<PyObjectRef> = OnceLock::new();
    RE_SCANNER_CLASS
        .get_or_init(|| {
            let mut ns = IndexMap::new();
            ns.insert(
                CompactString::from("__module__"),
                PyObject::str_val(CompactString::from("_sre")),
            );
            ns.insert(
                CompactString::from("match"),
                PyObject::native_function("Scanner.match", scanner_match),
            );
            ns.insert(
                CompactString::from("search"),
                PyObject::native_function("Scanner.search", scanner_search),
            );
            PyObject::class(CompactString::from("SRE_Scanner"), vec![], ns)
        })
        .clone()
}

fn regex_flag_class() -> PyObjectRef {
    static REGEX_FLAG_CLASS: OnceLock<PyObjectRef> = OnceLock::new();
    REGEX_FLAG_CLASS
        .get_or_init(|| {
            let mut ns = IndexMap::new();
            ns.insert(
                CompactString::from("__module__"),
                PyObject::str_val(CompactString::from("re")),
            );
            ns.insert(
                CompactString::from("__repr__"),
                PyObject::native_function("RegexFlag.__repr__", regex_flag_repr_method),
            );
            ns.insert(
                CompactString::from("__str__"),
                PyObject::native_function("RegexFlag.__str__", regex_flag_repr_method),
            );
            ns.insert(
                CompactString::from("__int__"),
                PyObject::native_function("RegexFlag.__int__", regex_flag_int_method),
            );
            ns.insert(
                CompactString::from("__index__"),
                PyObject::native_function("RegexFlag.__index__", regex_flag_int_method),
            );
            ns.insert(
                CompactString::from("__or__"),
                PyObject::native_function("RegexFlag.__or__", regex_flag_or_method),
            );
            ns.insert(
                CompactString::from("__ror__"),
                PyObject::native_function("RegexFlag.__ror__", regex_flag_or_method),
            );
            ns.insert(
                CompactString::from("__and__"),
                PyObject::native_function("RegexFlag.__and__", regex_flag_and_method),
            );
            ns.insert(
                CompactString::from("__rand__"),
                PyObject::native_function("RegexFlag.__rand__", regex_flag_and_method),
            );
            ns.insert(
                CompactString::from("__xor__"),
                PyObject::native_function("RegexFlag.__xor__", regex_flag_xor_method),
            );
            ns.insert(
                CompactString::from("__rxor__"),
                PyObject::native_function("RegexFlag.__rxor__", regex_flag_xor_method),
            );
            ns.insert(
                CompactString::from("__invert__"),
                PyObject::native_function("RegexFlag.__invert__", regex_flag_invert_method),
            );
            PyObject::class(
                CompactString::from("RegexFlag"),
                vec![PyObject::builtin_type(CompactString::from("int"))],
                ns,
            )
        })
        .clone()
}

fn regex_flag_int(obj: &PyObjectRef) -> Option<i64> {
    match &obj.payload {
        PyObjectPayload::Int(_) | PyObjectPayload::Bool(_) => obj.to_int().ok(),
        PyObjectPayload::Instance(inst) => inst
            .attrs
            .read()
            .get("__re_flag_value__")
            .and_then(|v| v.to_int().ok())
            .or_else(|| {
                inst.attrs
                    .read()
                    .get("__builtin_value__")
                    .and_then(|v| v.to_int().ok())
            }),
        _ => None,
    }
}

pub(super) fn regex_flag_obj(value: i64) -> PyObjectRef {
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("__re_flag_value__"),
        PyObject::int(value),
    );
    attrs.insert(
        CompactString::from("__builtin_value__"),
        PyObject::int(value),
    );
    PyObject::instance_with_attrs(regex_flag_class(), attrs)
}

fn regex_flag_repr_text(value: i64) -> String {
    if value < 0 {
        let inverted = !value;
        if let Some(inner) = re_flag_repr(inverted, true) {
            if inner.contains('|') {
                format!("~({})", inner)
            } else {
                format!("~{}", inner)
            }
        } else {
            format!("{}", value)
        }
    } else {
        re_flag_repr(value, true).unwrap_or_else(|| format!("{}", value))
    }
}

fn regex_flag_repr_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("RegexFlag.__repr__ requires self"));
    }
    let value = regex_flag_int(&args[0]).unwrap_or(0);
    Ok(PyObject::str_val(CompactString::from(
        regex_flag_repr_text(value),
    )))
}

fn regex_flag_int_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("RegexFlag.__int__ requires self"));
    }
    Ok(PyObject::int(regex_flag_int(&args[0]).unwrap_or(0)))
}

fn regex_flag_or_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("RegexFlag.__or__ requires other"));
    }
    Ok(regex_flag_obj(
        regex_flag_int(&args[0]).unwrap_or(0) | regex_flag_int(&args[1]).unwrap_or(0),
    ))
}

fn regex_flag_and_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("RegexFlag.__and__ requires other"));
    }
    Ok(regex_flag_obj(
        regex_flag_int(&args[0]).unwrap_or(0) & regex_flag_int(&args[1]).unwrap_or(0),
    ))
}

fn regex_flag_xor_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("RegexFlag.__xor__ requires other"));
    }
    Ok(regex_flag_obj(
        regex_flag_int(&args[0]).unwrap_or(0) ^ regex_flag_int(&args[1]).unwrap_or(0),
    ))
}

fn regex_flag_invert_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "RegexFlag.__invert__ requires self",
        ));
    }
    Ok(regex_flag_obj(!regex_flag_int(&args[0]).unwrap_or(0)))
}

fn shorten_pattern_repr(repr: String) -> String {
    const BODY_LIMIT: usize = 220;
    if repr.chars().count() <= BODY_LIMIT + 4 {
        return repr;
    }
    let (prefix, suffix_len) = if repr.starts_with("b'") && repr.ends_with('\'') {
        ("b'", 1)
    } else if repr.starts_with("b\"") && repr.ends_with('"') {
        ("b\"", 1)
    } else if repr.starts_with('\'') && repr.ends_with('\'') {
        ("'", 1)
    } else if repr.starts_with('"') && repr.ends_with('"') {
        ("\"", 1)
    } else {
        let mut s: String = repr.chars().take(BODY_LIMIT).collect();
        s.push_str("...");
        return s;
    };
    let body_start = prefix.len();
    let body_end = repr.len().saturating_sub(suffix_len);
    let body = &repr[body_start..body_end];
    let short_body: String = body.chars().take(BODY_LIMIT).collect();
    format!("{}{}...{}", prefix, short_body, &repr[body_end..])
}

fn compiled_pattern_text(self_obj: &PyObjectRef) -> PyResult<String> {
    if let Some(text) = re_pattern_text_attr(self_obj) {
        return Ok(text);
    }
    let pattern_obj = self_obj
        .get_attr("pattern")
        .ok_or(PyException::attribute_error("pattern"))?;
    extract_re_pattern(&pattern_obj)
}

fn compiled_pattern_flags(self_obj: &PyObjectRef) -> i64 {
    self_obj
        .get_attr("flags")
        .and_then(|f| f.to_int().ok())
        .unwrap_or(0)
}

fn pattern_repr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("Pattern.__repr__ requires self"));
    }
    let self_obj = &args[0];
    let pattern_obj = self_obj
        .get_attr("pattern")
        .unwrap_or_else(|| PyObject::str_val(CompactString::from("")));
    let pat_repr = shorten_pattern_repr(pattern_obj.repr());
    let flags = compiled_pattern_flags(self_obj);
    let is_bytes = re_pattern_is_bytes(self_obj);
    let result = if let Some(flag_repr) = re_flag_repr(flags, is_bytes) {
        format!("re.compile({}, {})", pat_repr, flag_repr)
    } else {
        format!("re.compile({})", pat_repr)
    };
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn pattern_hash(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("Pattern.__hash__ requires self"));
    }
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let self_obj = &args[0];
    let mut hasher = DefaultHasher::new();
    compiled_pattern_text(self_obj)?.hash(&mut hasher);
    compiled_pattern_flags(self_obj).hash(&mut hasher);
    re_pattern_is_bytes(self_obj).hash(&mut hasher);
    Ok(PyObject::int(hasher.finish() as i64))
}

fn pattern_eq(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Ok(PyObject::bool_val(false));
    }
    if !is_re_pattern_object(&args[1]) {
        return Ok(PyObject::bool_val(false));
    }
    let left = &args[0];
    let right = &args[1];
    Ok(PyObject::bool_val(
        compiled_pattern_text(left)? == compiled_pattern_text(right)?
            && compiled_pattern_flags(left) == compiled_pattern_flags(right)
            && re_pattern_is_bytes(left) == re_pattern_is_bytes(right),
    ))
}

fn pattern_order_error(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Err(PyException::type_error(
        "'<' not supported between instances of 're.Pattern' and 're.Pattern'",
    ))
}
