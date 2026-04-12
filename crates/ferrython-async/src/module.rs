//! asyncio Python module — complete API backed by the ferrython-async runtime.
//!
//! This replaces the simpler stub implementation that was in ferrython-stdlib.
//! Provides: run, sleep, gather, create_task, wait, wait_for, as_completed,
//! shield, ensure_future, get_event_loop, get_running_loop, new_event_loop,
//! iscoroutine, iscoroutinefunction, current_task, all_tasks, and
//! synchronization primitives (Queue, Event, Lock, Semaphore, etc.)

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectPayload, PyObjectRef, PyObjectMethods,
    make_module, make_builtin, check_args, check_args_min,
};
use ferrython_bytecode::code::CodeFlags;
use indexmap::IndexMap;
use std::cell::RefCell;

use crate::event_loop;
use crate::primitives;
use crate::task;

// ── Thread-local: asyncio.run() coroutine signal ────────────────────────
thread_local! {
    static ASYNCIO_RUN_CORO: RefCell<Option<PyObjectRef>> = RefCell::new(None);
    /// Active wait_for deadline: if set, asyncio.sleep should respect it
    static WAIT_FOR_DEADLINE: RefCell<Option<std::time::Instant>> = RefCell::new(None);
}

/// Called by the VM after NativeClosure calls to check if asyncio.run() was invoked.
pub fn take_asyncio_run_coro() -> Option<PyObjectRef> {
    ASYNCIO_RUN_CORO.with(|c| c.borrow_mut().take())
}

/// Store a coroutine for the VM to drive (used internally).
pub(crate) fn store_asyncio_run_coro(coro: PyObjectRef) {
    ASYNCIO_RUN_CORO.with(|c| *c.borrow_mut() = Some(coro));
    ferrython_core::object::set_intercept_pending();
}

pub fn set_wait_for_deadline(deadline: Option<std::time::Instant>) {
    WAIT_FOR_DEADLINE.with(|d| *d.borrow_mut() = deadline);
}

pub fn get_wait_for_deadline() -> Option<std::time::Instant> {
    WAIT_FOR_DEADLINE.with(|d| *d.borrow())
}

/// Create the complete `asyncio` module.
pub fn create_asyncio_module() -> PyObjectRef {
    // Exception classes
    let timeout_error = PyObject::class(
        CompactString::from("TimeoutError"),
        vec![], IndexMap::new(),
    );
    let cancelled_error = PyObject::class(
        CompactString::from("CancelledError"),
        vec![], IndexMap::new(),
    );
    let invalid_state_error = PyObject::class(
        CompactString::from("InvalidStateError"),
        vec![], IndexMap::new(),
    );

    make_module("asyncio", vec![
        // Core runners
        ("run", make_builtin(asyncio_run)),
        ("sleep", make_builtin(asyncio_sleep)),
        ("gather", make_builtin(asyncio_gather)),
        ("wait", make_builtin(asyncio_wait)),
        ("wait_for", make_builtin(asyncio_wait_for)),
        ("as_completed", make_builtin(asyncio_as_completed)),
        ("shield", make_builtin(asyncio_shield)),

        // Task management
        ("create_task", make_builtin(asyncio_create_task)),
        ("ensure_future", make_builtin(asyncio_ensure_future)),
        ("current_task", make_builtin(asyncio_current_task)),
        ("all_tasks", make_builtin(asyncio_all_tasks)),

        // Event loop
        ("get_event_loop", make_builtin(asyncio_get_event_loop)),
        ("get_running_loop", make_builtin(asyncio_get_running_loop)),
        ("new_event_loop", make_builtin(asyncio_new_event_loop)),
        ("set_event_loop", make_builtin(asyncio_set_event_loop)),

        // Introspection
        ("iscoroutine", make_builtin(asyncio_iscoroutine)),
        ("iscoroutinefunction", make_builtin(asyncio_iscoroutinefunction)),

        // Synchronization primitives
        ("Queue", primitives::make_queue_class()),
        ("PriorityQueue", primitives::make_queue_class()),
        ("LifoQueue", primitives::make_queue_class()),
        ("Event", primitives::make_event_class()),
        ("Lock", primitives::make_lock_class()),
        ("Semaphore", primitives::make_semaphore_class()),
        ("BoundedSemaphore", primitives::make_bounded_semaphore_class()),
        ("Condition", primitives::make_condition_class()),

        // Future/Task classes
        ("Future", make_builtin(|_| Ok(task::create_future_object()))),
        ("Task", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("Task() requires a coroutine"));
            }
            Ok(task::create_task_object(&args[0]))
        })),

        // Exception classes
        ("TimeoutError", timeout_error),
        ("CancelledError", cancelled_error),
        ("InvalidStateError", invalid_state_error),

        // Constants
        ("FIRST_COMPLETED", PyObject::str_val(CompactString::from("FIRST_COMPLETED"))),
        ("FIRST_EXCEPTION", PyObject::str_val(CompactString::from("FIRST_EXCEPTION"))),
        ("ALL_COMPLETED", PyObject::str_val(CompactString::from("ALL_COMPLETED"))),

        // ABC classes (used by aiohttp, anyio, etc.)
        ("AbstractEventLoop", make_abstract_class("AbstractEventLoop")),
        ("AbstractServer", make_abstract_class("AbstractServer")),
        ("AbstractEventLoopPolicy", make_abstract_class("AbstractEventLoopPolicy")),
        ("BaseProtocol", make_abstract_class("BaseProtocol")),
        ("Protocol", make_abstract_class("Protocol")),
        ("DatagramProtocol", make_abstract_class("DatagramProtocol")),
        ("SubprocessProtocol", make_abstract_class("SubprocessProtocol")),
        ("BufferedProtocol", make_abstract_class("BufferedProtocol")),
        ("BaseTransport", make_abstract_class("BaseTransport")),
        ("Transport", make_abstract_class("Transport")),
        ("DatagramTransport", make_abstract_class("DatagramTransport")),
        ("SubprocessTransport", make_abstract_class("SubprocessTransport")),
        ("ReadTransport", make_abstract_class("ReadTransport")),
        ("WriteTransport", make_abstract_class("WriteTransport")),
        ("StreamReader", make_abstract_class("StreamReader")),
        ("StreamWriter", make_abstract_class("StreamWriter")),
        ("StreamReaderProtocol", make_abstract_class("StreamReaderProtocol")),
        ("Server", make_abstract_class("Server")),

        // Timeout context manager (Python 3.11+)
        ("Timeout", {
            let mut ns = IndexMap::new();
            ns.insert(CompactString::from("__init__"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() >= 2 {
                    if let PyObjectPayload::Instance(ref d) = args[0].payload {
                        let mut w = d.attrs.write();
                        w.insert(CompactString::from("_deadline"), args[1].clone());
                        w.insert(CompactString::from("_expired"), PyObject::bool_val(false));
                    }
                }
                Ok(PyObject::none())
            }));
            ns.insert(CompactString::from("__enter__"), make_builtin(|args: &[PyObjectRef]| {
                Ok(if !args.is_empty() { args[0].clone() } else { PyObject::none() })
            }));
            ns.insert(CompactString::from("__exit__"), make_builtin(|_| {
                Ok(PyObject::bool_val(false))
            }));
            ns.insert(CompactString::from("__aenter__"), make_builtin(|args: &[PyObjectRef]| {
                Ok(if !args.is_empty() { args[0].clone() } else { PyObject::none() })
            }));
            ns.insert(CompactString::from("__aexit__"), make_builtin(|_| {
                Ok(PyObject::bool_val(false))
            }));
            ns.insert(CompactString::from("expired"), make_builtin(|args: &[PyObjectRef]| {
                if !args.is_empty() {
                    if let Some(v) = args[0].get_attr("_expired") { return Ok(v); }
                }
                Ok(PyObject::bool_val(false))
            }));
            ns.insert(CompactString::from("when"), make_builtin(|args: &[PyObjectRef]| {
                if !args.is_empty() {
                    if let Some(v) = args[0].get_attr("_deadline") { return Ok(v); }
                }
                Ok(PyObject::none())
            }));
            ns.insert(CompactString::from("reschedule"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() >= 2 {
                    if let PyObjectPayload::Instance(ref d) = args[0].payload {
                        d.attrs.write().insert(CompactString::from("_deadline"), args[1].clone());
                    }
                }
                Ok(PyObject::none())
            }));
            PyObject::class(CompactString::from("Timeout"), vec![], ns)
        }),

        // Subprocess constants
        ("PIPE", PyObject::int(-1)),
        ("STDOUT", PyObject::int(-2)),
        ("DEVNULL", PyObject::int(-3)),
    ])
}

fn make_abstract_class(name: &str) -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(CompactString::from("register"), make_builtin(|args: &[PyObjectRef]| {
        if args.len() >= 2 {
            Ok(args[1].clone())
        } else if args.len() == 1 {
            Ok(args[0].clone())
        } else {
            Ok(PyObject::none())
        }
    }));
    ns.insert(CompactString::from("__subclasshook__"), make_builtin(|_| {
        Ok(PyObject::str_val(CompactString::from("NotImplemented")))
    }));
    PyObject::class(CompactString::from(name), vec![], ns)
}

// ── Core functions ──────────────────────────────────────────────────────

/// `asyncio.run(coro)` — drive a coroutine to completion.
fn asyncio_run(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("asyncio.run", args, 1)?;
    let coro = &args[0];
    match &coro.payload {
        PyObjectPayload::Coroutine(_) => {
            store_asyncio_run_coro(coro.clone());
            Ok(coro.clone())
        }
        _ => Err(PyException::type_error(
            "asyncio.run() requires a coroutine object"
        )),
    }
}

/// `asyncio.sleep(delay, result=None)` — sleep for delay seconds.
/// Returns a DeferredSleep so the VM can enforce wait_for deadlines.
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

    let result = args.get(1).cloned().unwrap_or_else(PyObject::none);

    if secs <= 0.0 {
        // Zero or negative sleep resolves immediately
        return Ok(PyObject::builtin_awaitable(result));
    }

    // Return a deferred sleep — the VM will perform the actual thread::sleep
    // during YIELD_FROM, where it can also check wait_for deadlines.
    Ok(PyObject::deferred_sleep(secs, result))
}

/// `asyncio.gather(*coros_or_futures, return_exceptions=False)`
fn asyncio_gather(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let items: Vec<PyObjectRef> = args.to_vec();
    Ok(PyObject::builtin_awaitable(PyObject::list(items)))
}

/// `asyncio.wait(fs, *, timeout=None, return_when=ALL_COMPLETED)`
/// Returns (done_set, pending_set). In our single-threaded model, all complete.
fn asyncio_wait(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("asyncio.wait", args, 1)?;
    // In single-threaded mode, everything runs to completion
    // Return (done, pending) as a tuple of sets
    let done = args[0].clone();
    let pending = PyObject::set(ferrython_core::object::new_fx_hashkey_map());
    Ok(PyObject::builtin_awaitable(PyObject::tuple(vec![done, pending])))
}

/// `asyncio.wait_for(fut, timeout)`
fn asyncio_wait_for(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("asyncio.wait_for", args, 1)?;

    // Extract timeout: can be positional arg[1] or keyword in trailing Dict
    let timeout_secs = if args.len() > 1 {
        match &args[1].payload {
            PyObjectPayload::Int(n) => Some(n.to_f64()),
            PyObjectPayload::Float(f) => Some(*f),
            PyObjectPayload::None => None,
            // kwargs dict: extract "timeout" key
            PyObjectPayload::Dict(d) => {
                let map = d.read();
                if let Some(val) = map.get(&ferrython_core::types::HashableKey::Str(CompactString::from("timeout"))) {
                    match &val.payload {
                        PyObjectPayload::Int(n) => Some(n.to_f64()),
                        PyObjectPayload::Float(f) => Some(*f),
                        PyObjectPayload::None => None,
                        _ => None,
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    } else {
        None
    };

    // If timeout is 0 or negative, raise TimeoutError immediately
    if let Some(t) = timeout_secs {
        if t <= 0.0 {
            return Err(PyException::new(
                ferrython_core::error::ExceptionKind::TimeoutError,
                "",
            ));
        }
        // Set deadline so asyncio.sleep respects it
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs_f64(t);
        set_wait_for_deadline(Some(deadline));
    }

    // Return the coroutine — the VM will drive it; sleep will raise TimeoutError if needed
    Ok(args[0].clone())
}

/// `asyncio.as_completed(fs, *, timeout=None)` — iterator of awaitables
fn asyncio_as_completed(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("asyncio.as_completed", args, 1)?;
    // Return the list as-is (each element can be awaited)
    Ok(args[0].clone())
}

/// `asyncio.shield(aw)` — protect an awaitable from cancellation
fn asyncio_shield(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("asyncio.shield", args, 1)?;
    // In our simplified model, just pass through
    Ok(args[0].clone())
}

/// `asyncio.create_task(coro, *, name=None)`
fn asyncio_create_task(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("asyncio.create_task", args, 1)?;
    Ok(task::create_task_object(&args[0]))
}

/// `asyncio.ensure_future(coro_or_future)`
fn asyncio_ensure_future(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("asyncio.ensure_future", args, 1)?;
    match &args[0].payload {
        PyObjectPayload::Coroutine(_) => Ok(task::create_task_object(&args[0])),
        _ => Ok(args[0].clone()), // Already a future-like
    }
}

/// `asyncio.current_task(loop=None)`
fn asyncio_current_task(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::none())
}

/// `asyncio.all_tasks(loop=None)`
fn asyncio_all_tasks(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::set(ferrython_core::object::new_fx_hashkey_map()))
}

// ── Event loop management ───────────────────────────────────────────────

/// `asyncio.get_event_loop()`
fn asyncio_get_event_loop(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(event_loop::create_event_loop_object())
}

/// `asyncio.get_running_loop()`
fn asyncio_get_running_loop(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    match event_loop::get_running_loop() {
        Some(loop_obj) => Ok(loop_obj),
        None => Err(PyException::runtime_error("no running event loop")),
    }
}

/// `asyncio.new_event_loop()`
fn asyncio_new_event_loop(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(event_loop::create_event_loop_object())
}

/// `asyncio.set_event_loop(loop)`
fn asyncio_set_event_loop(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Accept but largely ignore (single-threaded)
    Ok(PyObject::none())
}

// ── Introspection ───────────────────────────────────────────────────────

/// `asyncio.iscoroutine(obj)`
fn asyncio_iscoroutine(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("asyncio.iscoroutine", args, 1)?;
    Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Coroutine(_))))
}

/// `asyncio.iscoroutinefunction(func)`
fn asyncio_iscoroutinefunction(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("asyncio.iscoroutinefunction", args, 1)?;
    let is_coro_fn = match &args[0].payload {
        PyObjectPayload::Function(pf) => pf.code.flags.contains(CodeFlags::COROUTINE),
        _ => false,
    };
    Ok(PyObject::bool_val(is_coro_fn))
}
