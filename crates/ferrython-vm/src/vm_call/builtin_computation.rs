use ferrython_core::error::PyResult;
use ferrython_core::object::PyObjectRef;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_computation_builtin(
        &mut self,
        name: &str,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        match name {
            "sum" => self.call_sum_builtin(&args),
            "sorted" => self.call_sorted_builtin(&args),
            "min" => self.call_min_max_builtin("min", &args, false),
            "max" => self.call_min_max_builtin("max", &args, true),
            _ => unreachable!("non-computation builtin routed to computation dispatch"),
        }
    }
}
