mod async_modules;
mod concurrency;
mod import_system;
mod introspection;
mod os_level;

use ferrython_core::object::PyObjectRef;

pub(super) fn introspection(name: &str) -> Option<PyObjectRef> {
    introspection::resolve(name)
}

pub(super) fn concurrency(name: &str) -> Option<PyObjectRef> {
    concurrency::resolve(name)
}

pub(super) fn os_level(name: &str) -> Option<PyObjectRef> {
    os_level::resolve(name)
}

pub(super) fn async_modules(name: &str) -> Option<PyObjectRef> {
    async_modules::resolve(name)
}

pub(super) fn import_system(name: &str) -> Option<PyObjectRef> {
    import_system::resolve(name)
}
