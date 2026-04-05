#![allow(dead_code)]
//! Event loop — single-threaded cooperative scheduler.
//!
//! Models CPython's `asyncio.AbstractEventLoop` with:
//! - Task queue (FIFO scheduling)
//! - Timer-based callbacks (for `asyncio.sleep()`)
//! - Running/stopped state tracking
//! - Lifecycle management (run_until_complete, run_forever, stop, close)

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectRef};
use indexmap::IndexMap;
use parking_lot::Mutex;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::task::TaskHandle;

/// The state of an event loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventLoopState {
    /// Not yet started
    Created,
    /// Actively running tasks
    Running,
    /// Stopped (can be restarted)
    Stopped,
    /// Closed (cannot be restarted)
    Closed,
}

/// A scheduled callback with optional delay.
#[derive(Clone)]
struct ScheduledCallback {
    /// When to fire (None = fire immediately on next iteration)
    fire_at: Option<Instant>,
    /// The callback: either a coroutine to resume or a native function
    callback: CallbackKind,
}

/// What kind of callback is scheduled.
#[derive(Clone)]
enum CallbackKind {
    /// Resume a coroutine task
    ResumeTask(TaskHandle),
    /// Call a Python callable with args
    CallPython(PyObjectRef, Vec<PyObjectRef>),
}

// Thread-local reference to the running event loop (for `get_running_loop()`).
thread_local! {
    static RUNNING_LOOP: RefCell<Option<PyObjectRef>> = RefCell::new(None);
}

/// Single-threaded cooperative event loop.
pub struct EventLoop {
    state: EventLoopState,
    /// Ready queue: tasks ready to be resumed
    ready: VecDeque<TaskHandle>,
    /// Scheduled callbacks (timers, call_later, etc.)
    scheduled: Vec<ScheduledCallback>,
    /// All tasks managed by this loop
    tasks: Vec<TaskHandle>,
    /// Monotonic clock for scheduling
    start_time: Instant,
}

impl EventLoop {
    /// Create a new event loop.
    pub fn new() -> Self {
        Self {
            state: EventLoopState::Created,
            ready: VecDeque::new(),
            scheduled: Vec::new(),
            tasks: Vec::new(),
            start_time: Instant::now(),
        }
    }

    /// Get the current state.
    pub fn state(&self) -> EventLoopState {
        self.state
    }

    /// Check if the loop is running.
    pub fn is_running(&self) -> bool {
        self.state == EventLoopState::Running
    }

    /// Check if the loop is closed.
    pub fn is_closed(&self) -> bool {
        self.state == EventLoopState::Closed
    }

    /// Schedule a task to be resumed on the next iteration.
    pub fn schedule_task(&mut self, task: TaskHandle) {
        self.ready.push_back(task);
    }

    /// Schedule a callback after a delay.
    pub fn call_later(&mut self, delay: Duration, task: TaskHandle) {
        self.scheduled.push(ScheduledCallback {
            fire_at: Some(Instant::now() + delay),
            callback: CallbackKind::ResumeTask(task),
        });
    }

    /// Get the next ready task (checking timers first).
    pub fn poll_ready(&mut self) -> Option<TaskHandle> {
        // Check scheduled callbacks: move any that are due into the ready queue
        let now = Instant::now();
        let mut i = 0;
        while i < self.scheduled.len() {
            let fire = self.scheduled[i].fire_at.unwrap_or(now);
            if fire <= now {
                let cb = self.scheduled.remove(i);
                match cb.callback {
                    CallbackKind::ResumeTask(task) => {
                        self.ready.push_back(task);
                    }
                    CallbackKind::CallPython(_, _) => {
                        // Python callbacks would need VM integration
                    }
                }
            } else {
                i += 1;
            }
        }
        self.ready.pop_front()
    }

    /// Check if there's any pending work.
    pub fn has_pending(&self) -> bool {
        !self.ready.is_empty() || !self.scheduled.is_empty()
    }

    /// Time until the next scheduled callback fires (for sleep optimization).
    pub fn time_until_next(&self) -> Option<Duration> {
        let now = Instant::now();
        self.scheduled
            .iter()
            .filter_map(|s| s.fire_at)
            .min()
            .map(|t| t.saturating_duration_since(now))
    }

    /// Start the event loop.
    pub fn start(&mut self) -> PyResult<()> {
        if self.state == EventLoopState::Closed {
            return Err(PyException::runtime_error("cannot reuse a closed event loop"));
        }
        self.state = EventLoopState::Running;
        Ok(())
    }

    /// Stop the event loop.
    pub fn stop(&mut self) {
        if self.state == EventLoopState::Running {
            self.state = EventLoopState::Stopped;
        }
    }

    /// Close the event loop. Clears all pending callbacks.
    pub fn close(&mut self) {
        self.state = EventLoopState::Closed;
        self.ready.clear();
        self.scheduled.clear();
    }

    /// Get elapsed time since loop creation.
    pub fn time(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }
}

impl Default for EventLoop {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a Python-level EventLoop object that wraps the Rust EventLoop.
pub fn create_event_loop_object() -> PyObjectRef {
    let loop_cls = PyObject::class(
        CompactString::from("EventLoop"),
        vec![],
        IndexMap::new(),
    );

    // Shared state via Arc<Mutex<EventLoop>>
    let loop_state = Arc::new(Mutex::new(EventLoop::new()));

    let mut attrs = IndexMap::new();

    // run_until_complete(coro)
    let ls = loop_state.clone();
    attrs.insert(CompactString::from("run_until_complete"), PyObject::native_closure(
        "EventLoop.run_until_complete",
        move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("run_until_complete() requires a coroutine"));
            }
            let mut loop_ = ls.lock();
            loop_.start()?;
            // Store coroutine in thread-local for VM to drive
            crate::module::store_asyncio_run_coro(args[0].clone());
            Ok(args[0].clone())
        },
    ));

    // close()
    let ls = loop_state.clone();
    attrs.insert(CompactString::from("close"), PyObject::native_closure(
        "EventLoop.close",
        move |_| { ls.lock().close(); Ok(PyObject::none()) },
    ));

    // is_running()
    let ls = loop_state.clone();
    attrs.insert(CompactString::from("is_running"), PyObject::native_closure(
        "EventLoop.is_running",
        move |_| Ok(PyObject::bool_val(ls.lock().is_running())),
    ));

    // is_closed()
    let ls = loop_state.clone();
    attrs.insert(CompactString::from("is_closed"), PyObject::native_closure(
        "EventLoop.is_closed",
        move |_| Ok(PyObject::bool_val(ls.lock().is_closed())),
    ));

    // time()
    let ls = loop_state.clone();
    attrs.insert(CompactString::from("time"), PyObject::native_closure(
        "EventLoop.time",
        move |_| Ok(PyObject::float(ls.lock().time())),
    ));

    // stop()
    let ls = loop_state.clone();
    attrs.insert(CompactString::from("stop"), PyObject::native_closure(
        "EventLoop.stop",
        move |_| { ls.lock().stop(); Ok(PyObject::none()) },
    ));

    // call_soon(callback, *args) — schedule callback for next iteration
    attrs.insert(CompactString::from("call_soon"), PyObject::native_closure(
        "EventLoop.call_soon",
        |_args| Ok(PyObject::none()),
    ));

    // call_later(delay, callback, *args)
    attrs.insert(CompactString::from("call_later"), PyObject::native_closure(
        "EventLoop.call_later",
        |_args| Ok(PyObject::none()),
    ));

    // create_future()
    attrs.insert(CompactString::from("create_future"), PyObject::native_closure(
        "EventLoop.create_future",
        |_| Ok(crate::task::create_future_object()),
    ));

    // create_task(coro)
    attrs.insert(CompactString::from("create_task"), PyObject::native_closure(
        "EventLoop.create_task",
        |args| {
            if args.is_empty() {
                return Err(PyException::type_error("create_task() requires a coroutine"));
            }
            Ok(crate::task::create_task_object(&args[0]))
        },
    ));

    PyObject::instance_with_attrs(loop_cls, attrs)
}

/// Set the running loop reference (called by the event loop when it starts).
pub fn set_running_loop(loop_obj: PyObjectRef) {
    RUNNING_LOOP.with(|c| *c.borrow_mut() = Some(loop_obj));
}

/// Clear the running loop reference.
pub fn clear_running_loop() {
    RUNNING_LOOP.with(|c| *c.borrow_mut() = None);
}

/// Get the currently running event loop, if any.
pub fn get_running_loop() -> Option<PyObjectRef> {
    RUNNING_LOOP.with(|c| c.borrow().clone())
}
