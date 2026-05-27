use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObjectMethods, PyObjectPayload, PyObjectRef};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn builtin_type_instance_operand(
        type_name: &str,
        method_name: &str,
        args: &[PyObjectRef],
    ) -> PyResult<(PyObjectRef, Vec<PyObjectRef>)> {
        let Some(instance) = args.first() else {
            return Err(PyException::type_error(format!(
                "unbound method {}.{}() needs an argument",
                type_name, method_name
            )));
        };
        let matches_receiver = match type_name {
            "bytes" => matches!(&instance.payload, PyObjectPayload::Bytes(_)),
            "bytearray" => matches!(&instance.payload, PyObjectPayload::ByteArray(_)),
            _ => false,
        };
        if matches_receiver {
            return Ok((instance.clone(), args[1..].to_vec()));
        }
        if let PyObjectPayload::Instance(inst) = &instance.payload {
            if let PyObjectPayload::Class(cd) = &inst.class.payload {
                if cd.builtin_base_name.as_ref().map(|s| s.as_str()) == Some(type_name) {
                    if let Some(value) = inst.attrs.read().get("__builtin_value__").cloned() {
                        return Ok((value, args[1..].to_vec()));
                    }
                }
            }
        }
        Err(PyException::type_error(format!(
            "descriptor '{}' for '{}' objects doesn't apply to a '{}' object",
            method_name,
            type_name,
            instance.type_name()
        )))
    }
}
