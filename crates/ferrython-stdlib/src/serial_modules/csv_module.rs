use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    check_args_min, make_builtin, make_module, IteratorData, NativeClosureData, PyCell, PyObject,
    PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

mod dialect;
mod dict_io;
mod reader;
mod writer;

use dialect::{
    csv_dialect_init, csv_get_dialect, csv_list_dialects, csv_register_dialect, csv_sniffer_ctor,
    csv_unregister_dialect,
};
use dict_io::{csv_dict_reader, csv_dict_writer};
use reader::csv_reader;
use writer::csv_writer;

static FIELD_SIZE_LIMIT: AtomicI64 = AtomicI64::new(131072);

pub(super) fn current_field_size_limit() -> i64 {
    FIELD_SIZE_LIMIT.load(Ordering::Relaxed)
}

fn csv_field_size_limit(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() > 1 {
        return Err(PyException::type_error(
            "field_size_limit expected at most 1 argument",
        ));
    }
    let old = current_field_size_limit();
    if let Some(limit) = args.first() {
        if !matches!(
            limit.payload,
            PyObjectPayload::Int(_) | PyObjectPayload::Bool(_)
        ) {
            return Err(PyException::type_error("limit must be an integer"));
        }
        FIELD_SIZE_LIMIT.store(limit.to_int()?, Ordering::Relaxed);
    }
    Ok(PyObject::int(old))
}

pub fn create_csv_module() -> PyObjectRef {
    let all_names = vec![
        "reader",
        "writer",
        "DictReader",
        "DictWriter",
        "register_dialect",
        "unregister_dialect",
        "get_dialect",
        "list_dialects",
        "Sniffer",
        "field_size_limit",
        "Error",
        "QUOTE_ALL",
        "QUOTE_MINIMAL",
        "QUOTE_NONNUMERIC",
        "QUOTE_NONE",
        "Dialect",
        "excel",
        "excel_tab",
        "unix_dialect",
        "__doc__",
        "__version__",
    ]
    .into_iter()
    .map(|name| PyObject::str_val(CompactString::from(name)))
    .collect();

    make_module(
        "csv",
        vec![
            (
                "__doc__",
                PyObject::str_val(CompactString::from("CSV parsing and writing")),
            ),
            ("__version__", PyObject::str_val(CompactString::from("1.0"))),
            ("__all__", PyObject::list(all_names)),
            ("reader", make_builtin(csv_reader)),
            ("writer", make_builtin(csv_writer)),
            ("DictReader", make_builtin(csv_dict_reader)),
            (
                "DictWriter",
                PyObject::native_function("csv.DictWriter", csv_dict_writer),
            ),
            ("register_dialect", make_builtin(csv_register_dialect)),
            ("unregister_dialect", make_builtin(csv_unregister_dialect)),
            ("get_dialect", make_builtin(csv_get_dialect)),
            ("list_dialects", make_builtin(csv_list_dialects)),
            ("Sniffer", make_builtin(csv_sniffer_ctor)),
            ("field_size_limit", make_builtin(csv_field_size_limit)),
            ("Error", PyObject::exception_type(ExceptionKind::CsvError)),
            ("QUOTE_ALL", PyObject::int(1)),
            ("QUOTE_MINIMAL", PyObject::int(0)),
            ("QUOTE_NONNUMERIC", PyObject::int(2)),
            ("QUOTE_NONE", PyObject::int(3)),
            (
                "Dialect",
                make_csv_dialect_class("Dialect", None, ',', "\r\n", 0),
            ),
            (
                "excel",
                make_csv_dialect_class("excel", None, ',', "\r\n", 0),
            ),
            (
                "excel_tab",
                make_csv_dialect_class("excel_tab", None, '\t', "\r\n", 0),
            ),
            (
                "unix_dialect",
                make_csv_dialect_class("unix_dialect", None, ',', "\n", 1),
            ),
        ],
    )
}

fn make_csv_dialect_class(
    name: &str,
    bases: Option<Vec<PyObjectRef>>,
    delimiter: char,
    lineterminator: &str,
    quoting: i64,
) -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("delimiter"),
        PyObject::str_val(CompactString::from(delimiter.to_string().as_str())),
    );
    ns.insert(
        CompactString::from("quotechar"),
        PyObject::str_val(CompactString::from("\"")),
    );
    ns.insert(CompactString::from("escapechar"), PyObject::none());
    ns.insert(CompactString::from("doublequote"), PyObject::bool_val(true));
    ns.insert(
        CompactString::from("skipinitialspace"),
        PyObject::bool_val(false),
    );
    ns.insert(
        CompactString::from("lineterminator"),
        PyObject::str_val(CompactString::from(lineterminator)),
    );
    ns.insert(CompactString::from("quoting"), PyObject::int(quoting));
    ns.insert(CompactString::from("strict"), PyObject::bool_val(false));
    ns.insert(
        CompactString::from("__init__"),
        make_builtin(csv_dialect_init),
    );
    PyObject::class(CompactString::from(name), bases.unwrap_or_default(), ns)
}
