//! Concurrency stdlib modules (threading, weakref, gc, _thread)

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    call_callable, call_callable_kw, make_builtin, make_module, CompareOp, FxAttrMap, FxHashKeyMap,
    PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, PyWeakRef, SyncUsize,
    VecIterData, WeakKeyIterData, WeakKeyIterKind, WeakObjectKind, WeakValueIterData,
    WeakValueIterKind,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// SAFETY: GIL semantics — only one thread runs Python at a time.
/// This wrapper lets us move Rc-based values into thread::spawn closures.
#[allow(dead_code)]
struct UnsafeSend<T>(T);
unsafe impl<T> Send for UnsafeSend<T> {}
unsafe impl<T> Sync for UnsafeSend<T> {}

// Deferred call mechanism for NativeClosures that need the VM to call Python functions.
// Thread.start() pushes (target, args) here; the VM drains and executes them after NativeClosure returns.
thread_local! {
    pub static DEFERRED_CALLS: RefCell<Vec<(PyObjectRef, Vec<PyObjectRef>)>> = RefCell::new(Vec::new());
}

pub fn push_deferred_call(func: PyObjectRef, args: Vec<PyObjectRef>) {
    DEFERRED_CALLS.with(|dc| dc.borrow_mut().push((func, args)));
}

pub fn drain_deferred_calls() -> Vec<(PyObjectRef, Vec<PyObjectRef>)> {
    DEFERRED_CALLS.with(|dc| std::mem::take(&mut *dc.borrow_mut()))
}

fn is_weak_method_ref(obj: &PyObjectRef) -> bool {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        inst.attrs.read().contains_key("__weakmethod__")
    } else {
        false
    }
}

fn weak_method_parts(obj: &PyObjectRef) -> PyResult<Option<(PyObjectRef, PyObjectRef)>> {
    let Some(call) = obj.get_attr("__call__") else {
        return Ok(None);
    };
    let bound = call_callable(&call, &[])?;
    if matches!(&bound.payload, PyObjectPayload::None) {
        return Ok(None);
    }
    if let PyObjectPayload::BoundMethod { receiver, method } = &bound.payload {
        Ok(Some((receiver.clone(), method.clone())))
    } else {
        Ok(None)
    }
}

fn compare_weak_methods(
    this: &PyObjectRef,
    other: &PyObjectRef,
    op: CompareOp,
) -> PyResult<PyObjectRef> {
    let eq = match (weak_method_parts(this)?, weak_method_parts(other)?) {
        (Some((this_receiver, this_func)), Some((other_receiver, other_func))) => {
            PyObjectRef::ptr_eq(&this_func, &other_func)
                && this_receiver
                    .compare(&other_receiver, CompareOp::Eq)?
                    .is_truthy()
        }
        _ => false,
    };
    Ok(PyObject::bool_val(if matches!(op, CompareOp::Eq) {
        eq
    } else {
        !eq
    }))
}

// ── logging module ──

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
                        match &jh.payload {
                            PyObjectPayload::NativeClosure(nc) => {
                                let _ = (nc.func)(&[]);
                            }
                            _ => {}
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

// ── datetime module ──

pub fn create_weakref_module() -> PyObjectRef {
    type WeakKeyStorage = Rc<PyCell<IndexMap<usize, (PyObjectRef, PyObjectRef)>>>;
    type WeakValueStorage = Rc<PyCell<IndexMap<HashableKey, (PyObjectRef, PyObjectRef)>>>;

    let mut reference_type_namespace = IndexMap::new();
    reference_type_namespace.insert(
        CompactString::from("__slots__"),
        PyObject::tuple(Vec::new()),
    );
    let reference_type = PyObject::class(
        CompactString::from("weakref"),
        vec![],
        reference_type_namespace,
    );
    let proxy_type = PyObject::class(CompactString::from("weakproxy"), vec![], IndexMap::new());
    let callable_proxy_type = PyObject::class(
        CompactString::from("weakcallableproxy"),
        vec![],
        IndexMap::new(),
    );
    let ref_constructor_type = reference_type.clone();
    let proxy_constructor_type = proxy_type.clone();
    let callable_proxy_constructor_type = callable_proxy_type.clone();
    let finalize_reference_type = reference_type.clone();

    // Helper: upgrade a PyWeakRef or return None
    fn upgrade_or_none(weak: &PyWeakRef) -> PyObjectRef {
        match weak.upgrade() {
            Some(arc) => arc,
            None => PyObject::none(),
        }
    }

    fn weak_ref_call(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if let Some(this) = args.first() {
            if let PyObjectPayload::Instance(inst) = &this.payload {
                if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
                    return call_callable(&target_fn, &[]);
                }
            }
        }
        Ok(PyObject::none())
    }

    fn weak_ref_init(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let marker = HashableKey::str_key(CompactString::from("__weakref_ref_kwargs__"));
        let (args, has_kwargs_marker) =
            if let Some(PyObjectPayload::Dict(map)) = args.last().map(|arg| &arg.payload) {
                let has_marker = map.read().contains_key(&marker);
                if has_marker {
                    (&args[..args.len() - 1], true)
                } else {
                    (args, false)
                }
            } else {
                (args, false)
            };
        if has_kwargs_marker {
            return Err(PyException::type_error("ref() takes no keyword arguments"));
        }
        if args.len() > 3 {
            return Err(PyException::type_error(format!(
                "__init__() takes at most 2 arguments ({} given)",
                args.len().saturating_sub(1)
            )));
        }
        Ok(PyObject::none())
    }

    fn weak_ref_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "ref.__new__ requires type and object",
            ));
        }
        let marker = HashableKey::str_key(CompactString::from("__weakref_ref_kwargs__"));
        let (args, has_kwargs_marker) =
            if let Some(PyObjectPayload::Dict(map)) = args.last().map(|arg| &arg.payload) {
                let has_marker = map.read().contains_key(&marker);
                if has_marker {
                    (&args[..args.len() - 1], true)
                } else {
                    (args, false)
                }
            } else {
                (args, false)
            };
        if args.len() > 3 {
            return Err(PyException::type_error(format!(
                "ref() takes at most 2 arguments ({} given)",
                args.len() - 1
            )));
        }
        let cls = args[0].clone();
        if has_kwargs_marker {
            if let PyObjectPayload::Class(cd) = &cls.payload {
                if cd.name.as_str() == "weakref" {
                    return Err(PyException::type_error("ref() takes no keyword arguments"));
                }
            }
        }
        let target = args[1].clone();
        let callback = args.get(2).cloned().unwrap_or_else(PyObject::none);
        let callback = if matches!(callback.payload, PyObjectPayload::None) {
            None
        } else {
            Some(callback)
        };
        if callback.is_none() {
            if let PyObjectPayload::Class(cd) = &cls.payload {
                if cd.name.as_str() == "weakref" {
                    if let Some(existing) =
                        PyObjectRef::find_shared_weak_object(&target, WeakObjectKind::Ref)
                    {
                        if let PyObjectPayload::Instance(inst) = &existing.payload {
                            if let PyObjectPayload::Class(cd) = &inst.class.payload {
                                if cd.name.as_str() == "weakref" {
                                    return Ok(existing);
                                }
                            }
                        }
                    }
                }
            }
        }
        let weak: PyWeakRef = PyObjectRef::downgrade(&target);
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(
                CompactString::from("__weakref_ref__"),
                PyObject::bool_val(true),
            );
            attrs.insert(
                CompactString::from("__weakref_callback__"),
                callback.clone().unwrap_or_else(PyObject::none),
            );
            let target_weak = weak.clone();
            attrs.insert(
                CompactString::from("__weakref_target__"),
                PyObject::native_closure("weakref.__target__", move |_| {
                    Ok(upgrade_or_none(&target_weak))
                }),
            );
            let repr_weak = weak.clone();
            attrs.insert(
                CompactString::from("__repr__"),
                PyObject::native_closure("weakref.__repr__", move |_| {
                    if repr_weak.upgrade().is_some() {
                        Ok(PyObject::str_val(CompactString::from("<weakref (alive)>")))
                    } else {
                        Ok(PyObject::str_val(CompactString::from("<weakref (dead)>")))
                    }
                }),
            );
            let bool_weak = weak.clone();
            attrs.insert(
                CompactString::from("__bool__"),
                PyObject::native_closure("weakref.__bool__", move |_| {
                    Ok(PyObject::bool_val(bool_weak.upgrade().is_some()))
                }),
            );
        }
        PyObjectRef::register_weak_object(&target, &inst, callback, WeakObjectKind::Ref);
        Ok(inst)
    }

    let mut reference_namespace = IndexMap::new();
    reference_namespace.insert(
        CompactString::from("__new__"),
        PyObject::native_function("weakref.__new__", weak_ref_new),
    );
    reference_namespace.insert(
        CompactString::from("__init__"),
        PyObject::native_function("weakref.__init__", weak_ref_init),
    );
    reference_namespace.insert(
        CompactString::from("__call__"),
        PyObject::native_function("weakref.__call__", weak_ref_call),
    );
    reference_namespace.insert(
        CompactString::from("__eq__"),
        PyObject::native_closure("weakref.__eq__", |args| {
            if let (Some(this), Some(other)) = (args.first(), args.get(1)) {
                if PyObjectRef::ptr_eq(this, other) {
                    return Ok(PyObject::bool_val(true));
                }
                if is_weak_method_ref(this) || is_weak_method_ref(other) {
                    if !is_weak_method_ref(this) || !is_weak_method_ref(other) {
                        return Ok(PyObject::bool_val(false));
                    }
                    return compare_weak_methods(this, other, CompareOp::Eq);
                }
                let this_obj = weak_ref_call(&[this.clone()])?;
                let other_obj = weak_ref_call(&[other.clone()])?;
                if matches!(&this_obj.payload, PyObjectPayload::None)
                    || matches!(&other_obj.payload, PyObjectPayload::None)
                {
                    return Ok(PyObject::bool_val(false));
                }
                return this_obj.compare(&other_obj, CompareOp::Eq);
            }
            Ok(PyObject::bool_val(false))
        }),
    );
    reference_namespace.insert(
        CompactString::from("__ne__"),
        PyObject::native_closure("weakref.__ne__", |args| {
            if let (Some(this), Some(other)) = (args.first(), args.get(1)) {
                if PyObjectRef::ptr_eq(this, other) {
                    return Ok(PyObject::bool_val(false));
                }
                if is_weak_method_ref(this) || is_weak_method_ref(other) {
                    if !is_weak_method_ref(this) || !is_weak_method_ref(other) {
                        return Ok(PyObject::bool_val(true));
                    }
                    return compare_weak_methods(this, other, CompareOp::Ne);
                }
                let this_obj = weak_ref_call(&[this.clone()])?;
                let other_obj = weak_ref_call(&[other.clone()])?;
                if matches!(&this_obj.payload, PyObjectPayload::None)
                    || matches!(&other_obj.payload, PyObjectPayload::None)
                {
                    return Ok(PyObject::bool_val(true));
                }
                return this_obj.compare(&other_obj, CompareOp::Ne);
            }
            Ok(PyObject::bool_val(true))
        }),
    );
    if let PyObjectPayload::Class(cd) = &reference_type.payload {
        cd.namespace.write().extend(reference_namespace);
        cd.has_custom_new.set(true);
        cd.is_simple_class.set(false);
        cd.method_cache.write().clear();
    }

    // Helper: upgrade a PyWeakRef or raise ReferenceError
    fn upgrade_or_err(weak: &PyWeakRef) -> Result<PyObjectRef, PyException> {
        weak.upgrade().ok_or_else(|| {
            PyException::new(
                ferrython_core::error::ExceptionKind::ReferenceError,
                "weakly-referenced object no longer exists",
            )
        })
    }

    fn weak_ref_target(ref_obj: &PyObjectRef) -> Option<PyObjectRef> {
        let PyObjectPayload::Instance(inst) = &ref_obj.payload else {
            return None;
        };
        let target_fn = inst.attrs.read().get("__weakref_target__").cloned()?;
        call_callable(&target_fn, &[]).ok().and_then(|obj| {
            if matches!(&obj.payload, PyObjectPayload::None) {
                None
            } else {
                Some(obj)
            }
        })
    }

    fn weak_key_items(storage: &WeakKeyStorage) -> Vec<(PyObjectRef, PyObjectRef)> {
        let mut store = storage.write();
        store.retain(|_, (r, _)| weak_ref_target(r).is_some());
        store
            .iter()
            .filter_map(|(_, (r, v))| weak_ref_target(r).map(|k| (k, v.clone())))
            .collect()
    }

    fn weak_value_items(storage: &WeakValueStorage) -> Vec<(PyObjectRef, PyObjectRef)> {
        let mut store = storage.write();
        store.retain(|_, (_, r)| weak_ref_target(r).is_some());
        store
            .iter()
            .filter_map(|(_, (k, r))| weak_ref_target(r).map(|v| (k.clone(), v)))
            .collect()
    }

    fn weak_iter(items: Vec<PyObjectRef>) -> PyObjectRef {
        PyObject::wrap(PyObjectPayload::VecIter(Box::new(VecIterData {
            items,
            index: SyncUsize::new(0),
        })))
    }

    fn weak_value_iter(storage: &WeakValueStorage, kind: WeakValueIterKind) -> PyObjectRef {
        let mut store = storage.write();
        store.retain(|_, (_, r)| weak_ref_target(r).is_some());
        let entries = store
            .values()
            .map(|(key, ref_obj)| (key.clone(), ref_obj.clone()))
            .collect();
        PyObject::wrap(PyObjectPayload::WeakValueIter(Box::new(
            WeakValueIterData {
                entries,
                index: SyncUsize::new(0),
                kind,
            },
        )))
    }

    fn weak_key_iter(storage: &WeakKeyStorage, kind: WeakKeyIterKind) -> PyObjectRef {
        let mut store = storage.write();
        store.retain(|_, (r, _)| weak_ref_target(r).is_some());
        let entries = store
            .values()
            .map(|(ref_obj, value)| (ref_obj.clone(), value.clone()))
            .collect();
        PyObject::wrap(PyObjectPayload::WeakKeyIter(Box::new(WeakKeyIterData {
            entries,
            index: SyncUsize::new(0),
            kind,
        })))
    }

    fn pair_from_internal_item(item: PyObjectRef) -> PyResult<(PyObjectRef, PyObjectRef)> {
        match &item.payload {
            PyObjectPayload::Tuple(items) if items.len() == 2 => {
                Ok((items[0].clone(), items[1].clone()))
            }
            _ => Err(PyException::type_error("invalid weakdict item")),
        }
    }

    fn internal_mapping_items(
        obj: &PyObjectRef,
        name: &str,
    ) -> Option<PyResult<Vec<(PyObjectRef, PyObjectRef)>>> {
        let PyObjectPayload::Instance(inst) = &obj.payload else {
            return None;
        };
        let items_fn = inst.attrs.read().get(name).cloned()?;
        Some(call_callable(&items_fn, &[]).and_then(|items| {
            items
                .to_list()?
                .into_iter()
                .map(pair_from_internal_item)
                .collect()
        }))
    }

    fn weak_mapping_items(obj: &PyObjectRef) -> PyResult<Vec<(PyObjectRef, PyObjectRef)>> {
        fn pair_from_object(item: PyObjectRef) -> PyResult<(PyObjectRef, PyObjectRef)> {
            match &item.payload {
                PyObjectPayload::Tuple(items) if items.len() == 2 => {
                    return Ok((items[0].clone(), items[1].clone()));
                }
                PyObjectPayload::List(items) if items.read().len() == 2 => {
                    let items = items.read();
                    return Ok((items[0].clone(), items[1].clone()));
                }
                _ => {}
            }
            let pair = item.to_list()?;
            if pair.len() != 2 {
                return Err(PyException::value_error(
                    "dictionary update sequence element has length other than 2",
                ));
            }
            Ok((pair[0].clone(), pair[1].clone()))
        }

        if let Some(items) = internal_mapping_items(obj, "__weakvalue_items__") {
            return items;
        }
        if let Some(items) = internal_mapping_items(obj, "__weakkey_items__") {
            return items;
        }

        match &obj.payload {
            PyObjectPayload::Dict(map) => {
                return Ok(map
                    .read()
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.original_object().unwrap_or_else(|| k.to_object()),
                            v.clone(),
                        )
                    })
                    .collect())
            }
            PyObjectPayload::Instance(inst) => {
                if let Some(storage) = inst.dict_storage.as_ref() {
                    return Ok(storage
                        .read()
                        .iter()
                        .map(|(k, v)| {
                            (
                                k.original_object().unwrap_or_else(|| k.to_object()),
                                v.clone(),
                            )
                        })
                        .collect());
                }
                if let Some(items_fn) = obj.get_attr("items") {
                    let items = call_callable(&items_fn, &[])?;
                    return items.to_list()?.into_iter().map(pair_from_object).collect();
                }
                if let Some(keys_fn) = obj.get_attr("keys") {
                    let keys = call_callable(&keys_fn, &[])?;
                    let mut items = Vec::new();
                    for key in keys.to_list()? {
                        let value = obj.get_item(&key)?;
                        items.push((key, value));
                    }
                    return Ok(items);
                }
            }
            _ => {}
        }
        obj.to_list()?.into_iter().map(pair_from_object).collect()
    }

    fn weak_key_update_from_dict_storage(
        storage: &WeakKeyStorage,
        source: &Rc<PyCell<FxHashKeyMap>>,
    ) -> PyResult<()> {
        for (key, value) in source.read().iter() {
            weak_key_set(
                storage,
                key.original_object().unwrap_or_else(|| key.to_object()),
                value.clone(),
            )?;
        }
        Ok(())
    }

    fn weak_ref_object(target: &PyObjectRef) -> PyObjectRef {
        let weak = PyObjectRef::downgrade(target);
        let cls = PyObject::class(CompactString::from("weakref"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(
                CompactString::from("__weakref_ref__"),
                PyObject::bool_val(true),
            );
            let w_call = weak.clone();
            attrs.insert(
                CompactString::from("__call__"),
                PyObject::native_closure("weakref.__call__", move |_| Ok(upgrade_or_none(&w_call))),
            );
            let w_target = weak.clone();
            attrs.insert(
                CompactString::from("__weakref_target__"),
                PyObject::native_closure("weakref.__target__", move |_| {
                    Ok(upgrade_or_none(&w_target))
                }),
            );
            let w_repr = weak.clone();
            attrs.insert(
                CompactString::from("__repr__"),
                PyObject::native_closure("weakref.__repr__", move |_| {
                    if w_repr.upgrade().is_some() {
                        Ok(PyObject::str_val(CompactString::from("<weakref (alive)>")))
                    } else {
                        Ok(PyObject::str_val(CompactString::from("<weakref (dead)>")))
                    }
                }),
            );
        }
        PyObjectRef::register_weak_object(target, &inst, None, WeakObjectKind::Ref);
        inst
    }

    fn weak_value_set(
        storage: &WeakValueStorage,
        key_obj: PyObjectRef,
        value: PyObjectRef,
    ) -> PyResult<()> {
        let key = key_obj.to_hashable_key()?;
        let ref_obj = weak_ref_object(&value);
        storage.write().insert(key, (key_obj, ref_obj));
        Ok(())
    }

    fn weak_value_get_alive(
        storage: &WeakValueStorage,
        key_obj: &PyObjectRef,
    ) -> PyResult<Option<PyObjectRef>> {
        let key = key_obj.to_hashable_key()?;
        let mut store = storage.write();
        match store
            .get(&key)
            .and_then(|(_, ref_obj)| weak_ref_target(ref_obj))
        {
            Some(obj) => Ok(Some(obj)),
            None if store.contains_key(&key) => {
                store.shift_remove(&key);
                Ok(None)
            }
            None => Ok(None),
        }
    }

    fn weak_key_require_weakable(key: &PyObjectRef) -> PyResult<()> {
        match &key.payload {
            PyObjectPayload::Int(_)
            | PyObjectPayload::Bool(_)
            | PyObjectPayload::Float(_)
            | PyObjectPayload::Complex { .. }
            | PyObjectPayload::Str(_)
            | PyObjectPayload::Bytes(_)
            | PyObjectPayload::ByteArray(_)
            | PyObjectPayload::Tuple(_)
            | PyObjectPayload::List(_)
            | PyObjectPayload::Dict(_)
            | PyObjectPayload::Set(_)
            | PyObjectPayload::FrozenSet(_) => Err(PyException::type_error(format!(
                "cannot create weak reference to '{}' object",
                key.type_name()
            ))),
            _ => Ok(()),
        }
    }

    fn weak_key_set(
        storage: &WeakKeyStorage,
        key: PyObjectRef,
        value: PyObjectRef,
    ) -> PyResult<()> {
        weak_key_require_weakable(&key)?;
        let ptr = PyObjectRef::as_ptr(&key) as usize;
        let ref_obj = weak_ref_object(&key);
        storage.write().insert(ptr, (ref_obj, value));
        Ok(())
    }

    fn weak_key_lookup_ptr(
        store: &IndexMap<usize, (PyObjectRef, PyObjectRef)>,
        key: &PyObjectRef,
    ) -> PyResult<Option<usize>> {
        let key_hash = key.to_hashable_key()?;
        for (ptr, (ref_obj, _)) in store.iter() {
            let Some(live_key) = weak_ref_target(ref_obj) else {
                continue;
            };
            if live_key.to_hashable_key()?.hash_key() != key_hash.hash_key() {
                continue;
            }
            let eq_result = if let Some(eq_method) = live_key.get_attr("__eq__") {
                call_callable(&eq_method, &[key.clone()])?
            } else {
                live_key.compare(key, CompareOp::Eq)?
            };
            if !matches!(&eq_result.payload, PyObjectPayload::NotImplemented)
                && eq_result.is_truthy()
            {
                return Ok(Some(*ptr));
            }
        }
        Ok(None)
    }

    fn weak_key_get_alive(
        storage: &WeakKeyStorage,
        key: &PyObjectRef,
        strict: bool,
    ) -> PyResult<Option<PyObjectRef>> {
        if strict {
            weak_key_require_weakable(key)?;
        }
        let mut store = storage.write();
        store.retain(|_, (ref_obj, _)| weak_ref_target(ref_obj).is_some());
        let Some(ptr) = weak_key_lookup_ptr(&store, key)? else {
            return Ok(None);
        };
        if let Some((_, val)) = store.get(&ptr) {
            Ok(Some(val.clone()))
        } else {
            Ok(None)
        }
    }

    fn py_default_key_error(key: &PyObjectRef) -> PyException {
        PyException::new(ExceptionKind::KeyError, key.repr())
    }

    fn weak_kwargs_marker_key() -> HashableKey {
        HashableKey::str_key(CompactString::from("__weakdict_kwargs__"))
    }

    fn weak_kwargs_items(obj: &PyObjectRef) -> Option<Vec<(PyObjectRef, PyObjectRef)>> {
        let PyObjectPayload::Dict(map) = &obj.payload else {
            return None;
        };
        let marker = weak_kwargs_marker_key();
        let map = map.read();
        if !map.contains_key(&marker) {
            return None;
        }
        Some(
            map.iter()
                .filter(|(k, _)| *k != &marker)
                .map(|(k, v)| {
                    (
                        k.original_object().unwrap_or_else(|| k.to_object()),
                        v.clone(),
                    )
                })
                .collect(),
        )
    }

    fn finalize_kwargs_marker_key() -> HashableKey {
        HashableKey::str_key(CompactString::from("__finalize_kwargs__"))
    }

    fn extract_marked_kwargs(
        args: &[PyObjectRef],
        marker: HashableKey,
    ) -> (Vec<PyObjectRef>, IndexMap<HashableKey, PyObjectRef>) {
        let Some((last, rest)) = args.split_last() else {
            return (Vec::new(), IndexMap::new());
        };
        let PyObjectPayload::Dict(map) = &last.payload else {
            return (args.to_vec(), IndexMap::new());
        };
        let map = map.read();
        if !map.contains_key(&marker) {
            return (args.to_vec(), IndexMap::new());
        }
        let kwargs = map
            .iter()
            .filter(|(key, _)| *key != &marker)
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        (rest.to_vec(), kwargs)
    }

    fn kwargs_without_keys(
        kwargs: &IndexMap<HashableKey, PyObjectRef>,
        names: &[&str],
    ) -> IndexMap<HashableKey, PyObjectRef> {
        kwargs
            .iter()
            .filter(|(key, _)| {
                !matches!(key, HashableKey::Str(s) if names.iter().any(|name| s.as_str() == *name))
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    fn kwargs_to_call_pairs(
        kwargs: &IndexMap<HashableKey, PyObjectRef>,
    ) -> PyResult<Vec<(CompactString, PyObjectRef)>> {
        let mut pairs = Vec::with_capacity(kwargs.len());
        for (key, value) in kwargs {
            let HashableKey::Str(name) = key else {
                return Err(PyException::type_error("keywords must be strings"));
            };
            pairs.push((CompactString::from(name.as_str()), value.clone()));
        }
        Ok(pairs)
    }

    fn warn_finalize_keyword_form() -> PyResult<()> {
        if let Some(warnings) = crate::load_module("warnings") {
            if let (Some(warn_fn), Some(dep_cls)) = (
                warnings.get_attr("warn"),
                warnings.get_attr("DeprecationWarning"),
            ) {
                call_callable(
                    &warn_fn,
                    &[
                        PyObject::str_val(CompactString::from(
                            "Passing obj or func as keyword arguments to weakref.finalize is deprecated",
                        )),
                        dep_cls,
                    ],
                )?;
            }
        }
        Ok(())
    }

    fn finalize_call_from_state(
        alive_state: &Rc<Cell<bool>>,
        attrs_ref: &Rc<PyCell<FxAttrMap>>,
        func: &PyObjectRef,
        extra: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<PyObjectRef> {
        if !alive_state.replace(false) {
            return Ok(PyObject::none());
        }
        {
            let mut attrs = attrs_ref.write();
            attrs.insert(CompactString::from("alive"), PyObject::bool_val(false));
            attrs.shift_remove("_func");
            attrs.shift_remove("_args");
            attrs.shift_remove("_kwargs");
            attrs.insert(
                CompactString::from("__call__"),
                PyObject::native_closure("finalize.__call__", |_| Ok(PyObject::none())),
            );
            attrs.insert(
                CompactString::from("detach"),
                PyObject::native_closure("finalize.detach", |_| Ok(PyObject::none())),
            );
            attrs.insert(
                CompactString::from("peek"),
                PyObject::native_closure("finalize.peek", |_| Ok(PyObject::none())),
            );
        }
        match &func.payload {
            PyObjectPayload::NativeFunction(nf) if kwargs.is_empty() => (nf.func)(extra),
            PyObjectPayload::NativeClosure(nc) if kwargs.is_empty() => (nc.func)(extra),
            _ => call_callable_kw(func, extra, kwargs.to_vec()),
        }
    }

    fn finalize_new(args: &[PyObjectRef], reference_type: &PyObjectRef) -> PyResult<PyObjectRef> {
        let marker = finalize_kwargs_marker_key();
        let (pos_args, kwargs) = extract_marked_kwargs(args, marker);
        if pos_args.is_empty() {
            return Err(PyException::type_error("finalize requires obj and func"));
        }
        let cls = pos_args[0].clone();
        let user_args = &pos_args[1..];
        let (obj, func, extra_start, call_kwargs) = if user_args.len() >= 2 {
            (user_args[0].clone(), user_args[1].clone(), 2, kwargs)
        } else {
            warn_finalize_keyword_form()?;
            let obj = if !user_args.is_empty() {
                Some(user_args[0].clone())
            } else {
                kwargs
                    .get(&HashableKey::str_key(CompactString::from("obj")))
                    .cloned()
            };
            let func = kwargs
                .get(&HashableKey::str_key(CompactString::from("func")))
                .cloned();
            let Some(obj) = obj else {
                return Err(PyException::type_error("finalize requires obj and func"));
            };
            let Some(func) = func else {
                return Err(PyException::type_error("finalize requires obj and func"));
            };
            (
                obj,
                func,
                user_args.len(),
                kwargs_without_keys(
                    &kwargs,
                    if user_args.is_empty() {
                        &["obj", "func"]
                    } else {
                        &["func"]
                    },
                ),
            )
        };

        let weak: PyWeakRef = PyObjectRef::downgrade(&obj);
        let extra = user_args[extra_start..].to_vec();
        let kwargs_obj = PyObject::dict(call_kwargs.clone());
        let kwargs_for_call = kwargs_to_call_pairs(&call_kwargs)?;

        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let alive_state = Rc::new(Cell::new(true));
            let atexit_handle: Rc<RefCell<Option<PyObjectRef>>> = Rc::new(RefCell::new(None));
            let attrs_ref = inst_data.attrs.clone();
            {
                let mut attrs = attrs_ref.write();
                attrs.insert(CompactString::from("alive"), PyObject::bool_val(true));
                attrs.insert(CompactString::from("atexit"), PyObject::bool_val(true));
                attrs.insert(CompactString::from("_func"), func.clone());
                attrs.insert(CompactString::from("_args"), PyObject::tuple(extra.clone()));
                attrs.insert(CompactString::from("_kwargs"), kwargs_obj.clone());

                let w_det = weak.clone();
                let f_det = func.clone();
                let e_det = extra.clone();
                let k_det = kwargs_obj.clone();
                let alive_det = alive_state.clone();
                let attrs_det = attrs_ref.clone();
                let atexit_det = atexit_handle.clone();
                attrs.insert(
                    CompactString::from("detach"),
                    PyObject::native_closure("finalize.detach", move |_| {
                        if !alive_det.replace(false) {
                            return Ok(PyObject::none());
                        }
                        if let Some(callback) = atexit_det.borrow_mut().take() {
                            crate::sys_modules::unregister_atexit_callback(&callback);
                        }
                        attrs_det
                            .write()
                            .insert(CompactString::from("alive"), PyObject::bool_val(false));
                        match w_det.upgrade() {
                            Some(obj) => Ok(PyObject::tuple(vec![
                                obj,
                                f_det.clone(),
                                PyObject::tuple(e_det.clone()),
                                k_det.clone(),
                            ])),
                            None => Ok(PyObject::none()),
                        }
                    }),
                );

                let w_peek = weak.clone();
                let f_peek = func.clone();
                let e_peek = extra.clone();
                let k_peek = kwargs_obj.clone();
                let alive_peek = alive_state.clone();
                attrs.insert(
                    CompactString::from("peek"),
                    PyObject::native_closure("finalize.peek", move |_| {
                        if !alive_peek.get() {
                            return Ok(PyObject::none());
                        }
                        match w_peek.upgrade() {
                            Some(obj) => Ok(PyObject::tuple(vec![
                                obj,
                                f_peek.clone(),
                                PyObject::tuple(e_peek.clone()),
                                k_peek.clone(),
                            ])),
                            None => Ok(PyObject::none()),
                        }
                    }),
                );

                let f_call = func.clone();
                let e_call = extra.clone();
                let k_call = kwargs_for_call.clone();
                let alive_call = alive_state.clone();
                let attrs_call = attrs_ref.clone();
                let atexit_call = atexit_handle.clone();
                attrs.insert(
                    CompactString::from("__call__"),
                    PyObject::native_closure("finalize.__call__", move |_| {
                        if let Some(callback) = atexit_call.borrow_mut().take() {
                            crate::sys_modules::unregister_atexit_callback(&callback);
                        }
                        finalize_call_from_state(
                            &alive_call,
                            &attrs_call,
                            &f_call,
                            &e_call,
                            &k_call,
                        )
                    }),
                );
            }

            let weak_inst = PyObject::instance(reference_type.clone());
            if let PyObjectPayload::Instance(ref weak_data) = weak_inst.payload {
                let mut weak_attrs = weak_data.attrs.write();
                let w_target = weak.clone();
                weak_attrs.insert(
                    CompactString::from("__weakref_target__"),
                    PyObject::native_closure("finalize.weakref.__target__", move |_| {
                        Ok(upgrade_or_none(&w_target))
                    }),
                );
            }

            let f_exit = func.clone();
            let e_exit = extra.clone();
            let k_exit = kwargs_for_call.clone();
            let alive_exit = alive_state.clone();
            let attrs_exit = attrs_ref.clone();
            let inst_exit = inst.clone();
            let atexit_callback = PyObject::native_closure("finalize.__atexit__", move |_| {
                if let Some(atexit) = inst_exit.get_attr("atexit") {
                    if !atexit.is_truthy() {
                        return Ok(PyObject::none());
                    }
                }
                finalize_call_from_state(&alive_exit, &attrs_exit, &f_exit, &e_exit, &k_exit)
            });
            *atexit_handle.borrow_mut() = Some(atexit_callback.clone());
            crate::sys_modules::register_atexit_callback(atexit_callback, Vec::new(), Vec::new());

            let f_auto = func;
            let e_auto = extra;
            let k_auto = kwargs_for_call;
            let alive_auto = alive_state;
            let attrs_auto = attrs_ref;
            let atexit_auto = atexit_handle;
            let inst_keepalive = inst.clone();
            let weak_keepalive = weak_inst.clone();
            let callback = Some(PyObject::native_closure(
                "finalize.__callback__",
                move |_| {
                    if let Some(callback) = atexit_auto.borrow_mut().take() {
                        crate::sys_modules::unregister_atexit_callback(&callback);
                    }
                    let _keepalive_inst = &inst_keepalive;
                    let _keepalive = &weak_keepalive;
                    finalize_call_from_state(&alive_auto, &attrs_auto, &f_auto, &e_auto, &k_auto)
                },
            ));
            PyObjectRef::register_weak_object(&obj, &weak_inst, callback, WeakObjectKind::Ref);
        }
        Ok(inst)
    }

    fn weak_mapping_eq(left: &[(PyObjectRef, PyObjectRef)], other: &PyObjectRef) -> PyResult<bool> {
        let right = if let Some(items) = internal_mapping_items(other, "__weakvalue_items__") {
            items?
        } else if let Some(items) = internal_mapping_items(other, "__weakkey_items__") {
            items?
        } else if let Some(items_fn) = other.get_attr("items") {
            let other_items = ferrython_core::object::call_callable(&items_fn, &[])?;
            other_items
                .to_list()?
                .into_iter()
                .map(pair_from_internal_item)
                .collect::<PyResult<Vec<_>>>()?
        } else {
            return Ok(false);
        };

        if left.len() != right.len() {
            return Ok(false);
        }
        for (lk, lv) in left {
            let mut found = false;
            for (rk, rv) in &right {
                let key_eq = lk.compare(rk, CompareOp::Eq)?.is_truthy();
                if key_eq {
                    let value_eq = lv.compare(rv, CompareOp::Eq)?.is_truthy();
                    if !value_eq {
                        return Ok(false);
                    }
                    found = true;
                    break;
                }
            }
            if !found {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn weak_value_update_args(storage: &WeakValueStorage, args: &[PyObjectRef]) -> PyResult<()> {
        let (source, kwargs) = match args {
            [] => (None, None),
            [only] => {
                if let Some(items) = weak_kwargs_items(only) {
                    (None, Some(items))
                } else {
                    (Some(only), None)
                }
            }
            [source, kwargs] => match weak_kwargs_items(kwargs) {
                Some(items) => (Some(source), Some(items)),
                None => {
                    return Err(PyException::type_error(
                        "WeakValueDictionary expected at most 1 argument",
                    ))
                }
            },
            _ => {
                return Err(PyException::type_error(
                    "WeakValueDictionary expected at most 1 argument",
                ))
            }
        };
        if let Some(source) = source {
            for (key, value) in weak_mapping_items(source)? {
                weak_value_set(storage, key, value)?;
            }
        }
        if let Some(items) = kwargs {
            for (key, value) in items {
                weak_value_set(storage, key, value)?;
            }
        }
        Ok(())
    }

    fn weak_key_update_args(storage: &WeakKeyStorage, args: &[PyObjectRef]) -> PyResult<()> {
        let (source, kwargs) = match args {
            [] => (None, None),
            [only] => {
                if let Some(items) = weak_kwargs_items(only) {
                    (None, Some(items))
                } else {
                    (Some(only), None)
                }
            }
            [source, kwargs] => match weak_kwargs_items(kwargs) {
                Some(items) => (Some(source), Some(items)),
                None => {
                    return Err(PyException::type_error(
                        "WeakKeyDictionary expected at most 1 argument",
                    ))
                }
            },
            _ => {
                return Err(PyException::type_error(
                    "WeakKeyDictionary expected at most 1 argument",
                ))
            }
        };
        if let Some(source) = source {
            match &source.payload {
                PyObjectPayload::Dict(map) => weak_key_update_from_dict_storage(storage, map)?,
                PyObjectPayload::Instance(inst) => {
                    if let Some(map) = inst.dict_storage.as_ref() {
                        weak_key_update_from_dict_storage(storage, map)?;
                    } else {
                        for (key, value) in weak_mapping_items(source)? {
                            weak_key_set(storage, key, value)?;
                        }
                    }
                }
                _ => {
                    for (key, value) in weak_mapping_items(source)? {
                        weak_key_set(storage, key, value)?;
                    }
                }
            }
        }
        if let Some(items) = kwargs {
            for (key, value) in items {
                weak_key_set(storage, key, value)?;
            }
        }
        Ok(())
    }

    fn build_weak_value_dictionary(storage: WeakValueStorage) -> PyObjectRef {
        let mut class_ns = IndexMap::new();
        let eq_storage = storage.clone();
        class_ns.insert(
            CompactString::from("__eq__"),
            PyObject::native_closure("WeakValueDictionary.__eq__", move |args| {
                if args.len() < 2 {
                    return Err(PyException::type_error("__eq__ requires an argument"));
                }
                let items = weak_value_items(&eq_storage);
                Ok(PyObject::bool_val(weak_mapping_eq(&items, &args[1])?))
            }),
        );
        let ne_storage = storage.clone();
        class_ns.insert(
            CompactString::from("__ne__"),
            PyObject::native_closure("WeakValueDictionary.__ne__", move |args| {
                if args.len() < 2 {
                    return Err(PyException::type_error("__ne__ requires an argument"));
                }
                let items = weak_value_items(&ne_storage);
                Ok(PyObject::bool_val(!weak_mapping_eq(&items, &args[1])?))
            }),
        );
        class_ns.insert(
            CompactString::from("__repr__"),
            PyObject::native_closure("WeakValueDictionary.__repr__", move |args| {
                let ptr = args
                    .first()
                    .map(|obj| PyObjectRef::as_ptr(obj) as usize)
                    .unwrap_or(0);
                Ok(PyObject::str_val(CompactString::from(format!(
                    "<WeakValueDictionary at 0x{:x}>",
                    ptr
                ))))
            }),
        );
        let cls = PyObject::class(CompactString::from("WeakValueDictionary"), vec![], class_ns);
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(
                CompactString::from("__weakvaluedict__"),
                PyObject::bool_val(true),
            );
            let internal_items_storage = storage.clone();
            attrs.insert(
                CompactString::from("__weakvalue_items__"),
                PyObject::native_closure("WeakValueDictionary.__weakvalue_items__", move |_| {
                    let items = weak_value_items(&internal_items_storage)
                        .into_iter()
                        .map(|(key, value)| PyObject::tuple(vec![key, value]))
                        .collect();
                    Ok(PyObject::list(items))
                }),
            );

            let set_storage = storage.clone();
            attrs.insert(
                CompactString::from("__setitem__"),
                PyObject::native_closure("WeakValueDictionary.__setitem__", move |args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error(
                            "__setitem__ requires key and value",
                        ));
                    }
                    weak_value_set(&set_storage, args[0].clone(), args[1].clone())?;
                    Ok(PyObject::none())
                }),
            );

            let get_storage = storage.clone();
            attrs.insert(
                CompactString::from("__getitem__"),
                PyObject::native_closure("WeakValueDictionary.__getitem__", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("__getitem__ requires a key"));
                    }
                    match weak_value_get_alive(&get_storage, &args[0])? {
                        Some(obj) => Ok(obj),
                        None => Err(py_default_key_error(&args[0])),
                    }
                }),
            );

            let del_storage = storage.clone();
            attrs.insert(
                CompactString::from("__delitem__"),
                PyObject::native_closure("WeakValueDictionary.__delitem__", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("__delitem__ requires a key"));
                    }
                    let key = args[0].to_hashable_key()?;
                    let mut store = del_storage.write();
                    match store.get(&key).and_then(|(_, r)| weak_ref_target(r)) {
                        Some(_) => {
                            store.shift_remove(&key);
                            Ok(PyObject::none())
                        }
                        None if store.contains_key(&key) => {
                            store.shift_remove(&key);
                            Err(py_default_key_error(&args[0]))
                        }
                        None => Err(py_default_key_error(&args[0])),
                    }
                }),
            );

            let contains_storage = storage.clone();
            attrs.insert(
                CompactString::from("__contains__"),
                PyObject::native_closure("WeakValueDictionary.__contains__", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("__contains__ requires a key"));
                    }
                    Ok(PyObject::bool_val(
                        weak_value_get_alive(&contains_storage, &args[0])?.is_some(),
                    ))
                }),
            );

            let len_storage = storage.clone();
            attrs.insert(
                CompactString::from("__len__"),
                PyObject::native_closure("WeakValueDictionary.__len__", move |_| {
                    let mut store = len_storage.write();
                    store.retain(|_, (_, r)| weak_ref_target(r).is_some());
                    Ok(PyObject::int(store.len() as i64))
                }),
            );

            let bool_storage = storage.clone();
            attrs.insert(
                CompactString::from("__bool__"),
                PyObject::native_closure("WeakValueDictionary.__bool__", move |_| {
                    let mut store = bool_storage.write();
                    store.retain(|_, (_, r)| weak_ref_target(r).is_some());
                    Ok(PyObject::bool_val(!store.is_empty()))
                }),
            );

            let get_method_storage = storage.clone();
            attrs.insert(
                CompactString::from("get"),
                PyObject::native_closure("WeakValueDictionary.get", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("get() requires a key"));
                    }
                    if args.len() > 2 {
                        return Err(PyException::type_error("get expected at most 2 arguments"));
                    }
                    let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
                    Ok(weak_value_get_alive(&get_method_storage, &args[0])?.unwrap_or(default))
                }),
            );

            let keys_storage = storage.clone();
            attrs.insert(
                CompactString::from("keys"),
                PyObject::native_closure("WeakValueDictionary.keys", move |args| {
                    if !args.is_empty() {
                        return Err(PyException::type_error("keys() takes no arguments"));
                    }
                    Ok(weak_value_iter(&keys_storage, WeakValueIterKind::Keys))
                }),
            );

            let iter_storage = storage.clone();
            attrs.insert(
                CompactString::from("__iter__"),
                PyObject::native_closure("WeakValueDictionary.__iter__", move |_| {
                    Ok(weak_value_iter(&iter_storage, WeakValueIterKind::Keys))
                }),
            );

            let values_storage = storage.clone();
            attrs.insert(
                CompactString::from("values"),
                PyObject::native_closure("WeakValueDictionary.values", move |args| {
                    if !args.is_empty() {
                        return Err(PyException::type_error("values() takes no arguments"));
                    }
                    Ok(weak_value_iter(&values_storage, WeakValueIterKind::Values))
                }),
            );

            let items_storage = storage.clone();
            attrs.insert(
                CompactString::from("items"),
                PyObject::native_closure("WeakValueDictionary.items", move |args| {
                    if !args.is_empty() {
                        return Err(PyException::type_error("items() takes no arguments"));
                    }
                    Ok(weak_value_iter(&items_storage, WeakValueIterKind::Items))
                }),
            );

            let update_storage = storage.clone();
            attrs.insert(
                CompactString::from("update"),
                PyObject::native_closure("WeakValueDictionary.update", move |args| {
                    weak_value_update_args(&update_storage, args)?;
                    Ok(PyObject::none())
                }),
            );

            let setdefault_storage = storage.clone();
            attrs.insert(
                CompactString::from("setdefault"),
                PyObject::native_closure("WeakValueDictionary.setdefault", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("setdefault() requires a key"));
                    }
                    if let Some(existing) = weak_value_get_alive(&setdefault_storage, &args[0])? {
                        return Ok(existing);
                    }
                    let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
                    weak_value_set(&setdefault_storage, args[0].clone(), default.clone())?;
                    Ok(default)
                }),
            );

            let pop_storage = storage.clone();
            attrs.insert(
                CompactString::from("pop"),
                PyObject::native_closure("WeakValueDictionary.pop", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("pop() requires a key"));
                    }
                    if args.len() > 2 {
                        return Err(PyException::type_error("pop expected at most 2 arguments"));
                    }
                    let key = args[0].to_hashable_key()?;
                    let mut store = pop_storage.write();
                    let state = store.get(&key).and_then(|(_, r)| weak_ref_target(r));
                    match state {
                        Some(value) => {
                            store.shift_remove(&key);
                            Ok(value)
                        }
                        None if store.contains_key(&key) => {
                            store.shift_remove(&key);
                            args.get(1)
                                .cloned()
                                .ok_or_else(|| py_default_key_error(&args[0]))
                        }
                        None => args
                            .get(1)
                            .cloned()
                            .ok_or_else(|| py_default_key_error(&args[0])),
                    }
                }),
            );

            let popitem_storage = storage.clone();
            attrs.insert(
                CompactString::from("popitem"),
                PyObject::native_closure("WeakValueDictionary.popitem", move |args| {
                    if !args.is_empty() {
                        return Err(PyException::type_error("popitem() takes no arguments"));
                    }
                    let mut store = popitem_storage.write();
                    store.retain(|_, (_, r)| weak_ref_target(r).is_some());
                    let item = store.iter().next().and_then(|(key, (orig, ref_obj))| {
                        weak_ref_target(ref_obj).map(|v| (key.clone(), orig.clone(), v))
                    });
                    if let Some((key, orig, value)) = item {
                        store.shift_remove(&key);
                        Ok(PyObject::tuple(vec![orig, value]))
                    } else {
                        Err(PyException::key_error("dictionary is empty"))
                    }
                }),
            );

            let clear_storage = storage.clone();
            attrs.insert(
                CompactString::from("clear"),
                PyObject::native_closure("WeakValueDictionary.clear", move |_| {
                    clear_storage.write().clear();
                    Ok(PyObject::none())
                }),
            );

            let copy_storage = storage.clone();
            attrs.insert(
                CompactString::from("copy"),
                PyObject::native_closure("WeakValueDictionary.copy", move |_| {
                    let new_storage: WeakValueStorage = Rc::new(PyCell::new(IndexMap::new()));
                    for (key, value) in weak_value_items(&copy_storage) {
                        weak_value_set(&new_storage, key, value)?;
                    }
                    Ok(build_weak_value_dictionary(new_storage))
                }),
            );

            let refs_storage = storage.clone();
            attrs.insert(
                CompactString::from("valuerefs"),
                PyObject::native_closure("WeakValueDictionary.valuerefs", move |_| {
                    let refs = {
                        let mut store = refs_storage.write();
                        store.retain(|_, (_, r)| weak_ref_target(r).is_some());
                        store.values().map(|(_, r)| r.clone()).collect()
                    };
                    Ok(PyObject::list(refs))
                }),
            );

            let iter_refs_storage = storage.clone();
            attrs.insert(
                CompactString::from("itervaluerefs"),
                PyObject::native_closure("WeakValueDictionary.itervaluerefs", move |_| {
                    let refs = {
                        let mut store = iter_refs_storage.write();
                        store.retain(|_, (_, r)| weak_ref_target(r).is_some());
                        store.values().map(|(_, r)| r.clone()).collect()
                    };
                    Ok(weak_iter(refs))
                }),
            );
        }
        inst
    }

    fn make_weak_value_dictionary(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let storage: WeakValueStorage = Rc::new(PyCell::new(IndexMap::new()));
        let inst = build_weak_value_dictionary(storage.clone());
        weak_value_update_args(&storage, args)?;
        Ok(inst)
    }

    fn build_weak_key_dictionary(storage: WeakKeyStorage) -> PyObjectRef {
        let mut class_ns = IndexMap::new();
        let eq_storage = storage.clone();
        class_ns.insert(
            CompactString::from("__eq__"),
            PyObject::native_closure("WeakKeyDictionary.__eq__", move |args| {
                if args.len() < 2 {
                    return Err(PyException::type_error("__eq__ requires an argument"));
                }
                let items = weak_key_items(&eq_storage);
                Ok(PyObject::bool_val(weak_mapping_eq(&items, &args[1])?))
            }),
        );
        let ne_storage = storage.clone();
        class_ns.insert(
            CompactString::from("__ne__"),
            PyObject::native_closure("WeakKeyDictionary.__ne__", move |args| {
                if args.len() < 2 {
                    return Err(PyException::type_error("__ne__ requires an argument"));
                }
                let items = weak_key_items(&ne_storage);
                Ok(PyObject::bool_val(!weak_mapping_eq(&items, &args[1])?))
            }),
        );
        class_ns.insert(
            CompactString::from("__repr__"),
            PyObject::native_closure("WeakKeyDictionary.__repr__", move |args| {
                let ptr = args
                    .first()
                    .map(|obj| PyObjectRef::as_ptr(obj) as usize)
                    .unwrap_or(0);
                Ok(PyObject::str_val(CompactString::from(format!(
                    "<WeakKeyDictionary at 0x{:x}>",
                    ptr
                ))))
            }),
        );
        let cls = PyObject::class(CompactString::from("WeakKeyDictionary"), vec![], class_ns);
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(
                CompactString::from("__weakkeydict__"),
                PyObject::bool_val(true),
            );
            let internal_items_storage = storage.clone();
            attrs.insert(
                CompactString::from("__weakkey_items__"),
                PyObject::native_closure("WeakKeyDictionary.__weakkey_items__", move |_| {
                    let items = weak_key_items(&internal_items_storage)
                        .into_iter()
                        .map(|(key, value)| PyObject::tuple(vec![key, value]))
                        .collect();
                    Ok(PyObject::list(items))
                }),
            );

            let set_storage = storage.clone();
            attrs.insert(
                CompactString::from("__setitem__"),
                PyObject::native_closure("WeakKeyDictionary.__setitem__", move |args| {
                    if args.len() < 2 {
                        return Err(PyException::type_error(
                            "__setitem__ requires key and value",
                        ));
                    }
                    weak_key_set(&set_storage, args[0].clone(), args[1].clone())?;
                    Ok(PyObject::none())
                }),
            );

            let get_storage = storage.clone();
            attrs.insert(
                CompactString::from("__getitem__"),
                PyObject::native_closure("WeakKeyDictionary.__getitem__", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("__getitem__ requires a key"));
                    }
                    match weak_key_get_alive(&get_storage, &args[0], true)? {
                        Some(val) => Ok(val),
                        None => Err(py_default_key_error(&args[0])),
                    }
                }),
            );

            let len_storage = storage.clone();
            attrs.insert(
                CompactString::from("__len__"),
                PyObject::native_closure("WeakKeyDictionary.__len__", move |_| {
                    let mut store = len_storage.write();
                    store.retain(|_, (r, _)| weak_ref_target(r).is_some());
                    Ok(PyObject::int(store.len() as i64))
                }),
            );

            let bool_storage = storage.clone();
            attrs.insert(
                CompactString::from("__bool__"),
                PyObject::native_closure("WeakKeyDictionary.__bool__", move |_| {
                    let mut store = bool_storage.write();
                    store.retain(|_, (r, _)| weak_ref_target(r).is_some());
                    Ok(PyObject::bool_val(!store.is_empty()))
                }),
            );

            let contains_storage = storage.clone();
            attrs.insert(
                CompactString::from("__contains__"),
                PyObject::native_closure("WeakKeyDictionary.__contains__", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("__contains__ requires a key"));
                    }
                    if weak_key_require_weakable(&args[0]).is_err() {
                        return Ok(PyObject::bool_val(false));
                    }
                    Ok(PyObject::bool_val(
                        weak_key_get_alive(&contains_storage, &args[0], false)?.is_some(),
                    ))
                }),
            );

            let del_storage = storage.clone();
            attrs.insert(
                CompactString::from("__delitem__"),
                PyObject::native_closure("WeakKeyDictionary.__delitem__", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("__delitem__ requires a key"));
                    }
                    weak_key_require_weakable(&args[0])?;
                    let mut store = del_storage.write();
                    store.retain(|_, (ref_obj, _)| weak_ref_target(ref_obj).is_some());
                    let Some(ptr) = weak_key_lookup_ptr(&store, &args[0])? else {
                        return Err(py_default_key_error(&args[0]));
                    };
                    match store.shift_remove(&ptr) {
                        Some((ref_obj, _)) if weak_ref_target(&ref_obj).is_some() => {
                            Ok(PyObject::none())
                        }
                        Some(_) => Err(py_default_key_error(&args[0])),
                        None => Err(py_default_key_error(&args[0])),
                    }
                }),
            );

            let get_method_storage = storage.clone();
            attrs.insert(
                CompactString::from("get"),
                PyObject::native_closure("WeakKeyDictionary.get", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("get() requires a key"));
                    }
                    if args.len() > 2 {
                        return Err(PyException::type_error("get expected at most 2 arguments"));
                    }
                    let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
                    Ok(weak_key_get_alive(&get_method_storage, &args[0], true)?.unwrap_or(default))
                }),
            );

            let keys_storage = storage.clone();
            attrs.insert(
                CompactString::from("keys"),
                PyObject::native_closure("WeakKeyDictionary.keys", move |args| {
                    if !args.is_empty() {
                        return Err(PyException::type_error("keys() takes no arguments"));
                    }
                    Ok(weak_key_iter(&keys_storage, WeakKeyIterKind::Keys))
                }),
            );

            let iter_storage = storage.clone();
            attrs.insert(
                CompactString::from("__iter__"),
                PyObject::native_closure("WeakKeyDictionary.__iter__", move |_| {
                    Ok(weak_key_iter(&iter_storage, WeakKeyIterKind::Keys))
                }),
            );

            let values_storage = storage.clone();
            attrs.insert(
                CompactString::from("values"),
                PyObject::native_closure("WeakKeyDictionary.values", move |args| {
                    if !args.is_empty() {
                        return Err(PyException::type_error("values() takes no arguments"));
                    }
                    let vals = weak_key_items(&values_storage)
                        .into_iter()
                        .map(|(_, v)| v)
                        .collect();
                    Ok(weak_iter(vals))
                }),
            );

            let items_storage = storage.clone();
            attrs.insert(
                CompactString::from("items"),
                PyObject::native_closure("WeakKeyDictionary.items", move |args| {
                    if !args.is_empty() {
                        return Err(PyException::type_error("items() takes no arguments"));
                    }
                    Ok(weak_key_iter(&items_storage, WeakKeyIterKind::Items))
                }),
            );

            let update_storage = storage.clone();
            attrs.insert(
                CompactString::from("update"),
                PyObject::native_closure("WeakKeyDictionary.update", move |args| {
                    weak_key_update_args(&update_storage, args)?;
                    Ok(PyObject::none())
                }),
            );

            let setdefault_storage = storage.clone();
            attrs.insert(
                CompactString::from("setdefault"),
                PyObject::native_closure("WeakKeyDictionary.setdefault", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("setdefault() requires a key"));
                    }
                    if let Some(existing) = weak_key_get_alive(&setdefault_storage, &args[0], true)?
                    {
                        return Ok(existing);
                    }
                    let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
                    weak_key_set(&setdefault_storage, args[0].clone(), default.clone())?;
                    Ok(default)
                }),
            );

            let pop_storage = storage.clone();
            attrs.insert(
                CompactString::from("pop"),
                PyObject::native_closure("WeakKeyDictionary.pop", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("pop() requires a key"));
                    }
                    if args.len() > 2 {
                        return Err(PyException::type_error("pop expected at most 2 arguments"));
                    }
                    weak_key_require_weakable(&args[0])?;
                    let mut store = pop_storage.write();
                    store.retain(|_, (ref_obj, _)| weak_ref_target(ref_obj).is_some());
                    let Some(ptr) = weak_key_lookup_ptr(&store, &args[0])? else {
                        return args
                            .get(1)
                            .cloned()
                            .ok_or_else(|| py_default_key_error(&args[0]));
                    };
                    match store.get(&ptr) {
                        Some((ref_obj, val)) if weak_ref_target(ref_obj).is_some() => {
                            let value = val.clone();
                            store.shift_remove(&ptr);
                            Ok(value)
                        }
                        Some(_) => {
                            store.shift_remove(&ptr);
                            args.get(1)
                                .cloned()
                                .ok_or_else(|| py_default_key_error(&args[0]))
                        }
                        None => args
                            .get(1)
                            .cloned()
                            .ok_or_else(|| py_default_key_error(&args[0])),
                    }
                }),
            );

            let popitem_storage = storage.clone();
            attrs.insert(
                CompactString::from("popitem"),
                PyObject::native_closure("WeakKeyDictionary.popitem", move |args| {
                    if !args.is_empty() {
                        return Err(PyException::type_error("popitem() takes no arguments"));
                    }
                    let mut store = popitem_storage.write();
                    store.retain(|_, (r, _)| weak_ref_target(r).is_some());
                    let item = store.iter().next().and_then(|(ptr, (ref_obj, val))| {
                        weak_ref_target(ref_obj).map(|k| (*ptr, k, val.clone()))
                    });
                    if let Some((ptr, key, value)) = item {
                        store.shift_remove(&ptr);
                        Ok(PyObject::tuple(vec![key, value]))
                    } else {
                        Err(PyException::key_error("dictionary is empty"))
                    }
                }),
            );

            let clear_storage = storage.clone();
            attrs.insert(
                CompactString::from("clear"),
                PyObject::native_closure("WeakKeyDictionary.clear", move |_| {
                    clear_storage.write().clear();
                    Ok(PyObject::none())
                }),
            );

            let copy_storage = storage.clone();
            attrs.insert(
                CompactString::from("copy"),
                PyObject::native_closure("WeakKeyDictionary.copy", move |_| {
                    let new_storage: WeakKeyStorage = Rc::new(PyCell::new(IndexMap::new()));
                    for (key, value) in weak_key_items(&copy_storage) {
                        weak_key_set(&new_storage, key, value)?;
                    }
                    Ok(build_weak_key_dictionary(new_storage))
                }),
            );

            let refs_storage = storage.clone();
            attrs.insert(
                CompactString::from("keyrefs"),
                PyObject::native_closure("WeakKeyDictionary.keyrefs", move |_| {
                    let refs = {
                        let mut store = refs_storage.write();
                        store.retain(|_, (r, _)| weak_ref_target(r).is_some());
                        store.values().map(|(r, _)| r.clone()).collect()
                    };
                    Ok(PyObject::list(refs))
                }),
            );
        }
        inst
    }

    fn make_weak_key_dictionary(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let storage: WeakKeyStorage = Rc::new(PyCell::new(IndexMap::new()));
        let inst = build_weak_key_dictionary(storage.clone());
        weak_key_update_args(&storage, args)?;
        Ok(inst)
    }

    let mut finalize_namespace = IndexMap::new();
    let finalize_new_ref_type = finalize_reference_type.clone();
    finalize_namespace.insert(
        CompactString::from("__new__"),
        PyObject::native_closure("finalize.__new__", move |args| {
            finalize_new(args, &finalize_new_ref_type)
        }),
    );
    finalize_namespace.insert(
        CompactString::from("__init__"),
        PyObject::native_function("finalize.__init__", |_| Ok(PyObject::none())),
    );
    let finalize_type =
        PyObject::class(CompactString::from("finalize"), vec![], finalize_namespace);

    make_module(
        "weakref",
        vec![
            // ── ref(obj, callback=None) ──
            // Returns a callable weak reference. Calling it returns the referent or None.
            ("ref", ref_constructor_type.clone()),
            // ── proxy(obj, callback=None) ──
            // Returns a proxy that auto-dereferences on attribute access.
            (
                "proxy",
                PyObject::native_closure("weakref.proxy", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "proxy() requires at least 1 argument",
                        ));
                    }
                    let callback = args.get(1).cloned().unwrap_or_else(PyObject::none);
                    let callback = if matches!(callback.payload, PyObjectPayload::None) {
                        None
                    } else {
                        Some(callback)
                    };
                    if callback.is_none() {
                        if let Some(existing) =
                            PyObjectRef::find_shared_weak_object(&args[0], WeakObjectKind::Proxy)
                        {
                            return Ok(existing);
                        }
                    }
                    let weak: PyWeakRef = PyObjectRef::downgrade(&args[0]);

                    let callable = args[0].is_callable();
                    let cls = if callable {
                        callable_proxy_constructor_type.clone()
                    } else {
                        proxy_constructor_type.clone()
                    };
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                        let mut attrs = inst_data.attrs.write();

                        // VM-accessible target accessor for transparent delegation
                        let w_target = weak.clone();
                        attrs.insert(
                            CompactString::from("__weakref_target__"),
                            PyObject::native_closure("__weakref_target__", move |_| {
                                upgrade_or_err(&w_target)
                            }),
                        );

                        // __getattr__(name) → forward to referent
                        let w_ga = weak.clone();
                        attrs.insert(
                            CompactString::from("__getattr__"),
                            PyObject::native_closure("weakproxy.__getattr__", move |args| {
                                let referent = upgrade_or_err(&w_ga)?;
                                if let Some(name_obj) = args.first() {
                                    let name = name_obj.py_to_string();
                                    referent.get_attr(&name).ok_or_else(|| {
                                        PyException::attribute_error(format!(
                                            "'weakproxy' object has no attribute '{}'",
                                            name
                                        ))
                                    })
                                } else {
                                    Err(PyException::type_error(
                                        "__getattr__ requires a name argument",
                                    ))
                                }
                            }),
                        );

                        // __repr__
                        let w_r = weak.clone();
                        attrs.insert(
                            CompactString::from("__repr__"),
                            PyObject::native_closure("weakproxy.__repr__", move |_| {
                                match w_r.upgrade() {
                                    Some(obj) => Ok(PyObject::str_val(CompactString::from(
                                        format!("<weakproxy at {:p}>", PyObjectRef::as_ptr(&obj)),
                                    ))),
                                    None => Err(PyException::new(
                                        ferrython_core::error::ExceptionKind::ReferenceError,
                                        "weakly-referenced object no longer exists",
                                    )),
                                }
                            }),
                        );

                        // __bool__
                        let w_b = weak.clone();
                        attrs.insert(
                            CompactString::from("__bool__"),
                            PyObject::native_closure("weakproxy.__bool__", move |_| {
                                let referent = upgrade_or_err(&w_b)?;
                                Ok(PyObject::bool_val(referent.is_truthy()))
                            }),
                        );

                        // __str__
                        let w_s = weak.clone();
                        attrs.insert(
                            CompactString::from("__str__"),
                            PyObject::native_closure("weakproxy.__str__", move |_| {
                                let referent = upgrade_or_err(&w_s)?;
                                Ok(PyObject::str_val(CompactString::from(
                                    referent.py_to_string(),
                                )))
                            }),
                        );

                        // __call__ — forward calls to callable referents
                        let w_c = weak.clone();
                        attrs.insert(
                            CompactString::from("__call__"),
                            PyObject::native_closure("weakproxy.__call__", move |args| {
                                let referent = upgrade_or_err(&w_c)?;
                                if !referent.is_callable() {
                                    return Err(PyException::type_error(
                                        "weakproxy object is not directly callable; access attributes instead",
                                    ));
                                }
                                let mut call_args = args.to_vec();
                                let kwargs = match call_args.last() {
                                    Some(last) => match &last.payload {
                                        PyObjectPayload::Dict(map) => {
                                            let mut kwargs = Vec::new();
                                            for (key, value) in map.read().iter() {
                                                if let HashableKey::Str(name) = key {
                                                    kwargs.push((
                                                        name.to_compact_string(),
                                                        value.clone(),
                                                    ));
                                                } else {
                                                    return Err(PyException::type_error(
                                                        "keywords must be strings",
                                                    ));
                                                }
                                            }
                                            call_args.pop();
                                            kwargs
                                        }
                                        _ => Vec::new(),
                                    },
                                    None => Vec::new(),
                                };
                                if kwargs.is_empty() {
                                    call_callable(&referent, &call_args)
                                } else {
                                    call_callable_kw(&referent, &call_args, kwargs)
                                }
                            }),
                        );
                    }
                    PyObjectRef::register_weak_object(
                        &args[0],
                        &inst,
                        callback,
                        WeakObjectKind::Proxy,
                    );
                    Ok(inst)
                }),
            ),
            // ── WeakValueDictionary() ──
            // Dict where values are weak references; dead entries are auto-pruned.
            (
                "WeakValueDictionary",
                PyObject::native_function("WeakValueDictionary", make_weak_value_dictionary),
            ),
            // ── WeakKeyDictionary() ──
            // Dict where keys are weak references; dead entries are auto-pruned.
            (
                "WeakKeyDictionary",
                PyObject::native_function("WeakKeyDictionary", make_weak_key_dictionary),
            ),
            // ── WeakSet() ──
            // A set of weak references. Dead entries are auto-pruned.
            (
                "WeakSet",
                make_builtin(|_| {
                    let storage: Rc<PyCell<IndexMap<usize, PyWeakRef>>> =
                        Rc::new(PyCell::new(IndexMap::new()));

                    let cls =
                        PyObject::class(CompactString::from("WeakSet"), vec![], IndexMap::new());
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                        let mut attrs = inst_data.attrs.write();

                        // add(obj)
                        let s1 = storage.clone();
                        attrs.insert(
                            CompactString::from("add"),
                            PyObject::native_closure("WeakSet.add", move |args| {
                                if args.is_empty() {
                                    return Err(PyException::type_error(
                                        "add() requires an argument",
                                    ));
                                }
                                let ptr = PyObjectRef::as_ptr(&args[0]) as usize;
                                let weak = PyObjectRef::downgrade(&args[0]);
                                s1.write().insert(ptr, weak);
                                Ok(PyObject::none())
                            }),
                        );

                        // discard(obj)
                        let s2 = storage.clone();
                        attrs.insert(
                            CompactString::from("discard"),
                            PyObject::native_closure("WeakSet.discard", move |args| {
                                if args.is_empty() {
                                    return Err(PyException::type_error(
                                        "discard() requires an argument",
                                    ));
                                }
                                let ptr = PyObjectRef::as_ptr(&args[0]) as usize;
                                s2.write().shift_remove(&ptr);
                                Ok(PyObject::none())
                            }),
                        );

                        // __contains__(obj)
                        let s3 = storage.clone();
                        attrs.insert(
                            CompactString::from("__contains__"),
                            PyObject::native_closure("WeakSet.__contains__", move |args| {
                                if args.is_empty() {
                                    return Err(PyException::type_error(
                                        "__contains__ requires an argument",
                                    ));
                                }
                                let ptr = PyObjectRef::as_ptr(&args[0]) as usize;
                                let mut store = s3.write();
                                match store.get(&ptr) {
                                    Some(weak) => {
                                        if weak.upgrade().is_some() {
                                            Ok(PyObject::bool_val(true))
                                        } else {
                                            store.shift_remove(&ptr);
                                            Ok(PyObject::bool_val(false))
                                        }
                                    }
                                    None => Ok(PyObject::bool_val(false)),
                                }
                            }),
                        );

                        // __len__()
                        let s4 = storage.clone();
                        attrs.insert(
                            CompactString::from("__len__"),
                            PyObject::native_closure("WeakSet.__len__", move |_| {
                                let mut store = s4.write();
                                store.retain(|_, w| w.upgrade().is_some());
                                Ok(PyObject::int(store.len() as i64))
                            }),
                        );

                        // __iter__() — return a list of live items
                        let s5 = storage.clone();
                        attrs.insert(
                            CompactString::from("__iter__"),
                            PyObject::native_closure("WeakSet.__iter__", move |_| {
                                let mut store = s5.write();
                                store.retain(|_, w| w.upgrade().is_some());
                                let items: Vec<PyObjectRef> =
                                    store.values().filter_map(|w| w.upgrade()).collect();
                                Ok(PyObject::list(items))
                            }),
                        );
                    }
                    Ok(inst)
                }),
            ),
            // ── finalize(obj, func, *args, **kwargs) ──
            ("finalize", finalize_type),
            // ── getweakrefcount(obj) ──
            (
                "getweakrefcount",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "getweakrefcount requires 1 argument",
                        ));
                    }
                    Ok(PyObject::int(PyObjectRef::weak_count(&args[0]) as i64))
                }),
            ),
            // ── getweakrefs(obj) ──
            (
                "getweakrefs",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("getweakrefs requires 1 argument"));
                    }
                    let mut refs = PyObjectRef::weak_objects(&args[0]);
                    refs.sort_by_key(|obj| {
                        if let PyObjectPayload::Instance(inst) = &obj.payload {
                            if let PyObjectPayload::Class(cd) = &inst.class.payload {
                                return if cd.name.as_str() == "weakref" { 0 } else { 1 };
                            }
                        }
                        1
                    });
                    Ok(PyObject::list(refs))
                }),
            ),
            // ── ReferenceType (the type of weak references) ──
            ("ReferenceType", reference_type.clone()),
            // ── ProxyType ──
            ("ProxyType", proxy_type),
            // ── CallableProxyType ──
            ("CallableProxyType", callable_proxy_type),
            // ── WeakMethod(method, callback=None) ──
            (
                "WeakMethod",
                PyObject::native_closure("weakref.WeakMethod", move |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "WeakMethod requires at least 1 argument",
                        ));
                    }
                    if args.len() > 2 {
                        return Err(PyException::type_error(
                            "WeakMethod expected at most 2 arguments",
                        ));
                    }
                    let method = args[0].clone();
                    let (receiver, func) = match &method.payload {
                        PyObjectPayload::BoundMethod { receiver, method } => {
                            (receiver.clone(), method.clone())
                        }
                        PyObjectPayload::BuiltinBoundMethod(bbm) => (
                            bbm.receiver.clone(),
                            PyObject::str_val(bbm.method_name.clone()),
                        ),
                        _ => {
                            let receiver = method.get_attr("__self__").ok_or_else(|| {
                                PyException::type_error(
                                    "argument should be a bound method, not other callable",
                                )
                            })?;
                            let func = method.get_attr("__func__").ok_or_else(|| {
                                PyException::type_error(
                                    "argument should be a bound method, not other callable",
                                )
                            })?;
                            (receiver, func)
                        }
                    };
                    let callback = args.get(1).cloned().unwrap_or_else(PyObject::none);
                    let callback = if matches!(callback.payload, PyObjectPayload::None) {
                        None
                    } else {
                        Some(callback)
                    };
                    let weak_receiver = PyObjectRef::downgrade(&receiver);
                    let weak_func = PyObjectRef::downgrade(&func);
                    let cls = PyObject::class(
                        CompactString::from("WeakMethod"),
                        vec![reference_type.clone()],
                        IndexMap::new(),
                    );
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref d) = inst.payload {
                        let mut w = d.attrs.write();
                        w.insert(
                            CompactString::from("__weakref_ref__"),
                            PyObject::bool_val(true),
                        );
                        w.insert(
                            CompactString::from("__weakmethod__"),
                            PyObject::bool_val(true),
                        );
                        w.insert(
                            CompactString::from("__weakref_callback__"),
                            callback.clone().unwrap_or_else(PyObject::none),
                        );
                        let call_receiver = weak_receiver.clone();
                        let call_func = weak_func.clone();
                        w.insert(
                            CompactString::from("__call__"),
                            PyObject::native_closure("WeakMethod.__call__", move |_| {
                                let Some(receiver) = call_receiver.upgrade() else {
                                    return Ok(PyObject::none());
                                };
                                let Some(func) = call_func.upgrade() else {
                                    return Ok(PyObject::none());
                                };
                                Ok(PyObject::wrap(PyObjectPayload::BoundMethod {
                                    receiver,
                                    method: func,
                                }))
                            }),
                        );
                        let bool_receiver = weak_receiver.clone();
                        let bool_func = weak_func.clone();
                        w.insert(
                            CompactString::from("__bool__"),
                            PyObject::native_closure("WeakMethod.__bool__", move |_| {
                                Ok(PyObject::bool_val(
                                    bool_receiver.upgrade().is_some()
                                        && bool_func.upgrade().is_some(),
                                ))
                            }),
                        );
                    }
                    if let Some(callback) = callback {
                        let fired = Rc::new(Cell::new(false));
                        let cb1 = callback.clone();
                        let weak_method = inst.clone();
                        let fired1 = fired.clone();
                        let callback_wrapper =
                            PyObject::native_closure("WeakMethod.callback", move |_| {
                                if !fired1.replace(true) {
                                    call_callable(&cb1, &[weak_method.clone()])?;
                                }
                                Ok(PyObject::none())
                            });
                        PyObjectRef::register_weak_object(
                            &receiver,
                            &inst,
                            Some(callback_wrapper.clone()),
                            WeakObjectKind::Ref,
                        );
                        let cb2 = callback;
                        let weak_method = inst.clone();
                        let fired2 = fired;
                        let callback_wrapper =
                            PyObject::native_closure("WeakMethod.callback", move |_| {
                                if !fired2.replace(true) {
                                    call_callable(&cb2, &[weak_method.clone()])?;
                                }
                                Ok(PyObject::none())
                            });
                        PyObjectRef::register_weak_object(
                            &func,
                            &inst,
                            Some(callback_wrapper),
                            WeakObjectKind::Ref,
                        );
                    }
                    Ok(inst)
                }),
            ),
        ],
    )
}
mod gc;
mod multiprocessing;
mod select;
mod selectors;
mod signal;
mod thread_module;

pub use gc::create_gc_module;
pub use multiprocessing::create_multiprocessing_module;
pub use select::create_select_module;
pub use selectors::create_selectors_module;
pub use signal::create_signal_module;
pub use thread_module::create_thread_module;
