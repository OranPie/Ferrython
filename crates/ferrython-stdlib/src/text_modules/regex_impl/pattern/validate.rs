use super::*;

pub(in crate::text_modules::regex_impl) fn repeat_quantifier_end(
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

pub(in crate::text_modules::regex_impl) fn validate_escape(
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

pub(in crate::text_modules::regex_impl) fn validate_character_class(
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

pub(in crate::text_modules::regex_impl) fn validate_group_name(
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

pub(in crate::text_modules::regex_impl) fn validate_re_pattern_syntax(
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

pub(in crate::text_modules::regex_impl) fn split_inline_flag_parts<'a>(
    flags: &'a [char],
) -> (&'a [char], &'a [char]) {
    if let Some(pos) = flags.iter().position(|&ch| ch == '-') {
        (&flags[..pos], &flags[pos + 1..])
    } else {
        (flags, &[])
    }
}

pub(in crate::text_modules::regex_impl) fn validate_inline_flag_set(
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

pub(in crate::text_modules::regex_impl) fn is_ascii_letter(ch: char) -> bool {
    ch.is_ascii_alphabetic()
}

pub(in crate::text_modules::regex_impl) fn is_group_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch.is_alphanumeric())
}

pub(in crate::text_modules::regex_impl) fn validate_numeric_group(
    group: usize,
    group_count: usize,
    pos: usize,
) -> PyResult<()> {
    if group > group_count {
        Err(re_error(
            format!("invalid group reference {}", group),
            Some(pos),
        ))
    } else {
        Ok(())
    }
}

pub(in crate::text_modules::regex_impl) fn validate_replacement_template(
    repl: &str,
    pattern_obj: &PyObjectRef,
) -> PyResult<()> {
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

pub(in crate::text_modules::regex_impl) fn validate_replacement_for_pattern(
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
