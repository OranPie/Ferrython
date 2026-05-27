use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
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
                    return Err(PyException::type_error(CompactString::from(format!(
                        "__bool__ should return bool, returned {}",
                        result.type_name()
                    ))));
                }
                return Ok(result);
            }
            if let Some(raw_method) = Self::resolve_instance_dunder(obj, "__len__") {
                let method = self.resolve_descriptor(&raw_method, obj)?;
                let result = self.call_object(method, vec![])?;
                match &result.payload {
                    PyObjectPayload::Int(n) => {
                        if n.to_i64().map(|value| value < 0).unwrap_or(false) {
                            return Err(PyException::value_error(CompactString::from(
                                "__len__() should return >= 0",
                            )));
                        }
                        return Ok(PyObject::bool_val(!n.is_zero()));
                    }
                    PyObjectPayload::Bool(value) => {
                        return Ok(PyObject::bool_val(*value));
                    }
                    _ => {
                        return Err(PyException::type_error(CompactString::from(format!(
                            "__len__() should return >= 0, returned {}",
                            result.type_name()
                        ))));
                    }
                }
            }
        }
        self.call_object(func, pos_args)
    }
}
