use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn init_namedtuple_instance(
        &self,
        cls: &PyObjectRef,
        instance: &PyObjectRef,
        pos_args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<()> {
        if let Some(fields) = cls.get_attr("_fields") {
            if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                if let PyObjectPayload::Instance(inst) = &instance.payload {
                    let defaults_map = cls.get_attr("_field_defaults").and_then(|d| {
                        if let PyObjectPayload::Dict(map) = &d.payload {
                            Some(map.read().clone())
                        } else {
                            None
                        }
                    });
                    if pos_args.len() > field_names.len() {
                        return Err(PyException::type_error(format!(
                            "__new__() takes {} positional arguments but {} were given",
                            field_names.len() + 1,
                            pos_args.len() + 1
                        )));
                    }
                    let field_name_strs: Vec<String> =
                        field_names.iter().map(|f| f.py_to_string()).collect();
                    for (k, _) in kwargs {
                        if !field_name_strs.iter().any(|n| n.as_str() == k.as_str()) {
                            return Err(PyException::type_error(format!(
                                "got an unexpected keyword argument '{}'",
                                k
                            )));
                        }
                    }
                    let mut attrs = inst.attrs.write();
                    let mut tuple_values = Vec::with_capacity(field_names.len());
                    let mut missing: Vec<String> = Vec::new();
                    for (i, field) in field_names.iter().enumerate() {
                        let name = field.py_to_string();
                        let value = if let Some((_, v)) =
                            kwargs.iter().find(|(k, _)| k.as_str() == name.as_str())
                        {
                            if i < pos_args.len() {
                                return Err(PyException::type_error(format!(
                                    "got multiple values for argument '{}'",
                                    name
                                )));
                            }
                            v.clone()
                        } else if i < pos_args.len() {
                            pos_args[i].clone()
                        } else if let Some(ref dmap) = defaults_map {
                            let key = HashableKey::str_key(CompactString::from(name.as_str()));
                            if let Some(v) = dmap.get(&key) {
                                v.clone()
                            } else {
                                missing.push(name.clone());
                                PyObject::none()
                            }
                        } else {
                            missing.push(name.clone());
                            PyObject::none()
                        };
                        tuple_values.push(value);
                    }
                    if !missing.is_empty() {
                        drop(attrs);
                        return Err(PyException::type_error(format!(
                            "__new__() missing {} required argument{}: {}",
                            missing.len(),
                            if missing.len() == 1 { "" } else { "s" },
                            missing
                                .iter()
                                .map(|n| format!("'{}'", n))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )));
                    }
                    attrs.insert(CompactString::from("_tuple"), PyObject::tuple(tuple_values));
                }
            }
        }
        Ok(())
    }
}
