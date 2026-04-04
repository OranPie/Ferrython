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
    // Thread class constructor — accepts target=, args=, kwargs=, daemon=
    let thread_cls = PyObject::class(CompactString::from("Thread"), vec![], IndexMap::new());
    let tc = thread_cls.clone();
    let thread_fn = PyObject::native_closure("Thread", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(tc.clone());
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            // Parse kwargs — VM passes kwargs dict as last arg
            let mut target = PyObject::none();
            let mut thread_args = PyObject::tuple(vec![]);
            let mut daemon = PyObject::bool_val(false);
            let mut name = PyObject::str_val(CompactString::from("Thread"));
            // Check for kwargs dict as last argument
            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(kw_map) = &last.payload {
                    let r = kw_map.read();
                    if let Some(t) = r.get(&HashableKey::Str(CompactString::from("target"))) {
                        target = t.clone();
                    }
                    if let Some(a) = r.get(&HashableKey::Str(CompactString::from("args"))) {
                        thread_args = a.clone();
                    }
                    if let Some(d) = r.get(&HashableKey::Str(CompactString::from("daemon"))) {
                        daemon = d.clone();
                    }
                    if let Some(n) = r.get(&HashableKey::Str(CompactString::from("name"))) {
                        name = n.clone();
                    }
                }
            }
            attrs.insert(CompactString::from("name"), name.clone());
            attrs.insert(CompactString::from("daemon"), daemon);

            // Shared state for thread lifecycle
            let alive = Arc::new(RwLock::new(false));
            let started = Arc::new(RwLock::new(false));

            // start() — call target(*args) synchronously (single-threaded interpreter)
            let tgt = target.clone();
            let targs = thread_args.clone();
            let a1 = alive.clone();
            let s1 = started.clone();
            attrs.insert(CompactString::from("start"), PyObject::native_closure(
                "start", move |_: &[PyObjectRef]| {
                    *s1.write() = true;
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
                            _ => {
                                // Python function — defer to VM via thread-local
                                push_deferred_call(tgt.clone(), call_args);
                            }
                        }
                    }
                    *a1.write() = false;
                    Ok(PyObject::none())
                }
            ));
            attrs.insert(CompactString::from("join"), make_builtin(|_| Ok(PyObject::none())));
            let a2 = alive.clone();
            attrs.insert(CompactString::from("is_alive"), PyObject::native_closure(
                "is_alive", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(*a2.read()))
                }
            ));
            let nm = name.clone();
            attrs.insert(CompactString::from("getName"), PyObject::native_closure(
                "getName", move |_: &[PyObjectRef]| {
                    Ok(nm.clone())
                }
            ));
            attrs.insert(CompactString::from("setDaemon"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("ident"), PyObject::none());
        }
        Ok(inst)
    });

    // Lock/RLock — context managers with acquire/release using shared state
    let lock_cls = PyObject::class(CompactString::from("Lock"), vec![], IndexMap::new());
    let lc = lock_cls.clone();
    let lock_fn = PyObject::native_closure("Lock", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(lc.clone());
        let locked = Arc::new(RwLock::new(false));
        let inst_ref = inst.clone(); // for __enter__ closure
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
            attrs.insert(CompactString::from("wait"), make_builtin(|_| Ok(PyObject::bool_val(true))));
        }
        Ok(inst)
    });

    make_module("threading", vec![
        ("Thread", thread_fn),
        ("Lock", lock_fn.clone()),
        ("RLock", lock_fn),
        ("Event", event_fn),
        ("Semaphore", make_builtin(|_| Ok(PyObject::none()))),
        ("BoundedSemaphore", make_builtin(|_| Ok(PyObject::none()))),
        ("Condition", make_builtin(|_| Ok(PyObject::none()))),
        ("Barrier", make_builtin(|_| Ok(PyObject::none()))),
        ("Timer", make_builtin(|_| Ok(PyObject::none()))),
        ("current_thread", make_builtin(|_| {
            let mut ns = IndexMap::new();
            ns.insert(CompactString::from("name"), PyObject::str_val(CompactString::from("MainThread")));
            ns.insert(CompactString::from("ident"), PyObject::int(1));
            ns.insert(CompactString::from("daemon"), PyObject::bool_val(false));
            ns.insert(CompactString::from("is_alive"), make_builtin(|_| Ok(PyObject::bool_val(true))));
            ns.insert(CompactString::from("getName"), make_builtin(|_| Ok(PyObject::str_val(CompactString::from("MainThread")))));
            let cls = PyObject::class(CompactString::from("Thread"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(i) = &inst.payload {
                let mut attrs = i.attrs.write();
                for (k, v) in ns { attrs.insert(k, v); }
            }
            Ok(inst)
        })),
        ("active_count", make_builtin(|_| Ok(PyObject::int(1)))),
        ("enumerate", make_builtin(|_| Ok(PyObject::list(vec![])))),
        ("main_thread", make_builtin(|_| Ok(PyObject::none()))),
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
