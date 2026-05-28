//! VM bridges for functools-style higher-order helpers.

use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;

impl VirtualMachine {
    /// Call a Python object (function, builtin, class).
    pub(crate) fn vm_functools_reduce(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "reduce() requires at least 2 arguments",
            ));
        }
        let func = args[0].clone();
        let items = self.collect_iterable(&args[1])?;
        let has_initial = args.len() > 2;
        let mut acc = if has_initial {
            args[2].clone()
        } else if !items.is_empty() {
            items[0].clone()
        } else {
            return Err(PyException::type_error(
                "reduce() of empty sequence with no initial value",
            ));
        };
        let start_idx = if has_initial { 0 } else { 1 };
        for item in &items[start_idx..] {
            acc = self.call_object(func.clone(), vec![acc, item.clone()])?;
        }
        Ok(acc)
    }

    /// VM-level singledispatch call: dispatch based on first arg's type.
    pub(crate) fn vm_singledispatch_call_instance(
        &mut self,
        dispatcher: &PyObjectRef,
        args: &[PyObjectRef],
    ) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error(
                "singledispatch function requires at least 1 argument",
            ));
        }
        let type_name_str = args[0].type_name();
        let default = dispatcher
            .get_attr("__default__")
            .ok_or_else(|| PyException::runtime_error("singledispatch: no default function"))?;
        let registry = dispatcher.get_attr("__registry__");

        let handler = if let Some(ref reg) = registry {
            if let PyObjectPayload::Dict(ref map) = reg.payload {
                let m = map.read();
                m.get(&HashableKey::str_key(CompactString::from(&*type_name_str)))
                    .cloned()
                    .unwrap_or_else(|| default.clone())
            } else {
                default.clone()
            }
        } else {
            default.clone()
        };

        self.call_object(handler, args.to_vec())
    }

    /// VM-level singledispatch.register: register(type) returns decorator.
    pub(crate) fn vm_singledispatch_register(
        &mut self,
        args: &[PyObjectRef],
    ) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "register() requires a type argument",
            ));
        }
        let dispatcher = args[0].clone();
        let type_obj = &args[1];
        let type_name = type_obj
            .get_attr("__name__")
            .map(|n| n.py_to_string().to_string())
            .unwrap_or_else(|| {
                let s = type_obj.py_to_string().to_string();
                if s.starts_with("<class '") && s.ends_with("'>") {
                    s[8..s.len() - 2].to_string()
                } else {
                    s
                }
            });

        if args.len() >= 3 {
            let func = args[2].clone();
            if let Some(reg) = dispatcher.get_attr("__registry__") {
                if let PyObjectPayload::Dict(ref map) = reg.payload {
                    map.write().insert(
                        HashableKey::str_key(CompactString::from(&*type_name)),
                        func.clone(),
                    );
                }
            }
            return Ok(func);
        }

        let tn = type_name.to_string();
        Ok(PyObject::native_closure(
            "singledispatch.register_decorator",
            move |deco_args| {
                if deco_args.is_empty() {
                    return Err(PyException::type_error(
                        "register decorator requires 1 argument",
                    ));
                }
                let func = deco_args[0].clone();
                if let Some(reg) = dispatcher.get_attr("__registry__") {
                    if let PyObjectPayload::Dict(ref map) = reg.payload {
                        map.write()
                            .insert(HashableKey::str_key(CompactString::from(&tn)), func.clone());
                    }
                }
                Ok(func)
            },
        ))
    }
}
