use crate::{concurrency_modules, import_modules, introspection_modules, sys_modules};
use ferrython_core::object::PyObjectRef;

pub(super) fn introspection(name: &str) -> Option<PyObjectRef> {
    match name {
        "warnings" => Some(introspection_modules::create_warnings_module()),
        "traceback" => Some(introspection_modules::create_traceback_module()),
        "inspect" => Some(introspection_modules::create_inspect_module()),
        "dis" => Some(introspection_modules::create_dis_module()),
        "_ast" => Some(introspection_modules::create_ast_module()),
        "linecache" => Some(introspection_modules::create_linecache_module()),
        "token" => Some(introspection_modules::create_token_module()),
        _ => None,
    }
}

pub(super) fn concurrency(name: &str) -> Option<PyObjectRef> {
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

pub(super) fn os_level(name: &str) -> Option<PyObjectRef> {
    match name {
        "mmap" => Some(sys_modules::create_mmap_module()),
        "resource" => Some(sys_modules::create_resource_module()),
        "fcntl" => Some(sys_modules::create_fcntl_module()),
        _ => None,
    }
}

pub(super) fn async_modules(name: &str) -> Option<PyObjectRef> {
    match name {
        "asyncio"
        | "asyncio.events"
        | "asyncio.tasks"
        | "asyncio.futures"
        | "asyncio.queues"
        | "asyncio.locks"
        | "asyncio.runners"
        | "asyncio.streams"
        | "asyncio.subprocess"
        | "asyncio.protocols"
        | "asyncio.transports"
        | "asyncio.exceptions"
        | "asyncio.base_events" => Some(ferrython_async::create_asyncio_module()),
        _ => None,
    }
}

pub(super) fn import_system(name: &str) -> Option<PyObjectRef> {
    match name {
        "importlib" => Some(import_modules::create_importlib_module()),
        "importlib.metadata" => Some(import_modules::create_importlib_metadata_module()),
        _ => None,
    }
}
