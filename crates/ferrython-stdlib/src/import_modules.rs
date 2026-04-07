//! importlib stdlib module — programmatic import API.
//!
//! Provides importlib.import_module() and importlib.reload() that route
//! through the VM's import machinery via thread-local intercept, matching
//! the same pattern used by __import__() and asyncio.run().

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
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
        // Check if module exists in sys.modules (can't check directly without VM)
        // Return a spec if the file exists on disk
        let possible_paths = vec![
            format!("{}.py", name.replace('.', "/")),
            format!("{}/__init__.py", name.replace('.', "/")),
        ];
        for path in &possible_paths {
            if std::path::Path::new(path).exists() {
                let cls = PyObject::class(CompactString::from("ModuleSpec"), vec![], IndexMap::new());
                let mut attrs = IndexMap::new();
                attrs.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(name.clone())));
                attrs.insert(CompactString::from("origin"), PyObject::str_val(CompactString::from(path.as_str())));
                attrs.insert(CompactString::from("loader"), PyObject::none());
                attrs.insert(CompactString::from("submodule_search_locations"), PyObject::none());
                return Ok(PyObject::instance_with_attrs(cls, attrs));
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
                if let Some(v) = kw_r.get(&HashableKey::Str(CompactString::from("origin"))) {
                    origin = CompactString::from(v.py_to_string());
                }
                if let Some(v) = kw_r.get(&HashableKey::Str(CompactString::from("is_package"))) {
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
        ("PackageNotFoundError", PyObject::exception_type(
            ferrython_core::error::ExceptionKind::ModuleNotFoundError
        )),
    ])
}

/// Read installed package metadata from dist-info directories.
/// Searches site-packages using the toolchain's discovered layout.
fn find_dist_info(package_name: &str) -> Option<std::path::PathBuf> {
    let normalized = package_name.to_lowercase().replace('-', "_");
    let layout = ferrython_toolchain::paths::InstallLayout::discover();

    // Search paths: ferrython's site-packages, user site, system locations
    let home = std::env::var("HOME").unwrap_or_default();
    let mut search_paths = vec![
        layout.site_packages.clone(),
        std::path::PathBuf::from(format!("{}/.local/lib/ferrython/site-packages", home)),
    ];

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
            if let Some((key, value)) = line.split_once(": ") {
                result.insert(
                    CompactString::from(key.trim()),
                    CompactString::from(value.trim()),
                );
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
                HashableKey::Str(k.clone()),
                PyObject::str_val(v.clone()),
            );
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

// ── importlib.resources ──

pub fn create_importlib_resources_module() -> PyObjectRef {
    use std::path::PathBuf;

    // files(package) — return a Traversable for the package directory
    let files_fn = make_builtin(|args: &[PyObjectRef]| {
        let pkg_name = if !args.is_empty() { args[0].py_to_string() } else { String::new() };
        // Convert package name to path
        let pkg_path = pkg_name.replace('.', "/");
        let cls = PyObject::class(CompactString::from("Path"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("_path"), PyObject::str_val(CompactString::from(&pkg_path)));

            let pp = pkg_path.clone();
            w.insert(CompactString::from("joinpath"), PyObject::native_closure("joinpath", move |a| {
                let child = if !a.is_empty() { a[0].py_to_string() } else { String::new() };
                let full = format!("{}/{}", pp, child);
                Ok(PyObject::str_val(CompactString::from(&full)))
            }));

            let pp2 = pkg_path.clone();
            w.insert(CompactString::from("__truediv__"), PyObject::native_closure("__truediv__", move |a| {
                let child = if a.len() > 1 { a[1].py_to_string() } else if !a.is_empty() { a[0].py_to_string() } else { String::new() };
                let full = format!("{}/{}", pp2, child);
                Ok(PyObject::str_val(CompactString::from(&full)))
            }));
        }
        Ok(inst)
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

    make_module("importlib.resources", vec![
        ("files", files_fn),
        ("read_text", read_text_fn),
        ("read_binary", read_binary_fn),
        ("path", path_fn),
        ("is_resource", is_resource_fn),
        ("contents", contents_fn),
    ])
}
