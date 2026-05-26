use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;
use std::sync::Arc;

use super::push_deferred_call;

// ── multiprocessing module ──────────────────────────────────────────

pub fn create_multiprocessing_module() -> PyObjectRef {
    // Process(target=, args=) — uses thread semantics since we can't fork
    let process_cls = PyObject::class(CompactString::from("Process"), vec![], IndexMap::new());
    let pc = process_cls.clone();
    let process_fn = PyObject::native_closure("Process", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(pc.clone());
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let mut target = PyObject::none();
            let mut proc_args = PyObject::tuple(vec![]);
            let mut name = PyObject::str_val(CompactString::from("Process"));
            let mut daemon = PyObject::bool_val(false);
            // Parse kwargs
            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(kw_map) = &last.payload {
                    let r = kw_map.read();
                    if let Some(t) = r.get(&HashableKey::str_key(CompactString::from("target"))) {
                        target = t.clone();
                    }
                    if let Some(a) = r.get(&HashableKey::str_key(CompactString::from("args"))) {
                        proc_args = a.clone();
                    }
                    if let Some(n) = r.get(&HashableKey::str_key(CompactString::from("name"))) {
                        name = n.clone();
                    }
                    if let Some(d) = r.get(&HashableKey::str_key(CompactString::from("daemon"))) {
                        daemon = d.clone();
                    }
                }
            }
            attrs.insert(CompactString::from("name"), name);
            attrs.insert(CompactString::from("daemon"), daemon);
            attrs.insert(
                CompactString::from("pid"),
                PyObject::int(std::process::id() as i64),
            );
            attrs.insert(CompactString::from("exitcode"), PyObject::none());

            let alive = Rc::new(PyCell::new(false));

            let tgt = target.clone();
            let targs = proc_args.clone();
            let a1 = alive.clone();
            attrs.insert(
                CompactString::from("start"),
                PyObject::native_closure("start", move |_: &[PyObjectRef]| {
                    *a1.write() = true;
                    if !matches!(&tgt.payload, PyObjectPayload::None) {
                        let call_args: Vec<PyObjectRef> = match &targs.payload {
                            PyObjectPayload::Tuple(items) => (**items).clone(),
                            PyObjectPayload::List(items) => items.read().clone(),
                            _ => vec![],
                        };
                        match &tgt.payload {
                            PyObjectPayload::NativeFunction(nf) => {
                                let _ = (nf.func)(&call_args);
                            }
                            PyObjectPayload::NativeClosure(nc) => {
                                let _ = (nc.func)(&call_args);
                            }
                            _ => {
                                push_deferred_call(tgt.clone(), call_args);
                            }
                        }
                    }
                    *a1.write() = false;
                    Ok(PyObject::none())
                }),
            );
            attrs.insert(CompactString::from("join"), {
                let a_join = alive.clone();
                PyObject::native_closure("join", move |args: &[PyObjectRef]| {
                    // Wait for process to complete; since start() runs synchronously,
                    // process is typically already done. Support optional timeout.
                    let timeout = args.first().and_then(|a| {
                        if matches!(&a.payload, PyObjectPayload::None) {
                            None
                        } else {
                            a.to_float().ok()
                        }
                    });
                    if *a_join.read() {
                        if let Some(t) = timeout {
                            let start = std::time::Instant::now();
                            let dur = std::time::Duration::from_secs_f64(t);
                            while *a_join.read() && start.elapsed() < dur {
                                std::thread::sleep(std::time::Duration::from_millis(5));
                            }
                        }
                    }
                    Ok(PyObject::none())
                })
            });
            let a2 = alive.clone();
            attrs.insert(
                CompactString::from("is_alive"),
                PyObject::native_closure("is_alive", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(*a2.read()))
                }),
            );
            let a3 = alive.clone();
            attrs.insert(
                CompactString::from("terminate"),
                PyObject::native_closure("terminate", move |_: &[PyObjectRef]| {
                    *a3.write() = false;
                    Ok(PyObject::none())
                }),
            );
            let a4 = alive.clone();
            attrs.insert(
                CompactString::from("kill"),
                PyObject::native_closure("kill", move |_: &[PyObjectRef]| {
                    *a4.write() = false;
                    Ok(PyObject::none())
                }),
            );
        }
        Ok(inst)
    });

    // Pool(processes=) — thread pool with state tracking
    let pool_cls = PyObject::class(CompactString::from("Pool"), vec![], IndexMap::new());
    let plc = pool_cls.clone();
    let pool_fn = PyObject::native_closure("Pool", move |args: &[PyObjectRef]| {
        let processes = if !args.is_empty() {
            args[0].as_int().unwrap_or(1)
        } else {
            1
        };
        let inst = PyObject::instance(plc.clone());
        let closed = Rc::new(PyCell::new(false));
        let terminated = Rc::new(PyCell::new(false));
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(CompactString::from("_processes"), PyObject::int(processes));
            let cl1 = closed.clone();
            let tm1 = terminated.clone();
            attrs.insert(
                CompactString::from("map"),
                PyObject::native_closure("map", move |args: &[PyObjectRef]| {
                    // Pool.map(func, iterable) — execute func(item) for each item sequentially
                    if *cl1.read() || *tm1.read() {
                        return Err(PyException::value_error("Pool not running"));
                    }
                    if args.len() < 2 {
                        return Err(PyException::type_error("map() requires func and iterable"));
                    }
                    let func = &args[0];
                    let iterable = args[1].to_list()?;
                    let mut results = Vec::with_capacity(iterable.len());
                    let mut has_deferred = false;
                    for item in &iterable {
                        match &func.payload {
                            PyObjectPayload::NativeFunction(nf) => {
                                results.push((nf.func)(&[item.clone()])?);
                            }
                            PyObjectPayload::NativeClosure(nc) => {
                                results.push((nc.func)(&[item.clone()])?);
                            }
                            _ => {
                                ferrython_core::error::request_vm_call(
                                    func.clone(),
                                    vec![item.clone()],
                                );
                                has_deferred = true;
                                results.push(PyObject::none());
                            }
                        }
                    }
                    if has_deferred {
                        ferrython_core::error::set_collect_vm_call_results(true);
                    }
                    Ok(PyObject::list(results))
                }),
            );
            let cl2 = closed.clone();
            let tm2 = terminated.clone();
            attrs.insert(
                CompactString::from("apply"),
                PyObject::native_closure("apply", move |args: &[PyObjectRef]| {
                    // Pool.apply(func, args=()) — call func with args
                    if *cl2.read() || *tm2.read() {
                        return Err(PyException::value_error("Pool not running"));
                    }
                    if args.is_empty() {
                        return Err(PyException::type_error("apply() requires func"));
                    }
                    let func = &args[0];
                    let call_args: Vec<PyObjectRef> = if args.len() > 1 {
                        args[1].to_list().unwrap_or_default()
                    } else {
                        vec![]
                    };
                    match &func.payload {
                        PyObjectPayload::NativeFunction(nf) => (nf.func)(&call_args),
                        PyObjectPayload::NativeClosure(nc) => (nc.func)(&call_args),
                        _ => {
                            push_deferred_call(func.clone(), call_args);
                            Ok(PyObject::none())
                        }
                    }
                }),
            );
            let cl3 = closed.clone();
            let tm3 = terminated.clone();
            attrs.insert(
                CompactString::from("apply_async"),
                PyObject::native_closure("apply_async", move |args: &[PyObjectRef]| {
                    // apply_async returns an AsyncResult; in our model, execute immediately
                    if *cl3.read() || *tm3.read() {
                        return Err(PyException::value_error("Pool not running"));
                    }
                    if args.is_empty() {
                        return Err(PyException::type_error("apply_async() requires func"));
                    }
                    let func = &args[0];
                    let call_args: Vec<PyObjectRef> = if args.len() > 1 {
                        args[1].to_list().unwrap_or_default()
                    } else {
                        vec![]
                    };
                    let result = match &func.payload {
                        PyObjectPayload::NativeFunction(nf) => (nf.func)(&call_args)?,
                        PyObjectPayload::NativeClosure(nc) => (nc.func)(&call_args)?,
                        _ => {
                            push_deferred_call(func.clone(), call_args);
                            PyObject::none()
                        }
                    };
                    // Return an AsyncResult-like object with get() method
                    let cls = PyObject::class(
                        CompactString::from("AsyncResult"),
                        vec![],
                        IndexMap::new(),
                    );
                    let async_inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref d) = async_inst.payload {
                        let r = result.clone();
                        d.attrs.write().insert(
                            CompactString::from("get"),
                            PyObject::native_closure("get", move |_: &[PyObjectRef]| Ok(r.clone())),
                        );
                        d.attrs.write().insert(
                            CompactString::from("ready"),
                            make_builtin(|_| Ok(PyObject::bool_val(true))),
                        );
                        d.attrs.write().insert(
                            CompactString::from("successful"),
                            make_builtin(|_| Ok(PyObject::bool_val(true))),
                        );
                        d.attrs.write().insert(
                            CompactString::from("wait"),
                            make_builtin(|_| Ok(PyObject::none())),
                        );
                    }
                    Ok(async_inst)
                }),
            );
            let cl4 = closed.clone();
            attrs.insert(
                CompactString::from("close"),
                PyObject::native_closure("close", move |_: &[PyObjectRef]| {
                    *cl4.write() = true;
                    Ok(PyObject::none())
                }),
            );
            let cl5 = closed.clone();
            attrs.insert(
                CompactString::from("join"),
                PyObject::native_closure("join", move |_: &[PyObjectRef]| {
                    if !*cl5.read() {
                        return Err(PyException::value_error("Pool is still running"));
                    }
                    // All work is synchronous, so join is immediate
                    Ok(PyObject::none())
                }),
            );
            let tm4 = terminated.clone();
            let cl6 = closed.clone();
            attrs.insert(
                CompactString::from("terminate"),
                PyObject::native_closure("terminate", move |_: &[PyObjectRef]| {
                    *tm4.write() = true;
                    *cl6.write() = true;
                    Ok(PyObject::none())
                }),
            );
            attrs.insert(CompactString::from("__enter__"), {
                let ir = inst.clone();
                PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| Ok(ir.clone()))
            });
            attrs.insert(
                CompactString::from("__exit__"),
                make_builtin(|_| Ok(PyObject::bool_val(false))),
            );
        }
        Ok(inst)
    });

    // cpu_count()
    let cpu_count_fn = make_builtin(|_| {
        let count = std::thread::available_parallelism()
            .map(|n| n.get() as i64)
            .unwrap_or(1);
        Ok(PyObject::int(count))
    });

    // current_process()
    let current_process_fn = make_builtin(|_| {
        let cls = PyObject::class(CompactString::from("Process"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref i) = inst.payload {
            let mut attrs = i.attrs.write();
            attrs.insert(
                CompactString::from("name"),
                PyObject::str_val(CompactString::from("MainProcess")),
            );
            attrs.insert(
                CompactString::from("pid"),
                PyObject::int(std::process::id() as i64),
            );
            attrs.insert(CompactString::from("daemon"), PyObject::bool_val(false));
            attrs.insert(CompactString::from("exitcode"), PyObject::none());
            attrs.insert(
                CompactString::from("is_alive"),
                make_builtin(|_| Ok(PyObject::bool_val(true))),
            );
        }
        Ok(inst)
    });

    make_module(
        "multiprocessing",
        vec![
            ("Process", process_fn),
            ("Pool", pool_fn),
            ("cpu_count", cpu_count_fn),
            ("current_process", current_process_fn),
            (
                "Queue",
                make_builtin(|_| {
                    let cls =
                        PyObject::class(CompactString::from("Queue"), vec![], IndexMap::new());
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref d) = inst.payload {
                        let items: Arc<std::sync::Mutex<std::collections::VecDeque<PyObjectRef>>> =
                            Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
                        let mut attrs = d.attrs.write();
                        let q1 = items.clone();
                        attrs.insert(
                            CompactString::from("put"),
                            PyObject::native_closure("put", move |args: &[PyObjectRef]| {
                                if args.is_empty() {
                                    return Err(PyException::type_error(
                                        "put() requires 1 argument",
                                    ));
                                }
                                q1.lock().unwrap().push_back(args[0].clone());
                                Ok(PyObject::none())
                            }),
                        );
                        let q2 = items.clone();
                        attrs.insert(
                            CompactString::from("get"),
                            PyObject::native_closure("get", move |_: &[PyObjectRef]| {
                                q2.lock().unwrap().pop_front().ok_or_else(|| {
                                    PyException::new(
                                        ferrython_core::error::ExceptionKind::RuntimeError,
                                        "Queue is empty",
                                    )
                                })
                            }),
                        );
                        let q3 = items.clone();
                        attrs.insert(
                            CompactString::from("empty"),
                            PyObject::native_closure("empty", move |_: &[PyObjectRef]| {
                                Ok(PyObject::bool_val(q3.lock().unwrap().is_empty()))
                            }),
                        );
                        let q4 = items.clone();
                        attrs.insert(
                            CompactString::from("qsize"),
                            PyObject::native_closure("qsize", move |_: &[PyObjectRef]| {
                                Ok(PyObject::int(q4.lock().unwrap().len() as i64))
                            }),
                        );
                        let q5 = items.clone();
                        attrs.insert(
                            CompactString::from("full"),
                            PyObject::native_closure("full", move |_: &[PyObjectRef]| {
                                let _ = q5; // unbounded → never full
                                Ok(PyObject::bool_val(false))
                            }),
                        );
                        attrs.insert(CompactString::from("put_nowait"), {
                            let q = items.clone();
                            PyObject::native_closure("put_nowait", move |args: &[PyObjectRef]| {
                                if args.is_empty() {
                                    return Err(PyException::type_error(
                                        "put_nowait() requires 1 argument",
                                    ));
                                }
                                q.lock().unwrap().push_back(args[0].clone());
                                Ok(PyObject::none())
                            })
                        });
                        attrs.insert(CompactString::from("get_nowait"), {
                            let q = items.clone();
                            PyObject::native_closure("get_nowait", move |_: &[PyObjectRef]| {
                                q.lock().unwrap().pop_front().ok_or_else(|| {
                                    PyException::new(
                                        ferrython_core::error::ExceptionKind::RuntimeError,
                                        "Queue is empty",
                                    )
                                })
                            })
                        });
                        attrs.insert(CompactString::from("close"), {
                            let q = items.clone();
                            PyObject::native_closure("close", move |_: &[PyObjectRef]| {
                                q.lock().unwrap().clear();
                                Ok(PyObject::none())
                            })
                        });
                    }
                    Ok(inst)
                }),
            ),
            (
                "Lock",
                make_builtin(|_| {
                    let cls = PyObject::class(CompactString::from("Lock"), vec![], IndexMap::new());
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref d) = inst.payload {
                        let locked = Arc::new(std::sync::Mutex::new(false));
                        let mut attrs = d.attrs.write();
                        let l1 = locked.clone();
                        attrs.insert(
                            CompactString::from("acquire"),
                            PyObject::native_closure("acquire", move |_: &[PyObjectRef]| {
                                *l1.lock().unwrap() = true;
                                Ok(PyObject::bool_val(true))
                            }),
                        );
                        let l2 = locked.clone();
                        attrs.insert(
                            CompactString::from("release"),
                            PyObject::native_closure("release", move |_: &[PyObjectRef]| {
                                *l2.lock().unwrap() = false;
                                Ok(PyObject::none())
                            }),
                        );
                        let l3 = locked.clone();
                        attrs.insert(
                            CompactString::from("locked"),
                            PyObject::native_closure("locked", move |_: &[PyObjectRef]| {
                                Ok(PyObject::bool_val(*l3.lock().unwrap()))
                            }),
                        );
                        let li = inst.clone();
                        attrs.insert(
                            CompactString::from("__enter__"),
                            PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| {
                                Ok(li.clone())
                            }),
                        );
                        attrs.insert(CompactString::from("__exit__"), {
                            let l = locked.clone();
                            PyObject::native_closure("__exit__", move |_: &[PyObjectRef]| {
                                *l.lock().unwrap() = false;
                                Ok(PyObject::bool_val(false))
                            })
                        });
                    }
                    Ok(inst)
                }),
            ),
            (
                "Value",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("Value() requires 2 arguments"));
                    }
                    Ok(args[1].clone())
                }),
            ),
            (
                "Array",
                make_builtin(|args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("Array() requires 2 arguments"));
                    }
                    Ok(args[1].clone())
                }),
            ),
            (
                "Manager",
                make_builtin(|_| {
                    let cls = PyObject::class(
                        CompactString::from("SyncManager"),
                        vec![],
                        IndexMap::new(),
                    );
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref d) = inst.payload {
                        let mut attrs = d.attrs.write();
                        // Manager.dict() -> shared dict
                        attrs.insert(
                            CompactString::from("dict"),
                            make_builtin(|_| Ok(PyObject::dict(IndexMap::new()))),
                        );
                        // Manager.list() -> shared list
                        attrs.insert(
                            CompactString::from("list"),
                            make_builtin(|_| Ok(PyObject::list(vec![]))),
                        );
                        // Manager.Value(typecode, value) -> value wrapper
                        attrs.insert(
                            CompactString::from("Value"),
                            make_builtin(|args: &[PyObjectRef]| {
                                if args.len() < 2 {
                                    return Err(PyException::type_error(
                                        "Value() requires 2 arguments",
                                    ));
                                }
                                Ok(args[1].clone())
                            }),
                        );
                        // Manager.Lock() -> Lock
                        attrs.insert(
                            CompactString::from("Lock"),
                            make_builtin(|_| {
                                let lock_cls = PyObject::class(
                                    CompactString::from("Lock"),
                                    vec![],
                                    IndexMap::new(),
                                );
                                let lock_inst = PyObject::instance(lock_cls);
                                if let PyObjectPayload::Instance(ref ld) = lock_inst.payload {
                                    let locked = Arc::new(std::sync::Mutex::new(false));
                                    let mut la = ld.attrs.write();
                                    let l1 = locked.clone();
                                    la.insert(
                                        CompactString::from("acquire"),
                                        PyObject::native_closure(
                                            "acquire",
                                            move |_: &[PyObjectRef]| {
                                                *l1.lock().unwrap() = true;
                                                Ok(PyObject::bool_val(true))
                                            },
                                        ),
                                    );
                                    let l2 = locked.clone();
                                    la.insert(
                                        CompactString::from("release"),
                                        PyObject::native_closure(
                                            "release",
                                            move |_: &[PyObjectRef]| {
                                                *l2.lock().unwrap() = false;
                                                Ok(PyObject::none())
                                            },
                                        ),
                                    );
                                }
                                Ok(lock_inst)
                            }),
                        );
                        // Manager.Namespace() -> namespace object
                        attrs.insert(
                            CompactString::from("Namespace"),
                            make_builtin(|_| {
                                let ns_cls = PyObject::class(
                                    CompactString::from("Namespace"),
                                    vec![],
                                    IndexMap::new(),
                                );
                                Ok(PyObject::instance(ns_cls))
                            }),
                        );
                        // Manager.Event() -> Event
                        attrs.insert(
                            CompactString::from("Event"),
                            make_builtin(|_| {
                                let ev_cls = PyObject::class(
                                    CompactString::from("Event"),
                                    vec![],
                                    IndexMap::new(),
                                );
                                let ev_inst = PyObject::instance(ev_cls);
                                if let PyObjectPayload::Instance(ref ed) = ev_inst.payload {
                                    let flag = Arc::new(std::sync::Mutex::new(false));
                                    let mut ea = ed.attrs.write();
                                    let f1 = flag.clone();
                                    ea.insert(
                                        CompactString::from("set"),
                                        PyObject::native_closure(
                                            "set",
                                            move |_: &[PyObjectRef]| {
                                                *f1.lock().unwrap() = true;
                                                Ok(PyObject::none())
                                            },
                                        ),
                                    );
                                    let f2 = flag.clone();
                                    ea.insert(
                                        CompactString::from("clear"),
                                        PyObject::native_closure(
                                            "clear",
                                            move |_: &[PyObjectRef]| {
                                                *f2.lock().unwrap() = false;
                                                Ok(PyObject::none())
                                            },
                                        ),
                                    );
                                    let f3 = flag.clone();
                                    ea.insert(
                                        CompactString::from("is_set"),
                                        PyObject::native_closure(
                                            "is_set",
                                            move |_: &[PyObjectRef]| {
                                                Ok(PyObject::bool_val(*f3.lock().unwrap()))
                                            },
                                        ),
                                    );
                                    let f4 = flag.clone();
                                    ea.insert(
                                        CompactString::from("wait"),
                                        PyObject::native_closure(
                                            "wait",
                                            move |_: &[PyObjectRef]| {
                                                Ok(PyObject::bool_val(*f4.lock().unwrap()))
                                            },
                                        ),
                                    );
                                }
                                Ok(ev_inst)
                            }),
                        );
                        // Context manager support
                        let ir = inst.clone();
                        attrs.insert(
                            CompactString::from("__enter__"),
                            PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| {
                                Ok(ir.clone())
                            }),
                        );
                        attrs.insert(
                            CompactString::from("__exit__"),
                            make_builtin(|_| Ok(PyObject::bool_val(false))),
                        );
                        attrs.insert(
                            CompactString::from("shutdown"),
                            make_builtin(|_| Ok(PyObject::none())),
                        );
                    }
                    Ok(inst)
                }),
            ),
            (
                "Pipe",
                make_builtin(|_| Ok(PyObject::tuple(vec![PyObject::none(), PyObject::none()]))),
            ),
            (
                "Event",
                make_builtin(|_| {
                    let cls =
                        PyObject::class(CompactString::from("Event"), vec![], IndexMap::new());
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref d) = inst.payload {
                        let flag = Arc::new(std::sync::Mutex::new(false));
                        let mut attrs = d.attrs.write();
                        let f1 = flag.clone();
                        attrs.insert(
                            CompactString::from("set"),
                            PyObject::native_closure("set", move |_: &[PyObjectRef]| {
                                *f1.lock().unwrap() = true;
                                Ok(PyObject::none())
                            }),
                        );
                        let f2 = flag.clone();
                        attrs.insert(
                            CompactString::from("clear"),
                            PyObject::native_closure("clear", move |_: &[PyObjectRef]| {
                                *f2.lock().unwrap() = false;
                                Ok(PyObject::none())
                            }),
                        );
                        let f3 = flag.clone();
                        attrs.insert(
                            CompactString::from("is_set"),
                            PyObject::native_closure("is_set", move |_: &[PyObjectRef]| {
                                Ok(PyObject::bool_val(*f3.lock().unwrap()))
                            }),
                        );
                        let f4 = flag.clone();
                        attrs.insert(
                            CompactString::from("wait"),
                            PyObject::native_closure("wait", move |args: &[PyObjectRef]| {
                                let timeout_secs = args.first().and_then(|a| {
                                    if matches!(&a.payload, PyObjectPayload::None) {
                                        None
                                    } else {
                                        a.to_float().ok()
                                    }
                                });
                                if *f4.lock().unwrap() {
                                    return Ok(PyObject::bool_val(true));
                                }
                                if let Some(t) = timeout_secs {
                                    std::thread::sleep(std::time::Duration::from_secs_f64(t));
                                }
                                Ok(PyObject::bool_val(*f4.lock().unwrap()))
                            }),
                        );
                    }
                    Ok(inst)
                }),
            ),
            (
                "Semaphore",
                make_builtin(|args: &[PyObjectRef]| {
                    let value = args.first().and_then(|a| a.as_int()).unwrap_or(1);
                    let cls =
                        PyObject::class(CompactString::from("Semaphore"), vec![], IndexMap::new());
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref d) = inst.payload {
                        let count = Arc::new(std::sync::Mutex::new(value));
                        let mut attrs = d.attrs.write();
                        let c1 = count.clone();
                        attrs.insert(
                            CompactString::from("acquire"),
                            PyObject::native_closure("acquire", move |_: &[PyObjectRef]| {
                                let mut c = c1.lock().unwrap();
                                if *c > 0 {
                                    *c -= 1;
                                    Ok(PyObject::bool_val(true))
                                } else {
                                    Ok(PyObject::bool_val(false))
                                }
                            }),
                        );
                        let c2 = count.clone();
                        attrs.insert(
                            CompactString::from("release"),
                            PyObject::native_closure("release", move |_: &[PyObjectRef]| {
                                *c2.lock().unwrap() += 1;
                                Ok(PyObject::none())
                            }),
                        );
                        let si = inst.clone();
                        attrs.insert(
                            CompactString::from("__enter__"),
                            PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| {
                                Ok(si.clone())
                            }),
                        );
                        let c3 = count.clone();
                        attrs.insert(
                            CompactString::from("__exit__"),
                            PyObject::native_closure("__exit__", move |_: &[PyObjectRef]| {
                                *c3.lock().unwrap() += 1;
                                Ok(PyObject::bool_val(false))
                            }),
                        );
                    }
                    Ok(inst)
                }),
            ),
        ],
    )
}
