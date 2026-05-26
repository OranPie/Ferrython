use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

// ── timeit module ──

/// Call a callable (NativeFunction or NativeClosure) with no args
fn call_callable(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::NativeFunction(nf) => (nf.func)(&[]),
        PyObjectPayload::NativeClosure(nc) => (nc.func)(&[]),
        _ => Ok(PyObject::none()),
    }
}

/// Check if object is callable
fn is_callable(obj: &PyObjectRef) -> bool {
    matches!(
        &obj.payload,
        PyObjectPayload::NativeFunction(_)
            | PyObjectPayload::NativeClosure(_)
            | PyObjectPayload::Function(_)
            | PyObjectPayload::BoundMethod { .. }
    )
}

pub fn create_timeit_module() -> PyObjectRef {
    // timeit.default_timer — alias for time.perf_counter
    let default_timer = make_builtin(|_args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        Ok(PyObject::float(t))
    });

    // timeit.timeit(stmt='pass', setup='pass', timer=<default>, number=1000000, globals=None)
    // If stmt is callable, calls it `number` times and returns total elapsed seconds
    // If stmt is a string, can't execute without VM — returns estimated time
    let timeit_fn = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        use std::time::Instant;
        // Extract kwargs dict if last arg is dict
        let (positional, kwargs) = if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw_map) = &last.payload {
                (&args[..args.len() - 1], Some(kw_map.read().clone()))
            } else {
                (args, None)
            }
        } else {
            (args, None)
        };

        // stmt from positional[0] or kwargs['stmt']
        let stmt = positional.first().cloned().or_else(|| {
            kwargs.as_ref().and_then(|kw| {
                kw.get(&HashableKey::str_key(CompactString::from("stmt")))
                    .cloned()
            })
        });
        // setup from positional[1] or kwargs['setup']
        let setup = if positional.len() > 1 {
            Some(positional[1].clone())
        } else {
            kwargs.as_ref().and_then(|kw| {
                kw.get(&HashableKey::str_key(CompactString::from("setup")))
                    .cloned()
            })
        };
        // number from positional[2] or kwargs['number']
        let number: i64 = if positional.len() > 2 {
            positional[2].as_int().unwrap_or(1_000_000)
        } else {
            kwargs
                .as_ref()
                .and_then(|kw| {
                    kw.get(&HashableKey::str_key(CompactString::from("number")))
                        .and_then(|v| v.as_int())
                })
                .unwrap_or(1_000_000)
        };

        // Run setup if callable
        if let Some(ref s) = setup {
            if is_callable(s) {
                let _ = call_callable(s);
            }
        }

        if let Some(ref s) = stmt {
            if is_callable(s) {
                // Actually call the function `number` times
                let start = Instant::now();
                for _ in 0..number {
                    let _ = call_callable(s);
                }
                return Ok(PyObject::float(start.elapsed().as_secs_f64()));
            }
        }

        // String stmt or no stmt — measure overhead of loop
        let start = Instant::now();
        for _ in 0..number {
            std::hint::black_box(0);
        }
        Ok(PyObject::float(start.elapsed().as_secs_f64()))
    });

    // timeit.repeat(stmt, setup, repeat=5, number=1000000)
    let repeat_fn = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        use std::time::Instant;
        let (positional, kwargs) = if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw_map) = &last.payload {
                (&args[..args.len() - 1], Some(kw_map.read().clone()))
            } else {
                (args, None)
            }
        } else {
            (args, None)
        };

        let stmt = positional.first().cloned().or_else(|| {
            kwargs.as_ref().and_then(|kw| {
                kw.get(&HashableKey::str_key(CompactString::from("stmt")))
                    .cloned()
            })
        });
        let setup = if positional.len() > 1 {
            Some(positional[1].clone())
        } else {
            kwargs.as_ref().and_then(|kw| {
                kw.get(&HashableKey::str_key(CompactString::from("setup")))
                    .cloned()
            })
        };
        let repeat_count: i64 = if positional.len() > 2 {
            positional[2].as_int().unwrap_or(5)
        } else {
            kwargs
                .as_ref()
                .and_then(|kw| {
                    kw.get(&HashableKey::str_key(CompactString::from("repeat")))
                        .and_then(|v| v.as_int())
                })
                .unwrap_or(5)
        };
        let number: i64 = if positional.len() > 3 {
            positional[3].as_int().unwrap_or(1_000_000)
        } else {
            kwargs
                .as_ref()
                .and_then(|kw| {
                    kw.get(&HashableKey::str_key(CompactString::from("number")))
                        .and_then(|v| v.as_int())
                })
                .unwrap_or(1_000_000)
        };

        if let Some(ref s) = setup {
            if is_callable(s) {
                let _ = call_callable(s);
            }
        }

        let is_stmt_callable = stmt.as_ref().map(|s| is_callable(s)).unwrap_or(false);
        let mut results = Vec::new();
        for _ in 0..repeat_count {
            let start = Instant::now();
            if is_stmt_callable {
                for _ in 0..number {
                    let _ = call_callable(stmt.as_ref().unwrap());
                }
            } else {
                for _ in 0..number {
                    std::hint::black_box(0);
                }
            }
            results.push(PyObject::float(start.elapsed().as_secs_f64()));
        }
        Ok(PyObject::list(results))
    });

    // Timer class
    let timer_cls = PyObject::class(CompactString::from("Timer"), vec![], IndexMap::new());
    let tc = timer_cls.clone();
    let timer_fn = PyObject::native_closure(
        "Timer",
        move |args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
            let inst = PyObject::instance(tc.clone());
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                let stmt = args.first().cloned().unwrap_or_else(PyObject::none);
                let setup = args.get(1).cloned().unwrap_or_else(PyObject::none);
                attrs.insert(CompactString::from("stmt"), stmt.clone());
                attrs.insert(CompactString::from("setup"), setup.clone());

                // timeit(number=1000000)
                let stmt2 = stmt.clone();
                let setup2 = setup.clone();
                attrs.insert(
                    CompactString::from("timeit"),
                    PyObject::native_closure(
                        "timeit",
                        move |inner_args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
                            use std::time::Instant;
                            let number: i64 = if inner_args.is_empty() {
                                1_000_000
                            } else if inner_args.len() == 1 {
                                inner_args[0].as_int().unwrap_or(1_000_000)
                            } else {
                                inner_args[1].as_int().unwrap_or(1_000_000)
                            };

                            if is_callable(&setup2) {
                                let _ = call_callable(&setup2);
                            }

                            let start = Instant::now();
                            if is_callable(&stmt2) {
                                for _ in 0..number {
                                    let _ = call_callable(&stmt2);
                                }
                            } else {
                                for _ in 0..number {
                                    std::hint::black_box(0);
                                }
                            }
                            Ok(PyObject::float(start.elapsed().as_secs_f64()))
                        },
                    ),
                );
                // repeat(repeat=5, number=1000000)
                let stmt3 = stmt.clone();
                let setup3 = setup.clone();
                attrs.insert(
                    CompactString::from("repeat"),
                    PyObject::native_closure(
                        "repeat",
                        move |inner_args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
                            use std::time::Instant;
                            let repeat_count: i64 = if inner_args.is_empty() {
                                5
                            } else if inner_args.len() == 1 {
                                inner_args[0].as_int().unwrap_or(5)
                            } else {
                                inner_args[1].as_int().unwrap_or(5)
                            };
                            let number: i64 = if inner_args.len() > 2 {
                                inner_args[2].as_int().unwrap_or(1_000_000)
                            } else {
                                1_000_000
                            };

                            if is_callable(&setup3) {
                                let _ = call_callable(&setup3);
                            }

                            let mut results = Vec::new();
                            for _ in 0..repeat_count {
                                let start = Instant::now();
                                if is_callable(&stmt3) {
                                    for _ in 0..number {
                                        let _ = call_callable(&stmt3);
                                    }
                                } else {
                                    for _ in 0..number {
                                        std::hint::black_box(0);
                                    }
                                }
                                results.push(PyObject::float(start.elapsed().as_secs_f64()));
                            }
                            Ok(PyObject::list(results))
                        },
                    ),
                );
                // autorange() — find a good number to run
                let stmt4 = stmt.clone();
                attrs.insert(
                    CompactString::from("autorange"),
                    PyObject::native_closure(
                        "autorange",
                        move |_: &[PyObjectRef]| -> PyResult<PyObjectRef> {
                            use std::time::Instant;
                            let mut number: i64 = 1;
                            loop {
                                let start = Instant::now();
                                if is_callable(&stmt4) {
                                    for _ in 0..number {
                                        let _ = call_callable(&stmt4);
                                    }
                                } else {
                                    for _ in 0..number {
                                        std::hint::black_box(0);
                                    }
                                }
                                let elapsed = start.elapsed().as_secs_f64();
                                if elapsed >= 0.2 {
                                    return Ok(PyObject::tuple(vec![
                                        PyObject::int(number),
                                        PyObject::float(elapsed),
                                    ]));
                                }
                                number *= 10;
                                if number > 1_000_000_000 {
                                    break;
                                }
                            }
                            Ok(PyObject::tuple(vec![
                                PyObject::int(number),
                                PyObject::float(0.0),
                            ]))
                        },
                    ),
                );
                // print_exc() — stub
                attrs.insert(
                    CompactString::from("print_exc"),
                    make_builtin(|_| Ok(PyObject::none())),
                );
            }
            Ok(inst)
        },
    );

    make_module(
        "timeit",
        vec![
            ("default_timer", default_timer),
            ("timeit", timeit_fn),
            ("repeat", repeat_fn),
            ("Timer", timer_fn),
            ("default_number", PyObject::int(1_000_000)),
            ("default_repeat", PyObject::int(5)),
        ],
    )
}
