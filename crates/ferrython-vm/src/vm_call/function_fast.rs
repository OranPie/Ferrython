use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObjectPayload, PyObjectRef};
use ferrython_core::types::PyFunction;

use crate::frame::{Frame, ScopeKind};
use crate::VirtualMachine;

impl VirtualMachine {
    pub(crate) fn call_object_one_arg_fast_or_fallback(
        &mut self,
        func: PyObjectRef,
        arg: PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        let pyfunc_ptr = match &func.payload {
            PyObjectPayload::Function(pyfunc)
                if pyfunc.is_simple
                    && !pyfunc.has_code_override()
                    && pyfunc.code.arg_count == 1
                    && pyfunc.defaults.read().is_empty()
                    && pyfunc.kw_defaults.read().is_empty()
                    && pyfunc.closure.is_empty() =>
            {
                &**pyfunc as *const PyFunction
            }
            _ => return self.call_object(func, vec![arg]),
        };

        if ferrython_stdlib::is_trace_active() || ferrython_stdlib::is_profile_active() {
            return self.call_object(func, vec![arg]);
        }

        // `func` owns the payload behind this pointer and is moved into the borrowed
        // frame below, so the code/globals/constant cache stay alive while it runs.
        let pyfunc = unsafe { &*pyfunc_ptr };
        if let Some(result) = Self::try_inline_simple_function_one_arg(pyfunc, &arg) {
            return Ok(result);
        }

        let mut frame =
            unsafe { Frame::new_borrowed(pyfunc, func, &self.builtins, &mut self.frame_pool) };
        frame.locals[0] = Some(arg);
        frame.scope_kind = ScopeKind::Function;

        self.call_stack.push(frame);
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

    pub(crate) fn call_object_two_arg_fast_or_fallback(
        &mut self,
        func: PyObjectRef,
        arg0: PyObjectRef,
        arg1: PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        let pyfunc_ptr = match &func.payload {
            PyObjectPayload::Function(pyfunc)
                if pyfunc.is_simple
                    && !pyfunc.has_code_override()
                    && pyfunc.code.arg_count == 2
                    && pyfunc.defaults.read().is_empty()
                    && pyfunc.kw_defaults.read().is_empty()
                    && pyfunc.closure.is_empty() =>
            {
                &**pyfunc as *const PyFunction
            }
            _ => return self.call_object(func, vec![arg0, arg1]),
        };

        if ferrython_stdlib::is_trace_active() || ferrython_stdlib::is_profile_active() {
            return self.call_object(func, vec![arg0, arg1]);
        }

        let pyfunc = unsafe { &*pyfunc_ptr };
        let args = [arg0, arg1];
        if let Some(result) = Self::try_inline_simple_function_args(pyfunc, &args) {
            return Ok(result);
        }

        let mut frame =
            unsafe { Frame::new_borrowed(pyfunc, func, &self.builtins, &mut self.frame_pool) };
        let [arg0, arg1] = args;
        frame.locals[0] = Some(arg0);
        frame.locals[1] = Some(arg1);
        frame.scope_kind = ScopeKind::Function;

        self.call_stack.push(frame);
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
