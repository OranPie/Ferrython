use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::{float_as_integer_ratio, PyInt};

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
                    if let PyObjectPayload::Float(f) = &args[0].payload {
                        if f.is_nan() {
                            return Err(PyException::value_error(
                                "cannot convert float NaN to integer",
                            ));
                        }
                        if f.is_infinite() {
                            return Err(PyException::overflow_error(
                                "cannot convert float infinity to integer",
                            ));
                        }
                        let truncated = f.trunc();
                        if truncated >= -9_007_199_254_740_992.0
                            && truncated <= 9_007_199_254_740_992.0
                        {
                            return Ok(Some(PyObject::int(truncated as i64)));
                        }
                        let (n, d) = float_as_integer_ratio(truncated);
                        return Ok(Some(PyObject::big_int(n / d)));
                    }
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__int__") {
                            let result = self.call_bound_or_unbound(method, &args[0])?;
                            return self.coerce_int_protocol_result(result, "__int__").map(Some);
                        }
                        if let Some(val) = inst.attrs.read().get("__builtin_value__").cloned() {
                            if matches!(&val.payload, PyObjectPayload::Int(_)) {
                                return Ok(Some(val));
                            }
                        }
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__index__") {
                            let result = self.call_bound_or_unbound(method, &args[0])?;
                            return self
                                .coerce_int_protocol_result(result, "__index__")
                                .map(Some);
                        }
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__trunc__") {
                            let result = self.call_bound_or_unbound(method, &args[0])?;
                            return self.coerce_trunc_result(result).map(Some);
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
                                let value = n.to_f64();
                                if value.is_finite() {
                                    return Ok(Some(PyObject::float(value)));
                                }
                                return Err(PyException::overflow_error(
                                    "int too large to convert to float",
                                ));
                            }
                        }
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__float__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            let result = self.call_object(method, ca)?;
                            if matches!(&result.payload, PyObjectPayload::Float(_)) {
                                return Ok(Some(result));
                            }
                            if let Some(bv) = Self::get_builtin_value(&result) {
                                if matches!(&bv.payload, PyObjectPayload::Float(_)) {
                                    return Ok(Some(bv));
                                }
                            }
                            return Err(PyException::type_error(format!(
                                "__float__ returned non-float (type {})",
                                result.type_name()
                            )));
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

    fn call_bound_or_unbound(
        &mut self,
        method: PyObjectRef,
        receiver: &PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        let args = if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
            vec![]
        } else {
            vec![receiver.clone()]
        };
        self.call_object(method, args)
    }

    fn coerce_int_protocol_result(
        &mut self,
        result: PyObjectRef,
        method_name: &str,
    ) -> PyResult<PyObjectRef> {
        match &result.payload {
            PyObjectPayload::Int(PyInt::Small(n)) => return Ok(PyObject::int(*n)),
            PyObjectPayload::Int(PyInt::Big(n)) => {
                return Ok(PyObject::big_int(n.as_ref().clone()))
            }
            PyObjectPayload::Bool(b) => {
                self.warn_int_protocol_subclass_result(method_name, "bool")?;
                return Ok(PyObject::int(if *b { 1 } else { 0 }));
            }
            PyObjectPayload::Instance(inst) => {
                if let Some(value) = inst.attrs.read().get("__builtin_value__").cloned() {
                    match &value.payload {
                        PyObjectPayload::Int(PyInt::Small(n)) => {
                            self.warn_int_protocol_subclass_result(method_name, "int")?;
                            return Ok(PyObject::int(*n));
                        }
                        PyObjectPayload::Int(PyInt::Big(n)) => {
                            self.warn_int_protocol_subclass_result(method_name, "int")?;
                            return Ok(PyObject::big_int(n.as_ref().clone()));
                        }
                        PyObjectPayload::Bool(b) => {
                            self.warn_int_protocol_subclass_result(method_name, "bool")?;
                            return Ok(PyObject::int(if *b { 1 } else { 0 }));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        Err(PyException::type_error(format!(
            "{} returned non-int (type {})",
            method_name,
            result.type_name()
        )))
    }

    fn warn_int_protocol_subclass_result(
        &mut self,
        method_name: &str,
        type_name: &str,
    ) -> PyResult<()> {
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
                PyObject::str_val(CompactString::from(format!(
                    "{} returned non-int (type {})",
                    method_name, type_name
                ))),
                category,
            ],
        )?;
        Ok(())
    }

    fn coerce_trunc_result(&mut self, result: PyObjectRef) -> PyResult<PyObjectRef> {
        if let Some(value) = self.coerce_trunc_direct_result(&result) {
            return Ok(value);
        }
        if let PyObjectPayload::Instance(_) = &result.payload {
            for method_name in ["__index__", "__int__"] {
                if let Some(method) = Self::resolve_instance_dunder(&result, method_name) {
                    let value = self.call_bound_or_unbound(method, &result)?;
                    return self.coerce_int_protocol_result(value, method_name);
                }
            }
        }
        Err(PyException::type_error(format!(
            "__trunc__ returned non-Integral (type {})",
            result.type_name()
        )))
    }

    fn coerce_trunc_direct_result(&mut self, result: &PyObjectRef) -> Option<PyObjectRef> {
        match &result.payload {
            PyObjectPayload::Int(PyInt::Small(n)) => Some(PyObject::int(*n)),
            PyObjectPayload::Int(PyInt::Big(n)) => Some(PyObject::big_int(n.as_ref().clone())),
            PyObjectPayload::Bool(b) => Some(PyObject::int(if *b { 1 } else { 0 })),
            PyObjectPayload::Instance(inst) => {
                let value = inst.attrs.read().get("__builtin_value__").cloned()?;
                self.coerce_trunc_direct_result(&value)
            }
            _ => None,
        }
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
