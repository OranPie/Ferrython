//! Database stdlib modules: sqlite3 (in-memory dict-based implementation)

use compact_str::CompactString;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectRef};
use indexmap::IndexMap;

mod connection;
mod cursor;
mod parser;
mod row;
mod sql;
mod storage;

use connection::sqlite3_connect;

pub fn create_sqlite3_module() -> PyObjectRef {
    make_module(
        "sqlite3",
        vec![
            ("connect", make_builtin(sqlite3_connect)),
            ("version", PyObject::str_val(CompactString::from("2.6.0"))),
            (
                "sqlite_version",
                PyObject::str_val(CompactString::from("3.39.0")),
            ),
            ("PARSE_DECLTYPES", PyObject::int(1)),
            ("PARSE_COLNAMES", PyObject::int(2)),
            ("apilevel", PyObject::str_val(CompactString::from("2.0"))),
            (
                "paramstyle",
                PyObject::str_val(CompactString::from("qmark")),
            ),
            ("threadsafety", PyObject::int(1)),
            (
                "Error",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError),
            ),
            (
                "DatabaseError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError),
            ),
            (
                "OperationalError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError),
            ),
            (
                "IntegrityError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError),
            ),
            (
                "ProgrammingError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError),
            ),
            (
                "InterfaceError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::RuntimeError),
            ),
            (
                "Row",
                PyObject::class(CompactString::from("Row"), vec![], IndexMap::new()),
            ),
        ],
    )
}
