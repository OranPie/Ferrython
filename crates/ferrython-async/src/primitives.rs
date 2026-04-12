//! Synchronization primitives: asyncio.Queue, Event, Semaphore, Lock.
//!
//! These model CPython's asyncio synchronization primitives.
//! Since Ferrython is single-threaded, locking is cooperative (no actual
//! OS-level synchronization needed), but we still model the API correctly.

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException};
use ferrython_core::object::{PyCell, 
    PyObject, PyObjectMethods, PyObjectRef,
    make_builtin,
};
use indexmap::IndexMap;
use std::rc::Rc;

// ── asyncio.Queue ───────────────────────────────────────────────────────

/// Create the asyncio.Queue constructor callable.
pub fn make_queue_class() -> PyObjectRef {
    make_builtin(|args: &[PyObjectRef]| {
        let maxsize = if !args.is_empty() {
            args[0].as_int().unwrap_or(0)
        } else {
            0
        };

        let cls = PyObject::class(CompactString::from("Queue"), vec![], IndexMap::new());
        let items = Rc::new(PyCell::new(Vec::<PyObjectRef>::new()));

        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("maxsize"), PyObject::int(maxsize));

        // put(item) → awaitable
        let items_put = items.clone();
        attrs.insert(CompactString::from("put"), PyObject::native_closure(
            "Queue.put",
            move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("Queue.put() requires an argument"));
                }
                items_put.write().push(args[0].clone());
                Ok(PyObject::builtin_awaitable(PyObject::none()))
            },
        ));

        // get() → awaitable(item)
        let items_get = items.clone();
        attrs.insert(CompactString::from("get"), PyObject::native_closure(
            "Queue.get",
            move |_| {
                let mut w = items_get.write();
                if w.is_empty() {
                    return Err(PyException::new(ExceptionKind::RuntimeError, "Queue is empty"));
                }
                Ok(PyObject::builtin_awaitable(w.remove(0)))
            },
        ));

        // put_nowait(item) → None
        let items_pn = items.clone();
        attrs.insert(CompactString::from("put_nowait"), PyObject::native_closure(
            "Queue.put_nowait",
            move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("Queue.put_nowait() requires an argument"));
                }
                items_pn.write().push(args[0].clone());
                Ok(PyObject::none())
            },
        ));

        // get_nowait() → item
        let items_gn = items.clone();
        attrs.insert(CompactString::from("get_nowait"), PyObject::native_closure(
            "Queue.get_nowait",
            move |_| {
                let mut w = items_gn.write();
                if w.is_empty() {
                    return Err(PyException::new(ExceptionKind::RuntimeError, "Queue is empty"));
                }
                Ok(w.remove(0))
            },
        ));

        // qsize() → int
        let items_qs = items.clone();
        attrs.insert(CompactString::from("qsize"), PyObject::native_closure(
            "Queue.qsize",
            move |_| Ok(PyObject::int(items_qs.read().len() as i64)),
        ));

        // empty() → bool
        let items_em = items.clone();
        attrs.insert(CompactString::from("empty"), PyObject::native_closure(
            "Queue.empty",
            move |_| Ok(PyObject::bool_val(items_em.read().is_empty())),
        ));

        // full() → bool
        let items_fu = items.clone();
        let ms = maxsize;
        attrs.insert(CompactString::from("full"), PyObject::native_closure(
            "Queue.full",
            move |_| {
                if ms <= 0 {
                    Ok(PyObject::bool_val(false))
                } else {
                    Ok(PyObject::bool_val(items_fu.read().len() as i64 >= ms))
                }
            },
        ));

        // task_done() / join() — stubs
        attrs.insert(CompactString::from("task_done"), make_builtin(|_| Ok(PyObject::none())));
        attrs.insert(CompactString::from("join"), make_builtin(|_| Ok(PyObject::builtin_awaitable(PyObject::none()))));

        Ok(PyObject::instance_with_attrs(cls, attrs))
    })
}

// ── asyncio.Event ───────────────────────────────────────────────────────

/// Create the asyncio.Event constructor callable.
pub fn make_event_class() -> PyObjectRef {
    make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("Event"), vec![], IndexMap::new());
        let flag = Rc::new(PyCell::new(false));

        let mut attrs = IndexMap::new();

        // set()
        let f = flag.clone();
        attrs.insert(CompactString::from("set"), PyObject::native_closure(
            "Event.set",
            move |_| { *f.write() = true; Ok(PyObject::none()) },
        ));

        // clear()
        let f = flag.clone();
        attrs.insert(CompactString::from("clear"), PyObject::native_closure(
            "Event.clear",
            move |_| { *f.write() = false; Ok(PyObject::none()) },
        ));

        // is_set() → bool
        let f = flag.clone();
        attrs.insert(CompactString::from("is_set"), PyObject::native_closure(
            "Event.is_set",
            move |_| Ok(PyObject::bool_val(*f.read())),
        ));

        // wait() → awaitable (resolves immediately if set)
        let f = flag.clone();
        attrs.insert(CompactString::from("wait"), PyObject::native_closure(
            "Event.wait",
            move |_| {
                if *f.read() {
                    Ok(PyObject::builtin_awaitable(PyObject::bool_val(true)))
                } else {
                    // In a real event loop, this would suspend until set()
                    // For now, return immediately with True
                    Ok(PyObject::builtin_awaitable(PyObject::bool_val(true)))
                }
            },
        ));

        Ok(PyObject::instance_with_attrs(cls, attrs))
    })
}

// ── asyncio.Lock ────────────────────────────────────────────────────────

/// Create the asyncio.Lock constructor callable.
pub fn make_lock_class() -> PyObjectRef {
    make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("Lock"), vec![], IndexMap::new());
        let locked = Rc::new(PyCell::new(false));

        let mut attrs = IndexMap::new();

        // acquire() → awaitable
        let l = locked.clone();
        attrs.insert(CompactString::from("acquire"), PyObject::native_closure(
            "Lock.acquire",
            move |_| {
                *l.write() = true;
                Ok(PyObject::builtin_awaitable(PyObject::bool_val(true)))
            },
        ));

        // release()
        let l = locked.clone();
        attrs.insert(CompactString::from("release"), PyObject::native_closure(
            "Lock.release",
            move |_| {
                let mut w = l.write();
                if !*w {
                    return Err(PyException::runtime_error("Lock is not acquired"));
                }
                *w = false;
                Ok(PyObject::none())
            },
        ));

        // locked() → bool
        let l = locked.clone();
        attrs.insert(CompactString::from("locked"), PyObject::native_closure(
            "Lock.locked",
            move |_| Ok(PyObject::bool_val(*l.read())),
        ));

        // Context manager support: __aenter__ / __aexit__
        let l = locked.clone();
        attrs.insert(CompactString::from("__aenter__"), PyObject::native_closure(
            "Lock.__aenter__",
            move |_| {
                *l.write() = true;
                Ok(PyObject::builtin_awaitable(PyObject::none()))
            },
        ));
        let l = locked.clone();
        attrs.insert(CompactString::from("__aexit__"), PyObject::native_closure(
            "Lock.__aexit__",
            move |_| {
                *l.write() = false;
                Ok(PyObject::builtin_awaitable(PyObject::none()))
            },
        ));

        Ok(PyObject::instance_with_attrs(cls, attrs))
    })
}

// ── asyncio.Semaphore ───────────────────────────────────────────────────

/// Create the asyncio.Semaphore constructor callable.
pub fn make_semaphore_class() -> PyObjectRef {
    make_builtin(|args: &[PyObjectRef]| {
        let initial_value = if !args.is_empty() {
            args[0].as_int().unwrap_or(1)
        } else {
            1
        };

        let cls = PyObject::class(CompactString::from("Semaphore"), vec![], IndexMap::new());
        let value = Rc::new(PyCell::new(initial_value));
        let bound = initial_value;

        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("_value"), PyObject::int(initial_value));
        attrs.insert(CompactString::from("_bound_value"), PyObject::int(bound));

        // acquire() → awaitable
        let v = value.clone();
        attrs.insert(CompactString::from("acquire"), PyObject::native_closure(
            "Semaphore.acquire",
            move |_| {
                let mut val = v.write();
                if *val > 0 {
                    *val -= 1;
                    Ok(PyObject::builtin_awaitable(PyObject::bool_val(true)))
                } else {
                    // Would normally suspend; for now, return immediately
                    Ok(PyObject::builtin_awaitable(PyObject::bool_val(false)))
                }
            },
        ));

        // release()
        let v = value.clone();
        attrs.insert(CompactString::from("release"), PyObject::native_closure(
            "Semaphore.release",
            move |_| {
                *v.write() += 1;
                Ok(PyObject::none())
            },
        ));

        // __aenter__ / __aexit__
        let v = value.clone();
        attrs.insert(CompactString::from("__aenter__"), PyObject::native_closure(
            "Semaphore.__aenter__",
            move |_| {
                let mut val = v.write();
                if *val > 0 { *val -= 1; }
                Ok(PyObject::builtin_awaitable(PyObject::none()))
            },
        ));
        let v = value.clone();
        attrs.insert(CompactString::from("__aexit__"), PyObject::native_closure(
            "Semaphore.__aexit__",
            move |_| {
                *v.write() += 1;
                Ok(PyObject::builtin_awaitable(PyObject::none()))
            },
        ));

        Ok(PyObject::instance_with_attrs(cls, attrs))
    })
}

// ── asyncio.BoundedSemaphore ────────────────────────────────────────────

/// Create the asyncio.BoundedSemaphore constructor callable.
pub fn make_bounded_semaphore_class() -> PyObjectRef {
    make_builtin(|args: &[PyObjectRef]| {
        let initial_value = if !args.is_empty() {
            args[0].as_int().unwrap_or(1)
        } else {
            1
        };

        let cls = PyObject::class(CompactString::from("BoundedSemaphore"), vec![], IndexMap::new());
        let value = Rc::new(PyCell::new(initial_value));
        let bound = initial_value;

        let mut attrs = IndexMap::new();

        // acquire()
        let v = value.clone();
        attrs.insert(CompactString::from("acquire"), PyObject::native_closure(
            "BoundedSemaphore.acquire",
            move |_| {
                let mut val = v.write();
                if *val > 0 {
                    *val -= 1;
                    Ok(PyObject::builtin_awaitable(PyObject::bool_val(true)))
                } else {
                    Ok(PyObject::builtin_awaitable(PyObject::bool_val(false)))
                }
            },
        ));

        // release() — raises ValueError if would exceed bound
        let v = value.clone();
        attrs.insert(CompactString::from("release"), PyObject::native_closure(
            "BoundedSemaphore.release",
            move |_| {
                let mut val = v.write();
                if *val >= bound {
                    return Err(PyException::value_error("BoundedSemaphore released too many times"));
                }
                *val += 1;
                Ok(PyObject::none())
            },
        ));

        Ok(PyObject::instance_with_attrs(cls, attrs))
    })
}

// ── asyncio.Condition ───────────────────────────────────────────────────

/// Create the asyncio.Condition constructor callable.
pub fn make_condition_class() -> PyObjectRef {
    make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("Condition"), vec![], IndexMap::new());
        let locked = Rc::new(PyCell::new(false));

        let mut attrs = IndexMap::new();

        let l = locked.clone();
        attrs.insert(CompactString::from("acquire"), PyObject::native_closure(
            "Condition.acquire",
            move |_| { *l.write() = true; Ok(PyObject::builtin_awaitable(PyObject::bool_val(true))) },
        ));

        let l = locked.clone();
        attrs.insert(CompactString::from("release"), PyObject::native_closure(
            "Condition.release",
            move |_| { *l.write() = false; Ok(PyObject::none()) },
        ));

        attrs.insert(CompactString::from("wait"), make_builtin(|_| Ok(PyObject::builtin_awaitable(PyObject::bool_val(true)))));
        attrs.insert(CompactString::from("wait_for"), make_builtin(|_| Ok(PyObject::builtin_awaitable(PyObject::bool_val(true)))));
        attrs.insert(CompactString::from("notify"), make_builtin(|_| Ok(PyObject::none())));
        attrs.insert(CompactString::from("notify_all"), make_builtin(|_| Ok(PyObject::none())));

        let l = locked.clone();
        attrs.insert(CompactString::from("locked"), PyObject::native_closure(
            "Condition.locked",
            move |_| Ok(PyObject::bool_val(*l.read())),
        ));

        Ok(PyObject::instance_with_attrs(cls, attrs))
    })
}
