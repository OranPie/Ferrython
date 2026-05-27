use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::PyObjectRef;

use crate::builtins;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_builtin_or_type(
        &mut self,
        func: &PyObjectRef,
        name: &CompactString,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        if name.as_str() == "__build_class__" {
            return self.build_class(args);
        }
        if matches!(
            name.as_str(),
            "list" | "tuple" | "set" | "frozenset" | "dict"
        ) {
            return self.call_collection_builtin(name.as_str(), args);
        }
        if matches!(name.as_str(), "any" | "all" | "isinstance" | "issubclass") {
            return self.call_predicate_builtin(name.as_str(), args);
        }
        if matches!(name.as_str(), "sum" | "sorted" | "min" | "max") {
            return self.call_computation_builtin(name.as_str(), args);
        }
        if matches!(name.as_str(), "getattr" | "setattr" | "delattr") {
            return self.call_attr_builtin(name.as_str(), args);
        }
        if matches!(name.as_str(), "globals" | "locals" | "vars" | "dir") {
            return self.call_scope_builtin(name.as_str(), args);
        }
        if matches!(name.as_str(), "str" | "repr" | "mappingproxy") {
            return self.call_text_builtin(name.as_str(), args);
        }
        if matches!(name.as_str(), "exec" | "eval" | "compile" | "__import__") {
            return self.call_exec_import_builtin(name.as_str(), args);
        }
        if name.as_str() == "NamedTuple" {
            if let Some(result) = self.call_namedtuple_builtin(args)? {
                return Ok(result);
            }
            return self.call_static_builtin(name.as_str(), &[]);
        }
        if matches!(
            name.as_str(),
            "map" | "filter" | "iter" | "next" | "reversed" | "enumerate" | "zip"
        ) {
            return self.call_iterable_builtin(name, args);
        }
        if matches!(
            name.as_str(),
            "len"
                | "abs"
                | "hash"
                | "bin"
                | "oct"
                | "hex"
                | "format"
                | "complex"
                | "int"
                | "float"
                | "round"
                | "bool"
        ) {
            return self.call_numeric_builtin(func, name, args);
        }
        // VM-aware builtins that need to call user-defined methods
        match name.as_str() {
            "print" => {
                return self.vm_print(&args, None, None, None, false);
            }
            "bytes" => {
                return self.vm_bytes_constructor(&args, false);
            }
            "bytearray" => {
                return self.vm_bytes_constructor(&args, true);
            }
            "super" => {
                return self.make_super(&args);
            }
            _ => {}
        }
        self.call_static_builtin(name.as_str(), &args)
    }

    fn call_static_builtin(&mut self, name: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        match builtins::get_builtin_fn(name) {
            Some(f) => {
                let result = f(args);
                // Check if breakpoint() was called
                if crate::builtins::core_fns::BREAKPOINT_TRIGGERED
                    .swap(false, std::sync::atomic::Ordering::Relaxed)
                {
                    self.breakpoints.builtin_breakpoint_pending = true;
                    self.handle_breakpoint_hit();
                }
                result
            }
            None => Err(PyException::type_error(format!(
                "'{}' is not callable",
                name
            ))),
        }
    }
}
