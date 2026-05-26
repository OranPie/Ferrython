//! SQL parsing and value matching helpers for sqlite3.

use indexmap::IndexMap;

use super::storage::DbValue;

// ── SQL Parser ─────────────────────────────────────────────────────────

pub(super) fn normalize_sql(sql: &str) -> String {
    // Collapse whitespace but preserve strings
    let mut result = String::new();
    let mut in_string = false;
    let mut string_char = ' ';
    let mut prev_space = false;
    for c in sql.chars() {
        if in_string {
            result.push(c);
            if c == string_char {
                in_string = false;
            }
            prev_space = false;
        } else if c == '\'' || c == '"' {
            in_string = true;
            string_char = c;
            result.push(c);
            prev_space = false;
        } else if c.is_whitespace() {
            if !prev_space && !result.is_empty() {
                result.push(' ');
                prev_space = true;
            }
        } else {
            result.push(c);
            prev_space = false;
        }
    }
    result.trim().to_string()
}

pub(super) fn split_respecting_parens(s: &str, delim: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut string_char = ' ';
    for c in s.chars() {
        if in_string {
            current.push(c);
            if c == string_char {
                in_string = false;
            }
        } else if c == '\'' || c == '"' {
            in_string = true;
            string_char = c;
            current.push(c);
        } else if c == '(' {
            depth += 1;
            current.push(c);
        } else if c == ')' {
            depth -= 1;
            current.push(c);
        } else if c == delim && depth == 0 {
            parts.push(current.trim().to_string());
            current = String::new();
        } else {
            current.push(c);
        }
    }
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        parts.push(trimmed);
    }
    parts
}

fn strip_quotes(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

pub(super) fn parse_value(s: &str) -> DbValue {
    let s = s.trim();
    if s.eq_ignore_ascii_case("NULL") || s.eq_ignore_ascii_case("None") {
        return DbValue::Null;
    }
    if (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')) {
        return DbValue::Text(s[1..s.len() - 1].to_string());
    }
    if let Ok(i) = s.parse::<i64>() {
        return DbValue::Int(i);
    }
    if let Ok(f) = s.parse::<f64>() {
        return DbValue::Float(f);
    }
    DbValue::Text(s.to_string())
}

pub(super) fn eval_where_condition(row: &IndexMap<String, DbValue>, condition: &str) -> bool {
    let condition = condition.trim();
    if condition.is_empty() {
        return true;
    }

    // Handle AND
    let lower = condition.to_lowercase();
    if let Some(idx) = find_keyword_pos(&lower, " and ") {
        let left = &condition[..idx];
        let right = &condition[idx + 5..];
        return eval_where_condition(row, left) && eval_where_condition(row, right);
    }

    // Handle OR
    if let Some(idx) = find_keyword_pos(&lower, " or ") {
        let left = &condition[..idx];
        let right = &condition[idx + 4..];
        return eval_where_condition(row, left) || eval_where_condition(row, right);
    }

    // Handle IS NOT NULL
    if lower.contains(" is not null") {
        let col = condition[..lower.find(" is not null").unwrap()].trim();
        return match row.get(col) {
            Some(DbValue::Null) | None => false,
            _ => true,
        };
    }

    // Handle IS NULL
    if lower.contains(" is null") {
        let col = condition[..lower.find(" is null").unwrap()].trim();
        return match row.get(col) {
            Some(DbValue::Null) | None => true,
            _ => false,
        };
    }

    // Handle LIKE
    if let Some(idx) = find_keyword_pos(&lower, " like ") {
        let col = condition[..idx].trim();
        let pattern = strip_quotes(condition[idx + 6..].trim());
        let col_val = match row.get(col) {
            Some(DbValue::Text(s)) => s.clone(),
            Some(DbValue::Int(i)) => i.to_string(),
            _ => return false,
        };
        return match_like(&col_val, &pattern);
    }

    // Handle operators: !=, >=, <=, >, <, =
    for op in &["!=", ">=", "<=", ">", "<", "="] {
        if let Some(idx) = condition.find(op) {
            let col = condition[..idx].trim();
            let val_str = condition[idx + op.len()..].trim();
            let val = if val_str == "?" {
                DbValue::Text("?".to_string())
            } else {
                parse_value(val_str)
            };
            let row_val = row.get(col);
            return compare_values(row_val, &val, op);
        }
    }

    true
}

fn find_keyword_pos(lower: &str, keyword: &str) -> Option<usize> {
    let mut in_string = false;
    let mut string_char = ' ';
    let bytes = lower.as_bytes();
    let kw_bytes = keyword.as_bytes();
    if bytes.len() < kw_bytes.len() {
        return None;
    }
    for i in 0..=bytes.len() - kw_bytes.len() {
        let c = bytes[i] as char;
        if in_string {
            if c == string_char {
                in_string = false;
            }
            continue;
        }
        if c == '\'' || c == '"' {
            in_string = true;
            string_char = c;
            continue;
        }
        if &bytes[i..i + kw_bytes.len()] == kw_bytes {
            return Some(i);
        }
    }
    None
}

fn match_like(value: &str, pattern: &str) -> bool {
    let pattern_lower = pattern.to_lowercase();
    let value_lower = value.to_lowercase();
    let parts: Vec<&str> = pattern_lower.split('%').collect();
    if parts.len() == 1 {
        return value_lower == pattern_lower;
    }
    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if let Some(found) = value_lower[pos..].find(part) {
            if i == 0 && found != 0 {
                return false;
            }
            pos += found + part.len();
        } else {
            return false;
        }
    }
    if !parts.last().unwrap_or(&"").is_empty() {
        return pos == value_lower.len();
    }
    true
}

fn compare_values(row_val: Option<&DbValue>, cmp_val: &DbValue, op: &str) -> bool {
    let row_val = match row_val {
        Some(v) => v,
        None => return op == "=" && matches!(cmp_val, DbValue::Null),
    };

    match (row_val, cmp_val) {
        (DbValue::Null, DbValue::Null) => op == "=",
        (DbValue::Null, _) | (_, DbValue::Null) => op == "!=",
        (DbValue::Int(a), DbValue::Int(b)) => match op {
            "=" => a == b,
            "!=" => a != b,
            ">" => a > b,
            "<" => a < b,
            ">=" => a >= b,
            "<=" => a <= b,
            _ => false,
        },
        (DbValue::Float(a), DbValue::Float(b)) => match op {
            "=" => a == b,
            "!=" => a != b,
            ">" => a > b,
            "<" => a < b,
            ">=" => a >= b,
            "<=" => a <= b,
            _ => false,
        },
        (DbValue::Int(a), DbValue::Float(b)) => {
            let a = *a as f64;
            match op {
                "=" => a == *b,
                "!=" => a != *b,
                ">" => a > *b,
                "<" => a < *b,
                ">=" => a >= *b,
                "<=" => a <= *b,
                _ => false,
            }
        }
        (DbValue::Float(a), DbValue::Int(b)) => {
            let b = *b as f64;
            match op {
                "=" => *a == b,
                "!=" => *a != b,
                ">" => *a > b,
                "<" => *a < b,
                ">=" => *a >= b,
                "<=" => *a <= b,
                _ => false,
            }
        }
        (DbValue::Text(a), DbValue::Text(b)) => match op {
            "=" => a == b,
            "!=" => a != b,
            ">" => a > b,
            "<" => a < b,
            ">=" => a >= b,
            "<=" => a <= b,
            _ => false,
        },
        (DbValue::Int(a), DbValue::Text(b)) => {
            let a_s = a.to_string();
            match op {
                "=" => a_s == *b,
                "!=" => a_s != *b,
                _ => false,
            }
        }
        (DbValue::Text(a), DbValue::Int(b)) => {
            if let Ok(a_i) = a.parse::<i64>() {
                match op {
                    "=" => a_i == *b,
                    "!=" => a_i != *b,
                    ">" => a_i > *b,
                    "<" => a_i < *b,
                    ">=" => a_i >= *b,
                    "<=" => a_i <= *b,
                    _ => false,
                }
            } else {
                op == "!="
            }
        }
        _ => false,
    }
}
