use crate::types::{HashableKey, PyInt};
use compact_str::CompactString;
use indexmap::IndexMap;
use std::rc::Rc;

use super::*;

pub(super) fn builtin_type_attr(
    obj: &PyObjectRef,
    n: &CompactString,
    name: &str,
) -> Option<PyObjectRef> {
    match name {
        "__name__" | "__qualname__" => Some(PyObject::str_val(n.clone())),
        "__module__" => Some(PyObject::str_val(CompactString::from("builtins"))),
        "__itemsize__" => Some(PyObject::int(8)),
        "__basicsize__" => Some(PyObject::int(0)),
        "__doc__" => Some(PyObject::none()),
        "__dict__" => {
            // Return a mappingproxy with common type descriptors
            let mut map = new_fx_hashkey_map();
            if n.as_str() == "type" || n.as_str() == "object" {
                // type.__dict__["__dict__"] is a getset_descriptor with __get__
                // that returns obj.__dict__ when called as descriptor.__get__(obj)
                let desc_cls = PyObject::class(
                    CompactString::from("getset_descriptor"),
                    vec![],
                    IndexMap::new(),
                );
                let desc = PyObject::instance(desc_cls);
                if let PyObjectPayload::Instance(ref inst) = desc.payload {
                    inst.attrs.write().insert(
                        CompactString::from("__get__"),
                        PyObject::native_function(
                            "getset_descriptor.__get__",
                            |args: &[PyObjectRef]| {
                                // __get__(self, obj, objtype=None) → obj.__dict__
                                if args.len() >= 2 {
                                    if let Some(d) = args[1].get_attr("__dict__") {
                                        return Ok(d);
                                    }
                                }
                                if args.len() >= 1 {
                                    if let Some(d) = args[0].get_attr("__dict__") {
                                        return Ok(d);
                                    }
                                }
                                Ok(PyObject::dict(new_fx_hashkey_map()))
                            },
                        ),
                    );
                }
                map.insert(HashableKey::str_key(CompactString::from("__dict__")), desc);
                map.insert(
                    HashableKey::str_key(CompactString::from("__doc__")),
                    PyObject::none(),
                );
                map.insert(
                    HashableKey::str_key(CompactString::from("__repr__")),
                    PyObject::builtin_type(CompactString::from("wrapper_descriptor")),
                );
                map.insert(
                    HashableKey::str_key(CompactString::from("__subclasshook__")),
                    PyObject::builtin_type(CompactString::from("method_descriptor")),
                );
            }
            Some(PyObject::wrap(PyObjectPayload::MappingProxy(Rc::new(
                PyCell::new(map),
            ))))
        }
        "__bases__" => {
            let parent = match n.as_str() {
                "object" => vec![],
                "bool" => vec![PyObject::builtin_type(CompactString::from("int"))],
                "bytearray" => vec![PyObject::builtin_type(CompactString::from("object"))],
                _ => vec![PyObject::builtin_type(CompactString::from("object"))],
            };
            Some(PyObject::tuple(parent))
        }
        "__mro__" => {
            let mro = match n.as_str() {
                "object" => vec![obj.clone()],
                "bool" => vec![
                    obj.clone(),
                    PyObject::builtin_type(CompactString::from("int")),
                    PyObject::builtin_type(CompactString::from("object")),
                ],
                _ => vec![
                    obj.clone(),
                    PyObject::builtin_type(CompactString::from("object")),
                ],
            };
            Some(PyObject::tuple(mro))
        }
        "fromkeys" if n.as_str() == "dict" => {
            Some(PyObject::native_function("dict.fromkeys", |args| {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "fromkeys() requires at least 1 argument",
                    ));
                }
                let value = if args.len() >= 2 {
                    args[1].clone()
                } else {
                    PyObject::none()
                };
                let mut map = new_fx_hashkey_map();
                match &args[0].payload {
                    PyObjectPayload::Dict(keys) => {
                        for key in keys.read().keys() {
                            map.insert(key.clone(), value.clone());
                        }
                    }
                    PyObjectPayload::Set(keys) => {
                        for key in keys.read().keys() {
                            map.insert(key.clone(), value.clone());
                        }
                    }
                    PyObjectPayload::FrozenSet(keys) => {
                        for key in keys.keys() {
                            map.insert(key.clone(), value.clone());
                        }
                    }
                    _ => {
                        let keys = args[0].to_list()?;
                        for k in keys {
                            let dk = HashableKey::from_object(&k)?;
                            map.insert(dk, value.clone());
                        }
                    }
                }
                Ok(PyObject::dict(map))
            }))
        }
        "__init__" | "get" | "pop" | "setdefault" | "clear" | "popitem" | "update"
            if n.as_str() == "dict" =>
        {
            super::super::helpers::resolve_builtin_type_method("dict", name)
        }
        "__getformat__" if n.as_str() == "float" => {
            Some(PyObject::native_function("float.__getformat__", |args| {
                if args.len() != 1 {
                    return Err(PyException::type_error(
                        "__getformat__() requires exactly one argument",
                    ));
                }
                let kind = args[0].py_to_string();
                match kind.as_str() {
                    "double" | "float" => Ok(PyObject::str_val(CompactString::from(
                        "IEEE, little-endian",
                    ))),
                    _ => Err(PyException::value_error(
                        "__getformat__() argument 1 must be 'double' or 'float'",
                    )),
                }
            }))
        }
        "maketrans" if n.as_str() == "str" => {
            Some(PyObject::native_function("str.maketrans", |args| {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "maketrans() requires at least 1 argument",
                    ));
                }
                let mut result_map = new_fx_hashkey_map();
                if args.len() == 1 {
                    if let PyObjectPayload::Dict(map) = &args[0].payload {
                        for (k, v) in map.read().iter() {
                            let key = match k {
                                HashableKey::Int(n) => n.clone(),
                                HashableKey::Str(s) => {
                                    if let Some(c) = s.chars().next() {
                                        PyInt::Small(c as i64)
                                    } else {
                                        continue;
                                    }
                                }
                                _ => continue,
                            };
                            result_map.insert(HashableKey::Int(key), v.clone());
                        }
                    }
                } else {
                    let x = args[0].py_to_string();
                    let y = args[1].py_to_string();
                    for (cx, cy) in x.chars().zip(y.chars()) {
                        result_map.insert(
                            HashableKey::Int(PyInt::Small(cx as i64)),
                            PyObject::int(cy as i64),
                        );
                    }
                    if args.len() > 2 {
                        let z = args[2].py_to_string();
                        for cz in z.chars() {
                            result_map.insert(
                                HashableKey::Int(PyInt::Small(cz as i64)),
                                PyObject::none(),
                            );
                        }
                    }
                }
                Ok(PyObject::dict(result_map))
            }))
        }
        "fromhex" if n.as_str() == "bytes" || n.as_str() == "bytearray" => {
            let is_bytearray = n.as_str() == "bytearray";
            Some(PyObject::wrap(PyObjectPayload::NativeClosure(Box::new(
                NativeClosureData {
                    name: CompactString::from("fromhex"),
                    pickle_args: None,
                    func: std::rc::Rc::new(move |args| {
                        if args.is_empty() {
                            return Err(PyException::type_error(
                                "fromhex() missing required argument",
                            ));
                        }
                        let bytes = bytes_fromhex_data(&args[0])?;
                        if is_bytearray {
                            Ok(PyObject::bytearray(bytes))
                        } else {
                            Ok(PyObject::bytes(bytes))
                        }
                    }),
                },
            ))))
        }
        "__setattr__" if n.as_str() == "type" => {
            Some(PyObject::native_function("type.__setattr__", |args| {
                if args.len() != 3 {
                    return Err(PyException::type_error(
                        "type.__setattr__() takes exactly 3 arguments",
                    ));
                }
                let name = args[1]
                    .as_str()
                    .ok_or_else(|| PyException::type_error("attribute name must be string"))?;
                let PyObjectPayload::Class(cd) = &args[0].payload else {
                    return Err(PyException::type_error(
                        "descriptor '__setattr__' requires a 'type' object",
                    ));
                };
                cd.namespace
                    .write()
                    .insert(CompactString::from(name), args[2].clone());
                cd.invalidate_cache();
                Ok(PyObject::none())
            }))
        }
        "__delattr__" if n.as_str() == "type" => {
            Some(PyObject::native_function("type.__delattr__", |args| {
                if args.len() != 2 {
                    return Err(PyException::type_error(
                        "type.__delattr__() takes exactly 2 arguments",
                    ));
                }
                let name = args[1]
                    .as_str()
                    .ok_or_else(|| PyException::type_error("attribute name must be string"))?;
                let PyObjectPayload::Class(cd) = &args[0].payload else {
                    return Err(PyException::type_error(
                        "descriptor '__delattr__' requires a 'type' object",
                    ));
                };
                if cd.namespace.write().shift_remove(name).is_none() {
                    return Err(PyException::attribute_error(name));
                }
                cd.invalidate_cache();
                Ok(PyObject::none())
            }))
        }
        // object.__setattr__(instance, name, value) — bypass custom __setattr__
        "__setattr__" => Some(PyObject::native_function("object.__setattr__", |args| {
            if args.len() != 3 {
                return Err(PyException::type_error(
                    "object.__setattr__() takes exactly 3 arguments",
                ));
            }
            let name = args[1]
                .as_str()
                .ok_or_else(|| PyException::type_error("attribute name must be string"))?;
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                inst.attrs
                    .write()
                    .insert(CompactString::from(name), args[2].clone());
            }
            Ok(PyObject::none())
        })),
        // object.__getattribute__(instance, name) — bypass custom __getattribute__
        "__getattribute__" => Some(PyObject::native_function(
            "object.__getattribute__",
            |args| {
                if args.len() != 2 {
                    return Err(PyException::type_error(
                        "object.__getattribute__() takes exactly 2 arguments",
                    ));
                }
                let name = args[1]
                    .as_str()
                    .ok_or_else(|| PyException::type_error("attribute name must be string"))?;
                if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    if let Some(val) = inst.attrs.read().get(name) {
                        return Ok(val.clone());
                    }
                }
                args[0].get_attr(name).ok_or_else(|| {
                    PyException::attribute_error(&format!(
                        "'{}' object has no attribute '{}'",
                        args[0].type_name(),
                        name
                    ))
                })
            },
        )),
        // object.__delattr__(instance, name) — bypass custom __delattr__
        "__delattr__" => Some(PyObject::native_function("object.__delattr__", |args| {
            if args.len() != 2 {
                return Err(PyException::type_error(
                    "object.__delattr__() takes exactly 2 arguments",
                ));
            }
            let name = args[1]
                .as_str()
                .ok_or_else(|| PyException::type_error("attribute name must be string"))?;
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                inst.attrs.write().shift_remove(name);
            }
            Ok(PyObject::none())
        })),
        _ => {
            if name.starts_with("__") && name.ends_with("__") {
                // O(1) lookup for supported dunders on builtin types.
                use std::collections::HashSet;
                use std::sync::LazyLock;
                static BUILTIN_DUNDERS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
                    [
                        "__init__",
                        "__new__",
                        "__str__",
                        "__repr__",
                        "__hash__",
                        "__eq__",
                        "__ne__",
                        "__lt__",
                        "__le__",
                        "__gt__",
                        "__ge__",
                        "__bool__",
                        "__len__",
                        "__getitem__",
                        "__setitem__",
                        "__delitem__",
                        "__contains__",
                        "__iter__",
                        "__next__",
                        "__call__",
                        "__add__",
                        "__sub__",
                        "__mul__",
                        "__truediv__",
                        "__floordiv__",
                        "__mod__",
                        "__pow__",
                        "__and__",
                        "__or__",
                        "__xor__",
                        "__neg__",
                        "__pos__",
                        "__abs__",
                        "__invert__",
                        "__radd__",
                        "__rsub__",
                        "__rmul__",
                        "__rtruediv__",
                        "__rfloordiv__",
                        "__rmod__",
                        "__rpow__",
                        "__rand__",
                        "__ror__",
                        "__rxor__",
                        "__iadd__",
                        "__isub__",
                        "__imul__",
                        "__itruediv__",
                        "__ifloordiv__",
                        "__imod__",
                        "__ipow__",
                        "__iand__",
                        "__ior__",
                        "__ixor__",
                        "__lshift__",
                        "__rshift__",
                        "__rlshift__",
                        "__rrshift__",
                        "__ilshift__",
                        "__irshift__",
                        "__enter__",
                        "__exit__",
                        "__format__",
                        "__index__",
                        "__int__",
                        "__float__",
                        "__complex__",
                        "__round__",
                        "__reversed__",
                        "__del__",
                        "__copy__",
                        "__deepcopy__",
                        "__reduce__",
                        "__reduce_ex__",
                        "__sizeof__",
                        "__class__",
                        "__subclasses__",
                        "__subclasshook__",
                    ]
                    .into_iter()
                    .collect()
                });
                // Container-only dunders: not valid for numeric/NoneType
                static CONTAINER_DUNDERS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
                    [
                        "__len__",
                        "__getitem__",
                        "__setitem__",
                        "__delitem__",
                        "__contains__",
                        "__iter__",
                        "__next__",
                        "__reversed__",
                    ]
                    .into_iter()
                    .collect()
                });
                if BUILTIN_DUNDERS.contains(name) {
                    // Exclude container dunders for non-container types
                    let is_non_container = matches!(
                        n.as_str(),
                        "int" | "float" | "complex" | "bool" | "NoneType" | "type"
                    );
                    if is_non_container && CONTAINER_DUNDERS.contains(name) {
                        return None;
                    }
                    // Check if resolve_builtin_type_method has a real implementation
                    if let Some(native) = resolve_builtin_type_method(n, name) {
                        return Some(native);
                    }
                    Some(PyObjectRef::new(PyObject {
                        payload: PyObjectPayload::BuiltinBoundMethod(
                            crate::object::constructors::alloc_bbm_box(
                                obj.clone(),
                                CompactString::from(name),
                            ),
                        ),
                    }))
                } else {
                    None
                }
            } else {
                // Unbound method access: str.upper, list.append, etc.
                // Only return a bound method if the type actually has this method.
                if let Some(native) = resolve_builtin_type_method(n, name) {
                    return Some(native);
                }
                // Check a known set of non-dunder methods per type
                let has_method = match n.as_str() {
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
                            | "find"
                            | "rfind"
                            | "index"
                            | "rindex"
                            | "count"
                            | "startswith"
                            | "endswith"
                            | "encode"
                            | "decode"
                            | "format"
                            | "format_map"
                            | "center"
                            | "ljust"
                            | "rjust"
                            | "zfill"
                            | "expandtabs"
                            | "title"
                            | "capitalize"
                            | "swapcase"
                            | "casefold"
                            | "isalpha"
                            | "isdigit"
                            | "isalnum"
                            | "isspace"
                            | "isupper"
                            | "islower"
                            | "istitle"
                            | "isnumeric"
                            | "isdecimal"
                            | "isidentifier"
                            | "isprintable"
                            | "isascii"
                            | "partition"
                            | "rpartition"
                            | "splitlines"
                            | "translate"
                            | "removeprefix"
                            | "removesuffix"
                            | "maketrans"
                    ),
                    "list" => matches!(
                        name,
                        "append"
                            | "extend"
                            | "insert"
                            | "remove"
                            | "pop"
                            | "clear"
                            | "index"
                            | "count"
                            | "sort"
                            | "reverse"
                            | "copy"
                    ),
                    "dict" => matches!(
                        name,
                        "keys"
                            | "values"
                            | "items"
                            | "get"
                            | "pop"
                            | "popitem"
                            | "setdefault"
                            | "update"
                            | "clear"
                            | "copy"
                            | "fromkeys"
                    ),
                    "set" | "frozenset" => matches!(
                        name,
                        "add"
                            | "remove"
                            | "discard"
                            | "pop"
                            | "clear"
                            | "copy"
                            | "union"
                            | "intersection"
                            | "difference"
                            | "symmetric_difference"
                            | "update"
                            | "intersection_update"
                            | "difference_update"
                            | "symmetric_difference_update"
                            | "issubset"
                            | "issuperset"
                            | "isdisjoint"
                    ),
                    "tuple" => matches!(name, "count" | "index"),
                    "bytes" | "bytearray" => matches!(
                        name,
                        "decode"
                            | "hex"
                            | "count"
                            | "find"
                            | "rfind"
                            | "index"
                            | "rindex"
                            | "split"
                            | "rsplit"
                            | "join"
                            | "replace"
                            | "strip"
                            | "lstrip"
                            | "rstrip"
                            | "startswith"
                            | "endswith"
                            | "upper"
                            | "lower"
                            | "title"
                            | "capitalize"
                            | "swapcase"
                            | "center"
                            | "ljust"
                            | "rjust"
                            | "zfill"
                            | "expandtabs"
                            | "isalpha"
                            | "isdigit"
                            | "isalnum"
                            | "isspace"
                            | "isupper"
                            | "islower"
                            | "translate"
                            | "partition"
                            | "rpartition"
                            | "splitlines"
                            | "fromhex"
                            | "extend"
                            | "append"
                            | "insert"
                            | "pop"
                            | "remove"
                            | "reverse"
                            | "copy"
                            | "clear"
                            | "maketrans"
                    ),
                    "int" => matches!(
                        name,
                        "bit_length"
                            | "to_bytes"
                            | "from_bytes"
                            | "conjugate"
                            | "real"
                            | "imag"
                            | "numerator"
                            | "denominator"
                    ),
                    "float" => matches!(
                        name,
                        "is_integer"
                            | "hex"
                            | "fromhex"
                            | "as_integer_ratio"
                            | "conjugate"
                            | "real"
                            | "imag"
                    ),
                    "type" => matches!(name, "mro"),
                    "property" => {
                        matches!(name, "__get__" | "getter" | "setter" | "deleter")
                    }
                    _ => false,
                };
                if has_method {
                    if n.as_str() == "dict" && name == "items" {
                        Some(PyObject::native_closure("dict.items", |_args| {
                            Ok(PyObject::none())
                        }))
                    } else {
                        Some(PyObjectRef::new(PyObject {
                            payload: PyObjectPayload::BuiltinBoundMethod(
                                crate::object::constructors::alloc_bbm_box(
                                    obj.clone(),
                                    CompactString::from(name),
                                ),
                            ),
                        }))
                    }
                } else {
                    None
                }
            }
        }
    }
}
