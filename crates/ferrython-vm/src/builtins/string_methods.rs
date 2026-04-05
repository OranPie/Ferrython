//! String method dispatch (upper, lower, split, replace, strip, join, find, format, etc.)

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    check_args_min,
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, PyInt};
use indexmap::IndexMap;

use super::apply_format_spec_str;

/// Extract a named kwarg from a trailing Dict argument (if present).
fn extract_kwarg(args: &[PyObjectRef], name: &str) -> Option<PyObjectRef> {
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(d) = &last.payload {
            let d = d.read();
            let key = HashableKey::Str(CompactString::from(name));
            return d.get(&key).cloned();
        }
    }
    None
}

/// Return positional args (everything except a trailing kwargs Dict).
fn positional_args(args: &[PyObjectRef]) -> &[PyObjectRef] {
    if let Some(last) = args.last() {
        if matches!(&last.payload, PyObjectPayload::Dict(_)) {
            return &args[..args.len() - 1];
        }
    }
    args
}

fn normalize_index(idx: i64, len: i64) -> usize {
    if idx < 0 {
        (len + idx).max(0) as usize
    } else {
        (idx as usize).min(len as usize)
    }
}

/// Resolve nested `{N}` references in a format spec.
/// E.g., `{1}>{2}` with args=['hi', '*', 10] → `*>10`
fn resolve_nested_spec(spec: &str, args: &[PyObjectRef]) -> String {
    let mut result = String::new();
    let mut chars = spec.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut ref_name = String::new();
            for c in chars.by_ref() {
                if c == '}' { break; }
                ref_name.push(c);
            }
            if let Ok(idx) = ref_name.parse::<usize>() {
                if let Some(val) = args.get(idx) {
                    result.push_str(&val.py_to_string());
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

pub(super) fn call_str_method(s: &str, method: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match method {
        "upper" => Ok(PyObject::str_val(CompactString::from(s.to_uppercase()))),
        "lower" => Ok(PyObject::str_val(CompactString::from(s.to_lowercase()))),
        "strip" => {
            if !args.is_empty() {
                if let Some(chars) = args[0].as_str() {
                    let ch: Vec<char> = chars.chars().collect();
                    let trimmed = s.trim_matches(|c: char| ch.contains(&c));
                    return Ok(PyObject::str_val(CompactString::from(trimmed)));
                }
            }
            Ok(PyObject::str_val(CompactString::from(s.trim())))
        }
        "lstrip" => {
            if !args.is_empty() {
                if let Some(chars) = args[0].as_str() {
                    let ch: Vec<char> = chars.chars().collect();
                    let trimmed = s.trim_start_matches(|c: char| ch.contains(&c));
                    return Ok(PyObject::str_val(CompactString::from(trimmed)));
                }
            }
            Ok(PyObject::str_val(CompactString::from(s.trim_start())))
        }
        "rstrip" => {
            if !args.is_empty() {
                if let Some(chars) = args[0].as_str() {
                    let ch: Vec<char> = chars.chars().collect();
                    let trimmed = s.trim_end_matches(|c: char| ch.contains(&c));
                    return Ok(PyObject::str_val(CompactString::from(trimmed)));
                }
            }
            Ok(PyObject::str_val(CompactString::from(s.trim_end())))
        }
        "split" => {
            let pos = positional_args(args);
            let maxsplit: Option<usize> = if pos.len() > 1 {
                pos[1].as_int().map(|n| n as usize)
            } else {
                extract_kwarg(args, "maxsplit").and_then(|v| v.as_int().map(|n| n as usize))
            };
            let sep_arg = pos.first();
            let parts: Vec<&str> = match sep_arg {
                None => match maxsplit {
                    Some(n) => s.splitn(n + 1, char::is_whitespace).filter(|p| !p.is_empty()).collect(),
                    None => s.split_whitespace().collect(),
                },
                Some(a) if matches!(&a.payload, PyObjectPayload::None) => match maxsplit {
                    Some(n) => s.splitn(n + 1, char::is_whitespace).filter(|p| !p.is_empty()).collect(),
                    None => s.split_whitespace().collect(),
                },
                Some(a) => {
                    let sep = a.as_str().ok_or_else(|| PyException::type_error("split() argument must be str or None"))?;
                    match maxsplit {
                        Some(n) => s.splitn(n + 1, sep).collect(),
                        None => s.split(sep).collect(),
                    }
                }
            };
            Ok(PyObject::list(parts.iter().map(|p| PyObject::str_val(CompactString::from(*p))).collect()))
        }
        "rsplit" => {
            let pos = positional_args(args);
            let maxsplit: Option<usize> = if pos.len() > 1 {
                pos[1].as_int().map(|n| n as usize)
            } else {
                extract_kwarg(args, "maxsplit").and_then(|v| v.as_int().map(|n| n as usize))
            };
            let sep_arg = pos.first();
            let is_none_sep = sep_arg.is_none() || sep_arg.map_or(false, |a| matches!(&a.payload, PyObjectPayload::None));
            if is_none_sep {
                let words: Vec<&str> = s.split_whitespace().collect();
                let result = match maxsplit {
                    Some(n) if n < words.len() => {
                        // rejoin leftmost words, then keep the last n
                        let boundary = words.len() - n;
                        let mut out: Vec<String> = Vec::with_capacity(n + 1);
                        out.push(words[..boundary].join(" "));
                        for w in &words[boundary..] { out.push((*w).to_string()); }
                        out
                    }
                    _ => words.iter().map(|w| w.to_string()).collect(),
                };
                Ok(PyObject::list(result.iter().map(|p| PyObject::str_val(CompactString::from(p.as_str()))).collect()))
            } else {
                let sep = sep_arg.unwrap().as_str().ok_or_else(|| PyException::type_error("rsplit() argument must be str or None"))?;
                let mut parts: Vec<&str> = match maxsplit {
                    Some(n) => s.rsplitn(n + 1, sep).collect(),
                    None => s.rsplit(sep).collect(),
                };
                parts.reverse();
                Ok(PyObject::list(parts.iter().map(|p| PyObject::str_val(CompactString::from(*p))).collect()))
            }
        }
        "join" => {
            check_args_min("join", args, 1)?;
            let items = args[0].to_list()?;
            let strs: Result<Vec<String>, _> = items.iter()
                .map(|x| x.as_str().map(String::from).ok_or_else(||
                    PyException::type_error("sequence item: expected str")))
                .collect();
            Ok(PyObject::str_val(CompactString::from(strs?.join(s))))
        }
        "replace" => {
            check_args_min("replace", args, 2)?;
            let old = args[0].as_str().ok_or_else(|| PyException::type_error("replace() argument 1 must be str"))?;
            let new = args[1].as_str().ok_or_else(|| PyException::type_error("replace() argument 2 must be str"))?;
            if args.len() >= 3 {
                let count = args[2].to_int()? as usize;
                Ok(PyObject::str_val(CompactString::from(s.replacen(old, new, count))))
            } else {
                Ok(PyObject::str_val(CompactString::from(s.replace(old, new))))
            }
        }
        "find" => {
            check_args_min("find", args, 1)?;
            let sub = args[0].as_str().ok_or_else(|| PyException::type_error("find() argument must be str"))?;
            let start = if args.len() >= 2 { args[1].as_int().unwrap_or(0).max(0) as usize } else { 0 };
            let end = if args.len() >= 3 { args[2].as_int().unwrap_or(s.len() as i64).max(0) as usize } else { s.len() };
            let search_area = &s[start.min(s.len())..end.min(s.len())];
            Ok(PyObject::int(search_area.find(sub).map(|i| (i + start) as i64).unwrap_or(-1)))
        }
        "rfind" => {
            check_args_min("rfind", args, 1)?;
            let sub = args[0].as_str().ok_or_else(|| PyException::type_error("rfind() argument must be str"))?;
            let start = if args.len() >= 2 { args[1].as_int().unwrap_or(0).max(0) as usize } else { 0 };
            let end = if args.len() >= 3 { args[2].as_int().unwrap_or(s.len() as i64).max(0) as usize } else { s.len() };
            let search_area = &s[start.min(s.len())..end.min(s.len())];
            Ok(PyObject::int(search_area.rfind(sub).map(|i| (i + start) as i64).unwrap_or(-1)))
        }
        "index" => {
            check_args_min("index", args, 1)?;
            let sub = args[0].as_str().ok_or_else(|| PyException::type_error("index() argument must be str"))?;
            match s.find(sub) {
                Some(i) => Ok(PyObject::int(i as i64)),
                None => Err(PyException::value_error("substring not found")),
            }
        }
        "count" => {
            check_args_min("count", args, 1)?;
            let sub = args[0].as_str().ok_or_else(|| PyException::type_error("count() argument must be str"))?;
            let len = s.chars().count() as i64;
            let start = if args.len() >= 2 { normalize_index(args[1].as_int().unwrap_or(0), len) } else { 0usize };
            let end = if args.len() >= 3 { normalize_index(args[2].as_int().unwrap_or(len), len) } else { len as usize };
            let slice: String = s.chars().skip(start).take(end.saturating_sub(start)).collect();
            Ok(PyObject::int(slice.matches(sub).count() as i64))
        }
        "startswith" => {
            check_args_min("startswith", args, 1)?;
            match &args[0].payload {
                PyObjectPayload::Tuple(prefixes) => {
                    let result = prefixes.iter().any(|p| {
                        p.as_str().map(|ps| s.starts_with(ps)).unwrap_or(false)
                    });
                    Ok(PyObject::bool_val(result))
                }
                _ => {
                    let prefix = args[0].as_str().ok_or_else(|| PyException::type_error("startswith() argument must be str or tuple"))?;
                    Ok(PyObject::bool_val(s.starts_with(prefix)))
                }
            }
        }
        "endswith" => {
            check_args_min("endswith", args, 1)?;
            match &args[0].payload {
                PyObjectPayload::Tuple(suffixes) => {
                    let result = suffixes.iter().any(|p| {
                        p.as_str().map(|ps| s.ends_with(ps)).unwrap_or(false)
                    });
                    Ok(PyObject::bool_val(result))
                }
                _ => {
                    let suffix = args[0].as_str().ok_or_else(|| PyException::type_error("endswith() argument must be str or tuple"))?;
                    Ok(PyObject::bool_val(s.ends_with(suffix)))
                }
            }
        }
        "isdigit" => Ok(PyObject::bool_val(!s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))),
        "isalpha" => Ok(PyObject::bool_val(!s.is_empty() && s.chars().all(|c| c.is_alphabetic()))),
        "isalnum" => Ok(PyObject::bool_val(!s.is_empty() && s.chars().all(|c| c.is_alphanumeric()))),
        "isspace" => Ok(PyObject::bool_val(!s.is_empty() && s.chars().all(|c| c.is_whitespace()))),
        "isupper" => Ok(PyObject::bool_val(s.chars().any(|c| c.is_uppercase()) && !s.chars().any(|c| c.is_lowercase()))),
        "islower" => Ok(PyObject::bool_val(s.chars().any(|c| c.is_lowercase()) && !s.chars().any(|c| c.is_uppercase()))),
        "title" => {
            let mut result = String::with_capacity(s.len());
            let mut prev_alpha = false;
            for c in s.chars() {
                if c.is_alphabetic() {
                    if prev_alpha { result.extend(c.to_lowercase()); }
                    else { result.extend(c.to_uppercase()); }
                    prev_alpha = true;
                } else {
                    result.push(c);
                    prev_alpha = false;
                }
            }
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "capitalize" => {
            let mut chars = s.chars();
            let result = match chars.next() {
                None => String::new(),
                Some(c) => {
                    let mut r = c.to_uppercase().to_string();
                    for c in chars { r.extend(c.to_lowercase()); }
                    r
                }
            };
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "swapcase" => {
            let result: String = s.chars().map(|c| {
                if c.is_uppercase() { c.to_lowercase().to_string() }
                else if c.is_lowercase() { c.to_uppercase().to_string() }
                else { c.to_string() }
            }).collect();
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "center" => {
            check_args_min("center", args, 1)?;
            let width = args[0].to_int()? as usize;
            let fillchar = if args.len() >= 2 {
                args[1].as_str().and_then(|s| s.chars().next()).unwrap_or(' ')
            } else { ' ' };
            let len = s.chars().count();
            if width <= len { return Ok(PyObject::str_val(CompactString::from(s))); }
            let pad = width - len;
            let right = pad / 2;
            let left = pad - right;
            let result = format!("{}{}{}", fillchar.to_string().repeat(left), s, fillchar.to_string().repeat(right));
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "ljust" => {
            check_args_min("ljust", args, 1)?;
            let width = args[0].to_int()? as usize;
            let fillchar = if args.len() >= 2 {
                args[1].as_str().and_then(|s| s.chars().next()).unwrap_or(' ')
            } else { ' ' };
            let len = s.chars().count();
            if width <= len { return Ok(PyObject::str_val(CompactString::from(s))); }
            let result = format!("{}{}", s, fillchar.to_string().repeat(width - len));
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "rjust" => {
            check_args_min("rjust", args, 1)?;
            let width = args[0].to_int()? as usize;
            let fillchar = if args.len() >= 2 {
                args[1].as_str().and_then(|s| s.chars().next()).unwrap_or(' ')
            } else { ' ' };
            let len = s.chars().count();
            if width <= len { return Ok(PyObject::str_val(CompactString::from(s))); }
            let result = format!("{}{}", fillchar.to_string().repeat(width - len), s);
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "zfill" => {
            check_args_min("zfill", args, 1)?;
            let width = args[0].to_int()? as usize;
            let len = s.len();
            if width <= len { return Ok(PyObject::str_val(CompactString::from(s))); }
            let pad = "0".repeat(width - len);
            if s.starts_with('-') || s.starts_with('+') {
                Ok(PyObject::str_val(CompactString::from(format!("{}{}{}", &s[..1], pad, &s[1..]))))
            } else {
                Ok(PyObject::str_val(CompactString::from(format!("{}{}", pad, s))))
            }
        }
        "expandtabs" => {
            let tabsize = if args.is_empty() { 8 } else { args[0].to_int()? as usize };
            let mut result = String::new();
            let mut col = 0usize;
            for ch in s.chars() {
                if ch == '\t' {
                    let spaces = if tabsize == 0 { 0 } else { tabsize - (col % tabsize) };
                    result.extend(std::iter::repeat(' ').take(spaces));
                    col += spaces;
                } else if ch == '\n' || ch == '\r' {
                    result.push(ch);
                    col = 0;
                } else {
                    result.push(ch);
                    col += 1;
                }
            }
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "encode" => {
            let encoding = if !args.is_empty() {
                args[0].py_to_string().to_lowercase()
            } else {
                "utf-8".to_string()
            };
            let errors = if args.len() > 1 {
                args[1].py_to_string()
            } else {
                "strict".to_string()
            };
            match encoding.as_str() {
                "utf-8" | "utf8" => {
                    Ok(PyObject::bytes(s.as_bytes().to_vec()))
                }
                "ascii" => {
                    let mut result = Vec::new();
                    for ch in s.chars() {
                        if ch.is_ascii() {
                            result.push(ch as u8);
                        } else {
                            match errors.as_str() {
                                "ignore" => {}
                                "replace" => result.push(b'?'),
                                "xmlcharrefreplace" => {
                                    result.extend_from_slice(format!("&#{};", ch as u32).as_bytes());
                                }
                                _ => {
                                    return Err(PyException::new(
                                        ExceptionKind::UnicodeEncodeError,
                                        format!(
                                            "'ascii' codec can't encode character '\\u{:04x}' in position", ch as u32
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                    Ok(PyObject::bytes(result))
                }
                "latin-1" | "latin1" | "iso-8859-1" | "iso8859-1" => {
                    let mut result = Vec::new();
                    for ch in s.chars() {
                        if (ch as u32) <= 0xFF {
                            result.push(ch as u8);
                        } else {
                            match errors.as_str() {
                                "ignore" => {}
                                "replace" => result.push(b'?'),
                                _ => {
                                    return Err(PyException::new(
                                        ExceptionKind::UnicodeEncodeError,
                                        format!(
                                            "'latin-1' codec can't encode character '\\u{:04x}'", ch as u32
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                    Ok(PyObject::bytes(result))
                }
                _ => {
                    Err(PyException::value_error(format!("unknown encoding: {}", encoding)))
                }
            }
        }
        "partition" => {
            check_args_min("partition", args, 1)?;
            let sep = args[0].py_to_string();
            if let Some(idx) = s.find(&sep) {
                Ok(PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from(&s[..idx])),
                    PyObject::str_val(CompactString::from(&sep)),
                    PyObject::str_val(CompactString::from(&s[idx + sep.len()..])),
                ]))
            } else {
                Ok(PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from(s)),
                    PyObject::str_val(CompactString::from("")),
                    PyObject::str_val(CompactString::from("")),
                ]))
            }
        }
        "rpartition" => {
            check_args_min("rpartition", args, 1)?;
            let sep = args[0].py_to_string();
            if let Some(idx) = s.rfind(&sep) {
                Ok(PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from(&s[..idx])),
                    PyObject::str_val(CompactString::from(&sep)),
                    PyObject::str_val(CompactString::from(&s[idx + sep.len()..])),
                ]))
            } else {
                Ok(PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from("")),
                    PyObject::str_val(CompactString::from("")),
                    PyObject::str_val(CompactString::from(s)),
                ]))
            }
        }
        "casefold" => {
            // casefold: aggressive lowercase for caseless matching
            // Rust's to_lowercase handles most Unicode, but ß → ss needs explicit handling
            let folded: String = s.chars().flat_map(|c| {
                if c == '\u{00DF}' { // ß
                    vec!['s', 's']
                } else {
                    c.to_lowercase().collect::<Vec<_>>()
                }
            }).collect();
            Ok(PyObject::str_val(CompactString::from(folded)))
        }
        "removeprefix" => {
            check_args_min("removeprefix", args, 1)?;
            let prefix = args[0].py_to_string();
            if s.starts_with(&prefix) {
                Ok(PyObject::str_val(CompactString::from(&s[prefix.len()..])))
            } else {
                Ok(PyObject::str_val(CompactString::from(s)))
            }
        }
        "removesuffix" => {
            check_args_min("removesuffix", args, 1)?;
            let suffix = args[0].py_to_string();
            if s.ends_with(&suffix) {
                Ok(PyObject::str_val(CompactString::from(&s[..s.len() - suffix.len()])))
            } else {
                Ok(PyObject::str_val(CompactString::from(s)))
            }
        }
        "splitlines" => {
            let keepends = !args.is_empty() && args[0].is_truthy();
            let mut lines = Vec::new();
            let mut start = 0;
            let bytes = s.as_bytes();
            let len = bytes.len();
            let mut i = 0;
            while i < len {
                if bytes[i] == b'\r' && i + 1 < len && bytes[i + 1] == b'\n' {
                    if keepends { lines.push(PyObject::str_val(CompactString::from(&s[start..i + 2]))); }
                    else { lines.push(PyObject::str_val(CompactString::from(&s[start..i]))); }
                    i += 2; start = i;
                } else if bytes[i] == b'\n' || bytes[i] == b'\r' {
                    if keepends { lines.push(PyObject::str_val(CompactString::from(&s[start..i + 1]))); }
                    else { lines.push(PyObject::str_val(CompactString::from(&s[start..i]))); }
                    i += 1; start = i;
                } else {
                    i += 1;
                }
            }
            if start < len {
                lines.push(PyObject::str_val(CompactString::from(&s[start..])));
            }
            Ok(PyObject::list(lines))
        }
        "istitle" => {
            let mut prev_cased = false;
            let mut is_title = false;
            for c in s.chars() {
                if c.is_uppercase() {
                    if prev_cased { return Ok(PyObject::bool_val(false)); }
                    prev_cased = true;
                    is_title = true;
                } else if c.is_lowercase() {
                    if !prev_cased { return Ok(PyObject::bool_val(false)); }
                    prev_cased = true;
                } else {
                    prev_cased = false;
                }
            }
            Ok(PyObject::bool_val(is_title))
        }
        "isprintable" => {
            Ok(PyObject::bool_val(!s.is_empty() && s.chars().all(|c| !c.is_control() || c == ' ')))
        }
        "isidentifier" => {
            let mut chars = s.chars();
            let valid = match chars.next() {
                Some(c) if c == '_' || c.is_alphabetic() => chars.all(|c| c == '_' || c.is_alphanumeric()),
                _ => false,
            };
            Ok(PyObject::bool_val(valid))
        }
        "isascii" => {
            Ok(PyObject::bool_val(s.is_ascii()))
        }
        "isdecimal" => {
            Ok(PyObject::bool_val(!s.is_empty() && s.chars().all(|c| c.is_ascii_digit())))
        }
        "isnumeric" => {
            Ok(PyObject::bool_val(!s.is_empty() && s.chars().all(|c| c.is_numeric())))
        }
        "format" => {
            let mut result = String::new();
            let mut chars = s.chars().peekable();
            let mut auto_idx = 0usize;
            while let Some(c) = chars.next() {
                if c == '{' {
                    if chars.peek() == Some(&'{') {
                        chars.next();
                        result.push('{');
                    } else {
                        // Collect everything until matching '}', tracking nested braces
                        let mut field_spec = String::new();
                        let mut depth = 1;
                        for c in chars.by_ref() {
                            if c == '{' { depth += 1; }
                            else if c == '}' {
                                depth -= 1;
                                if depth == 0 { break; }
                            }
                            field_spec.push(c);
                        }
                        // Split field_spec: {field_name!conversion:format_spec}
                        let (field_part, format_spec) = if let Some(colon_pos) = field_spec.find(':') {
                            (&field_spec[..colon_pos], Some(&field_spec[colon_pos+1..]))
                        } else {
                            (field_spec.as_str(), None)
                        };
                        // Resolve nested {N} references in format spec (e.g., {0:{1}>{2}})
                        let resolved_spec = format_spec.map(|spec| {
                            if spec.contains('{') {
                                resolve_nested_spec(spec, args)
                            } else {
                                spec.to_string()
                            }
                        });
                        // Split field_part on '!' for conversion
                        let (field_name, conversion) = if let Some(bang_pos) = field_part.find('!') {
                            (&field_part[..bang_pos], Some(&field_part[bang_pos+1..]))
                        } else {
                            (field_part, None)
                        };
                        // Resolve the value (supports {idx}, {name}, {idx.attr}, {idx[key]})
                        let value = if field_name.is_empty() {
                            let v = args.get(auto_idx).cloned();
                            auto_idx += 1;
                            v
                        } else {
                            // Split on '.' for attribute access: "0.val" → base="0", attrs=["val"]
                            let parts: Vec<&str> = field_name.splitn(2, '.').collect();
                            let base_name = parts[0];
                            let mut val = if let Ok(idx) = base_name.parse::<usize>() {
                                args.get(idx).cloned()
                            } else {
                                None
                            };
                            // Resolve attribute chain if present
                            if parts.len() > 1 {
                                if let Some(ref base_val) = val {
                                    let attr_chain = parts[1];
                                    let mut current = base_val.clone();
                                    for attr in attr_chain.split('.') {
                                        if let Some(v) = current.get_attr(attr) {
                                            current = v;
                                        } else {
                                            current = PyObject::str_val(CompactString::from(""));
                                            break;
                                        }
                                    }
                                    val = Some(current);
                                }
                            }
                            val
                        };
                        if let Some(val) = value {
                            if let Some(conv) = conversion {
                                // Apply conversion first, then format spec on the string
                                let converted = match conv {
                                    "r" => val.repr(),
                                    "s" => val.py_to_string(),
                                    "a" => val.repr(),
                                    _ => val.py_to_string(),
                                };
                                if let Some(ref spec) = resolved_spec {
                                    result.push_str(&apply_format_spec_str(&converted, spec));
                                } else {
                                    result.push_str(&converted);
                                }
                            } else if let Some(ref spec) = resolved_spec {
                                // No conversion — use format_value on the object directly
                                match val.format_value(spec) {
                                    Ok(formatted) => result.push_str(&formatted),
                                    Err(_) => result.push_str(&val.py_to_string()),
                                }
                            } else {
                                result.push_str(&val.py_to_string());
                            }
                        }
                    }
                } else if c == '}' && chars.peek() == Some(&'}') {
                    chars.next();
                    result.push('}');
                } else {
                    result.push(c);
                }
            }
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "translate" => {
            check_args_min("translate", args, 1)?;
            let table = &args[0];
            let mut result = String::new();
            if let PyObjectPayload::Dict(map) = &table.payload {
                let map = map.read();
                for ch in s.chars() {
                    let key = HashableKey::Int(PyInt::Small(ch as i64));
                    match map.get(&key) {
                        Some(val) => {
                            if matches!(&val.payload, PyObjectPayload::None) {
                                // Delete the character
                            } else if let Ok(n) = val.to_int() {
                                if let Some(c) = char::from_u32(n as u32) {
                                    result.push(c);
                                }
                            } else {
                                result.push_str(&val.py_to_string());
                            }
                        }
                        None => result.push(ch),
                    }
                }
            } else {
                result = s.to_string();
            }
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "maketrans" => {
            if args.is_empty() {
                return Err(PyException::type_error("maketrans() requires at least 1 argument"));
            }
            let mut result_map = IndexMap::new();
            if args.len() == 1 {
                if let PyObjectPayload::Dict(map) = &args[0].payload {
                    for (k, v) in map.read().iter() {
                        let key = match k {
                            HashableKey::Int(n) => n.clone(),
                            HashableKey::Str(s) => {
                                if let Some(c) = s.chars().next() { PyInt::Small(c as i64) } else { continue; }
                            }
                            _ => continue,
                        };
                        result_map.insert(HashableKey::Int(key), v.clone());
                    }
                }
            } else {
                let x = args[0].py_to_string();
                let y = args[1].py_to_string();
                for (cx, cy) in x.chars().zip(y.chars()) {
                    result_map.insert(HashableKey::Int(PyInt::Small(cx as i64)), PyObject::int(cy as i64));
                }
                if args.len() > 2 {
                    let z = args[2].py_to_string();
                    for cz in z.chars() {
                        result_map.insert(HashableKey::Int(PyInt::Small(cz as i64)), PyObject::none());
                    }
                }
            }
            Ok(PyObject::dict(result_map))
        }
        "rindex" => {
            check_args_min("rindex", args, 1)?;
            let sub = args[0].as_str().ok_or_else(|| PyException::type_error("rindex() argument must be str"))?;
            match s.rfind(sub) {
                Some(i) => Ok(PyObject::int(i as i64)),
                None => Err(PyException::value_error("substring not found")),
            }
        }
        "format_map" => {
            check_args_min("format_map", args, 1)?;
            let mapping = &args[0];
            let mut result = String::new();
            let mut chars = s.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '{' {
                    if chars.peek() == Some(&'{') {
                        chars.next();
                        result.push('{');
                    } else {
                        let mut field = String::new();
                        for c in chars.by_ref() {
                            if c == '}' { break; }
                            field.push(c);
                        }
                        // Look up field in mapping (dict subscript, not attribute)
                        if let PyObjectPayload::Dict(m) = &mapping.payload {
                            let key = HashableKey::Str(CompactString::from(&field));
                            if let Some(val) = m.read().get(&key) {
                                result.push_str(&val.py_to_string());
                            } else {
                                return Err(PyException::key_error(field));
                            }
                        } else if let Some(val) = mapping.get_attr(&field) {
                            result.push_str(&val.py_to_string());
                        } else {
                            return Err(PyException::key_error(field));
                        }
                    }
                } else if c == '}' && chars.peek() == Some(&'}') {
                    chars.next();
                    result.push('}');
                } else {
                    result.push(c);
                }
            }
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        _ => Err(PyException::attribute_error(format!(
            "'str' object has no attribute '{}'", method
        ))),
    }
}
