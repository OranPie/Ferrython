use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    BuiltinBoundMethodData, IteratorData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

use crate::builtins;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_builtin_bound_fast_path(
        &mut self,
        bbm: &BuiltinBoundMethodData,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        if let Some(receiver) = set_like_receiver(&bbm.receiver) {
            if !args.is_empty()
                && is_set_iterable_method(bbm.method_name.as_str())
                && args.iter().any(needs_vm_iterable_collection)
            {
                let mut resolved = Vec::with_capacity(args.len());
                for arg in args {
                    if needs_vm_iterable_collection(arg) {
                        resolved.push(PyObject::list(self.collect_iterable(arg)?));
                    } else {
                        resolved.push(arg.clone());
                    }
                }
                return Ok(Some(builtins::call_method(
                    &receiver,
                    bbm.method_name.as_str(),
                    &resolved,
                )?));
            }
        }

        match &bbm.receiver.payload {
            PyObjectPayload::DictKeys { .. } | PyObjectPayload::DictItems { .. } => {
                match bbm.method_name.as_str() {
                    "__contains__" => {
                        if args.len() != 1 {
                            return Err(PyException::type_error(
                                "__contains__() takes exactly one argument",
                            ));
                        }
                        return Ok(Some(PyObject::bool_val(bbm.receiver.contains(&args[0])?)));
                    }
                    "isdisjoint" => {
                        if args.len() != 1 {
                            return Err(PyException::type_error(
                                "isdisjoint() takes exactly one argument",
                            ));
                        }
                        let other = if needs_vm_iterable_collection(&args[0]) {
                            PyObject::list(self.collect_iterable(&args[0])?)
                        } else {
                            args[0].clone()
                        };
                        let intersection = bbm.receiver.bit_and(&other)?;
                        return Ok(Some(PyObject::bool_val(intersection.py_len()? == 0)));
                    }
                    "__copy__" | "__deepcopy__" | "__reduce__" | "__reduce_ex__" => {
                        return Err(PyException::type_error(format!(
                            "cannot pickle '{}' object",
                            bbm.receiver.type_name()
                        )));
                    }
                    _ => {}
                }
                return Ok(None);
            }
            PyObjectPayload::DictValues { .. } => {
                match bbm.method_name.as_str() {
                    "__contains__" => {
                        if args.len() != 1 {
                            return Err(PyException::type_error(
                                "__contains__() takes exactly one argument",
                            ));
                        }
                        return Ok(Some(PyObject::bool_val(bbm.receiver.contains(&args[0])?)));
                    }
                    "__copy__" | "__deepcopy__" | "__reduce__" | "__reduce_ex__" => {
                        return Err(PyException::type_error(format!(
                            "cannot pickle '{}' object",
                            bbm.receiver.type_name()
                        )));
                    }
                    _ => {}
                }
                return Ok(None);
            }
            PyObjectPayload::Set(_) | PyObjectPayload::FrozenSet(_)
                if !args.is_empty()
                    && is_set_iterable_method(bbm.method_name.as_str())
                    && args.iter().any(needs_vm_iterable_collection) =>
            {
                let mut resolved = Vec::with_capacity(args.len());
                for arg in args {
                    if needs_vm_iterable_collection(arg) {
                        resolved.push(PyObject::list(self.collect_iterable(arg)?));
                    } else {
                        resolved.push(arg.clone());
                    }
                }
                Ok(Some(builtins::call_method(
                    &bbm.receiver,
                    bbm.method_name.as_str(),
                    &resolved,
                )?))
            }
            PyObjectPayload::List(_)
                if bbm.method_name.as_str() == "extend"
                    && !args.is_empty()
                    && (matches!(
                        &args[0].payload,
                        PyObjectPayload::Generator(_) | PyObjectPayload::Instance(_)
                    ) || matches!(&args[0].payload, PyObjectPayload::Iterator(ref d) if {
                        let data = d.read();
                        matches!(&*data, IteratorData::Enumerate { .. } | IteratorData::Zip { .. }
                            | IteratorData::ZipLongest { .. } | IteratorData::Islice { .. }
                            | IteratorData::MapOne { .. }
                            | IteratorData::Map { .. } | IteratorData::Filter { .. }
                            | IteratorData::FilterFalse { .. }
                            | IteratorData::Sentinel { .. })
                    })) =>
            {
                let items = self.collect_iterable(&args[0])?;
                Ok(Some(builtins::call_method(
                    &bbm.receiver,
                    "extend",
                    &[PyObject::list(items)],
                )?))
            }
            PyObjectPayload::Dict(_) | PyObjectPayload::MappingProxy(_)
                if bbm.method_name.as_str() == "fromkeys" =>
            {
                if !args.is_empty()
                    && matches!(
                        &args[0].payload,
                        PyObjectPayload::Generator(_)
                            | PyObjectPayload::Instance(_)
                            | PyObjectPayload::Iterator(_)
                    )
                {
                    let mut resolved = Vec::with_capacity(args.len());
                    resolved.push(PyObject::list(self.collect_iterable(&args[0])?));
                    resolved.extend_from_slice(&args[1..]);
                    Ok(Some(builtins::core_fns::builtin_dict_fromkeys(&resolved)?))
                } else {
                    Ok(Some(builtins::core_fns::builtin_dict_fromkeys(args)?))
                }
            }
            PyObjectPayload::Str(_)
            | PyObjectPayload::List(_)
            | PyObjectPayload::Dict(_)
            | PyObjectPayload::Tuple(_)
            | PyObjectPayload::Set(_)
            | PyObjectPayload::Int(_)
            | PyObjectPayload::Float(_)
            | PyObjectPayload::Bool(_)
            | PyObjectPayload::Range(_)
            | PyObjectPayload::Bytes(_)
            | PyObjectPayload::ByteArray(_)
            | PyObjectPayload::FrozenSet(_)
                if !(matches!(&bbm.receiver.payload, PyObjectPayload::List(_))
                    && bbm.method_name.as_str() == "sort")
                    && !(bbm.method_name.as_str() == "join"
                        && matches!(
                            &bbm.receiver.payload,
                            PyObjectPayload::Str(_)
                                | PyObjectPayload::Bytes(_)
                                | PyObjectPayload::ByteArray(_)
                        )) =>
            {
                Ok(Some(builtins::call_method(
                    &bbm.receiver,
                    bbm.method_name.as_str(),
                    args,
                )?))
            }
            _ => Ok(None),
        }
    }
}

fn is_set_iterable_method(name: &str) -> bool {
    matches!(
        name,
        "union"
            | "intersection"
            | "difference"
            | "symmetric_difference"
            | "update"
            | "intersection_update"
            | "difference_update"
            | "symmetric_difference_update"
            | "issubset"
            | "issuperset"
            | "isdisjoint"
            | "__or__"
            | "__and__"
            | "__sub__"
            | "__xor__"
    )
}

fn needs_vm_iterable_collection(obj: &PyObjectRef) -> bool {
    matches!(
        &obj.payload,
        PyObjectPayload::Generator(_)
            | PyObjectPayload::Instance(_)
            | PyObjectPayload::Iterator(_)
            | PyObjectPayload::RangeIter(_)
            | PyObjectPayload::VecIter(_)
            | PyObjectPayload::DictValueIter(_)
            | PyObjectPayload::WeakValueIter(_)
            | PyObjectPayload::WeakKeyIter(_)
            | PyObjectPayload::DequeIter(_)
            | PyObjectPayload::RefIter { .. }
            | PyObjectPayload::RevRefIter { .. }
    )
}

fn set_like_receiver(receiver: &PyObjectRef) -> Option<PyObjectRef> {
    match &receiver.payload {
        PyObjectPayload::Set(_) | PyObjectPayload::FrozenSet(_) => Some(receiver.clone()),
        PyObjectPayload::Instance(inst) => inst
            .attrs
            .read()
            .get("__builtin_value__")
            .cloned()
            .filter(|value| {
                matches!(
                    &value.payload,
                    PyObjectPayload::Set(_) | PyObjectPayload::FrozenSet(_)
                )
            }),
        _ => None,
    }
}
