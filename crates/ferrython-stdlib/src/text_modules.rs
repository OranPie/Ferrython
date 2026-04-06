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
        ("Formatter", create_formatter_class()),
        ("capwords", make_builtin(string_capwords)),
    ])
}

fn string_capwords(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("capwords() requires a string")); }
    let s = args[0].py_to_string();
    let sep = if args.len() > 1 { Some(args[1].py_to_string()) } else { None };
    let result: String = match sep {
        Some(ref sep_str) => s.split(sep_str.as_str())
            .map(|w| { let mut c = w.chars(); match c.next() { None => String::new(), Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase() }})
            .collect::<Vec<_>>().join(sep_str),
        None => s.split_whitespace()
            .map(|w| { let mut c = w.chars(); match c.next() { None => String::new(), Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase() }})
            .collect::<Vec<_>>().join(" "),
    };
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn create_formatter_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(CompactString::from("format"), make_builtin(formatter_format));
    ns.insert(CompactString::from("vformat"), make_builtin(formatter_format));
    PyObject::class(CompactString::from("Formatter"), vec![], ns)
}

fn formatter_format(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // args[0] = self (Formatter instance), args[1] = format_string, rest = positional/kwargs
    if args.len() < 2 { return Err(PyException::type_error("format() requires a format string")); }
    let fmt_str = args[1].py_to_string();
    let pos_args = if args.len() > 2 { &args[2..] } else { &[] as &[PyObjectRef] };
    // Check if last arg is a kwargs dict
    let (pos_args_final, kwargs) = if let Some(last) = pos_args.last() {
        if let PyObjectPayload::Dict(map) = &last.payload {
            (&pos_args[..pos_args.len()-1], Some(map.read().clone()))
        } else {
            (pos_args, None)
        }
    } else {
        (pos_args, None)
    };
    let result = format_string_impl(&fmt_str, pos_args_final, &kwargs)?;
    Ok(PyObject::str_val(CompactString::from(result)))
}

fn format_string_impl(
    fmt: &str,
    pos_args: &[PyObjectRef],
    kwargs: &Option<IndexMap<HashableKey, PyObjectRef>>,
) -> PyResult<String> {
    let mut result = String::new();
    let mut auto_idx = 0usize;
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            if i + 1 < chars.len() && chars[i+1] == '{' {
                result.push('{');
                i += 2;
                continue;
            }
            i += 1;
            let start = i;
            let mut depth = 1;
            while i < chars.len() && depth > 0 {
                if chars[i] == '{' { depth += 1; }
                if chars[i] == '}' { depth -= 1; }
                if depth > 0 { i += 1; }
            }
            let field: String = chars[start..i].iter().collect();
            i += 1; // skip }
            // Parse field_name:format_spec
            let (field_name, _format_spec) = if let Some(colon) = field.find(':') {
                (&field[..colon], &field[colon+1..])
            } else {
                (field.as_str(), "")
            };
            let value = if field_name.is_empty() {
                if auto_idx < pos_args.len() {
                    let v = pos_args[auto_idx].clone();
                    auto_idx += 1;
                    v
                } else {
                    return Err(PyException::index_error("Replacement index out of range"));
                }
            } else if let Ok(idx) = field_name.parse::<usize>() {
                if idx < pos_args.len() { pos_args[idx].clone() }
                else { return Err(PyException::index_error("Replacement index out of range")); }
            } else if let Some(ref kw) = kwargs {
                kw.get(&HashableKey::Str(CompactString::from(field_name)))
                    .cloned()
                    .ok_or_else(|| PyException::key_error(format!("'{}'", field_name)))?
            } else {
                return Err(PyException::key_error(format!("'{}'", field_name)));
            };
            result.push_str(&value.py_to_string());
        } else if chars[i] == '}' {
            if i + 1 < chars.len() && chars[i+1] == '}' {
                result.push('}');
                i += 2;
                continue;
            }
            result.push('}');
            i += 1;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    Ok(result)
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

/// Convert Python replacement string syntax to Rust regex syntax.
/// Python uses `\1`, `\2`, `\g<name>`, `\g<1>` for backreferences.
/// Rust regex uses `$1`, `$2`, `$name`, `${1}`.
fn python_repl_to_rust(repl: &str) -> String {
    let mut result = String::with_capacity(repl.len());
    let bytes = repl.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            if next.is_ascii_digit() {
                // \1, \2, ... → $1, $2, ...
                result.push('$');
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    result.push(bytes[i] as char);
                    i += 1;
                }
            } else if next == b'g' && i + 2 < bytes.len() && bytes[i + 2] == b'<' {
                // \g<name> or \g<1> → $name or ${1}
                i += 3; // skip \g<
                let start = i;
                while i < bytes.len() && bytes[i] != b'>' {
                    i += 1;
                }
                let group = &repl[start..i];
                if i < bytes.len() { i += 1; } // skip >
                if group.bytes().all(|b| b.is_ascii_digit()) {
                    result.push_str(&format!("${{{}}}", group));
                } else {
                    result.push_str(&format!("${{{}}}", group));
                }
            } else if next == b'\\' {
                result.push('\\');
                i += 2;
            } else if next == b'n' {
                result.push('\n');
                i += 2;
            } else if next == b't' {
                result.push('\t');
                i += 2;
            } else {
                // Pass other escapes through
                result.push(bytes[i] as char);
                result.push(bytes[i + 1] as char);
                i += 2;
            }
        } else if bytes[i] == b'$' {
            // Escape literal $ to avoid Rust regex interpreting it
            result.push_str("$$");
            i += 1;
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    result
}

fn needs_fancy_regex(pattern: &str) -> bool {
    // Detect lookahead/lookbehind which require fancy-regex
    let bytes = pattern.as_bytes();
    let len = bytes.len();
    for i in 0..len.saturating_sub(1) {
        if bytes[i] == b'(' && bytes[i + 1] == b'?' && i + 2 < len {
            match bytes[i + 2] {
                b'=' | b'!' => return true,  // (?= (?!
                b'<' if i + 3 < len && (bytes[i + 3] == b'=' || bytes[i + 3] == b'!') => return true, // (?<= (?<!
                _ => {}
            }
        }
    }
    false
}

/// Strip VERBOSE (re.X) comments and unescaped whitespace from a regex pattern.
fn strip_verbose(pattern: &str) -> String {
    let mut result = String::with_capacity(pattern.len());
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    let mut in_char_class = false;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' && i + 1 < chars.len() {
            // Escaped character — always keep
            result.push(ch);
            result.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if ch == '[' && !in_char_class {
            in_char_class = true;
            result.push(ch);
            i += 1;
            continue;
        }
        if ch == ']' && in_char_class {
            in_char_class = false;
            result.push(ch);
            i += 1;
            continue;
        }
        if in_char_class {
            result.push(ch);
            i += 1;
            continue;
        }
        if ch == '#' {
            // Skip to end of line
            while i < chars.len() && chars[i] != '\n' { i += 1; }
            i += 1; // skip the newline too
            continue;
        }
        if ch.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        result.push(ch);
        i += 1;
    }
    result
}

fn build_regex(pattern: &str, flags: i64) -> Result<regex::Regex, PyException> {
    let mut pat = if flags & 64 != 0 { strip_verbose(pattern) } else { pattern.to_string() };
    pat = convert_python_regex(&pat);
    let mut prefix = String::new();
    if flags & 2 != 0 { prefix.push_str("(?i)"); }
    if flags & 8 != 0 { prefix.push_str("(?m)"); }
    if flags & 16 != 0 { prefix.push_str("(?s)"); }
    pat = format!("{}{}", prefix, pat);
    regex::Regex::new(&pat).map_err(|e| PyException::runtime_error(format!("re: {}", e)))
}

fn build_fancy_regex(pattern: &str, flags: i64) -> Result<fancy_regex::Regex, PyException> {
    let mut pat = if flags & 64 != 0 { strip_verbose(pattern) } else { pattern.to_string() };
    pat = convert_python_regex(&pat);
    let mut prefix = String::new();
    if flags & 2 != 0 { prefix.push_str("(?i)"); }
    if flags & 8 != 0 { prefix.push_str("(?m)"); }
    if flags & 16 != 0 { prefix.push_str("(?s)"); }
    pat = format!("{}{}", prefix, pat);
    fancy_regex::Regex::new(&pat).map_err(|e| PyException::runtime_error(format!("re: {}", e)))
}

fn fancy_find_all(re: &fancy_regex::Regex, text: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut pos = 0;
    while pos <= text.len() {
        match re.find(&text[pos..]) {
            Ok(Some(m)) => {
                if m.start() == m.end() { pos += 1; continue; }
                results.push(m.as_str().to_string());
                pos += m.end();
            }
            _ => break,
        }
    }
    results
}

fn fancy_captures(re: &fancy_regex::Regex, text: &str) -> Vec<Vec<Option<String>>> {
    let mut results = Vec::new();
    let mut pos = 0;
    while pos <= text.len() {
        match re.captures(&text[pos..]) {
            Ok(Some(caps)) => {
                let whole = caps.get(0).unwrap();
                if whole.start() == whole.end() { pos += 1; continue; }
                let mut groups = Vec::new();
                for i in 0..caps.len() {
                    groups.push(caps.get(i).map(|m| m.as_str().to_string()));
                }
                results.push(groups);
                pos += whole.end();
            }
            _ => break,
        }
    }
    results
}

fn make_fancy_match_object(text: &str, start: usize, end: usize, full: &str, groups: Vec<Option<String>>) -> PyObjectRef {
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("_match"), PyObject::str_val(CompactString::from(full)));
    attrs.insert(CompactString::from("_start"), PyObject::int(start as i64));
    attrs.insert(CompactString::from("_end"), PyObject::int(end as i64));
    attrs.insert(CompactString::from("_text"), PyObject::str_val(CompactString::from(text)));
    let group_objs: Vec<PyObjectRef> = groups.into_iter()
        .map(|g| g.map(|s| PyObject::str_val(CompactString::from(s))).unwrap_or(PyObject::none()))
        .collect();
    attrs.insert(CompactString::from("_groups"), PyObject::tuple(group_objs));
    attrs.insert(CompactString::from("_groupindex"), PyObject::dict(IndexMap::new()));
    attrs.insert(CompactString::from("group"), PyObject::native_function("Match.group", match_group));
    attrs.insert(CompactString::from("groups"), PyObject::native_function("Match.groups", match_groups));
    attrs.insert(CompactString::from("groupdict"), PyObject::native_function("Match.groupdict", match_groupdict));
    attrs.insert(CompactString::from("start"), PyObject::native_function("Match.start", match_start));
    attrs.insert(CompactString::from("end"), PyObject::native_function("Match.end", match_end));
    attrs.insert(CompactString::from("span"), PyObject::native_function("Match.span", match_span));
    attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));
    PyObject::module_with_attrs(CompactString::from("Match"), attrs)
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
    let anchored = format!("^(?:{})", pattern);
    if needs_fancy_regex(&pattern) {
        let re = build_fancy_regex(&anchored, flags)?;
        match re.captures(&text) {
            Ok(Some(caps)) => {
                let whole = caps.get(0).unwrap();
                let groups: Vec<Option<String>> = (1..caps.len())
                    .map(|i| caps.get(i).map(|m| m.as_str().to_string())).collect();
                Ok(make_fancy_match_object(&text, whole.start(), whole.end(), whole.as_str(), groups))
            }
            _ => Ok(PyObject::none()),
        }
    } else {
        let re = build_regex(&anchored, flags)?;
        match re.find(&text) {
            Some(m) => {
                let orig_re = build_regex(&pattern, flags)?;
                Ok(make_match_object(m, &text, &orig_re))
            }
            None => Ok(PyObject::none()),
        }
    }
}

fn re_search(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("re.search() requires pattern and string")); }
    let pattern = args[0].py_to_string();
    let text = args[1].py_to_string();
    let flags = if args.len() > 2 { args[2].to_int().unwrap_or(0) } else { 0 };
    if needs_fancy_regex(&pattern) {
        let re = build_fancy_regex(&pattern, flags)?;
        match re.captures(&text) {
            Ok(Some(caps)) => {
                let whole = caps.get(0).unwrap();
                let groups: Vec<Option<String>> = (1..caps.len())
                    .map(|i| caps.get(i).map(|m| m.as_str().to_string())).collect();
                Ok(make_fancy_match_object(&text, whole.start(), whole.end(), whole.as_str(), groups))
            }
            _ => Ok(PyObject::none()),
        }
    } else {
        let re = build_regex(&pattern, flags)?;
        match re.find(&text) {
            Some(m) => Ok(make_match_object(m, &text, &re)),
            None => Ok(PyObject::none()),
        }
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
    if needs_fancy_regex(&pattern) {
        let re = build_fancy_regex(&pattern, flags)?;
        // Determine capture group count from first match
        let all_caps = fancy_captures(&re, &text);
        if all_caps.is_empty() { return Ok(PyObject::list(vec![])); }
        let cap_count = all_caps[0].len() - 1;
        if cap_count == 0 {
            let results: Vec<PyObjectRef> = fancy_find_all(&re, &text)
                .into_iter().map(|s| PyObject::str_val(CompactString::from(s))).collect();
            Ok(PyObject::list(results))
        } else if cap_count == 1 {
            let results: Vec<PyObjectRef> = all_caps.into_iter()
                .filter_map(|g| g.get(1).cloned().flatten().map(|s| PyObject::str_val(CompactString::from(s))))
                .collect();
            Ok(PyObject::list(results))
        } else {
            let results: Vec<PyObjectRef> = all_caps.into_iter()
                .map(|g| {
                    let items: Vec<PyObjectRef> = g[1..].iter()
                        .map(|o| o.as_ref().map(|s| PyObject::str_val(CompactString::from(s.as_str())))
                            .unwrap_or(PyObject::none())).collect();
                    PyObject::tuple(items)
                }).collect();
            Ok(PyObject::list(results))
        }
    } else {
        let re = build_regex(&pattern, flags)?;
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
    let rust_repl = python_repl_to_rust(&repl);
    if needs_fancy_regex(&pattern) {
        let re = build_fancy_regex(&pattern, flags)?;
        let mut result = String::new();
        let mut last = 0;
        let mut n = 0;
        let mut pos = 0;
        while pos <= text.len() {
            if count > 0 && n >= count { break; }
            match re.find(&text[pos..]) {
                Ok(Some(m)) => {
                    if m.start() == m.end() { pos += 1; continue; }
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
        Ok(PyObject::str_val(CompactString::from(result)))
    } else {
        let re = build_regex(&pattern, flags)?;
        let result = if count == 0 {
            re.replace_all(&text, rust_repl.as_str()).to_string()
        } else {
            re.replacen(&text, count, rust_repl.as_str()).to_string()
        };
        Ok(PyObject::str_val(CompactString::from(result)))
    }
}

fn re_subn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 { return Err(PyException::type_error("re.subn() requires pattern, repl, and string")); }
    let pattern = args[0].py_to_string();
    let repl = args[1].py_to_string();
    let text = args[2].py_to_string();
    let flags = if args.len() > 3 { args[3].to_int().unwrap_or(0) } else { 0 };
    let re = build_regex(&pattern, flags)?;
    let rust_repl = python_repl_to_rust(&repl);
    let count = re.find_iter(&text).count();
    let result = re.replace_all(&text, rust_repl.as_str()).to_string();
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
    if needs_fancy_regex(&pattern) {
        let re = build_fancy_regex(&pattern, flags)?;
        let mut result = Vec::new();
        let mut last = 0;
        let mut splits = 0;
        let mut pos = 0;
        while pos <= text.len() {
            if maxsplit > 0 && splits >= maxsplit { break; }
            match re.find(&text[pos..]) {
                Ok(Some(m)) => {
                    if m.start() == m.end() { pos += 1; continue; }
                    let abs_start = pos + m.start();
                    let abs_end = pos + m.end();
                    result.push(PyObject::str_val(CompactString::from(&text[last..abs_start])));
                    last = abs_end;
                    pos = abs_end;
                    splits += 1;
                }
                _ => break,
            }
        }
        result.push(PyObject::str_val(CompactString::from(&text[last..])));
        Ok(PyObject::list(result))
    } else {
        let re = build_regex(&pattern, flags)?;
        let num_groups = re.captures_len() - 1;

    let parts: Vec<PyObjectRef> = if num_groups == 0 {
        // No capturing groups: use simple split
        if maxsplit == 0 {
            re.split(&text).map(|s| PyObject::str_val(CompactString::from(s))).collect()
        } else {
            re.splitn(&text, maxsplit + 1).map(|s| PyObject::str_val(CompactString::from(s))).collect()
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
            result.push(PyObject::str_val(CompactString::from(&text[last..whole.start()])));
            // Each capturing group
            for i in 1..=num_groups {
                match caps.get(i) {
                    Some(m) => result.push(PyObject::str_val(CompactString::from(m.as_str()))),
                    None => result.push(PyObject::none()),
                }
            }
            last = whole.end();
            splits += 1;
        }
        // Remaining text after last match
        result.push(PyObject::str_val(CompactString::from(&text[last..])));
        result
    };
    Ok(PyObject::list(parts))
    }
}

fn re_compile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("re.compile() requires a pattern")); }
    let pattern = args[0].py_to_string();
    let flags = if args.len() > 1 { args[1].to_int().unwrap_or(0) } else { 0 };
    // Validate the pattern compiles (try fancy if needed)
    if needs_fancy_regex(&pattern) {
        build_fancy_regex(&pattern, flags)?;
    } else {
        build_regex(&pattern, flags)?;
    }
    let pat_str = PyObject::str_val(CompactString::from(pattern.clone()));
    let flags_obj = PyObject::int(flags);
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("pattern"), pat_str);
    attrs.insert(CompactString::from("flags"), flags_obj);
    attrs.insert(CompactString::from("match"), PyObject::native_function("Pattern.match", compiled_match));
    attrs.insert(CompactString::from("search"), PyObject::native_function("Pattern.search", compiled_search));
    attrs.insert(CompactString::from("findall"), PyObject::native_function("Pattern.findall", compiled_findall));
    attrs.insert(CompactString::from("finditer"), PyObject::native_function("Pattern.finditer", compiled_findall));
    attrs.insert(CompactString::from("sub"), PyObject::native_function("Pattern.sub", compiled_sub));
    attrs.insert(CompactString::from("split"), PyObject::native_function("Pattern.split", compiled_split));
    attrs.insert(CompactString::from("fullmatch"), PyObject::native_function("Pattern.fullmatch", compiled_fullmatch));
    // groups/groupindex: best-effort for standard regex
    if !needs_fancy_regex(&pattern) {
        if let Ok(re_obj) = build_regex(&pattern, flags) {
            let group_count = re_obj.captures_len() - 1;
            let mut groupindex_map = IndexMap::new();
            for name in re_obj.capture_names().flatten() {
                if let Some(idx) = re_obj.capture_names().enumerate()
                    .find(|(_, n)| n.as_deref() == Some(name))
                    .map(|(i, _)| i) {
                    groupindex_map.insert(
                        HashableKey::Str(CompactString::from(name)),
                        PyObject::int(idx as i64),
                    );
                }
            }
            attrs.insert(CompactString::from("groupindex"), PyObject::dict(groupindex_map));
            attrs.insert(CompactString::from("groups"), PyObject::int(group_count as i64));
        }
    } else {
        attrs.insert(CompactString::from("groupindex"), PyObject::dict(IndexMap::new()));
        attrs.insert(CompactString::from("groups"), PyObject::int(0));
    }
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

// ── html module ──────────────────────────────────────────────────────
pub fn create_html_module() -> PyObjectRef {
    fn html_escape(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("html.escape requires 1 argument")); }
        let s = args[0].py_to_string();
        let quote = if args.len() > 1 {
            match &args[1].payload { PyObjectPayload::Bool(b) => *b, _ => true }
        } else { true };
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' if quote => out.push_str("&quot;"),
                '\'' if quote => out.push_str("&#x27;"),
                _ => out.push(c),
            }
        }
        Ok(PyObject::str_val(CompactString::from(out)))
    }

    fn html_unescape(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("html.unescape requires 1 argument")); }
        let s = args[0].py_to_string();
        let out = s
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#x27;", "'")
            .replace("&#39;", "'")
            .replace("&apos;", "'")
            .replace("&#x2F;", "/")
            .replace("&#x3D;", "=");
        // Handle numeric character references &#NNN; and &#xHHH;
        let mut result = String::new();
        let mut chars = out.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '&' && chars.peek() == Some(&'#') {
                chars.next(); // consume '#'
                let mut num_str = String::new();
                let is_hex = chars.peek() == Some(&'x') || chars.peek() == Some(&'X');
                if is_hex { chars.next(); }
                for nc in chars.by_ref() {
                    if nc == ';' { break; }
                    num_str.push(nc);
                }
                let code = if is_hex {
                    u32::from_str_radix(&num_str, 16).ok()
                } else {
                    num_str.parse::<u32>().ok()
                };
                if let Some(cp) = code.and_then(char::from_u32) {
                    result.push(cp);
                } else {
                    result.push('&');
                    result.push('#');
                    if is_hex { result.push('x'); }
                    result.push_str(&num_str);
                    result.push(';');
                }
            } else {
                result.push(c);
            }
        }
        Ok(PyObject::str_val(CompactString::from(result)))
    }

    make_module("html", vec![
        ("escape", make_builtin(html_escape)),
        ("unescape", make_builtin(html_unescape)),
    ])
}

// ── shlex module ─────────────────────────────────────────────────────
pub fn create_shlex_module() -> PyObjectRef {
    fn shlex_split(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("shlex.split requires 1 argument")); }
        let s = args[0].py_to_string();
        let mut result = Vec::new();
        let mut current = String::new();
        let mut in_single = false;
        let mut in_double = false;
        let mut escape_next = false;
        for c in s.chars() {
            if escape_next {
                current.push(c);
                escape_next = false;
                continue;
            }
            if c == '\\' && !in_single {
                escape_next = true;
                continue;
            }
            if c == '\'' && !in_double {
                in_single = !in_single;
                continue;
            }
            if c == '"' && !in_single {
                in_double = !in_double;
                continue;
            }
            if c.is_whitespace() && !in_single && !in_double {
                if !current.is_empty() {
                    result.push(PyObject::str_val(CompactString::from(&current)));
                    current.clear();
                }
                continue;
            }
            current.push(c);
        }
        if !current.is_empty() {
            result.push(PyObject::str_val(CompactString::from(&current)));
        }
        Ok(PyObject::list(result))
    }

    fn shlex_quote(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("shlex.quote requires 1 argument")); }
        let s = args[0].py_to_string();
        if s.is_empty() {
            return Ok(PyObject::str_val(CompactString::from("''")));
        }
        // If safe chars only, return as-is
        if s.chars().all(|c| c.is_alphanumeric() || matches!(c, '@' | '%' | '+' | '=' | ':' | ',' | '.' | '/' | '-' | '_')) {
            return Ok(PyObject::str_val(CompactString::from(&s)));
        }
        // Wrap in single quotes, escaping any single quotes
        let escaped = s.replace('\'', "'\"'\"'");
        Ok(PyObject::str_val(CompactString::from(format!("'{}'", escaped))))
    }

    fn shlex_join(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("shlex.join requires 1 argument")); }
        let items = match &args[0].payload {
            PyObjectPayload::List(items) => items.read().clone(),
            PyObjectPayload::Tuple(items) => items.clone(),
            _ => return Err(PyException::type_error("shlex.join expects an iterable")),
        };
        let parts: Vec<String> = items.iter().map(|item| {
            let s = item.py_to_string();
            if s.is_empty() || s.chars().any(|c| c.is_whitespace() || matches!(c, '\'' | '"' | '\\' | '|' | '&' | ';' | '(' | ')' | '<' | '>' | '!' | '`' | '$' | '{' | '}' | '[' | ']')) {
                let escaped = s.replace('\'', "'\"'\"'");
                format!("'{}'", escaped)
            } else { s }
        }).collect();
        Ok(PyObject::str_val(CompactString::from(parts.join(" "))))
    }

    make_module("shlex", vec![
        ("split", make_builtin(shlex_split)),
        ("quote", make_builtin(shlex_quote)),
        ("join", make_builtin(shlex_join)),
    ])
}

// ── difflib module ───────────────────────────────────────────────────

/// Compute matching blocks between two string sequences using LCS dynamic programming.
/// Returns (a_start, b_start, size) triples with a sentinel (a.len(), b.len(), 0).
fn find_matching_blocks(a: &[String], b: &[String]) -> Vec<(usize, usize, usize)> {
    let m = a.len();
    let n = b.len();
    if m == 0 || n == 0 {
        return vec![(m, n, 0)];
    }
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if a[i - 1] == b[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let (mut i, mut j) = (m, n);
    while i > 0 && j > 0 {
        if a[i - 1] == b[j - 1] {
            pairs.push((i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if dp[i - 1][j] >= dp[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    pairs.reverse();
    let mut blocks = Vec::new();
    let mut idx = 0;
    while idx < pairs.len() {
        let (sa, sb) = pairs[idx];
        let mut cnt = 1;
        while idx + cnt < pairs.len()
            && pairs[idx + cnt].0 == sa + cnt
            && pairs[idx + cnt].1 == sb + cnt
        {
            cnt += 1;
        }
        blocks.push((sa, sb, cnt));
        idx += cnt;
    }
    blocks.push((m, n, 0));
    blocks
}

/// Convert matching blocks to opcodes (tag, a_start, a_end, b_start, b_end).
fn opcodes_from_matching_blocks(
    blocks: &[(usize, usize, usize)],
) -> Vec<(String, usize, usize, usize, usize)> {
    let mut ops = Vec::new();
    let (mut ai, mut bj) = (0usize, 0usize);
    for &(a_start, b_start, size) in blocks {
        let tag = if ai < a_start && bj < b_start {
            Some("replace")
        } else if ai < a_start {
            Some("delete")
        } else if bj < b_start {
            Some("insert")
        } else {
            None
        };
        if let Some(t) = tag {
            ops.push((t.to_string(), ai, a_start, bj, b_start));
        }
        if size > 0 {
            ops.push(("equal".to_string(), a_start, a_start + size, b_start, b_start + size));
        }
        ai = a_start + size;
        bj = b_start + size;
    }
    ops
}

/// Group opcodes into hunks for diff output, respecting context size n.
fn group_opcodes(
    opcodes: &[(String, usize, usize, usize, usize)],
    n: usize,
) -> Vec<Vec<(String, usize, usize, usize, usize)>> {
    let mut codes: Vec<(String, usize, usize, usize, usize)> = if opcodes.is_empty() {
        vec![("equal".to_string(), 0, 1, 0, 1)]
    } else {
        opcodes.to_vec()
    };
    if codes[0].0 == "equal" {
        let (ref t, i1, i2, j1, j2) = codes[0];
        codes[0] = (t.clone(), i2.saturating_sub(n).max(i1), i2, j2.saturating_sub(n).max(j1), j2);
    }
    let last = codes.len() - 1;
    if codes[last].0 == "equal" {
        let (ref t, i1, i2, j1, j2) = codes[last];
        codes[last] = (t.clone(), i1, (i1 + n).min(i2), j1, (j1 + n).min(j2));
    }
    let nn = n + n;
    let mut groups: Vec<Vec<(String, usize, usize, usize, usize)>> = Vec::new();
    let mut group: Vec<(String, usize, usize, usize, usize)> = Vec::new();
    for (tag, i1, i2, j1, j2) in codes {
        if tag == "equal" && i2 - i1 > nn {
            group.push((tag.clone(), i1, (i1 + n).min(i2), j1, (j1 + n).min(j2)));
            groups.push(group);
            group = Vec::new();
            let ni1 = i2.saturating_sub(n).max(i1);
            let nj1 = j2.saturating_sub(n).max(j1);
            group.push((tag, ni1, i2, nj1, j2));
        } else {
            group.push((tag, i1, i2, j1, j2));
        }
    }
    if !group.is_empty() && group.iter().any(|(t, ..)| t != "equal") {
        groups.push(group);
    }
    groups
}

fn format_range_unified(start: usize, stop: usize) -> String {
    let beginning = start + 1;
    let length = stop - start;
    if length == 1 {
        format!("{}", beginning)
    } else if length == 0 {
        format!("{},0", start)
    } else {
        format!("{},{}", beginning, length)
    }
}

fn format_range_context(start: usize, stop: usize) -> String {
    let beginning = start + 1;
    let length = stop - start;
    if length == 0 {
        format!("{}", start)
    } else if length == 1 {
        format!("{}", beginning)
    } else {
        format!("{},{}", beginning, beginning + length - 1)
    }
}

pub fn create_difflib_module() -> PyObjectRef {
    fn extract_lines(obj: &PyObjectRef) -> PyResult<Vec<String>> {
        match &obj.payload {
            PyObjectPayload::List(items) => Ok(items.read().iter().map(|i| i.py_to_string()).collect()),
            _ => Err(PyException::type_error("expected list")),
        }
    }

    fn parse_diff_kwargs(args: &[PyObjectRef]) -> (String, String, String, String, usize, String) {
        let mut fromfile = String::new();
        let mut tofile = String::new();
        let mut fromfiledate = String::new();
        let mut tofiledate = String::new();
        let mut n = 3usize;
        let mut lineterm = String::from("\n");
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw) = &last.payload {
                let kw = kw.read();
                if let Some(v) = kw.get(&HashableKey::Str(CompactString::from("fromfile"))) {
                    fromfile = v.py_to_string();
                }
                if let Some(v) = kw.get(&HashableKey::Str(CompactString::from("tofile"))) {
                    tofile = v.py_to_string();
                }
                if let Some(v) = kw.get(&HashableKey::Str(CompactString::from("fromfiledate"))) {
                    fromfiledate = v.py_to_string();
                }
                if let Some(v) = kw.get(&HashableKey::Str(CompactString::from("tofiledate"))) {
                    tofiledate = v.py_to_string();
                }
                if let Some(v) = kw.get(&HashableKey::Str(CompactString::from("n"))) {
                    n = v.to_int().unwrap_or(3) as usize;
                }
                if let Some(v) = kw.get(&HashableKey::Str(CompactString::from("lineterm"))) {
                    lineterm = v.py_to_string();
                }
            }
        }
        for (i, arg) in args.iter().enumerate().skip(2) {
            if matches!(&arg.payload, PyObjectPayload::Dict(_)) { break; }
            match i {
                2 => fromfile = arg.py_to_string(),
                3 => tofile = arg.py_to_string(),
                4 => fromfiledate = arg.py_to_string(),
                5 => tofiledate = arg.py_to_string(),
                6 => n = arg.to_int().unwrap_or(3) as usize,
                7 => lineterm = arg.py_to_string(),
                _ => break,
            }
        }
        (fromfile, tofile, fromfiledate, tofiledate, n, lineterm)
    }

    fn unified_diff(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("unified_diff requires at least 2 arguments"));
        }
        let a_lines = extract_lines(&args[0])?;
        let b_lines = extract_lines(&args[1])?;
        let (fromfile, tofile, fromfiledate, tofiledate, n, _lineterm) = parse_diff_kwargs(args);

        let blocks = find_matching_blocks(&a_lines, &b_lines);
        let opcodes = opcodes_from_matching_blocks(&blocks);
        let groups = group_opcodes(&opcodes, n);

        let mut result: Vec<PyObjectRef> = Vec::new();
        let mut started = false;
        for group in &groups {
            if !started {
                let from_h = if fromfiledate.is_empty() {
                    format!("--- {}\n", fromfile)
                } else {
                    format!("--- {}\t{}\n", fromfile, fromfiledate)
                };
                let to_h = if tofiledate.is_empty() {
                    format!("+++ {}\n", tofile)
                } else {
                    format!("+++ {}\t{}\n", tofile, tofiledate)
                };
                result.push(PyObject::str_val(CompactString::from(from_h)));
                result.push(PyObject::str_val(CompactString::from(to_h)));
                started = true;
            }
            let first = &group[0];
            let last_op = &group[group.len() - 1];
            let hunk = format!(
                "@@ -{} +{} @@\n",
                format_range_unified(first.1, last_op.2),
                format_range_unified(first.3, last_op.4),
            );
            result.push(PyObject::str_val(CompactString::from(hunk)));
            for (tag, i1, i2, j1, j2) in group {
                match tag.as_str() {
                    "equal" => {
                        for k in *i1..*i2 {
                            result.push(PyObject::str_val(CompactString::from(format!(" {}", a_lines[k]))));
                        }
                    }
                    "replace" => {
                        for k in *i1..*i2 {
                            result.push(PyObject::str_val(CompactString::from(format!("-{}", a_lines[k]))));
                        }
                        for k in *j1..*j2 {
                            result.push(PyObject::str_val(CompactString::from(format!("+{}", b_lines[k]))));
                        }
                    }
                    "delete" => {
                        for k in *i1..*i2 {
                            result.push(PyObject::str_val(CompactString::from(format!("-{}", a_lines[k]))));
                        }
                    }
                    "insert" => {
                        for k in *j1..*j2 {
                            result.push(PyObject::str_val(CompactString::from(format!("+{}", b_lines[k]))));
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(PyObject::list(result))
    }

    fn ndiff(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("ndiff requires at least 2 arguments"));
        }
        let a_lines = extract_lines(&args[0])?;
        let b_lines = extract_lines(&args[1])?;

        let blocks = find_matching_blocks(&a_lines, &b_lines);
        let opcodes = opcodes_from_matching_blocks(&blocks);

        let mut result: Vec<PyObjectRef> = Vec::new();
        for (tag, i1, i2, j1, j2) in &opcodes {
            match tag.as_str() {
                "equal" => {
                    for k in *i1..*i2 {
                        result.push(PyObject::str_val(CompactString::from(format!("  {}", a_lines[k]))));
                    }
                }
                "replace" => {
                    for k in *i1..*i2 {
                        result.push(PyObject::str_val(CompactString::from(format!("- {}", a_lines[k]))));
                    }
                    for k in *j1..*j2 {
                        result.push(PyObject::str_val(CompactString::from(format!("+ {}", b_lines[k]))));
                    }
                }
                "delete" => {
                    for k in *i1..*i2 {
                        result.push(PyObject::str_val(CompactString::from(format!("- {}", a_lines[k]))));
                    }
                }
                "insert" => {
                    for k in *j1..*j2 {
                        result.push(PyObject::str_val(CompactString::from(format!("+ {}", b_lines[k]))));
                    }
                }
                _ => {}
            }
        }
        Ok(PyObject::list(result))
    }

    fn context_diff(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("context_diff requires at least 2 arguments"));
        }
        let a_lines = extract_lines(&args[0])?;
        let b_lines = extract_lines(&args[1])?;
        let (fromfile, tofile, fromfiledate, tofiledate, n, _lineterm) = parse_diff_kwargs(args);

        let blocks = find_matching_blocks(&a_lines, &b_lines);
        let opcodes = opcodes_from_matching_blocks(&blocks);
        let groups = group_opcodes(&opcodes, n);

        let mut result: Vec<PyObjectRef> = Vec::new();
        let mut started = false;
        for group in &groups {
            if !started {
                let from_h = if fromfiledate.is_empty() {
                    format!("*** {}\n", fromfile)
                } else {
                    format!("*** {}\t{}\n", fromfile, fromfiledate)
                };
                let to_h = if tofiledate.is_empty() {
                    format!("--- {}\n", tofile)
                } else {
                    format!("--- {}\t{}\n", tofile, tofiledate)
                };
                result.push(PyObject::str_val(CompactString::from(from_h)));
                result.push(PyObject::str_val(CompactString::from(to_h)));
                started = true;
            }
            result.push(PyObject::str_val(CompactString::from("***************\n")));

            let first = &group[0];
            let last_op = &group[group.len() - 1];

            // "From" section
            result.push(PyObject::str_val(CompactString::from(
                format!("*** {} ****\n", format_range_context(first.1, last_op.2))
            )));
            let has_from_changes = group.iter().any(|(t, ..)| t == "replace" || t == "delete");
            if has_from_changes {
                for (tag, i1, i2, _, _) in group {
                    match tag.as_str() {
                        "equal" => {
                            for k in *i1..*i2 {
                                result.push(PyObject::str_val(CompactString::from(format!("  {}", a_lines[k]))));
                            }
                        }
                        "replace" => {
                            for k in *i1..*i2 {
                                result.push(PyObject::str_val(CompactString::from(format!("! {}", a_lines[k]))));
                            }
                        }
                        "delete" => {
                            for k in *i1..*i2 {
                                result.push(PyObject::str_val(CompactString::from(format!("- {}", a_lines[k]))));
                            }
                        }
                        _ => {}
                    }
                }
            }

            // "To" section
            result.push(PyObject::str_val(CompactString::from(
                format!("--- {} ----\n", format_range_context(first.3, last_op.4))
            )));
            let has_to_changes = group.iter().any(|(t, ..)| t == "replace" || t == "insert");
            if has_to_changes {
                for (tag, _, _, j1, j2) in group {
                    match tag.as_str() {
                        "equal" => {
                            for k in *j1..*j2 {
                                result.push(PyObject::str_val(CompactString::from(format!("  {}", b_lines[k]))));
                            }
                        }
                        "replace" => {
                            for k in *j1..*j2 {
                                result.push(PyObject::str_val(CompactString::from(format!("! {}", b_lines[k]))));
                            }
                        }
                        "insert" => {
                            for k in *j1..*j2 {
                                result.push(PyObject::str_val(CompactString::from(format!("+ {}", b_lines[k]))));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(PyObject::list(result))
    }

    fn get_close_matches(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("get_close_matches requires at least 2 arguments")); }
        let word = args[0].py_to_string();
        let possibilities: Vec<String> = match &args[1].payload {
            PyObjectPayload::List(items) => items.read().iter().map(|i| i.py_to_string()).collect(),
            _ => return Err(PyException::type_error("expected list")),
        };
        let n = if args.len() > 2 { args[2].to_int().unwrap_or(3) as usize } else { 3 };
        let cutoff = if args.len() > 3 {
            match &args[3].payload { PyObjectPayload::Float(f) => *f, _ => 0.6 }
        } else { 0.6 };

        let word_chars: Vec<char> = word.chars().collect();
        let mut scored: Vec<(f64, &String)> = possibilities.iter().filter_map(|p| {
            let p_chars: Vec<char> = p.chars().collect();
            let matches = lcs_length(&word_chars, &p_chars);
            let total = word_chars.len() + p_chars.len();
            let ratio = if total > 0 { 2.0 * matches as f64 / total as f64 } else { 1.0 };
            if ratio >= cutoff { Some((ratio, p)) } else { None }
        }).collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(n);
        Ok(PyObject::list(scored.iter().map(|(_, s)| PyObject::str_val(CompactString::from(s.as_str()))).collect()))
    }

    fn sequence_matcher_ctor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        fn seq_from_obj(obj: &PyObjectRef) -> Vec<String> {
            match &obj.payload {
                PyObjectPayload::List(items) => items.read().iter().map(|i| i.py_to_string()).collect(),
                PyObjectPayload::Str(s) => s.chars().map(|c| c.to_string()).collect(),
                _ => vec![obj.py_to_string()],
            }
        }

        let mut a_seq: Vec<String> = Vec::new();
        let mut b_seq: Vec<String> = Vec::new();
        if args.len() > 1 { a_seq = seq_from_obj(&args[1]); }
        if args.len() > 2 { b_seq = seq_from_obj(&args[2]); }
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw) = &last.payload {
                let kw = kw.read();
                if let Some(v) = kw.get(&HashableKey::Str(CompactString::from("a"))) {
                    a_seq = seq_from_obj(v);
                }
                if let Some(v) = kw.get(&HashableKey::Str(CompactString::from("b"))) {
                    b_seq = seq_from_obj(v);
                }
            }
        }

        let blocks = find_matching_blocks(&a_seq, &b_seq);
        let opcodes = opcodes_from_matching_blocks(&blocks);

        let matching: usize = blocks.iter().map(|&(_, _, s)| s).sum();
        let total = a_seq.len() + b_seq.len();
        let ratio_val = if total > 0 { 2.0 * matching as f64 / total as f64 } else { 1.0 };

        let cls = PyObject::class(CompactString::from("SequenceMatcher"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut attrs = d.attrs.write();
            let a_obj = if args.len() > 1 { args[1].clone() } else { PyObject::str_val(CompactString::from("")) };
            let b_obj = if args.len() > 2 { args[2].clone() } else { PyObject::str_val(CompactString::from("")) };
            attrs.insert(CompactString::from("a"), a_obj);
            attrs.insert(CompactString::from("b"), b_obj);

            let rf = ratio_val;
            attrs.insert(CompactString::from("ratio"), PyObject::native_closure(
                "SequenceMatcher.ratio", move |_: &[PyObjectRef]| Ok(PyObject::float(rf))
            ));
            attrs.insert(CompactString::from("quick_ratio"), PyObject::native_closure(
                "SequenceMatcher.quick_ratio", move |_: &[PyObjectRef]| Ok(PyObject::float(rf))
            ));

            let bc = blocks.clone();
            attrs.insert(CompactString::from("get_matching_blocks"), PyObject::native_closure(
                "SequenceMatcher.get_matching_blocks", move |_: &[PyObjectRef]| {
                    let r: Vec<PyObjectRef> = bc.iter().map(|&(a, b, s)| {
                        PyObject::tuple(vec![PyObject::int(a as i64), PyObject::int(b as i64), PyObject::int(s as i64)])
                    }).collect();
                    Ok(PyObject::list(r))
                }
            ));

            let oc = opcodes;
            attrs.insert(CompactString::from("get_opcodes"), PyObject::native_closure(
                "SequenceMatcher.get_opcodes", move |_: &[PyObjectRef]| {
                    let r: Vec<PyObjectRef> = oc.iter().map(|(tag, i1, i2, j1, j2)| {
                        PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from(tag.as_str())),
                            PyObject::int(*i1 as i64),
                            PyObject::int(*i2 as i64),
                            PyObject::int(*j1 as i64),
                            PyObject::int(*j2 as i64),
                        ])
                    }).collect();
                    Ok(PyObject::list(r))
                }
            ));
        }
        Ok(inst)
    }

    make_module("difflib", vec![
        ("unified_diff", make_builtin(unified_diff)),
        ("ndiff", make_builtin(ndiff)),
        ("context_diff", make_builtin(context_diff)),
        ("get_close_matches", make_builtin(get_close_matches)),
        ("SequenceMatcher", make_builtin(sequence_matcher_ctor)),
    ])
}

/// Compute Longest Common Subsequence length (character-level, used by get_close_matches)
fn lcs_length(a: &[char], b: &[char]) -> usize {
    let m = a.len();
    let n = b.len();
    if m == 0 || n == 0 { return 0; }
    let mut prev = vec![0usize; n + 1];
    let mut curr = vec![0usize; n + 1];
    for i in 1..=m {
        for j in 1..=n {
            if a[i - 1] == b[j - 1] {
                curr[j] = prev[j - 1] + 1;
            } else {
                curr[j] = prev[j].max(curr[j - 1]);
            }
        }
        std::mem::swap(&mut prev, &mut curr);
        curr.iter_mut().for_each(|x| *x = 0);
    }
    prev[n]
}

// ── html.parser module ──

pub fn create_html_parser_module() -> PyObjectRef {
    // Build HTMLParser as a proper Class so subclasses inherit methods via MRO.
    let mut ns = IndexMap::new();

    // __init__: set up per-instance state
    ns.insert(CompactString::from("__init__"), make_builtin(|args: &[PyObjectRef]| {
        // args[0] is self
        if !args.is_empty() {
            if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                let mut w = inst.attrs.write();
                w.insert(CompactString::from("_data_buf"), PyObject::str_val(CompactString::from("")));
                w.insert(CompactString::from("_pos"), PyObject::tuple(vec![PyObject::int(1), PyObject::int(0)]));
            }
        }
        Ok(PyObject::none())
    }));

    // feed(self, data): parse HTML data and invoke callbacks
    ns.insert(CompactString::from("feed"), make_builtin(|args: &[PyObjectRef]| {
            check_args_min("HTMLParser.feed", args, 2)?;
            let _self_obj = &args[0];
            let data = args[1].py_to_string();

            // Store raw data
            if let PyObjectPayload::Instance(ref inst) = _self_obj.payload {
                let existing = inst.attrs.read().get("_data_buf").cloned()
                    .map(|v| v.py_to_string()).unwrap_or_default();
                inst.attrs.write().insert(
                    CompactString::from("_data_buf"),
                    PyObject::str_val(CompactString::from(format!("{}{}", existing, data))),
                );
            }

            // Simple HTML tag parsing — extract tags and invoke callbacks
            // Store callback requests as a list for the VM to process
            let mut pending = Vec::new();
            let chars: Vec<char> = data.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                if chars[i] == '<' {
                    // Find closing >
                    if let Some(end) = chars[i..].iter().position(|&c| c == '>') {
                        let tag_content: String = chars[i+1..i+end].iter().collect();
                        let tag_content = tag_content.trim().to_string();
                        if tag_content.starts_with('/') {
                            // End tag
                            let tag_name = tag_content[1..].trim().to_lowercase();
                            pending.push(("handle_endtag", tag_name, Vec::new()));
                        } else if tag_content.starts_with('!') {
                            // Comment or declaration
                            if tag_content.starts_with("!--") {
                                let comment = tag_content.strip_prefix("!--").unwrap_or("")
                                    .strip_suffix("--").unwrap_or(&tag_content[3..]).to_string();
                                pending.push(("handle_comment", comment, Vec::new()));
                            } else {
                                let decl = tag_content[1..].to_string();
                                pending.push(("handle_decl", decl, Vec::new()));
                            }
                        } else {
                            // Start tag: parse name and attributes
                            let parts: Vec<&str> = tag_content.splitn(2, char::is_whitespace).collect();
                            let tag_name = parts[0].trim_end_matches('/').to_lowercase();
                            let mut attrs = Vec::new();
                            if parts.len() > 1 {
                                // Simple attribute parsing
                                let attr_str = parts[1].trim_end_matches('/');
                                for attr in attr_str.split_whitespace() {
                                    if let Some(eq_pos) = attr.find('=') {
                                        let k = &attr[..eq_pos];
                                        let v = attr[eq_pos+1..].trim_matches('"').trim_matches('\'');
                                        attrs.push((k.to_string(), v.to_string()));
                                    } else {
                                        attrs.push((attr.to_string(), String::new()));
                                    }
                                }
                            }
                            if tag_content.ends_with('/') {
                                pending.push(("handle_startendtag", tag_name, attrs));
                            } else {
                                pending.push(("handle_starttag", tag_name, attrs));
                            }
                        }
                        i += end + 1;
                    } else {
                        i += 1;
                    }
                } else {
                    // Text data
                    let start = i;
                    while i < chars.len() && chars[i] != '<' {
                        i += 1;
                    }
                    let text: String = chars[start..i].iter().collect();
                    if !text.is_empty() {
                        pending.push(("handle_data", text, Vec::new()));
                    }
                }
            }

            // Queue callbacks via pending_vm_call mechanism
            // Since we can't call Python methods from a NativeClosure, store them for the VM
            if let PyObjectPayload::Instance(ref inst) = _self_obj.payload {
                let mut callback_list = Vec::new();
                for (method_name, arg, attrs) in &pending {
                    let method = inst.attrs.read().get(&CompactString::from(*method_name)).cloned();
                    if method.is_none() {
                        // Try class namespace (inherited methods)
                        if let PyObjectPayload::Class(cd) = &inst.class.payload {
                            if let Some(m) = cd.namespace.read().get(&CompactString::from(*method_name)).cloned() {
                                let mut call_args = vec![_self_obj.clone(), PyObject::str_val(CompactString::from(arg.as_str()))];
                                if *method_name == "handle_starttag" || *method_name == "handle_startendtag" {
                                    let attr_list: Vec<PyObjectRef> = attrs.iter().map(|(k, v)| {
                                        PyObject::tuple(vec![
                                            PyObject::str_val(CompactString::from(k.as_str())),
                                            PyObject::str_val(CompactString::from(v.as_str())),
                                        ])
                                    }).collect();
                                    call_args.push(PyObject::list(attr_list));
                                }
                                callback_list.push((m, call_args));
                                continue;
                            }
                        }
                    }
                    if let Some(m) = method {
                        let mut call_args = vec![PyObject::str_val(CompactString::from(arg.as_str()))];
                        if *method_name == "handle_starttag" || *method_name == "handle_startendtag" {
                            let attr_list: Vec<PyObjectRef> = attrs.iter().map(|(k, v)| {
                                PyObject::tuple(vec![
                                    PyObject::str_val(CompactString::from(k.as_str())),
                                    PyObject::str_val(CompactString::from(v.as_str())),
                                ])
                            }).collect();
                            call_args.push(PyObject::list(attr_list));
                        }
                        callback_list.push((m, call_args));
                    }
                }
                // Store callbacks for the VM to process
                for (func, call_args) in callback_list {
                    crate::concurrency_modules::push_deferred_call(func, call_args);
                }
            }

            Ok(PyObject::none())
        }
    ));

    // close(self)
    ns.insert(CompactString::from("close"), make_builtin(|args: &[PyObjectRef]| {
        if !args.is_empty() {
            if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                inst.attrs.write().insert(
                    CompactString::from("_data_buf"),
                    PyObject::str_val(CompactString::from("")),
                );
            }
        }
        Ok(PyObject::none())
    }));

    // reset(self)
    ns.insert(CompactString::from("reset"), make_builtin(|args: &[PyObjectRef]| {
        if !args.is_empty() {
            if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                inst.attrs.write().insert(
                    CompactString::from("_data_buf"),
                    PyObject::str_val(CompactString::from("")),
                );
            }
        }
        Ok(PyObject::none())
    }));

    // getpos(self)
    ns.insert(CompactString::from("getpos"), make_builtin(|args: &[PyObjectRef]| {
        if !args.is_empty() {
            if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                if let Some(pos) = inst.attrs.read().get("_pos").cloned() {
                    return Ok(pos);
                }
            }
        }
        Ok(PyObject::tuple(vec![PyObject::int(1), PyObject::int(0)]))
    }));

    // Callback stubs (no-ops by default, subclasses override)
    for name in &["handle_starttag", "handle_endtag", "handle_data",
                  "handle_comment", "handle_decl", "handle_pi",
                  "handle_startendtag", "handle_entityref", "handle_charref"] {
        ns.insert(CompactString::from(*name), make_builtin(|_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }));
    }

    let html_parser_class = PyObject::class(CompactString::from("HTMLParser"), vec![], ns);

    make_module("html.parser", vec![
        ("HTMLParser", html_parser_class),
    ])
}

// ── unicodedata module ──

pub fn create_unicodedata_module() -> PyObjectRef {
    let name_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.name", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        // Return a placeholder name based on codepoint
        let cp = ch as u32;
        let name = if ch.is_ascii_uppercase() {
            format!("LATIN CAPITAL LETTER {}", ch)
        } else if ch.is_ascii_lowercase() {
            format!("LATIN SMALL LETTER {}", ch.to_uppercase().next().unwrap_or(ch))
        } else if ch.is_ascii_digit() {
            format!("DIGIT {}", ch)
        } else {
            format!("U+{:04X}", cp)
        };
        Ok(PyObject::str_val(CompactString::from(name)))
    });

    let lookup_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.lookup", args, 1)?;
        let name = args[0].py_to_string().to_uppercase();
        // Handle common lookups
        if name.starts_with("LATIN CAPITAL LETTER ") {
            let letter = name.strip_prefix("LATIN CAPITAL LETTER ").unwrap_or("A");
            let ch = letter.chars().next().unwrap_or('A');
            Ok(PyObject::str_val(CompactString::from(ch.to_string().as_str())))
        } else if name.starts_with("LATIN SMALL LETTER ") {
            let letter = name.strip_prefix("LATIN SMALL LETTER ").unwrap_or("a");
            let ch = letter.chars().next().unwrap_or('a').to_lowercase().next().unwrap_or('a');
            Ok(PyObject::str_val(CompactString::from(ch.to_string().as_str())))
        } else if name.starts_with("DIGIT ") {
            let digit = name.strip_prefix("DIGIT ").unwrap_or("0");
            Ok(PyObject::str_val(CompactString::from(digit)))
        } else {
            Err(PyException::key_error(format!("undefined character name '{}'", name)))
        }
    });

    let category_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.category", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        let cat = if ch.is_ascii_uppercase() { "Lu" }
            else if ch.is_ascii_lowercase() { "Ll" }
            else if ch.is_ascii_digit() { "Nd" }
            else if ch.is_ascii_punctuation() { "Po" }
            else if ch.is_ascii_whitespace() { "Zs" }
            else if ch.is_alphabetic() { "L" }
            else { "Cn" };
        Ok(PyObject::str_val(CompactString::from(cat)))
    });

    let numeric_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.numeric", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        if let Some(d) = ch.to_digit(10) {
            Ok(PyObject::float(d as f64))
        } else if args.len() > 1 {
            Ok(args[1].clone())
        } else {
            Err(PyException::value_error("not a numeric character"))
        }
    });

    let decimal_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.decimal", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        if let Some(d) = ch.to_digit(10) {
            Ok(PyObject::int(d as i64))
        } else if args.len() > 1 {
            Ok(args[1].clone())
        } else {
            Err(PyException::value_error("not a decimal character"))
        }
    });

    let normalize_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.normalize", args, 2)?;
        // Return the string as-is
        Ok(args[1].clone())
    });

    make_module("unicodedata", vec![
        ("name", name_fn),
        ("lookup", lookup_fn),
        ("category", category_fn),
        ("numeric", numeric_fn),
        ("decimal", decimal_fn),
        ("normalize", normalize_fn),
        ("unidata_version", PyObject::str_val(CompactString::from("15.0.0"))),
    ])
}

// ── pprint module ──

fn pformat_value(obj: &PyObjectRef, indent: usize, width: usize, depth: Option<usize>, current_depth: usize) -> String {
    if let Some(max_d) = depth {
        if current_depth > max_d { return "...".to_string(); }
    }
    let prefix = " ".repeat(indent * current_depth);
    let inner_prefix = " ".repeat(indent * (current_depth + 1));

    match &obj.payload {
        PyObjectPayload::Dict(map) => {
            let r = map.read();
            if r.is_empty() { return "{}".to_string(); }
            let mut entries: Vec<String> = Vec::new();
            for (k, v) in r.iter() {
                let ks = match k {
                    HashableKey::Str(s) => format!("'{}'", s),
                    HashableKey::Int(i) => i.to_string(),
                    HashableKey::Float(f) => format!("{}", f),
                    HashableKey::Bool(b) => if *b { "True".to_string() } else { "False".to_string() },
                    HashableKey::None => "None".to_string(),
                    HashableKey::Tuple(t) => format!("({})", t.iter().map(|x| match x {
                        HashableKey::Str(s) => format!("'{}'", s),
                        HashableKey::Int(i) => i.to_string(),
                        _ => "...".to_string(),
                    }).collect::<Vec<_>>().join(", ")),
                    HashableKey::FrozenSet(_) => "frozenset(...)".to_string(),
                    HashableKey::Bytes(_) | HashableKey::Identity(_, _) | HashableKey::Custom { .. } => "...".to_string(),
                };
                let vs = pformat_value(v, indent, width, depth, current_depth + 1);
                entries.push(format!("{}: {}", ks, vs));
            }
            let oneline = format!("{{{}}}", entries.join(", "));
            if oneline.len() + prefix.len() <= width {
                return oneline;
            }
            let mut s = String::from("{\n");
            for (i, e) in entries.iter().enumerate() {
                s.push_str(&inner_prefix);
                s.push_str(e);
                if i < entries.len() - 1 { s.push(','); }
                s.push('\n');
            }
            s.push_str(&prefix);
            s.push('}');
            s
        }
        PyObjectPayload::List(items) => {
            let r = items.read();
            if r.is_empty() { return "[]".to_string(); }
            let oneline = format!("[{}]", r.iter().map(|v| pformat_value(v, indent, width, depth, current_depth + 1)).collect::<Vec<_>>().join(", "));
            if oneline.len() + prefix.len() <= width {
                return oneline;
            }
            let mut s = String::from("[\n");
            for (i, v) in r.iter().enumerate() {
                s.push_str(&inner_prefix);
                s.push_str(&pformat_value(v, indent, width, depth, current_depth + 1));
                if i < r.len() - 1 { s.push(','); }
                s.push('\n');
            }
            s.push_str(&prefix);
            s.push(']');
            s
        }
        PyObjectPayload::Tuple(items) => {
            if items.is_empty() { return "()".to_string(); }
            if items.len() == 1 {
                return format!("({},)", pformat_value(&items[0], indent, width, depth, current_depth + 1));
            }
            let oneline = format!("({})", items.iter().map(|v| pformat_value(v, indent, width, depth, current_depth + 1)).collect::<Vec<_>>().join(", "));
            if oneline.len() + prefix.len() <= width { return oneline; }
            let mut s = String::from("(\n");
            for (i, v) in items.iter().enumerate() {
                s.push_str(&inner_prefix);
                s.push_str(&pformat_value(v, indent, width, depth, current_depth + 1));
                if i < items.len() - 1 { s.push(','); }
                s.push('\n');
            }
            s.push_str(&prefix);
            s.push(')');
            s
        }
        PyObjectPayload::Set(items) => {
            let r = items.read();
            if r.is_empty() { return "set()".to_string(); }
            let elems: Vec<String> = r.iter().map(|(k, _)| match k {
                HashableKey::Str(s) => format!("'{}'", s),
                HashableKey::Int(i) => i.to_string(),
                HashableKey::Float(f) => f.0.to_string(),
                HashableKey::Bool(b) => if *b { "True".to_string() } else { "False".to_string() },
                HashableKey::None => "None".to_string(),
                _ => format!("{:?}", k),
            }).collect();
            format!("{{{}}}", elems.join(", "))
        }
        _ => {
            // For strings, add quotes (like repr)
            if let PyObjectPayload::Str(s) = &obj.payload {
                return format!("'{}'", s);
            }
            obj.py_to_string()
        }
    }
}

pub fn create_pprint_module() -> PyObjectRef {
    make_module("pprint", vec![
        ("pprint", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::none()); }
            // Parse kwargs: stream, indent, width, depth
            let mut indent = 1usize;
            let mut width = 80usize;
            let mut depth: Option<usize> = None;
            let mut stream_obj: Option<PyObjectRef> = None;
            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(kw) = &last.payload {
                    let r = kw.read();
                    if let Some(s) = r.get(&HashableKey::Str(CompactString::from("stream"))) {
                        if !matches!(s.payload, PyObjectPayload::None) {
                            stream_obj = Some(s.clone());
                        }
                    }
                    if let Some(v) = r.get(&HashableKey::Str(CompactString::from("indent"))) {
                        indent = v.as_int().unwrap_or(1) as usize;
                    }
                    if let Some(v) = r.get(&HashableKey::Str(CompactString::from("width"))) {
                        width = v.as_int().unwrap_or(80) as usize;
                    }
                    if let Some(v) = r.get(&HashableKey::Str(CompactString::from("depth"))) {
                        depth = v.as_int().map(|d| d as usize);
                    }
                }
            }
            let text = pformat_value(&args[0], indent, width, depth, 0);
            if let Some(stream) = stream_obj {
                if let Some(write_fn) = stream.get_attr("write") {
                    let line = format!("{}\n", text);
                    let text_arg = PyObject::str_val(CompactString::from(&line));
                    match &write_fn.payload {
                        PyObjectPayload::NativeFunction { func, .. } => { let _ = func(&[text_arg]); }
                        PyObjectPayload::NativeClosure { func, .. } => { let _ = func(&[text_arg]); }
                        _ => { println!("{}", text); }
                    }
                } else {
                    println!("{}", text);
                }
            } else {
                println!("{}", text);
            }
            Ok(PyObject::none())
        })),
        ("pformat", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
            let mut indent = 1usize;
            let mut width = 80usize;
            let mut depth: Option<usize> = None;
            if args.len() > 1 { indent = args[1].as_int().unwrap_or(1) as usize; }
            if args.len() > 2 { width = args[2].as_int().unwrap_or(80) as usize; }
            if args.len() > 3 { depth = args[3].as_int().map(|d| d as usize); }
            let text = pformat_value(&args[0], indent, width, depth, 0);
            Ok(PyObject::str_val(CompactString::from(text)))
        })),
        ("PrettyPrinter", make_builtin(|_| Ok(PyObject::none()))),
        ("isreadable", make_builtin(|_| Ok(PyObject::bool_val(true)))),
        ("isrecursive", make_builtin(|_| Ok(PyObject::bool_val(false)))),
        ("saferepr", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
            Ok(PyObject::str_val(CompactString::from(args[0].py_to_string())))
        })),
    ])
}

// ── encodings module ──

pub fn create_encodings_module() -> PyObjectRef {
    // The encodings module provides codec registration and lookup.
    // In CPython this is a package (encodings/__init__.py) with sub-modules for each codec.
    // We provide a minimal stub that covers common use cases.

    let search_function = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::none()); }
        let name = args[0].py_to_string().to_lowercase().replace('-', "_");
        match name.as_str() {
            "utf_8" | "utf8" | "utf_8_sig" => {
                Ok(PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from("utf-8")),
                    PyObject::none(), // encode
                    PyObject::none(), // decode
                    PyObject::none(), // streamreader
                    PyObject::none(), // streamwriter
                ]))
            }
            "ascii" | "us_ascii" => {
                Ok(PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from("ascii")),
                    PyObject::none(),
                    PyObject::none(),
                    PyObject::none(),
                    PyObject::none(),
                ]))
            }
            "latin_1" | "iso8859_1" | "latin1" | "iso_8859_1" => {
                Ok(PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from("latin-1")),
                    PyObject::none(),
                    PyObject::none(),
                    PyObject::none(),
                    PyObject::none(),
                ]))
            }
            _ => Ok(PyObject::none()),
        }
    });

    let normalize_encoding = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
        let name = args[0].py_to_string().to_lowercase().replace('-', "_").replace(' ', "_");
        Ok(PyObject::str_val(CompactString::from(name)))
    });

    make_module("encodings", vec![
        ("search_function", search_function),
        ("normalize_encoding", normalize_encoding),
        // Sub-module aliases
        ("utf_8", make_builtin(|_| Ok(PyObject::none()))),
        ("ascii", make_builtin(|_| Ok(PyObject::none()))),
        ("latin_1", make_builtin(|_| Ok(PyObject::none()))),
    ])
}
