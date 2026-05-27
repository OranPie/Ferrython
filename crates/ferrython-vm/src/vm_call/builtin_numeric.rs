use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_numeric_builtin(
        &mut self,
        func: &PyObjectRef,
        name: &CompactString,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        if let Some(result) = self.call_numeric_protocol_builtin(func, name.as_str(), &args)? {
            return Ok(result);
        }
        match name.as_str() {
            "complex" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        // Check for user-defined __complex__ FIRST (takes priority over __builtin_value__)
                        let has_user_complex = inst.class.get_attr("__complex__").is_some() && {
                            // Distinguish user-defined from inherited builtin
                            let m = Self::resolve_instance_dunder(&args[0], "__complex__");
                            matches!(
                                m.as_ref().map(|o| &o.payload),
                                Some(
                                    PyObjectPayload::BoundMethod { .. }
                                        | PyObjectPayload::Function(_)
                                )
                            )
                        };
                        if has_user_complex {
                            if let Some(method) =
                                Self::resolve_instance_dunder(&args[0], "__complex__")
                            {
                                let ca = if matches!(
                                    &method.payload,
                                    PyObjectPayload::BoundMethod { .. }
                                ) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                                let result = self.call_object(method, ca)?;
                                match &result.payload {
                                    PyObjectPayload::Complex { .. } => return Ok(result),
                                    PyObjectPayload::Instance(i2) => {
                                        // subclass of complex — extract via __builtin_value__
                                        if let Some(v) =
                                            i2.attrs.read().get("__builtin_value__").cloned()
                                        {
                                            if matches!(&v.payload, PyObjectPayload::Complex { .. })
                                            {
                                                return Ok(v);
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
                                return Ok(val);
                            }
                        }
                        // Fallback: __float__
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__float__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            let result = self.call_object(method, ca)?;
                            match &result.payload {
                                PyObjectPayload::Float(f) => return Ok(PyObject::complex(*f, 0.0)),
                                PyObjectPayload::Int(n) => {
                                    return Ok(PyObject::complex(n.to_f64(), 0.0))
                                }
                                PyObjectPayload::Bool(b) => {
                                    return Ok(PyObject::complex(if *b { 1.0 } else { 0.0 }, 0.0))
                                }
                                _ => {
                                    return Err(PyException::type_error(format!(
                                        "__float__ returned non-float (type {})",
                                        result.type_name()
                                    )))
                                }
                            }
                        }
                        // Fallback: __index__
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__index__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
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
                                    return Ok(PyObject::complex(f, 0.0));
                                }
                                PyObjectPayload::Bool(b) => {
                                    return Ok(PyObject::complex(if *b { 1.0 } else { 0.0 }, 0.0))
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
                    // Handle instances as either arg via __float__/__index__/__complex__
                    let coerce_for_complex = |vm: &mut Self,
                                              obj: &PyObjectRef,
                                              which: &str|
                     -> PyResult<PyObjectRef> {
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
                                    let ca = if matches!(
                                        &method.payload,
                                        PyObjectPayload::BoundMethod { .. }
                                    ) {
                                        vec![]
                                    } else {
                                        vec![obj.clone()]
                                    };
                                    let res = vm.call_object(method, ca)?;
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
                    };
                    let has_inst = matches!(&args[0].payload, PyObjectPayload::Instance(_))
                        || matches!(&args[1].payload, PyObjectPayload::Instance(_));
                    if has_inst {
                        let which_first = if matches!(&args[0].payload, PyObjectPayload::Str(_)) {
                            ""
                        } else {
                            "first"
                        };
                        let which_second = "second";
                        let a = coerce_for_complex(
                            self,
                            &args[0],
                            if which_first.is_empty() {
                                "first"
                            } else {
                                which_first
                            },
                        )?;
                        let b = coerce_for_complex(self, &args[1], which_second)?;
                        return crate::builtins::core_fns::builtin_complex(&[a, b]);
                    }
                }
            }
            "int" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        // Check for __builtin_value__ first (int subclass)
                        if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                            if matches!(&val.payload, PyObjectPayload::Int(_)) {
                                return Ok(val);
                            }
                        }
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__int__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            return self.call_object(method, ca);
                        }
                    }
                }
            }
            "float" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        // Check for __builtin_value__ first (float subclass)
                        if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                            if matches!(&val.payload, PyObjectPayload::Float(_)) {
                                return Ok(val);
                            }
                            // int subclass → convert to float
                            if let PyObjectPayload::Int(n) = &val.payload {
                                return Ok(PyObject::float(n.to_f64()));
                            }
                        }
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__float__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            return self.call_object(method, ca);
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
                            return self.call_object(method, ca);
                        }
                    }
                }
            }
            "bool" => {
                if args.len() == 1 {
                    let obj = &args[0];
                    if let ferrython_core::object::PyObjectPayload::Instance(inst) = &obj.payload {
                        if let Some(target_fn) =
                            inst.attrs.read().get("__weakref_target__").cloned()
                        {
                            let referent = self.call_object(target_fn, vec![])?;
                            return Ok(PyObject::bool_val(self.vm_is_truthy(&referent)?));
                        }
                    }
                    // Instance with __bool__: call it and enforce return type == bool
                    if let ferrython_core::object::PyObjectPayload::Instance(_) = &obj.payload {
                        if let Some(raw_method) = Self::resolve_instance_dunder(obj, "__bool__") {
                            let method = self.resolve_descriptor(&raw_method, obj)?;
                            let result = self.call_object(method, vec![])?;
                            if !matches!(
                                &result.payload,
                                ferrython_core::object::PyObjectPayload::Bool(_)
                            ) {
                                let tn = result.type_name();
                                return Err(ferrython_core::error::PyException::type_error(
                                    compact_str::CompactString::from(format!(
                                        "__bool__ should return bool, returned {}",
                                        tn
                                    )),
                                ));
                            }
                            return Ok(result);
                        }
                        if let Some(raw_method) = Self::resolve_instance_dunder(obj, "__len__") {
                            let method = self.resolve_descriptor(&raw_method, obj)?;
                            let result = self.call_object(method, vec![])?;
                            match &result.payload {
                                ferrython_core::object::PyObjectPayload::Int(n) => {
                                    let is_neg = n.to_i64().map(|v| v < 0).unwrap_or(false);
                                    if is_neg {
                                        return Err(
                                            ferrython_core::error::PyException::value_error(
                                                compact_str::CompactString::from(
                                                    "__len__() should return >= 0",
                                                ),
                                            ),
                                        );
                                    }
                                    return Ok(PyObject::bool_val(!n.is_zero()));
                                }
                                ferrython_core::object::PyObjectPayload::Bool(b) => {
                                    return Ok(PyObject::bool_val(*b));
                                }
                                _ => {
                                    let tn = result.type_name();
                                    return Err(ferrython_core::error::PyException::type_error(
                                        compact_str::CompactString::from(format!(
                                            "__len__() should return >= 0, returned {}",
                                            tn
                                        )),
                                    ));
                                }
                            }
                        }
                    }
                    return Ok(PyObject::bool_val(self.vm_is_truthy(obj)?));
                }
            }
            _ => {}
        }
        crate::builtins::dispatch(name.as_str(), &args)
    }
}
