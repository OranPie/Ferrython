use compact_str::CompactString;
use ferrython_bytecode::code::{CodeFlags, CodeObject};
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyCell, PyObject, PyObjectRef};
use std::rc::Rc;

use crate::frame::{Frame, ScopeKind};
use crate::VirtualMachine;

impl VirtualMachine {
    /// Install closure cells, set scope, and either return generator/coroutine or execute frame.
    pub(super) fn install_closure_and_run(
        &mut self,
        mut frame: Frame,
        code: &CodeObject,
        closure: &[Rc<PyCell<Option<PyObjectRef>>>],
        func_name: CompactString,
        func_qualname: CompactString,
    ) -> PyResult<PyObjectRef> {
        let n_cell = code.cellvars.len();
        for (i, cell) in closure.iter().enumerate() {
            if n_cell + i < frame.cells.len() {
                frame.cells[n_cell + i] = cell.clone();
            }
        }
        for (cell_idx, cell_name) in code.cellvars.iter().enumerate() {
            for (var_idx, var_name) in code.varnames.iter().enumerate() {
                if cell_name == var_name {
                    if let Some(val) = frame.locals[var_idx].take() {
                        *frame.cells[cell_idx].write() = Some(val);
                    }
                    break;
                }
            }
        }
        frame.scope_kind = ScopeKind::Function;

        if code.flags.contains(CodeFlags::GENERATOR) && code.flags.contains(CodeFlags::COROUTINE) {
            let code_obj = Rc::clone(&frame.code);
            let ptr = Box::into_raw(Box::new(frame)) as *mut u8;
            return Ok(PyObject::async_generator(
                func_name,
                func_qualname,
                code_obj,
                ptr,
            ));
        }
        if code.flags.contains(CodeFlags::COROUTINE) {
            let code_obj = Rc::clone(&frame.code);
            let ptr = Box::into_raw(Box::new(frame)) as *mut u8;
            return Ok(PyObject::coroutine(func_name, func_qualname, code_obj, ptr));
        }
        if code.flags.contains(CodeFlags::GENERATOR) {
            let code_obj = Rc::clone(&frame.code);
            let ptr = Box::into_raw(Box::new(frame)) as *mut u8;
            return Ok(PyObject::generator(func_name, func_qualname, code_obj, ptr));
        }

        self.call_stack.push(frame);
        // Check recursion limit
        if self.call_stack.len() > self.recursion_limit {
            if let Some(frame) = self.call_stack.pop() {
                frame.recycle(&mut self.frame_pool);
            }
            return Err(PyException::recursion_error(
                "maximum recursion depth exceeded",
            ));
        }
        let result = self.run_frame();
        if let Some(frame) = self.call_stack.pop() {
            frame.recycle(&mut self.frame_pool);
        }
        result
    }
}
