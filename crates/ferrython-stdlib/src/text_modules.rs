//! Text processing stdlib modules (string, re, textwrap, fnmatch)

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    IteratorData,
    make_module, make_builtin, check_args_min,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::sync::Arc;

use super::fs_modules::glob_match;

pub fn create_string_module() -> PyObjectRef {
    make_module("string", vec![
        ("ascii_lowercase", PyObject::str_val(CompactString::from("abcdefghijklmnopqrstuvwxyz"))),
        ("ascii_uppercase", PyObject::str_val(CompactString::from("ABCDEFGHIJKLMNOPQRSTUVWXYZ"))),
        ("ascii_letters", PyObject::str_val(CompactString::from("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"))),
        ("digits", PyObject::str_val(CompactString::from("0123456789"))),
        ("hexdigits", PyObject::str_val(CompactString::from("0123456789abcdefABCDEF"))),
        ("octdigits", PyObject::str_val(CompactString::from("01234567"))),
        ("punctuation", PyObject::str_val(CompactString::from("!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~"))),
        ("whitespace", PyObject::str_val(CompactString::from(" \t\n\r\x0b\x0c"))),
        ("printable", PyObject::str_val(CompactString::from("0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~ \t\n\r\x0b\x0c"))),
        ("Template", PyObject::native_function("string.Template", template_new)),
    ])
}

fn template_substitute(template: &str, kwargs: &IndexMap<HashableKey, PyObjectRef>, safe: bool) -> PyResult<String> {
    let mut result = String::new();
    let chars: Vec<char> = template.chars().collect();
    let len = chars.len();
    let mut i = 0;
    while i < len {
        if chars[i] == '$' && i + 1 < len {
            if chars[i + 1] == '$' {
                // Escaped $$
                result.push('$');
                i += 2;
            } else if chars[i + 1] == '{' {
                // ${name} form
                let start = i + 2;
                if let Some(end_pos) = chars[start..].iter().position(|&c| c == '}') {
                    let name: String = chars[start..start + end_pos].iter().collect();
                    let key = HashableKey::Str(CompactString::from(&name));
                    if let Some(val) = kwargs.get(&key) {
                        result.push_str(&val.py_to_string());
                    } else if safe {
                        result.push_str(&format!("${{{}}}", name));
                    } else {
                        return Err(PyException::key_error(format!("'{}'", name)));
                    }
                    i = start + end_pos + 1;
                } else {
                    result.push('$');
                    i += 1;
                }
            } else if chars[i + 1].is_alphanumeric() || chars[i + 1] == '_' {
                // $name form
                let start = i + 1;
                let mut end = start;
                while end < len && (chars[end].is_alphanumeric() || chars[end] == '_') {
                    end += 1;
                }
                let name: String = chars[start..end].iter().collect();
                let key = HashableKey::Str(CompactString::from(&name));
                if let Some(val) = kwargs.get(&key) {
                    result.push_str(&val.py_to_string());
                } else if safe {
                    result.push('$');
                    result.push_str(&name);
                } else {
                    return Err(PyException::key_error(format!("'{}'", name)));
                }
                i = end;
            } else {
                result.push('$');
                i += 1;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    Ok(result)
}

fn extract_kwargs_dict(args: &[PyObjectRef]) -> IndexMap<HashableKey, PyObjectRef> {
    for arg in args.iter().rev() {
        if let PyObjectPayload::Dict(d) = &arg.payload {
            return d.read().clone();
        }
    }
    IndexMap::new()
}

fn template_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("Template() requires a template string")); }
    let tmpl_str = args[0].py_to_string();
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("template"), PyObject::str_val(CompactString::from(tmpl_str)));
    attrs.insert(CompactString::from("substitute"), PyObject::native_function("Template.substitute", template_substitute_method));
    attrs.insert(CompactString::from("safe_substitute"), PyObject::native_function("Template.safe_substitute", template_safe_substitute_method));
    attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));
    Ok(PyObject::module_with_attrs(CompactString::from("Template"), attrs))
}

fn template_substitute_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("substitute() needs self")); }
    let self_obj = &args[0];
    let tmpl = self_obj.get_attr("template").ok_or(PyException::attribute_error("template"))?.py_to_string();
    let kwargs = extract_kwargs_dict(&args[1..]);
    let result = template_substitute(&tmpl, &kwargs, false)?;
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn template_safe_substitute_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("safe_substitute() needs self")); }
    let self_obj = &args[0];
    let tmpl = self_obj.get_attr("template").ok_or(PyException::attribute_error("template"))?.py_to_string();
    let kwargs = extract_kwargs_dict(&args[1..]);
    let result = template_substitute(&tmpl, &kwargs, true)?;
    Ok(PyObject::str_val(CompactString::from(result)))
}

// ── json module (basic) ──


pub fn create_re_module() -> PyObjectRef {
    make_module("re", vec![
        ("IGNORECASE", PyObject::int(2)),
        ("I", PyObject::int(2)),
        ("MULTILINE", PyObject::int(8)),
        ("M", PyObject::int(8)),
        ("DOTALL", PyObject::int(16)),
        ("S", PyObject::int(16)),
        ("VERBOSE", PyObject::int(64)),
        ("X", PyObject::int(64)),
        ("match", PyObject::native_function("re.match", re_match)),
        ("search", PyObject::native_function("re.search", re_search)),
        ("findall", PyObject::native_function("re.findall", re_findall)),
        ("finditer", PyObject::native_function("re.finditer", re_finditer)),
        ("sub", PyObject::native_function("re.sub", re_sub)),
        ("subn", PyObject::native_function("re.subn", re_subn)),
        ("split", PyObject::native_function("re.split", re_split)),
        ("compile", PyObject::native_function("re.compile", re_compile)),
        ("escape", PyObject::native_function("re.escape", re_escape)),
        ("fullmatch", PyObject::native_function("re.fullmatch", re_fullmatch)),
    ])
}

fn convert_python_regex(pattern: &str) -> String {
    // Convert Python regex syntax to Rust regex syntax
    // Most are compatible, but a few need translation
    let result = pattern.to_string();
    // Python uses (?P<name>) for named groups, Rust regex uses (?P<name>) too — compatible!
    // Python uses \d, \w, \s etc — compatible
    // Python uses (?:...) for non-capturing groups — compatible
    result
}

fn build_regex(pattern: &str, flags: i64) -> Result<regex::Regex, PyException> {
    let mut pat = convert_python_regex(pattern);
    // Apply flags as inline flags
    let mut prefix = String::new();
    if flags & 2 != 0 { prefix.push_str("(?i)"); }
    if flags & 8 != 0 { prefix.push_str("(?m)"); }
    if flags & 16 != 0 { prefix.push_str("(?s)"); }
    pat = format!("{}{}", prefix, pat);
    regex::Regex::new(&pat).map_err(|e| PyException::runtime_error(format!("re: {}", e)))
}

fn make_match_object(m: regex::Match, text: &str, re_obj: &regex::Regex) -> PyObjectRef {
    let full_match = m.as_str().to_string();
    let start = m.start() as i64;
    let end = m.end() as i64;
    // groups - store captured groups
    let captures = re_obj.captures(text);
    let mut groups = Vec::new();
    if let Some(caps) = &captures {
        for i in 1..caps.len() {
            if let Some(g) = caps.get(i) {
                groups.push(PyObject::str_val(CompactString::from(g.as_str().to_string())));
            } else {
                groups.push(PyObject::none());
            }
        }
    }
    let groups_tuple = PyObject::tuple(groups);
    // Build name→index mapping for named capture groups
    let mut groupindex_map = IndexMap::new();
    for (i, name_opt) in re_obj.capture_names().enumerate() {
        if let Some(name) = name_opt {
            groupindex_map.insert(
                HashableKey::Str(CompactString::from(name)),
                PyObject::int(i as i64),
            );
        }
    }
    let groupindex = PyObject::dict(groupindex_map);
    // Build the match object with pre-bound data attributes
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("_match"), PyObject::str_val(CompactString::from(full_match)));
    attrs.insert(CompactString::from("_start"), PyObject::int(start));
    attrs.insert(CompactString::from("_end"), PyObject::int(end));
    attrs.insert(CompactString::from("_text"), PyObject::str_val(CompactString::from(text.to_string())));
    attrs.insert(CompactString::from("_groups"), groups_tuple);
    attrs.insert(CompactString::from("_groupindex"), groupindex);
    attrs.insert(CompactString::from("group"), PyObject::native_function("Match.group", match_group));
    attrs.insert(CompactString::from("groups"), PyObject::native_function("Match.groups", match_groups));
    attrs.insert(CompactString::from("groupdict"), PyObject::native_function("Match.groupdict", match_groupdict));
    attrs.insert(CompactString::from("start"), PyObject::native_function("Match.start", match_start));
    attrs.insert(CompactString::from("end"), PyObject::native_function("Match.end", match_end));
    attrs.insert(CompactString::from("span"), PyObject::native_function("Match.span", match_span));
    attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));
    let match_obj = PyObject::module_with_attrs(CompactString::from("Match"), attrs);
    match_obj
}

fn match_group(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("group() needs self")); }
    let self_obj = &args[0];
    if args.len() <= 1 {
        // group() with no args returns full match
        if let Some(m) = self_obj.get_attr("_match") {
            return Ok(m);
        }
    }
    if args.len() > 1 {
        // Check if arg is a string (named group)
        if let PyObjectPayload::Str(name) = &args[1].payload {
            if let Some(groupindex) = self_obj.get_attr("_groupindex") {
                if let PyObjectPayload::Dict(d) = &groupindex.payload {
                    let key = HashableKey::Str(name.clone());
                    if let Some(idx_obj) = d.read().get(&key).cloned() {
                        let idx = idx_obj.to_int().unwrap_or(0);
                        if idx == 0 {
                            if let Some(m) = self_obj.get_attr("_match") {
                                return Ok(m);
                            }
                        }
                        if let Some(groups) = self_obj.get_attr("_groups") {
                            if let PyObjectPayload::Tuple(items) = &groups.payload {
                                let i = (idx - 1) as usize;
                                if i < items.len() {
                                    return Ok(items[i].clone());
                                }
                            }
                        }
                    }
                }
            }
            return Err(PyException::index_error(format!("no such group: '{}'", name)));
        }
        // Numeric group
        let group_num = args[1].to_int().unwrap_or(0);
        if group_num == 0 {
            if let Some(m) = self_obj.get_attr("_match") {
                return Ok(m);
            }
        }
        if let Some(groups) = self_obj.get_attr("_groups") {
            if let PyObjectPayload::Tuple(items) = &groups.payload {
                let idx = (group_num - 1) as usize;
                if idx < items.len() {
                    return Ok(items[idx].clone());
                }
            }
        }
    }
    Err(PyException::index_error("no such group"))
}

fn match_groupdict(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("groupdict() needs self")); }
    let self_obj = &args[0];
    let mut result = IndexMap::new();
    if let Some(groupindex) = self_obj.get_attr("_groupindex") {
        if let PyObjectPayload::Dict(d) = &groupindex.payload {
            if let Some(groups) = self_obj.get_attr("_groups") {
                if let PyObjectPayload::Tuple(items) = &groups.payload {
                    for (key, idx_obj) in d.read().iter() {
                        let idx = idx_obj.to_int().unwrap_or(0);
                        let i = (idx - 1) as usize;
                        let val = if i < items.len() { items[i].clone() } else { PyObject::none() };
                        result.insert(key.clone(), val);
                    }
                }
            }
        }
    }
    Ok(PyObject::dict(result))
}

fn match_groups(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("groups() needs self")); }
    if let Some(groups) = args[0].get_attr("_groups") {
        return Ok(groups);
    }
    Ok(PyObject::tuple(vec![]))
}

fn match_start(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("start() needs self")); }
    if let Some(s) = args[0].get_attr("_start") { return Ok(s); }
    Ok(PyObject::int(0))
}

fn match_end(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("end() needs self")); }
    if let Some(e) = args[0].get_attr("_end") { return Ok(e); }
    Ok(PyObject::int(0))
}

fn match_span(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("span() needs self")); }
    let start = args[0].get_attr("_start").unwrap_or(PyObject::int(0));
    let end = args[0].get_attr("_end").unwrap_or(PyObject::int(0));
    Ok(PyObject::tuple(vec![start, end]))
}

fn re_match(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("re.match() requires pattern and string")); }
    let pattern = args[0].py_to_string();
    let text = args[1].py_to_string();
    let flags = if args.len() > 2 { args[2].to_int().unwrap_or(0) } else { 0 };
    // re.match anchors at start
    let anchored = format!("^(?:{})", pattern);
    let re = build_regex(&anchored, flags)?;
    match re.find(&text) {
        Some(m) => {
            let orig_re = build_regex(&pattern, flags)?;
            Ok(make_match_object(m, &text, &orig_re))
        }
        None => Ok(PyObject::none()),
    }
}

fn re_search(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("re.search() requires pattern and string")); }
    let pattern = args[0].py_to_string();
    let text = args[1].py_to_string();
    let flags = if args.len() > 2 { args[2].to_int().unwrap_or(0) } else { 0 };
    let re = build_regex(&pattern, flags)?;
    match re.find(&text) {
        Some(m) => Ok(make_match_object(m, &text, &re)),
        None => Ok(PyObject::none()),
    }
}

fn re_fullmatch(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("re.fullmatch() requires pattern and string")); }
    let pattern = args[0].py_to_string();
    let text = args[1].py_to_string();
    let flags = if args.len() > 2 { args[2].to_int().unwrap_or(0) } else { 0 };
    let anchored = format!("^(?:{})$", pattern);
    let re = build_regex(&anchored, flags)?;
    let orig_re = build_regex(&pattern, flags)?;
    match re.find(&text) {
        Some(m) => Ok(make_match_object(m, &text, &orig_re)),
        None => Ok(PyObject::none()),
    }
}

fn re_findall(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("re.findall() requires pattern and string")); }
    let pattern = args[0].py_to_string();
    let text = args[1].py_to_string();
    let flags = if args.len() > 2 { args[2].to_int().unwrap_or(0) } else { 0 };
    let re = build_regex(&pattern, flags)?;
    // If pattern has groups, return group(1) for single group, tuple for multiple
    let cap_count = re.captures_len() - 1;
    if cap_count == 0 {
        let results: Vec<PyObjectRef> = re.find_iter(&text)
            .map(|m| PyObject::str_val(CompactString::from(m.as_str())))
            .collect();
        Ok(PyObject::list(results))
    } else if cap_count == 1 {
        let results: Vec<PyObjectRef> = re.captures_iter(&text)
            .filter_map(|caps| caps.get(1).map(|m| PyObject::str_val(CompactString::from(m.as_str()))))
            .collect();
        Ok(PyObject::list(results))
    } else {
        let results: Vec<PyObjectRef> = re.captures_iter(&text)
            .map(|caps| {
                let groups: Vec<PyObjectRef> = (1..=cap_count)
                    .map(|i| caps.get(i)
                        .map(|m| PyObject::str_val(CompactString::from(m.as_str())))
                        .unwrap_or(PyObject::none()))
                    .collect();
                PyObject::tuple(groups)
            })
            .collect();
        Ok(PyObject::list(results))
    }
}

fn re_finditer(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("re.finditer() requires pattern and string")); }
    let pattern = args[0].py_to_string();
    let text = args[1].py_to_string();
    let flags = if args.len() > 2 { args[2].to_int().unwrap_or(0) } else { 0 };
    let re = build_regex(&pattern, flags)?;
    let matches: Vec<PyObjectRef> = re.find_iter(&text)
        .map(|m| make_match_object(m, &text, &re))
        .collect();
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(std::sync::Mutex::new(
        IteratorData::List { items: matches, index: 0 }
    )))))
}

fn re_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 { return Err(PyException::type_error("re.sub() requires pattern, repl, and string")); }
    let pattern = args[0].py_to_string();
    let repl = args[1].py_to_string();
    let text = args[2].py_to_string();
    // count and flags can be positional or in trailing kwargs dict
    let mut count = if args.len() > 3 && !matches!(&args[3].payload, PyObjectPayload::Dict(_)) {
        args[3].to_int().unwrap_or(0) as usize
    } else { 0 };
    let mut flags = if args.len() > 4 && !matches!(&args[4].payload, PyObjectPayload::Dict(_)) {
        args[4].to_int().unwrap_or(0)
    } else { 0 };
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
    let re = build_regex(&pattern, flags)?;
    let result = if count == 0 {
        re.replace_all(&text, repl.as_str()).to_string()
    } else {
        re.replacen(&text, count, repl.as_str()).to_string()
    };
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn re_subn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 { return Err(PyException::type_error("re.subn() requires pattern, repl, and string")); }
    let pattern = args[0].py_to_string();
    let repl = args[1].py_to_string();
    let text = args[2].py_to_string();
    let flags = if args.len() > 3 { args[3].to_int().unwrap_or(0) } else { 0 };
    let re = build_regex(&pattern, flags)?;
    let count = re.find_iter(&text).count();
    let result = re.replace_all(&text, repl.as_str()).to_string();
    Ok(PyObject::tuple(vec![
        PyObject::str_val(CompactString::from(result)),
        PyObject::int(count as i64),
    ]))
}

fn re_split(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("re.split() requires pattern and string")); }
    let pattern = args[0].py_to_string();
    let text = args[1].py_to_string();
    let maxsplit = if args.len() > 2 { args[2].to_int().unwrap_or(0) as usize } else { 0 };
    let flags = if args.len() > 3 { args[3].to_int().unwrap_or(0) } else { 0 };
    let re = build_regex(&pattern, flags)?;
    let parts: Vec<PyObjectRef> = if maxsplit == 0 {
        re.split(&text).map(|s| PyObject::str_val(CompactString::from(s))).collect()
    } else {
        re.splitn(&text, maxsplit + 1).map(|s| PyObject::str_val(CompactString::from(s))).collect()
    };
    Ok(PyObject::list(parts))
}

fn re_compile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("re.compile() requires a pattern")); }
    let pattern = args[0].py_to_string();
    let flags = if args.len() > 1 { args[1].to_int().unwrap_or(0) } else { 0 };
    // Validate the pattern compiles
    let _ = build_regex(&pattern, flags)?;
    // Return a compiled pattern object with match/search/findall etc.
    let pat_str = PyObject::str_val(CompactString::from(pattern.clone()));
    let flags_obj = PyObject::int(flags);
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("pattern"), pat_str);
    attrs.insert(CompactString::from("flags"), flags_obj);
    attrs.insert(CompactString::from("match"), PyObject::native_function("Pattern.match", compiled_match));
    attrs.insert(CompactString::from("search"), PyObject::native_function("Pattern.search", compiled_search));
    attrs.insert(CompactString::from("findall"), PyObject::native_function("Pattern.findall", compiled_findall));
    attrs.insert(CompactString::from("sub"), PyObject::native_function("Pattern.sub", compiled_sub));
    attrs.insert(CompactString::from("split"), PyObject::native_function("Pattern.split", compiled_split));
    attrs.insert(CompactString::from("fullmatch"), PyObject::native_function("Pattern.fullmatch", compiled_fullmatch));
    attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));
    Ok(PyObject::module_with_attrs(CompactString::from("Pattern"), attrs))
}

fn compiled_match(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("Pattern.match() requires self and string")); }
    let self_obj = &args[0];
    let pattern = self_obj.get_attr("pattern").ok_or(PyException::attribute_error("pattern"))?.py_to_string();
    let flags = self_obj.get_attr("flags").and_then(|f| f.to_int().ok()).unwrap_or(0);
    re_match(&[PyObject::str_val(CompactString::from(pattern)), args[1].clone(), PyObject::int(flags)])
}

fn compiled_search(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("Pattern.search() requires self and string")); }
    let self_obj = &args[0];
    let pattern = self_obj.get_attr("pattern").ok_or(PyException::attribute_error("pattern"))?.py_to_string();
    let flags = self_obj.get_attr("flags").and_then(|f| f.to_int().ok()).unwrap_or(0);
    re_search(&[PyObject::str_val(CompactString::from(pattern)), args[1].clone(), PyObject::int(flags)])
}

fn compiled_findall(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("Pattern.findall() requires self and string")); }
    let self_obj = &args[0];
    let pattern = self_obj.get_attr("pattern").ok_or(PyException::attribute_error("pattern"))?.py_to_string();
    let flags = self_obj.get_attr("flags").and_then(|f| f.to_int().ok()).unwrap_or(0);
    re_findall(&[PyObject::str_val(CompactString::from(pattern)), args[1].clone(), PyObject::int(flags)])
}

fn compiled_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 { return Err(PyException::type_error("Pattern.sub() requires self, repl, and string")); }
    let self_obj = &args[0];
    let pattern = self_obj.get_attr("pattern").ok_or(PyException::attribute_error("pattern"))?.py_to_string();
    let flags = self_obj.get_attr("flags").and_then(|f| f.to_int().ok()).unwrap_or(0);
    re_sub(&[PyObject::str_val(CompactString::from(pattern)), args[1].clone(), args[2].clone(), PyObject::int(0), PyObject::int(flags)])
}

fn compiled_split(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("Pattern.split() requires self and string")); }
    let self_obj = &args[0];
    let pattern = self_obj.get_attr("pattern").ok_or(PyException::attribute_error("pattern"))?.py_to_string();
    let flags = self_obj.get_attr("flags").and_then(|f| f.to_int().ok()).unwrap_or(0);
    re_split(&[PyObject::str_val(CompactString::from(pattern)), args[1].clone(), PyObject::int(0), PyObject::int(flags)])
}

fn compiled_fullmatch(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("Pattern.fullmatch() requires self and string")); }
    let self_obj = &args[0];
    let pattern = self_obj.get_attr("pattern").ok_or(PyException::attribute_error("pattern"))?.py_to_string();
    let flags = self_obj.get_attr("flags").and_then(|f| f.to_int().ok()).unwrap_or(0);
    re_fullmatch(&[PyObject::str_val(CompactString::from(pattern)), args[1].clone(), PyObject::int(flags)])
}

fn re_escape(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("re.escape() requires a string")); }
    let s = args[0].py_to_string();
    let escaped = regex::escape(&s);
    Ok(PyObject::str_val(CompactString::from(escaped)))
}


fn extract_textwrap_width(args: &[PyObjectRef], default: usize) -> usize {
    // Check positional arg first
    if args.len() >= 2 {
        if let Ok(v) = args[1].to_int() {
            return v as usize;
        }
    }
    // Check trailing kwargs dict for "width"
    for arg in args.iter().rev() {
        if let PyObjectPayload::Dict(d) = &arg.payload {
            if let Some(v) = d.read().get(&HashableKey::Str(CompactString::from("width"))) {
                if let Ok(w) = v.to_int() {
                    return w as usize;
                }
            }
            break;
        }
    }
    default
}

pub fn create_textwrap_module() -> PyObjectRef {
    make_module("textwrap", vec![
        ("dedent", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("dedent requires 1 argument")); }
            let text = args[0].py_to_string();
            let mut min_indent = usize::MAX;
            for line in text.lines() {
                if line.trim().is_empty() { continue; }
                let indent = line.len() - line.trim_start().len();
                if indent < min_indent { min_indent = indent; }
            }
            if min_indent == usize::MAX || min_indent == 0 { return Ok(args[0].clone()); }
            // Extract the actual whitespace prefix to match (spaces/tabs)
            let prefix: &str = text.lines()
                .find(|l| !l.trim().is_empty() && l.len() - l.trim_start().len() == min_indent)
                .map(|l| &l[..min_indent])
                .unwrap_or("");
            let result: Vec<&str> = text.lines().map(|line| {
                if line.trim().is_empty() { line.trim() }
                else if line.starts_with(prefix) { &line[min_indent..] }
                else if line.len() >= min_indent { &line[min_indent..] }
                else { line }
            }).collect();
            Ok(PyObject::str_val(CompactString::from(result.join("\n"))))
        })),
        ("indent", make_builtin(|args| {
            check_args_min("indent", args, 2)?;
            let text = args[0].py_to_string();
            let prefix = args[1].py_to_string();
            let result: Vec<String> = text.lines().map(|line| {
                if line.trim().is_empty() { line.to_string() }
                else { format!("{}{}", prefix, line) }
            }).collect();
            Ok(PyObject::str_val(CompactString::from(result.join("\n"))))
        })),
        ("wrap", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("wrap requires 1 argument")); }
            let text = args[0].py_to_string();
            let width = extract_textwrap_width(args, 70);
            let words: Vec<&str> = text.split_whitespace().collect();
            let mut lines = Vec::new();
            let mut current = String::new();
            for word in words {
                if current.is_empty() {
                    current = word.to_string();
                } else if current.len() + 1 + word.len() <= width {
                    current.push(' ');
                    current.push_str(word);
                } else {
                    lines.push(PyObject::str_val(CompactString::from(current)));
                    current = word.to_string();
                }
            }
            if !current.is_empty() {
                lines.push(PyObject::str_val(CompactString::from(current)));
            }
            Ok(PyObject::list(lines))
        })),
        ("fill", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("fill requires 1 argument")); }
            let text = args[0].py_to_string();
            let width = extract_textwrap_width(args, 70);
            let words: Vec<&str> = text.split_whitespace().collect();
            let mut lines = Vec::new();
            let mut current = String::new();
            for word in words {
                if current.is_empty() {
                    current = word.to_string();
                } else if current.len() + 1 + word.len() <= width {
                    current.push(' ');
                    current.push_str(word);
                } else {
                    lines.push(current);
                    current = word.to_string();
                }
            }
            if !current.is_empty() { lines.push(current); }
            Ok(PyObject::str_val(CompactString::from(lines.join("\n"))))
        })),
        ("shorten", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("shorten requires text and width")); }
            let text = args[0].py_to_string();
            let width = extract_textwrap_width(args, 70);
            let placeholder = if args.len() >= 3 { args[2].py_to_string().to_string() } else { "...".to_string() };
            let words: Vec<&str> = text.split_whitespace().collect();
            let joined = words.join(" ");
            if joined.len() <= width {
                return Ok(PyObject::str_val(CompactString::from(joined)));
            }
            if width < placeholder.len() {
                return Ok(PyObject::str_val(CompactString::from(placeholder)));
            }
            let target = width - placeholder.len();
            let mut result = String::new();
            for word in &words {
                if result.is_empty() {
                    if word.len() > target { break; }
                    result = word.to_string();
                } else if result.len() + 1 + word.len() <= target {
                    result.push(' ');
                    result.push_str(word);
                } else {
                    break;
                }
            }
            result.push_str(&placeholder);
            Ok(PyObject::str_val(CompactString::from(result)))
        })),
    ])
}

// ── traceback module (stub) ──


pub fn create_fnmatch_module() -> PyObjectRef {
    make_module("fnmatch", vec![
        ("fnmatch", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("fnmatch requires name and pattern")); }
            let name = args[0].py_to_string();
            let pattern = args[1].py_to_string();
            Ok(PyObject::bool_val(glob_match(&pattern, &name)))
        })),
        ("fnmatchcase", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("fnmatchcase requires name and pattern")); }
            let name = args[0].py_to_string();
            let pattern = args[1].py_to_string();
            Ok(PyObject::bool_val(glob_match(&pattern, &name)))
        })),
        ("filter", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("filter requires names and pattern")); }
            let names = args[0].to_list()?;
            let pattern = args[1].py_to_string();
            let filtered: Vec<PyObjectRef> = names.iter()
                .filter(|n| glob_match(&pattern, &n.py_to_string()))
                .cloned().collect();
            Ok(PyObject::list(filtered))
        })),
    ])
}

// ── base64 module ──


