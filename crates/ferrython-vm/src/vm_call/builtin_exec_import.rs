use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObjectMethods, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_exec_import_builtin(
        &mut self,
        name: &str,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        match name {
            "exec" => self.builtin_exec(&args),
            "eval" => self.builtin_eval(&args),
            "compile" => self.builtin_compile(&args),
            "__import__" => {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "__import__() requires at least 1 argument",
                    ));
                }
                let name = args[0].py_to_string();
                let level = if args.len() >= 5 {
                    args[4].as_int().unwrap_or(0) as usize
                } else {
                    0
                };
                self.import_module_simple(&name, level)
            }
            _ => unreachable!("non-exec/import builtin routed to exec/import dispatch"),
        }
    }
}
