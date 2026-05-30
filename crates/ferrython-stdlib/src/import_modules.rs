//! importlib stdlib module — programmatic import API.
//!
//! Provides importlib.import_module() and importlib.reload() that route
//! through the VM's import machinery via thread-local intercept, matching
//! the same pattern used by __import__() and asyncio.run().

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args_min, make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::cell::RefCell;

mod metadata;
mod resources;

pub use metadata::create_importlib_metadata_module;
pub use resources::create_importlib_resources_module;

fn create_source_file_loader(name: CompactString, location: CompactString) -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_function("SourceFileLoader.__init__", |args| {
            check_args_min("SourceFileLoader.__init__", args, 3)?;
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                let mut attrs = inst.attrs.write();
                attrs.insert(
                    CompactString::from("name"),
                    PyObject::str_val(CompactString::from(args[1].py_to_string())),
                );
                attrs.insert(
                    CompactString::from("path"),
                    PyObject::str_val(CompactString::from(args[2].py_to_string())),
                );
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("exec_module"),
        PyObject::native_function("SourceFileLoader.exec_module", |args| {
            check_args_min("SourceFileLoader.exec_module", args, 2)?;
            let path = args[0]
                .get_attr("path")
                .map(|p| p.py_to_string())
                .ok_or_else(|| PyException::attribute_error("loader has no path"))?;
            let source = std::fs::read_to_string(&path)
                .map_err(|e| PyException::import_error(format!("{}: '{}'", e, path)))?;
            let globals = args[1]
                .get_attr("__dict__")
                .ok_or_else(|| PyException::type_error("exec_module() requires a module"))?;
            let compile_fn = PyObject::builtin_function(CompactString::from("compile"));
            let code = ferrython_core::object::call_callable(
                &compile_fn,
                &[
                    PyObject::str_val(CompactString::from(source)),
                    PyObject::str_val(CompactString::from(path.as_str())),
                    PyObject::str_val(CompactString::from("exec")),
                ],
            )?;
            let exec_fn = PyObject::builtin_function(CompactString::from("exec"));
            ferrython_core::object::call_callable(&exec_fn, &[code, globals])?;
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("get_filename"),
        PyObject::native_function("SourceFileLoader.get_filename", |args| {
            check_args_min("SourceFileLoader.get_filename", args, 1)?;
            Ok(args[0].get_attr("path").unwrap_or_else(PyObject::none))
        }),
    );
    ns.insert(
        CompactString::from("get_data"),
        PyObject::native_function("SourceFileLoader.get_data", |args| {
            check_args_min("SourceFileLoader.get_data", args, 2)?;
            let path = args[1].py_to_string();
            let data = std::fs::read(&path)
                .map_err(|e| PyException::os_error(format!("{}: '{}'", e, path)))?;
            Ok(PyObject::bytes(data))
        }),
    );
    let cls = PyObject::class(CompactString::from("SourceFileLoader"), vec![], ns);
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(data) = &inst.payload {
        let mut attrs = data.attrs.write();
        attrs.insert(CompactString::from("name"), PyObject::str_val(name));
        attrs.insert(CompactString::from("path"), PyObject::str_val(location));
    }
    inst
}

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
    make_module(
        "importlib",
        vec![
            ("import_module", make_builtin(importlib_import_module)),
            ("reload", make_builtin(importlib_reload)),
            (
                "invalidate_caches",
                make_builtin(importlib_invalidate_caches),
            ),
            ("__import__", make_builtin(importlib_import_fn)),
            ("util", create_importlib_util_module()),
            ("abc", create_importlib_abc_module()),
            ("machinery", create_importlib_machinery_module()),
            ("metadata", create_importlib_metadata_module()),
        ],
    )
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
        attrs.insert(
            CompactString::from("origin"),
            PyObject::str_val(location.clone()),
        );
        attrs.insert(
            CompactString::from("submodule_search_locations"),
            PyObject::none(),
        );
        let loader = if location.is_empty() {
            PyObject::none()
        } else {
            create_source_file_loader(name.clone(), location.clone())
        };
        attrs.insert(CompactString::from("loader"), loader);
        attrs.insert(CompactString::from("cached"), PyObject::none());
        attrs.insert(CompactString::from("parent"), {
            if let Some(dot) = name.rfind('.') {
                PyObject::str_val(CompactString::from(&name[..dot]))
            } else {
                PyObject::str_val(CompactString::from(""))
            }
        });
        attrs.insert(
            CompactString::from("has_location"),
            PyObject::bool_val(!location.is_empty()),
        );
        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    // find_spec(name, package=None)
    let find_spec = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        check_args_min("importlib.util.find_spec", args, 1)?;
        let name = args[0].py_to_string();

        let make_spec = |origin: Option<&str>| -> PyObjectRef {
            let cls = PyObject::class(CompactString::from("ModuleSpec"), vec![], IndexMap::new());
            let mut attrs = IndexMap::new();
            attrs.insert(
                CompactString::from("name"),
                PyObject::str_val(CompactString::from(name.clone())),
            );
            attrs.insert(
                CompactString::from("origin"),
                match origin {
                    Some(o) => PyObject::str_val(CompactString::from(o)),
                    None => PyObject::none(),
                },
            );
            attrs.insert(CompactString::from("loader"), PyObject::none());
            attrs.insert(
                CompactString::from("submodule_search_locations"),
                PyObject::none(),
            );
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
        let name = spec
            .get_attr("name")
            .map(|n| n.py_to_string())
            .unwrap_or_else(|| "<unknown>".to_string());
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
                return Err(PyException::import_error(
                    "attempted relative import beyond top-level package",
                ));
            }
            let base = parts[..parts.len() - (dots - 1).min(parts.len())].join(".");
            let remainder = &name[dots..];
            let resolved = if remainder.is_empty() {
                base
            } else {
                format!("{}.{}", base, remainder)
            };
            Ok(PyObject::str_val(CompactString::from(resolved)))
        } else {
            Ok(PyObject::str_val(CompactString::from(name)))
        }
    });

    let cache_from_source = make_builtin(|args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        check_args_min("importlib.util.cache_from_source", args, 1)?;
        let path = args[0].py_to_string();
        let separator = std::path::MAIN_SEPARATOR;
        let (dir, filename) = match path.rfind(separator) {
            Some(pos) => (&path[..pos], &path[pos + 1..]),
            None => ("", path.as_str()),
        };
        let stem = filename.strip_suffix(".py").unwrap_or(filename);
        let cached = format!("{}.cpython-38.pyc", stem);
        let result = if dir.is_empty() {
            format!("__pycache__{}{}", separator, cached)
        } else {
            format!("{}{}__pycache__{}{}", dir, separator, separator, cached)
        };
        Ok(PyObject::str_val(CompactString::from(result)))
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
                if let Some(v) = kw_r.get(&HashableKey::str_key(CompactString::from("is_package")))
                {
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
        attrs.insert(
            CompactString::from("origin"),
            if origin.is_empty() {
                PyObject::none()
            } else {
                PyObject::str_val(origin)
            },
        );
        attrs.insert(
            CompactString::from("submodule_search_locations"),
            if is_package {
                PyObject::list(vec![])
            } else {
                PyObject::none()
            },
        );
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

    make_module(
        "importlib.util",
        vec![
            ("spec_from_file_location", spec_from_file),
            ("find_spec", find_spec),
            ("module_from_spec", module_from_spec),
            ("resolve_name", resolve_name),
            ("cache_from_source", cache_from_source),
            ("ModuleSpec", module_spec_cls),
            (
                "MAGIC_NUMBER",
                PyObject::bytes(vec![0x42, 0x0d, 0x0d, 0x0a]),
            ),
        ],
    )
}

// ── importlib.abc ───────────────────────────────────────────────────────

fn create_importlib_abc_module() -> PyObjectRef {
    let finder = PyObject::class(CompactString::from("Finder"), vec![], IndexMap::new());
    make_module(
        "importlib.abc",
        vec![
            ("Finder", finder.clone()),
            (
                "Loader",
                PyObject::class(CompactString::from("Loader"), vec![], IndexMap::new()),
            ),
            (
                "MetaPathFinder",
                PyObject::class(
                    CompactString::from("MetaPathFinder"),
                    vec![finder.clone()],
                    IndexMap::new(),
                ),
            ),
            (
                "PathEntryFinder",
                PyObject::class(
                    CompactString::from("PathEntryFinder"),
                    vec![finder],
                    IndexMap::new(),
                ),
            ),
            (
                "ResourceLoader",
                PyObject::class(
                    CompactString::from("ResourceLoader"),
                    vec![],
                    IndexMap::new(),
                ),
            ),
            (
                "InspectLoader",
                PyObject::class(
                    CompactString::from("InspectLoader"),
                    vec![],
                    IndexMap::new(),
                ),
            ),
            (
                "ExecutionLoader",
                PyObject::class(
                    CompactString::from("ExecutionLoader"),
                    vec![],
                    IndexMap::new(),
                ),
            ),
            (
                "SourceLoader",
                PyObject::class(CompactString::from("SourceLoader"), vec![], IndexMap::new()),
            ),
            (
                "FileLoader",
                PyObject::class(CompactString::from("FileLoader"), vec![], IndexMap::new()),
            ),
        ],
    )
}

// ── importlib.machinery ─────────────────────────────────────────────────

fn create_importlib_machinery_module() -> PyObjectRef {
    make_module(
        "importlib.machinery",
        vec![
            (
                "ModuleSpec",
                PyObject::class(CompactString::from("ModuleSpec"), vec![], IndexMap::new()),
            ),
            (
                "SourceFileLoader",
                PyObject::class(
                    CompactString::from("SourceFileLoader"),
                    vec![],
                    IndexMap::new(),
                ),
            ),
            (
                "SOURCE_SUFFIXES",
                PyObject::list(vec![PyObject::str_val(CompactString::from(".py"))]),
            ),
            (
                "BYTECODE_SUFFIXES",
                PyObject::list(vec![PyObject::str_val(CompactString::from(".pyc"))]),
            ),
            ("EXTENSION_SUFFIXES", PyObject::list(vec![])),
            (
                "all_suffixes",
                make_builtin(|_| {
                    Ok(PyObject::list(vec![
                        PyObject::str_val(CompactString::from(".py")),
                        PyObject::str_val(CompactString::from(".pyc")),
                    ]))
                }),
            ),
        ],
    )
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
            "importlib.import_module() requires package argument for relative imports",
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
        return Err(PyException::type_error(
            "reload() argument must be a module",
        ));
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
