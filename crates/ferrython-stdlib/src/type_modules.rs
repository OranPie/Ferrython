//! Type-system stdlib modules (typing, abc, enum, types, collections.abc)

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    check_args, check_args_min, make_builtin, make_module, new_fx_hashkey_map, CompareOp,
    FxHashKeyFlatMap, FxHashKeyMap, PyCell, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

mod abc;
mod collections_abc;
mod enum_module;
mod types_module;
mod typing;

pub use abc::create_abc_module;
pub use collections_abc::create_collections_abc_module;
pub use enum_module::create_enum_module;
pub use types_module::create_types_module;
pub use typing::create_typing_module;
