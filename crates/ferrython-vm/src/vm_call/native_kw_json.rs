use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{
    NativeFunctionData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_json_native_kw(
        &mut self,
        nf_data: &NativeFunctionData,
        pos_args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<Option<PyObjectRef>> {
        if nf_data.name.as_str() == "json.loads" && !kwargs.is_empty() {
            let has_py_hook = kwargs.iter().any(|(k, v)| {
                matches!(
                    k.as_str(),
                    "object_hook" | "parse_float" | "parse_int" | "object_pairs_hook"
                ) && matches!(
                    &v.payload,
                    PyObjectPayload::Function(_) | PyObjectPayload::Class(_)
                )
            });
            if has_py_hook {
                let filtered_kwargs: Vec<(CompactString, PyObjectRef)> = kwargs
                    .iter()
                    .filter(|(k, _)| {
                        !matches!(
                            k.as_str(),
                            "object_hook" | "parse_float" | "parse_int" | "object_pairs_hook"
                        )
                    })
                    .cloned()
                    .collect();
                let mut load_args = pos_args.to_vec();
                if !filtered_kwargs.is_empty() {
                    load_args.push(kwargs_to_dict(filtered_kwargs));
                }
                let parsed = (nf_data.func)(&load_args)?;
                let object_hook = kwargs
                    .iter()
                    .find(|(k, _)| k.as_str() == "object_hook")
                    .map(|(_, v)| v.clone());
                let parse_float = kwargs
                    .iter()
                    .find(|(k, _)| k.as_str() == "parse_float")
                    .map(|(_, v)| v.clone());
                let parse_int = kwargs
                    .iter()
                    .find(|(k, _)| k.as_str() == "parse_int")
                    .map(|(_, v)| v.clone());
                return self
                    .json_apply_hooks(&parsed, &object_hook, &parse_float, &parse_int)
                    .map(Some);
            }
        }

        if (nf_data.name.as_str() == "json.dumps" || nf_data.name.as_str() == "json.dump")
            && !kwargs.is_empty()
        {
            let default_fn = kwargs
                .iter()
                .find(|(k, _)| k.as_str() == "default")
                .map(|(_, v)| v.clone());
            let cls_default = if default_fn.is_none() {
                kwargs
                    .iter()
                    .find(|(k, _)| k.as_str() == "cls")
                    .and_then(|(_, cls_val)| {
                        let encoder_inst = PyObject::instance(cls_val.clone());
                        cls_val.get_attr("default").map(|method| {
                            PyObject::wrap(PyObjectPayload::BoundMethod {
                                receiver: encoder_inst,
                                method,
                            })
                        })
                    })
            } else {
                None
            };
            let effective_default = default_fn.or(cls_default);
            if let Some(ref def) = effective_default {
                let needs_vm_prepare = match &def.payload {
                    PyObjectPayload::Function(_) => true,
                    PyObjectPayload::BoundMethod { method, .. } => {
                        matches!(&method.payload, PyObjectPayload::Function(_))
                    }
                    PyObjectPayload::NativeFunction(_)
                    | PyObjectPayload::NativeClosure(_)
                    | PyObjectPayload::Class(_)
                    | PyObjectPayload::BuiltinFunction(_)
                    | PyObjectPayload::BuiltinType(_) => true,
                    _ => false,
                };
                if needs_vm_prepare {
                    let prepared = self.json_prepare_with_default(&pos_args[0], def)?;
                    let filtered_kwargs: Vec<(CompactString, PyObjectRef)> = kwargs
                        .iter()
                        .filter(|(k, _)| k.as_str() != "default" && k.as_str() != "cls")
                        .cloned()
                        .collect();
                    if nf_data.name.as_str() == "json.dump" {
                        let mut dump_args = vec![prepared];
                        if pos_args.len() > 1 {
                            dump_args.push(pos_args[1].clone());
                        }
                        if !filtered_kwargs.is_empty() {
                            dump_args.push(kwargs_to_dict(filtered_kwargs));
                        }
                        return (nf_data.func)(&dump_args).map(Some);
                    }

                    let mut dump_args = vec![prepared];
                    if !filtered_kwargs.is_empty() {
                        dump_args.push(kwargs_to_dict(filtered_kwargs));
                    }
                    return (nf_data.func)(&dump_args).map(Some);
                }
            }
        }

        Ok(None)
    }
}

fn kwargs_to_dict(kwargs: Vec<(CompactString, PyObjectRef)>) -> PyObjectRef {
    let mut kw_map = IndexMap::new();
    for (k, v) in kwargs {
        kw_map.insert(HashableKey::str_key(k), v);
    }
    PyObject::dict(kw_map)
}
