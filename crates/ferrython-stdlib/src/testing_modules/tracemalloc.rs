use compact_str::CompactString;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef};

// ── tracemalloc module ──

pub fn create_tracemalloc_module() -> PyObjectRef {
    use parking_lot::RwLock;
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

    static TRACING: AtomicBool = AtomicBool::new(false);
    static NFRAME: AtomicI64 = AtomicI64::new(1);

    // Snapshot data: list of (filename, lineno, size) triples
    static ALLOCS: std::sync::LazyLock<RwLock<Vec<(String, i64, i64)>>> =
        std::sync::LazyLock::new(|| RwLock::new(Vec::new()));

    let start = PyObject::native_closure("tracemalloc.start", move |args: &[PyObjectRef]| {
        let nframe = if !args.is_empty() {
            args[0].as_int().unwrap_or(1).max(1)
        } else {
            1
        };
        NFRAME.store(nframe, Ordering::Relaxed);
        TRACING.store(true, Ordering::Relaxed);
        ALLOCS.write().clear();
        Ok(PyObject::none())
    });
    let stop = PyObject::native_closure("tracemalloc.stop", move |_: &[PyObjectRef]| {
        TRACING.store(false, Ordering::Relaxed);
        Ok(PyObject::none())
    });
    let is_tracing =
        PyObject::native_closure("tracemalloc.is_tracing", move |_: &[PyObjectRef]| {
            Ok(PyObject::bool_val(TRACING.load(Ordering::Relaxed)))
        });
    let get_traced_memory = PyObject::native_closure(
        "tracemalloc.get_traced_memory",
        move |_: &[PyObjectRef]| {
            // Return (current, peak) in bytes — use process RSS as estimate
            let current = {
                #[cfg(target_os = "linux")]
                {
                    std::fs::read_to_string("/proc/self/statm")
                        .ok()
                        .and_then(|s| {
                            s.split_whitespace()
                                .nth(1)
                                .and_then(|v| v.parse::<i64>().ok())
                        })
                        .map(|pages| pages * 4096)
                        .unwrap_or(0)
                }
                #[cfg(not(target_os = "linux"))]
                {
                    0i64
                }
            };
            Ok(PyObject::tuple(vec![
                PyObject::int(current),
                PyObject::int(current),
            ]))
        },
    );
    let get_tracemalloc_memory = make_builtin(|_: &[PyObjectRef]| Ok(PyObject::int(0)));
    let take_snapshot =
        PyObject::native_closure("tracemalloc.take_snapshot", move |_: &[PyObjectRef]| {
            let allocs = ALLOCS.read().clone();
            let traces = PyObject::list(
                allocs
                    .iter()
                    .map(|(f, l, s)| {
                        PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from(f.as_str())),
                            PyObject::int(*l),
                            PyObject::int(*s),
                        ])
                    })
                    .collect(),
            );
            Ok(make_module(
                "Snapshot",
                vec![
                    ("traces", traces),
                    (
                        "statistics",
                        make_builtin(|_: &[PyObjectRef]| Ok(PyObject::list(vec![]))),
                    ),
                    (
                        "compare_to",
                        make_builtin(|_: &[PyObjectRef]| Ok(PyObject::list(vec![]))),
                    ),
                    (
                        "filter_traces",
                        make_builtin(|_: &[PyObjectRef]| {
                            Ok(make_module(
                                "_filtered",
                                vec![
                                    ("traces", PyObject::list(vec![])),
                                    (
                                        "statistics",
                                        make_builtin(|_: &[PyObjectRef]| {
                                            Ok(PyObject::list(vec![]))
                                        }),
                                    ),
                                ],
                            ))
                        }),
                    ),
                ],
            ))
        });
    let get_object_traceback = make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none()));
    let clear_traces =
        PyObject::native_closure("tracemalloc.clear_traces", move |_: &[PyObjectRef]| {
            ALLOCS.write().clear();
            Ok(PyObject::none())
        });

    make_module(
        "tracemalloc",
        vec![
            ("start", start),
            ("stop", stop),
            ("is_tracing", is_tracing),
            ("get_traced_memory", get_traced_memory),
            ("get_tracemalloc_memory", get_tracemalloc_memory),
            ("take_snapshot", take_snapshot),
            ("get_object_traceback", get_object_traceback),
            ("clear_traces", clear_traces),
        ],
    )
}
