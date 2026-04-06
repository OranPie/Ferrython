//! Concurrency stdlib modules (threading, weakref, gc, _thread)

use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    PyObject, PyObjectPayload, PyObjectRef, PyObjectMethods,
    make_module, make_builtin, check_args_min,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::cell::RefCell;
use std::sync::Arc;

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
                    if let Some(t) = r.get(&HashableKey::Str(CompactString::from("target"))) { target = t.clone(); }
                    if let Some(a) = r.get(&HashableKey::Str(CompactString::from("args"))) { thread_args = a.clone(); }
                    if let Some(d) = r.get(&HashableKey::Str(CompactString::from("daemon"))) { daemon = d.clone(); }
                    if let Some(n) = r.get(&HashableKey::Str(CompactString::from("name"))) { name = n.clone(); }
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
                match &target.payload {
                    PyObjectPayload::NativeFunction { func, .. } => { let _ = func(&call_args); }
                    PyObjectPayload::NativeClosure { func, .. } => { let _ = func(&call_args); }
                    _ => { push_deferred_call(target, call_args); }
                }
            }
            inst.attrs.write().insert(CompactString::from("_alive"), PyObject::bool_val(false));
        }
        Ok(PyObject::none())
    }));

    // join(self, timeout=None) — wait for thread to complete
    // In Ferrython's sequential execution model, threads complete during start(),
    // so join() mainly validates state and respects the API contract.
    thread_ns.insert(CompactString::from("join"), make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::none()); }
        if let PyObjectPayload::Instance(ref inst) = args[0].payload {
            let attrs = inst.attrs.read();
            let started = attrs.get("_started")
                .map(|v| v.is_truthy()).unwrap_or(false);
            if !started {
                return Err(PyException::runtime_error("cannot join thread before it is started"));
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
    thread_ns.insert(CompactString::from("setDaemon"), make_builtin(|_| Ok(PyObject::none())));

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

    // Lock — context manager with acquire/release using shared state
    let lock_cls = PyObject::class(CompactString::from("Lock"), vec![], IndexMap::new());
    let lc = lock_cls.clone();
    let lock_fn = PyObject::native_closure("Lock", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(lc.clone());
        let locked = Arc::new(RwLock::new(false));
        let inst_ref = inst.clone();
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let l1 = locked.clone();
            attrs.insert(CompactString::from("acquire"), PyObject::native_closure(
                "acquire", move |_: &[PyObjectRef]| { *l1.write() = true; Ok(PyObject::bool_val(true)) }));
            let l2 = locked.clone();
            attrs.insert(CompactString::from("release"), PyObject::native_closure(
                "release", move |_: &[PyObjectRef]| { *l2.write() = false; Ok(PyObject::none()) }));
            let l3 = locked.clone();
            attrs.insert(CompactString::from("locked"), PyObject::native_closure(
                "locked", move |_: &[PyObjectRef]| { Ok(PyObject::bool_val(*l3.read())) }));
            let l4 = locked.clone();
            attrs.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "__enter__", move |_: &[PyObjectRef]| { *l4.write() = true; Ok(inst_ref.clone()) }));
            let l5 = locked.clone();
            attrs.insert(CompactString::from("__exit__"), PyObject::native_closure(
                "__exit__", move |_: &[PyObjectRef]| { *l5.write() = false; Ok(PyObject::bool_val(false)) }));
        }
        Ok(inst)
    });

    // RLock — reentrant lock with count tracking
    let rlock_cls = PyObject::class(CompactString::from("RLock"), vec![], IndexMap::new());
    let rlc = rlock_cls.clone();
    let rlock_fn = PyObject::native_closure("RLock", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(rlc.clone());
        // (locked, reentrant_count)
        let state = Arc::new(RwLock::new((false, 0u32)));
        let inst_ref = inst.clone();
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let s1 = state.clone();
            attrs.insert(CompactString::from("acquire"), PyObject::native_closure(
                "acquire", move |_: &[PyObjectRef]| {
                    let mut s = s1.write();
                    s.0 = true;
                    s.1 += 1;
                    Ok(PyObject::bool_val(true))
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
        let counter = Arc::new(RwLock::new(initial));
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
        let counter = Arc::new(RwLock::new(initial));
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
        let flag = Arc::new(RwLock::new(false));
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
        let waiting = Arc::new(RwLock::new(0i64));
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(CompactString::from("parties"), PyObject::int(parties));
            let w1 = waiting.clone();
            attrs.insert(CompactString::from("n_waiting"), PyObject::native_closure(
                "n_waiting", move |_: &[PyObjectRef]| { Ok(PyObject::int(*w1.read())) }));
            let w2 = waiting.clone();
            let p = parties;
            attrs.insert(CompactString::from("wait"), PyObject::native_closure(
                "wait", move |_: &[PyObjectRef]| {
                    let mut w = w2.write();
                    *w += 1;
                    if *w >= p { *w = 0; }
                    Ok(PyObject::int(0))
                }));
            attrs.insert(CompactString::from("reset"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("abort"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("broken"), PyObject::bool_val(false));
        }
        Ok(inst)
    });

    // Condition — condition variable
    let cond_cls = PyObject::class(CompactString::from("Condition"), vec![], IndexMap::new());
    let cc = cond_cls.clone();
    let condition_fn = PyObject::native_closure("Condition", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(cc.clone());
        let locked = Arc::new(RwLock::new(false));
        let inst_ref = inst.clone();
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let l1 = locked.clone();
            attrs.insert(CompactString::from("acquire"), PyObject::native_closure(
                "acquire", move |_: &[PyObjectRef]| { *l1.write() = true; Ok(PyObject::bool_val(true)) }));
            let l2 = locked.clone();
            attrs.insert(CompactString::from("release"), PyObject::native_closure(
                "release", move |_: &[PyObjectRef]| { *l2.write() = false; Ok(PyObject::none()) }));
            attrs.insert(CompactString::from("wait"), make_builtin(|_| Ok(PyObject::bool_val(true))));
            attrs.insert(CompactString::from("wait_for"), make_builtin(|_| Ok(PyObject::bool_val(true))));
            attrs.insert(CompactString::from("notify"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("notify_all"), make_builtin(|_| Ok(PyObject::none())));
            let l3 = locked.clone();
            let ir = inst_ref.clone();
            attrs.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "__enter__", move |_: &[PyObjectRef]| { *l3.write() = true; Ok(ir.clone()) }));
            let l4 = locked.clone();
            attrs.insert(CompactString::from("__exit__"), PyObject::native_closure(
                "__exit__", move |_: &[PyObjectRef]| { *l4.write() = false; Ok(PyObject::bool_val(false)) }));
        }
        Ok(inst)
    });

    // Timer — subclass of Thread with cancel()
    let timer_cls = PyObject::class(CompactString::from("Timer"), vec![], IndexMap::new());
    let tmc = timer_cls.clone();
    let timer_fn = PyObject::native_closure("Timer", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(tmc.clone());
        let cancelled = Arc::new(RwLock::new(false));
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
                    if let Some(t) = r.get(&HashableKey::Str(CompactString::from("target"))) {
                        target = t.clone();
                    }
                    if let Some(a) = r.get(&HashableKey::Str(CompactString::from("args"))) {
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
            let tgt = target.clone();
            let targs = fn_args.clone();
            attrs.insert(CompactString::from("start"), PyObject::native_closure(
                "start", move |_: &[PyObjectRef]| {
                    if *c2.read() { return Ok(PyObject::none()); }
                    if !matches!(&tgt.payload, PyObjectPayload::None) {
                        let call_args: Vec<PyObjectRef> = match &targs.payload {
                            PyObjectPayload::Tuple(items) => items.clone(),
                            PyObjectPayload::List(items) => items.read().clone(),
                            _ => vec![],
                        };
                        match &tgt.payload {
                            PyObjectPayload::NativeFunction { func, .. } => { let _ = func(&call_args); }
                            PyObjectPayload::NativeClosure { func, .. } => { let _ = func(&call_args); }
                            _ => { push_deferred_call(tgt.clone(), call_args); }
                        }
                    }
                    Ok(PyObject::none())
                }));
            attrs.insert(CompactString::from("join"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("is_alive"), make_builtin(|_| Ok(PyObject::bool_val(false))));
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
    ])
}

// ── datetime module ──


pub fn create_weakref_module() -> PyObjectRef {
    make_module("weakref", vec![
        ("ref", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("ref requires 1 argument")); }
            let referent = args[0].clone();
            let mut cls_ns = IndexMap::new();
            let ref2 = referent.clone();
            cls_ns.insert(CompactString::from("__call__"), PyObject::native_closure("weakref.__call__", move |_a| Ok(ref2.clone())));
            let cls = PyObject::class(CompactString::from("weakref"), vec![], cls_ns);
            let mut inst_attrs = IndexMap::new();
            if let PyObjectPayload::Instance(inst) = &referent.payload {
                let r = inst.attrs.read();
                for (k, v) in r.iter() {
                    inst_attrs.insert(k.clone(), v.clone());
                }
            }
            Ok(PyObject::instance_with_attrs(cls, inst_attrs))
        })),
        ("proxy", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("proxy requires 1 argument")); }
            Ok(args[0].clone())
        })),
        ("WeakValueDictionary", make_builtin(|_| Ok(PyObject::dict(IndexMap::new())))),
        ("WeakKeyDictionary", make_builtin(|_| Ok(PyObject::dict(IndexMap::new())))),
        ("WeakSet", make_builtin(|_| Ok(PyObject::set(IndexMap::new())))),
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
                    HashableKey::Str(CompactString::from("collections")),
                    PyObject::int(stats.collections as i64),
                );
                m.insert(
                    HashableKey::Str(CompactString::from("collected")),
                    PyObject::int(0),
                );
                m.insert(
                    HashableKey::Str(CompactString::from("uncollectable")),
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
    ])
}

// ── _thread module ──

pub fn create_thread_module() -> PyObjectRef {
    make_module("_thread", vec![
        ("allocate_lock", make_builtin(|_| {
            let cls = PyObject::class(CompactString::from("lock"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("_locked"), PyObject::bool_val(false));
                w.insert(CompactString::from("acquire"), make_builtin(|_| Ok(PyObject::bool_val(true))));
                w.insert(CompactString::from("release"), make_builtin(|_| Ok(PyObject::none())));
                w.insert(CompactString::from("locked"), make_builtin(|_| Ok(PyObject::bool_val(false))));
                w.insert(CompactString::from("__enter__"), make_builtin(|_| Ok(PyObject::bool_val(true))));
                w.insert(CompactString::from("__exit__"), make_builtin(|_| Ok(PyObject::none())));
            }
            Ok(inst)
        })),
        ("LockType", PyObject::class(CompactString::from("lock"), vec![], IndexMap::new())),
        ("start_new_thread", make_builtin(|_| Ok(PyObject::int(0)))),
        ("get_ident", make_builtin(|_| Ok(PyObject::int(1)))),
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
        ("SIG_DFL", PyObject::int(0)),
        ("SIG_IGN", PyObject::int(1)),
        ("signal", make_builtin(|args| {
            if args.len() < 2 { return Err(PyException::type_error("signal() requires 2 arguments")); }
            let signum = args[0].to_int()?;
            let handler = args[1].clone();
            // Return previous handler, store new one
            let prev = SIGNAL_HANDLERS.with(|h| {
                let mut map = h.borrow_mut();
                let old = map.get(&signum).cloned().unwrap_or_else(|| PyObject::int(0));
                map.insert(signum, handler);
                old
            });
            // Install real OS signal handler for safe signals
            #[cfg(unix)]
            {
                use std::sync::atomic::{AtomicBool, Ordering};
                static SIGINT_FLAG: AtomicBool = AtomicBool::new(false);
                if signum == 2 {
                    // SIGINT — install a handler that sets a flag
                    unsafe {
                        libc::signal(libc::SIGINT, signal_handler as *const () as libc::sighandler_t);
                    }
                    extern "C" fn signal_handler(_sig: libc::c_int) {
                        SIGINT_FLAG.store(true, Ordering::SeqCst);
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
            #[cfg(unix)]
            unsafe { libc::raise(signum as libc::c_int); }
            Ok(PyObject::none())
        })),
        ("alarm", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::int(0)); }
            let secs = args[0].to_int()? as u32;
            #[cfg(unix)]
            let remaining = unsafe { libc::alarm(secs) };
            #[cfg(not(unix))]
            let remaining = 0u32;
            Ok(PyObject::int(remaining as i64))
        })),
        ("pause", make_builtin(|_| {
            #[cfg(unix)]
            unsafe { libc::pause(); }
            Ok(PyObject::none())
        })),
        ("set_wakeup_fd", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::int(-1)); }
            let _fd = args[0].to_int()?;
            Ok(PyObject::int(-1)) // return previous fd (-1 = none)
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
                    if let Some(t) = r.get(&HashableKey::Str(CompactString::from("target"))) {
                        target = t.clone();
                    }
                    if let Some(a) = r.get(&HashableKey::Str(CompactString::from("args"))) {
                        proc_args = a.clone();
                    }
                    if let Some(n) = r.get(&HashableKey::Str(CompactString::from("name"))) {
                        name = n.clone();
                    }
                    if let Some(d) = r.get(&HashableKey::Str(CompactString::from("daemon"))) {
                        daemon = d.clone();
                    }
                }
            }
            attrs.insert(CompactString::from("name"), name);
            attrs.insert(CompactString::from("daemon"), daemon);
            attrs.insert(CompactString::from("pid"), PyObject::int(std::process::id() as i64));
            attrs.insert(CompactString::from("exitcode"), PyObject::none());

            let alive = Arc::new(RwLock::new(false));

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
                            PyObjectPayload::NativeFunction { func, .. } => { let _ = func(&call_args); }
                            PyObjectPayload::NativeClosure { func, .. } => { let _ = func(&call_args); }
                            _ => { push_deferred_call(tgt.clone(), call_args); }
                        }
                    }
                    *a1.write() = false;
                    Ok(PyObject::none())
                }));
            attrs.insert(CompactString::from("join"), make_builtin(|_| Ok(PyObject::none())));
            let a2 = alive.clone();
            attrs.insert(CompactString::from("is_alive"), PyObject::native_closure(
                "is_alive", move |_: &[PyObjectRef]| { Ok(PyObject::bool_val(*a2.read())) }));
            attrs.insert(CompactString::from("terminate"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("kill"), make_builtin(|_| Ok(PyObject::none())));
        }
        Ok(inst)
    });

    // Pool(processes=) — stub
    let pool_cls = PyObject::class(CompactString::from("Pool"), vec![], IndexMap::new());
    let plc = pool_cls.clone();
    let pool_fn = PyObject::native_closure("Pool", move |args: &[PyObjectRef]| {
        let processes = if !args.is_empty() { args[0].as_int().unwrap_or(1) } else { 1 };
        let inst = PyObject::instance(plc.clone());
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(CompactString::from("_processes"), PyObject::int(processes));
            attrs.insert(CompactString::from("map"), make_builtin(|args| {
                // Pool.map(func, iterable) — execute func(item) for each item sequentially
                if args.len() < 2 { return Err(PyException::type_error("map() requires func and iterable")); }
                let func = &args[0];
                let iterable = args[1].to_list()?;
                let mut results = Vec::with_capacity(iterable.len());
                for item in &iterable {
                    match &func.payload {
                        PyObjectPayload::NativeFunction { func: f, .. } => {
                            results.push(f(&[item.clone()])?);
                        }
                        PyObjectPayload::NativeClosure { func: f, .. } => {
                            results.push(f(&[item.clone()])?);
                        }
                        _ => {
                            // For Python functions, use deferred calls
                            push_deferred_call(func.clone(), vec![item.clone()]);
                            results.push(PyObject::none()); // placeholder
                        }
                    }
                }
                Ok(PyObject::list(results))
            }));
            attrs.insert(CompactString::from("apply"), make_builtin(|args| {
                // Pool.apply(func, args=()) — call func with args
                if args.is_empty() { return Err(PyException::type_error("apply() requires func")); }
                let func = &args[0];
                let call_args: Vec<PyObjectRef> = if args.len() > 1 {
                    args[1].to_list().unwrap_or_default()
                } else { vec![] };
                match &func.payload {
                    PyObjectPayload::NativeFunction { func: f, .. } => f(&call_args),
                    PyObjectPayload::NativeClosure { func: f, .. } => f(&call_args),
                    _ => {
                        push_deferred_call(func.clone(), call_args);
                        Ok(PyObject::none())
                    }
                }
            }));
            attrs.insert(CompactString::from("apply_async"), make_builtin(|args| {
                // apply_async returns an AsyncResult; in our model, execute immediately
                if args.is_empty() { return Err(PyException::type_error("apply_async() requires func")); }
                let func = &args[0];
                let call_args: Vec<PyObjectRef> = if args.len() > 1 {
                    args[1].to_list().unwrap_or_default()
                } else { vec![] };
                let result = match &func.payload {
                    PyObjectPayload::NativeFunction { func: f, .. } => f(&call_args)?,
                    PyObjectPayload::NativeClosure { func: f, .. } => f(&call_args)?,
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
                }
                Ok(async_inst)
            }));
            attrs.insert(CompactString::from("close"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("join"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("terminate"), make_builtin(|_| Ok(PyObject::none())));
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
                let mut attrs = d.attrs.write();
                attrs.insert(CompactString::from("put"), make_builtin(|_| Ok(PyObject::none())));
                attrs.insert(CompactString::from("get"), make_builtin(|_| Ok(PyObject::none())));
                attrs.insert(CompactString::from("empty"), make_builtin(|_| Ok(PyObject::bool_val(true))));
            }
            Ok(inst)
        })),
        ("Lock", make_builtin(|_| {
            let cls = PyObject::class(CompactString::from("Lock"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut attrs = d.attrs.write();
                attrs.insert(CompactString::from("acquire"), make_builtin(|_| Ok(PyObject::bool_val(true))));
                attrs.insert(CompactString::from("release"), make_builtin(|_| Ok(PyObject::none())));
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
        ("Manager", make_builtin(|_| Ok(PyObject::none()))),
        ("Pipe", make_builtin(|_| Ok(PyObject::tuple(vec![PyObject::none(), PyObject::none()])))),
        ("Event", make_builtin(|_| Ok(PyObject::none()))),
        ("Semaphore", make_builtin(|_| Ok(PyObject::none()))),
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
            let registry: Arc<RwLock<IndexMap<i64, PyObjectRef>>> = Arc::new(RwLock::new(IndexMap::new()));
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
                    "select", move |_: &[PyObjectRef]| {
                        let r = reg3.read();
                        let results: Vec<PyObjectRef> = r.values().map(|key| {
                            let events = if let PyObjectPayload::Instance(ref d) = key.payload {
                                d.attrs.read().get(&CompactString::from("events")).cloned().unwrap_or_else(|| PyObject::int(0))
                            } else {
                                PyObject::int(0)
                            };
                            PyObject::tuple(vec![key.clone(), events])
                        }).collect();
                        Ok(PyObject::list(results))
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
        // Simplified: return empty lists (no real fd polling)
        if args.len() < 3 {
            return Err(PyException::type_error(
                "select() requires at least 3 arguments",
            ));
        }
        // In a real implementation, we'd use libc::select or poll.
        // For now, return (rlist, [], []) to unblock common patterns.
        let rlist = match &args[0].payload {
            PyObjectPayload::List(items) => {
                let r = items.read();
                if r.is_empty() { vec![] } else { r.clone() }
            }
            _ => vec![],
        };
        Ok(PyObject::tuple(vec![
            PyObject::list(rlist),
            PyObject::list(vec![]),
            PyObject::list(vec![]),
        ]))
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
            Ok(PyObject::instance(poll_cls.clone()))
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
        ("error", PyObject::str_val(CompactString::from("select.error"))),
    ])
}
