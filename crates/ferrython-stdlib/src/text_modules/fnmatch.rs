use super::*;

fn ensure_same_text_kind(name: &PyObjectRef, pattern: &PyObjectRef) -> PyResult<()> {
    let name_is_bytes = matches!(
        &name.payload,
        PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_)
    );
    let pattern_is_bytes = matches!(
        &pattern.payload,
        PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_)
    );
    if name_is_bytes != pattern_is_bytes {
        return Err(PyException::type_error(
            "cannot use a string pattern on a bytes-like object",
        ));
    }
    Ok(())
}

fn pattern_to_string(obj: &PyObjectRef) -> String {
    match &obj.payload {
        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => {
            b.iter().map(|&byte| byte as char).collect()
        }
        _ => obj.py_to_string(),
    }
}

fn maybe_lowercase_for_normcase(value: String) -> String {
    if cfg!(windows) {
        value.replace('\\', "/").to_ascii_lowercase()
    } else {
        value
    }
}

pub fn create_fnmatch_module() -> PyObjectRef {
    make_module(
        "fnmatch",
        vec![
            (
                "fnmatch",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("fnmatch requires name and pattern"));
                    }
                    ensure_same_text_kind(&args[0], &args[1])?;
                    let name = maybe_lowercase_for_normcase(pattern_to_string(&args[0]));
                    let pattern = maybe_lowercase_for_normcase(pattern_to_string(&args[1]));
                    Ok(PyObject::bool_val(glob_match(&pattern, &name)))
                }),
            ),
            (
                "fnmatchcase",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error(
                            "fnmatchcase requires name and pattern",
                        ));
                    }
                    ensure_same_text_kind(&args[0], &args[1])?;
                    let name = pattern_to_string(&args[0]);
                    let pattern = pattern_to_string(&args[1]);
                    Ok(PyObject::bool_val(glob_match(&pattern, &name)))
                }),
            ),
            (
                "filter",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("filter requires names and pattern"));
                    }
                    let names = args[0].to_list()?;
                    let pattern_is_bytes = matches!(
                        &args[1].payload,
                        PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_)
                    );
                    for name in &names {
                        let name_is_bytes = matches!(
                            &name.payload,
                            PyObjectPayload::Bytes(_) | PyObjectPayload::ByteArray(_)
                        );
                        if name_is_bytes != pattern_is_bytes {
                            return Err(PyException::type_error(
                                "cannot use a string pattern on a bytes-like object",
                            ));
                        }
                    }
                    let pattern = maybe_lowercase_for_normcase(pattern_to_string(&args[1]));
                    let filtered: Vec<PyObjectRef> = names
                        .iter()
                        .filter(|n| {
                            glob_match(
                                &pattern,
                                &maybe_lowercase_for_normcase(pattern_to_string(n)),
                            )
                        })
                        .cloned()
                        .collect();
                    Ok(PyObject::list(filtered))
                }),
            ),
            (
                "translate",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("translate requires a pattern"));
                    }
                    let pat = pattern_to_string(&args[0]);
                    let mut res = String::from("(?s:");
                    let chars: Vec<char> = pat.chars().collect();
                    let mut i = 0;
                    while i < chars.len() {
                        let c = chars[i];
                        match c {
                            '*' => res.push_str(".*"),
                            '?' => res.push('.'),
                            '[' => {
                                let mut j = i + 1;
                                if j < chars.len() && chars[j] == '!' {
                                    j += 1;
                                }
                                if j < chars.len() && chars[j] == ']' {
                                    j += 1;
                                }
                                while j < chars.len() && chars[j] != ']' {
                                    j += 1;
                                }
                                if j >= chars.len() {
                                    res.push_str("\\[");
                                } else {
                                    let mut stuff = String::new();
                                    for &ch in &chars[i + 1..j] {
                                        stuff.push(ch);
                                    }
                                    stuff = stuff.replace("\\", "\\\\");
                                    let mut bracket = String::from("[");
                                    if stuff.starts_with('!') {
                                        bracket.push('^');
                                        bracket.push_str(&stuff[1..]);
                                    } else {
                                        if stuff.starts_with('^') {
                                            bracket.push('\\');
                                        }
                                        bracket.push_str(&stuff);
                                    }
                                    bracket.push(']');
                                    res.push_str(&bracket);
                                    i = j;
                                }
                            }
                            _ => {
                                // Escape regex special characters
                                if "(){}+.^$|\\".contains(c) {
                                    res.push('\\');
                                }
                                res.push(c);
                            }
                        }
                        i += 1;
                    }
                    res.push_str(r")\Z");
                    Ok(PyObject::str_val(CompactString::from(res)))
                }),
            ),
        ],
    )
}
