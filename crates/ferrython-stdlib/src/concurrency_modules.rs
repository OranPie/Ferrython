//! Concurrency stdlib modules (threading, weakref, gc, _thread)

use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    FxHashKeyMap, new_fx_hashkey_map,PyCell,
    PyObject, PyObjectPayload, PyObjectRef, PyObjectMethods, PyWeakRef,
    make_module, make_builtin, check_args_min,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::cell::RefCell;
use std::sync::Arc;
use std::rc::Rc;

/// SAFETY: GIL semantics — only one thread runs Python at a time.
/// This wrapper lets us move Rc-based values into thread::spawn closures.
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

// ── logging module ──


pub fn create_threading_module() -> PyObjectRef {
    // Build Thread as a proper Class so subclasses inherit methods via MRO.
    let mut thread_ns = IndexMap::new();

    // __init__(self, *, target=None, args=(), daemon=False, name="Thread")
    thread_ns.insert(CompactString::from("__init__"), make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::none()); }
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
                    if let Some(t) = r.get(&HashableKey::str_key(CompactString::from("target"))) { target = t.clone(); }
                    if let Some(a) = r.get(&HashableKey::str_key(CompactString::from("args"))) { thread_args = a.clone(); }
                    if let Some(d) = r.get(&HashableKey::str_key(CompactString::from("daemon"))) { daemon = d.clone(); }
                    if let Some(n) = r.get(&HashableKey::str_key(CompactString::from("name"))) { name = n.clone(); }
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
    }));

    // start(self)
    thread_ns.insert(CompactString::from("start"), make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::none()); }
        let self_obj = &args[0];
        if let PyObjectPayload::Instance(ref inst) = self_obj.payload {
            inst.attrs.write().insert(CompactString::from("_started"), PyObject::bool_val(true));
            inst.attrs.write().insert(CompactString::from("_alive"), PyObject::bool_val(true));
            let target = inst.attrs.read().get("_target").cloned().unwrap_or_else(PyObject::none);
            let thread_args = inst.attrs.read().get("_args").cloned().unwrap_or_else(|| PyObject::tuple(vec![]));
            if !matches!(&target.payload, PyObjectPayload::None) {
                let call_args: Vec<PyObjectRef> = match &thread_args.payload {
                    PyObjectPayload::Tuple(items) => items.clone(),
                    PyObjectPayload::List(items) => items.read().clone(),
                    _ => vec![],
                };
                // For native functions, spawn a real OS thread for true parallelism
                match &target.payload {
                    PyObjectPayload::NativeFunction(nf) => {
                        let f = nf.func;
                        let alive_attrs = inst.attrs.clone();
                        let call_args_owned = call_args;
                        let join_handle = std::sync::Arc::new(std::sync::Mutex::new(None::<std::thread::JoinHandle<()>>));
                        let jh = join_handle.clone();
                        // SAFETY: GIL semantics — the spawned thread won't race
                        // with the main interpreter thread.
                        let closure: Box<dyn FnOnce()> = Box::new(move || {
                            let _ = (nf.func)(&call_args_owned);
                            alive_attrs.write().insert(CompactString::from("_alive"), PyObject::bool_val(false));
                        });
                        let send_closure: Box<dyn FnOnce() + Send> = unsafe {
                            std::mem::transmute(closure)
                        };
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
                        let join_handle = std::sync::Arc::new(std::sync::Mutex::new(None::<std::thread::JoinHandle<()>>));
                        let jh = join_handle.clone();
                        let closure: Box<dyn FnOnce()> = Box::new(move || {
                            let _ = (nc.func)(&call_args);
                            alive_attrs.write().insert(CompactString::from("_alive"), PyObject::bool_val(false));
                        });
                        let send_closure: Box<dyn FnOnce() + Send> = unsafe {
                            std::mem::transmute(closure)
                        };
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
                        if let Some(handle) = ferrython_core::error::spawn_python_thread(target.clone(), call_args.clone()) {
                            let join_handle = std::sync::Arc::new(std::sync::Mutex::new(Some(handle)));
                            let jh = join_handle.clone();
                            let alive_flag = alive_attrs.clone();
                            // Monitor thread completion in a background helper
                            let closure: Box<dyn FnOnce()> = Box::new(move || {
                                if let Some(h) = jh.lock().unwrap().take() {
                                    let _ = h.join();
                                }
                                alive_flag.write().insert(CompactString::from("_alive"), PyObject::bool_val(false));
                            });
                            let send_closure: Box<dyn FnOnce() + Send> = unsafe {
                                std::mem::transmute(closure)
                            };
                            std::thread::spawn(move || { send_closure(); });
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
                        let is_daemon = inst.attrs.read().get("daemon")
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
            inst.attrs.write().insert(CompactString::from("_alive"), PyObject::bool_val(false));
        }
        Ok(PyObject::none())
    }));

    // join(self, timeout=None) — wait for thread to complete
    thread_ns.insert(CompactString::from("join"), make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::none()); }
        if let PyObjectPayload::Instance(ref inst) = args[0].payload {
            let attrs = inst.attrs.read();
            let started = attrs.get("_started")
                .map(|v| v.is_truthy()).unwrap_or(false);
            if !started {
                return Err(PyException::runtime_error("cannot join thread before it is started"));
            }
            // If there's a real OS thread join handle, use it
            if let Some(jh) = attrs.get("_join_handle").cloned() {
                drop(attrs);
                // Get timeout if provided
                let timeout_secs = if args.len() > 1 && !matches!(&args[1].payload, PyObjectPayload::None) {
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
                            let alive = inst2.attrs.read().get("_alive")
                                .map(|v| v.is_truthy()).unwrap_or(false);
                            if !alive { break; }
                        }
                        if start.elapsed() >= dur { break; }
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                } else {
                    // Blocking join
                    match &jh.payload {
                        PyObjectPayload::NativeClosure(nc) => { let _ = (nc.func)(&[]); }
                        _ => {}
                    }
                }
            }
        }
        Ok(PyObject::none())
    }));

    // is_alive(self)
    thread_ns.insert(CompactString::from("is_alive"), make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::bool_val(false)); }
        if let PyObjectPayload::Instance(ref inst) = args[0].payload {
            if let Some(alive) = inst.attrs.read().get("_alive").cloned() {
                return Ok(alive);
            }
        }
        Ok(PyObject::bool_val(false))
    }));

    // getName(self)
    thread_ns.insert(CompactString::from("getName"), make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::str_val(CompactString::from("Thread"))); }
        if let PyObjectPayload::Instance(ref inst) = args[0].payload {
            if let Some(name) = inst.attrs.read().get("name").cloned() {
                return Ok(name);
            }
        }
        Ok(PyObject::str_val(CompactString::from("Thread")))
    }));

    // setDaemon(self, val)
    thread_ns.insert(CompactString::from("setDaemon"), make_builtin(|args: &[PyObjectRef]| {
        if args.len() >= 2 {
            if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                inst.attrs.write().insert(CompactString::from("daemon"), args[1].clone());
            }
        }
        Ok(PyObject::none())
    }));

    // run(self) — default implementation calls target
    thread_ns.insert(CompactString::from("run"), make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::none()); }
        if let PyObjectPayload::Instance(ref inst) = args[0].payload {
            let target = inst.attrs.read().get("_target").cloned().unwrap_or_else(PyObject::none);
            if !matches!(&target.payload, PyObjectPayload::None) {
                let thread_args = inst.attrs.read().get("_args").cloned().unwrap_or_else(|| PyObject::tuple(vec![]));
                let call_args: Vec<PyObjectRef> = match &thread_args.payload {
                    PyObjectPayload::Tuple(items) => items.clone(),
                    PyObjectPayload::List(items) => items.read().clone(),
                    _ => vec![],
                };
                push_deferred_call(target, call_args);
            }
        }
        Ok(PyObject::none())
    }));

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
            attrs.insert(CompactString::from("acquire"), PyObject::native_closure(
                "acquire", move |args: &[PyObjectRef]| {
                    let mut blocking = true;
                    for a in args {
                        match &a.payload {
                            PyObjectPayload::Bool(b) => { blocking = *b; }
                            PyObjectPayload::Dict(map) => {
                                let r = map.read();
                                if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("blocking"))) {
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
                }));
            let m2 = mutex.clone();
            let lf2 = locked_flag.clone();
            attrs.insert(CompactString::from("release"), PyObject::native_closure(
                "release", move |_: &[PyObjectRef]| {
                    if lf2.swap(false, std::sync::atomic::Ordering::AcqRel) {
                        // Safety: we know the mutex was locked by acquire()
                        unsafe { m2.force_unlock(); }
                    }
                    Ok(PyObject::none())
                }));
            let lf3 = locked_flag.clone();
            attrs.insert(CompactString::from("locked"), PyObject::native_closure(
                "locked", move |_: &[PyObjectRef]| { Ok(PyObject::bool_val(lf3.load(std::sync::atomic::Ordering::Acquire))) }));
            let m4 = mutex.clone();
            let lf4 = locked_flag.clone();
            attrs.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "__enter__", move |_: &[PyObjectRef]| {
                    let guard = m4.lock();
                    lf4.store(true, std::sync::atomic::Ordering::Release);
                    std::mem::forget(guard);
                    Ok(inst_ref.clone())
                }));
            let m5 = mutex.clone();
            let lf5 = locked_flag.clone();
            attrs.insert(CompactString::from("__exit__"), PyObject::native_closure(
                "__exit__", move |_: &[PyObjectRef]| {
                    if lf5.swap(false, std::sync::atomic::Ordering::AcqRel) {
                        unsafe { m5.force_unlock(); }
                    }
                    Ok(PyObject::bool_val(false))
                }));
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
            attrs.insert(CompactString::from("acquire"), PyObject::native_closure(
                "acquire", move |args: &[PyObjectRef]| {
                    let mut blocking = true;
                    for a in args {
                        match &a.payload {
                            PyObjectPayload::Bool(b) => { blocking = *b; }
                            PyObjectPayload::Dict(map) => {
                                let r = map.read();
                                if let Some(v) = r.get(&HashableKey::str_key(CompactString::from("blocking"))) {
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
                }));
            let s2 = state.clone();
            attrs.insert(CompactString::from("release"), PyObject::native_closure(
                "release", move |_: &[PyObjectRef]| {
                    let mut s = s2.write();
                    if s.1 > 0 { s.1 -= 1; }
                    if s.1 == 0 { s.0 = false; }
                    Ok(PyObject::none())
                }));
            let s3 = state.clone();
            attrs.insert(CompactString::from("locked"), PyObject::native_closure(
                "locked", move |_: &[PyObjectRef]| { Ok(PyObject::bool_val(s3.read().0)) }));
            let s4 = state.clone();
            let ir = inst_ref.clone();
            attrs.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "__enter__", move |_: &[PyObjectRef]| {
                    let mut s = s4.write();
                    s.0 = true;
                    s.1 += 1;
                    Ok(ir.clone())
                }));
            let s5 = state.clone();
            attrs.insert(CompactString::from("__exit__"), PyObject::native_closure(
                "__exit__", move |_: &[PyObjectRef]| {
                    let mut s = s5.write();
                    if s.1 > 0 { s.1 -= 1; }
                    if s.1 == 0 { s.0 = false; }
                    Ok(PyObject::bool_val(false))
                }));
        }
        Ok(inst)
    });

    // Semaphore — counting semaphore
    let sem_cls = PyObject::class(CompactString::from("Semaphore"), vec![], IndexMap::new());
    let sc = sem_cls.clone();
    let semaphore_fn = PyObject::native_closure("Semaphore", move |args: &[PyObjectRef]| {
        let initial = if !args.is_empty() {
            args[0].as_int().unwrap_or(1)
        } else { 1 };
        let inst = PyObject::instance(sc.clone());
        let counter = Rc::new(PyCell::new(initial));
        let inst_ref = inst.clone();
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let c1 = counter.clone();
            attrs.insert(CompactString::from("acquire"), PyObject::native_closure(
                "acquire", move |_: &[PyObjectRef]| {
                    let mut c = c1.write();
                    if *c > 0 { *c -= 1; Ok(PyObject::bool_val(true)) }
                    else { Ok(PyObject::bool_val(false)) }
                }));
            let c2 = counter.clone();
            attrs.insert(CompactString::from("release"), PyObject::native_closure(
                "release", move |_: &[PyObjectRef]| {
                    *c2.write() += 1;
                    Ok(PyObject::none())
                }));
            let c3 = counter.clone();
            attrs.insert(CompactString::from("_value"), PyObject::native_closure(
                "_value", move |_: &[PyObjectRef]| { Ok(PyObject::int(*c3.read())) }));
            let c4 = counter.clone();
            let ir = inst_ref.clone();
            attrs.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "__enter__", move |_: &[PyObjectRef]| {
                    let mut c = c4.write();
                    if *c > 0 { *c -= 1; }
                    Ok(ir.clone())
                }));
            let c5 = counter.clone();
            attrs.insert(CompactString::from("__exit__"), PyObject::native_closure(
                "__exit__", move |_: &[PyObjectRef]| {
                    *c5.write() += 1;
                    Ok(PyObject::bool_val(false))
                }));
        }
        Ok(inst)
    });

    // BoundedSemaphore — same as Semaphore with upper bound check
    let bsem_cls = PyObject::class(CompactString::from("BoundedSemaphore"), vec![], IndexMap::new());
    let bsc = bsem_cls.clone();
    let bounded_semaphore_fn = PyObject::native_closure("BoundedSemaphore", move |args: &[PyObjectRef]| {
        let initial = if !args.is_empty() {
            args[0].as_int().unwrap_or(1)
        } else { 1 };
        let inst = PyObject::instance(bsc.clone());
        let counter = Rc::new(PyCell::new(initial));
        let bound = initial;
        let inst_ref = inst.clone();
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let c1 = counter.clone();
            attrs.insert(CompactString::from("acquire"), PyObject::native_closure(
                "acquire", move |_: &[PyObjectRef]| {
                    let mut c = c1.write();
                    if *c > 0 { *c -= 1; Ok(PyObject::bool_val(true)) }
                    else { Ok(PyObject::bool_val(false)) }
                }));
            let c2 = counter.clone();
            attrs.insert(CompactString::from("release"), PyObject::native_closure(
                "release", move |_: &[PyObjectRef]| {
                    let mut c = c2.write();
                    if *c >= bound {
                        return Err(PyException::value_error("Semaphore released too many times"));
                    }
                    *c += 1;
                    Ok(PyObject::none())
                }));
            let c3 = counter.clone();
            attrs.insert(CompactString::from("_value"), PyObject::native_closure(
                "_value", move |_: &[PyObjectRef]| { Ok(PyObject::int(*c3.read())) }));
            let c4 = counter.clone();
            let ir = inst_ref.clone();
            attrs.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "__enter__", move |_: &[PyObjectRef]| {
                    let mut c = c4.write();
                    if *c > 0 { *c -= 1; }
                    Ok(ir.clone())
                }));
            let c5 = counter.clone();
            attrs.insert(CompactString::from("__exit__"), PyObject::native_closure(
                "__exit__", move |_: &[PyObjectRef]| {
                    *c5.write() += 1;
                    Ok(PyObject::bool_val(false))
                }));
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
            attrs.insert(CompactString::from("set"), PyObject::native_closure(
                "set", move |_: &[PyObjectRef]| { *f1.write() = true; Ok(PyObject::none()) }));
            let f2 = flag.clone();
            attrs.insert(CompactString::from("clear"), PyObject::native_closure(
                "clear", move |_: &[PyObjectRef]| { *f2.write() = false; Ok(PyObject::none()) }));
            let f3 = flag.clone();
            attrs.insert(CompactString::from("is_set"), PyObject::native_closure(
                "is_set", move |_: &[PyObjectRef]| { Ok(PyObject::bool_val(*f3.read())) }));
            let f4 = flag.clone();
            attrs.insert(CompactString::from("wait"), PyObject::native_closure(
                "wait", move |_: &[PyObjectRef]| { Ok(PyObject::bool_val(*f4.read())) }));
        }
        Ok(inst)
    });

    // Barrier — synchronization barrier
    let barrier_cls = PyObject::class(CompactString::from("Barrier"), vec![], IndexMap::new());
    let bc = barrier_cls.clone();
    let barrier_fn = PyObject::native_closure("Barrier", move |args: &[PyObjectRef]| {
        let parties = if !args.is_empty() {
            args[0].as_int().unwrap_or(1)
        } else { 1 };
        let inst = PyObject::instance(bc.clone());
        let waiting = Rc::new(PyCell::new(0i64));
        let broken = Rc::new(PyCell::new(false));
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(CompactString::from("parties"), PyObject::int(parties));
            let w1 = waiting.clone();
            attrs.insert(CompactString::from("n_waiting"), PyObject::native_closure(
                "n_waiting", move |_: &[PyObjectRef]| { Ok(PyObject::int(*w1.read())) }));
            let w2 = waiting.clone();
            let b2 = broken.clone();
            let p = parties;
            attrs.insert(CompactString::from("wait"), PyObject::native_closure(
                "wait", move |_: &[PyObjectRef]| {
                    if *b2.read() {
                        return Err(PyException::runtime_error("BrokenBarrierError"));
                    }
                    let mut w = w2.write();
                    *w += 1;
                    if *w >= p { *w = 0; }
                    Ok(PyObject::int(0))
                }));
            let w3 = waiting.clone();
            attrs.insert(CompactString::from("reset"), PyObject::native_closure(
                "reset", move |_: &[PyObjectRef]| {
                    *w3.write() = 0;
                    Ok(PyObject::none())
                }));
            let w4 = waiting.clone();
            let b4 = broken.clone();
            attrs.insert(CompactString::from("abort"), PyObject::native_closure(
                "abort", move |_: &[PyObjectRef]| {
                    *b4.write() = true;
                    *w4.write() = 0;
                    Ok(PyObject::none())
                }));
            let b5 = broken.clone();
            attrs.insert(CompactString::from("broken"), PyObject::native_closure(
                "broken", move |_: &[PyObjectRef]| { Ok(PyObject::bool_val(*b5.read())) }));
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
            attrs.insert(CompactString::from("acquire"), PyObject::native_closure(
                "acquire", move |_: &[PyObjectRef]| {
                    let _guard = m1.lock().unwrap();
                    Ok(PyObject::bool_val(true))
                }));
            let m2 = mutex.clone();
            attrs.insert(CompactString::from("release"), PyObject::native_closure(
                "release", move |_: &[PyObjectRef]| {
                    // Release is implicit when guard drops
                    let _guard = m2.lock();
                    Ok(PyObject::none())
                }));
            // wait(timeout=None) — release lock, wait for notify, re-acquire lock
            let m3 = mutex.clone();
            let c3 = condvar.clone();
            attrs.insert(CompactString::from("wait"), PyObject::native_closure(
                "wait", move |args: &[PyObjectRef]| {
                    let timeout = args.get(0)
                        .and_then(|a| if matches!(&a.payload, PyObjectPayload::None) { None } else { Some(a) })
                        .and_then(|a| a.to_float().ok());
                    let guard = m3.lock().unwrap();
                    if let Some(secs) = timeout {
                        let dur = std::time::Duration::from_secs_f64(secs);
                        let _result = c3.wait_timeout(guard, dur).unwrap();
                    } else {
                        let _result = c3.wait(guard).unwrap();
                    }
                    Ok(PyObject::bool_val(true))
                }));
            // wait_for(predicate, timeout=None) — simplified: evaluates predicate, waits if false
            let m3b = mutex.clone();
            let c3b = condvar.clone();
            attrs.insert(CompactString::from("wait_for"), PyObject::native_closure(
                "wait_for", move |args: &[PyObjectRef]| {
                    // In CPython, wait_for calls wait() in a loop until predicate() is true.
                    // Since we can't call Python functions from native code without VM access,
                    // we do a single condvar wait then return True (like CPython's successful case).
                    let timeout = args.get(1)
                        .and_then(|a| if matches!(&a.payload, PyObjectPayload::None) { None } else { Some(a) })
                        .and_then(|a| a.to_float().ok());
                    let guard = m3b.lock().unwrap();
                    if let Some(secs) = timeout {
                        let dur = std::time::Duration::from_secs_f64(secs);
                        let _result = c3b.wait_timeout(guard, dur).unwrap();
                    } else {
                        let _result = c3b.wait(guard).unwrap();
                    }
                    Ok(PyObject::bool_val(true))
                }));
            let c4 = condvar.clone();
            attrs.insert(CompactString::from("notify"), PyObject::native_closure(
                "notify", move |_: &[PyObjectRef]| {
                    c4.notify_one();
                    Ok(PyObject::none())
                }));
            let c5 = condvar.clone();
            attrs.insert(CompactString::from("notify_all"), PyObject::native_closure(
                "notify_all", move |_: &[PyObjectRef]| {
                    c5.notify_all();
                    Ok(PyObject::none())
                }));
            let m6 = mutex.clone();
            let ir = inst_ref.clone();
            attrs.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "__enter__", move |_: &[PyObjectRef]| {
                    let _guard = m6.lock().unwrap();
                    Ok(ir.clone())
                }));
            let m7 = mutex.clone();
            attrs.insert(CompactString::from("__exit__"), PyObject::native_closure(
                "__exit__", move |_: &[PyObjectRef]| {
                    let _guard = m7.lock();
                    Ok(PyObject::bool_val(false))
                }));
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
            } else { 0.0 };
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
            if args.len() >= 3 && matches!(&fn_args.payload, PyObjectPayload::Tuple(t) if t.is_empty()) {
                fn_args = args[2].clone();
            }
            attrs.insert(CompactString::from("function"), target.clone());
            attrs.insert(CompactString::from("args"), fn_args.clone());
            attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from("Timer")));
            attrs.insert(CompactString::from("daemon"), PyObject::bool_val(false));

            let c1 = cancelled.clone();
            attrs.insert(CompactString::from("cancel"), PyObject::native_closure(
                "cancel", move |_: &[PyObjectRef]| { *c1.write() = true; Ok(PyObject::none()) }));

            let c2 = cancelled.clone();
            let a1 = alive.clone();
            let tgt = target.clone();
            let targs = fn_args.clone();
            attrs.insert(CompactString::from("start"), PyObject::native_closure(
                "start", move |_: &[PyObjectRef]| {
                    if *c2.read() { return Ok(PyObject::none()); }
                    *a1.write() = true;
                    if !matches!(&tgt.payload, PyObjectPayload::None) {
                        let call_args: Vec<PyObjectRef> = match &targs.payload {
                            PyObjectPayload::Tuple(items) => items.clone(),
                            PyObjectPayload::List(items) => items.read().clone(),
                            _ => vec![],
                        };
                        match &tgt.payload {
                            PyObjectPayload::NativeFunction(nf) => { let _ = (nf.func)(&call_args); }
                            PyObjectPayload::NativeClosure(nc) => { let _ = (nc.func)(&call_args); }
                            _ => { push_deferred_call(tgt.clone(), call_args); }
                        }
                    }
                    *a1.write() = false;
                    Ok(PyObject::none())
                }));
            let a2 = alive.clone();
            attrs.insert(CompactString::from("join"), PyObject::native_closure(
                "join", move |args: &[PyObjectRef]| {
                    // Timer runs synchronously, so if alive, spin-wait with optional timeout
                    let timeout = args.first()
                        .and_then(|a| if matches!(&a.payload, PyObjectPayload::None) { None } else { a.to_float().ok() });
                    if *a2.read() {
                        if let Some(t) = timeout {
                            std::thread::sleep(std::time::Duration::from_secs_f64(t));
                        }
                    }
                    Ok(PyObject::none())
                }));
            let a3 = alive.clone();
            let c3 = cancelled.clone();
            attrs.insert(CompactString::from("is_alive"), PyObject::native_closure(
                "is_alive", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(*a3.read() && !*c3.read()))
                }));
            attrs.insert(CompactString::from("ident"), PyObject::none());
        }
        Ok(inst)
    });

    // current_thread() — return Thread-like object
    let current_thread_fn = PyObject::native_closure("current_thread", move |_: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("Thread"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref i) = inst.payload {
            let mut attrs = i.attrs.write();
            attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from("MainThread")));
            attrs.insert(CompactString::from("ident"), PyObject::int(1));
            attrs.insert(CompactString::from("daemon"), PyObject::bool_val(false));
            attrs.insert(CompactString::from("is_alive"), make_builtin(|_| Ok(PyObject::bool_val(true))));
            attrs.insert(CompactString::from("getName"), make_builtin(|_| Ok(PyObject::str_val(CompactString::from("MainThread")))));
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
            attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from("MainThread")));
            attrs.insert(CompactString::from("ident"), PyObject::int(1));
            attrs.insert(CompactString::from("daemon"), PyObject::bool_val(false));
            attrs.insert(CompactString::from("is_alive"), make_builtin(|_| Ok(PyObject::bool_val(true))));
        }
        Ok(PyObject::list(vec![main]))
    });

    make_module("threading", vec![
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
        ("main_thread", make_builtin(|_| {
            let cls = PyObject::class(CompactString::from("Thread"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref i) = inst.payload {
                let mut attrs = i.attrs.write();
                attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from("MainThread")));
                attrs.insert(CompactString::from("ident"), PyObject::int(1));
                attrs.insert(CompactString::from("daemon"), PyObject::bool_val(false));
                attrs.insert(CompactString::from("is_alive"), make_builtin(|_| Ok(PyObject::bool_val(true))));
            }
            Ok(inst)
        })),
        ("local", make_builtin(|_| {
            let cls = PyObject::class(CompactString::from("local"), vec![], IndexMap::new());
            Ok(PyObject::instance(cls))
        })),
        ("get_ident", make_builtin(|_| {
            let tid = std::thread::current().id();
            let id_str = format!("{:?}", tid);
            // Extract numeric id from "ThreadId(N)"
            let num: i64 = id_str.trim_start_matches("ThreadId(").trim_end_matches(')')
                .parse().unwrap_or(1);
            Ok(PyObject::int(num))
        })),
        ("get_native_id", make_builtin(|_| {
            Ok(PyObject::int(std::process::id() as i64))
        })),
        ("stack_size", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                Ok(PyObject::int(0))
            } else {
                Ok(PyObject::int(0))
            }
        })),
        ("settrace", make_builtin(|_| Ok(PyObject::none()))),
        ("setprofile", make_builtin(|_| Ok(PyObject::none()))),
        ("excepthook", make_builtin(|_| Ok(PyObject::none()))),
        ("TIMEOUT_MAX", PyObject::float(f64::MAX)),
    ])
}

// ── datetime module ──


pub fn create_weakref_module() -> PyObjectRef {

    // Helper: upgrade a PyWeakRef or return None
    fn upgrade_or_none(weak: &PyWeakRef) -> PyObjectRef {
        match weak.upgrade() {
            Some(arc) => arc,
            None => PyObject::none(),
        }
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

    make_module("weakref", vec![
        // ── ref(obj, callback=None) ──
        // Returns a callable weak reference. Calling it returns the referent or None.
        ("ref", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("ref() requires at least 1 argument")); }
            let weak: PyWeakRef = PyObjectRef::downgrade(&args[0]);
            let _callback = args.get(1).cloned(); // stored but not auto-invoked in refcount GC

            let cls = PyObject::class(CompactString::from("weakref"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                let mut attrs = inst_data.attrs.write();

                // __call__() → referent or None
                let w1 = weak.clone();
                attrs.insert(CompactString::from("__call__"), PyObject::native_closure(
                    "weakref.__call__", move |_args| Ok(upgrade_or_none(&w1)),
                ));

                // __repr__
                let w_repr = weak.clone();
                attrs.insert(CompactString::from("__repr__"), PyObject::native_closure(
                    "weakref.__repr__", move |_| {
                        if w_repr.upgrade().is_some() {
                            Ok(PyObject::str_val(CompactString::from("<weakref (alive)>")))
                        } else {
                            Ok(PyObject::str_val(CompactString::from("<weakref (dead)>")))
                        }
                    },
                ));

                // __bool__: True if referent is alive
                let w_bool = weak.clone();
                attrs.insert(CompactString::from("__bool__"), PyObject::native_closure(
                    "weakref.__bool__", move |_| Ok(PyObject::bool_val(w_bool.upgrade().is_some())),
                ));

                // __eq__: two refs are equal if they point to the same object
                let w_eq = weak.clone();
                attrs.insert(CompactString::from("__eq__"), PyObject::native_closure(
                    "weakref.__eq__", move |args| {
                        if let Some(other) = args.first() {
                            if let Some(strong) = w_eq.upgrade() {
                                return Ok(PyObject::bool_val(PyObjectRef::ptr_eq(&strong, other)));
                            }
                        }
                        Ok(PyObject::bool_val(false))
                    },
                ));
            }
            Ok(inst)
        })),

        // ── proxy(obj, callback=None) ──
        // Returns a proxy that auto-dereferences on attribute access.
        ("proxy", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("proxy() requires at least 1 argument")); }
            let weak: PyWeakRef = PyObjectRef::downgrade(&args[0]);
            let _callback = args.get(1).cloned();

            let cls = PyObject::class(CompactString::from("weakproxy"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                let mut attrs = inst_data.attrs.write();

                // VM-accessible target accessor for transparent delegation
                let w_target = weak.clone();
                attrs.insert(CompactString::from("__weakref_target__"), PyObject::native_closure(
                    "__weakref_target__", move |_| { upgrade_or_err(&w_target) },
                ));

                // __getattr__(name) → forward to referent
                let w_ga = weak.clone();
                attrs.insert(CompactString::from("__getattr__"), PyObject::native_closure(
                    "weakproxy.__getattr__", move |args| {
                        let referent = upgrade_or_err(&w_ga)?;
                        if let Some(name_obj) = args.first() {
                            let name = name_obj.py_to_string();
                            referent.get_attr(&name).ok_or_else(|| {
                                PyException::attribute_error(format!(
                                    "'weakproxy' object has no attribute '{}'", name
                                ))
                            })
                        } else {
                            Err(PyException::type_error("__getattr__ requires a name argument"))
                        }
                    },
                ));

                // __repr__
                let w_r = weak.clone();
                attrs.insert(CompactString::from("__repr__"), PyObject::native_closure(
                    "weakproxy.__repr__", move |_| {
                        match w_r.upgrade() {
                            Some(obj) => Ok(PyObject::str_val(CompactString::from(
                                format!("<weakproxy at {:p}>", PyObjectRef::as_ptr(&obj))
                            ))),
                            None => Err(PyException::new(
                                ferrython_core::error::ExceptionKind::ReferenceError,
                                "weakly-referenced object no longer exists",
                            )),
                        }
                    },
                ));

                // __bool__
                let w_b = weak.clone();
                attrs.insert(CompactString::from("__bool__"), PyObject::native_closure(
                    "weakproxy.__bool__", move |_| {
                        let referent = upgrade_or_err(&w_b)?;
                        Ok(PyObject::bool_val(referent.is_truthy()))
                    },
                ));

                // __str__
                let w_s = weak.clone();
                attrs.insert(CompactString::from("__str__"), PyObject::native_closure(
                    "weakproxy.__str__", move |_| {
                        let referent = upgrade_or_err(&w_s)?;
                        Ok(PyObject::str_val(CompactString::from(referent.py_to_string())))
                    },
                ));

                // __call__ — forward calls to the referent
                let w_c = weak.clone();
                attrs.insert(CompactString::from("__call__"), PyObject::native_closure(
                    "weakproxy.__call__", move |_args| {
                        let _referent = upgrade_or_err(&w_c)?;
                        // Return the live object so the VM can call it
                        // For proxy, calling the proxy attempts to call the referent
                        Err(PyException::type_error("weakproxy object is not directly callable; access attributes instead"))
                    },
                ));
            }
            Ok(inst)
        })),

        // ── WeakValueDictionary() ──
        // Dict where values are weak references; dead entries are auto-pruned.
        ("WeakValueDictionary", make_builtin(|_| {
            let storage: Rc<PyCell<IndexMap<CompactString, PyWeakRef>>> =
                Rc::new(PyCell::new(IndexMap::new()));

            let cls = PyObject::class(CompactString::from("WeakValueDictionary"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                let mut attrs = inst_data.attrs.write();

                // __setitem__(key, value)
                let s1 = storage.clone();
                attrs.insert(CompactString::from("__setitem__"), PyObject::native_closure(
                    "WeakValueDictionary.__setitem__", move |args| {
                        if args.len() < 2 { return Err(PyException::type_error("__setitem__ requires key and value")); }
                        let key = CompactString::from(args[0].py_to_string());
                        let weak = PyObjectRef::downgrade(&args[1]);
                        s1.write().insert(key, weak);
                        Ok(PyObject::none())
                    },
                ));

                // __getitem__(key)
                let s2 = storage.clone();
                attrs.insert(CompactString::from("__getitem__"), PyObject::native_closure(
                    "WeakValueDictionary.__getitem__", move |args| {
                        if args.is_empty() { return Err(PyException::type_error("__getitem__ requires a key")); }
                        let key = CompactString::from(args[0].py_to_string());
                        let mut store = s2.write();
                        match store.get(&key) {
                            Some(weak) => match weak.upgrade() {
                                Some(obj) => Ok(obj),
                                None => {
                                    store.shift_remove(&key);
                                    Err(PyException::key_error(key.to_string()))
                                }
                            },
                            None => Err(PyException::key_error(key.to_string())),
                        }
                    },
                ));

                // __delitem__(key)
                let s3 = storage.clone();
                attrs.insert(CompactString::from("__delitem__"), PyObject::native_closure(
                    "WeakValueDictionary.__delitem__", move |args| {
                        if args.is_empty() { return Err(PyException::type_error("__delitem__ requires a key")); }
                        let key = CompactString::from(args[0].py_to_string());
                        let mut store = s3.write();
                        if store.shift_remove(&key).is_some() {
                            Ok(PyObject::none())
                        } else {
                            Err(PyException::key_error(key.to_string()))
                        }
                    },
                ));

                // __contains__(key)
                let s4 = storage.clone();
                attrs.insert(CompactString::from("__contains__"), PyObject::native_closure(
                    "WeakValueDictionary.__contains__", move |args| {
                        if args.is_empty() { return Err(PyException::type_error("__contains__ requires a key")); }
                        let key = CompactString::from(args[0].py_to_string());
                        let mut store = s4.write();
                        match store.get(&key) {
                            Some(weak) => {
                                if weak.upgrade().is_some() {
                                    Ok(PyObject::bool_val(true))
                                } else {
                                    store.shift_remove(&key);
                                    Ok(PyObject::bool_val(false))
                                }
                            }
                            None => Ok(PyObject::bool_val(false)),
                        }
                    },
                ));

                // __len__()
                let s5 = storage.clone();
                attrs.insert(CompactString::from("__len__"), PyObject::native_closure(
                    "WeakValueDictionary.__len__", move |_| {
                        let mut store = s5.write();
                        store.retain(|_, w| w.upgrade().is_some());
                        Ok(PyObject::int(store.len() as i64))
                    },
                ));

                // get(key, default=None)
                let s_get = storage.clone();
                attrs.insert(CompactString::from("get"), PyObject::native_closure(
                    "WeakValueDictionary.get", move |args| {
                        if args.is_empty() { return Err(PyException::type_error("get() requires a key")); }
                        let key = CompactString::from(args[0].py_to_string());
                        let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
                        let mut store = s_get.write();
                        match store.get(&key) {
                            Some(weak) => match weak.upgrade() {
                                Some(obj) => Ok(obj),
                                None => { store.shift_remove(&key); Ok(default) }
                            },
                            None => Ok(default),
                        }
                    },
                ));

                // keys()
                let s6 = storage.clone();
                attrs.insert(CompactString::from("keys"), PyObject::native_closure(
                    "WeakValueDictionary.keys", move |_| {
                        let mut store = s6.write();
                        store.retain(|_, w| w.upgrade().is_some());
                        let keys: Vec<PyObjectRef> = store.keys()
                            .map(|k| PyObject::str_val(k.clone()))
                            .collect();
                        Ok(PyObject::list(keys))
                    },
                ));

                // values()
                let s7 = storage.clone();
                attrs.insert(CompactString::from("values"), PyObject::native_closure(
                    "WeakValueDictionary.values", move |_| {
                        let mut store = s7.write();
                        store.retain(|_, w| w.upgrade().is_some());
                        let vals: Vec<PyObjectRef> = store.values()
                            .filter_map(|w| w.upgrade())
                            .collect();
                        Ok(PyObject::list(vals))
                    },
                ));

                // items()
                let s8 = storage.clone();
                attrs.insert(CompactString::from("items"), PyObject::native_closure(
                    "WeakValueDictionary.items", move |_| {
                        let mut store = s8.write();
                        store.retain(|_, w| w.upgrade().is_some());
                        let items: Vec<PyObjectRef> = store.iter()
                            .filter_map(|(k, w)| {
                                w.upgrade().map(|v| PyObject::tuple(vec![
                                    PyObject::str_val(k.clone()),
                                    v,
                                ]))
                            })
                            .collect();
                        Ok(PyObject::list(items))
                    },
                ));
            }
            Ok(inst)
        })),

        // ── WeakKeyDictionary() ──
        // Dict where keys are weak references; dead entries are auto-pruned.
        ("WeakKeyDictionary", make_builtin(|_| {
            // Store (PyWeakRef, value) keyed by raw pointer (usize)
            let storage: Rc<PyCell<IndexMap<usize, (PyWeakRef, PyObjectRef)>>> =
                Rc::new(PyCell::new(IndexMap::new()));

            let cls = PyObject::class(CompactString::from("WeakKeyDictionary"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                let mut attrs = inst_data.attrs.write();

                let s1 = storage.clone();
                attrs.insert(CompactString::from("__setitem__"), PyObject::native_closure(
                    "WeakKeyDictionary.__setitem__", move |args| {
                        if args.len() < 2 { return Err(PyException::type_error("__setitem__ requires key and value")); }
                        let ptr = PyObjectRef::as_ptr(&args[0]) as usize;
                        let weak = PyObjectRef::downgrade(&args[0]);
                        s1.write().insert(ptr, (weak, args[1].clone()));
                        Ok(PyObject::none())
                    },
                ));

                let s2 = storage.clone();
                attrs.insert(CompactString::from("__getitem__"), PyObject::native_closure(
                    "WeakKeyDictionary.__getitem__", move |args| {
                        if args.is_empty() { return Err(PyException::type_error("__getitem__ requires a key")); }
                        let ptr = PyObjectRef::as_ptr(&args[0]) as usize;
                        let mut store = s2.write();
                        match store.get(&ptr) {
                            Some((weak, val)) => {
                                if weak.upgrade().is_some() {
                                    Ok(val.clone())
                                } else {
                                    store.shift_remove(&ptr);
                                    Err(PyException::key_error("dead weak key"))
                                }
                            }
                            None => Err(PyException::key_error("key not found")),
                        }
                    },
                ));

                let s3 = storage.clone();
                attrs.insert(CompactString::from("__len__"), PyObject::native_closure(
                    "WeakKeyDictionary.__len__", move |_| {
                        let mut store = s3.write();
                        store.retain(|_, (w, _)| w.upgrade().is_some());
                        Ok(PyObject::int(store.len() as i64))
                    },
                ));

                let s4 = storage.clone();
                attrs.insert(CompactString::from("__contains__"), PyObject::native_closure(
                    "WeakKeyDictionary.__contains__", move |args| {
                        if args.is_empty() { return Err(PyException::type_error("__contains__ requires a key")); }
                        let ptr = PyObjectRef::as_ptr(&args[0]) as usize;
                        let mut store = s4.write();
                        match store.get(&ptr) {
                            Some((weak, _)) => {
                                if weak.upgrade().is_some() {
                                    Ok(PyObject::bool_val(true))
                                } else {
                                    store.shift_remove(&ptr);
                                    Ok(PyObject::bool_val(false))
                                }
                            }
                            None => Ok(PyObject::bool_val(false)),
                        }
                    },
                ));
            }
            Ok(inst)
        })),

        // ── WeakSet() ──
        // A set of weak references. Dead entries are auto-pruned.
        ("WeakSet", make_builtin(|_| {
            let storage: Rc<PyCell<IndexMap<usize, PyWeakRef>>> =
                Rc::new(PyCell::new(IndexMap::new()));

            let cls = PyObject::class(CompactString::from("WeakSet"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                let mut attrs = inst_data.attrs.write();

                // add(obj)
                let s1 = storage.clone();
                attrs.insert(CompactString::from("add"), PyObject::native_closure(
                    "WeakSet.add", move |args| {
                        if args.is_empty() { return Err(PyException::type_error("add() requires an argument")); }
                        let ptr = PyObjectRef::as_ptr(&args[0]) as usize;
                        let weak = PyObjectRef::downgrade(&args[0]);
                        s1.write().insert(ptr, weak);
                        Ok(PyObject::none())
                    },
                ));

                // discard(obj)
                let s2 = storage.clone();
                attrs.insert(CompactString::from("discard"), PyObject::native_closure(
                    "WeakSet.discard", move |args| {
                        if args.is_empty() { return Err(PyException::type_error("discard() requires an argument")); }
                        let ptr = PyObjectRef::as_ptr(&args[0]) as usize;
                        s2.write().shift_remove(&ptr);
                        Ok(PyObject::none())
                    },
                ));

                // __contains__(obj)
                let s3 = storage.clone();
                attrs.insert(CompactString::from("__contains__"), PyObject::native_closure(
                    "WeakSet.__contains__", move |args| {
                        if args.is_empty() { return Err(PyException::type_error("__contains__ requires an argument")); }
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
                    },
                ));

                // __len__()
                let s4 = storage.clone();
                attrs.insert(CompactString::from("__len__"), PyObject::native_closure(
                    "WeakSet.__len__", move |_| {
                        let mut store = s4.write();
                        store.retain(|_, w| w.upgrade().is_some());
                        Ok(PyObject::int(store.len() as i64))
                    },
                ));

                // __iter__() — return a list of live items
                let s5 = storage.clone();
                attrs.insert(CompactString::from("__iter__"), PyObject::native_closure(
                    "WeakSet.__iter__", move |_| {
                        let mut store = s5.write();
                        store.retain(|_, w| w.upgrade().is_some());
                        let items: Vec<PyObjectRef> = store.values()
                            .filter_map(|w| w.upgrade())
                            .collect();
                        Ok(PyObject::list(items))
                    },
                ));
            }
            Ok(inst)
        })),

        // ── finalize(obj, func, *args, **kwargs) ──
        // Release callback. Stores a weak ref + callback; invokes when ref dies (best-effort).
        ("finalize", PyObject::native_closure("finalize", |args: &[PyObjectRef]| {
            if args.len() < 2 { return Err(PyException::type_error("finalize requires obj and func")); }
            let weak: PyWeakRef = PyObjectRef::downgrade(&args[0]);
            let func = args[1].clone();
            let extra = if args.len() > 2 { args[2..].to_vec() } else { vec![] };

            let cls = PyObject::class(CompactString::from("finalize"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                let mut attrs = inst_data.attrs.write();

                // alive — True if the weak ref is still valid (simplified: always True while ref exists)
                attrs.insert(CompactString::from("alive"), PyObject::bool_val(true));

                attrs.insert(CompactString::from("_func"), func.clone());
                attrs.insert(CompactString::from("_args"), PyObject::tuple(extra.clone()));

                // detach() — disarm the finalizer, return (obj, func, args, kwargs) or None
                let w_det = weak.clone();
                let f_det = func.clone();
                let e_det = extra.clone();
                attrs.insert(CompactString::from("detach"), PyObject::native_closure(
                    "finalize.detach", move |_| {
                        match w_det.upgrade() {
                            Some(obj) => Ok(PyObject::tuple(vec![
                                obj,
                                f_det.clone(),
                                PyObject::tuple(e_det.clone()),
                                PyObject::none(), // kwargs placeholder
                            ])),
                            None => Ok(PyObject::none()),
                        }
                    },
                ));

                // peek() — return (obj, func, args, kwargs) without disarming, or None
                let w_peek = weak.clone();
                let f_peek = func.clone();
                let e_peek = extra.clone();
                attrs.insert(CompactString::from("peek"), PyObject::native_closure(
                    "finalize.peek", move |_| {
                        match w_peek.upgrade() {
                            Some(obj) => Ok(PyObject::tuple(vec![
                                obj,
                                f_peek.clone(),
                                PyObject::tuple(e_peek.clone()),
                                PyObject::none(),
                            ])),
                            None => Ok(PyObject::none()),
                        }
                    },
                ));

                // __call__() — manually invoke the callback (if referent is alive or freshly dead)
                let _w_call = weak;
                let f_call = func;
                let e_call = extra;
                attrs.insert(CompactString::from("__call__"), PyObject::native_closure(
                    "finalize.__call__", move |_| {
                        // Invoke callback with stored args (best-effort for native closures)
                        match &f_call.payload {
                            PyObjectPayload::NativeFunction(nf) => (nf.func)(&e_call),
                            PyObjectPayload::NativeClosure(nc) => (nc.func)(&e_call),
                            _ => {
                                // For Python-defined functions, we'd need VM access.
                                // Use deferred call mechanism.
                                ferrython_core::error::request_vm_call(f_call.clone(), e_call.clone());
                                Ok(PyObject::none())
                            }
                        }
                    },
                ));
            }
            Ok(inst)
        })),

        // ── getweakrefcount(obj) ──
        ("getweakrefcount", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("getweakrefcount requires 1 argument")); }
            Ok(PyObject::int(PyObjectRef::weak_count(&args[0]) as i64))
        })),

        // ── getweakrefs(obj) ──
        ("getweakrefs", make_builtin(|_args| {
            // Cannot enumerate all Weak pointers from an Arc — return empty list
            Ok(PyObject::list(vec![]))
        })),

        // ── ReferenceType (the type of weak references) ──
        ("ReferenceType", PyObject::class(CompactString::from("weakref"), vec![], IndexMap::new())),

        // ── ProxyType ──
        ("ProxyType", PyObject::class(CompactString::from("weakproxy"), vec![], IndexMap::new())),

        // ── CallableProxyType ──
        ("CallableProxyType", PyObject::class(CompactString::from("weakcallableproxy"), vec![], IndexMap::new())),

        // ── WeakMethod(method, callback=None) ──
        ("WeakMethod", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("WeakMethod requires at least 1 argument")); }
            let method = args[0].clone();
            let weak: PyWeakRef = PyObjectRef::downgrade(&method);
            let cls = PyObject::class(CompactString::from("WeakMethod"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                let w1 = weak.clone();
                w.insert(CompactString::from("__call__"), PyObject::native_closure(
                    "WeakMethod.__call__", move |_| Ok(upgrade_or_none(&w1)),
                ));
                let w2 = weak.clone();
                w.insert(CompactString::from("__bool__"), PyObject::native_closure(
                    "WeakMethod.__bool__", move |_| Ok(PyObject::bool_val(w2.upgrade().is_some())),
                ));
            }
            Ok(inst)
        })),
    ])
}

// ── gc module ──


pub fn create_gc_module() -> PyObjectRef {
    make_module("gc", vec![
        ("enable", make_builtin(|_| {
            ferrython_gc::enable();
            Ok(PyObject::none())
        })),
        ("disable", make_builtin(|_| {
            ferrython_gc::disable();
            Ok(PyObject::none())
        })),
        ("isenabled", make_builtin(|_| {
            Ok(PyObject::bool_val(ferrython_gc::is_enabled()))
        })),
        ("collect", make_builtin(|_| {
            let collected = ferrython_gc::collect();
            Ok(PyObject::int(collected as i64))
        })),
        ("get_threshold", make_builtin(|_| {
            let (g0, g1, g2) = ferrython_gc::get_threshold();
            Ok(PyObject::tuple(vec![
                PyObject::int(g0 as i64),
                PyObject::int(g1 as i64),
                PyObject::int(g2 as i64),
            ]))
        })),
        ("set_threshold", make_builtin(|args| {
            check_args_min("gc.set_threshold", args, 1)?;
            let g0 = args[0].as_int().ok_or_else(|| {
                PyException::type_error("threshold must be an integer")
            })? as u64;
            let g1 = args.get(1).and_then(|a| a.as_int()).unwrap_or(10) as u64;
            let g2 = args.get(2).and_then(|a| a.as_int()).unwrap_or(10) as u64;
            ferrython_gc::set_threshold(g0, g1, g2);
            Ok(PyObject::none())
        })),
        ("get_stats", make_builtin(|_| {
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
        })),
        ("get_count", make_builtin(|_| {
            let stats = ferrython_gc::get_stats();
            Ok(PyObject::tuple(vec![
                PyObject::int(stats.allocations as i64),
                PyObject::int(0),
                PyObject::int(0),
            ]))
        })),
        ("get_objects", make_builtin(|_| {
            // CPython returns all tracked objects; we return empty list (Rust manages memory)
            Ok(PyObject::list(vec![]))
        })),
        ("get_referrers", make_builtin(|_| {
            Ok(PyObject::list(vec![]))
        })),
        ("get_referents", make_builtin(|_| {
            Ok(PyObject::list(vec![]))
        })),
        ("freeze", make_builtin(|_| Ok(PyObject::none()))),
        ("unfreeze", make_builtin(|_| Ok(PyObject::none()))),
        ("get_freeze_count", make_builtin(|_| Ok(PyObject::int(0)))),
        ("callbacks", PyObject::list(vec![])),
        ("garbage", PyObject::list(vec![])),
        ("DEBUG_STATS", PyObject::int(1)),
        ("DEBUG_COLLECTABLE", PyObject::int(2)),
        ("DEBUG_UNCOLLECTABLE", PyObject::int(4)),
        ("DEBUG_SAVEALL", PyObject::int(32)),
        ("DEBUG_LEAK", PyObject::int(38)),
    ])
}

// ── _thread module ──

pub fn create_thread_module() -> PyObjectRef {
    make_module("_thread", vec![
        ("allocate_lock", make_builtin(|_| {
            let locked = Arc::new(std::sync::Mutex::new(false));
            let cls = PyObject::class(CompactString::from("lock"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                let l1 = locked.clone();
                w.insert(CompactString::from("acquire"), PyObject::native_closure(
                    "acquire", move |args: &[PyObjectRef]| {
                        let blocking = args.first().map(|a| a.is_truthy()).unwrap_or(true);
                        let mut guard = l1.lock().unwrap();
                        if *guard {
                            if !blocking { return Ok(PyObject::bool_val(false)); }
                            // In single-threaded context, can't block — return false
                            return Ok(PyObject::bool_val(false));
                        }
                        *guard = true;
                        Ok(PyObject::bool_val(true))
                    }));
                let l2 = locked.clone();
                w.insert(CompactString::from("release"), PyObject::native_closure(
                    "release", move |_: &[PyObjectRef]| {
                        let mut guard = l2.lock().unwrap();
                        if !*guard {
                            return Err(PyException::runtime_error("release unlocked lock"));
                        }
                        *guard = false;
                        Ok(PyObject::none())
                    }));
                let l3 = locked.clone();
                w.insert(CompactString::from("locked"), PyObject::native_closure(
                    "locked", move |_: &[PyObjectRef]| {
                        Ok(PyObject::bool_val(*l3.lock().unwrap()))
                    }));
                let l4 = locked.clone();
                w.insert(CompactString::from("__enter__"), PyObject::native_closure(
                    "__enter__", move |_: &[PyObjectRef]| {
                        let mut guard = l4.lock().unwrap();
                        *guard = true;
                        Ok(PyObject::bool_val(true))
                    }));
                let l5 = locked;
                w.insert(CompactString::from("__exit__"), PyObject::native_closure(
                    "__exit__", move |_: &[PyObjectRef]| {
                        let mut guard = l5.lock().unwrap();
                        *guard = false;
                        Ok(PyObject::none())
                    }));
            }
            Ok(inst)
        })),
        ("LockType", PyObject::class(CompactString::from("lock"), vec![], IndexMap::new())),
        ("start_new_thread", make_builtin(|args| {
            if args.is_empty() {
                return Err(PyException::type_error("start_new_thread requires a callable"));
            }
            let func = args[0].clone();
            let call_args: Vec<PyObjectRef> = if args.len() > 1 {
                args[1].to_list().unwrap_or_default()
            } else { vec![] };
            // Spawn a real OS thread for native closures/functions
            let closure: Box<dyn FnOnce()> = Box::new(move || {
                match &func.payload {
                    PyObjectPayload::NativeClosure(nc) => { let _ = (nc.func)(&call_args); }
                    PyObjectPayload::NativeFunction(nf) => { let _ = (nf.func)(&call_args); }
                    _ => {} // Python-defined functions need VM — can't call from here
                }
            });
            let send_closure: Box<dyn FnOnce() + Send> = unsafe {
                std::mem::transmute(closure)
            };
            let handle = std::thread::spawn(move || { send_closure(); });
            // Return thread ID
            let tid = format!("{:?}", handle.thread().id());
            let id_num: i64 = tid.chars().filter(|c| c.is_ascii_digit()).collect::<String>()
                .parse().unwrap_or(1);
            Ok(PyObject::int(id_num))
        })),
        ("get_ident", make_builtin(|_| {
            let tid = format!("{:?}", std::thread::current().id());
            let id_num: i64 = tid.chars().filter(|c| c.is_ascii_digit()).collect::<String>()
                .parse().unwrap_or(1);
            Ok(PyObject::int(id_num))
        })),
        ("stack_size", make_builtin(|_| Ok(PyObject::int(0)))),
        ("TIMEOUT_MAX", PyObject::float(f64::MAX)),
    ])
}

// ── signal module ────────────────────────────────────────────────────

use std::cell::RefCell as SignalRefCell;
use std::collections::HashMap as SignalMap;

thread_local! {
    static SIGNAL_HANDLERS: SignalRefCell<SignalMap<i64, PyObjectRef>> = SignalRefCell::new(SignalMap::new());
}

pub fn create_signal_module() -> PyObjectRef {
    // Signal constants (POSIX values)
    make_module("signal", vec![
        ("SIGABRT", PyObject::int(6)),
        ("SIGALRM", PyObject::int(14)),
        ("SIGBUS", PyObject::int(7)),
        ("SIGCHLD", PyObject::int(17)),
        ("SIGCONT", PyObject::int(18)),
        ("SIGFPE", PyObject::int(8)),
        ("SIGHUP", PyObject::int(1)),
        ("SIGILL", PyObject::int(4)),
        ("SIGINT", PyObject::int(2)),
        ("SIGKILL", PyObject::int(9)),
        ("SIGPIPE", PyObject::int(13)),
        ("SIGQUIT", PyObject::int(3)),
        ("SIGSEGV", PyObject::int(11)),
        ("SIGSTOP", PyObject::int(19)),
        ("SIGTERM", PyObject::int(15)),
        ("SIGUSR1", PyObject::int(10)),
        ("SIGUSR2", PyObject::int(12)),
        ("SIGWINCH", PyObject::int(28)),
        ("NSIG", PyObject::int(65)),
        ("SIG_DFL", PyObject::int(0)),
        ("SIG_IGN", PyObject::int(1)),
        ("signal", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("signal() requires 2 arguments")); }
            let signum = args[0].to_int()?;
            let handler = args[1].clone();
            let handler_is_callable = matches!(
                handler.payload,
                PyObjectPayload::Function(_)
                | PyObjectPayload::NativeFunction(_)
                | PyObjectPayload::NativeClosure(_)
                | PyObjectPayload::BoundMethod { .. }
            );
            // Return previous handler, store new one
            let prev = SIGNAL_HANDLERS.with(|h| {
                let mut map = h.borrow_mut();
                let old = map.get(&signum).cloned().unwrap_or_else(|| PyObject::int(0));
                map.insert(signum, handler.clone());
                old
            });
            // Install real OS signal handler so the process doesn't die
            #[cfg(unix)]
            {
                use std::sync::atomic::{AtomicU64, Ordering};
                // Global bitmask of signals with pending Python handlers
                static PENDING_SIGNALS: AtomicU64 = AtomicU64::new(0);

                let handler_int = handler.to_int().unwrap_or(-1);
                if handler_int == 0 {
                    // SIG_DFL — restore default
                    unsafe { libc::signal(signum as libc::c_int, libc::SIG_DFL); }
                } else if handler_int == 1 {
                    // SIG_IGN — ignore
                    unsafe { libc::signal(signum as libc::c_int, libc::SIG_IGN); }
                } else if handler_is_callable && signum < 64 {
                    // Install a C handler that sets the pending bit
                    unsafe {
                        libc::signal(signum as libc::c_int, flag_signal_handler as *const () as libc::sighandler_t);
                    }
                }

                extern "C" fn flag_signal_handler(sig: libc::c_int) {
                    if sig >= 0 && sig < 64 {
                        PENDING_SIGNALS.fetch_or(1u64 << sig, Ordering::SeqCst);
                    }
                    // Re-arm (System V signal semantics reset to SIG_DFL after delivery)
                    unsafe {
                        libc::signal(sig, flag_signal_handler as *const () as libc::sighandler_t);
                    }
                }
            }
            Ok(prev)
        })),
        ("getsignal", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("getsignal() requires 1 argument")); }
            let signum = args[0].to_int()?;
            let handler = SIGNAL_HANDLERS.with(|h| {
                h.borrow().get(&signum).cloned().unwrap_or_else(|| PyObject::int(0))
            });
            Ok(handler)
        })),
        ("raise_signal", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("raise_signal() requires 1 argument")); }
            let signum = args[0].to_int()?;
            // Dispatch Python handler directly if registered
            let handler = SIGNAL_HANDLERS.with(|h| {
                h.borrow().get(&signum).cloned()
            });
            if let Some(ref h) = handler {
                let h_int = h.to_int().unwrap_or(-1);
                if h_int != 0 && h_int != 1 {
                    // It's a Python callable — invoke via deferred calls (VM will execute)
                    let call_args = vec![PyObject::int(signum), PyObject::none()];
                    match &h.payload {
                        PyObjectPayload::NativeFunction(nf) => {
                            return (nf.func)(&call_args);
                        }
                        PyObjectPayload::NativeClosure(nc) => {
                            return (nc.func)(&call_args);
                        }
                        _ => {
                            // Python function — use deferred call mechanism
                            DEFERRED_CALLS.with(|dc| {
                                dc.borrow_mut().push((h.clone(), call_args));
                            });
                            return Ok(PyObject::none());
                        }
                    }
                }
            }
            // No Python handler or SIG_DFL/SIG_IGN — raise through OS
            #[cfg(unix)]
            unsafe { libc::raise(signum as libc::c_int); }
            Ok(PyObject::none())
        })),
        ("alarm", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("alarm() requires 1 argument")); }
            let secs = args[0].to_int()? as u32;
            #[cfg(unix)]
            let remaining = unsafe { libc::alarm(secs) };
            #[cfg(not(unix))]
            {
                let _ = secs;
                return Err(PyException::os_error("alarm() is not supported on this platform"));
            }
            #[cfg(unix)]
            Ok(PyObject::int(remaining as i64))
        })),
        ("pause", make_builtin(|_| {
            #[cfg(unix)]
            { unsafe { libc::pause(); } Ok(PyObject::none()) }
            #[cfg(not(unix))]
            Err(PyException::os_error("pause() is not supported on this platform"))
        })),
        ("set_wakeup_fd", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::int(-1)); }
            let _fd = args[0].to_int()?;
            Ok(PyObject::int(-1)) // return previous fd (-1 = none)
        })),
        ("valid_signals", make_builtin(|_| {
            // Return set of valid signal numbers
            let mut sigs = IndexMap::new();
            for i in 1..32i64 {
                if i != 9 && i != 19 { // SIGKILL and SIGSTOP can't be caught
                    let obj = PyObject::int(i);
                    let key = HashableKey::Int(ferrython_core::types::PyInt::Small(i));
                    sigs.insert(key, obj);
                }
            }
            Ok(PyObject::set(sigs))
        })),
        ("strsignal", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("strsignal() requires 1 argument")); }
            let signum = args[0].to_int()?;
            let name = match signum {
                1 => "Hangup", 2 => "Interrupt", 3 => "Quit",
                4 => "Illegal instruction", 6 => "Aborted", 7 => "Bus error",
                8 => "Floating point exception", 9 => "Killed",
                10 => "User defined signal 1", 11 => "Segmentation fault",
                12 => "User defined signal 2", 13 => "Broken pipe",
                14 => "Alarm clock", 15 => "Terminated",
                17 => "Child exited", 18 => "Continued", 19 => "Stopped",
                28 => "Window changed",
                _ => "Unknown signal",
            };
            Ok(PyObject::str_val(CompactString::from(name)))
        })),
        ("Signals", PyObject::none()),
        ("Handlers", PyObject::none()),
    ])
}

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
            attrs.insert(CompactString::from("pid"), PyObject::int(std::process::id() as i64));
            attrs.insert(CompactString::from("exitcode"), PyObject::none());

            let alive = Rc::new(PyCell::new(false));

            let tgt = target.clone();
            let targs = proc_args.clone();
            let a1 = alive.clone();
            attrs.insert(CompactString::from("start"), PyObject::native_closure(
                "start", move |_: &[PyObjectRef]| {
                    *a1.write() = true;
                    if !matches!(&tgt.payload, PyObjectPayload::None) {
                        let call_args: Vec<PyObjectRef> = match &targs.payload {
                            PyObjectPayload::Tuple(items) => items.clone(),
                            PyObjectPayload::List(items) => items.read().clone(),
                            _ => vec![],
                        };
                        match &tgt.payload {
                            PyObjectPayload::NativeFunction(nf) => { let _ = (nf.func)(&call_args); }
                            PyObjectPayload::NativeClosure(nc) => { let _ = (nc.func)(&call_args); }
                            _ => { push_deferred_call(tgt.clone(), call_args); }
                        }
                    }
                    *a1.write() = false;
                    Ok(PyObject::none())
                }));
            attrs.insert(CompactString::from("join"), {
                let a_join = alive.clone();
                PyObject::native_closure("join", move |args: &[PyObjectRef]| {
                    // Wait for process to complete; since start() runs synchronously,
                    // process is typically already done. Support optional timeout.
                    let timeout = args.first()
                        .and_then(|a| if matches!(&a.payload, PyObjectPayload::None) { None } else { a.to_float().ok() });
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
            attrs.insert(CompactString::from("is_alive"), PyObject::native_closure(
                "is_alive", move |_: &[PyObjectRef]| { Ok(PyObject::bool_val(*a2.read())) }));
            let a3 = alive.clone();
            attrs.insert(CompactString::from("terminate"), PyObject::native_closure(
                "terminate", move |_: &[PyObjectRef]| {
                    *a3.write() = false;
                    Ok(PyObject::none())
                }));
            let a4 = alive.clone();
            attrs.insert(CompactString::from("kill"), PyObject::native_closure(
                "kill", move |_: &[PyObjectRef]| {
                    *a4.write() = false;
                    Ok(PyObject::none())
                }));
        }
        Ok(inst)
    });

    // Pool(processes=) — thread pool with state tracking
    let pool_cls = PyObject::class(CompactString::from("Pool"), vec![], IndexMap::new());
    let plc = pool_cls.clone();
    let pool_fn = PyObject::native_closure("Pool", move |args: &[PyObjectRef]| {
        let processes = if !args.is_empty() { args[0].as_int().unwrap_or(1) } else { 1 };
        let inst = PyObject::instance(plc.clone());
        let closed = Rc::new(PyCell::new(false));
        let terminated = Rc::new(PyCell::new(false));
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(CompactString::from("_processes"), PyObject::int(processes));
            let cl1 = closed.clone();
            let tm1 = terminated.clone();
            attrs.insert(CompactString::from("map"), PyObject::native_closure("map", move |args: &[PyObjectRef]| {
                // Pool.map(func, iterable) — execute func(item) for each item sequentially
                if *cl1.read() || *tm1.read() {
                    return Err(PyException::value_error("Pool not running"));
                }
                if args.len() < 2 { return Err(PyException::type_error("map() requires func and iterable")); }
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
                            ferrython_core::error::request_vm_call(func.clone(), vec![item.clone()]);
                            has_deferred = true;
                            results.push(PyObject::none());
                        }
                    }
                }
                if has_deferred {
                    ferrython_core::error::set_collect_vm_call_results(true);
                }
                Ok(PyObject::list(results))
            }));
            let cl2 = closed.clone();
            let tm2 = terminated.clone();
            attrs.insert(CompactString::from("apply"), PyObject::native_closure("apply", move |args: &[PyObjectRef]| {
                // Pool.apply(func, args=()) — call func with args
                if *cl2.read() || *tm2.read() {
                    return Err(PyException::value_error("Pool not running"));
                }
                if args.is_empty() { return Err(PyException::type_error("apply() requires func")); }
                let func = &args[0];
                let call_args: Vec<PyObjectRef> = if args.len() > 1 {
                    args[1].to_list().unwrap_or_default()
                } else { vec![] };
                match &func.payload {
                    PyObjectPayload::NativeFunction(nf) => (nf.func)(&call_args),
                    PyObjectPayload::NativeClosure(nc) => (nc.func)(&call_args),
                    _ => {
                        push_deferred_call(func.clone(), call_args);
                        Ok(PyObject::none())
                    }
                }
            }));
            let cl3 = closed.clone();
            let tm3 = terminated.clone();
            attrs.insert(CompactString::from("apply_async"), PyObject::native_closure("apply_async", move |args: &[PyObjectRef]| {
                // apply_async returns an AsyncResult; in our model, execute immediately
                if *cl3.read() || *tm3.read() {
                    return Err(PyException::value_error("Pool not running"));
                }
                if args.is_empty() { return Err(PyException::type_error("apply_async() requires func")); }
                let func = &args[0];
                let call_args: Vec<PyObjectRef> = if args.len() > 1 {
                    args[1].to_list().unwrap_or_default()
                } else { vec![] };
                let result = match &func.payload {
                    PyObjectPayload::NativeFunction(nf) => (nf.func)(&call_args)?,
                    PyObjectPayload::NativeClosure(nc) => (nc.func)(&call_args)?,
                    _ => { push_deferred_call(func.clone(), call_args); PyObject::none() }
                };
                // Return an AsyncResult-like object with get() method
                let cls = PyObject::class(CompactString::from("AsyncResult"), vec![], IndexMap::new());
                let async_inst = PyObject::instance(cls);
                if let PyObjectPayload::Instance(ref d) = async_inst.payload {
                    let r = result.clone();
                    d.attrs.write().insert(CompactString::from("get"), PyObject::native_closure(
                        "get", move |_: &[PyObjectRef]| Ok(r.clone())));
                    d.attrs.write().insert(CompactString::from("ready"), make_builtin(|_| Ok(PyObject::bool_val(true))));
                    d.attrs.write().insert(CompactString::from("successful"), make_builtin(|_| Ok(PyObject::bool_val(true))));
                    d.attrs.write().insert(CompactString::from("wait"), make_builtin(|_| Ok(PyObject::none())));
                }
                Ok(async_inst)
            }));
            let cl4 = closed.clone();
            attrs.insert(CompactString::from("close"), PyObject::native_closure(
                "close", move |_: &[PyObjectRef]| {
                    *cl4.write() = true;
                    Ok(PyObject::none())
                }));
            let cl5 = closed.clone();
            attrs.insert(CompactString::from("join"), PyObject::native_closure(
                "join", move |_: &[PyObjectRef]| {
                    if !*cl5.read() {
                        return Err(PyException::value_error("Pool is still running"));
                    }
                    // All work is synchronous, so join is immediate
                    Ok(PyObject::none())
                }));
            let tm4 = terminated.clone();
            let cl6 = closed.clone();
            attrs.insert(CompactString::from("terminate"), PyObject::native_closure(
                "terminate", move |_: &[PyObjectRef]| {
                    *tm4.write() = true;
                    *cl6.write() = true;
                    Ok(PyObject::none())
                }));
            attrs.insert(CompactString::from("__enter__"), {
                let ir = inst.clone();
                PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| Ok(ir.clone()))
            });
            attrs.insert(CompactString::from("__exit__"), make_builtin(|_| Ok(PyObject::bool_val(false))));
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
            attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from("MainProcess")));
            attrs.insert(CompactString::from("pid"), PyObject::int(std::process::id() as i64));
            attrs.insert(CompactString::from("daemon"), PyObject::bool_val(false));
            attrs.insert(CompactString::from("exitcode"), PyObject::none());
            attrs.insert(CompactString::from("is_alive"), make_builtin(|_| Ok(PyObject::bool_val(true))));
        }
        Ok(inst)
    });

    make_module("multiprocessing", vec![
        ("Process", process_fn),
        ("Pool", pool_fn),
        ("cpu_count", cpu_count_fn),
        ("current_process", current_process_fn),
        ("Queue", make_builtin(|_| {
            let cls = PyObject::class(CompactString::from("Queue"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let items: Arc<std::sync::Mutex<std::collections::VecDeque<PyObjectRef>>> = Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
                let mut attrs = d.attrs.write();
                let q1 = items.clone();
                attrs.insert(CompactString::from("put"), PyObject::native_closure("put", move |args: &[PyObjectRef]| {
                    if args.is_empty() { return Err(PyException::type_error("put() requires 1 argument")); }
                    q1.lock().unwrap().push_back(args[0].clone());
                    Ok(PyObject::none())
                }));
                let q2 = items.clone();
                attrs.insert(CompactString::from("get"), PyObject::native_closure("get", move |_: &[PyObjectRef]| {
                    q2.lock().unwrap().pop_front().ok_or_else(|| {
                        PyException::new(ferrython_core::error::ExceptionKind::RuntimeError, "Queue is empty")
                    })
                }));
                let q3 = items.clone();
                attrs.insert(CompactString::from("empty"), PyObject::native_closure("empty", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(q3.lock().unwrap().is_empty()))
                }));
                let q4 = items.clone();
                attrs.insert(CompactString::from("qsize"), PyObject::native_closure("qsize", move |_: &[PyObjectRef]| {
                    Ok(PyObject::int(q4.lock().unwrap().len() as i64))
                }));
                let q5 = items.clone();
                attrs.insert(CompactString::from("full"), PyObject::native_closure("full", move |_: &[PyObjectRef]| {
                    let _ = q5; // unbounded → never full
                    Ok(PyObject::bool_val(false))
                }));
                attrs.insert(CompactString::from("put_nowait"), {
                    let q = items.clone();
                    PyObject::native_closure("put_nowait", move |args: &[PyObjectRef]| {
                        if args.is_empty() { return Err(PyException::type_error("put_nowait() requires 1 argument")); }
                        q.lock().unwrap().push_back(args[0].clone());
                        Ok(PyObject::none())
                    })
                });
                attrs.insert(CompactString::from("get_nowait"), {
                    let q = items.clone();
                    PyObject::native_closure("get_nowait", move |_: &[PyObjectRef]| {
                        q.lock().unwrap().pop_front().ok_or_else(|| {
                            PyException::new(ferrython_core::error::ExceptionKind::RuntimeError, "Queue is empty")
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
        })),
        ("Lock", make_builtin(|_| {
            let cls = PyObject::class(CompactString::from("Lock"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let locked = Arc::new(std::sync::Mutex::new(false));
                let mut attrs = d.attrs.write();
                let l1 = locked.clone();
                attrs.insert(CompactString::from("acquire"), PyObject::native_closure("acquire", move |_: &[PyObjectRef]| {
                    *l1.lock().unwrap() = true;
                    Ok(PyObject::bool_val(true))
                }));
                let l2 = locked.clone();
                attrs.insert(CompactString::from("release"), PyObject::native_closure("release", move |_: &[PyObjectRef]| {
                    *l2.lock().unwrap() = false;
                    Ok(PyObject::none())
                }));
                let l3 = locked.clone();
                attrs.insert(CompactString::from("locked"), PyObject::native_closure("locked", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(*l3.lock().unwrap()))
                }));
                let li = inst.clone();
                attrs.insert(CompactString::from("__enter__"), PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| Ok(li.clone())));
                attrs.insert(CompactString::from("__exit__"), {
                    let l = locked.clone();
                    PyObject::native_closure("__exit__", move |_: &[PyObjectRef]| {
                        *l.lock().unwrap() = false;
                        Ok(PyObject::bool_val(false))
                    })
                });
            }
            Ok(inst)
        })),
        ("Value", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("Value() requires 2 arguments")); }
            Ok(args[1].clone())
        })),
        ("Array", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("Array() requires 2 arguments")); }
            Ok(args[1].clone())
        })),
        ("Manager", make_builtin(|_| {
            let cls = PyObject::class(CompactString::from("SyncManager"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut attrs = d.attrs.write();
                // Manager.dict() -> shared dict
                attrs.insert(CompactString::from("dict"), make_builtin(|_| {
                    Ok(PyObject::dict(IndexMap::new()))
                }));
                // Manager.list() -> shared list
                attrs.insert(CompactString::from("list"), make_builtin(|_| {
                    Ok(PyObject::list(vec![]))
                }));
                // Manager.Value(typecode, value) -> value wrapper
                attrs.insert(CompactString::from("Value"), make_builtin(|args: &[PyObjectRef]| {
                    if args.len() < 2 { return Err(PyException::type_error("Value() requires 2 arguments")); }
                    Ok(args[1].clone())
                }));
                // Manager.Lock() -> Lock
                attrs.insert(CompactString::from("Lock"), make_builtin(|_| {
                    let lock_cls = PyObject::class(CompactString::from("Lock"), vec![], IndexMap::new());
                    let lock_inst = PyObject::instance(lock_cls);
                    if let PyObjectPayload::Instance(ref ld) = lock_inst.payload {
                        let locked = Arc::new(std::sync::Mutex::new(false));
                        let mut la = ld.attrs.write();
                        let l1 = locked.clone();
                        la.insert(CompactString::from("acquire"), PyObject::native_closure("acquire", move |_: &[PyObjectRef]| {
                            *l1.lock().unwrap() = true;
                            Ok(PyObject::bool_val(true))
                        }));
                        let l2 = locked.clone();
                        la.insert(CompactString::from("release"), PyObject::native_closure("release", move |_: &[PyObjectRef]| {
                            *l2.lock().unwrap() = false;
                            Ok(PyObject::none())
                        }));
                    }
                    Ok(lock_inst)
                }));
                // Manager.Namespace() -> namespace object
                attrs.insert(CompactString::from("Namespace"), make_builtin(|_| {
                    let ns_cls = PyObject::class(CompactString::from("Namespace"), vec![], IndexMap::new());
                    Ok(PyObject::instance(ns_cls))
                }));
                // Manager.Event() -> Event
                attrs.insert(CompactString::from("Event"), make_builtin(|_| {
                    let ev_cls = PyObject::class(CompactString::from("Event"), vec![], IndexMap::new());
                    let ev_inst = PyObject::instance(ev_cls);
                    if let PyObjectPayload::Instance(ref ed) = ev_inst.payload {
                        let flag = Arc::new(std::sync::Mutex::new(false));
                        let mut ea = ed.attrs.write();
                        let f1 = flag.clone();
                        ea.insert(CompactString::from("set"), PyObject::native_closure("set", move |_: &[PyObjectRef]| {
                            *f1.lock().unwrap() = true;
                            Ok(PyObject::none())
                        }));
                        let f2 = flag.clone();
                        ea.insert(CompactString::from("clear"), PyObject::native_closure("clear", move |_: &[PyObjectRef]| {
                            *f2.lock().unwrap() = false;
                            Ok(PyObject::none())
                        }));
                        let f3 = flag.clone();
                        ea.insert(CompactString::from("is_set"), PyObject::native_closure("is_set", move |_: &[PyObjectRef]| {
                            Ok(PyObject::bool_val(*f3.lock().unwrap()))
                        }));
                        let f4 = flag.clone();
                        ea.insert(CompactString::from("wait"), PyObject::native_closure("wait", move |_: &[PyObjectRef]| {
                            Ok(PyObject::bool_val(*f4.lock().unwrap()))
                        }));
                    }
                    Ok(ev_inst)
                }));
                // Context manager support
                let ir = inst.clone();
                attrs.insert(CompactString::from("__enter__"), PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| Ok(ir.clone())));
                attrs.insert(CompactString::from("__exit__"), make_builtin(|_| Ok(PyObject::bool_val(false))));
                attrs.insert(CompactString::from("shutdown"), make_builtin(|_| Ok(PyObject::none())));
            }
            Ok(inst)
        })),
        ("Pipe", make_builtin(|_| Ok(PyObject::tuple(vec![PyObject::none(), PyObject::none()])))),
        ("Event", make_builtin(|_| {
            let cls = PyObject::class(CompactString::from("Event"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let flag = Arc::new(std::sync::Mutex::new(false));
                let mut attrs = d.attrs.write();
                let f1 = flag.clone();
                attrs.insert(CompactString::from("set"), PyObject::native_closure("set", move |_: &[PyObjectRef]| {
                    *f1.lock().unwrap() = true;
                    Ok(PyObject::none())
                }));
                let f2 = flag.clone();
                attrs.insert(CompactString::from("clear"), PyObject::native_closure("clear", move |_: &[PyObjectRef]| {
                    *f2.lock().unwrap() = false;
                    Ok(PyObject::none())
                }));
                let f3 = flag.clone();
                attrs.insert(CompactString::from("is_set"), PyObject::native_closure("is_set", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(*f3.lock().unwrap()))
                }));
                let f4 = flag.clone();
                attrs.insert(CompactString::from("wait"), PyObject::native_closure("wait", move |args: &[PyObjectRef]| {
                    let timeout_secs = args.first()
                        .and_then(|a| if matches!(&a.payload, PyObjectPayload::None) { None } else { a.to_float().ok() });
                    if *f4.lock().unwrap() {
                        return Ok(PyObject::bool_val(true));
                    }
                    if let Some(t) = timeout_secs {
                        std::thread::sleep(std::time::Duration::from_secs_f64(t));
                    }
                    Ok(PyObject::bool_val(*f4.lock().unwrap()))
                }));
            }
            Ok(inst)
        })),
        ("Semaphore", make_builtin(|args: &[PyObjectRef]| {
            let value = args.first().and_then(|a| a.as_int()).unwrap_or(1);
            let cls = PyObject::class(CompactString::from("Semaphore"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let count = Arc::new(std::sync::Mutex::new(value));
                let mut attrs = d.attrs.write();
                let c1 = count.clone();
                attrs.insert(CompactString::from("acquire"), PyObject::native_closure("acquire", move |_: &[PyObjectRef]| {
                    let mut c = c1.lock().unwrap();
                    if *c > 0 { *c -= 1; Ok(PyObject::bool_val(true)) }
                    else { Ok(PyObject::bool_val(false)) }
                }));
                let c2 = count.clone();
                attrs.insert(CompactString::from("release"), PyObject::native_closure("release", move |_: &[PyObjectRef]| {
                    *c2.lock().unwrap() += 1;
                    Ok(PyObject::none())
                }));
                let si = inst.clone();
                attrs.insert(CompactString::from("__enter__"), PyObject::native_closure("__enter__", move |_: &[PyObjectRef]| Ok(si.clone())));
                let c3 = count.clone();
                attrs.insert(CompactString::from("__exit__"), PyObject::native_closure("__exit__", move |_: &[PyObjectRef]| {
                    *c3.lock().unwrap() += 1;
                    Ok(PyObject::bool_val(false))
                }));
            }
            Ok(inst)
        })),
    ])
}

// ── selectors module ────────────────────────────────────────────────

pub fn create_selectors_module() -> PyObjectRef {
    // SelectorKey namedtuple-like
    let selector_key_fn = make_builtin(|args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("SelectorKey"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut attrs = d.attrs.write();
            attrs.insert(CompactString::from("fileobj"), args.first().cloned().unwrap_or_else(PyObject::none));
            attrs.insert(CompactString::from("fd"), args.get(1).cloned().unwrap_or_else(|| PyObject::int(0)));
            attrs.insert(CompactString::from("events"), args.get(2).cloned().unwrap_or_else(|| PyObject::int(0)));
            attrs.insert(CompactString::from("data"), args.get(3).cloned().unwrap_or_else(PyObject::none));
        }
        Ok(inst)
    });

    // Create selector constructor with register/unregister/select/close/get_map
    fn make_selector(name: &str) -> PyObjectRef {
        let cls_name = CompactString::from(name);
        let cls = PyObject::class(cls_name, vec![], IndexMap::new());
        let c = cls.clone();
        PyObject::native_closure(name, move |_args: &[PyObjectRef]| {
            let inst = PyObject::instance(c.clone());
            let registry: Rc<PyCell<IndexMap<i64, PyObjectRef>>> = Rc::new(PyCell::new(IndexMap::new()));
            let inst_ref = inst.clone();

            if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
                let mut attrs = inst_data.attrs.write();

                // register(fileobj, events, data=None) -> SelectorKey
                let reg1 = registry.clone();
                attrs.insert(CompactString::from("register"), PyObject::native_closure(
                    "register", move |args: &[PyObjectRef]| {
                        if args.is_empty() { return Err(PyException::type_error("register() requires at least 1 argument")); }
                        let fileobj = args[0].clone();
                        let events = args.get(1).and_then(|a| a.as_int()).unwrap_or(0);
                        let data = args.get(2).cloned().unwrap_or_else(PyObject::none);
                        let fd = fileobj.as_int().unwrap_or(0);

                        let key_cls = PyObject::class(CompactString::from("SelectorKey"), vec![], IndexMap::new());
                        let key = PyObject::instance(key_cls);
                        if let PyObjectPayload::Instance(ref d) = key.payload {
                            let mut ka = d.attrs.write();
                            ka.insert(CompactString::from("fileobj"), fileobj);
                            ka.insert(CompactString::from("fd"), PyObject::int(fd));
                            ka.insert(CompactString::from("events"), PyObject::int(events));
                            ka.insert(CompactString::from("data"), data);
                        }
                        reg1.write().insert(fd, key.clone());
                        Ok(key)
                    }));

                // unregister(fileobj) -> SelectorKey
                let reg2 = registry.clone();
                attrs.insert(CompactString::from("unregister"), PyObject::native_closure(
                    "unregister", move |args: &[PyObjectRef]| {
                        if args.is_empty() { return Err(PyException::type_error("unregister() requires 1 argument")); }
                        let fd = args[0].as_int().unwrap_or(0);
                        let key = reg2.write().swap_remove(&fd).unwrap_or_else(PyObject::none);
                        Ok(key)
                    }));

                // modify(fileobj, events, data=None) -> SelectorKey
                let reg2b = registry.clone();
                attrs.insert(CompactString::from("modify"), PyObject::native_closure(
                    "modify", move |args: &[PyObjectRef]| {
                        if args.is_empty() { return Err(PyException::type_error("modify() requires at least 1 argument")); }
                        let fileobj = args[0].clone();
                        let events = args.get(1).and_then(|a| a.as_int()).unwrap_or(0);
                        let data = args.get(2).cloned().unwrap_or_else(PyObject::none);
                        let fd = fileobj.as_int().unwrap_or(0);

                        let key_cls = PyObject::class(CompactString::from("SelectorKey"), vec![], IndexMap::new());
                        let key = PyObject::instance(key_cls);
                        if let PyObjectPayload::Instance(ref d) = key.payload {
                            let mut ka = d.attrs.write();
                            ka.insert(CompactString::from("fileobj"), fileobj);
                            ka.insert(CompactString::from("fd"), PyObject::int(fd));
                            ka.insert(CompactString::from("events"), PyObject::int(events));
                            ka.insert(CompactString::from("data"), data);
                        }
                        reg2b.write().insert(fd, key.clone());
                        Ok(key)
                    }));

                // select(timeout=None) -> list of (key, events)
                let reg3 = registry.clone();
                attrs.insert(CompactString::from("select"), PyObject::native_closure(
                    "select", move |args: &[PyObjectRef]| {
                        let timeout_ms: i32 = if let Some(t_arg) = args.first() {
                            if matches!(&t_arg.payload, PyObjectPayload::None) {
                                -1 // block forever
                            } else if let Some(t) = t_arg.as_int() {
                                (t * 1000) as i32
                            } else if let Ok(t) = t_arg.to_float() {
                                (t * 1000.0) as i32
                            } else {
                                -1
                            }
                        } else {
                            -1
                        };

                        let r = reg3.read();

                        #[cfg(unix)]
                        {
                            if r.is_empty() {
                                return Ok(PyObject::list(vec![]));
                            }

                            // Build pollfd array from registered fds
                            let mut pollfds: Vec<libc::pollfd> = Vec::with_capacity(r.len());
                            let mut keys: Vec<(&i64, &PyObjectRef)> = Vec::with_capacity(r.len());

                            for (fd, key) in r.iter() {
                                let events_val = if let PyObjectPayload::Instance(ref d) = key.payload {
                                    d.attrs.read().get(&CompactString::from("events")).and_then(|e| e.as_int()).unwrap_or(0)
                                } else { 0 };
                                // Map EVENT_READ (1) -> POLLIN, EVENT_WRITE (2) -> POLLOUT
                                let mut poll_events: i16 = 0;
                                if events_val & 1 != 0 { poll_events |= libc::POLLIN as i16; }
                                if events_val & 2 != 0 { poll_events |= libc::POLLOUT as i16; }
                                pollfds.push(libc::pollfd { fd: *fd as i32, events: poll_events, revents: 0 });
                                keys.push((fd, key));
                            }

                            let ret = unsafe {
                                libc::poll(pollfds.as_mut_ptr(), pollfds.len() as libc::nfds_t, timeout_ms)
                            };

                            if ret < 0 {
                                return Err(PyException::os_error("select: poll() failed"));
                            }

                            // Only return keys where revents is non-zero
                            let results: Vec<PyObjectRef> = pollfds.iter().enumerate()
                                .filter(|(_, pfd)| pfd.revents != 0)
                                .map(|(i, pfd)| {
                                    let key = keys[i].1.clone();
                                    // Map revents back to EVENT_READ/EVENT_WRITE
                                    let mut ready_events: i64 = 0;
                                    let rev = pfd.revents;
                                    if rev & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
                                        ready_events |= 1; // EVENT_READ
                                    }
                                    if rev & libc::POLLOUT != 0 {
                                        ready_events |= 2; // EVENT_WRITE
                                    }
                                    PyObject::tuple(vec![key, PyObject::int(ready_events)])
                                })
                                .collect();
                            return Ok(PyObject::list(results));
                        }

                        #[cfg(not(unix))]
                        {
                            let _ = timeout_ms;
                            let results: Vec<PyObjectRef> = r.values().map(|key| {
                                let events = if let PyObjectPayload::Instance(ref d) = key.payload {
                                    d.attrs.read().get(&CompactString::from("events")).cloned().unwrap_or_else(|| PyObject::int(0))
                                } else {
                                    PyObject::int(0)
                                };
                                PyObject::tuple(vec![key.clone(), events])
                            }).collect();
                            Ok(PyObject::list(results))
                        }
                    }));

                // close()
                let reg4 = registry.clone();
                attrs.insert(CompactString::from("close"), PyObject::native_closure(
                    "close", move |_: &[PyObjectRef]| {
                        reg4.write().clear();
                        Ok(PyObject::none())
                    }));

                // get_map()
                attrs.insert(CompactString::from("get_map"), PyObject::native_closure(
                    "get_map", move |_: &[PyObjectRef]| {
                        Ok(PyObject::dict(IndexMap::new()))
                    }));

                // Context manager
                let ir = inst_ref.clone();
                attrs.insert(CompactString::from("__enter__"), PyObject::native_closure(
                    "__enter__", move |_: &[PyObjectRef]| { Ok(ir.clone()) }));
                let reg6 = registry.clone();
                attrs.insert(CompactString::from("__exit__"), PyObject::native_closure(
                    "__exit__", move |_: &[PyObjectRef]| {
                        reg6.write().clear();
                        Ok(PyObject::bool_val(false))
                    }));
            }
            Ok(inst)
        })
    }

    make_module("selectors", vec![
        ("DefaultSelector", make_selector("DefaultSelector")),
        ("SelectSelector", make_selector("SelectSelector")),
        ("PollSelector", make_selector("PollSelector")),
        ("EpollSelector", make_selector("EpollSelector")),
        ("KqueueSelector", make_selector("KqueueSelector")),
        ("SelectorKey", selector_key_fn),
        ("EVENT_READ", PyObject::int(1)),
        ("EVENT_WRITE", PyObject::int(2)),
    ])
}

// concurrent.futures is now implemented in pure Python: stdlib/Lib/concurrent/futures.py

// ── select module ──

pub fn create_select_module() -> PyObjectRef {
    // select.select(rlist, wlist, xlist[, timeout])
    let select_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 3 {
            return Err(PyException::type_error(
                "select() requires at least 3 arguments",
            ));
        }
        // Extract file descriptors from the lists
        let extract_fds = |obj: &PyObjectRef| -> Vec<(i32, PyObjectRef)> {
            match &obj.payload {
                PyObjectPayload::List(items) => {
                    items.read().iter().map(|item| {
                        let fd = if let Some(fileno) = item.get_attr("fileno") {
                            match &fileno.payload {
                                PyObjectPayload::NativeFunction(nf) => {
                                    (nf.func)(&[item.clone()]).ok().and_then(|v| v.as_int()).unwrap_or(-1) as i32
                                }
                                PyObjectPayload::NativeClosure(nc) => {
                                    (nc.func)(&[item.clone()]).ok().and_then(|v| v.as_int()).unwrap_or(-1) as i32
                                }
                                _ => item.as_int().unwrap_or(-1) as i32,
                            }
                        } else {
                            item.as_int().unwrap_or(-1) as i32
                        };
                        (fd, item.clone())
                    }).collect()
                }
                _ => vec![],
            }
        };

        let rlist_fds = extract_fds(&args[0]);
        let wlist_fds = extract_fds(&args[1]);
        let xlist_fds = extract_fds(&args[2]);

        // Timeout in milliseconds (None = -1 = block forever)
        let timeout_ms: i32 = if args.len() > 3 && !matches!(&args[3].payload, PyObjectPayload::None) {
            if let Some(t) = args[3].as_int() {
                (t * 1000) as i32
            } else if let Ok(t) = args[3].to_float() {
                (t * 1000.0) as i32
            } else { -1 }
        } else { -1 };

        #[cfg(unix)]
        {
            // Use libc::poll for real fd polling
            let mut pollfds: Vec<libc::pollfd> = Vec::new();
            let mut fd_map: Vec<(usize, PyObjectRef)> = Vec::new(); // index -> original object

            // rlist fds -> POLLIN
            for (fd, obj) in &rlist_fds {
                if *fd >= 0 {
                    pollfds.push(libc::pollfd { fd: *fd, events: libc::POLLIN, revents: 0 });
                    fd_map.push((pollfds.len() - 1, obj.clone()));
                }
            }
            let rlist_count = pollfds.len();
            // wlist fds -> POLLOUT
            for (fd, obj) in &wlist_fds {
                if *fd >= 0 {
                    pollfds.push(libc::pollfd { fd: *fd, events: libc::POLLOUT, revents: 0 });
                    fd_map.push((pollfds.len() - 1, obj.clone()));
                }
            }
            let wlist_count = pollfds.len() - rlist_count;
            // xlist fds -> POLLPRI
            for (fd, obj) in &xlist_fds {
                if *fd >= 0 {
                    pollfds.push(libc::pollfd { fd: *fd, events: libc::POLLPRI, revents: 0 });
                    fd_map.push((pollfds.len() - 1, obj.clone()));
                }
            }

            if pollfds.is_empty() {
                // No valid fds — sleep for timeout if given
                if timeout_ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(timeout_ms as u64));
                }
                return Ok(PyObject::tuple(vec![
                    PyObject::list(vec![]), PyObject::list(vec![]), PyObject::list(vec![]),
                ]));
            }

            let ret = unsafe { libc::poll(pollfds.as_mut_ptr(), pollfds.len() as libc::nfds_t, timeout_ms) };
            if ret < 0 {
                return Err(PyException::os_error("select.select: poll() failed"));
            }

            let mut readable = Vec::new();
            let mut writable = Vec::new();
            let mut exceptional = Vec::new();

            for (i, pfd) in pollfds.iter().enumerate() {
                if pfd.revents != 0 {
                    // Find original object for this fd
                    let obj = fd_map.iter().find(|(idx, _)| *idx == i).map(|(_, o)| o.clone());
                    if let Some(o) = obj {
                        if i < rlist_count && (pfd.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR)) != 0 {
                            readable.push(o);
                        } else if i >= rlist_count && i < rlist_count + wlist_count && (pfd.revents & libc::POLLOUT) != 0 {
                            writable.push(o);
                        } else if i >= rlist_count + wlist_count && (pfd.revents & libc::POLLPRI) != 0 {
                            exceptional.push(o);
                        }
                    }
                }
            }

            return Ok(PyObject::tuple(vec![
                PyObject::list(readable), PyObject::list(writable), PyObject::list(exceptional),
            ]));
        }

        #[cfg(not(unix))]
        {
            // Fallback: return rlist as readable (same as before)
            let rlist: Vec<PyObjectRef> = rlist_fds.into_iter().map(|(_, obj)| obj).collect();
            Ok(PyObject::tuple(vec![
                PyObject::list(rlist), PyObject::list(vec![]), PyObject::list(vec![]),
            ]))
        }
    });

    let poll_cls = {
        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("register"), make_builtin(|_args| Ok(PyObject::none())));
        ns.insert(CompactString::from("unregister"), make_builtin(|_args| Ok(PyObject::none())));
        ns.insert(CompactString::from("modify"), make_builtin(|_args| Ok(PyObject::none())));
        ns.insert(CompactString::from("poll"), make_builtin(|_args| Ok(PyObject::list(vec![]))));
        PyObject::class(CompactString::from("poll"), vec![], ns)
    };

    let poll_fn = {
        let poll_cls = poll_cls.clone();
        PyObject::native_closure("poll", move |_args: &[PyObjectRef]| {
            // Create a poll instance with shared fd registry
            let registered_fds: Rc<PyCell<Vec<(i32, i16)>>> = Rc::new(PyCell::new(Vec::new()));
            let inst = PyObject::instance(poll_cls.clone());
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                let fds = registered_fds.clone();
                attrs.insert(CompactString::from("register"), PyObject::native_closure(
                    "register", move |args: &[PyObjectRef]| {
                        if args.is_empty() { return Ok(PyObject::none()); }
                        let fd = args[0].as_int().unwrap_or(-1) as i32;
                        let events = if args.len() > 1 { args[1].as_int().unwrap_or(0x001) as i16 } else { 0x001 | 0x002 | 0x004 };
                        let mut fds_w = fds.write();
                        fds_w.retain(|(f, _)| *f != fd);
                        fds_w.push((fd, events));
                        Ok(PyObject::none())
                    }
                ));
                let fds2 = registered_fds.clone();
                attrs.insert(CompactString::from("unregister"), PyObject::native_closure(
                    "unregister", move |args: &[PyObjectRef]| {
                        if let Some(fd) = args.first().and_then(|a| a.as_int()) {
                            fds2.write().retain(|(f, _)| *f != fd as i32);
                        }
                        Ok(PyObject::none())
                    }
                ));
                let fds3 = registered_fds.clone();
                attrs.insert(CompactString::from("modify"), PyObject::native_closure(
                    "modify", move |args: &[PyObjectRef]| {
                        if args.len() >= 2 {
                            let fd = args[0].as_int().unwrap_or(-1) as i32;
                            let events = args[1].as_int().unwrap_or(0x001) as i16;
                            let mut fds_w = fds3.write();
                            if let Some(entry) = fds_w.iter_mut().find(|(f, _)| *f == fd) {
                                entry.1 = events;
                            }
                        }
                        Ok(PyObject::none())
                    }
                ));
                let fds4 = registered_fds.clone();
                attrs.insert(CompactString::from("poll"), PyObject::native_closure(
                    "poll", move |args: &[PyObjectRef]| {
                        let timeout_ms: i32 = if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None) {
                            args[0].as_int().unwrap_or(-1) as i32
                        } else { -1 };
                        let fds_r = fds4.read();
                        if fds_r.is_empty() {
                            return Ok(PyObject::list(vec![]));
                        }
                        #[cfg(unix)]
                        {
                            let mut pollfds: Vec<libc::pollfd> = fds_r.iter()
                                .map(|(fd, events)| libc::pollfd { fd: *fd, events: *events, revents: 0 })
                                .collect();
                            let ret = unsafe { libc::poll(pollfds.as_mut_ptr(), pollfds.len() as libc::nfds_t, timeout_ms) };
                            if ret <= 0 { return Ok(PyObject::list(vec![])); }
                            let results: Vec<PyObjectRef> = pollfds.iter()
                                .filter(|pfd| pfd.revents != 0)
                                .map(|pfd| PyObject::tuple(vec![PyObject::int(pfd.fd as i64), PyObject::int(pfd.revents as i64)]))
                                .collect();
                            return Ok(PyObject::list(results));
                        }
                        #[cfg(not(unix))]
                        Ok(PyObject::list(vec![]))
                    }
                ));
            }
            Ok(inst)
        })
    };

    make_module("select", vec![
        ("select", select_fn),
        ("poll", poll_fn),
        ("POLLIN", PyObject::int(0x001)),
        ("POLLPRI", PyObject::int(0x002)),
        ("POLLOUT", PyObject::int(0x004)),
        ("POLLERR", PyObject::int(0x008)),
        ("POLLHUP", PyObject::int(0x010)),
        ("POLLNVAL", PyObject::int(0x020)),
        // epoll constants (Linux)
        ("EPOLLIN", PyObject::int(0x001)),
        ("EPOLLOUT", PyObject::int(0x004)),
        ("EPOLLERR", PyObject::int(0x008)),
        ("EPOLLHUP", PyObject::int(0x010)),
        ("EPOLLET", PyObject::int(1 << 31)),
        ("EPOLLONESHOT", PyObject::int(1 << 30)),
        ("EPOLLRDHUP", PyObject::int(0x2000)),
        ("error", PyObject::str_val(CompactString::from("select.error"))),
    ])
}
