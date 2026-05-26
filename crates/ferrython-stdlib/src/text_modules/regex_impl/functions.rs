use super::*;

#[derive(Clone, Copy)]
struct SimpleDotRepeat {
    min: u64,
    max: Option<u64>,
    lazy: bool,
    exact: bool,
}

fn parse_simple_dot_repeat(pattern: &str) -> PyResult<Option<SimpleDotRepeat>> {
    let bytes = pattern.as_bytes();
    if bytes.len() < 4 || bytes[0] != b'.' || bytes[1] != b'{' {
        return Ok(None);
    }
    let Some(close_rel) = bytes[2..].iter().position(|&byte| byte == b'}') else {
        return Ok(None);
    };
    let close = close_rel + 2;
    let lazy = matches!(bytes.get(close + 1), Some(b'?'));
    let expected_len = close + 1 + usize::from(lazy);
    if expected_len != bytes.len() {
        return Ok(None);
    }

    let body = &bytes[2..close];
    let comma = body.iter().position(|&byte| byte == b',');
    if let Some(pos) = comma {
        if body[pos + 1..].contains(&b',') {
            return Ok(None);
        }
    }

    let limit = u32::MAX as u64;
    let (min, max, exact) = match comma {
        Some(pos) => {
            let left = &body[..pos];
            let right = &body[pos + 1..];
            if left.is_empty() && right.is_empty() {
                return Ok(None);
            }
            let min = if left.is_empty() {
                0
            } else {
                let Some(value) = parse_decimal_bytes_limited(left, limit)? else {
                    return Ok(None);
                };
                value
            };
            let max = if right.is_empty() {
                None
            } else {
                let Some(value) = parse_decimal_bytes_limited(right, limit)? else {
                    return Ok(None);
                };
                Some(value)
            };
            (min, max, false)
        }
        None => {
            let Some(value) = parse_decimal_bytes_limited(body, limit)? else {
                return Ok(None);
            };
            (value, Some(value), true)
        }
    };

    if let Some(max) = max {
        if min > max {
            return Ok(None);
        }
    }

    Ok(Some(SimpleDotRepeat {
        min,
        max,
        lazy,
        exact,
    }))
}

fn dot_repeat_prefix_end(
    text: &str,
    _is_bytes: bool,
    dotall: bool,
    cap: Option<u64>,
) -> (u64, usize) {
    let mut count = 0_u64;
    let mut end_offset = 0_usize;
    for (idx, ch) in text.char_indices() {
        if matches!(cap, Some(limit) if count >= limit) {
            break;
        }
        if !dotall && ch == '\n' {
            break;
        }
        count += 1;
        end_offset = idx + ch.len_utf8();
    }
    (count, end_offset)
}

fn simple_dot_repeat_match(
    pattern: &str,
    text: &str,
    is_bytes: bool,
    flags: i64,
) -> PyResult<Option<PyObjectRef>> {
    let Some(plan) = parse_simple_dot_repeat(pattern)? else {
        return Ok(None);
    };
    let cap = if plan.exact || plan.lazy {
        Some(plan.min)
    } else {
        plan.max
    };
    let (available, end_offset) =
        dot_repeat_prefix_end(text, is_bytes, flags & RE_FLAG_DOTALL != 0, cap);
    if available < plan.min {
        return Ok(Some(PyObject::none()));
    }
    if plan.exact {
        if available < plan.min {
            return Ok(Some(PyObject::none()));
        }
    } else if let Some(max) = plan.max {
        if available > max {
            return Ok(None);
        }
    }
    Ok(Some(make_simple_match_object(
        text, 0, end_offset, is_bytes,
    )))
}

fn simple_ascii_ignorecase_literal_match(
    pattern: &str,
    text: &str,
    is_bytes: bool,
    flags: i64,
) -> Option<PyObjectRef> {
    let (body, inline_flags) = split_leading_inline_flags(pattern);
    let effective_flags = flags | inline_flags;
    if effective_flags & RE_FLAG_IGNORECASE == 0 {
        return None;
    }
    if is_bytes {
        if effective_flags & RE_FLAG_LOCALE != 0 {
            return None;
        }
    } else if effective_flags & RE_FLAG_ASCII == 0 {
        return None;
    }
    let mut chars = body.chars();
    let literal = chars.next()?;
    if chars.next().is_some() || literal.is_ascii() {
        return None;
    }
    if text.chars().next() == Some(literal) {
        Some(make_simple_match_object(
            text,
            0,
            literal.len_utf8(),
            is_bytes,
        ))
    } else {
        Some(PyObject::none())
    }
}

pub(super) fn re_match(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "re.match() requires pattern and string",
        ));
    }
    let (text, subject_is_bytes) = extract_re_subject(&args[1])?;
    let supplied_flags = if args.len() > 2 {
        args[2].to_int().unwrap_or(0)
    } else {
        0
    };
    ensure_re_compatible(&args[0], subject_is_bytes)?;
    let (pattern, flags) = extract_re_pattern_and_flags(&args[0], supplied_flags)?;
    let engine_flags = regex_engine_flags(flags, subject_is_bytes);
    if text.is_empty() && is_simple_nonboundary_pattern(&pattern) {
        return Ok(PyObject::none());
    }
    if let Some(result) =
        simple_ascii_ignorecase_literal_match(&pattern, &text, subject_is_bytes, flags)
    {
        return Ok(result);
    }
    if let Some(result) = simple_dot_repeat_match(&pattern, &text, subject_is_bytes, flags)? {
        return Ok(result);
    }
    let anchored = anchor_pattern(&pattern, "");
    if needs_fancy_regex_with_flags(&pattern, engine_flags) {
        let re = build_fancy_regex(&anchored, engine_flags)?;
        match re.captures(&text) {
            Ok(Some(caps)) => {
                let whole = caps.get(0).unwrap();
                let groups: Vec<Option<String>> = (1..caps.len())
                    .map(|i| caps.get(i).map(|m| m.as_str().to_string()))
                    .collect();
                let result = make_fancy_match_object(
                    &text,
                    whole.start(),
                    whole.end(),
                    whole.as_str(),
                    groups,
                    extract_fancy_group_names(&re),
                    subject_is_bytes,
                );
                attach_bytearray_source(&result, &args[1]);
                Ok(result)
            }
            _ => Ok(PyObject::none()),
        }
    } else {
        let re = cached_build_regex(&anchored, engine_flags)?;
        match re.find(&text) {
            Some(m) => {
                let result = make_match_object(m, &text, &re, subject_is_bytes);
                attach_bytearray_source(&result, &args[1]);
                Ok(result)
            }
            None => Ok(PyObject::none()),
        }
    }
}

pub(super) fn re_search(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "re.search() requires pattern and string",
        ));
    }
    let (text, subject_is_bytes) = extract_re_subject(&args[1])?;
    let supplied_flags = if args.len() > 2 {
        args[2].to_int().unwrap_or(0)
    } else {
        0
    };
    ensure_re_compatible(&args[0], subject_is_bytes)?;
    let (pattern, flags) = extract_re_pattern_and_flags(&args[0], supplied_flags)?;
    let engine_flags = regex_engine_flags(flags, subject_is_bytes);
    if text.is_empty() && is_simple_nonboundary_pattern(&pattern) {
        return Ok(PyObject::none());
    }
    if needs_fancy_regex_with_flags(&pattern, engine_flags) {
        let re = build_fancy_regex(&pattern, engine_flags)?;
        match re.captures(&text) {
            Ok(Some(caps)) => {
                let whole = caps.get(0).unwrap();
                let groups: Vec<Option<String>> = (1..caps.len())
                    .map(|i| caps.get(i).map(|m| m.as_str().to_string()))
                    .collect();
                let result = make_fancy_match_object(
                    &text,
                    whole.start(),
                    whole.end(),
                    whole.as_str(),
                    groups,
                    extract_fancy_group_names(&re),
                    subject_is_bytes,
                );
                attach_bytearray_source(&result, &args[1]);
                Ok(result)
            }
            _ => Ok(PyObject::none()),
        }
    } else {
        let re = build_regex(&pattern, engine_flags)?;
        match re.find(&text) {
            Some(m) => {
                let result = make_match_object(m, &text, &re, subject_is_bytes);
                attach_bytearray_source(&result, &args[1]);
                Ok(result)
            }
            None => Ok(PyObject::none()),
        }
    }
}

pub(super) fn re_fullmatch(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "re.fullmatch() requires pattern and string",
        ));
    }
    let (text, subject_is_bytes) = extract_re_subject(&args[1])?;
    let supplied_flags = if args.len() > 2 {
        args[2].to_int().unwrap_or(0)
    } else {
        0
    };
    ensure_re_compatible(&args[0], subject_is_bytes)?;
    let (pattern, flags) = extract_re_pattern_and_flags(&args[0], supplied_flags)?;
    let engine_flags = regex_engine_flags(flags, subject_is_bytes);
    let anchored = anchor_pattern(&pattern, r"\z");
    if needs_fancy_regex_with_flags(&pattern, engine_flags) {
        let re = build_fancy_regex(&anchored, engine_flags)?;
        match re.captures(&text) {
            Ok(Some(caps)) => {
                let whole = caps.get(0).unwrap();
                let groups: Vec<Option<String>> = (1..caps.len())
                    .map(|i| caps.get(i).map(|m| m.as_str().to_string()))
                    .collect();
                let result = make_fancy_match_object(
                    &text,
                    whole.start(),
                    whole.end(),
                    whole.as_str(),
                    groups,
                    extract_fancy_group_names(&re),
                    subject_is_bytes,
                );
                attach_bytearray_source(&result, &args[1]);
                Ok(result)
            }
            _ => Ok(PyObject::none()),
        }
    } else {
        let re = build_regex(&anchored, engine_flags)?;
        let orig_re = build_regex(&pattern, engine_flags)?;
        match re.find(&text) {
            Some(m) => {
                let result = make_match_object(m, &text, &orig_re, subject_is_bytes);
                attach_bytearray_source(&result, &args[1]);
                Ok(result)
            }
            None => Ok(PyObject::none()),
        }
    }
}

pub(super) fn re_findall(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "re.findall() requires pattern and string",
        ));
    }
    let (text, subject_is_bytes) = extract_re_subject(&args[1])?;
    let supplied_flags = if args.len() > 2 {
        args[2].to_int().unwrap_or(0)
    } else {
        0
    };
    ensure_re_compatible(&args[0], subject_is_bytes)?;
    let (pattern, flags) = extract_re_pattern_and_flags(&args[0], supplied_flags)?;
    let engine_flags = regex_engine_flags(flags, subject_is_bytes);
    if text.is_empty() && is_simple_nonboundary_pattern(&pattern) {
        return Ok(PyObject::list(vec![]));
    }
    if needs_fancy_regex_with_flags(&pattern, engine_flags) {
        let re = build_fancy_regex(&pattern, engine_flags)?;
        // Determine capture group count from first match
        let all_caps = fancy_captures(&re, &text);
        if all_caps.is_empty() {
            return Ok(PyObject::list(vec![]));
        }
        let cap_count = all_caps[0].len() - 1;
        if cap_count == 0 {
            let results: Vec<PyObjectRef> = fancy_find_all(&re, &text)
                .into_iter()
                .map(|s| py_re_text(&s, subject_is_bytes))
                .collect();
            Ok(PyObject::list(results))
        } else if cap_count == 1 {
            let results: Vec<PyObjectRef> = all_caps
                .into_iter()
                .map(|g| {
                    g.get(1)
                        .cloned()
                        .flatten()
                        .map(|s| py_re_text(&s, subject_is_bytes))
                        .unwrap_or_else(|| py_re_text("", subject_is_bytes))
                })
                .collect();
            Ok(PyObject::list(results))
        } else {
            let results: Vec<PyObjectRef> = all_caps
                .into_iter()
                .map(|g| {
                    let items: Vec<PyObjectRef> = g[1..]
                        .iter()
                        .map(|o| {
                            o.as_ref()
                                .map(|s| py_re_text(s.as_str(), subject_is_bytes))
                                .unwrap_or_else(|| py_re_text("", subject_is_bytes))
                        })
                        .collect();
                    PyObject::tuple(items)
                })
                .collect();
            Ok(PyObject::list(results))
        }
    } else {
        let re = build_regex(&pattern, engine_flags)?;
        let cap_count = re.captures_len() - 1;
        if cap_count == 0 {
            let results: Vec<PyObjectRef> = re
                .find_iter(&text)
                .map(|m| py_re_text(m.as_str(), subject_is_bytes))
                .collect();
            Ok(PyObject::list(results))
        } else if cap_count == 1 {
            let results: Vec<PyObjectRef> = re
                .captures_iter(&text)
                .map(|caps| {
                    caps.get(1)
                        .map(|m| py_re_text(m.as_str(), subject_is_bytes))
                        .unwrap_or_else(|| py_re_text("", subject_is_bytes))
                })
                .collect();
            Ok(PyObject::list(results))
        } else {
            let results: Vec<PyObjectRef> = re
                .captures_iter(&text)
                .map(|caps| {
                    let groups: Vec<PyObjectRef> = (1..=cap_count)
                        .map(|i| {
                            caps.get(i)
                                .map(|m| py_re_text(m.as_str(), subject_is_bytes))
                                .unwrap_or_else(|| py_re_text("", subject_is_bytes))
                        })
                        .collect();
                    PyObject::tuple(groups)
                })
                .collect();
            Ok(PyObject::list(results))
        }
    }
}

pub(super) fn re_finditer(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "re.finditer() requires pattern and string",
        ));
    }
    let (text, subject_is_bytes) = extract_re_subject(&args[1])?;
    let supplied_flags = if args.len() > 2 {
        args[2].to_int().unwrap_or(0)
    } else {
        0
    };
    ensure_re_compatible(&args[0], subject_is_bytes)?;
    let (pattern, flags) = extract_re_pattern_and_flags(&args[0], supplied_flags)?;
    if matches!(args[1].payload, PyObjectPayload::ByteArray(_)) {
        register_bytearray_export(&args[1]);
    }
    let engine_flags = regex_engine_flags(flags, subject_is_bytes);
    if text.is_empty() && is_simple_nonboundary_pattern(&pattern) {
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::List {
                items: Vec::new(),
                index: 0,
            }),
        ))));
    }
    if needs_fancy_regex_with_flags(&pattern, engine_flags) {
        let re = build_fancy_regex(&pattern, engine_flags)?;
        let group_names = extract_fancy_group_names(&re);
        let mut matches: Vec<PyObjectRef> = Vec::new();
        let mut pos = 0;
        while pos <= text.len() {
            match re.captures(&text[pos..]) {
                Ok(Some(caps)) => {
                    let whole = caps.get(0).unwrap();
                    if whole.start() == whole.end() {
                        pos += 1;
                        continue;
                    }
                    let abs_start = pos + whole.start();
                    let abs_end = pos + whole.end();
                    let mut groups = Vec::new();
                    for i in 1..caps.len() {
                        groups.push(caps.get(i).map(|g| g.as_str().to_string()));
                    }
                    matches.push(make_fancy_match_object(
                        &text,
                        abs_start,
                        abs_end,
                        &text[abs_start..abs_end],
                        groups,
                        group_names.clone(),
                        subject_is_bytes,
                    ));
                    pos = abs_end;
                }
                _ => break,
            }
        }
        Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::List {
                items: matches,
                index: 0,
            }),
        ))))
    } else {
        let re = build_regex(&pattern, engine_flags)?;
        let mut matches: Vec<PyObjectRef> = Vec::new();
        let mut last_span: Option<(usize, usize)> = None;
        for caps in re.captures_iter(&text) {
            let whole = caps.get(0).unwrap();
            last_span = Some((whole.start(), whole.end()));
            matches.push(make_match_object_from_captures(
                &caps,
                &text,
                &re,
                subject_is_bytes,
            ));
        }
        if matches!(
            last_span,
            Some((start, end)) if start != end && end == text.len()
        ) {
            if let Some(caps) = re.captures_at(&text, text.len()) {
                let whole = caps.get(0).unwrap();
                if whole.start() == text.len() && whole.end() == text.len() {
                    matches.push(make_match_object_from_captures(
                        &caps,
                        &text,
                        &re,
                        subject_is_bytes,
                    ));
                }
            }
        }
        Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::List {
                items: matches,
                index: 0,
            }),
        ))))
    }
}

fn dollar_match_offsets(pattern: &str, text: &str, flags: i64) -> Option<Vec<usize>> {
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

fn re_sub_plain_offsets(
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

pub(super) fn re_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
fn re_sub_callable(
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

pub(super) fn re_subn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn re_split(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(super) fn re_compile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
    let simple_dot_repeat =
        parse_simple_dot_repeat(split_leading_inline_flags(&pattern).0)?.is_some();
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

fn re_escape_needs_backslash(ch: char) -> bool {
    matches!(
        ch,
        '(' | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '?'
            | '*'
            | '+'
            | '-'
            | '|'
            | '^'
            | '$'
            | '\\'
            | '.'
            | '&'
            | '~'
            | '#'
            | ' '
            | '\t'
            | '\n'
            | '\r'
            | '\u{0b}'
            | '\u{0c}'
    )
}

pub(super) fn re_escape(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("re.escape() requires a string"));
    }
    match &args[0].payload {
        PyObjectPayload::Bytes(bytes) | PyObjectPayload::ByteArray(bytes) => {
            let mut escaped = Vec::with_capacity(bytes.len());
            for &byte in bytes.iter() {
                if re_escape_needs_backslash(byte as char) {
                    escaped.push(b'\\');
                }
                escaped.push(byte);
            }
            Ok(PyObject::bytes(escaped))
        }
        _ => {
            let s = args[0].py_to_string();
            let mut escaped = String::with_capacity(s.len());
            for ch in s.chars() {
                if re_escape_needs_backslash(ch) {
                    escaped.push('\\');
                }
                escaped.push(ch);
            }
            Ok(PyObject::str_val(CompactString::from(escaped)))
        }
    }
}
