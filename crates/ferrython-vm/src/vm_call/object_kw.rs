use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(crate) fn call_object_kw(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: Vec<(CompactString, PyObjectRef)>,
    ) -> PyResult<PyObjectRef> {
        if kwargs.is_empty() {
            return self.call_object(func, pos_args);
        }
        match &func.payload {
            PyObjectPayload::Function(pyfunc) => {
                let globals = pyfunc.globals.clone();
                let defaults = pyfunc.defaults.read();
                let kw_defaults = pyfunc.kw_defaults.read();
                self.call_function_kw(
                    &pyfunc.code,
                    pos_args,
                    kwargs,
                    &defaults,
                    &kw_defaults,
                    globals,
                    &pyfunc.closure,
                    &pyfunc.constant_cache,
                )
            }
            PyObjectPayload::BoundMethod { receiver, method } => {
                let mut bound_args = vec![receiver.clone()];
                bound_args.extend(pos_args);
                self.call_object_kw(method.clone(), bound_args, kwargs)
            }
            PyObjectPayload::Class(cd) => {
                if cd.name.as_str() == "weakref" && !kwargs.is_empty() {
                    return Err(PyException::type_error("ref() takes no keyword arguments"));
                }
                // If the metaclass defines its own __call__ (not just type.__call__),
                // dispatch through it.
                if let Some(meta) = &cd.metaclass {
                    if let Some(call_method) = meta.get_attr("__call__") {
                        let is_inherited_type_call = matches!(
                            &call_method.payload,
                            PyObjectPayload::BuiltinBoundMethod(bbm)
                                if bbm.method_name.as_str() == "__call__"
                                && matches!(&bbm.receiver.payload, PyObjectPayload::BuiltinType(t) if t.as_str() == "type")
                        );
                        if !is_inherited_type_call {
                            let mut call_args = vec![func.clone()];
                            call_args.extend(pos_args);
                            if kwargs.is_empty() {
                                return self.call_object(call_method, call_args);
                            } else {
                                return self.call_object_kw(call_method, call_args, kwargs);
                            }
                        }
                    }
                }
                self.instantiate_class(&func, pos_args, kwargs)
            }
            _ => {
                // For BuiltinBoundMethod on str.format, pass kwargs as a dict
                if let PyObjectPayload::BuiltinBoundMethod(bbm) = &func.payload {
                    // Handle list.sort(key=..., reverse=...)
                    if bbm.method_name.as_str() == "sort" {
                        if matches!(&bbm.receiver.payload, PyObjectPayload::List(_)) {
                            let key_fn = kwargs
                                .iter()
                                .find(|(k, _)| k.as_str() == "key")
                                .map(|(_, v)| v.clone());
                            let reverse = kwargs
                                .iter()
                                .find(|(k, _)| k.as_str() == "reverse")
                                .map(|(_, v)| v.is_truthy())
                                .unwrap_or(false);
                            self.vm_sort_list_in_place(&bbm.receiver, key_fn, reverse)?;
                            return Ok(PyObject::none());
                        }
                    }
                    // Handle dict.update(key=val, ...)
                    if bbm.method_name.as_str() == "update" && !kwargs.is_empty() {
                        if let PyObjectPayload::Dict(map) = &bbm.receiver.payload {
                            if let Some(first) = pos_args.first() {
                                let update = bbm.receiver.get_attr("update").ok_or_else(|| {
                                    PyException::attribute_error(
                                        "'dict' object has no attribute 'update'",
                                    )
                                })?;
                                self.call_object(update, vec![first.clone()])?;
                            }
                            let mut w = map.write();
                            for (k, v) in &kwargs {
                                w.insert(HashableKey::str_key(k.clone()), v.clone());
                            }
                            return Ok(PyObject::none());
                        }
                    }
                    if bbm.method_name.as_str() == "format" && !kwargs.is_empty() {
                        if let PyObjectPayload::Str(s) = &bbm.receiver.payload {
                            // Handle str.format() with named args via VM-aware formatter
                            return self.vm_str_format_kw(s, &pos_args, &kwargs);
                        }
                    }
                }
                // BuiltinBoundMethod kwargs: resolve known kwargs to positional args
                if let PyObjectPayload::BuiltinBoundMethod(bbm) = &func.payload {
                    if !kwargs.is_empty() {
                        match bbm.method_name.as_str() {
                            // str.encode(encoding=, errors=) / bytes.decode(encoding=, errors=)
                            "encode" | "decode" => {
                                let mut resolved = pos_args;
                                if resolved.is_empty() {
                                    // encoding kwarg or default
                                    let enc = kwargs
                                        .iter()
                                        .find(|(k, _)| k.as_str() == "encoding")
                                        .map(|(_, v)| v.clone())
                                        .unwrap_or_else(|| {
                                            PyObject::str_val(CompactString::from("utf-8"))
                                        });
                                    resolved.push(enc);
                                }
                                if resolved.len() < 2 {
                                    if let Some((_, v)) =
                                        kwargs.iter().find(|(k, _)| k.as_str() == "errors")
                                    {
                                        resolved.push(v.clone());
                                    }
                                }
                                return self.call_object(func, resolved);
                            }
                            _ => {
                                if matches!(
                                    &bbm.receiver.payload,
                                    PyObjectPayload::Set(_) | PyObjectPayload::FrozenSet(_)
                                ) && bbm.method_name.as_str() == "__init__"
                                {
                                    return Err(PyException::type_error(format!(
                                        "{}() takes no keyword arguments",
                                        bbm.method_name
                                    )));
                                }
                                // Generic fallback: pass kwargs as trailing dict
                                let mut all_args = pos_args;
                                let mut kw_map = IndexMap::new();
                                for (k, v) in kwargs {
                                    kw_map.insert(HashableKey::str_key(k), v);
                                }
                                all_args.push(PyObject::dict(kw_map));
                                return self.call_object(func, all_args);
                            }
                        }
                    }
                }
                let builtin_name = match &func.payload {
                    PyObjectPayload::BuiltinFunction(name) | PyObjectPayload::BuiltinType(name) => {
                        Some((**name).clone())
                    }
                    _ => None,
                };
                if let Some(name) = builtin_name {
                    return self.call_builtin_kw(func, &name, pos_args, kwargs);
                }
                self.call_native_or_fallback_kw(func, pos_args, kwargs)
            }
        }
    }
}
