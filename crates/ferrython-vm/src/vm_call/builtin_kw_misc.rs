use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObject, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn builtin_open_kw(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<PyObjectRef> {
        let mut all_args = pos_args;
        if let Some((_, value)) = kwargs.iter().find(|(k, _)| k.as_str() == "mode") {
            while all_args.len() < 2 {
                all_args.push(PyObject::str_val(CompactString::from("r")));
            }
            all_args[1] = value.clone();
        }
        if let Some((_, value)) = kwargs.iter().find(|(k, _)| k.as_str() == "encoding") {
            while all_args.len() < 4 {
                all_args.push(PyObject::none());
            }
            all_args[3] = value.clone();
        }
        self.call_object(func, all_args)
    }

    pub(super) fn builtin_property_kw(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<PyObjectRef> {
        let mut all_args = pos_args;
        for (idx, key) in ["fget", "fset", "fdel", "doc"].iter().enumerate() {
            if let Some((_, value)) = kwargs.iter().find(|(k, _)| k.as_str() == *key) {
                while all_args.len() < idx {
                    all_args.push(PyObject::none());
                }
                if all_args.len() == idx {
                    all_args.push(value.clone());
                } else {
                    all_args[idx] = value.clone();
                }
            }
        }
        self.call_object(func, all_args)
    }
}
