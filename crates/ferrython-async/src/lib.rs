//! Ferrython async runtime — event loop, task scheduling, and asyncio module.
//!
//! This crate provides:
//! - **Event loop**: Single-threaded cooperative event loop with task scheduling
//! - **Task/Future**: State machine types for async operations
//! - **asyncio module**: Complete asyncio Python module API
//! - **Synchronization primitives**: Queue, Event, Semaphore, Lock
//!
//! The event loop is single-threaded and cooperative (like CPython's asyncio).
//! Coroutines yield control via `await` expressions, and the event loop
//! schedules them round-robin.

mod event_loop;
mod module;
mod primitives;
mod task;

pub use event_loop::{EventLoop, EventLoopState};
pub use module::{
    create_asyncio_module, get_wait_for_deadline, set_wait_for_deadline, take_asyncio_run_coro,
};
pub use task::{TaskHandle, TaskState};
