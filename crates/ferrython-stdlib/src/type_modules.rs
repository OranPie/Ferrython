//! Type-system stdlib modules (typing, abc, enum, types, collections.abc)

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    check_args, check_args_min, make_builtin, make_module, new_fx_hashkey_map, CompareOp,
    FxHashKeyFlatMap, FxHashKeyMap, PyCell, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

fn is_set_like_for_comparison(
    obj: &PyObjectRef,
    set_cls: &PyObjectRef,
    mutable_set_cls: &PyObjectRef,
) -> bool {
    match &obj.payload {
        PyObjectPayload::Set(_)
        | PyObjectPayload::FrozenSet(_)
        | PyObjectPayload::DictKeys { .. }
        | PyObjectPayload::DictItems { .. } => true,
        PyObjectPayload::Instance(inst) => match &inst.class.payload {
            PyObjectPayload::Class(cd) => cd.mro.iter().any(|base| {
                PyObjectRef::ptr_eq(base, set_cls) || PyObjectRef::ptr_eq(base, mutable_set_cls)
            }),
            _ => false,
        },
        _ => false,
    }
}

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
                    Ok(ann)
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

// ── enum module (stub) ──

pub fn create_enum_module() -> PyObjectRef {
    // Create Enum as a proper base class with __getitem__ and __iter__ support
    let mut enum_ns = IndexMap::new();
    enum_ns.insert(CompactString::from("__enum__"), PyObject::bool_val(true));

    // __getitem__ on class — Color['RED'] looks up member by name
    enum_ns.insert(
        CompactString::from("__getitem__"),
        PyObject::native_function("Enum.__getitem__", |args: &[PyObjectRef]| {
            // args[0] = class (self), args[1] = name key
            check_args_min("Enum.__getitem__", args, 2)?;
            let cls = &args[0];
            let name = args[1].py_to_string();
            if let PyObjectPayload::Class(cd) = &cls.payload {
                let ns = cd.namespace.read();
                if let Some(member) = ns.get(name.as_str()) {
                    return Ok(member.clone());
                }
                // Check __members__ dict
                if let Some(members) = ns.get("__members__") {
                    if let PyObjectPayload::Dict(map) = &members.payload {
                        let key = HashableKey::str_key(CompactString::from(name.as_str()));
                        if let Some(member) = map.read().get(&key) {
                            return Ok(member.clone());
                        }
                    }
                }
            }
            Err(PyException::key_error(format!("'{}'", name)))
        }),
    );

    // __call__ on class — Color(1) looks up member by value,
    // OR functional API: Enum("Name", "member1 member2") creates a new enum
    enum_ns.insert(
        CompactString::from("__call__"),
        PyObject::native_function("Enum.__call__", |args: &[PyObjectRef]| {
            check_args_min("Enum.__call__", args, 2)?;
            let cls = &args[0];
            let value = &args[1];

            // Functional API: Enum("Name", "member1 member2") or Enum("Name", ["m1", "m2"])
            if args.len() >= 3 {
                let class_name = value.py_to_string();
                let names_arg = &args[2];
                let member_names: Vec<String> = match &names_arg.payload {
                    PyObjectPayload::Str(s) => {
                        // "member1 member2" or "member1, member2"
                        s.replace(',', " ")
                            .split_whitespace()
                            .map(|s| s.to_string())
                            .collect()
                    }
                    PyObjectPayload::Tuple(items) => items
                        .iter()
                        .map(|i: &PyObjectRef| i.py_to_string())
                        .collect(),
                    PyObjectPayload::List(items) => items
                        .read()
                        .iter()
                        .map(|i: &PyObjectRef| i.py_to_string())
                        .collect(),
                    _ => vec![names_arg.py_to_string()],
                };
                // Create a new class with members
                let mut members_map: FxHashKeyMap = new_fx_hashkey_map();
                let new_cls = PyObject::class(
                    CompactString::from(class_name.as_str()),
                    vec![cls.clone()],
                    IndexMap::new(),
                );
                if let PyObjectPayload::Class(ref cd) = new_cls.payload {
                    let mut ns = cd.namespace.write();
                    for (i, mname) in member_names.iter().enumerate() {
                        let cs_name = CompactString::from(mname.as_str());
                        let mut member_attrs: IndexMap<CompactString, PyObjectRef> =
                            IndexMap::new();
                        member_attrs.insert(
                            CompactString::from("name"),
                            PyObject::str_val(cs_name.clone()),
                        );
                        member_attrs.insert(
                            CompactString::from("_name_"),
                            PyObject::str_val(cs_name.clone()),
                        );
                        member_attrs
                            .insert(CompactString::from("value"), PyObject::int(i as i64 + 1));
                        member_attrs
                            .insert(CompactString::from("_value_"), PyObject::int(i as i64 + 1));
                        let member = PyObject::instance_with_attrs(new_cls.clone(), member_attrs);
                        ns.insert(cs_name.clone(), member.clone());
                        members_map.insert(HashableKey::str_key(cs_name), member);
                    }
                    ns.insert(
                        CompactString::from("__members__"),
                        PyObject::dict(members_map),
                    );
                }
                return Ok(new_cls);
            }

            // Normal call: Color(1) looks up member by value
            if let PyObjectPayload::Class(cd) = &cls.payload {
                let ns = cd.namespace.read();
                if let Some(members) = ns.get("__members__") {
                    if let PyObjectPayload::Dict(map) = &members.payload {
                        for (_, member) in map.read().iter() {
                            if let Some(v) = member.get_attr("value") {
                                if v.py_to_string() == value.py_to_string() {
                                    return Ok(member.clone());
                                }
                            }
                        }
                    }
                }
            }
            Err(PyException::value_error(format!(
                "{} is not a valid enum value",
                value.repr()
            )))
        }),
    );

    // __iter__ on class — list(Color) iterates members
    enum_ns.insert(
        CompactString::from("__iter__"),
        PyObject::native_function("Enum.__iter__", |args: &[PyObjectRef]| {
            check_args_min("Enum.__iter__", args, 1)?;
            let cls = &args[0];
            if let PyObjectPayload::Class(cd) = &cls.payload {
                let ns = cd.namespace.read();
                if let Some(members) = ns.get("__members__") {
                    if let PyObjectPayload::Dict(map) = &members.payload {
                        let items: Vec<PyObjectRef> = map.read().values().cloned().collect();
                        return Ok(PyObject::list(items));
                    }
                }
            }
            Ok(PyObject::list(vec![]))
        }),
    );

    // __len__ on class — len(Color) returns member count
    enum_ns.insert(
        CompactString::from("__len__"),
        PyObject::native_function("Enum.__len__", |args: &[PyObjectRef]| {
            check_args_min("Enum.__len__", args, 1)?;
            let cls = &args[0];
            if let PyObjectPayload::Class(cd) = &cls.payload {
                let ns = cd.namespace.read();
                if let Some(members) = ns.get("__members__") {
                    if let PyObjectPayload::Dict(map) = &members.payload {
                        return Ok(PyObject::int(map.read().len() as i64));
                    }
                }
            }
            Ok(PyObject::int(0))
        }),
    );

    // __contains__ on class — Color.RED in Color
    enum_ns.insert(
        CompactString::from("__contains__"),
        PyObject::native_function("Enum.__contains__", |args: &[PyObjectRef]| {
            check_args_min("Enum.__contains__", args, 2)?;
            let cls = &args[0];
            let item = &args[1];
            if let PyObjectPayload::Class(cd) = &cls.payload {
                let ns = cd.namespace.read();
                if let Some(members) = ns.get("__members__") {
                    if let PyObjectPayload::Dict(map) = &members.payload {
                        for member in map.read().values() {
                            if PyObjectRef::ptr_eq(member, item) {
                                return Ok(PyObject::bool_val(true));
                            }
                            // Also check by value comparison
                            if let (Some(mv), Some(iv)) =
                                (member.get_attr("value"), item.get_attr("value"))
                            {
                                if mv.py_to_string() == iv.py_to_string() {
                                    return Ok(PyObject::bool_val(true));
                                }
                            }
                        }
                    }
                }
            }
            Ok(PyObject::bool_val(false))
        }),
    );

    let enum_class = PyObject::class(CompactString::from("Enum"), vec![], enum_ns);

    // IntEnum — Enum subclass where values are ints and support int operations
    let mut int_enum_ns = IndexMap::new();
    int_enum_ns.insert(CompactString::from("__enum__"), PyObject::bool_val(true));
    int_enum_ns.insert(
        CompactString::from("__int_enum__"),
        PyObject::bool_val(true),
    );

    // Helper: extract int value from an IntEnum member or plain int
    fn int_enum_val(obj: &PyObjectRef) -> Option<i64> {
        if let Some(v) = obj.get_attr("_value_") {
            match &v.payload {
                PyObjectPayload::Int(n) => n.to_i64(),
                _ => None,
            }
        } else {
            match &obj.payload {
                PyObjectPayload::Int(n) => n.to_i64(),
                _ => None,
            }
        }
    }

    // __int__ — convert to int
    int_enum_ns.insert(
        CompactString::from("__int__"),
        PyObject::native_function("IntEnum.__int__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::int(0));
            }
            Ok(PyObject::int(int_enum_val(&args[0]).unwrap_or(0)))
        }),
    );

    // __eq__ — compare with int or another IntEnum member
    int_enum_ns.insert(
        CompactString::from("__eq__"),
        PyObject::native_function("IntEnum.__eq__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__eq__", args, 2)?;
            let a = int_enum_val(&args[0]);
            let b = int_enum_val(&args[1]);
            Ok(PyObject::bool_val(a == b))
        }),
    );

    // __lt__
    int_enum_ns.insert(
        CompactString::from("__lt__"),
        PyObject::native_function("IntEnum.__lt__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__lt__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a < b))
        }),
    );

    // __le__
    int_enum_ns.insert(
        CompactString::from("__le__"),
        PyObject::native_function("IntEnum.__le__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__le__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a <= b))
        }),
    );

    // __gt__
    int_enum_ns.insert(
        CompactString::from("__gt__"),
        PyObject::native_function("IntEnum.__gt__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__gt__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a > b))
        }),
    );

    // __ge__
    int_enum_ns.insert(
        CompactString::from("__ge__"),
        PyObject::native_function("IntEnum.__ge__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__ge__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a >= b))
        }),
    );

    // __add__ — IntEnum + int
    int_enum_ns.insert(
        CompactString::from("__add__"),
        PyObject::native_function("IntEnum.__add__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__add__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a + b))
        }),
    );

    // __sub__ — IntEnum - int
    int_enum_ns.insert(
        CompactString::from("__sub__"),
        PyObject::native_function("IntEnum.__sub__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__sub__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a - b))
        }),
    );

    // __mul__ — IntEnum * int
    int_enum_ns.insert(
        CompactString::from("__mul__"),
        PyObject::native_function("IntEnum.__mul__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__mul__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a * b))
        }),
    );

    let int_enum = PyObject::class(
        CompactString::from("IntEnum"),
        vec![
            enum_class.clone(),
            PyObject::builtin_type(CompactString::from("int")),
        ],
        int_enum_ns,
    );

    // Helper: extract int value from a Flag member or plain int
    fn flag_int_val(obj: &PyObjectRef) -> Option<i64> {
        if let Some(v) = obj.get_attr("value") {
            if let PyObjectPayload::Int(ref i) = v.payload {
                return i.to_i64();
            }
        }
        if let PyObjectPayload::Int(ref i) = obj.payload {
            return i.to_i64();
        }
        if let Some(v) = obj.get_attr("_value_") {
            if let PyObjectPayload::Int(ref i) = v.payload {
                return i.to_i64();
            }
        }
        None
    }

    // Flag — class with bitwise support
    let mut flag_ns = IndexMap::new();
    flag_ns.insert(CompactString::from("__enum__"), PyObject::bool_val(true));
    flag_ns.insert(CompactString::from("__flag__"), PyObject::bool_val(true));

    // __or__ — combine flags with | operator
    flag_ns.insert(
        CompactString::from("__or__"),
        PyObject::native_function("Flag.__or__", |args: &[PyObjectRef]| {
            check_args("Flag.__or__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a | b))
        }),
    );

    // __and__ — bitwise AND of flags
    flag_ns.insert(
        CompactString::from("__and__"),
        PyObject::native_function("Flag.__and__", |args: &[PyObjectRef]| {
            check_args("Flag.__and__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a & b))
        }),
    );

    // __xor__ — bitwise XOR of flags
    flag_ns.insert(
        CompactString::from("__xor__"),
        PyObject::native_function("Flag.__xor__", |args: &[PyObjectRef]| {
            check_args("Flag.__xor__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a ^ b))
        }),
    );

    // __invert__ — bitwise complement
    flag_ns.insert(
        CompactString::from("__invert__"),
        PyObject::native_function("Flag.__invert__", |args: &[PyObjectRef]| {
            check_args("Flag.__invert__", args, 1)?;
            let v = flag_int_val(&args[0]).unwrap_or(0);
            Ok(PyObject::int(!v))
        }),
    );

    // __contains__ — check if one flag contains another (a & b == b)
    flag_ns.insert(
        CompactString::from("__contains__"),
        PyObject::native_function("Flag.__contains__", |args: &[PyObjectRef]| {
            check_args("Flag.__contains__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a & b == b))
        }),
    );

    // __bool__ — Flag(0) is falsy
    flag_ns.insert(
        CompactString::from("__bool__"),
        PyObject::native_function("Flag.__bool__", |args: &[PyObjectRef]| {
            check_args("Flag.__bool__", args, 1)?;
            let v = flag_int_val(&args[0]).unwrap_or(0);
            Ok(PyObject::bool_val(v != 0))
        }),
    );

    // __repr__ — show combined flags in "Flag1|Flag2" format
    flag_ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_function("Flag.__repr__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::str_val(CompactString::from("<Flag>")));
            }
            let self_obj = &args[0];
            let val = flag_int_val(self_obj).unwrap_or(0);
            // Try to get the name directly (single member)
            if let Some(name) = self_obj.get_attr("name") {
                let name_s = name.py_to_string();
                if name_s != "None" && !name_s.is_empty() {
                    // Get class name if available
                    let cls_name = self_obj
                        .get_attr("__class__")
                        .and_then(|c| c.get_attr("__name__"))
                        .map(|n| n.py_to_string())
                        .unwrap_or_else(|| "Flag".to_string());
                    return Ok(PyObject::str_val(CompactString::from(format!(
                        "<{}.{}: {}>",
                        cls_name, name_s, val
                    ))));
                }
            }
            // Combined flags — try to decompose by iterating class members
            if val == 0 {
                let cls_name = self_obj
                    .get_attr("__class__")
                    .and_then(|c| c.get_attr("__name__"))
                    .map(|n| n.py_to_string())
                    .unwrap_or_else(|| "Flag".to_string());
                return Ok(PyObject::str_val(CompactString::from(format!(
                    "<{}: 0>",
                    cls_name
                ))));
            }
            Ok(PyObject::str_val(CompactString::from(format!(
                "<Flag: {}>",
                val
            ))))
        }),
    );

    let flag_class = PyObject::class(
        CompactString::from("Flag"),
        vec![
            enum_class.clone(),
            PyObject::builtin_type(CompactString::from("int")),
        ],
        flag_ns,
    );

    // IntFlag — Flag subclass with int arithmetic support
    let mut int_flag_ns = IndexMap::new();
    int_flag_ns.insert(CompactString::from("__enum__"), PyObject::bool_val(true));
    int_flag_ns.insert(CompactString::from("__flag__"), PyObject::bool_val(true));
    int_flag_ns.insert(
        CompactString::from("__int_enum__"),
        PyObject::bool_val(true),
    );

    // Bitwise ops (duplicated from Flag since Ferrython doesn't do full MRO for class namespaces)
    int_flag_ns.insert(
        CompactString::from("__or__"),
        PyObject::native_function("IntFlag.__or__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__or__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a | b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__and__"),
        PyObject::native_function("IntFlag.__and__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__and__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a & b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__xor__"),
        PyObject::native_function("IntFlag.__xor__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__xor__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a ^ b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__invert__"),
        PyObject::native_function("IntFlag.__invert__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__invert__", args, 1)?;
            let v = flag_int_val(&args[0]).unwrap_or(0);
            Ok(PyObject::int(!v))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__contains__"),
        PyObject::native_function("IntFlag.__contains__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__contains__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a & b == b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__bool__"),
        PyObject::native_function("IntFlag.__bool__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__bool__", args, 1)?;
            let v = flag_int_val(&args[0]).unwrap_or(0);
            Ok(PyObject::bool_val(v != 0))
        }),
    );

    // Int conversion
    int_flag_ns.insert(
        CompactString::from("__int__"),
        PyObject::native_function("IntFlag.__int__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::int(0));
            }
            Ok(PyObject::int(flag_int_val(&args[0]).unwrap_or(0)))
        }),
    );

    // Comparison ops (same as IntEnum)
    int_flag_ns.insert(
        CompactString::from("__eq__"),
        PyObject::native_function("IntFlag.__eq__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__eq__", args, 2)?;
            let a = flag_int_val(&args[0]);
            let b = flag_int_val(&args[1]);
            Ok(PyObject::bool_val(a == b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__lt__"),
        PyObject::native_function("IntFlag.__lt__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__lt__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a < b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__le__"),
        PyObject::native_function("IntFlag.__le__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__le__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a <= b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__gt__"),
        PyObject::native_function("IntFlag.__gt__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__gt__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a > b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__ge__"),
        PyObject::native_function("IntFlag.__ge__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__ge__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a >= b))
        }),
    );

    // Arithmetic ops (IntFlag acts as int)
    int_flag_ns.insert(
        CompactString::from("__add__"),
        PyObject::native_function("IntFlag.__add__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__add__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a + b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__sub__"),
        PyObject::native_function("IntFlag.__sub__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__sub__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a - b))
        }),
    );

    int_flag_ns.insert(
        CompactString::from("__mul__"),
        PyObject::native_function("IntFlag.__mul__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__mul__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a * b))
        }),
    );

    // __repr__ for IntFlag
    int_flag_ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_function("IntFlag.__repr__", |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::str_val(CompactString::from("<IntFlag>")));
            }
            let self_obj = &args[0];
            let val = flag_int_val(self_obj).unwrap_or(0);
            if let Some(name) = self_obj.get_attr("name") {
                let name_s = name.py_to_string();
                if name_s != "None" && !name_s.is_empty() {
                    let cls_name = self_obj
                        .get_attr("__class__")
                        .and_then(|c| c.get_attr("__name__"))
                        .map(|n| n.py_to_string())
                        .unwrap_or_else(|| "IntFlag".to_string());
                    return Ok(PyObject::str_val(CompactString::from(format!(
                        "<{}.{}: {}>",
                        cls_name, name_s, val
                    ))));
                }
            }
            if val == 0 {
                let cls_name = self_obj
                    .get_attr("__class__")
                    .and_then(|c| c.get_attr("__name__"))
                    .map(|n| n.py_to_string())
                    .unwrap_or_else(|| "IntFlag".to_string());
                return Ok(PyObject::str_val(CompactString::from(format!(
                    "<{}: 0>",
                    cls_name
                ))));
            }
            Ok(PyObject::str_val(CompactString::from(format!(
                "<IntFlag: {}>",
                val
            ))))
        }),
    );

    let int_flag_class = PyObject::class(
        CompactString::from("IntFlag"),
        vec![flag_class.clone()],
        int_flag_ns,
    );

    // auto() counter — returns a sentinel that process_enum_class resolves
    static AUTO_COUNTER: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);

    // unique decorator — validates all values in enum are unique
    let unique_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Ok(PyObject::none());
        }
        let cls = &args[0];
        if let PyObjectPayload::Class(cd) = &cls.payload {
            let ns = cd.namespace.read();
            if let Some(members) = ns.get("__members__") {
                if let PyObjectPayload::Dict(map) = &members.payload {
                    let members_map = map.read();
                    let mut seen_values = Vec::new();
                    for (_, member) in members_map.iter() {
                        if let Some(v) = member.get_attr("value") {
                            let val_str = v.py_to_string();
                            if seen_values.contains(&val_str) {
                                return Err(PyException::value_error(format!(
                                    "duplicate values found in enum {}",
                                    cd.name
                                )));
                            }
                            seen_values.push(val_str);
                        }
                    }
                }
            }
        }
        Ok(args[0].clone())
    });

    // StrEnum (Python 3.11+) — enum where members are also strings
    let mut str_enum_ns = IndexMap::new();
    str_enum_ns.insert(CompactString::from("__enum__"), PyObject::bool_val(true));
    str_enum_ns.insert(
        CompactString::from("__str_enum__"),
        PyObject::bool_val(true),
    );
    let str_enum = PyObject::class(
        CompactString::from("StrEnum"),
        vec![
            enum_class.clone(),
            PyObject::builtin_type(CompactString::from("str")),
        ],
        str_enum_ns,
    );

    make_module(
        "enum",
        vec![
            ("Enum", enum_class),
            ("IntEnum", int_enum),
            ("Flag", flag_class),
            ("IntFlag", int_flag_class),
            ("StrEnum", str_enum),
            (
                "auto",
                make_builtin(|_| {
                    // Return a sentinel tuple ("__enum_auto__", counter_value)
                    // process_enum_class will detect this and assign sequential values
                    let val = AUTO_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    Ok(PyObject::tuple(vec![
                        PyObject::str_val(CompactString::from("__enum_auto__")),
                        PyObject::int(val),
                    ]))
                }),
            ),
            ("unique", unique_fn),
            // sentinel — creates a unique sentinel value (Python 3.13+)
            (
                "sentinel",
                make_builtin(|args: &[PyObjectRef]| {
                    let name = if !args.is_empty() {
                        args[0].py_to_string()
                    } else {
                        "MISSING".to_string()
                    };
                    let mut attrs = IndexMap::new();
                    attrs.insert(
                        CompactString::from("_name"),
                        PyObject::str_val(CompactString::from(name.clone())),
                    );
                    attrs.insert(
                        CompactString::from("__repr__"),
                        PyObject::native_closure("sentinel.__repr__", {
                            let n = name.clone();
                            move |_| Ok(PyObject::str_val(CompactString::from(format!("<{}>", n))))
                        }),
                    );
                    attrs.insert(
                        CompactString::from("__bool__"),
                        make_builtin(|_: &[PyObjectRef]| Ok(PyObject::bool_val(false))),
                    );
                    Ok(PyObject::module_with_attrs(
                        CompactString::from(name),
                        attrs,
                    ))
                }),
            ),
        ],
    )
}

// ── types module ──

fn compare_namespaces(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Instance(ref sd), PyObjectPayload::Instance(ref od)) => {
            let sa = sd.attrs.read();
            let oa = od.attrs.read();
            let self_user: Vec<_> = sa.iter().filter(|(k, _)| !k.starts_with('_')).collect();
            let other_user: Vec<_> = oa.iter().filter(|(k, _)| !k.starts_with('_')).collect();
            if self_user.len() != other_user.len() {
                return Ok(PyObject::bool_val(false));
            }
            for (k, v) in &self_user {
                if let Some(ov) = oa.get(*k) {
                    let eq = v.compare(ov, CompareOp::Eq)?;
                    if !eq.is_truthy() {
                        return Ok(PyObject::bool_val(false));
                    }
                } else {
                    return Ok(PyObject::bool_val(false));
                }
            }
            Ok(PyObject::bool_val(true))
        }
        _ => Ok(PyObject::bool_val(false)),
    }
}

pub fn create_types_module() -> PyObjectRef {
    make_module(
        "types",
        vec![
            (
                "NoneType",
                PyObject::builtin_type(CompactString::from("NoneType")),
            ),
            (
                "FunctionType",
                PyObject::builtin_type(CompactString::from("function")),
            ),
            (
                "LambdaType",
                PyObject::builtin_type(CompactString::from("function")),
            ),
            (
                "BuiltinFunctionType",
                PyObject::builtin_type(CompactString::from("builtin_function_or_method")),
            ),
            (
                "BuiltinMethodType",
                PyObject::builtin_type(CompactString::from("builtin_function_or_method")),
            ),
            (
                "MethodType",
                PyObject::builtin_type(CompactString::from("method")),
            ),
            (
                "ModuleType",
                make_builtin(|args| {
                    check_args_min("ModuleType", args, 1)?;
                    let name = args[0].py_to_string();
                    let mut module_attrs = IndexMap::new();
                    if args.len() > 1 {
                        module_attrs.insert(CompactString::from("__doc__"), args[1].clone());
                    } else {
                        module_attrs.insert(CompactString::from("__doc__"), PyObject::none());
                    }
                    Ok(PyObject::module_with_attrs(
                        CompactString::from(name.as_str()),
                        module_attrs,
                    ))
                }),
            ),
            (
                "GeneratorType",
                PyObject::builtin_type(CompactString::from("generator")),
            ),
            (
                "CodeType",
                PyObject::builtin_type(CompactString::from("code")),
            ),
            (
                "FrameType",
                PyObject::builtin_type(CompactString::from("frame")),
            ),
            (
                "TracebackType",
                PyObject::builtin_type(CompactString::from("traceback")),
            ),
            (
                "CoroutineType",
                PyObject::builtin_type(CompactString::from("coroutine")),
            ),
            (
                "AsyncGeneratorType",
                PyObject::builtin_type(CompactString::from("async_generator")),
            ),
            (
                "MappingProxyType",
                PyObject::builtin_type(CompactString::from("mappingproxy")),
            ),
            (
                "GetSetDescriptorType",
                PyObject::builtin_type(CompactString::from("getset_descriptor")),
            ),
            (
                "MemberDescriptorType",
                PyObject::builtin_type(CompactString::from("member_descriptor")),
            ),
            (
                "WrapperDescriptorType",
                PyObject::builtin_type(CompactString::from("wrapper_descriptor")),
            ),
            (
                "MethodWrapperType",
                PyObject::builtin_type(CompactString::from("method-wrapper")),
            ),
            (
                "MethodDescriptorType",
                PyObject::builtin_type(CompactString::from("method_descriptor")),
            ),
            (
                "ClassMethodDescriptorType",
                PyObject::builtin_type(CompactString::from("classmethod_descriptor")),
            ),
            (
                "CellType",
                PyObject::builtin_type(CompactString::from("cell")),
            ),
            (
                "UnionType",
                PyObject::builtin_type(CompactString::from("UnionType")),
            ),
            (
                "EllipsisType",
                PyObject::builtin_type(CompactString::from("ellipsis")),
            ),
            (
                "NotImplementedType",
                PyObject::builtin_type(CompactString::from("NotImplementedType")),
            ),
            (
                "SimpleNamespace",
                make_builtin(|args| {
                    // Build class-level __repr__ and __eq__ so the VM can dispatch them
                    let mut methods = IndexMap::new();
                    methods.insert(
                        CompactString::from("__repr__"),
                        PyObject::native_closure(
                            "SimpleNamespace.__repr__",
                            |repr_args: &[PyObjectRef]| {
                                if repr_args.is_empty() {
                                    return Ok(PyObject::str_val(CompactString::from(
                                        "namespace()",
                                    )));
                                }
                                let self_obj = &repr_args[0];
                                if let PyObjectPayload::Instance(ref d) = self_obj.payload {
                                    let attrs = d.attrs.read();
                                    let parts: Vec<String> = attrs
                                        .iter()
                                        .filter(|(k, _)| !k.starts_with('_'))
                                        .map(|(k, v)| format!("{}={}", k, v.repr()))
                                        .collect();
                                    Ok(PyObject::str_val(CompactString::from(format!(
                                        "namespace({})",
                                        parts.join(", ")
                                    ))))
                                } else {
                                    Ok(PyObject::str_val(CompactString::from("namespace()")))
                                }
                            },
                        ),
                    );
                    methods.insert(
                        CompactString::from("__eq__"),
                        PyObject::native_closure(
                            "SimpleNamespace.__eq__",
                            |eq_args: &[PyObjectRef]| {
                                // When called via == operator: args = [self, other]
                                // When called via ns1.__eq__(ns2): args = [ns2] (no self)
                                // We handle the 2-arg case here; 1-arg case handled at instance level
                                if eq_args.len() < 2 {
                                    return Ok(PyObject::bool_val(false));
                                }
                                compare_namespaces(&eq_args[0], &eq_args[1])
                            },
                        ),
                    );
                    let cls =
                        PyObject::class(CompactString::from("SimpleNamespace"), vec![], methods);
                    let inst = PyObject::instance(cls);
                    if let Some(last) = args.last() {
                        if let PyObjectPayload::Dict(kw) = &last.payload {
                            if let PyObjectPayload::Instance(ref d) = inst.payload {
                                let mut attrs = d.attrs.write();
                                for (k, v) in kw.read().iter() {
                                    if let HashableKey::Str(s) = k {
                                        attrs.insert(s.to_compact_string(), v.clone());
                                    }
                                }
                            }
                        }
                    }
                    // Install per-instance __eq__ capturing self for ns.__eq__(other) calls
                    if let PyObjectPayload::Instance(ref d) = inst.payload {
                        let self_ref = d.attrs.clone();
                        let mut attrs = d.attrs.write();
                        attrs.insert(
                            CompactString::from("__eq__"),
                            PyObject::native_closure(
                                "SimpleNamespace.__eq__",
                                move |eq_args: &[PyObjectRef]| {
                                    if eq_args.is_empty() {
                                        return Ok(PyObject::bool_val(false));
                                    }
                                    // When called as ns.__eq__(other), eq_args[0] is other
                                    // Build a fake self from our captured attrs
                                    let other = &eq_args[eq_args.len() - 1];
                                    if let PyObjectPayload::Instance(ref od) = other.payload {
                                        let sa = self_ref.read();
                                        let oa = od.attrs.read();
                                        let self_user: Vec<_> = sa
                                            .iter()
                                            .filter(|(k, _)| !k.starts_with('_'))
                                            .collect();
                                        let other_user: Vec<_> = oa
                                            .iter()
                                            .filter(|(k, _)| !k.starts_with('_'))
                                            .collect();
                                        if self_user.len() != other_user.len() {
                                            return Ok(PyObject::bool_val(false));
                                        }
                                        for (k, v) in &self_user {
                                            if let Some(ov) = oa.get(*k) {
                                                let eq = v.compare(ov, CompareOp::Eq)?;
                                                if !eq.is_truthy() {
                                                    return Ok(PyObject::bool_val(false));
                                                }
                                            } else {
                                                return Ok(PyObject::bool_val(false));
                                            }
                                        }
                                        Ok(PyObject::bool_val(true))
                                    } else {
                                        Ok(PyObject::bool_val(false))
                                    }
                                },
                            ),
                        );
                    }
                    Ok(inst)
                }),
            ),
            (
                "new_class",
                make_builtin(|args| {
                    check_args_min("new_class", args, 1)?;
                    let name = args[0].py_to_string();
                    let bases = if args.len() > 1 {
                        args[1].to_list().unwrap_or_default()
                    } else {
                        vec![]
                    };
                    Ok(PyObject::class(
                        CompactString::from(&name),
                        bases,
                        IndexMap::new(),
                    ))
                }),
            ),
            (
                "prepare_class",
                make_builtin(|_| {
                    Ok(PyObject::tuple(vec![
                        PyObject::none(),
                        PyObject::dict(IndexMap::new()),
                        PyObject::dict(IndexMap::new()),
                    ]))
                }),
            ),
            (
                "DynamicClassAttribute",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Ok(PyObject::none());
                    }
                    Ok(args[0].clone())
                }),
            ),
            (
                "coroutine",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Ok(PyObject::none());
                    }
                    Ok(args[0].clone())
                }),
            ),
        ],
    )
}

// ── collections.abc module ──

pub fn create_collections_abc_module() -> PyObjectRef {
    let make_abc = |name: &str,
                    builtin_types: &[&str],
                    bases: Vec<PyObjectRef>,
                    abstract_methods: &[&str]|
     -> PyObjectRef {
        let mut ns = IndexMap::new();
        let mut abstract_set = IndexMap::new();
        for method in abstract_methods {
            let key = HashableKey::str_key(CompactString::from(*method));
            abstract_set.insert(key, PyObject::str_val(CompactString::from(*method)));
        }
        ns.insert(
            CompactString::from("__abstractmethods__"),
            PyObject::set(abstract_set),
        );
        if !builtin_types.is_empty() {
            let mut type_set = IndexMap::new();
            for t in builtin_types {
                let key = ferrython_core::types::HashableKey::str_key(CompactString::from(*t));
                type_set.insert(key, PyObject::str_val(CompactString::from(*t)));
            }
            ns.insert(
                CompactString::from("_abc_builtin_types"),
                PyObject::set(type_set),
            );
        }
        let cls = PyObject::class(CompactString::from(name), bases, ns);
        // Add register() method so ABCs support Mapping.register(MyClass)
        if let PyObjectPayload::Class(ref cd) = cls.payload {
            let cls_ref = cls.clone();
            let register_fn = PyObject::native_closure(
                &format!("{}.register", name),
                move |args: &[PyObjectRef]| {
                    let subclass = if args.is_empty() {
                        return Err(PyException::type_error(
                            "register() requires a subclass argument",
                        ));
                    } else {
                        args.last().unwrap().clone()
                    };
                    if let PyObjectPayload::Class(ref cd) = cls_ref.payload {
                        let mut ns = cd.namespace.write();
                        let registry = ns
                            .entry(CompactString::from("_abc_registry"))
                            .or_insert_with(|| PyObject::dict(IndexMap::new()))
                            .clone();
                        if let PyObjectPayload::Dict(ref map) = registry.payload {
                            let ptr = PyObjectRef::as_ptr(&subclass) as usize;
                            map.write().insert(
                                HashableKey::Identity(ptr, subclass.clone()),
                                PyObject::bool_val(true),
                            );
                        }
                    }
                    if let PyObjectPayload::Class(ref cd) = subclass.payload {
                        cd.namespace
                            .write()
                            .insert(CompactString::from("__abc_registered__"), cls_ref.clone());
                    }
                    Ok(subclass)
                },
            );
            cd.namespace
                .write()
                .insert(CompactString::from("register"), register_fn);
        }
        cls
    };

    let hashable_cls = make_abc(
        "Hashable",
        &[
            "int",
            "float",
            "complex",
            "str",
            "bool",
            "bytes",
            "tuple",
            "frozenset",
            "NoneType",
            "type",
        ],
        vec![],
        &["__hash__"],
    );
    let iterable_cls = make_abc(
        "Iterable",
        &[
            "list",
            "tuple",
            "dict",
            "set",
            "frozenset",
            "str",
            "bytes",
            "bytearray",
            "range",
            "dict_keys",
            "dict_items",
            "dict_values",
            "iterator",
            "generator",
            "str_ascii_iterator",
            "bytes_iterator",
            "bytearray_iterator",
            "range_iterator",
            "list_iterator",
            "tuple_iterator",
            "dict_keyiterator",
            "dict_valueiterator",
            "dict_itemiterator",
            "list_reverseiterator",
        ],
        vec![],
        &["__iter__"],
    );
    let iterator_cls = make_abc(
        "Iterator",
        &[
            "iterator",
            "generator",
            "str_ascii_iterator",
            "bytes_iterator",
            "bytearray_iterator",
            "range_iterator",
            "list_iterator",
            "tuple_iterator",
            "dict_keyiterator",
            "dict_valueiterator",
            "dict_itemiterator",
            "list_reverseiterator",
        ],
        vec![iterable_cls.clone()],
        &["__iter__", "__next__"],
    );
    let reversible_cls = make_abc(
        "Reversible",
        &[
            "list",
            "tuple",
            "str",
            "dict",
            "bytes",
            "bytearray",
            "range",
            "dict_keys",
            "dict_items",
            "dict_values",
            "OrderedDict",
            "Counter",
        ],
        vec![iterable_cls.clone()],
        &["__iter__", "__reversed__"],
    );
    let generator_cls = make_abc(
        "Generator",
        &["generator"],
        vec![iterator_cls.clone()],
        &["__iter__", "__next__", "send", "throw", "close"],
    );
    let sized_cls = make_abc(
        "Sized",
        &[
            "list",
            "tuple",
            "dict",
            "set",
            "frozenset",
            "str",
            "bytes",
            "bytearray",
            "range",
            "dict_keys",
            "dict_items",
            "dict_values",
        ],
        vec![],
        &["__len__"],
    );
    let container_cls = make_abc(
        "Container",
        &[
            "list",
            "tuple",
            "dict",
            "set",
            "frozenset",
            "str",
            "bytes",
            "bytearray",
            "range",
            "dict_keys",
            "dict_items",
        ],
        vec![],
        &["__contains__"],
    );
    let callable_cls = make_abc(
        "Callable",
        &[
            "function",
            "builtin_function_or_method",
            "builtin_method",
            "method",
            "method_descriptor",
            "wrapper_descriptor",
            "type",
        ],
        vec![],
        &["__call__"],
    );
    let collection_cls = make_abc(
        "Collection",
        &[
            "list",
            "tuple",
            "dict",
            "set",
            "frozenset",
            "str",
            "bytes",
            "bytearray",
            "range",
            "dict_keys",
            "dict_items",
            "dict_values",
        ],
        vec![
            sized_cls.clone(),
            iterable_cls.clone(),
            container_cls.clone(),
        ],
        &["__len__", "__iter__", "__contains__"],
    );
    let sequence_cls = make_abc(
        "Sequence",
        &[
            "list",
            "tuple",
            "str",
            "bytes",
            "bytearray",
            "range",
            "memoryview",
        ],
        vec![reversible_cls.clone(), collection_cls.clone()],
        &["__getitem__", "__len__"],
    );
    let mutable_sequence_cls = make_abc(
        "MutableSequence",
        &["list", "bytearray", "deque"],
        vec![sequence_cls.clone()],
        &[
            "__getitem__",
            "__len__",
            "__setitem__",
            "__delitem__",
            "insert",
        ],
    );
    let bytestring_cls = make_abc(
        "ByteString",
        &["bytes", "bytearray"],
        vec![sequence_cls.clone()],
        &["__getitem__", "__len__"],
    );
    let set_cls = make_abc(
        "Set",
        &["set", "frozenset", "dict_keys", "dict_items"],
        vec![collection_cls.clone()],
        &["__contains__", "__iter__", "__len__"],
    );
    let mutable_set_cls = make_abc(
        "MutableSet",
        &["set"],
        vec![set_cls.clone()],
        &["__contains__", "__iter__", "__len__", "add", "discard"],
    );
    let mapping_cls = make_abc(
        "Mapping",
        &["dict", "Counter", "UserDict"],
        vec![collection_cls.clone()],
        &["__getitem__", "__iter__", "__len__"],
    );
    let mutable_mapping_cls = make_abc(
        "MutableMapping",
        &["dict", "Counter", "UserDict"],
        vec![mapping_cls.clone()],
        &[
            "__getitem__",
            "__iter__",
            "__len__",
            "__setitem__",
            "__delitem__",
        ],
    );
    let mapping_view_cls = make_abc("MappingView", &[], vec![sized_cls.clone()], &[]);
    let keys_view_cls = make_abc(
        "KeysView",
        &["dict_keys"],
        vec![mapping_view_cls.clone(), set_cls.clone()],
        &[],
    );
    let items_view_cls = make_abc(
        "ItemsView",
        &["dict_items"],
        vec![mapping_view_cls.clone(), set_cls.clone()],
        &[],
    );
    let values_view_cls = make_abc(
        "ValuesView",
        &["dict_values"],
        vec![mapping_view_cls.clone(), collection_cls.clone()],
        &[],
    );
    let awaitable_cls = make_abc("Awaitable", &["coroutine"], vec![], &["__await__"]);
    let coroutine_cls = make_abc(
        "Coroutine",
        &["coroutine"],
        vec![awaitable_cls.clone()],
        &["send", "throw", "close", "__await__"],
    );
    let async_iterable_cls = make_abc(
        "AsyncIterable",
        &["async_generator"],
        vec![],
        &["__aiter__"],
    );
    let async_iterator_cls = make_abc(
        "AsyncIterator",
        &["async_generator"],
        vec![async_iterable_cls.clone()],
        &["__aiter__", "__anext__"],
    );
    let async_generator_cls = make_abc(
        "AsyncGenerator",
        &["async_generator"],
        vec![async_iterator_cls.clone()],
        &["__aiter__", "__anext__", "asend", "athrow"],
    );
    let buffer_cls = make_abc("Buffer", &["bytes", "bytearray", "memoryview"], vec![], &[]);
    let set_cls_for_compare = set_cls.clone();
    let mutable_set_cls_for_compare = mutable_set_cls.clone();

    let add_method = |cls: &PyObjectRef, name: &str, func: PyObjectRef| {
        if let PyObjectPayload::Class(cd) = &cls.payload {
            cd.namespace.write().insert(CompactString::from(name), func);
        }
    };
    let drop_abstract = |cls: &PyObjectRef, names: &[&str]| {
        if let PyObjectPayload::Class(cd) = &cls.payload {
            let mut ns = cd.namespace.write();
            if let Some(abs) = ns.get("__abstractmethods__").cloned() {
                let new_abs = match &abs.payload {
                    PyObjectPayload::Set(set) => {
                        let mut w = set.read().clone();
                        for name in names {
                            w.remove(&HashableKey::str_key(CompactString::from(*name)));
                        }
                        PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(w))))
                    }
                    PyObjectPayload::FrozenSet(set) => {
                        let mut w = set.items.clone();
                        for name in names {
                            w.remove(&HashableKey::str_key(CompactString::from(*name)));
                        }
                        PyObject::frozenset(w)
                    }
                    PyObjectPayload::Tuple(items) => {
                        let filtered: Vec<_> = items
                            .iter()
                            .filter(|item| !names.iter().any(|name| item.py_to_string() == *name))
                            .cloned()
                            .collect();
                        PyObject::tuple(filtered)
                    }
                    PyObjectPayload::List(items) => {
                        let filtered: Vec<_> = items
                            .read()
                            .iter()
                            .filter(|item| !names.iter().any(|name| item.py_to_string() == *name))
                            .cloned()
                            .collect();
                        PyObject::list(filtered)
                    }
                    _ => abs.clone(),
                };
                ns.insert(CompactString::from("__abstractmethods__"), new_abs);
            }
        }
    };

    let make_index_iterator = |obj: &PyObjectRef, reverse: bool| -> PyResult<PyObjectRef> {
        let len = obj.py_len()? as i64;
        let mut items = Vec::new();
        if reverse {
            for i in (0..len).rev() {
                items.push(obj.get_item(&PyObject::int(i))?);
            }
        } else {
            for i in 0..len {
                items.push(obj.get_item(&PyObject::int(i))?);
            }
        }
        Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
            PyCell::new(ferrython_core::object::IteratorData::List { items, index: 0 }),
        ))))
    };

    let make_set_items = |obj: &PyObjectRef| -> PyResult<Vec<PyObjectRef>> { obj.to_list() };

    add_method(
        &sequence_cls,
        "__contains__",
        PyObject::native_closure("Sequence.__contains__", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let self_obj = &args[0];
            let target = &args[1];
            let len = self_obj.py_len()? as i64;
            for i in 0..len {
                let item = self_obj.get_item(&PyObject::int(i))?;
                if item
                    .compare(target, CompareOp::Eq)
                    .map(|v| v.is_truthy())
                    .unwrap_or(false)
                {
                    return Ok(PyObject::bool_val(true));
                }
            }
            Ok(PyObject::bool_val(false))
        }),
    );
    add_method(
        &sequence_cls,
        "__iter__",
        PyObject::native_closure("Sequence.__iter__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("Sequence.__iter__ requires self"));
            }
            make_index_iterator(&args[0], false)
        }),
    );
    add_method(
        &sequence_cls,
        "__reversed__",
        PyObject::native_closure("Sequence.__reversed__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "Sequence.__reversed__ requires self",
                ));
            }
            make_index_iterator(&args[0], true)
        }),
    );
    add_method(
        &sequence_cls,
        "index",
        PyObject::native_closure("Sequence.index", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("index() requires 1 argument"));
            }
            let self_obj = &args[0];
            let target = &args[1];
            let len = self_obj.py_len()? as i64;
            let start = if args.len() > 2 {
                args[2].to_int().unwrap_or(0)
            } else {
                0
            };
            let stop = if args.len() > 3 {
                args[3].to_int().unwrap_or(len)
            } else {
                len
            };
            let start = if start < 0 {
                (len + start).max(0)
            } else {
                start
            }
            .min(len);
            let stop = if stop < 0 { (len + stop).max(0) } else { stop }.min(len);
            for i in start..stop {
                let item = self_obj.get_item(&PyObject::int(i))?;
                if item
                    .compare(target, CompareOp::Eq)
                    .map(|v| v.is_truthy())
                    .unwrap_or(false)
                {
                    return Ok(PyObject::int(i));
                }
            }
            Err(PyException::value_error(format!(
                "{} is not in sequence",
                target.py_to_string()
            )))
        }),
    );
    add_method(
        &sequence_cls,
        "count",
        PyObject::native_closure("Sequence.count", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("count() requires 1 argument"));
            }
            let self_obj = &args[0];
            let target = &args[1];
            let len = self_obj.py_len()? as i64;
            let mut count = 0i64;
            for i in 0..len {
                let item = self_obj.get_item(&PyObject::int(i))?;
                if item
                    .compare(target, CompareOp::Eq)
                    .map(|v| v.is_truthy())
                    .unwrap_or(false)
                {
                    count += 1;
                }
            }
            Ok(PyObject::int(count))
        }),
    );

    add_method(
        &mutable_sequence_cls,
        "append",
        PyObject::native_closure("MutableSequence.append", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("append() requires 1 argument"));
            }
            let self_obj = &args[0];
            let insert = self_obj
                .get_attr("insert")
                .ok_or_else(|| PyException::attribute_error("insert"))?;
            let len = self_obj.py_len()? as i64;
            ferrython_core::object::helpers::call_callable(
                &insert,
                &[PyObject::int(len), args[1].clone()],
            )?;
            Ok(PyObject::none())
        }),
    );
    add_method(
        &mutable_sequence_cls,
        "extend",
        PyObject::native_closure("MutableSequence.extend", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("extend() requires 1 argument"));
            }
            let self_obj = &args[0];
            let insert = self_obj
                .get_attr("insert")
                .ok_or_else(|| PyException::attribute_error("insert"))?;
            let mut idx = self_obj.py_len()? as i64;
            for item in args[1].to_list()? {
                ferrython_core::object::helpers::call_callable(
                    &insert,
                    &[PyObject::int(idx), item],
                )?;
                idx += 1;
            }
            Ok(PyObject::none())
        }),
    );
    add_method(
        &mutable_sequence_cls,
        "pop",
        PyObject::native_closure("MutableSequence.pop", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("pop() requires self"));
            }
            let self_obj = &args[0];
            let len = self_obj.py_len()? as i64;
            if len == 0 {
                return Err(PyException::index_error("pop from empty list"));
            }
            let idx = if args.len() > 1 {
                args[1].to_int().unwrap_or(-1)
            } else {
                -1
            };
            let actual = if idx < 0 { len + idx } else { idx };
            if actual < 0 || actual >= len {
                return Err(PyException::index_error("pop index out of range"));
            }
            let item = self_obj.get_item(&PyObject::int(actual))?;
            let del = self_obj
                .get_attr("__delitem__")
                .ok_or_else(|| PyException::attribute_error("__delitem__"))?;
            ferrython_core::object::helpers::call_callable(&del, &[PyObject::int(actual)])?;
            Ok(item)
        }),
    );
    add_method(
        &mutable_sequence_cls,
        "remove",
        PyObject::native_closure("MutableSequence.remove", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("remove() requires 1 argument"));
            }
            let self_obj = &args[0];
            let del = self_obj
                .get_attr("__delitem__")
                .ok_or_else(|| PyException::attribute_error("__delitem__"))?;
            let len = self_obj.py_len()? as i64;
            for i in 0..len {
                let item = self_obj.get_item(&PyObject::int(i))?;
                if item
                    .compare(&args[1], CompareOp::Eq)
                    .map(|v| v.is_truthy())
                    .unwrap_or(false)
                {
                    ferrython_core::object::helpers::call_callable(&del, &[PyObject::int(i)])?;
                    return Ok(PyObject::none());
                }
            }
            Err(PyException::value_error("list.remove(x): x not in list"))
        }),
    );
    add_method(
        &mutable_sequence_cls,
        "clear",
        PyObject::native_closure("MutableSequence.clear", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("clear() requires self"));
            }
            let self_obj = &args[0];
            let pop = self_obj
                .get_attr("pop")
                .ok_or_else(|| PyException::attribute_error("pop"))?;
            while self_obj.py_len()? > 0 {
                ferrython_core::object::helpers::call_callable(&pop, &[])?;
            }
            Ok(PyObject::none())
        }),
    );
    add_method(
        &mutable_sequence_cls,
        "reverse",
        PyObject::native_closure("MutableSequence.reverse", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("reverse() requires self"));
            }
            let self_obj = &args[0];
            let len = self_obj.py_len()? as i64;
            let setitem = self_obj
                .get_attr("__setitem__")
                .ok_or_else(|| PyException::attribute_error("__setitem__"))?;
            let mut items = Vec::new();
            for i in 0..len {
                items.push(self_obj.get_item(&PyObject::int(i))?);
            }
            for (i, item) in items.into_iter().rev().enumerate() {
                ferrython_core::object::helpers::call_callable(
                    &setitem,
                    &[PyObject::int(i as i64), item],
                )?;
            }
            Ok(PyObject::none())
        }),
    );
    add_method(
        &mutable_sequence_cls,
        "__iadd__",
        PyObject::native_closure("MutableSequence.__iadd__", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("__iadd__ requires other"));
            }
            let self_obj = &args[0];
            let extend = self_obj
                .get_attr("extend")
                .ok_or_else(|| PyException::attribute_error("extend"))?;
            ferrython_core::object::helpers::call_callable(&extend, &[args[1].clone()])?;
            Ok(self_obj.clone())
        }),
    );

    let make_set_like = |cls: &PyObjectRef| {
        let op_impl = |name: &'static str, reflected: bool| {
            let set_cls_for_compare = set_cls_for_compare.clone();
            let mutable_set_cls_for_compare = mutable_set_cls_for_compare.clone();
            PyObject::native_closure(name, move |args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Ok(PyObject::not_implemented());
                }
                let (left, right) = if reflected {
                    (&args[1], &args[0])
                } else {
                    (&args[0], &args[1])
                };
                if matches!(name, "__le__" | "__lt__" | "__ge__" | "__gt__")
                    && (!is_set_like_for_comparison(
                        left,
                        &set_cls_for_compare,
                        &mutable_set_cls_for_compare,
                    ) || !is_set_like_for_comparison(
                        right,
                        &set_cls_for_compare,
                        &mutable_set_cls_for_compare,
                    ))
                {
                    return Ok(PyObject::not_implemented());
                }
                let left_items = match make_set_items(left) {
                    Ok(items) => items,
                    Err(_) => return Ok(PyObject::not_implemented()),
                };
                let right_items = match make_set_items(right) {
                    Ok(items) => items,
                    Err(_) => return Ok(PyObject::not_implemented()),
                };
                let right_keys: std::collections::HashSet<_> = right_items
                    .iter()
                    .filter_map(|x| x.to_hashable_key().ok())
                    .collect();
                let left_keys: std::collections::HashSet<_> = left_items
                    .iter()
                    .filter_map(|x| x.to_hashable_key().ok())
                    .collect();
                match name {
                    "__le__" => Ok(PyObject::bool_val(left_keys.is_subset(&right_keys))),
                    "__lt__" => Ok(PyObject::bool_val(
                        left_keys.len() < right_keys.len() && left_keys.is_subset(&right_keys),
                    )),
                    "__ge__" => Ok(PyObject::bool_val(left_keys.is_superset(&right_keys))),
                    "__gt__" => Ok(PyObject::bool_val(
                        left_keys.len() > right_keys.len() && left_keys.is_superset(&right_keys),
                    )),
                    "__and__" | "__rand__" => {
                        if reflected
                            && left_items.is_empty()
                            && !matches!(
                                &left.payload,
                                PyObjectPayload::Set(_)
                                    | PyObjectPayload::FrozenSet(_)
                                    | PyObjectPayload::DictKeys { .. }
                                    | PyObjectPayload::DictItems { .. }
                            )
                        {
                            return Ok(PyObject::not_implemented());
                        }
                        let mut result = Vec::new();
                        for item in &left_items {
                            if let Ok(hk) = item.to_hashable_key() {
                                if right_keys.contains(&hk) {
                                    result.push(item.clone());
                                }
                            }
                        }
                        let flat: FxHashKeyFlatMap = result
                            .into_iter()
                            .filter_map(|item| item.to_hashable_key().ok().map(|hk| (hk, item)))
                            .collect();
                        Ok(PyObject::set_from_flatmap(flat))
                    }
                    "__or__" | "__ror__" => {
                        let mut result: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
                        for item in left_items.iter().chain(right_items.iter()) {
                            if let Ok(hk) = item.to_hashable_key() {
                                result.entry(hk).or_insert_with(|| item.clone());
                            }
                        }
                        let flat: FxHashKeyFlatMap = result.into_iter().collect();
                        Ok(PyObject::set_from_flatmap(flat))
                    }
                    "__sub__" | "__rsub__" => {
                        let mut result = Vec::new();
                        for item in &left_items {
                            if let Ok(hk) = item.to_hashable_key() {
                                if !right_keys.contains(&hk) {
                                    result.push(item.clone());
                                }
                            }
                        }
                        let flat: FxHashKeyFlatMap = result
                            .into_iter()
                            .filter_map(|item| item.to_hashable_key().ok().map(|hk| (hk, item)))
                            .collect();
                        Ok(PyObject::set_from_flatmap(flat))
                    }
                    "__xor__" | "__rxor__" => {
                        let mut result: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
                        for item in &left_items {
                            if let Ok(hk) = item.to_hashable_key() {
                                if !right_keys.contains(&hk) {
                                    result.insert(hk, item.clone());
                                }
                            }
                        }
                        for item in &right_items {
                            if let Ok(hk) = item.to_hashable_key() {
                                if !left_keys.contains(&hk) {
                                    result.insert(hk, item.clone());
                                }
                            }
                        }
                        let flat: FxHashKeyFlatMap = result.into_iter().collect();
                        Ok(PyObject::set_from_flatmap(flat))
                    }
                    _ => Ok(PyObject::not_implemented()),
                }
            })
        };
        add_method(cls, "__le__", op_impl("__le__", false));
        add_method(cls, "__lt__", op_impl("__lt__", false));
        add_method(cls, "__ge__", op_impl("__ge__", false));
        add_method(cls, "__gt__", op_impl("__gt__", false));
        add_method(cls, "__and__", op_impl("__and__", false));
        add_method(cls, "__rand__", op_impl("__and__", true));
        add_method(cls, "__or__", op_impl("__or__", false));
        add_method(cls, "__ror__", op_impl("__or__", true));
        add_method(cls, "__sub__", op_impl("__sub__", false));
        add_method(cls, "__rsub__", op_impl("__sub__", true));
        add_method(cls, "__xor__", op_impl("__xor__", false));
        add_method(cls, "__rxor__", op_impl("__xor__", true));
        add_method(
            cls,
            "isdisjoint",
            PyObject::native_closure("Set.isdisjoint", move |args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Err(PyException::type_error("isdisjoint() requires 1 argument"));
                }
                let left_items = make_set_items(&args[0])?;
                let right_items = match make_set_items(&args[1]) {
                    Ok(items) => items,
                    Err(_) => return Ok(PyObject::not_implemented()),
                };
                let left_keys: std::collections::HashSet<_> = left_items
                    .iter()
                    .filter_map(|x| x.to_hashable_key().ok())
                    .collect();
                let disjoint = right_items
                    .iter()
                    .filter_map(|x| x.to_hashable_key().ok())
                    .all(|hk| !left_keys.contains(&hk));
                Ok(PyObject::bool_val(disjoint))
            }),
        );
    };
    make_set_like(&set_cls);
    make_set_like(&mutable_set_cls);

    add_method(
        &generator_cls,
        "__iter__",
        PyObject::native_closure("Generator.__iter__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("Generator.__iter__ requires self"));
            }
            Ok(args[0].clone())
        }),
    );
    drop_abstract(&generator_cls, &["__iter__", "__next__", "close"]);
    add_method(
        &generator_cls,
        "__next__",
        PyObject::native_closure("Generator.__next__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("Generator.__next__ requires self"));
            }
            let send = args[0]
                .get_attr("send")
                .ok_or_else(|| PyException::attribute_error("send"))?;
            ferrython_core::object::helpers::call_callable(&send, &[PyObject::none()])
        }),
    );
    add_method(
        &generator_cls,
        "close",
        PyObject::native_closure("Generator.close", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("Generator.close requires self"));
            }
            let throw = args[0]
                .get_attr("throw")
                .ok_or_else(|| PyException::attribute_error("throw"))?;
            let gen_exit = PyObject::exception_type(ExceptionKind::GeneratorExit);
            let _ = ferrython_core::object::helpers::call_callable(&throw, &[gen_exit]);
            Ok(PyObject::none())
        }),
    );
    drop_abstract(&coroutine_cls, &["close"]);
    add_method(
        &coroutine_cls,
        "close",
        PyObject::native_closure("Coroutine.close", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("Coroutine.close requires self"));
            }
            let throw = args[0]
                .get_attr("throw")
                .ok_or_else(|| PyException::attribute_error("throw"))?;
            let gen_exit = PyObject::exception_type(ExceptionKind::GeneratorExit);
            let _ = ferrython_core::object::helpers::call_callable(&throw, &[gen_exit]);
            Ok(PyObject::none())
        }),
    );
    add_method(
        &async_iterator_cls,
        "__aiter__",
        PyObject::native_closure("AsyncIterator.__aiter__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "AsyncIterator.__aiter__ requires self",
                ));
            }
            Ok(args[0].clone())
        }),
    );
    add_method(
        &async_generator_cls,
        "__aiter__",
        PyObject::native_closure("AsyncGenerator.__aiter__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "AsyncGenerator.__aiter__ requires self",
                ));
            }
            Ok(args[0].clone())
        }),
    );
    drop_abstract(&async_generator_cls, &["__aiter__", "__anext__"]);
    add_method(
        &async_generator_cls,
        "__anext__",
        PyObject::native_closure("AsyncGenerator.__anext__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "AsyncGenerator.__anext__ requires self",
                ));
            }
            let asend = args[0]
                .get_attr("asend")
                .ok_or_else(|| PyException::attribute_error("asend"))?;
            ferrython_core::object::helpers::call_callable(&asend, &[PyObject::none()])
        }),
    );
    add_method(
        &async_generator_cls,
        "asend",
        PyObject::native_closure("AsyncGenerator.asend", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("asend() requires value"));
            }
            Ok(PyObject::builtin_awaitable(args[1].clone()))
        }),
    );
    add_method(
        &async_generator_cls,
        "athrow",
        PyObject::native_closure("AsyncGenerator.athrow", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Err(PyException::type_error("athrow() requires an exception"));
            }
            let typ_name = args[1].type_name();
            if typ_name == "GeneratorExit" {
                Err(PyException::new(
                    ExceptionKind::GeneratorExit,
                    String::new(),
                ))
            } else {
                Err(PyException::value_error(args[1].py_to_string()))
            }
        }),
    );
    add_method(
        &async_generator_cls,
        "aclose",
        PyObject::native_closure("AsyncGenerator.aclose", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "AsyncGenerator.aclose requires self",
                ));
            }
            let athrow = args[0]
                .get_attr("athrow")
                .ok_or_else(|| PyException::attribute_error("athrow"))?;
            let gen_exit = PyObject::exception_type(ExceptionKind::GeneratorExit);
            ferrython_core::object::helpers::call_callable(&athrow, &[gen_exit])
        }),
    );

    let make_mapping_view = |cls: &PyObjectRef, kind: &'static str| {
        add_method(
            cls,
            "__init__",
            PyObject::native_closure(kind, move |args: &[PyObjectRef]| {
                if args.len() < 2 {
                    return Err(PyException::type_error("view requires mapping"));
                }
                if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    inst.attrs
                        .write()
                        .insert(CompactString::from("_mapping"), args[1].clone());
                }
                Ok(PyObject::none())
            }),
        );
        add_method(
            cls,
            "__len__",
            PyObject::native_closure(kind, move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Err(PyException::type_error("view requires self"));
                }
                let mapping = args[0]
                    .get_attr("_mapping")
                    .or_else(|| args[0].get_attr("mapping"))
                    .unwrap_or_else(PyObject::none);
                Ok(PyObject::int(mapping.py_len()? as i64))
            }),
        );
    };

    make_mapping_view(&mapping_view_cls, "MappingView.__init__");
    make_mapping_view(&keys_view_cls, "KeysView.__init__");
    make_mapping_view(&items_view_cls, "ItemsView.__init__");
    make_mapping_view(&values_view_cls, "ValuesView.__init__");

    add_method(
        &keys_view_cls,
        "__iter__",
        PyObject::native_closure("KeysView.__iter__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("KeysView.__iter__ requires self"));
            }
            let mapping = args[0]
                .get_attr("_mapping")
                .or_else(|| args[0].get_attr("mapping"))
                .ok_or_else(|| PyException::attribute_error("_mapping"))?;
            match &mapping.payload {
                PyObjectPayload::Dict(map) | PyObjectPayload::MappingProxy(map) => Ok(
                    PyObject::list(map.read().keys().map(|k| k.to_object()).collect()),
                ),
                PyObjectPayload::InstanceDict(attrs) => {
                    let keys = attrs
                        .read()
                        .keys()
                        .map(|k| PyObject::str_val(k.clone()))
                        .collect();
                    Ok(PyObject::list(keys))
                }
                _ => Ok(PyObject::list(mapping.to_list()?)),
            }
        }),
    );
    add_method(
        &keys_view_cls,
        "__contains__",
        PyObject::native_closure("KeysView.__contains__", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let mapping = args[0]
                .get_attr("_mapping")
                .or_else(|| args[0].get_attr("mapping"))
                .ok_or_else(|| PyException::attribute_error("_mapping"))?;
            Ok(PyObject::bool_val(mapping.contains(&args[1])?))
        }),
    );
    add_method(
        &items_view_cls,
        "__iter__",
        PyObject::native_closure("ItemsView.__iter__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("ItemsView.__iter__ requires self"));
            }
            let mapping = args[0]
                .get_attr("_mapping")
                .or_else(|| args[0].get_attr("mapping"))
                .ok_or_else(|| PyException::attribute_error("_mapping"))?;
            if let PyObjectPayload::Dict(map) = &mapping.payload {
                let items: Vec<PyObjectRef> = map
                    .read()
                    .iter()
                    .map(|(k, v)| PyObject::tuple(vec![k.to_object(), v.clone()]))
                    .collect();
                Ok(PyObject::list(items))
            } else {
                Ok(PyObject::list(mapping.to_list()?))
            }
        }),
    );
    add_method(
        &items_view_cls,
        "__contains__",
        PyObject::native_closure("ItemsView.__contains__", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let mapping = args[0]
                .get_attr("_mapping")
                .or_else(|| args[0].get_attr("mapping"))
                .ok_or_else(|| PyException::attribute_error("_mapping"))?;
            if let PyObjectPayload::Dict(map) = &mapping.payload {
                let pair = args[1].to_list()?;
                if pair.len() != 2 {
                    return Ok(PyObject::bool_val(false));
                }
                let hk = pair[0].to_hashable_key()?;
                if let Some(v) = map.read().get(&hk) {
                    return Ok(PyObject::bool_val(
                        v.compare(&pair[1], CompareOp::Eq)
                            .map(|r| r.is_truthy())
                            .unwrap_or(false),
                    ));
                }
            }
            Ok(PyObject::bool_val(false))
        }),
    );
    add_method(
        &values_view_cls,
        "__iter__",
        PyObject::native_closure("ValuesView.__iter__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("ValuesView.__iter__ requires self"));
            }
            let mapping = args[0]
                .get_attr("_mapping")
                .or_else(|| args[0].get_attr("mapping"))
                .ok_or_else(|| PyException::attribute_error("_mapping"))?;
            match &mapping.payload {
                PyObjectPayload::Dict(map) | PyObjectPayload::MappingProxy(map) => {
                    Ok(PyObject::list(map.read().values().cloned().collect()))
                }
                PyObjectPayload::InstanceDict(attrs) => {
                    Ok(PyObject::list(attrs.read().values().cloned().collect()))
                }
                _ => Ok(PyObject::list(mapping.to_list()?)),
            }
        }),
    );
    add_method(
        &values_view_cls,
        "__contains__",
        PyObject::native_closure("ValuesView.__contains__", move |args: &[PyObjectRef]| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let mapping = args[0]
                .get_attr("_mapping")
                .or_else(|| args[0].get_attr("mapping"))
                .ok_or_else(|| PyException::attribute_error("_mapping"))?;
            Ok(PyObject::bool_val(mapping.to_list()?.iter().any(|v| {
                v.compare(&args[1], CompareOp::Eq)
                    .map(|r| r.is_truthy())
                    .unwrap_or(false)
            })))
        }),
    );

    make_module(
        "collections.abc",
        vec![
            ("Hashable", hashable_cls),
            ("Iterable", iterable_cls),
            ("Iterator", iterator_cls),
            ("Reversible", reversible_cls),
            ("Generator", generator_cls),
            ("Sized", sized_cls),
            ("Container", container_cls),
            ("Callable", callable_cls),
            ("Collection", collection_cls),
            ("Sequence", sequence_cls),
            ("MutableSequence", mutable_sequence_cls),
            ("ByteString", bytestring_cls),
            ("Set", set_cls),
            ("MutableSet", mutable_set_cls),
            ("Mapping", mapping_cls),
            ("MutableMapping", mutable_mapping_cls),
            ("MappingView", mapping_view_cls),
            ("KeysView", keys_view_cls),
            ("ItemsView", items_view_cls),
            ("ValuesView", values_view_cls),
            ("Awaitable", awaitable_cls),
            ("Coroutine", coroutine_cls),
            ("AsyncIterable", async_iterable_cls),
            ("AsyncIterator", async_iterator_cls),
            ("AsyncGenerator", async_generator_cls),
            ("Buffer", buffer_cls),
        ],
    )
}

// ── abc module ──

pub fn create_abc_module() -> PyObjectRef {
    // ABC base class with __abstractmethods__ marker
    let abc_class = PyObject::class(CompactString::from("ABC"), vec![], IndexMap::new());
    if let PyObjectPayload::Class(ref cd) = abc_class.payload {
        let mut ns = cd.namespace.write();
        ns.insert(
            CompactString::from("__abstractmethods__"),
            PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(
                ferrython_core::object::new_fx_hashkey_flatmap(),
            )))),
        );
        // ABC.register(subclass) — registers subclass as a virtual subclass
        let abc_ref = abc_class.clone();
        let register_fn = PyObject::native_closure("register", move |args: &[PyObjectRef]| {
            // When called as Printable.register(MyInt), args = [MyInt]
            // When called bound, args = [Printable, MyInt]
            let (cls, subclass) =
                if args.len() >= 2 && matches!(&args[0].payload, PyObjectPayload::Class(_)) {
                    (args[0].clone(), args[1].clone())
                } else if args.len() == 1 {
                    // Called unbound: use the ABC class this register was defined on
                    (abc_ref.clone(), args[0].clone())
                } else {
                    return Err(PyException::type_error(
                        "register() requires a subclass argument",
                    ));
                };
            // Store virtual subclass in _abc_registry on the ABC class (Dict with Identity keys)
            if let PyObjectPayload::Class(ref cd) = cls.payload {
                let mut ns = cd.namespace.write();
                let registry = ns
                    .entry(CompactString::from("_abc_registry"))
                    .or_insert_with(|| PyObject::dict(IndexMap::new()))
                    .clone();
                if let PyObjectPayload::Dict(ref map) = registry.payload {
                    let ptr = PyObjectRef::as_ptr(&subclass) as usize;
                    map.write().insert(
                        HashableKey::Identity(ptr, subclass.clone()),
                        PyObject::bool_val(true),
                    );
                }
            }
            // Also mark the subclass with __abc_registered__ pointing to the ABC
            if let PyObjectPayload::Class(ref cd) = subclass.payload {
                cd.namespace
                    .write()
                    .insert(CompactString::from("__abc_registered__"), cls.clone());
            }
            Ok(subclass.clone())
        });
        ns.insert(CompactString::from("register"), register_fn);
    }

    let abstractmethod_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "abstractmethod requires 1 argument",
            ));
        }
        let func = args[0].clone();
        // Return a marker tuple: ("__abstract__", func)
        let marker = PyObject::tuple(vec![
            PyObject::str_val(CompactString::from("__abstract__")),
            func,
        ]);
        Ok(marker)
    });

    let abcmeta_cls = {
        let mut ns = IndexMap::new();
        // register(cls, subclass) — register a virtual subclass
        ns.insert(
            CompactString::from("register"),
            PyObject::native_closure("ABCMeta.register", |args: &[PyObjectRef]| {
                // args: [cls (ABCMeta instance), subclass]
                if args.len() < 2 {
                    return Err(PyException::type_error(
                        "register() requires a subclass argument",
                    ));
                }
                let cls = &args[0];
                let subclass = &args[1];
                // Store in _abc_registry on the class
                if let PyObjectPayload::Class(cd) = &cls.payload {
                    let mut ns = cd.namespace.write();
                    let registry = ns
                        .entry(CompactString::from("_abc_registry"))
                        .or_insert_with(|| PyObject::dict(IndexMap::new()))
                        .clone();
                    if let PyObjectPayload::Dict(map) = &registry.payload {
                        let ptr = PyObjectRef::as_ptr(subclass) as usize;
                        let key = HashableKey::Identity(ptr, subclass.clone());
                        map.write().insert(key, PyObject::bool_val(true));
                    }
                }
                Ok(subclass.clone())
            }),
        );
        PyObject::class(
            CompactString::from("ABCMeta"),
            vec![PyObject::builtin_type(CompactString::from("type"))],
            ns,
        )
    };

    let abstractclassmethod_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "abstractclassmethod requires 1 argument",
            ));
        }
        Ok(args[0].clone())
    });

    let abstractstaticmethod_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "abstractstaticmethod requires 1 argument",
            ));
        }
        Ok(args[0].clone())
    });

    let abstractproperty_fn = make_builtin(|args: &[PyObjectRef]| {
        if !args.is_empty() {
            Ok(args[0].clone())
        } else {
            Ok(PyObject::none())
        }
    });

    let cache_token: Rc<PyCell<i64>> = Rc::new(PyCell::new(0));
    let get_cache_token_fn =
        PyObject::native_closure("abc.get_cache_token", move |_args: &[PyObjectRef]| {
            Ok(PyObject::int(*cache_token.read()))
        });

    make_module(
        "abc",
        vec![
            ("ABC", abc_class),
            ("ABCMeta", abcmeta_cls),
            ("abstractmethod", abstractmethod_fn),
            ("abstractclassmethod", abstractclassmethod_fn),
            ("abstractstaticmethod", abstractstaticmethod_fn),
            ("abstractproperty", abstractproperty_fn),
            ("get_cache_token", get_cache_token_fn),
        ],
    )
}
