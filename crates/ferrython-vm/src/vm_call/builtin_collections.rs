use ferrython_core::error::PyResult;
use ferrython_core::object::{
    new_fx_hashkey_flatmap, new_fx_hashkey_map, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

use crate::builtins;
use crate::VirtualMachine;

impl VirtualMachine {
    pub(super) fn call_collection_builtin(
        &mut self,
        name: &str,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        match name {
            "list" => {
                if args.is_empty() {
                    return Ok(PyObject::list(vec![]));
                }
                let items = self.collect_iterable(&args[0])?;
                Ok(PyObject::list(items))
            }
            "tuple" => {
                if args.is_empty() {
                    return Ok(PyObject::tuple(vec![]));
                }
                if matches!(&args[0].payload, PyObjectPayload::Tuple(_)) {
                    return Ok(args[0].clone());
                }
                let items = self.collect_iterable(&args[0])?;
                Ok(PyObject::tuple(items))
            }
            "set" => {
                if args.len() > 1 {
                    return builtins::dispatch("set", &args);
                }
                if args.is_empty() {
                    return builtins::dispatch("set", &[]);
                }
                if let PyObjectPayload::Dict(items) = &args[0].payload {
                    let read = items.read();
                    let mut map = new_fx_hashkey_flatmap();
                    map.reserve(read.len());
                    for key in read.keys() {
                        map.insert(key.clone(), key.to_object());
                    }
                    return Ok(PyObject::set_from_flatmap(map));
                }
                let items = self.collect_iterable(&args[0])?;
                builtins::dispatch("set", &[PyObject::list(items)])
            }
            "frozenset" => {
                if args.len() > 1 {
                    return builtins::dispatch("frozenset", &args);
                }
                if args.is_empty() {
                    return builtins::dispatch("frozenset", &[]);
                }
                if matches!(&args[0].payload, PyObjectPayload::FrozenSet(_)) {
                    return Ok(args[0].clone());
                }
                if let PyObjectPayload::Dict(items) = &args[0].payload {
                    let read = items.read();
                    let mut map = new_fx_hashkey_map();
                    for key in read.keys() {
                        map.insert(key.clone(), key.to_object());
                    }
                    return Ok(PyObject::frozenset(map));
                }
                let items = self.collect_iterable(&args[0])?;
                builtins::dispatch("frozenset", &[PyObject::list(items)])
            }
            "dict" => {
                if args.len() > 1 {
                    return builtins::dispatch("dict", &args);
                }
                if args.is_empty() {
                    return Ok(PyObject::dict(new_fx_hashkey_map()));
                }
                if let PyObjectPayload::Dict(_) = &args[0].payload {
                    return builtins::dispatch("dict", &args);
                }
                if let PyObjectPayload::MappingProxy(src) = &args[0].payload {
                    return Ok(PyObject::dict(src.read().clone()));
                }
                if let PyObjectPayload::InstanceDict(src) = &args[0].payload {
                    let read = src.read();
                    let mut map = IndexMap::new();
                    for (k, v) in read.iter() {
                        map.insert(HashableKey::str_key(k.clone()), v.clone());
                    }
                    return Ok(PyObject::dict(map));
                }
                if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    if let Some(keys_method) = args[0].get_attr("keys") {
                        let keys_obj = self.call_object(keys_method, vec![])?;
                        let keys = self.collect_iterable(&keys_obj)?;
                        let mut map = IndexMap::new();
                        for key_obj in keys {
                            let value = args[0].get_item(&key_obj)?;
                            map.insert(key_obj.to_hashable_key()?, value);
                        }
                        return Ok(PyObject::dict(map));
                    }
                    if let Some(ref ds) = inst.dict_storage {
                        let mut map = IndexMap::new();
                        for (k, v) in ds.read().iter() {
                            map.insert(k.clone(), v.clone());
                        }
                        return Ok(PyObject::dict(map));
                    }
                    if inst.attrs.read().contains_key("__chainmap__") {
                        if let Some(items_method) = args[0].get_attr("items") {
                            let items_obj = self.call_object(items_method, vec![])?;
                            let items = self.collect_iterable(&items_obj)?;
                            let mut map = IndexMap::new();
                            for item in &items {
                                let kv = item.to_list()?;
                                if kv.len() == 2 {
                                    let key = kv[0].to_hashable_key()?;
                                    map.insert(key, kv[1].clone());
                                }
                            }
                            return Ok(PyObject::dict(map));
                        }
                    }
                }
                let items = self.collect_iterable(&args[0])?;
                builtins::dispatch("dict", &[PyObject::list(items)])
            }
            _ => unreachable!("non-collection builtin routed to collection dispatch"),
        }
    }
}
