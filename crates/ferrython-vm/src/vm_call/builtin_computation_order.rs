use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObject, PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_sorted_builtin(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if !args.is_empty() {
            let mut items = if let PyObjectPayload::List(ref cell) = args[0].payload {
                if PyObjectRef::strong_count(&args[0]) == 1 {
                    std::mem::take(&mut *cell.write())
                } else {
                    cell.read().clone()
                }
            } else if let PyObjectPayload::Tuple(ref t) = args[0].payload {
                t.to_vec()
            } else {
                self.collect_iterable(&args[0])?
            };
            self.vm_sort(&mut items)?;
            return Ok(PyObject::list(items));
        }
        fallback_computation_builtin("sorted", args)
    }

    pub(super) fn call_min_max_builtin(
        &mut self,
        name: &str,
        args: &[PyObjectRef],
        is_max: bool,
    ) -> PyResult<PyObjectRef> {
        if args.len() == 1 {
            if let Some(r) = self.native_min_max_list(&args[0], is_max)? {
                return Ok(r);
            }
            let items = self.collect_iterable(&args[0])?;
            return self.compute_min_max(items, is_max, None, None, name);
        }
        fallback_computation_builtin(name, args)
    }
}

pub(super) fn fallback_computation_builtin(
    name: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match crate::builtins::get_builtin_fn(name) {
        Some(f) => f(args),
        None => Err(ferrython_core::error::PyException::type_error(format!(
            "'{}' is not callable",
            name
        ))),
    }
}
