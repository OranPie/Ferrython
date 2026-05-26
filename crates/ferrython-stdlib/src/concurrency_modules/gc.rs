use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    check_args_min, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

// ── gc module ──

pub fn create_gc_module() -> PyObjectRef {
    make_module(
        "gc",
        vec![
            (
                "enable",
                make_builtin(|_| {
                    ferrython_gc::enable();
                    Ok(PyObject::none())
                }),
            ),
            (
                "disable",
                make_builtin(|_| {
                    ferrython_gc::disable();
                    Ok(PyObject::none())
                }),
            ),
            (
                "isenabled",
                make_builtin(|_| Ok(PyObject::bool_val(ferrython_gc::is_enabled()))),
            ),
            (
                "collect",
                make_builtin(|_| {
                    let collected = ferrython_gc::collect();
                    Ok(PyObject::int(collected as i64))
                }),
            ),
            (
                "get_threshold",
                make_builtin(|_| {
                    let (g0, g1, g2) = ferrython_gc::get_threshold();
                    Ok(PyObject::tuple(vec![
                        PyObject::int(g0 as i64),
                        PyObject::int(g1 as i64),
                        PyObject::int(g2 as i64),
                    ]))
                }),
            ),
            (
                "set_threshold",
                make_builtin(|args| {
                    check_args_min("gc.set_threshold", args, 1)?;
                    let g0 = args[0]
                        .as_int()
                        .ok_or_else(|| PyException::type_error("threshold must be an integer"))?
                        as u64;
                    let g1 = args.get(1).and_then(|a| a.as_int()).unwrap_or(10) as u64;
                    let g2 = args.get(2).and_then(|a| a.as_int()).unwrap_or(10) as u64;
                    ferrython_gc::set_threshold(g0, g1, g2);
                    Ok(PyObject::none())
                }),
            ),
            (
                "get_stats",
                make_builtin(|_| {
                    let stats = ferrython_gc::get_stats();
                    let entry = PyObject::dict({
                        let mut m = IndexMap::new();
                        m.insert(
                            HashableKey::str_key(CompactString::from("collections")),
                            PyObject::int(stats.collections as i64),
                        );
                        m.insert(
                            HashableKey::str_key(CompactString::from("collected")),
                            PyObject::int(0),
                        );
                        m.insert(
                            HashableKey::str_key(CompactString::from("uncollectable")),
                            PyObject::int(0),
                        );
                        m
                    });
                    // CPython returns a list of 3 dicts, one per generation
                    Ok(PyObject::list(vec![entry.clone(), entry.clone(), entry]))
                }),
            ),
            (
                "get_count",
                make_builtin(|_| {
                    let stats = ferrython_gc::get_stats();
                    Ok(PyObject::tuple(vec![
                        PyObject::int(stats.allocations as i64),
                        PyObject::int(0),
                        PyObject::int(0),
                    ]))
                }),
            ),
            (
                "get_objects",
                make_builtin(|_| {
                    // CPython returns all tracked objects; we return empty list (Rust manages memory)
                    Ok(PyObject::list(vec![]))
                }),
            ),
            (
                "get_referrers",
                make_builtin(|_| Ok(PyObject::list(vec![]))),
            ),
            (
                "get_referents",
                make_builtin(|_| Ok(PyObject::list(vec![]))),
            ),
            ("freeze", make_builtin(|_| Ok(PyObject::none()))),
            ("unfreeze", make_builtin(|_| Ok(PyObject::none()))),
            ("get_freeze_count", make_builtin(|_| Ok(PyObject::int(0)))),
            ("get_debug", make_builtin(|_| Ok(PyObject::int(0)))),
            ("set_debug", make_builtin(|_| Ok(PyObject::none()))),
            (
                "is_tracked",
                make_builtin(|_| Ok(PyObject::bool_val(false))),
            ),
            (
                "is_finalized",
                make_builtin(|_| Ok(PyObject::bool_val(false))),
            ),
            ("callbacks", PyObject::list(vec![])),
            ("garbage", PyObject::list(vec![])),
            ("DEBUG_STATS", PyObject::int(1)),
            ("DEBUG_COLLECTABLE", PyObject::int(2)),
            ("DEBUG_UNCOLLECTABLE", PyObject::int(4)),
            ("DEBUG_SAVEALL", PyObject::int(32)),
            ("DEBUG_LEAK", PyObject::int(38)),
        ],
    )
}
