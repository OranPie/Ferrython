mod collections;
mod filesystem;
mod math;
mod misc;
mod serialization;
mod system;
mod text;
mod time;
mod type_system;

use ferrython_core::object::PyObjectRef;

pub(super) fn math(name: &str) -> Option<PyObjectRef> {
    math::resolve(name)
}

pub(super) fn system(name: &str) -> Option<PyObjectRef> {
    system::resolve(name)
}

pub(super) fn text(name: &str) -> Option<PyObjectRef> {
    text::resolve(name)
}

pub(super) fn collections(name: &str) -> Option<PyObjectRef> {
    collections::resolve(name)
}

pub(super) fn serialization(name: &str) -> Option<PyObjectRef> {
    serialization::resolve(name)
}

pub(super) fn filesystem(name: &str) -> Option<PyObjectRef> {
    filesystem::resolve(name)
}

pub(super) fn time(name: &str) -> Option<PyObjectRef> {
    time::resolve(name)
}

pub(super) fn type_system(name: &str) -> Option<PyObjectRef> {
    type_system::resolve(name)
}

pub(super) fn misc(name: &str) -> Option<PyObjectRef> {
    misc::resolve(name)
}
