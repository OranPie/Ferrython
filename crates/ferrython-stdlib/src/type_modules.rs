//! Type-system stdlib modules (typing, abc, enum, types, collections.abc)

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    FxHashKeyMap, new_fx_hashkey_map,PyCell, 
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin, check_args, check_args_min, CompareOp,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

pub fn create_typing_module() -> PyObjectRef {
    // TypeVar class — shared so isinstance(T, TypeVar) works
    let typevar_class = PyObject::class(CompactString::from("TypeVar"), vec![], IndexMap::new());
    let typevar_cls_ref = typevar_class.clone();

    // TypeVar(name, *constraints, bound=None, covariant=False, contravariant=False)
    let typevar_new = {
        let tv_cls = typevar_cls_ref.clone();
        PyObject::native_closure("TypeVar.__new__", move |args: &[PyObjectRef]| {
            // First arg is the class itself (cls), rest are user args
            let real_args = if !args.is_empty() && matches!(&args[0].payload, PyObjectPayload::Class(_)) {
                &args[1..]
            } else {
                args
            };
            check_args_min("TypeVar", real_args, 1)?;

            let last_is_kwargs = real_args.len() >= 2
                && matches!(&real_args[real_args.len() - 1].payload, PyObjectPayload::Dict(_));

            let kwargs_dict: Option<FxHashKeyMap> = if last_is_kwargs {
                if let PyObjectPayload::Dict(d) = &real_args[real_args.len() - 1].payload {
                    Some(d.read().clone())
                } else { None }
            } else { None };

            let positional_end = if last_is_kwargs { real_args.len() - 1 } else { real_args.len() };
            let name = CompactString::from(real_args[0].py_to_string());

            let constraints: Vec<PyObjectRef> = if positional_end > 1 {
                real_args[1..positional_end].to_vec()
            } else {
                vec![]
            };

            let get_kwarg = |key: &str| -> Option<PyObjectRef> {
                kwargs_dict.as_ref().and_then(|kw| {
                    kw.get(&HashableKey::str_key(CompactString::from(key))).cloned()
                })
            };
            let bound = get_kwarg("bound").unwrap_or_else(PyObject::none);
            let covariant = get_kwarg("covariant")
                .map_or(false, |v| v.is_truthy());
            let contravariant = get_kwarg("contravariant")
                .map_or(false, |v| v.is_truthy());

            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("__name__"), PyObject::str_val(name.clone()));
            attrs.insert(CompactString::from("__constraints__"), PyObject::tuple(constraints));
            attrs.insert(CompactString::from("__bound__"), bound);
            attrs.insert(CompactString::from("__covariant__"), PyObject::bool_val(covariant));
            attrs.insert(CompactString::from("__contravariant__"), PyObject::bool_val(contravariant));

            let repr_name = name.clone();
            attrs.insert(CompactString::from("__repr__"), PyObject::native_closure(
                "__repr__",
                move |_args| Ok(PyObject::str_val(CompactString::from(format!("~{}", repr_name).as_str()))),
            ));

            Ok(PyObject::instance_with_attrs(tv_cls.clone(), attrs))
        })
    };

    // Set __new__ on the TypeVar class so TypeVar("T") creates proper instances
    if let PyObjectPayload::Class(ref data) = typevar_class.payload {
        data.namespace.write().insert(
            CompactString::from("__new__"),
            typevar_new,
        );
    }

    // Generic — placeholder class
    let generic_class = PyObject::class(
        CompactString::from("Generic"),
        vec![],
        IndexMap::new(),
    );

    // Protocol — placeholder class
    let protocol_class = PyObject::class(
        CompactString::from("Protocol"),
        vec![],
        IndexMap::new(),
    );

    // Helper to create typing generic alias classes
    // These support subscript notation: List[int] → _GenericAlias with __origin__ and __args__
    let make_typing_alias = |display_name: &str| -> PyObjectRef {
        let name = CompactString::from(display_name);
        let display = name.clone();
        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("__typing_name__"), PyObject::str_val(name));
        // __class_getitem__ to support List[int] etc.
        ns.insert(CompactString::from("__class_getitem__"), PyObject::native_closure(
            "__class_getitem__",
            {
                let display = display.clone();
                move |args: &[PyObjectRef]| -> Result<PyObjectRef, PyException> {
                    // Build a _GenericAlias object with __origin__, __args__, __repr__
                    let origin_display = display.clone();
                    let params = if args.len() >= 2 { args[1].clone() } else if args.len() == 1 { args[0].clone() } else { PyObject::none() };

                    // Build __args__ tuple
                    let args_tuple = match &params.payload {
                        PyObjectPayload::Tuple(items) => PyObject::tuple((**items).clone()),
                        _ => PyObject::tuple(vec![params.clone()]),
                    };

                    let params_str = match &params.payload {
                        PyObjectPayload::Tuple(items) => {
                            items.iter().map(|i| {
                                if let PyObjectPayload::Class(cd) = &i.payload {
                                    cd.name.to_string()
                                } else if let PyObjectPayload::BuiltinType(n) = &i.payload {
                                    n.to_string()
                                } else {
                                    i.py_to_string()
                                }
                            }).collect::<Vec<_>>().join(", ")
                        }
                        PyObjectPayload::Class(cd) => cd.name.to_string(),
                        PyObjectPayload::BuiltinType(n) => n.to_string(),
                        _ => params.py_to_string(),
                    };
                    let repr = format!("typing.{}[{}]", origin_display, params_str);

                    let cls = PyObject::class(CompactString::from("_GenericAlias"), vec![], IndexMap::new());
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
                    attrs.insert(CompactString::from("__repr__"), PyObject::native_closure(
                        "__repr__",
                        move |_args| Ok(PyObject::str_val(CompactString::from(repr_clone.as_str()))),
                    ));
                    let str_clone = repr.clone();
                    attrs.insert(CompactString::from("__str__"), PyObject::native_closure(
                        "__str__",
                        move |_args| Ok(PyObject::str_val(CompactString::from(str_clone.as_str()))),
                    ));
                    Ok(PyObject::instance_with_attrs(cls, attrs))
                }
            },
        ));
        PyObject::class(CompactString::from(display_name), vec![], ns)
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
        ("Generic", generic_class),
        ("Protocol", protocol_class),
        ("ClassVar", make_typing_alias("ClassVar")),
        ("Final", make_typing_alias("Final")),
        ("Literal", make_typing_alias("Literal")),
        ("NamedTuple", PyObject::builtin_type(CompactString::from("NamedTuple"))),
        ("get_type_hints", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() {
                return Ok(PyObject::dict(IndexMap::new()));
            }
            let obj = &args[0];
            if let Some(ann) = obj.get_attr("__annotations__") {
                Ok(ann)
            } else {
                Ok(PyObject::dict(IndexMap::new()))
            }
        })),
        ("get_args", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::tuple(vec![])); }
            if let Some(type_args) = args[0].get_attr("__args__") {
                Ok(type_args)
            } else {
                Ok(PyObject::tuple(vec![]))
            }
        })),
        ("get_origin", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::none()); }
            if let Some(origin) = args[0].get_attr("__origin__") {
                Ok(origin)
            } else {
                Ok(PyObject::none())
            }
        })),
        ("cast", make_builtin(|args: &[PyObjectRef]| {
            check_args("cast", args, 2)?;
            Ok(args[1].clone())
        })),
        ("overload", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::none()); }
            Ok(args[0].clone())
        })),
        ("no_type_check", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::none()); }
            Ok(args[0].clone())
        })),
        ("runtime_checkable", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::none()); }
            // Mark the class as runtime_checkable by adding __protocol_attrs__
            // which lists the methods that must be present for isinstance checks
            if let PyObjectPayload::Class(cd) = &args[0].payload {
                let mut ns = cd.namespace.write();
                // Collect all non-dunder, non-private method names from the protocol
                let protocol_attrs: Vec<PyObjectRef> = ns.iter()
                    .filter(|(k, _)| !k.starts_with('_'))
                    .map(|(k, _)| PyObject::str_val(k.clone()))
                    .collect();
                let attrs_tuple = PyObject::tuple(protocol_attrs);
                ns.insert(CompactString::from("__protocol_attrs__"), attrs_tuple.clone());
                ns.insert(CompactString::from("_is_runtime_checkable"), PyObject::bool_val(true));
                // __instancecheck__(cls, obj) — structural check
                ns.insert(CompactString::from("__instancecheck__"), PyObject::native_closure(
                    "__instancecheck__",
                    move |ic_args: &[PyObjectRef]| {
                        // ic_args[0] = cls (self), ic_args[1] = obj
                        if ic_args.len() < 2 { return Ok(PyObject::bool_val(false)); }
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
                ));
            }
            Ok(args[0].clone())
        })),
    ];
    attrs.push(("TYPE_CHECKING", PyObject::bool_val(false)));
    // InitVar — simple marker type for use with dataclasses
    attrs.push(("InitVar", make_typing_alias("InitVar")));
    // final — no-op decorator at runtime
    attrs.push(("final", make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::none()); }
        Ok(args[0].clone())
    })));
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
    attrs.push(("AsyncContextManager", make_typing_alias("AsyncContextManager")));
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
    attrs.push(("NewType", make_builtin(|args: &[PyObjectRef]| {
        check_args("NewType", args, 2)?;
        Ok(args[1].clone()) // NewType(name, tp) returns tp
    })));
    // TypedDict: In CPython, TypedDict creates a class that, when instantiated,
    // returns a plain dict. TypedDict subclasses are conceptually dict subclasses.
    // We implement __new__ to return a dict built from kwargs.
    let typed_dict_cls = {
        let mut td_ns = IndexMap::new();
        td_ns.insert(CompactString::from("__init_subclass__"), make_builtin(|_args| {
            Ok(PyObject::none())
        }));
        // __new__ returns a plain dict from kwargs
        td_ns.insert(CompactString::from("__new__"), PyObject::native_closure(
            "__new__", |args: &[PyObjectRef]| {
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
            }));
        PyObject::class(CompactString::from("TypedDict"), vec![], td_ns)
    };
    attrs.push(("TypedDict", typed_dict_cls));
    attrs.push(("ForwardRef", make_typing_alias("ForwardRef")));

    // Python 3.9+ additions
    attrs.push(("Annotated", make_typing_alias("Annotated")));
    attrs.push(("ParamSpec", PyObject::native_closure("ParamSpec", |args: &[PyObjectRef]| {
        let name = if let Some(a) = args.first() { a.py_to_string() } else { "P".into() };
        let cls = PyObject::class(CompactString::from("ParamSpec"), vec![], IndexMap::new());
        let mut iattrs = IndexMap::new();
        iattrs.insert(CompactString::from("__name__"), PyObject::str_val(CompactString::from(name.as_str())));
        iattrs.insert(CompactString::from("args"), PyObject::str_val(CompactString::from(format!("{}.args", name))));
        iattrs.insert(CompactString::from("kwargs"), PyObject::str_val(CompactString::from(format!("{}.kwargs", name))));
        Ok(PyObject::instance_with_attrs(cls, iattrs))
    })));
    attrs.push(("TypeAlias", make_typing_alias("TypeAlias")));
    attrs.push(("TypeGuard", make_typing_alias("TypeGuard")));

    // Python 3.11+ additions
    attrs.push(("Never", make_typing_alias("Never")));
    attrs.push(("Self", make_typing_alias("Self")));
    attrs.push(("assert_type", make_builtin(|args: &[PyObjectRef]| {
        // assert_type(val, typ) → returns val (no-op at runtime)
        check_args("assert_type", args, 2)?;
        Ok(args[0].clone())
    })));
    attrs.push(("reveal_type", make_builtin(|args: &[PyObjectRef]| {
        check_args("reveal_type", args, 1)?;
        let val = &args[0];
        eprintln!("Runtime type is '{}'", val.type_name());
        Ok(val.clone())
    })));

    // Python 3.12+ additions
    attrs.push(("TypeAliasType", make_typing_alias("TypeAliasType")));
    attrs.push(("override", make_builtin(|args: &[PyObjectRef]| {
        // @override decorator — no-op at runtime
        check_args("override", args, 1)?;
        Ok(args[0].clone())
    })));

    // Common type forms
    attrs.push(("NoReturn", make_typing_alias("NoReturn")));
    attrs.push(("AnyStr", make_typing_alias("AnyStr")));
    attrs.push(("LiteralString", make_typing_alias("LiteralString")));
    attrs.push(("Unpack", make_typing_alias("Unpack")));
    attrs.push(("TypeVarTuple", PyObject::native_closure("TypeVarTuple", |args: &[PyObjectRef]| {
        let name = if let Some(a) = args.first() { a.py_to_string() } else { "Ts".into() };
        let cls = PyObject::class(CompactString::from("TypeVarTuple"), vec![], IndexMap::new());
        let mut iattrs = IndexMap::new();
        iattrs.insert(CompactString::from("__name__"), PyObject::str_val(CompactString::from(name.as_str())));
        Ok(PyObject::instance_with_attrs(cls, iattrs))
    })));
    attrs.push(("Concatenate", make_typing_alias("Concatenate")));

    // Python 3.11+ Required/NotRequired/ReadOnly for TypedDict
    attrs.push(("Required", make_typing_alias("Required")));
    attrs.push(("NotRequired", make_typing_alias("NotRequired")));
    attrs.push(("ReadOnly", make_typing_alias("ReadOnly")));
    attrs.push(("Buffer", make_typing_alias("Buffer")));

    // dataclass_transform — PEP 681
    attrs.push(("dataclass_transform", make_builtin(|args: &[PyObjectRef]| {
        // @dataclass_transform() decorator — marks a class/function as
        // creating dataclass-like semantics
        if args.is_empty() {
            // Called as @dataclass_transform() — return decorator
            return Ok(make_builtin(|inner_args: &[PyObjectRef]| {
                if inner_args.is_empty() { return Ok(PyObject::none()); }
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
    })));

    // get_overloads / clear_overloads
    attrs.push(("get_overloads", make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::list(vec![]))
    })));
    attrs.push(("clear_overloads", make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::none())
    })));

    // is_typeddict
    attrs.push(("is_typeddict", make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::bool_val(false)); }
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
    })));

    // _GenericAlias — importable class used by typing internals (attrs, pydantic, etc.)
    let generic_alias_cls = {
        let mut ga_ns = IndexMap::new();
        ga_ns.insert(CompactString::from("__new__"), PyObject::native_closure(
            "_GenericAlias.__new__", |args: &[PyObjectRef]| {
                // _GenericAlias(origin, params) → args[0]=cls, args[1]=origin, args[2]=params
                let origin = if args.len() > 1 { args[1].clone() } else { PyObject::none() };
                let type_args = if args.len() > 2 { args[2].clone() } else { PyObject::tuple(vec![]) };
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
                    PyObjectPayload::Tuple(items) => {
                        items.iter().map(|i| match &i.payload {
                            PyObjectPayload::Class(cd) => cd.name.to_string(),
                            PyObjectPayload::BuiltinType(n) => n.to_string(),
                            _ => i.py_to_string(),
                        }).collect::<Vec<_>>().join(", ")
                    }
                    _ => args_tuple.py_to_string(),
                };
                let repr_str = format!("{}[{}]", origin_name, args_str);

                let inst_cls = PyObject::class(CompactString::from("_GenericAlias"), vec![], IndexMap::new());
                let mut iattrs = IndexMap::new();
                iattrs.insert(CompactString::from("__origin__"), origin.clone());
                iattrs.insert(CompactString::from("__args__"), args_tuple);
                let rc1 = repr_str.clone();
                iattrs.insert(CompactString::from("__repr__"), PyObject::native_closure(
                    "__repr__", move |_| Ok(PyObject::str_val(CompactString::from(rc1.as_str()))),
                ));
                let rc2 = repr_str;
                iattrs.insert(CompactString::from("__str__"), PyObject::native_closure(
                    "__str__", move |_| Ok(PyObject::str_val(CompactString::from(rc2.as_str()))),
                ));
                // copy_with(new_args) — new alias with same origin but different args
                let origin_cw = origin.clone();
                iattrs.insert(CompactString::from("copy_with"), PyObject::native_closure(
                    "copy_with", move |cw_args: &[PyObjectRef]| {
                        let new_a = if cw_args.is_empty() { PyObject::tuple(vec![]) } else { cw_args[0].clone() };
                        let new_at = match &new_a.payload {
                            PyObjectPayload::Tuple(_) => new_a,
                            _ => PyObject::tuple(vec![new_a]),
                        };
                        let c = PyObject::class(CompactString::from("_GenericAlias"), vec![], IndexMap::new());
                        let mut a = IndexMap::new();
                        a.insert(CompactString::from("__origin__"), origin_cw.clone());
                        a.insert(CompactString::from("__args__"), new_at);
                        Ok(PyObject::instance_with_attrs(c, a))
                    },
                ));
                // __getitem__ for further parameterization
                let origin_gi = origin;
                iattrs.insert(CompactString::from("__getitem__"), PyObject::native_closure(
                    "__getitem__", move |gi_args: &[PyObjectRef]| {
                        let params = if gi_args.is_empty() { PyObject::none() } else { gi_args[0].clone() };
                        let new_at = match &params.payload {
                            PyObjectPayload::Tuple(items) => PyObject::tuple((**items).clone()),
                            _ => PyObject::tuple(vec![params]),
                        };
                        let c = PyObject::class(CompactString::from("_GenericAlias"), vec![], IndexMap::new());
                        let mut a = IndexMap::new();
                        a.insert(CompactString::from("__origin__"), origin_gi.clone());
                        a.insert(CompactString::from("__args__"), new_at);
                        Ok(PyObject::instance_with_attrs(c, a))
                    },
                ));
                Ok(PyObject::instance_with_attrs(inst_cls, iattrs))
            }
        ));
        PyObject::class(CompactString::from("_GenericAlias"), vec![], ga_ns)
    };
    attrs.push(("_GenericAlias", generic_alias_cls));

    // _SpecialForm — internal marker class used by typing
    attrs.push(("_SpecialForm", PyObject::class(
        CompactString::from("_SpecialForm"),
        vec![],
        IndexMap::new(),
    )));

    // assert_never — should always raise TypeError at runtime (PEP 782)
    attrs.push(("assert_never", make_builtin(|args: &[PyObjectRef]| {
        check_args("assert_never", args, 1)?;
        Err(PyException::type_error(format!(
            "Expected code to be unreachable, but got: {}", args[0].repr()
        )))
    })));

    // _type_check — internal helper used by mypy_extensions, typing_extensions
    attrs.push(("_type_check", make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::none()); }
        Ok(args[0].clone())
    })));
    attrs.push(("_GenericForm", make_typing_alias("_GenericForm")));
    attrs.push(("_AnnotatedAlias", make_typing_alias("_AnnotatedAlias")));
    attrs.push(("_collect_parameters", make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::tuple(vec![]))
    })));

    // Sentinel — Python 3.13+ typing.Sentinel (PEP 661)
    let sentinel_cls = PyObject::class(CompactString::from("Sentinel"), vec![], IndexMap::new());
    if let PyObjectPayload::Class(ref cd) = sentinel_cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(CompactString::from("__init__"), make_builtin(|args: &[PyObjectRef]| {
            if args.len() >= 2 {
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    inst.attrs.write().insert(CompactString::from("_name"), args[1].clone());
                }
            }
            Ok(PyObject::none())
        }));
        ns.insert(CompactString::from("__repr__"), make_builtin(|args: &[PyObjectRef]| {
            let name = args[0].get_attr("_name").map(|v| v.py_to_string()).unwrap_or_else(|| "Sentinel".to_string());
            Ok(PyObject::str_val(CompactString::from(name)))
        }));
        ns.insert(CompactString::from("__bool__"), make_builtin(|_args: &[PyObjectRef]| {
            Ok(PyObject::bool_val(false))
        }));
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
    enum_ns.insert(CompactString::from("__getitem__"), PyObject::native_function(
        "Enum.__getitem__", |args: &[PyObjectRef]| {
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
        }
    ));

    // __call__ on class — Color(1) looks up member by value,
    // OR functional API: Enum("Name", "member1 member2") creates a new enum
    enum_ns.insert(CompactString::from("__call__"), PyObject::native_function(
        "Enum.__call__", |args: &[PyObjectRef]| {
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
                        s.replace(',', " ").split_whitespace().map(|s| s.to_string()).collect()
                    }
                    PyObjectPayload::Tuple(items) => {
                        items.iter().map(|i: &PyObjectRef| i.py_to_string()).collect()
                    }
                    PyObjectPayload::List(items) => {
                        items.read().iter().map(|i: &PyObjectRef| i.py_to_string()).collect()
                    }
                    _ => vec![names_arg.py_to_string()],
                };
                // Create a new class with members
                let mut members_map: FxHashKeyMap = new_fx_hashkey_map();
                let new_cls = PyObject::class(CompactString::from(class_name.as_str()), vec![cls.clone()], IndexMap::new());
                if let PyObjectPayload::Class(ref cd) = new_cls.payload {
                    let mut ns = cd.namespace.write();
                    for (i, mname) in member_names.iter().enumerate() {
                        let cs_name = CompactString::from(mname.as_str());
                        let mut member_attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
                        member_attrs.insert(CompactString::from("name"), PyObject::str_val(cs_name.clone()));
                        member_attrs.insert(CompactString::from("_name_"), PyObject::str_val(cs_name.clone()));
                        member_attrs.insert(CompactString::from("value"), PyObject::int(i as i64 + 1));
                        member_attrs.insert(CompactString::from("_value_"), PyObject::int(i as i64 + 1));
                        let member = PyObject::instance_with_attrs(new_cls.clone(), member_attrs);
                        ns.insert(cs_name.clone(), member.clone());
                        members_map.insert(
                            HashableKey::str_key(cs_name),
                            member,
                        );
                    }
                    ns.insert(CompactString::from("__members__"), PyObject::dict(members_map));
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
            Err(PyException::value_error(format!("{} is not a valid enum value", value.repr())))
        }
    ));

    // __iter__ on class — list(Color) iterates members
    enum_ns.insert(CompactString::from("__iter__"), PyObject::native_function(
        "Enum.__iter__", |args: &[PyObjectRef]| {
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
        }
    ));

    // __len__ on class — len(Color) returns member count
    enum_ns.insert(CompactString::from("__len__"), PyObject::native_function(
        "Enum.__len__", |args: &[PyObjectRef]| {
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
        }
    ));

    // __contains__ on class — Color.RED in Color
    enum_ns.insert(CompactString::from("__contains__"), PyObject::native_function(
        "Enum.__contains__", |args: &[PyObjectRef]| {
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
                            if let (Some(mv), Some(iv)) = (member.get_attr("value"), item.get_attr("value")) {
                                if mv.py_to_string() == iv.py_to_string() {
                                    return Ok(PyObject::bool_val(true));
                                }
                            }
                        }
                    }
                }
            }
            Ok(PyObject::bool_val(false))
        }
    ));

    let enum_class = PyObject::class(
        CompactString::from("Enum"),
        vec![],
        enum_ns,
    );

    // IntEnum — Enum subclass where values are ints and support int operations
    let mut int_enum_ns = IndexMap::new();
    int_enum_ns.insert(CompactString::from("__enum__"), PyObject::bool_val(true));
    int_enum_ns.insert(CompactString::from("__int_enum__"), PyObject::bool_val(true));

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
    int_enum_ns.insert(CompactString::from("__int__"), PyObject::native_function(
        "IntEnum.__int__", |args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::int(0)); }
            Ok(PyObject::int(int_enum_val(&args[0]).unwrap_or(0)))
        }
    ));

    // __eq__ — compare with int or another IntEnum member
    int_enum_ns.insert(CompactString::from("__eq__"), PyObject::native_function(
        "IntEnum.__eq__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__eq__", args, 2)?;
            let a = int_enum_val(&args[0]);
            let b = int_enum_val(&args[1]);
            Ok(PyObject::bool_val(a == b))
        }
    ));

    // __lt__
    int_enum_ns.insert(CompactString::from("__lt__"), PyObject::native_function(
        "IntEnum.__lt__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__lt__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a < b))
        }
    ));

    // __le__
    int_enum_ns.insert(CompactString::from("__le__"), PyObject::native_function(
        "IntEnum.__le__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__le__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a <= b))
        }
    ));

    // __gt__
    int_enum_ns.insert(CompactString::from("__gt__"), PyObject::native_function(
        "IntEnum.__gt__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__gt__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a > b))
        }
    ));

    // __ge__
    int_enum_ns.insert(CompactString::from("__ge__"), PyObject::native_function(
        "IntEnum.__ge__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__ge__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a >= b))
        }
    ));

    // __add__ — IntEnum + int
    int_enum_ns.insert(CompactString::from("__add__"), PyObject::native_function(
        "IntEnum.__add__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__add__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a + b))
        }
    ));

    // __sub__ — IntEnum - int
    int_enum_ns.insert(CompactString::from("__sub__"), PyObject::native_function(
        "IntEnum.__sub__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__sub__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a - b))
        }
    ));

    // __mul__ — IntEnum * int
    int_enum_ns.insert(CompactString::from("__mul__"), PyObject::native_function(
        "IntEnum.__mul__", |args: &[PyObjectRef]| {
            check_args("IntEnum.__mul__", args, 2)?;
            let a = int_enum_val(&args[0]).unwrap_or(0);
            let b = int_enum_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a * b))
        }
    ));

    let int_enum = PyObject::class(
        CompactString::from("IntEnum"),
        vec![enum_class.clone(), PyObject::builtin_type(CompactString::from("int"))],
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
    flag_ns.insert(CompactString::from("__or__"), PyObject::native_function(
        "Flag.__or__", |args: &[PyObjectRef]| {
            check_args("Flag.__or__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a | b))
        }
    ));

    // __and__ — bitwise AND of flags
    flag_ns.insert(CompactString::from("__and__"), PyObject::native_function(
        "Flag.__and__", |args: &[PyObjectRef]| {
            check_args("Flag.__and__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a & b))
        }
    ));

    // __xor__ — bitwise XOR of flags
    flag_ns.insert(CompactString::from("__xor__"), PyObject::native_function(
        "Flag.__xor__", |args: &[PyObjectRef]| {
            check_args("Flag.__xor__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a ^ b))
        }
    ));

    // __invert__ — bitwise complement
    flag_ns.insert(CompactString::from("__invert__"), PyObject::native_function(
        "Flag.__invert__", |args: &[PyObjectRef]| {
            check_args("Flag.__invert__", args, 1)?;
            let v = flag_int_val(&args[0]).unwrap_or(0);
            Ok(PyObject::int(!v))
        }
    ));

    // __contains__ — check if one flag contains another (a & b == b)
    flag_ns.insert(CompactString::from("__contains__"), PyObject::native_function(
        "Flag.__contains__", |args: &[PyObjectRef]| {
            check_args("Flag.__contains__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a & b == b))
        }
    ));

    // __bool__ — Flag(0) is falsy
    flag_ns.insert(CompactString::from("__bool__"), PyObject::native_function(
        "Flag.__bool__", |args: &[PyObjectRef]| {
            check_args("Flag.__bool__", args, 1)?;
            let v = flag_int_val(&args[0]).unwrap_or(0);
            Ok(PyObject::bool_val(v != 0))
        }
    ));

    // __repr__ — show combined flags in "Flag1|Flag2" format
    flag_ns.insert(CompactString::from("__repr__"), PyObject::native_function(
        "Flag.__repr__", |args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::str_val(CompactString::from("<Flag>"))); }
            let self_obj = &args[0];
            let val = flag_int_val(self_obj).unwrap_or(0);
            // Try to get the name directly (single member)
            if let Some(name) = self_obj.get_attr("name") {
                let name_s = name.py_to_string();
                if name_s != "None" && !name_s.is_empty() {
                    // Get class name if available
                    let cls_name = self_obj.get_attr("__class__")
                        .and_then(|c| c.get_attr("__name__"))
                        .map(|n| n.py_to_string())
                        .unwrap_or_else(|| "Flag".to_string());
                    return Ok(PyObject::str_val(CompactString::from(
                        format!("<{}.{}: {}>", cls_name, name_s, val)
                    )));
                }
            }
            // Combined flags — try to decompose by iterating class members
            if val == 0 {
                let cls_name = self_obj.get_attr("__class__")
                    .and_then(|c| c.get_attr("__name__"))
                    .map(|n| n.py_to_string())
                    .unwrap_or_else(|| "Flag".to_string());
                return Ok(PyObject::str_val(CompactString::from(
                    format!("<{}: 0>", cls_name)
                )));
            }
            Ok(PyObject::str_val(CompactString::from(format!("<Flag: {}>", val))))
        }
    ));

    let flag_class = PyObject::class(
        CompactString::from("Flag"),
        vec![enum_class.clone(), PyObject::builtin_type(CompactString::from("int"))],
        flag_ns,
    );

    // IntFlag — Flag subclass with int arithmetic support
    let mut int_flag_ns = IndexMap::new();
    int_flag_ns.insert(CompactString::from("__enum__"), PyObject::bool_val(true));
    int_flag_ns.insert(CompactString::from("__flag__"), PyObject::bool_val(true));
    int_flag_ns.insert(CompactString::from("__int_enum__"), PyObject::bool_val(true));

    // Bitwise ops (duplicated from Flag since Ferrython doesn't do full MRO for class namespaces)
    int_flag_ns.insert(CompactString::from("__or__"), PyObject::native_function(
        "IntFlag.__or__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__or__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a | b))
        }
    ));

    int_flag_ns.insert(CompactString::from("__and__"), PyObject::native_function(
        "IntFlag.__and__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__and__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a & b))
        }
    ));

    int_flag_ns.insert(CompactString::from("__xor__"), PyObject::native_function(
        "IntFlag.__xor__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__xor__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a ^ b))
        }
    ));

    int_flag_ns.insert(CompactString::from("__invert__"), PyObject::native_function(
        "IntFlag.__invert__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__invert__", args, 1)?;
            let v = flag_int_val(&args[0]).unwrap_or(0);
            Ok(PyObject::int(!v))
        }
    ));

    int_flag_ns.insert(CompactString::from("__contains__"), PyObject::native_function(
        "IntFlag.__contains__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__contains__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a & b == b))
        }
    ));

    int_flag_ns.insert(CompactString::from("__bool__"), PyObject::native_function(
        "IntFlag.__bool__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__bool__", args, 1)?;
            let v = flag_int_val(&args[0]).unwrap_or(0);
            Ok(PyObject::bool_val(v != 0))
        }
    ));

    // Int conversion
    int_flag_ns.insert(CompactString::from("__int__"), PyObject::native_function(
        "IntFlag.__int__", |args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::int(0)); }
            Ok(PyObject::int(flag_int_val(&args[0]).unwrap_or(0)))
        }
    ));

    // Comparison ops (same as IntEnum)
    int_flag_ns.insert(CompactString::from("__eq__"), PyObject::native_function(
        "IntFlag.__eq__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__eq__", args, 2)?;
            let a = flag_int_val(&args[0]);
            let b = flag_int_val(&args[1]);
            Ok(PyObject::bool_val(a == b))
        }
    ));

    int_flag_ns.insert(CompactString::from("__lt__"), PyObject::native_function(
        "IntFlag.__lt__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__lt__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a < b))
        }
    ));

    int_flag_ns.insert(CompactString::from("__le__"), PyObject::native_function(
        "IntFlag.__le__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__le__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a <= b))
        }
    ));

    int_flag_ns.insert(CompactString::from("__gt__"), PyObject::native_function(
        "IntFlag.__gt__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__gt__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a > b))
        }
    ));

    int_flag_ns.insert(CompactString::from("__ge__"), PyObject::native_function(
        "IntFlag.__ge__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__ge__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::bool_val(a >= b))
        }
    ));

    // Arithmetic ops (IntFlag acts as int)
    int_flag_ns.insert(CompactString::from("__add__"), PyObject::native_function(
        "IntFlag.__add__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__add__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a + b))
        }
    ));

    int_flag_ns.insert(CompactString::from("__sub__"), PyObject::native_function(
        "IntFlag.__sub__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__sub__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a - b))
        }
    ));

    int_flag_ns.insert(CompactString::from("__mul__"), PyObject::native_function(
        "IntFlag.__mul__", |args: &[PyObjectRef]| {
            check_args("IntFlag.__mul__", args, 2)?;
            let a = flag_int_val(&args[0]).unwrap_or(0);
            let b = flag_int_val(&args[1]).unwrap_or(0);
            Ok(PyObject::int(a * b))
        }
    ));

    // __repr__ for IntFlag
    int_flag_ns.insert(CompactString::from("__repr__"), PyObject::native_function(
        "IntFlag.__repr__", |args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::str_val(CompactString::from("<IntFlag>"))); }
            let self_obj = &args[0];
            let val = flag_int_val(self_obj).unwrap_or(0);
            if let Some(name) = self_obj.get_attr("name") {
                let name_s = name.py_to_string();
                if name_s != "None" && !name_s.is_empty() {
                    let cls_name = self_obj.get_attr("__class__")
                        .and_then(|c| c.get_attr("__name__"))
                        .map(|n| n.py_to_string())
                        .unwrap_or_else(|| "IntFlag".to_string());
                    return Ok(PyObject::str_val(CompactString::from(
                        format!("<{}.{}: {}>", cls_name, name_s, val)
                    )));
                }
            }
            if val == 0 {
                let cls_name = self_obj.get_attr("__class__")
                    .and_then(|c| c.get_attr("__name__"))
                    .map(|n| n.py_to_string())
                    .unwrap_or_else(|| "IntFlag".to_string());
                return Ok(PyObject::str_val(CompactString::from(
                    format!("<{}: 0>", cls_name)
                )));
            }
            Ok(PyObject::str_val(CompactString::from(format!("<IntFlag: {}>", val))))
        }
    ));

    let int_flag_class = PyObject::class(
        CompactString::from("IntFlag"),
        vec![flag_class.clone()],
        int_flag_ns,
    );

    // auto() counter — returns a sentinel that process_enum_class resolves
    static AUTO_COUNTER: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(1);

    // unique decorator — validates all values in enum are unique
    let unique_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::none()); }
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
                                return Err(PyException::value_error(
                                    format!("duplicate values found in enum {}", cd.name)
                                ));
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
    str_enum_ns.insert(CompactString::from("__str_enum__"), PyObject::bool_val(true));
    let str_enum = PyObject::class(
        CompactString::from("StrEnum"),
        vec![enum_class.clone(), PyObject::builtin_type(CompactString::from("str"))],
        str_enum_ns,
    );

    make_module("enum", vec![
        ("Enum", enum_class),
        ("IntEnum", int_enum),
        ("Flag", flag_class),
        ("IntFlag", int_flag_class),
        ("StrEnum", str_enum),
        ("auto", make_builtin(|_| {
            // Return a sentinel tuple ("__enum_auto__", counter_value)
            // process_enum_class will detect this and assign sequential values
            let val = AUTO_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(PyObject::tuple(vec![
                PyObject::str_val(CompactString::from("__enum_auto__")),
                PyObject::int(val),
            ]))
        })),
        ("unique", unique_fn),
        // sentinel — creates a unique sentinel value (Python 3.13+)
        ("sentinel", make_builtin(|args: &[PyObjectRef]| {
            let name = if !args.is_empty() { args[0].py_to_string() } else { "MISSING".to_string() };
            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("_name"), PyObject::str_val(CompactString::from(name.clone())));
            attrs.insert(CompactString::from("__repr__"), PyObject::native_closure("sentinel.__repr__", {
                let n = name.clone();
                move |_| Ok(PyObject::str_val(CompactString::from(format!("<{}>", n))))
            }));
            attrs.insert(CompactString::from("__bool__"), make_builtin(|_: &[PyObjectRef]| {
                Ok(PyObject::bool_val(false))
            }));
            Ok(PyObject::module_with_attrs(CompactString::from(name), attrs))
        })),
    ])
}

// ── types module ──

fn compare_namespaces(a: &PyObjectRef, b: &PyObjectRef) -> PyResult<PyObjectRef> {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Instance(ref sd), PyObjectPayload::Instance(ref od)) => {
            let sa = sd.attrs.read();
            let oa = od.attrs.read();
            let self_user: Vec<_> = sa.iter().filter(|(k, _)| !k.starts_with('_')).collect();
            let other_user: Vec<_> = oa.iter().filter(|(k, _)| !k.starts_with('_')).collect();
            if self_user.len() != other_user.len() { return Ok(PyObject::bool_val(false)); }
            for (k, v) in &self_user {
                if let Some(ov) = oa.get(*k) {
                    let eq = v.compare(ov, CompareOp::Eq)?;
                    if !eq.is_truthy() { return Ok(PyObject::bool_val(false)); }
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
    make_module("types", vec![
        ("NoneType", PyObject::builtin_type(CompactString::from("NoneType"))),
        ("FunctionType", PyObject::builtin_type(CompactString::from("function"))),
        ("LambdaType", PyObject::builtin_type(CompactString::from("function"))),
        ("BuiltinFunctionType", PyObject::builtin_type(CompactString::from("builtin_function_or_method"))),
        ("BuiltinMethodType", PyObject::builtin_type(CompactString::from("builtin_function_or_method"))),
        ("MethodType", PyObject::builtin_type(CompactString::from("method"))),
        ("ModuleType", make_builtin(|args| {
            check_args_min("ModuleType", args, 1)?;
            let name = args[0].py_to_string();
            let mut module_attrs = IndexMap::new();
            if args.len() > 1 {
                module_attrs.insert(CompactString::from("__doc__"), args[1].clone());
            } else {
                module_attrs.insert(CompactString::from("__doc__"), PyObject::none());
            }
            Ok(PyObject::module_with_attrs(CompactString::from(name.as_str()), module_attrs))
        })),
        ("GeneratorType", PyObject::builtin_type(CompactString::from("generator"))),
        ("CodeType", PyObject::builtin_type(CompactString::from("code"))),
        ("FrameType", PyObject::builtin_type(CompactString::from("frame"))),
        ("TracebackType", PyObject::builtin_type(CompactString::from("traceback"))),
        ("CoroutineType", PyObject::builtin_type(CompactString::from("coroutine"))),
        ("AsyncGeneratorType", PyObject::builtin_type(CompactString::from("async_generator"))),
        ("MappingProxyType", PyObject::builtin_type(CompactString::from("mappingproxy"))),
        ("GetSetDescriptorType", PyObject::builtin_type(CompactString::from("getset_descriptor"))),
        ("MemberDescriptorType", PyObject::builtin_type(CompactString::from("member_descriptor"))),
        ("WrapperDescriptorType", PyObject::builtin_type(CompactString::from("wrapper_descriptor"))),
        ("MethodWrapperType", PyObject::builtin_type(CompactString::from("method-wrapper"))),
        ("MethodDescriptorType", PyObject::builtin_type(CompactString::from("method_descriptor"))),
        ("ClassMethodDescriptorType", PyObject::builtin_type(CompactString::from("classmethod_descriptor"))),
        ("CellType", PyObject::builtin_type(CompactString::from("cell"))),
        ("UnionType", PyObject::builtin_type(CompactString::from("UnionType"))),
        ("EllipsisType", PyObject::builtin_type(CompactString::from("ellipsis"))),
        ("NotImplementedType", PyObject::builtin_type(CompactString::from("NotImplementedType"))),
        ("SimpleNamespace", make_builtin(|args| {
            // Build class-level __repr__ and __eq__ so the VM can dispatch them
            let mut methods = IndexMap::new();
            methods.insert(CompactString::from("__repr__"), PyObject::native_closure(
                "SimpleNamespace.__repr__", |repr_args: &[PyObjectRef]| {
                    if repr_args.is_empty() {
                        return Ok(PyObject::str_val(CompactString::from("namespace()")));
                    }
                    let self_obj = &repr_args[0];
                    if let PyObjectPayload::Instance(ref d) = self_obj.payload {
                        let attrs = d.attrs.read();
                        let parts: Vec<String> = attrs.iter()
                            .filter(|(k, _)| !k.starts_with('_'))
                            .map(|(k, v)| format!("{}={}", k, v.repr()))
                            .collect();
                        Ok(PyObject::str_val(CompactString::from(format!("namespace({})", parts.join(", ")))))
                    } else {
                        Ok(PyObject::str_val(CompactString::from("namespace()")))
                    }
                },
            ));
            methods.insert(CompactString::from("__eq__"), PyObject::native_closure(
                "SimpleNamespace.__eq__", |eq_args: &[PyObjectRef]| {
                    // When called via == operator: args = [self, other]
                    // When called via ns1.__eq__(ns2): args = [ns2] (no self)
                    // We handle the 2-arg case here; 1-arg case handled at instance level
                    if eq_args.len() < 2 {
                        return Ok(PyObject::bool_val(false));
                    }
                    compare_namespaces(&eq_args[0], &eq_args[1])
                },
            ));
            let cls = PyObject::class(CompactString::from("SimpleNamespace"), vec![], methods);
            let inst = PyObject::instance(cls);
            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(kw) = &last.payload {
                    if let PyObjectPayload::Instance(ref d) = inst.payload {
                        let mut attrs = d.attrs.write();
                        for (k, v) in kw.read().iter() {
                            if let HashableKey::Str(s) = k {
                                attrs.insert(s.as_ref().clone(), v.clone());
                            }
                        }
                    }
                }
            }
            // Install per-instance __eq__ capturing self for ns.__eq__(other) calls
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let self_ref = d.attrs.clone();
                let mut attrs = d.attrs.write();
                attrs.insert(CompactString::from("__eq__"), PyObject::native_closure(
                    "SimpleNamespace.__eq__", move |eq_args: &[PyObjectRef]| {
                        if eq_args.is_empty() {
                            return Ok(PyObject::bool_val(false));
                        }
                        // When called as ns.__eq__(other), eq_args[0] is other
                        // Build a fake self from our captured attrs
                        let other = &eq_args[eq_args.len() - 1];
                        if let PyObjectPayload::Instance(ref od) = other.payload {
                            let sa = self_ref.read();
                            let oa = od.attrs.read();
                            let self_user: Vec<_> = sa.iter().filter(|(k, _)| !k.starts_with('_')).collect();
                            let other_user: Vec<_> = oa.iter().filter(|(k, _)| !k.starts_with('_')).collect();
                            if self_user.len() != other_user.len() { return Ok(PyObject::bool_val(false)); }
                            for (k, v) in &self_user {
                                if let Some(ov) = oa.get(*k) {
                                    let eq = v.compare(ov, CompareOp::Eq)?;
                                    if !eq.is_truthy() { return Ok(PyObject::bool_val(false)); }
                                } else {
                                    return Ok(PyObject::bool_val(false));
                                }
                            }
                            Ok(PyObject::bool_val(true))
                        } else {
                            Ok(PyObject::bool_val(false))
                        }
                    },
                ));
            }
            Ok(inst)
        })),
        ("new_class", make_builtin(|args| {
            check_args_min("new_class", args, 1)?;
            let name = args[0].py_to_string();
            let bases = if args.len() > 1 { args[1].to_list().unwrap_or_default() } else { vec![] };
            Ok(PyObject::class(CompactString::from(&name), bases, IndexMap::new()))
        })),
        ("prepare_class", make_builtin(|_| {
            Ok(PyObject::tuple(vec![PyObject::none(), PyObject::dict(IndexMap::new()), PyObject::dict(IndexMap::new())]))
        })),
        ("DynamicClassAttribute", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::none()); }
            Ok(args[0].clone())
        })),
        ("coroutine", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::none()); }
            Ok(args[0].clone())
        })),
    ])
}

// ── collections.abc module ──

pub fn create_collections_abc_module() -> PyObjectRef {
    // Create ABC class with a set of builtin type names that are virtual subclasses.
    // This enables `issubclass(dict, Mapping)` etc. without needing metaclass dispatch.
    let make_abc = |name: &str, builtin_types: &[&str]| -> PyObjectRef {
        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("__abstractmethods__"), PyObject::set(IndexMap::new()));
        if !builtin_types.is_empty() {
            let mut type_set = IndexMap::new();
            for t in builtin_types {
                let key = ferrython_core::types::HashableKey::str_key(CompactString::from(*t));
                type_set.insert(key, PyObject::str_val(CompactString::from(*t)));
            }
            ns.insert(CompactString::from("_abc_builtin_types"), PyObject::set(type_set));
        }
        let cls = PyObject::class(CompactString::from(name), vec![], ns);
        // Add register() method so ABCs support Mapping.register(MyClass)
        if let PyObjectPayload::Class(ref cd) = cls.payload {
            let cls_ref = cls.clone();
            let register_fn = PyObject::native_closure(
                &format!("{}.register", name),
                move |args: &[PyObjectRef]| {
                    let subclass = if args.is_empty() {
                        return Err(PyException::type_error("register() requires a subclass argument"));
                    } else {
                        args.last().unwrap().clone()
                    };
                    if let PyObjectPayload::Class(ref cd) = cls_ref.payload {
                        let mut ns = cd.namespace.write();
                        let registry = ns.entry(CompactString::from("_abc_registry"))
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
                        cd.namespace.write().insert(
                            CompactString::from("__abc_registered__"),
                            cls_ref.clone(),
                        );
                    }
                    Ok(subclass)
                },
            );
            cd.namespace.write().insert(CompactString::from("register"), register_fn);
        }
        cls
    };

    // Build Mapping and MutableMapping separately to add mixin methods
    let mapping_cls = make_abc("Mapping", &["dict"]);
    let mutable_mapping_cls = make_abc("MutableMapping", &["dict"]);

    make_module("collections.abc", vec![
        ("Hashable",        make_abc("Hashable", &["int", "float", "str", "bool", "bytes", "tuple", "frozenset", "NoneType"])),
        ("Iterable",        make_abc("Iterable", &["list", "tuple", "dict", "set", "frozenset", "str", "bytes", "bytearray", "range", "iterator", "generator"])),
        ("Iterator",        make_abc("Iterator", &["iterator", "generator"])),
        ("Reversible",      make_abc("Reversible", &["list", "dict", "range"])),
        ("Generator",       make_abc("Generator", &["generator"])),
        ("Sized",           make_abc("Sized", &["list", "tuple", "dict", "set", "frozenset", "str", "bytes", "bytearray", "range"])),
        ("Container",       make_abc("Container", &["list", "tuple", "dict", "set", "frozenset", "str", "bytes", "bytearray", "range"])),
        ("Callable",        make_abc("Callable", &["function", "builtin_function_or_method", "method"])),
        ("Collection",      make_abc("Collection", &["list", "tuple", "dict", "set", "frozenset", "str", "bytes", "bytearray", "range"])),
        ("Sequence",        make_abc("Sequence", &["list", "tuple", "str", "bytes", "bytearray", "range"])),
        ("MutableSequence", make_abc("MutableSequence", &["list", "bytearray"])),
        ("ByteString",      make_abc("ByteString", &["bytes", "bytearray"])),
        ("Set",             make_abc("Set", &["set", "frozenset"])),
        ("MutableSet",      make_abc("MutableSet", &["set"])),
        ("Mapping",         mapping_cls),
        ("MutableMapping",  mutable_mapping_cls),
        ("MappingView",     make_abc("MappingView", &[])),
        ("KeysView",        make_abc("KeysView", &[])),
        ("ItemsView",       make_abc("ItemsView", &[])),
        ("ValuesView",      make_abc("ValuesView", &[])),
        ("Awaitable",       make_abc("Awaitable", &[])),
        ("Coroutine",       make_abc("Coroutine", &[])),
        ("AsyncIterable",   make_abc("AsyncIterable", &[])),
        ("AsyncIterator",   make_abc("AsyncIterator", &[])),
        ("AsyncGenerator",  make_abc("AsyncGenerator", &[])),
        ("Buffer",          make_abc("Buffer", &["bytes", "bytearray", "memoryview"])),
    ])
}

// ── abc module ──

pub fn create_abc_module() -> PyObjectRef {
    // ABC base class with __abstractmethods__ marker
    let abc_class = PyObject::class(
        CompactString::from("ABC"),
        vec![],
        IndexMap::new(),
    );
    if let PyObjectPayload::Class(ref cd) = abc_class.payload {
        let mut ns = cd.namespace.write();
        ns.insert(
            CompactString::from("__abstractmethods__"),
            PyObject::wrap(PyObjectPayload::Set(Rc::new(PyCell::new(ferrython_core::object::new_fx_hashkey_flatmap())))),
        );
        // ABC.register(subclass) — registers subclass as a virtual subclass
        let abc_ref = abc_class.clone();
        let register_fn = PyObject::native_closure("register", move |args: &[PyObjectRef]| {
            // When called as Printable.register(MyInt), args = [MyInt]
            // When called bound, args = [Printable, MyInt]
            let (cls, subclass) = if args.len() >= 2 && matches!(&args[0].payload, PyObjectPayload::Class(_)) {
                (args[0].clone(), args[1].clone())
            } else if args.len() == 1 {
                // Called unbound: use the ABC class this register was defined on
                (abc_ref.clone(), args[0].clone())
            } else {
                return Err(PyException::type_error("register() requires a subclass argument"));
            };
            // Store virtual subclass in _abc_registry on the ABC class (Dict with Identity keys)
            if let PyObjectPayload::Class(ref cd) = cls.payload {
                let mut ns = cd.namespace.write();
                let registry = ns.entry(CompactString::from("_abc_registry"))
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
                cd.namespace.write().insert(
                    CompactString::from("__abc_registered__"),
                    cls.clone(),
                );
            }
            Ok(subclass.clone())
        });
        ns.insert(CompactString::from("register"), register_fn);
    }

    let abstractmethod_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("abstractmethod requires 1 argument"));
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
        ns.insert(CompactString::from("register"), PyObject::native_closure(
            "ABCMeta.register",
            |args: &[PyObjectRef]| {
                // args: [cls (ABCMeta instance), subclass]
                if args.len() < 2 {
                    return Err(PyException::type_error("register() requires a subclass argument"));
                }
                let cls = &args[0];
                let subclass = &args[1];
                // Store in _abc_registry on the class
                if let PyObjectPayload::Class(cd) = &cls.payload {
                    let mut ns = cd.namespace.write();
                    let registry = ns.entry(CompactString::from("_abc_registry"))
                        .or_insert_with(|| {
                            PyObject::dict(IndexMap::new())
                        }).clone();
                    if let PyObjectPayload::Dict(map) = &registry.payload {
                        let ptr = PyObjectRef::as_ptr(subclass) as usize;
                        let key = HashableKey::Identity(ptr, subclass.clone());
                        map.write().insert(key, PyObject::bool_val(true));
                    }
                }
                Ok(subclass.clone())
            },
        ));
        PyObject::class(CompactString::from("ABCMeta"), vec![PyObject::builtin_type(CompactString::from("type"))], ns)
    };

    let abstractclassmethod_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("abstractclassmethod requires 1 argument"));
        }
        Ok(args[0].clone())
    });

    let abstractstaticmethod_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("abstractstaticmethod requires 1 argument"));
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
    let get_cache_token_fn = PyObject::native_closure(
        "abc.get_cache_token", move |_args: &[PyObjectRef]| {
            Ok(PyObject::int(*cache_token.read()))
        }
    );

    make_module("abc", vec![
        ("ABC", abc_class),
        ("ABCMeta", abcmeta_cls),
        ("abstractmethod", abstractmethod_fn),
        ("abstractclassmethod", abstractclassmethod_fn),
        ("abstractstaticmethod", abstractstaticmethod_fn),
        ("abstractproperty", abstractproperty_fn),
        ("get_cache_token", get_cache_token_fn),
    ])
}
