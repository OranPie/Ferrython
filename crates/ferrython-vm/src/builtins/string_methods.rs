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

/// Extract a string value from a PyObject, accepting both Str payload and str subclasses.
fn extract_str_value(obj: &PyObjectRef) -> Option<String> {
    match &obj.payload {
        PyObjectPayload::Str(s) => Some(s.to_string()),
        PyObjectPayload::Instance(inst) => {
            inst.attrs.read().get("__builtin_value__")
                .and_then(|bv| match &bv.payload {
                    PyObjectPayload::Str(s) => Some(s.to_string()),
                    _ => None,
                })
        }
        _ => None,
    }
}

/// Extract a named kwarg from a trailing Dict argument (if present).
fn extract_kwarg(args: &[PyObjectRef], name: &str) -> Option<PyObjectRef> {
    if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(d) = &last.payload {
            let d = d.read();
            let key = HashableKey::str_key(CompactString::from(name));
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

/// Resolve a format field like "0", "0.attr", "0[key]", "0.attr[0]", or "name".
/// Supports chained attribute access (`.`) and getitem (`[...]`) in any order.
fn resolve_format_field(field_name: &str, args: &[PyObjectRef]) -> Option<PyObjectRef> {
    // Parse the base name: everything before the first '.' or '['
    let base_end = field_name.find(|c: char| c == '.' || c == '[').unwrap_or(field_name.len());
    let base_name = &field_name[..base_end];
    let rest = &field_name[base_end..];

    // Resolve base value from positional args or kwargs
    let mut current = if let Ok(idx) = base_name.parse::<usize>() {
        args.get(idx)?.clone()
    } else {
        // Named field — look in kwargs (last arg if it's a dict from **kwargs unpacking)
        // For now, named fields aren't supported without kwargs unpacking at call site
        return None;
    };

    // Process accessor chain: .attr and [key] in sequence
    let mut chars = rest.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == '.' {
            chars.next(); // consume '.'
            let mut attr = String::new();
            while let Some(&nc) = chars.peek() {
                if nc == '.' || nc == '[' { break; }
                attr.push(nc);
                chars.next();
            }
            if let Some(v) = current.get_attr(&attr) {
                current = v;
            } else {
                return Some(PyObject::str_val(CompactString::from("")));
            }
        } else if c == '[' {
            chars.next(); // consume '['
            let mut key = String::new();
            for nc in chars.by_ref() {
                if nc == ']' { break; }
                key.push(nc);
            }
            // Try integer index first, then string key
            if let Ok(idx) = key.parse::<i64>() {
                let key_obj = PyObject::int(idx);
                if let Ok(v) = current.get_item(&key_obj) {
                    current = v;
                } else {
                    return Some(PyObject::str_val(CompactString::from("")));
                }
            } else {
                let key_obj = PyObject::str_val(CompactString::from(key));
                if let Ok(v) = current.get_item(&key_obj) {
                    current = v;
                } else {
                    return Some(PyObject::str_val(CompactString::from("")));
                }
            }
        } else {
            break;
        }
    }

    Some(current)
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
            // Build result Vec<PyObjectRef> directly — no intermediate Vec<&str>
            let parts: Vec<PyObjectRef> = match sep_arg {
                None => match maxsplit {
                    Some(n) => s.splitn(n + 1, char::is_whitespace)
                        .filter(|p| !p.is_empty())
                        .map(|p| PyObject::str_val(CompactString::from(p)))
                        .collect(),
                    None => s.split_whitespace()
                        .map(|p| PyObject::str_val(CompactString::from(p)))
                        .collect(),
                },
                Some(a) if matches!(&a.payload, PyObjectPayload::None) => match maxsplit {
                    Some(n) => s.splitn(n + 1, char::is_whitespace)
                        .filter(|p| !p.is_empty())
                        .map(|p| PyObject::str_val(CompactString::from(p)))
                        .collect(),
                    None => s.split_whitespace()
                        .map(|p| PyObject::str_val(CompactString::from(p)))
                        .collect(),
                },
                Some(a) => {
                    let sep = a.as_str().ok_or_else(|| PyException::type_error("split() argument must be str or None"))?;
                    match maxsplit {
                        Some(n) => s.splitn(n + 1, sep)
                            .map(|p| PyObject::str_val(CompactString::from(p)))
                            .collect(),
                        None => s.split(sep)
                            .map(|p| PyObject::str_val(CompactString::from(p)))
                            .collect(),
                    }
                }
            };
            Ok(PyObject::list(parts))
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
            // Fast path: borrow List/Tuple inner data directly without cloning
            match &args[0].payload {
                PyObjectPayload::List(v) => {
                    let items = v.read();
                    join_str_slice(s, &items)
                }
                PyObjectPayload::Tuple(items) => join_str_slice(s, items),
                _ => {
                    let items = args[0].to_list()?;
                    join_str_slice(s, &items)
                }
            }
        }
        "replace" => {
            check_args_min("replace", args, 2)?;
            // Fast path: borrow &str directly when args are Str payloads (avoids String alloc)
            let old_ref = args[0].as_str();
            let new_ref = args[1].as_str();
            match (old_ref, new_ref) {
                (Some(old), Some(new)) => {
                    if args.len() >= 3 {
                        let count = args[2].to_int()? as usize;
                        // Build directly into CompactString — avoid intermediate String
                        let result = replace_into_compact(s, old, new, Some(count));
                        Ok(PyObject::str_val(result))
                    } else {
                        let result = replace_into_compact(s, old, new, None);
                        Ok(PyObject::str_val(result))
                    }
                }
                _ => {
                    // Fallback: extract via to_string() for subclasses
                    let old = extract_str_value(&args[0])
                        .ok_or_else(|| PyException::type_error("replace() argument 1 must be str"))?;
                    let new = extract_str_value(&args[1])
                        .ok_or_else(|| PyException::type_error("replace() argument 2 must be str"))?;
                    if args.len() >= 3 {
                        let count = args[2].to_int()? as usize;
                        Ok(PyObject::str_val(CompactString::from(s.replacen(&old, &new, count))))
                    } else {
                        Ok(PyObject::str_val(CompactString::from(s.replace(&old[..], &new[..]))))
                    }
                }
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
            // startswith(prefix[, start[, end]])
            let start = if args.len() > 1 { args[1].as_int().unwrap_or(0).max(0) as usize } else { 0 };
            let end = if args.len() > 2 { args[2].as_int().unwrap_or(s.len() as i64).max(0) as usize } else { s.len() };
            let slice = if start <= s.len() && end <= s.len() && start <= end {
                &s[start..end]
            } else if start <= s.len() {
                &s[start..]
            } else {
                ""
            };
            match &args[0].payload {
                PyObjectPayload::Tuple(prefixes) => {
                    let result = prefixes.iter().any(|p| {
                        p.as_str().map(|ps| slice.starts_with(ps)).unwrap_or(false)
                    });
                    Ok(PyObject::bool_val(result))
                }
                _ => {
                    let prefix = args[0].as_str().ok_or_else(|| PyException::type_error("startswith() argument must be str or tuple"))?;
                    Ok(PyObject::bool_val(slice.starts_with(prefix)))
                }
            }
        }
        "endswith" => {
            check_args_min("endswith", args, 1)?;
            // endswith(suffix[, start[, end]])
            let start = if args.len() > 1 { args[1].as_int().unwrap_or(0).max(0) as usize } else { 0 };
            let end = if args.len() > 2 { args[2].as_int().unwrap_or(s.len() as i64).max(0) as usize } else { s.len() };
            let slice = if start <= s.len() && end <= s.len() && start <= end {
                &s[start..end]
            } else if start <= s.len() {
                &s[start..]
            } else {
                ""
            };
            match &args[0].payload {
                PyObjectPayload::Tuple(suffixes) => {
                    let result = suffixes.iter().any(|p| {
                        p.as_str().map(|ps| slice.ends_with(ps)).unwrap_or(false)
                    });
                    Ok(PyObject::bool_val(result))
                }
                _ => {
                    let suffix = args[0].as_str().ok_or_else(|| PyException::type_error("endswith() argument must be str or tuple"))?;
                    Ok(PyObject::bool_val(slice.ends_with(suffix)))
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
            let left = pad / 2;
            let right = pad - left;
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
                "utf-16" | "utf16" => {
                    let mut bytes = vec![0xFF_u8, 0xFE]; // BOM (little-endian)
                    for unit in s.encode_utf16() {
                        bytes.extend_from_slice(&unit.to_le_bytes());
                    }
                    Ok(PyObject::bytes(bytes))
                }
                "utf-16-le" | "utf16-le" | "utf-16le" | "utf16le" => {
                    let bytes: Vec<u8> = s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
                    Ok(PyObject::bytes(bytes))
                }
                "utf-16-be" | "utf16-be" | "utf-16be" | "utf16be" => {
                    let bytes: Vec<u8> = s.encode_utf16().flat_map(|c| c.to_be_bytes()).collect();
                    Ok(PyObject::bytes(bytes))
                }
                "utf-32" | "utf32" => {
                    let mut bytes = vec![0xFF_u8, 0xFE, 0x00, 0x00]; // BOM
                    for ch in s.chars() {
                        bytes.extend_from_slice(&(ch as u32).to_le_bytes());
                    }
                    Ok(PyObject::bytes(bytes))
                }
                "utf-32-le" | "utf32-le" | "utf-32le" | "utf32le" => {
                    let bytes: Vec<u8> = s.chars().flat_map(|c| (c as u32).to_le_bytes()).collect();
                    Ok(PyObject::bytes(bytes))
                }
                "utf-32-be" | "utf32-be" | "utf-32be" | "utf32be" => {
                    let bytes: Vec<u8> = s.chars().flat_map(|c| (c as u32).to_be_bytes()).collect();
                    Ok(PyObject::bytes(bytes))
                }
                "cp1252" | "windows-1252" | "windows1252" => {
                    let mut result = Vec::new();
                    for ch in s.chars() {
                        let u = ch as u32;
                        if u < 0x80 || (0xA0..=0xFF).contains(&u) {
                            result.push(u as u8);
                        } else {
                            let byte = match u {
                                0x20AC => Some(0x80u8), 0x201A => Some(0x82), 0x0192 => Some(0x83),
                                0x201E => Some(0x84), 0x2026 => Some(0x85), 0x2020 => Some(0x86),
                                0x2021 => Some(0x87), 0x02C6 => Some(0x88), 0x2030 => Some(0x89),
                                0x0160 => Some(0x8A), 0x2039 => Some(0x8B), 0x0152 => Some(0x8C),
                                0x017D => Some(0x8E), 0x2018 => Some(0x91), 0x2019 => Some(0x92),
                                0x201C => Some(0x93), 0x201D => Some(0x94), 0x2022 => Some(0x95),
                                0x2013 => Some(0x96), 0x2014 => Some(0x97), 0x02DC => Some(0x98),
                                0x2122 => Some(0x99), 0x0161 => Some(0x9A), 0x203A => Some(0x9B),
                                0x0153 => Some(0x9C), 0x017E => Some(0x9E), 0x0178 => Some(0x9F),
                                _ => None,
                            };
                            match byte {
                                Some(b) => result.push(b),
                                None => match errors.as_str() {
                                    "ignore" => {}
                                    "replace" => result.push(b'?'),
                                    _ => return Err(PyException::new(
                                        ExceptionKind::UnicodeEncodeError,
                                        format!("'cp1252' codec can't encode character '\\u{:04x}'", u),
                                    )),
                                }
                            }
                        }
                    }
                    Ok(PyObject::bytes(result))
                }
                "punycode" => {
                    crate::builtins::string_methods::punycode_encode_str(s)
                }
                "idna" => {
                    Ok(PyObject::bytes(s.to_ascii_lowercase().into_bytes()))
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
                            resolve_format_field(field_name, args)
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
                            let key = HashableKey::str_key(CompactString::from(&field));
                            let guard = m.read();
                            if let Some(val) = guard.get(&key) {
                                result.push_str(&val.py_to_string());
                            } else {
                                // Support defaultdict: check for __defaultdict_factory__
                                let factory_key = HashableKey::str_key(CompactString::from("__defaultdict_factory__"));
                                if let Some(factory) = guard.get(&factory_key).cloned() {
                                    drop(guard);
                                    let val = match &factory.payload {
                                        PyObjectPayload::NativeFunction(nf) => (nf.func)(&[])?,
                                        PyObjectPayload::NativeClosure(nc) => (nc.func)(&[])?,
                                        _ => return Err(PyException::key_error(field)),
                                    };
                                    m.write().insert(key, val.clone());
                                    result.push_str(&val.py_to_string());
                                } else {
                                    return Err(PyException::key_error(field));
                                }
                            }
                        } else {
                            // Custom mapping: try __getitem__ subscription
                            let key_obj = PyObject::str_val(CompactString::from(&field));
                            let resolved = if let Some(getitem) = mapping.get_attr("__getitem__") {
                                match &getitem.payload {
                                    PyObjectPayload::NativeFunction(nf) => {
                                        Some((nf.func)(&[key_obj])?)
                                    }
                                    PyObjectPayload::NativeClosure(nc) => {
                                        Some((nc.func)(&[key_obj])?)
                                    }
                                    _ => None,
                                }
                            } else {
                                None
                            };
                            if let Some(val) = resolved {
                                result.push_str(&val.py_to_string());
                            } else if let Some(val) = mapping.get_attr(&field) {
                                result.push_str(&val.py_to_string());
                            } else {
                                return Err(PyException::key_error(field));
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
        // Dunder methods accessible as attributes (CPython compatibility)
        "__format__" => {
            let spec = if args.is_empty() { "" } else { args[0].as_str().unwrap_or("") };
            if spec.is_empty() {
                Ok(PyObject::str_val(CompactString::from(s)))
            } else {
                Ok(PyObject::str_val(CompactString::from(apply_format_spec_str(s, spec))))
            }
        }
        "__str__" => Ok(PyObject::str_val(CompactString::from(s))),
        "__repr__" => Ok(PyObject::str_val(CompactString::from(format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))))),
        "__hash__" => {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            s.hash(&mut hasher);
            Ok(PyObject::int(hasher.finish() as i64))
        }
        "__len__" => Ok(PyObject::int(s.chars().count() as i64)),
        "__contains__" => {
            check_args_min("str.__contains__", &args, 1)?;
            let sub = args[0].as_str().unwrap_or("");
            Ok(PyObject::bool_val(s.contains(sub)))
        }
        "__eq__" => {
            check_args_min("str.__eq__", &args, 1)?;
            Ok(if let Some(other) = args[0].as_str() { PyObject::bool_val(s == other) } else { PyObject::bool_val(false) })
        }
        "__ne__" => {
            check_args_min("str.__ne__", &args, 1)?;
            Ok(if let Some(other) = args[0].as_str() { PyObject::bool_val(s != other) } else { PyObject::bool_val(true) })
        }
        "__lt__" => {
            check_args_min("str.__lt__", &args, 1)?;
            Ok(if let Some(other) = args[0].as_str() { PyObject::bool_val(s < other) } else { PyObject::not_implemented() })
        }
        "__le__" => {
            check_args_min("str.__le__", &args, 1)?;
            Ok(if let Some(other) = args[0].as_str() { PyObject::bool_val(s <= other) } else { PyObject::not_implemented() })
        }
        "__gt__" => {
            check_args_min("str.__gt__", &args, 1)?;
            Ok(if let Some(other) = args[0].as_str() { PyObject::bool_val(s > other) } else { PyObject::not_implemented() })
        }
        "__ge__" => {
            check_args_min("str.__ge__", &args, 1)?;
            Ok(if let Some(other) = args[0].as_str() { PyObject::bool_val(s >= other) } else { PyObject::not_implemented() })
        }
        "__add__" => {
            check_args_min("str.__add__", &args, 1)?;
            let other = args[0].py_to_string();
            Ok(PyObject::str_val(CompactString::from(format!("{}{}", s, other))))
        }
        "__mul__" | "__rmul__" => {
            check_args_min("str.__mul__", &args, 1)?;
            let n = args[0].as_int().unwrap_or(0);
            Ok(PyObject::str_val(CompactString::from(s.repeat(n.max(0) as usize))))
        }
        "__getitem__" => {
            check_args_min("str.__getitem__", &args, 1)?;
            let idx = args[0].as_int().unwrap_or(0);
            let chars: Vec<char> = s.chars().collect();
            let real_idx = if idx < 0 { (chars.len() as i64 + idx) as usize } else { idx as usize };
            if real_idx < chars.len() {
                Ok(PyObject::str_val(CompactString::from(chars[real_idx].to_string())))
            } else {
                Err(PyException::index_error("string index out of range"))
            }
        }
        "__iter__" => {
            let chars: Vec<PyObjectRef> = s.chars().map(|c| PyObject::str_val(CompactString::from(c.to_string()))).collect();
            Ok(PyObject::list(chars))
        }
        "__mod__" => {
            // Basic %-formatting: "hello %s" % "world"
            check_args_min("str.__mod__", &args, 1)?;
            let val = &args[0];
            // Simplified: replace first %s, %d, %r, etc.
            let result = s.replacen("%s", &val.py_to_string(), 1)
                          .replacen("%d", &val.py_to_string(), 1)
                          .replacen("%r", &val.repr(), 1);
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        _ => Err(PyException::attribute_error(format!(
            "'str' object has no attribute '{}'", method
        ))),
    }
}

// ── Punycode helpers (RFC 3492) ────────────────────────────────────

fn punycode_digit(d: u32) -> u8 {
    if d < 26 { b'a' + d as u8 } else { b'0' + (d as u8 - 26) }
}

fn punycode_adapt(delta: u32, numpoints: u32, firsttime: bool) -> u32 {
    let mut d = if firsttime { delta / 700 } else { delta / 2 };
    d += d / numpoints;
    let mut k = 0u32;
    while d > 455 { d /= 35; k += 36; }
    k + (36 * d) / (d + 38)
}

pub fn punycode_encode_str(s: &str) -> PyResult<PyObjectRef> {
    let mut output = Vec::new();
    let mut basic_count = 0u32;
    for ch in s.chars() {
        if ch.is_ascii() {
            output.push(ch as u8);
            basic_count += 1;
        }
    }
    // RFC 3492: always output delimiter when basic code points exist
    if basic_count > 0 {
        output.push(b'-');
    }
    let mut n: u32 = 128;
    let mut delta: u32 = 0;
    let mut bias: u32 = 72;
    let mut h = basic_count;
    let all_chars: Vec<u32> = s.chars().map(|c| c as u32).collect();
    let total = all_chars.len() as u32;
    while h < total {
        let m = *all_chars.iter().filter(|&&cp| cp >= n).min().unwrap_or(&n);
        delta = delta.wrapping_add((m - n).wrapping_mul(h + 1));
        n = m;
        for &cp in &all_chars {
            if cp < n { delta = delta.wrapping_add(1); }
            if cp == n {
                let mut q = delta;
                let mut k = 36u32;
                loop {
                    let t = if k <= bias { 1 } else if k >= bias + 26 { 26 } else { k - bias };
                    if q < t { break; }
                    let digit = t + (q - t) % (36 - t);
                    output.push(punycode_digit(digit));
                    q = (q - t) / (36 - t);
                    k += 36;
                }
                output.push(punycode_digit(q));
                bias = punycode_adapt(delta, h + 1, h == basic_count);
                delta = 0;
                h += 1;
            }
        }
        delta += 1;
        n += 1;
    }
    Ok(PyObject::bytes(output))
}

pub fn punycode_decode_bytes(bytes: &[u8]) -> PyResult<PyObjectRef> {
    let input = std::str::from_utf8(bytes)
        .map_err(|_| PyException::value_error("punycode: invalid input"))?;
    let (basic_part, encoded_part) = if let Some(pos) = input.rfind('-') {
        (&input[..pos], &input[pos + 1..])
    } else {
        ("", input)
    };
    let mut output: Vec<u32> = basic_part.chars().map(|c| c as u32).collect();
    let mut n: u32 = 128;
    let mut i: u32 = 0;
    let mut bias: u32 = 72;
    let encoded_bytes = encoded_part.as_bytes();
    let mut idx = 0;
    while idx < encoded_bytes.len() {
        let oldi = i;
        let mut w: u32 = 1;
        let mut k: u32 = 36;
        loop {
            if idx >= encoded_bytes.len() { break; }
            let byte = encoded_bytes[idx];
            idx += 1;
            let digit = match byte {
                b'a'..=b'z' => (byte - b'a') as u32,
                b'A'..=b'Z' => (byte - b'A') as u32,
                b'0'..=b'9' => (byte - b'0') as u32 + 26,
                _ => return Err(PyException::value_error("punycode: bad input")),
            };
            i = i.wrapping_add(digit.wrapping_mul(w));
            let t = if k <= bias { 1 } else if k >= bias + 26 { 26 } else { k - bias };
            if digit < t { break; }
            w = w.wrapping_mul(36 - t);
            k += 36;
        }
        let out_len = output.len() as u32 + 1;
        bias = punycode_adapt(i.wrapping_sub(oldi), out_len, oldi == 0);
        n = n.wrapping_add(i / out_len);
        i %= out_len;
        output.insert(i as usize, n);
        i += 1;
    }
    let result: String = output.iter().filter_map(|&cp| char::from_u32(cp)).collect();
    Ok(PyObject::str_val(CompactString::from(result)))
}

/// Replace occurrences of `old` with `new` in `s`, writing directly into a CompactString.
/// Avoids the intermediate String allocation that `str::replace()` creates.
fn replace_into_compact(s: &str, old: &str, new: &str, max_count: Option<usize>) -> CompactString {
    if old.is_empty() {
        // Empty pattern: insert `new` between each character (CPython behavior)
        let char_count = s.chars().count();
        let limit = max_count.unwrap_or(char_count + 1);
        let mut result = CompactString::with_capacity(s.len() + new.len() * limit.min(char_count + 1));
        let mut count = 0;
        for ch in s.chars() {
            if count < limit {
                result.push_str(new);
                count += 1;
            }
            result.push(ch);
        }
        if count < limit {
            result.push_str(new);
        }
        return result;
    }
    // Estimate capacity
    let mut result = CompactString::with_capacity(s.len());
    let mut remaining = s;
    let mut count = 0;
    let limit = max_count.unwrap_or(usize::MAX);
    while count < limit {
        match remaining.find(old) {
            Some(pos) => {
                result.push_str(&remaining[..pos]);
                result.push_str(new);
                remaining = &remaining[pos + old.len()..];
                count += 1;
            }
            None => break,
        }
    }
    result.push_str(remaining);
    result
}
/// Avoids cloning the list/tuple just to iterate.
fn join_str_slice(sep: &str, items: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if items.is_empty() {
        return Ok(PyObject::str_val(CompactString::new("")));
    }
    let mut total_len = sep.len() * (items.len() - 1);
    for (i, item) in items.iter().enumerate() {
        match item.as_str() {
            Some(part) => total_len += part.len(),
            None => return Err(PyException::type_error(
                format!("sequence item {}: expected str instance, {} found", i, item.type_name())
            )),
        }
    }
    let mut result = String::with_capacity(total_len);
    for (i, item) in items.iter().enumerate() {
        if i > 0 { result.push_str(sep); }
        // SAFETY: validated all items are str in the loop above
        if let Some(part) = item.as_str() {
            result.push_str(part);
        }
    }
    Ok(PyObject::str_val(CompactString::from(result)))
}
