//! sqlite3 in-memory storage types.

use compact_str::CompactString;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

/// Global registry: canonical DB path → shared Database.
/// Multiple sqlite3.connect() calls to the same path reuse the same in-memory DB.
/// The special path ":memory:" always gets a fresh database.
pub(super) static DB_REGISTRY: LazyLock<Mutex<HashMap<String, Arc<Mutex<Database>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// ── Database storage ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(super) struct Column {
    pub(super) name: String,
    pub(super) col_type: String,
    pub(super) primary_key: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(super) struct Table {
    pub(super) name: String,
    pub(super) columns: Vec<Column>,
    pub(super) rows: Vec<IndexMap<String, DbValue>>,
    pub(super) auto_increment: i64,
}

#[derive(Debug, Clone)]
pub(super) enum DbValue {
    Null,
    Int(i64),
    Float(f64),
    Text(String),
}

impl DbValue {
    pub(super) fn to_pyobject(&self) -> PyObjectRef {
        match self {
            DbValue::Null => PyObject::none(),
            DbValue::Int(i) => PyObject::int(*i),
            DbValue::Float(f) => PyObject::float(*f),
            DbValue::Text(s) => PyObject::str_val(CompactString::from(s.as_str())),
        }
    }

    pub(super) fn from_pyobject(obj: &PyObjectRef) -> Self {
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
pub(super) struct Database {
    pub(super) tables: IndexMap<String, Table>,
    pub(super) path: String,
    pub(super) closed: bool,
}

impl Database {
    pub(super) fn new(path: &str) -> Self {
        Self {
            tables: IndexMap::new(),
            path: path.to_string(),
            closed: false,
        }
    }
}
