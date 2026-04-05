//! Ferrython import system — module resolution, compilation, and caching.
//!
//! This crate handles the `import` statement pipeline:
//! 1. Check builtin modules (via ferrython-stdlib)
//! 2. Check FFI native modules (via ferrython-ffi)
//! 3. Search the filesystem for `.py` files using search paths
//! 4. Parse and compile source to bytecode
//!
//! The actual *execution* of module code happens in the VM — this crate returns
//! compiled `CodeObject`s that the VM executes.

use compact_str::CompactString;
use ferrython_bytecode::code::CodeObject;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::PyObjectRef;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::SystemTime;

/// Result of resolving an import: either a pre-built module or compiled source.
pub enum ResolvedModule {
    /// A built-in module (from ferrython-stdlib or FFI), ready to use.
    Builtin(PyObjectRef),
    /// Source code compiled to bytecode — VM must execute it to produce the module.
    Source {
        code: CodeObject,
        name: CompactString,
        /// Filesystem path of the source file (for __file__ metadata).
        file_path: Option<CompactString>,
    },
}

// ── Configurable search paths (equivalent to sys.path) ──

static SEARCH_PATHS: LazyLock<RwLock<Vec<PathBuf>>> = LazyLock::new(|| {
    let mut paths = Vec::new();

    // Read PYTHONPATH environment variable
    if let Ok(pypath) = std::env::var("PYTHONPATH") {
        for p in std::env::split_paths(&pypath) {
            if p.is_dir() {
                paths.push(p);
            }
        }
    }

    // Current directory is always searched
    paths.push(PathBuf::from("."));

    RwLock::new(paths)
});

/// Get a copy of the current search paths (sys.path equivalent).
pub fn get_search_paths() -> Vec<PathBuf> {
    SEARCH_PATHS.read().clone()
}

/// Set the search paths (called when sys.path is modified).
pub fn set_search_paths(paths: Vec<PathBuf>) {
    *SEARCH_PATHS.write() = paths;
}

/// Add a path to the front of the search list.
pub fn prepend_search_path(path: PathBuf) {
    let mut paths = SEARCH_PATHS.write();
    if !paths.contains(&path) {
        paths.insert(0, path);
    }
}

/// Add a path to the end of the search list.
pub fn append_search_path(path: PathBuf) {
    let mut paths = SEARCH_PATHS.write();
    if !paths.contains(&path) {
        paths.push(path);
    }
}

/// Resolve a module by name.
///
/// Search order:
/// 1. Builtin stdlib modules (ferrython-stdlib)
/// 2. FFI native modules (ferrython-ffi)
/// 3. Filesystem: importer directory, then each entry in search_paths
///
/// Supports dotted names (`a.b.c` → `a/b/c.py` or `a/b/c/__init__.py`).
pub fn resolve_module(name: &str, importer_filename: &str) -> PyResult<ResolvedModule> {
    // 1. Check builtin modules
    if let Some(module) = ferrython_stdlib::load_module(name) {
        return Ok(ResolvedModule::Builtin(module));
    }

    // 2. Check FFI native modules
    if let Some(module) = ferrython_ffi::load_native_module(name) {
        return Ok(ResolvedModule::Builtin(module));
    }

    // 3. Build search directory list
    let module_path = name.replace('.', "/");
    let importer_dir = Path::new(importer_filename)
        .parent()
        .unwrap_or(Path::new("."));

    let search_paths = SEARCH_PATHS.read();
    let mut dirs: Vec<PathBuf> = Vec::with_capacity(1 + search_paths.len());
    dirs.push(importer_dir.to_path_buf());
    for sp in search_paths.iter() {
        if !dirs.contains(sp) {
            dirs.push(sp.clone());
        }
    }
    drop(search_paths);

    // 4. Search filesystem
    for dir in &dirs {
        let candidates = [
            dir.join(format!("{}.py", module_path)),
            dir.join(format!("{}/__init__.py", module_path)),
        ];
        for candidate in &candidates {
            if candidate.exists() {
                return compile_source(candidate, name);
            }
        }
    }

    Err(PyException::import_error(format!("No module named '{}'", name)))
}

/// Resolve a relative import (leading dots).
///
/// `level` is the number of dots (e.g., `from ..foo import bar` → level=2, name="foo").
pub fn resolve_relative_import(
    name: &str,
    importer_filename: &str,
    level: usize,
) -> PyResult<ResolvedModule> {
    let mut base = Path::new(importer_filename)
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    for _ in 1..level {
        base = base.parent().unwrap_or(Path::new(".")).to_path_buf();
    }

    if name.is_empty() {
        let init = base.join("__init__.py");
        if init.exists() {
            return compile_source(&init, "<package>");
        }
        return Err(PyException::import_error(
            "attempted relative import with no known parent package"
        ));
    }

    let module_path = name.replace('.', "/");
    let candidates = [
        base.join(format!("{}.py", module_path)),
        base.join(format!("{}/__init__.py", module_path)),
    ];
    for candidate in &candidates {
        if candidate.exists() {
            return compile_source(candidate, name);
        }
    }

    Err(PyException::import_error(format!(
        "No module named '{}' (relative import level={})", name, level
    )))
}

// ── In-memory bytecode cache (keyed by canonical path + mtime) ──

static BYTECODE_CACHE: LazyLock<Mutex<HashMap<(PathBuf, SystemTime), Arc<CodeObject>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Clear the bytecode cache (useful in tests or when reloading).
pub fn clear_bytecode_cache() {
    BYTECODE_CACHE.lock().clear();
}

// ── Internal helpers ──

fn compile_source(path: &Path, module_name: &str) -> PyResult<ResolvedModule> {
    let path_str = path.to_string_lossy().to_string();

    // Check bytecode cache: if the file path + mtime match, reuse compiled code.
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mtime = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok();

    if let Some(mtime) = mtime {
        let cache = BYTECODE_CACHE.lock();
        if let Some(cached) = cache.get(&(canonical.clone(), mtime)) {
            return Ok(ResolvedModule::Source {
                code: CodeObject::clone(&cached),
                name: CompactString::from(module_name),
                file_path: Some(CompactString::from(path_str)),
            });
        }
    }

    let source = std::fs::read_to_string(path)
        .map_err(|e| PyException::import_error(
            format!("cannot read '{}': {}", path_str, e)
        ))?;
    let ast = ferrython_parser::parse(&source, &path_str)
        .map_err(|e| PyException::import_error(
            format!("syntax error in '{}': {}", path_str, e)
        ))?;
    let code = ferrython_compiler::compile(&ast, &path_str)
        .map_err(|e| PyException::import_error(
            format!("compile error in '{}': {}", path_str, e)
        ))?;

    // Store in cache if we have a valid mtime.
    if let Some(mtime) = mtime {
        BYTECODE_CACHE.lock().insert(
            (canonical, mtime),
            Arc::new(code.clone()),
        );
    }

    Ok(ResolvedModule::Source {
        code,
        name: CompactString::from(module_name),
        file_path: Some(CompactString::from(path_str)),
    })
}
