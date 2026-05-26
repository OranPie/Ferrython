//! sqlite3 cursor object builder.

use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use indexmap::IndexMap;
use std::sync::{Arc, Mutex};

use super::row::build_sqlite_row;
use super::sql::execute_sql;
use super::storage::{Database, DbValue};

// ── Cursor builder ─────────────────────────────────────────────────────

#[allow(dead_code)]
pub(super) fn build_cursor_object(db: Arc<Mutex<Database>>) -> PyObjectRef {
    build_cursor_object_with_conn(db, None)
}

pub(super) fn build_cursor_object_with_conn(
    db: Arc<Mutex<Database>>,
    conn: Option<PyObjectRef>,
) -> PyObjectRef {
    let result_rows: Arc<Mutex<Vec<Vec<DbValue>>>> = Arc::new(Mutex::new(Vec::new()));
    let result_cols: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let fetch_pos: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    let rowcount: Arc<Mutex<i64>> = Arc::new(Mutex::new(-1));
    let lastrowid: Arc<Mutex<i64>> = Arc::new(Mutex::new(0));
    let self_ref: Arc<Mutex<Option<PyObjectRef>>> = Arc::new(Mutex::new(None));

    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("__sqlite_cursor__"),
        PyObject::bool_val(true),
    );
    attrs.insert(CompactString::from("arraysize"), PyObject::int(1));

    // execute(sql, params=())
    let db_ref = db.clone();
    let rows_ref = result_rows.clone();
    let cols_ref = result_cols.clone();
    let pos_ref = fetch_pos.clone();
    let rc_ref = rowcount.clone();
    let lid_ref = lastrowid.clone();
    let sr = self_ref.clone();
    attrs.insert(
        CompactString::from("execute"),
        PyObject::native_closure("execute", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "execute() requires at least 1 argument",
                ));
            }
            let sql = args[0].py_to_string();
            let params: Vec<PyObjectRef> = if args.len() > 1 {
                match &args[1].payload {
                    PyObjectPayload::Tuple(items) => (**items).clone(),
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
            // Update attrs on cursor self-reference for property access
            let cursor = sr
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| PyObject::none());
            if let PyObjectPayload::Instance(inst) = &cursor.payload {
                let mut a = inst.attrs.write();
                a.insert(
                    CompactString::from("rowcount"),
                    PyObject::int(result.rowcount),
                );
                a.insert(
                    CompactString::from("lastrowid"),
                    PyObject::int(result.lastrowid),
                );
            }
            Ok(cursor)
        }),
    );

    // executemany(sql, seq_of_params)
    let db_ref = db.clone();
    let rc_ref = rowcount.clone();
    let lid_ref = lastrowid.clone();
    let sr = self_ref.clone();
    attrs.insert(
        CompactString::from("executemany"),
        PyObject::native_closure("executemany", move |args| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "executemany() requires 2 arguments",
                ));
            }
            let sql = args[0].py_to_string();
            let seq = match &args[1].payload {
                PyObjectPayload::List(items) => items.read().clone(),
                PyObjectPayload::Tuple(items) => (**items).clone(),
                _ => {
                    return Err(PyException::type_error(
                        "executemany() second arg must be iterable",
                    ))
                }
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
                    PyObjectPayload::Tuple(items) => (**items).clone(),
                    PyObjectPayload::List(items) => items.read().clone(),
                    _ => vec![param_set.clone()],
                };
                let result = execute_sql(&mut db_guard, &sql, &params)?;
                total_rowcount += result.rowcount;
                last_id = result.lastrowid;
            }
            *rc_ref.lock().unwrap() = total_rowcount;
            *lid_ref.lock().unwrap() = last_id;
            let cursor = sr
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| PyObject::none());
            if let PyObjectPayload::Instance(inst) = &cursor.payload {
                let mut a = inst.attrs.write();
                a.insert(
                    CompactString::from("rowcount"),
                    PyObject::int(total_rowcount),
                );
                a.insert(CompactString::from("lastrowid"), PyObject::int(last_id));
            }
            Ok(cursor)
        }),
    );

    // executescript(sql_script) — execute multiple SQL statements separated by semicolons
    let db_ref = db.clone();
    let sr = self_ref.clone();
    attrs.insert(
        CompactString::from("executescript"),
        PyObject::native_closure("executescript", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "executescript() requires 1 argument",
                ));
            }
            let script = args[0].py_to_string();
            let mut db_guard = db_ref.lock().unwrap();
            if db_guard.closed {
                return Err(PyException::new(
                    ferrython_core::error::ExceptionKind::RuntimeError,
                    "Cannot operate on a closed database.",
                ));
            }
            for stmt in script.split(';') {
                let trimmed = stmt.trim();
                if trimmed.is_empty() {
                    continue;
                }
                execute_sql(&mut db_guard, trimmed, &[])?;
            }
            Ok(sr
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| PyObject::none()))
        }),
    );

    // fetchone()
    let rows_ref = result_rows.clone();
    let pos_ref = fetch_pos.clone();
    let cols_for_fetch = result_cols.clone();
    let conn_one = conn.clone();
    attrs.insert(
        CompactString::from("fetchone"),
        PyObject::native_closure("fetchone", move |_args| {
            let rows = rows_ref.lock().unwrap();
            let mut pos = pos_ref.lock().unwrap();
            if *pos >= rows.len() {
                return Ok(PyObject::none());
            }
            let row = &rows[*pos];
            *pos += 1;
            let items: Vec<PyObjectRef> = row.iter().map(|v| v.to_pyobject()).collect();
            let use_row = conn_one
                .as_ref()
                .and_then(|c| c.get_attr("row_factory"))
                .map(|rf| !matches!(&rf.payload, PyObjectPayload::None))
                .unwrap_or(false);
            if use_row {
                let cols = cols_for_fetch.lock().unwrap();
                Ok(build_sqlite_row(&cols, &items))
            } else {
                Ok(PyObject::tuple(items))
            }
        }),
    );

    // fetchall()
    let rows_ref = result_rows.clone();
    let pos_ref = fetch_pos.clone();
    let cols_for_fetch = result_cols.clone();
    let conn_all = conn.clone();
    attrs.insert(
        CompactString::from("fetchall"),
        PyObject::native_closure("fetchall", move |_args| {
            let rows = rows_ref.lock().unwrap();
            let mut pos = pos_ref.lock().unwrap();
            let use_row = conn_all
                .as_ref()
                .and_then(|c| c.get_attr("row_factory"))
                .map(|rf| !matches!(&rf.payload, PyObjectPayload::None))
                .unwrap_or(false);
            let cols = if use_row {
                cols_for_fetch.lock().unwrap().clone()
            } else {
                vec![]
            };
            let mut result = Vec::new();
            while *pos < rows.len() {
                let row = &rows[*pos];
                let items: Vec<PyObjectRef> = row.iter().map(|v| v.to_pyobject()).collect();
                if use_row {
                    result.push(build_sqlite_row(&cols, &items));
                } else {
                    result.push(PyObject::tuple(items));
                }
                *pos += 1;
            }
            Ok(PyObject::list(result))
        }),
    );

    // fetchmany(size=arraysize)
    let rows_ref = result_rows.clone();
    let pos_ref = fetch_pos.clone();
    attrs.insert(
        CompactString::from("fetchmany"),
        PyObject::native_closure("fetchmany", move |args| {
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
        }),
    );

    // description
    let cols_ref = result_cols.clone();
    attrs.insert(
        CompactString::from("description"),
        PyObject::native_closure("description", move |_args| {
            let cols = cols_ref.lock().unwrap();
            if cols.is_empty() {
                return Ok(PyObject::none());
            }
            let items: Vec<PyObjectRef> = cols
                .iter()
                .map(|name| {
                    PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from(name.as_str())),
                        PyObject::none(),
                        PyObject::none(),
                        PyObject::none(),
                        PyObject::none(),
                        PyObject::none(),
                        PyObject::none(),
                    ])
                })
                .collect();
            Ok(PyObject::list(items))
        }),
    );

    // rowcount and lastrowid — direct values, updated by execute/executemany
    attrs.insert(CompactString::from("rowcount"), PyObject::int(-1));
    attrs.insert(CompactString::from("lastrowid"), PyObject::none());

    // close()
    attrs.insert(
        CompactString::from("close"),
        PyObject::native_function("close", |_args| Ok(PyObject::none())),
    );

    // __iter__ / __next__ for iteration
    let rows_ref = result_rows.clone();
    let pos_ref = fetch_pos.clone();
    attrs.insert(
        CompactString::from("__iter__"),
        PyObject::native_closure("__iter__", move |_args| {
            let rows = rows_ref.lock().unwrap();
            let pos = pos_ref.lock().unwrap();
            let mut result = Vec::new();
            for i in *pos..rows.len() {
                let row = &rows[i];
                let items: Vec<PyObjectRef> = row.iter().map(|v| v.to_pyobject()).collect();
                result.push(PyObject::tuple(items));
            }
            Ok(PyObject::list(result))
        }),
    );

    let cls = PyObject::class(CompactString::from("Cursor"), vec![], IndexMap::new());
    let cursor = PyObject::instance_with_attrs(cls, attrs);
    // Populate self-reference so execute() can return the cursor
    *self_ref.lock().unwrap() = Some(cursor.clone());
    cursor
}
