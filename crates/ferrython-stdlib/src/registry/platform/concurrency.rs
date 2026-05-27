use crate::concurrency_modules;
use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
    match name {
        "threading" => Some(concurrency_modules::create_threading_module()),
        "weakref" => Some(concurrency_modules::create_weakref_module()),
        "gc" => Some(concurrency_modules::create_gc_module()),
        "_thread" => Some(concurrency_modules::create_thread_module()),
        "signal" => Some(concurrency_modules::create_signal_module()),
        "multiprocessing" => Some(concurrency_modules::create_multiprocessing_module()),
        "multiprocessing.pool" => Some(concurrency_modules::create_multiprocessing_module()),
        "multiprocessing.managers" => Some(concurrency_modules::create_multiprocessing_module()),
        "multiprocessing.queues" => Some(concurrency_modules::create_multiprocessing_module()),
        "selectors" => Some(concurrency_modules::create_selectors_module()),
        "select" => Some(concurrency_modules::create_select_module()),
        _ => None,
    }
}
