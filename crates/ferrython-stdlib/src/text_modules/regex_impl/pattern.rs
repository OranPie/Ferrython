use super::*;

pub(super) fn is_re_pattern_object(obj: &PyObjectRef) -> bool {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        let attrs = inst.attrs.read();
        if attrs.contains_key("__re_pattern__") {
            return true;
        }
        if !attrs.contains_key("_pattern_text") {
            return false;
        }
        drop(attrs);
        if let PyObjectPayload::Class(cd) = &inst.class.payload {
            return cd.name.as_str() == "Pattern";
        }
    }
    false
}

pub(super) fn re_pattern_text_attr(obj: &PyObjectRef) -> Option<String> {
    if !is_re_pattern_object(obj) {
        return None;
    }
    obj.get_attr("_pattern_text").map(|v| v.py_to_string())
}

pub(super) fn re_pattern_is_bytes(obj: &PyObjectRef) -> bool {
    if is_re_pattern_object(obj) {
        return obj
            .get_attr("_pattern_is_bytes")
            .map(|v| v.is_truthy())
            .unwrap_or(false);
    }
    extract_bytes_like(obj).is_some()
}

pub(super) fn readonly_mapping(map: FxHashKeyMap) -> PyObjectRef {
    PyObject::wrap(PyObjectPayload::MappingProxy(Rc::new(PyCell::new(map))))
}

pub(super) fn bytes_to_regex_text(bytes: &[u8]) -> String {
    bytes.iter().map(|&byte| byte as char).collect()
}

pub(super) fn regex_text_to_bytes(text: &str) -> Vec<u8> {
    text.chars().map(|ch| ch as u32 as u8).collect()
}

pub(super) fn py_re_text(text: &str, is_bytes: bool) -> PyObjectRef {
    if is_bytes {
        PyObject::bytes(regex_text_to_bytes(text))
    } else {
        PyObject::str_val(CompactString::from(text))
    }
}

pub(super) fn extract_bytes_like(obj: &PyObjectRef) -> Option<Vec<u8>> {
    match &obj.payload {
        PyObjectPayload::Bytes(bytes) | PyObjectPayload::ByteArray(bytes) => {
            Some((**bytes).clone())
        }
        PyObjectPayload::Instance(inst) => {
            let next = {
                let attrs = inst.attrs.read();
                if attrs
                    .get("__array__")
                    .map(|flag| flag.is_truthy())
                    .unwrap_or(false)
                {
                    if let Some(data) = attrs.get("_data") {
                        if let PyObjectPayload::List(items) = &data.payload {
                            let bytes: Vec<u8> = items
                                .read()
                                .iter()
                                .filter_map(|item| item.to_int().ok().map(|value| value as u8))
                                .collect();
                            return Some(bytes);
                        }
                    }
                }
                attrs.get("__builtin_value__").cloned().or_else(|| {
                    if attrs
                        .get("__memoryview__")
                        .map(|flag| flag.is_truthy())
                        .unwrap_or(false)
                    {
                        attrs.get("obj").cloned()
                    } else {
                        None
                    }
                })
            };
            next.and_then(|value| extract_bytes_like(&value))
        }
        _ => None,
    }
}

pub(super) fn extract_str_like(obj: &PyObjectRef) -> Option<String> {
    match &obj.payload {
        PyObjectPayload::Str(_) => Some(obj.py_to_string()),
        PyObjectPayload::Instance(inst) => {
            let next = inst.attrs.read().get("__builtin_value__").cloned();
            next.and_then(|value| extract_str_like(&value))
        }
        _ => None,
    }
}

pub(super) fn extract_re_subject(obj: &PyObjectRef) -> PyResult<(String, bool)> {
    if let Some(bytes) = extract_bytes_like(obj) {
        return Ok((bytes_to_regex_text(&bytes), true));
    }
    if let Some(text) = extract_str_like(obj) {
        return Ok((text, false));
    }
    Err(PyException::type_error(
        "expected string or bytes-like object",
    ))
}

pub(super) fn ensure_re_compatible(
    pattern_obj: &PyObjectRef,
    subject_is_bytes: bool,
) -> PyResult<()> {
    if re_pattern_is_bytes(pattern_obj) != subject_is_bytes {
        return Err(PyException::type_error(
            "cannot use a string pattern on a bytes-like object",
        ));
    }
    Ok(())
}

pub(super) fn extract_re_replacement(
    obj: &PyObjectRef,
    subject_is_bytes: bool,
) -> PyResult<String> {
    if subject_is_bytes {
        if let Some(bytes) = extract_bytes_like(obj) {
            return Ok(bytes_to_regex_text(&bytes));
        }
        return Err(PyException::type_error(
            "sequence item must be bytes-like object",
        ));
    }
    if let Some(text) = extract_str_like(obj) {
        return Ok(text);
    }
    Err(PyException::type_error(
        "sequence item must be str instance",
    ))
}

/// Extract regex pattern string from either a str, bytes, or compiled Pattern.
/// For bytes, decodes as Latin-1 to preserve all byte values as chars.
pub(super) fn extract_re_pattern(obj: &PyObjectRef) -> PyResult<String> {
    if let Some(pattern) = re_pattern_text_attr(obj) {
        return Ok(pattern);
    }
    if let Some(bytes) = extract_bytes_like(obj) {
        return Ok(bytes_to_regex_text(&bytes));
    }
    if let Some(text) = extract_str_like(obj) {
        return Ok(text);
    }
    match &obj.payload {
        _ => Err(PyException::type_error(
            "first argument must be string or compiled pattern",
        )),
    }
}

pub(super) fn extract_re_pattern_and_flags(
    obj: &PyObjectRef,
    supplied_flags: i64,
) -> PyResult<(String, i64)> {
    let pattern = extract_re_pattern(obj)?;
    if is_re_pattern_object(obj) {
        if supplied_flags != 0 {
            return Err(PyException::value_error(
                "cannot process flags argument with a compiled pattern",
            ));
        }
        let flags = obj
            .get_attr("flags")
            .and_then(|f| f.to_int().ok())
            .unwrap_or(0);
        Ok((pattern, flags))
    } else {
        Ok((pattern, supplied_flags))
    }
}

pub(super) fn leading_inline_flags(pattern: &str) -> i64 {
    split_leading_inline_flags(pattern).1
}

pub(super) fn split_leading_inline_flags(pattern: &str) -> (&str, i64) {
    let bytes = pattern.as_bytes();
    if bytes.len() < 4 || bytes[0] != b'(' || bytes[1] != b'?' {
        return (pattern, 0);
    }
    let mut flags = 0;
    let mut i = 2;
    while i < bytes.len() {
        match bytes[i] {
            b'i' => flags |= RE_FLAG_IGNORECASE,
            b'L' => flags |= RE_FLAG_LOCALE,
            b'm' => flags |= RE_FLAG_MULTILINE,
            b's' => flags |= RE_FLAG_DOTALL,
            b'u' => flags |= RE_FLAG_UNICODE,
            b'x' => flags |= RE_FLAG_VERBOSE,
            b'a' => flags |= RE_FLAG_ASCII,
            b')' => return (&pattern[i + 1..], flags),
            b':' | b'-' => return (pattern, 0),
            _ => return (pattern, 0),
        }
        i += 1;
    }
    (pattern, 0)
}

pub(super) fn anchor_pattern(pattern: &str, suffix: &str) -> String {
    let (body, inline_flags) = split_leading_inline_flags(pattern);
    let mut anchored = format!("^(?:{}){}", body, suffix);
    if inline_flags != 0 {
        let mut prefix = String::from("(?");
        if inline_flags & RE_FLAG_IGNORECASE != 0 {
            prefix.push('i');
        }
        if inline_flags & RE_FLAG_LOCALE != 0 {
            prefix.push('L');
        }
        if inline_flags & RE_FLAG_MULTILINE != 0 {
            prefix.push('m');
        }
        if inline_flags & RE_FLAG_DOTALL != 0 {
            prefix.push('s');
        }
        if inline_flags & RE_FLAG_UNICODE != 0 {
            prefix.push('u');
        }
        if inline_flags & RE_FLAG_VERBOSE != 0 {
            prefix.push('x');
        }
        if inline_flags & RE_FLAG_ASCII != 0 {
            prefix.push('a');
        }
        prefix.push(')');
        anchored = format!("{}{}", prefix, anchored);
    }
    anchored
}

pub(super) fn effective_re_flags(pattern: &str, flags: i64, is_bytes: bool) -> i64 {
    let mut effective = flags | leading_inline_flags(pattern);
    if !is_bytes && effective & RE_FLAG_ASCII == 0 {
        effective |= RE_FLAG_UNICODE;
    }
    effective
}

pub(super) fn regex_engine_flags(flags: i64, is_bytes: bool) -> i64 {
    if is_bytes && flags & RE_FLAG_LOCALE == 0 {
        flags | RE_FLAG_ASCII
    } else {
        flags
    }
}

pub(super) fn is_simple_nonboundary_pattern(pattern: &str) -> bool {
    split_leading_inline_flags(pattern).0 == r"\B"
}

pub(super) fn re_flag_repr(flags: i64, is_bytes: bool) -> Option<String> {
    let mut remaining = if is_bytes {
        flags
    } else {
        flags & !RE_FLAG_UNICODE
    };
    let mut parts = Vec::new();
    for (bit, name) in [
        (RE_FLAG_IGNORECASE, "re.IGNORECASE"),
        (RE_FLAG_LOCALE, "re.LOCALE"),
        (RE_FLAG_MULTILINE, "re.MULTILINE"),
        (RE_FLAG_DOTALL, "re.DOTALL"),
        (RE_FLAG_VERBOSE, "re.VERBOSE"),
        (RE_FLAG_ASCII, "re.ASCII"),
        (RE_FLAG_TEMPLATE, "re.TEMPLATE"),
    ] {
        if remaining & bit != 0 {
            parts.push(name.to_string());
            remaining &= !bit;
        }
    }
    if remaining != 0 {
        parts.push(format!("0x{:x}", remaining));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("|"))
    }
}

pub(super) fn write_re_debug_output(pattern: &str) -> PyResult<()> {
    let text = re_debug_dump(pattern);
    let target = crate::get_stdout_override().or_else(crate::sys_modules::get_current_stdout);
    if let Some(target) = target {
        return write_text_to_file_object(&target, &text);
    }
    print!("{}", text);
    Ok(())
}

pub(super) fn re_debug_dump(pattern: &str) -> String {
    fn push_line(out: &mut String, indent: usize, text: impl AsRef<str>) {
        out.push_str(&"  ".repeat(indent));
        out.push_str(text.as_ref());
        out.push('\n');
    }

    fn dump_class(chars: &[char], i: &mut usize, indent: usize, out: &mut String) {
        push_line(out, indent, "IN");
        *i += 1;
        if *i < chars.len() && chars[*i] == '^' {
            push_line(out, indent + 1, "NEGATE");
            *i += 1;
        }
        while *i < chars.len() {
            match chars[*i] {
                ']' => {
                    *i += 1;
                    break;
                }
                '\\' if *i + 1 < chars.len() => {
                    push_line(out, indent + 1, format!("CATEGORY \\{}", chars[*i + 1]));
                    *i += 2;
                }
                ch => {
                    push_line(out, indent + 1, format!("LITERAL {}", ch as u32));
                    *i += 1;
                }
            }
        }
    }

    fn dump_until(
        chars: &[char],
        i: &mut usize,
        indent: usize,
        group_no: &mut usize,
        out: &mut String,
    ) {
        while *i < chars.len() {
            match chars[*i] {
                ')' => break,
                '|' => {
                    push_line(out, indent, "OR");
                    *i += 1;
                }
                '[' => dump_class(chars, i, indent, out),
                '\\' if *i + 1 < chars.len() => {
                    let esc = chars[*i + 1];
                    match esc {
                        'A' | 'Z' | 'b' | 'B' => push_line(out, indent, format!("AT \\{}", esc)),
                        'd' | 'D' | 's' | 'S' | 'w' | 'W' => {
                            push_line(out, indent, format!("CATEGORY \\{}", esc))
                        }
                        _ => push_line(out, indent, format!("LITERAL {}", esc as u32)),
                    }
                    *i += 2;
                }
                '(' if *i + 1 < chars.len() && chars[*i + 1] == '?' => {
                    if *i + 2 < chars.len() && chars[*i + 2] == '(' {
                        push_line(out, indent, "GROUPREF_EXISTS");
                        *i += 3;
                        while *i < chars.len() && chars[*i] != ')' {
                            *i += 1;
                        }
                        if *i < chars.len() {
                            *i += 1;
                        }
                    } else if *i + 2 < chars.len() && chars[*i + 2] == ':' {
                        *i += 3;
                        dump_until(chars, i, indent, group_no, out);
                        if *i < chars.len() && chars[*i] == ')' {
                            *i += 1;
                        }
                    } else {
                        let start = *i;
                        *i += 2;
                        while *i < chars.len() && chars[*i] != ':' && chars[*i] != ')' {
                            *i += 1;
                        }
                        if *i < chars.len() && chars[*i] == ':' {
                            let flags: String = chars[start + 2..*i].iter().collect();
                            push_line(out, indent, format!("FLAGS {}", flags));
                            *i += 1;
                            dump_until(chars, i, indent + 1, group_no, out);
                            if *i < chars.len() && chars[*i] == ')' {
                                *i += 1;
                            }
                        }
                    }
                }
                '(' => {
                    *group_no += 1;
                    push_line(out, indent, format!("SUBPATTERN {} 0 0", *group_no));
                    *i += 1;
                    dump_until(chars, i, indent + 1, group_no, out);
                    if *i < chars.len() && chars[*i] == ')' {
                        *i += 1;
                    }
                }
                '*' | '+' | '?' => {
                    push_line(out, indent, format!("REPEAT {}", chars[*i]));
                    *i += 1;
                }
                '{' => {
                    let start = *i;
                    while *i < chars.len() && chars[*i] != '}' {
                        *i += 1;
                    }
                    if *i < chars.len() {
                        *i += 1;
                    }
                    let repeat: String = chars[start..*i].iter().collect();
                    push_line(out, indent, format!("REPEAT {}", repeat));
                }
                '^' => {
                    push_line(out, indent, "AT AT_BEGINNING");
                    *i += 1;
                }
                '$' => {
                    push_line(out, indent, "AT AT_END");
                    *i += 1;
                }
                ch => {
                    push_line(out, indent, format!("LITERAL {}", ch as u32));
                    *i += 1;
                }
            }
        }
    }

    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    let mut group_no = 0;
    let mut out = String::new();
    dump_until(&chars, &mut i, 0, &mut group_no, &mut out);
    out.push('\n');
    out.push_str("SUCCESS\n");
    out
}

pub(super) fn write_text_to_file_object(target: &PyObjectRef, text: &str) -> PyResult<()> {
    if let Some(write_fn) = target.get_attr("write") {
        let text_obj = PyObject::str_val(CompactString::from(text));
        let bind_self = matches!(write_fn.payload, PyObjectPayload::NativeFunction(_))
            && matches!(target.payload, PyObjectPayload::Module(_))
            && target.get_attr("_bind_methods").is_some();
        if bind_self {
            ferrython_core::object::call_callable(&write_fn, &[target.clone(), text_obj])?;
        } else {
            ferrython_core::object::call_callable(&write_fn, &[text_obj])?;
        }
    } else {
        print!("{}", text);
    }
    Ok(())
}

pub(super) fn ascii_escape_class(ch: char) -> Option<&'static str> {
    match ch {
        's' => Some(r"[ \t\n\r\f\v]"),
        'S' => Some(r"[^ \t\n\r\f\v]"),
        'd' => Some(r"[0-9]"),
        'D' => Some(r"[^0-9]"),
        'w' => Some(r"[A-Za-z0-9_]"),
        'W' => Some(r"[^A-Za-z0-9_]"),
        _ => None,
    }
}

pub(super) fn normalize_future_set_ops(pattern: &str) -> String {
    if !(pattern.contains("--")
        || pattern.contains("&&")
        || pattern.contains("||")
        || pattern.contains("~~"))
    {
        return pattern.to_string();
    }
    let chars: Vec<char> = pattern.chars().collect();
    let mut result = String::with_capacity(pattern.len());
    let mut i = 0;
    let mut in_class = false;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            result.push(chars[i]);
            result.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if chars[i] == '[' && !in_class {
            in_class = true;
            result.push(chars[i]);
            i += 1;
            continue;
        }
        if chars[i] == ']' && in_class {
            in_class = false;
            result.push(chars[i]);
            i += 1;
            continue;
        }
        if in_class
            && i + 1 < chars.len()
            && chars[i] == chars[i + 1]
            && matches!(chars[i], '-' | '&' | '|' | '~')
        {
            let code = chars[i] as u32;
            result.push_str(&format!(r"\x{{{:x}}}\x{{{:x}}}", code, code));
            i += 2;
            continue;
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

pub(super) fn parse_decimal_saturating(chars: &[char]) -> Option<u64> {
    if chars.is_empty() || !chars.iter().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let mut value = 0_u64;
    for &ch in chars {
        value = value
            .saturating_mul(10)
            .saturating_add(ch.to_digit(10).unwrap_or(0) as u64);
    }
    Some(value)
}

pub(super) fn parse_decimal_bytes_limited(bytes: &[u8], limit: u64) -> PyResult<Option<u64>> {
    if bytes.is_empty() || !bytes.iter().all(|byte| byte.is_ascii_digit()) {
        return Ok(None);
    }
    let mut value = 0_u64;
    for &byte in bytes {
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add((byte - b'0') as u64))
            .ok_or_else(|| PyException::overflow_error("the repetition number is too large"))?;
        if value >= limit {
            return Err(PyException::overflow_error(
                "the repetition number is too large",
            ));
        }
    }
    Ok(Some(value))
}

pub(super) fn normalize_repeat_for_rust(chars: &[char], start: usize) -> Option<(String, usize)> {
    const REPEAT_COMPILE_LIMIT: u64 = 100_001;
    if chars.get(start) != Some(&'{') {
        return None;
    }
    let mut close = start + 1;
    while close < chars.len() && chars[close] != '}' {
        close += 1;
    }
    if close >= chars.len() {
        return None;
    }
    let body = &chars[start + 1..close];
    let comma = body.iter().position(|&ch| ch == ',');
    let (min, max, valid) = match comma {
        Some(pos) => {
            let left = &body[..pos];
            let right = &body[pos + 1..];
            let min = if left.is_empty() {
                Some(0)
            } else {
                parse_decimal_saturating(left)
            };
            let max = if right.is_empty() {
                None
            } else {
                parse_decimal_saturating(right)
            };
            (
                min,
                max,
                min.is_some() && (right.is_empty() || max.is_some()),
            )
        }
        None => {
            let value = parse_decimal_saturating(body);
            (value, value, value.is_some())
        }
    };
    if !valid {
        return None;
    }
    let min = min.unwrap_or(0);
    let mut end = close + 1;
    let lazy = end < chars.len() && chars[end] == '?';
    if lazy {
        end += 1;
    }
    let suffix = if lazy { "?" } else { "" };
    let normalized = match (comma, max) {
        (Some(_), Some(max)) if min == 0 && max > REPEAT_COMPILE_LIMIT => {
            format!("*{}", suffix)
        }
        (Some(_), None) if min > REPEAT_COMPILE_LIMIT => {
            format!("{{{}}}{}", REPEAT_COMPILE_LIMIT, suffix)
        }
        (Some(_), Some(max)) if min > REPEAT_COMPILE_LIMIT => {
            let capped = REPEAT_COMPILE_LIMIT.min(max);
            format!("{{{}}}{}", capped, suffix)
        }
        (Some(_), Some(max)) if max > REPEAT_COMPILE_LIMIT => {
            format!("{{{},}}{}", min, suffix)
        }
        (Some(_), Some(max)) => format!("{{{},{}}}{}", min, max, suffix),
        (Some(_), None) => format!("{{{},}}{}", min, suffix),
        (None, _) if min > REPEAT_COMPILE_LIMIT => {
            format!("{{{}}}{}", REPEAT_COMPILE_LIMIT, suffix)
        }
        (None, _) => format!("{{{}}}{}", min, suffix),
    };
    Some((normalized, end))
}

pub(super) fn parse_named_unicode_escape(chars: &[char], start: usize) -> Option<(char, usize)> {
    if start + 2 >= chars.len() || chars[start] != '\\' || chars[start + 1] != 'N' {
        return None;
    }
    if chars[start + 2] != '{' {
        return None;
    }
    let name_start = start + 3;
    let mut end = name_start;
    while end < chars.len() && chars[end] != '}' {
        end += 1;
    }
    if end >= chars.len() || end == name_start {
        return None;
    }
    let name: String = chars[name_start..end].iter().collect();
    unicode_lookup_name(&name).map(|ch| (ch, end + 1))
}

pub(super) fn convert_python_regex(pattern: &str, flags: i64) -> String {
    // Convert Python regex syntax to Rust regex syntax
    let normalized_pattern = normalize_future_set_ops(pattern);
    let normalized_pattern =
        convert_scoped_ascii_flags(&normalized_pattern, flags & RE_FLAG_ASCII != 0);
    let chars: Vec<char> = normalized_pattern.chars().collect();
    let mut result = String::with_capacity(normalized_pattern.len());
    let mut i = 0;
    let mut in_char_class = false;
    let ascii_mode = flags & 256 != 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            // Octal escapes apply both inside and outside char classes
            match chars[i + 1] {
                'N' => {
                    if let Some((ch, end)) = parse_named_unicode_escape(&chars, i) {
                        result.push(ch);
                        i = end;
                        continue;
                    }
                }
                '0'..='7' => {
                    let start = i + 1;
                    let mut end = start + 1;
                    // Consume up to 3 octal digits total (Python allows \0 through \377)
                    while end < chars.len()
                        && end < start + 3
                        && chars[end] >= '0'
                        && chars[end] <= '7'
                    {
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
            if ascii_mode {
                if let Some(class) = ascii_escape_class(chars[i + 1]) {
                    result.push_str(class);
                    i += 2;
                    continue;
                }
            }
            if !in_char_class {
                match chars[i + 1] {
                    'Z' => {
                        result.push_str("\\z");
                        i += 2;
                        continue;
                    }
                    'a' => {
                        result.push_str("\\x07");
                        i += 2;
                        continue;
                    } // Python \a = bell (BEL)
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
        } else if !in_char_class && chars[i] == '{' {
            if let Some((repeat, end)) = normalize_repeat_for_rust(&chars, i) {
                result.push_str(&repeat);
                i = end;
            } else if i + 1 < chars.len() && chars[i + 1] == '}' {
                // CPython treats an empty repeat marker as literal braces.
                result.push_str("\\{\\}");
                i += 2;
            } else {
                result.push(chars[i]);
                i += 1;
            }
        } else if !in_char_class && chars[i] == '(' && i + 1 < chars.len() && chars[i + 1] == '?' {
            // Convert conditional groups (?(N)yes|no) → (?:yes|no)
            if i + 2 < chars.len() && chars[i + 2] == '(' {
                let mut j = i + 3;
                while j < chars.len() && chars[j] != ')' {
                    j += 1;
                }
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

pub(super) fn convert_scoped_ascii_flags(pattern: &str, default_ascii: bool) -> String {
    fn parse_flags(chars: &[char], start: usize) -> Option<(usize, bool, String)> {
        let mut i = start;
        let mut ascii = None;
        let mut rust_flags = String::new();
        while i < chars.len() {
            match chars[i] {
                'a' => ascii = Some(true),
                'u' => ascii = Some(false),
                'L' => {}
                'i' | 'm' | 's' | 'x' | '-' => rust_flags.push(chars[i]),
                ':' => return Some((i, ascii.unwrap_or(false), rust_flags)),
                ')' => return None,
                _ => return None,
            }
            i += 1;
        }
        None
    }

    fn find_group_end(chars: &[char], start: usize) -> Option<usize> {
        let mut i = start;
        let mut depth = 1usize;
        let mut in_class = false;
        while i < chars.len() {
            match chars[i] {
                '\\' => i += 2,
                '[' if !in_class => {
                    in_class = true;
                    i += 1;
                }
                ']' if in_class => {
                    in_class = false;
                    i += 1;
                }
                '(' if !in_class => {
                    depth += 1;
                    i += 1;
                }
                ')' if !in_class => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                    i += 1;
                }
                _ => i += 1,
            }
        }
        None
    }

    fn push_escape(out: &mut String, esc: char, ascii: bool, in_class: bool) {
        if ascii {
            if in_class {
                match esc {
                    'w' => out.push_str("A-Za-z0-9_"),
                    'd' => out.push_str("0-9"),
                    's' => out.push_str(" \\t\\n\\r\\f\\v"),
                    _ => {
                        out.push('\\');
                        out.push(esc);
                    }
                }
            } else if let Some(class) = ascii_escape_class(esc) {
                out.push_str(class);
            } else {
                out.push('\\');
                out.push(esc);
            }
        } else {
            out.push('\\');
            out.push(esc);
        }
    }

    fn convert_range(chars: &[char], start: usize, end: usize, ascii: bool, out: &mut String) {
        let mut i = start;
        let mut in_class = false;
        while i < end {
            if chars[i] == '\\' && i + 1 < end {
                push_escape(out, chars[i + 1], ascii, in_class);
                i += 2;
            } else if chars[i] == '[' && !in_class {
                in_class = true;
                out.push('[');
                i += 1;
            } else if chars[i] == ']' && in_class {
                in_class = false;
                out.push(']');
                i += 1;
            } else if !in_class && chars[i] == '(' && i + 2 < end && chars[i + 1] == '?' {
                if let Some((colon, scoped_ascii, rust_flags)) = parse_flags(chars, i + 2) {
                    if let Some(close) = find_group_end(chars, colon + 1) {
                        if rust_flags.is_empty() {
                            out.push_str("(?:");
                        } else {
                            out.push_str("(?");
                            out.push_str(&rust_flags);
                            out.push(':');
                        }
                        convert_range(chars, colon + 1, close, scoped_ascii, out);
                        out.push(')');
                        i = close + 1;
                        continue;
                    }
                }
                out.push(chars[i]);
                i += 1;
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
    }

    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len());
    convert_range(&chars, 0, chars.len(), default_ascii, &mut out);
    out
}

/// Convert Python replacement string syntax to Rust regex syntax.
/// Python uses `\1`, `\2`, `\g<name>`, `\g<1>` for backreferences.
/// Rust regex uses `$1`, `$2`, `$name`, `${1}`.
pub(super) fn python_repl_to_rust(repl: &str) -> String {
    fn push_literal(result: &mut String, ch: char) {
        if ch == '$' {
            result.push_str("$$");
        } else {
            result.push(ch);
        }
    }

    let mut result = String::with_capacity(repl.len());
    let chars: Vec<char> = repl.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            let next = chars[i + 1];
            if next == '0' {
                let mut j = i + 1;
                let mut digits = String::new();
                while j < chars.len() && digits.len() < 3 && matches!(chars[j], '0'..='7') {
                    digits.push(chars[j]);
                    j += 1;
                }
                if let Ok(value) = u32::from_str_radix(&digits, 8) {
                    if let Some(ch) = char::from_u32(value) {
                        push_literal(&mut result, ch);
                    }
                }
                i = j;
            } else if matches!(next, '1'..='9') {
                if i + 3 < chars.len()
                    && matches!(chars[i + 1], '0'..='3')
                    && matches!(chars[i + 2], '0'..='7')
                    && matches!(chars[i + 3], '0'..='7')
                {
                    let digits: String = chars[i + 1..=i + 3].iter().collect();
                    if let Ok(value) = u32::from_str_radix(&digits, 8) {
                        if let Some(ch) = char::from_u32(value) {
                            push_literal(&mut result, ch);
                        }
                    }
                    i += 4;
                } else {
                    let mut j = i + 1;
                    let mut digits = String::new();
                    while j < chars.len() && digits.len() < 2 && chars[j].is_ascii_digit() {
                        digits.push(chars[j]);
                        j += 1;
                    }
                    result.push_str(&format!("${{{}}}", digits));
                    i = j;
                }
            } else if next == 'g' && i + 2 < chars.len() && chars[i + 2] == '<' {
                i += 3;
                let start = i;
                while i < chars.len() && chars[i] != '>' {
                    i += 1;
                }
                let group: String = chars[start..i].iter().collect();
                if i < chars.len() {
                    i += 1;
                }
                result.push_str(&format!("${{{}}}", group));
            } else if next == '\\' {
                result.push('\\');
                i += 2;
            } else {
                let literal = match next {
                    'a' => Some('\x07'),
                    'b' => Some('\x08'),
                    'f' => Some('\x0c'),
                    'n' => Some('\n'),
                    'r' => Some('\r'),
                    't' => Some('\t'),
                    'v' => Some('\x0b'),
                    _ => None,
                };
                if let Some(ch) = literal {
                    push_literal(&mut result, ch);
                } else {
                    result.push('\\');
                    result.push(next);
                }
                i += 2;
            }
        } else {
            push_literal(&mut result, chars[i]);
            i += 1;
        }
    }
    result
}

pub(super) fn group_count_from_pattern_obj(obj: &PyObjectRef) -> usize {
    obj.get_attr("groups")
        .and_then(|v| v.to_int().ok())
        .and_then(|v| usize::try_from(v).ok())
        .unwrap_or(0)
}

pub(super) fn groupindex_contains(obj: &PyObjectRef, name: &str) -> bool {
    obj.get_attr("groupindex")
        .and_then(|groupindex| {
            if let PyObjectPayload::MappingProxy(map) | PyObjectPayload::Dict(map) =
                &groupindex.payload
            {
                let key = HashableKey::str_key(CompactString::from(name));
                Some(map.read().contains_key(&key))
            } else {
                None
            }
        })
        .unwrap_or(false)
}

pub(super) fn re_error_with_pattern(
    message: impl Into<String>,
    pos: Option<usize>,
    pattern: Option<PyObjectRef>,
) -> PyException {
    let msg = message.into();
    let (lineno, colno) = if let (Some(pos), Some(pattern_obj)) = (pos, pattern.as_ref()) {
        let text = extract_re_pattern(pattern_obj).unwrap_or_else(|_| pattern_obj.py_to_string());
        let before = text.chars().take(pos).collect::<String>();
        let line = before.chars().filter(|&ch| ch == '\n').count() + 1;
        let col = before
            .rsplit_once('\n')
            .map(|(_, tail)| tail.chars().count() + 1)
            .unwrap_or_else(|| before.chars().count() + 1);
        (line, col)
    } else {
        (0, 0)
    };
    let display = match pos {
        Some(pos) if lineno > 1 => format!(
            "{} at position {} (line {}, column {})",
            msg, pos, lineno, colno
        ),
        Some(pos) => format!("{} at position {}", msg, pos),
        None => msg.clone(),
    };
    let mut attrs = ferrython_core::object::FxAttrMap::default();
    attrs.insert(
        CompactString::from("msg"),
        PyObject::str_val(CompactString::from(msg)),
    );
    attrs.insert(
        CompactString::from("pos"),
        pos.map(|p| PyObject::int(p as i64))
            .unwrap_or_else(PyObject::none),
    );
    attrs.insert(
        CompactString::from("pattern"),
        pattern.unwrap_or_else(PyObject::none),
    );
    attrs.insert(CompactString::from("lineno"), PyObject::int(lineno as i64));
    attrs.insert(CompactString::from("colno"), PyObject::int(colno as i64));
    let original = PyObject::wrap(PyObjectPayload::ExceptionInstance(
        std::mem::ManuallyDrop::new(Box::new(
            ferrython_core::object::ExceptionInstanceData::new_attrs(
                ExceptionKind::ReError,
                CompactString::from(display.clone()),
                vec![PyObject::str_val(CompactString::from(display.clone()))],
                Some(Rc::new(PyCell::new(attrs))),
            ),
        )),
    ));
    PyException::with_original(
        ExceptionKind::ReError,
        CompactString::from(display),
        original,
    )
}

pub(super) fn re_error(message: impl Into<String>, pos: Option<usize>) -> PyException {
    re_error_with_pattern(message, pos, None)
}

pub(super) fn re_pattern_error(
    message: impl Into<String>,
    pos: Option<usize>,
    pattern_obj: &PyObjectRef,
) -> PyException {
    re_error_with_pattern(message, pos, Some(pattern_obj.clone()))
}

pub(super) fn parse_decimal_limited(chars: &[char], limit: u64) -> Result<u64, ()> {
    let mut value = 0_u64;
    for &ch in chars {
        let digit = ch.to_digit(10).ok_or(())? as u64;
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(digit))
            .ok_or(())?;
        if value >= limit {
            return Err(());
        }
    }
    Ok(value)
}

pub(super) fn repeat_quantifier_end(
    chars: &[char],
    start: usize,
    pattern_obj: &PyObjectRef,
) -> PyResult<Option<usize>> {
    let len = chars.len();
    let mut end = match chars[start] {
        '*' | '+' | '?' => start + 1,
        '{' => {
            let mut close = start + 1;
            while close < len && chars[close] != '}' {
                close += 1;
            }
            if close >= len {
                return Ok(None);
            }
            let body = &chars[start + 1..close];
            let comma = body.iter().position(|&ch| ch == ',');
            let valid = match comma {
                Some(pos) => {
                    let left = &body[..pos];
                    let right = &body[pos + 1..];
                    (!left.is_empty() && left.iter().all(|ch| ch.is_ascii_digit()))
                        || (!right.is_empty() && right.iter().all(|ch| ch.is_ascii_digit()))
                }
                None => !body.is_empty() && body.iter().all(|ch| ch.is_ascii_digit()),
            };
            if !valid {
                return Ok(None);
            }
            let limit = u32::MAX as u64;
            let min = match comma {
                Some(0) => 0,
                Some(pos) => parse_decimal_limited(&body[..pos], limit).map_err(|_| {
                    PyException::overflow_error("the repetition number is too large")
                })?,
                None => parse_decimal_limited(body, limit).map_err(|_| {
                    PyException::overflow_error("the repetition number is too large")
                })?,
            };
            let max = match comma {
                Some(pos) if pos + 1 == body.len() => None,
                Some(pos) => {
                    Some(parse_decimal_limited(&body[pos + 1..], limit).map_err(|_| {
                        PyException::overflow_error("the repetition number is too large")
                    })?)
                }
                None => Some(min),
            };
            if let Some(max) = max {
                if min > max {
                    return Err(re_pattern_error(
                        "min repeat greater than max repeat",
                        Some(start + 1),
                        pattern_obj,
                    ));
                }
            }
            close + 1
        }
        _ => return Ok(None),
    };
    if end < len && chars[end] == '?' {
        end += 1;
    }
    Ok(Some(end))
}

pub(super) fn validate_escape(
    chars: &[char],
    start: usize,
    in_class: bool,
    is_bytes: bool,
    group_count: usize,
    open_captures: &[(usize, usize)],
    pattern_obj: &PyObjectRef,
) -> PyResult<usize> {
    if start + 1 >= chars.len() {
        return Err(re_pattern_error(
            "bad escape (end of pattern)",
            Some(start),
            pattern_obj,
        ));
    }
    let next = chars[start + 1];
    if is_bytes && matches!(next, 'u' | 'U' | 'N') {
        return Err(re_pattern_error(
            format!("bad escape \\{}", next),
            Some(start),
            pattern_obj,
        ));
    }
    match next {
        '0'..='7' => {
            let mut end = start + 1;
            while end < chars.len() && end < start + 4 && matches!(chars[end], '0'..='7') {
                end += 1;
            }
            let digits: String = chars[start + 1..end].iter().collect();
            if digits.len() == 3 {
                if let Ok(value) = u32::from_str_radix(&digits, 8) {
                    if value > 0o377 {
                        return Err(re_pattern_error(
                            format!("octal escape value \\{} outside of range 0-0o377", digits),
                            Some(start),
                            pattern_obj,
                        ));
                    }
                }
            }
            Ok(end)
        }
        '8' | '9' if in_class => Err(re_pattern_error(
            format!("bad escape \\{}", next),
            Some(start),
            pattern_obj,
        )),
        '1'..='9' => {
            let mut end = start + 1;
            while end < chars.len() && end < start + 3 && chars[end].is_ascii_digit() {
                end += 1;
            }
            let digits: String = chars[start + 1..end].iter().collect();
            let group = digits.parse::<usize>().unwrap_or(usize::MAX);
            if open_captures.iter().any(|&(_, n)| n == group) {
                return Err(re_pattern_error(
                    "cannot refer to an open group",
                    Some(start),
                    pattern_obj,
                ));
            }
            if group > group_count {
                return Err(re_pattern_error(
                    format!("invalid group reference {}", group),
                    Some(start + 1),
                    pattern_obj,
                ));
            }
            Ok(end)
        }
        'x' => {
            let end = (start + 4).min(chars.len());
            let ok = start + 3 < chars.len()
                && chars[start + 2].is_ascii_hexdigit()
                && chars[start + 3].is_ascii_hexdigit();
            if !ok {
                let mut frag_end = start + 2;
                while frag_end < chars.len()
                    && frag_end < start + 4
                    && chars[frag_end].is_ascii_hexdigit()
                {
                    frag_end += 1;
                }
                let fragment: String = chars[start..frag_end].iter().collect();
                return Err(re_pattern_error(
                    format!("incomplete escape {}", fragment),
                    Some(start),
                    pattern_obj,
                ));
            }
            Ok(end)
        }
        'u' | 'U' => {
            let needed = if next == 'u' { 4 } else { 8 };
            let end = start + 2 + needed;
            let ok = end <= chars.len()
                && chars[start + 2..end]
                    .iter()
                    .all(|ch| ch.is_ascii_hexdigit());
            if !ok {
                let mut frag_end = start + 2;
                while frag_end < chars.len()
                    && frag_end < end
                    && chars[frag_end].is_ascii_hexdigit()
                {
                    frag_end += 1;
                }
                let fragment: String = chars[start..frag_end].iter().collect();
                return Err(re_pattern_error(
                    format!("incomplete escape {}", fragment),
                    Some(start),
                    pattern_obj,
                ));
            }
            let digits: String = chars[start + 2..end].iter().collect();
            if u32::from_str_radix(&digits, 16).map_or(true, |value| value > 0x10ffff) {
                let fragment: String = chars[start..end].iter().collect();
                return Err(re_pattern_error(
                    format!("bad escape {}", fragment),
                    Some(start),
                    pattern_obj,
                ));
            }
            Ok(end)
        }
        'N' => {
            if start + 2 >= chars.len() || chars[start + 2] != '{' {
                return Err(re_pattern_error("missing {", Some(start + 2), pattern_obj));
            }
            let name_start = start + 3;
            if name_start >= chars.len() || chars[name_start] == '}' {
                return Err(re_pattern_error(
                    "missing character name",
                    Some(name_start),
                    pattern_obj,
                ));
            }
            let mut end = name_start;
            while end < chars.len() && chars[end] != '}' {
                end += 1;
            }
            if end >= chars.len() {
                return Err(re_pattern_error(
                    "missing }, unterminated name",
                    Some(name_start),
                    pattern_obj,
                ));
            }
            let name: String = chars[name_start..end].iter().collect();
            if unicode_lookup_name(&name).is_none() {
                return Err(re_pattern_error(
                    format!("undefined character name '{}'", name),
                    Some(start),
                    pattern_obj,
                ));
            }
            Ok(end + 1)
        }
        _ if next.is_ascii_alphabetic() => {
            let allowed = if in_class {
                "bBdDsSwWafnrtvxuUN"
            } else {
                "AbBdDsSwWZafnrtvxuUN"
            };
            if !allowed.contains(next) {
                return Err(re_pattern_error(
                    format!("bad escape \\{}", next),
                    Some(start),
                    pattern_obj,
                ));
            }
            Ok(start + 2)
        }
        _ => Ok(start + 2),
    }
}

pub(super) fn validate_character_class(
    chars: &[char],
    start: usize,
    is_bytes: bool,
    pattern_obj: &PyObjectRef,
) -> PyResult<usize> {
    let len = chars.len();
    let mut end = start + 1;
    if end < len && chars[end] == '^' {
        end += 1;
    }
    if end < len && chars[end] == ']' {
        end += 1;
    }
    while end < len {
        if chars[end] == '\\' {
            if end + 2 < len && chars[end + 1] == 'N' && chars[end + 2] == '{' {
                end += 3;
                while end < len && chars[end] != '}' {
                    end += 1;
                }
                if end < len {
                    end += 1;
                }
            } else {
                end = (end + 2).min(len);
            }
            continue;
        }
        if chars[end] == ']' {
            break;
        }
        end += 1;
    }
    if end >= len {
        let mut i = start + 1;
        while i < len {
            if chars[i] == '\\' {
                i = validate_escape(chars, i, true, is_bytes, 0, &[], pattern_obj)?;
            } else {
                i += 1;
            }
        }
        return Err(re_pattern_error(
            "unterminated character set",
            Some(start),
            pattern_obj,
        ));
    }
    let mut i = start + 1;
    while i < end {
        if chars[i] == '\\' {
            i = validate_escape(chars, i, true, is_bytes, 0, &[], pattern_obj)?;
        } else {
            i += 1;
        }
    }
    let body: String = chars[start + 1..end].iter().collect();
    if let Some(pos) = body.find("\\w-") {
        let after = body[pos + 3..].chars().next().unwrap_or(']');
        return Err(re_pattern_error(
            format!("bad character range \\w-{}", after),
            Some(start + 1 + pos),
            pattern_obj,
        ));
    }
    if let Some(pos) = body.find("-\\w") {
        let before = body[..pos].chars().next_back().unwrap_or('[');
        return Err(re_pattern_error(
            format!("bad character range {}-\\w", before),
            Some(start + 1 + pos.saturating_sub(1)),
            pattern_obj,
        ));
    }
    for i in start + 1..end.saturating_sub(2) {
        if chars[i + 1] == '-'
            && chars[i] != '\\'
            && chars[i + 2] != '\\'
            && chars[i + 2] != '-'
            && chars[i] > chars[i + 2]
        {
            return Err(re_pattern_error(
                format!("bad character range {}-{}", chars[i], chars[i + 2]),
                Some(i),
                pattern_obj,
            ));
        }
    }
    Ok(end + 1)
}

pub(super) fn validate_group_name(
    name: &str,
    pos: usize,
    pattern_obj: &PyObjectRef,
) -> PyResult<()> {
    if name.is_empty() {
        return Err(re_pattern_error(
            "missing group name",
            Some(pos),
            pattern_obj,
        ));
    }
    if !is_group_name(name) {
        return Err(re_pattern_error(
            format!("bad character in group name '{}'", name),
            Some(pos),
            pattern_obj,
        ));
    }
    Ok(())
}

pub(super) fn validate_re_pattern_syntax(
    pattern: &str,
    is_bytes: bool,
    pattern_obj: &PyObjectRef,
) -> PyResult<()> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let mut group_count = 0usize;
    let mut atom_available = false;
    let mut last_was_repeat = false;
    let mut i = 0usize;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                i = validate_escape(
                    &chars,
                    i,
                    false,
                    is_bytes,
                    group_count,
                    &groups,
                    pattern_obj,
                )?;
                atom_available = true;
                last_was_repeat = false;
            }
            '[' => {
                i = validate_character_class(&chars, i, is_bytes, pattern_obj)?;
                atom_available = true;
                last_was_repeat = false;
            }
            '(' => {
                if i + 1 < chars.len() && chars[i + 1] == '?' {
                    if i + 2 >= chars.len() {
                        return Err(re_pattern_error(
                            "unexpected end of pattern",
                            Some(i + 2),
                            pattern_obj,
                        ));
                    }
                    match chars[i + 2] {
                        '#' => {
                            let mut end = i + 3;
                            while end < chars.len() && chars[end] != ')' {
                                end += 1;
                            }
                            if end >= chars.len() {
                                return Err(re_pattern_error(
                                    "missing ), unterminated comment",
                                    Some(i),
                                    pattern_obj,
                                ));
                            }
                            i = end + 1;
                            atom_available = false;
                            last_was_repeat = false;
                        }
                        ':' => {
                            groups.push((i, 0));
                            i += 3;
                            atom_available = false;
                            last_was_repeat = false;
                        }
                        'P' => {
                            if i + 3 >= chars.len() {
                                return Err(re_pattern_error(
                                    "unexpected end of pattern",
                                    Some(i + 3),
                                    pattern_obj,
                                ));
                            }
                            match chars[i + 3] {
                                '<' => {
                                    let name_start = i + 4;
                                    let mut end = name_start;
                                    while end < chars.len() && chars[end] != '>' {
                                        end += 1;
                                    }
                                    if end >= chars.len() {
                                        return Err(re_pattern_error(
                                            "missing >, unterminated name",
                                            Some(name_start),
                                            pattern_obj,
                                        ));
                                    }
                                    let name: String = chars[name_start..end].iter().collect();
                                    validate_group_name(&name, name_start, pattern_obj)?;
                                    group_count += 1;
                                    groups.push((i, group_count));
                                    i = end + 1;
                                    atom_available = false;
                                    last_was_repeat = false;
                                }
                                '=' => {
                                    let name_start = i + 4;
                                    let mut end = name_start;
                                    while end < chars.len() && chars[end] != ')' {
                                        end += 1;
                                    }
                                    let name: String = chars[name_start..end].iter().collect();
                                    validate_group_name(&name, name_start, pattern_obj)?;
                                    i = end;
                                    atom_available = true;
                                    last_was_repeat = false;
                                }
                                other => {
                                    return Err(re_pattern_error(
                                        format!("unknown extension ?P{}", other),
                                        Some(i + 1),
                                        pattern_obj,
                                    ));
                                }
                            }
                        }
                        '<' => {
                            if i + 3 >= chars.len() {
                                return Err(re_pattern_error(
                                    "unexpected end of pattern",
                                    Some(i + 3),
                                    pattern_obj,
                                ));
                            }
                            if chars[i + 3] == '=' || chars[i + 3] == '!' {
                                groups.push((i, 0));
                                i += 4;
                                atom_available = false;
                                last_was_repeat = false;
                            } else {
                                let mut end = i + 3;
                                while end < chars.len() && chars[end] != ')' {
                                    end += 1;
                                }
                                let ext: String =
                                    chars[i + 1..end.min(chars.len())].iter().collect();
                                return Err(re_pattern_error(
                                    format!("unknown extension {}", ext),
                                    Some(i + 1),
                                    pattern_obj,
                                ));
                            }
                        }
                        '(' => {
                            let name_start = i + 3;
                            let mut end = name_start;
                            while end < chars.len() && chars[end] != ')' {
                                end += 1;
                            }
                            let name: String =
                                chars[name_start..end.min(chars.len())].iter().collect();
                            if name.is_empty() {
                                return Err(re_pattern_error(
                                    "missing group name",
                                    Some(name_start),
                                    pattern_obj,
                                ));
                            }
                            if name.chars().all(|ch| ch.is_ascii_digit()) {
                                let group = parse_decimal_limited(
                                    &chars[name_start..end],
                                    usize::MAX as u64,
                                )
                                .unwrap_or(usize::MAX as u64)
                                    as usize;
                                if group > group_count {
                                    return Err(re_pattern_error(
                                        format!("invalid group reference {}", group),
                                        Some(name_start),
                                        pattern_obj,
                                    ));
                                }
                            } else {
                                validate_group_name(&name, name_start, pattern_obj)?;
                            }
                            groups.push((i, 0));
                            i = if end < chars.len() { end + 1 } else { end };
                            atom_available = false;
                            last_was_repeat = false;
                        }
                        flag if matches!(flag, 'a' | 'i' | 'L' | 'm' | 's' | 'u' | 'x' | '-') => {
                            let mut end = i + 2;
                            while end < chars.len()
                                && matches!(
                                    chars[end],
                                    'a' | 'i' | 'L' | 'm' | 's' | 'u' | 'x' | '-'
                                )
                            {
                                end += 1;
                            }
                            if end >= chars.len() {
                                return Err(re_pattern_error(
                                    "missing -, : or )",
                                    Some(end),
                                    pattern_obj,
                                ));
                            }
                            match chars[end] {
                                ')' => {
                                    validate_inline_flag_set(
                                        &chars[i + 2..end],
                                        &[],
                                        i + 2,
                                        pattern_obj,
                                    )?;
                                    i = end + 1;
                                    atom_available = false;
                                    last_was_repeat = false;
                                }
                                ':' => {
                                    let (enabled, disabled) =
                                        split_inline_flag_parts(&chars[i + 2..end]);
                                    validate_inline_flag_set(
                                        enabled,
                                        disabled,
                                        i + 2,
                                        pattern_obj,
                                    )?;
                                    groups.push((i, 0));
                                    i = end + 1;
                                    atom_available = false;
                                    last_was_repeat = false;
                                }
                                _ => {
                                    return Err(re_pattern_error(
                                        "unknown flag",
                                        Some(end),
                                        pattern_obj,
                                    ));
                                }
                            }
                        }
                        other => {
                            return Err(re_pattern_error(
                                format!("unknown extension ?{}", other),
                                Some(i + 1),
                                pattern_obj,
                            ));
                        }
                    }
                } else {
                    group_count += 1;
                    groups.push((i, group_count));
                    i += 1;
                    atom_available = false;
                    last_was_repeat = false;
                }
            }
            ')' => {
                if groups.pop().is_none() {
                    return Err(re_pattern_error(
                        "unbalanced parenthesis",
                        Some(i),
                        pattern_obj,
                    ));
                }
                i += 1;
                atom_available = true;
                last_was_repeat = false;
            }
            '*' | '+' | '?' | '{' => {
                if let Some(end) = repeat_quantifier_end(&chars, i, pattern_obj)? {
                    if last_was_repeat {
                        return Err(re_pattern_error("multiple repeat", Some(i), pattern_obj));
                    }
                    if !atom_available {
                        return Err(re_pattern_error("nothing to repeat", Some(i), pattern_obj));
                    }
                    i = end;
                    atom_available = false;
                    last_was_repeat = true;
                } else {
                    i += 1;
                    atom_available = true;
                    last_was_repeat = false;
                }
            }
            '|' | '^' | '$' => {
                i += 1;
                atom_available = false;
                last_was_repeat = false;
            }
            _ => {
                i += 1;
                atom_available = true;
                last_was_repeat = false;
            }
        }
    }
    if let Some(&(pos, _)) = groups.first() {
        return Err(re_pattern_error(
            "missing ), unterminated subpattern",
            Some(pos),
            pattern_obj,
        ));
    }
    Ok(())
}

pub(super) fn split_inline_flag_parts<'a>(flags: &'a [char]) -> (&'a [char], &'a [char]) {
    if let Some(pos) = flags.iter().position(|&ch| ch == '-') {
        (&flags[..pos], &flags[pos + 1..])
    } else {
        (flags, &[])
    }
}

pub(super) fn validate_inline_flag_set(
    enabled: &[char],
    disabled: &[char],
    base_pos: usize,
    pattern_obj: &PyObjectRef,
) -> PyResult<()> {
    if enabled.is_empty() && !disabled.is_empty() {
        return Err(re_pattern_error(
            "missing flag",
            Some(base_pos + enabled.len() + 1),
            pattern_obj,
        ));
    }
    for (idx, flag) in enabled.iter().enumerate() {
        if !matches!(flag, 'a' | 'i' | 'L' | 'm' | 's' | 'u' | 'x') {
            return Err(re_pattern_error(
                "unknown flag",
                Some(base_pos + idx),
                pattern_obj,
            ));
        }
    }
    for (idx, flag) in disabled.iter().enumerate() {
        if !matches!(flag, 'i' | 'm' | 's' | 'x' | 'a' | 'u' | 'L') {
            return Err(re_pattern_error(
                "unknown flag",
                Some(base_pos + enabled.len() + 1 + idx),
                pattern_obj,
            ));
        }
        if matches!(flag, 'a' | 'u' | 'L') {
            return Err(re_pattern_error(
                "bad inline flags: cannot turn off flags 'a', 'u' and 'L'",
                Some(base_pos + enabled.len() + 1 + idx),
                pattern_obj,
            ));
        }
    }
    if enabled
        .iter()
        .any(|flag| disabled.iter().any(|disabled| disabled == flag))
    {
        let off_pos = disabled
            .iter()
            .position(|flag| enabled.iter().any(|enabled| enabled == flag))
            .unwrap_or(0);
        return Err(re_pattern_error(
            "bad inline flags: flag turned on and off",
            Some(base_pos + enabled.len() + 1 + off_pos),
            pattern_obj,
        ));
    }
    let mode_flags = enabled
        .iter()
        .filter(|&&flag| matches!(flag, 'a' | 'u' | 'L'))
        .count();
    if mode_flags > 1 {
        let pos = enabled
            .iter()
            .enumerate()
            .filter(|(_, &flag)| matches!(flag, 'a' | 'u' | 'L'))
            .nth(1)
            .map(|(idx, _)| base_pos + idx)
            .unwrap_or(base_pos);
        return Err(re_pattern_error(
            "bad inline flags: flags 'a', 'u' and 'L' are incompatible",
            Some(pos),
            pattern_obj,
        ));
    }
    Ok(())
}

pub(super) fn is_ascii_letter(ch: char) -> bool {
    ch.is_ascii_alphabetic()
}

pub(super) fn is_group_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_alphanumeric())
}

pub(super) fn validate_numeric_group(group: usize, group_count: usize, pos: usize) -> PyResult<()> {
    if group > group_count {
        Err(re_error(
            format!("invalid group reference {}", group),
            Some(pos),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn validate_replacement_template(repl: &str, pattern_obj: &PyObjectRef) -> PyResult<()> {
    let group_count = group_count_from_pattern_obj(pattern_obj);
    let chars: Vec<char> = repl.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '\\' {
            i += 1;
            continue;
        }
        let slash_pos = i;
        if i + 1 >= chars.len() {
            break;
        }
        let next = chars[i + 1];
        match next {
            'a' | 'b' | 'f' | 'n' | 'r' | 't' | 'v' | '\\' => {
                i += 2;
            }
            '0' => {
                let mut j = i + 1;
                let mut digits = String::new();
                while j < chars.len() && digits.len() < 3 && matches!(chars[j], '0'..='7') {
                    digits.push(chars[j]);
                    j += 1;
                }
                if digits.len() == 3 {
                    if let Ok(value) = u32::from_str_radix(&digits, 8) {
                        if value > 0o377 {
                            return Err(re_error(
                                format!("octal escape value \\{} outside of range 0-0o377", digits),
                                Some(slash_pos),
                            ));
                        }
                    }
                }
                i = j;
            }
            '1'..='9' => {
                if i + 3 < chars.len()
                    && matches!(chars[i + 1], '0'..='7')
                    && matches!(chars[i + 2], '0'..='7')
                    && matches!(chars[i + 3], '0'..='7')
                {
                    let digits: String = chars[i + 1..=i + 3].iter().collect();
                    if let Ok(value) = u32::from_str_radix(&digits, 8) {
                        if value > 0o377 {
                            return Err(re_error(
                                format!("octal escape value \\{} outside of range 0-0o377", digits),
                                Some(slash_pos),
                            ));
                        }
                    }
                    i += 4;
                    continue;
                }
                let mut j = i + 1;
                let mut digits = String::new();
                while j < chars.len() && digits.len() < 2 && chars[j].is_ascii_digit() {
                    digits.push(chars[j]);
                    j += 1;
                }
                let group = digits.parse::<usize>().unwrap_or(0);
                validate_numeric_group(group, group_count, slash_pos + 1)?;
                i = j;
            }
            'g' => {
                if i + 2 >= chars.len() || chars[i + 2] != '<' {
                    return Err(re_error("missing <", Some(slash_pos + 2)));
                }
                let name_start = i + 3;
                let mut j = name_start;
                while j < chars.len() && chars[j] != '>' {
                    j += 1;
                }
                if j >= chars.len() {
                    if name_start >= chars.len() {
                        return Err(re_error("missing group name", Some(name_start)));
                    }
                    return Err(re_error("missing >, unterminated name", Some(name_start)));
                }
                let name: String = chars[name_start..j].iter().collect();
                if name.is_empty() {
                    return Err(re_error("missing group name", Some(name_start)));
                }
                if name.chars().all(|ch| ch.is_ascii_digit()) {
                    let group = name.parse::<usize>().unwrap_or(usize::MAX);
                    validate_numeric_group(group, group_count, name_start)?;
                } else if !is_group_name(&name) {
                    return Err(re_error(
                        format!("bad character in group name '{}'", name),
                        Some(name_start),
                    ));
                } else if !groupindex_contains(pattern_obj, &name) {
                    return Err(PyException::index_error(format!(
                        "unknown group name '{}'",
                        name
                    )));
                }
                i = j + 1;
            }
            _ if is_ascii_letter(next) => {
                return Err(re_error(format!("bad escape \\{}", next), Some(slash_pos)));
            }
            _ => {
                i += 2;
            }
        }
    }
    Ok(())
}

pub(super) fn validate_replacement_for_pattern(
    pattern_obj: &PyObjectRef,
    flags: i64,
    repl: &str,
) -> PyResult<()> {
    if !repl.contains('\\') {
        return Ok(());
    }
    if is_re_pattern_object(pattern_obj) {
        validate_replacement_template(repl, pattern_obj)
    } else {
        let compiled = re_compile(&[pattern_obj.clone(), PyObject::int(flags)])?;
        validate_replacement_template(repl, &compiled)
    }
}

pub(super) fn needs_fancy_regex(pattern: &str) -> bool {
    // Detect lookahead/lookbehind which require fancy-regex
    let bytes = pattern.as_bytes();
    let len = bytes.len();
    for i in 0..len.saturating_sub(1) {
        if bytes[i] == b'(' && bytes[i + 1] == b'?' && i + 2 < len {
            match bytes[i + 2] {
                b'=' | b'!' => return true, // (?= (?!
                b'<' if i + 3 < len && (bytes[i + 3] == b'=' || bytes[i + 3] == b'!') => {
                    return true
                } // (?<= (?<!
                _ => {}
            }
        }
        if bytes[i] == b'\\' && i + 1 < len && bytes[i + 1].is_ascii_digit() && bytes[i + 1] != b'0'
        {
            return true;
        }
    }
    false
}

/// Strip VERBOSE (re.X) comments and unescaped whitespace from a regex pattern.
pub(super) fn strip_verbose(pattern: &str) -> String {
    let mut result = String::with_capacity(pattern.len());
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    let mut in_char_class = false;
    let mut verbose = true;
    let mut verbose_stack: Vec<bool> = Vec::new();
    'outer: while i < chars.len() {
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
        if ch == '(' && i + 2 < chars.len() && chars[i + 1] == '?' && chars[i + 2] == '#' {
            while i < chars.len() {
                result.push(chars[i]);
                if chars[i] == ')' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if ch == '(' && i + 1 < chars.len() && chars[i + 1] == '?' {
            let mut j = i + 2;
            let mut scoped_verbose = verbose;
            let mut negated = false;
            while j < chars.len() {
                match chars[j] {
                    'x' if negated => scoped_verbose = false,
                    'x' => scoped_verbose = true,
                    '-' => negated = true,
                    'a' | 'i' | 'L' | 'm' | 's' | 'u' => {}
                    ':' => {
                        for ch in &chars[i..=j] {
                            result.push(*ch);
                        }
                        verbose_stack.push(verbose);
                        verbose = scoped_verbose;
                        i = j + 1;
                        continue 'outer;
                    }
                    ')' => {
                        for ch in &chars[i..=j] {
                            result.push(*ch);
                        }
                        verbose = scoped_verbose;
                        i = j + 1;
                        continue 'outer;
                    }
                    _ => break,
                }
                j += 1;
            }
        }
        if ch == '(' {
            verbose_stack.push(verbose);
            result.push(ch);
            i += 1;
            continue;
        }
        if ch == ')' {
            if let Some(previous) = verbose_stack.pop() {
                verbose = previous;
            }
            result.push(ch);
            i += 1;
            continue;
        }
        if verbose && ch == '#' {
            // Skip to end of line
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            i += 1; // skip the newline too
            continue;
        }
        if verbose && ch.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        result.push(ch);
        i += 1;
    }
    result
}

pub(super) fn build_regex(pattern: &str, flags: i64) -> Result<regex::Regex, PyException> {
    let (body, inline_flags) = split_leading_inline_flags(pattern);
    let effective_flags = flags | inline_flags;
    let mut pat = if effective_flags & RE_FLAG_VERBOSE != 0 {
        strip_verbose(body)
    } else {
        body.to_string()
    };
    pat = convert_python_regex(&pat, effective_flags);
    let mut prefix = String::new();
    if effective_flags & RE_FLAG_IGNORECASE != 0 {
        prefix.push_str("(?i)");
    }
    if effective_flags & RE_FLAG_MULTILINE != 0 {
        prefix.push_str("(?m)");
    }
    if effective_flags & RE_FLAG_DOTALL != 0 {
        prefix.push_str("(?s)");
    }
    pat = format!("{}{}", prefix, pat);
    regex::Regex::new(&pat).map_err(|e| PyException::runtime_error(format!("re: {}", e)))
}

pub(super) fn build_fancy_regex(
    pattern: &str,
    flags: i64,
) -> Result<fancy_regex::Regex, PyException> {
    let (body, inline_flags) = split_leading_inline_flags(pattern);
    let effective_flags = flags | inline_flags;
    let mut pat = if effective_flags & RE_FLAG_VERBOSE != 0 {
        strip_verbose(body)
    } else {
        body.to_string()
    };
    pat = convert_python_regex(&pat, effective_flags);
    let mut prefix = String::new();
    if effective_flags & RE_FLAG_IGNORECASE != 0 {
        prefix.push_str("(?i)");
    }
    if effective_flags & RE_FLAG_MULTILINE != 0 {
        prefix.push_str("(?m)");
    }
    if effective_flags & RE_FLAG_DOTALL != 0 {
        prefix.push_str("(?s)");
    }
    pat = format!("{}{}", prefix, pat);
    fancy_regex::Regex::new(&pat).map_err(|e| PyException::runtime_error(format!("re: {}", e)))
}

pub(super) fn fancy_find_all(re: &fancy_regex::Regex, text: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut pos = 0;
    while pos <= text.len() {
        match re.find(&text[pos..]) {
            Ok(Some(m)) => {
                if m.start() == m.end() {
                    pos += 1;
                    continue;
                }
                results.push(m.as_str().to_string());
                pos += m.end();
            }
            _ => break,
        }
    }
    results
}

pub(super) fn fancy_captures(re: &fancy_regex::Regex, text: &str) -> Vec<Vec<Option<String>>> {
    let mut results = Vec::new();
    let mut pos = 0;
    while pos <= text.len() {
        match re.captures(&text[pos..]) {
            Ok(Some(caps)) => {
                let whole = caps.get(0).unwrap();
                if whole.start() == whole.end() {
                    pos += 1;
                    continue;
                }
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
pub(super) fn extract_fancy_group_names(re: &fancy_regex::Regex) -> FxHashKeyMap {
    let mut map = new_fx_hashkey_map();
    // fancy_regex exposes capture_names()
    for (idx, name_opt) in re.capture_names().enumerate() {
        if let Some(name) = name_opt {
            map.insert(
                HashableKey::str_key(CompactString::from(name)),
                PyObject::int(idx as i64),
            );
        }
    }
    map
}

pub(super) fn regex_offset_to_py_index(text: &str, offset: usize, is_bytes: bool) -> i64 {
    let _ = is_bytes;
    text[..offset.min(text.len())].chars().count() as i64
}

pub(super) fn py_index_to_regex_offset(text: &str, index: usize) -> usize {
    if index == 0 {
        return 0;
    }
    text.char_indices()
        .nth(index)
        .map(|(offset, _)| offset)
        .unwrap_or(text.len())
}

pub(super) fn warn_nonleading_flags(pattern: &str, pattern_obj: &PyObjectRef) -> PyResult<()> {
    let Some(pos) = pattern.find("(?i)") else {
        return Ok(());
    };
    if pos == 0 {
        return Ok(());
    }
    let display = if pattern.chars().count() > 40 {
        let prefix: String = pattern.chars().take(20).collect();
        format!(
            "Flags not at the start of the expression {} (truncated)",
            pattern_obj.repr_for_message(&prefix)
        )
    } else {
        format!(
            "Flags not at the start of the expression {}",
            pattern_obj.repr()
        )
    };
    if let Some(warnings) = crate::load_module("warnings") {
        if let (Some(warn_fn), Some(dep_cls)) = (
            warnings.get_attr("warn"),
            warnings.get_attr("DeprecationWarning"),
        ) {
            ferrython_core::object::call_callable(
                &warn_fn,
                &[PyObject::str_val(CompactString::from(display)), dep_cls],
            )?;
        }
    }
    Ok(())
}

trait ReprForWarning {
    fn repr_for_message(&self, text: &str) -> String;
}

impl ReprForWarning for PyObjectRef {
    fn repr_for_message(&self, text: &str) -> String {
        if extract_bytes_like(self).is_some() {
            PyObject::bytes(regex_text_to_bytes(text)).repr()
        } else {
            PyObject::str_val(CompactString::from(text)).repr()
        }
    }
}

pub(super) fn strip_nonleading_global_flags(pattern: &str) -> (String, i64, bool) {
    let chars: Vec<char> = pattern.chars().collect();
    let mut result = String::with_capacity(pattern.len());
    let mut flags = 0;
    let mut changed = false;
    let mut i = 0;
    while i < chars.len() {
        if i > 0 && i + 3 < chars.len() && chars[i] == '(' && chars[i + 1] == '?' {
            let mut j = i + 2;
            let mut seen = 0;
            while j < chars.len() {
                match chars[j] {
                    'i' => seen |= RE_FLAG_IGNORECASE,
                    'm' => seen |= RE_FLAG_MULTILINE,
                    's' => seen |= RE_FLAG_DOTALL,
                    'x' => seen |= RE_FLAG_VERBOSE,
                    'a' => seen |= RE_FLAG_ASCII,
                    'u' => seen |= RE_FLAG_UNICODE,
                    'L' => seen |= RE_FLAG_LOCALE,
                    ')' if seen != 0 => {
                        flags |= seen;
                        changed = true;
                        i = j + 1;
                        continue;
                    }
                    _ => break,
                }
                j += 1;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    (result, flags, changed)
}

pub(super) fn needs_fancy_regex_with_flags(pattern: &str, flags: i64) -> bool {
    // Check both original and verbose-stripped pattern
    if needs_fancy_regex(pattern) {
        return true;
    }
    if flags & 64 != 0 {
        let stripped = strip_verbose(pattern);
        if needs_fancy_regex(&stripped) {
            return true;
        }
    }
    false
}
