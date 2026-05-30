use crate::concurrency_modules::push_deferred_call;
use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

pub(super) fn create_thread_class() -> PyObjectRef {
    // Build Thread as a proper Class so subclasses inherit methods via MRO.
    let mut thread_ns = IndexMap::new();

    // __init__(self, *, target=None, args=(), daemon=False, name="Thread")
    thread_ns.insert(
        CompactString::from("__init__"),
        PyObject::native_function("Thread.__init__", |args: &[PyObjectRef]| {
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
        PyObject::native_function("Thread.start", |args: &[PyObjectRef]| {
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
                            // Python targets must run in the current VM so closures and shared
                            // interpreter objects observe the same state.
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
        PyObject::native_function("Thread.join", |args: &[PyObjectRef]| {
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
        PyObject::native_function("Thread.is_alive", |args: &[PyObjectRef]| {
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
        PyObject::native_function("Thread.getName", |args: &[PyObjectRef]| {
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
        PyObject::native_function("Thread.setDaemon", |args: &[PyObjectRef]| {
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
        PyObject::native_function("Thread.run", |args: &[PyObjectRef]| {
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

    PyObject::class(CompactString::from("Thread"), vec![], thread_ns)
}
