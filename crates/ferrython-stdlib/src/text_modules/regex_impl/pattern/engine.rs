use super::*;

pub(in crate::text_modules::regex_impl) fn leading_inline_flags(pattern: &str) -> i64 {
    split_leading_inline_flags(pattern).1
}

pub(in crate::text_modules::regex_impl) fn split_leading_inline_flags(
    pattern: &str,
) -> (&str, i64) {
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

pub(in crate::text_modules::regex_impl) fn anchor_pattern(pattern: &str, suffix: &str) -> String {
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

pub(in crate::text_modules::regex_impl) fn effective_re_flags(
    pattern: &str,
    flags: i64,
    is_bytes: bool,
) -> i64 {
    let mut effective = flags | leading_inline_flags(pattern);
    if !is_bytes && effective & RE_FLAG_ASCII == 0 {
        effective |= RE_FLAG_UNICODE;
    }
    effective
}

pub(in crate::text_modules::regex_impl) fn regex_engine_flags(flags: i64, is_bytes: bool) -> i64 {
    if is_bytes && flags & RE_FLAG_LOCALE == 0 {
        flags | RE_FLAG_ASCII
    } else {
        flags
    }
}

pub(in crate::text_modules::regex_impl) fn is_simple_nonboundary_pattern(pattern: &str) -> bool {
    split_leading_inline_flags(pattern).0 == r"\B"
}

pub(in crate::text_modules::regex_impl) fn re_flag_repr(
    flags: i64,
    is_bytes: bool,
) -> Option<String> {
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

pub(in crate::text_modules::regex_impl) fn needs_fancy_regex(pattern: &str) -> bool {
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
pub(in crate::text_modules::regex_impl) fn strip_verbose(pattern: &str) -> String {
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

pub(in crate::text_modules::regex_impl) fn build_regex(
    pattern: &str,
    flags: i64,
) -> Result<regex::Regex, PyException> {
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

pub(in crate::text_modules::regex_impl) fn build_fancy_regex(
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

pub(in crate::text_modules::regex_impl) fn fancy_find_all(
    re: &fancy_regex::Regex,
    text: &str,
) -> Vec<String> {
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

pub(in crate::text_modules::regex_impl) fn fancy_captures(
    re: &fancy_regex::Regex,
    text: &str,
) -> Vec<Vec<Option<String>>> {
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
pub(in crate::text_modules::regex_impl) fn extract_fancy_group_names(
    re: &fancy_regex::Regex,
) -> FxHashKeyMap {
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

pub(in crate::text_modules::regex_impl) fn regex_offset_to_py_index(
    text: &str,
    offset: usize,
    is_bytes: bool,
) -> i64 {
    let _ = is_bytes;
    text[..offset.min(text.len())].chars().count() as i64
}

pub(in crate::text_modules::regex_impl) fn py_index_to_regex_offset(
    text: &str,
    index: usize,
) -> usize {
    if index == 0 {
        return 0;
    }
    text.char_indices()
        .nth(index)
        .map(|(offset, _)| offset)
        .unwrap_or(text.len())
}

pub(in crate::text_modules::regex_impl) fn warn_nonleading_flags(
    pattern: &str,
    pattern_obj: &PyObjectRef,
) -> PyResult<()> {
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

pub(in crate::text_modules::regex_impl) fn strip_nonleading_global_flags(
    pattern: &str,
) -> (String, i64, bool) {
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

pub(in crate::text_modules::regex_impl) fn needs_fancy_regex_with_flags(
    pattern: &str,
    flags: i64,
) -> bool {
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
