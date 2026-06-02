use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};

use crate::builtins;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_numeric_protocol_builtin(
        &mut self,
        func: &PyObjectRef,
        name: &str,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        match name {
            "len" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        if inst.attrs.read().contains_key("__chainmap__") {
                            if let Some(method) = args[0].get_attr("__len__") {
                                return self.call_object(method, vec![]).map(Some);
                            }
                        }
                        if let Some(ref ds) = inst.dict_storage {
                            return Ok(Some(PyObject::int(ds.read().len() as i64)));
                        }
                        if inst.class.get_attr("__namedtuple__").is_some() {
                            return builtins::call_method(&args[0], "__len__", &[]).map(Some);
                        }
                        if let Some(method) = args[0].get_attr("__len__") {
                            if !matches!(&method.payload, PyObjectPayload::BuiltinBoundMethod(_)) {
                                let ca = if matches!(
                                    &method.payload,
                                    PyObjectPayload::BoundMethod { .. }
                                ) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                                return self.call_object(method, ca).map(Some);
                            }
                        }
                        if let Some(bv) = inst.attrs.read().get("__builtin_value__").cloned() {
                            if let Ok(n) = bv.py_len() {
                                return Ok(Some(PyObject::int(n as i64)));
                            }
                        }
                    }
                }
            }
            "abs" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__abs__") {
                            let call_args =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            return self.call_object(method, call_args).map(Some);
                        }
                    }
                }
            }
            "hash" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        let is_weak_ref_like = {
                            let attrs = inst.attrs.read();
                            attrs.contains_key("__weakref_ref__")
                                || attrs.contains_key("__weakmethod__")
                        };
                        if !is_weak_ref_like && Self::class_blocks_hash(&inst.class) {
                            return Err(PyException::type_error(format!(
                                "unhashable type: '{}'",
                                args[0].type_name()
                            )));
                        }
                        if let Some(result) =
                            self.call_plain_instance_dunder(&args[0], inst, "__hash__", Vec::new())?
                        {
                            return Ok(Some(result));
                        }
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__hash__") {
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
            "divmod" => {
                if args.len() == 2 {
                    if let Some(result) = self.try_binary_dunder(
                        &args[0],
                        &args[1],
                        "__divmod__",
                        Some("__rdivmod__"),
                    )? {
                        return Ok(Some(result));
                    }
                }
            }
            "pow" => {
                if args.len() == 2 {
                    if let Some(result) =
                        self.try_binary_dunder(&args[0], &args[1], "__pow__", Some("__rpow__"))?
                    {
                        return Ok(Some(result));
                    }
                } else if args.len() == 3 {
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        if let Some(result) = self.call_plain_instance_dunder(
                            &args[0],
                            inst,
                            "__pow__",
                            vec![args[1].clone(), args[2].clone()],
                        )? {
                            if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                                return Ok(Some(result));
                            }
                        }
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__pow__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![args[1].clone(), args[2].clone()]
                                } else {
                                    vec![args[0].clone(), args[1].clone(), args[2].clone()]
                                };
                            let result = self.call_object(method, ca)?;
                            if !matches!(&result.payload, PyObjectPayload::NotImplemented) {
                                return Ok(Some(result));
                            }
                        }
                    }
                }
            }
            "bin" | "oct" | "hex" => {
                if args.len() == 1 {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__index__") {
                            let ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            let idx_val = self.call_object(method, ca)?;
                            if !matches!(&idx_val.payload, PyObjectPayload::Int(_))
                                && !matches!(&idx_val.payload, PyObjectPayload::Bool(_))
                            {
                                if let Some(bv) = Self::get_builtin_value(&idx_val) {
                                    if matches!(&bv.payload, PyObjectPayload::Int(_))
                                        || matches!(&bv.payload, PyObjectPayload::Bool(_))
                                    {
                                        return self.call_object(func.clone(), vec![bv]).map(Some);
                                    }
                                }
                                return Err(PyException::type_error(format!(
                                    "__index__ returned non-int (type {})",
                                    idx_val.type_name()
                                )));
                            }
                            return self.call_object(func.clone(), vec![idx_val]).map(Some);
                        }
                    }
                }
            }
            "format" => {
                if !args.is_empty() {
                    if let PyObjectPayload::Instance(_) = &args[0].payload {
                        if let Some(method) = Self::resolve_instance_dunder(&args[0], "__format__")
                        {
                            let spec = if args.len() > 1 {
                                args[1].clone()
                            } else {
                                PyObject::str_val(CompactString::from(""))
                            };
                            let mut ca =
                                if matches!(&method.payload, PyObjectPayload::BoundMethod { .. }) {
                                    vec![]
                                } else {
                                    vec![args[0].clone()]
                                };
                            ca.push(spec);
                            return self.call_object(method, ca).map(Some);
                        }
                        let has_spec = args.len() > 1 && !args[1].py_to_string().is_empty();
                        if !has_spec {
                            let s = self.vm_str(&args[0])?;
                            return Ok(Some(PyObject::str_val(CompactString::from(s))));
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(None)
    }
}
