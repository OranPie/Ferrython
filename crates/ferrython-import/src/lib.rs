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
        code: Arc<CodeObject>,
        name: CompactString,
        /// Filesystem path of the source file (for __file__ metadata).
        file_path: Option<CompactString>,
    },
}

/// Whether .pth files have already been processed for the initial search paths.
static PTH_PROCESSED: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

// ── Configurable search paths (equivalent to sys.path) ──

static SEARCH_PATHS: LazyLock<RwLock<Vec<PathBuf>>> = LazyLock::new(|| {
    let mut paths = Vec::new();

    // ── stdlib/Lib (pure Python standard library) ──
    // Priority: builtin Rust → Python stdlib/Lib → user PYTHONPATH → current dir
    // (Rust builtins are checked first in resolve_module, before search paths.)

    // Strategy 1: FERRYTHON_STDLIB env variable (explicit override)
    if let Ok(stdlib) = std::env::var("FERRYTHON_STDLIB") {
        let p = PathBuf::from(stdlib);
        if p.is_dir() {
            paths.push(p);
        }
    }

    // Strategy 2: Relative to current exe — walk up ancestors looking for stdlib/Lib.
    // Handles: target/release/ferrython (2 up), target/release/deps/test-bin (3 up), etc.
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..5 {
            if let Some(ref d) = dir {
                let candidate = d.join("stdlib/Lib");
                if let Ok(canon) = candidate.canonicalize() {
                    if canon.is_dir() && !paths.contains(&canon) {
                        paths.push(canon);
                        break;
                    }
                }
                dir = d.parent().map(|p| p.to_path_buf());
            } else {
                break;
            }
        }
    }

    // Strategy 3: Relative to current working directory (./stdlib/Lib)
    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join("stdlib/Lib");
        if candidate.is_dir() {
            let canon = candidate.canonicalize().unwrap_or(candidate);
            if !paths.contains(&canon) {
                paths.push(canon);
            }
        }
    }

    // Read PYTHONPATH environment variable
    if let Ok(pypath) = std::env::var("PYTHONPATH") {
        for p in std::env::split_paths(&pypath) {
            if p.is_dir() && !paths.contains(&p) {
                paths.push(p);
            }
        }
    }

    // ── site-packages discovery ──
    // Check for venv first (VIRTUAL_ENV env var), then system site-packages
    if let Ok(venv) = std::env::var("VIRTUAL_ENV") {
        let venv_site = PathBuf::from(&venv)
            .join("lib").join("ferrython").join("site-packages");
        if venv_site.is_dir() && !paths.contains(&venv_site) {
            paths.push(venv_site);
        }
    }

    // System site-packages relative to the binary
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            // prefix/bin/ferrython → prefix/lib/ferrython/site-packages
            if let Some(prefix) = bin_dir.parent() {
                let site = prefix.join("lib").join("ferrython").join("site-packages");
                if site.is_dir() && !paths.contains(&site) {
                    paths.push(site);
                }
            }
            // Also check alongside the binary (for cargo builds)
            let nearby_site = bin_dir.join("lib").join("ferrython").join("site-packages");
            if nearby_site.is_dir() && !paths.contains(&nearby_site) {
                paths.push(nearby_site);
            }
        }
    }

    // Current directory is always searched last
    paths.push(PathBuf::from("."));

    // Sync search paths to ferrython-core so sys.path can pick them up
    ferrython_core::set_extra_sys_paths(
        paths.iter()
            .map(|p| p.to_string_lossy().to_string())
            .filter(|s| s != "." && !s.is_empty())
            .collect()
    );

    RwLock::new(paths)
});

/// Force initialization of search paths (call early in startup).
/// This ensures site-packages and stdlib paths are discovered before
/// any module is imported, so sys.path reflects them.
pub fn init() {
    let _ = SEARCH_PATHS.read();
}

/// Get a copy of the current search paths (sys.path equivalent).
pub fn get_search_paths() -> Vec<PathBuf> {
    SEARCH_PATHS.read().clone()
}

/// Set the search paths (called when sys.path is modified).
pub fn set_search_paths(paths: Vec<PathBuf>) {
    ferrython_core::set_extra_sys_paths(
        paths.iter()
            .map(|p| p.to_string_lossy().to_string())
            .filter(|s| s != "." && !s.is_empty())
            .collect()
    );
    *SEARCH_PATHS.write() = paths;
}

/// Add a path to the front of the search list.
pub fn prepend_search_path(path: PathBuf) {
    let mut paths = SEARCH_PATHS.write();
    if !paths.contains(&path) {
        paths.insert(0, path.clone());
        // Also update core's extra paths
        drop(paths);
        sync_paths_to_core();
    }
}

/// Add a path to the end of the search list.
pub fn append_search_path(path: PathBuf) {
    let mut paths = SEARCH_PATHS.write();
    if !paths.contains(&path) {
        paths.push(path.clone());
        drop(paths);
        sync_paths_to_core();
    }
}

fn sync_paths_to_core() {
    let paths = SEARCH_PATHS.read();
    ferrython_core::set_extra_sys_paths(
        paths.iter()
            .map(|p| p.to_string_lossy().to_string())
            .filter(|s| s != "." && !s.is_empty())
            .collect()
    );
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
    // Process .pth files on first module resolution
    process_pth_files_once();

    // Force SEARCH_PATHS initialization so extra sys paths are synced to core
    // before any builtin module (like sys) reads them.
    let _ = SEARCH_PATHS.read();

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
                code: Arc::clone(cached),
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
    let code = Arc::new(ferrython_compiler::compile(&ast, &path_str)
        .map_err(|e| PyException::import_error(
            format!("compile error in '{}': {}", path_str, e)
        ))?);

    // Store in cache if we have a valid mtime.
    if let Some(mtime) = mtime {
        BYTECODE_CACHE.lock().insert(
            (canonical, mtime),
            Arc::clone(&code),
        );
    }

    Ok(ResolvedModule::Source {
        code,
        name: CompactString::from(module_name),
        file_path: Some(CompactString::from(path_str)),
    })
}

// ── .pth file processing ──

/// Process .pth files in all site-packages directories (called once on first import).
fn process_pth_files_once() {
    let mut processed = PTH_PROCESSED.lock();
    if *processed {
        return;
    }
    *processed = true;

    let search_paths = SEARCH_PATHS.read().clone();
    let mut new_paths = Vec::new();

    for sp in &search_paths {
        // Look for .pth files in directories that look like site-packages
        if sp.ends_with("site-packages") || sp.to_string_lossy().contains("site-packages") {
            if let Ok(entries) = std::fs::read_dir(sp) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.ends_with(".pth") {
                        if let Ok(content) = std::fs::read_to_string(entry.path()) {
                            for line in content.lines() {
                                let line = line.trim();
                                // Skip empty lines, comments, and import lines
                                if line.is_empty() || line.starts_with('#') || line.starts_with("import ") {
                                    continue;
                                }
                                let path = if Path::new(line).is_absolute() {
                                    PathBuf::from(line)
                                } else {
                                    sp.join(line)
                                };
                                if path.is_dir() && !search_paths.contains(&path) && !new_paths.contains(&path) {
                                    new_paths.push(path);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Add discovered paths to the search list
    if !new_paths.is_empty() {
        let mut paths = SEARCH_PATHS.write();
        for p in new_paths {
            if !paths.contains(&p) {
                // Insert before the "." entry (which is always last)
                let insert_pos = paths.len().saturating_sub(1);
                paths.insert(insert_pos, p);
            }
        }
    }
}

/// Manually trigger .pth file processing for a specific site-packages directory.
pub fn process_pth_in_dir(site_dir: &Path) {
    if !site_dir.is_dir() {
        return;
    }
    let mut new_paths = Vec::new();
    let search_paths = SEARCH_PATHS.read().clone();

    if let Ok(entries) = std::fs::read_dir(site_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".pth") {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    for line in content.lines() {
                        let line = line.trim();
                        if line.is_empty() || line.starts_with('#') || line.starts_with("import ") {
                            continue;
                        }
                        let path = if Path::new(line).is_absolute() {
                            PathBuf::from(line)
                        } else {
                            site_dir.join(line)
                        };
                        if path.is_dir() && !search_paths.contains(&path) && !new_paths.contains(&path) {
                            new_paths.push(path);
                        }
                    }
                }
            }
        }
    }

    if !new_paths.is_empty() {
        let mut paths = SEARCH_PATHS.write();
        for p in new_paths {
            if !paths.contains(&p) {
                let insert_pos = paths.len().saturating_sub(1);
                paths.insert(insert_pos, p);
            }
        }
    }
}
