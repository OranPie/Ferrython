use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::PyObjectRef;

use crate::builtins;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_iterable_builtin(
        &mut self,
        name: &CompactString,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        match name.as_str() {
            "map" => {
                return self.call_map_builtin(&args);
            }
            "filter" => {
                return self.call_filter_builtin(&args);
            }
            "iter" => {
                if let Some(result) = self.call_iter_builtin(&args)? {
                    return Ok(result);
                }
            }
            "next" => {
                return self.call_next_builtin(&args);
            }
            "reversed" => {
                if let Some(result) = self.call_reversed_builtin(&args)? {
                    return Ok(result);
                }
            }
            "enumerate" => {
                return self.call_enumerate_builtin(&args);
            }
            "zip" => {
                return self.call_zip_builtin(&args);
            }
            _ => {}
        }
        match builtins::get_builtin_fn(name.as_str()) {
            Some(f) => f(&args),
            None => Err(PyException::type_error(format!(
                "'{}' is not callable",
                name
            ))),
        }
    }
}
