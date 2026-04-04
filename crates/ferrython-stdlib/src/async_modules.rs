//! asyncio stdlib module — single-threaded cooperative event loop.
//!
//! Provides asyncio.run(), asyncio.sleep(), asyncio.gather(), etc.
//! Since Ferrython is single-threaded, the event loop runs coroutines
//! cooperatively via round-robin scheduling.

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin, check_args, check_args_min,
};
use indexmap::IndexMap;
use std::cell::RefCell;

// ── Thread-local: asyncio.run() signal ──────────────────────────────────
// When asyncio.run(coro) is called from a NativeClosure, it stores the
// coroutine here. The VM checks this after NativeClosure returns and
// drives the coroutine to completion, returning its result.
thread_local! {
    static ASYNCIO_RUN_CORO: RefCell<Option<PyObjectRef>> = RefCell::new(None);
}

/// Called by the VM after NativeClosure calls to check if asyncio.run() was invoked.
/// Returns the coroutine to drive if set, clearing the flag.
pub fn take_asyncio_run_coro() -> Option<PyObjectRef> {
    ASYNCIO_RUN_CORO.with(|c| c.borrow_mut().take())
}

// ── asyncio module ──────────────────────────────────────────────────────

pub fn create_asyncio_module() -> PyObjectRef {
    // asyncio.run(coro) — drive a coroutine to completion
    let run_fn = make_builtin(asyncio_run);

    // asyncio.sleep(secs) — return a special marker; the VM event loop handles actual delay
    let sleep_fn = make_builtin(asyncio_sleep);

    // asyncio.gather(*coros) — run multiple coroutines, return list of results
    let gather_fn = make_builtin(asyncio_gather);

    // asyncio.create_task(coro) — wrap coroutine as a Task (in our impl, just returns it)
    let create_task_fn = make_builtin(asyncio_create_task);

    // asyncio.get_event_loop() — return a stub event loop
    let get_event_loop_fn = make_builtin(asyncio_get_event_loop);

    // asyncio.ensure_future(coro) — alias for create_task
    let ensure_future_fn = make_builtin(asyncio_create_task);

    // asyncio.wait_for(coro, timeout) — simplified: just runs the coro
    let wait_for_fn = make_builtin(asyncio_wait_for);

    // asyncio.iscoroutine(obj) / iscoroutinefunction(obj)
    let iscoroutine_fn = make_builtin(asyncio_iscoroutine);
    let iscoroutinefunction_fn = make_builtin(asyncio_iscoroutinefunction);

    // Exception classes
    let timeout_error = PyObject::class(
        CompactString::from("TimeoutError"),
        vec![],
        IndexMap::new(),
    );
    let cancelled_error = PyObject::class(
        CompactString::from("CancelledError"),
        vec![],
        IndexMap::new(),
    );
    let invalid_state_error = PyObject::class(
        CompactString::from("InvalidStateError"),
        vec![],
        IndexMap::new(),
    );

    // Queue class (asyncio.Queue)
    let queue_class = make_asyncio_queue_class();

    // Event class (asyncio.Event)
    let event_class = make_asyncio_event_class();

    // Semaphore class
    let semaphore_class = make_asyncio_semaphore_class();

    // Lock class
    let lock_class = make_asyncio_lock_class();

    make_module("asyncio", vec![
        ("run", run_fn),
        ("sleep", sleep_fn),
        ("gather", gather_fn),
        ("create_task", create_task_fn),
        ("ensure_future", ensure_future_fn),
        ("get_event_loop", get_event_loop_fn),
        ("wait_for", wait_for_fn),
        ("iscoroutine", iscoroutine_fn),
        ("iscoroutinefunction", iscoroutinefunction_fn),
        ("TimeoutError", timeout_error),
        ("CancelledError", cancelled_error),
        ("InvalidStateError", invalid_state_error),
        ("Queue", queue_class),
        ("Event", event_class),
        ("Semaphore", semaphore_class),
        ("Lock", lock_class),
    ])
}

// ── Core functions ──────────────────────────────────────────────────────

fn asyncio_run(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("asyncio.run", args, 1)?;
    let coro = &args[0];
    match &coro.payload {
        PyObjectPayload::Coroutine(_) => {
            // Store the coroutine for the VM to drive after this NativeClosure returns
            ASYNCIO_RUN_CORO.with(|c| {
                *c.borrow_mut() = Some(coro.clone());
            });
            // Return the coroutine itself — the VM will replace this with the actual result
            Ok(coro.clone())
        }
        _ => Err(PyException::type_error(
            "asyncio.run() requires a coroutine object"
        )),
    }
}

fn asyncio_sleep(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let secs = if args.is_empty() {
        0.0
    } else {
        match &args[0].payload {
            PyObjectPayload::Int(n) => n.to_f64(),
            PyObjectPayload::Float(f) => *f,
            _ => 0.0,
        }
    };
    // Actually sleep (single-threaded, blocking is fine)
    if secs > 0.0 {
        std::thread::sleep(std::time::Duration::from_secs_f64(secs));
    }
    // Return a BuiltinAwaitable so `await asyncio.sleep()` works correctly
    Ok(PyObject::builtin_awaitable(PyObject::none()))
}

fn asyncio_gather(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // asyncio.gather(*coros) — wrap in BuiltinAwaitable with list of coroutines.
    // The VM's YieldFrom handler will drive each coroutine and collect results.
    let items: Vec<PyObjectRef> = args.to_vec();
    Ok(PyObject::builtin_awaitable(PyObject::list(items)))
}

fn asyncio_create_task(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("asyncio.create_task", args, 1)?;
    let coro = &args[0];
    // Wrap in a Task-like object
    let task_cls = PyObject::class(
        CompactString::from("Task"),
        vec![],
        IndexMap::new(),
    );
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("_coro"), coro.clone());
    attrs.insert(CompactString::from("_result"), PyObject::none());
    attrs.insert(CompactString::from("_done"), PyObject::bool_val(false));
    Ok(PyObject::instance_with_attrs(task_cls, attrs))
}

fn asyncio_get_event_loop(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Return a stub event loop object with run_until_complete method
    let loop_cls = PyObject::class(
        CompactString::from("EventLoop"),
        vec![],
        IndexMap::new(),
    );
    let run_until_complete = make_builtin(|args: &[PyObjectRef]| {
        // EventLoop.run_until_complete(coro) — same as asyncio.run(coro)
        check_args_min("run_until_complete", args, 1)?;
        asyncio_run(&args[0..1])
    });
    let close_fn = make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none()));
    let is_running_fn = make_builtin(|_: &[PyObjectRef]| Ok(PyObject::bool_val(false)));
    let is_closed_fn = make_builtin(|_: &[PyObjectRef]| Ok(PyObject::bool_val(false)));

    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("run_until_complete"), run_until_complete);
    attrs.insert(CompactString::from("close"), close_fn);
    attrs.insert(CompactString::from("is_running"), is_running_fn);
    attrs.insert(CompactString::from("is_closed"), is_closed_fn);
    Ok(PyObject::instance_with_attrs(loop_cls, attrs))
}

fn asyncio_wait_for(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("asyncio.wait_for", args, 1)?;
    // Simplified: just return the coroutine to be awaited
    Ok(args[0].clone())
}

fn asyncio_iscoroutine(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("asyncio.iscoroutine", args, 1)?;
    let is_coro = matches!(&args[0].payload, PyObjectPayload::Coroutine(_));
    Ok(PyObject::bool_val(is_coro))
}

fn asyncio_iscoroutinefunction(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("asyncio.iscoroutinefunction", args, 1)?;
    let is_coro_fn = match &args[0].payload {
        PyObjectPayload::Function(pf) => {
            // CO_COROUTINE = 0x0100
            pf.code.flags.bits() & 0x0100 != 0
        }
        _ => false,
    };
    Ok(PyObject::bool_val(is_coro_fn))
}

// ── Synchronization primitives ──────────────────────────────────────────

fn make_asyncio_queue_class() -> PyObjectRef {
    make_builtin(|args: &[PyObjectRef]| {
        let maxsize = if args.len() > 1 {
            args[1].as_int().unwrap_or(0)
        } else {
            0
        };
        let q_cls = PyObject::class(CompactString::from("Queue"), vec![], IndexMap::new());
        let items_list = PyObject::list(vec![]);
        let items_ref = items_list.clone();
        let items_ref2 = items_list.clone();
        let items_ref3 = items_list.clone();
        let items_ref4 = items_list.clone();
        let items_ref5 = items_list.clone();
        
        let put_fn = PyObject::native_closure("Queue.put", move |put_args: &[PyObjectRef]| {
            if put_args.is_empty() {
                return Err(PyException::type_error("Queue.put() requires an argument"));
            }
            if let PyObjectPayload::List(items) = &items_ref.payload {
                items.write().push(put_args[0].clone());
            }
            Ok(PyObject::builtin_awaitable(PyObject::none()))
        });
        let get_fn = PyObject::native_closure("Queue.get", move |_args: &[PyObjectRef]| {
            if let PyObjectPayload::List(items) = &items_ref2.payload {
                let mut w = items.write();
                if w.is_empty() {
                    return Err(PyException::new(ExceptionKind::RuntimeError, "Queue is empty"));
                }
                return Ok(PyObject::builtin_awaitable(w.remove(0)));
            }
            Ok(PyObject::builtin_awaitable(PyObject::none()))
        });
        let qsize_fn = PyObject::native_closure("Queue.qsize", move |_args: &[PyObjectRef]| {
            if let PyObjectPayload::List(items) = &items_ref3.payload {
                return Ok(PyObject::int(items.read().len() as i64));
            }
            Ok(PyObject::int(0))
        });
        let empty_fn = PyObject::native_closure("Queue.empty", move |_args: &[PyObjectRef]| {
            if let PyObjectPayload::List(items) = &items_ref4.payload {
                return Ok(PyObject::bool_val(items.read().is_empty()));
            }
            Ok(PyObject::bool_val(true))
        });
        let put_nowait_fn = PyObject::native_closure("Queue.put_nowait", move |put_args: &[PyObjectRef]| {
            if put_args.is_empty() {
                return Err(PyException::type_error("Queue.put_nowait() requires an argument"));
            }
            if let PyObjectPayload::List(items) = &items_ref5.payload {
                items.write().push(put_args[0].clone());
            }
            Ok(PyObject::none())
        });
        
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("_items"), items_list);
        attrs.insert(CompactString::from("maxsize"), PyObject::int(maxsize));
        attrs.insert(CompactString::from("put"), put_fn);
        attrs.insert(CompactString::from("get"), get_fn);
        attrs.insert(CompactString::from("qsize"), qsize_fn);
        attrs.insert(CompactString::from("empty"), empty_fn);
        attrs.insert(CompactString::from("put_nowait"), put_nowait_fn);
        Ok(PyObject::instance_with_attrs(q_cls, attrs))
    })
}

fn make_asyncio_event_class() -> PyObjectRef {
    make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("Event"), vec![], IndexMap::new());
        let set_fn = make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none()));
        let clear_fn = make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none()));
        let wait_fn = make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none()));
        let is_set_fn = make_builtin(|_: &[PyObjectRef]| Ok(PyObject::bool_val(false)));
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("set"), set_fn);
        attrs.insert(CompactString::from("clear"), clear_fn);
        attrs.insert(CompactString::from("wait"), wait_fn);
        attrs.insert(CompactString::from("is_set"), is_set_fn);
        Ok(PyObject::instance_with_attrs(cls, attrs))
    })
}

fn make_asyncio_semaphore_class() -> PyObjectRef {
    make_builtin(|args: &[PyObjectRef]| {
        let value = if !args.is_empty() {
            args[0].as_int().unwrap_or(1)
        } else {
            1
        };
        let cls = PyObject::class(CompactString::from("Semaphore"), vec![], IndexMap::new());
        let acquire_fn = make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none()));
        let release_fn = make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none()));
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("_value"), PyObject::int(value));
        attrs.insert(CompactString::from("acquire"), acquire_fn);
        attrs.insert(CompactString::from("release"), release_fn);
        Ok(PyObject::instance_with_attrs(cls, attrs))
    })
}

fn make_asyncio_lock_class() -> PyObjectRef {
    make_builtin(|_args: &[PyObjectRef]| {
        let cls = PyObject::class(CompactString::from("Lock"), vec![], IndexMap::new());
        let acquire_fn = make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none()));
        let release_fn = make_builtin(|_: &[PyObjectRef]| Ok(PyObject::none()));
        let locked_fn = make_builtin(|_: &[PyObjectRef]| Ok(PyObject::bool_val(false)));
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("acquire"), acquire_fn);
        attrs.insert(CompactString::from("release"), release_fn);
        attrs.insert(CompactString::from("locked"), locked_fn);
        Ok(PyObject::instance_with_attrs(cls, attrs))
    })
}
