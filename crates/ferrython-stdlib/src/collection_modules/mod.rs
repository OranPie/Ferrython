//! Collection and functional stdlib modules

mod collections;
mod functools;
mod itertools;
mod operator;
mod other;

pub use collections::create_collections_module;
pub use functools::create_functools_module;
pub use itertools::create_itertools_module;
pub use operator::create_operator_module;
pub use other::{create_queue_module, create_array_module};
