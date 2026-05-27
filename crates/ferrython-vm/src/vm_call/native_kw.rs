use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_native_or_fallback_kw(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        // Handle other payload types that support kwargs
        match &func.payload {
            PyObjectPayload::NativeFunction(nf_data) => {
                if let Some(result) = self.call_special_native_kw(nf_data, &pos_args, &kwargs)? {
                    return Ok(result);
                }
                if let Some(result) = self.call_collection_native_kw(nf_data, &pos_args, &kwargs)? {
                    return Ok(result);
                }
                if let Some(result) =
                    self.call_regex_or_iter_native_kw(nf_data, &pos_args, &kwargs)?
                {
                    return Ok(result);
                }
                if let Some(result) = self.call_json_native_kw(nf_data, &pos_args, &kwargs)? {
                    return Ok(result);
                }
                return self.call_native_function_trailing_kw(nf_data, pos_args, kwargs);
            }
            PyObjectPayload::NativeClosure(nc) => {
                return self.call_native_closure_kw(nc, pos_args, kwargs);
            }
            PyObjectPayload::Partial(pd) => {
                return self.call_partial_kw(pd, pos_args, kwargs);
            }
            PyObjectPayload::ExceptionType(kind) => {
                return self.call_exception_type_kw(*kind, pos_args, &kwargs);
            }
            PyObjectPayload::Instance(_) => {
                return self.call_instance_with_kw(func, pos_args, kwargs);
            }
            _ => {}
        }
        self.call_object_with_trailing_kwargs(func, pos_args, kwargs)
    }
}
