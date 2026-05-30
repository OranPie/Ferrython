//! Introspection stdlib modules (warnings, traceback, inspect, dis)

use compact_str::CompactString;
use ferrython_bytecode::CodeFlags;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, check_args_min, make_builtin, make_module, new_fx_hashkey_map, to_shared_fx,
    FxHashKeyMap, InstanceData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

mod ast;
pub(crate) use ast::ast_empty_fields_node_names;
pub use ast::{ast_unparse_module, create_ast_module, module_ast_to_pyobject};
mod ast_convert;
pub use ast_convert::pyobj_ast_to_module;
mod dis;
pub use dis::create_dis_module;
mod inspect;
pub use inspect::create_inspect_module;
mod linecache;
pub use linecache::create_linecache_module;
mod symtable;
pub use symtable::create_symtable_module;
mod token;
pub use token::create_token_module;
mod tokenize;
pub use tokenize::create_tokenize_module;
mod traceback;
pub use traceback::create_traceback_module;
mod warnings;
pub use warnings::create_warnings_module;
pub(crate) use warnings::emit_deprecation_warning;

fn clean_docstring(doc: &str) -> String {
    let lines: Vec<&str> = doc.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    let mut indent = usize::MAX;
    for line in lines.iter().skip(1) {
        if !line.trim().is_empty() {
            indent = indent.min(line.len() - line.trim_start().len());
        }
    }
    if indent == usize::MAX {
        indent = 0;
    }

    let mut trimmed: Vec<String> = Vec::with_capacity(lines.len());
    trimmed.push(lines[0].trim().to_string());
    for line in lines.iter().skip(1) {
        let text = if line.len() >= indent {
            &line[indent..]
        } else {
            line.trim()
        };
        trimmed.push(text.trim_end().to_string());
    }

    while trimmed.first().map(|line| line.is_empty()).unwrap_or(false) {
        trimmed.remove(0);
    }
    while trimmed.last().map(|line| line.is_empty()).unwrap_or(false) {
        trimmed.pop();
    }
    trimmed.join("\n")
}
