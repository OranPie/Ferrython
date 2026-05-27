use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn builtin_int_kw(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<PyObjectRef> {
        let mut all_args = pos_args;
        if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "base") {
            while all_args.is_empty() {
                all_args.push(PyObject::int(0));
            }
            all_args.push(v.clone());
        }
        self.call_object(func, all_args)
    }

    pub(super) fn builtin_bool_kw(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<PyObjectRef> {
        if !kwargs.is_empty() {
            return Err(PyException::type_error(CompactString::from(
                "bool() takes no keyword arguments",
            )));
        }
        if pos_args.len() > 1 {
            return Err(PyException::type_error(CompactString::from(format!(
                "bool() takes at most 1 argument ({} given)",
                pos_args.len()
            ))));
        }
        if pos_args.is_empty() {
            return Ok(PyObject::bool_val(false));
        }
        let obj = &pos_args[0];
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            if let Some(target_fn) = inst.attrs.read().get("__weakref_target__").cloned() {
                let referent = self.call_object(target_fn, vec![])?;
                return Ok(PyObject::bool_val(self.vm_is_truthy(&referent)?));
            }
        }
        if let PyObjectPayload::Instance(_) = &obj.payload {
            if let Some(raw_method) = Self::resolve_instance_dunder(obj, "__bool__") {
                let method = self.resolve_descriptor(&raw_method, obj)?;
                let result = self.call_object(method, vec![])?;
                if !matches!(&result.payload, PyObjectPayload::Bool(_)) {
                    let tn = result.type_name();
                    return Err(PyException::type_error(CompactString::from(format!(
                        "__bool__ should return bool, returned {}",
                        tn
                    ))));
                }
                return Ok(result);
            }
            if let Some(raw_method) = Self::resolve_instance_dunder(obj, "__len__") {
                let method = self.resolve_descriptor(&raw_method, obj)?;
                let result = self.call_object(method, vec![])?;
                match &result.payload {
                    PyObjectPayload::Int(n) => {
                        let is_neg = match n.to_i64() {
                            Some(v) => v < 0,
                            None => false,
                        };
                        if is_neg {
                            return Err(PyException::value_error(CompactString::from(
                                "__len__() should return >= 0",
                            )));
                        }
                        return Ok(PyObject::bool_val(!n.is_zero()));
                    }
                    PyObjectPayload::Bool(b) => {
                        return Ok(PyObject::bool_val(*b));
                    }
                    _ => {
                        let tn = result.type_name();
                        return Err(PyException::type_error(CompactString::from(format!(
                            "__len__() should return >= 0, returned {}",
                            tn
                        ))));
                    }
                }
            }
        }
        self.call_object(func, pos_args)
    }

    pub(super) fn builtin_complex_kw(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<PyObjectRef> {
        let mut real_arg: Option<PyObjectRef> = None;
        let mut imag_arg: Option<PyObjectRef> = None;
        for (k, v) in kwargs {
            match k.as_str() {
                "real" => real_arg = Some(v.clone()),
                "imag" => imag_arg = Some(v.clone()),
                _ => {
                    return Err(PyException::type_error(format!(
                        "'{}' is an invalid keyword argument for complex()",
                        k
                    )))
                }
            }
        }
        let mut all_args = pos_args;
        if let Some(r) = real_arg {
            if all_args.is_empty() {
                all_args.push(r);
            } else {
                return Err(PyException::type_error(
                    "argument for complex() given by name ('real') and position (1)",
                ));
            }
        }
        if let Some(i) = imag_arg {
            while all_args.is_empty() {
                all_args.push(PyObject::int(0));
            }
            if all_args.len() == 1 {
                all_args.push(i);
            } else {
                return Err(PyException::type_error(
                    "argument for complex() given by name ('imag') and position (2)",
                ));
            }
        }
        self.call_object(func, all_args)
    }

    pub(super) fn builtin_open_kw(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<PyObjectRef> {
        let mut all_args = pos_args;
        if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "mode") {
            while all_args.len() < 2 {
                all_args.push(PyObject::str_val(CompactString::from("r")));
            }
            all_args[1] = v.clone();
        }
        if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "encoding") {
            while all_args.len() < 4 {
                all_args.push(PyObject::none());
            }
            all_args[3] = v.clone();
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
