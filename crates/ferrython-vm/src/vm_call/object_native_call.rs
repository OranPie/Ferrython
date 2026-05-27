use ferrython_core::error::PyResult;
use ferrython_core::object::{NativeFunctionData, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_native_function_object(
        &mut self,
        nf_data: &NativeFunctionData,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        if let Some(result) = self.call_ast_or_type_native_object(nf_data, &args)? {
            return Ok(result);
        }
        if nf_data.name.as_str() == "property.__get__" {
            return self.call_property_get_native(&args);
        }
        if nf_data.name.as_str() == "functools.reduce" {
            return self.vm_functools_reduce(&args);
        }
        if nf_data.name.as_str() == "itertools.islice" {
            return self.vm_itertools_islice(&args);
        }
        if nf_data.name.as_str() == "singledispatch.register" {
            return self.vm_singledispatch_register(&args);
        }
        if let Some(result) = self.call_iter_regex_or_path_native_object(nf_data, &args)? {
            return Ok(result);
        }

        let result = (nf_data.func)(&args)?;
        self.finish_native_callable_result(result, false)
    }
}
