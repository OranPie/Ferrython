use ferrython_core::error::PyResult;
use ferrython_core::object::{NativeClosureData, PyObject, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_native_closure_object(
        &mut self,
        nc: &NativeClosureData,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        let result = (nc.func)(&args)?;
        self.finish_native_callable_result(result, true)
    }

    pub(super) fn finish_native_callable_result(
        &mut self,
        result: PyObjectRef,
        check_asyncio_run: bool,
    ) -> PyResult<PyObjectRef> {
        let collect_mode = ferrython_core::error::take_collect_vm_call_results();
        if collect_mode {
            let mut collected = Vec::new();
            while let Some((method, margs)) = ferrython_core::error::take_pending_vm_call() {
                collected.push(self.call_object(method, margs)?);
            }
            if !collected.is_empty() {
                return Ok(PyObject::list(collected));
            }
        }
        while let Some((method, margs)) = ferrython_core::error::take_pending_vm_call() {
            self.call_object(method, margs)?;
        }
        let deferred = ferrython_stdlib::drain_deferred_calls();
        for (dfunc, dargs) in deferred {
            self.call_object(dfunc, dargs)?;
        }
        if check_asyncio_run {
            if let Some(coro) = ferrython_stdlib::take_asyncio_run_coro() {
                return self.maybe_await_result(coro);
            }
        }
        Ok(result)
    }
}
