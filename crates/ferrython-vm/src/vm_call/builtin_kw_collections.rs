use compact_str::CompactString;
use ferrython_core::error::PyResult;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn builtin_sorted_kw(
        &mut self,
        pos_args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<Option<PyObjectRef>> {
        if pos_args.is_empty() {
            return Ok(None);
        }
        // Steal contents if list is temporary (refcount==1) to avoid cloning.
        let mut items_vec = if let PyObjectPayload::List(ref cell) = pos_args[0].payload {
            if PyObjectRef::strong_count(&pos_args[0]) == 1 {
                std::mem::take(&mut *cell.write())
            } else {
                cell.read().clone()
            }
        } else if let PyObjectPayload::Tuple(ref t) = pos_args[0].payload {
            t.to_vec()
        } else {
            self.collect_iterable(&pos_args[0])?
        };
        let key_fn = kwargs
            .iter()
            .find(|(k, _)| k.as_str() == "key")
            .map(|(_, v)| v.clone());
        let reverse = kwargs
            .iter()
            .find(|(k, _)| k.as_str() == "reverse")
            .map(|(_, v)| v.is_truthy())
            .unwrap_or(false);
        self.sort_with_key(&mut items_vec, key_fn, reverse)?;
        Ok(Some(PyObject::list(items_vec)))
    }

    pub(super) fn builtin_dict_kw(
        &mut self,
        pos_args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<PyObjectRef> {
        let mut map = IndexMap::new();
        if !pos_args.is_empty() {
            let mut handled = false;
            if let PyObjectPayload::Dict(src) = &pos_args[0].payload {
                for (k, v) in src.read().iter() {
                    map.insert(k.clone(), v.clone());
                }
                handled = true;
            }
            if !handled {
                if let PyObjectPayload::MappingProxy(src) = &pos_args[0].payload {
                    for (k, v) in src.read().iter() {
                        map.insert(k.clone(), v.clone());
                    }
                    handled = true;
                }
            }
            if !handled {
                if let PyObjectPayload::InstanceDict(src) = &pos_args[0].payload {
                    let read = ferrython_core::object::helpers::instance_dict_as_hashkey_map(src);
                    for (k, v) in read {
                        map.insert(k, v);
                    }
                    handled = true;
                }
            }
            if !handled {
                if let PyObjectPayload::Instance(inst) = &pos_args[0].payload {
                    if let Some(ref ds) = inst.dict_storage {
                        for (k, v) in ds.read().iter() {
                            map.insert(k.clone(), v.clone());
                        }
                        handled = true;
                    }
                }
            }
            if !handled {
                let items = self.collect_iterable(&pos_args[0])?;
                for item in &items {
                    let pair = item.to_list()?;
                    if pair.len() == 2 {
                        let hk = pair[0].to_hashable_key()?;
                        map.insert(hk, pair[1].clone());
                    }
                }
            }
        }
        for (k, v) in kwargs {
            map.insert(HashableKey::str_key(k.clone()), v.clone());
        }
        Ok(PyObject::dict(map))
    }

    pub(super) fn builtin_enumerate_kw(
        &mut self,
        func: PyObjectRef,
        pos_args: Vec<PyObjectRef>,
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<PyObjectRef> {
        let start = kwargs
            .iter()
            .find(|(k, _)| k.as_str() == "start")
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| PyObject::int(0));
        let mut all_args = pos_args;
        all_args.push(start);
        self.call_object(func, all_args)
    }
}
