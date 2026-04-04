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
