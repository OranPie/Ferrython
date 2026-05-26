use super::*;

fn trailing_kwarg(args: &[PyObjectRef], name: &str) -> Option<PyObjectRef> {
    let last = args.last()?;
    if let PyObjectPayload::Dict(map) = &last.payload {
        let key = HashableKey::str_key(CompactString::from(name));
        return map.read().get(&key).cloned();
    }
    None
}

fn positional_arg(args: &[PyObjectRef], index: usize) -> Option<PyObjectRef> {
    let positional_len = if args
        .last()
        .map(|last| matches!(&last.payload, PyObjectPayload::Dict(_)))
        .unwrap_or(false)
    {
        args.len().saturating_sub(1)
    } else {
        args.len()
    };
    if index < positional_len {
        Some(args[index].clone())
    } else {
        None
    }
}

fn method_arg(args: &[PyObjectRef], index: usize, name: &str) -> Option<PyObjectRef> {
    positional_arg(args, index).or_else(|| trailing_kwarg(args, name))
}

fn method_int_arg(args: &[PyObjectRef], index: usize, name: &str, default: i64) -> i64 {
    method_arg(args, index, name)
        .and_then(|obj| obj.to_int().ok())
        .unwrap_or(default)
}

fn normalize_re_bound(value: i64, len: usize) -> usize {
    if value <= 0 {
        0
    } else {
        (value as usize).min(len)
    }
}

struct PatternWindow {
    pattern: PyObjectRef,
    string_obj: PyObjectRef,
    text: String,
    subject_is_bytes: bool,
    pos: usize,
    endpos: usize,
    pos_offset: usize,
    endpos_offset: usize,
}

fn pattern_window_args(args: &[PyObjectRef], method: &str) -> PyResult<PatternWindow> {
    if args.is_empty() {
        return Err(PyException::type_error(format!(
            "Pattern.{}() requires self",
            method
        )));
    }
    let pattern = args[0].clone();
    let string_obj = method_arg(args, 1, "string").ok_or_else(|| {
        PyException::type_error(format!("Pattern.{}() requires self and string", method))
    })?;
    let (text, subject_is_bytes) = extract_re_subject(&string_obj)?;
    ensure_re_compatible(&pattern, subject_is_bytes)?;
    let subject_len = regex_offset_to_py_index(&text, text.len(), subject_is_bytes) as usize;
    let pos = normalize_re_bound(method_int_arg(args, 2, "pos", 0), subject_len);
    let endpos = normalize_re_bound(
        method_int_arg(args, 3, "endpos", subject_len as i64),
        subject_len,
    );
    let pos_offset = py_index_to_regex_offset(&text, pos);
    let endpos_offset = py_index_to_regex_offset(&text, endpos);
    Ok(PatternWindow {
        pattern,
        string_obj,
        text,
        subject_is_bytes,
        pos,
        endpos,
        pos_offset,
        endpos_offset,
    })
}

fn window_slice(text: &str, pos: usize, endpos: usize) -> &str {
    if pos > endpos {
        ""
    } else {
        &text[pos.min(text.len())..endpos.min(text.len())]
    }
}

fn offset_match_result(
    result: &PyObjectRef,
    text: &str,
    subject_is_bytes: bool,
    pos: usize,
    endpos: usize,
) {
    if matches!(result.payload, PyObjectPayload::None) {
        return;
    }
    let PyObjectPayload::Module(md) = &result.payload else {
        return;
    };
    let offset = pos as i64;
    let mut attrs = md.attrs.write();
    let start = attrs
        .get("_start")
        .and_then(|v| v.to_int().ok())
        .unwrap_or(0)
        + offset;
    let end = attrs.get("_end").and_then(|v| v.to_int().ok()).unwrap_or(0) + offset;
    let adjusted_spans = attrs.get("_group_spans").and_then(|spans_obj| {
        if let PyObjectPayload::Tuple(items) = &spans_obj.payload {
            Some(
                items
                    .iter()
                    .map(|item| {
                        if let PyObjectPayload::Tuple(pair) = &item.payload {
                            if pair.len() == 2 {
                                let start = pair[0].to_int().unwrap_or(-1);
                                let end = pair[1].to_int().unwrap_or(-1);
                                if start >= 0 && end >= 0 {
                                    return Some((start + offset, end + offset));
                                }
                            }
                        }
                        None
                    })
                    .collect::<Vec<_>>(),
            )
        } else {
            None
        }
    });
    attrs.insert(CompactString::from("_start"), PyObject::int(start));
    attrs.insert(CompactString::from("_end"), PyObject::int(end));
    attrs.insert(
        CompactString::from("_text"),
        py_re_text(text, subject_is_bytes),
    );
    attrs.insert(
        CompactString::from("string"),
        py_re_text(text, subject_is_bytes),
    );
    attrs.insert(CompactString::from("pos"), PyObject::int(pos as i64));
    attrs.insert(CompactString::from("endpos"), PyObject::int(endpos as i64));
    if let Some(spans) = adjusted_spans {
        attrs.insert(
            CompactString::from("_group_spans"),
            group_spans_to_py(&spans),
        );
        attrs.insert(CompactString::from("regs"), match_regs(start, end, &spans));
    }
}

fn make_re_scanner(window: PatternWindow) -> PyObjectRef {
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("pattern"), window.pattern.clone());
    attrs.insert(CompactString::from("_pattern"), window.pattern);
    attrs.insert(CompactString::from("string"), window.string_obj);
    attrs.insert(
        CompactString::from("_text"),
        py_re_text(&window.text, window.subject_is_bytes),
    );
    attrs.insert(
        CompactString::from("_is_bytes"),
        PyObject::bool_val(window.subject_is_bytes),
    );
    attrs.insert(
        CompactString::from("_pos"),
        PyObject::int(window.pos as i64),
    );
    attrs.insert(
        CompactString::from("_endpos"),
        PyObject::int(window.endpos as i64),
    );
    PyObject::instance_with_attrs(re_scanner_class(), attrs)
}

fn scanner_set_pos(scanner: &PyObjectRef, pos: i64) {
    if let PyObjectPayload::Instance(inst) = &scanner.payload {
        inst.attrs
            .write()
            .insert(CompactString::from("_pos"), PyObject::int(pos));
    }
}

fn scanner_next(args: &[PyObjectRef], search: bool) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("scanner method requires self"));
    }
    let scanner = &args[0];
    let pattern = scanner
        .get_attr("_pattern")
        .ok_or_else(|| PyException::attribute_error("pattern"))?;
    let string_obj = scanner
        .get_attr("string")
        .ok_or_else(|| PyException::attribute_error("string"))?;
    let pos = scanner
        .get_attr("_pos")
        .and_then(|v| v.to_int().ok())
        .unwrap_or(0);
    let endpos = scanner
        .get_attr("_endpos")
        .and_then(|v| v.to_int().ok())
        .unwrap_or(pos);
    let call_args = vec![
        pattern,
        string_obj,
        PyObject::int(pos),
        PyObject::int(endpos),
    ];
    let result = if search {
        compiled_search(&call_args)?
    } else {
        compiled_match(&call_args)?
    };
    if matches!(result.payload, PyObjectPayload::None) {
        scanner_set_pos(scanner, endpos);
    } else {
        let end = result
            .get_attr("_end")
            .and_then(|v| v.to_int().ok())
            .unwrap_or(pos);
        scanner_set_pos(scanner, if end <= pos { pos + 1 } else { end });
    }
    Ok(result)
}

pub(super) fn scanner_match(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    scanner_next(args, false)
}

pub(super) fn scanner_search(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    scanner_next(args, true)
}

pub(super) fn re_scanner_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("Scanner() requires a lexicon"));
    }
    let lexicon = args[0].clone();
    let mut parts = Vec::new();
    if let PyObjectPayload::List(items) = &lexicon.payload {
        for item in items.read().iter() {
            if let PyObjectPayload::Tuple(pair) = &item.payload {
                if let Some(pattern_obj) = pair.first() {
                    if let Ok(pattern) = extract_re_pattern(pattern_obj) {
                        parts.push(format!("(?:{})", pattern));
                    }
                }
            }
        }
    }
    let combined = if parts.is_empty() {
        String::from("(?!)")
    } else {
        parts.join("|")
    };
    let pattern_obj = re_compile(&[PyObject::str_val(CompactString::from(combined))])?;
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("_lexicon"), lexicon);
    attrs.insert(CompactString::from("scanner"), pattern_obj);
    attrs.insert(
        CompactString::from("scan"),
        PyObject::native_function("Scanner.scan", re_scanner_scan),
    );
    attrs.insert(
        CompactString::from("_bind_methods"),
        PyObject::bool_val(true),
    );
    Ok(PyObject::module_with_attrs(
        CompactString::from("Scanner"),
        attrs,
    ))
}

fn re_scanner_scan(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("scan() requires self and string"));
    }
    let scanner = &args[0];
    let lexicon = scanner
        .get_attr("_lexicon")
        .ok_or_else(|| PyException::attribute_error("_lexicon"))?;
    let (text, subject_is_bytes) = extract_re_subject(&args[1])?;
    let mut results = Vec::new();
    let mut pos = 0usize;
    while pos < text.len() {
        let mut matched = false;
        if let PyObjectPayload::List(items) = &lexicon.payload {
            for item in items.read().iter() {
                let PyObjectPayload::Tuple(pair) = &item.payload else {
                    continue;
                };
                if pair.len() < 2 {
                    continue;
                }
                let tail = py_re_text(&text[pos..], subject_is_bytes);
                let match_obj = re_match(&[pair[0].clone(), tail])?;
                if matches!(match_obj.payload, PyObjectPayload::None) {
                    continue;
                }
                let token = match_obj
                    .get_attr("_match")
                    .unwrap_or_else(|| py_re_text("", subject_is_bytes));
                let end = match_obj
                    .get_attr("_end")
                    .and_then(|v| v.to_int().ok())
                    .unwrap_or(0);
                if end <= 0 {
                    return Err(PyException::runtime_error(
                        "scanner pattern matched empty text",
                    ));
                }
                if !matches!(pair[1].payload, PyObjectPayload::None) {
                    let value =
                        ferrython_core::object::call_callable(&pair[1], &[scanner.clone(), token])?;
                    if !matches!(value.payload, PyObjectPayload::None) {
                        results.push(value);
                    }
                }
                pos += end as usize;
                matched = true;
                break;
            }
        }
        if !matched {
            break;
        }
    }
    Ok(PyObject::tuple(vec![
        PyObject::list(results),
        py_re_text(&text[pos..], subject_is_bytes),
    ]))
}

pub(super) fn compiled_match(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let window = pattern_window_args(args, "match")?;
    if window.pos > window.endpos {
        return Ok(PyObject::none());
    }
    let sliced = window_slice(&window.text, window.pos_offset, window.endpos_offset);
    let result = re_match(&[
        window.pattern.clone(),
        py_re_text(sliced, window.subject_is_bytes),
    ])?;
    offset_match_result(
        &result,
        &window.text,
        window.subject_is_bytes,
        window.pos,
        window.endpos,
    );
    Ok(result)
}

pub(super) fn compiled_search(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let window = pattern_window_args(args, "search")?;
    if window.pos > window.endpos {
        return Ok(PyObject::none());
    }
    let sliced = window_slice(&window.text, window.pos_offset, window.endpos_offset);
    let result = re_search(&[
        window.pattern.clone(),
        py_re_text(sliced, window.subject_is_bytes),
    ])?;
    offset_match_result(
        &result,
        &window.text,
        window.subject_is_bytes,
        window.pos,
        window.endpos,
    );
    Ok(result)
}

pub(super) fn compiled_findall(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let window = pattern_window_args(args, "findall")?;
    if window.pos > window.endpos {
        return Ok(PyObject::list(vec![]));
    }
    let sliced = window_slice(&window.text, window.pos_offset, window.endpos_offset);
    re_findall(&[
        window.pattern.clone(),
        py_re_text(sliced, window.subject_is_bytes),
    ])
}

pub(super) fn compiled_finditer(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let window = pattern_window_args(args, "finditer")?;
    if window.pos > window.endpos {
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(IteratorData::List {
                items: Vec::new(),
                index: 0,
            }),
        ))));
    }
    let sliced = window_slice(&window.text, window.pos_offset, window.endpos_offset);
    re_finditer(&[
        window.pattern.clone(),
        py_re_text(sliced, window.subject_is_bytes),
    ])
}

pub(super) fn compiled_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error(
            "Pattern.sub() requires self, repl, and string",
        ));
    }
    let self_obj = &args[0];
    let count = if args.len() > 3 {
        args[3].clone()
    } else {
        PyObject::int(0)
    };
    re_sub(&[self_obj.clone(), args[1].clone(), args[2].clone(), count])
}

pub(super) fn compiled_split(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("Pattern.split() requires self"));
    }
    let self_obj = &args[0];
    let string_obj = method_arg(args, 1, "string")
        .ok_or_else(|| PyException::type_error("Pattern.split() requires self and string"))?;
    let maxsplit = method_arg(args, 2, "maxsplit").unwrap_or_else(|| PyObject::int(0));
    re_split(&[self_obj.clone(), string_obj, maxsplit])
}

pub(super) fn compiled_fullmatch(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let window = pattern_window_args(args, "fullmatch")?;
    if window.pos > window.endpos {
        return Ok(PyObject::none());
    }
    let sliced = window_slice(&window.text, window.pos_offset, window.endpos_offset);
    let result = re_fullmatch(&[
        window.pattern.clone(),
        py_re_text(sliced, window.subject_is_bytes),
    ])?;
    offset_match_result(
        &result,
        &window.text,
        window.subject_is_bytes,
        window.pos,
        window.endpos,
    );
    Ok(result)
}

pub(super) fn compiled_subn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 {
        return Err(PyException::type_error(
            "Pattern.subn() requires self, repl, and string",
        ));
    }
    let self_obj = &args[0];
    let count = if args.len() > 3 {
        args[3].clone()
    } else {
        PyObject::int(0)
    };
    re_subn(&[self_obj.clone(), args[1].clone(), args[2].clone(), count])
}

pub(super) fn compiled_scanner(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(make_re_scanner(pattern_window_args(args, "scanner")?))
}
