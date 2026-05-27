use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_native_or_fallback_kw(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        // Handle other payload types that support kwargs
        match &func.payload {
            PyObjectPayload::NativeFunction(nf_data) => {
                if nf_data.name.as_str() == "_ast.AST.__init__" {
                    if pos_args.is_empty() {
                        return Err(PyException::type_error("__init__ requires self"));
                    }
                    let instance = &pos_args[0];
                    let cls = match &instance.payload {
                        PyObjectPayload::Instance(inst) => inst.class.clone(),
                        _ => {
                            return Err(PyException::type_error(
                                "AST.__init__ requires an AST instance",
                            ))
                        }
                    };
                    Self::populate_ast_node_attrs(instance, &cls, &pos_args[1..], &kwargs)?;
                    return Ok(PyObject::none());
                }
                if nf_data.name.as_str() == "_ast.AST.__new__" {
                    if pos_args.is_empty() {
                        return Err(PyException::type_error("__new__ requires cls"));
                    }
                    let cls = pos_args[0].clone();
                    let args = pos_args[1..].to_vec();
                    return Ok(self
                        .try_instantiate_ast_node(&cls, args, kwargs)?
                        .unwrap_or_else(|| PyObject::instance(cls)));
                }
                // property.__init__(self, fget=None, fset=None, fdel=None, doc=None)
                if nf_data.name.as_str() == "property.__init__" {
                    if pos_args.is_empty() {
                        return Ok(PyObject::none());
                    }
                    Self::init_property_instance_attrs(&pos_args[0], &pos_args[1..], &kwargs)?;
                    return Ok(PyObject::none());
                }
                if let Some(result) = self.call_collection_native_kw(nf_data, &pos_args, &kwargs)? {
                    return Ok(result);
                }
                if let Some(result) =
                    self.call_regex_or_iter_native_kw(nf_data, &pos_args, &kwargs)?
                {
                    return Ok(result);
                }
                // type.__call__(cls, *args, **kwargs) — standard class instantiation
                if nf_data.name.as_str() == "__type_call__" {
                    if pos_args.is_empty() {
                        return Err(PyException::type_error("type.__call__ requires cls"));
                    }
                    let cls = pos_args[0].clone();
                    let rest = pos_args[1..].to_vec();
                    return self.instantiate_class(&cls, rest, kwargs);
                }
                // json.loads with object_hook/parse_float/parse_int Python callables
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
                        // Call native json.loads without hooks to get parsed data
                        let filtered_kwargs: Vec<(CompactString, PyObjectRef)> = kwargs
                            .iter()
                            .filter(|(k, _)| {
                                !matches!(
                                    k.as_str(),
                                    "object_hook"
                                        | "parse_float"
                                        | "parse_int"
                                        | "object_pairs_hook"
                                )
                            })
                            .cloned()
                            .collect();
                        let mut load_args = pos_args.clone();
                        if !filtered_kwargs.is_empty() {
                            let mut kw_map = IndexMap::new();
                            for (k, v) in filtered_kwargs {
                                kw_map.insert(HashableKey::str_key(k), v);
                            }
                            load_args.push(PyObject::dict(kw_map));
                        }
                        let parsed = (nf_data.func)(&load_args)?;
                        // Apply hooks via VM (can call Python functions)
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
                        return self.json_apply_hooks(
                            &parsed,
                            &object_hook,
                            &parse_float,
                            &parse_int,
                        );
                    }
                }
                // json.dumps / json.dump with `default` kwarg that may be a Python function
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
                                // Create an encoder instance and bind its default method
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
                            // Pre-process object tree: call `default` on non-serializable values
                            let prepared = self.json_prepare_with_default(&pos_args[0], def)?;
                            // Rebuild kwargs without `default` and `cls`
                            let filtered_kwargs: Vec<(CompactString, PyObjectRef)> = kwargs
                                .into_iter()
                                .filter(|(k, _)| k.as_str() != "default" && k.as_str() != "cls")
                                .collect();
                            if nf_data.name.as_str() == "json.dump" {
                                // json.dump(obj, fp, **kwargs) → dump prepared obj to fp
                                let mut dump_args = vec![prepared];
                                if pos_args.len() > 1 {
                                    dump_args.push(pos_args[1].clone());
                                }
                                if !filtered_kwargs.is_empty() {
                                    let mut kw_map = IndexMap::new();
                                    for (k, v) in filtered_kwargs {
                                        kw_map.insert(HashableKey::str_key(k), v);
                                    }
                                    dump_args.push(PyObject::dict(kw_map));
                                }
                                return (nf_data.func)(&dump_args);
                            }
                            // json.dumps(prepared, **remaining_kwargs)
                            let mut dump_args = vec![prepared];
                            if !filtered_kwargs.is_empty() {
                                let mut kw_map = IndexMap::new();
                                for (k, v) in filtered_kwargs {
                                    kw_map.insert(HashableKey::str_key(k), v);
                                }
                                dump_args.push(PyObject::dict(kw_map));
                            }
                            return (nf_data.func)(&dump_args);
                        }
                    }
                }
                // Pass kwargs as trailing dict if present
                if !kwargs.is_empty() {
                    let mut all_args = pos_args;
                    let mut kw_map = IndexMap::new();
                    for (k, v) in kwargs {
                        kw_map.insert(HashableKey::str_key(k), v);
                    }
                    if matches!(
                        nf_data.name.as_str(),
                        "weakref.__new__" | "weakref.__init__"
                    ) {
                        kw_map.insert(
                            HashableKey::str_key(CompactString::from("__weakref_ref_kwargs__")),
                            PyObject::bool_val(true),
                        );
                    }
                    all_args.push(PyObject::dict(kw_map));
                    return (nf_data.func)(&all_args);
                }
                return (nf_data.func)(&pos_args);
            }
            PyObjectPayload::NativeClosure(nc) => {
                return self.call_native_closure_kw(nc, pos_args, kwargs);
            }
            PyObjectPayload::Partial(pd) => {
                return self.call_partial_kw(pd, pos_args, kwargs);
            }
            PyObjectPayload::ExceptionType(kind) => {
                return self.call_exception_type_kw(*kind, pos_args, &kwargs);
            }
            PyObjectPayload::Instance(_) => {
                return self.call_instance_with_kw(func, pos_args, kwargs);
            }
            _ => {}
        }
        self.call_object_with_trailing_kwargs(func, pos_args, kwargs)
    }
}
