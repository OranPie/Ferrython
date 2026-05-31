use crate::error::PyException;
use compact_str::CompactString;

use super::*;

pub(super) fn super_attr(
    obj: &PyObjectRef,
    cls: &PyObjectRef,
    instance: &PyObjectRef,
    name: &str,
) -> Option<PyObjectRef> {
    // super().__class__ → the 'super' type itself
    if name == "__class__" {
        return Some(PyObject::builtin_type(CompactString::from("super")));
    }
    // super().__getattribute__(name) → behaves like object.__getattribute__(self, name)
    // Must check both MRO (from parent) AND instance __dict__ to match CPython.
    if name == "__getattribute__" {
        let super_obj = obj.clone();
        let inst_ref = instance.clone();
        return Some(PyObjectRef::new(PyObject {
            payload: PyObjectPayload::NativeClosure(Box::new(NativeClosureData {
                name: CompactString::from("super.__getattribute__"),
                pickle_args: None,
                func: std::rc::Rc::new(move |args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "__getattribute__() requires at least 1 argument",
                        ));
                    }
                    let attr_name = args[0].py_to_string();
                    // First try MRO lookup through the super proxy
                    if let Some(v) = super_obj.get_attr(&attr_name) {
                        return Ok(v);
                    }
                    // Fall back to instance __dict__ (like object.__getattribute__)
                    if let PyObjectPayload::Instance(inst) = &inst_ref.payload {
                        if let Some(v) = inst.attrs.read().get(attr_name.as_str()) {
                            return Ok(v.clone());
                        }
                    }
                    Err(PyException::attribute_error(format!(
                        "'super' object has no attribute '{}'",
                        attr_name
                    )))
                }),
            })),
        }));
    }
    // super() proxy: look up in the RUNTIME class MRO, skipping up to and including cls
    let runtime_cls = match &instance.payload {
        PyObjectPayload::Instance(inst) => Some(inst.class.clone()),
        PyObjectPayload::Class(cd) => {
            // For metaclass methods: walk metaclass MRO, not class MRO
            if let Some(meta) = &cd.metaclass {
                Some(meta.clone())
            } else {
                Some(instance.clone())
            }
        }
        _ => None,
    };
    if let Some(rt_cls) = runtime_cls {
        if let PyObjectPayload::Class(cd) = &rt_cls.payload {
            let mro = &cd.mro;
            // If cls IS the runtime class itself, start from index 0.
            // Otherwise, skip entries up to and including cls in the MRO.
            let cls_is_self = PyObjectRef::ptr_eq(cls, &rt_cls);
            let mut found_cls = cls_is_self;
            for base in mro {
                if !found_cls {
                    if PyObjectRef::ptr_eq(base, cls) {
                        found_cls = true;
                    }
                    continue;
                }
                // Look in this base's namespace directly
                if let PyObjectPayload::Class(bcd) = &base.payload {
                    if let Some(v) = bcd.namespace.read().get(name) {
                        if matches!(
                            &v.payload,
                            PyObjectPayload::Function(_)
                                | PyObjectPayload::NativeClosure(_)
                                | PyObjectPayload::NativeFunction(_)
                        ) {
                            return Some(PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: instance.clone(),
                                    method: v.clone(),
                                },
                            }));
                        }
                        // Unwrap descriptors: ClassMethod → bind to class,
                        // StaticMethod → return raw function
                        if let PyObjectPayload::ClassMethod(func) = &v.payload {
                            let bound_cls = match &instance.payload {
                                PyObjectPayload::Instance(inst) => inst.class.clone(),
                                _ => instance.clone(),
                            };
                            return Some(PyObjectRef::new(PyObject {
                                payload: PyObjectPayload::BoundMethod {
                                    receiver: bound_cls,
                                    method: func.clone(),
                                },
                            }));
                        }
                        if let PyObjectPayload::StaticMethod(func) = &v.payload {
                            return Some(func.clone());
                        }
                        return Some(v.clone());
                    }
                }
                // ExceptionType base: provide synthetic __init__/__str__
                if matches!(&base.payload, PyObjectPayload::ExceptionType(_)) {
                    if let Some(resolved) =
                        exception_attrs::resolve_exception_type_method(name, instance)
                    {
                        // Bind to instance so obj is prepended
                        return Some(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: instance.clone(),
                                method: resolved,
                            },
                        }));
                    }
                }
                // BuiltinType base in MRO
                if let PyObjectPayload::BuiltinType(bt_name) = &base.payload {
                    // type.__call__ needs VM access; return a BoundMethod marker
                    if bt_name.as_str() == "type" && name == "__call__" {
                        return Some(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: instance.clone(),
                                method: PyObject::native_function("__type_call__", |_| {
                                    Ok(PyObject::none())
                                }),
                            },
                        }));
                    }
                    if let Some(resolved) = resolve_builtin_type_method(bt_name.as_str(), name) {
                        // __new__ is a static method: don't bind obj
                        if name == "__new__" {
                            return Some(resolved);
                        }
                        // Wrap as BoundMethod so obj is prepended
                        return Some(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BoundMethod {
                                receiver: instance.clone(),
                                method: resolved,
                            },
                        }));
                    }
                    // For builtin type methods (list.append, dict.update, etc.)
                    // that aren't in resolve_builtin_type_method, return a
                    // BuiltinBoundMethod that the VM dispatches via __builtin_value__
                    let known_methods = match bt_name.as_str() {
                        "list" => matches!(
                            name,
                            "append"
                                | "extend"
                                | "insert"
                                | "remove"
                                | "pop"
                                | "clear"
                                | "reverse"
                                | "sort"
                                | "copy"
                                | "count"
                                | "index"
                                | "__len__"
                                | "__iter__"
                                | "__contains__"
                                | "__getitem__"
                                | "__setitem__"
                                | "__delitem__"
                        ),
                        "dict" => matches!(
                            name,
                            "keys"
                                | "values"
                                | "items"
                                | "get"
                                | "pop"
                                | "update"
                                | "setdefault"
                                | "clear"
                                | "copy"
                                | "popitem"
                                | "__len__"
                                | "__iter__"
                                | "__contains__"
                                | "__getitem__"
                                | "__setitem__"
                                | "__delitem__"
                        ),
                        "set" => matches!(
                            name,
                            "add"
                                | "remove"
                                | "discard"
                                | "pop"
                                | "clear"
                                | "copy"
                                | "update"
                                | "intersection_update"
                                | "difference_update"
                                | "symmetric_difference_update"
                                | "union"
                                | "intersection"
                                | "difference"
                                | "symmetric_difference"
                                | "issubset"
                                | "issuperset"
                                | "__len__"
                                | "__iter__"
                                | "__contains__"
                        ),
                        "str" => matches!(
                            name,
                            "upper"
                                | "lower"
                                | "strip"
                                | "lstrip"
                                | "rstrip"
                                | "split"
                                | "rsplit"
                                | "join"
                                | "replace"
                                | "startswith"
                                | "endswith"
                                | "find"
                                | "rfind"
                                | "index"
                                | "rindex"
                                | "count"
                                | "encode"
                                | "format"
                                | "center"
                                | "ljust"
                                | "rjust"
                                | "zfill"
                                | "title"
                                | "capitalize"
                                | "swapcase"
                                | "partition"
                                | "rpartition"
                                | "expandtabs"
                                | "__len__"
                                | "__iter__"
                                | "__contains__"
                                | "__getitem__"
                        ),
                        "int" => matches!(
                            name,
                            "bit_length"
                                | "to_bytes"
                                | "from_bytes"
                                | "__int__"
                                | "__float__"
                                | "__index__"
                        ),
                        "tuple" => matches!(
                            name,
                            "count"
                                | "index"
                                | "__len__"
                                | "__iter__"
                                | "__contains__"
                                | "__getitem__"
                        ),
                        _ => false,
                    };
                    if known_methods {
                        return Some(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod(
                                crate::object::constructors::alloc_bbm_box(
                                    instance.clone(),
                                    CompactString::from(name),
                                ),
                            ),
                        }));
                    }
                }
            }
            // Fallback: if cls not found in MRO, look in cls's own bases
            if !found_cls {
                if let PyObjectPayload::Class(ccd) = &cls.payload {
                    for base in &ccd.bases {
                        if let PyObjectPayload::Class(bcd) = &base.payload {
                            if let Some(v) = bcd.namespace.read().get(name) {
                                if matches!(&v.payload, PyObjectPayload::Function(_)) {
                                    return Some(PyObjectRef::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod {
                                            receiver: instance.clone(),
                                            method: v.clone(),
                                        },
                                    }));
                                }
                                if let PyObjectPayload::ClassMethod(func) = &v.payload {
                                    let bound_cls = match &instance.payload {
                                        PyObjectPayload::Instance(inst) => inst.class.clone(),
                                        _ => instance.clone(),
                                    };
                                    return Some(PyObjectRef::new(PyObject {
                                        payload: PyObjectPayload::BoundMethod {
                                            receiver: bound_cls,
                                            method: func.clone(),
                                        },
                                    }));
                                }
                                if let PyObjectPayload::StaticMethod(func) = &v.payload {
                                    return Some(func.clone());
                                }
                                return Some(v.clone());
                            }
                        }
                        // Check BuiltinType bases (e.g., type, object)
                        if let PyObjectPayload::BuiltinType(bt_name) = &base.payload {
                            if let Some(resolved) =
                                resolve_builtin_type_method(bt_name.as_str(), name)
                            {
                                return Some(PyObjectRef::new(PyObject {
                                    payload: PyObjectPayload::BoundMethod {
                                        receiver: instance.clone(),
                                        method: resolved,
                                    },
                                }));
                            }
                        }
                        // Check ExceptionType bases
                        if matches!(&base.payload, PyObjectPayload::ExceptionType(_)) {
                            if let Some(resolved) =
                                exception_attrs::resolve_exception_type_method(name, instance)
                            {
                                return Some(PyObjectRef::new(PyObject {
                                    payload: PyObjectPayload::BoundMethod {
                                        receiver: instance.clone(),
                                        method: resolved,
                                    },
                                }));
                            }
                        }
                    }
                }
            }
            // Builtin __new__: object.__new__(cls) creates a new instance
            if name == "__new__" {
                return Some(PyObject::native_function("__new__", |args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("__new__ requires cls"));
                    }
                    if args.len() != 1 {
                        return Err(PyException::type_error(
                            "object.__new__() takes exactly one argument (the type to instantiate)",
                        ));
                    }
                    Ok(PyObject::instance(args[0].clone()))
                }));
            }
            // Fallback: check instance attrs for methods installed by
            // parent __init__ (e.g., BytesIO.__init__ installs write/read
            // as NativeClosure on the instance, not in the class namespace)
            if let PyObjectPayload::Instance(inst) = &instance.payload {
                if let Some(v) = inst.attrs.read().get(name).cloned() {
                    if matches!(
                        &v.payload,
                        PyObjectPayload::NativeClosure(_) | PyObjectPayload::NativeFunction(_)
                    ) {
                        return Some(v);
                    }
                    return Some(v);
                }
            }
            // Builtin __init__: object.__init__() is a no-op
            if name == "__init__" {
                return Some(PyObject::native_function("__init__", |args| {
                    if args.len() != 1 {
                        return Err(PyException::type_error(
                            "object.__init__() takes exactly one argument (the instance to initialize)",
                        ));
                    }
                    Ok(PyObject::none())
                }));
            }
            // Builtin __init_subclass__: object.__init_subclass__() is a no-op
            if name == "__init_subclass__" {
                return Some(PyObject::native_function("__init_subclass__", |_args| {
                    Ok(PyObject::none())
                }));
            }
            // Builtin __setattr__: object.__setattr__(self, name, value)
            if name == "__setattr__" {
                let inst = instance.clone();
                return Some(PyObject::native_closure(
                    "__setattr__",
                    move |args: &[PyObjectRef]| {
                        if args.len() < 2 {
                            return Err(PyException::type_error(
                                "__setattr__ requires name and value",
                            ));
                        }
                        let attr_name = args[0].py_to_string();
                        let value = args[1].clone();
                        if let PyObjectPayload::Instance(data) = &inst.payload {
                            data.attrs
                                .write()
                                .insert(CompactString::from(attr_name.as_str()), value);
                        }
                        Ok(PyObject::none())
                    },
                ));
            }
            // Builtin __delattr__: object.__delattr__(self, name)
            if name == "__delattr__" {
                let inst = instance.clone();
                return Some(PyObject::native_closure(
                    "__delattr__",
                    move |args: &[PyObjectRef]| {
                        if args.is_empty() {
                            return Err(PyException::type_error(
                                "__delattr__ requires name argument",
                            ));
                        }
                        let attr_name = args[0].py_to_string();
                        if let PyObjectPayload::Instance(data) = &inst.payload {
                            let removed = data.attrs.write().shift_remove(attr_name.as_str());
                            if removed.is_none() {
                                return Err(PyException::attribute_error(format!(
                                    "'{}' object has no attribute '{}'",
                                    data.class.py_to_string(),
                                    attr_name
                                )));
                            }
                        }
                        Ok(PyObject::none())
                    },
                ));
            }
            // Builtin __eq__: object.__eq__ is identity comparison
            if name == "__eq__" {
                let inst = instance.clone();
                return Some(PyObject::native_closure(
                    "__eq__",
                    move |args: &[PyObjectRef]| {
                        if args.is_empty() {
                            return Err(PyException::type_error("__eq__ requires an argument"));
                        }
                        Ok(PyObject::bool_val(PyObjectRef::ptr_eq(&inst, &args[0])))
                    },
                ));
            }
            // Builtin __ne__: object.__ne__ is negated identity
            if name == "__ne__" {
                let inst = instance.clone();
                return Some(PyObject::native_closure(
                    "__ne__",
                    move |args: &[PyObjectRef]| {
                        if args.is_empty() {
                            return Err(PyException::type_error("__ne__ requires an argument"));
                        }
                        Ok(PyObject::bool_val(!PyObjectRef::ptr_eq(&inst, &args[0])))
                    },
                ));
            }
            // Builtin __repr__ / __str__: default object repr
            if name == "__repr__" || name == "__str__" {
                let inst = instance.clone();
                return Some(PyObject::native_closure(
                    name,
                    move |_args: &[PyObjectRef]| {
                        let cls_name = if let PyObjectPayload::Instance(data) = &inst.payload {
                            data.class.py_to_string()
                        } else {
                            "object".into()
                        };
                        Ok(PyObject::str_val(CompactString::from(format!(
                            "<{} object>",
                            cls_name
                        ))))
                    },
                ));
            }
            // Builtin __hash__: default hash from object id
            if name == "__hash__" {
                let inst = instance.clone();
                return Some(PyObject::native_closure(
                    "__hash__",
                    move |_args: &[PyObjectRef]| {
                        let ptr = PyObjectRef::as_ptr(&inst) as usize;
                        Ok(PyObject::int(ptr as i64))
                    },
                ));
            }
        }
    }
    None
}
