//! Database stdlib modules: sqlite3 (in-memory dict-based implementation)

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin,
};
use indexmap::IndexMap;
use std::sync::{Arc, Mutex};

// ── Database storage ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Column {
    name: String,
    col_type: String,
    primary_key: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Table {
    name: String,
    columns: Vec<Column>,
    rows: Vec<IndexMap<String, DbValue>>,
    auto_increment: i64,
}

#[derive(Debug, Clone)]
enum DbValue {
    Null,
    Int(i64),
    Float(f64),
    Text(String),
}

impl DbValue {
    fn to_pyobject(&self) -> PyObjectRef {
        match self {
            DbValue::Null => PyObject::none(),
            DbValue::Int(i) => PyObject::int(*i),
            DbValue::Float(f) => PyObject::float(*f),
            DbValue::Text(s) => PyObject::str_val(CompactString::from(s.as_str())),
        }
    }

    fn from_pyobject(obj: &PyObjectRef) -> Self {
        match &obj.payload {
            PyObjectPayload::None => DbValue::Null,
            PyObjectPayload::Int(n) => DbValue::Int(n.to_i64().unwrap_or(0)),
            PyObjectPayload::Float(f) => DbValue::Float(*f),
            PyObjectPayload::Bool(b) => DbValue::Int(if *b { 1 } else { 0 }),
            PyObjectPayload::Str(s) => DbValue::Text(s.to_string()),
            _ => DbValue::Text(obj.py_to_string()),
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct Database {
    tables: IndexMap<String, Table>,
    path: String,
    closed: bool,
}

impl Database {
    fn new(path: &str) -> Self {
        Self {
            tables: IndexMap::new(),
            path: path.to_string(),
            closed: false,
        }
    }
}

// ── SQL Parser ─────────────────────────────────────────────────────────

fn normalize_sql(sql: &str) -> String {
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

fn split_respecting_parens(s: &str, delim: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut string_char = ' ';
    for c in s.chars() {
        if in_string {
            current.push(c);
            if c == string_char { in_string = false; }
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
        s[1..s.len()-1].to_string()
    } else {
        s.to_string()
    }
}

fn parse_value(s: &str) -> DbValue {
    let s = s.trim();
    if s.eq_ignore_ascii_case("NULL") || s.eq_ignore_ascii_case("None") {
        return DbValue::Null;
    }
    if (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')) {
        return DbValue::Text(s[1..s.len()-1].to_string());
    }
    if let Ok(i) = s.parse::<i64>() {
        return DbValue::Int(i);
    }
    if let Ok(f) = s.parse::<f64>() {
        return DbValue::Float(f);
    }
    DbValue::Text(s.to_string())
}

fn eval_where_condition(row: &IndexMap<String, DbValue>, condition: &str) -> bool {
    let condition = condition.trim();
    if condition.is_empty() {
        return true;
    }

    // Handle AND
    let lower = condition.to_lowercase();
    if let Some(idx) = find_keyword_pos(&lower, " and ") {
        let left = &condition[..idx];
        let right = &condition[idx+5..];
        return eval_where_condition(row, left) && eval_where_condition(row, right);
    }

    // Handle OR
    if let Some(idx) = find_keyword_pos(&lower, " or ") {
        let left = &condition[..idx];
        let right = &condition[idx+4..];
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
        let pattern = strip_quotes(condition[idx+6..].trim());
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
            let val_str = condition[idx+op.len()..].trim();
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
    if bytes.len() < kw_bytes.len() { return None; }
    for i in 0..=bytes.len() - kw_bytes.len() {
        let c = bytes[i] as char;
        if in_string {
            if c == string_char { in_string = false; }
            continue;
        }
        if c == '\'' || c == '"' {
            in_string = true;
            string_char = c;
            continue;
        }
        if &bytes[i..i+kw_bytes.len()] == kw_bytes {
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
        if part.is_empty() { continue; }
        if let Some(found) = value_lower[pos..].find(part) {
            if i == 0 && found != 0 { return false; }
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
            "=" => a == b, "!=" => a != b, ">" => a > b, "<" => a < b, ">=" => a >= b, "<=" => a <= b,
            _ => false,
        },
        (DbValue::Float(a), DbValue::Float(b)) => match op {
            "=" => a == b, "!=" => a != b, ">" => a > b, "<" => a < b, ">=" => a >= b, "<=" => a <= b,
            _ => false,
        },
        (DbValue::Int(a), DbValue::Float(b)) => { let a = *a as f64; match op {
            "=" => a == *b, "!=" => a != *b, ">" => a > *b, "<" => a < *b, ">=" => a >= *b, "<=" => a <= *b,
            _ => false,
        }},
        (DbValue::Float(a), DbValue::Int(b)) => { let b = *b as f64; match op {
            "=" => *a == b, "!=" => *a != b, ">" => *a > b, "<" => *a < b, ">=" => *a >= b, "<=" => *a <= b,
            _ => false,
        }},
        (DbValue::Text(a), DbValue::Text(b)) => match op {
            "=" => a == b, "!=" => a != b, ">" => a > b, "<" => a < b, ">=" => a >= b, "<=" => a <= b,
            _ => false,
        },
        (DbValue::Int(a), DbValue::Text(b)) => {
            let a_s = a.to_string();
            match op { "=" => a_s == *b, "!=" => a_s != *b, _ => false }
        },
        (DbValue::Text(a), DbValue::Int(b)) => {
            if let Ok(a_i) = a.parse::<i64>() {
                match op { "=" => a_i == *b, "!=" => a_i != *b, ">" => a_i > *b, "<" => a_i < *b, ">=" => a_i >= *b, "<=" => a_i <= *b, _ => false }
            } else {
                op == "!="
            }
        },
        _ => false,
    }
}

// ── SQL execution ──────────────────────────────────────────────────────

struct QueryResult {
    rows: Vec<Vec<DbValue>>,
    columns: Vec<String>,
    rowcount: i64,
    lastrowid: i64,
}

fn execute_sql(db: &mut Database, sql: &str, params: &[PyObjectRef]) -> PyResult<QueryResult> {
    let sql = normalize_sql(sql);
    let sql = substitute_params(&sql, params);
    let upper = sql.to_uppercase();
    let upper = upper.trim();

    if upper.starts_with("CREATE TABLE") {
        execute_create_table(db, &sql)
    } else if upper.starts_with("INSERT INTO") || upper.starts_with("INSERT OR") {
        execute_insert(db, &sql)
    } else if upper.starts_with("SELECT") {
        execute_select(db, &sql)
    } else if upper.starts_with("UPDATE") {
        execute_update(db, &sql)
    } else if upper.starts_with("DELETE") {
        execute_delete(db, &sql)
    } else if upper.starts_with("DROP TABLE") {
        execute_drop_table(db, &sql)
    } else if upper.starts_with("BEGIN") || upper.starts_with("COMMIT") || upper.starts_with("ROLLBACK") {
        Ok(QueryResult { rows: vec![], columns: vec![], rowcount: 0, lastrowid: 0 })
    } else if upper.starts_with("PRAGMA") {
        Ok(QueryResult { rows: vec![], columns: vec![], rowcount: 0, lastrowid: 0 })
    } else {
        Err(PyException::new(
            ferrython_core::error::ExceptionKind::RuntimeError,
            format!("unsupported SQL: {}", sql),
        ))
    }
}

fn substitute_params(sql: &str, params: &[PyObjectRef]) -> String {
    if params.is_empty() { return sql.to_string(); }
    let mut result = String::new();
    let mut param_idx = 0;
    let mut in_string = false;
    let mut string_char = ' ';
    for c in sql.chars() {
        if in_string {
            result.push(c);
            if c == string_char { in_string = false; }
        } else if c == '\'' || c == '"' {
            in_string = true;
            string_char = c;
            result.push(c);
        } else if c == '?' && param_idx < params.len() {
            let val = DbValue::from_pyobject(&params[param_idx]);
            match val {
                DbValue::Null => result.push_str("NULL"),
                DbValue::Int(i) => result.push_str(&i.to_string()),
                DbValue::Float(f) => result.push_str(&f.to_string()),
                DbValue::Text(s) => {
                    result.push('\'');
                    result.push_str(&s.replace('\'', "''"));
                    result.push('\'');
                }
            }
            param_idx += 1;
        } else {
            result.push(c);
        }
    }
    result
}

fn execute_create_table(db: &mut Database, sql: &str) -> PyResult<QueryResult> {
    let upper = sql.to_uppercase();
    let if_not_exists = upper.contains("IF NOT EXISTS");

    // Extract table name and columns
    let paren_start = sql.find('(').ok_or_else(|| PyException::new(
        ferrython_core::error::ExceptionKind::RuntimeError, "malformed CREATE TABLE"))?;
    let paren_end = sql.rfind(')').ok_or_else(|| PyException::new(
        ferrython_core::error::ExceptionKind::RuntimeError, "malformed CREATE TABLE"))?;

    let before_paren = &sql[..paren_start].trim();
    let words: Vec<&str> = before_paren.split_whitespace().collect();
    let table_name = if if_not_exists {
        words.last().unwrap_or(&"").to_string()
    } else {
        // CREATE TABLE name
        words.last().unwrap_or(&"").to_string()
    };

    if if_not_exists && db.tables.contains_key(&table_name) {
        return Ok(QueryResult { rows: vec![], columns: vec![], rowcount: 0, lastrowid: 0 });
    }

    if db.tables.contains_key(&table_name) {
        return Err(PyException::new(
            ferrython_core::error::ExceptionKind::RuntimeError,
            format!("table {} already exists", table_name),
        ));
    }

    let cols_str = &sql[paren_start+1..paren_end];
    let col_defs = split_respecting_parens(cols_str, ',');
    let mut columns = Vec::new();
    for def in &col_defs {
        let def = def.trim();
        let def_upper = def.to_uppercase();
        // Skip constraints like PRIMARY KEY(...), UNIQUE(...), FOREIGN KEY(...)
        if def_upper.starts_with("PRIMARY KEY") || def_upper.starts_with("UNIQUE")
            || def_upper.starts_with("FOREIGN KEY") || def_upper.starts_with("CHECK")
            || def_upper.starts_with("CONSTRAINT") {
            continue;
        }
        let parts: Vec<&str> = def.split_whitespace().collect();
        if parts.is_empty() { continue; }
        let name = parts[0].to_string();
        let col_type = if parts.len() > 1 { parts[1].to_uppercase() } else { "TEXT".to_string() };
        let is_pk = def_upper.contains("PRIMARY KEY");
        columns.push(Column { name, col_type, primary_key: is_pk });
    }

    db.tables.insert(table_name.clone(), Table {
        name: table_name,
        columns,
        rows: Vec::new(),
        auto_increment: 1,
    });

    Ok(QueryResult { rows: vec![], columns: vec![], rowcount: 0, lastrowid: 0 })
}

fn execute_insert(db: &mut Database, sql: &str) -> PyResult<QueryResult> {
    let upper = sql.to_uppercase();

    // Find table name - after INTO
    let into_pos = upper.find("INTO").ok_or_else(|| PyException::new(
        ferrython_core::error::ExceptionKind::RuntimeError, "malformed INSERT"))?;
    let after_into = sql[into_pos+4..].trim();

    // Table name is first word
    let table_end = after_into.find(|c: char| c == '(' || c.is_whitespace()).unwrap_or(after_into.len());
    let table_name = after_into[..table_end].trim().to_string();

    let table = db.tables.get(&table_name).ok_or_else(|| PyException::new(
        ferrython_core::error::ExceptionKind::RuntimeError,
        format!("no such table: {}", table_name),
    ))?.clone();

    let rest = after_into[table_end..].trim();

    // Extract column names if provided
    let (col_names, values_str) = if rest.starts_with('(') {
        let close = rest.find(')').ok_or_else(|| PyException::new(
            ferrython_core::error::ExceptionKind::RuntimeError, "malformed INSERT: missing )"))?;
        let cols: Vec<String> = rest[1..close].split(',').map(|s| s.trim().to_string()).collect();
        let after = rest[close+1..].trim();
        (cols, after.to_string())
    } else {
        let cols: Vec<String> = table.columns.iter().map(|c| c.name.clone()).collect();
        (cols, rest.to_string())
    };

    // Find VALUES(...)
    let values_upper = values_str.to_uppercase();
    let vals_pos = values_upper.find("VALUES").ok_or_else(|| PyException::new(
        ferrython_core::error::ExceptionKind::RuntimeError, "malformed INSERT: missing VALUES"))?;
    let after_values = values_str[vals_pos+6..].trim();

    // Could be multiple value tuples
    let mut row_strings = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut in_string = false;
    let mut string_char = ' ';
    for (i, c) in after_values.chars().enumerate() {
        if in_string {
            if c == string_char { in_string = false; }
        } else if c == '\'' || c == '"' {
            in_string = true;
            string_char = c;
        } else if c == '(' {
            if depth == 0 { start = i + 1; }
            depth += 1;
        } else if c == ')' {
            depth -= 1;
            if depth == 0 {
                row_strings.push(after_values[start..i].to_string());
            }
        }
    }

    let table = db.tables.get_mut(&table_name).unwrap();
    let mut lastrowid = 0i64;
    let mut rowcount = 0i64;

    for row_str in &row_strings {
        let values = split_respecting_parens(row_str, ',');
        let mut row = IndexMap::new();

        // Fill defaults
        for col in &table.columns {
            if col.primary_key && col.col_type.contains("INTEGER") {
                row.insert(col.name.clone(), DbValue::Int(table.auto_increment));
            } else {
                row.insert(col.name.clone(), DbValue::Null);
            }
        }

        for (i, val_str) in values.iter().enumerate() {
            if i < col_names.len() {
                let col_name = &col_names[i];
                let val = parse_value(val_str.trim());
                row.insert(col_name.clone(), val);
            }
        }

        // Handle auto-increment for integer primary key
        for col in &table.columns {
            if col.primary_key && col.col_type.contains("INTEGER") {
                if let Some(DbValue::Int(id)) = row.get(&col.name) {
                    if *id >= table.auto_increment {
                        table.auto_increment = *id + 1;
                    }
                    lastrowid = row.get(&col.name).map(|v| match v { DbValue::Int(i) => *i, _ => 0 }).unwrap_or(0);
                } else {
                    row.insert(col.name.clone(), DbValue::Int(table.auto_increment));
                    lastrowid = table.auto_increment;
                    table.auto_increment += 1;
                }
            }
        }

        table.rows.push(row);
        rowcount += 1;
    }

    if lastrowid == 0 {
        lastrowid = table.rows.len() as i64;
    }

    Ok(QueryResult { rows: vec![], columns: vec![], rowcount, lastrowid })
}

fn execute_select(db: &Database, sql: &str) -> PyResult<QueryResult> {
    let upper = sql.to_uppercase();

    // Extract FROM table
    let from_pos = upper.find(" FROM ").ok_or_else(|| PyException::new(
        ferrython_core::error::ExceptionKind::RuntimeError, "malformed SELECT: missing FROM"))?;

    let select_cols_str = sql[6..from_pos].trim(); // after SELECT
    let after_from = sql[from_pos+6..].trim();

    // Table name
    let table_end = after_from.find(|c: char| c.is_whitespace()).unwrap_or(after_from.len());
    let table_name = after_from[..table_end].trim().to_string();
    let rest = after_from[table_end..].trim();

    let table = db.tables.get(&table_name).ok_or_else(|| PyException::new(
        ferrython_core::error::ExceptionKind::RuntimeError,
        format!("no such table: {}", table_name),
    ))?;

    // WHERE clause
    let rest_upper = rest.to_uppercase();
    let (where_clause, rest_after_where) = if let Some(w_pos) = rest_upper.find("WHERE ") {
        let after_where = &rest[w_pos+6..];
        // Find end (ORDER BY, LIMIT, GROUP BY, or end)
        let end_pos = find_clause_end(&after_where.to_uppercase());
        let wc = after_where[..end_pos].trim().to_string();
        let remaining = after_where[end_pos..].trim().to_string();
        (wc, remaining)
    } else {
        (String::new(), rest.to_string());
        let rau = rest.to_uppercase();
        let end_pos = if rau.starts_with("ORDER") || rau.starts_with("LIMIT") || rau.starts_with("GROUP") {
            0
        } else {
            rest.len()
        };
        (String::new(), rest[end_pos.min(rest.len())..].to_string())
    };

    // Determine columns to select
    let select_cols: Vec<String> = if select_cols_str.trim() == "*" {
        table.columns.iter().map(|c| c.name.clone()).collect()
    } else {
        select_cols_str.split(',').map(|s| {
            let s = s.trim();
            // Handle COUNT(*), etc.
            s.to_string()
        }).collect()
    };

    // Filter rows
    let mut result_rows: Vec<&IndexMap<String, DbValue>> = table.rows.iter()
        .filter(|row| eval_where_condition(row, &where_clause))
        .collect();

    // ORDER BY
    let rest_after_where_upper = rest_after_where.to_uppercase();
    if let Some(order_pos) = rest_after_where_upper.find("ORDER BY") {
        let order_str = &rest_after_where[order_pos+8..];
        let end = order_str.to_uppercase().find("LIMIT").unwrap_or(order_str.len());
        let order_clause = order_str[..end].trim();
        let parts: Vec<&str> = order_clause.split(',').collect();
        if let Some(first) = parts.first() {
            let tokens: Vec<&str> = first.trim().split_whitespace().collect();
            let col = tokens[0].to_string();
            let desc = tokens.get(1).map(|t| t.to_uppercase() == "DESC").unwrap_or(false);
            result_rows.sort_by(|a, b| {
                let va = a.get(&col);
                let vb = b.get(&col);
                let cmp = compare_db_values(va, vb);
                if desc { cmp.reverse() } else { cmp }
            });
        }
    }

    // LIMIT
    if let Some(limit_pos) = rest_after_where_upper.find("LIMIT") {
        let limit_str = rest_after_where[limit_pos+5..].trim();
        let limit_end = limit_str.find(|c: char| !c.is_ascii_digit()).unwrap_or(limit_str.len());
        if let Ok(limit) = limit_str[..limit_end].parse::<usize>() {
            result_rows.truncate(limit);
        }
    }

    // Build result
    let mut rows = Vec::new();

    // Check if any columns are aggregates
    let has_aggregate = select_cols.iter().any(|col| {
        let upper = col.trim().to_uppercase();
        upper.starts_with("COUNT(") || upper.starts_with("SUM(")
            || upper.starts_with("AVG(") || upper.starts_with("MIN(")
            || upper.starts_with("MAX(")
    });

    if has_aggregate {
        // Aggregate query — produce single result row
        let mut vals = Vec::new();
        for col in &select_cols {
            if let Some(agg_val) = compute_aggregate(col, &result_rows) {
                vals.push(agg_val);
            } else {
                // Non-aggregate column in aggregate query: take first row value
                let col_trimmed = col.trim();
                let col_upper = col_trimmed.to_uppercase();
                let actual_col = if let Some(as_pos) = col_upper.find(" AS ") {
                    col_trimmed[..as_pos].trim()
                } else {
                    col_trimmed
                };
                vals.push(result_rows.first()
                    .and_then(|r| r.get(actual_col).cloned())
                    .unwrap_or(DbValue::Null));
            }
        }
        rows.push(vals);
    } else {
        // Non-aggregate query — return all matching rows
        for row in &result_rows {
            let mut vals = Vec::new();
            for col in &select_cols {
                let col_trimmed = col.trim();
                let col_upper = col_trimmed.to_uppercase();
                let actual_col = if let Some(as_pos) = col_upper.find(" AS ") {
                    col_trimmed[..as_pos].trim()
                } else {
                    col_trimmed
                };
                vals.push(row.get(actual_col).cloned().unwrap_or(DbValue::Null));
            }
            rows.push(vals);
        }
    }

    let columns: Vec<String> = select_cols.iter().map(|c| {
        let c = c.trim();
        let upper = c.to_uppercase();
        if let Some(as_pos) = upper.find(" AS ") {
            c[as_pos+4..].trim().to_string()
        } else {
            c.to_string()
        }
    }).collect();

    Ok(QueryResult { rows, columns, rowcount: -1, lastrowid: 0 })
}

fn find_clause_end(upper: &str) -> usize {
    for kw in &["ORDER BY", "LIMIT", "GROUP BY", "HAVING"] {
        if let Some(pos) = upper.find(kw) {
            return pos;
        }
    }
    upper.len()
}

fn compute_aggregate(col_expr: &str, rows: &[&IndexMap<String, DbValue>]) -> Option<DbValue> {
    let upper = col_expr.trim().to_uppercase();
    let (func, inner) = if let Some(paren) = upper.find('(') {
        let end = upper.rfind(')')?;
        let func_name = upper[..paren].trim();
        let inner = col_expr.trim()[paren + 1..end].trim().to_string();
        (func_name.to_string(), inner)
    } else {
        return None;
    };

    match func.as_str() {
        "COUNT" => {
            if inner == "*" {
                Some(DbValue::Int(rows.len() as i64))
            } else {
                let col = inner;
                let count = rows.iter().filter(|r| {
                    matches!(r.get(&col as &str), Some(v) if !matches!(v, DbValue::Null))
                }).count();
                Some(DbValue::Int(count as i64))
            }
        }
        "SUM" => {
            let col = inner;
            let mut sum_f: f64 = 0.0;
            let mut has_float = false;
            let mut count = 0i64;
            for row in rows {
                match row.get(&col as &str) {
                    Some(DbValue::Int(n)) => { sum_f += *n as f64; count += 1; }
                    Some(DbValue::Float(f)) => { sum_f += f; has_float = true; count += 1; }
                    _ => {}
                }
            }
            if count == 0 { return Some(DbValue::Null); }
            if has_float { Some(DbValue::Float(sum_f)) } else { Some(DbValue::Int(sum_f as i64)) }
        }
        "AVG" => {
            let col = inner;
            let mut sum: f64 = 0.0;
            let mut count = 0i64;
            for row in rows {
                match row.get(&col as &str) {
                    Some(DbValue::Int(n)) => { sum += *n as f64; count += 1; }
                    Some(DbValue::Float(f)) => { sum += f; count += 1; }
                    _ => {}
                }
            }
            if count == 0 { Some(DbValue::Null) } else { Some(DbValue::Float(sum / count as f64)) }
        }
        "MIN" => {
            let col = inner;
            let mut min_val: Option<DbValue> = None;
            for row in rows {
                match row.get(&col as &str) {
                    Some(DbValue::Null) | None => {}
                    Some(v) => {
                        min_val = Some(match min_val {
                            None => v.clone(),
                            Some(ref cur) => {
                                if compare_db_values(Some(v), Some(cur)) == std::cmp::Ordering::Less {
                                    v.clone()
                                } else {
                                    cur.clone()
                                }
                            }
                        });
                    }
                }
            }
            Some(min_val.unwrap_or(DbValue::Null))
        }
        "MAX" => {
            let col = inner;
            let mut max_val: Option<DbValue> = None;
            for row in rows {
                match row.get(&col as &str) {
                    Some(DbValue::Null) | None => {}
                    Some(v) => {
                        max_val = Some(match max_val {
                            None => v.clone(),
                            Some(ref cur) => {
                                if compare_db_values(Some(v), Some(cur)) == std::cmp::Ordering::Greater {
                                    v.clone()
                                } else {
                                    cur.clone()
                                }
                            }
                        });
                    }
                }
            }
            Some(max_val.unwrap_or(DbValue::Null))
        }
        _ => None,
    }
}

fn compare_db_values(a: Option<&DbValue>, b: Option<&DbValue>) -> std::cmp::Ordering {
    match (a, b) {
        (None, None) | (Some(DbValue::Null), Some(DbValue::Null)) => std::cmp::Ordering::Equal,
        (None, _) | (Some(DbValue::Null), _) => std::cmp::Ordering::Less,
        (_, None) | (_, Some(DbValue::Null)) => std::cmp::Ordering::Greater,
        (Some(DbValue::Int(a)), Some(DbValue::Int(b))) => a.cmp(b),
        (Some(DbValue::Float(a)), Some(DbValue::Float(b))) => a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
        (Some(DbValue::Text(a)), Some(DbValue::Text(b))) => a.cmp(b),
        _ => std::cmp::Ordering::Equal,
    }
}

fn execute_update(db: &mut Database, sql: &str) -> PyResult<QueryResult> {
    let upper = sql.to_uppercase();

    // UPDATE table SET col=val,... WHERE ...
    let set_pos = upper.find(" SET ").ok_or_else(|| PyException::new(
        ferrython_core::error::ExceptionKind::RuntimeError, "malformed UPDATE: missing SET"))?;

    let table_name = sql[6..set_pos].trim().to_string(); // after "UPDATE"

    let after_set = sql[set_pos+5..].trim();
    let where_pos = after_set.to_uppercase().find(" WHERE ");
    let (set_clause, where_clause) = if let Some(wp) = where_pos {
        (after_set[..wp].trim(), after_set[wp+7..].trim().to_string())
    } else {
        (after_set, String::new())
    };

    let assignments: Vec<(String, String)> = split_respecting_parens(set_clause, ',')
        .iter()
        .filter_map(|s| {
            let eq = s.find('=')?;
            Some((s[..eq].trim().to_string(), s[eq+1..].trim().to_string()))
        })
        .collect();

    let table = db.tables.get_mut(&table_name).ok_or_else(|| PyException::new(
        ferrython_core::error::ExceptionKind::RuntimeError,
        format!("no such table: {}", table_name),
    ))?;

    let mut rowcount = 0i64;
    for row in &mut table.rows {
        if eval_where_condition(row, &where_clause) {
            for (col, val) in &assignments {
                row.insert(col.clone(), parse_value(val));
            }
            rowcount += 1;
        }
    }

    Ok(QueryResult { rows: vec![], columns: vec![], rowcount, lastrowid: 0 })
}

fn execute_delete(db: &mut Database, sql: &str) -> PyResult<QueryResult> {
    let upper = sql.to_uppercase();

    let from_pos = upper.find("FROM ").ok_or_else(|| PyException::new(
        ferrython_core::error::ExceptionKind::RuntimeError, "malformed DELETE: missing FROM"))?;

    let after_from = sql[from_pos+5..].trim();
    let table_end = after_from.find(|c: char| c.is_whitespace()).unwrap_or(after_from.len());
    let table_name = after_from[..table_end].trim().to_string();
    let rest = after_from[table_end..].trim();

    let where_clause = if rest.to_uppercase().starts_with("WHERE ") {
        rest[6..].trim().to_string()
    } else {
        String::new()
    };

    let table = db.tables.get_mut(&table_name).ok_or_else(|| PyException::new(
        ferrython_core::error::ExceptionKind::RuntimeError,
        format!("no such table: {}", table_name),
    ))?;

    let before = table.rows.len();
    table.rows.retain(|row| !eval_where_condition(row, &where_clause));
    let rowcount = (before - table.rows.len()) as i64;

    Ok(QueryResult { rows: vec![], columns: vec![], rowcount, lastrowid: 0 })
}

fn execute_drop_table(db: &mut Database, sql: &str) -> PyResult<QueryResult> {
    let upper = sql.to_uppercase();
    let if_exists = upper.contains("IF EXISTS");

    let words: Vec<&str> = sql.split_whitespace().collect();
    let table_name = words.last().unwrap_or(&"").to_string();

    if !db.tables.contains_key(&table_name) {
        if if_exists {
            return Ok(QueryResult { rows: vec![], columns: vec![], rowcount: 0, lastrowid: 0 });
        }
        return Err(PyException::new(
            ferrython_core::error::ExceptionKind::RuntimeError,
            format!("no such table: {}", table_name),
        ));
    }

    db.tables.shift_remove(&table_name);
    Ok(QueryResult { rows: vec![], columns: vec![], rowcount: 0, lastrowid: 0 })
}

// ── Cursor builder ─────────────────────────────────────────────────────

fn build_cursor_object(db: Arc<Mutex<Database>>) -> PyObjectRef {
    let result_rows: Arc<Mutex<Vec<Vec<DbValue>>>> = Arc::new(Mutex::new(Vec::new()));
    let result_cols: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let fetch_pos: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    let rowcount: Arc<Mutex<i64>> = Arc::new(Mutex::new(-1));
    let lastrowid: Arc<Mutex<i64>> = Arc::new(Mutex::new(0));

    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("__sqlite_cursor__"), PyObject::bool_val(true));
    attrs.insert(CompactString::from("arraysize"), PyObject::int(1));

    // execute(sql, params=())
    let db_ref = db.clone();
    let rows_ref = result_rows.clone();
    let cols_ref = result_cols.clone();
    let pos_ref = fetch_pos.clone();
    let rc_ref = rowcount.clone();
    let lid_ref = lastrowid.clone();
    attrs.insert(CompactString::from("execute"), PyObject::native_closure("execute", move |args| {
        if args.is_empty() {
            return Err(PyException::type_error("execute() requires at least 1 argument"));
        }
        let sql = args[0].py_to_string();
        let params: Vec<PyObjectRef> = if args.len() > 1 {
            match &args[1].payload {
                PyObjectPayload::Tuple(items) => items.clone(),
                PyObjectPayload::List(items) => items.read().clone(),
                _ => vec![args[1].clone()],
            }
        } else {
            vec![]
        };

        let mut db_guard = db_ref.lock().unwrap();
        if db_guard.closed {
            return Err(PyException::new(
                ferrython_core::error::ExceptionKind::RuntimeError,
                "Cannot operate on a closed database.",
            ));
        }
        let result = execute_sql(&mut db_guard, &sql, &params)?;
        *rows_ref.lock().unwrap() = result.rows;
        *cols_ref.lock().unwrap() = result.columns;
        *pos_ref.lock().unwrap() = 0;
        *rc_ref.lock().unwrap() = result.rowcount;
        *lid_ref.lock().unwrap() = result.lastrowid;
        Ok(PyObject::none())
    }));

    // executemany(sql, seq_of_params)
    let db_ref = db.clone();
    let rc_ref = rowcount.clone();
    let lid_ref = lastrowid.clone();
    attrs.insert(CompactString::from("executemany"), PyObject::native_closure("executemany", move |args| {
        if args.len() < 2 {
            return Err(PyException::type_error("executemany() requires 2 arguments"));
        }
        let sql = args[0].py_to_string();
        let seq = match &args[1].payload {
            PyObjectPayload::List(items) => items.read().clone(),
            PyObjectPayload::Tuple(items) => items.clone(),
            _ => return Err(PyException::type_error("executemany() second arg must be iterable")),
        };

        let mut db_guard = db_ref.lock().unwrap();
        if db_guard.closed {
            return Err(PyException::new(
                ferrython_core::error::ExceptionKind::RuntimeError,
                "Cannot operate on a closed database.",
            ));
        }
        let mut total_rowcount = 0i64;
        let mut last_id = 0i64;
        for param_set in &seq {
            let params: Vec<PyObjectRef> = match &param_set.payload {
                PyObjectPayload::Tuple(items) => items.clone(),
                PyObjectPayload::List(items) => items.read().clone(),
                _ => vec![param_set.clone()],
            };
            let result = execute_sql(&mut db_guard, &sql, &params)?;
            total_rowcount += result.rowcount;
            last_id = result.lastrowid;
        }
        *rc_ref.lock().unwrap() = total_rowcount;
        *lid_ref.lock().unwrap() = last_id;
        Ok(PyObject::none())
    }));

    // fetchone()
    let rows_ref = result_rows.clone();
    let pos_ref = fetch_pos.clone();
    attrs.insert(CompactString::from("fetchone"), PyObject::native_closure("fetchone", move |_args| {
        let rows = rows_ref.lock().unwrap();
        let mut pos = pos_ref.lock().unwrap();
        if *pos >= rows.len() {
            return Ok(PyObject::none());
        }
        let row = &rows[*pos];
        *pos += 1;
        let items: Vec<PyObjectRef> = row.iter().map(|v| v.to_pyobject()).collect();
        Ok(PyObject::tuple(items))
    }));

    // fetchall()
    let rows_ref = result_rows.clone();
    let pos_ref = fetch_pos.clone();
    attrs.insert(CompactString::from("fetchall"), PyObject::native_closure("fetchall", move |_args| {
        let rows = rows_ref.lock().unwrap();
        let mut pos = pos_ref.lock().unwrap();
        let mut result = Vec::new();
        while *pos < rows.len() {
            let row = &rows[*pos];
            let items: Vec<PyObjectRef> = row.iter().map(|v| v.to_pyobject()).collect();
            result.push(PyObject::tuple(items));
            *pos += 1;
        }
        Ok(PyObject::list(result))
    }));

    // fetchmany(size=arraysize)
    let rows_ref = result_rows.clone();
    let pos_ref = fetch_pos.clone();
    attrs.insert(CompactString::from("fetchmany"), PyObject::native_closure("fetchmany", move |args| {
        let size = if !args.is_empty() {
            args[0].to_int().unwrap_or(1) as usize
        } else {
            1
        };
        let rows = rows_ref.lock().unwrap();
        let mut pos = pos_ref.lock().unwrap();
        let mut result = Vec::new();
        let mut count = 0;
        while *pos < rows.len() && count < size {
            let row = &rows[*pos];
            let items: Vec<PyObjectRef> = row.iter().map(|v| v.to_pyobject()).collect();
            result.push(PyObject::tuple(items));
            *pos += 1;
            count += 1;
        }
        Ok(PyObject::list(result))
    }));

    // description
    let cols_ref = result_cols.clone();
    attrs.insert(CompactString::from("description"), PyObject::native_closure("description", move |_args| {
        let cols = cols_ref.lock().unwrap();
        if cols.is_empty() {
            return Ok(PyObject::none());
        }
        let items: Vec<PyObjectRef> = cols.iter().map(|name| {
            PyObject::tuple(vec![
                PyObject::str_val(CompactString::from(name.as_str())),
                PyObject::none(), PyObject::none(), PyObject::none(),
                PyObject::none(), PyObject::none(), PyObject::none(),
            ])
        }).collect();
        Ok(PyObject::list(items))
    }));

    // rowcount property
    let rc_ref = rowcount.clone();
    attrs.insert(CompactString::from("rowcount"), PyObject::native_closure("rowcount", move |_args| {
        Ok(PyObject::int(*rc_ref.lock().unwrap()))
    }));

    // lastrowid property
    let lid_ref = lastrowid.clone();
    attrs.insert(CompactString::from("lastrowid"), PyObject::native_closure("lastrowid", move |_args| {
        Ok(PyObject::int(*lid_ref.lock().unwrap()))
    }));

    // close()
    attrs.insert(CompactString::from("close"), PyObject::native_function("close", |_args| {
        Ok(PyObject::none())
    }));

    // __iter__ / __next__ for iteration
    let rows_ref = result_rows.clone();
    let pos_ref = fetch_pos.clone();
    attrs.insert(CompactString::from("__iter__"), PyObject::native_closure("__iter__", move |_args| {
        let rows = rows_ref.lock().unwrap();
        let pos = pos_ref.lock().unwrap();
        let mut result = Vec::new();
        for i in *pos..rows.len() {
            let row = &rows[i];
            let items: Vec<PyObjectRef> = row.iter().map(|v| v.to_pyobject()).collect();
            result.push(PyObject::tuple(items));
        }
        Ok(PyObject::list(result))
    }));

    let cls = PyObject::class(CompactString::from("Cursor"), vec![], IndexMap::new());
    PyObject::instance_with_attrs(cls, attrs)
}

// ── Connection builder ─────────────────────────────────────────────────

fn build_connection_object(db: Arc<Mutex<Database>>) -> PyObjectRef {
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("__sqlite_conn__"), PyObject::bool_val(true));

    // cursor()
    let db_ref = db.clone();
    attrs.insert(CompactString::from("cursor"), PyObject::native_closure("cursor", move |_args| {
        Ok(build_cursor_object(db_ref.clone()))
    }));

    // execute(sql, params=()) — convenience: creates cursor, executes, returns cursor
    let db_ref = db.clone();
    attrs.insert(CompactString::from("execute"), PyObject::native_closure("execute", move |args| {
        let cursor = build_cursor_object(db_ref.clone());
        if let PyObjectPayload::Instance(ref d) = cursor.payload {
            let r = d.attrs.read();
            if let Some(exec_fn) = r.get(&CompactString::from("execute")) {
                if let PyObjectPayload::NativeClosure { func, .. } = &exec_fn.payload {
                    func(args)?;
                }
            }
        }
        Ok(cursor)
    }));

    // executemany(sql, seq_of_params)
    let db_ref = db.clone();
    attrs.insert(CompactString::from("executemany"), PyObject::native_closure("executemany", move |args| {
        let cursor = build_cursor_object(db_ref.clone());
        if let PyObjectPayload::Instance(ref d) = cursor.payload {
            let r = d.attrs.read();
            if let Some(exec_fn) = r.get(&CompactString::from("executemany")) {
                if let PyObjectPayload::NativeClosure { func, .. } = &exec_fn.payload {
                    func(args)?;
                }
            }
        }
        Ok(cursor)
    }));

    // commit()
    attrs.insert(CompactString::from("commit"), PyObject::native_function("commit", |_args| {
        Ok(PyObject::none())
    }));

    // rollback()
    attrs.insert(CompactString::from("rollback"), PyObject::native_function("rollback", |_args| {
        Ok(PyObject::none())
    }));

    // close()
    let db_ref = db.clone();
    attrs.insert(CompactString::from("close"), PyObject::native_closure("close", move |_args| {
        let mut guard = db_ref.lock().unwrap();
        guard.closed = true;
        Ok(PyObject::none())
    }));

    // create_function(name, num_params, func)
    attrs.insert(CompactString::from("create_function"), PyObject::native_function("create_function", |_args| {
        // Stub — real function registration not supported in dict-based DB
        Ok(PyObject::none())
    }));

    // total_changes
    let db_ref = db.clone();
    attrs.insert(CompactString::from("total_changes"), PyObject::native_closure("total_changes", move |_args| {
        let guard = db_ref.lock().unwrap();
        let total: usize = guard.tables.values().map(|t| t.rows.len()).sum();
        Ok(PyObject::int(total as i64))
    }));

    // isolation_level
    attrs.insert(CompactString::from("isolation_level"), PyObject::str_val(CompactString::from("")));

    // row_factory
    attrs.insert(CompactString::from("row_factory"), PyObject::none());

    // __enter__ / __exit__ for context manager
    attrs.insert(CompactString::from("__enter__"), PyObject::native_function("__enter__", |args| {
        if args.is_empty() { return Ok(PyObject::none()); }
        Ok(args[0].clone())
    }));

    let db_ref = db.clone();
    attrs.insert(CompactString::from("__exit__"), PyObject::native_closure("__exit__", move |_args| {
        // auto-commit on exit
        let guard = db_ref.lock().unwrap();
        drop(guard);
        Ok(PyObject::bool_val(false))
    }));

    let cls = PyObject::class(CompactString::from("Connection"), vec![], IndexMap::new());
    PyObject::instance_with_attrs(cls, attrs)
}

// ── Module constructor ─────────────────────────────────────────────────

fn sqlite3_connect(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("connect() requires 1 argument: database"));
    }
    let path = args[0].py_to_string();
    let db = Arc::new(Mutex::new(Database::new(&path)));
    Ok(build_connection_object(db))
}

pub fn create_sqlite3_module() -> PyObjectRef {
    make_module("sqlite3", vec![
        ("connect", make_builtin(sqlite3_connect)),
        ("version", PyObject::str_val(CompactString::from("2.6.0"))),
        ("sqlite_version", PyObject::str_val(CompactString::from("3.39.0"))),
        ("PARSE_DECLTYPES", PyObject::int(1)),
        ("PARSE_COLNAMES", PyObject::int(2)),
        ("apilevel", PyObject::str_val(CompactString::from("2.0"))),
        ("paramstyle", PyObject::str_val(CompactString::from("qmark"))),
        ("threadsafety", PyObject::int(1)),
        ("Error", PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError)),
        ("DatabaseError", PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError)),
        ("OperationalError", PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError)),
        ("IntegrityError", PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError)),
        ("ProgrammingError", PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError)),
        ("InterfaceError", PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError)),
        ("Row", make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none()))),
    ])
}
