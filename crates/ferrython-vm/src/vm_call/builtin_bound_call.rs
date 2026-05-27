use ferrython_core::error::PyResult;
use ferrython_core::object::{BuiltinBoundMethodData, PyObjectRef};

use crate::builtins;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_builtin_bound_method(
        &mut self,
        bbm: &BuiltinBoundMethodData,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        if let Some(result) = self.call_builtin_bound_fast_path(bbm, &args)? {
            return Ok(result);
        }
        if let Some(result) = self.call_generator_bound_method(bbm, &args)? {
            return Ok(result);
        }

        if let Some(result) = self.call_iterator_or_range_bound_method(bbm, &args)? {
            return Ok(result);
        }

        if let Some(result) = self.call_join_bound_method(bbm, &args)? {
            return Ok(result);
        }
        if let Some(result) = self.call_class_or_property_bound_method(bbm, &args)? {
            return Ok(result);
        }
        if let Some(result) = self.call_namedtuple_deque_or_hashlib_bound_method(bbm, &args)? {
            return Ok(result);
        }
        if let Some(result) = self.call_builtin_type_bound_method(bbm, &args)? {
            return Ok(result);
        }
        if let Some(result) = self.call_list_bound_method(bbm, &args)? {
            return Ok(result);
        }
        if let Some(result) = self.call_format_bound_method(bbm, &args)? {
            return Ok(result);
        }
        builtins::call_method(&bbm.receiver, bbm.method_name.as_str(), &args)
    }
}
