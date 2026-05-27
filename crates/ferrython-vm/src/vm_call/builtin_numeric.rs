use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::PyObjectRef;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_numeric_builtin(
        &mut self,
        func: &PyObjectRef,
        name: &CompactString,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        if let Some(result) = self.call_numeric_protocol_builtin(func, name.as_str(), &args)? {
            return Ok(result);
        }
        match name.as_str() {
            "complex" => {
                if let Some(result) = self.call_complex_numeric_builtin(&args)? {
                    return Ok(result);
                }
            }
            "int" | "float" | "round" | "bool" => {
                if let Some(result) = self.call_scalar_numeric_builtin(name.as_str(), &args)? {
                    return Ok(result);
                }
            }
            _ => {}
        }
        crate::builtins::dispatch(name.as_str(), &args)
    }
}
