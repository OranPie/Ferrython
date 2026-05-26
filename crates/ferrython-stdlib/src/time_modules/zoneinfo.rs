//! `zoneinfo` stdlib module implementation.

use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

pub fn create_zoneinfo_module() -> PyObjectRef {
    // ZoneInfo class — wraps IANA timezone names
    let mut zi_ns = IndexMap::new();
    zi_ns.insert(
        CompactString::from("__init__"),
        make_builtin(|args: &[PyObjectRef]| {
            // ZoneInfo(key) — store the key
            if args.len() < 2 {
                return Err(PyException::type_error("ZoneInfo() requires key argument"));
            }
            let key = args[1].py_to_string();
            if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                let mut w = inst.attrs.write();
                w.insert(
                    CompactString::from("key"),
                    PyObject::str_val(CompactString::from(key.as_str())),
                );
                w.insert(
                    CompactString::from("_name"),
                    PyObject::str_val(CompactString::from(key.as_str())),
                );
            }
            Ok(PyObject::none())
        }),
    );
    zi_ns.insert(
        CompactString::from("__repr__"),
        make_builtin(|args: &[PyObjectRef]| {
            let key = args
                .first()
                .and_then(|a| a.get_attr("key"))
                .map(|k| k.py_to_string())
                .unwrap_or_default();
            Ok(PyObject::str_val(CompactString::from(format!(
                "zoneinfo.ZoneInfo(key='{}')",
                key
            ))))
        }),
    );
    zi_ns.insert(
        CompactString::from("__str__"),
        make_builtin(|args: &[PyObjectRef]| {
            let key = args
                .first()
                .and_then(|a| a.get_attr("key"))
                .map(|k| k.py_to_string())
                .unwrap_or_default();
            Ok(PyObject::str_val(CompactString::from(key)))
        }),
    );
    zi_ns.insert(
        CompactString::from("utcoffset"),
        make_builtin(|_args: &[PyObjectRef]| {
            // Return None (unknown offset for now)
            Ok(PyObject::none())
        }),
    );
    zi_ns.insert(
        CompactString::from("tzname"),
        make_builtin(|args: &[PyObjectRef]| {
            let key = args
                .first()
                .and_then(|a| a.get_attr("key"))
                .map(|k| k.py_to_string())
                .unwrap_or_default();
            Ok(PyObject::str_val(CompactString::from(key)))
        }),
    );
    zi_ns.insert(
        CompactString::from("dst"),
        make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
    );
    let zi_cls = PyObject::class(CompactString::from("ZoneInfo"), vec![], zi_ns);

    // ZoneInfoNotFoundError — subclass of KeyError
    let err_cls = PyObject::class(
        CompactString::from("ZoneInfoNotFoundError"),
        vec![],
        IndexMap::new(),
    );

    // available_timezones()
    let available_tz = make_builtin(|_args: &[PyObjectRef]| {
        let tzs = vec![
            "UTC",
            "US/Eastern",
            "US/Central",
            "US/Mountain",
            "US/Pacific",
            "Europe/London",
            "Europe/Paris",
            "Europe/Berlin",
            "Asia/Tokyo",
            "Asia/Shanghai",
            "Australia/Sydney",
        ];
        let mut items = IndexMap::new();
        for tz in tzs {
            let key = HashableKey::str_key(CompactString::from(tz));
            items.insert(key, PyObject::str_val(CompactString::from(tz)));
        }
        Ok(PyObject::frozenset(items))
    });

    make_module(
        "zoneinfo",
        vec![
            ("ZoneInfo", zi_cls),
            ("ZoneInfoNotFoundError", err_cls),
            ("available_timezones", available_tz),
        ],
    )
}

// ── weakref module ──
