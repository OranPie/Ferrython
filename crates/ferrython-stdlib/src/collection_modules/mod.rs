//! Collection and functional stdlib modules

mod collections;
mod functools;
mod itertools;
mod operator;
mod other;
mod user_types;

pub use collections::create_collections_module;
pub(crate) use collections::{namedtuple_rebuild_field, namedtuple_rebuild_instance};
pub use functools::create_functools_module;
pub use itertools::create_itertools_module;
pub use operator::create_operator_module;
pub use other::{create_array_module, create_queue_module};
