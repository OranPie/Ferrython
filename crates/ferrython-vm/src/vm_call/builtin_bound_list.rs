use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{
    BuiltinBoundMethodData, IteratorData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;

use crate::builtins;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_list_bound_method(
        &mut self,
        bbm: &BuiltinBoundMethodData,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        let receiver = if let PyObjectPayload::Instance(inst) = &bbm.receiver.payload {
            inst.attrs
                .read()
                .get("__builtin_value__")
                .cloned()
                .filter(|value| matches!(&value.payload, PyObjectPayload::List(_)))
                .unwrap_or_else(|| bbm.receiver.clone())
        } else {
            bbm.receiver.clone()
        };
        if bbm.method_name.as_str() == "extend" && !args.is_empty() {
            if matches!(receiver.payload, PyObjectPayload::List(_)) {
                if matches!(
                    args[0].payload,
                    PyObjectPayload::Generator(_) | PyObjectPayload::Instance(_)
                ) || (matches!(&args[0].payload, PyObjectPayload::Iterator(ref d) if {
                    let data = d.read();
                    matches!(&*data, IteratorData::Enumerate { .. } | IteratorData::Zip { .. }
                        | IteratorData::ZipLongest { .. } | IteratorData::Islice { .. }
                        | IteratorData::MapOne { .. }
                        | IteratorData::Map { .. } | IteratorData::Filter { .. }
                        | IteratorData::FilterFalse { .. }
                        | IteratorData::Sentinel { .. })
                })) {
                    let items = self.collect_iterable(&args[0])?;
                    return builtins::call_method(&receiver, "extend", &[PyObject::list(items)])
                        .map(Some);
                }
            }
        }

        if bbm.method_name.as_str() == "sort" {
            if matches!(&receiver.payload, PyObjectPayload::List(_)) {
                let mut key_fn: Option<PyObjectRef> = None;
                let mut reverse = false;
                let mut positional = 0usize;
                for arg in args {
                    if let PyObjectPayload::Dict(d) = &arg.payload {
                        let rd = d.read();
                        if let Some(v) =
                            rd.get(&HashableKey::str_key(CompactString::from("reverse")))
                        {
                            reverse = v.is_truthy();
                        }
                        if let Some(v) = rd.get(&HashableKey::str_key(CompactString::from("key"))) {
                            if !matches!(v.payload, PyObjectPayload::None) {
                                key_fn = Some(v.clone());
                            }
                        }
                    } else {
                        positional += 1;
                    }
                }
                if positional > 0 {
                    return builtins::call_method(&receiver, "sort", args).map(Some);
                }
                self.vm_sort_list_in_place(&receiver, key_fn, reverse)?;
                return Ok(Some(PyObject::none()));
            }
        }

        if !PyObjectRef::ptr_eq(&receiver, &bbm.receiver)
            && matches!(&receiver.payload, PyObjectPayload::List(_))
        {
            let result = builtins::call_method(&receiver, bbm.method_name.as_str(), args)?;
            if matches!(bbm.method_name.as_str(), "__iadd__" | "__imul__") {
                return Ok(Some(bbm.receiver.clone()));
            }
            return Ok(Some(result));
        }

        Ok(None)
    }
}
