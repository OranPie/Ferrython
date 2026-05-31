use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::PyObjectRef;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_builtin_kw(
        &mut self,
        func: PyObjectRef,
        name: &CompactString,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        match name.as_str() {
            "__build_class__" => {
                return self.build_class_kw(pos_args, kwargs);
            }
            "sorted" => {
                if let Some(result) = self.builtin_sorted_kw(&pos_args, &kwargs)? {
                    return Ok(result);
                }
            }
            "globals" => {
                return Ok(self.builtin_globals_kw());
            }
            "locals" => {
                return Ok(self.builtin_locals_kw());
            }
            "print" => {
                return self.builtin_print_kw(&pos_args, &kwargs);
            }
            "max" | "min" => {
                let is_max = name.as_str() == "max";
                let key_fn = kwargs
                    .iter()
                    .find(|(k, _)| k.as_str() == "key")
                    .map(|(_, v)| v.clone());
                let default = kwargs
                    .iter()
                    .find(|(k, _)| k.as_str() == "default")
                    .map(|(_, v)| v.clone());
                let items = if pos_args.len() == 1 {
                    self.collect_iterable(&pos_args[0])?
                } else {
                    pos_args.clone()
                };
                return self.compute_min_max(items, is_max, key_fn, default, name.as_str());
            }
            "super" => {
                return self.make_super(&pos_args);
            }
            "dict" => {
                return self.builtin_dict_kw(&pos_args, &kwargs);
            }
            "enumerate" => {
                return self.builtin_enumerate_kw(func, pos_args, &kwargs);
            }
            "int" => {
                return self.builtin_int_kw(func, pos_args, &kwargs);
            }
            "bool" => {
                return self.builtin_bool_kw(func, pos_args, &kwargs);
            }
            "float" | "str" | "bytes" | "bytearray" => {
                return self.call_object(func, pos_args);
            }
            "list" | "set" | "frozenset" => {
                if !kwargs.is_empty() {
                    return Err(PyException::type_error(format!(
                        "{}() takes no keyword arguments",
                        name
                    )));
                }
                return self.call_object(func, pos_args);
            }
            "tuple" => {
                if !kwargs.is_empty() {
                    return Err(PyException::type_error(
                        "tuple() takes no keyword arguments",
                    ));
                }
                return self.call_object(func, pos_args);
            }
            "complex" => {
                return self.builtin_complex_kw(func, pos_args, &kwargs);
            }
            "open" => {
                return self.builtin_open_kw(func, pos_args, &kwargs);
            }
            "property" => {
                return self.builtin_property_kw(func, pos_args, &kwargs);
            }
            "type" => {
                return self.builtin_type_kw(func, pos_args, kwargs);
            }
            _ => {
                return self.call_builtin_kw_fallback(func, pos_args, kwargs);
            }
        }
        self.call_builtin_kw_trailing_dict(func, pos_args, kwargs)
    }
}
