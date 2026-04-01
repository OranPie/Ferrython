//! Ferrython import system — module resolution, compilation, and caching.
//!
//! This crate handles the `import` statement pipeline:
//! 1. Check builtin modules (via ferrython-stdlib)
//! 2. Search the filesystem for `.py` files
//! 3. Parse and compile source to bytecode
//!
//! The actual *execution* of module code happens in the VM — this crate returns
//! compiled `CodeObject`s that the VM executes.

use compact_str::CompactString;
use ferrython_bytecode::code::CodeObject;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::PyObjectRef;
use std::path::{Path, PathBuf};

/// Result of resolving an import: either a pre-built module or compiled source.
pub enum ResolvedModule {
    /// A built-in module (from ferrython-stdlib), ready to use.
    Builtin(PyObjectRef),
    /// Source code compiled to bytecode — VM must execute it to produce the module.
    Source {
        code: CodeObject,
        name: CompactString,
    },
}

/// Resolve a module by name.
///
/// Checks builtin modules first, then searches the filesystem relative to
/// the importer's location and the current directory.
pub fn resolve_module(name: &str, importer_filename: &str) -> PyResult<ResolvedModule> {
    // 1. Check builtin modules
    if let Some(module) = ferrython_stdlib::load_module(name) {
        return Ok(ResolvedModule::Builtin(module));
    }

    // 2. Search filesystem
    let module_path = name.replace('.', "/");
    let importer_dir = Path::new(importer_filename)
        .parent()
        .unwrap_or(Path::new("."));
    let search_dirs = [importer_dir.to_path_buf(), PathBuf::from(".")];

    for dir in &search_dirs {
        let candidates = [
            dir.join(format!("{}.py", module_path)),
            dir.join(format!("{}/__init__.py", module_path)),
        ];
        for candidate in &candidates {
            if candidate.exists() {
                let candidate_str = candidate.to_string_lossy().to_string();
                let source = std::fs::read_to_string(candidate)
                    .map_err(|e| PyException::import_error(
                        format!("cannot read '{}': {}", candidate_str, e)
                    ))?;
                let ast = ferrython_parser::parse(&source, &candidate_str)
                    .map_err(|e| PyException::import_error(
                        format!("syntax error in '{}': {}", candidate_str, e)
                    ))?;
                let code = ferrython_compiler::compile(&ast, &candidate_str)
                    .map_err(|e| PyException::import_error(
                        format!("compile error in '{}': {}", candidate_str, e)
                    ))?;
                return Ok(ResolvedModule::Source {
                    code,
                    name: CompactString::from(name),
                });
            }
        }
    }

    Err(PyException::import_error(format!("No module named '{}'", name)))
}