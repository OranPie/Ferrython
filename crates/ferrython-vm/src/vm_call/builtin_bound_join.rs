use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    BuiltinBoundMethodData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_join_bound_method(
        &mut self,
        bbm: &BuiltinBoundMethodData,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        if bbm.method_name.as_str() != "join" || args.is_empty() {
            return Ok(None);
        }
        let str_receiver = match &bbm.receiver.payload {
            PyObjectPayload::Str(sep) => Some(sep.as_str().to_string()),
            PyObjectPayload::Instance(inst) => inst
                .attrs
                .read()
                .get("__builtin_value__")
                .and_then(|value| value.as_str().map(ToString::to_string)),
            _ => None,
        };
        if let Some(sep) = str_receiver {
            let items = self.collect_iterable(&args[0])?;
            let strs: Result<Vec<String>, _> = items
                .iter()
                .map(|x| {
                    x.as_str()
                        .map(String::from)
                        .ok_or_else(|| PyException::type_error("sequence item: expected str"))
                })
                .collect();
            return Ok(Some(PyObject::str_val(CompactString::from(
                strs?.join(&sep),
            ))));
        }
        if let PyObjectPayload::Bytes(sep) | PyObjectPayload::ByteArray(sep) = &bbm.receiver.payload
        {
            let sep = sep.clone();
            let mutable_result = matches!(&bbm.receiver.payload, PyObjectPayload::ByteArray(_));
            let items = self.collect_iterable(&args[0])?;
            let mut result = Vec::new();
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    result.extend_from_slice(&sep);
                }
                if let Some(data) = Self::bytes_like_data(item) {
                    result.extend_from_slice(&data);
                } else {
                    return Err(PyException::type_error(
                        "sequence item: expected a bytes-like object",
                    ));
                }
            }
            return Ok(Some(if mutable_result {
                PyObject::bytearray(result)
            } else {
                PyObject::bytes(result)
            }));
        }
        Ok(None)
    }
}
