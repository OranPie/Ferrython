use super::*;

fn py_span(start: i64, end: i64) -> PyObjectRef {
    PyObject::tuple(vec![PyObject::int(start), PyObject::int(end)])
}

pub(super) fn group_spans_to_py(spans: &[Option<(i64, i64)>]) -> PyObjectRef {
    PyObject::tuple(
        spans
            .iter()
            .map(|span| match span {
                Some((start, end)) => py_span(*start, *end),
                None => py_span(-1, -1),
            })
            .collect(),
    )
}

pub(super) fn match_regs(start: i64, end: i64, spans: &[Option<(i64, i64)>]) -> PyObjectRef {
    let mut regs = Vec::with_capacity(spans.len() + 1);
    regs.push(py_span(start, end));
    regs.extend(spans.iter().map(|span| match span {
        Some((start, end)) => py_span(*start, *end),
        None => py_span(-1, -1),
    }));
    PyObject::tuple(regs)
}

fn match_lastindex(spans: &[Option<(i64, i64)>]) -> Option<i64> {
    let mut best: Option<(usize, i64)> = None;
    for (idx, span) in spans.iter().enumerate() {
        if let Some((_, end)) = span {
            match best {
                Some((_, best_end)) if *end < best_end => {}
                Some((_, best_end)) if *end == best_end => {}
                _ => best = Some((idx + 1, *end)),
            }
        }
    }
    best.map(|(idx, _)| idx as i64)
}

fn match_lastgroup(lastindex: Option<i64>, groupindex_map: &FxHashKeyMap) -> PyObjectRef {
    let Some(lastindex) = lastindex else {
        return PyObject::none();
    };
    for (key, value) in groupindex_map.iter() {
        if value.to_int().ok() == Some(lastindex) {
            if let HashableKey::Str(name) = key {
                return PyObject::str_val(name.to_compact_string());
            }
        }
    }
    PyObject::none()
}

fn match_repr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("Match.__repr__ requires self"));
    }
    let self_obj = &args[0];
    let start = self_obj
        .get_attr("_start")
        .and_then(|v| v.to_int().ok())
        .unwrap_or(0);
    let end = self_obj
        .get_attr("_end")
        .and_then(|v| v.to_int().ok())
        .unwrap_or(0);
    let matched = self_obj
        .get_attr("_match")
        .map(|v| v.repr())
        .unwrap_or_else(|| "''".to_string());
    Ok(PyObject::str_val(CompactString::from(format!(
        "<re.Match object; span=({}, {}), match={}>",
        start, end, matched
    ))))
}

fn insert_match_methods(attrs: &mut IndexMap<CompactString, PyObjectRef>) {
    attrs.insert(
        CompactString::from("group"),
        PyObject::native_function("Match.group", match_group),
    );
    attrs.insert(
        CompactString::from("groups"),
        PyObject::native_function("Match.groups", match_groups),
    );
    attrs.insert(
        CompactString::from("groupdict"),
        PyObject::native_function("Match.groupdict", match_groupdict),
    );
    attrs.insert(
        CompactString::from("start"),
        PyObject::native_function("Match.start", match_start),
    );
    attrs.insert(
        CompactString::from("end"),
        PyObject::native_function("Match.end", match_end),
    );
    attrs.insert(
        CompactString::from("span"),
        PyObject::native_function("Match.span", match_span),
    );
    attrs.insert(
        CompactString::from("expand"),
        PyObject::native_function("Match.expand", match_expand),
    );
    attrs.insert(
        CompactString::from("__getitem__"),
        PyObject::native_function("Match.__getitem__", match_getitem),
    );
    attrs.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("Match.__repr__", match_repr),
    );
    attrs.insert(
        CompactString::from("_bind_methods"),
        PyObject::bool_val(true),
    );
}

pub(super) fn make_fancy_match_object(
    text: &str,
    start: usize,
    end: usize,
    full: &str,
    groups: Vec<Option<String>>,
    group_names: FxHashKeyMap,
    is_bytes: bool,
) -> PyObjectRef {
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("_match"), py_re_text(full, is_bytes));
    let start_index = regex_offset_to_py_index(text, start, is_bytes);
    let end_index = regex_offset_to_py_index(text, end, is_bytes);
    attrs.insert(CompactString::from("_start"), PyObject::int(start_index));
    attrs.insert(CompactString::from("_end"), PyObject::int(end_index));
    attrs.insert(CompactString::from("_text"), py_re_text(text, is_bytes));
    attrs.insert(CompactString::from("string"), py_re_text(text, is_bytes));
    attrs.insert(CompactString::from("pos"), PyObject::int(0));
    attrs.insert(
        CompactString::from("endpos"),
        PyObject::int(regex_offset_to_py_index(text, text.len(), is_bytes)),
    );
    attrs.insert(
        CompactString::from("_is_bytes"),
        PyObject::bool_val(is_bytes),
    );
    let group_objs: Vec<PyObjectRef> = groups
        .iter()
        .map(|g| {
            g.as_ref()
                .map(|s| py_re_text(s, is_bytes))
                .unwrap_or(PyObject::none())
        })
        .collect();
    let group_spans: Vec<Option<(i64, i64)>> = groups
        .iter()
        .map(|group| {
            group.as_ref().and_then(|value| {
                text[start..end].find(value).map(|rel| {
                    let abs_start = start + rel;
                    let abs_end = abs_start + value.len();
                    (
                        regex_offset_to_py_index(text, abs_start, is_bytes),
                        regex_offset_to_py_index(text, abs_end, is_bytes),
                    )
                })
            })
        })
        .collect();
    let lastindex = match_lastindex(&group_spans);
    attrs.insert(CompactString::from("_groups"), PyObject::tuple(group_objs));
    attrs.insert(
        CompactString::from("_group_spans"),
        group_spans_to_py(&group_spans),
    );
    attrs.insert(
        CompactString::from("_groupindex"),
        PyObject::dict_fx(group_names.clone()),
    );
    attrs.insert(
        CompactString::from("regs"),
        match_regs(start_index, end_index, &group_spans),
    );
    attrs.insert(
        CompactString::from("lastindex"),
        lastindex.map(PyObject::int).unwrap_or_else(PyObject::none),
    );
    attrs.insert(
        CompactString::from("lastgroup"),
        match_lastgroup(lastindex, &group_names),
    );
    attrs.insert(CompactString::from("re"), PyObject::bool_val(true));
    insert_match_methods(&mut attrs);
    PyObject::module_with_attrs(CompactString::from("Match"), attrs)
}

pub(super) fn make_match_object_from_captures(
    caps: &regex::Captures,
    text: &str,
    re_obj: &regex::Regex,
    is_bytes: bool,
) -> PyObjectRef {
    let whole = caps.get(0).unwrap();
    let full_match = whole.as_str().to_string();
    let start = regex_offset_to_py_index(text, whole.start(), is_bytes);
    let end = regex_offset_to_py_index(text, whole.end(), is_bytes);
    let mut groups = Vec::new();
    let mut group_spans = Vec::new();
    for i in 1..caps.len() {
        if let Some(g) = caps.get(i) {
            groups.push(py_re_text(g.as_str(), is_bytes));
            group_spans.push(Some((
                regex_offset_to_py_index(text, g.start(), is_bytes),
                regex_offset_to_py_index(text, g.end(), is_bytes),
            )));
        } else {
            groups.push(PyObject::none());
            group_spans.push(None);
        }
    }
    let groups_tuple = PyObject::tuple(groups);
    let mut groupindex_map = new_fx_hashkey_map();
    for (i, name_opt) in re_obj.capture_names().enumerate() {
        if let Some(name) = name_opt {
            groupindex_map.insert(
                HashableKey::str_key(CompactString::from(name)),
                PyObject::int(i as i64),
            );
        }
    }
    let lastindex = match_lastindex(&group_spans);
    let groupindex = PyObject::dict_fx(groupindex_map.clone());
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("_match"),
        py_re_text(&full_match, is_bytes),
    );
    attrs.insert(CompactString::from("_start"), PyObject::int(start));
    attrs.insert(CompactString::from("_end"), PyObject::int(end));
    attrs.insert(CompactString::from("_text"), py_re_text(text, is_bytes));
    attrs.insert(CompactString::from("string"), py_re_text(text, is_bytes));
    attrs.insert(CompactString::from("pos"), PyObject::int(0));
    attrs.insert(
        CompactString::from("endpos"),
        PyObject::int(regex_offset_to_py_index(text, text.len(), is_bytes)),
    );
    attrs.insert(
        CompactString::from("_is_bytes"),
        PyObject::bool_val(is_bytes),
    );
    attrs.insert(CompactString::from("_groups"), groups_tuple);
    attrs.insert(
        CompactString::from("_group_spans"),
        group_spans_to_py(&group_spans),
    );
    attrs.insert(CompactString::from("_groupindex"), groupindex);
    attrs.insert(
        CompactString::from("regs"),
        match_regs(start, end, &group_spans),
    );
    attrs.insert(
        CompactString::from("lastindex"),
        lastindex.map(PyObject::int).unwrap_or_else(PyObject::none),
    );
    attrs.insert(
        CompactString::from("lastgroup"),
        match_lastgroup(lastindex, &groupindex_map),
    );
    attrs.insert(CompactString::from("re"), PyObject::bool_val(true));
    insert_match_methods(&mut attrs);
    PyObject::module_with_attrs(CompactString::from("Match"), attrs)
}

pub(super) fn make_match_object(
    m: regex::Match,
    text: &str,
    re_obj: &regex::Regex,
    is_bytes: bool,
) -> PyObjectRef {
    let full_match = m.as_str().to_string();
    let start = regex_offset_to_py_index(text, m.start(), is_bytes);
    let end = regex_offset_to_py_index(text, m.end(), is_bytes);
    // groups - store captured groups
    // Use captures_at to find the capture at this match's start position
    let captures = re_obj.captures_at(text, m.start());
    let mut groups = Vec::new();
    let mut group_spans = Vec::new();
    if let Some(caps) = &captures {
        for i in 1..caps.len() {
            if let Some(g) = caps.get(i) {
                groups.push(py_re_text(g.as_str(), is_bytes));
                group_spans.push(Some((
                    regex_offset_to_py_index(text, g.start(), is_bytes),
                    regex_offset_to_py_index(text, g.end(), is_bytes),
                )));
            } else {
                groups.push(PyObject::none());
                group_spans.push(None);
            }
        }
    }
    let groups_tuple = PyObject::tuple(groups);
    // Build name→index mapping for named capture groups
    let mut groupindex_map = new_fx_hashkey_map();
    for (i, name_opt) in re_obj.capture_names().enumerate() {
        if let Some(name) = name_opt {
            groupindex_map.insert(
                HashableKey::str_key(CompactString::from(name)),
                PyObject::int(i as i64),
            );
        }
    }
    let lastindex = match_lastindex(&group_spans);
    let groupindex = PyObject::dict_fx(groupindex_map.clone());
    // Build the match object with pre-bound data attributes
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("_match"),
        py_re_text(&full_match, is_bytes),
    );
    attrs.insert(CompactString::from("_start"), PyObject::int(start));
    attrs.insert(CompactString::from("_end"), PyObject::int(end));
    attrs.insert(CompactString::from("_text"), py_re_text(text, is_bytes));
    attrs.insert(CompactString::from("string"), py_re_text(text, is_bytes));
    attrs.insert(CompactString::from("pos"), PyObject::int(0));
    attrs.insert(
        CompactString::from("endpos"),
        PyObject::int(regex_offset_to_py_index(text, text.len(), is_bytes)),
    );
    attrs.insert(
        CompactString::from("_is_bytes"),
        PyObject::bool_val(is_bytes),
    );
    attrs.insert(CompactString::from("_groups"), groups_tuple);
    attrs.insert(
        CompactString::from("_group_spans"),
        group_spans_to_py(&group_spans),
    );
    attrs.insert(CompactString::from("_groupindex"), groupindex);
    attrs.insert(
        CompactString::from("regs"),
        match_regs(start, end, &group_spans),
    );
    attrs.insert(
        CompactString::from("lastindex"),
        lastindex.map(PyObject::int).unwrap_or_else(PyObject::none),
    );
    attrs.insert(
        CompactString::from("lastgroup"),
        match_lastgroup(lastindex, &groupindex_map),
    );
    attrs.insert(CompactString::from("re"), PyObject::bool_val(true));
    insert_match_methods(&mut attrs);
    let match_obj = PyObject::module_with_attrs(CompactString::from("Match"), attrs);
    match_obj
}

pub(super) fn make_simple_match_object(
    text: &str,
    start_offset: usize,
    end_offset: usize,
    is_bytes: bool,
) -> PyObjectRef {
    let start = regex_offset_to_py_index(text, start_offset, is_bytes);
    let end = regex_offset_to_py_index(text, end_offset, is_bytes);
    let group_spans: Vec<Option<(i64, i64)>> = Vec::new();
    let groupindex_map = new_fx_hashkey_map();
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("_match"),
        py_re_text(&text[start_offset..end_offset], is_bytes),
    );
    attrs.insert(CompactString::from("_start"), PyObject::int(start));
    attrs.insert(CompactString::from("_end"), PyObject::int(end));
    attrs.insert(CompactString::from("_text"), py_re_text(text, is_bytes));
    attrs.insert(CompactString::from("string"), py_re_text(text, is_bytes));
    attrs.insert(CompactString::from("pos"), PyObject::int(0));
    attrs.insert(
        CompactString::from("endpos"),
        PyObject::int(regex_offset_to_py_index(text, text.len(), is_bytes)),
    );
    attrs.insert(
        CompactString::from("_is_bytes"),
        PyObject::bool_val(is_bytes),
    );
    attrs.insert(CompactString::from("_groups"), PyObject::tuple(Vec::new()));
    attrs.insert(
        CompactString::from("_group_spans"),
        group_spans_to_py(&group_spans),
    );
    attrs.insert(
        CompactString::from("_groupindex"),
        PyObject::dict_fx(groupindex_map.clone()),
    );
    attrs.insert(
        CompactString::from("regs"),
        match_regs(start, end, &group_spans),
    );
    attrs.insert(CompactString::from("lastindex"), PyObject::none());
    attrs.insert(CompactString::from("lastgroup"), PyObject::none());
    attrs.insert(CompactString::from("re"), PyObject::bool_val(true));
    insert_match_methods(&mut attrs);
    PyObject::module_with_attrs(CompactString::from("Match"), attrs)
}

pub(super) fn attach_bytearray_source(match_obj: &PyObjectRef, source: &PyObjectRef) {
    if !matches!(source.payload, PyObjectPayload::ByteArray(_)) {
        return;
    }
    if let PyObjectPayload::Module(md) = &match_obj.payload {
        md.attrs
            .write()
            .insert(CompactString::from("_bytearray_source"), source.clone());
    }
}

fn match_group_count(self_obj: &PyObjectRef) -> usize {
    self_obj
        .get_attr("_groups")
        .and_then(|groups| {
            if let PyObjectPayload::Tuple(items) = &groups.payload {
                Some(items.len())
            } else {
                None
            }
        })
        .unwrap_or(0)
}

fn match_int_arg(arg: &PyObjectRef) -> PyResult<i64> {
    if let Ok(value) = arg.to_int() {
        return Ok(value);
    }
    if let Some(index_method) = arg.get_attr("__index__") {
        let value = ferrython_core::object::call_callable(&index_method, &[])?;
        return value
            .to_int()
            .map_err(|_| PyException::index_error("no such group"));
    }
    Err(PyException::index_error("no such group"))
}

fn match_group_index(self_obj: &PyObjectRef, arg: &PyObjectRef) -> PyResult<i64> {
    if let PyObjectPayload::Str(name) = &arg.payload {
        if let Some(groupindex) = self_obj.get_attr("_groupindex") {
            if let PyObjectPayload::Dict(d) = &groupindex.payload {
                let key = HashableKey::str_key(name.to_compact_string());
                if let Some(idx_obj) = d.read().get(&key).cloned() {
                    return idx_obj.to_int();
                }
            }
        }
        return Err(PyException::index_error(format!(
            "no such group: '{}'",
            name
        )));
    }
    let idx = match_int_arg(arg)?;
    let idx_usize = usize::try_from(idx).map_err(|_| PyException::index_error("no such group"))?;
    if idx_usize > match_group_count(self_obj) {
        return Err(PyException::index_error("no such group"));
    }
    Ok(idx)
}

fn match_bytearray_group(self_obj: &PyObjectRef, idx: i64) -> Option<PyObjectRef> {
    if !match_is_bytes(self_obj) {
        return None;
    }
    let source = self_obj.get_attr("_bytearray_source")?;
    let PyObjectPayload::ByteArray(bytes) = &source.payload else {
        return None;
    };
    let (start, end) = if idx == 0 {
        (
            self_obj.get_attr("_start")?.to_int().ok()?,
            self_obj.get_attr("_end")?.to_int().ok()?,
        )
    } else {
        let group_spans = self_obj.get_attr("_group_spans")?;
        let PyObjectPayload::Tuple(items) = &group_spans.payload else {
            return None;
        };
        let item = items.get((idx - 1) as usize)?;
        match &item.payload {
            PyObjectPayload::None => return Some(PyObject::none()),
            PyObjectPayload::Tuple(span) if span.len() == 2 => {
                (span[0].to_int().ok()?, span[1].to_int().ok()?)
            }
            _ => return None,
        }
    };
    let len = bytes.len();
    let start = start.max(0) as usize;
    let end = end.max(0) as usize;
    if start >= len || start >= end {
        return Some(PyObject::bytes(Vec::new()));
    }
    Some(PyObject::bytes(bytes[start..end.min(len)].to_vec()))
}

fn match_group_one(self_obj: &PyObjectRef, arg: Option<&PyObjectRef>) -> PyResult<PyObjectRef> {
    let idx = match arg {
        Some(arg) => match_group_index(self_obj, arg)?,
        None => 0,
    };
    if let Some(value) = match_bytearray_group(self_obj, idx) {
        return Ok(value);
    }
    if idx == 0 {
        return self_obj
            .get_attr("_match")
            .ok_or_else(|| PyException::index_error("no such group"));
    }
    if let Some(groups) = self_obj.get_attr("_groups") {
        if let PyObjectPayload::Tuple(items) = &groups.payload {
            let item_idx = (idx - 1) as usize;
            if item_idx < items.len() {
                return Ok(items[item_idx].clone());
            }
        }
    }
    Err(PyException::index_error("no such group"))
}

fn match_group(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("group() needs self"));
    }
    let self_obj = &args[0];
    if args.len() <= 2 {
        return match_group_one(self_obj, args.get(1));
    }
    let mut items = Vec::with_capacity(args.len() - 1);
    for arg in &args[1..] {
        items.push(match_group_one(self_obj, Some(arg))?);
    }
    Ok(PyObject::tuple(items))
}

fn match_groupdict(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("groupdict() needs self"));
    }
    let self_obj = &args[0];
    let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
    let mut result = IndexMap::new();
    if let Some(groupindex) = self_obj.get_attr("_groupindex") {
        if let PyObjectPayload::Dict(d) = &groupindex.payload {
            if let Some(groups) = self_obj.get_attr("_groups") {
                if let PyObjectPayload::Tuple(items) = &groups.payload {
                    for (key, idx_obj) in d.read().iter() {
                        let idx = idx_obj.to_int().unwrap_or(0);
                        let i = (idx - 1) as usize;
                        let val = if i < items.len() {
                            if matches!(items[i].payload, PyObjectPayload::None) {
                                default.clone()
                            } else {
                                items[i].clone()
                            }
                        } else {
                            default.clone()
                        };
                        result.insert(key.clone(), val);
                    }
                }
            }
        }
    }
    Ok(PyObject::dict(result))
}

fn match_groups(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("groups() needs self"));
    }
    let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
    if let Some(groups) = args[0].get_attr("_groups") {
        if let PyObjectPayload::Tuple(items) = &groups.payload {
            let values: Vec<PyObjectRef> = items
                .iter()
                .map(|item| {
                    if matches!(item.payload, PyObjectPayload::None) {
                        default.clone()
                    } else {
                        item.clone()
                    }
                })
                .collect();
            return Ok(PyObject::tuple(values));
        }
    }
    Ok(PyObject::tuple(vec![]))
}

fn match_span_bounds(self_obj: &PyObjectRef, arg: Option<&PyObjectRef>) -> PyResult<(i64, i64)> {
    let idx = match arg {
        Some(arg) => match_group_index(self_obj, arg)?,
        None => 0,
    };
    if idx == 0 {
        let start = self_obj
            .get_attr("_start")
            .and_then(|v| v.to_int().ok())
            .unwrap_or(0);
        let end = self_obj
            .get_attr("_end")
            .and_then(|v| v.to_int().ok())
            .unwrap_or(0);
        return Ok((start, end));
    }
    if let Some(group_spans) = self_obj.get_attr("_group_spans") {
        if let PyObjectPayload::Tuple(items) = &group_spans.payload {
            let item_idx = (idx - 1) as usize;
            if item_idx < items.len() {
                if let PyObjectPayload::Tuple(span) = &items[item_idx].payload {
                    if span.len() == 2 {
                        return Ok((
                            span[0].to_int().unwrap_or(-1),
                            span[1].to_int().unwrap_or(-1),
                        ));
                    }
                }
            }
        }
    }
    Err(PyException::index_error("no such group"))
}

fn match_start(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("start() needs self"));
    }
    let (start, _) = match_span_bounds(&args[0], args.get(1))?;
    Ok(PyObject::int(start))
}

fn match_end(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("end() needs self"));
    }
    let (_, end) = match_span_bounds(&args[0], args.get(1))?;
    Ok(PyObject::int(end))
}

fn match_span(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("span() needs self"));
    }
    let (start, end) = match_span_bounds(&args[0], args.get(1))?;
    Ok(py_span(start, end))
}

fn match_is_bytes(self_obj: &PyObjectRef) -> bool {
    self_obj
        .get_attr("_is_bytes")
        .map(|flag| flag.is_truthy())
        .unwrap_or(false)
}

fn match_group_template_text(self_obj: &PyObjectRef, group: PyObjectRef) -> PyResult<String> {
    let value = match_group_one(self_obj, Some(&group))?;
    if matches!(value.payload, PyObjectPayload::None) {
        return Ok(String::new());
    }
    if let Some(bytes) = extract_bytes_like(&value) {
        return Ok(bytes_to_regex_text(&bytes));
    }
    if let Some(text) = extract_str_like(&value) {
        return Ok(text);
    }
    Ok(value.py_to_string())
}

fn push_octal_escape(result: &mut String, digits: &str) {
    if let Ok(value) = u32::from_str_radix(digits, 8) {
        if let Some(ch) = char::from_u32(value) {
            result.push(ch);
        }
    }
}

fn expand_match_template(self_obj: &PyObjectRef, template: &str) -> PyResult<String> {
    let chars: Vec<char> = template.chars().collect();
    let mut result = String::with_capacity(template.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '\\' {
            result.push(chars[i]);
            i += 1;
            continue;
        }
        if i + 1 >= chars.len() {
            result.push('\\');
            break;
        }
        let next = chars[i + 1];
        match next {
            'g' if i + 2 < chars.len() && chars[i + 2] == '<' => {
                let mut j = i + 3;
                while j < chars.len() && chars[j] != '>' {
                    j += 1;
                }
                let name: String = chars[i + 3..j].iter().collect();
                let group_arg = if name.chars().all(|ch| ch.is_ascii_digit()) {
                    PyObject::int(name.parse::<i64>().unwrap_or(0))
                } else {
                    PyObject::str_val(CompactString::from(name))
                };
                result.push_str(&match_group_template_text(self_obj, group_arg)?);
                i = if j < chars.len() { j + 1 } else { j };
            }
            '0' => {
                let mut j = i + 1;
                let mut digits = String::new();
                while j < chars.len() && digits.len() < 3 && matches!(chars[j], '0'..='7') {
                    digits.push(chars[j]);
                    j += 1;
                }
                push_octal_escape(&mut result, &digits);
                i = j;
            }
            '1'..='9' => {
                let mut j = i + 1;
                let mut digits = String::new();
                while j < chars.len() && chars[j].is_ascii_digit() {
                    digits.push(chars[j]);
                    j += 1;
                }
                result.push_str(&match_group_template_text(
                    self_obj,
                    PyObject::int(digits.parse::<i64>().unwrap_or(0)),
                )?);
                i = j;
            }
            'a' => {
                result.push('\x07');
                i += 2;
            }
            'b' => {
                result.push('\x08');
                i += 2;
            }
            'f' => {
                result.push('\x0c');
                i += 2;
            }
            'n' => {
                result.push('\n');
                i += 2;
            }
            'r' => {
                result.push('\r');
                i += 2;
            }
            't' => {
                result.push('\t');
                i += 2;
            }
            'v' => {
                result.push('\x0b');
                i += 2;
            }
            '\\' => {
                result.push('\\');
                i += 2;
            }
            _ => {
                result.push(next);
                i += 2;
            }
        }
    }
    Ok(result)
}

fn match_expand(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("expand() needs self and template"));
    }
    let is_bytes = match_is_bytes(&args[0]);
    let template = extract_re_replacement(&args[1], is_bytes)?;
    let expanded = expand_match_template(&args[0], &template)?;
    Ok(py_re_text(&expanded, is_bytes))
}

/// Match.__getitem__: m[0], m[1], m['name'] — delegates to match_group
fn match_getitem(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "Match.__getitem__() requires self and index",
        ));
    }
    // Repack as [self, index] for match_group
    match_group(args)
}

// Public wrappers for match object methods (used by VM re_sub_with_callable)
pub fn match_group_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match_group(args)
}
pub fn match_groups_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match_groups(args)
}
pub fn match_groupdict_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match_groupdict(args)
}
pub fn match_start_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match_start(args)
}
pub fn match_end_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match_end(args)
}
pub fn match_span_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match_span(args)
}
