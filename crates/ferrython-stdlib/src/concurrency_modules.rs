//! Concurrency stdlib modules.

use ferrython_core::object::PyObjectRef;
use std::cell::RefCell;

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

mod gc;
mod multiprocessing;
mod select;
mod selectors;
mod signal;
mod thread_module;
mod threading;
mod weakref;

pub use gc::create_gc_module;
pub use multiprocessing::create_multiprocessing_module;
pub use select::create_select_module;
pub use selectors::create_selectors_module;
pub use signal::create_signal_module;
pub use thread_module::create_thread_module;
pub use threading::create_threading_module;
pub use weakref::create_weakref_module;
