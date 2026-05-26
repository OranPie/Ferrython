use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    check_args_min, make_builtin, make_module, new_fx_hashkey_map, FxHashKeyMap, PyCell, PyObject,
    PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;
use std::sync::Arc;

/// Simple key-value database stub with a small length-prefixed disk format.
pub fn create_dbm_module() -> PyObjectRef {
    let open_fn = make_builtin(|args: &[PyObjectRef]| {
        let filename = if !args.is_empty() {
            args[0].py_to_string().to_string()
        } else {
            "db".to_string()
        };
        let flag = if args.len() >= 2 {
            args[1].py_to_string().to_string()
        } else {
            "r".to_string()
        };

        let db_path = if filename.contains('.') {
            filename.clone()
        } else {
            format!("{}.db", filename)
        };

        let mut initial_data = new_fx_hashkey_map();
        if flag != "n" {
            if let Ok(content) = std::fs::read(&db_path) {
                let mut pos = 0;
                while pos + 4 <= content.len() {
                    let kl = u32::from_le_bytes([
                        content[pos],
                        content[pos + 1],
                        content[pos + 2],
                        content[pos + 3],
                    ]) as usize;
                    pos += 4;
                    if pos + kl > content.len() {
                        break;
                    }
                    let key = String::from_utf8_lossy(&content[pos..pos + kl]).to_string();
                    pos += kl;
                    if pos + 4 > content.len() {
                        break;
                    }
                    let vl = u32::from_le_bytes([
                        content[pos],
                        content[pos + 1],
                        content[pos + 2],
                        content[pos + 3],
                    ]) as usize;
                    pos += 4;
                    if pos + vl > content.len() {
                        break;
                    }
                    let val = content[pos..pos + vl].to_vec();
                    pos += vl;
                    initial_data.insert(
                        HashableKey::str_key(CompactString::from(key.as_str())),
                        PyObject::bytes(val),
                    );
                }
            } else if flag == "r" {
                return Err(PyException::os_error(format!(
                    "No such file: '{}'",
                    db_path
                )));
            }
        }

        let data: Rc<PyCell<FxHashKeyMap>> = Rc::new(PyCell::new(initial_data));
        let path_for_sync = Arc::new(db_path.clone());

        let cls = PyObject::class(CompactString::from("_Database"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(
                CompactString::from("_path"),
                PyObject::str_val(CompactString::from(db_path.as_str())),
            );

            let d1 = data.clone();
            w.insert(
                CompactString::from("__getitem__"),
                PyObject::native_closure("dbm.__getitem__", move |args: &[PyObjectRef]| {
                    check_args_min("dbm.__getitem__", args, 1)?;
                    let key =
                        HashableKey::str_key(CompactString::from(args[0].py_to_string().as_str()));
                    d1.read()
                        .get(&key)
                        .cloned()
                        .ok_or_else(|| PyException::key_error(args[0].py_to_string()))
                }),
            );
            let d2 = data.clone();
            let p2 = path_for_sync.clone();
            w.insert(
                CompactString::from("__setitem__"),
                PyObject::native_closure("dbm.__setitem__", move |args: &[PyObjectRef]| {
                    check_args_min("dbm.__setitem__", args, 2)?;
                    let key_str = args[0].py_to_string();
                    let key = HashableKey::str_key(CompactString::from(key_str.as_str()));
                    let val = match &args[1].payload {
                        PyObjectPayload::Bytes(b) => PyObject::bytes((**b).clone()),
                        _ => PyObject::bytes(args[1].py_to_string().as_bytes().to_vec()),
                    };
                    d2.write().insert(key, val);
                    sync_dbm_to_disk(&d2, &p2);
                    Ok(PyObject::none())
                }),
            );
            let d3 = data.clone();
            w.insert(
                CompactString::from("__contains__"),
                PyObject::native_closure("dbm.__contains__", move |args: &[PyObjectRef]| {
                    check_args_min("dbm.__contains__", args, 1)?;
                    let key =
                        HashableKey::str_key(CompactString::from(args[0].py_to_string().as_str()));
                    Ok(PyObject::bool_val(d3.read().contains_key(&key)))
                }),
            );
            let d4 = data.clone();
            w.insert(
                CompactString::from("keys"),
                PyObject::native_closure("dbm.keys", move |_args: &[PyObjectRef]| {
                    let keys: Vec<PyObjectRef> = d4
                        .read()
                        .keys()
                        .map(|k| match k {
                            HashableKey::Str(s) => PyObject::str_val(s.to_compact_string()),
                            _ => PyObject::str_val(CompactString::from(format!("{:?}", k))),
                        })
                        .collect();
                    Ok(PyObject::list(keys))
                }),
            );
            let d5 = data.clone();
            w.insert(
                CompactString::from("values"),
                PyObject::native_closure("dbm.values", move |_args: &[PyObjectRef]| {
                    let vals: Vec<PyObjectRef> = d5.read().values().cloned().collect();
                    Ok(PyObject::list(vals))
                }),
            );
            let d6 = data.clone();
            w.insert(
                CompactString::from("__len__"),
                PyObject::native_closure("dbm.__len__", move |_args: &[PyObjectRef]| {
                    Ok(PyObject::int(d6.read().len() as i64))
                }),
            );
            let d7 = data.clone();
            let p7 = path_for_sync.clone();
            w.insert(
                CompactString::from("__delitem__"),
                PyObject::native_closure("dbm.__delitem__", move |args: &[PyObjectRef]| {
                    check_args_min("dbm.__delitem__", args, 1)?;
                    let key =
                        HashableKey::str_key(CompactString::from(args[0].py_to_string().as_str()));
                    if d7.write().shift_remove(&key).is_none() {
                        return Err(PyException::key_error(args[0].py_to_string()));
                    }
                    sync_dbm_to_disk(&d7, &p7);
                    Ok(PyObject::none())
                }),
            );
            let d8 = data.clone();
            let p8 = path_for_sync.clone();
            w.insert(
                CompactString::from("sync"),
                PyObject::native_closure("dbm.sync", move |_args: &[PyObjectRef]| {
                    sync_dbm_to_disk(&d8, &p8);
                    Ok(PyObject::none())
                }),
            );
            let d9 = data.clone();
            let p9 = path_for_sync.clone();
            w.insert(
                CompactString::from("close"),
                PyObject::native_closure("dbm.close", move |_args: &[PyObjectRef]| {
                    sync_dbm_to_disk(&d9, &p9);
                    Ok(PyObject::none())
                }),
            );
            w.insert(
                CompactString::from("__enter__"),
                make_builtin(|args: &[PyObjectRef]| {
                    check_args_min("dbm.__enter__", args, 1)?;
                    Ok(args[0].clone())
                }),
            );
            let d10 = data.clone();
            let p10 = path_for_sync.clone();
            w.insert(
                CompactString::from("__exit__"),
                PyObject::native_closure("dbm.__exit__", move |_args: &[PyObjectRef]| {
                    sync_dbm_to_disk(&d10, &p10);
                    Ok(PyObject::bool_val(false))
                }),
            );
        }
        Ok(inst)
    });

    make_module(
        "dbm",
        vec![
            ("open", open_fn),
            ("error", PyObject::str_val(CompactString::from("dbm.error"))),
        ],
    )
}

fn sync_dbm_to_disk(data: &Rc<PyCell<FxHashKeyMap>>, path: &str) {
    let guard = data.read();
    let mut buf = Vec::new();
    for (k, v) in guard.iter() {
        let key_bytes = match k {
            HashableKey::Str(s) => s.as_bytes().to_vec(),
            _ => format!("{:?}", k).into_bytes(),
        };
        let val_bytes = match &v.payload {
            PyObjectPayload::Bytes(b) => (**b).clone(),
            _ => v.py_to_string().as_bytes().to_vec(),
        };
        buf.extend_from_slice(&(key_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(&key_bytes);
        buf.extend_from_slice(&(val_bytes.len() as u32).to_le_bytes());
        buf.extend_from_slice(&val_bytes);
    }
    let _ = std::fs::write(path, &buf);
}
