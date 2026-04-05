//! Type-system stdlib modules (typing, abc, enum, types, collections.abc)

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin, check_args, check_args_min, CompareOp,
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
                        PyObjectPayload::Tuple(items) => PyObject::tuple(items.clone()),
                        _ => PyObject::tuple(vec![params.clone()]),
                    };

                    let params_str = if args.len() >= 2 {
                        args[1].py_to_string()
                    } else if args.len() == 1 {
                        args[0].py_to_string()
                    } else {
                        "?".to_string()
                    };
                    let repr = format!("typing.{}[{}]", origin_display, params_str);

                    let cls = PyObject::class(CompactString::from("_GenericAlias"), vec![], IndexMap::new());
                    let mut attrs = IndexMap::new();
                    attrs.insert(CompactString::from("__origin__"), PyObject::str_val(CompactString::from(origin_display.as_str())));
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
        ("TypeVar", typevar_fn),
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
                ns.insert(CompactString::from("__protocol_attrs__"), PyObject::tuple(protocol_attrs));
                ns.insert(CompactString::from("_is_runtime_checkable"), PyObject::bool_val(true));
            }
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
        ("ModuleType", PyObject::builtin_type(CompactString::from("module"))),
        ("GeneratorType", PyObject::builtin_type(CompactString::from("generator"))),
        ("CodeType", PyObject::builtin_type(CompactString::from("code"))),
        ("FrameType", PyObject::builtin_type(CompactString::from("frame"))),
        ("TracebackType", PyObject::builtin_type(CompactString::from("traceback"))),
        ("CoroutineType", PyObject::builtin_type(CompactString::from("coroutine"))),
        ("AsyncGeneratorType", PyObject::builtin_type(CompactString::from("async_generator"))),
        ("MappingProxyType", PyObject::builtin_type(CompactString::from("mappingproxy"))),
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
                                attrs.insert(s.clone(), v.clone());
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
                let key = ferrython_core::types::HashableKey::Str(CompactString::from(*t));
                type_set.insert(key, PyObject::str_val(CompactString::from(*t)));
            }
            ns.insert(CompactString::from("_abc_builtin_types"), PyObject::set(type_set));
        }
        PyObject::class(CompactString::from(name), vec![], ns)
    };
    make_module("collections.abc", vec![
        ("Hashable",        make_abc("Hashable", &["int", "float", "str", "bool", "bytes", "tuple", "frozenset", "NoneType"])),
        ("Iterable",        make_abc("Iterable", &["list", "tuple", "dict", "set", "frozenset", "str", "bytes", "bytearray", "range"])),
        ("Iterator",        make_abc("Iterator", &[])),
        ("Reversible",      make_abc("Reversible", &["list", "dict", "range"])),
        ("Generator",       make_abc("Generator", &[])),
        ("Sized",           make_abc("Sized", &["list", "tuple", "dict", "set", "frozenset", "str", "bytes", "bytearray", "range"])),
        ("Container",       make_abc("Container", &["list", "tuple", "dict", "set", "frozenset", "str", "bytes", "bytearray", "range"])),
        ("Callable",        make_abc("Callable", &[])),
        ("Collection",      make_abc("Collection", &["list", "tuple", "dict", "set", "frozenset", "str", "bytes", "bytearray", "range"])),
        ("Sequence",        make_abc("Sequence", &["list", "tuple", "str", "bytes", "bytearray", "range"])),
        ("MutableSequence", make_abc("MutableSequence", &["list", "bytearray"])),
        ("ByteString",      make_abc("ByteString", &["bytes", "bytearray"])),
        ("Set",             make_abc("Set", &["set", "frozenset"])),
        ("MutableSet",      make_abc("MutableSet", &["set"])),
        ("Mapping",         make_abc("Mapping", &["dict"])),
        ("MutableMapping",  make_abc("MutableMapping", &["dict"])),
        ("MappingView",     make_abc("MappingView", &[])),
        ("KeysView",        make_abc("KeysView", &[])),
        ("ItemsView",       make_abc("ItemsView", &[])),
        ("ValuesView",      make_abc("ValuesView", &[])),
        ("Awaitable",       make_abc("Awaitable", &[])),
        ("Coroutine",       make_abc("Coroutine", &[])),
        ("AsyncIterable",   make_abc("AsyncIterable", &[])),
        ("AsyncIterator",   make_abc("AsyncIterator", &[])),
        ("AsyncGenerator",  make_abc("AsyncGenerator", &[])),
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
