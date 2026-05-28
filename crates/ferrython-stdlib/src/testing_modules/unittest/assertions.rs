use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    make_builtin, CompareOp, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;
use std::rc::Rc;

mod basic;
mod collections_regex;
mod exceptions;
mod logs;
mod numeric_types;

use basic::register_basic_assertions;
use collections_regex::register_collection_regex_assertions;
use exceptions::register_exception_assertions;
use logs::register_log_assertions;
use numeric_types::register_numeric_type_assertions;

/// Helper: extract optional message from args at given index.
pub(super) fn assert_msg(args: &[PyObjectRef], idx: usize) -> String {
    if args.len() > idx {
        args[idx].py_to_string()
    } else {
        String::new()
    }
}

pub(super) fn add_assertion_methods(tc_ns: &mut IndexMap<CompactString, PyObjectRef>) {
    register_basic_assertions(tc_ns);
    register_exception_assertions(tc_ns);
    register_numeric_type_assertions(tc_ns);
    register_collection_regex_assertions(tc_ns);
    register_log_assertions(tc_ns);
}
