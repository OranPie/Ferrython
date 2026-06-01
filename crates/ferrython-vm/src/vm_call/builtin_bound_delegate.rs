use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{
    BuiltinBoundMethodData, IteratorData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    CLASS_FLAG_HAS_DESCRIPTORS,
};
use ferrython_core::types::HashableKey;

use crate::builtins;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_namedtuple_deque_or_hashlib_bound_method(
        &mut self,
        bbm: &BuiltinBoundMethodData,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        if let PyObjectPayload::Instance(inst) = &bbm.receiver.payload {
            if matches!(&inst.class.payload, PyObjectPayload::Class(cd) if cd.namespace.read().contains_key("__namedtuple__"))
                || inst.attrs.read().contains_key("__deque__")
            {
                if inst.attrs.read().contains_key("__deque__")
                    && matches!(bbm.method_name.as_str(), "extend" | "extendleft")
                {
                    let items = self.collect_iterable(&args[0])?;
                    return builtins::call_method(
                        &bbm.receiver,
                        bbm.method_name.as_str(),
                        &[PyObject::list(items)],
                    )
                    .map(Some);
                }
                return builtins::call_method(&bbm.receiver, bbm.method_name.as_str(), args)
                    .map(Some);
            }

            let class_name = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                cd.name.to_string()
            } else {
                String::new()
            };
            if matches!(
                class_name.as_str(),
                "md5" | "sha1" | "sha256" | "sha224" | "sha384" | "sha512"
            ) {
                return builtins::call_method(&bbm.receiver, bbm.method_name.as_str(), args)
                    .map(Some);
            }
        }
        Ok(None)
    }

    pub(super) fn call_builtin_type_bound_method(
        &mut self,
        bbm: &BuiltinBoundMethodData,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        let PyObjectPayload::BuiltinType(tn) = &bbm.receiver.payload else {
            if bbm.method_name.as_str() == "fromkeys" {
                if let PyObjectPayload::Instance(inst) = &bbm.receiver.payload {
                    if inst.dict_storage.is_some() {
                        let call_args = if args
                            .first()
                            .is_some_and(|arg| PyObjectRef::ptr_eq(arg, &bbm.receiver))
                        {
                            &args[1..]
                        } else {
                            args
                        };
                        let Some(iterable) = call_args.first() else {
                            return Ok(None);
                        };
                        let value = call_args.get(1).cloned().unwrap_or_else(PyObject::none);
                        return self
                            .dict_fromkeys_for_class(&inst.class, iterable, value)
                            .map(Some);
                    }
                }
            }
            if bbm.method_name.as_str() == "fromkeys"
                && matches!(
                    bbm.receiver.payload,
                    PyObjectPayload::Dict(_)
                        | PyObjectPayload::InstanceDict(_)
                        | PyObjectPayload::MappingProxy(_)
                )
            {
                let Some(class_method) = builtins::resolve_type_class_method("dict", "fromkeys")
                else {
                    return Ok(None);
                };
                if let PyObjectPayload::NativeFunction(nf) = &class_method.payload {
                    return (nf.func)(args).map(Some);
                }
            }
            return Ok(None);
        };

        if tn.as_str() == "type"
            && matches!(bbm.method_name.as_str(), "__setattr__" | "__delattr__")
        {
            if let Some(method) =
                builtins::resolve_type_class_method("type", bbm.method_name.as_str())
            {
                if let PyObjectPayload::NativeFunction(nf) = &method.payload {
                    return (nf.func)(args).map(Some);
                }
            }
        }

        if tn.as_str() == "type" && bbm.method_name.as_str() == "__call__" && !args.is_empty() {
            if matches!(&args[0].payload, PyObjectPayload::Class(_)) {
                let cls = args[0].clone();
                let mut rest = args[1..].to_vec();
                let kw = {
                    let mut extracted = vec![];
                    let should_pop = if let Some(last) = rest.last() {
                        if let PyObjectPayload::Dict(map) = &last.payload {
                            let rd = map.read();
                            let all_str = rd.keys().all(|k| matches!(k, HashableKey::Str(_)));
                            if all_str && !rd.is_empty() {
                                for (k, v) in rd.iter() {
                                    if let HashableKey::Str(s) = k {
                                        extracted.push((s.to_compact_string(), v.clone()));
                                    }
                                }
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if should_pop {
                        rest.pop();
                    }
                    extracted
                };
                return self.instantiate_class(&cls, rest, kw).map(Some);
            }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if inst.class_flags & CLASS_FLAG_HAS_DESCRIPTORS != 0 {
                    if let Some(call_attr) =
                        ferrython_core::object::lookup_in_class_mro(&inst.class, "__call__")
                    {
                        if ferrython_core::object::is_property_like(&call_attr) {
                            if let Some(getter) =
                                ferrython_core::object::property_field(&call_attr, "fget")
                            {
                                if !matches!(&getter.payload, PyObjectPayload::None) {
                                    let getter = builtins::unwrap_abstract_fget(&getter);
                                    return self
                                        .call_object(getter, vec![args[0].clone()])
                                        .map(Some);
                                }
                            }
                        }
                    }
                }
            }
        }

        if matches!(
            bbm.method_name.as_str(),
            "__copy__" | "__deepcopy__" | "__reduce__" | "__reduce_ex__"
        ) && !args.is_empty()
            && matches!(&args[0].payload, PyObjectPayload::Iterator(iter_data)
                if matches!(&*iter_data.read(),
                    IteratorData::Islice { .. }
                        | IteratorData::TakeWhile { .. }
                        | IteratorData::DropWhile { .. }
                        | IteratorData::Tee { .. }))
        {
            let rest_args = if args.len() > 1 {
                args[1..].to_vec()
            } else {
                vec![]
            };
            if let Some(method) = args[0].get_attr(bbm.method_name.as_str()) {
                return self.call_object(method, rest_args).map(Some);
            }
        }

        if let Some(class_method) =
            builtins::resolve_type_class_method(tn, bbm.method_name.as_str())
        {
            if let PyObjectPayload::NativeFunction(nf) = &class_method.payload {
                if nf.name.as_str() == "dict.fromkeys"
                    && !args.is_empty()
                    && matches!(
                        args[0].payload,
                        PyObjectPayload::Generator(_)
                            | PyObjectPayload::Instance(_)
                            | PyObjectPayload::Iterator(_)
                    )
                {
                    let mut resolved = Vec::with_capacity(args.len());
                    resolved.push(PyObject::list(self.collect_iterable(&args[0])?));
                    resolved.extend_from_slice(&args[1..]);
                    return (nf.func)(&resolved).map(Some);
                }
                if nf.name.as_str() == "type.__new__" {
                    let bases = if args.len() == 4 {
                        args[2].to_list().unwrap_or_default()
                    } else if args.len() == 3 {
                        args[1].to_list().unwrap_or_default()
                    } else {
                        vec![]
                    };
                    let result = (nf.func)(args)?;
                    if matches!(&result.payload, PyObjectPayload::Class(_)) {
                        self.finish_pep487_class(&result, &bases, &[])?;
                        if let PyObjectPayload::Class(cd) = &result.payload {
                            cd.namespace.write().insert(
                                CompactString::from("__ferrython_pep487_done__"),
                                PyObject::bool_val(true),
                            );
                        }
                    }
                    return Ok(Some(result));
                }
                return (nf.func)(args).map(Some);
            }
        }

        if matches!(tn.as_str(), "bytes" | "bytearray") && bbm.method_name.as_str() == "hex" {
            let (instance, rest_args) =
                Self::builtin_type_instance_operand(tn.as_str(), bbm.method_name.as_str(), args)?;
            return builtins::call_method(&instance, bbm.method_name.as_str(), &rest_args)
                .map(Some);
        }

        if !args.is_empty() {
            let instance = args[0].clone();
            let rest_args = if args.len() > 1 {
                args[1..].to_vec()
            } else {
                vec![]
            };
            return builtins::call_method(&instance, bbm.method_name.as_str(), &rest_args)
                .map(Some);
        }

        Ok(None)
    }
}
