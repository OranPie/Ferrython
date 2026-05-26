//! sqlite3 connection object builder and connect entrypoint.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use indexmap::IndexMap;
use std::sync::{Arc, Mutex};

use super::cursor::build_cursor_object_with_conn;
use super::storage::{Database, DB_REGISTRY};

// ── Connection builder ─────────────────────────────────────────────────

pub(super) fn build_connection_object(db: Arc<Mutex<Database>>) -> PyObjectRef {
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("__sqlite_conn__"),
        PyObject::bool_val(true),
    );

    // Self-reference so cursor() can pass the connection for row_factory access
    let conn_ref: Arc<Mutex<Option<PyObjectRef>>> = Arc::new(Mutex::new(None));

    // cursor()
    let db_ref = db.clone();
    let cr = conn_ref.clone();
    attrs.insert(
        CompactString::from("cursor"),
        PyObject::native_closure("cursor", move |_args| {
            let conn = cr.lock().unwrap().clone();
            Ok(build_cursor_object_with_conn(db_ref.clone(), conn))
        }),
    );

    // execute(sql, params=()) — convenience: creates cursor, executes, returns cursor
    let db_ref = db.clone();
    let cr = conn_ref.clone();
    attrs.insert(
        CompactString::from("execute"),
        PyObject::native_closure("execute", move |args| {
            let conn = cr.lock().unwrap().clone();
            let cursor = build_cursor_object_with_conn(db_ref.clone(), conn);
            if let PyObjectPayload::Instance(ref d) = cursor.payload {
                let exec_fn = d.attrs.read().get(&CompactString::from("execute")).cloned();
                if let Some(f) = exec_fn {
                    if let PyObjectPayload::NativeClosure(nc) = &f.payload {
                        (nc.func)(args)?;
                    }
                }
            }
            Ok(cursor)
        }),
    );

    // executemany(sql, seq_of_params)
    let db_ref = db.clone();
    let cr = conn_ref.clone();
    attrs.insert(
        CompactString::from("executemany"),
        PyObject::native_closure("executemany", move |args| {
            let conn = cr.lock().unwrap().clone();
            let cursor = build_cursor_object_with_conn(db_ref.clone(), conn);
            if let PyObjectPayload::Instance(ref d) = cursor.payload {
                let exec_fn = d
                    .attrs
                    .read()
                    .get(&CompactString::from("executemany"))
                    .cloned();
                if let Some(f) = exec_fn {
                    if let PyObjectPayload::NativeClosure(nc) = &f.payload {
                        (nc.func)(args)?;
                    }
                }
            }
            Ok(cursor)
        }),
    );

    // executescript(sql_script) — convenience on connection
    let db_ref = db.clone();
    let cr = conn_ref.clone();
    attrs.insert(
        CompactString::from("executescript"),
        PyObject::native_closure("executescript", move |args| {
            let conn = cr.lock().unwrap().clone();
            let cursor = build_cursor_object_with_conn(db_ref.clone(), conn);
            if let PyObjectPayload::Instance(ref d) = cursor.payload {
                let exec_fn = d
                    .attrs
                    .read()
                    .get(&CompactString::from("executescript"))
                    .cloned();
                if let Some(f) = exec_fn {
                    if let PyObjectPayload::NativeClosure(nc) = &f.payload {
                        (nc.func)(args)?;
                    }
                }
            }
            Ok(cursor)
        }),
    );

    // commit()
    attrs.insert(
        CompactString::from("commit"),
        PyObject::native_function("commit", |_args| Ok(PyObject::none())),
    );

    // rollback()
    attrs.insert(
        CompactString::from("rollback"),
        PyObject::native_function("rollback", |_args| Ok(PyObject::none())),
    );

    // close()
    let db_ref = db.clone();
    attrs.insert(
        CompactString::from("close"),
        PyObject::native_closure("close", move |_args| {
            let mut guard = db_ref.lock().unwrap();
            guard.closed = true;
            Ok(PyObject::none())
        }),
    );

    // create_function(name, num_params, func)
    attrs.insert(
        CompactString::from("create_function"),
        PyObject::native_function("create_function", |_args| Ok(PyObject::none())),
    );

    // total_changes
    let db_ref = db.clone();
    attrs.insert(
        CompactString::from("total_changes"),
        PyObject::native_closure("total_changes", move |_args| {
            let guard = db_ref.lock().unwrap();
            let total: usize = guard.tables.values().map(|t| t.rows.len()).sum();
            Ok(PyObject::int(total as i64))
        }),
    );

    // isolation_level
    attrs.insert(
        CompactString::from("isolation_level"),
        PyObject::str_val(CompactString::from("")),
    );

    // row_factory — set via conn.row_factory = sqlite3.Row
    // Cursors read it dynamically via get_attr on the connection
    attrs.insert(CompactString::from("row_factory"), PyObject::none());

    // __enter__ / __exit__ for context manager
    attrs.insert(
        CompactString::from("__enter__"),
        PyObject::native_function("__enter__", |args| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            Ok(args[0].clone())
        }),
    );

    let db_ref = db.clone();
    attrs.insert(
        CompactString::from("__exit__"),
        PyObject::native_closure("__exit__", move |_args| {
            let guard = db_ref.lock().unwrap();
            drop(guard);
            Ok(PyObject::bool_val(false))
        }),
    );

    let cls = PyObject::class(CompactString::from("Connection"), vec![], IndexMap::new());
    let connection = PyObject::instance_with_attrs(cls, attrs);
    // Populate self-reference so cursor() can pass connection for row_factory
    *conn_ref.lock().unwrap() = Some(connection.clone());
    connection
}

pub(super) fn sqlite3_connect(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "connect() requires 1 argument: database",
        ));
    }
    let path = args[0].py_to_string();
    let db = if path == ":memory:" {
        Arc::new(Mutex::new(Database::new(&path)))
    } else {
        let mut registry = DB_REGISTRY.lock().unwrap();
        registry
            .entry(path.clone())
            .or_insert_with(|| Arc::new(Mutex::new(Database::new(&path))))
            .clone()
    };
    Ok(build_connection_object(db))
}
