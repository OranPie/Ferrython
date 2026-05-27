use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args_min, FxHashKeyMap, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    SharedFxAttrMap,
};
use ferrython_core::types::HashableKey;

pub(crate) fn call_instance_dict_method(
    attrs: &SharedFxAttrMap,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match method {
        "get" => {
            check_args_min("get", args, 1)?;
            let key_str = args[0].py_to_string();
            let default = if args.len() >= 2 {
                args[1].clone()
            } else {
                PyObject::none()
            };
            Ok(attrs
                .read()
                .get(key_str.as_str())
                .cloned()
                .unwrap_or(default))
        }
        "keys" => {
            let guard = attrs.read();
            let keys: Vec<PyObjectRef> =
                guard.keys().map(|k| PyObject::str_val(k.clone())).collect();
            Ok(PyObject::list(keys))
        }
        "values" => {
            let guard = attrs.read();
            let vals: Vec<PyObjectRef> = guard.values().cloned().collect();
            Ok(PyObject::list(vals))
        }
        "items" => {
            let guard = attrs.read();
            let items: Vec<PyObjectRef> = guard
                .iter()
                .map(|(k, v)| PyObject::tuple(vec![PyObject::str_val(k.clone()), v.clone()]))
                .collect();
            Ok(PyObject::list(items))
        }
        "__contains__" => {
            check_args_min("__contains__", args, 1)?;
            let key_str = args[0].py_to_string();
            Ok(PyObject::bool_val(
                attrs.read().contains_key(key_str.as_str()),
            ))
        }
        "pop" => {
            check_args_min("pop", args, 1)?;
            let key_str = CompactString::from(args[0].py_to_string());
            let default = if args.len() >= 2 {
                Some(args[1].clone())
            } else {
                None
            };
            match attrs.write().swap_remove(&key_str) {
                Some(v) => Ok(v),
                None => match default {
                    Some(d) => Ok(d),
                    None => Err(PyException::key_error(args[0].repr())),
                },
            }
        }
        "update" => {
            check_args_min("update", args, 1)?;
            if let PyObjectPayload::Dict(other) = &args[0].payload {
                let other_items = other.read().clone();
                let mut w = attrs.write();
                for (k, v) in other_items {
                    w.insert(CompactString::from(k.to_object().py_to_string()), v);
                }
            } else if let PyObjectPayload::InstanceDict(other) = &args[0].payload {
                let other_items: Vec<_> = other
                    .read()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let mut w = attrs.write();
                for (k, v) in other_items {
                    w.insert(k, v);
                }
            }
            Ok(PyObject::none())
        }
        "copy" => {
            let guard = attrs.read();
            let copy: FxHashKeyMap = guard
                .iter()
                .map(|(k, v)| (HashableKey::str_key(k.clone()), v.clone()))
                .collect();
            Ok(PyObject::dict(copy))
        }
        "clear" => {
            attrs.write().clear();
            Ok(PyObject::none())
        }
        "setdefault" => {
            check_args_min("setdefault", args, 1)?;
            let key = CompactString::from(args[0].py_to_string());
            let default = if args.len() >= 2 {
                args[1].clone()
            } else {
                PyObject::none()
            };
            let mut w = attrs.write();
            if let Some(v) = w.get(key.as_str()) {
                Ok(v.clone())
            } else {
                w.insert(key, default.clone());
                Ok(default)
            }
        }
        _ => Err(PyException::attribute_error(format!(
            "'dict' object has no attribute '{}'",
            method
        ))),
    }
}
