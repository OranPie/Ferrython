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

        // Handle `from . import X` (empty name with relative level)
        if name.is_empty() && level > 0 {
            let pkg_name = self.resolve_package_name(importer_file, level);
            if !pkg_name.is_empty() {
                // Return cached parent package module if available
                if let Some(module) = self.modules.get(pkg_name.as_str()) {
                    return Ok(module.clone());
                }
                // Otherwise resolve it
                return self.resolve_single_module_with_filename(
                    &pkg_name, 0, importer_file,
                );
            }
            // Fall through to the relative import resolver
            return self.resolve_single_module_with_filename("", level, importer_file);
        }

        let parts: Vec<&str> = name.split('.').collect();
        let mut current_name = String::new();
        let mut parent: Option<PyObjectRef> = None;
        let mut top_level: Option<PyObjectRef> = None;

        // For relative imports (level > 0), compute the fully-qualified base package name.
        // E.g., `from .util import X` in urllib3/__init__.py → base_pkg = "urllib3",
        // so "util" becomes "urllib3.util" for caching/naming.
        let fq_prefix = if level > 0 {
            let pkg = self.resolve_package_name(importer_file, level);
            if pkg.is_empty() { String::new() } else { format!("{}.", pkg) }
        } else {
            String::new()
        };

        for (i, part) in parts.iter().enumerate() {
            if i > 0 { current_name.push('.'); }
            current_name.push_str(part);

            // Build fully-qualified cache name for relative imports
            let fq_name = if !fq_prefix.is_empty() {
                format!("{}{}", fq_prefix, current_name)
            } else {
                current_name.clone()
            };

            // Check cache with fully-qualified name first
            let module = if let Some(cached) = self.modules.get(fq_name.as_str()) {
                cached.clone()
            } else if fq_prefix.is_empty() {
                // Absolute import: use standard resolution (includes cache)
                if let Some(cached) = self.modules.get(current_name.as_str()) {
                    cached.clone()
                } else {
                    match self.resolve_single_module_with_filename(
                        &current_name,
                        if level > 0 && i == 0 { level } else { 0 },
                        importer_file,
                    ) {
                        Ok(m) => m,
                        Err(e) => {
                            // Fallback: check if parent has this attribute (e.g., six.moves)
                            if let Some(ref p) = parent {
                                if let Some(attr) = p.get_attr(part) {
                                    self.cache_module(&fq_name, &attr);
                                    attr
                                } else {
                                    return Err(e);
                                }
                            } else {
                                return Err(e);
                            }
                        }
                    }
                }
            } else {
                // Relative import: bypass bare-name cache, resolve from filesystem
                let the_level = if i == 0 { level } else { 0 };
                let resolved_mod = if the_level > 0 {
                    ferrython_import::resolve_relative_import(&current_name, importer_file, the_level)
                        .map_err(|e| { e })?
                } else {
                    // For i > 0 in a dotted relative import (e.g., _backends.sync),
                    // resolve the full dotted path relative to the original base,
                    // not the importer file — use the same level as the original import.
                    ferrython_import::resolve_relative_import(&current_name, importer_file, level)?
                };
                let module = match resolved_mod {
                    ferrython_import::ResolvedModule::Builtin(m) => {
                        self.cache_module(&fq_name, &m);
                        m
                    }
                    ferrython_import::ResolvedModule::Source { code, name: mod_name, file_path } => {
                        self.exec_module_source(&fq_name, code, CompactString::from(fq_name.as_str()), file_path)?
                    }
                };
                module
            };

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
        // 1. Check VM cache
        if let Some(module) = self.modules.get(name) {
            return Ok(module.clone());
        }

        // 1b. Check sys.modules dict (catches dynamically-inserted modules).
        // CPython allows *any* object in sys.modules — module proxies, lazy
        // loaders, plain instances, etc.  We accept everything except None
        // (which CPython treats as "module was deleted / import failed").
        if let Some(ref sys_mod_dict) = self.sys_modules_dict {
            if let PyObjectPayload::Dict(ref d) = sys_mod_dict.payload {
                let key = HashableKey::Str(CompactString::from(name));
                if let Some(module) = d.read().get(&key).cloned() {
                    if !matches!(&module.payload, PyObjectPayload::None) {
                        self.modules.insert(CompactString::from(name), module.clone());
                        return Ok(module);
                    }
                }
            }
        }

        // 2. Try stdlib
        let _base_name = name.split('.').last().unwrap_or(name);
        if let Some(module) = ferrython_stdlib::load_module(name) {
            self.cache_module(name, &module);
            // Post-import hook: inject mixin methods for collections.abc
            if name == "collections.abc" || name == "_collections_abc" {
                self.inject_collections_abc_mixins(&module);
            }
            return Ok(module);
        }

        // 3. Try filesystem — sync sys.path to import search paths first
        self.sync_sys_path_to_import();
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
        code: std::sync::Arc<ferrython_bytecode::CodeObject>,
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
            // __package__: for __init__.py, it's the module name itself;
            // for regular modules (foo.bar), it's the parent package (foo).
            let is_init = file_path.as_ref()
                .map(|fp| fp.ends_with("__init__.py"))
                .unwrap_or(false);
            let pkg = if is_init {
                cache_name
            } else if let Some(pos) = cache_name.rfind('.') {
                &cache_name[..pos]
            } else {
                ""
            };
            g.insert(CompactString::from("__package__"), PyObject::str_val(CompactString::from(pkg)));
            g.insert(CompactString::from("__doc__"), PyObject::none());

            // __path__: for packages (__init__.py), set to a list containing the
            // package directory. This is essential for submodule resolution.
            if is_init {
                if let Some(ref fp) = file_path {
                    let p = std::path::Path::new(fp.as_str());
                    if let Some(pkg_dir) = p.parent() {
                        g.insert(
                            CompactString::from("__path__"),
                            PyObject::list(vec![PyObject::str_val(
                                CompactString::from(pkg_dir.to_string_lossy().as_ref()),
                            )]),
                        );
                    }
                }
            }
        }

        // Circular import protection: insert partial module that shares the same
        // globals Arc, so submodules attached during circular imports and names
        // added during execution are all visible through the same module object.
        let partial_mod = PyObject::module_with_shared_globals(mod_name.clone(), globals.clone());
        self.cache_module(cache_name, &partial_mod);

        // Execute module body — writes go to `globals`, which is the same
        // Arc backing partial_mod's attrs.
        let frame = Frame::new(code, globals.clone(), Arc::clone(&self.builtins));
        self.call_stack.push(frame);
        let exec_result = self.run_frame();
        if let Some(frame) = self.call_stack.pop() {
            frame.recycle(&mut self.frame_pool);
        }

        // The partial_mod's attrs are already up-to-date (shared globals).
        // Just propagate it as the final module — no need to rebuild.
        exec_result?;
        Ok(partial_mod)
    }

    /// Attach a submodule as an attribute of a parent module.
    fn attach_submodule(&self, parent: &PyObjectRef, name: &str, child: &PyObjectRef) {
        if let PyObjectPayload::Module(ref mod_data) = &parent.payload {
            mod_data.attrs.write().insert(CompactString::from(name), child.clone());
        }
    }

    /// Get the current frame's filename (for import resolution).
    fn current_filename(&self) -> String {
        self.call_stack.last()
            .map(|f| f.code.filename.as_str().to_string())
            .unwrap_or_default()
    }

    /// Determine the package name from a file path and relative import level.
    /// E.g., for `/path/to/site-packages/urllib3/__init__.py` with level=1,
    /// returns "urllib3". For `/path/to/pkg/sub.py` with level=1, returns "pkg".
    fn resolve_package_name(&self, importer_file: &str, level: usize) -> String {
        // First try __package__ from the current frame's globals
        if let Some(frame) = self.call_stack.last() {
            if let Some(pkg) = frame.globals.read().get("__package__") {
                let pkg_str = pkg.py_to_string();
                if !pkg_str.is_empty() {
                    // For level > 1, go up extra levels
                    if level <= 1 {
                        return pkg_str;
                    }
                    let parts: Vec<&str> = pkg_str.split('.').collect();
                    if parts.len() >= level {
                        return parts[..parts.len() - (level - 1)].join(".");
                    }
                    return pkg_str;
                }
            }
        }
        // Fallback: derive from file path
        let path = std::path::Path::new(importer_file);
        let is_init = path.file_name().map(|f| f == "__init__.py").unwrap_or(false);
        let mut base = if is_init {
            path.parent().unwrap_or(path)
        } else {
            path.parent().unwrap_or(path)
        };
        // Go up extra levels for level > 1
        for _ in 1..level {
            base = base.parent().unwrap_or(base);
        }
        // Try to find the package name from the cached modules
        let dir_name = base.file_name().map(|f| f.to_str().unwrap_or("")).unwrap_or("");
        if self.modules.contains_key(dir_name) {
            return dir_name.to_string();
        }
        dir_name.to_string()
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

    /// Sync sys.path entries to the import system's search paths.
    /// This allows runtime modifications to sys.path (e.g. sys.path.insert(0, '/foo'))
    /// to be picked up by the import resolver.
    fn sync_sys_path_to_import(&self) {
        let sys_mod = if let Some(m) = self.modules.get("sys") {
            m.clone()
        } else {
            return;
        };
        if let Some(path_list) = sys_mod.get_attr("path") {
            if let PyObjectPayload::List(ref items) = path_list.payload {
                let items = items.read();
                let mut paths = Vec::with_capacity(items.len());
                for item in items.iter() {
                    let s = item.py_to_string();
                    if !s.is_empty() {
                        paths.push(std::path::PathBuf::from(s));
                    }
                }
                ferrython_import::set_search_paths(paths);
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

    /// Inject mixin methods into Mapping and MutableMapping ABCs.
    /// These need to be real Python functions so they can call self.__getitem__ etc.
    fn inject_collections_abc_mixins(&mut self, module: &PyObjectRef) {
        let code = r#"
class _MappingMixin:
    def get(self, key, default=None):
        try:
            return self[key]
        except KeyError:
            return default
    def __contains__(self, key):
        try:
            self[key]
        except KeyError:
            return False
        return True
    def keys(self):
        result = []
        for k in self:
            result.append(k)
        return result
    def values(self):
        result = []
        for k in self:
            result.append(self[k])
        return result
    def items(self):
        result = []
        for k in self:
            result.append((k, self[k]))
        return result
    def __eq__(self, other):
        if not isinstance(other, type(self)) and not isinstance(self, type(other)):
            return NotImplemented
        if len(self) != len(other):
            return False
        for key in self:
            if key not in other or self[key] != other[key]:
                return False
        return True
    def __ne__(self, other):
        result = self.__eq__(other)
        if result is NotImplemented:
            return result
        return not result

class _MutableMappingMixin(_MappingMixin):
    def pop(self, key, *args):
        try:
            v = self[key]
        except KeyError:
            if args:
                return args[0]
            raise
        del self[key]
        return v
    def setdefault(self, key, default=None):
        try:
            return self[key]
        except KeyError:
            self[key] = default
        return default
    def update(self, other=(), **kwds):
        if isinstance(other, type({})):
            for key in other:
                self[key] = other[key]
        elif hasattr(other, 'keys'):
            for key in other.keys():
                self[key] = other[key]
        else:
            for key, value in other:
                self[key] = value
        for key in kwds:
            self[key] = kwds[key]
    def clear(self):
        try:
            while True:
                k = next(iter(self))
                del self[k]
        except StopIteration:
            pass
    def popitem(self):
        try:
            key = next(iter(self))
        except StopIteration:
            raise KeyError('dictionary is empty')
        value = self[key]
        del self[key]
        return key, value
"#;

        // Compile and execute the mixin code
        let parse_result = ferrython_parser::parse(code, "<collections_abc_mixin>");
        let ast = match parse_result {
            Ok(ast) => ast,
            Err(_) => return,
        };
        let code_obj = match ferrython_compiler::compile(&ast, "<collections_abc_mixin>") {
            Ok(c) => c,
            Err(_) => return,
        };

        let globals = Arc::new(RwLock::new(IndexMap::new()));
        {
            let mut g = globals.write();
            if let Some(builtins_mod) = ferrython_stdlib::load_module("builtins") {
                g.insert(CompactString::from("__builtins__"), builtins_mod);
            }
        }
        let frame = crate::frame::Frame::new(Arc::new(code_obj), globals.clone(), Arc::clone(&self.builtins));
        self.call_stack.push(frame);
        let _ = self.run_frame();
        if let Some(frame) = self.call_stack.pop() {
            frame.recycle(&mut self.frame_pool);
        }

        let g = globals.read();
        let mapping_mixin = g.get("_MappingMixin").cloned();
        let mutable_mapping_mixin = g.get("_MutableMappingMixin").cloned();
        drop(g);

        // Inject mixin methods into Mapping class
        if let (Some(mixin), Some(mapping)) = (&mapping_mixin, module.get_attr("Mapping")) {
            if let (PyObjectPayload::Class(mixin_cd), PyObjectPayload::Class(target_cd)) = (&mixin.payload, &mapping.payload) {
                let mixin_ns = mixin_cd.namespace.read();
                let mut target_ns = target_cd.namespace.write();
                for (k, v) in mixin_ns.iter() {
                    if !k.starts_with('_') || k == "__contains__" || k == "__eq__" || k == "__ne__" {
                        target_ns.insert(k.clone(), v.clone());
                    }
                }
                target_cd.invalidate_cache();
            }
        }

        // Inject mixin methods into MutableMapping class
        // MutableMapping gets both Mapping mixin methods AND its own
        if let Some(mm) = module.get_attr("MutableMapping") {
            if let PyObjectPayload::Class(target_cd) = &mm.payload {
                let mut target_ns = target_cd.namespace.write();
                // First inject Mapping mixin methods
                if let Some(ref mixin) = mapping_mixin {
                    if let PyObjectPayload::Class(mixin_cd) = &mixin.payload {
                        let mixin_ns = mixin_cd.namespace.read();
                        for (k, v) in mixin_ns.iter() {
                            if !k.starts_with('_') || k == "__contains__" || k == "__eq__" || k == "__ne__" {
                                target_ns.insert(k.clone(), v.clone());
                            }
                        }
                    }
                }
                // Then inject MutableMapping-specific methods (overrides if any)
                if let Some(ref mixin) = mutable_mapping_mixin {
                    if let PyObjectPayload::Class(mixin_cd) = &mixin.payload {
                        let mixin_ns = mixin_cd.namespace.read();
                        for (k, v) in mixin_ns.iter() {
                            if !k.starts_with('_') || k == "__contains__" || k == "__eq__" || k == "__ne__" {
                                target_ns.insert(k.clone(), v.clone());
                            }
                        }
                    }
                }
                target_cd.invalidate_cache();
            }
        }
    }
}
