use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args_min, make_builtin, make_module, FxHashKeyMap, PyCell, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;
use std::sync::Arc;

use super::pickle_module::{pickle_loads_stack, pickle_serialize};

pub fn create_shelve_module() -> PyObjectRef {
    let open_fn = make_builtin(|args: &[PyObjectRef]| {
        let filename = if !args.is_empty() {
            args[0].py_to_string()
        } else {
            "shelf.db".to_string()
        };
        let cls = PyObject::class(CompactString::from("Shelf"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            let data: Rc<PyCell<FxHashKeyMap>> = ferrython_core::object::alloc_map_inner();
            let file_path = Arc::new(filename.clone());

            if let Ok(bytes) = std::fs::read(&*file_path) {
                if let Ok(loaded) = pickle_loads_stack(&bytes) {
                    if let PyObjectPayload::Dict(ref dict_data) = loaded.payload {
                        let mut store = data.write();
                        for (k, v) in dict_data.read().iter() {
                            store.insert(k.clone(), v.clone());
                        }
                    }
                }
            }

            let d1 = data.clone();
            w.insert(
                CompactString::from("__getitem__"),
                PyObject::native_closure("Shelf.__getitem__", move |args: &[PyObjectRef]| {
                    check_args_min("Shelf.__getitem__", args, 1)?;
                    let key =
                        HashableKey::str_key(CompactString::from(args[0].py_to_string().as_str()));
                    d1.read()
                        .get(&key)
                        .cloned()
                        .ok_or_else(|| PyException::key_error(args[0].py_to_string()))
                }),
            );

            let d2 = data.clone();
            w.insert(
                CompactString::from("__setitem__"),
                PyObject::native_closure("Shelf.__setitem__", move |args: &[PyObjectRef]| {
                    check_args_min("Shelf.__setitem__", args, 2)?;
                    let key =
                        HashableKey::str_key(CompactString::from(args[0].py_to_string().as_str()));
                    d2.write().insert(key, args[1].clone());
                    Ok(PyObject::none())
                }),
            );

            let d2b = data.clone();
            w.insert(
                CompactString::from("__delitem__"),
                PyObject::native_closure("Shelf.__delitem__", move |args: &[PyObjectRef]| {
                    check_args_min("Shelf.__delitem__", args, 1)?;
                    let key =
                        HashableKey::str_key(CompactString::from(args[0].py_to_string().as_str()));
                    match d2b.write().swap_remove(&key) {
                        Some(_) => Ok(PyObject::none()),
                        None => Err(PyException::key_error(args[0].py_to_string())),
                    }
                }),
            );

            let d3 = data.clone();
            w.insert(
                CompactString::from("__contains__"),
                PyObject::native_closure("Shelf.__contains__", move |args: &[PyObjectRef]| {
                    check_args_min("Shelf.__contains__", args, 1)?;
                    let key =
                        HashableKey::str_key(CompactString::from(args[0].py_to_string().as_str()));
                    Ok(PyObject::bool_val(d3.read().contains_key(&key)))
                }),
            );

            let d4 = data.clone();
            w.insert(
                CompactString::from("keys"),
                PyObject::native_closure("Shelf.keys", move |_: &[PyObjectRef]| {
                    let keys: Vec<PyObjectRef> = d4
                        .read()
                        .keys()
                        .map(|k| match k {
                            HashableKey::Str(s) => PyObject::str_val(s.to_compact_string()),
                            _ => PyObject::none(),
                        })
                        .collect();
                    Ok(PyObject::list(keys))
                }),
            );

            let d4b = data.clone();
            w.insert(
                CompactString::from("values"),
                PyObject::native_closure("Shelf.values", move |_: &[PyObjectRef]| {
                    let vals: Vec<PyObjectRef> = d4b.read().values().cloned().collect();
                    Ok(PyObject::list(vals))
                }),
            );

            let d4c = data.clone();
            w.insert(
                CompactString::from("items"),
                PyObject::native_closure("Shelf.items", move |_: &[PyObjectRef]| {
                    let items: Vec<PyObjectRef> = d4c
                        .read()
                        .iter()
                        .map(|(k, v)| {
                            let key = match k {
                                HashableKey::Str(s) => PyObject::str_val(s.to_compact_string()),
                                _ => PyObject::none(),
                            };
                            PyObject::tuple(vec![key, v.clone()])
                        })
                        .collect();
                    Ok(PyObject::list(items))
                }),
            );

            let d5 = data.clone();
            w.insert(
                CompactString::from("__len__"),
                PyObject::native_closure("Shelf.__len__", move |_: &[PyObjectRef]| {
                    Ok(PyObject::int(d5.read().len() as i64))
                }),
            );

            let d6 = data.clone();
            w.insert(
                CompactString::from("get"),
                PyObject::native_closure("Shelf.get", move |args: &[PyObjectRef]| {
                    check_args_min("Shelf.get", args, 1)?;
                    let key =
                        HashableKey::str_key(CompactString::from(args[0].py_to_string().as_str()));
                    let default = args.get(1).cloned().unwrap_or_else(PyObject::none);
                    Ok(d6.read().get(&key).cloned().unwrap_or(default))
                }),
            );

            let sync_data = data.clone();
            let sync_path = file_path.clone();
            let sync_fn = move || -> PyResult<()> {
                let store = sync_data.read();
                let dict = PyObject::dict(store.clone());
                let mut buf = Vec::new();
                pickle_serialize(&dict, &mut buf)?;
                std::fs::write(&**sync_path, &buf)
                    .map_err(|e| PyException::runtime_error(format!("shelve.sync: {}", e)))?;
                Ok(())
            };
            let sf1 = sync_fn.clone();
            w.insert(
                CompactString::from("sync"),
                PyObject::native_closure("Shelf.sync", move |_: &[PyObjectRef]| {
                    sf1()?;
                    Ok(PyObject::none())
                }),
            );
            let sf2 = sync_fn.clone();
            w.insert(
                CompactString::from("close"),
                PyObject::native_closure("Shelf.close", move |_: &[PyObjectRef]| {
                    sf2()?;
                    Ok(PyObject::none())
                }),
            );

            let ir = inst.clone();
            w.insert(
                CompactString::from("__enter__"),
                PyObject::native_closure(
                    "Shelf.__enter__",
                    move |_: &[PyObjectRef]| Ok(ir.clone()),
                ),
            );
            let sf3 = sync_fn;
            w.insert(
                CompactString::from("__exit__"),
                PyObject::native_closure("Shelf.__exit__", move |_: &[PyObjectRef]| {
                    let _ = sf3();
                    Ok(PyObject::bool_val(false))
                }),
            );
        }
        Ok(inst)
    });

    make_module("shelve", vec![("open", open_fn)])
}
