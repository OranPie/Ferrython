//! sqlite3.Row implementation.

use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use indexmap::IndexMap;
use std::sync::Arc;

// ── sqlite3.Row implementation ─────────────────────────────────────────

pub(super) fn build_sqlite_row(columns: &[String], values: &[PyObjectRef]) -> PyObjectRef {
    let col_names: Vec<String> = columns.to_vec();
    let val_list: Vec<PyObjectRef> = values.to_vec();
    let mut attrs = IndexMap::new();

    // Store column-value pairs for string key access
    let col_map: IndexMap<CompactString, PyObjectRef> = col_names
        .iter()
        .zip(val_list.iter())
        .map(|(k, v)| (CompactString::from(k.as_str()), v.clone()))
        .collect();
    let cm = Arc::new(col_map.clone());

    // __getitem__ — access by string key or integer index
    let cm2 = cm.clone();
    let vl = val_list.clone();
    attrs.insert(
        CompactString::from("__getitem__"),
        PyObject::native_closure("Row.__getitem__", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("__getitem__ requires a key"));
            }
            match &args[0].payload {
                PyObjectPayload::Str(key) => cm2
                    .get(key.as_str())
                    .cloned()
                    .ok_or_else(|| PyException::key_error(format!("'{}'", key))),
                PyObjectPayload::Int(idx) => {
                    let i = idx.to_i64().unwrap_or(0);
                    let idx = if i < 0 {
                        (vl.len() as i64 + i) as usize
                    } else {
                        i as usize
                    };
                    vl.get(idx)
                        .cloned()
                        .ok_or_else(|| PyException::index_error("index out of range"))
                }
                _ => Err(PyException::type_error(
                    "Row indices must be integers or strings",
                )),
            }
        }),
    );

    // keys()
    let cn = col_names.clone();
    attrs.insert(
        CompactString::from("keys"),
        PyObject::native_closure("Row.keys", move |_| {
            Ok(PyObject::list(
                cn.iter()
                    .map(|s| PyObject::str_val(CompactString::from(s.as_str())))
                    .collect(),
            ))
        }),
    );

    // values() (the row data as a tuple)
    let vl = val_list.clone();
    attrs.insert(
        CompactString::from("values"),
        PyObject::native_closure("Row.values", move |_| Ok(PyObject::list(vl.clone()))),
    );

    // __len__
    let n = val_list.len();
    attrs.insert(
        CompactString::from("__len__"),
        PyObject::native_closure("Row.__len__", move |_| Ok(PyObject::int(n as i64))),
    );

    // __repr__
    let cm3 = cm.clone();
    attrs.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("Row.__repr__", move |_| {
            let pairs: Vec<String> = cm3
                .iter()
                .map(|(k, v)| format!("{}={}", k, v.py_to_string()))
                .collect();
            Ok(PyObject::str_val(CompactString::from(format!(
                "Row({})",
                pairs.join(", ")
            ))))
        }),
    );

    // __iter__ — iterate over values
    let vl = val_list.clone();
    attrs.insert(
        CompactString::from("__iter__"),
        PyObject::native_closure("Row.__iter__", move |_| Ok(PyObject::list(vl.clone()))),
    );

    // Make it tuple-like: store values as a tuple
    let cls = PyObject::class(CompactString::from("Row"), vec![], IndexMap::new());
    PyObject::instance_with_attrs(cls, attrs)
}
