use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_complex_numeric_builtin(
        &mut self,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        if args.len() == 1 {
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                let has_user_complex = inst.class.get_attr("__complex__").is_some() && {
                    let m = Self::resolve_instance_dunder(&args[0], "__complex__");
                    matches!(
                        m.as_ref().map(|o| &o.payload),
                        Some(PyObjectPayload::BoundMethod { .. } | PyObjectPayload::Function(_))
                    )
                };
                if has_user_complex {
                    if let Some(method) = Self::resolve_instance_dunder(&args[0], "__complex__") {
                        let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                            vec![]
                        } else {
                            vec![args[0].clone()]
                        };
                        let result = self.call_object(method, ca)?;
                        match &result.payload {
                            PyObjectPayload::Complex { .. } => return Ok(Some(result)),
                            PyObjectPayload::Instance(i2) => {
                                if let Some(v) = i2.attrs.read().get("__builtin_value__").cloned() {
                                    if matches!(&v.payload, PyObjectPayload::Complex { .. }) {
                                        self.warn_complex_subclass_returned_from_complex()?;
                                        return Ok(Some(v));
                                    }
                                }
                                return Err(PyException::type_error(format!(
                                    "__complex__ returned non-complex (type {})",
                                    result.type_name()
                                )));
                            }
                            _ => {
                                return Err(PyException::type_error(format!(
                                    "__complex__ returned non-complex (type {})",
                                    result.type_name()
                                )))
                            }
                        }
                    }
                }
                if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                    if matches!(&val.payload, PyObjectPayload::Complex { .. }) {
                        return Ok(Some(val));
                    }
                }
                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__float__") {
                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                        vec![]
                    } else {
                        vec![args[0].clone()]
                    };
                    let result = self.call_object(method, ca)?;
                    match &result.payload {
                        PyObjectPayload::Float(f) => return Ok(Some(PyObject::complex(*f, 0.0))),
                        PyObjectPayload::Int(n) => {
                            return Ok(Some(PyObject::complex(n.to_f64(), 0.0)))
                        }
                        PyObjectPayload::Bool(b) => {
                            return Ok(Some(PyObject::complex(if *b { 1.0 } else { 0.0 }, 0.0)))
                        }
                        _ => {
                            return Err(PyException::type_error(format!(
                                "__float__ returned non-float (type {})",
                                result.type_name()
                            )))
                        }
                    }
                }
                if let Some(method) = Self::resolve_instance_dunder(&args[0], "__index__") {
                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                        vec![]
                    } else {
                        vec![args[0].clone()]
                    };
                    let result = self.call_object(method, ca)?;
                    match &result.payload {
                        PyObjectPayload::Int(n) => {
                            let f = n.to_f64();
                            if f.is_infinite() {
                                return Err(PyException::overflow_error(
                                    "int too large to convert to float",
                                ));
                            }
                            return Ok(Some(PyObject::complex(f, 0.0)));
                        }
                        PyObjectPayload::Bool(b) => {
                            return Ok(Some(PyObject::complex(if *b { 1.0 } else { 0.0 }, 0.0)))
                        }
                        _ => {
                            return Err(PyException::type_error(format!(
                                "__index__ returned non-int (type {})",
                                result.type_name()
                            )))
                        }
                    }
                }
                return Err(PyException::type_error(format!(
                    "complex() first argument must be a string or a number, not '{}'",
                    args[0].type_name()
                )));
            }
        } else if args.len() == 2 {
            let has_inst = matches!(&args[0].payload, PyObjectPayload::Instance(_))
                || matches!(&args[1].payload, PyObjectPayload::Instance(_));
            if has_inst {
                let a = self.coerce_complex_part(&args[0], "first")?;
                let b = self.coerce_complex_part(&args[1], "second")?;
                return crate::builtins::core_fns::builtin_complex(&[a, b]).map(Some);
            }
        }
        Ok(None)
    }

    fn coerce_complex_part(&mut self, obj: &PyObjectRef, which: &str) -> PyResult<PyObjectRef> {
        if matches!(
            &obj.payload,
            PyObjectPayload::Complex { .. }
                | PyObjectPayload::Int(_)
                | PyObjectPayload::Float(_)
                | PyObjectPayload::Bool(_)
        ) {
            return Ok(obj.clone());
        }
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                if matches!(
                    &val.payload,
                    PyObjectPayload::Complex { .. }
                        | PyObjectPayload::Int(_)
                        | PyObjectPayload::Float(_)
                ) {
                    return Ok(val);
                }
            }
            for dunder in &["__complex__", "__float__", "__index__"] {
                if let Some(method) = Self::resolve_instance_dunder(obj, dunder) {
                    let ca = if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                        vec![]
                    } else {
                        vec![obj.clone()]
                    };
                    let res = self.call_object(method, ca)?;
                    if matches!(
                        &res.payload,
                        PyObjectPayload::Complex { .. }
                            | PyObjectPayload::Int(_)
                            | PyObjectPayload::Float(_)
                            | PyObjectPayload::Bool(_)
                    ) {
                        return Ok(res);
                    }
                }
            }
        }
        Err(PyException::type_error(format!(
            "complex() {} argument must be a number, not '{}'",
            which,
            obj.type_name()
        )))
    }

    fn warn_complex_subclass_returned_from_complex(&mut self) -> PyResult<()> {
        let warnings = self.import_module_simple("warnings", 0)?;
        let warn = warnings.get_attr("warn").ok_or_else(|| {
            PyException::attribute_error("module 'warnings' has no attribute 'warn'")
        })?;
        let category = warnings.get_attr("DeprecationWarning").ok_or_else(|| {
            PyException::attribute_error("module 'warnings' has no attribute 'DeprecationWarning'")
        })?;
        self.call_object(
            warn,
            vec![
                PyObject::str_val(CompactString::from(
                    "__complex__ returned non-complex (type complex subclass)",
                )),
                category,
            ],
        )?;
        Ok(())
    }
}
