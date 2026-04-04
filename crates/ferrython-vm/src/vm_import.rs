//! Module import system — single source of truth for all import operations.
//!
//! Consolidates import logic previously scattered across opcodes.rs (ImportName),
//! vm_helpers.rs (execute_import), and vm_call.rs (__import__ special case).
//!
//! All import paths converge here:
//! - `import foo` / `from foo import bar` → ImportName opcode → import_module_dotted()
//! - `__import__('foo')` builtin → post_call_intercept → import_module_simple()
//! - `importlib.import_module('foo')` → post_call_intercept → import_module_simple()
//! - `importlib.reload(mod)` → post_call_intercept → reload_module()

use crate::frame::Frame;
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, SharedGlobals};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

impl VirtualMachine {
    // ── Public import entry points ──────────────────────────────────────

    /// Import a single (possibly dotted) module name.
    /// Used by `__import__()` builtin and `importlib.import_module()`.
    pub(crate) fn import_module_simple(&mut self, name: &str, level: usize) -> PyResult<PyObjectRef> {
        self.ensure_sys_modules();

        // For simple (non-dotted) names, fast path
        let _top_name = name.split('.').next().unwrap_or(name);
        if !name.contains('.') {
            return self.resolve_single_module(name, level);
        }

        // Dotted import: resolve each component
        let importer_file = self.current_filename();
        let parts: Vec<&str> = name.split('.').collect();
        let mut current_name = String::new();
        let mut last_module = None;

        for (i, part) in parts.iter().enumerate() {
            if i > 0 { current_name.push('.'); }
            current_name.push_str(part);

            let module = self.resolve_single_module_with_filename(
                &current_name,
                if level > 0 && i == 0 { level } else { 0 },
                &importer_file,
            )?;

            // Attach submodule to parent
            if let Some(ref parent) = last_module {
                self.attach_submodule(parent, part, &module);
            }

            last_module = Some(module);
        }

        Ok(last_module.unwrap_or_else(PyObject::none))
    }

    /// Import a dotted module for the ImportName opcode.
    /// Handles `import a.b.c` (returns top-level `a`) vs `from a.b import c` (returns `a.b`).
    pub(crate) fn import_module_dotted(
        &mut self,
        name: &str,
        level: usize,
        has_fromlist: bool,
        importer_file: &str,
    ) -> PyResult<PyObjectRef> {
        self.ensure_sys_modules();

        let parts: Vec<&str> = name.split('.').collect();
        let mut current_name = String::new();
        let mut parent: Option<PyObjectRef> = None;
        let mut top_level: Option<PyObjectRef> = None;

        for (i, part) in parts.iter().enumerate() {
            if i > 0 { current_name.push('.'); }
            current_name.push_str(part);

            let module = self.resolve_single_module_with_filename(
                &current_name,
                if level > 0 && i == 0 { level } else { 0 },
                importer_file,
            )?;

            // Attach submodule to parent (e.g., os.path on os)
            if let Some(ref p) = parent {
                self.attach_submodule(p, part, &module);
            }

            if i == 0 { top_level = Some(module.clone()); }
            parent = Some(module);
        }

        // `import a.b.c` pushes top-level `a` (STORE_NAME will bind it)
        // `from a.b import c` pushes the final module `a.b` (IMPORT_FROM extracts `c`)
        Ok(if has_fromlist {
            parent.unwrap_or_else(PyObject::none)
        } else {
            top_level.unwrap_or_else(PyObject::none)
        })
    }

    /// Re-execute a module's source, updating the cache.
    pub(crate) fn reload_module(&mut self, module: PyObjectRef) -> PyResult<PyObjectRef> {
        let (mod_name, file_path) = if let PyObjectPayload::Module(ref md) = module.payload {
            let attrs = md.attrs.read();
            let name = attrs.get("__name__")
                .map(|v| v.py_to_string())
                .unwrap_or_else(|| md.name.to_string());
            let file = attrs.get("__file__")
                .map(|v| v.py_to_string());
            (name, file)
        } else {
            return Err(PyException::type_error("reload() argument must be a module"));
        };

        let file_str = file_path.unwrap_or_default();
        if file_str.is_empty() {
            // Builtin module — reload from stdlib
            if let Some(fresh) = ferrython_stdlib::load_module(&mod_name) {
                self.cache_module(&mod_name, &fresh);
                return Ok(fresh);
            }
            return Err(PyException::import_error(format!(
                "module '{}' has no __file__ attribute (cannot reload)", mod_name
            )));
        }

        let importer_file = self.current_filename();
        let resolved = ferrython_import::resolve_module(&mod_name, &importer_file)?;
        match resolved {
            ferrython_import::ResolvedModule::Builtin(m) => {
                self.cache_module(&mod_name, &m);
                Ok(m)
            }
            ferrython_import::ResolvedModule::Source { code, name: rmod_name, file_path: rfp } => {
                self.modules.swap_remove(mod_name.as_str());
                self.exec_module_source(&mod_name, code, rmod_name, rfp)
            }
        }
    }

    // ── Internal helpers ────────────────────────────────────────────────

    /// Resolve a single module name (no dots). Check cache → stdlib → filesystem.
    fn resolve_single_module(&mut self, name: &str, level: usize) -> PyResult<PyObjectRef> {
        let importer_file = self.current_filename();
        self.resolve_single_module_with_filename(name, level, &importer_file)
    }

    /// Resolve a single module name with an explicit importer filename.
    fn resolve_single_module_with_filename(
        &mut self,
        name: &str,
        level: usize,
        importer_file: &str,
    ) -> PyResult<PyObjectRef> {
        // 1. Check cache
        if let Some(module) = self.modules.get(name) {
            return Ok(module.clone());
        }

        // 2. Try stdlib
        let _base_name = name.split('.').last().unwrap_or(name);
        if let Some(module) = ferrython_stdlib::load_module(name) {
            self.cache_module(name, &module);
            return Ok(module);
        }

        // 3. Try filesystem
        let resolved = if level > 0 {
            ferrython_import::resolve_relative_import(name, importer_file, level)?
        } else {
            ferrython_import::resolve_module(name, importer_file)?
        };

        match resolved {
            ferrython_import::ResolvedModule::Builtin(m) => {
                self.cache_module(name, &m);
                Ok(m)
            }
            ferrython_import::ResolvedModule::Source { code, name: mod_name, file_path } => {
                self.exec_module_source(name, code, mod_name, file_path)
            }
        }
    }

    /// Compile and execute module source, with circular import protection.
    fn exec_module_source(
        &mut self,
        cache_name: &str,
        code: ferrython_bytecode::CodeObject,
        mod_name: CompactString,
        file_path: Option<CompactString>,
    ) -> PyResult<PyObjectRef> {
        // Build shared globals with metadata
        let globals: SharedGlobals = Arc::new(RwLock::new(IndexMap::new()));
        {
            let mut g = globals.write();
            g.insert(CompactString::from("__name__"), PyObject::str_val(mod_name.clone()));
            if let Some(ref fp) = file_path {
                g.insert(CompactString::from("__file__"), PyObject::str_val(fp.clone()));
            }
            let pkg = if let Some(pos) = cache_name.rfind('.') {
                &cache_name[..pos]
            } else {
                ""
            };
            g.insert(CompactString::from("__package__"), PyObject::str_val(CompactString::from(pkg)));
        }

        // Circular import protection: insert partial module before executing
        let partial_mod = PyObject::module_with_attrs(mod_name.clone(), globals.read().clone());
        self.cache_module(cache_name, &partial_mod);

        // Execute module body
        let frame = Frame::new(Arc::new(code), globals.clone(), Arc::clone(&self.builtins));
        self.call_stack.push(frame);
        let exec_result = self.run_frame();
        self.call_stack.pop();

        // Build final module from executed globals
        let final_mod = PyObject::module_with_attrs(mod_name, globals.read().clone());
        self.cache_module(cache_name, &final_mod);

        exec_result?;
        Ok(final_mod)
    }

    /// Attach a submodule as an attribute of a parent module.
    fn attach_submodule(&self, parent: &PyObjectRef, name: &str, child: &PyObjectRef) {
        if let PyObjectPayload::Module(ref mod_data) = &parent.payload {
            if mod_data.attrs.read().get(name).is_none() {
                mod_data.attrs.write().insert(CompactString::from(name), child.clone());
            }
        }
    }

    /// Get the current frame's filename (for import resolution).
    fn current_filename(&self) -> String {
        self.call_stack.last()
            .map(|f| f.code.filename.as_str().to_string())
            .unwrap_or_default()
    }

    // ── Module cache management ─────────────────────────────────────────

    /// Cache a module in both VM.modules and sys.modules dict.
    pub(crate) fn cache_module(&mut self, name: &str, module: &PyObjectRef) {
        self.modules.insert(CompactString::from(name), module.clone());
        if let Some(ref sys_mod_dict) = self.sys_modules_dict {
            if let PyObjectPayload::Dict(ref d) = sys_mod_dict.payload {
                d.write().insert(
                    HashableKey::Str(CompactString::from(name)),
                    module.clone(),
                );
            }
        }
    }

    /// Initialize sys.modules reference from the sys module.
    /// Called lazily on first import to avoid circular initialization.
    pub(crate) fn ensure_sys_modules(&mut self) {
        if self.sys_modules_dict.is_some() { return; }
        let sys_mod = if let Some(m) = self.modules.get("sys") {
            m.clone()
        } else if let Some(m) = ferrython_stdlib::load_module("sys") {
            self.modules.insert(CompactString::from("sys"), m.clone());
            m
        } else {
            return;
        };
        if let Some(modules_dict) = sys_mod.get_attr("modules") {
            if matches!(&modules_dict.payload, PyObjectPayload::Dict(_)) {
                self.sys_modules_dict = Some(modules_dict.clone());
                if let PyObjectPayload::Dict(ref d) = modules_dict.payload {
                    for (name, module) in &self.modules {
                        d.write().insert(
                            HashableKey::Str(name.clone()),
                            module.clone(),
                        );
                    }
                }
            }
        }
    }
}
