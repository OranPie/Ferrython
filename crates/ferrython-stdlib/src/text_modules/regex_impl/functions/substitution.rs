use super::*;

pub(super) fn dollar_match_offsets(pattern: &str, text: &str, flags: i64) -> Option<Vec<usize>> {
    let (body, inline_flags) = split_leading_inline_flags(pattern);
    if body != "$" {
        return None;
    }
    let effective_flags = flags | inline_flags;
    let mut offsets = Vec::new();
    if effective_flags & RE_FLAG_MULTILINE != 0 {
        for (idx, ch) in text.char_indices() {
            if ch == '\n' {
                offsets.push(idx);
            }
        }
        offsets.push(text.len());
    } else {
        if text.ends_with('\n') {
            offsets.push(text.len() - 1);
        }
        offsets.push(text.len());
    }
    Some(offsets)
}

pub(in crate::text_modules::regex_impl) fn re_sub_plain_offsets(
    text: &str,
    repl: &str,
    count: usize,
    offsets: &[usize],
    subject_is_bytes: bool,
) -> (PyObjectRef, usize) {
    let limit = if count == 0 {
        offsets.len()
    } else {
        count.min(offsets.len())
    };
    let mut result = String::with_capacity(text.len() + repl.len().saturating_mul(limit));
    let mut last = 0;
    for &offset in offsets.iter().take(limit) {
        result.push_str(&text[last..offset]);
        result.push_str(repl);
        last = offset;
    }
    result.push_str(&text[last..]);
    (py_re_text(&result, subject_is_bytes), limit)
}

pub(in crate::text_modules::regex_impl) fn re_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error(
            "re.sub() requires pattern, repl, and string",
        ));
    }
    let repl_obj = &args[1];
    let (text, subject_is_bytes) = extract_re_subject(&args[2])?;
    // count and flags can be positional or in trailing kwargs dict
    let mut count = if args.len() > 3 && !matches!(&args[3].payload, PyObjectPayload::Dict(_)) {
        args[3].to_int().unwrap_or(0) as usize
    } else {
        0
    };
    let mut flags = if args.len() > 4 && !matches!(&args[4].payload, PyObjectPayload::Dict(_)) {
        args[4].to_int().unwrap_or(0)
    } else {
        0
    };
    // Check for trailing kwargs dict
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(map) = &last.payload {
            let map_r = map.read();
            for (k, v) in map_r.iter() {
                if let HashableKey::Str(s) = k {
                    match s.as_str() {
                        "count" => count = v.to_int().unwrap_or(0) as usize,
                        "flags" => flags = v.to_int().unwrap_or(0),
                        _ => {}
                    }
                }
            }
        }
    }
    ensure_re_compatible(&args[0], subject_is_bytes)?;
    let (pattern, flags) = extract_re_pattern_and_flags(&args[0], flags)?;
    let engine_flags = regex_engine_flags(flags, subject_is_bytes);
    // Check if repl is callable
    let repl_is_callable = matches!(
        &repl_obj.payload,
        PyObjectPayload::Function(_)
            | PyObjectPayload::NativeFunction(_)
            | PyObjectPayload::NativeClosure(_)
            | PyObjectPayload::BoundMethod { .. }
    );
    if repl_is_callable {
        return re_sub_callable(
            &pattern,
            repl_obj,
            &text,
            count,
            engine_flags,
            subject_is_bytes,
        );
    }
    let repl = extract_re_replacement(repl_obj, subject_is_bytes)?;
    validate_replacement_for_pattern(&args[0], flags, &repl)?;
    if !repl.contains('\\') {
        if let Some(offsets) = dollar_match_offsets(&pattern, &text, flags) {
            return Ok(re_sub_plain_offsets(&text, &repl, count, &offsets, subject_is_bytes).0);
        }
    }
    let rust_repl = python_repl_to_rust(&repl);
    if needs_fancy_regex_with_flags(&pattern, engine_flags) {
        let re = build_fancy_regex(&pattern, engine_flags)?;
        let mut result = String::new();
        let mut last = 0;
        let mut n = 0;
        let mut pos = 0;
        while pos <= text.len() {
            if count > 0 && n >= count {
                break;
            }
            match re.find(&text[pos..]) {
                Ok(Some(m)) => {
                    if m.start() == m.end() {
                        pos += 1;
                        continue;
                    }
                    let abs_start = pos + m.start();
                    let abs_end = pos + m.end();
                    result.push_str(&text[last..abs_start]);
                    result.push_str(&rust_repl);
                    last = abs_end;
                    pos = abs_end;
                    n += 1;
                }
                _ => break,
            }
        }
        result.push_str(&text[last..]);
        Ok(py_re_text(&result, subject_is_bytes))
    } else {
        let re = build_regex(&pattern, engine_flags)?;
        let result = if count == 0 {
            re.replace_all(&text, rust_repl.as_str()).to_string()
        } else {
            re.replacen(&text, count, rust_repl.as_str()).to_string()
        };
        Ok(py_re_text(&result, subject_is_bytes))
    }
}

/// re.sub with a callable replacement function
pub(in crate::text_modules::regex_impl) fn re_sub_callable(
    pattern: &str,
    repl_fn: &PyObjectRef,
    text: &str,
    count: usize,
    flags: i64,
    is_bytes: bool,
) -> PyResult<PyObjectRef> {
    if needs_fancy_regex_with_flags(pattern, flags) {
        let re = build_fancy_regex(pattern, flags)?;
        let mut result = String::new();
        let mut last = 0;
        let mut n = 0;
        let mut pos = 0;
        while pos <= text.len() {
            if count > 0 && n >= count {
                break;
            }
            match re.captures(&text[pos..]) {
                Ok(Some(caps)) => {
                    let whole = caps.get(0).unwrap();
                    if whole.start() == whole.end() {
                        pos += 1;
                        continue;
                    }
                    let abs_start = pos + whole.start();
                    let abs_end = pos + whole.end();
                    result.push_str(&text[last..abs_start]);
                    let groups: Vec<Option<String>> = (1..caps.len())
                        .map(|i| caps.get(i).map(|m| m.as_str().to_string()))
                        .collect();
                    let match_obj = make_fancy_match_object(
                        text,
                        abs_start,
                        abs_end,
                        whole.as_str(),
                        groups,
                        extract_fancy_group_names(&re),
                        is_bytes,
                    );
                    let replacement = ferrython_core::object::call_callable(repl_fn, &[match_obj])?;
                    result.push_str(&extract_re_replacement(&replacement, is_bytes)?);
                    last = abs_end;
                    pos = abs_end;
                    n += 1;
                }
                _ => break,
            }
        }
        result.push_str(&text[last..]);
        Ok(py_re_text(&result, is_bytes))
    } else {
        let re = build_regex(pattern, flags)?;
        let mut result = String::new();
        let mut last = 0;
        let mut n = 0;
        for caps in re.captures_iter(text) {
            if count > 0 && n >= count {
                break;
            }
            let whole = caps.get(0).unwrap();
            result.push_str(&text[last..whole.start()]);
            let match_obj = make_match_object_from_captures(&caps, text, &re, is_bytes);
            let replacement = ferrython_core::object::call_callable(repl_fn, &[match_obj])?;
            result.push_str(&extract_re_replacement(&replacement, is_bytes)?);
            last = whole.end();
            n += 1;
        }
        result.push_str(&text[last..]);
        Ok(py_re_text(&result, is_bytes))
    }
}

pub(in crate::text_modules::regex_impl) fn re_subn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error(
            "re.subn() requires pattern, repl, and string",
        ));
    }
    let (text, subject_is_bytes) = extract_re_subject(&args[2])?;
    let mut count = if args.len() > 3 && !matches!(&args[3].payload, PyObjectPayload::Dict(_)) {
        args[3].to_int().unwrap_or(0) as usize
    } else {
        0
    };
    let mut flags = if args.len() > 4 && !matches!(&args[4].payload, PyObjectPayload::Dict(_)) {
        args[4].to_int().unwrap_or(0)
    } else {
        0
    };
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(map) = &last.payload {
            let map_r = map.read();
            for (k, v) in map_r.iter() {
                if let HashableKey::Str(s) = k {
                    match s.as_str() {
                        "count" => count = v.to_int().unwrap_or(0) as usize,
                        "flags" => flags = v.to_int().unwrap_or(0),
                        _ => {}
                    }
                }
            }
        }
    }
    ensure_re_compatible(&args[0], subject_is_bytes)?;
    let (pattern, flags) = extract_re_pattern_and_flags(&args[0], flags)?;
    let engine_flags = regex_engine_flags(flags, subject_is_bytes);
    let repl = extract_re_replacement(&args[1], subject_is_bytes)?;
    validate_replacement_for_pattern(&args[0], flags, &repl)?;
    if !repl.contains('\\') {
        if let Some(offsets) = dollar_match_offsets(&pattern, &text, flags) {
            let (result, replacements) =
                re_sub_plain_offsets(&text, &repl, count, &offsets, subject_is_bytes);
            return Ok(PyObject::tuple(vec![
                result,
                PyObject::int(replacements as i64),
            ]));
        }
    }
    let rust_repl = python_repl_to_rust(&repl);
    if needs_fancy_regex_with_flags(&pattern, engine_flags) {
        let re = build_fancy_regex(&pattern, engine_flags)?;
        let mut result = String::new();
        let mut last = 0;
        let mut n = 0;
        let mut pos = 0;
        while pos <= text.len() {
            if count > 0 && n >= count {
                break;
            }
            match re.find(&text[pos..]) {
                Ok(Some(m)) => {
                    if m.start() == m.end() {
                        pos += 1;
                        continue;
                    }
                    let abs_start = pos + m.start();
                    let abs_end = pos + m.end();
                    result.push_str(&text[last..abs_start]);
                    result.push_str(&rust_repl);
                    last = abs_end;
                    pos = abs_end;
                    n += 1;
                }
                _ => break,
            }
        }
        result.push_str(&text[last..]);
        Ok(PyObject::tuple(vec![
            py_re_text(&result, subject_is_bytes),
            PyObject::int(n as i64),
        ]))
    } else {
        let re = build_regex(&pattern, engine_flags)?;
        let found = re.find_iter(&text).count();
        let replacements = if count == 0 { found } else { found.min(count) };
        let result = if count == 0 {
            re.replace_all(&text, rust_repl.as_str()).to_string()
        } else {
            re.replacen(&text, count, rust_repl.as_str()).to_string()
        };
        Ok(PyObject::tuple(vec![
            py_re_text(&result, subject_is_bytes),
            PyObject::int(replacements as i64),
        ]))
    }
}

pub(in crate::text_modules::regex_impl) fn re_split(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "re.split() requires pattern and string",
        ));
    }
    let (text, subject_is_bytes) = extract_re_subject(&args[1])?;
    let maxsplit = if args.len() > 2 {
        args[2].to_int().unwrap_or(0) as usize
    } else {
        0
    };
    let supplied_flags = if args.len() > 3 {
        args[3].to_int().unwrap_or(0)
    } else {
        0
    };
    ensure_re_compatible(&args[0], subject_is_bytes)?;
    let (pattern, flags) = extract_re_pattern_and_flags(&args[0], supplied_flags)?;
    let engine_flags = regex_engine_flags(flags, subject_is_bytes);
    if needs_fancy_regex_with_flags(&pattern, engine_flags) {
        let re = build_fancy_regex(&pattern, engine_flags)?;
        let mut result = Vec::new();
        let mut last = 0;
        let mut splits = 0;
        let mut pos = 0;
        while pos <= text.len() {
            if maxsplit > 0 && splits >= maxsplit {
                break;
            }
            match re.find(&text[pos..]) {
                Ok(Some(m)) => {
                    if m.start() == m.end() {
                        pos += 1;
                        continue;
                    }
                    let abs_start = pos + m.start();
                    let abs_end = pos + m.end();
                    result.push(py_re_text(&text[last..abs_start], subject_is_bytes));
                    last = abs_end;
                    pos = abs_end;
                    splits += 1;
                }
                _ => break,
            }
        }
        result.push(py_re_text(&text[last..], subject_is_bytes));
        Ok(PyObject::list(result))
    } else {
        let re = build_regex(&pattern, engine_flags)?;
        let num_groups = re.captures_len() - 1;

        let parts: Vec<PyObjectRef> = if num_groups == 0 {
            // No capturing groups: use simple split
            if maxsplit == 0 {
                re.split(&text)
                    .map(|s| py_re_text(s, subject_is_bytes))
                    .collect()
            } else {
                re.splitn(&text, maxsplit + 1)
                    .map(|s| py_re_text(s, subject_is_bytes))
                    .collect()
            }
        } else {
            // Capturing groups: include captured text in result (CPython behavior)
            let mut result = Vec::new();
            let mut last = 0;
            let mut splits = 0;
            for caps in re.captures_iter(&text) {
                if maxsplit > 0 && splits >= maxsplit {
                    break;
                }
                let whole = caps.get(0).unwrap();
                // Text before the match
                result.push(py_re_text(&text[last..whole.start()], subject_is_bytes));
                // Each capturing group
                for i in 1..=num_groups {
                    match caps.get(i) {
                        Some(m) => result.push(py_re_text(m.as_str(), subject_is_bytes)),
                        None => result.push(PyObject::none()),
                    }
                }
                last = whole.end();
                splits += 1;
            }
            // Remaining text after last match
            result.push(py_re_text(&text[last..], subject_is_bytes));
            result
        };
        Ok(PyObject::list(parts))
    }
}
