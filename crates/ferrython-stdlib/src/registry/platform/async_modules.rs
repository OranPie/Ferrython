use ferrython_core::object::PyObjectRef;

pub(super) fn resolve(name: &str) -> Option<PyObjectRef> {
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
