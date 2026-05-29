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
use std::sync::{Arc, Mutex};

mod dialect;
mod dict_io;
mod reader;
mod writer;

use dialect::{
    csv_get_dialect, csv_list_dialects, csv_register_dialect, csv_sniffer_ctor,
    csv_unregister_dialect,
};
use dict_io::{csv_dict_reader, csv_dict_writer};
use reader::csv_reader;
use writer::csv_writer;

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
            (
                "field_size_limit",
                make_builtin(|args: &[PyObjectRef]| {
                    // field_size_limit([new_limit]) — get/set maximum field size
                    static FIELD_SIZE_LIMIT: std::sync::atomic::AtomicI64 =
                        std::sync::atomic::AtomicI64::new(131072);
                    let old = FIELD_SIZE_LIMIT.load(std::sync::atomic::Ordering::Relaxed);
                    if let Some(n) = args.first().and_then(|a| a.as_int()) {
                        FIELD_SIZE_LIMIT.store(n, std::sync::atomic::Ordering::Relaxed);
                    }
                    Ok(PyObject::int(old))
                }),
            ),
            ("Error", PyObject::exception_type(ExceptionKind::CsvError)),
            ("QUOTE_ALL", PyObject::int(1)),
            ("QUOTE_MINIMAL", PyObject::int(0)),
            ("QUOTE_NONNUMERIC", PyObject::int(2)),
            ("QUOTE_NONE", PyObject::int(3)),
            ("Dialect", {
                let mut ns = IndexMap::new();
                ns.insert(
                    CompactString::from("delimiter"),
                    PyObject::str_val(CompactString::from(",")),
                );
                ns.insert(
                    CompactString::from("quotechar"),
                    PyObject::str_val(CompactString::from("\"")),
                );
                ns.insert(CompactString::from("doublequote"), PyObject::bool_val(true));
                ns.insert(
                    CompactString::from("skipinitialspace"),
                    PyObject::bool_val(false),
                );
                ns.insert(
                    CompactString::from("lineterminator"),
                    PyObject::str_val(CompactString::from("\r\n")),
                );
                ns.insert(CompactString::from("quoting"), PyObject::int(0));
                PyObject::class(CompactString::from("Dialect"), vec![], ns)
            }),
            ("excel", {
                let mut ns = IndexMap::new();
                ns.insert(
                    CompactString::from("delimiter"),
                    PyObject::str_val(CompactString::from(",")),
                );
                ns.insert(
                    CompactString::from("quotechar"),
                    PyObject::str_val(CompactString::from("\"")),
                );
                ns.insert(CompactString::from("doublequote"), PyObject::bool_val(true));
                ns.insert(
                    CompactString::from("skipinitialspace"),
                    PyObject::bool_val(false),
                );
                ns.insert(
                    CompactString::from("lineterminator"),
                    PyObject::str_val(CompactString::from("\r\n")),
                );
                ns.insert(CompactString::from("quoting"), PyObject::int(0));
                let cls = PyObject::class(CompactString::from("excel"), vec![], ns);
                PyObject::instance(cls)
            }),
            ("excel_tab", {
                let mut ns = IndexMap::new();
                ns.insert(
                    CompactString::from("delimiter"),
                    PyObject::str_val(CompactString::from("\t")),
                );
                ns.insert(
                    CompactString::from("quotechar"),
                    PyObject::str_val(CompactString::from("\"")),
                );
                ns.insert(CompactString::from("doublequote"), PyObject::bool_val(true));
                ns.insert(
                    CompactString::from("skipinitialspace"),
                    PyObject::bool_val(false),
                );
                ns.insert(
                    CompactString::from("lineterminator"),
                    PyObject::str_val(CompactString::from("\r\n")),
                );
                ns.insert(CompactString::from("quoting"), PyObject::int(0));
                let cls = PyObject::class(CompactString::from("excel_tab"), vec![], ns);
                PyObject::instance(cls)
            }),
            ("unix_dialect", {
                let mut ns = IndexMap::new();
                ns.insert(
                    CompactString::from("delimiter"),
                    PyObject::str_val(CompactString::from(",")),
                );
                ns.insert(
                    CompactString::from("quotechar"),
                    PyObject::str_val(CompactString::from("\"")),
                );
                ns.insert(CompactString::from("doublequote"), PyObject::bool_val(true));
                ns.insert(
                    CompactString::from("skipinitialspace"),
                    PyObject::bool_val(false),
                );
                ns.insert(
                    CompactString::from("lineterminator"),
                    PyObject::str_val(CompactString::from("\n")),
                );
                ns.insert(CompactString::from("quoting"), PyObject::int(1));
                let cls = PyObject::class(CompactString::from("unix_dialect"), vec![], ns);
                PyObject::instance(cls)
            }),
        ],
    )
}
