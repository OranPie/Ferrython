use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn populate_dict_subclass_storage(
        &mut self,
        instance: &PyObjectRef,
        pos_args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<()> {
        let PyObjectPayload::Instance(inst) = &instance.payload else {
            return Ok(());
        };
        let Some(ref ds) = inst.dict_storage else {
            return Ok(());
        };

        let mut entries = Vec::new();
        if !pos_args.is_empty() {
            match &pos_args[0].payload {
                PyObjectPayload::Dict(src) => {
                    for (k, v) in src.read().iter() {
                        entries.push((k.clone(), v.clone()));
                    }
                }
                PyObjectPayload::Instance(src_inst) if src_inst.dict_storage.is_some() => {
                    if let Some(src_ds) = src_inst.dict_storage.as_ref() {
                        for (k, v) in src_ds.read().iter() {
                            entries.push((k.clone(), v.clone()));
                        }
                    }
                }
                _ => {
                    let items = self.collect_iterable(&pos_args[0])?;
                    for item in &items {
                        let pair = item.to_list()?;
                        if pair.len() == 2 {
                            entries.push((pair[0].to_hashable_key()?, pair[1].clone()));
                        }
                    }
                }
            }
        }

        let mut storage = ds.write();
        for (k, v) in entries {
            storage.insert(k, v);
        }
        for (k, v) in kwargs {
            storage.insert(HashableKey::str_key(k.clone()), v.clone());
        }
        Ok(())
    }
}
