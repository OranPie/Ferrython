//! VM execution entry points and frame lifecycle helpers.

use crate::frame::Frame;
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_bytecode::code::CodeObject;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    FxAttrMap, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::{HashableKey, SharedGlobals};
use std::rc::Rc;

impl VirtualMachine {
    pub fn run_atexit(&mut self) -> PyResult<PyObjectRef> {
        let module = self.import_module_simple("atexit", 0)?;
        let Some(run_exitfuncs) = module.get_attr("_run_exitfuncs") else {
            return Ok(PyObject::none());
        };
        self.call_object(run_exitfuncs, Vec::new())
    }

    pub(crate) fn keep_frame_objects_alive(exc: &mut PyException, frame: &Frame) {
        exc.keepalive.extend(frame.stack.iter().cloned());
        exc.keepalive
            .extend(frame.locals.iter().filter_map(|item| item.clone()));
        if let Some(local_names) = &frame.local_names {
            exc.keepalive.extend(local_names.values().cloned());
        }
        for cell in &frame.cells {
            if let Some(value) = cell.read().clone() {
                exc.keepalive.push(value);
            }
        }
        if let Some(obj) = &frame.prepare_dict {
            exc.keepalive.push(obj.clone());
        }
        if let Some(obj) = &frame.exec_locals {
            exc.keepalive.push(obj.clone());
        }
        if let Some(obj) = &frame.exec_globals {
            exc.keepalive.push(obj.clone());
        }
    }

    pub(crate) fn enter_exception_handler(&mut self, exc: PyException) {
        self.exception_state_stack
            .push(self.active_exception.clone());
        ferrython_core::error::set_thread_exc_info(
            exc.kind,
            exc.message.clone(),
            exc.traceback.clone(),
        );
        self.active_exception = Some(exc);
    }

    pub(crate) fn restore_previous_exception(&mut self) {
        self.active_exception = self.exception_state_stack.pop().unwrap_or(None);
        if let Some(exc) = &self.active_exception {
            ferrython_core::error::set_thread_exc_info(
                exc.kind,
                exc.message.clone(),
                exc.traceback.clone(),
            );
        } else {
            ferrython_core::error::clear_thread_exc_info();
        }
    }

    pub(crate) fn builtins_module(&mut self) -> Option<PyObjectRef> {
        if let Some(module) = self.modules.get("builtins") {
            return Some(module.clone());
        }
        let module = ferrython_stdlib::load_module("builtins")?;
        self.cache_module("builtins", &module);
        Some(module)
    }

    /// Execute a Python function object with arguments on this VM.
    /// Used by thread spawning to run Python-defined thread targets.
    pub fn call_function_standalone(
        &mut self,
        func: PyObjectRef,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        self.install_hash_eq_dispatch();
        self.call_object(func, args)
    }

    /// Create a new empty shared globals map.
    pub fn new_globals() -> SharedGlobals {
        Rc::new(PyCell::new(FxAttrMap::default()))
    }

    /// Execute a code object (module-level).
    pub fn execute(&mut self, code: CodeObject) -> PyResult<PyObjectRef> {
        self.install_hash_eq_dispatch();
        let globals = Rc::new(PyCell::new(FxAttrMap::default()));
        {
            let mut g = globals.write();
            g.insert(
                CompactString::from("__name__"),
                PyObject::str_val(CompactString::from("__main__")),
            );
            if !code.filename.is_empty() {
                g.insert(
                    CompactString::from("__file__"),
                    PyObject::str_val(code.filename.clone()),
                );
            }
            if let Some(builtins_mod) = self.builtins_module() {
                g.insert(CompactString::from("__builtins__"), builtins_mod);
            }
        }
        let main_mod =
            PyObject::module_with_shared_globals(CompactString::from("__main__"), globals.clone());
        self.modules
            .insert(CompactString::from("__main__"), main_mod.clone());
        if let Some(ref sys_mod_dict) = self.sys_modules_dict {
            if let PyObjectPayload::Dict(ref d) = sys_mod_dict.payload {
                d.write().insert(
                    HashableKey::str_key(CompactString::from("__main__")),
                    main_mod,
                );
            }
        }
        self.execute_with_globals(Rc::new(code), globals)
    }

    /// Execute a code object with shared globals (for REPL).
    pub fn execute_with_globals(
        &mut self,
        code: Rc<CodeObject>,
        globals: SharedGlobals,
    ) -> PyResult<PyObjectRef> {
        self.execute_with_globals_and_locals(code, globals, None)
    }

    pub(crate) fn execute_with_globals_and_locals(
        &mut self,
        code: Rc<CodeObject>,
        globals: SharedGlobals,
        exec_locals: Option<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        self.execute_with_globals_and_locals_obj(code, globals, exec_locals, None)
    }

    pub(crate) fn execute_with_globals_and_locals_obj(
        &mut self,
        code: Rc<CodeObject>,
        globals: SharedGlobals,
        exec_locals: Option<PyObjectRef>,
        exec_globals: Option<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        self.install_hash_eq_dispatch();
        ferrython_stdlib::set_current_globals(Some(globals.clone()));
        let stack_depth = self.call_stack.len();
        let mut frame = Frame::new(code, globals.clone(), self.builtins.clone());
        frame.exec_locals = exec_locals;
        frame.exec_globals = exec_globals;
        self.call_stack.push(frame);
        let result = self.run_frame();
        while self.call_stack.len() > stack_depth {
            if let Some(frame) = self.call_stack.pop() {
                if self.call_stack.len() == stack_depth && result.is_ok() {
                    if !frame.code.cellvars.is_empty() {
                        let mut g = globals.write();
                        for (i, name) in frame.code.cellvars.iter().enumerate() {
                            if let Some(cell) = frame.cells.get(i) {
                                if let Some(val) = cell.read().as_ref() {
                                    g.insert(name.clone(), val.clone());
                                }
                            }
                        }
                    }
                }
                frame.recycle(&mut self.frame_pool);
            }
        }
        result
    }

    /// Cold helper: generate NameError for unbound locals.
    #[cold]
    #[inline(never)]
    pub(crate) fn err_unbound_local(
        varnames: &[compact_str::CompactString],
        idx: usize,
    ) -> Result<Option<PyObjectRef>, PyException> {
        Err(PyException::unbound_local_error(format!(
            "local variable '{}' referenced before assignment",
            varnames.get(idx).map(|s| s.as_str()).unwrap_or("?")
        )))
    }

    /// Cold helper: generate NameError for unresolved names.
    #[allow(dead_code)]
    #[cold]
    #[inline(never)]
    pub(crate) fn err_name_not_found(name: &str) -> Result<Option<PyObjectRef>, PyException> {
        Err(PyException::name_error(format!(
            "name '{}' is not defined",
            name
        )))
    }

    /// Cold helper: generate NameError with a custom message.
    #[allow(dead_code)]
    #[cold]
    #[inline(never)]
    pub(crate) fn err_name_error_msg(msg: String) -> Result<Option<PyObjectRef>, PyException> {
        Err(PyException::name_error(msg))
    }
}
