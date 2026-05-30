use crate::intern::intern_or_new;
use crate::object::payload::NativeFunctionData;
use crate::types::{HashableKey, PyFunction};
use compact_str::CompactString;
use std::rc::Rc;

use super::*;

pub(super) fn function_attr(obj: &PyObjectRef, f: &PyFunction, name: &str) -> Option<PyObjectRef> {
    // Check user-set attrs first (allows overriding __name__ etc.)
    if let Some(v) = f.attrs.read().get(name).cloned() {
        return Some(v);
    }
    match name {
        "__name__" => {
            let value = PyObject::str_val(f.name.clone());
            f.attrs
                .write()
                .insert(CompactString::from("__name__"), value.clone());
            Some(value)
        }
        "__qualname__" => {
            let value = PyObject::str_val(f.qualname.clone());
            f.attrs
                .write()
                .insert(CompactString::from("__qualname__"), value.clone());
            Some(value)
        }
        "__class__" => Some(PyObject::builtin_type(CompactString::from("function"))),
        "__defaults__" => {
            if f.defaults.is_empty() {
                Some(PyObject::none())
            } else {
                Some(PyObject::tuple(f.defaults.clone()))
            }
        }
        "__module__" => {
            let value = PyObject::str_val(intern_or_new("__main__"));
            f.attrs
                .write()
                .insert(CompactString::from("__module__"), value.clone());
            Some(value)
        }
        "__doc__" => {
            // Check attrs first (set by functools.wraps etc.)
            if let Some(doc) = f.attrs.read().get("__doc__").cloned() {
                return Some(doc);
            }
            let value = if let Some(s) = &f.code.docstring {
                PyObject::str_val(s.clone())
            } else {
                PyObject::none()
            };
            f.attrs
                .write()
                .insert(CompactString::from("__doc__"), value.clone());
            Some(value)
        }
        "__dict__" => Some(PyObject::wrap(PyObjectPayload::InstanceDict(
            f.attrs.clone(),
        ))),
        "__annotations__" => {
            let mut map = new_fx_hashkey_map();
            for (k, v) in &f.annotations {
                if let Ok(hk) = PyObject::str_val(k.clone()).to_hashable_key() {
                    map.insert(hk, v.clone());
                }
            }
            let value = PyObject::dict(map);
            f.attrs
                .write()
                .insert(CompactString::from("__annotations__"), value.clone());
            Some(value)
        }
        "__closure__" => {
            if f.closure.is_empty() {
                Some(PyObject::none())
            } else {
                let cells: Vec<PyObjectRef> = f
                    .closure
                    .iter()
                    .map(|cell| PyObject::cell(cell.clone()))
                    .collect();
                Some(PyObject::tuple(cells))
            }
        }
        "__code__" => Some(PyObject::wrap(PyObjectPayload::Code(Rc::clone(&f.code)))),
        "__kwdefaults__" => {
            if f.kw_defaults.is_empty() {
                Some(PyObject::none())
            } else {
                let mut map = new_fx_hashkey_map();
                for (k, v) in &f.kw_defaults {
                    if let Ok(hk) = PyObject::str_val(k.clone()).to_hashable_key() {
                        map.insert(hk, v.clone());
                    }
                }
                Some(PyObject::dict(map))
            }
        }
        "__globals__" => {
            let g = f.globals.read();
            let mut map: FxHashKeyMap = new_fx_hashkey_map();
            for (k, v) in g.iter() {
                if let Ok(hk) = PyObject::str_val(k.clone()).to_hashable_key() {
                    map.insert(hk, v.clone());
                }
            }
            Some(PyObject::dict(map))
        }
        "__get__" => {
            let func = obj.clone();
            Some(PyObject::native_closure("__get__", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "__get__ requires at least 1 argument",
                    ));
                }
                let instance = &args[0];
                if matches!(&instance.payload, PyObjectPayload::None) {
                    return Ok(func.clone());
                }
                Ok(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BoundMethod {
                        receiver: instance.clone(),
                        method: func.clone(),
                    },
                }))
            }))
        }
        _ => None,
    }
}

pub(super) fn native_function_attr(
    obj: &PyObjectRef,
    nf: &NativeFunctionData,
    name: &str,
) -> Option<PyObjectRef> {
    if nf.name.as_str() == "csv.DictWriter"
        && matches!(name, "writeheader" | "writerow" | "writerows")
    {
        let method_name = CompactString::from(name);
        let closure_name = CompactString::from(format!("csv.DictWriter.{}", name));
        return Some(PyObject::native_closure(
            closure_name.as_str(),
            move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error(format!(
                        "descriptor '{}' for 'csv.DictWriter' objects needs an argument",
                        method_name
                    )));
                }
                let method = args[0].get_attr(method_name.as_str()).ok_or_else(|| {
                    PyException::attribute_error(format!(
                        "'{}' object has no attribute '{}'",
                        args[0].type_name(),
                        method_name
                    ))
                })?;
                call_callable(&method, &args[1..])
            },
        ));
    }
    match name {
        "__name__" => {
            let name = nf.name.as_str();
            if let Some(func_name) = name
                .strip_prefix("heapq.")
                .or_else(|| name.strip_prefix("_heapq."))
            {
                Some(PyObject::str_val(CompactString::from(func_name)))
            } else {
                Some(PyObject::str_val(CompactString::from(name)))
            }
        }
        "__qualname__" => {
            let name = nf.name.as_str();
            if let Some(func_name) = name
                .strip_prefix("heapq.")
                .or_else(|| name.strip_prefix("_heapq."))
            {
                Some(PyObject::str_val(CompactString::from(func_name)))
            } else {
                Some(PyObject::str_val(CompactString::from(name)))
            }
        }
        "__module__" => {
            let name = nf.name.as_str();
            if name.starts_with("heapq.") {
                Some(PyObject::str_val(CompactString::from("heapq")))
            } else if name.starts_with("_heapq.") {
                Some(PyObject::str_val(CompactString::from("_heapq")))
            } else if matches!(name, "WeakValueDictionary" | "WeakKeyDictionary") {
                Some(PyObject::str_val(CompactString::from("weakref")))
            } else {
                Some(PyObject::str_val(CompactString::from("builtins")))
            }
        }
        "__class__" => Some(PyObject::builtin_type(CompactString::from(
            "builtin_function_or_method",
        ))),
        "__doc__" => Some(PyObject::none()),
        "__call__" => Some(obj.clone()),
        "__init__" if nf.name.as_str() == "collections.deque" => Some(PyObject::native_function(
            "collections.deque.__init__",
            |args| {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "descriptor '__init__' for 'collections.deque' objects needs an argument",
                    ));
                }
                if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    let mut positional_end = args.len();
                    let mut kw_maxlen = None;
                    if let Some(last) = args.last() {
                        if let PyObjectPayload::Dict(map) = &last.payload {
                            if let Some(value) = map
                                .read()
                                .get(&HashableKey::str_key(CompactString::from("maxlen")))
                                .cloned()
                            {
                                kw_maxlen = Some(value);
                                positional_end = positional_end.saturating_sub(1);
                            }
                        }
                    }
                    if positional_end > 3 {
                        return Err(PyException::type_error(
                            "deque() takes at most 2 positional arguments",
                        ));
                    }
                    let maxlen_obj = if positional_end >= 3 {
                        Some(args[2].clone())
                    } else {
                        kw_maxlen
                    };
                    let maxlen = if let Some(value) = maxlen_obj {
                        if matches!(&value.payload, PyObjectPayload::None) {
                            None
                        } else {
                            let raw = value.to_int()?;
                            if raw < 0 {
                                return Err(PyException::value_error(
                                    "maxlen must be non-negative",
                                ));
                            }
                            Some(raw as usize)
                        }
                    } else {
                        None
                    };
                    let mut items = if positional_end < 2
                        || matches!(&args[1].payload, PyObjectPayload::None)
                    {
                        Vec::new()
                    } else {
                        if let PyObjectPayload::Instance(_) = &args[1].payload {
                            if args[1].get_attr("__iter__").is_some()
                                && args[1].get_attr("__next__").is_none()
                                && args[1].get_attr("__getitem__").is_none()
                            {
                                return Err(PyException::type_error(format!(
                                    "'{}' object is not iterable",
                                    args[1].type_name()
                                )));
                            }
                        }
                        args[1].to_list()?
                    };
                    if let Some(ml) = maxlen {
                        if items.len() > ml {
                            items = items[items.len() - ml..].to_vec();
                        }
                    }
                    let mut attrs = inst.attrs.write();
                    attrs.insert(CompactString::from("__deque__"), PyObject::bool_val(true));
                    attrs.insert(CompactString::from("_data"), PyObject::list(items));
                    attrs.insert(
                        CompactString::from("__maxlen__"),
                        maxlen
                            .map(|n| PyObject::int(n as i64))
                            .unwrap_or_else(PyObject::none),
                    );
                    Ok(PyObject::none())
                } else {
                    Err(PyException::type_error(format!(
                        "descriptor '__init__' for 'collections.deque' objects does not apply to '{}'",
                        args[0].type_name()
                    )))
                }
            },
        )),
        "__get__" => {
            let func_obj = obj.clone();
            Some(PyObject::native_closure("__get__", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "__get__ requires at least 1 argument",
                    ));
                }
                let instance = &args[0];
                if matches!(&instance.payload, PyObjectPayload::None) {
                    return Ok(func_obj.clone());
                }
                Ok(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BoundMethod {
                        receiver: instance.clone(),
                        method: func_obj.clone(),
                    },
                }))
            }))
        }
        _ => weakdict_class_attr(nf.name.as_str(), name),
    }
}

pub(super) fn builtin_function_attr(
    obj: &PyObjectRef,
    fname: &CompactString,
    name: &str,
) -> Option<PyObjectRef> {
    match name {
        "__name__" | "__qualname__" => Some(PyObject::str_val(fname.clone())),
        "__module__" => Some(PyObject::str_val(CompactString::from("builtins"))),
        "__class__" => Some(PyObject::builtin_type(CompactString::from(
            "builtin_function_or_method",
        ))),
        "__doc__" => Some(PyObject::none()),
        "__call__" => Some(obj.clone()),
        _ => None,
    }
}

pub(super) fn classmethod_attr(func: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    match name {
        "__class__" => Some(PyObject::builtin_type(CompactString::from("classmethod"))),
        "__func__" => Some(func.clone()),
        "__wrapped__" => Some(func.clone()),
        "__get__" => {
            let func = func.clone();
            Some(PyObject::native_closure("__get__", move |args| {
                if args.len() < 2 {
                    return Err(PyException::type_error("__get__ requires 2 arguments"));
                }
                let owner = &args[1];
                Ok(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BoundMethod {
                        receiver: owner.clone(),
                        method: func.clone(),
                    },
                }))
            }))
        }
        _ => None,
    }
}

pub(super) fn staticmethod_attr(func: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    match name {
        "__class__" => Some(PyObject::builtin_type(CompactString::from("staticmethod"))),
        "__func__" => Some(func.clone()),
        "__wrapped__" => Some(func.clone()),
        "__get__" => {
            let func = func.clone();
            Some(PyObject::native_closure("__get__", move |_args| {
                Ok(func.clone())
            }))
        }
        _ => func.get_attr(name),
    }
}

pub(super) fn bound_method_attr(
    receiver: &PyObjectRef,
    method: &PyObjectRef,
    name: &str,
) -> Option<PyObjectRef> {
    match name {
        "__self__" => Some(receiver.clone()),
        "__func__" => Some(method.clone()),
        _ => method.get_attr(name),
    }
}
