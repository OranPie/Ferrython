//! SQL execution for the in-memory sqlite3 implementation.

use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::PyObjectRef;
use indexmap::IndexMap;

use super::parser::{eval_where_condition, normalize_sql, parse_value, split_respecting_parens};
use super::storage::{Column, Database, DbValue, Table};

// ── SQL execution ──────────────────────────────────────────────────────

pub(super) struct QueryResult {
    pub(super) rows: Vec<Vec<DbValue>>,
    pub(super) columns: Vec<String>,
    pub(super) rowcount: i64,
    pub(super) lastrowid: i64,
}

pub(super) fn execute_sql(
    db: &mut Database,
    sql: &str,
    params: &[PyObjectRef],
) -> PyResult<QueryResult> {
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
    } else if upper.starts_with("CREATE INDEX") || upper.starts_with("CREATE UNIQUE INDEX") {
        // Indexes are not needed for our in-memory engine; accept and no-op.
        Ok(QueryResult {
            rows: vec![],
            columns: vec![],
            rowcount: 0,
            lastrowid: 0,
        })
    } else if upper.starts_with("BEGIN")
        || upper.starts_with("COMMIT")
        || upper.starts_with("ROLLBACK")
    {
        Ok(QueryResult {
            rows: vec![],
            columns: vec![],
            rowcount: 0,
            lastrowid: 0,
        })
    } else if upper.starts_with("PRAGMA") {
        Ok(QueryResult {
            rows: vec![],
            columns: vec![],
            rowcount: 0,
            lastrowid: 0,
        })
    } else {
        Err(PyException::new(
            ferrython_core::error::ExceptionKind::RuntimeError,
            format!("unsupported SQL: {}", sql),
        ))
    }
}

fn substitute_params(sql: &str, params: &[PyObjectRef]) -> String {
    if params.is_empty() {
        return sql.to_string();
    }
    let mut result = String::new();
    let mut param_idx = 0;
    let mut in_string = false;
    let mut string_char = ' ';
    for c in sql.chars() {
        if in_string {
            result.push(c);
            if c == string_char {
                in_string = false;
            }
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
    let paren_start = sql.find('(').ok_or_else(|| {
        PyException::new(
            ferrython_core::error::ExceptionKind::RuntimeError,
            "malformed CREATE TABLE",
        )
    })?;
    let paren_end = sql.rfind(')').ok_or_else(|| {
        PyException::new(
            ferrython_core::error::ExceptionKind::RuntimeError,
            "malformed CREATE TABLE",
        )
    })?;

    let before_paren = &sql[..paren_start].trim();
    let words: Vec<&str> = before_paren.split_whitespace().collect();
    let table_name = if if_not_exists {
        words.last().unwrap_or(&"").to_string()
    } else {
        // CREATE TABLE name
        words.last().unwrap_or(&"").to_string()
    };

    if if_not_exists && db.tables.contains_key(&table_name) {
        return Ok(QueryResult {
            rows: vec![],
            columns: vec![],
            rowcount: 0,
            lastrowid: 0,
        });
    }

    if db.tables.contains_key(&table_name) {
        return Err(PyException::new(
            ferrython_core::error::ExceptionKind::RuntimeError,
            format!("table {} already exists", table_name),
        ));
    }

    let cols_str = &sql[paren_start + 1..paren_end];
    let col_defs = split_respecting_parens(cols_str, ',');
    let mut columns = Vec::new();
    for def in &col_defs {
        let def = def.trim();
        let def_upper = def.to_uppercase();
        // Skip constraints like PRIMARY KEY(...), UNIQUE(...), FOREIGN KEY(...)
        if def_upper.starts_with("PRIMARY KEY")
            || def_upper.starts_with("UNIQUE")
            || def_upper.starts_with("FOREIGN KEY")
            || def_upper.starts_with("CHECK")
            || def_upper.starts_with("CONSTRAINT")
        {
            continue;
        }
        let parts: Vec<&str> = def.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        let name = parts[0].to_string();
        let col_type = if parts.len() > 1 {
            parts[1].to_uppercase()
        } else {
            "TEXT".to_string()
        };
        let is_pk = def_upper.contains("PRIMARY KEY");
        columns.push(Column {
            name,
            col_type,
            primary_key: is_pk,
        });
    }

    db.tables.insert(
        table_name.clone(),
        Table {
            name: table_name,
            columns,
            rows: Vec::new(),
            auto_increment: 1,
        },
    );

    Ok(QueryResult {
        rows: vec![],
        columns: vec![],
        rowcount: 0,
        lastrowid: 0,
    })
}

fn execute_insert(db: &mut Database, sql: &str) -> PyResult<QueryResult> {
    let upper = sql.to_uppercase();

    // Find table name - after INTO
    let into_pos = upper.find("INTO").ok_or_else(|| {
        PyException::new(
            ferrython_core::error::ExceptionKind::RuntimeError,
            "malformed INSERT",
        )
    })?;
    let after_into = sql[into_pos + 4..].trim();

    // Table name is first word
    let table_end = after_into
        .find(|c: char| c == '(' || c.is_whitespace())
        .unwrap_or(after_into.len());
    let table_name = after_into[..table_end].trim().to_string();

    let table = db
        .tables
        .get(&table_name)
        .ok_or_else(|| {
            PyException::new(
                ferrython_core::error::ExceptionKind::RuntimeError,
                format!("no such table: {}", table_name),
            )
        })?
        .clone();

    let rest = after_into[table_end..].trim();

    // Extract column names if provided
    let (col_names, values_str) = if rest.starts_with('(') {
        let close = rest.find(')').ok_or_else(|| {
            PyException::new(
                ferrython_core::error::ExceptionKind::RuntimeError,
                "malformed INSERT: missing )",
            )
        })?;
        let cols: Vec<String> = rest[1..close]
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
        let after = rest[close + 1..].trim();
        (cols, after.to_string())
    } else {
        let cols: Vec<String> = table.columns.iter().map(|c| c.name.clone()).collect();
        (cols, rest.to_string())
    };

    // Find VALUES(...)
    let values_upper = values_str.to_uppercase();
    let vals_pos = values_upper.find("VALUES").ok_or_else(|| {
        PyException::new(
            ferrython_core::error::ExceptionKind::RuntimeError,
            "malformed INSERT: missing VALUES",
        )
    })?;
    let after_values = values_str[vals_pos + 6..].trim();

    // Could be multiple value tuples
    let mut row_strings = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut in_string = false;
    let mut string_char = ' ';
    for (i, c) in after_values.chars().enumerate() {
        if in_string {
            if c == string_char {
                in_string = false;
            }
        } else if c == '\'' || c == '"' {
            in_string = true;
            string_char = c;
        } else if c == '(' {
            if depth == 0 {
                start = i + 1;
            }
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
                    lastrowid = row
                        .get(&col.name)
                        .map(|v| match v {
                            DbValue::Int(i) => *i,
                            _ => 0,
                        })
                        .unwrap_or(0);
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

    Ok(QueryResult {
        rows: vec![],
        columns: vec![],
        rowcount,
        lastrowid,
    })
}

fn execute_select(db: &Database, sql: &str) -> PyResult<QueryResult> {
    let upper = sql.to_uppercase();

    // Extract FROM table
    let from_pos = upper.find(" FROM ").ok_or_else(|| {
        PyException::new(
            ferrython_core::error::ExceptionKind::RuntimeError,
            "malformed SELECT: missing FROM",
        )
    })?;

    let select_cols_str = sql[6..from_pos].trim(); // after SELECT
    let after_from = sql[from_pos + 6..].trim();

    // Table name
    let table_end = after_from
        .find(|c: char| c.is_whitespace())
        .unwrap_or(after_from.len());
    let table_name = after_from[..table_end].trim().to_string();
    let rest = after_from[table_end..].trim();

    let table = db.tables.get(&table_name).ok_or_else(|| {
        PyException::new(
            ferrython_core::error::ExceptionKind::RuntimeError,
            format!("no such table: {}", table_name),
        )
    })?;

    // WHERE clause
    let rest_upper = rest.to_uppercase();
    let (where_clause, rest_after_where) = if let Some(w_pos) = rest_upper.find("WHERE ") {
        let after_where = &rest[w_pos + 6..];
        // Find end (ORDER BY, LIMIT, GROUP BY, or end)
        let end_pos = find_clause_end(&after_where.to_uppercase());
        let wc = after_where[..end_pos].trim().to_string();
        let remaining = after_where[end_pos..].trim().to_string();
        (wc, remaining)
    } else {
        (String::new(), rest.to_string());
        let rau = rest.to_uppercase();
        let end_pos =
            if rau.starts_with("ORDER") || rau.starts_with("LIMIT") || rau.starts_with("GROUP") {
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
        select_cols_str
            .split(',')
            .map(|s| {
                let s = s.trim();
                // Handle COUNT(*), etc.
                s.to_string()
            })
            .collect()
    };

    // Filter rows
    let mut result_rows: Vec<&IndexMap<String, DbValue>> = table
        .rows
        .iter()
        .filter(|row| eval_where_condition(row, &where_clause))
        .collect();

    // ORDER BY
    let rest_after_where_upper = rest_after_where.to_uppercase();
    if let Some(order_pos) = rest_after_where_upper.find("ORDER BY") {
        let order_str = &rest_after_where[order_pos + 8..];
        let end = order_str
            .to_uppercase()
            .find("LIMIT")
            .unwrap_or(order_str.len());
        let order_clause = order_str[..end].trim();
        let parts: Vec<&str> = order_clause.split(',').collect();
        if let Some(first) = parts.first() {
            let tokens: Vec<&str> = first.trim().split_whitespace().collect();
            let col = tokens[0].to_string();
            let desc = tokens
                .get(1)
                .map(|t| t.to_uppercase() == "DESC")
                .unwrap_or(false);
            result_rows.sort_by(|a, b| {
                let va = a.get(&col);
                let vb = b.get(&col);
                let cmp = compare_db_values(va, vb);
                if desc {
                    cmp.reverse()
                } else {
                    cmp
                }
            });
        }
    }

    // LIMIT
    if let Some(limit_pos) = rest_after_where_upper.find("LIMIT") {
        let limit_str = rest_after_where[limit_pos + 5..].trim();
        let limit_end = limit_str
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(limit_str.len());
        if let Ok(limit) = limit_str[..limit_end].parse::<usize>() {
            result_rows.truncate(limit);
        }
    }

    // Build result
    let mut rows = Vec::new();

    // Check if any columns are aggregates
    let has_aggregate = select_cols.iter().any(|col| {
        let upper = col.trim().to_uppercase();
        upper.starts_with("COUNT(")
            || upper.starts_with("SUM(")
            || upper.starts_with("AVG(")
            || upper.starts_with("MIN(")
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
                vals.push(
                    result_rows
                        .first()
                        .and_then(|r| r.get(actual_col).cloned())
                        .unwrap_or(DbValue::Null),
                );
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

    let columns: Vec<String> = select_cols
        .iter()
        .map(|c| {
            let c = c.trim();
            let upper = c.to_uppercase();
            if let Some(as_pos) = upper.find(" AS ") {
                c[as_pos + 4..].trim().to_string()
            } else {
                c.to_string()
            }
        })
        .collect();

    Ok(QueryResult {
        rows,
        columns,
        rowcount: -1,
        lastrowid: 0,
    })
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
                let count = rows
                    .iter()
                    .filter(
                        |r| matches!(r.get(&col as &str), Some(v) if !matches!(v, DbValue::Null)),
                    )
                    .count();
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
                    Some(DbValue::Int(n)) => {
                        sum_f += *n as f64;
                        count += 1;
                    }
                    Some(DbValue::Float(f)) => {
                        sum_f += f;
                        has_float = true;
                        count += 1;
                    }
                    _ => {}
                }
            }
            if count == 0 {
                return Some(DbValue::Null);
            }
            if has_float {
                Some(DbValue::Float(sum_f))
            } else {
                Some(DbValue::Int(sum_f as i64))
            }
        }
        "AVG" => {
            let col = inner;
            let mut sum: f64 = 0.0;
            let mut count = 0i64;
            for row in rows {
                match row.get(&col as &str) {
                    Some(DbValue::Int(n)) => {
                        sum += *n as f64;
                        count += 1;
                    }
                    Some(DbValue::Float(f)) => {
                        sum += f;
                        count += 1;
                    }
                    _ => {}
                }
            }
            if count == 0 {
                Some(DbValue::Null)
            } else {
                Some(DbValue::Float(sum / count as f64))
            }
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
                                if compare_db_values(Some(v), Some(cur)) == std::cmp::Ordering::Less
                                {
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
                                if compare_db_values(Some(v), Some(cur))
                                    == std::cmp::Ordering::Greater
                                {
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
        (Some(DbValue::Float(a)), Some(DbValue::Float(b))) => {
            a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
        }
        (Some(DbValue::Text(a)), Some(DbValue::Text(b))) => a.cmp(b),
        _ => std::cmp::Ordering::Equal,
    }
}

fn execute_update(db: &mut Database, sql: &str) -> PyResult<QueryResult> {
    let upper = sql.to_uppercase();

    // UPDATE table SET col=val,... WHERE ...
    let set_pos = upper.find(" SET ").ok_or_else(|| {
        PyException::new(
            ferrython_core::error::ExceptionKind::RuntimeError,
            "malformed UPDATE: missing SET",
        )
    })?;

    let table_name = sql[6..set_pos].trim().to_string(); // after "UPDATE"

    let after_set = sql[set_pos + 5..].trim();
    let where_pos = after_set.to_uppercase().find(" WHERE ");
    let (set_clause, where_clause) = if let Some(wp) = where_pos {
        (
            after_set[..wp].trim(),
            after_set[wp + 7..].trim().to_string(),
        )
    } else {
        (after_set, String::new())
    };

    let assignments: Vec<(String, String)> = split_respecting_parens(set_clause, ',')
        .iter()
        .filter_map(|s| {
            let eq = s.find('=')?;
            Some((s[..eq].trim().to_string(), s[eq + 1..].trim().to_string()))
        })
        .collect();

    let table = db.tables.get_mut(&table_name).ok_or_else(|| {
        PyException::new(
            ferrython_core::error::ExceptionKind::RuntimeError,
            format!("no such table: {}", table_name),
        )
    })?;

    let mut rowcount = 0i64;
    for row in &mut table.rows {
        if eval_where_condition(row, &where_clause) {
            for (col, val) in &assignments {
                row.insert(col.clone(), parse_value(val));
            }
            rowcount += 1;
        }
    }

    Ok(QueryResult {
        rows: vec![],
        columns: vec![],
        rowcount,
        lastrowid: 0,
    })
}

fn execute_delete(db: &mut Database, sql: &str) -> PyResult<QueryResult> {
    let upper = sql.to_uppercase();

    let from_pos = upper.find("FROM ").ok_or_else(|| {
        PyException::new(
            ferrython_core::error::ExceptionKind::RuntimeError,
            "malformed DELETE: missing FROM",
        )
    })?;

    let after_from = sql[from_pos + 5..].trim();
    let table_end = after_from
        .find(|c: char| c.is_whitespace())
        .unwrap_or(after_from.len());
    let table_name = after_from[..table_end].trim().to_string();
    let rest = after_from[table_end..].trim();

    let where_clause = if rest.to_uppercase().starts_with("WHERE ") {
        rest[6..].trim().to_string()
    } else {
        String::new()
    };

    let table = db.tables.get_mut(&table_name).ok_or_else(|| {
        PyException::new(
            ferrython_core::error::ExceptionKind::RuntimeError,
            format!("no such table: {}", table_name),
        )
    })?;

    let before = table.rows.len();
    table
        .rows
        .retain(|row| !eval_where_condition(row, &where_clause));
    let rowcount = (before - table.rows.len()) as i64;

    Ok(QueryResult {
        rows: vec![],
        columns: vec![],
        rowcount,
        lastrowid: 0,
    })
}

fn execute_drop_table(db: &mut Database, sql: &str) -> PyResult<QueryResult> {
    let upper = sql.to_uppercase();
    let if_exists = upper.contains("IF EXISTS");

    let words: Vec<&str> = sql.split_whitespace().collect();
    let table_name = words.last().unwrap_or(&"").to_string();

    if !db.tables.contains_key(&table_name) {
        if if_exists {
            return Ok(QueryResult {
                rows: vec![],
                columns: vec![],
                rowcount: 0,
                lastrowid: 0,
            });
        }
        return Err(PyException::new(
            ferrython_core::error::ExceptionKind::RuntimeError,
            format!("no such table: {}", table_name),
        ));
    }

    db.tables.shift_remove(&table_name);
    Ok(QueryResult {
        rows: vec![],
        columns: vec![],
        rowcount: 0,
        lastrowid: 0,
    })
}
