use super::*;
use crate::text_modules::regex_impl::functions::simple::is_simple_dot_repeat_pattern;

pub(in crate::text_modules::regex_impl) fn re_compile(
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("re.compile() requires a pattern"));
    }
    let supplied_flags = if args.len() > 1 {
        args[1].to_int().unwrap_or(0)
    } else {
        0
    };
    if is_re_pattern_object(&args[0]) {
        if supplied_flags != 0 {
            return Err(PyException::value_error(
                "cannot process flags argument with a compiled pattern",
            ));
        }
        return Ok(args[0].clone());
    }
    let original_pattern = extract_re_pattern(&args[0])?;
    let is_bytes = re_pattern_is_bytes(&args[0]);
    let original_pattern_obj = if is_bytes {
        PyObject::bytes(
            extract_bytes_like(&args[0]).unwrap_or_else(|| regex_text_to_bytes(&original_pattern)),
        )
    } else {
        PyObject::str_val(CompactString::from(original_pattern.clone()))
    };
    warn_nonleading_flags(&original_pattern, &original_pattern_obj)?;
    let (pattern, extra_flags, _) = strip_nonleading_global_flags(&original_pattern);
    let supplied_flags = supplied_flags | extra_flags;
    let pattern_obj = if is_bytes {
        PyObject::bytes(
            extract_bytes_like(&args[0]).unwrap_or_else(|| regex_text_to_bytes(&pattern)),
        )
    } else {
        PyObject::str_val(CompactString::from(pattern.clone()))
    };
    let inline_flags = leading_inline_flags(&pattern);
    let inline_ascii = inline_flags & RE_FLAG_ASCII != 0;
    let inline_unicode = inline_flags & RE_FLAG_UNICODE != 0;
    let inline_locale = inline_flags & RE_FLAG_LOCALE != 0;
    if (inline_ascii && inline_unicode)
        || (inline_ascii && inline_locale)
        || (inline_unicode && inline_locale)
    {
        return Err(re_pattern_error("bad inline flags", Some(2), &pattern_obj));
    }
    if is_bytes && supplied_flags & RE_FLAG_UNICODE != 0 {
        return Err(PyException::value_error(
            "cannot use UNICODE flag with a bytes pattern",
        ));
    }
    if is_bytes && inline_unicode {
        return Err(re_pattern_error("bad inline flags", Some(2), &pattern_obj));
    }
    if is_bytes
        && (supplied_flags | inline_flags) & RE_FLAG_LOCALE != 0
        && (supplied_flags | inline_flags) & RE_FLAG_ASCII != 0
    {
        return Err(PyException::value_error(
            "ASCII and LOCALE flags are incompatible",
        ));
    }
    if !is_bytes && supplied_flags & RE_FLAG_LOCALE != 0 {
        return Err(PyException::value_error(
            "cannot use LOCALE flag with a str pattern",
        ));
    }
    if !is_bytes && inline_locale {
        return Err(re_pattern_error("bad inline flags", Some(2), &pattern_obj));
    }
    if !is_bytes && supplied_flags & RE_FLAG_ASCII != 0 && supplied_flags & RE_FLAG_UNICODE != 0 {
        return Err(PyException::value_error(
            "ASCII and UNICODE flags are incompatible",
        ));
    }
    if !is_bytes
        && ((supplied_flags & RE_FLAG_ASCII != 0 && inline_unicode)
            || (supplied_flags & RE_FLAG_UNICODE != 0 && inline_ascii))
    {
        return Err(PyException::value_error(
            "ASCII and UNICODE flags are incompatible",
        ));
    }
    let flags = effective_re_flags(&pattern, supplied_flags, is_bytes);
    let engine_flags = regex_engine_flags(flags, is_bytes);
    validate_re_pattern_syntax(&pattern, is_bytes, &pattern_obj)?;
    if flags & 128 != 0 {
        write_re_debug_output(&pattern)?;
    }
    let simple_dot_repeat = is_simple_dot_repeat_pattern(split_leading_inline_flags(&pattern).0)?;
    // Validate the pattern compiles (try fancy if needed)
    let compile_result = if simple_dot_repeat {
        Ok(())
    } else if needs_fancy_regex_with_flags(&pattern, engine_flags) {
        build_fancy_regex(&pattern, engine_flags).map(|_| ())
    } else {
        build_regex(&pattern, engine_flags).map(|_| ())
    };
    if let Err(exc) = compile_result {
        if matches!(exc.kind, ExceptionKind::RuntimeError) {
            let msg = exc
                .message
                .strip_prefix("re: ")
                .unwrap_or(exc.message.as_str())
                .to_string();
            return Err(re_pattern_error(msg, None, &pattern_obj));
        }
        return Err(exc);
    }
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("__re_pattern__"),
        PyObject::bool_val(true),
    );
    attrs.insert(CompactString::from("pattern"), pattern_obj);
    attrs.insert(
        CompactString::from("_pattern_text"),
        PyObject::str_val(CompactString::from(pattern.clone())),
    );
    attrs.insert(
        CompactString::from("_pattern_is_bytes"),
        PyObject::bool_val(is_bytes),
    );
    attrs.insert(CompactString::from("flags"), PyObject::int(flags));
    // groups/groupindex: best-effort for standard regex
    if simple_dot_repeat {
        attrs.insert(
            CompactString::from("groupindex"),
            readonly_mapping(new_fx_hashkey_map()),
        );
        attrs.insert(CompactString::from("groups"), PyObject::int(0));
    } else if !needs_fancy_regex_with_flags(&pattern, engine_flags) {
        if let Ok(re_obj) = build_regex(&pattern, engine_flags) {
            let group_count = re_obj.captures_len() - 1;
            let mut groupindex_map = new_fx_hashkey_map();
            for name in re_obj.capture_names().flatten() {
                if let Some(idx) = re_obj
                    .capture_names()
                    .enumerate()
                    .find(|(_, n)| n.as_deref() == Some(name))
                    .map(|(i, _)| i)
                {
                    groupindex_map.insert(
                        HashableKey::str_key(CompactString::from(name)),
                        PyObject::int(idx as i64),
                    );
                }
            }
            attrs.insert(
                CompactString::from("groupindex"),
                readonly_mapping(groupindex_map),
            );
            attrs.insert(
                CompactString::from("groups"),
                PyObject::int(group_count as i64),
            );
        }
    } else {
        attrs.insert(
            CompactString::from("groupindex"),
            readonly_mapping(new_fx_hashkey_map()),
        );
        attrs.insert(CompactString::from("groups"), PyObject::int(0));
    }
    Ok(PyObject::instance_with_attrs(re_pattern_class(), attrs))
}
