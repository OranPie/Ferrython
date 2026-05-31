use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::helpers::{
    instance_dict_as_hashkey_map, instance_dict_get_item, instance_dict_remove_item,
    instance_dict_set_item,
};
use ferrython_core::object::{
    check_args_min, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, SharedFxAttrMap,
};

pub(crate) fn call_instance_dict_method(
    attrs: &SharedFxAttrMap,
    method: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    match method {
        "get" => {
            check_args_min("get", args, 1)?;
            let default = if args.len() >= 2 {
                args[1].clone()
            } else {
                PyObject::none()
            };
            Ok(instance_dict_get_item(attrs, &args[0])?.unwrap_or(default))
        }
        "keys" => {
            let map = instance_dict_as_hashkey_map(attrs);
            let keys: Vec<PyObjectRef> = map.keys().map(|k| k.to_object()).collect();
            Ok(PyObject::list(keys))
        }
        "values" => {
            let map = instance_dict_as_hashkey_map(attrs);
            let vals: Vec<PyObjectRef> = map.values().cloned().collect();
            Ok(PyObject::list(vals))
        }
        "items" => {
            let map = instance_dict_as_hashkey_map(attrs);
            let items: Vec<PyObjectRef> = map
                .iter()
                .map(|(k, v)| PyObject::tuple(vec![k.to_object(), v.clone()]))
                .collect();
            Ok(PyObject::list(items))
        }
        "__contains__" => {
            check_args_min("__contains__", args, 1)?;
            Ok(PyObject::bool_val(
                instance_dict_get_item(attrs, &args[0])?.is_some(),
            ))
        }
        "pop" => {
            check_args_min("pop", args, 1)?;
            let default = if args.len() >= 2 {
                Some(args[1].clone())
            } else {
                None
            };
            match instance_dict_remove_item(attrs, &args[0])? {
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
                for (k, v) in other_items {
                    instance_dict_set_item(attrs, &k.to_object(), v)?;
                }
            } else if let PyObjectPayload::InstanceDict(other) = &args[0].payload {
                let other_items = instance_dict_as_hashkey_map(other);
                for (k, v) in other_items {
                    instance_dict_set_item(attrs, &k.to_object(), v)?;
                }
            }
            Ok(PyObject::none())
        }
        "copy" => Ok(PyObject::dict(instance_dict_as_hashkey_map(attrs))),
        "clear" => {
            attrs.write().clear();
            Ok(PyObject::none())
        }
        "setdefault" => {
            check_args_min("setdefault", args, 1)?;
            let default = if args.len() >= 2 {
                args[1].clone()
            } else {
                PyObject::none()
            };
            if let Some(v) = instance_dict_get_item(attrs, &args[0])? {
                Ok(v.clone())
            } else {
                instance_dict_set_item(attrs, &args[0], default.clone())?;
                Ok(default)
            }
        }
        _ => Err(PyException::attribute_error(format!(
            "'dict' object has no attribute '{}'",
            method
        ))),
    }
}
