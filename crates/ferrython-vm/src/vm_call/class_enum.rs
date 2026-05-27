use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{CompareOp, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn try_instantiate_enum(
        &mut self,
        cls: &PyObjectRef,
        pos_args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<Option<PyObjectRef>> {
        let PyObjectPayload::Class(cd) = &cls.payload else {
            return Ok(None);
        };

        let is_enum_base = cd.name.as_str() == "Enum"
            || cd.name.as_str() == "Flag"
            || cd.name.as_str() == "IntEnum"
            || cd.name.as_str() == "IntFlag"
            || cd.name.as_str() == "StrEnum";

        if is_enum_base && pos_args.len() >= 2 {
            if let PyObjectPayload::Str(ref name_str) = pos_args[0].payload {
                let members: Vec<(String, PyObjectRef)> = match &pos_args[1].payload {
                    PyObjectPayload::Str(s) => s
                        .replace(',', " ")
                        .split_whitespace()
                        .enumerate()
                        .map(|(i, n)| (n.to_string(), PyObject::int((i + 1) as i64)))
                        .collect(),
                    PyObjectPayload::List(items) => items
                        .read()
                        .iter()
                        .enumerate()
                        .map(|(i, item)| (item.py_to_string(), PyObject::int((i + 1) as i64)))
                        .collect(),
                    PyObjectPayload::Tuple(items) => items
                        .iter()
                        .enumerate()
                        .map(|(i, item)| (item.py_to_string(), PyObject::int((i + 1) as i64)))
                        .collect(),
                    PyObjectPayload::Dict(map) => map
                        .read()
                        .iter()
                        .map(|(k, v)| {
                            let name = match k {
                                HashableKey::Str(s) => s.to_string(),
                                _ => format!("{:?}", k),
                            };
                            (name, v.clone())
                        })
                        .collect(),
                    _ => vec![],
                };
                if !members.is_empty() {
                    let mut ns = IndexMap::new();
                    ns.insert(CompactString::from("__enum__"), PyObject::bool_val(true));
                    let new_cls =
                        PyObject::class(name_str.to_compact_string(), vec![cls.clone()], ns);
                    if let PyObjectPayload::Class(ref new_cd) = new_cls.payload {
                        let mut new_ns = new_cd.namespace.write();
                        for (member_name, member_value) in &members {
                            let member = PyObject::instance_with_attrs(new_cls.clone(), {
                                let mut m = IndexMap::new();
                                m.insert(
                                    CompactString::from("name"),
                                    PyObject::str_val(CompactString::from(member_name.as_str())),
                                );
                                m.insert(CompactString::from("value"), member_value.clone());
                                m.insert(
                                    CompactString::from("_name_"),
                                    PyObject::str_val(CompactString::from(member_name.as_str())),
                                );
                                m.insert(CompactString::from("_value_"), member_value.clone());
                                m
                            });
                            new_ns.insert(CompactString::from(member_name.as_str()), member);
                        }
                    }
                    return Ok(Some(new_cls));
                }
            }
        }

        if cd.namespace.read().contains_key("__enum__") && pos_args.len() == 1 && kwargs.is_empty()
        {
            let target_val = &pos_args[0];
            let ns = cd.namespace.read();
            for (_, member) in ns.iter() {
                if let PyObjectPayload::Instance(inst) = &member.payload {
                    if let Some(val) = inst.attrs.read().get("value") {
                        if val
                            .compare(target_val, CompareOp::Eq)
                            .map(|r| r.is_truthy())
                            .unwrap_or(false)
                        {
                            return Ok(Some(member.clone()));
                        }
                    }
                }
            }
            return Err(PyException::value_error(format!(
                "{} is not a valid {}",
                target_val.repr(),
                cd.name
            )));
        }

        Ok(None)
    }
}
