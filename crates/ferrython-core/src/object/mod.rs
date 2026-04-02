//! The Python object model — `PyObject`, `PyObjectRef`, `PyObjectPayload`.

mod payload;
mod constructors;
mod methods;
mod methods_type;
mod methods_arith;
mod methods_compare;
mod methods_attr;
mod methods_container;
mod methods_format;
mod helpers;

// Re-export all public types and functions
pub use payload::*;
pub use methods::*;
pub use methods_attr::{lookup_in_class_mro, is_data_descriptor, has_descriptor_get};
pub use helpers::*;
pub use constructors::init_gc;
