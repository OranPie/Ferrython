use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::intern::intern_or_new;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn class_has_namespace_key(obj: &PyObjectRef, key: &str) -> bool {
        if let PyObjectPayload::Class(cd) = &obj.payload {
            if cd.namespace.read().contains_key(key) {
                return true;
            }
            for base in &cd.bases {
                if let PyObjectPayload::Class(bcd) = &base.payload {
                    if bcd.namespace.read().contains_key(key) {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub(super) fn init_dataclass_instance(
        &mut self,
        cls: &PyObjectRef,
        instance: &PyObjectRef,
        pos_args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<()> {
        let is_frozen = Self::class_has_namespace_key(cls, "__dataclass_frozen__");
        if let Some(fields) = cls.get_attr("__dataclass_fields__") {
            let field_entries: Vec<(String, bool, PyObjectRef, bool)> = match &fields.payload {
                PyObjectPayload::Tuple(field_tuples) => field_tuples
                    .iter()
                    .filter_map(|ft| {
                        if let PyObjectPayload::Tuple(info) = &ft.payload {
                            let name = info[0].py_to_string();
                            let has_default = info[1].is_truthy();
                            let default_val = info[2].clone();
                            let field_init = if info.len() > 3 {
                                info[3].is_truthy()
                            } else {
                                true
                            };
                            Some((name, has_default, default_val, field_init))
                        } else {
                            None
                        }
                    })
                    .collect(),
                PyObjectPayload::Dict(map) => {
                    let r = map.read();
                    r.iter()
                        .map(|(k, field_obj)| {
                            let name = match k {
                                HashableKey::Str(s) => s.to_string(),
                                _ => field_obj
                                    .get_attr("name")
                                    .map(|n| n.py_to_string())
                                    .unwrap_or_default(),
                            };
                            let field_init = field_obj
                                .get_attr("init")
                                .map(|v| v.is_truthy())
                                .unwrap_or(true);
                            let has_default_flag = field_obj
                                .get_attr("__has_default__")
                                .map(|v| v.is_truthy())
                                .unwrap_or(false);
                            let default_factory = field_obj.get_attr("default_factory");
                            let has_factory = default_factory
                                .as_ref()
                                .map(|f| f.is_callable())
                                .unwrap_or(false);
                            let (has_default, default_val) = if has_factory {
                                (true, default_factory.unwrap_or_else(PyObject::none))
                            } else if has_default_flag {
                                let default =
                                    field_obj.get_attr("default").unwrap_or_else(PyObject::none);
                                (true, default)
                            } else {
                                (false, PyObject::none())
                            };
                            (name, has_default, default_val, field_init)
                        })
                        .collect()
                }
                _ => Vec::new(),
            };

            let mut arg_idx = 0;
            for (name, has_default, default_val, field_init) in &field_entries {
                let value = if !field_init {
                    if *has_default {
                        if default_val.is_callable() {
                            self.call_object(default_val.clone(), vec![])?
                        } else {
                            default_val.clone()
                        }
                    } else {
                        continue;
                    }
                } else if let Some((_, v)) =
                    kwargs.iter().find(|(k, _)| k.as_str() == name.as_str())
                {
                    v.clone()
                } else if arg_idx < pos_args.len() {
                    let v = pos_args[arg_idx].clone();
                    arg_idx += 1;
                    v
                } else if *has_default {
                    if default_val.is_callable() {
                        self.call_object(default_val.clone(), vec![])?
                    } else {
                        default_val.clone()
                    }
                } else {
                    return Err(PyException::type_error(format!(
                        "__init__() missing required argument: '{}'",
                        name
                    )));
                };

                if let PyObjectPayload::Instance(inst) = &instance.payload {
                    inst.attrs
                        .write()
                        .insert(CompactString::from(name.as_str()), value);
                }
            }
        }

        if let Some(post_init) = cls.get_attr("__post_init__") {
            let pi_fn = match &post_init.payload {
                PyObjectPayload::BoundMethod { method, .. } => method.clone(),
                _ => post_init.clone(),
            };
            self.call_object(pi_fn, vec![instance.clone()])?;
        }

        if is_frozen {
            if let PyObjectPayload::Class(cd) = &cls.payload {
                let ns = cd.namespace.read();
                if !ns.contains_key("__setattr__") {
                    drop(ns);
                    let mut ns = cd.namespace.write();
                    ns.insert(
                        intern_or_new("__setattr__"),
                        PyObject::native_function("__setattr__", |_args| {
                            Err(PyException::attribute_error(String::from(
                                "cannot assign to field of frozen dataclass",
                            )))
                        }),
                    );
                    ns.insert(
                        intern_or_new("__delattr__"),
                        PyObject::native_function("__delattr__", |_args| {
                            Err(PyException::attribute_error(String::from(
                                "cannot delete field of frozen dataclass",
                            )))
                        }),
                    );
                }
            }
        }

        Ok(())
    }
}
