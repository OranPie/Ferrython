use ferrython_core::error::PyResult;
use ferrython_core::object::{
    BuiltinBoundMethodData, IteratorData, PyObject, PyObjectPayload, PyObjectRef,
};

use crate::builtins;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_builtin_bound_fast_path(
        &mut self,
        bbm: &BuiltinBoundMethodData,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        match &bbm.receiver.payload {
            PyObjectPayload::Set(_) | PyObjectPayload::FrozenSet(_)
                if !args.is_empty()
                    && matches!(
                        bbm.method_name.as_str(),
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
                    && matches!(
                        &args[0].payload,
                        PyObjectPayload::Generator(_)
                            | PyObjectPayload::Instance(_)
                            | PyObjectPayload::Iterator(_)
                    ) =>
            {
                let mut resolved = Vec::with_capacity(args.len());
                resolved.push(PyObject::list(self.collect_iterable(&args[0])?));
                resolved.extend_from_slice(&args[1..]);
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
