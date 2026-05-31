use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::helpers::mark_dict_storage_mutated;
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
            if storage.insert(k, v).is_none() {
                mark_dict_storage_mutated(ds);
            }
        }
        for (k, v) in kwargs {
            if storage
                .insert(HashableKey::str_key(k.clone()), v.clone())
                .is_none()
            {
                mark_dict_storage_mutated(ds);
            }
        }
        Ok(())
    }

    pub(super) fn dict_fromkeys_for_class(
        &mut self,
        cls: &PyObjectRef,
        iterable: &PyObjectRef,
        value: PyObjectRef,
    ) -> PyResult<PyObjectRef> {
        let result = self.instantiate_class(cls, vec![], vec![])?;
        let keys = self.collect_iterable(iterable)?;
        if let PyObjectPayload::Instance(inst) = &result.payload {
            if let Some(ref ds) = inst.dict_storage {
                if Self::class_has_user_override(&inst.class, "__setitem__") {
                    if let Some(setitem) = result.get_attr("__setitem__") {
                        for key in keys {
                            self.call_object(setitem.clone(), vec![key, value.clone()])?;
                        }
                        return Ok(result);
                    }
                } else {
                    let mut storage = ds.write();
                    for key in keys {
                        if storage
                            .insert(key.to_hashable_key()?, value.clone())
                            .is_none()
                        {
                            mark_dict_storage_mutated(ds);
                        }
                    }
                    return Ok(result);
                }
            }
        }
        for key in keys {
            let setitem = result.get_attr("__setitem__").ok_or_else(|| {
                ferrython_core::error::PyException::type_error(format!(
                    "'{}' object does not support item assignment",
                    result.type_name()
                ))
            })?;
            self.call_object(setitem, vec![key, value.clone()])?;
        }
        Ok(result)
    }
}
