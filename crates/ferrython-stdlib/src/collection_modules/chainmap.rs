//! `collections.ChainMap` implementation.

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::helpers::call_callable;
use ferrython_core::object::{
    new_fx_hashkey_map, CompareOp, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

fn chainmap_make_instance(
    class_obj: PyObjectRef,
    maps: Vec<PyObjectRef>,
    set_parents: bool,
) -> PyResult<PyObjectRef> {
    let maps = if maps.is_empty() {
        vec![PyObject::dict(IndexMap::new())]
    } else {
        maps
    };
    let inst = PyObject::instance(class_obj.clone());
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(
            CompactString::from("__chainmap__"),
            PyObject::bool_val(true),
        );
        w.insert(CompactString::from("maps"), PyObject::list(maps.clone()));
        if set_parents {
            let parent_maps = if maps.len() > 1 {
                maps[1..].to_vec()
            } else {
                Vec::new()
            };
            let parents = chainmap_make_instance(class_obj, parent_maps, false)?;
            w.insert(CompactString::from("parents"), parents);
        }
    }
    Ok(inst)
}

fn chainmap_init_instance(inst: &PyObjectRef, maps: Vec<PyObjectRef>) -> PyResult<()> {
    let class_obj = if let PyObjectPayload::Instance(d) = &inst.payload {
        d.class.clone()
    } else {
        return Err(PyException::type_error("ChainMap expects an instance"));
    };
    let maps = if maps.is_empty() {
        vec![PyObject::dict(IndexMap::new())]
    } else {
        maps
    };
    if let PyObjectPayload::Instance(d) = &inst.payload {
        let mut w = d.attrs.write();
        w.insert(
            CompactString::from("__chainmap__"),
            PyObject::bool_val(true),
        );
        w.insert(CompactString::from("maps"), PyObject::list(maps.clone()));
        let parent_maps = if maps.len() > 1 {
            maps[1..].to_vec()
        } else {
            Vec::new()
        };
        let parents = chainmap_make_instance(class_obj, parent_maps, false)?;
        w.insert(CompactString::from("parents"), parents);
    }
    Ok(())
}

pub(super) fn make_chainmap_class() -> PyObjectRef {
    let mut ns = IndexMap::new();

    let maps_from_self = |self_obj: &PyObjectRef| -> PyResult<Vec<PyObjectRef>> {
        let maps = self_obj
            .get_attr("maps")
            .ok_or_else(|| PyException::type_error("ChainMap missing maps"))?;
        maps.to_list()
    };

    let build_builtin_value = |maps: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        let mut combined = IndexMap::new();
        for mapping in maps.iter().rev() {
            for key in mapping.to_list()? {
                let hk = key.to_hashable_key()?;
                let value = mapping.get_item(&key)?;
                combined.insert(hk, value);
            }
        }
        Ok(PyObject::dict(combined))
    };

    let lookup_in_maps =
        |maps: &[PyObjectRef], key: &PyObjectRef| -> PyResult<Option<PyObjectRef>> {
            for mapping in maps {
                match mapping.get_item(key) {
                    Ok(value) => return Ok(Some(value)),
                    Err(e) if e.kind == ExceptionKind::KeyError => continue,
                    Err(e) => return Err(e),
                }
            }
            Ok(None)
        };

    let unique_keys = |maps: &[PyObjectRef]| -> PyResult<Vec<PyObjectRef>> {
        let mut seen = IndexMap::<HashableKey, ()>::new();
        let mut keys = Vec::new();
        for mapping in maps.iter().rev() {
            let list = mapping.to_list()?;
            for key in list {
                let hk = HashableKey::from_object(&key)?;
                if seen.insert(hk, ()).is_none() {
                    keys.push(key);
                }
            }
        }
        Ok(keys)
    };

    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("ChainMap.__init__", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("ChainMap.__init__ requires self"));
            }
            let maps = if call_args.len() > 1 {
                call_args[1..].to_vec()
            } else {
                vec![PyObject::dict(IndexMap::new())]
            };
            chainmap_init_instance(&call_args[0], maps)?;
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__getitem__"),
        PyObject::native_closure("ChainMap.__getitem__", move |call_args| {
            if call_args.len() < 2 {
                return Err(PyException::type_error("__getitem__ requires key"));
            }
            let maps = maps_from_self(&call_args[0])?;
            if let Some(value) = lookup_in_maps(&maps, &call_args[1])? {
                return Ok(value);
            }
            if let Some(missing) = call_args[0].get_attr("__missing__") {
                return call_callable(&missing, &[call_args[1].clone()]);
            }
            Err(PyException::key_error(call_args[1].py_to_string()))
        }),
    );
    ns.insert(
        CompactString::from("__contains__"),
        PyObject::native_closure("ChainMap.__contains__", move |call_args| {
            if call_args.len() < 2 {
                return Err(PyException::type_error("__contains__ requires key"));
            }
            let maps = maps_from_self(&call_args[0])?;
            Ok(PyObject::bool_val(
                lookup_in_maps(&maps, &call_args[1])?.is_some(),
            ))
        }),
    );
    ns.insert(
        CompactString::from("__len__"),
        PyObject::native_closure("ChainMap.__len__", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("__len__ requires self"));
            }
            let maps = maps_from_self(&call_args[0])?;
            Ok(PyObject::int(unique_keys(&maps)?.len() as i64))
        }),
    );
    ns.insert(
        CompactString::from("__iter__"),
        PyObject::native_closure("ChainMap.__iter__", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("__iter__ requires self"));
            }
            let maps = maps_from_self(&call_args[0])?;
            let combined = build_builtin_value(&maps)?;
            if let PyObjectPayload::Dict(m) = &combined.payload {
                Ok(PyObject::wrap(PyObjectPayload::DictKeys {
                    map: m.clone(),
                    owner: Some(combined.clone()),
                }))
            } else {
                Ok(PyObject::wrap(PyObjectPayload::DictKeys {
                    map: Rc::new(PyCell::new(new_fx_hashkey_map())),
                    owner: None,
                }))
            }
        }),
    );
    ns.insert(
        CompactString::from("keys"),
        PyObject::native_closure("ChainMap.keys", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("keys requires self"));
            }
            let maps = maps_from_self(&call_args[0])?;
            let combined = build_builtin_value(&maps)?;
            if let PyObjectPayload::Dict(m) = &combined.payload {
                Ok(PyObject::wrap(PyObjectPayload::DictKeys {
                    map: m.clone(),
                    owner: Some(combined.clone()),
                }))
            } else {
                Ok(PyObject::wrap(PyObjectPayload::DictKeys {
                    map: Rc::new(PyCell::new(new_fx_hashkey_map())),
                    owner: None,
                }))
            }
        }),
    );
    ns.insert(
        CompactString::from("values"),
        PyObject::native_closure("ChainMap.values", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("values requires self"));
            }
            let maps = maps_from_self(&call_args[0])?;
            let combined = build_builtin_value(&maps)?;
            if let PyObjectPayload::Dict(m) = &combined.payload {
                Ok(PyObject::wrap(PyObjectPayload::DictValues {
                    map: m.clone(),
                    owner: Some(combined.clone()),
                }))
            } else {
                Ok(PyObject::wrap(PyObjectPayload::DictValues {
                    map: Rc::new(PyCell::new(new_fx_hashkey_map())),
                    owner: None,
                }))
            }
        }),
    );
    ns.insert(
        CompactString::from("items"),
        PyObject::native_closure("ChainMap.items", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("items requires self"));
            }
            let maps = maps_from_self(&call_args[0])?;
            let combined = build_builtin_value(&maps)?;
            if let PyObjectPayload::Dict(m) = &combined.payload {
                Ok(PyObject::wrap(PyObjectPayload::DictItems {
                    map: m.clone(),
                    owner: Some(combined.clone()),
                }))
            } else {
                Ok(PyObject::wrap(PyObjectPayload::DictItems {
                    map: Rc::new(PyCell::new(new_fx_hashkey_map())),
                    owner: None,
                }))
            }
        }),
    );
    ns.insert(
        CompactString::from("get"),
        PyObject::native_closure("ChainMap.get", move |call_args| {
            if call_args.len() < 2 {
                return Err(PyException::type_error("get requires key"));
            }
            let maps = maps_from_self(&call_args[0])?;
            let default = call_args.get(2).cloned().unwrap_or_else(PyObject::none);
            Ok(lookup_in_maps(&maps, &call_args[1])?.unwrap_or(default))
        }),
    );
    ns.insert(
        CompactString::from("__eq__"),
        PyObject::native_closure("ChainMap.__eq__", move |call_args| {
            if call_args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let self_maps = maps_from_self(&call_args[0])?;
            let self_value = build_builtin_value(&self_maps)?;
            let other_value = if let Ok(other_maps) = maps_from_self(&call_args[1]) {
                build_builtin_value(&other_maps)?
            } else {
                call_args[1].clone()
            };
            self_value.compare(&other_value, CompareOp::Eq)
        }),
    );
    ns.insert(
        CompactString::from("__ne__"),
        PyObject::native_closure("ChainMap.__ne__", move |call_args| {
            if call_args.len() < 2 {
                return Ok(PyObject::bool_val(true));
            }
            let self_maps = maps_from_self(&call_args[0])?;
            let self_value = build_builtin_value(&self_maps)?;
            let other_value = if let Ok(other_maps) = maps_from_self(&call_args[1]) {
                build_builtin_value(&other_maps)?
            } else {
                call_args[1].clone()
            };
            self_value.compare(&other_value, CompareOp::Ne)
        }),
    );
    ns.insert(
        CompactString::from("__setitem__"),
        PyObject::native_closure("ChainMap.__setitem__", move |call_args| {
            if call_args.len() < 3 {
                return Err(PyException::type_error(
                    "__setitem__ requires key and value",
                ));
            }
            let key = &call_args[1];
            let value = &call_args[2];
            let hk = HashableKey::from_object(key)?;
            let maps = maps_from_self(&call_args[0])?;
            if let Some(first) = maps.first() {
                if let PyObjectPayload::Dict(dict) = &first.payload {
                    dict.write().insert(hk, value.clone());
                }
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__delitem__"),
        PyObject::native_closure("ChainMap.__delitem__", move |call_args| {
            if call_args.len() < 2 {
                return Err(PyException::type_error("__delitem__ requires key"));
            }
            let key = &call_args[1];
            let hk = HashableKey::from_object(key)?;
            let maps = maps_from_self(&call_args[0])?;
            if let Some(first) = maps.first() {
                if let PyObjectPayload::Dict(dict) = &first.payload {
                    if dict.write().shift_remove(&hk).is_none() {
                        return Err(PyException::key_error(&key.py_to_string()));
                    }
                    return Ok(PyObject::none());
                }
            }
            Err(PyException::key_error(&call_args[1].py_to_string()))
        }),
    );
    ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("ChainMap.__repr__", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("__repr__ requires self"));
            }
            let mut parts = Vec::new();
            let maps = maps_from_self(&call_args[0])?;
            for m in &maps {
                parts.push(m.py_to_string());
            }
            let class_name = if let PyObjectPayload::Instance(inst) = &call_args[0].payload {
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    cd.name.to_string()
                } else {
                    "ChainMap".to_string()
                }
            } else {
                "ChainMap".to_string()
            };
            Ok(PyObject::str_val(CompactString::from(format!(
                "{}({})",
                class_name,
                parts.join(", ")
            ))))
        }),
    );

    let copy_fn = PyObject::native_closure("ChainMap.copy", move |call_args| {
        if call_args.is_empty() {
            return Err(PyException::type_error("copy requires self"));
        }
        let maps = maps_from_self(&call_args[0])?;
        let mut new_maps = Vec::with_capacity(maps.len());
        if let Some(first) = maps.first() {
            let copied = match &first.payload {
                PyObjectPayload::Dict(dict) => PyObject::dict(dict.read().clone()),
                _ => first.clone(),
            };
            new_maps.push(copied);
            new_maps.extend(maps.iter().skip(1).cloned());
        }
        let class_obj = if let PyObjectPayload::Instance(inst) = &call_args[0].payload {
            inst.class.clone()
        } else {
            PyObject::builtin_type(CompactString::from("object"))
        };
        chainmap_make_instance(class_obj, new_maps, true)
    });
    ns.insert(CompactString::from("copy"), copy_fn.clone());
    ns.insert(CompactString::from("__copy__"), copy_fn);
    ns.insert(
        CompactString::from("new_child"),
        PyObject::native_closure("ChainMap.new_child", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("new_child requires self"));
            }
            let child_map = if call_args.len() > 1 {
                call_args[1].clone()
            } else {
                PyObject::dict(IndexMap::new())
            };
            let maps = maps_from_self(&call_args[0])?;
            let mut new_maps = vec![child_map];
            new_maps.extend(maps.into_iter());
            let class_obj = if let PyObjectPayload::Instance(inst) = &call_args[0].payload {
                inst.class.clone()
            } else {
                PyObject::builtin_type(CompactString::from("object"))
            };
            chainmap_make_instance(class_obj, new_maps, true)
        }),
    );
    ns.insert(
        CompactString::from("pop"),
        PyObject::native_closure("ChainMap.pop", move |call_args| {
            if call_args.len() < 2 {
                return Err(PyException::type_error("pop requires key"));
            }
            let self_obj = &call_args[0];
            let key = &call_args[1];
            let default = call_args.get(2).cloned();
            let maps = maps_from_self(self_obj)?;
            if let Some(first) = maps.first() {
                if let PyObjectPayload::Dict(dict) = &first.payload {
                    let hk = HashableKey::from_object(key)?;
                    if let Some(v) = dict.write().shift_remove(&hk) {
                        return Ok(v);
                    }
                }
            }
            default.ok_or_else(|| PyException::key_error(key.py_to_string()))
        }),
    );
    ns.insert(
        CompactString::from("popitem"),
        PyObject::native_closure("ChainMap.popitem", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("popitem requires self"));
            }
            let maps = maps_from_self(&call_args[0])?;
            if let Some(first) = maps.first() {
                if let PyObjectPayload::Dict(dict) = &first.payload {
                    let mut w = dict.write();
                    if let Some((k, v)) = w.iter().next().map(|(k, v)| (k.clone(), v.clone())) {
                        w.shift_remove(&k);
                        return Ok(PyObject::tuple(vec![k.to_object(), v]));
                    }
                }
            }
            Err(PyException::key_error("popitem(): dictionary is empty"))
        }),
    );
    ns.insert(
        CompactString::from("clear"),
        PyObject::native_closure("ChainMap.clear", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("clear requires self"));
            }
            let maps = maps_from_self(&call_args[0])?;
            if let Some(first) = maps.first() {
                if let PyObjectPayload::Dict(dict) = &first.payload {
                    dict.write().clear();
                }
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__reduce__"),
        PyObject::native_closure("ChainMap.__reduce__", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("__reduce__ requires self"));
            }
            let self_obj = &call_args[0];
            let maps = maps_from_self(self_obj)?;
            let class_obj = if let PyObjectPayload::Instance(inst) = &self_obj.payload {
                inst.class.clone()
            } else {
                PyObject::builtin_type(CompactString::from("object"))
            };
            Ok(PyObject::tuple(vec![class_obj, PyObject::tuple(maps)]))
        }),
    );

    PyObject::class(CompactString::from("ChainMap"), vec![], ns)
}
