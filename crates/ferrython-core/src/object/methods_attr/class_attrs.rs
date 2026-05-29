use crate::error::PyException;
use crate::intern::intern_or_new;
use compact_str::CompactString;
use std::rc::Rc;

use super::*;
use crate::object::ClassData;

pub(super) fn class_attr(obj: &PyObjectRef, cd: &ClassData, name: &str) -> Option<PyObjectRef> {
    // Special class attributes
    if name == "__class__" {
        // In CPython, a class's __class__ is its metaclass (usually 'type')
        if let Some(meta) = &cd.metaclass {
            return Some(meta.clone());
        }
        return Some(PyObject::builtin_type(CompactString::from("type")));
    }
    if name == "__name__" {
        return Some(PyObject::str_val(cd.name.clone()));
    }
    if name == "__bases__" {
        return Some(PyObject::tuple(cd.bases.clone()));
    }
    if name == "__mro__" {
        let mut mro_list = vec![obj.clone()];
        mro_list.extend(cd.mro.iter().cloned());
        // Append 'object' as the universal base (like CPython)
        mro_list.push(PyObject::builtin_type(CompactString::from("object")));
        return Some(PyObject::tuple(mro_list));
    }
    if name == "__dict__" {
        let ns = cd.namespace.read();
        let mut map = new_fx_hashkey_map();
        for (k, v) in ns.iter() {
            if let Ok(hk) = PyObject::str_val(k.clone()).to_hashable_key() {
                map.insert(hk, v.clone());
            }
        }
        return Some(PyObject::wrap(PyObjectPayload::MappingProxy(Rc::new(
            PyCell::new(map),
        ))));
    }
    if name == "__module__" {
        // Check namespace first for explicitly set __module__
        if let Some(v) = cd.namespace.read().get("__module__") {
            return Some(v.clone());
        }
        return Some(PyObject::str_val(intern_or_new("__main__")));
    }
    if name == "__qualname__" {
        // Check namespace first
        if let Some(v) = cd.namespace.read().get("__qualname__") {
            return Some(v.clone());
        }
        return Some(PyObject::str_val(cd.name.clone()));
    }
    if name == "__subclasses__" {
        let subs = cd.subclasses.clone();
        return Some(PyObject::native_closure("__subclasses__", move |_args| {
            let refs = subs.read();
            let alive: Vec<PyObjectRef> = refs.iter().filter_map(|w| w.upgrade()).collect();
            Ok(PyObject::list(alive))
        }));
    }
    if name == "mro" {
        let self_cls = obj.clone();
        let mro_data = cd.mro.clone();
        return Some(PyObject::native_closure("mro", move |_args| {
            let mut mro_list = vec![self_cls.clone()];
            mro_list.extend(mro_data.iter().cloned());
            Ok(PyObject::list(mro_list))
        }));
    }
    if name == "fromkeys" && class_has_builtin_dict_base(cd) {
        if let Some(method) = resolve_builtin_type_method("dict", "fromkeys") {
            return Some(PyObjectRef::new(PyObject {
                payload: PyObjectPayload::BoundMethod {
                    receiver: obj.clone(),
                    method,
                },
            }));
        }
    }
    // Check own namespace first, then bases
    if let Some(v) = cd.namespace.read().get(name).cloned() {
        match &v.payload {
            PyObjectPayload::StaticMethod(func) => return Some(func.clone()),
            PyObjectPayload::ClassMethod(func) => {
                return Some(PyObjectRef::new(PyObject {
                    payload: PyObjectPayload::BoundMethod {
                        receiver: obj.clone(),
                        method: func.clone(),
                    },
                }));
            }
            _ => return Some(v),
        }
    }
    // Walk the computed MRO (C3 linearization) for correct diamond resolution
    let mro_chain: &[PyObjectRef] = if !cd.mro.is_empty() {
        &cd.mro
    } else {
        &cd.bases
    };
    for base in mro_chain {
        if let PyObjectPayload::Class(bcd) = &base.payload {
            if let Some(v) = bcd.namespace.read().get(name).cloned() {
                match &v.payload {
                    PyObjectPayload::StaticMethod(func) => return Some(func.clone()),
                    PyObjectPayload::ClassMethod(func) => {
                        return Some(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: obj.clone(),
                                method: func.clone(),
                            },
                        }));
                    }
                    PyObjectPayload::NativeFunction(_)
                        if name == "fromkeys" && bcd.name.as_str() == "dict" =>
                    {
                        return Some(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: obj.clone(),
                                method: v,
                            },
                        }));
                    }
                    _ => return Some(v),
                }
            }
            // If base has its own bases/MRO, recurse (for Rust-created classes with empty MRO)
            if bcd.mro.is_empty() && !bcd.bases.is_empty() {
                if let Some(v) = py_get_attr(base, name) {
                    return Some(v);
                }
            }
        } else if let PyObjectPayload::BuiltinType(base_name) = &base.payload {
            if let Some(v) = base.get_attr(name) {
                if name == "fromkeys" && base_name.as_str() == "dict" {
                    return Some(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BoundMethod {
                            receiver: obj.clone(),
                            method: v,
                        },
                    }));
                }
                return Some(v);
            }
        } else if let Some(v) = base.get_attr(name) {
            return Some(v);
        }
    }
    // If class has a metaclass, look in metaclass namespace too
    // (e.g., cls._instances where _instances is a metaclass class attribute)
    // But skip __new__/__init__ — those are type-level constructors,
    // not methods on instances of the metaclass.
    if let Some(meta) = &cd.metaclass {
        if name != "__new__" && name != "__init__" {
            if let PyObjectPayload::Class(mcd) = &meta.payload {
                if let Some(v) = mcd.namespace.read().get(name).cloned() {
                    return Some(v);
                }
            }
        }
    }
    // Fallback: synthesize object-level attributes for user classes
    if name == "__new__" {
        return Some(PyObject::native_function("__new__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("__new__ requires cls"));
            }
            Ok(PyObject::instance(args[0].clone()))
        }));
    }
    if name == "__init_subclass__" {
        return Some(PyObject::native_function("__init_subclass__", |_args| {
            Ok(PyObject::none())
        }));
    }
    // Fallback: synthesize object-level dunder methods that all classes inherit
    if name == "__setattr__" {
        return Some(PyObject::native_function("__setattr__", |args| {
            if args.len() < 3 {
                return Err(PyException::type_error("__setattr__ requires 3 arguments"));
            }
            let attr_name = args[1].py_to_string();
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                inst.attrs
                    .write()
                    .insert(CompactString::from(attr_name.as_str()), args[2].clone());
            }
            Ok(PyObject::none())
        }));
    }
    if name == "__delattr__" {
        return Some(PyObject::native_function("__delattr__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("__delattr__ requires 2 arguments"));
            }
            let attr_name = args[1].py_to_string();
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                inst.attrs.write().shift_remove(attr_name.as_str());
            }
            Ok(PyObject::none())
        }));
    }
    if name == "__getattribute__" {
        return Some(PyObject::native_function("__getattribute__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error(
                    "__getattribute__ requires 2 arguments",
                ));
            }
            let attr_name = args[1].py_to_string();
            args[0].get_attr(&attr_name).ok_or_else(|| {
                PyException::attribute_error(format!(
                    "'{}' object has no attribute '{}'",
                    args[0].type_name(),
                    attr_name
                ))
            })
        }));
    }
    // Note: Do NOT add __init__ fallback here — it breaks
    // dataclass auto-init detection (which checks cls.get_attr("__init__")).
    if name == "__repr__" {
        let cls_name = cd.name.clone();
        return Some(PyObject::native_closure("__repr__", move |args| {
            if args.is_empty() {
                return Ok(PyObject::str_val(CompactString::from(format!(
                    "<class '{}'>",
                    cls_name
                ))));
            }
            let addr = PyObjectRef::as_ptr(&args[0]) as usize;
            Ok(PyObject::str_val(CompactString::from(format!(
                "<{} object at 0x{:x}>",
                cls_name, addr
            ))))
        }));
    }
    if name == "__hash__" {
        return Some(PyObject::native_function("__hash__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("__hash__ requires 1 argument"));
            }
            Ok(PyObject::int(PyObjectRef::as_ptr(&args[0]) as i64))
        }));
    }
    if name == "__eq__" {
        return Some(PyObject::native_function("__eq__", |args| {
            if args.len() < 2 {
                return Ok(PyObject::not_implemented());
            }
            if PyObjectRef::ptr_eq(&args[0], &args[1]) {
                Ok(PyObject::bool_val(true))
            } else {
                Ok(PyObject::not_implemented())
            }
        }));
    }
    if name == "__ne__" {
        return Some(PyObject::native_function("__ne__", |args| {
            if args.len() < 2 {
                return Ok(PyObject::not_implemented());
            }
            if PyObjectRef::ptr_eq(&args[0], &args[1]) {
                Ok(PyObject::bool_val(false))
            } else {
                Ok(PyObject::not_implemented())
            }
        }));
    }
    None
}

fn class_has_builtin_dict_base(cd: &ClassData) -> bool {
    if cd
        .builtin_base_name
        .as_ref()
        .is_some_and(|base| base.as_str() == "dict")
    {
        return true;
    }
    cd.bases.iter().any(|base| match &base.payload {
        PyObjectPayload::BuiltinType(name) => name.as_str() == "dict",
        PyObjectPayload::Class(base_cd) => {
            base_cd.name.as_str() == "dict" || class_has_builtin_dict_base(base_cd)
        }
        _ => false,
    })
}
