//! importlib stdlib module — programmatic import API.
//!
//! Provides importlib.import_module() and importlib.reload() that route
//! through the VM's import machinery via thread-local intercept, matching
//! the same pattern used by __import__() and asyncio.run().

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    FxHashKeyMap, new_fx_hashkey_map,
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin, check_args_min,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::cell::RefCell;

// ── Thread-local: import_module() signal ────────────────────────────────
// importlib.import_module(name, package=None) stores the request here.
// The VM intercepts after the call and performs the actual import.

pub struct ImportModuleRequest {
    pub name: CompactString,
    pub package: Option<CompactString>,
}

thread_local! {
    static IMPORT_MODULE_REQ: RefCell<Option<ImportModuleRequest>> = RefCell::new(None);
}

/// Called by the VM post_call_intercept to check if import_module was invoked.
pub fn take_import_module_request() -> Option<ImportModuleRequest> {
    IMPORT_MODULE_REQ.with(|c| c.borrow_mut().take())
}

// ── Thread-local: reload() signal ───────────────────────────────────────

pub struct ReloadRequest {
    pub module: PyObjectRef,
}

thread_local! {
    static RELOAD_REQ: RefCell<Option<ReloadRequest>> = RefCell::new(None);
}

/// Called by the VM post_call_intercept to check if reload was invoked.
pub fn take_reload_request() -> Option<ReloadRequest> {
    RELOAD_REQ.with(|c| c.borrow_mut().take())
}

// ── importlib module ────────────────────────────────────────────────────

pub fn create_importlib_module() -> PyObjectRef {
    make_module("importlib", vec![
        ("import_module", make_builtin(importlib_import_module)),
        ("reload", make_builtin(importlib_reload)),
        ("invalidate_caches", make_builtin(importlib_invalidate_caches)),
        ("__import__", make_builtin(importlib_import_fn)),
        ("util", create_importlib_util_module()),
        ("abc", create_importlib_abc_module()),
        ("machinery", create_importlib_machinery_module()),
        ("metadata", create_importlib_metadata_module()),
    ])
}

// ── importlib.util ──────────────────────────────────────────────────────

fn create_importlib_util_module() -> PyObjectRef {
    // spec_from_file_location(name, location)
    let spec_from_file = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        check_args_min("importlib.util.spec_from_file_location", args, 1)?;
        let name = CompactString::from(args[0].py_to_string());
        let location = if args.len() > 1 && !matches!(&args[1].payload, PyObjectPayload::None) {
            CompactString::from(args[1].py_to_string())
        } else {
            CompactString::from("")
        };
        let cls = PyObject::class(CompactString::from("ModuleSpec"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("name"), PyObject::str_val(name.clone()));
        attrs.insert(CompactString::from("origin"), PyObject::str_val(location.clone()));
        attrs.insert(CompactString::from("submodule_search_locations"), PyObject::none());
        attrs.insert(CompactString::from("loader"), PyObject::none());
        attrs.insert(CompactString::from("cached"), PyObject::none());
        attrs.insert(CompactString::from("parent"), {
            if let Some(dot) = name.rfind('.') {
                PyObject::str_val(CompactString::from(&name[..dot]))
            } else {
                PyObject::str_val(CompactString::from(""))
            }
        });
        attrs.insert(CompactString::from("has_location"), PyObject::bool_val(!location.is_empty()));
        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    // find_spec(name, package=None)
    let find_spec = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        check_args_min("importlib.util.find_spec", args, 1)?;
        let name = args[0].py_to_string();

        let make_spec = |origin: Option<&str>| -> PyObjectRef {
            let cls = PyObject::class(CompactString::from("ModuleSpec"), vec![], IndexMap::new());
            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(name.clone())));
            attrs.insert(CompactString::from("origin"), match origin {
                Some(o) => PyObject::str_val(CompactString::from(o)),
                None => PyObject::none(),
            });
            attrs.insert(CompactString::from("loader"), PyObject::none());
            attrs.insert(CompactString::from("submodule_search_locations"), PyObject::none());
            PyObject::instance_with_attrs(cls, attrs)
        };

        // Check if it's a built-in module
        if crate::is_builtin_module(&name) {
            return Ok(make_spec(Some("built-in")));
        }

        // Search import paths for the module file
        let search_paths = ferrython_core::get_extra_sys_paths();
        let mut all_paths: Vec<String> = vec![".".to_string()];
        all_paths.extend(search_paths);

        let rel_path = name.replace('.', "/");
        for base in &all_paths {
            let base_path = std::path::Path::new(base);
            let file_path = base_path.join(format!("{}.py", rel_path));
            if file_path.exists() {
                return Ok(make_spec(Some(&file_path.to_string_lossy())));
            }
            let init_path = base_path.join(&rel_path).join("__init__.py");
            if init_path.exists() {
                return Ok(make_spec(Some(&init_path.to_string_lossy())));
            }
        }
        Ok(PyObject::none())
    });

    // module_from_spec(spec) — create empty module from spec
    let module_from_spec = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        check_args_min("importlib.util.module_from_spec", args, 1)?;
        let spec = &args[0];
        let name = spec.get_attr("name").map(|n| n.py_to_string()).unwrap_or_else(|| "<unknown>".to_string());
        let module = make_module(&name, vec![]);
        if let PyObjectPayload::Module(ref md) = module.payload {
            let mut attrs = md.attrs.write();
            attrs.insert(CompactString::from("__spec__"), spec.clone());
            if let Some(origin) = spec.get_attr("origin") {
                attrs.insert(CompactString::from("__file__"), origin);
            }
        }
        Ok(module)
    });

    // resolve_name(name, package) — resolve relative module name
    let resolve_name = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        check_args_min("importlib.util.resolve_name", args, 2)?;
        let name = args[0].py_to_string();
        let package = args[1].py_to_string();
        if name.starts_with('.') {
            let dots = name.chars().take_while(|c| *c == '.').count();
            let parts: Vec<&str> = package.split('.').collect();
            if dots > parts.len() {
                return Err(PyException::import_error("attempted relative import beyond top-level package"));
            }
            let base = parts[..parts.len() - (dots - 1).min(parts.len())].join(".");
            let remainder = &name[dots..];
            let resolved = if remainder.is_empty() { base } else { format!("{}.{}", base, remainder) };
            Ok(PyObject::str_val(CompactString::from(resolved)))
        } else {
            Ok(PyObject::str_val(CompactString::from(name)))
        }
    });

    // ModuleSpec class constructor
    let module_spec_cls = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        check_args_min("ModuleSpec", args, 2)?;
        let name = CompactString::from(args[0].py_to_string());
        let loader = args[1].clone();
        // Extract origin from positional arg[2] or kwargs dict (last arg if Dict)
        let mut origin = CompactString::from("");
        let mut is_package = false;
        if args.len() > 2 {
            if let PyObjectPayload::Dict(kw) = &args[args.len() - 1].payload {
                let kw_r = kw.read();
                if let Some(v) = kw_r.get(&HashableKey::str_key(CompactString::from("origin"))) {
                    origin = CompactString::from(v.py_to_string());
                }
                if let Some(v) = kw_r.get(&HashableKey::str_key(CompactString::from("is_package"))) {
                    is_package = v.is_truthy();
                }
            } else {
                origin = CompactString::from(args[2].py_to_string());
            }
        }
        let cls = PyObject::class(CompactString::from("ModuleSpec"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("name"), PyObject::str_val(name.clone()));
        attrs.insert(CompactString::from("loader"), loader);
        attrs.insert(CompactString::from("origin"), if origin.is_empty() { PyObject::none() } else { PyObject::str_val(origin) });
        attrs.insert(CompactString::from("submodule_search_locations"), if is_package { PyObject::list(vec![]) } else { PyObject::none() });
        attrs.insert(CompactString::from("cached"), PyObject::none());
        attrs.insert(CompactString::from("parent"), {
            if let Some(dot) = name.rfind('.') {
                PyObject::str_val(CompactString::from(&name[..dot]))
            } else {
                PyObject::str_val(CompactString::from(""))
            }
        });
        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    make_module("importlib.util", vec![
        ("spec_from_file_location", spec_from_file),
        ("find_spec", find_spec),
        ("module_from_spec", module_from_spec),
        ("resolve_name", resolve_name),
        ("ModuleSpec", module_spec_cls),
        ("MAGIC_NUMBER", PyObject::bytes(vec![0x42, 0x0d, 0x0d, 0x0a])),
    ])
}

// ── importlib.abc ───────────────────────────────────────────────────────

fn create_importlib_abc_module() -> PyObjectRef {
    let finder = PyObject::class(CompactString::from("Finder"), vec![], IndexMap::new());
    make_module("importlib.abc", vec![
        ("Finder", finder.clone()),
        ("Loader", PyObject::class(CompactString::from("Loader"), vec![], IndexMap::new())),
        ("MetaPathFinder", PyObject::class(CompactString::from("MetaPathFinder"), vec![finder.clone()], IndexMap::new())),
        ("PathEntryFinder", PyObject::class(CompactString::from("PathEntryFinder"), vec![finder], IndexMap::new())),
        ("ResourceLoader", PyObject::class(CompactString::from("ResourceLoader"), vec![], IndexMap::new())),
        ("InspectLoader", PyObject::class(CompactString::from("InspectLoader"), vec![], IndexMap::new())),
        ("ExecutionLoader", PyObject::class(CompactString::from("ExecutionLoader"), vec![], IndexMap::new())),
        ("SourceLoader", PyObject::class(CompactString::from("SourceLoader"), vec![], IndexMap::new())),
        ("FileLoader", PyObject::class(CompactString::from("FileLoader"), vec![], IndexMap::new())),
    ])
}

// ── importlib.machinery ─────────────────────────────────────────────────

fn create_importlib_machinery_module() -> PyObjectRef {
    make_module("importlib.machinery", vec![
        ("ModuleSpec", PyObject::class(CompactString::from("ModuleSpec"), vec![], IndexMap::new())),
        ("SourceFileLoader", PyObject::class(CompactString::from("SourceFileLoader"), vec![], IndexMap::new())),
        ("SOURCE_SUFFIXES", PyObject::list(vec![
            PyObject::str_val(CompactString::from(".py")),
        ])),
        ("BYTECODE_SUFFIXES", PyObject::list(vec![
            PyObject::str_val(CompactString::from(".pyc")),
        ])),
        ("EXTENSION_SUFFIXES", PyObject::list(vec![])),
        ("all_suffixes", make_builtin(|_| {
            Ok(PyObject::list(vec![
                PyObject::str_val(CompactString::from(".py")),
                PyObject::str_val(CompactString::from(".pyc")),
            ]))
        })),
    ])
}

/// importlib.import_module(name, package=None)
/// Resolve a module name, handling relative imports when package is given.
fn importlib_import_module(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("importlib.import_module", args, 1)?;
    let name = args[0].py_to_string();
    let package = if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
        Some(CompactString::from(args[1].py_to_string()))
    } else {
        None
    };

    // For relative imports (leading dots), package is required
    if name.starts_with('.') && package.is_none() {
        return Err(PyException::type_error(
            "importlib.import_module() requires package argument for relative imports"
        ));
    }

    IMPORT_MODULE_REQ.with(|r| {
        *r.borrow_mut() = Some(ImportModuleRequest {
            name: CompactString::from(name),
            package,
        });
    });
    ferrython_core::object::set_intercept_pending();
    // Return placeholder — VM replaces with actual module
    Ok(PyObject::none())
}

/// importlib.reload(module)
/// Re-execute a module's code, updating the existing module dict.
fn importlib_reload(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() != 1 {
        return Err(PyException::type_error("reload() takes exactly 1 argument"));
    }
    if !matches!(&args[0].payload, PyObjectPayload::Module(_)) {
        return Err(PyException::type_error("reload() argument must be a module"));
    }
    RELOAD_REQ.with(|r| {
        *r.borrow_mut() = Some(ReloadRequest {
            module: args[0].clone(),
        });
    });
    ferrython_core::object::set_intercept_pending();
    Ok(args[0].clone())
}

/// importlib.invalidate_caches() — no-op for now
fn importlib_invalidate_caches(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let _ = args;
    Ok(PyObject::none())
}

/// importlib.__import__() — same as builtins.__import__
fn importlib_import_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("__import__", args, 1)?;
    let name = args[0].py_to_string();
    let _level = if args.len() >= 5 {
        args[4].as_int().unwrap_or(0) as usize
    } else {
        0
    };
    // Reuse the same import_module mechanism
    IMPORT_MODULE_REQ.with(|r| {
        *r.borrow_mut() = Some(ImportModuleRequest {
            name: CompactString::from(name),
            package: None,
        });
    });
    ferrython_core::object::set_intercept_pending();
    Ok(PyObject::none())
}

// ── importlib.metadata ──────────────────────────────────────────────────
// Provides metadata about installed packages (version, name, etc.)

pub fn create_importlib_metadata_module() -> PyObjectRef {
    make_module("importlib.metadata", vec![
        ("version", make_builtin(metadata_version)),
        ("metadata", make_builtin(metadata_metadata)),
        ("packages_distributions", make_builtin(metadata_packages_distributions)),
        ("requires", make_builtin(metadata_requires)),
        ("distributions", make_builtin(metadata_distributions)),
        ("entry_points", make_builtin(metadata_entry_points)),
        ("files", make_builtin(metadata_files)),
        ("PackageNotFoundError", PyObject::exception_type(
            ferrython_core::error::ExceptionKind::ModuleNotFoundError
        )),
    ])
}

/// Read installed package metadata from dist-info directories.
/// Searches site-packages using the toolchain's discovered layout and binary-relative paths.
fn find_dist_info(package_name: &str) -> Option<std::path::PathBuf> {
    let normalized = package_name.to_lowercase().replace('-', "_");
    let layout = ferrython_toolchain::paths::InstallLayout::discover();

    let home = std::env::var("HOME").unwrap_or_default();
    let mut search_paths = vec![
        layout.site_packages.clone(),
        std::path::PathBuf::from(format!("{}/.local/lib/ferrython/site-packages", home)),
    ];

    // Search relative to the binary (target/release/lib/ferrython/site-packages)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            search_paths.push(exe_dir.join("lib").join("ferrython").join("site-packages"));
        }
    }

    // Also check cwd-relative site-packages for development
    if let Ok(cwd) = std::env::current_dir() {
        let local_site = cwd.join("lib").join("ferrython").join("site-packages");
        if local_site.is_dir() {
            search_paths.push(local_site);
        }
    }

    // Add system Python dist-packages as fallback
    search_paths.push(std::path::PathBuf::from("/usr/lib/python3/dist-packages"));

    for base in &search_paths {
        if !base.is_dir() { continue; }
        if let Ok(entries) = std::fs::read_dir(base) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".dist-info") {
                    let dist_name = name.trim_end_matches(".dist-info")
                        .split('-').next().unwrap_or("")
                        .to_lowercase().replace('-', "_");
                    if dist_name == normalized {
                        return Some(entry.path());
                    }
                }
            }
        }
    }
    None
}

fn parse_metadata_file(path: &std::path::Path) -> IndexMap<CompactString, CompactString> {
    let mut result = IndexMap::new();
    if let Ok(content) = std::fs::read_to_string(path) {
        for line in content.lines() {
            if line.is_empty() { break; } // stop at body separator
            if let Some((key, value)) = line.split_once(": ") {
                let k = CompactString::from(key.trim());
                let v = CompactString::from(value.trim());
                // For multi-value keys, join with newline
                if let Some(existing) = result.get(&k) {
                    let joined = CompactString::from(format!("{}\n{}", existing, v));
                    result.insert(k, joined);
                } else {
                    result.insert(k, v);
                }
            }
        }
    }
    result
}

fn metadata_version(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("importlib.metadata.version", args, 1)?;
    let name = args[0].as_str()
        .ok_or_else(|| PyException::type_error("version() argument must be str"))?;
    if let Some(dist_info) = find_dist_info(name) {
        let metadata_path = dist_info.join("METADATA");
        let meta = parse_metadata_file(&metadata_path);
        if let Some(version) = meta.get("Version") {
            return Ok(PyObject::str_val(version.clone()));
        }
    }
    Err(PyException::runtime_error(format!(
        "No package metadata found for '{}'", name
    )))
}

fn metadata_metadata(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("importlib.metadata.metadata", args, 1)?;
    let name = args[0].as_str()
        .ok_or_else(|| PyException::type_error("metadata() argument must be str"))?;
    if let Some(dist_info) = find_dist_info(name) {
        let metadata_path = dist_info.join("METADATA");
        let meta = parse_metadata_file(&metadata_path);
        let mut dict_map = IndexMap::new();
        for (k, v) in &meta {
            dict_map.insert(
                HashableKey::str_key(k.clone()),
                PyObject::str_val(v.clone()),
            );
            // Also store lowercase key for case-insensitive access
            // (CPython's metadata returns email.Message which is case-insensitive)
            let lower = CompactString::from(k.to_lowercase());
            if lower != *k {
                dict_map.insert(
                    HashableKey::str_key(lower),
                    PyObject::str_val(v.clone()),
                );
            }
        }
        return Ok(PyObject::dict(dict_map));
    }
    Err(PyException::runtime_error(format!(
        "No package metadata found for '{}'", name
    )))
}

fn metadata_packages_distributions(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::dict(IndexMap::new()))
}

fn metadata_requires(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("importlib.metadata.requires", args, 1)?;
    let name = args[0].as_str()
        .ok_or_else(|| PyException::type_error("requires() argument must be str"))?;
    if let Some(dist_info) = find_dist_info(name) {
        let metadata_path = dist_info.join("METADATA");
        let _meta = parse_metadata_file(&metadata_path);
        let mut requires = Vec::new();
        // Collect all "Requires-Dist" entries
        if let Ok(content) = std::fs::read_to_string(&metadata_path) {
            for line in content.lines() {
                if line.starts_with("Requires-Dist: ") {
                    let req = line.trim_start_matches("Requires-Dist: ");
                    requires.push(PyObject::str_val(CompactString::from(req)));
                }
            }
        }
        if requires.is_empty() {
            return Ok(PyObject::none());
        }
        return Ok(PyObject::list(requires));
    }
    Ok(PyObject::none())
}

/// List all installed distributions (packages) by scanning site-packages.
fn metadata_distributions(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let layout = ferrython_toolchain::paths::InstallLayout::discover();
    let home = std::env::var("HOME").unwrap_or_default();
    let mut search_paths = vec![
        layout.site_packages.clone(),
        std::path::PathBuf::from(format!("{}/.local/lib/ferrython/site-packages", home)),
    ];
    if let Ok(cwd) = std::env::current_dir() {
        let local_site = cwd.join("lib").join("ferrython").join("site-packages");
        if local_site.is_dir() {
            search_paths.push(local_site);
        }
    }

    let mut distributions = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for base in &search_paths {
        if !base.is_dir() { continue; }
        if let Ok(entries) = std::fs::read_dir(base) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".dist-info") {
                    let dist_name = name.trim_end_matches(".dist-info")
                        .split('-').next().unwrap_or("")
                        .to_string();
                    if !dist_name.is_empty() && seen.insert(dist_name.clone()) {
                        let metadata_path = entry.path().join("METADATA");
                        let meta = parse_metadata_file(&metadata_path);
                        let mut attrs = IndexMap::new();
                        attrs.insert(CompactString::from("name"),
                            PyObject::str_val(CompactString::from(
                                meta.get("Name").map(|s| s.as_str()).unwrap_or(&dist_name)
                            )));
                        attrs.insert(CompactString::from("version"),
                            PyObject::str_val(CompactString::from(
                                meta.get("Version").map(|s| s.as_str()).unwrap_or("0.0.0")
                            )));
                        attrs.insert(CompactString::from("_path"),
                            PyObject::str_val(CompactString::from(
                                entry.path().to_string_lossy().as_ref()
                            )));
                        let cls = PyObject::class(
                            CompactString::from("Distribution"),
                            vec![], IndexMap::new(),
                        );
                        distributions.push(PyObject::instance_with_attrs(cls, attrs));
                    }
                }
            }
        }
    }
    Ok(PyObject::list(distributions))
}

fn metadata_entry_points(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Ok(PyObject::list(vec![]))
}

fn metadata_files(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("importlib.metadata.files", args, 1)?;
    let name = args[0].as_str()
        .ok_or_else(|| PyException::type_error("files() argument must be str"))?;
    if let Some(dist_info) = find_dist_info(name) {
        let record_path = dist_info.join("RECORD");
        if let Ok(content) = std::fs::read_to_string(&record_path) {
            let files: Vec<PyObjectRef> = content.lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| {
                    let file_path = l.split(',').next().unwrap_or(l);
                    PyObject::str_val(CompactString::from(file_path.trim()))
                })
                .collect();
            return Ok(PyObject::list(files));
        }
    }
    Ok(PyObject::none())
}

// ── importlib.resources ──

pub fn create_importlib_resources_module() -> PyObjectRef {
    use std::path::PathBuf;

    // Helper: create a Traversable path object with read_bytes, read_text, joinpath
    fn make_traversable(pkg_path: String) -> PyObjectRef {
        let cls = PyObject::class(CompactString::from("Path"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("_path"), PyObject::str_val(CompactString::from(&pkg_path)));

            let pp = pkg_path.clone();
            w.insert(CompactString::from("joinpath"), PyObject::native_closure("joinpath", move |a| {
                let child = if !a.is_empty() { a[0].py_to_string() } else { String::new() };
                let full = format!("{}/{}", pp, child);
                Ok(make_traversable(full))
            }));

            let pp2 = pkg_path.clone();
            w.insert(CompactString::from("__truediv__"), PyObject::native_closure("__truediv__", move |a| {
                let child = if a.len() > 1 { a[1].py_to_string() } else if !a.is_empty() { a[0].py_to_string() } else { String::new() };
                let full = format!("{}/{}", pp2, child);
                Ok(make_traversable(full))
            }));

            let pp3 = pkg_path.clone();
            w.insert(CompactString::from("read_bytes"), PyObject::native_closure("read_bytes", move |_| {
                // Search site-packages for the path
                let search = [
                    PathBuf::from(&pp3),
                    PathBuf::from(format!("target/release/lib/ferrython/site-packages/{}", pp3)),
                ];
                for p in &search {
                    if p.exists() {
                        match std::fs::read(p) {
                            Ok(data) => return Ok(PyObject::bytes(data)),
                            Err(e) => return Err(PyException::os_error(format!("cannot read {}: {}", p.display(), e))),
                        }
                    }
                }
                Err(PyException::file_not_found_error(format!("resource not found: {}", pp3)))
            }));

            let pp4 = pkg_path.clone();
            w.insert(CompactString::from("read_text"), PyObject::native_closure("read_text", move |args| {
                let encoding = if !args.is_empty() { args[0].py_to_string() } else { "utf-8".to_string() };
                let _ = encoding; // always UTF-8 for now
                let search = [
                    PathBuf::from(&pp4),
                    PathBuf::from(format!("target/release/lib/ferrython/site-packages/{}", pp4)),
                ];
                for p in &search {
                    if p.exists() {
                        match std::fs::read_to_string(p) {
                            Ok(data) => return Ok(PyObject::str_val(CompactString::from(&data))),
                            Err(e) => return Err(PyException::os_error(format!("cannot read {}: {}", p.display(), e))),
                        }
                    }
                }
                Err(PyException::file_not_found_error(format!("resource not found: {}", pp4)))
            }));

            let pp5 = pkg_path.clone();
            w.insert(CompactString::from("__str__"), PyObject::native_closure("__str__", move |_| {
                Ok(PyObject::str_val(CompactString::from(&pp5)))
            }));

            let pp6 = pkg_path;
            w.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(
                pp6.rsplit('/').next().unwrap_or(&pp6)
            )));
        }
        inst
    }

    // files(package) — return a Traversable for the package directory
    let files_fn = make_builtin(|args: &[PyObjectRef]| {
        let pkg_name = if !args.is_empty() { args[0].py_to_string() } else { String::new() };
        let pkg_path = pkg_name.replace('.', "/");
        Ok(make_traversable(pkg_path))
    });

    // read_text(package, resource, encoding='utf-8', errors='strict')
    let read_text_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("importlib.resources.read_text", args, 2)?;
        let pkg = args[0].py_to_string().replace('.', "/");
        let resource = args[1].py_to_string();
        let path = PathBuf::from(&pkg).join(&resource);
        match std::fs::read_to_string(&path) {
            Ok(content) => Ok(PyObject::str_val(CompactString::from(&content))),
            Err(e) => Err(PyException::runtime_error(format!("resource not found: {}", e))),
        }
    });

    // read_binary(package, resource)
    let read_binary_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("importlib.resources.read_binary", args, 2)?;
        let pkg = args[0].py_to_string().replace('.', "/");
        let resource = args[1].py_to_string();
        let path = PathBuf::from(&pkg).join(&resource);
        match std::fs::read(&path) {
            Ok(data) => Ok(PyObject::bytes(data)),
            Err(e) => Err(PyException::runtime_error(format!("resource not found: {}", e))),
        }
    });

    // path(package, resource) — context manager yielding path
    let path_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("importlib.resources.path", args, 2)?;
        let pkg = args[0].py_to_string().replace('.', "/");
        let resource = args[1].py_to_string();
        let full = format!("{}/{}", pkg, resource);
        let path_obj = PyObject::str_val(CompactString::from(&full));
        // Wrap in a context manager
        let cls = PyObject::class(CompactString::from("_ResourcePath"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            let p = path_obj.clone();
            w.insert(CompactString::from("__enter__"), PyObject::native_closure("__enter__", move |_| Ok(p.clone())));
            w.insert(CompactString::from("__exit__"), make_builtin(|_| Ok(PyObject::bool_val(false))));
        }
        Ok(inst)
    });

    // is_resource(package, name)
    let is_resource_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("importlib.resources.is_resource", args, 2)?;
        let pkg = args[0].py_to_string().replace('.', "/");
        let name = args[1].py_to_string();
        let path = PathBuf::from(&pkg).join(&name);
        Ok(PyObject::bool_val(path.is_file()))
    });

    // contents(package)
    let contents_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("importlib.resources.contents", args, 1)?;
        let pkg = args[0].py_to_string().replace('.', "/");
        let path = PathBuf::from(&pkg);
        match std::fs::read_dir(&path) {
            Ok(entries) => {
                let names: Vec<PyObjectRef> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| PyObject::str_val(CompactString::from(e.file_name().to_string_lossy().as_ref())))
                    .collect();
                Ok(PyObject::list(names))
            }
            Err(_) => Ok(PyObject::list(vec![])),
        }
    });

    // as_file(traversable) — context manager that yields a path to the resource
    let as_file_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("as_file() requires a traversable argument"));
        }
        let traversable = args[0].clone();
        // Get the path string from the traversable
        let path_str = traversable.get_attr("_path")
            .map(|p| p.py_to_string())
            .or_else(|| Some(traversable.py_to_string()))
            .unwrap_or_default();
        // Try to find the actual file in known locations
        let search = [
            PathBuf::from(&path_str),
            PathBuf::from(format!("target/release/lib/ferrython/site-packages/{}", path_str)),
        ];
        let resolved = search.iter()
            .find(|p| p.exists())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(path_str);
        let resolved_obj = PyObject::str_val(CompactString::from(&resolved));
        // Create a pathlib.Path-like context manager
        let cls = PyObject::class(CompactString::from("_AsFilePath"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            let p = resolved_obj.clone();
            w.insert(CompactString::from("__enter__"), PyObject::native_closure("__enter__", move |_| {
                // Return a pathlib.Path-like object
                let path_cls = PyObject::class(CompactString::from("PosixPath"), vec![], IndexMap::new());
                let path_inst = PyObject::instance(path_cls);
                if let PyObjectPayload::Instance(ref pd) = path_inst.payload {
                    let mut pw = pd.attrs.write();
                    pw.insert(CompactString::from("_path"), p.clone());
                    let ps = p.py_to_string();
                    pw.insert(CompactString::from("__str__"), PyObject::native_closure("__str__", move |_| {
                        Ok(PyObject::str_val(CompactString::from(&ps)))
                    }));
                    let ps2 = p.py_to_string();
                    pw.insert(CompactString::from("__fspath__"), PyObject::native_closure("__fspath__", move |_| {
                        Ok(PyObject::str_val(CompactString::from(&ps2)))
                    }));
                }
                Ok(path_inst)
            }));
            w.insert(CompactString::from("__exit__"), make_builtin(|_| Ok(PyObject::bool_val(false))));
        }
        Ok(inst)
    });

    make_module("importlib.resources", vec![
        ("files", files_fn),
        ("as_file", as_file_fn),
        ("read_text", read_text_fn),
        ("read_binary", read_binary_fn),
        ("path", path_fn),
        ("is_resource", is_resource_fn),
        ("contents", contents_fn),
    ])
}
