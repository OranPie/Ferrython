//! Type-system stdlib modules (typing, abc, enum, types, collections.abc)

use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin, check_args, check_args_min,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

pub fn create_typing_module() -> PyObjectRef {
    // TypeVar(name) — returns a placeholder object with __name__
    let typevar_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("TypeVar", args, 1)?;
        let name = CompactString::from(args[0].py_to_string());
        let cls = PyObject::class(CompactString::from("TypeVar"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("__name__"), PyObject::str_val(name));
        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

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
    // These support subscript notation: List[int] → _GenericAlias("List", (int,))
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
                    // args[0] = cls, args[1] = params
                    let params_str = if args.len() >= 2 {
                        args[1].py_to_string()
                    } else if args.len() == 1 {
                        args[0].py_to_string()
                    } else {
                        "?".to_string()
                    };
                    let repr = format!("typing.{}[{}]", display, params_str);
                    Ok(PyObject::str_val(CompactString::from(repr)))
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
        ("TypeVar", typevar_fn),
        ("Generic", generic_class),
        ("Protocol", protocol_class),
        ("ClassVar", make_typing_alias("ClassVar")),
        ("Final", make_typing_alias("Final")),
        ("Literal", make_typing_alias("Literal")),
        ("NamedTuple", PyObject::builtin_type(CompactString::from("NamedTuple"))),
        ("get_type_hints", make_builtin(|args: &[PyObjectRef]| {
            // Return __annotations__ dict from the function/class, or empty dict
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
            Ok(args[0].clone())
        })),
    ];
    attrs.push(("TYPE_CHECKING", PyObject::bool_val(false)));
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
    attrs.push(("TypedDict", make_typing_alias("TypedDict")));
    attrs.push(("ForwardRef", make_typing_alias("ForwardRef")));
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
                        let key = HashableKey::Str(CompactString::from(name.as_str()));
                        if let Some(member) = map.read().get(&key) {
                            return Ok(member.clone());
                        }
                    }
                }
            }
            Err(PyException::key_error(format!("'{}'", name)))
        }
    ));

    // __call__ on class — Color(1) looks up member by value
    enum_ns.insert(CompactString::from("__call__"), PyObject::native_function(
        "Enum.__call__", |args: &[PyObjectRef]| {
            check_args_min("Enum.__call__", args, 2)?;
            let cls = &args[0];
            let value = &args[1];
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
                            if Arc::ptr_eq(member, item) {
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

    // IntEnum — Enum subclass where values are ints
    let mut int_enum_ns = IndexMap::new();
    int_enum_ns.insert(CompactString::from("__enum__"), PyObject::bool_val(true));
    int_enum_ns.insert(CompactString::from("__int_enum__"), PyObject::bool_val(true));
    let int_enum = PyObject::class(
        CompactString::from("IntEnum"),
        vec![enum_class.clone(), PyObject::builtin_type(CompactString::from("int"))],
        int_enum_ns,
    );

    // Flag — class with bitwise support marker
    let mut flag_ns = IndexMap::new();
    flag_ns.insert(CompactString::from("__enum__"), PyObject::bool_val(true));
    flag_ns.insert(CompactString::from("__flag__"), PyObject::bool_val(true));
    let flag_class = PyObject::class(
        CompactString::from("Flag"),
        vec![enum_class.clone(), PyObject::builtin_type(CompactString::from("int"))],
        flag_ns,
    );

    // IntFlag — Flag subclass
    let mut int_flag_ns = IndexMap::new();
    int_flag_ns.insert(CompactString::from("__enum__"), PyObject::bool_val(true));
    int_flag_ns.insert(CompactString::from("__flag__"), PyObject::bool_val(true));
    let int_flag_class = PyObject::class(
        CompactString::from("IntFlag"),
        vec![flag_class.clone()],
        int_flag_ns,
    );

    // auto() counter
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

    make_module("enum", vec![
        ("Enum", enum_class),
        ("IntEnum", int_enum),
        ("Flag", flag_class),
        ("IntFlag", int_flag_class),
        ("auto", make_builtin(|_| {
            let val = AUTO_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(PyObject::int(val))
        })),
        ("unique", unique_fn),
    ])
}

// ── types module ──

pub fn create_types_module() -> PyObjectRef {
    make_module("types", vec![
        ("NoneType", PyObject::builtin_type(CompactString::from("NoneType"))),
        ("FunctionType", PyObject::builtin_type(CompactString::from("function"))),
        ("LambdaType", PyObject::builtin_type(CompactString::from("function"))),
        ("BuiltinFunctionType", PyObject::builtin_type(CompactString::from("builtin_function_or_method"))),
        ("BuiltinMethodType", PyObject::builtin_type(CompactString::from("builtin_function_or_method"))),
        ("MethodType", PyObject::builtin_type(CompactString::from("method"))),
        ("ModuleType", PyObject::builtin_type(CompactString::from("module"))),
        ("GeneratorType", PyObject::builtin_type(CompactString::from("generator"))),
        ("CodeType", PyObject::builtin_type(CompactString::from("code"))),
        ("FrameType", PyObject::builtin_type(CompactString::from("frame"))),
        ("TracebackType", PyObject::builtin_type(CompactString::from("traceback"))),
        ("CoroutineType", PyObject::builtin_type(CompactString::from("coroutine"))),
        ("AsyncGeneratorType", PyObject::builtin_type(CompactString::from("async_generator"))),
        ("MappingProxyType", PyObject::builtin_type(CompactString::from("mappingproxy"))),
        ("SimpleNamespace", make_builtin(|args| {
            let cls = PyObject::class(CompactString::from("SimpleNamespace"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(kw) = &last.payload {
                    if let PyObjectPayload::Instance(ref d) = inst.payload {
                        let mut attrs = d.attrs.write();
                        for (k, v) in kw.read().iter() {
                            if let HashableKey::Str(s) = k {
                                attrs.insert(s.clone(), v.clone());
                            }
                        }
                    }
                }
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
    let make_abc = |name: &str| -> PyObjectRef {
        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("__abstractmethods__"), PyObject::set(IndexMap::new()));
        PyObject::class(CompactString::from(name), vec![], ns)
    };
    make_module("collections.abc", vec![
        ("Hashable", make_abc("Hashable")),
        ("Iterable", make_abc("Iterable")),
        ("Iterator", make_abc("Iterator")),
        ("Reversible", make_abc("Reversible")),
        ("Generator", make_abc("Generator")),
        ("Sized", make_abc("Sized")),
        ("Container", make_abc("Container")),
        ("Callable", make_abc("Callable")),
        ("Collection", make_abc("Collection")),
        ("Sequence", make_abc("Sequence")),
        ("MutableSequence", make_abc("MutableSequence")),
        ("ByteString", make_abc("ByteString")),
        ("Set", make_abc("Set")),
        ("MutableSet", make_abc("MutableSet")),
        ("Mapping", make_abc("Mapping")),
        ("MutableMapping", make_abc("MutableMapping")),
        ("MappingView", make_abc("MappingView")),
        ("KeysView", make_abc("KeysView")),
        ("ItemsView", make_abc("ItemsView")),
        ("ValuesView", make_abc("ValuesView")),
        ("Awaitable", make_abc("Awaitable")),
        ("Coroutine", make_abc("Coroutine")),
        ("AsyncIterable", make_abc("AsyncIterable")),
        ("AsyncIterator", make_abc("AsyncIterator")),
        ("AsyncGenerator", make_abc("AsyncGenerator")),
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
            PyObject::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(IndexMap::new())))),
        );
        // ABC.register(subclass) — registers subclass as a virtual subclass
        let register_fn = make_builtin(|args: &[PyObjectRef]| {
            // args[0] = cls (the ABC), args[1] = subclass
            if args.len() < 2 {
                return Err(PyException::type_error("register() requires a subclass argument"));
            }
            let cls = &args[0];
            let subclass = &args[1];
            // Store virtual subclass in __abc_registry__ on the ABC class
            if let PyObjectPayload::Class(ref cd) = cls.payload {
                let mut ns = cd.namespace.write();
                let registry = ns.entry(CompactString::from("__abc_registry__"))
                    .or_insert_with(|| PyObject::list(vec![]));
                if let PyObjectPayload::List(ref list) = registry.payload {
                    list.write().push(subclass.clone());
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

    let abcmeta_cls = PyObject::class(
        CompactString::from("ABCMeta"), vec![], IndexMap::new(),
    );

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

    let cache_token: Arc<RwLock<i64>> = Arc::new(RwLock::new(0));
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
