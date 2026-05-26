use super::*;

pub(in crate::text_modules::regex_impl) fn ascii_escape_class(ch: char) -> Option<&'static str> {
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

pub(in crate::text_modules::regex_impl) fn normalize_future_set_ops(pattern: &str) -> String {
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

pub(in crate::text_modules::regex_impl) fn parse_decimal_saturating(chars: &[char]) -> Option<u64> {
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

pub(in crate::text_modules::regex_impl) fn parse_decimal_bytes_limited(
    bytes: &[u8],
    limit: u64,
) -> PyResult<Option<u64>> {
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

pub(in crate::text_modules::regex_impl) fn normalize_repeat_for_rust(
    chars: &[char],
    start: usize,
) -> Option<(String, usize)> {
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

pub(in crate::text_modules::regex_impl) fn parse_named_unicode_escape(
    chars: &[char],
    start: usize,
) -> Option<(char, usize)> {
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

pub(in crate::text_modules::regex_impl) fn convert_python_regex(
    pattern: &str,
    flags: i64,
) -> String {
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

pub(in crate::text_modules::regex_impl) fn convert_scoped_ascii_flags(
    pattern: &str,
    default_ascii: bool,
) -> String {
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
pub(in crate::text_modules::regex_impl) fn python_repl_to_rust(repl: &str) -> String {
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
