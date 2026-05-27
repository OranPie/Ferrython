use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn builtin_int_kw(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<PyObjectRef> {
        let mut all_args = pos_args;
        if let Some((_, value)) = kwargs.iter().find(|(k, _)| k.as_str() == "base") {
            while all_args.is_empty() {
                all_args.push(PyObject::int(0));
            }
            all_args.push(value.clone());
        }
        self.call_object(func, all_args)
    }

    pub(super) fn builtin_complex_kw(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<PyObjectRef> {
        let mut real_arg: Option<PyObjectRef> = None;
        let mut imag_arg: Option<PyObjectRef> = None;
        for (key, value) in kwargs {
            match key.as_str() {
                "real" => real_arg = Some(value.clone()),
                "imag" => imag_arg = Some(value.clone()),
                _ => {
                    return Err(PyException::type_error(format!(
                        "'{}' is an invalid keyword argument for complex()",
                        key
                    )))
                }
            }
        }
        let mut all_args = pos_args;
        if let Some(real) = real_arg {
            if all_args.is_empty() {
                all_args.push(real);
            } else {
                return Err(PyException::type_error(
                    "argument for complex() given by name ('real') and position (1)",
                ));
            }
        }
        if let Some(imag) = imag_arg {
            while all_args.is_empty() {
                all_args.push(PyObject::int(0));
            }
            if all_args.len() == 1 {
                all_args.push(imag);
            } else {
                return Err(PyException::type_error(
                    "argument for complex() given by name ('imag') and position (2)",
                ));
            }
        }
        self.call_object(func, all_args)
    }
}
