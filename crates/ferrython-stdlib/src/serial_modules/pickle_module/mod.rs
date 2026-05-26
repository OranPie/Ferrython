//! Pickle serialization module split by API, writer, reader, and shared helpers.

mod api;
mod read;
mod shared;
mod write;

pub use api::create_pickle_module;
pub(super) use read::pickle_loads_stack;
pub(super) use write::pickle_serialize;
