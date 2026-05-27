//! The Python object model — `PyObject`, `PyObjectRef`, `PyObjectPayload`.

mod constructors;
pub mod helpers;
mod methods;
mod methods_arith;
mod methods_attr;
mod methods_attr_helpers;
mod methods_compare;
mod methods_container;
pub mod methods_format;
mod methods_type;
mod payload;

// Re-export all public types and functions
pub use constructors::alloc_list_box_empty;
pub use constructors::alloc_map_inner;
pub use constructors::alloc_tuple_box_empty;
pub use constructors::init_gc;
pub use helpers::*;
pub use methods::*;
pub use methods_attr::py_has_attr;
pub use methods_attr_helpers::{has_descriptor_get, is_data_descriptor, lookup_in_class_mro};
pub use payload::*;
