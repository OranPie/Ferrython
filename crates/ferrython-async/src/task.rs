#![allow(dead_code)]
//! Task and Future types for the async runtime.
//!
//! Models CPython's `asyncio.Task` and `asyncio.Future`:
//! - Future: A placeholder for a result that will be available later
//! - Task: A Future that wraps a coroutine and drives it to completion
//!
//! State transitions:
//! ```text
//! PENDING → CANCELLED
//! PENDING → FINISHED (with result)
//! PENDING → FINISHED (with exception)
//! ```

use std::rc::Rc;
use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{PyCell, 
    PyObject, PyObjectMethods, PyObjectRef,
};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

/// The state of a Task or Future.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskState {
    /// Waiting for a result
    Pending,
    /// Completed with a result
    Finished,
    /// Cancelled
    Cancelled,
}

/// A handle to a managed task.
#[derive(Clone)]
pub struct TaskHandle {
    /// The coroutine being driven
    pub coroutine: PyObjectRef,
    /// Current state
    pub state: Rc<PyCell<TaskState>>,
    /// Result once finished
    pub result: Rc<PyCell<Option<PyObjectRef>>>,
    /// Exception if failed
    pub exception: Rc<PyCell<Option<PyException>>>,
    /// Name for debugging
    pub name: CompactString,
    /// Callbacks to run when done
    pub done_callbacks: Rc<PyCell<Vec<PyObjectRef>>>,
}

impl TaskHandle {
    /// Create a new task for a coroutine.
    pub fn new(coroutine: PyObjectRef, name: CompactString) -> Self {
        Self {
            coroutine,
            state: Rc::new(PyCell::new(TaskState::Pending)),
            result: Rc::new(PyCell::new(None)),
            exception: Rc::new(PyCell::new(None)),
            name,
            done_callbacks: Rc::new(PyCell::new(Vec::new())),
        }
    }

    /// Check if the task is done (finished or cancelled).
    pub fn is_done(&self) -> bool {
        let state = self.state.read();
        *state != TaskState::Pending
    }

    /// Set the result (transitions to Finished).
    pub fn set_result(&self, result: PyObjectRef) {
        *self.state.write() = TaskState::Finished;
        *self.result.write() = Some(result);
    }

    /// Set an exception (transitions to Finished).
    pub fn set_exception(&self, exc: PyException) {
        *self.state.write() = TaskState::Finished;
        *self.exception.write() = Some(exc);
    }

    /// Cancel the task.
    pub fn cancel(&self) -> bool {
        let mut state = self.state.write();
        if *state == TaskState::Pending {
            *state = TaskState::Cancelled;
            true
        } else {
            false
        }
    }

    /// Get the result, or raise if not done / cancelled / exception.
    pub fn get_result(&self) -> PyResult<PyObjectRef> {
        let state = self.state.read();
        match *state {
            TaskState::Pending => {
                Err(PyException::new(ExceptionKind::RuntimeError, "Result is not ready"))
            }
            TaskState::Cancelled => {
                Err(PyException::new(ExceptionKind::RuntimeError, "Task was cancelled"))
            }
            TaskState::Finished => {
                if let Some(exc) = self.exception.read().clone() {
                    Err(exc)
                } else if let Some(result) = self.result.read().clone() {
                    Ok(result)
                } else {
                    Ok(PyObject::none())
                }
            }
        }
    }
}

/// Create a Python-level Task object wrapping a coroutine.
pub fn create_task_object(coroutine: &PyObjectRef) -> PyObjectRef {
    let task_cls = PyObject::class(
        CompactString::from("Task"),
        vec![],
        IndexMap::new(),
    );

    let handle = TaskHandle::new(coroutine.clone(), CompactString::from("Task"));

    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("_coro"), coroutine.clone());
    attrs.insert(CompactString::from("_state"), PyObject::str_val(CompactString::from("PENDING")));

    // done()
    let h = handle.clone();
    attrs.insert(CompactString::from("done"), PyObject::native_closure(
        "Task.done",
        move |_| Ok(PyObject::bool_val(h.is_done())),
    ));

    // result()
    let h = handle.clone();
    attrs.insert(CompactString::from("result"), PyObject::native_closure(
        "Task.result",
        move |_| h.get_result(),
    ));

    // cancel()
    let h = handle.clone();
    attrs.insert(CompactString::from("cancel"), PyObject::native_closure(
        "Task.cancel",
        move |_| Ok(PyObject::bool_val(h.cancel())),
    ));

    // cancelled()
    let h = handle.clone();
    attrs.insert(CompactString::from("cancelled"), PyObject::native_closure(
        "Task.cancelled",
        move |_| Ok(PyObject::bool_val(*h.state.read() == TaskState::Cancelled)),
    ));

    // add_done_callback(fn)
    let h = handle.clone();
    attrs.insert(CompactString::from("add_done_callback"), PyObject::native_closure(
        "Task.add_done_callback",
        move |args| {
            if !args.is_empty() {
                h.done_callbacks.write().push(args[0].clone());
            }
            Ok(PyObject::none())
        },
    ));

    // remove_done_callback(fn) — returns number removed
    let _h = handle.clone();
    attrs.insert(CompactString::from("remove_done_callback"), PyObject::native_closure(
        "Task.remove_done_callback",
        move |_args| Ok(PyObject::int(0)),
    ));

    // get_name() / set_name(name)
    let name_ref = Rc::new(PyCell::new(CompactString::from("Task")));
    let nr = name_ref.clone();
    attrs.insert(CompactString::from("get_name"), PyObject::native_closure(
        "Task.get_name",
        move |_| Ok(PyObject::str_val(nr.read().clone())),
    ));
    let nr = name_ref.clone();
    attrs.insert(CompactString::from("set_name"), PyObject::native_closure(
        "Task.set_name",
        move |args| {
            if !args.is_empty() {
                *nr.write() = CompactString::from(args[0].py_to_string());
            }
            Ok(PyObject::none())
        },
    ));

    // __repr__
    let h = handle.clone();
    attrs.insert(CompactString::from("__repr__"), PyObject::native_closure(
        "Task.__repr__",
        move |_| {
            let state = match *h.state.read() {
                TaskState::Pending => "pending",
                TaskState::Finished => "finished",
                TaskState::Cancelled => "cancelled",
            };
            Ok(PyObject::str_val(CompactString::from(format!(
                "<Task {} state={}>", h.name, state
            ))))
        },
    ));

    // __await__ — makes Task awaitable; returns the coroutine for await to drive
    let coro = coroutine.clone();
    attrs.insert(CompactString::from("__await__"), PyObject::native_closure(
        "Task.__await__",
        move |_| Ok(coro.clone()),
    ));

    PyObject::instance_with_attrs(task_cls, attrs)
}

/// Create a Python-level Future object.
pub fn create_future_object() -> PyObjectRef {
    let future_cls = PyObject::class(
        CompactString::from("Future"),
        vec![],
        IndexMap::new(),
    );

    let state = Rc::new(PyCell::new(TaskState::Pending));
    let result = Rc::new(PyCell::new(Option::<PyObjectRef>::None));
    let exception = Rc::new(PyCell::new(Option::<PyException>::None));

    let mut attrs = IndexMap::new();

    // done()
    let s = state.clone();
    attrs.insert(CompactString::from("done"), PyObject::native_closure(
        "Future.done",
        move |_| Ok(PyObject::bool_val(*s.read() != TaskState::Pending)),
    ));

    // result()
    let s = state.clone();
    let r = result.clone();
    let e = exception.clone();
    attrs.insert(CompactString::from("result"), PyObject::native_closure(
        "Future.result",
        move |_| {
            match *s.read() {
                TaskState::Pending => Err(PyException::new(
                    ExceptionKind::RuntimeError, "Result is not ready",
                )),
                TaskState::Cancelled => Err(PyException::new(
                    ExceptionKind::RuntimeError, "Future was cancelled",
                )),
                TaskState::Finished => {
                    if let Some(exc) = e.read().clone() {
                        Err(exc)
                    } else {
                        Ok(r.read().clone().unwrap_or_else(PyObject::none))
                    }
                }
            }
        },
    ));

    // set_result(value)
    let s = state.clone();
    let r = result.clone();
    attrs.insert(CompactString::from("set_result"), PyObject::native_closure(
        "Future.set_result",
        move |args| {
            if *s.read() != TaskState::Pending {
                return Err(PyException::new(
                    ExceptionKind::RuntimeError, "Future already done",
                ));
            }
            *s.write() = TaskState::Finished;
            *r.write() = Some(if args.is_empty() { PyObject::none() } else { args[0].clone() });
            Ok(PyObject::none())
        },
    ));

    // set_exception(exc)
    let s = state.clone();
    let e = exception.clone();
    attrs.insert(CompactString::from("set_exception"), PyObject::native_closure(
        "Future.set_exception",
        move |args| {
            if *s.read() != TaskState::Pending {
                return Err(PyException::new(
                    ExceptionKind::RuntimeError, "Future already done",
                ));
            }
            *s.write() = TaskState::Finished;
            let exc = if !args.is_empty() {
                PyException::new(ExceptionKind::Exception, args[0].py_to_string())
            } else {
                PyException::new(ExceptionKind::Exception, "")
            };
            *e.write() = Some(exc);
            Ok(PyObject::none())
        },
    ));

    // cancel()
    let s = state.clone();
    attrs.insert(CompactString::from("cancel"), PyObject::native_closure(
        "Future.cancel",
        move |_| {
            let mut st = s.write();
            if *st == TaskState::Pending {
                *st = TaskState::Cancelled;
                Ok(PyObject::bool_val(true))
            } else {
                Ok(PyObject::bool_val(false))
            }
        },
    ));

    // cancelled()
    let s = state.clone();
    attrs.insert(CompactString::from("cancelled"), PyObject::native_closure(
        "Future.cancelled",
        move |_| Ok(PyObject::bool_val(*s.read() == TaskState::Cancelled)),
    ));

    // __await__ — makes Future awaitable. For our sequential model,
    // return a finished coroutine-like that yields the result when done.
    let s = state.clone();
    let r = result.clone();
    let e = exception.clone();
    attrs.insert(CompactString::from("__await__"), PyObject::native_closure(
        "Future.__await__",
        move |_| {
            match *s.read() {
                TaskState::Finished => {
                    if let Some(exc) = e.read().clone() {
                        Err(exc)
                    } else {
                        Ok(r.read().clone().unwrap_or_else(PyObject::none))
                    }
                }
                TaskState::Cancelled => {
                    Err(PyException::new(ExceptionKind::RuntimeError, "Future was cancelled"))
                }
                TaskState::Pending => {
                    // In sequential mode, return None (will be iterated once)
                    Ok(PyObject::none())
                }
            }
        },
    ));

    PyObject::instance_with_attrs(future_cls, attrs)
}
