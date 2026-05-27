use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    NativeFunctionData, PartialData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_collection_native_kw(
        &mut self,
        nf_data: &NativeFunctionData,
        pos_args: &[PyObjectRef],
        kwargs: &[(CompactString, PyObjectRef)],
    ) -> PyResult<Option<PyObjectRef>> {
        if nf_data.name.as_str() == "collections.OrderedDict"
            || nf_data.name.as_str() == "collections.Counter"
        {
            let mut map = IndexMap::new();
            if !pos_args.is_empty() {
                if let PyObjectPayload::Dict(src) = &pos_args[0].payload {
                    for (k, v) in src.read().iter() {
                        map.insert(k.clone(), v.clone());
                    }
                } else {
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
            if nf_data.name.as_str() == "collections.Counter" {
                return (nf_data.func)(&[PyObject::dict(map)]).map(Some);
            }
            return Ok(Some(PyObject::dict(map)));
        }

        if nf_data.name.as_str() == "collections.defaultdict" {
            let mut all = pos_args.to_vec();
            if !kwargs.is_empty() {
                let mut map = IndexMap::new();
                if all.len() >= 2 {
                    if let PyObjectPayload::Dict(src) = &all[1].payload {
                        for (k, v) in src.read().iter() {
                            map.insert(k.clone(), v.clone());
                        }
                    }
                }
                for (k, v) in kwargs {
                    map.insert(HashableKey::str_key(k.clone()), v.clone());
                }
                if all.len() >= 2 {
                    all[1] = PyObject::dict(map);
                } else {
                    while all.is_empty() {
                        all.push(PyObject::none());
                    }
                    all.push(PyObject::dict(map));
                }
            }
            return (nf_data.func)(&all).map(Some);
        }

        if nf_data.name.as_str() == "collections.deque" {
            let mut all = pos_args.to_vec();
            if let Some((_, v)) = kwargs.iter().find(|(k, _)| k.as_str() == "maxlen") {
                while all.is_empty() {
                    all.push(PyObject::list(vec![]));
                }
                if all.len() < 2 {
                    all.push(v.clone());
                } else {
                    all[1] = v.clone();
                }
            }
            return (nf_data.func)(&all).map(Some);
        }

        if nf_data.name.as_str() == "WeakValueDictionary"
            || nf_data.name.as_str() == "WeakKeyDictionary"
        {
            let instance = (nf_data.func)(pos_args)?;
            if !kwargs.is_empty() {
                if let Some(update) = instance.get_attr("update") {
                    self.call_object_kw(update, vec![], kwargs.to_vec())?;
                }
            }
            return Ok(Some(instance));
        }

        if nf_data.name.as_str() == "functools.partial" {
            if pos_args.is_empty() {
                return Err(PyException::type_error(
                    "partial() requires at least 1 argument",
                ));
            }
            let pf = pos_args[0].clone();
            let pa = if pos_args.len() > 1 {
                pos_args[1..].to_vec()
            } else {
                vec![]
            };
            return Ok(Some(PyObject::wrap(PyObjectPayload::Partial(Box::new(
                PartialData {
                    func: pf,
                    args: pa,
                    kwargs: kwargs.to_vec(),
                },
            )))));
        }

        Ok(None)
    }
}
