use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{BuiltinBoundMethodData, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_format_bound_method(
        &mut self,
        bbm: &BuiltinBoundMethodData,
        args: &[PyObjectRef],
    ) -> PyResult<Option<PyObjectRef>> {
        if bbm.method_name.as_str() == "format" {
            if let PyObjectPayload::Str(s) = &bbm.receiver.payload {
                return self.vm_str_format(s, args).map(Some);
            }
        }
        if bbm.method_name.as_str() == "format_map" && !args.is_empty() {
            if let PyObjectPayload::Str(s) = &bbm.receiver.payload {
                if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    if let Some(ref ds) = inst.dict_storage {
                        return self.vm_format_map(s, &args[0], ds, &inst.class).map(Some);
                    }
                }
                if let PyObjectPayload::Dict(m) = &args[0].payload {
                    let factory_key =
                        HashableKey::str_key(CompactString::from("__defaultdict_factory__"));
                    if m.read().contains_key(&factory_key) {
                        return self.vm_format_map_dict(s, &args[0], m).map(Some);
                    }
                }
            }
        }
        Ok(None)
    }
}
