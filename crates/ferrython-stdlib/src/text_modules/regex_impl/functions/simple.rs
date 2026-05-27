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

pub(in crate::text_modules::regex_impl) fn is_simple_dot_repeat_pattern(
    pattern: &str,
) -> PyResult<bool> {
    Ok(parse_simple_dot_repeat(pattern)?.is_some())
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

pub(in crate::text_modules::regex_impl) fn simple_dot_repeat_match(
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

pub(in crate::text_modules::regex_impl) fn simple_ascii_ignorecase_literal_match(
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
