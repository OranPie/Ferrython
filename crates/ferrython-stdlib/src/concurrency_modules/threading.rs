use super::push_deferred_call;
use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, make_module, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;
use std::sync::Arc;

pub fn create_threading_module() -> PyObjectRef {
    // Build Thread as a proper Class so subclasses inherit methods via MRO.
    let mut thread_ns = IndexMap::new();

    // __init__(self, *, target=None, args=(), daemon=False, name="Thread")
    thread_ns.insert(
        CompactString::from("__init__"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            let self_obj = &args[0];
            if let PyObjectPayload::Instance(ref inst) = self_obj.payload {
                let mut attrs = inst.attrs.write();
                let mut target = PyObject::none();
                let mut thread_args = PyObject::tuple(vec![]);
                let mut daemon = PyObject::bool_val(false);
                let mut name = PyObject::str_val(CompactString::from("Thread"));
                // kwargs dict is last arg
                if let Some(last) = args.last() {
                    if let PyObjectPayload::Dict(kw_map) = &last.payload {
                        let r = kw_map.read();
                        if let Some(t) = r.get(&HashableKey::str_key(CompactString::from("target")))
                        {
                            target = t.clone();
                        }
                        if let Some(a) = r.get(&HashableKey::str_key(CompactString::from("args"))) {
                            thread_args = a.clone();
                        }
                        if let Some(d) = r.get(&HashableKey::str_key(CompactString::from("daemon")))
                        {
                            daemon = d.clone();
                        }
                        if let Some(n) = r.get(&HashableKey::str_key(CompactString::from("name"))) {
                            name = n.clone();
                        }
                    }
                }
                attrs.insert(CompactString::from("_target"), target);
                attrs.insert(CompactString::from("_args"), thread_args);
                attrs.insert(CompactString::from("name"), name);
                attrs.insert(CompactString::from("daemon"), daemon);
                attrs.insert(CompactString::from("_alive"), PyObject::bool_val(false));
                attrs.insert(CompactString::from("_started"), PyObject::bool_val(false));
                attrs.insert(CompactString::from("ident"), PyObject::none());
            }
            Ok(PyObject::none())
        }),
    );

    // start(self)
    thread_ns.insert(
        CompactString::from("start"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            let self_obj = &args[0];
            if let PyObjectPayload::Instance(ref inst) = self_obj.payload {
                inst.attrs
                    .write()
                    .insert(CompactString::from("_started"), PyObject::bool_val(true));
                inst.attrs
                    .write()
                    .insert(CompactString::from("_alive"), PyObject::bool_val(true));
                let target = inst
                    .attrs
                    .read()
                    .get("_target")
                    .cloned()
                    .unwrap_or_else(PyObject::none);
                let thread_args = inst
                    .attrs
                    .read()
                    .get("_args")
                    .cloned()
                    .unwrap_or_else(|| PyObject::tuple(vec![]));
                if !matches!(&target.payload, PyObjectPayload::None) {
                    let call_args: Vec<PyObjectRef> = match &thread_args.payload {
                        PyObjectPayload::Tuple(items) => (**items).clone(),
                        PyObjectPayload::List(items) => items.read().clone(),
                        _ => vec![],
                    };
                    // For native functions, spawn a real OS thread for true parallelism
                    match &target.payload {
                        PyObjectPayload::NativeFunction(nf) => {
                            let _ = nf.func;
                            let alive_attrs = inst.attrs.clone();
                            let call_args_owned = call_args;
                            let join_handle = std::sync::Arc::new(std::sync::Mutex::new(
                                None::<std::thread::JoinHandle<()>>,
                            ));
                            let jh = join_handle.clone();
                            // SAFETY: GIL semantics — the spawned thread won't race
                            // with the main interpreter thread.
                            let closure: Box<dyn FnOnce()> = Box::new(move || {
                                let _ = (nf.func)(&call_args_owned);
                                alive_attrs.write().insert(
                                    CompactString::from("_alive"),
                                    PyObject::bool_val(false),
                                );
                            });
                            let send_closure: Box<dyn FnOnce() + Send> =
                                unsafe { std::mem::transmute(closure) };
                            let handle = std::thread::spawn(move || {
                                send_closure();
                            });
                            *jh.lock().unwrap() = Some(handle);
                            inst.attrs.write().insert(
                                CompactString::from("_join_handle"),
                                PyObject::native_closure("_join_handle", move |_| {
                                    if let Some(h) = join_handle.lock().unwrap().take() {
                                        let _ = h.join();
                                    }
                                    Ok(PyObject::none())
                                }),
                            );
                            return Ok(PyObject::none());
                        }
                        PyObjectPayload::NativeClosure(nc) => {
                            let nc = nc.clone();
                            let alive_attrs = inst.attrs.clone();
                            let join_handle = std::sync::Arc::new(std::sync::Mutex::new(
                                None::<std::thread::JoinHandle<()>>,
                            ));
                            let jh = join_handle.clone();
                            let closure: Box<dyn FnOnce()> = Box::new(move || {
                                let _ = (nc.func)(&call_args);
                                alive_attrs.write().insert(
                                    CompactString::from("_alive"),
                                    PyObject::bool_val(false),
                                );
                            });
                            let send_closure: Box<dyn FnOnce() + Send> =
                                unsafe { std::mem::transmute(closure) };
                            let handle = std::thread::spawn(move || {
                                send_closure();
                            });
                            *jh.lock().unwrap() = Some(handle);
                            inst.attrs.write().insert(
                                CompactString::from("_join_handle"),
                                PyObject::native_closure("_join_handle", move |_| {
                                    if let Some(h) = join_handle.lock().unwrap().take() {
                                        let _ = h.join();
                                    }
                                    Ok(PyObject::none())
                                }),
                            );
                            return Ok(PyObject::none());
                        }
                        _ => {
                            // Python-defined functions: spawn a real OS thread with its own VM
                            let alive_attrs = inst.attrs.clone();
                            if let Some(handle) = ferrython_core::error::spawn_python_thread(
                                target.clone(),
                                call_args.clone(),
                            ) {
                                let join_handle =
                                    std::sync::Arc::new(std::sync::Mutex::new(Some(handle)));
                                let jh = join_handle.clone();
                                let alive_flag = alive_attrs.clone();
                                // Monitor thread completion in a background helper
                                let closure: Box<dyn FnOnce()> = Box::new(move || {
                                    if let Some(h) = jh.lock().unwrap().take() {
                                        let _ = h.join();
                                    }
                                    alive_flag.write().insert(
                                        CompactString::from("_alive"),
                                        PyObject::bool_val(false),
                                    );
                                });
                                let send_closure: Box<dyn FnOnce() + Send> =
                                    unsafe { std::mem::transmute(closure) };
                                std::thread::spawn(move || {
                                    send_closure();
                                });
                                inst.attrs.write().insert(
                                    CompactString::from("_join_handle"),
                                    PyObject::native_closure("_join_handle", move |_| {
                                        if let Some(h) = join_handle.lock().unwrap().take() {
                                            let _ = h.join();
                                        }
                                        Ok(PyObject::none())
                                    }),
                                );
                                return Ok(PyObject::none());
                            }
                            // Fallback: deferred sequential execution
                            let is_daemon = inst
                                .attrs
                                .read()
                                .get("daemon")
                                .cloned()
                                .or_else(|| inst.attrs.read().get("_daemon").cloned())
                                .map(|v| v.is_truthy())
                                .unwrap_or(false);
                            if !is_daemon {
                                push_deferred_call(target, call_args);
                            }
                        }
                    }
                }
                inst.attrs
                    .write()
                    .insert(CompactString::from("_alive"), PyObject::bool_val(false));
            }
            Ok(PyObject::none())
        }),
    );

    // join(self, timeout=None) — wait for thread to complete
    thread_ns.insert(
        CompactString::from("join"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                let attrs = inst.attrs.read();
                let started = attrs
                    .get("_started")
                    .map(|v| v.is_truthy())
                    .unwrap_or(false);
                if !started {
                    return Err(PyException::runtime_error(
                        "cannot join thread before it is started",
                    ));
                }
                // If there's a real OS thread join handle, use it
                if let Some(jh) = attrs.get("_join_handle").cloned() {
                    drop(attrs);
                    // Get timeout if provided
                    let timeout_secs =
                        if args.len() > 1 && !matches!(&args[1].payload, PyObjectPayload::None) {
                            args[1].to_float().ok()
                        } else {
                            None
                        };
                    if let Some(t) = timeout_secs {
                        // Poll-based join with timeout
                        let start = std::time::Instant::now();
                        let dur = std::time::Duration::from_secs_f64(t);
                        loop {
                            if let PyObjectPayload::Instance(ref inst2) = args[0].payload {
                                let alive = inst2
                                    .attrs
                                    .read()
                                    .get("_alive")
                                    .map(|v| v.is_truthy())
                                    .unwrap_or(false);
                                if !alive {
                                    break;
                                }
                            }
                            if start.elapsed() >= dur {
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                    } else {
                        // Blocking join
                        if let PyObjectPayload::NativeClosure(nc) = &jh.payload {
                            let _ = (nc.func)(&[]);
                        }
                    }
                }
            }
            Ok(PyObject::none())
        }),
    );

    // is_alive(self)
    thread_ns.insert(
        CompactString::from("is_alive"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::bool_val(false));
            }
            if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                if let Some(alive) = inst.attrs.read().get("_alive").cloned() {
                    return Ok(alive);
                }
            }
            Ok(PyObject::bool_val(false))
        }),
    );

    // getName(self)
    thread_ns.insert(
        CompactString::from("getName"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::str_val(CompactString::from("Thread")));
            }
            if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                if let Some(name) = inst.attrs.read().get("name").cloned() {
                    return Ok(name);
                }
            }
            Ok(PyObject::str_val(CompactString::from("Thread")))
        }),
    );

    // setDaemon(self, val)
    thread_ns.insert(
        CompactString::from("setDaemon"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.len() >= 2 {
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    inst.attrs
                        .write()
                        .insert(CompactString::from("daemon"), args[1].clone());
                }
            }
            Ok(PyObject::none())
        }),
    );

    // run(self) — default implementation calls target
    thread_ns.insert(
        CompactString::from("run"),
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                let target = inst
                    .attrs
                    .read()
                    .get("_target")
                    .cloned()
                    .unwrap_or_else(PyObject::none);
                if !matches!(&target.payload, PyObjectPayload::None) {
                    let thread_args = inst
                        .attrs
                        .read()
                        .get("_args")
                        .cloned()
                        .unwrap_or_else(|| PyObject::tuple(vec![]));
                    let call_args: Vec<PyObjectRef> = match &thread_args.payload {
                        PyObjectPayload::Tuple(items) => (**items).clone(),
                        PyObjectPayload::List(items) => items.read().clone(),
                        _ => vec![],
                    };
                    push_deferred_call(target, call_args);
                }
            }
            Ok(PyObject::none())
        }),
    );

    let thread_class = PyObject::class(CompactString::from("Thread"), vec![], thread_ns);

    // Lock — real mutex for cross-thread synchronization
    let lock_cls = PyObject::class(CompactString::from("Lock"), vec![], IndexMap::new());
    let lc = lock_cls.clone();
    let lock_fn = PyObject::native_closure("Lock", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(lc.clone());
        let mutex = Arc::new(parking_lot::Mutex::new(()));
        let locked_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let inst_ref = inst.clone();
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let m1 = mutex.clone();
            let lf1 = locked_flag.clone();
            attrs.insert(
                CompactString::from("acquire"),
                PyObject::native_closure("acquire", move |args: &[PyObjectRef]| {
                    let mut blocking = true;
                    for a in args {
                        match &a.payload {
                            PyObjectPayload::Bool(b) => {
                                blocking = *b;
                            }
                            PyObjectPayload::Dict(map) => {
                                let r = map.read();
                                if let Some(v) =
                                    r.get(&HashableKey::str_key(CompactString::from("blocking")))
                                {
                                    blocking = v.is_truthy();
                                }
                            }
                            _ => {}
                        }
                    }
                    if blocking {
                        let guard = m1.lock();
                        lf1.store(true, std::sync::atomic::Ordering::Release);
                        // Leak the guard to keep the mutex locked until release()
                        std::mem::forget(guard);
                        Ok(PyObject::bool_val(true))
                    } else {
                        match m1.try_lock() {
                            Some(guard) => {
                                lf1.store(true, std::sync::atomic::Ordering::Release);
                                std::mem::forget(guard);
                                Ok(PyObject::bool_val(true))
                            }
                            None => Ok(PyObject::bool_val(false)),
                        }
                    }
                }),
            );
            let m2 = mutex.clone();
            let lf2 = locked_flag.clone();
            attrs.insert(
                CompactString::from("release"),
                PyObject::native_closure("release", move |_: &[PyObjectRef]| {
                    if lf2.swap(false, std::sync::atomic::Ordering::AcqRel) {
                        // Safety: we know the mutex was locked by acquire()
                        unsafe {
                            m2.force_unlock();
                        }
                    }
                    Ok(PyObject::none())
                }),
            );
            let lf3 = locked_flag.clone();
            attrs.insert(
                CompactString::from("locked"),
                PyObject::native_closure("locked", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(
                        lf3.load(std::sync::atomic::Ordering::Acquire),
                    ))
                }),
            );
            let m4 = mutex.clone();
            let lf4 = locked_flag.clone();
            attrs.insert(
                CompactString::from("__enter__"),
                PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| {
                    let guard = m4.lock();
                    lf4.store(true, std::sync::atomic::Ordering::Release);
                    std::mem::forget(guard);
                    Ok(inst_ref.clone())
                }),
            );
            let m5 = mutex.clone();
            let lf5 = locked_flag.clone();
            attrs.insert(
                CompactString::from("__exit__"),
                PyObject::native_closure("__exit__", move |_: &[PyObjectRef]| {
                    if lf5.swap(false, std::sync::atomic::Ordering::AcqRel) {
                        unsafe {
                            m5.force_unlock();
                        }
                    }
                    Ok(PyObject::bool_val(false))
                }),
            );
        }
        Ok(inst)
    });

    // RLock — reentrant lock with count tracking
    let rlock_cls = PyObject::class(CompactString::from("RLock"), vec![], IndexMap::new());
    let rlc = rlock_cls.clone();
    let rlock_fn = PyObject::native_closure("RLock", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(rlc.clone());
        // (locked, reentrant_count)
        let state = Rc::new(PyCell::new((false, 0u32)));
        let inst_ref = inst.clone();
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let s1 = state.clone();
            attrs.insert(
                CompactString::from("acquire"),
                PyObject::native_closure("acquire", move |args: &[PyObjectRef]| {
                    let mut blocking = true;
                    for a in args {
                        match &a.payload {
                            PyObjectPayload::Bool(b) => {
                                blocking = *b;
                            }
                            PyObjectPayload::Dict(map) => {
                                let r = map.read();
                                if let Some(v) =
                                    r.get(&HashableKey::str_key(CompactString::from("blocking")))
                                {
                                    blocking = v.is_truthy();
                                }
                            }
                            _ => {}
                        }
                    }
                    let mut s = s1.write();
                    if s.0 && blocking {
                        // Reentrant — always succeeds
                        s.1 += 1;
                        Ok(PyObject::bool_val(true))
                    } else if s.0 && !blocking {
                        // Non-blocking on already-locked: RLock is reentrant, so succeed
                        s.1 += 1;
                        Ok(PyObject::bool_val(true))
                    } else {
                        s.0 = true;
                        s.1 += 1;
                        Ok(PyObject::bool_val(true))
                    }
                }),
            );
            let s2 = state.clone();
            attrs.insert(
                CompactString::from("release"),
                PyObject::native_closure("release", move |_: &[PyObjectRef]| {
                    let mut s = s2.write();
                    if s.1 > 0 {
                        s.1 -= 1;
                    }
                    if s.1 == 0 {
                        s.0 = false;
                    }
                    Ok(PyObject::none())
                }),
            );
            let s3 = state.clone();
            attrs.insert(
                CompactString::from("locked"),
                PyObject::native_closure("locked", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(s3.read().0))
                }),
            );
            let s4 = state.clone();
            let ir = inst_ref.clone();
            attrs.insert(
                CompactString::from("__enter__"),
                PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| {
                    let mut s = s4.write();
                    s.0 = true;
                    s.1 += 1;
                    Ok(ir.clone())
                }),
            );
            let s5 = state.clone();
            attrs.insert(
                CompactString::from("__exit__"),
                PyObject::native_closure("__exit__", move |_: &[PyObjectRef]| {
                    let mut s = s5.write();
                    if s.1 > 0 {
                        s.1 -= 1;
                    }
                    if s.1 == 0 {
                        s.0 = false;
                    }
                    Ok(PyObject::bool_val(false))
                }),
            );
        }
        Ok(inst)
    });

    // Semaphore — counting semaphore
    let sem_cls = PyObject::class(CompactString::from("Semaphore"), vec![], IndexMap::new());
    let sc = sem_cls.clone();
    let semaphore_fn = PyObject::native_closure("Semaphore", move |args: &[PyObjectRef]| {
        let initial = if !args.is_empty() {
            args[0].as_int().unwrap_or(1)
        } else {
            1
        };
        let inst = PyObject::instance(sc.clone());
        let counter = Rc::new(PyCell::new(initial));
        let inst_ref = inst.clone();
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let c1 = counter.clone();
            attrs.insert(
                CompactString::from("acquire"),
                PyObject::native_closure("acquire", move |_: &[PyObjectRef]| {
                    let mut c = c1.write();
                    if *c > 0 {
                        *c -= 1;
                        Ok(PyObject::bool_val(true))
                    } else {
                        Ok(PyObject::bool_val(false))
                    }
                }),
            );
            let c2 = counter.clone();
            attrs.insert(
                CompactString::from("release"),
                PyObject::native_closure("release", move |_: &[PyObjectRef]| {
                    *c2.write() += 1;
                    Ok(PyObject::none())
                }),
            );
            let c3 = counter.clone();
            attrs.insert(
                CompactString::from("_value"),
                PyObject::native_closure("_value", move |_: &[PyObjectRef]| {
                    Ok(PyObject::int(*c3.read()))
                }),
            );
            let c4 = counter.clone();
            let ir = inst_ref.clone();
            attrs.insert(
                CompactString::from("__enter__"),
                PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| {
                    let mut c = c4.write();
                    if *c > 0 {
                        *c -= 1;
                    }
                    Ok(ir.clone())
                }),
            );
            let c5 = counter.clone();
            attrs.insert(
                CompactString::from("__exit__"),
                PyObject::native_closure("__exit__", move |_: &[PyObjectRef]| {
                    *c5.write() += 1;
                    Ok(PyObject::bool_val(false))
                }),
            );
        }
        Ok(inst)
    });

    // BoundedSemaphore — same as Semaphore with upper bound check
    let bsem_cls = PyObject::class(
        CompactString::from("BoundedSemaphore"),
        vec![],
        IndexMap::new(),
    );
    let bsc = bsem_cls.clone();
    let bounded_semaphore_fn =
        PyObject::native_closure("BoundedSemaphore", move |args: &[PyObjectRef]| {
            let initial = if !args.is_empty() {
                args[0].as_int().unwrap_or(1)
            } else {
                1
            };
            let inst = PyObject::instance(bsc.clone());
            let counter = Rc::new(PyCell::new(initial));
            let bound = initial;
            let inst_ref = inst.clone();
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                let mut attrs = inst_data.attrs.write();
                let c1 = counter.clone();
                attrs.insert(
                    CompactString::from("acquire"),
                    PyObject::native_closure("acquire", move |_: &[PyObjectRef]| {
                        let mut c = c1.write();
                        if *c > 0 {
                            *c -= 1;
                            Ok(PyObject::bool_val(true))
                        } else {
                            Ok(PyObject::bool_val(false))
                        }
                    }),
                );
                let c2 = counter.clone();
                attrs.insert(
                    CompactString::from("release"),
                    PyObject::native_closure("release", move |_: &[PyObjectRef]| {
                        let mut c = c2.write();
                        if *c >= bound {
                            return Err(PyException::value_error(
                                "Semaphore released too many times",
                            ));
                        }
                        *c += 1;
                        Ok(PyObject::none())
                    }),
                );
                let c3 = counter.clone();
                attrs.insert(
                    CompactString::from("_value"),
                    PyObject::native_closure("_value", move |_: &[PyObjectRef]| {
                        Ok(PyObject::int(*c3.read()))
                    }),
                );
                let c4 = counter.clone();
                let ir = inst_ref.clone();
                attrs.insert(
                    CompactString::from("__enter__"),
                    PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| {
                        let mut c = c4.write();
                        if *c > 0 {
                            *c -= 1;
                        }
                        Ok(ir.clone())
                    }),
                );
                let c5 = counter.clone();
                attrs.insert(
                    CompactString::from("__exit__"),
                    PyObject::native_closure("__exit__", move |_: &[PyObjectRef]| {
                        *c5.write() += 1;
                        Ok(PyObject::bool_val(false))
                    }),
                );
            }
            Ok(inst)
        });

    // Event — simple threading event using shared state
    let event_cls = PyObject::class(CompactString::from("Event"), vec![], IndexMap::new());
    let ec = event_cls.clone();
    let event_fn = PyObject::native_closure("Event", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(ec.clone());
        let flag = Rc::new(PyCell::new(false));
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let f1 = flag.clone();
            attrs.insert(
                CompactString::from("set"),
                PyObject::native_closure("set", move |_: &[PyObjectRef]| {
                    *f1.write() = true;
                    Ok(PyObject::none())
                }),
            );
            let f2 = flag.clone();
            attrs.insert(
                CompactString::from("clear"),
                PyObject::native_closure("clear", move |_: &[PyObjectRef]| {
                    *f2.write() = false;
                    Ok(PyObject::none())
                }),
            );
            let f3 = flag.clone();
            attrs.insert(
                CompactString::from("is_set"),
                PyObject::native_closure("is_set", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(*f3.read()))
                }),
            );
            let f4 = flag.clone();
            attrs.insert(
                CompactString::from("wait"),
                PyObject::native_closure("wait", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(*f4.read()))
                }),
            );
        }
        Ok(inst)
    });

    // Barrier — synchronization barrier
    let barrier_cls = PyObject::class(CompactString::from("Barrier"), vec![], IndexMap::new());
    let bc = barrier_cls.clone();
    let barrier_fn = PyObject::native_closure("Barrier", move |args: &[PyObjectRef]| {
        let parties = if !args.is_empty() {
            args[0].as_int().unwrap_or(1)
        } else {
            1
        };
        let inst = PyObject::instance(bc.clone());
        let waiting = Rc::new(PyCell::new(0i64));
        let broken = Rc::new(PyCell::new(false));
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(CompactString::from("parties"), PyObject::int(parties));
            let w1 = waiting.clone();
            attrs.insert(
                CompactString::from("n_waiting"),
                PyObject::native_closure("n_waiting", move |_: &[PyObjectRef]| {
                    Ok(PyObject::int(*w1.read()))
                }),
            );
            let w2 = waiting.clone();
            let b2 = broken.clone();
            let p = parties;
            attrs.insert(
                CompactString::from("wait"),
                PyObject::native_closure("wait", move |_: &[PyObjectRef]| {
                    if *b2.read() {
                        return Err(PyException::runtime_error("BrokenBarrierError"));
                    }
                    let mut w = w2.write();
                    *w += 1;
                    if *w >= p {
                        *w = 0;
                    }
                    Ok(PyObject::int(0))
                }),
            );
            let w3 = waiting.clone();
            attrs.insert(
                CompactString::from("reset"),
                PyObject::native_closure("reset", move |_: &[PyObjectRef]| {
                    *w3.write() = 0;
                    Ok(PyObject::none())
                }),
            );
            let w4 = waiting.clone();
            let b4 = broken.clone();
            attrs.insert(
                CompactString::from("abort"),
                PyObject::native_closure("abort", move |_: &[PyObjectRef]| {
                    *b4.write() = true;
                    *w4.write() = 0;
                    Ok(PyObject::none())
                }),
            );
            let b5 = broken.clone();
            attrs.insert(
                CompactString::from("broken"),
                PyObject::native_closure("broken", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(*b5.read()))
                }),
            );
        }
        Ok(inst)
    });

    // Condition — real condition variable using std::sync::Condvar
    let cond_cls = PyObject::class(CompactString::from("Condition"), vec![], IndexMap::new());
    let cc = cond_cls.clone();
    let condition_fn = PyObject::native_closure("Condition", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(cc.clone());
        let mutex = Arc::new(std::sync::Mutex::new(false));
        let condvar = Arc::new(std::sync::Condvar::new());
        let inst_ref = inst.clone();
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let m1 = mutex.clone();
            attrs.insert(
                CompactString::from("acquire"),
                PyObject::native_closure("acquire", move |_: &[PyObjectRef]| {
                    let _guard = m1.lock().unwrap();
                    Ok(PyObject::bool_val(true))
                }),
            );
            let m2 = mutex.clone();
            attrs.insert(
                CompactString::from("release"),
                PyObject::native_closure("release", move |_: &[PyObjectRef]| {
                    // Release is implicit when guard drops
                    let _guard = m2.lock();
                    Ok(PyObject::none())
                }),
            );
            // wait(timeout=None) — release lock, wait for notify, re-acquire lock
            let m3 = mutex.clone();
            let c3 = condvar.clone();
            attrs.insert(
                CompactString::from("wait"),
                PyObject::native_closure("wait", move |args: &[PyObjectRef]| {
                    let timeout = args
                        .get(0)
                        .and_then(|a| {
                            if matches!(&a.payload, PyObjectPayload::None) {
                                None
                            } else {
                                Some(a)
                            }
                        })
                        .and_then(|a| a.to_float().ok());
                    let guard = m3.lock().unwrap();
                    if let Some(secs) = timeout {
                        let dur = std::time::Duration::from_secs_f64(secs);
                        let _result = c3.wait_timeout(guard, dur).unwrap();
                    } else {
                        let _result = c3.wait(guard).unwrap();
                    }
                    Ok(PyObject::bool_val(true))
                }),
            );
            // wait_for(predicate, timeout=None) — simplified: evaluates predicate, waits if false
            let m3b = mutex.clone();
            let c3b = condvar.clone();
            attrs.insert(
                CompactString::from("wait_for"),
                PyObject::native_closure("wait_for", move |args: &[PyObjectRef]| {
                    // In CPython, wait_for calls wait() in a loop until predicate() is true.
                    // Since we can't call Python functions from native code without VM access,
                    // we do a single condvar wait then return True (like CPython's successful case).
                    let timeout = args
                        .get(1)
                        .and_then(|a| {
                            if matches!(&a.payload, PyObjectPayload::None) {
                                None
                            } else {
                                Some(a)
                            }
                        })
                        .and_then(|a| a.to_float().ok());
                    let guard = m3b.lock().unwrap();
                    if let Some(secs) = timeout {
                        let dur = std::time::Duration::from_secs_f64(secs);
                        let _result = c3b.wait_timeout(guard, dur).unwrap();
                    } else {
                        let _result = c3b.wait(guard).unwrap();
                    }
                    Ok(PyObject::bool_val(true))
                }),
            );
            let c4 = condvar.clone();
            attrs.insert(
                CompactString::from("notify"),
                PyObject::native_closure("notify", move |_: &[PyObjectRef]| {
                    c4.notify_one();
                    Ok(PyObject::none())
                }),
            );
            let c5 = condvar.clone();
            attrs.insert(
                CompactString::from("notify_all"),
                PyObject::native_closure("notify_all", move |_: &[PyObjectRef]| {
                    c5.notify_all();
                    Ok(PyObject::none())
                }),
            );
            let m6 = mutex.clone();
            let ir = inst_ref.clone();
            attrs.insert(
                CompactString::from("__enter__"),
                PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| {
                    let _guard = m6.lock().unwrap();
                    Ok(ir.clone())
                }),
            );
            let m7 = mutex.clone();
            attrs.insert(
                CompactString::from("__exit__"),
                PyObject::native_closure("__exit__", move |_: &[PyObjectRef]| {
                    let _guard = m7.lock();
                    Ok(PyObject::bool_val(false))
                }),
            );
        }
        Ok(inst)
    });

    // Timer — subclass of Thread with cancel()
    let timer_cls = PyObject::class(CompactString::from("Timer"), vec![], IndexMap::new());
    let tmc = timer_cls.clone();
    let timer_fn = PyObject::native_closure("Timer", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(tmc.clone());
        let cancelled = Rc::new(PyCell::new(false));
        let alive = Rc::new(PyCell::new(false));
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            // interval
            let interval = if !args.is_empty() {
                args[0].to_float().unwrap_or(0.0)
            } else {
                0.0
            };
            attrs.insert(CompactString::from("interval"), PyObject::float(interval));
            // Parse target/args from kwargs
            let mut target = PyObject::none();
            let mut fn_args = PyObject::tuple(vec![]);
            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(kw_map) = &last.payload {
                    let r = kw_map.read();
                    if let Some(t) = r.get(&HashableKey::str_key(CompactString::from("target"))) {
                        target = t.clone();
                    }
                    if let Some(a) = r.get(&HashableKey::str_key(CompactString::from("args"))) {
                        fn_args = a.clone();
                    }
                }
            }
            // Also check positional: Timer(interval, function, args)
            if args.len() >= 2 && matches!(&target.payload, PyObjectPayload::None) {
                target = args[1].clone();
            }
            if args.len() >= 3
                && matches!(&fn_args.payload, PyObjectPayload::Tuple(t) if t.is_empty())
            {
                fn_args = args[2].clone();
            }
            attrs.insert(CompactString::from("function"), target.clone());
            attrs.insert(CompactString::from("args"), fn_args.clone());
            attrs.insert(
                CompactString::from("name"),
                PyObject::str_val(CompactString::from("Timer")),
            );
            attrs.insert(CompactString::from("daemon"), PyObject::bool_val(false));

            let c1 = cancelled.clone();
            attrs.insert(
                CompactString::from("cancel"),
                PyObject::native_closure("cancel", move |_: &[PyObjectRef]| {
                    *c1.write() = true;
                    Ok(PyObject::none())
                }),
            );

            let c2 = cancelled.clone();
            let a1 = alive.clone();
            let tgt = target.clone();
            let targs = fn_args.clone();
            attrs.insert(
                CompactString::from("start"),
                PyObject::native_closure("start", move |_: &[PyObjectRef]| {
                    if *c2.read() {
                        return Ok(PyObject::none());
                    }
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
            let a2 = alive.clone();
            attrs.insert(
                CompactString::from("join"),
                PyObject::native_closure("join", move |args: &[PyObjectRef]| {
                    // Timer runs synchronously, so if alive, spin-wait with optional timeout
                    let timeout = args.first().and_then(|a| {
                        if matches!(&a.payload, PyObjectPayload::None) {
                            None
                        } else {
                            a.to_float().ok()
                        }
                    });
                    if *a2.read() {
                        if let Some(t) = timeout {
                            std::thread::sleep(std::time::Duration::from_secs_f64(t));
                        }
                    }
                    Ok(PyObject::none())
                }),
            );
            let a3 = alive.clone();
            let c3 = cancelled.clone();
            attrs.insert(
                CompactString::from("is_alive"),
                PyObject::native_closure("is_alive", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(*a3.read() && !*c3.read()))
                }),
            );
            attrs.insert(CompactString::from("ident"), PyObject::none());
        }
        Ok(inst)
    });

    // current_thread() — return Thread-like object
    let current_thread_fn =
        PyObject::native_closure("current_thread", move |_: &[PyObjectRef]| {
            let cls = PyObject::class(CompactString::from("Thread"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref i) = inst.payload {
                let mut attrs = i.attrs.write();
                attrs.insert(
                    CompactString::from("name"),
                    PyObject::str_val(CompactString::from("MainThread")),
                );
                attrs.insert(CompactString::from("ident"), PyObject::int(1));
                attrs.insert(CompactString::from("daemon"), PyObject::bool_val(false));
                attrs.insert(
                    CompactString::from("is_alive"),
                    make_builtin(|_| Ok(PyObject::bool_val(true))),
                );
                attrs.insert(
                    CompactString::from("getName"),
                    make_builtin(|_| Ok(PyObject::str_val(CompactString::from("MainThread")))),
                );
            }
            Ok(inst)
        });

    // active_count() — return count of active threads
    let active_count_fn = make_builtin(|_| Ok(PyObject::int(1)));

    // enumerate() — return list of active threads
    let enumerate_fn = PyObject::native_closure("enumerate", move |_: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("Thread"), vec![], IndexMap::new());
        let main = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref i) = main.payload {
            let mut attrs = i.attrs.write();
            attrs.insert(
                CompactString::from("name"),
                PyObject::str_val(CompactString::from("MainThread")),
            );
            attrs.insert(CompactString::from("ident"), PyObject::int(1));
            attrs.insert(CompactString::from("daemon"), PyObject::bool_val(false));
            attrs.insert(
                CompactString::from("is_alive"),
                make_builtin(|_| Ok(PyObject::bool_val(true))),
            );
        }
        Ok(PyObject::list(vec![main]))
    });

    make_module(
        "threading",
        vec![
            ("Thread", thread_class),
            ("Lock", lock_fn),
            ("RLock", rlock_fn),
            ("Event", event_fn),
            ("Semaphore", semaphore_fn.clone()),
            ("BoundedSemaphore", bounded_semaphore_fn),
            ("Condition", condition_fn),
            ("Barrier", barrier_fn),
            ("Timer", timer_fn),
            ("current_thread", current_thread_fn),
            ("active_count", active_count_fn),
            ("enumerate", enumerate_fn),
            (
                "main_thread",
                make_builtin(|_| {
                    let cls =
                        PyObject::class(CompactString::from("Thread"), vec![], IndexMap::new());
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref i) = inst.payload {
                        let mut attrs = i.attrs.write();
                        attrs.insert(
                            CompactString::from("name"),
                            PyObject::str_val(CompactString::from("MainThread")),
                        );
                        attrs.insert(CompactString::from("ident"), PyObject::int(1));
                        attrs.insert(CompactString::from("daemon"), PyObject::bool_val(false));
                        attrs.insert(
                            CompactString::from("is_alive"),
                            make_builtin(|_| Ok(PyObject::bool_val(true))),
                        );
                    }
                    Ok(inst)
                }),
            ),
            (
                "local",
                make_builtin(|_| {
                    let cls =
                        PyObject::class(CompactString::from("local"), vec![], IndexMap::new());
                    Ok(PyObject::instance(cls))
                }),
            ),
            (
                "get_ident",
                make_builtin(|_| {
                    let tid = std::thread::current().id();
                    let id_str = format!("{:?}", tid);
                    // Extract numeric id from "ThreadId(N)"
                    let num: i64 = id_str
                        .trim_start_matches("ThreadId(")
                        .trim_end_matches(')')
                        .parse()
                        .unwrap_or(1);
                    Ok(PyObject::int(num))
                }),
            ),
            (
                "get_native_id",
                make_builtin(|_| Ok(PyObject::int(std::process::id() as i64))),
            ),
            (
                "stack_size",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        Ok(PyObject::int(0))
                    } else {
                        Ok(PyObject::int(0))
                    }
                }),
            ),
            ("settrace", make_builtin(|_| Ok(PyObject::none()))),
            ("setprofile", make_builtin(|_| Ok(PyObject::none()))),
            ("excepthook", make_builtin(|_| Ok(PyObject::none()))),
            ("TIMEOUT_MAX", PyObject::float(f64::MAX)),
        ],
    )
}
