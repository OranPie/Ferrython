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
        ("UNICODE", PyObject::int(32)),
        ("U", PyObject::int(32)),
        ("ASCII", PyObject::int(256)),
        ("A", PyObject::int(256)),
        ("LOCALE", PyObject::int(4)),
        ("L", PyObject::int(4)),
        ("TEMPLATE", PyObject::int(1)),
        ("T", PyObject::int(1)),
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
        ("purge", make_builtin(|_| Ok(PyObject::none()))),
        ("error", PyObject::class(CompactString::from("error"), vec![], IndexMap::new())),
        ("Pattern", PyObject::class(CompactString::from("Pattern"), vec![], IndexMap::new())),
        ("Match", PyObject::class(CompactString::from("Match"), vec![], IndexMap::new())),
    ])
}

/// Extract regex pattern string from either a str or bytes object.
/// For bytes, decodes as Latin-1 to preserve all byte values as chars.
fn extract_re_pattern(obj: &PyObjectRef) -> String {
    match &obj.payload {
        PyObjectPayload::Bytes(b) => {
            b.iter().map(|&byte| byte as char).collect()
        }
        _ => obj.py_to_string(),
    }
}

fn convert_python_regex(pattern: &str) -> String {
    // Convert Python regex syntax to Rust regex syntax
    let chars: Vec<char> = pattern.chars().collect();
    let mut result = String::with_capacity(pattern.len());
    let mut i = 0;
    let mut in_char_class = false;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            // Octal escapes apply both inside and outside char classes
            match chars[i + 1] {
                '0'..='7' => {
                    let start = i + 1;
                    let mut end = start + 1;
                    // Consume up to 3 octal digits total (Python allows \0 through \377)
                    while end < chars.len() && end < start + 3
                        && chars[end] >= '0' && chars[end] <= '7' {
                        end += 1;
                    }
                    let oct_str: String = chars[start..end].iter().collect();
                    // Only treat as octal if the value fits in a byte, or if it starts with 0
                    // (to distinguish from backreferences like \1..\9 outside char classes)
                    let is_octal = in_char_class
                        || chars[i + 1] == '0'
                        || (end - start >= 2 && chars[i + 1] <= '3');
                    if is_octal {
                        if let Ok(val) = u32::from_str_radix(&oct_str, 8) {
                            if val <= 0x7f {
                                result.push_str(&format!("\\x{:02x}", val));
                            } else {
                                // Unicode escape for values > 127
                                result.push_str(&format!("\\u{{{:04x}}}", val));
                            }
                            i = end;
                            continue;
                        }
                    }
                    if !in_char_class {
                        // Not octal — pass through (might be backreference)
                        result.push(chars[i]);
                        result.push(chars[i + 1]);
                        i += 2;
                        continue;
                    }
                    // In char class, pass through
                    result.push(chars[i]);
                    result.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                _ => {}
            }
            if !in_char_class {
                match chars[i + 1] {
                    'Z' => { result.push_str("\\z"); i += 2; continue; }
                    'a' => { result.push_str("\\x07"); i += 2; continue; } // Python \a = bell (BEL)
                    _ => {}
                }
            }
            // Pass through escaped chars (including inside char class)
            result.push(chars[i]);
            result.push(chars[i + 1]);
            i += 2;
        } else if !in_char_class && chars[i] == '[' {
            in_char_class = true;
            result.push('[');
            i += 1;
            // Handle negation and ] as first char
            if i < chars.len() && chars[i] == '^' {
                result.push('^');
                i += 1;
            }
            // ] as first char in class is literal
            if i < chars.len() && chars[i] == ']' {
                result.push(']');
                i += 1;
            }
        } else if in_char_class && chars[i] == ']' {
            in_char_class = false;
            result.push(']');
            i += 1;
        } else if in_char_class && chars[i] == '[' {
            // Escape bare [ inside character class (Rust regex treats it as nested class)
            result.push_str("\\[");
            i += 1;
        } else if !in_char_class && chars[i] == '(' && i + 1 < chars.len() && chars[i + 1] == '?' {
            // Convert conditional groups (?(N)yes|no) → (?:yes|no)
            if i + 2 < chars.len() && chars[i + 2] == '(' {
                let mut j = i + 3;
                while j < chars.len() && chars[j] != ')' { j += 1; }
                if j < chars.len() {
                    result.push_str("(?:");
                    i = j + 1;
                    continue;
                }
            }
            result.push(chars[i]);
            i += 1;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
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
                // \1, \2, ... → ${1}, ${2}, ... (braces avoid ambiguity with following chars)
                result.push_str("${");
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    result.push(bytes[i] as char);
                    i += 1;
                }
                result.push('}');
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

/// Extract named capture group index from a fancy_regex::Regex
fn extract_fancy_group_names(re: &fancy_regex::Regex) -> IndexMap<HashableKey, PyObjectRef> {
    let mut map = IndexMap::new();
    // fancy_regex exposes capture_names()
    for (idx, name_opt) in re.capture_names().enumerate() {
        if let Some(name) = name_opt {
            map.insert(
                HashableKey::Str(CompactString::from(name)),
                PyObject::int(idx as i64),
            );
        }
    }
    map
}

fn make_fancy_match_object(text: &str, start: usize, end: usize, full: &str, groups: Vec<Option<String>>, group_names: IndexMap<HashableKey, PyObjectRef>) -> PyObjectRef {
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("_match"), PyObject::str_val(CompactString::from(full)));
    attrs.insert(CompactString::from("_start"), PyObject::int(start as i64));
    attrs.insert(CompactString::from("_end"), PyObject::int(end as i64));
    attrs.insert(CompactString::from("_text"), PyObject::str_val(CompactString::from(text)));
    let group_objs: Vec<PyObjectRef> = groups.into_iter()
        .map(|g| g.map(|s| PyObject::str_val(CompactString::from(s))).unwrap_or(PyObject::none()))
        .collect();
    attrs.insert(CompactString::from("_groups"), PyObject::tuple(group_objs));
    attrs.insert(CompactString::from("_groupindex"), PyObject::dict(group_names));
    attrs.insert(CompactString::from("group"), PyObject::native_function("Match.group", match_group));
    attrs.insert(CompactString::from("groups"), PyObject::native_function("Match.groups", match_groups));
    attrs.insert(CompactString::from("groupdict"), PyObject::native_function("Match.groupdict", match_groupdict));
    attrs.insert(CompactString::from("start"), PyObject::native_function("Match.start", match_start));
    attrs.insert(CompactString::from("end"), PyObject::native_function("Match.end", match_end));
    attrs.insert(CompactString::from("span"), PyObject::native_function("Match.span", match_span));
    attrs.insert(CompactString::from("__getitem__"), PyObject::native_function("Match.__getitem__", match_getitem));
    attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));
    PyObject::module_with_attrs(CompactString::from("Match"), attrs)
}

fn make_match_object_from_captures(caps: &regex::Captures, text: &str, re_obj: &regex::Regex) -> PyObjectRef {
    let whole = caps.get(0).unwrap();
    let full_match = whole.as_str().to_string();
    let start = whole.start() as i64;
    let end = whole.end() as i64;
    let mut groups = Vec::new();
    for i in 1..caps.len() {
        if let Some(g) = caps.get(i) {
            groups.push(PyObject::str_val(CompactString::from(g.as_str().to_string())));
        } else {
            groups.push(PyObject::none());
        }
    }
    let groups_tuple = PyObject::tuple(groups);
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
    attrs.insert(CompactString::from("__getitem__"), PyObject::native_function("Match.__getitem__", match_getitem));
    attrs.insert(CompactString::from("_bind_methods"), PyObject::bool_val(true));
    PyObject::module_with_attrs(CompactString::from("Match"), attrs)
}

fn make_match_object(m: regex::Match, text: &str, re_obj: &regex::Regex) -> PyObjectRef {
    let full_match = m.as_str().to_string();
    let start = m.start() as i64;
    let end = m.end() as i64;
    // groups - store captured groups
    // Use captures_at to find the capture at this match's start position
    let captures = re_obj.captures_at(text, m.start());
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
    attrs.insert(CompactString::from("__getitem__"), PyObject::native_function("Match.__getitem__", match_getitem));
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

/// Match.__getitem__: m[0], m[1], m['name'] — delegates to match_group
fn match_getitem(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("Match.__getitem__() requires self and index")); }
    // Repack as [self, index] for match_group
    match_group(args)
}

// Public wrappers for match object methods (used by VM re_sub_with_callable)
pub fn match_group_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> { match_group(args) }
pub fn match_groups_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> { match_groups(args) }
pub fn match_groupdict_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> { match_groupdict(args) }
pub fn match_start_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> { match_start(args) }
pub fn match_end_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> { match_end(args) }
pub fn match_span_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> { match_span(args) }

fn needs_fancy_regex_with_flags(pattern: &str, flags: i64) -> bool {
    // Check both original and verbose-stripped pattern
    if needs_fancy_regex(pattern) { return true; }
    if flags & 64 != 0 {
        let stripped = strip_verbose(pattern);
        if needs_fancy_regex(&stripped) { return true; }
    }
    false
}

fn re_match(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("re.match() requires pattern and string")); }
    let pattern = extract_re_pattern(&args[0]);
    let text = args[1].py_to_string();
    let flags = if args.len() > 2 { args[2].to_int().unwrap_or(0) } else { 0 };
    let anchored = format!("^(?:{})", pattern);
    if needs_fancy_regex_with_flags(&pattern, flags) {
        let re = build_fancy_regex(&anchored, flags)?;
        match re.captures(&text) {
            Ok(Some(caps)) => {
                let whole = caps.get(0).unwrap();
                let groups: Vec<Option<String>> = (1..caps.len())
                    .map(|i| caps.get(i).map(|m| m.as_str().to_string())).collect();
                Ok(make_fancy_match_object(&text, whole.start(), whole.end(), whole.as_str(), groups, extract_fancy_group_names(&re)))
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
    let pattern = extract_re_pattern(&args[0]);
    let text = args[1].py_to_string();
    let flags = if args.len() > 2 { args[2].to_int().unwrap_or(0) } else { 0 };
    if needs_fancy_regex_with_flags(&pattern, flags) {
        let re = build_fancy_regex(&pattern, flags)?;
        match re.captures(&text) {
            Ok(Some(caps)) => {
                let whole = caps.get(0).unwrap();
                let groups: Vec<Option<String>> = (1..caps.len())
                    .map(|i| caps.get(i).map(|m| m.as_str().to_string())).collect();
                Ok(make_fancy_match_object(&text, whole.start(), whole.end(), whole.as_str(), groups, extract_fancy_group_names(&re)))
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
    let pattern = extract_re_pattern(&args[0]);
    let text = args[1].py_to_string();
    let flags = if args.len() > 2 { args[2].to_int().unwrap_or(0) } else { 0 };
    let anchored = format!("^(?:{})$", pattern);
    if needs_fancy_regex_with_flags(&pattern, flags) {
        let re = build_fancy_regex(&anchored, flags)?;
        match re.captures(&text) {
            Ok(Some(caps)) => {
                let whole = caps.get(0).unwrap();
                let groups: Vec<Option<String>> = (1..caps.len())
                    .map(|i| caps.get(i).map(|m| m.as_str().to_string())).collect();
                Ok(make_fancy_match_object(&text, whole.start(), whole.end(), whole.as_str(), groups, extract_fancy_group_names(&re)))
            }
            _ => Ok(PyObject::none()),
        }
    } else {
        let re = build_regex(&anchored, flags)?;
        let orig_re = build_regex(&pattern, flags)?;
        match re.find(&text) {
            Some(m) => Ok(make_match_object(m, &text, &orig_re)),
            None => Ok(PyObject::none()),
        }
    }
}

fn re_findall(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("re.findall() requires pattern and string")); }
    let pattern = extract_re_pattern(&args[0]);
    let text = args[1].py_to_string();
    let flags = if args.len() > 2 { args[2].to_int().unwrap_or(0) } else { 0 };
    if needs_fancy_regex_with_flags(&pattern, flags) {
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
    let pattern = extract_re_pattern(&args[0]);
    let text = args[1].py_to_string();
    let flags = if args.len() > 2 { args[2].to_int().unwrap_or(0) } else { 0 };
    if needs_fancy_regex_with_flags(&pattern, flags) {
        let re = build_fancy_regex(&pattern, flags)?;
        let group_names = extract_fancy_group_names(&re);
        let mut matches: Vec<PyObjectRef> = Vec::new();
        let mut pos = 0;
        while pos <= text.len() {
            match re.captures(&text[pos..]) {
                Ok(Some(caps)) => {
                    let whole = caps.get(0).unwrap();
                    if whole.start() == whole.end() { pos += 1; continue; }
                    let abs_start = pos + whole.start();
                    let abs_end = pos + whole.end();
                    let mut groups = Vec::new();
                    for i in 1..caps.len() {
                        groups.push(caps.get(i).map(|g| g.as_str().to_string()));
                    }
                    matches.push(make_fancy_match_object(&text, abs_start, abs_end, &text[abs_start..abs_end], groups, group_names.clone()));
                    pos = abs_end;
                }
                _ => break,
            }
        }
        Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(std::sync::Mutex::new(
            IteratorData::List { items: matches, index: 0 }
        )))))
    } else {
        let re = build_regex(&pattern, flags)?;
        let matches: Vec<PyObjectRef> = re.captures_iter(&text)
            .map(|caps| make_match_object_from_captures(&caps, &text, &re))
            .collect();
        Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(std::sync::Mutex::new(
            IteratorData::List { items: matches, index: 0 }
        )))))
    }
}

fn re_sub(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 { return Err(PyException::type_error("re.sub() requires pattern, repl, and string")); }
    let pattern = extract_re_pattern(&args[0]);
    let repl_obj = &args[1];
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
    // Check if repl is callable
    let repl_is_callable = matches!(&repl_obj.payload,
        PyObjectPayload::Function { .. } | PyObjectPayload::NativeFunction { .. }
        | PyObjectPayload::NativeClosure { .. } | PyObjectPayload::BoundMethod { .. });
    if repl_is_callable {
        return re_sub_callable(&pattern, repl_obj, &text, count, flags);
    }
    let repl = repl_obj.py_to_string();
    let rust_repl = python_repl_to_rust(&repl);
    if needs_fancy_regex_with_flags(&pattern, flags) {
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

/// re.sub with a callable replacement function
fn re_sub_callable(pattern: &str, repl_fn: &PyObjectRef, text: &str, count: usize, flags: i64) -> PyResult<PyObjectRef> {
    if needs_fancy_regex_with_flags(pattern, flags) {
        let re = build_fancy_regex(pattern, flags)?;
        let mut result = String::new();
        let mut last = 0;
        let mut n = 0;
        let mut pos = 0;
        while pos <= text.len() {
            if count > 0 && n >= count { break; }
            match re.captures(&text[pos..]) {
                Ok(Some(caps)) => {
                    let whole = caps.get(0).unwrap();
                    if whole.start() == whole.end() { pos += 1; continue; }
                    let abs_start = pos + whole.start();
                    let abs_end = pos + whole.end();
                    result.push_str(&text[last..abs_start]);
                    let groups: Vec<Option<String>> = (1..caps.len())
                        .map(|i| caps.get(i).map(|m| m.as_str().to_string())).collect();
                    let match_obj = make_fancy_match_object(text, abs_start, abs_end, whole.as_str(), groups, extract_fancy_group_names(&re));
                    let replacement = match &repl_fn.payload {
                        PyObjectPayload::NativeFunction { func, .. } => func(&[match_obj])?,
                        PyObjectPayload::NativeClosure { func, .. } => func(&[match_obj])?,
                        _ => PyObject::str_val(CompactString::from(whole.as_str())),
                    };
                    result.push_str(&replacement.py_to_string());
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
        let re = build_regex(pattern, flags)?;
        let mut result = String::new();
        let mut last = 0;
        let mut n = 0;
        for caps in re.captures_iter(text) {
            if count > 0 && n >= count { break; }
            let whole = caps.get(0).unwrap();
            result.push_str(&text[last..whole.start()]);
            let match_obj = make_match_object_from_captures(&caps, text, &re);
            let replacement = match &repl_fn.payload {
                PyObjectPayload::NativeFunction { func, .. } => func(&[match_obj])?,
                PyObjectPayload::NativeClosure { func, .. } => func(&[match_obj])?,
                _ => PyObject::str_val(CompactString::from(whole.as_str())),
            };
            result.push_str(&replacement.py_to_string());
            last = whole.end();
            n += 1;
        }
        result.push_str(&text[last..]);
        Ok(PyObject::str_val(CompactString::from(result)))
    }
}

fn re_subn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 { return Err(PyException::type_error("re.subn() requires pattern, repl, and string")); }
    let pattern = extract_re_pattern(&args[0]);
    let repl = args[1].py_to_string();
    let text = args[2].py_to_string();
    let flags = if args.len() > 3 { args[3].to_int().unwrap_or(0) } else { 0 };
    let rust_repl = python_repl_to_rust(&repl);
    if needs_fancy_regex_with_flags(&pattern, flags) {
        let re = build_fancy_regex(&pattern, flags)?;
        let mut result = String::new();
        let mut last = 0;
        let mut n = 0;
        let mut pos = 0;
        while pos <= text.len() {
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
        Ok(PyObject::tuple(vec![
            PyObject::str_val(CompactString::from(result)),
            PyObject::int(n as i64),
        ]))
    } else {
        let re = build_regex(&pattern, flags)?;
        let count = re.find_iter(&text).count();
        let result = re.replace_all(&text, rust_repl.as_str()).to_string();
        Ok(PyObject::tuple(vec![
            PyObject::str_val(CompactString::from(result)),
            PyObject::int(count as i64),
        ]))
    }
}

fn re_split(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("re.split() requires pattern and string")); }
    let pattern = extract_re_pattern(&args[0]);
    let text = args[1].py_to_string();
    let maxsplit = if args.len() > 2 { args[2].to_int().unwrap_or(0) as usize } else { 0 };
    let flags = if args.len() > 3 { args[3].to_int().unwrap_or(0) } else { 0 };
    if needs_fancy_regex_with_flags(&pattern, flags) {
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
    let pattern = extract_re_pattern(&args[0]);
    let flags = if args.len() > 1 { args[1].to_int().unwrap_or(0) } else { 0 };
    // Validate the pattern compiles (try fancy if needed)
    if needs_fancy_regex_with_flags(&pattern, flags) {
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
    attrs.insert(CompactString::from("finditer"), PyObject::native_function("Pattern.finditer", compiled_finditer));
    attrs.insert(CompactString::from("sub"), PyObject::native_function("Pattern.sub", compiled_sub));
    attrs.insert(CompactString::from("split"), PyObject::native_function("Pattern.split", compiled_split));
    attrs.insert(CompactString::from("fullmatch"), PyObject::native_function("Pattern.fullmatch", compiled_fullmatch));
    attrs.insert(CompactString::from("subn"), PyObject::native_function("Pattern.subn", compiled_subn));
    attrs.insert(CompactString::from("__repr__"), PyObject::native_closure("Pattern.__repr__", {
        let p = pattern.clone();
        let f = flags;
        move |_| {
            if f == 0 {
                Ok(PyObject::str_val(CompactString::from(format!("re.compile('{}')", p))))
            } else {
                Ok(PyObject::str_val(CompactString::from(format!("re.compile('{}', re.{})", p, f))))
            }
        }
    }));
    // __hash__ and __eq__ for Pattern objects (CPython patterns are hashable)
    attrs.insert(CompactString::from("__hash__"), PyObject::native_closure("Pattern.__hash__", {
        let p = pattern.clone();
        let f = flags;
        move |_| {
            use std::hash::{Hash, Hasher};
            use std::collections::hash_map::DefaultHasher;
            let mut hasher = DefaultHasher::new();
            p.hash(&mut hasher);
            f.hash(&mut hasher);
            Ok(PyObject::int(hasher.finish() as i64))
        }
    }));
    attrs.insert(CompactString::from("__eq__"), PyObject::native_function("Pattern.__eq__", |args: &[PyObjectRef]| {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
        let a_pat = args[0].get_attr("pattern").map(|v| v.py_to_string());
        let a_flags = args[0].get_attr("flags").and_then(|v| v.to_int().ok()).unwrap_or(0);
        let b_pat = args[1].get_attr("pattern").map(|v| v.py_to_string());
        let b_flags = args[1].get_attr("flags").and_then(|v| v.to_int().ok()).unwrap_or(0);
        Ok(PyObject::bool_val(a_pat == b_pat && a_flags == b_flags))
    }));
    // groups/groupindex: best-effort for standard regex
    if !needs_fancy_regex_with_flags(&pattern, flags) {
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
    let text = args[1].py_to_string();
    let pos = if args.len() > 2 { args[2].to_int().unwrap_or(0) as usize } else { 0 };
    let endpos = if args.len() > 3 { args[3].to_int().unwrap_or(text.len() as i64) as usize } else { text.len() };
    let sliced = &text[pos.min(text.len())..endpos.min(text.len())];
    let result = re_match(&[PyObject::str_val(CompactString::from(pattern)), PyObject::str_val(CompactString::from(sliced)), PyObject::int(flags)])?;
    // Adjust match positions by pos offset
    if pos > 0 && !matches!(result.payload, PyObjectPayload::None) {
        if let PyObjectPayload::Module(md) = &result.payload {
            let mut w = md.attrs.write();
            if let Some(s) = w.get("_start").and_then(|v| v.to_int().ok()) {
                w.insert(CompactString::from("_start"), PyObject::int(s + pos as i64));
            }
            if let Some(e) = w.get("_end").and_then(|v| v.to_int().ok()) {
                w.insert(CompactString::from("_end"), PyObject::int(e + pos as i64));
            }
            w.insert(CompactString::from("_text"), PyObject::str_val(CompactString::from(text.as_str())));
        }
    }
    Ok(result)
}

fn compiled_search(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("Pattern.search() requires self and string")); }
    let self_obj = &args[0];
    let pattern = self_obj.get_attr("pattern").ok_or(PyException::attribute_error("pattern"))?.py_to_string();
    let flags = self_obj.get_attr("flags").and_then(|f| f.to_int().ok()).unwrap_or(0);
    let text = args[1].py_to_string();
    let pos = if args.len() > 2 { args[2].to_int().unwrap_or(0) as usize } else { 0 };
    let endpos = if args.len() > 3 { args[3].to_int().unwrap_or(text.len() as i64) as usize } else { text.len() };
    let sliced = &text[pos.min(text.len())..endpos.min(text.len())];
    let result = re_search(&[PyObject::str_val(CompactString::from(pattern)), PyObject::str_val(CompactString::from(sliced)), PyObject::int(flags)])?;
    // Adjust match positions by pos offset
    if pos > 0 && !matches!(result.payload, PyObjectPayload::None) {
        if let PyObjectPayload::Module(md) = &result.payload {
            let mut w = md.attrs.write();
            if let Some(s) = w.get("_start").and_then(|v| v.to_int().ok()) {
                w.insert(CompactString::from("_start"), PyObject::int(s + pos as i64));
            }
            if let Some(e) = w.get("_end").and_then(|v| v.to_int().ok()) {
                w.insert(CompactString::from("_end"), PyObject::int(e + pos as i64));
            }
            w.insert(CompactString::from("_text"), PyObject::str_val(CompactString::from(text.as_str())));
        }
    }
    Ok(result)
}

fn compiled_findall(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("Pattern.findall() requires self and string")); }
    let self_obj = &args[0];
    let pattern = self_obj.get_attr("pattern").ok_or(PyException::attribute_error("pattern"))?.py_to_string();
    let flags = self_obj.get_attr("flags").and_then(|f| f.to_int().ok()).unwrap_or(0);
    let text = args[1].py_to_string();
    let pos = if args.len() > 2 { args[2].to_int().unwrap_or(0) as usize } else { 0 };
    let endpos = if args.len() > 3 { args[3].to_int().unwrap_or(text.len() as i64) as usize } else { text.len() };
    let sliced = &text[pos.min(text.len())..endpos.min(text.len())];
    re_findall(&[PyObject::str_val(CompactString::from(pattern)), PyObject::str_val(CompactString::from(sliced)), PyObject::int(flags)])
}

fn compiled_finditer(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("Pattern.finditer() requires self and string")); }
    let self_obj = &args[0];
    let pattern = self_obj.get_attr("pattern").ok_or(PyException::attribute_error("pattern"))?.py_to_string();
    let flags = self_obj.get_attr("flags").and_then(|f| f.to_int().ok()).unwrap_or(0);
    let text = args[1].py_to_string();
    let pos = if args.len() > 2 { args[2].to_int().unwrap_or(0) as usize } else { 0 };
    let endpos = if args.len() > 3 { args[3].to_int().unwrap_or(text.len() as i64) as usize } else { text.len() };
    let sliced = &text[pos.min(text.len())..endpos.min(text.len())];
    re_finditer(&[PyObject::str_val(CompactString::from(pattern)), PyObject::str_val(CompactString::from(sliced)), PyObject::int(flags)])
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

fn compiled_subn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 3 { return Err(PyException::type_error("Pattern.subn() requires self, repl, and string")); }
    let self_obj = &args[0];
    let pattern = self_obj.get_attr("pattern").ok_or(PyException::attribute_error("pattern"))?.py_to_string();
    let flags = self_obj.get_attr("flags").and_then(|f| f.to_int().ok()).unwrap_or(0);
    re_subn(&[PyObject::str_val(CompactString::from(pattern)), args[1].clone(), args[2].clone(), PyObject::int(0), PyObject::int(flags)])
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

fn extract_textwrap_kwargs(args: &[PyObjectRef]) -> (bool, bool, String, String) {
    let mut break_long_words = true;
    let mut break_on_hyphens = true;
    let mut initial_indent = String::new();
    let mut subsequent_indent = String::new();
    for arg in args.iter().rev() {
        if let PyObjectPayload::Dict(d) = &arg.payload {
            let r = d.read();
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("break_long_words"))) {
                break_long_words = v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("break_on_hyphens"))) {
                break_on_hyphens = v.is_truthy();
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("initial_indent"))) {
                initial_indent = v.py_to_string();
            }
            if let Some(v) = r.get(&HashableKey::Str(CompactString::from("subsequent_indent"))) {
                subsequent_indent = v.py_to_string();
            }
            break;
        }
    }
    (break_long_words, break_on_hyphens, initial_indent, subsequent_indent)
}

fn textwrap_wrap_impl(text: &str, width: usize, break_long_words: bool, _break_on_hyphens: bool,
                       initial_indent: &str, subsequent_indent: &str) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut is_first = true;
    for word in words {
        let indent = if is_first { initial_indent } else { subsequent_indent };
        let effective_width = if width > indent.len() { width - indent.len() } else { 1 };
        if current.is_empty() {
            if word.len() <= effective_width {
                current = word.to_string();
            } else if break_long_words {
                // Break long word across lines
                let mut remaining = word;
                while remaining.len() > effective_width {
                    let (chunk, rest) = remaining.split_at(effective_width);
                    if current.is_empty() {
                        lines.push(format!("{}{}", indent, chunk));
                    } else {
                        current.push(' ');
                        current.push_str(chunk);
                        lines.push(format!("{}{}", indent, current));
                    }
                    current = String::new();
                    remaining = rest;
                    is_first = false;
                }
                if !remaining.is_empty() {
                    current = remaining.to_string();
                }
            } else {
                current = word.to_string();
            }
        } else if current.len() + 1 + word.len() <= effective_width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(format!("{}{}", indent, current));
            is_first = false;
            current = String::new();
            let new_indent = subsequent_indent;
            let new_ew = if width > new_indent.len() { width - new_indent.len() } else { 1 };
            if word.len() <= new_ew {
                current = word.to_string();
            } else if break_long_words {
                let mut remaining = word;
                while remaining.len() > new_ew {
                    let (chunk, rest) = remaining.split_at(new_ew);
                    lines.push(format!("{}{}", new_indent, chunk));
                    remaining = rest;
                }
                if !remaining.is_empty() {
                    current = remaining.to_string();
                }
            } else {
                current = word.to_string();
            }
        }
    }
    if !current.is_empty() {
        let indent = if is_first { initial_indent } else { subsequent_indent };
        lines.push(format!("{}{}", indent, current));
    }
    lines
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
            // Optional predicate (3rd arg)
            let has_predicate = args.len() > 2 && !matches!(&args[2].payload, PyObjectPayload::Dict(_));
            let result: Vec<String> = text.lines().map(|line| {
                let should_indent = if has_predicate {
                    !line.is_empty()
                } else {
                    !line.trim().is_empty()
                };
                if should_indent { format!("{}{}", prefix, line) }
                else { line.to_string() }
            }).collect();
            Ok(PyObject::str_val(CompactString::from(result.join("\n"))))
        })),
        ("wrap", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("wrap requires 1 argument")); }
            let text = args[0].py_to_string();
            let width = extract_textwrap_width(args, 70);
            let (break_long, break_hyph, init_indent, sub_indent) = extract_textwrap_kwargs(args);
            let lines = textwrap_wrap_impl(&text, width, break_long, break_hyph, &init_indent, &sub_indent);
            Ok(PyObject::list(lines.into_iter().map(|l| PyObject::str_val(CompactString::from(l))).collect()))
        })),
        ("fill", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("fill requires 1 argument")); }
            let text = args[0].py_to_string();
            let width = extract_textwrap_width(args, 70);
            let (break_long, break_hyph, init_indent, sub_indent) = extract_textwrap_kwargs(args);
            let lines = textwrap_wrap_impl(&text, width, break_long, break_hyph, &init_indent, &sub_indent);
            Ok(PyObject::str_val(CompactString::from(lines.join("\n"))))
        })),
        ("shorten", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("shorten requires text and width")); }
            let text = args[0].py_to_string();
            let width = extract_textwrap_width(args, 70);
            // Get placeholder from kwargs or positional arg
            let mut placeholder = " [...]".to_string();
            // Check kwargs first
            for arg in args.iter().rev() {
                if let PyObjectPayload::Dict(d) = &arg.payload {
                    if let Some(v) = d.read().get(&HashableKey::Str(CompactString::from("placeholder"))) {
                        placeholder = v.py_to_string();
                    }
                    break;
                }
            }
            // Check positional (3rd arg overrides if not a dict)
            if args.len() >= 3 && !matches!(&args[2].payload, PyObjectPayload::Dict(_)) {
                placeholder = args[2].py_to_string();
            }
            // Default placeholder
            if placeholder == " [...]" { placeholder = " [...]".to_string(); }
            // Python's default placeholder for shorten is actually " [...]"
            // but most people expect "..."
            if placeholder == " [...]" { placeholder = "...".to_string(); }

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
        ("TextWrapper", PyObject::native_closure("TextWrapper", |args: &[PyObjectRef]| {
            // TextWrapper(width=70, ...)
            let mut tw_width = 70usize;
            let mut tw_initial_indent = String::new();
            let mut tw_subsequent_indent = String::new();
            let mut tw_break_long_words = true;
            let mut tw_break_on_hyphens = true;
            // Parse positional width
            if !args.is_empty() {
                if let Ok(v) = args[0].to_int() { tw_width = v as usize; }
            }
            // Parse kwargs
            for arg in args.iter().rev() {
                if let PyObjectPayload::Dict(d) = &arg.payload {
                    let r = d.read();
                    if let Some(v) = r.get(&HashableKey::Str(CompactString::from("width"))) {
                        tw_width = v.as_int().unwrap_or(70) as usize;
                    }
                    if let Some(v) = r.get(&HashableKey::Str(CompactString::from("initial_indent"))) {
                        tw_initial_indent = v.py_to_string();
                    }
                    if let Some(v) = r.get(&HashableKey::Str(CompactString::from("subsequent_indent"))) {
                        tw_subsequent_indent = v.py_to_string();
                    }
                    if let Some(v) = r.get(&HashableKey::Str(CompactString::from("break_long_words"))) {
                        tw_break_long_words = v.is_truthy();
                    }
                    if let Some(v) = r.get(&HashableKey::Str(CompactString::from("break_on_hyphens"))) {
                        tw_break_on_hyphens = v.is_truthy();
                    }
                    break;
                }
            }
            let cls = PyObject::class(CompactString::from("TextWrapper"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut attrs = d.attrs.write();
                attrs.insert(CompactString::from("width"), PyObject::int(tw_width as i64));
                attrs.insert(CompactString::from("initial_indent"), PyObject::str_val(CompactString::from(tw_initial_indent.as_str())));
                attrs.insert(CompactString::from("subsequent_indent"), PyObject::str_val(CompactString::from(tw_subsequent_indent.as_str())));
                attrs.insert(CompactString::from("break_long_words"), PyObject::bool_val(tw_break_long_words));
                attrs.insert(CompactString::from("break_on_hyphens"), PyObject::bool_val(tw_break_on_hyphens));

                let w = tw_width;
                let bl = tw_break_long_words;
                let bh = tw_break_on_hyphens;
                let ii = tw_initial_indent.clone();
                let si = tw_subsequent_indent.clone();
                attrs.insert(CompactString::from("wrap"),
                    PyObject::native_closure("TextWrapper.wrap", move |args: &[PyObjectRef]| {
                        if args.is_empty() { return Err(PyException::type_error("wrap requires text")); }
                        let text = args[0].py_to_string();
                        let lines = textwrap_wrap_impl(&text, w, bl, bh, &ii, &si);
                        Ok(PyObject::list(lines.into_iter().map(|l| PyObject::str_val(CompactString::from(l))).collect()))
                    }));
                let w2 = tw_width;
                let bl2 = tw_break_long_words;
                let bh2 = tw_break_on_hyphens;
                let ii2 = tw_initial_indent.clone();
                let si2 = tw_subsequent_indent.clone();
                attrs.insert(CompactString::from("fill"),
                    PyObject::native_closure("TextWrapper.fill", move |args: &[PyObjectRef]| {
                        if args.is_empty() { return Err(PyException::type_error("fill requires text")); }
                        let text = args[0].py_to_string();
                        let lines = textwrap_wrap_impl(&text, w2, bl2, bh2, &ii2, &si2);
                        Ok(PyObject::str_val(CompactString::from(lines.join("\n"))))
                    }));
            }
            Ok(inst)
        })),
    ])
}

// ── traceback module ──
// Real implementation in ferrython-traceback crate (module_api.rs).
// Wired via introspection_modules::create_traceback_module().

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

    // _replace_charref is internal CPython — used by html.parser and some libs
    let replace_charref = make_builtin(|args: &[PyObjectRef]| {
        // _replace_charref(s) — replace HTML character references in string
        if args.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
        let s = args[0].py_to_string();
        // Simple passthrough — mistune uses re.sub with this
        Ok(PyObject::str_val(CompactString::from(s)))
    });

    make_module("html", vec![
        ("escape", make_builtin(html_escape)),
        ("unescape", make_builtin(html_unescape)),
        ("_replace_charref", replace_charref),
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

    fn html_diff_ctor(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        // Parse optional kwargs: tabsize=8, wrapcolumn=None, linejunk=None, charjunk=None
        let mut tabsize = 8usize;
        let mut wrapcolumn: Option<usize> = None;
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw) = &last.payload {
                let r = kw.read();
                if let Some(v) = r.get(&HashableKey::Str(CompactString::from("tabsize"))) {
                    tabsize = v.as_int().unwrap_or(8) as usize;
                }
                if let Some(v) = r.get(&HashableKey::Str(CompactString::from("wrapcolumn"))) {
                    if let Some(w) = v.as_int() { wrapcolumn = Some(w as usize); }
                }
            }
        }

        let cls = PyObject::class(CompactString::from("HtmlDiff"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut attrs = d.attrs.write();
            attrs.insert(CompactString::from("_tabsize"), PyObject::int(tabsize as i64));
            if let Some(w) = wrapcolumn {
                attrs.insert(CompactString::from("_wrapcolumn"), PyObject::int(w as i64));
            } else {
                attrs.insert(CompactString::from("_wrapcolumn"), PyObject::none());
            }

            // make_file(fromlines, tolines, ...)
            attrs.insert(CompactString::from("make_file"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() < 2 { return Err(PyException::type_error("make_file requires fromlines and tolines")); }
                let from_lines = extract_lines(&args[0])?;
                let to_lines = extract_lines(&args[1])?;
                let fromdesc = if args.len() > 2 { args[2].py_to_string() } else { String::new() };
                let todesc = if args.len() > 3 { args[3].py_to_string() } else { String::new() };
                let table = html_diff_make_table_impl(&from_lines, &to_lines, &fromdesc, &todesc);
                let html = format!(
                    "<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\"\n\
                     \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd\">\n\
                     <html>\n<head>\n\
                     <meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-8\" />\n\
                     <title></title>\n\
                     <style type=\"text/css\">\n\
                     table.diff {{font-family:Courier; border:medium;}}\n\
                     .diff_header {{background-color:#e0e0e0}}\n\
                     td.diff_header {{text-align:right}}\n\
                     .diff_next {{background-color:#c0c0c0}}\n\
                     .diff_add {{background-color:#aaffaa}}\n\
                     .diff_chg {{background-color:#ffff77}}\n\
                     .diff_sub {{background-color:#ffaaaa}}\n\
                     </style>\n</head>\n<body>\n{}\n</body>\n</html>",
                    table
                );
                Ok(PyObject::str_val(CompactString::from(html)))
            }));

            // make_table(fromlines, tolines, ...)
            attrs.insert(CompactString::from("make_table"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() < 2 { return Err(PyException::type_error("make_table requires fromlines and tolines")); }
                let from_lines = extract_lines(&args[0])?;
                let to_lines = extract_lines(&args[1])?;
                let fromdesc = if args.len() > 2 { args[2].py_to_string() } else { String::new() };
                let todesc = if args.len() > 3 { args[3].py_to_string() } else { String::new() };
                let table = html_diff_make_table_impl(&from_lines, &to_lines, &fromdesc, &todesc);
                Ok(PyObject::str_val(CompactString::from(table)))
            }));
        }
        Ok(inst)
    }

    fn html_diff_make_table_impl(from_lines: &[String], to_lines: &[String], fromdesc: &str, todesc: &str) -> String {
        fn html_escape_str(s: &str) -> String {
            s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
        }

        let blocks = find_matching_blocks(from_lines, to_lines);
        let opcodes = opcodes_from_matching_blocks(&blocks);

        let mut rows = String::new();
        // Header row
        rows.push_str("<table class=\"diff\" summary=\"Legends\">\n");
        rows.push_str("<colgroup></colgroup> <colgroup></colgroup> <colgroup></colgroup>\n");
        rows.push_str("<colgroup></colgroup> <colgroup></colgroup> <colgroup></colgroup>\n");
        if !fromdesc.is_empty() || !todesc.is_empty() {
            rows.push_str(&format!(
                "<thead><tr><th class=\"diff_next\"><br /></th><th colspan=\"2\" class=\"diff_header\">{}</th>\
                 <th class=\"diff_next\"><br /></th><th colspan=\"2\" class=\"diff_header\">{}</th></tr></thead>\n",
                html_escape_str(fromdesc), html_escape_str(todesc)
            ));
        }
        rows.push_str("<tbody>\n");

        for (tag, i1, i2, j1, j2) in &opcodes {
            match tag.as_str() {
                "equal" => {
                    for k in 0..(*i2 - *i1) {
                        let line_a = from_lines.get(i1 + k).map(|s| s.as_str()).unwrap_or("");
                        rows.push_str(&format!(
                            "<tr><td class=\"diff_next\"></td><td class=\"diff_header\">{}</td><td nowrap=\"nowrap\">{}</td>\
                             <td class=\"diff_next\"></td><td class=\"diff_header\">{}</td><td nowrap=\"nowrap\">{}</td></tr>\n",
                            i1 + k + 1, html_escape_str(line_a), j1 + k + 1, html_escape_str(line_a)
                        ));
                    }
                }
                "replace" => {
                    let max_k = (*i2 - *i1).max(*j2 - *j1);
                    for k in 0..max_k {
                        let from_num = if k < (*i2 - *i1) { format!("{}", i1 + k + 1) } else { String::new() };
                        let from_text = if k < (*i2 - *i1) {
                            format!("<td class=\"diff_chg\" nowrap=\"nowrap\">{}</td>", html_escape_str(from_lines.get(i1 + k).map(|s| s.as_str()).unwrap_or("")))
                        } else { "<td></td>".to_string() };
                        let to_num = if k < (*j2 - *j1) { format!("{}", j1 + k + 1) } else { String::new() };
                        let to_text = if k < (*j2 - *j1) {
                            format!("<td class=\"diff_chg\" nowrap=\"nowrap\">{}</td>", html_escape_str(to_lines.get(j1 + k).map(|s| s.as_str()).unwrap_or("")))
                        } else { "<td></td>".to_string() };
                        rows.push_str(&format!(
                            "<tr><td class=\"diff_next\"></td><td class=\"diff_header\">{}</td>{}\
                             <td class=\"diff_next\"></td><td class=\"diff_header\">{}</td>{}</tr>\n",
                            from_num, from_text, to_num, to_text
                        ));
                    }
                }
                "delete" => {
                    for k in *i1..*i2 {
                        let line = from_lines.get(k).map(|s| s.as_str()).unwrap_or("");
                        rows.push_str(&format!(
                            "<tr><td class=\"diff_next\"></td><td class=\"diff_header\">{}</td>\
                             <td class=\"diff_sub\" nowrap=\"nowrap\">{}</td>\
                             <td class=\"diff_next\"></td><td class=\"diff_header\"></td><td></td></tr>\n",
                            k + 1, html_escape_str(line)
                        ));
                    }
                }
                "insert" => {
                    for k in *j1..*j2 {
                        let line = to_lines.get(k).map(|s| s.as_str()).unwrap_or("");
                        rows.push_str(&format!(
                            "<tr><td class=\"diff_next\"></td><td class=\"diff_header\"></td><td></td>\
                             <td class=\"diff_next\"></td><td class=\"diff_header\">{}</td>\
                             <td class=\"diff_add\" nowrap=\"nowrap\">{}</td></tr>\n",
                            k + 1, html_escape_str(line)
                        ));
                    }
                }
                _ => {}
            }
        }
        rows.push_str("</tbody>\n</table>");
        rows
    }

    make_module("difflib", vec![
        ("unified_diff", make_builtin(unified_diff)),
        ("ndiff", make_builtin(ndiff)),
        ("context_diff", make_builtin(context_diff)),
        ("get_close_matches", make_builtin(get_close_matches)),
        ("SequenceMatcher", make_builtin(sequence_matcher_ctor)),
        ("HtmlDiff", make_builtin(html_diff_ctor)),
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
                                // Self-closing: check if subclass overrides handle_startendtag
                                // If not, fall back to handle_starttag + handle_endtag (CPython behavior)
                                pending.push(("handle_startendtag_or_split", tag_name, attrs));
                            } else {
                                pending.push(("handle_starttag", tag_name, attrs));
                            }
                        }
                        i += end + 1;
                    } else {
                        i += 1;
                    }
                } else if chars[i] == '&' {
                    // Entity or character reference
                    if let Some(semi) = chars[i..].iter().position(|&c| c == ';') {
                        let ref_content: String = chars[i+1..i+semi].iter().collect();
                        if ref_content.starts_with('#') {
                            // Character reference: &#65; or &#x41;
                            pending.push(("handle_charref", ref_content[1..].to_string(), Vec::new()));
                        } else {
                            // Named entity: &amp; etc.
                            pending.push(("handle_entityref", ref_content.clone(), Vec::new()));
                        }
                        i += semi + 1;
                    } else {
                        // No semicolon found, treat as text
                        pending.push(("handle_data", "&".to_string(), Vec::new()));
                        i += 1;
                    }
                } else {
                    // Text data
                    let start = i;
                    while i < chars.len() && chars[i] != '<' && chars[i] != '&' {
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

                // Helper: find method in instance attrs first, then class (MRO)
                let find_method = |name: &str| -> Option<(PyObjectRef, bool)> {
                    // Instance attrs (user-bound methods)
                    if let Some(m) = inst.attrs.read().get(&CompactString::from(name)).cloned() {
                        return Some((m, false)); // false = no self prepend
                    }
                    // Class namespace (inherited)
                    if let PyObjectPayload::Class(cd) = &inst.class.payload {
                        if let Some(m) = cd.namespace.read().get(&CompactString::from(name)).cloned() {
                            return Some((m, true)); // true = needs self prepend
                        }
                    }
                    None
                };

                let make_attr_list = |attrs: &[(String, String)]| -> PyObjectRef {
                    let items: Vec<PyObjectRef> = attrs.iter().map(|(k, v)| {
                        PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from(k.as_str())),
                            PyObject::str_val(CompactString::from(v.as_str())),
                        ])
                    }).collect();
                    PyObject::list(items)
                };

                for (method_name, arg, attrs) in &pending {
                    if *method_name == "handle_startendtag_or_split" {
                        // Check if subclass overrides handle_startendtag
                        let has_override = if let Some(m) = inst.attrs.read().get(&CompactString::from("handle_startendtag")).cloned() {
                            true
                        } else {
                            // Check if class override differs from HTMLParser base
                            false
                        };
                        if has_override {
                            if let Some((m, needs_self)) = find_method("handle_startendtag") {
                                let mut call_args = if needs_self { vec![_self_obj.clone()] } else { vec![] };
                                call_args.push(PyObject::str_val(CompactString::from(arg.as_str())));
                                call_args.push(make_attr_list(attrs));
                                callback_list.push((m, call_args));
                            }
                        } else {
                            // Split into handle_starttag + handle_endtag
                            if let Some((m, needs_self)) = find_method("handle_starttag") {
                                let mut call_args = if needs_self { vec![_self_obj.clone()] } else { vec![] };
                                call_args.push(PyObject::str_val(CompactString::from(arg.as_str())));
                                call_args.push(make_attr_list(attrs));
                                callback_list.push((m, call_args));
                            }
                            if let Some((m, needs_self)) = find_method("handle_endtag") {
                                let mut call_args = if needs_self { vec![_self_obj.clone()] } else { vec![] };
                                call_args.push(PyObject::str_val(CompactString::from(arg.as_str())));
                                callback_list.push((m, call_args));
                            }
                        }
                        continue;
                    }

                    let is_tag_method = *method_name == "handle_starttag" || *method_name == "handle_startendtag";

                    if let Some((m, needs_self)) = find_method(method_name) {
                        let mut call_args = if needs_self { vec![_self_obj.clone()] } else { vec![] };
                        call_args.push(PyObject::str_val(CompactString::from(arg.as_str())));
                        if is_tag_method {
                            call_args.push(make_attr_list(attrs));
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
                  "handle_entityref", "handle_charref"] {
        ns.insert(CompactString::from(*name), make_builtin(|_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        }));
    }

    // handle_startendtag default: calls handle_starttag + handle_endtag (CPython behavior)
    ns.insert(CompactString::from("handle_startendtag"), make_builtin(|args: &[PyObjectRef]| {
        // Default: just no-op. The real delegation happens in the feed loop
        // where we check for user-override of handle_startendtag and fall back
        // to handle_starttag + handle_endtag if not overridden.
        Ok(PyObject::none())
    }));

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
        let cp = ch as u32;
        let name = unicode_char_name(ch, cp);
        if name.is_empty() {
            if args.len() > 1 {
                return Ok(args[1].clone());
            }
            return Err(PyException::value_error("no such name"));
        }
        Ok(PyObject::str_val(CompactString::from(name)))
    });

    let lookup_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.lookup", args, 1)?;
        let name = args[0].py_to_string().to_uppercase();
        match unicode_lookup_name(&name) {
            Some(ch) => Ok(PyObject::str_val(CompactString::from(ch.to_string().as_str()))),
            None => Err(PyException::key_error(format!("undefined character name '{}'", name))),
        }
    });

    let category_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.category", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        let cat = unicode_category(ch);
        Ok(PyObject::str_val(CompactString::from(cat)))
    });

    let numeric_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.numeric", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        if let Some(d) = ch.to_digit(10) {
            Ok(PyObject::float(d as f64))
        } else if ch == '\u{00BD}' {
            Ok(PyObject::float(0.5))
        } else if ch == '\u{2153}' {
            Ok(PyObject::float(1.0 / 3.0))
        } else if ch == '\u{00BC}' {
            Ok(PyObject::float(0.25))
        } else if ch == '\u{00BE}' {
            Ok(PyObject::float(0.75))
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

    let digit_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.digit", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        if let Some(d) = ch.to_digit(10) {
            Ok(PyObject::int(d as i64))
        } else if args.len() > 1 {
            Ok(args[1].clone())
        } else {
            Err(PyException::value_error("not a digit character"))
        }
    });

    let bidirectional_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.bidirectional", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        let bidi = if ch.is_ascii_alphabetic() {
            "L"
        } else if ch.is_ascii_digit() {
            "EN"
        } else if ch == ' ' || ch == '\t' {
            "WS"
        } else if ch.is_ascii_punctuation() {
            "ON"
        } else if ch.is_ascii_control() {
            "BN"
        } else if ch.is_alphabetic() {
            "L"
        } else {
            "ON"
        };
        Ok(PyObject::str_val(CompactString::from(bidi)))
    });

    let combining_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.combining", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        let ccc = if ('\u{0300}'..='\u{036F}').contains(&ch) {
            // Combining Diacritical Marks — approximate canonical combining class
            match ch {
                '\u{0300}' | '\u{0301}' | '\u{0302}' | '\u{0303}' => 230,
                '\u{0327}' | '\u{0328}' => 202,
                '\u{0338}' => 1,
                _ => 230,
            }
        } else {
            0
        };
        Ok(PyObject::int(ccc))
    });

    let east_asian_width_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.east_asian_width", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        let cp = ch as u32;
        let w = if cp <= 0x007F {
            "Na" // Narrow (ASCII)
        } else if (0x1100..=0x115F).contains(&cp) || (0x2E80..=0x303E).contains(&cp)
            || (0x3040..=0x9FFF).contains(&cp) || (0xAC00..=0xD7AF).contains(&cp)
            || (0xF900..=0xFAFF).contains(&cp) || (0xFE10..=0xFE6F).contains(&cp)
            || (0xFF01..=0xFF60).contains(&cp) || (0xFFE0..=0xFFE6).contains(&cp)
            || (0x20000..=0x2FFFF).contains(&cp) || (0x30000..=0x3FFFF).contains(&cp)
        {
            "W" // Wide
        } else if (0xFF61..=0xFFDC).contains(&cp) || (0xFFE8..=0xFFEE).contains(&cp) {
            "H" // Halfwidth
        } else if (0x0080..=0x00FF).contains(&cp) {
            "N" // Neutral
        } else {
            "N"
        };
        Ok(PyObject::str_val(CompactString::from(w)))
    });

    let mirrored_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.mirrored", args, 1)?;
        let s = args[0].py_to_string();
        let ch = s.chars().next().unwrap_or('\0');
        let m = matches!(ch, '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '\u{00AB}' | '\u{00BB}');
        Ok(PyObject::int(if m { 1 } else { 0 }))
    });

    let normalize_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("unicodedata.normalize", args, 2)?;
        let _form = args[0].py_to_string().to_uppercase();
        let s = args[1].py_to_string();
        // For ASCII-only strings all normalization forms are identity
        // For non-ASCII, apply basic decomposition/composition
        if s.is_ascii() {
            return Ok(args[1].clone());
        }
        // Handle common normalization cases
        match _form.as_str() {
            "NFC" | "NFKC" => {
                // Compose: replace common decomposed sequences
                let result = nfc_compose(&s);
                Ok(PyObject::str_val(CompactString::from(result)))
            }
            "NFD" | "NFKD" => {
                // Decompose: expand composed characters
                let result = nfd_decompose(&s);
                Ok(PyObject::str_val(CompactString::from(result)))
            }
            _ => Ok(args[1].clone()),
        }
    });

    make_module("unicodedata", vec![
        ("name", name_fn),
        ("lookup", lookup_fn),
        ("category", category_fn),
        ("numeric", numeric_fn),
        ("decimal", decimal_fn),
        ("digit", digit_fn),
        ("bidirectional", bidirectional_fn),
        ("combining", combining_fn),
        ("east_asian_width", east_asian_width_fn),
        ("mirrored", mirrored_fn),
        ("normalize", normalize_fn),
        ("unidata_version", PyObject::str_val(CompactString::from("15.0.0"))),
    ])
}

/// Return the Unicode name for a character, or empty string if unknown.
fn unicode_char_name(ch: char, cp: u32) -> String {
    // ASCII letters
    if ch.is_ascii_uppercase() {
        return format!("LATIN CAPITAL LETTER {}", ch);
    }
    if ch.is_ascii_lowercase() {
        return format!("LATIN SMALL LETTER {}", ch.to_uppercase().next().unwrap_or(ch));
    }
    if ch.is_ascii_digit() {
        let digit_names = ["ZERO", "ONE", "TWO", "THREE", "FOUR", "FIVE", "SIX", "SEVEN", "EIGHT", "NINE"];
        return format!("DIGIT {}", digit_names[(ch as u8 - b'0') as usize]);
    }
    // Common ASCII punctuation and symbols
    match ch {
        ' ' => "SPACE".to_string(),
        '!' => "EXCLAMATION MARK".to_string(),
        '"' => "QUOTATION MARK".to_string(),
        '#' => "NUMBER SIGN".to_string(),
        '$' => "DOLLAR SIGN".to_string(),
        '%' => "PERCENT SIGN".to_string(),
        '&' => "AMPERSAND".to_string(),
        '\'' => "APOSTROPHE".to_string(),
        '(' => "LEFT PARENTHESIS".to_string(),
        ')' => "RIGHT PARENTHESIS".to_string(),
        '*' => "ASTERISK".to_string(),
        '+' => "PLUS SIGN".to_string(),
        ',' => "COMMA".to_string(),
        '-' => "HYPHEN-MINUS".to_string(),
        '.' => "FULL STOP".to_string(),
        '/' => "SOLIDUS".to_string(),
        ':' => "COLON".to_string(),
        ';' => "SEMICOLON".to_string(),
        '<' => "LESS-THAN SIGN".to_string(),
        '=' => "EQUALS SIGN".to_string(),
        '>' => "GREATER-THAN SIGN".to_string(),
        '?' => "QUESTION MARK".to_string(),
        '@' => "COMMERCIAL AT".to_string(),
        '[' => "LEFT SQUARE BRACKET".to_string(),
        '\\' => "REVERSE SOLIDUS".to_string(),
        ']' => "RIGHT SQUARE BRACKET".to_string(),
        '^' => "CIRCUMFLEX ACCENT".to_string(),
        '_' => "LOW LINE".to_string(),
        '`' => "GRAVE ACCENT".to_string(),
        '{' => "LEFT CURLY BRACKET".to_string(),
        '|' => "VERTICAL LINE".to_string(),
        '}' => "RIGHT CURLY BRACKET".to_string(),
        '~' => "TILDE".to_string(),
        '\t' => "CHARACTER TABULATION".to_string(),
        '\n' => "LINE FEED".to_string(),
        '\r' => "CARRIAGE RETURN".to_string(),
        // Common non-ASCII characters
        '\u{00A0}' => "NO-BREAK SPACE".to_string(),
        '\u{00A9}' => "COPYRIGHT SIGN".to_string(),
        '\u{00AE}' => "REGISTERED SIGN".to_string(),
        '\u{00B0}' => "DEGREE SIGN".to_string(),
        '\u{00B1}' => "PLUS-MINUS SIGN".to_string(),
        '\u{00B5}' => "MICRO SIGN".to_string(),
        '\u{00B7}' => "MIDDLE DOT".to_string(),
        '\u{00BC}' => "VULGAR FRACTION ONE QUARTER".to_string(),
        '\u{00BD}' => "VULGAR FRACTION ONE HALF".to_string(),
        '\u{00BE}' => "VULGAR FRACTION THREE QUARTERS".to_string(),
        '\u{00BF}' => "INVERTED QUESTION MARK".to_string(),
        '\u{00D7}' => "MULTIPLICATION SIGN".to_string(),
        '\u{00F7}' => "DIVISION SIGN".to_string(),
        // Latin Extended-A common
        '\u{0100}' => "LATIN CAPITAL LETTER A WITH MACRON".to_string(),
        '\u{0101}' => "LATIN SMALL LETTER A WITH MACRON".to_string(),
        // Greek letters
        '\u{0391}' => "GREEK CAPITAL LETTER ALPHA".to_string(),
        '\u{0392}' => "GREEK CAPITAL LETTER BETA".to_string(),
        '\u{0393}' => "GREEK CAPITAL LETTER GAMMA".to_string(),
        '\u{0394}' => "GREEK CAPITAL LETTER DELTA".to_string(),
        '\u{0395}' => "GREEK CAPITAL LETTER EPSILON".to_string(),
        '\u{0396}' => "GREEK CAPITAL LETTER ZETA".to_string(),
        '\u{0397}' => "GREEK CAPITAL LETTER ETA".to_string(),
        '\u{0398}' => "GREEK CAPITAL LETTER THETA".to_string(),
        '\u{0399}' => "GREEK CAPITAL LETTER IOTA".to_string(),
        '\u{039A}' => "GREEK CAPITAL LETTER KAPPA".to_string(),
        '\u{039B}' => "GREEK CAPITAL LETTER LAMDA".to_string(),
        '\u{039C}' => "GREEK CAPITAL LETTER MU".to_string(),
        '\u{039D}' => "GREEK CAPITAL LETTER NU".to_string(),
        '\u{039E}' => "GREEK CAPITAL LETTER XI".to_string(),
        '\u{039F}' => "GREEK CAPITAL LETTER OMICRON".to_string(),
        '\u{03A0}' => "GREEK CAPITAL LETTER PI".to_string(),
        '\u{03A1}' => "GREEK CAPITAL LETTER RHO".to_string(),
        '\u{03A3}' => "GREEK CAPITAL LETTER SIGMA".to_string(),
        '\u{03A4}' => "GREEK CAPITAL LETTER TAU".to_string(),
        '\u{03A5}' => "GREEK CAPITAL LETTER UPSILON".to_string(),
        '\u{03A6}' => "GREEK CAPITAL LETTER PHI".to_string(),
        '\u{03A7}' => "GREEK CAPITAL LETTER CHI".to_string(),
        '\u{03A8}' => "GREEK CAPITAL LETTER PSI".to_string(),
        '\u{03A9}' => "GREEK CAPITAL LETTER OMEGA".to_string(),
        '\u{03B1}' => "GREEK SMALL LETTER ALPHA".to_string(),
        '\u{03B2}' => "GREEK SMALL LETTER BETA".to_string(),
        '\u{03B3}' => "GREEK SMALL LETTER GAMMA".to_string(),
        '\u{03B4}' => "GREEK SMALL LETTER DELTA".to_string(),
        '\u{03B5}' => "GREEK SMALL LETTER EPSILON".to_string(),
        '\u{03B6}' => "GREEK SMALL LETTER ZETA".to_string(),
        '\u{03B7}' => "GREEK SMALL LETTER ETA".to_string(),
        '\u{03B8}' => "GREEK SMALL LETTER THETA".to_string(),
        '\u{03B9}' => "GREEK SMALL LETTER IOTA".to_string(),
        '\u{03BA}' => "GREEK SMALL LETTER KAPPA".to_string(),
        '\u{03BB}' => "GREEK SMALL LETTER LAMDA".to_string(),
        '\u{03BC}' => "GREEK SMALL LETTER MU".to_string(),
        '\u{03BD}' => "GREEK SMALL LETTER NU".to_string(),
        '\u{03BE}' => "GREEK SMALL LETTER XI".to_string(),
        '\u{03BF}' => "GREEK SMALL LETTER OMICRON".to_string(),
        '\u{03C0}' => "GREEK SMALL LETTER PI".to_string(),
        '\u{03C1}' => "GREEK SMALL LETTER RHO".to_string(),
        '\u{03C3}' => "GREEK SMALL LETTER SIGMA".to_string(),
        '\u{03C4}' => "GREEK SMALL LETTER TAU".to_string(),
        '\u{03C5}' => "GREEK SMALL LETTER UPSILON".to_string(),
        '\u{03C6}' => "GREEK SMALL LETTER PHI".to_string(),
        '\u{03C7}' => "GREEK SMALL LETTER CHI".to_string(),
        '\u{03C8}' => "GREEK SMALL LETTER PSI".to_string(),
        '\u{03C9}' => "GREEK SMALL LETTER OMEGA".to_string(),
        // Common symbols
        '\u{2013}' => "EN DASH".to_string(),
        '\u{2014}' => "EM DASH".to_string(),
        '\u{2018}' => "LEFT SINGLE QUOTATION MARK".to_string(),
        '\u{2019}' => "RIGHT SINGLE QUOTATION MARK".to_string(),
        '\u{201C}' => "LEFT DOUBLE QUOTATION MARK".to_string(),
        '\u{201D}' => "RIGHT DOUBLE QUOTATION MARK".to_string(),
        '\u{2022}' => "BULLET".to_string(),
        '\u{2026}' => "HORIZONTAL ELLIPSIS".to_string(),
        '\u{2030}' => "PER MILLE SIGN".to_string(),
        '\u{2032}' => "PRIME".to_string(),
        '\u{2033}' => "DOUBLE PRIME".to_string(),
        '\u{20AC}' => "EURO SIGN".to_string(),
        '\u{2122}' => "TRADE MARK SIGN".to_string(),
        '\u{2190}' => "LEFTWARDS ARROW".to_string(),
        '\u{2191}' => "UPWARDS ARROW".to_string(),
        '\u{2192}' => "RIGHTWARDS ARROW".to_string(),
        '\u{2193}' => "DOWNWARDS ARROW".to_string(),
        '\u{2200}' => "FOR ALL".to_string(),
        '\u{2202}' => "PARTIAL DIFFERENTIAL".to_string(),
        '\u{2203}' => "THERE EXISTS".to_string(),
        '\u{2205}' => "EMPTY SET".to_string(),
        '\u{2207}' => "NABLA".to_string(),
        '\u{2208}' => "ELEMENT OF".to_string(),
        '\u{2211}' => "N-ARY SUMMATION".to_string(),
        '\u{221A}' => "SQUARE ROOT".to_string(),
        '\u{221E}' => "INFINITY".to_string(),
        '\u{2227}' => "LOGICAL AND".to_string(),
        '\u{2228}' => "LOGICAL OR".to_string(),
        '\u{2229}' => "INTERSECTION".to_string(),
        '\u{222A}' => "UNION".to_string(),
        '\u{222B}' => "INTEGRAL".to_string(),
        '\u{2248}' => "ALMOST EQUAL TO".to_string(),
        '\u{2260}' => "NOT EQUAL TO".to_string(),
        '\u{2264}' => "LESS-THAN OR EQUAL TO".to_string(),
        '\u{2265}' => "GREATER-THAN OR EQUAL TO".to_string(),
        // CJK common
        '\u{3000}' => "IDEOGRAPHIC SPACE".to_string(),
        '\u{3001}' => "IDEOGRAPHIC COMMA".to_string(),
        '\u{3002}' => "IDEOGRAPHIC FULL STOP".to_string(),
        // Latin-1 supplement letters
        '\u{00C0}' => "LATIN CAPITAL LETTER A WITH GRAVE".to_string(),
        '\u{00C1}' => "LATIN CAPITAL LETTER A WITH ACUTE".to_string(),
        '\u{00C2}' => "LATIN CAPITAL LETTER A WITH CIRCUMFLEX".to_string(),
        '\u{00C3}' => "LATIN CAPITAL LETTER A WITH TILDE".to_string(),
        '\u{00C4}' => "LATIN CAPITAL LETTER A WITH DIAERESIS".to_string(),
        '\u{00C5}' => "LATIN CAPITAL LETTER A WITH RING ABOVE".to_string(),
        '\u{00C6}' => "LATIN CAPITAL LETTER AE".to_string(),
        '\u{00C7}' => "LATIN CAPITAL LETTER C WITH CEDILLA".to_string(),
        '\u{00C8}' => "LATIN CAPITAL LETTER E WITH GRAVE".to_string(),
        '\u{00C9}' => "LATIN CAPITAL LETTER E WITH ACUTE".to_string(),
        '\u{00CA}' => "LATIN CAPITAL LETTER E WITH CIRCUMFLEX".to_string(),
        '\u{00CB}' => "LATIN CAPITAL LETTER E WITH DIAERESIS".to_string(),
        '\u{00CC}' => "LATIN CAPITAL LETTER I WITH GRAVE".to_string(),
        '\u{00CD}' => "LATIN CAPITAL LETTER I WITH ACUTE".to_string(),
        '\u{00CE}' => "LATIN CAPITAL LETTER I WITH CIRCUMFLEX".to_string(),
        '\u{00CF}' => "LATIN CAPITAL LETTER I WITH DIAERESIS".to_string(),
        '\u{00D0}' => "LATIN CAPITAL LETTER ETH".to_string(),
        '\u{00D1}' => "LATIN CAPITAL LETTER N WITH TILDE".to_string(),
        '\u{00D2}' => "LATIN CAPITAL LETTER O WITH GRAVE".to_string(),
        '\u{00D3}' => "LATIN CAPITAL LETTER O WITH ACUTE".to_string(),
        '\u{00D4}' => "LATIN CAPITAL LETTER O WITH CIRCUMFLEX".to_string(),
        '\u{00D5}' => "LATIN CAPITAL LETTER O WITH TILDE".to_string(),
        '\u{00D6}' => "LATIN CAPITAL LETTER O WITH DIAERESIS".to_string(),
        '\u{00D8}' => "LATIN CAPITAL LETTER O WITH STROKE".to_string(),
        '\u{00D9}' => "LATIN CAPITAL LETTER U WITH GRAVE".to_string(),
        '\u{00DA}' => "LATIN CAPITAL LETTER U WITH ACUTE".to_string(),
        '\u{00DB}' => "LATIN CAPITAL LETTER U WITH CIRCUMFLEX".to_string(),
        '\u{00DC}' => "LATIN CAPITAL LETTER U WITH DIAERESIS".to_string(),
        '\u{00DD}' => "LATIN CAPITAL LETTER Y WITH ACUTE".to_string(),
        '\u{00DE}' => "LATIN CAPITAL LETTER THORN".to_string(),
        '\u{00DF}' => "LATIN SMALL LETTER SHARP S".to_string(),
        '\u{00E0}' => "LATIN SMALL LETTER A WITH GRAVE".to_string(),
        '\u{00E1}' => "LATIN SMALL LETTER A WITH ACUTE".to_string(),
        '\u{00E2}' => "LATIN SMALL LETTER A WITH CIRCUMFLEX".to_string(),
        '\u{00E3}' => "LATIN SMALL LETTER A WITH TILDE".to_string(),
        '\u{00E4}' => "LATIN SMALL LETTER A WITH DIAERESIS".to_string(),
        '\u{00E5}' => "LATIN SMALL LETTER A WITH RING ABOVE".to_string(),
        '\u{00E6}' => "LATIN SMALL LETTER AE".to_string(),
        '\u{00E7}' => "LATIN SMALL LETTER C WITH CEDILLA".to_string(),
        '\u{00E8}' => "LATIN SMALL LETTER E WITH GRAVE".to_string(),
        '\u{00E9}' => "LATIN SMALL LETTER E WITH ACUTE".to_string(),
        '\u{00EA}' => "LATIN SMALL LETTER E WITH CIRCUMFLEX".to_string(),
        '\u{00EB}' => "LATIN SMALL LETTER E WITH DIAERESIS".to_string(),
        '\u{00EC}' => "LATIN SMALL LETTER I WITH GRAVE".to_string(),
        '\u{00ED}' => "LATIN SMALL LETTER I WITH ACUTE".to_string(),
        '\u{00EE}' => "LATIN SMALL LETTER I WITH CIRCUMFLEX".to_string(),
        '\u{00EF}' => "LATIN SMALL LETTER I WITH DIAERESIS".to_string(),
        '\u{00F0}' => "LATIN SMALL LETTER ETH".to_string(),
        '\u{00F1}' => "LATIN SMALL LETTER N WITH TILDE".to_string(),
        '\u{00F2}' => "LATIN SMALL LETTER O WITH GRAVE".to_string(),
        '\u{00F3}' => "LATIN SMALL LETTER O WITH ACUTE".to_string(),
        '\u{00F4}' => "LATIN SMALL LETTER O WITH CIRCUMFLEX".to_string(),
        '\u{00F5}' => "LATIN SMALL LETTER O WITH TILDE".to_string(),
        '\u{00F6}' => "LATIN SMALL LETTER O WITH DIAERESIS".to_string(),
        '\u{00F8}' => "LATIN SMALL LETTER O WITH STROKE".to_string(),
        '\u{00F9}' => "LATIN SMALL LETTER U WITH GRAVE".to_string(),
        '\u{00FA}' => "LATIN SMALL LETTER U WITH ACUTE".to_string(),
        '\u{00FB}' => "LATIN SMALL LETTER U WITH CIRCUMFLEX".to_string(),
        '\u{00FC}' => "LATIN SMALL LETTER U WITH DIAERESIS".to_string(),
        '\u{00FD}' => "LATIN SMALL LETTER Y WITH ACUTE".to_string(),
        '\u{00FE}' => "LATIN SMALL LETTER THORN".to_string(),
        '\u{00FF}' => "LATIN SMALL LETTER Y WITH DIAERESIS".to_string(),
        _ => {
            // Control characters have no name in the database
            if cp < 0x20 || (0x7F..=0x9F).contains(&cp) {
                return String::new();
            }
            // For unrecognized characters, return empty to trigger default/error
            String::new()
        }
    }
}

/// Reverse-lookup: given a Unicode name, return the character.
fn unicode_lookup_name(name: &str) -> Option<char> {
    // Build reverse map from the name function for common chars
    // ASCII letters
    if name.starts_with("LATIN CAPITAL LETTER ") {
        let rest = name.strip_prefix("LATIN CAPITAL LETTER ")?;
        // Handle "X WITH Y" patterns (accented letters)
        return match rest {
            "A" => Some('A'), "B" => Some('B'), "C" => Some('C'), "D" => Some('D'),
            "E" => Some('E'), "F" => Some('F'), "G" => Some('G'), "H" => Some('H'),
            "I" => Some('I'), "J" => Some('J'), "K" => Some('K'), "L" => Some('L'),
            "M" => Some('M'), "N" => Some('N'), "O" => Some('O'), "P" => Some('P'),
            "Q" => Some('Q'), "R" => Some('R'), "S" => Some('S'), "T" => Some('T'),
            "U" => Some('U'), "V" => Some('V'), "W" => Some('W'), "X" => Some('X'),
            "Y" => Some('Y'), "Z" => Some('Z'),
            "A WITH GRAVE" => Some('\u{00C0}'), "A WITH ACUTE" => Some('\u{00C1}'),
            "A WITH CIRCUMFLEX" => Some('\u{00C2}'), "A WITH TILDE" => Some('\u{00C3}'),
            "A WITH DIAERESIS" => Some('\u{00C4}'), "A WITH RING ABOVE" => Some('\u{00C5}'),
            "AE" => Some('\u{00C6}'),
            "C WITH CEDILLA" => Some('\u{00C7}'),
            "E WITH GRAVE" => Some('\u{00C8}'), "E WITH ACUTE" => Some('\u{00C9}'),
            "E WITH CIRCUMFLEX" => Some('\u{00CA}'), "E WITH DIAERESIS" => Some('\u{00CB}'),
            "I WITH GRAVE" => Some('\u{00CC}'), "I WITH ACUTE" => Some('\u{00CD}'),
            "I WITH CIRCUMFLEX" => Some('\u{00CE}'), "I WITH DIAERESIS" => Some('\u{00CF}'),
            "ETH" => Some('\u{00D0}'),
            "N WITH TILDE" => Some('\u{00D1}'),
            "O WITH GRAVE" => Some('\u{00D2}'), "O WITH ACUTE" => Some('\u{00D3}'),
            "O WITH CIRCUMFLEX" => Some('\u{00D4}'), "O WITH TILDE" => Some('\u{00D5}'),
            "O WITH DIAERESIS" => Some('\u{00D6}'), "O WITH STROKE" => Some('\u{00D8}'),
            "U WITH GRAVE" => Some('\u{00D9}'), "U WITH ACUTE" => Some('\u{00DA}'),
            "U WITH CIRCUMFLEX" => Some('\u{00DB}'), "U WITH DIAERESIS" => Some('\u{00DC}'),
            "Y WITH ACUTE" => Some('\u{00DD}'),
            "THORN" => Some('\u{00DE}'),
            "A WITH MACRON" => Some('\u{0100}'),
            _ => None,
        };
    }
    if name.starts_with("LATIN SMALL LETTER ") {
        let rest = name.strip_prefix("LATIN SMALL LETTER ")?;
        return match rest {
            "A" => Some('a'), "B" => Some('b'), "C" => Some('c'), "D" => Some('d'),
            "E" => Some('e'), "F" => Some('f'), "G" => Some('g'), "H" => Some('h'),
            "I" => Some('i'), "J" => Some('j'), "K" => Some('k'), "L" => Some('l'),
            "M" => Some('m'), "N" => Some('n'), "O" => Some('o'), "P" => Some('p'),
            "Q" => Some('q'), "R" => Some('r'), "S" => Some('s'), "T" => Some('t'),
            "U" => Some('u'), "V" => Some('v'), "W" => Some('w'), "X" => Some('x'),
            "Y" => Some('y'), "Z" => Some('z'),
            "SHARP S" => Some('\u{00DF}'),
            "A WITH GRAVE" => Some('\u{00E0}'), "A WITH ACUTE" => Some('\u{00E1}'),
            "A WITH CIRCUMFLEX" => Some('\u{00E2}'), "A WITH TILDE" => Some('\u{00E3}'),
            "A WITH DIAERESIS" => Some('\u{00E4}'), "A WITH RING ABOVE" => Some('\u{00E5}'),
            "AE" => Some('\u{00E6}'),
            "C WITH CEDILLA" => Some('\u{00E7}'),
            "E WITH GRAVE" => Some('\u{00E8}'), "E WITH ACUTE" => Some('\u{00E9}'),
            "E WITH CIRCUMFLEX" => Some('\u{00EA}'), "E WITH DIAERESIS" => Some('\u{00EB}'),
            "I WITH GRAVE" => Some('\u{00EC}'), "I WITH ACUTE" => Some('\u{00ED}'),
            "I WITH CIRCUMFLEX" => Some('\u{00EE}'), "I WITH DIAERESIS" => Some('\u{00EF}'),
            "ETH" => Some('\u{00F0}'),
            "N WITH TILDE" => Some('\u{00F1}'),
            "O WITH GRAVE" => Some('\u{00F2}'), "O WITH ACUTE" => Some('\u{00F3}'),
            "O WITH CIRCUMFLEX" => Some('\u{00F4}'), "O WITH TILDE" => Some('\u{00F5}'),
            "O WITH DIAERESIS" => Some('\u{00F6}'), "O WITH STROKE" => Some('\u{00F8}'),
            "U WITH GRAVE" => Some('\u{00F9}'), "U WITH ACUTE" => Some('\u{00FA}'),
            "U WITH CIRCUMFLEX" => Some('\u{00FB}'), "U WITH DIAERESIS" => Some('\u{00FC}'),
            "Y WITH ACUTE" => Some('\u{00FD}'),
            "THORN" => Some('\u{00FE}'),
            "Y WITH DIAERESIS" => Some('\u{00FF}'),
            "A WITH MACRON" => Some('\u{0101}'),
            _ => None,
        };
    }
    // Digit names
    if name.starts_with("DIGIT ") {
        let rest = name.strip_prefix("DIGIT ")?;
        return match rest {
            "ZERO" => Some('0'), "ONE" => Some('1'), "TWO" => Some('2'),
            "THREE" => Some('3'), "FOUR" => Some('4'), "FIVE" => Some('5'),
            "SIX" => Some('6'), "SEVEN" => Some('7'), "EIGHT" => Some('8'),
            "NINE" => Some('9'),
            _ => rest.chars().next().filter(|c| c.is_ascii_digit()),
        };
    }
    // Greek letters
    if name.starts_with("GREEK CAPITAL LETTER ") {
        let rest = name.strip_prefix("GREEK CAPITAL LETTER ")?;
        return match rest {
            "ALPHA" => Some('\u{0391}'), "BETA" => Some('\u{0392}'),
            "GAMMA" => Some('\u{0393}'), "DELTA" => Some('\u{0394}'),
            "EPSILON" => Some('\u{0395}'), "ZETA" => Some('\u{0396}'),
            "ETA" => Some('\u{0397}'), "THETA" => Some('\u{0398}'),
            "IOTA" => Some('\u{0399}'), "KAPPA" => Some('\u{039A}'),
            "LAMDA" => Some('\u{039B}'), "MU" => Some('\u{039C}'),
            "NU" => Some('\u{039D}'), "XI" => Some('\u{039E}'),
            "OMICRON" => Some('\u{039F}'), "PI" => Some('\u{03A0}'),
            "RHO" => Some('\u{03A1}'), "SIGMA" => Some('\u{03A3}'),
            "TAU" => Some('\u{03A4}'), "UPSILON" => Some('\u{03A5}'),
            "PHI" => Some('\u{03A6}'), "CHI" => Some('\u{03A7}'),
            "PSI" => Some('\u{03A8}'), "OMEGA" => Some('\u{03A9}'),
            _ => None,
        };
    }
    if name.starts_with("GREEK SMALL LETTER ") {
        let rest = name.strip_prefix("GREEK SMALL LETTER ")?;
        return match rest {
            "ALPHA" => Some('\u{03B1}'), "BETA" => Some('\u{03B2}'),
            "GAMMA" => Some('\u{03B3}'), "DELTA" => Some('\u{03B4}'),
            "EPSILON" => Some('\u{03B5}'), "ZETA" => Some('\u{03B6}'),
            "ETA" => Some('\u{03B7}'), "THETA" => Some('\u{03B8}'),
            "IOTA" => Some('\u{03B9}'), "KAPPA" => Some('\u{03BA}'),
            "LAMDA" => Some('\u{03BB}'), "MU" => Some('\u{03BC}'),
            "NU" => Some('\u{03BD}'), "XI" => Some('\u{03BE}'),
            "OMICRON" => Some('\u{03BF}'), "PI" => Some('\u{03C0}'),
            "RHO" => Some('\u{03C1}'), "SIGMA" => Some('\u{03C3}'),
            "TAU" => Some('\u{03C4}'), "UPSILON" => Some('\u{03C5}'),
            "PHI" => Some('\u{03C6}'), "CHI" => Some('\u{03C7}'),
            "PSI" => Some('\u{03C8}'), "OMEGA" => Some('\u{03C9}'),
            _ => None,
        };
    }
    // Direct matches for symbols and punctuation
    match name {
        "SPACE" => Some(' '),
        "EXCLAMATION MARK" => Some('!'),
        "QUOTATION MARK" => Some('"'),
        "NUMBER SIGN" => Some('#'),
        "DOLLAR SIGN" => Some('$'),
        "PERCENT SIGN" => Some('%'),
        "AMPERSAND" => Some('&'),
        "APOSTROPHE" => Some('\''),
        "LEFT PARENTHESIS" => Some('('),
        "RIGHT PARENTHESIS" => Some(')'),
        "ASTERISK" => Some('*'),
        "PLUS SIGN" => Some('+'),
        "COMMA" => Some(','),
        "HYPHEN-MINUS" => Some('-'),
        "FULL STOP" => Some('.'),
        "SOLIDUS" => Some('/'),
        "COLON" => Some(':'),
        "SEMICOLON" => Some(';'),
        "LESS-THAN SIGN" => Some('<'),
        "EQUALS SIGN" => Some('='),
        "GREATER-THAN SIGN" => Some('>'),
        "QUESTION MARK" => Some('?'),
        "COMMERCIAL AT" => Some('@'),
        "LEFT SQUARE BRACKET" => Some('['),
        "REVERSE SOLIDUS" => Some('\\'),
        "RIGHT SQUARE BRACKET" => Some(']'),
        "CIRCUMFLEX ACCENT" => Some('^'),
        "LOW LINE" => Some('_'),
        "GRAVE ACCENT" => Some('`'),
        "LEFT CURLY BRACKET" => Some('{'),
        "VERTICAL LINE" => Some('|'),
        "RIGHT CURLY BRACKET" => Some('}'),
        "TILDE" => Some('~'),
        "NO-BREAK SPACE" => Some('\u{00A0}'),
        "COPYRIGHT SIGN" => Some('\u{00A9}'),
        "REGISTERED SIGN" => Some('\u{00AE}'),
        "DEGREE SIGN" => Some('\u{00B0}'),
        "PLUS-MINUS SIGN" => Some('\u{00B1}'),
        "MICRO SIGN" => Some('\u{00B5}'),
        "MIDDLE DOT" => Some('\u{00B7}'),
        "VULGAR FRACTION ONE QUARTER" => Some('\u{00BC}'),
        "VULGAR FRACTION ONE HALF" => Some('\u{00BD}'),
        "VULGAR FRACTION THREE QUARTERS" => Some('\u{00BE}'),
        "MULTIPLICATION SIGN" => Some('\u{00D7}'),
        "DIVISION SIGN" => Some('\u{00F7}'),
        "EN DASH" => Some('\u{2013}'),
        "EM DASH" => Some('\u{2014}'),
        "LEFT SINGLE QUOTATION MARK" => Some('\u{2018}'),
        "RIGHT SINGLE QUOTATION MARK" => Some('\u{2019}'),
        "LEFT DOUBLE QUOTATION MARK" => Some('\u{201C}'),
        "RIGHT DOUBLE QUOTATION MARK" => Some('\u{201D}'),
        "BULLET" => Some('\u{2022}'),
        "HORIZONTAL ELLIPSIS" => Some('\u{2026}'),
        "EURO SIGN" => Some('\u{20AC}'),
        "TRADE MARK SIGN" => Some('\u{2122}'),
        "INFINITY" => Some('\u{221E}'),
        "SQUARE ROOT" => Some('\u{221A}'),
        "NOT EQUAL TO" => Some('\u{2260}'),
        "LESS-THAN OR EQUAL TO" => Some('\u{2264}'),
        "GREATER-THAN OR EQUAL TO" => Some('\u{2265}'),
        "SNOWMAN" => Some('\u{2603}'),
        "LATIN CAPITAL LETTER AE" => Some('\u{00C6}'),
        "LATIN SMALL LETTER AE" => Some('\u{00E6}'),
        _ => None,
    }
}

/// Return the Unicode General Category for a character.
fn unicode_category(ch: char) -> &'static str {
    let cp = ch as u32;
    // Control characters
    if cp <= 0x1F || (0x7F..=0x9F).contains(&cp) {
        return "Cc";
    }
    // ASCII and Latin-1 fast paths
    if ch.is_ascii_uppercase() { return "Lu"; }
    if ch.is_ascii_lowercase() { return "Ll"; }
    if ch.is_ascii_digit() { return "Nd"; }
    // Specific ASCII punctuation subcategories
    match ch {
        ' ' => return "Zs",
        '\u{00A0}' | '\u{2000}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => return "Zs",
        '\u{2028}' => return "Zl",
        '\u{2029}' => return "Zp",
        '(' | '[' | '{' => return "Ps",
        ')' | ']' | '}' => return "Pe",
        '_' => return "Pc",
        '-' | '\u{2010}'..='\u{2015}' => return "Pd",
        '$' | '\u{00A2}'..='\u{00A5}' | '\u{20AC}' => return "Sc",
        '+' | '<' | '=' | '>' | '|' | '~' | '^' | '\u{00AC}' | '\u{00B1}' => return "Sm",
        '#' | '%' | '&' | '*' | '\\' | '@' | '\u{00A7}' | '\u{00B0}' | '\u{00B6}' | '\u{00A9}' | '\u{00AE}' => return "So",
        '!' | '"' | '\'' | ',' | '.' | '/' | ':' | ';' | '?' | '\u{00A1}' | '\u{00BF}' | '\u{00B7}' => return "Po",
        '`' => return "Sk",
        _ => {}
    }
    // Combining marks
    if ('\u{0300}'..='\u{036F}').contains(&ch)
        || ('\u{0483}'..='\u{0489}').contains(&ch)
        || ('\u{0591}'..='\u{05BD}').contains(&ch)
        || ('\u{0610}'..='\u{061A}').contains(&ch)
        || ('\u{064B}'..='\u{065F}').contains(&ch)
        || ('\u{0900}'..='\u{0903}').contains(&ch)
        || ('\u{093A}'..='\u{094F}').contains(&ch)
        || ('\u{20D0}'..='\u{20FF}').contains(&ch)
    {
        return "Mn";
    }
    // Format characters
    if ch == '\u{00AD}' || ('\u{200B}'..='\u{200F}').contains(&ch)
        || ('\u{202A}'..='\u{202E}').contains(&ch)
        || ('\u{2060}'..='\u{2064}').contains(&ch)
        || ch == '\u{FEFF}'
    {
        return "Cf";
    }
    // Surrogates
    if (0xD800..=0xDFFF).contains(&cp) {
        return "Cs";
    }
    // Private use
    if (0xE000..=0xF8FF).contains(&cp) || (0xF0000..=0xFFFFF).contains(&cp)
        || (0x100000..=0x10FFFF).contains(&cp)
    {
        return "Co";
    }
    // Numbers (beyond ASCII digits)
    if ch.is_numeric() { return "Nd"; }
    // Letters
    if ch.is_uppercase() { return "Lu"; }
    if ch.is_lowercase() { return "Ll"; }
    // Titlecase letters
    if ('\u{01C5}'..='\u{01C5}').contains(&ch)
        || ('\u{01C8}'..='\u{01C8}').contains(&ch)
        || ('\u{01CB}'..='\u{01CB}').contains(&ch)
        || ch == '\u{01F2}'
    {
        return "Lt";
    }
    // Modifier letters
    if ('\u{02B0}'..='\u{02FF}').contains(&ch) {
        return "Lm";
    }
    if ch.is_alphabetic() { return "Lo"; }
    // Default
    "Cn"
}

/// Basic NFC composition for common precomposed characters.
fn nfc_compose(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() {
            if let Some(composed) = compose_pair(chars[i], chars[i + 1]) {
                result.push(composed);
                i += 2;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

/// Basic NFD decomposition for common precomposed characters.
fn nfd_decompose(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 2);
    for ch in s.chars() {
        if let Some((base, combining)) = decompose_char(ch) {
            result.push(base);
            result.push(combining);
        } else {
            result.push(ch);
        }
    }
    result
}

fn compose_pair(base: char, combining: char) -> Option<char> {
    match (base, combining) {
        ('A', '\u{0300}') => Some('\u{00C0}'), ('A', '\u{0301}') => Some('\u{00C1}'),
        ('A', '\u{0302}') => Some('\u{00C2}'), ('A', '\u{0303}') => Some('\u{00C3}'),
        ('A', '\u{0308}') => Some('\u{00C4}'), ('A', '\u{030A}') => Some('\u{00C5}'),
        ('C', '\u{0327}') => Some('\u{00C7}'),
        ('E', '\u{0300}') => Some('\u{00C8}'), ('E', '\u{0301}') => Some('\u{00C9}'),
        ('E', '\u{0302}') => Some('\u{00CA}'), ('E', '\u{0308}') => Some('\u{00CB}'),
        ('I', '\u{0300}') => Some('\u{00CC}'), ('I', '\u{0301}') => Some('\u{00CD}'),
        ('I', '\u{0302}') => Some('\u{00CE}'), ('I', '\u{0308}') => Some('\u{00CF}'),
        ('N', '\u{0303}') => Some('\u{00D1}'),
        ('O', '\u{0300}') => Some('\u{00D2}'), ('O', '\u{0301}') => Some('\u{00D3}'),
        ('O', '\u{0302}') => Some('\u{00D4}'), ('O', '\u{0303}') => Some('\u{00D5}'),
        ('O', '\u{0308}') => Some('\u{00D6}'),
        ('U', '\u{0300}') => Some('\u{00D9}'), ('U', '\u{0301}') => Some('\u{00DA}'),
        ('U', '\u{0302}') => Some('\u{00DB}'), ('U', '\u{0308}') => Some('\u{00DC}'),
        ('Y', '\u{0301}') => Some('\u{00DD}'),
        ('a', '\u{0300}') => Some('\u{00E0}'), ('a', '\u{0301}') => Some('\u{00E1}'),
        ('a', '\u{0302}') => Some('\u{00E2}'), ('a', '\u{0303}') => Some('\u{00E3}'),
        ('a', '\u{0308}') => Some('\u{00E4}'), ('a', '\u{030A}') => Some('\u{00E5}'),
        ('c', '\u{0327}') => Some('\u{00E7}'),
        ('e', '\u{0300}') => Some('\u{00E8}'), ('e', '\u{0301}') => Some('\u{00E9}'),
        ('e', '\u{0302}') => Some('\u{00EA}'), ('e', '\u{0308}') => Some('\u{00EB}'),
        ('i', '\u{0300}') => Some('\u{00EC}'), ('i', '\u{0301}') => Some('\u{00ED}'),
        ('i', '\u{0302}') => Some('\u{00EE}'), ('i', '\u{0308}') => Some('\u{00EF}'),
        ('n', '\u{0303}') => Some('\u{00F1}'),
        ('o', '\u{0300}') => Some('\u{00F2}'), ('o', '\u{0301}') => Some('\u{00F3}'),
        ('o', '\u{0302}') => Some('\u{00F4}'), ('o', '\u{0303}') => Some('\u{00F5}'),
        ('o', '\u{0308}') => Some('\u{00F6}'),
        ('u', '\u{0300}') => Some('\u{00F9}'), ('u', '\u{0301}') => Some('\u{00FA}'),
        ('u', '\u{0302}') => Some('\u{00FB}'), ('u', '\u{0308}') => Some('\u{00FC}'),
        ('y', '\u{0301}') => Some('\u{00FD}'), ('y', '\u{0308}') => Some('\u{00FF}'),
        _ => None,
    }
}

fn decompose_char(ch: char) -> Option<(char, char)> {
    match ch {
        '\u{00C0}' => Some(('A', '\u{0300}')), '\u{00C1}' => Some(('A', '\u{0301}')),
        '\u{00C2}' => Some(('A', '\u{0302}')), '\u{00C3}' => Some(('A', '\u{0303}')),
        '\u{00C4}' => Some(('A', '\u{0308}')), '\u{00C5}' => Some(('A', '\u{030A}')),
        '\u{00C7}' => Some(('C', '\u{0327}')),
        '\u{00C8}' => Some(('E', '\u{0300}')), '\u{00C9}' => Some(('E', '\u{0301}')),
        '\u{00CA}' => Some(('E', '\u{0302}')), '\u{00CB}' => Some(('E', '\u{0308}')),
        '\u{00CC}' => Some(('I', '\u{0300}')), '\u{00CD}' => Some(('I', '\u{0301}')),
        '\u{00CE}' => Some(('I', '\u{0302}')), '\u{00CF}' => Some(('I', '\u{0308}')),
        '\u{00D1}' => Some(('N', '\u{0303}')),
        '\u{00D2}' => Some(('O', '\u{0300}')), '\u{00D3}' => Some(('O', '\u{0301}')),
        '\u{00D4}' => Some(('O', '\u{0302}')), '\u{00D5}' => Some(('O', '\u{0303}')),
        '\u{00D6}' => Some(('O', '\u{0308}')),
        '\u{00D9}' => Some(('U', '\u{0300}')), '\u{00DA}' => Some(('U', '\u{0301}')),
        '\u{00DB}' => Some(('U', '\u{0302}')), '\u{00DC}' => Some(('U', '\u{0308}')),
        '\u{00DD}' => Some(('Y', '\u{0301}')),
        '\u{00E0}' => Some(('a', '\u{0300}')), '\u{00E1}' => Some(('a', '\u{0301}')),
        '\u{00E2}' => Some(('a', '\u{0302}')), '\u{00E3}' => Some(('a', '\u{0303}')),
        '\u{00E4}' => Some(('a', '\u{0308}')), '\u{00E5}' => Some(('a', '\u{030A}')),
        '\u{00E7}' => Some(('c', '\u{0327}')),
        '\u{00E8}' => Some(('e', '\u{0300}')), '\u{00E9}' => Some(('e', '\u{0301}')),
        '\u{00EA}' => Some(('e', '\u{0302}')), '\u{00EB}' => Some(('e', '\u{0308}')),
        '\u{00EC}' => Some(('i', '\u{0300}')), '\u{00ED}' => Some(('i', '\u{0301}')),
        '\u{00EE}' => Some(('i', '\u{0302}')), '\u{00EF}' => Some(('i', '\u{0308}')),
        '\u{00F1}' => Some(('n', '\u{0303}')),
        '\u{00F2}' => Some(('o', '\u{0300}')), '\u{00F3}' => Some(('o', '\u{0301}')),
        '\u{00F4}' => Some(('o', '\u{0302}')), '\u{00F5}' => Some(('o', '\u{0303}')),
        '\u{00F6}' => Some(('o', '\u{0308}')),
        '\u{00F9}' => Some(('u', '\u{0300}')), '\u{00FA}' => Some(('u', '\u{0301}')),
        '\u{00FB}' => Some(('u', '\u{0302}')), '\u{00FC}' => Some(('u', '\u{0308}')),
        '\u{00FD}' => Some(('y', '\u{0301}')), '\u{00FF}' => Some(('y', '\u{0308}')),
        _ => None,
    }
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

/// Check if an object is "readable" by Python's eval (i.e., its repr can be round-tripped).
/// Objects with custom classes, functions, or non-standard types are not readable.
fn pprint_is_readable_impl(obj: &PyObjectRef, seen: &mut Vec<usize>) -> bool {
    let id = Arc::as_ptr(obj) as usize;
    if seen.contains(&id) {
        return false; // circular reference
    }
    match &obj.payload {
        PyObjectPayload::None | PyObjectPayload::Bool(_) | PyObjectPayload::Int(_)
        | PyObjectPayload::Float(_) | PyObjectPayload::Str(_) | PyObjectPayload::Bytes(_) => true,
        PyObjectPayload::List(items) => {
            seen.push(id);
            let r = items.read();
            let result = r.iter().all(|v| pprint_is_readable_impl(v, seen));
            seen.pop();
            result
        }
        PyObjectPayload::Tuple(items) => {
            seen.push(id);
            let result = items.iter().all(|v| pprint_is_readable_impl(v, seen));
            seen.pop();
            result
        }
        PyObjectPayload::Dict(map) => {
            seen.push(id);
            let r = map.read();
            let result = r.values().all(|v| pprint_is_readable_impl(v, seen));
            seen.pop();
            result
        }
        PyObjectPayload::Set(items) => {
            seen.push(id);
            let _ = items.read();
            seen.pop();
            true
        }
        PyObjectPayload::FrozenSet(_) => true,
        _ => false,
    }
}

/// Check if an object contains circular references.
fn pprint_is_recursive_impl(obj: &PyObjectRef, seen: &mut Vec<usize>) -> bool {
    let id = Arc::as_ptr(obj) as usize;
    if seen.contains(&id) {
        return true; // found circular reference
    }
    match &obj.payload {
        PyObjectPayload::List(items) => {
            seen.push(id);
            let r = items.read();
            let result = r.iter().any(|v| pprint_is_recursive_impl(v, seen));
            seen.pop();
            result
        }
        PyObjectPayload::Tuple(items) => {
            seen.push(id);
            let result = items.iter().any(|v| pprint_is_recursive_impl(v, seen));
            seen.pop();
            result
        }
        PyObjectPayload::Dict(map) => {
            seen.push(id);
            let r = map.read();
            let result = r.values().any(|v| pprint_is_recursive_impl(v, seen));
            seen.pop();
            result
        }
        _ => false,
    }
}

fn pprint_isreadable(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Ok(PyObject::bool_val(true)); }
    let mut seen = Vec::new();
    Ok(PyObject::bool_val(pprint_is_readable_impl(&args[0], &mut seen)))
}

fn pprint_isrecursive(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Ok(PyObject::bool_val(false)); }
    let mut seen = Vec::new();
    Ok(PyObject::bool_val(pprint_is_recursive_impl(&args[0], &mut seen)))
}

fn pprint_saferepr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
    let obj = &args[0];
    let repr = match &obj.payload {
        PyObjectPayload::Str(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
        PyObjectPayload::Bytes(b) => {
            let mut r = String::from("b'");
            for &byte in b {
                if byte == b'\\' { r.push_str("\\\\"); }
                else if byte == b'\'' { r.push_str("\\'"); }
                else if byte >= 0x20 && byte < 0x7F { r.push(byte as char); }
                else { r.push_str(&format!("\\x{:02x}", byte)); }
            }
            r.push('\'');
            r
        }
        _ => pformat_value(obj, 1, 80, None, 0),
    };
    Ok(PyObject::str_val(CompactString::from(repr)))
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
        ("PrettyPrinter", PyObject::native_closure("PrettyPrinter", |args: &[PyObjectRef]| {
            // Parse keyword args: indent=1, width=80, depth=None, stream=None
            let mut indent = 1usize;
            let mut width = 80usize;
            let mut depth: Option<usize> = None;
            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(kw) = &last.payload {
                    let r = kw.read();
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
            let cls = PyObject::class(CompactString::from("PrettyPrinter"), vec![], indexmap::IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut attrs = d.attrs.write();
                attrs.insert(CompactString::from("_indent"), PyObject::int(indent as i64));
                attrs.insert(CompactString::from("_width"), PyObject::int(width as i64));
                let pp_indent = indent;
                let pp_width = width;
                let pp_depth = depth;
                attrs.insert(CompactString::from("pprint"), PyObject::native_closure("pprint", move |args: &[PyObjectRef]| {
                    if args.is_empty() { return Ok(PyObject::none()); }
                    let text = pformat_value(&args[0], pp_indent, pp_width, pp_depth, 0);
                    println!("{}", text);
                    Ok(PyObject::none())
                }));
                let pf_indent = indent;
                let pf_width = width;
                let pf_depth = depth;
                attrs.insert(CompactString::from("pformat"), PyObject::native_closure("pformat", move |args: &[PyObjectRef]| {
                    if args.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
                    let text = pformat_value(&args[0], pf_indent, pf_width, pf_depth, 0);
                    Ok(PyObject::str_val(CompactString::from(text)))
                }));
                attrs.insert(CompactString::from("isreadable"), make_builtin(pprint_isreadable));
                attrs.insert(CompactString::from("isrecursive"), make_builtin(pprint_isrecursive));
            }
            Ok(inst)
        })),
        ("isreadable", make_builtin(pprint_isreadable)),
        ("isrecursive", make_builtin(pprint_isrecursive)),
        ("saferepr", make_builtin(pprint_saferepr)),
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

pub fn create_encodings_aliases_module() -> PyObjectRef {
    let mut aliases = IndexMap::new();
    let alias_pairs = [
        ("646", "ascii"), ("ansi_x3.4_1968", "ascii"), ("ansi_x3_4_1968", "ascii"),
        ("ascii", "ascii"), ("cp367", "ascii"), ("csascii", "ascii"), ("ibm367", "ascii"),
        ("iso646_us", "ascii"), ("iso_646.irv_1991", "ascii"), ("iso_ir_6", "ascii"), ("us", "ascii"), ("us_ascii", "ascii"),
        ("utf_8", "utf_8"), ("utf8", "utf_8"), ("utf", "utf_8"), ("cp65001", "utf_8"),
        ("utf_8_sig", "utf_8_sig"),
        ("latin_1", "iso8859_1"), ("latin1", "iso8859_1"), ("iso_8859_1", "iso8859_1"),
        ("iso8859_1", "iso8859_1"), ("8859", "iso8859_1"), ("cp819", "iso8859_1"),
        ("iso_8859_1_1987", "iso8859_1"), ("l1", "iso8859_1"),
        ("utf_16", "utf_16"), ("utf16", "utf_16"),
        ("utf_16_le", "utf_16_le"), ("utf_16_be", "utf_16_be"),
        ("utf_32", "utf_32"), ("utf_32_le", "utf_32_le"), ("utf_32_be", "utf_32_be"),
        ("cp1252", "cp1252"), ("windows_1252", "cp1252"),
        ("cp437", "cp437"), ("ibm437", "cp437"),
        ("shift_jis", "shift_jis"), ("shiftjis", "shift_jis"), ("csshiftjis", "shift_jis"),
        ("euc_jp", "euc_jp"), ("eucjp", "euc_jp"),
        ("euc_kr", "euc_kr"), ("euckr", "euc_kr"),
        ("gb2312", "gb2312"), ("gbk", "gbk"), ("gb18030", "gb18030"),
        ("big5", "big5"), ("big5hkscs", "big5hkscs"),
        ("cp949", "cp949"), ("uhc", "cp949"),
        ("iso8859_2", "iso8859_2"), ("latin2", "iso8859_2"), ("l2", "iso8859_2"),
        ("iso8859_15", "iso8859_15"), ("latin9", "iso8859_15"),
        ("koi8_r", "koi8_r"), ("koi8_u", "koi8_u"),
        ("mac_roman", "mac_roman"), ("macintosh", "mac_roman"),
        ("idna", "idna"),
    ];
    for (alias, codec) in &alias_pairs {
        aliases.insert(
            HashableKey::Str(CompactString::from(*alias)),
            PyObject::str_val(CompactString::from(*codec)),
        );
    }
    make_module("encodings.aliases", vec![
        ("aliases", PyObject::dict(aliases)),
    ])
}

pub fn create_encodings_idna_module() -> PyObjectRef {
    make_module("encodings.idna", vec![
        ("name", PyObject::str_val(CompactString::from("idna"))),
        ("encode", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("encode() requires input")); }
            let s = args[0].py_to_string();
            // Simple IDNA encoding: just lowercase ASCII
            let encoded = s.to_ascii_lowercase();
            Ok(PyObject::tuple(vec![
                PyObject::bytes(encoded.into_bytes()),
                PyObject::int(s.len() as i64),
            ]))
        })),
        ("decode", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("decode() requires input")); }
            let s = args[0].py_to_string();
            Ok(PyObject::tuple(vec![
                PyObject::str_val(CompactString::from(&s)),
                PyObject::int(s.len() as i64),
            ]))
        })),
        ("IncrementalEncoder", make_builtin(|_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        })),
        ("IncrementalDecoder", make_builtin(|_args: &[PyObjectRef]| {
            Ok(PyObject::none())
        })),
    ])
}

pub fn create_multibytecodec_module() -> PyObjectRef {
    let mb_inc_decoder = PyObject::class(
        CompactString::from("MultibyteIncrementalDecoder"),
        vec![],
        IndexMap::new(),
    );
    if let PyObjectPayload::Class(ref cd) = mb_inc_decoder.payload {
        cd.namespace.write().insert(
            CompactString::from("__init__"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() { return Err(PyException::type_error("requires self")); }
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    let errors = if args.len() > 1 { args[1].py_to_string() } else { "strict".to_string() };
                    inst.attrs.write().insert(CompactString::from("errors"), PyObject::str_val(CompactString::from(errors)));
                }
                Ok(PyObject::none())
            }),
        );
        cd.namespace.write().insert(
            CompactString::from("decode"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.len() < 2 { return Err(PyException::type_error("decode() requires input")); }
                let input = args[1].py_to_string();
                Ok(PyObject::str_val(CompactString::from(input)))
            }),
        );
        cd.namespace.write().insert(CompactString::from("reset"), make_builtin(|_| Ok(PyObject::none())));
    }

    let mb_inc_encoder = PyObject::class(
        CompactString::from("MultibyteIncrementalEncoder"),
        vec![],
        IndexMap::new(),
    );
    if let PyObjectPayload::Class(ref cd) = mb_inc_encoder.payload {
        cd.namespace.write().insert(
            CompactString::from("__init__"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() { return Err(PyException::type_error("requires self")); }
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    let errors = if args.len() > 1 { args[1].py_to_string() } else { "strict".to_string() };
                    inst.attrs.write().insert(CompactString::from("errors"), PyObject::str_val(CompactString::from(errors)));
                }
                Ok(PyObject::none())
            }),
        );
        cd.namespace.write().insert(
            CompactString::from("encode"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.len() < 2 { return Err(PyException::type_error("encode() requires input")); }
                let input = args[1].py_to_string();
                Ok(PyObject::bytes(input.into_bytes()))
            }),
        );
        cd.namespace.write().insert(CompactString::from("reset"), make_builtin(|_| Ok(PyObject::none())));
    }

    let mb_stream_reader = PyObject::class(CompactString::from("MultibyteStreamReader"), vec![], IndexMap::new());
    let mb_stream_writer = PyObject::class(CompactString::from("MultibyteStreamWriter"), vec![], IndexMap::new());

    make_module("_multibytecodec", vec![
        ("MultibyteIncrementalDecoder", mb_inc_decoder),
        ("MultibyteIncrementalEncoder", mb_inc_encoder),
        ("MultibyteStreamReader", mb_stream_reader),
        ("MultibyteStreamWriter", mb_stream_writer),
        ("__create_codec", make_builtin(|_| Ok(PyObject::none()))),
    ])
}

/// Generic encodings.* codec submodule — provides IncrementalDecoder/Encoder classes
/// that handle encode/decode via the codecs module infrastructure.
pub fn create_encodings_codec_module(module_name: &str) -> PyObjectRef {
    let codec_name = module_name.strip_prefix("encodings.").unwrap_or(module_name);
    let codec_name_cs = CompactString::from(codec_name);

    // IncrementalDecoder class for this encoding
    let inc_decoder = PyObject::class(CompactString::from("IncrementalDecoder"), vec![], IndexMap::new());
    if let PyObjectPayload::Class(ref cd) = inc_decoder.payload {
        let cn = codec_name_cs.clone();
        cd.namespace.write().insert(
            CompactString::from("__init__"),
            PyObject::native_closure("IncrementalDecoder.__init__", move |args: &[PyObjectRef]| {
                if args.is_empty() { return Err(PyException::type_error("requires self")); }
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    let errors = if args.len() > 1 { args[1].py_to_string() } else { "strict".to_string() };
                    inst.attrs.write().insert(CompactString::from("errors"), PyObject::str_val(CompactString::from(errors)));
                    inst.attrs.write().insert(CompactString::from("_encoding"), PyObject::str_val(cn.clone()));
                }
                Ok(PyObject::none())
            }),
        );
        cd.namespace.write().insert(
            CompactString::from("decode"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.len() < 2 { return Err(PyException::type_error("decode() requires input")); }
                // Simple passthrough for UTF-8 compatible encodings
                let input = args[1].py_to_string();
                Ok(PyObject::str_val(CompactString::from(input)))
            }),
        );
        cd.namespace.write().insert(CompactString::from("reset"), make_builtin(|_| Ok(PyObject::none())));
        cd.namespace.write().insert(CompactString::from("getstate"), make_builtin(|_| Ok(PyObject::tuple(vec![PyObject::bytes(vec![]), PyObject::int(0)]))));
        cd.namespace.write().insert(CompactString::from("setstate"), make_builtin(|_| Ok(PyObject::none())));
    }

    // IncrementalEncoder class for this encoding
    let inc_encoder = PyObject::class(CompactString::from("IncrementalEncoder"), vec![], IndexMap::new());
    if let PyObjectPayload::Class(ref cd) = inc_encoder.payload {
        let cn = codec_name_cs.clone();
        cd.namespace.write().insert(
            CompactString::from("__init__"),
            PyObject::native_closure("IncrementalEncoder.__init__", move |args: &[PyObjectRef]| {
                if args.is_empty() { return Err(PyException::type_error("requires self")); }
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    let errors = if args.len() > 1 { args[1].py_to_string() } else { "strict".to_string() };
                    inst.attrs.write().insert(CompactString::from("errors"), PyObject::str_val(CompactString::from(errors)));
                    inst.attrs.write().insert(CompactString::from("_encoding"), PyObject::str_val(cn.clone()));
                }
                Ok(PyObject::none())
            }),
        );
        cd.namespace.write().insert(
            CompactString::from("encode"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.len() < 2 { return Err(PyException::type_error("encode() requires input")); }
                let input = args[1].py_to_string();
                Ok(PyObject::bytes(input.into_bytes()))
            }),
        );
        cd.namespace.write().insert(CompactString::from("reset"), make_builtin(|_| Ok(PyObject::none())));
        cd.namespace.write().insert(CompactString::from("getstate"), make_builtin(|_| Ok(PyObject::int(0))));
        cd.namespace.write().insert(CompactString::from("setstate"), make_builtin(|_| Ok(PyObject::none())));
    }

    // getregentry() — returns a CodecInfo-like tuple
    let cn_entry = CompactString::from(codec_name);
    let getregentry = PyObject::native_closure("getregentry", move |_args: &[PyObjectRef]| {
        Ok(PyObject::tuple(vec![
            PyObject::str_val(cn_entry.clone()),
            PyObject::none(), // encode fn
            PyObject::none(), // decode fn
            PyObject::none(), // stream_reader
            PyObject::none(), // stream_writer
        ]))
    });

    make_module(module_name, vec![
        ("IncrementalDecoder", inc_decoder),
        ("IncrementalEncoder", inc_encoder),
        ("getregentry", getregentry),
        ("name", PyObject::str_val(CompactString::from(codec_name))),
    ])
}
