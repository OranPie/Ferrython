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
mod task;
mod module;
mod primitives;

pub use event_loop::{EventLoop, EventLoopState};
pub use task::{TaskState, TaskHandle};
pub use module::{create_asyncio_module, take_asyncio_run_coro};
