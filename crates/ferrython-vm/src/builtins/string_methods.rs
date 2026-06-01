//! String method dispatch (upper, lower, split, replace, strip, join, find, format, etc.)

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    alloc_list_box_empty, check_args_min, checked_repeat_len, index_to_i64, index_to_usize_repeat,
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, PyInt};
use indexmap::IndexMap;
use num_traits::ToPrimitive;

use super::apply_format_spec_str;

mod encoding;
mod fast_ops;
mod formatting;
mod punycode;

use encoding::str_encode;
use fast_ops::{
    fast_count, fast_find, join_str_slice, replace_into_compact, split_single_byte_into,
};
use formatting::{resolve_format_field, resolve_nested_spec};
pub(crate) use punycode::{punycode_decode_bytes, punycode_encode_str};

/// Extract a string value from a PyObject, accepting both Str payload and str subclasses.
fn extract_str_value(obj: &PyObjectRef) -> Option<String> {
    match &obj.payload {
        PyObjectPayload::Str(s) => Some(s.to_string()),
        PyObjectPayload::Instance(inst) => {
            inst.attrs
                .read()
                .get("__builtin_value__")
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

fn check_args_range(name: &str, args: &[PyObjectRef], min: usize, max: usize) -> PyResult<()> {
    if args.len() < min || args.len() > max {
        Err(PyException::type_error(format!(
            "{}() takes from {} to {} argument(s) ({} given)",
            name,
            min,
            max,
            args.len()
        )))
    } else {
        Ok(())
    }
}

fn check_no_args(name: &str, args: &[PyObjectRef]) -> PyResult<()> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(PyException::type_error(format!(
            "{}() takes no arguments ({} given)",
            name,
            args.len()
        )))
    }
}

fn is_none(obj: &PyObjectRef) -> bool {
    matches!(&obj.payload, PyObjectPayload::None)
}

fn extract_string_arg(obj: &PyObjectRef, method: &str) -> PyResult<String> {
    extract_str_value(obj)
        .ok_or_else(|| PyException::type_error(format!("{}() argument must be str", method)))
}

fn py_index_arg(obj: &PyObjectRef) -> PyResult<i64> {
    obj.to_index()?
        .to_i64()
        .ok_or_else(|| PyException::overflow_error("cannot fit 'int' into an index-sized integer"))
}

fn clamped_index_arg(obj: &PyObjectRef) -> PyResult<i64> {
    match obj.to_index()? {
        PyInt::Small(n) => Ok(n),
        PyInt::Big(n) => Ok(n.to_i64().unwrap_or_else(|| {
            if n.as_ref().sign() == num_bigint::Sign::Minus {
                i64::MIN
            } else {
                i64::MAX
            }
        })),
    }
}

fn optional_index_arg(obj: Option<&PyObjectRef>, default: i64) -> PyResult<i64> {
    match obj {
        Some(value) if !is_none(value) => clamped_index_arg(value),
        _ => Ok(default),
    }
}

fn slice_char_bounds(
    s: &str,
    start: Option<&PyObjectRef>,
    end: Option<&PyObjectRef>,
) -> PyResult<(usize, usize)> {
    let len = s.chars().count() as i64;
    let start = normalize_index(optional_index_arg(start, 0)?, len);
    let end = normalize_index(optional_index_arg(end, len)?, len);
    Ok((start, end))
}

fn raw_slice_char_bounds(
    s: &str,
    start: Option<&PyObjectRef>,
    end: Option<&PyObjectRef>,
) -> PyResult<(i64, i64)> {
    let len = s.chars().count() as i64;
    Ok((optional_index_arg(start, 0)?, optional_index_arg(end, len)?))
}

fn char_to_byte_idx(s: &str, char_idx: usize) -> usize {
    if char_idx == 0 {
        0
    } else if char_idx >= s.chars().count() {
        s.len()
    } else {
        s.char_indices()
            .nth(char_idx)
            .map(|(idx, _)| idx)
            .unwrap_or(s.len())
    }
}

fn char_range_to_byte_range(s: &str, start: usize, end: usize) -> (usize, usize) {
    if s.is_ascii() {
        (start.min(s.len()), end.min(s.len()))
    } else {
        (char_to_byte_idx(s, start), char_to_byte_idx(s, end))
    }
}

fn split_max_arg(obj: Option<&PyObjectRef>) -> PyResult<Option<usize>> {
    match obj {
        Some(value) if !is_none(value) => {
            let n = py_index_arg(value)?;
            if n < 0 {
                Ok(None)
            } else {
                Ok(Some(n as usize))
            }
        }
        _ => Ok(None),
    }
}

fn py_split_whitespace(s: &str, maxsplit: Option<usize>) -> Vec<String> {
    let maxsplit = maxsplit.unwrap_or(usize::MAX);
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut parts = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        while i < chars.len() && chars[i].1.is_whitespace() {
            i += 1;
        }
        if i == chars.len() {
            break;
        }
        if parts.len() == maxsplit {
            parts.push(s[chars[i].0..].to_string());
            return parts;
        }
        let start = chars[i].0;
        while i < chars.len() && !chars[i].1.is_whitespace() {
            i += 1;
        }
        let end = if i < chars.len() { chars[i].0 } else { s.len() };
        parts.push(s[start..end].to_string());
    }
    parts
}

fn py_rsplit_whitespace(s: &str, maxsplit: Option<usize>) -> Vec<String> {
    let maxsplit = maxsplit.unwrap_or(usize::MAX);
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut parts = Vec::new();
    let mut i = chars.len();
    while i > 0 {
        while i > 0 && chars[i - 1].1.is_whitespace() {
            i -= 1;
        }
        if i == 0 {
            break;
        }
        let end = chars[i - 1].0 + chars[i - 1].1.len_utf8();
        if parts.len() == maxsplit {
            parts.push(s[..end].to_string());
            break;
        }
        while i > 0 && !chars[i - 1].1.is_whitespace() {
            i -= 1;
        }
        let start = if i < chars.len() { chars[i].0 } else { 0 };
        parts.push(s[start..end].to_string());
    }
    parts.reverse();
    parts
}

fn list_from_strings(parts: Vec<String>) -> PyObjectRef {
    PyObject::list(
        parts
            .into_iter()
            .map(|part| PyObject::str_val(CompactString::from(part)))
            .collect(),
    )
}

fn tuple_str3(a: &str, b: &str, c: &str) -> PyObjectRef {
    PyObject::tuple(vec![
        PyObject::str_val(CompactString::from(a)),
        PyObject::str_val(CompactString::from(b)),
        PyObject::str_val(CompactString::from(c)),
    ])
}

fn empty_search_result(start: usize, end: usize) -> Option<i64> {
    if start > end {
        None
    } else {
        Some(start as i64)
    }
}

fn empty_rsearch_result(start: usize, end: usize) -> Option<i64> {
    if start > end {
        None
    } else {
        Some(end as i64)
    }
}

fn empty_match_allowed(
    s: &str,
    start_arg: Option<&PyObjectRef>,
    end_arg: Option<&PyObjectRef>,
) -> PyResult<bool> {
    let len = s.chars().count() as i64;
    let (raw_start, raw_end) = raw_slice_char_bounds(s, start_arg, end_arg)?;
    Ok(raw_start <= len && raw_end >= -len)
}

fn empty_boundary_allowed(
    s: &str,
    start: usize,
    end: usize,
    start_arg: Option<&PyObjectRef>,
    end_arg: Option<&PyObjectRef>,
) -> PyResult<bool> {
    Ok(start <= end && empty_match_allowed(s, start_arg, end_arg)?)
}

fn extract_tuple_str_items<'a>(items: &'a [PyObjectRef], method: &str) -> PyResult<Vec<String>> {
    let mut strings = Vec::with_capacity(items.len());
    for item in items {
        strings.push(extract_str_value(item).ok_or_else(|| {
            PyException::type_error(format!("{}() argument must be str or tuple", method))
        })?);
    }
    Ok(strings)
}

fn titlecase_char(c: char) -> String {
    match c as u32 {
        0x1f80..=0x1f87 => char::from_u32(c as u32 + 8).unwrap().to_string(),
        0x1f88..=0x1f8f => c.to_string(),
        0x1f90..=0x1f97 => char::from_u32(c as u32 + 8).unwrap().to_string(),
        0x1f98..=0x1f9f => c.to_string(),
        0x1fa0..=0x1fa7 => char::from_u32(c as u32 + 8).unwrap().to_string(),
        0x1fa8..=0x1faf => c.to_string(),
        0x1fb2 => "\u{1fba}\u{0345}".to_string(),
        0x1fb3 | 0x1fbc => "\u{1fbc}".to_string(),
        0x1fb4 => "\u{0386}\u{0345}".to_string(),
        0x1fb7 => "\u{0391}\u{0342}\u{0345}".to_string(),
        0x019b => "\u{019b}".to_string(),
        0x1fc2 => "\u{1fca}\u{0345}".to_string(),
        0x1fc3 | 0x1fcc => "\u{1fcc}".to_string(),
        0x1fc4 => "\u{0389}\u{0345}".to_string(),
        0x1fc7 => "\u{0397}\u{0342}\u{0345}".to_string(),
        0x1ff2 => "\u{1ffa}\u{0345}".to_string(),
        0x1ff3 | 0x1ffc => "\u{1ffc}".to_string(),
        0x1ff4 => "\u{038f}\u{0345}".to_string(),
        0x1ff7 => "\u{03a9}\u{0342}\u{0345}".to_string(),
        0xa7dc => "\u{a7dc}".to_string(),
        _ => c.to_uppercase().collect(),
    }
}

pub(crate) fn call_str_method(
    s: &str,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match method {
        "upper" => {
            check_no_args("upper", args)?;
            Ok(PyObject::str_val(CompactString::from(s.to_uppercase())))
        }
        "lower" => {
            check_no_args("lower", args)?;
            Ok(PyObject::str_val(CompactString::from(s.to_lowercase())))
        }
        "strip" => {
            check_args_range("strip", args, 0, 1)?;
            if let Some(arg) = args.first() {
                if !is_none(arg) {
                    let chars = arg
                        .as_str()
                        .ok_or_else(|| PyException::type_error("strip arg must be None or str"))?;
                    let ch: Vec<char> = chars.chars().collect();
                    let trimmed = s.trim_matches(|c: char| ch.contains(&c));
                    return Ok(PyObject::str_val(CompactString::from(trimmed)));
                }
            }
            Ok(PyObject::str_val(CompactString::from(s.trim())))
        }
        "lstrip" => {
            check_args_range("lstrip", args, 0, 1)?;
            if let Some(arg) = args.first() {
                if !is_none(arg) {
                    let chars = arg
                        .as_str()
                        .ok_or_else(|| PyException::type_error("lstrip arg must be None or str"))?;
                    let ch: Vec<char> = chars.chars().collect();
                    let trimmed = s.trim_start_matches(|c: char| ch.contains(&c));
                    return Ok(PyObject::str_val(CompactString::from(trimmed)));
                }
            }
            Ok(PyObject::str_val(CompactString::from(s.trim_start())))
        }
        "rstrip" => {
            check_args_range("rstrip", args, 0, 1)?;
            if let Some(arg) = args.first() {
                if !is_none(arg) {
                    let chars = arg
                        .as_str()
                        .ok_or_else(|| PyException::type_error("rstrip arg must be None or str"))?;
                    let ch: Vec<char> = chars.chars().collect();
                    let trimmed = s.trim_end_matches(|c: char| ch.contains(&c));
                    return Ok(PyObject::str_val(CompactString::from(trimmed)));
                }
            }
            Ok(PyObject::str_val(CompactString::from(s.trim_end())))
        }
        "split" => {
            let pos = positional_args(args);
            check_args_range("split", pos, 0, 2)?;
            let sep_obj = extract_kwarg(args, "sep").or_else(|| pos.first().cloned());
            let maxsplit_obj = if pos.len() > 1 {
                pos.get(1).cloned()
            } else {
                extract_kwarg(args, "maxsplit")
            };
            let maxsplit = split_max_arg(maxsplit_obj.as_ref())?;
            if sep_obj.as_ref().map(is_none).unwrap_or(true) {
                return Ok(list_from_strings(py_split_whitespace(s, maxsplit)));
            }
            let sep_obj = sep_obj.unwrap();
            let sep = sep_obj
                .as_str()
                .ok_or_else(|| PyException::type_error("split() argument must be str or None"))?;
            if sep.is_empty() {
                return Err(PyException::value_error("empty separator"));
            }
            let parts: Vec<String> = match maxsplit {
                Some(n) => s.splitn(n + 1, sep).map(ToString::to_string).collect(),
                None => {
                    if sep.len() == 1 && s.is_ascii() {
                        let list_box = alloc_list_box_empty();
                        let parts = unsafe { &mut *list_box.data_ptr() };
                        split_single_byte_into(s.as_bytes(), sep.as_bytes()[0], parts);
                        return Ok(PyObject::wrap_leaf(PyObjectPayload::List(list_box)));
                    }
                    s.split(sep).map(ToString::to_string).collect()
                }
            };
            Ok(list_from_strings(parts))
        }
        "rsplit" => {
            let pos = positional_args(args);
            check_args_range("rsplit", pos, 0, 2)?;
            let sep_obj = extract_kwarg(args, "sep").or_else(|| pos.first().cloned());
            let maxsplit_obj = if pos.len() > 1 {
                pos.get(1).cloned()
            } else {
                extract_kwarg(args, "maxsplit")
            };
            let maxsplit = split_max_arg(maxsplit_obj.as_ref())?;
            if sep_obj.as_ref().map(is_none).unwrap_or(true) {
                return Ok(list_from_strings(py_rsplit_whitespace(s, maxsplit)));
            }
            let sep_obj = sep_obj.unwrap();
            let sep = sep_obj
                .as_str()
                .ok_or_else(|| PyException::type_error("rsplit() argument must be str or None"))?;
            if sep.is_empty() {
                return Err(PyException::value_error("empty separator"));
            }
            let mut parts: Vec<String> = match maxsplit {
                Some(n) => s.rsplitn(n + 1, sep).map(ToString::to_string).collect(),
                None => s.rsplit(sep).map(ToString::to_string).collect(),
            };
            parts.reverse();
            Ok(list_from_strings(parts))
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
                    let old = extract_str_value(&args[0]).ok_or_else(|| {
                        PyException::type_error("replace() argument 1 must be str")
                    })?;
                    let new = extract_str_value(&args[1]).ok_or_else(|| {
                        PyException::type_error("replace() argument 2 must be str")
                    })?;
                    if args.len() >= 3 {
                        let count = args[2].to_int()? as usize;
                        Ok(PyObject::str_val(CompactString::from(
                            s.replacen(&old, &new, count),
                        )))
                    } else {
                        Ok(PyObject::str_val(CompactString::from(
                            s.replace(&old[..], &new[..]),
                        )))
                    }
                }
            }
        }
        "find" => {
            check_args_range("find", args, 1, 3)?;
            let sub = extract_string_arg(&args[0], "find")?;
            let (start, end) = slice_char_bounds(s, args.get(1), args.get(2))?;
            if sub.is_empty() {
                let result = if empty_match_allowed(s, args.get(1), args.get(2))? {
                    empty_search_result(start, end).unwrap_or(-1)
                } else {
                    -1
                };
                return Ok(PyObject::int(result));
            }
            let (area_start, area_end) = char_range_to_byte_range(s, start, end);
            if start > end {
                return Ok(PyObject::int(-1));
            }
            let search_area = s[area_start..area_end].as_bytes();
            Ok(PyObject::int(
                fast_find(search_area, 0, sub.as_bytes())
                    .map(|i| (s[..area_start + i].chars().count()) as i64)
                    .unwrap_or(-1),
            ))
        }
        "rfind" => {
            check_args_range("rfind", args, 1, 3)?;
            let sub = extract_string_arg(&args[0], "rfind")?;
            let (start, end) = slice_char_bounds(s, args.get(1), args.get(2))?;
            if sub.is_empty() {
                let result = if empty_match_allowed(s, args.get(1), args.get(2))? {
                    empty_rsearch_result(start, end).unwrap_or(-1)
                } else {
                    -1
                };
                return Ok(PyObject::int(result));
            }
            if start > end {
                return Ok(PyObject::int(-1));
            }
            let (area_start, area_end) = char_range_to_byte_range(s, start, end);
            let search_area = &s[area_start..area_end];
            Ok(PyObject::int(
                search_area
                    .rfind(&sub)
                    .map(|i| (s[..area_start + i].chars().count()) as i64)
                    .unwrap_or(-1),
            ))
        }
        "index" => {
            check_args_range("index", args, 1, 3)?;
            let result = call_str_method(s, "find", args)?;
            match result.as_int() {
                Some(i) if i >= 0 => Ok(PyObject::int(i)),
                _ => Err(PyException::value_error("substring not found")),
            }
        }
        "rindex" => {
            check_args_range("rindex", args, 1, 3)?;
            let result = call_str_method(s, "rfind", args)?;
            match result.as_int() {
                Some(i) if i >= 0 => Ok(PyObject::int(i)),
                None => Err(PyException::value_error("substring not found")),
                _ => Err(PyException::value_error("substring not found")),
            }
        }
        "count" => {
            check_args_range("count", args, 1, 3)?;
            let sub = extract_string_arg(&args[0], "count")?;
            let (start, end) = slice_char_bounds(s, args.get(1), args.get(2))?;
            if sub.is_empty() {
                if start > end || !empty_match_allowed(s, args.get(1), args.get(2))? {
                    return Ok(PyObject::int(0));
                }
                return Ok(PyObject::int((end.saturating_sub(start) + 1) as i64));
            }
            if start > end {
                return Ok(PyObject::int(0));
            }
            let (byte_start, byte_end) = char_range_to_byte_range(s, start, end);
            let slice = &s.as_bytes()[byte_start..byte_end];
            Ok(PyObject::int(
                fast_count(slice, sub.as_bytes(), usize::MAX) as i64
            ))
        }
        "startswith" => {
            check_args_range("startswith", args, 1, 3)?;
            let (start, end) = slice_char_bounds(s, args.get(1), args.get(2))?;
            let (byte_start, byte_end) = char_range_to_byte_range(s, start, end);
            let empty_allowed = empty_boundary_allowed(s, start, end, args.get(1), args.get(2))?;
            let slice = if start <= end && empty_allowed {
                &s[byte_start..byte_end]
            } else {
                ""
            };
            match &args[0].payload {
                PyObjectPayload::Tuple(prefixes) => {
                    let prefixes = extract_tuple_str_items(prefixes, "startswith")?;
                    let mut result = false;
                    for p in prefixes.iter() {
                        if slice.starts_with(p) {
                            result = true;
                            break;
                        }
                    }
                    Ok(PyObject::bool_val(result))
                }
                _ => {
                    let prefix = extract_str_value(&args[0]).ok_or_else(|| {
                        PyException::type_error("startswith() argument must be str or tuple")
                    })?;
                    if prefix.is_empty() {
                        return Ok(PyObject::bool_val(empty_allowed));
                    }
                    Ok(PyObject::bool_val(slice.starts_with(&prefix)))
                }
            }
        }
        "endswith" => {
            check_args_range("endswith", args, 1, 3)?;
            let (start, end) = slice_char_bounds(s, args.get(1), args.get(2))?;
            let (byte_start, byte_end) = char_range_to_byte_range(s, start, end);
            let empty_allowed = empty_boundary_allowed(s, start, end, args.get(1), args.get(2))?;
            let slice = if start <= end && empty_allowed {
                &s[byte_start..byte_end]
            } else {
                ""
            };
            match &args[0].payload {
                PyObjectPayload::Tuple(suffixes) => {
                    let suffixes = extract_tuple_str_items(suffixes, "endswith")?;
                    let mut result = false;
                    for p in suffixes.iter() {
                        if slice.ends_with(p) {
                            result = true;
                            break;
                        }
                    }
                    Ok(PyObject::bool_val(result))
                }
                _ => {
                    let suffix = extract_str_value(&args[0]).ok_or_else(|| {
                        PyException::type_error("endswith() argument must be str or tuple")
                    })?;
                    if suffix.is_empty() {
                        return Ok(PyObject::bool_val(empty_allowed));
                    }
                    Ok(PyObject::bool_val(slice.ends_with(&suffix)))
                }
            }
        }
        "isdigit" => {
            check_no_args("isdigit", args)?;
            Ok(PyObject::bool_val(
                !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()),
            ))
        }
        "isalpha" => {
            check_no_args("isalpha", args)?;
            Ok(PyObject::bool_val(
                !s.is_empty() && s.chars().all(|c| c.is_alphabetic()),
            ))
        }
        "isalnum" => {
            check_no_args("isalnum", args)?;
            Ok(PyObject::bool_val(
                !s.is_empty() && s.chars().all(|c| c.is_alphanumeric()),
            ))
        }
        "isspace" => {
            check_no_args("isspace", args)?;
            Ok(PyObject::bool_val(
                !s.is_empty() && s.chars().all(|c| c.is_whitespace()),
            ))
        }
        "isupper" => {
            check_no_args("isupper", args)?;
            Ok(PyObject::bool_val(
                s.chars().any(|c| c.is_uppercase()) && !s.chars().any(|c| c.is_lowercase()),
            ))
        }
        "islower" => {
            check_no_args("islower", args)?;
            Ok(PyObject::bool_val(
                s.chars().any(|c| c.is_lowercase()) && !s.chars().any(|c| c.is_uppercase()),
            ))
        }
        "title" => {
            check_no_args("title", args)?;
            let mut result = String::with_capacity(s.len());
            let mut prev_alpha = false;
            for c in s.chars() {
                if c.is_alphabetic() {
                    if prev_alpha {
                        result.extend(c.to_lowercase());
                    } else {
                        result.extend(c.to_uppercase());
                    }
                    prev_alpha = true;
                } else {
                    result.push(c);
                    prev_alpha = false;
                }
            }
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "capitalize" => {
            check_no_args("capitalize", args)?;
            let mut chars = s.chars();
            let result = match chars.next() {
                None => String::new(),
                Some(c) => {
                    let mut r = titlecase_char(c);
                    for c in chars {
                        r.extend(c.to_lowercase());
                    }
                    r
                }
            };
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "swapcase" => {
            check_no_args("swapcase", args)?;
            let result: String = s
                .chars()
                .map(|c| {
                    if c.is_uppercase() {
                        c.to_lowercase().to_string()
                    } else if c.is_lowercase() {
                        c.to_uppercase().to_string()
                    } else {
                        c.to_string()
                    }
                })
                .collect();
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "center" => {
            check_args_min("center", args, 1)?;
            let width = args[0].to_int()? as usize;
            let fillchar = if args.len() >= 2 {
                args[1]
                    .as_str()
                    .and_then(|s| s.chars().next())
                    .unwrap_or(' ')
            } else {
                ' '
            };
            let len = s.chars().count();
            if width <= len {
                return Ok(PyObject::str_val(CompactString::from(s)));
            }
            let pad = width - len;
            // CPython formula: left = marg//2 + (marg & width & 1)
            // When padding is odd, left gets the extra character (same as CPython)
            let left = pad / 2 + (pad & width & 1);
            let right = pad - left;
            checked_repeat_len(1, pad, "str.center")?;
            let result = format!(
                "{}{}{}",
                fillchar.to_string().repeat(left),
                s,
                fillchar.to_string().repeat(right)
            );
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "ljust" => {
            check_args_min("ljust", args, 1)?;
            let width = args[0].to_int()? as usize;
            let fillchar = if args.len() >= 2 {
                args[1]
                    .as_str()
                    .and_then(|s| s.chars().next())
                    .unwrap_or(' ')
            } else {
                ' '
            };
            let len = s.chars().count();
            if width <= len {
                return Ok(PyObject::str_val(CompactString::from(s)));
            }
            checked_repeat_len(1, width - len, "str.ljust")?;
            let result = format!("{}{}", s, fillchar.to_string().repeat(width - len));
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "rjust" => {
            check_args_min("rjust", args, 1)?;
            let width = args[0].to_int()? as usize;
            let fillchar = if args.len() >= 2 {
                args[1]
                    .as_str()
                    .and_then(|s| s.chars().next())
                    .unwrap_or(' ')
            } else {
                ' '
            };
            let len = s.chars().count();
            if width <= len {
                return Ok(PyObject::str_val(CompactString::from(s)));
            }
            checked_repeat_len(1, width - len, "str.rjust")?;
            let result = format!("{}{}", fillchar.to_string().repeat(width - len), s);
            Ok(PyObject::str_val(CompactString::from(result)))
        }
        "zfill" => {
            check_args_min("zfill", args, 1)?;
            let width = args[0].to_int()? as usize;
            let len = s.len();
            if width <= len {
                return Ok(PyObject::str_val(CompactString::from(s)));
            }
            checked_repeat_len(1, width - len, "str.zfill")?;
            let pad = "0".repeat(width - len);
            if s.starts_with('-') || s.starts_with('+') {
                Ok(PyObject::str_val(CompactString::from(format!(
                    "{}{}{}",
                    &s[..1],
                    pad,
                    &s[1..]
                ))))
            } else {
                Ok(PyObject::str_val(CompactString::from(format!(
                    "{}{}",
                    pad, s
                ))))
            }
        }
        "expandtabs" => {
            let tabsize_obj = extract_kwarg(args, "tabsize");
            let pos = positional_args(args);
            check_args_range("expandtabs", pos, 0, 1)?;
            let tabsize = if let Some(value) = tabsize_obj.as_ref().or_else(|| pos.first()) {
                py_index_arg(value)?.max(0) as usize
            } else {
                8
            };
            let mut result = String::new();
            let mut col = 0usize;
            for ch in s.chars() {
                if ch == '\t' {
                    let spaces = if tabsize == 0 {
                        0
                    } else {
                        tabsize - (col % tabsize)
                    };
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
            check_args_range("encode", args, 0, 2)?;
            let encoding = if !args.is_empty() && !is_none(&args[0]) {
                args[0].py_to_string().to_lowercase()
            } else {
                "utf-8".to_string()
            };
            let errors = if args.len() > 1 && !is_none(&args[1]) {
                args[1].py_to_string()
            } else {
                "strict".to_string()
            };
            str_encode(s, &encoding, &errors)
        }
        "partition" => {
            check_args_range("partition", args, 1, 1)?;
            let sep = args[0]
                .as_str()
                .ok_or_else(|| PyException::type_error("partition() argument must be str"))?;
            if sep.is_empty() {
                return Err(PyException::value_error("empty separator"));
            }
            if let Some(idx) = s.find(&sep) {
                Ok(tuple_str3(&s[..idx], sep, &s[idx + sep.len()..]))
            } else {
                Ok(tuple_str3(s, "", ""))
            }
        }
        "rpartition" => {
            check_args_range("rpartition", args, 1, 1)?;
            let sep = args[0]
                .as_str()
                .ok_or_else(|| PyException::type_error("rpartition() argument must be str"))?;
            if sep.is_empty() {
                return Err(PyException::value_error("empty separator"));
            }
            if let Some(idx) = s.rfind(&sep) {
                Ok(tuple_str3(&s[..idx], sep, &s[idx + sep.len()..]))
            } else {
                Ok(tuple_str3("", "", s))
            }
        }
        "casefold" => {
            // casefold: aggressive lowercase for caseless matching
            // Rust's to_lowercase handles most Unicode, but ß → ss needs explicit handling
            let folded: String = s
                .chars()
                .flat_map(|c| {
                    if c == '\u{00DF}' {
                        // ß
                        vec!['s', 's']
                    } else {
                        c.to_lowercase().collect::<Vec<_>>()
                    }
                })
                .collect();
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
                Ok(PyObject::str_val(CompactString::from(
                    &s[..s.len() - suffix.len()],
                )))
            } else {
                Ok(PyObject::str_val(CompactString::from(s)))
            }
        }
        "splitlines" => {
            let keepends_kwarg = extract_kwarg(args, "keepends");
            let pos = positional_args(args);
            check_args_range("splitlines", pos, 0, 1)?;
            let keepends = keepends_kwarg
                .as_ref()
                .or_else(|| pos.first())
                .map(|arg| arg.is_truthy())
                .unwrap_or(false);
            let mut lines = Vec::new();
            let mut start = 0;
            let bytes = s.as_bytes();
            let len = bytes.len();
            let mut i = 0;
            while i < len {
                if bytes[i] == b'\r' && i + 1 < len && bytes[i + 1] == b'\n' {
                    if keepends {
                        lines.push(PyObject::str_val(CompactString::from(&s[start..i + 2])));
                    } else {
                        lines.push(PyObject::str_val(CompactString::from(&s[start..i])));
                    }
                    i += 2;
                    start = i;
                } else if bytes[i] == b'\n' || bytes[i] == b'\r' {
                    if keepends {
                        lines.push(PyObject::str_val(CompactString::from(&s[start..i + 1])));
                    } else {
                        lines.push(PyObject::str_val(CompactString::from(&s[start..i])));
                    }
                    i += 1;
                    start = i;
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
            check_no_args("istitle", args)?;
            let mut prev_cased = false;
            let mut is_title = false;
            for c in s.chars() {
                if c.is_uppercase() {
                    if prev_cased {
                        return Ok(PyObject::bool_val(false));
                    }
                    prev_cased = true;
                    is_title = true;
                } else if c.is_lowercase() {
                    if !prev_cased {
                        return Ok(PyObject::bool_val(false));
                    }
                    prev_cased = true;
                } else {
                    prev_cased = false;
                }
            }
            Ok(PyObject::bool_val(is_title))
        }
        "isprintable" => Ok(PyObject::bool_val(
            !s.is_empty() && s.chars().all(|c| !c.is_control() || c == ' '),
        )),
        "isidentifier" => {
            let mut chars = s.chars();
            let valid = match chars.next() {
                Some(c) if c == '_' || c.is_alphabetic() => {
                    chars.all(|c| c == '_' || c.is_alphanumeric())
                }
                _ => false,
            };
            Ok(PyObject::bool_val(valid))
        }
        "isascii" => Ok(PyObject::bool_val(s.is_ascii())),
        "isdecimal" => Ok(PyObject::bool_val(
            !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()),
        )),
        "isnumeric" => Ok(PyObject::bool_val(
            !s.is_empty() && s.chars().all(|c| c.is_numeric()),
        )),
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
                            if c == '{' {
                                depth += 1;
                            } else if c == '}' {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            field_spec.push(c);
                        }
                        // Split field_spec: {field_name!conversion:format_spec}
                        let (field_part, format_spec) =
                            if let Some(colon_pos) = field_spec.find(':') {
                                (&field_spec[..colon_pos], Some(&field_spec[colon_pos + 1..]))
                            } else {
                                (field_spec.as_str(), None)
                            };
                        // Split field_part on '!' for conversion
                        let (field_name, conversion) = if let Some(bang_pos) = field_part.find('!')
                        {
                            (&field_part[..bang_pos], Some(&field_part[bang_pos + 1..]))
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
                            // Resolve nested {N} references after consuming the outer
                            // automatic field index, matching CPython's numbering.
                            let resolved_spec = format_spec.map(|spec| {
                                if spec.contains('{') {
                                    resolve_nested_spec(spec, args, &mut auto_idx)
                                } else {
                                    spec.to_string()
                                }
                            });
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
                return Err(PyException::type_error(
                    "maketrans() requires at least 1 argument",
                ));
            }
            let mut result_map = IndexMap::new();
            if args.len() == 1 {
                if let PyObjectPayload::Dict(map) = &args[0].payload {
                    for (k, v) in map.read().iter() {
                        let key = match k {
                            HashableKey::Int(n) => n.clone(),
                            HashableKey::Str(s) => {
                                if let Some(c) = s.chars().next() {
                                    PyInt::Small(c as i64)
                                } else {
                                    continue;
                                }
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
                    result_map.insert(
                        HashableKey::Int(PyInt::Small(cx as i64)),
                        PyObject::int(cy as i64),
                    );
                }
                if args.len() > 2 {
                    let z = args[2].py_to_string();
                    for cz in z.chars() {
                        result_map
                            .insert(HashableKey::Int(PyInt::Small(cz as i64)), PyObject::none());
                    }
                }
            }
            Ok(PyObject::dict(result_map))
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
                            if c == '}' {
                                break;
                            }
                            field.push(c);
                        }
                        let (field, conversion) = if let Some((name, conv)) = field.split_once('!')
                        {
                            (name.to_string(), Some(conv.to_string()))
                        } else {
                            (field, None)
                        };
                        // Look up field in mapping (dict subscript, not attribute)
                        let mut append_formatted = |val: &PyObjectRef| -> PyResult<()> {
                            match conversion.as_deref() {
                                Some("r") => result.push_str(&val.repr()),
                                Some("s") | None => result.push_str(&val.py_to_string()),
                                Some("a") => result.push_str(&val.repr()),
                                Some(other) => {
                                    return Err(PyException::value_error(format!(
                                        "Unknown conversion specifier {}",
                                        other
                                    )));
                                }
                            }
                            Ok(())
                        };
                        if let PyObjectPayload::Dict(m) = &mapping.payload {
                            let key = HashableKey::str_key(CompactString::from(field.as_str()));
                            let guard = m.read();
                            if let Some(val) = guard.get(&key) {
                                append_formatted(val)?;
                            } else {
                                // Support defaultdict: check for __defaultdict_factory__
                                let factory_key = HashableKey::str_key(CompactString::from(
                                    "__defaultdict_factory__",
                                ));
                                if let Some(factory) = guard.get(&factory_key).cloned() {
                                    drop(guard);
                                    let val = match &factory.payload {
                                        PyObjectPayload::NativeFunction(nf) => (nf.func)(&[])?,
                                        PyObjectPayload::NativeClosure(nc) => (nc.func)(&[])?,
                                        _ => return Err(PyException::key_error(field)),
                                    };
                                    m.write().insert(key, val.clone());
                                    append_formatted(&val)?;
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
                                        Some((nf.func)(&[mapping.clone(), key_obj])?)
                                    }
                                    PyObjectPayload::NativeClosure(nc) => {
                                        Some((nc.func)(&[mapping.clone(), key_obj])?)
                                    }
                                    PyObjectPayload::BoundMethod { receiver, method } => {
                                        match &method.payload {
                                            PyObjectPayload::NativeFunction(nf) => {
                                                Some((nf.func)(&[receiver.clone(), key_obj])?)
                                            }
                                            PyObjectPayload::NativeClosure(nc) => {
                                                Some((nc.func)(&[receiver.clone(), key_obj])?)
                                            }
                                            _ => None,
                                        }
                                    }
                                    _ => None,
                                }
                            } else {
                                None
                            };
                            if let Some(val) = resolved {
                                append_formatted(&val)?;
                            } else if let Some(val) = mapping.get_attr(&field) {
                                append_formatted(&val)?;
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
            let spec = if args.is_empty() {
                ""
            } else {
                args[0].as_str().unwrap_or("")
            };
            if spec.is_empty() {
                Ok(PyObject::str_val(CompactString::from(s)))
            } else {
                Ok(PyObject::str_val(CompactString::from(
                    apply_format_spec_str(s, spec),
                )))
            }
        }
        "__str__" => Ok(PyObject::str_val(CompactString::from(s))),
        "__repr__" => Ok(PyObject::str_val(CompactString::from(format!(
            "'{}'",
            s.replace('\\', "\\\\").replace('\'', "\\'")
        )))),
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
            Ok(if let Some(other) = args[0].as_str() {
                PyObject::bool_val(s == other)
            } else {
                PyObject::bool_val(false)
            })
        }
        "__ne__" => {
            check_args_min("str.__ne__", &args, 1)?;
            Ok(if let Some(other) = args[0].as_str() {
                PyObject::bool_val(s != other)
            } else {
                PyObject::bool_val(true)
            })
        }
        "__lt__" => {
            check_args_min("str.__lt__", &args, 1)?;
            Ok(if let Some(other) = args[0].as_str() {
                PyObject::bool_val(s < other)
            } else {
                PyObject::not_implemented()
            })
        }
        "__le__" => {
            check_args_min("str.__le__", &args, 1)?;
            Ok(if let Some(other) = args[0].as_str() {
                PyObject::bool_val(s <= other)
            } else {
                PyObject::not_implemented()
            })
        }
        "__gt__" => {
            check_args_min("str.__gt__", &args, 1)?;
            Ok(if let Some(other) = args[0].as_str() {
                PyObject::bool_val(s > other)
            } else {
                PyObject::not_implemented()
            })
        }
        "__ge__" => {
            check_args_min("str.__ge__", &args, 1)?;
            Ok(if let Some(other) = args[0].as_str() {
                PyObject::bool_val(s >= other)
            } else {
                PyObject::not_implemented()
            })
        }
        "__add__" => {
            check_args_min("str.__add__", &args, 1)?;
            let other = args[0].py_to_string();
            Ok(PyObject::str_val(CompactString::from(format!(
                "{}{}",
                s, other
            ))))
        }
        "__mul__" | "__rmul__" => {
            check_args_min("str.__mul__", &args, 1)?;
            let n = index_to_usize_repeat(&args[0])?;
            checked_repeat_len(s.len(), n, "str repeat")?;
            Ok(PyObject::str_val(CompactString::from(s.repeat(n))))
        }
        "__getitem__" => {
            check_args_min("str.__getitem__", &args, 1)?;
            if matches!(&args[0].payload, PyObjectPayload::Slice(_)) {
                return PyObject::str_val(CompactString::from(s)).get_item(&args[0]);
            }
            let idx = index_to_i64(&args[0])?;
            let chars: Vec<char> = s.chars().collect();
            let real_idx = if idx < 0 {
                chars.len() as i64 + idx
            } else {
                idx
            };
            if real_idx >= 0 && (real_idx as usize) < chars.len() {
                Ok(PyObject::str_val(CompactString::from(
                    chars[real_idx as usize].to_string(),
                )))
            } else {
                Err(PyException::index_error("string index out of range"))
            }
        }
        "__iter__" => {
            let chars: Vec<PyObjectRef> = s
                .chars()
                .map(|c| PyObject::str_val(CompactString::from(c.to_string())))
                .collect();
            Ok(PyObject::list(chars))
        }
        "__mod__" => {
            check_args_min("str.__mod__", &args, 1)?;
            PyObject::str_val(CompactString::from(s)).modulo(&args[0])
        }
        _ => Err(PyException::attribute_error(format!(
            "'str' object has no attribute '{}'",
            method
        ))),
    }
}
