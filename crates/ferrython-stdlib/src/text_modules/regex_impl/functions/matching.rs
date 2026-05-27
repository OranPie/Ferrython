use super::*;
use crate::text_modules::regex_impl::functions::simple::{
    simple_ascii_ignorecase_literal_match, simple_dot_repeat_match,
};

pub(in crate::text_modules::regex_impl) fn re_match(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

pub(in crate::text_modules::regex_impl) fn re_search(
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
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

pub(in crate::text_modules::regex_impl) fn re_fullmatch(
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
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

pub(in crate::text_modules::regex_impl) fn re_findall(
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
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

pub(in crate::text_modules::regex_impl) fn re_finditer(
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
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
