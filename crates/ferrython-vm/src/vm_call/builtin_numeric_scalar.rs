use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_scalar_numeric_builtin(
        &mut self,
        name: &str,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        match name {
            "int" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                            if matches!(&val.payload, PyObjectPayload::Int(_)) {
                                return Ok(Some(val));
                            }
                        }
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__int__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            return self.call_object(method, ca).map(Some);
                        }
                    }
                }
            }
            "float" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                            if matches!(&val.payload, PyObjectPayload::Float(_)) {
                                return Ok(Some(val));
                            }
                            if let PyObjectPayload::Int(n) = &val.payload {
                                return Ok(Some(PyObject::float(n.to_f64())));
                            }
                        }
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__float__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            return self.call_object(method, ca).map(Some);
                        }
                    }
                }
            }
            "round" => {
                if !args.is_empty() {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__round__") {
                            let mut ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            if args.len() >= 2 {
                                ca.push(args[1].clone());
                            }
                            return self.call_object(method, ca).map(Some);
                        }
                    }
                }
            }
            "bool" => {
                if args.len() == 1 {
                    return self.call_bool_numeric_builtin(&args[0]).map(Some);
                }
            }
            _ => {}
        }
        Ok(None)
    }

    fn call_bool_numeric_builtin(&mut self, obj: &PyObjectRef) -> PyResult<PyObjectRef> {
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
                        let is_neg = n.to_i64().map(|v| v < 0).unwrap_or(false);
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
        Ok(PyObject::bool_val(self.vm_is_truthy(obj)?))
    }
}
