use super::*;

pub fn create_typing_module() -> PyObjectRef {
    // TypeVar class — shared so isinstance(T, TypeVar) works
    let typevar_class = PyObject::class(CompactString::from("TypeVar"), vec![], IndexMap::new());
    let typevar_cls_ref = typevar_class.clone();

    // TypeVar(name, *constraints, bound=None, covariant=False, contravariant=False)
    let typevar_new = {
        let tv_cls = typevar_cls_ref.clone();
        PyObject::native_closure("TypeVar.__new__", move |args: &[PyObjectRef]| {
            // First arg is the class itself (cls), rest are user args
            let real_args =
                if !args.is_empty() && matches!(&args[0].payload, PyObjectPayload::Class(_)) {
                    &args[1..]
                } else {
                    args
                };
            check_args_min("TypeVar", real_args, 1)?;

            let last_is_kwargs = real_args.len() >= 2
                && matches!(
                    &real_args[real_args.len() - 1].payload,
                    PyObjectPayload::Dict(_)
                );

            let kwargs_dict: Option<FxHashKeyMap> = if last_is_kwargs {
                if let PyObjectPayload::Dict(d) = &real_args[real_args.len() - 1].payload {
                    Some(d.read().clone())
                } else {
                    None
                }
            } else {
                None
            };

            let positional_end = if last_is_kwargs {
                real_args.len() - 1
            } else {
                real_args.len()
            };
            let name = CompactString::from(real_args[0].py_to_string());

            let constraints: Vec<PyObjectRef> = if positional_end > 1 {
                real_args[1..positional_end].to_vec()
            } else {
                vec![]
            };

            let get_kwarg = |key: &str| -> Option<PyObjectRef> {
                kwargs_dict.as_ref().and_then(|kw| {
                    kw.get(&HashableKey::str_key(CompactString::from(key)))
                        .cloned()
                })
            };
            let bound = get_kwarg("bound").unwrap_or_else(PyObject::none);
            let covariant = get_kwarg("covariant").map_or(false, |v| v.is_truthy());
            let contravariant = get_kwarg("contravariant").map_or(false, |v| v.is_truthy());

            let mut attrs = IndexMap::new();
            attrs.insert(
                CompactString::from("__name__"),
                PyObject::str_val(name.clone()),
            );
            attrs.insert(
                CompactString::from("__constraints__"),
                PyObject::tuple(constraints),
            );
            attrs.insert(CompactString::from("__bound__"), bound);
            attrs.insert(
                CompactString::from("__covariant__"),
                PyObject::bool_val(covariant),
            );
            attrs.insert(
                CompactString::from("__contravariant__"),
                PyObject::bool_val(contravariant),
            );

            let repr_name = name.clone();
            attrs.insert(
                CompactString::from("__repr__"),
                PyObject::native_closure("__repr__", move |_args| {
                    Ok(PyObject::str_val(CompactString::from(
                        format!("~{}", repr_name).as_str(),
                    )))
                }),
            );

            Ok(PyObject::instance_with_attrs(tv_cls.clone(), attrs))
        })
    };

    // Set __new__ on the TypeVar class so TypeVar("T") creates proper instances
    if let PyObjectPayload::Class(ref data) = typevar_class.payload {
        data.namespace
            .write()
            .insert(CompactString::from("__new__"), typevar_new);
        data.invalidate_cache();
    }

    // Generic — placeholder class
    let generic_class = PyObject::class(CompactString::from("Generic"), vec![], IndexMap::new());

    // Protocol — placeholder class
    let protocol_class = PyObject::class(CompactString::from("Protocol"), vec![], IndexMap::new());

    // Helper to create typing generic alias classes
    // These support subscript notation: List[int] → _GenericAlias with __origin__ and __args__
    let make_typing_alias = |display_name: &str| -> PyObjectRef {
        let name = CompactString::from(display_name);
        let display = name.clone();
        let mut ns = IndexMap::new();
        ns.insert(
            CompactString::from("__typing_name__"),
            PyObject::str_val(name),
        );
        // __class_getitem__ to support List[int] etc.
        ns.insert(
            CompactString::from("__class_getitem__"),
            PyObject::native_closure("__class_getitem__", {
                let display = display.clone();
                move |args: &[PyObjectRef]| -> Result<PyObjectRef, PyException> {
                    // Build a _GenericAlias object with __origin__, __args__, __repr__
                    let origin_display = display.clone();
                    let params = if args.len() >= 2 {
                        args[1].clone()
                    } else if args.len() == 1 {
                        args[0].clone()
                    } else {
                        PyObject::none()
                    };

                    // Build __args__ tuple
                    let args_tuple = match &params.payload {
                        PyObjectPayload::Tuple(items) => PyObject::tuple((**items).clone()),
                        _ => PyObject::tuple(vec![params.clone()]),
                    };

                    let params_str = match &params.payload {
                        PyObjectPayload::Tuple(items) => items
                            .iter()
                            .map(|i| {
                                if let PyObjectPayload::Class(cd) = &i.payload {
                                    cd.name.to_string()
                                } else if let PyObjectPayload::BuiltinType(n) = &i.payload {
                                    n.to_string()
                                } else {
                                    i.py_to_string()
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(", "),
                        PyObjectPayload::Class(cd) => cd.name.to_string(),
                        PyObjectPayload::BuiltinType(n) => n.to_string(),
                        _ => params.py_to_string(),
                    };
                    let repr = format!("typing.{}[{}]", origin_display, params_str);

                    let cls = PyObject::class(
                        CompactString::from("_GenericAlias"),
                        vec![],
                        IndexMap::new(),
                    );
                    let mut attrs = IndexMap::new();
                    // Map typing names to actual builtin types for get_origin() CPython compat
                    let origin_obj = match origin_display.as_str() {
                        "List" => PyObject::builtin_type(CompactString::from("list")),
                        "Dict" => PyObject::builtin_type(CompactString::from("dict")),
                        "Set" => PyObject::builtin_type(CompactString::from("set")),
                        "FrozenSet" => PyObject::builtin_type(CompactString::from("frozenset")),
                        "Tuple" => PyObject::builtin_type(CompactString::from("tuple")),
                        "Type" => PyObject::builtin_type(CompactString::from("type")),
                        _ => PyObject::str_val(CompactString::from(origin_display.as_str())),
                    };
                    attrs.insert(CompactString::from("__origin__"), origin_obj);
                    attrs.insert(CompactString::from("__args__"), args_tuple);
                    // __repr__ and __str__ must be callable (not plain strings)
                    let repr_clone = repr.clone();
                    attrs.insert(
                        CompactString::from("__repr__"),
                        PyObject::native_closure("__repr__", move |_args| {
                            Ok(PyObject::str_val(CompactString::from(repr_clone.as_str())))
                        }),
                    );
                    let str_clone = repr.clone();
                    attrs.insert(
                        CompactString::from("__str__"),
                        PyObject::native_closure("__str__", move |_args| {
                            Ok(PyObject::str_val(CompactString::from(str_clone.as_str())))
                        }),
                    );
                    Ok(PyObject::instance_with_attrs(cls, attrs))
                }
            }),
        );
        PyObject::class(CompactString::from(display_name), vec![], ns)
    };

    // Helper to create a simple TypeVar instance
    let make_typevar = |name: &str| -> PyObjectRef {
        let n = CompactString::from(name);
        let mut tv_attrs = IndexMap::new();
        tv_attrs.insert(
            CompactString::from("__name__"),
            PyObject::str_val(n.clone()),
        );
        tv_attrs.insert(
            CompactString::from("__constraints__"),
            PyObject::tuple(vec![]),
        );
        tv_attrs.insert(CompactString::from("__bound__"), PyObject::none());
        tv_attrs.insert(
            CompactString::from("__covariant__"),
            PyObject::bool_val(false),
        );
        tv_attrs.insert(
            CompactString::from("__contravariant__"),
            PyObject::bool_val(false),
        );
        let repr_name = n.clone();
        tv_attrs.insert(
            CompactString::from("__repr__"),
            PyObject::native_closure("__repr__", move |_args| {
                Ok(PyObject::str_val(CompactString::from(
                    format!("~{}", repr_name).as_str(),
                )))
            }),
        );
        PyObject::instance_with_attrs(typevar_cls_ref.clone(), tv_attrs)
    };

    let mut attrs: Vec<(&str, PyObjectRef)> = vec![
        ("Any", PyObject::builtin_type(CompactString::from("Any"))),
        ("Union", make_typing_alias("Union")),
        ("Optional", make_typing_alias("Optional")),
        ("List", make_typing_alias("List")),
        ("Dict", make_typing_alias("Dict")),
        ("Set", make_typing_alias("Set")),
        ("Tuple", make_typing_alias("Tuple")),
        ("FrozenSet", make_typing_alias("FrozenSet")),
        ("Type", make_typing_alias("Type")),
        ("Callable", make_typing_alias("Callable")),
        ("Iterator", make_typing_alias("Iterator")),
        ("Generator", make_typing_alias("Generator")),
        ("Sequence", make_typing_alias("Sequence")),
        ("Mapping", make_typing_alias("Mapping")),
        ("MutableMapping", make_typing_alias("MutableMapping")),
        ("Iterable", make_typing_alias("Iterable")),
        ("TypeVar", typevar_class),
        ("T", make_typevar("T")),
        ("T_co", make_typevar("T_co")),
        ("T_contra", make_typevar("T_contra")),
        ("KT", make_typevar("KT")),
        ("VT", make_typevar("VT")),
        ("VT_co", make_typevar("VT_co")),
        ("AnyStr", make_typevar("AnyStr")),
        ("Generic", generic_class),
        ("Protocol", protocol_class),
        ("ClassVar", make_typing_alias("ClassVar")),
        ("Final", make_typing_alias("Final")),
        ("Literal", make_typing_alias("Literal")),
        (
            "NamedTuple",
            PyObject::builtin_type(CompactString::from("NamedTuple")),
        ),
        (
            "get_type_hints",
            make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(PyObject::dict(IndexMap::new()));
                }
                let obj = &args[0];
                if let Some(ann) = obj.get_attr("__annotations__") {
                    if let PyObjectPayload::Dict(map) = &ann.payload {
                        let mut resolved = IndexMap::new();
                        for (key, value) in map.read().iter() {
                            let value = if let PyObjectPayload::Str(s) = &value.payload {
                                resolve_string_annotation(s.as_str())
                                    .unwrap_or_else(|| value.clone())
                            } else {
                                value.clone()
                            };
                            resolved.insert(key.clone(), value);
                        }
                        Ok(PyObject::dict(resolved))
                    } else {
                        Ok(ann)
                    }
                } else {
                    Ok(PyObject::dict(IndexMap::new()))
                }
            }),
        ),
        (
            "get_args",
            make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(PyObject::tuple(vec![]));
                }
                if let Some(type_args) = args[0].get_attr("__args__") {
                    Ok(type_args)
                } else {
                    Ok(PyObject::tuple(vec![]))
                }
            }),
        ),
        (
            "get_origin",
            make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(PyObject::none());
                }
                if let Some(origin) = args[0].get_attr("__origin__") {
                    Ok(origin)
                } else {
                    Ok(PyObject::none())
                }
            }),
        ),
        (
            "cast",
            make_builtin(|args: &[PyObjectRef]| {
                check_args("cast", args, 2)?;
                Ok(args[1].clone())
            }),
        ),
        (
            "overload",
            make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(PyObject::none());
                }
                Ok(args[0].clone())
            }),
        ),
        (
            "no_type_check",
            make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(PyObject::none());
                }
                Ok(args[0].clone())
            }),
        ),
        (
            "no_type_check_decorator",
            make_builtin(|args: &[PyObjectRef]| {
                // no_type_check_decorator is a decorator factory that returns a decorator
                // which applies no_type_check to functions decorated with it
                if args.is_empty() {
                    return Ok(PyObject::none());
                }
                Ok(args[0].clone())
            }),
        ),
        (
            "runtime_checkable",
            make_builtin(|args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(PyObject::none());
                }
                // Mark the class as runtime_checkable by adding __protocol_attrs__
                // which lists the methods that must be present for isinstance checks
                if let PyObjectPayload::Class(cd) = &args[0].payload {
                    let mut ns = cd.namespace.write();
                    // Collect all non-dunder, non-private method names from the protocol
                    let protocol_attrs: Vec<PyObjectRef> = ns
                        .iter()
                        .filter(|(k, _)| !k.starts_with('_'))
                        .map(|(k, _)| PyObject::str_val(k.clone()))
                        .collect();
                    let attrs_tuple = PyObject::tuple(protocol_attrs);
                    ns.insert(
                        CompactString::from("__protocol_attrs__"),
                        attrs_tuple.clone(),
                    );
                    ns.insert(
                        CompactString::from("_is_runtime_checkable"),
                        PyObject::bool_val(true),
                    );
                    // __instancecheck__(cls, obj) — structural check
                    ns.insert(
                        CompactString::from("__instancecheck__"),
                        PyObject::native_closure(
                            "__instancecheck__",
                            move |ic_args: &[PyObjectRef]| {
                                // ic_args[0] = cls (self), ic_args[1] = obj
                                if ic_args.len() < 2 {
                                    return Ok(PyObject::bool_val(false));
                                }
                                let obj = &ic_args[ic_args.len() - 1];
                                if let PyObjectPayload::Tuple(required) = &attrs_tuple.payload {
                                    let has_all = required.iter().all(|attr_name| {
                                        let name = attr_name.py_to_string();
                                        obj.get_attr(&name).is_some()
                                    });
                                    return Ok(PyObject::bool_val(has_all));
                                }
                                Ok(PyObject::bool_val(false))
                            },
                        ),
                    );
                }
                Ok(args[0].clone())
            }),
        ),
    ];
    attrs.push(("TYPE_CHECKING", PyObject::bool_val(false)));
    // InitVar — simple marker type for use with dataclasses
    attrs.push(("InitVar", make_typing_alias("InitVar")));
    // final — no-op decorator at runtime
    attrs.push((
        "final",
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            Ok(args[0].clone())
        }),
    ));
    // Additional typing constructs
    attrs.push(("Deque", make_typing_alias("Deque")));
    attrs.push(("DefaultDict", make_typing_alias("DefaultDict")));
    attrs.push(("OrderedDict", make_typing_alias("OrderedDict")));
    attrs.push(("Counter", make_typing_alias("Counter")));
    attrs.push(("ChainMap", make_typing_alias("ChainMap")));
    attrs.push(("Awaitable", make_typing_alias("Awaitable")));
    attrs.push(("Coroutine", make_typing_alias("Coroutine")));
    attrs.push(("AsyncIterator", make_typing_alias("AsyncIterator")));
    attrs.push(("AsyncGenerator", make_typing_alias("AsyncGenerator")));
    attrs.push(("AsyncIterable", make_typing_alias("AsyncIterable")));
    attrs.push(("ContextManager", make_typing_alias("ContextManager")));
    attrs.push((
        "AsyncContextManager",
        make_typing_alias("AsyncContextManager"),
    ));
    attrs.push(("Pattern", make_typing_alias("Pattern")));
    attrs.push(("Match", make_typing_alias("Match")));
    attrs.push(("IO", make_typing_alias("IO")));
    attrs.push(("TextIO", make_typing_alias("TextIO")));
    attrs.push(("BinaryIO", make_typing_alias("BinaryIO")));
    attrs.push(("SupportsInt", make_typing_alias("SupportsInt")));
    attrs.push(("SupportsFloat", make_typing_alias("SupportsFloat")));
    attrs.push(("SupportsComplex", make_typing_alias("SupportsComplex")));
    attrs.push(("SupportsBytes", make_typing_alias("SupportsBytes")));
    attrs.push(("SupportsAbs", make_typing_alias("SupportsAbs")));
    attrs.push(("SupportsRound", make_typing_alias("SupportsRound")));
    attrs.push(("SupportsIndex", make_typing_alias("SupportsIndex")));
    attrs.push(("Reversible", make_typing_alias("Reversible")));
    attrs.push(("Container", make_typing_alias("Container")));
    attrs.push(("Collection", make_typing_alias("Collection")));
    attrs.push(("Hashable", make_typing_alias("Hashable")));
    attrs.push(("Sized", make_typing_alias("Sized")));
    attrs.push(("MutableSequence", make_typing_alias("MutableSequence")));
    attrs.push(("MutableSet", make_typing_alias("MutableSet")));
    attrs.push(("MappingView", make_typing_alias("MappingView")));
    attrs.push(("KeysView", make_typing_alias("KeysView")));
    attrs.push(("ItemsView", make_typing_alias("ItemsView")));
    attrs.push(("ValuesView", make_typing_alias("ValuesView")));
    attrs.push(("AbstractSet", make_typing_alias("AbstractSet")));
    // Type guard utilities
    attrs.push((
        "NewType",
        make_builtin(|args: &[PyObjectRef]| {
            check_args("NewType", args, 2)?;
            Ok(args[1].clone()) // NewType(name, tp) returns tp
        }),
    ));
    // TypedDict: In CPython, TypedDict creates a class that, when instantiated,
    // returns a plain dict. TypedDict subclasses are conceptually dict subclasses.
    // We implement __new__ to return a dict built from kwargs.
    let typed_dict_cls = {
        let mut td_ns = IndexMap::new();
        td_ns.insert(
            CompactString::from("__init_subclass__"),
            make_builtin(|_args| Ok(PyObject::none())),
        );
        // __new__ returns a plain dict from kwargs
        td_ns.insert(
            CompactString::from("__new__"),
            PyObject::native_closure("__new__", |args: &[PyObjectRef]| {
                // args[0] = cls, rest = positional, last may be kwargs dict
                if args.len() > 1 {
                    if let PyObjectPayload::Dict(kw_map) = &args[args.len() - 1].payload {
                        let r = kw_map.read();
                        let mut data = IndexMap::new();
                        for (k, v) in r.iter() {
                            data.insert(k.clone(), v.clone());
                        }
                        return Ok(PyObject::dict(data));
                    }
                }
                Ok(PyObject::dict(IndexMap::new()))
            }),
        );
        PyObject::class(CompactString::from("TypedDict"), vec![], td_ns)
    };
    attrs.push(("TypedDict", typed_dict_cls));
    attrs.push(("ForwardRef", make_typing_alias("ForwardRef")));

    // Python 3.9+ additions
    attrs.push(("Annotated", make_typing_alias("Annotated")));
    attrs.push((
        "ParamSpec",
        PyObject::native_closure("ParamSpec", |args: &[PyObjectRef]| {
            let name = if let Some(a) = args.first() {
                a.py_to_string()
            } else {
                "P".into()
            };
            let cls = PyObject::class(CompactString::from("ParamSpec"), vec![], IndexMap::new());
            let mut iattrs = IndexMap::new();
            iattrs.insert(
                CompactString::from("__name__"),
                PyObject::str_val(CompactString::from(name.as_str())),
            );
            iattrs.insert(
                CompactString::from("args"),
                PyObject::str_val(CompactString::from(format!("{}.args", name))),
            );
            iattrs.insert(
                CompactString::from("kwargs"),
                PyObject::str_val(CompactString::from(format!("{}.kwargs", name))),
            );
            Ok(PyObject::instance_with_attrs(cls, iattrs))
        }),
    ));
    attrs.push(("TypeAlias", make_typing_alias("TypeAlias")));
    attrs.push(("TypeGuard", make_typing_alias("TypeGuard")));

    // Python 3.11+ additions
    attrs.push(("Never", make_typing_alias("Never")));
    attrs.push(("Self", make_typing_alias("Self")));
    attrs.push((
        "assert_type",
        make_builtin(|args: &[PyObjectRef]| {
            // assert_type(val, typ) → returns val (no-op at runtime)
            check_args("assert_type", args, 2)?;
            Ok(args[0].clone())
        }),
    ));
    attrs.push((
        "reveal_type",
        make_builtin(|args: &[PyObjectRef]| {
            check_args("reveal_type", args, 1)?;
            let val = &args[0];
            eprintln!("Runtime type is '{}'", val.type_name());
            Ok(val.clone())
        }),
    ));

    // Python 3.12+ additions
    attrs.push(("TypeAliasType", make_typing_alias("TypeAliasType")));
    attrs.push((
        "override",
        make_builtin(|args: &[PyObjectRef]| {
            // @override decorator — no-op at runtime
            check_args("override", args, 1)?;
            Ok(args[0].clone())
        }),
    ));

    // Common type forms
    attrs.push(("NoReturn", make_typing_alias("NoReturn")));
    attrs.push(("AnyStr", make_typing_alias("AnyStr")));
    attrs.push(("LiteralString", make_typing_alias("LiteralString")));
    attrs.push(("Unpack", make_typing_alias("Unpack")));
    attrs.push((
        "TypeVarTuple",
        PyObject::native_closure("TypeVarTuple", |args: &[PyObjectRef]| {
            let name = if let Some(a) = args.first() {
                a.py_to_string()
            } else {
                "Ts".into()
            };
            let cls = PyObject::class(CompactString::from("TypeVarTuple"), vec![], IndexMap::new());
            let mut iattrs = IndexMap::new();
            iattrs.insert(
                CompactString::from("__name__"),
                PyObject::str_val(CompactString::from(name.as_str())),
            );
            Ok(PyObject::instance_with_attrs(cls, iattrs))
        }),
    ));
    attrs.push(("Concatenate", make_typing_alias("Concatenate")));

    // Python 3.11+ Required/NotRequired/ReadOnly for TypedDict
    attrs.push(("Required", make_typing_alias("Required")));
    attrs.push(("NotRequired", make_typing_alias("NotRequired")));
    attrs.push(("ReadOnly", make_typing_alias("ReadOnly")));
    attrs.push(("Buffer", make_typing_alias("Buffer")));

    // dataclass_transform — PEP 681
    attrs.push((
        "dataclass_transform",
        make_builtin(|args: &[PyObjectRef]| {
            // @dataclass_transform() decorator — marks a class/function as
            // creating dataclass-like semantics
            if args.is_empty() {
                // Called as @dataclass_transform() — return decorator
                return Ok(make_builtin(|inner_args: &[PyObjectRef]| {
                    if inner_args.is_empty() {
                        return Ok(PyObject::none());
                    }
                    if let PyObjectPayload::Instance(ref d) = inner_args[0].payload {
                        d.attrs.write().insert(
                            CompactString::from("__dataclass_transform__"),
                            PyObject::bool_val(true),
                        );
                    } else if let PyObjectPayload::Class(cd) = &inner_args[0].payload {
                        cd.namespace.write().insert(
                            CompactString::from("__dataclass_transform__"),
                            PyObject::bool_val(true),
                        );
                    }
                    Ok(inner_args[0].clone())
                }));
            }
            // Called as @dataclass_transform (without parens) — apply directly
            if let PyObjectPayload::Class(cd) = &args[0].payload {
                cd.namespace.write().insert(
                    CompactString::from("__dataclass_transform__"),
                    PyObject::bool_val(true),
                );
            }
            Ok(args[0].clone())
        }),
    ));

    // get_overloads / clear_overloads
    attrs.push((
        "get_overloads",
        make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::list(vec![]))),
    ));
    attrs.push((
        "clear_overloads",
        make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
    ));

    // is_typeddict
    attrs.push((
        "is_typeddict",
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::bool_val(false));
            }
            // Check if class has __annotations__ and __total__ (TypedDict markers)
            let obj = &args[0];
            if let PyObjectPayload::Class(cd) = &obj.payload {
                let ns = cd.namespace.read();
                let has = ns.contains_key("__annotations__") && ns.contains_key("__total__");
                return Ok(PyObject::bool_val(has));
            }
            if obj.get_attr("__annotations__").is_some() && obj.get_attr("__total__").is_some() {
                return Ok(PyObject::bool_val(true));
            }
            Ok(PyObject::bool_val(false))
        }),
    ));

    // _GenericAlias — importable class used by typing internals (attrs, pydantic, etc.)
    let generic_alias_cls = {
        let mut ga_ns = IndexMap::new();
        ga_ns.insert(
            CompactString::from("__new__"),
            PyObject::native_closure("_GenericAlias.__new__", |args: &[PyObjectRef]| {
                // _GenericAlias(origin, params) → args[0]=cls, args[1]=origin, args[2]=params
                let origin = if args.len() > 1 {
                    args[1].clone()
                } else {
                    PyObject::none()
                };
                let type_args = if args.len() > 2 {
                    args[2].clone()
                } else {
                    PyObject::tuple(vec![])
                };
                let args_tuple = match &type_args.payload {
                    PyObjectPayload::Tuple(_) => type_args.clone(),
                    _ => PyObject::tuple(vec![type_args]),
                };
                let origin_name = match &origin.payload {
                    PyObjectPayload::Class(cd) => cd.name.to_string(),
                    PyObjectPayload::BuiltinType(n) => n.to_string(),
                    PyObjectPayload::Str(s) => s.to_string(),
                    _ => origin.py_to_string(),
                };
                let args_str = match &args_tuple.payload {
                    PyObjectPayload::Tuple(items) => items
                        .iter()
                        .map(|i| match &i.payload {
                            PyObjectPayload::Class(cd) => cd.name.to_string(),
                            PyObjectPayload::BuiltinType(n) => n.to_string(),
                            _ => i.py_to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join(", "),
                    _ => args_tuple.py_to_string(),
                };
                let repr_str = format!("{}[{}]", origin_name, args_str);

                let inst_cls = PyObject::class(
                    CompactString::from("_GenericAlias"),
                    vec![],
                    IndexMap::new(),
                );
                let mut iattrs = IndexMap::new();
                iattrs.insert(CompactString::from("__origin__"), origin.clone());
                iattrs.insert(CompactString::from("__args__"), args_tuple);
                let rc1 = repr_str.clone();
                iattrs.insert(
                    CompactString::from("__repr__"),
                    PyObject::native_closure("__repr__", move |_| {
                        Ok(PyObject::str_val(CompactString::from(rc1.as_str())))
                    }),
                );
                let rc2 = repr_str;
                iattrs.insert(
                    CompactString::from("__str__"),
                    PyObject::native_closure("__str__", move |_| {
                        Ok(PyObject::str_val(CompactString::from(rc2.as_str())))
                    }),
                );
                // copy_with(new_args) — new alias with same origin but different args
                let origin_cw = origin.clone();
                iattrs.insert(
                    CompactString::from("copy_with"),
                    PyObject::native_closure("copy_with", move |cw_args: &[PyObjectRef]| {
                        let new_a = if cw_args.is_empty() {
                            PyObject::tuple(vec![])
                        } else {
                            cw_args[0].clone()
                        };
                        let new_at = match &new_a.payload {
                            PyObjectPayload::Tuple(_) => new_a,
                            _ => PyObject::tuple(vec![new_a]),
                        };
                        let c = PyObject::class(
                            CompactString::from("_GenericAlias"),
                            vec![],
                            IndexMap::new(),
                        );
                        let mut a = IndexMap::new();
                        a.insert(CompactString::from("__origin__"), origin_cw.clone());
                        a.insert(CompactString::from("__args__"), new_at);
                        Ok(PyObject::instance_with_attrs(c, a))
                    }),
                );
                // __getitem__ for further parameterization
                let origin_gi = origin;
                iattrs.insert(
                    CompactString::from("__getitem__"),
                    PyObject::native_closure("__getitem__", move |gi_args: &[PyObjectRef]| {
                        let params = if gi_args.is_empty() {
                            PyObject::none()
                        } else {
                            gi_args[0].clone()
                        };
                        let new_at = match &params.payload {
                            PyObjectPayload::Tuple(items) => PyObject::tuple((**items).clone()),
                            _ => PyObject::tuple(vec![params]),
                        };
                        let c = PyObject::class(
                            CompactString::from("_GenericAlias"),
                            vec![],
                            IndexMap::new(),
                        );
                        let mut a = IndexMap::new();
                        a.insert(CompactString::from("__origin__"), origin_gi.clone());
                        a.insert(CompactString::from("__args__"), new_at);
                        Ok(PyObject::instance_with_attrs(c, a))
                    }),
                );
                Ok(PyObject::instance_with_attrs(inst_cls, iattrs))
            }),
        );
        PyObject::class(CompactString::from("_GenericAlias"), vec![], ga_ns)
    };
    attrs.push(("_GenericAlias", generic_alias_cls));

    // _SpecialForm — internal marker class used by typing
    attrs.push((
        "_SpecialForm",
        PyObject::class(CompactString::from("_SpecialForm"), vec![], IndexMap::new()),
    ));

    // assert_never — should always raise TypeError at runtime (PEP 782)
    attrs.push((
        "assert_never",
        make_builtin(|args: &[PyObjectRef]| {
            check_args("assert_never", args, 1)?;
            Err(PyException::type_error(format!(
                "Expected code to be unreachable, but got: {}",
                args[0].repr()
            )))
        }),
    ));

    // _type_check — internal helper used by mypy_extensions, typing_extensions
    attrs.push((
        "_type_check",
        make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::none());
            }
            Ok(args[0].clone())
        }),
    ));
    attrs.push(("_GenericForm", make_typing_alias("_GenericForm")));
    attrs.push(("_AnnotatedAlias", make_typing_alias("_AnnotatedAlias")));
    attrs.push((
        "_collect_parameters",
        make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::tuple(vec![]))),
    ));

    // Sentinel — Python 3.13+ typing.Sentinel (PEP 661)
    let sentinel_cls = PyObject::class(CompactString::from("Sentinel"), vec![], IndexMap::new());
    if let PyObjectPayload::Class(ref cd) = sentinel_cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(
            CompactString::from("__init__"),
            make_builtin(|args: &[PyObjectRef]| {
                if args.len() >= 2 {
                    if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                        inst.attrs
                            .write()
                            .insert(CompactString::from("_name"), args[1].clone());
                    }
                }
                Ok(PyObject::none())
            }),
        );
        ns.insert(
            CompactString::from("__repr__"),
            make_builtin(|args: &[PyObjectRef]| {
                let name = args[0]
                    .get_attr("_name")
                    .map(|v| v.py_to_string())
                    .unwrap_or_else(|| "Sentinel".to_string());
                Ok(PyObject::str_val(CompactString::from(name)))
            }),
        );
        ns.insert(
            CompactString::from("__bool__"),
            make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::bool_val(false))),
        );
    }
    attrs.push(("Sentinel", sentinel_cls));

    make_module("typing", attrs)
}

fn resolve_string_annotation(name: &str) -> Option<PyObjectRef> {
    if let Some(attr) = name.strip_prefix("collections.abc.") {
        return crate::type_modules::create_collections_abc_module().get_attr(attr);
    }
    if let Some(attr) = name.strip_prefix("typing.") {
        return create_typing_module().get_attr(attr);
    }
    match name {
        "int" | "str" | "bytes" | "bytearray" | "list" | "tuple" | "dict" | "set" | "frozenset"
        | "bool" | "float" | "complex" | "object" | "type" => {
            Some(PyObject::builtin_type(CompactString::from(name)))
        }
        _ => None,
    }
}
