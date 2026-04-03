//! Miscellaneous stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, CompareOp, InstanceData,
    make_module, make_builtin, check_args, check_args_min,
};
use ferrython_core::types::{HashableKey, PyInt};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::cell::RefCell;
use std::sync::Arc;

use super::serial_modules::extract_bytes;

// Deferred call mechanism for NativeClosures that need the VM to call Python functions.
// Thread.start() pushes (target, args) here; the VM drains and executes them after NativeClosure returns.
thread_local! {
    pub static DEFERRED_CALLS: RefCell<Vec<(PyObjectRef, Vec<PyObjectRef>)>> = RefCell::new(Vec::new());
}

pub fn push_deferred_call(func: PyObjectRef, args: Vec<PyObjectRef>) {
    DEFERRED_CALLS.with(|dc| dc.borrow_mut().push((func, args)));
}

pub fn drain_deferred_calls() -> Vec<(PyObjectRef, Vec<PyObjectRef>)> {
    DEFERRED_CALLS.with(|dc| std::mem::take(&mut *dc.borrow_mut()))
}

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
        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("__typing_name__"), PyObject::str_val(name));
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
    make_module("typing", attrs)
}

// ── abc module (stub) ──


pub fn create_abc_module() -> PyObjectRef {
    // ABC base class with __abstract_methods__ marker
    let abc_class = PyObject::class(
        CompactString::from("ABC"),
        vec![],
        IndexMap::new(),
    );
    if let PyObjectPayload::Class(ref cd) = abc_class.payload {
        cd.namespace.write().insert(
            CompactString::from("__abstractmethods__"),
            PyObject::wrap(PyObjectPayload::Set(Arc::new(RwLock::new(IndexMap::new())))),
        );
    }

    make_module("abc", vec![
        ("ABC", abc_class),
        ("ABCMeta", PyObject::none()),
        ("abstractmethod", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("abstractmethod requires 1 argument")); }
            // Wrap function to mark it as abstract — store original with __isabstractmethod__ flag
            let func = args[0].clone();
            // Create a wrapper that carries the flag; the function itself is the wrapper
            // We attach __isabstractmethod__ as a class-level attribute during class creation
            // For now, return a special tuple marker: ("__abstract__", func)
            let marker = PyObject::tuple(vec![
                PyObject::str_val(CompactString::from("__abstract__")),
                func,
            ]);
            Ok(marker)
        })),
    ])
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

// ── contextlib module ──


pub fn create_contextlib_module() -> PyObjectRef {
    // suppress(*exceptions) — context manager that suppresses specified exceptions
    let suppress_fn = make_builtin(|args: &[PyObjectRef]| {
        let exceptions: Vec<PyObjectRef> = args.to_vec();
        let suppress_cls = PyObject::class(
            CompactString::from("suppress"),
            vec![],
            IndexMap::new(),
        );
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("__suppress_exceptions__"), PyObject::list(exceptions));
        attrs.insert(CompactString::from("__enter__"), PyObject::native_function(
            "suppress.__enter__", |args: &[PyObjectRef]| {
                if args.is_empty() { return Ok(PyObject::none()); }
                Ok(args[0].clone())
            }
        ));
        attrs.insert(CompactString::from("__exit__"), PyObject::native_function(
            "suppress.__exit__", |args: &[PyObjectRef]| {
                // args: self, exc_type, exc_val, exc_tb
                if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
                    let exc_type = &args[1];
                    let exc_name = exc_type.py_to_string();
                    // Check if exc_type matches any of the suppressed exceptions
                    if let Some(suppressed) = args[0].get_attr("__suppress_exceptions__") {
                        if let Ok(exc_list) = suppressed.to_list() {
                            for allowed in &exc_list {
                                let allowed_name = allowed.py_to_string();
                                // Direct match or hierarchy check (Exception catches all std exceptions)
                                if exc_name == allowed_name {
                                    return Ok(PyObject::bool_val(true));
                                }
                                // Check if allowed is a parent exception type
                                if allowed_name.contains("Exception") && !exc_name.contains("BaseException") {
                                    return Ok(PyObject::bool_val(true));
                                }
                                if allowed_name.contains("BaseException") {
                                    return Ok(PyObject::bool_val(true));
                                }
                                // Common hierarchies
                                let is_subclass = match allowed_name.as_str() {
                                    s if s.contains("ValueError") => exc_name.contains("ValueError"),
                                    s if s.contains("TypeError") => exc_name.contains("TypeError"),
                                    s if s.contains("KeyError") => exc_name.contains("KeyError"),
                                    s if s.contains("IndexError") => exc_name.contains("IndexError"),
                                    s if s.contains("LookupError") => exc_name.contains("KeyError") || exc_name.contains("IndexError"),
                                    s if s.contains("ArithmeticError") => exc_name.contains("ZeroDivisionError") || exc_name.contains("OverflowError"),
                                    _ => false,
                                };
                                if is_subclass {
                                    return Ok(PyObject::bool_val(true));
                                }
                            }
                        }
                    }
                }
                Ok(PyObject::bool_val(false))
            }
        ));
        Ok(PyObject::instance_with_attrs(suppress_cls, attrs))
    });

    // ExitStack — real context manager with callback registration
    let exit_stack_cls = {
        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("__exitstack__"), PyObject::bool_val(true));
        PyObject::class(CompactString::from("ExitStack"), vec![], ns)
    };

    let exit_stack_cls_clone = exit_stack_cls.clone();
    let exit_stack_fn = PyObject::native_closure("ExitStack", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(exit_stack_cls_clone.clone());
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(CompactString::from("_callbacks"), PyObject::list(vec![]));

            attrs.insert(CompactString::from("__enter__"), PyObject::native_function(
                "ExitStack.__enter__", |args: &[PyObjectRef]| {
                    if args.is_empty() { return Ok(PyObject::none()); }
                    Ok(args[0].clone())
                }
            ));

            attrs.insert(CompactString::from("__exit__"), PyObject::native_function(
                "ExitStack.__exit__", |args: &[PyObjectRef]| {
                    // Call registered callbacks in reverse order (best-effort for native fns)
                    if let Some(self_obj) = args.first() {
                        if let Some(cbs) = self_obj.get_attr("_callbacks") {
                            if let Ok(items) = cbs.to_list() {
                                for cb in items.iter().rev() {
                                    match &cb.payload {
                                        PyObjectPayload::NativeFunction { func, .. } => {
                                            let _ = func(&[]);
                                        }
                                        PyObjectPayload::NativeClosure { func, .. } => {
                                            let _ = func(&[]);
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    Ok(PyObject::bool_val(false))
                }
            ));

            attrs.insert(CompactString::from("push"), PyObject::native_function(
                "ExitStack.push", |args: &[PyObjectRef]| {
                    check_args_min("ExitStack.push", args, 2)?;
                    let self_obj = &args[0];
                    let callback = &args[1];
                    if let Some(cbs) = self_obj.get_attr("_callbacks") {
                        if let PyObjectPayload::List(items) = &cbs.payload {
                            items.write().push(callback.clone());
                        }
                    }
                    Ok(callback.clone())
                }
            ));

            attrs.insert(CompactString::from("callback"), PyObject::native_function(
                "ExitStack.callback", |args: &[PyObjectRef]| {
                    check_args_min("ExitStack.callback", args, 2)?;
                    let self_obj = &args[0];
                    let func = &args[1];
                    if let Some(cbs) = self_obj.get_attr("_callbacks") {
                        if let PyObjectPayload::List(items) = &cbs.payload {
                            items.write().push(func.clone());
                        }
                    }
                    Ok(func.clone())
                }
            ));

            attrs.insert(CompactString::from("enter_context"), PyObject::native_function(
                "ExitStack.enter_context", |args: &[PyObjectRef]| {
                    check_args_min("ExitStack.enter_context", args, 2)?;
                    let self_obj = &args[0];
                    let cm = &args[1];
                    // Call __enter__ if it's a native function
                    let result = if let Some(enter) = cm.get_attr("__enter__") {
                        match &enter.payload {
                            PyObjectPayload::NativeFunction { func, .. } => {
                                func(&[cm.clone()])?
                            }
                            PyObjectPayload::NativeClosure { func, .. } => {
                                func(&[cm.clone()])?
                            }
                            _ => PyObject::none()
                        }
                    } else {
                        PyObject::none()
                    };
                    // Register __exit__ as callback
                    if let Some(exit_fn) = cm.get_attr("__exit__") {
                        if let Some(cbs) = self_obj.get_attr("_callbacks") {
                            if let PyObjectPayload::List(items) = &cbs.payload {
                                items.write().push(exit_fn);
                            }
                        }
                    }
                    Ok(result)
                }
            ));
        }
        Ok(inst)
    });

    make_module("contextlib", vec![
        ("contextmanager", make_builtin(contextlib_contextmanager)),
        ("suppress", suppress_fn),
        ("closing", make_builtin(|args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("closing requires 1 argument")); }
            Ok(args[0].clone())
        })),
        ("ExitStack", exit_stack_fn),
        ("redirect_stdout", make_builtin(|_| Ok(PyObject::none()))),
        ("redirect_stderr", make_builtin(|_| Ok(PyObject::none()))),
    ])
}

fn contextlib_contextmanager(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // contextmanager decorator — returns the function unchanged.
    // The function is a generator function. When called, it returns a Generator.
    // The VM's SetupWith handles Generator objects as context managers directly.
    if args.is_empty() { return Err(PyException::type_error("contextmanager requires 1 argument")); }
    Ok(args[0].clone())
}

// ── dataclasses module ──


pub fn create_dataclasses_module() -> PyObjectRef {
    make_module("dataclasses", vec![
        ("dataclass", make_builtin(dataclass_decorator)),
        ("field", make_builtin(|args| {
            // field(default=..., default_factory=..., ...)
            // kwargs passed as trailing dict by VM
            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(kw_map) = &last.payload {
                    let r = kw_map.read();
                    // Check for default_factory
                    if let Some(factory) = r.get(&HashableKey::Str(CompactString::from("default_factory"))) {
                        // Return a sentinel with __field_factory__ marker
                        let mut attrs = IndexMap::new();
                        attrs.insert(CompactString::from("__field_factory__"), factory.clone());
                        return Ok(PyObject::module_with_attrs(CompactString::from("_field"), attrs));
                    }
                    if let Some(default) = r.get(&HashableKey::Str(CompactString::from("default"))) {
                        return Ok(default.clone());
                    }
                }
            }
            let default = if args.is_empty() { PyObject::none() } else { args[0].clone() };
            Ok(default)
        })),
        ("asdict", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("asdict requires 1 argument")); }
            // Convert instance attrs to dict
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                let attrs = inst.attrs.read();
                let mut map = IndexMap::new();
                for (k, v) in attrs.iter() {
                    if !k.starts_with('_') {
                        map.insert(HashableKey::Str(k.clone()), v.clone());
                    }
                }
                Ok(PyObject::dict(map))
            } else {
                Err(PyException::type_error("asdict() should be called on dataclass instances"))
            }
        })),
        ("astuple", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("astuple requires 1 argument")); }
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                let attrs = inst.attrs.read();
                let items: Vec<_> = attrs.values().cloned().collect();
                Ok(PyObject::tuple(items))
            } else {
                Err(PyException::type_error("astuple() should be called on dataclass instances"))
            }
        })),
        ("fields", make_builtin(|_| Ok(PyObject::tuple(vec![])))),
    ])
}

fn dataclass_decorator(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("dataclass requires 1 argument")); }
    let cls = &args[0];
    
    // Get annotations to discover fields
    let mut field_names: Vec<CompactString> = Vec::new();
    let mut field_defaults: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    
    if let PyObjectPayload::Class(cd) = &cls.payload {
        let ns = cd.namespace.read();
        if let Some(annotations) = ns.get("__annotations__") {
            if let PyObjectPayload::Dict(ann_map) = &annotations.payload {
                for (k, _v) in ann_map.read().iter() {
                    if let HashableKey::Str(name) = k {
                        field_names.push(name.clone());
                        // Check for default value in class namespace
                        if let Some(default) = ns.get(name.as_str()) {
                            // Check if it's a field() sentinel with factory
                            if let Some(factory) = default.get_attr("__field_factory__") {
                                field_defaults.insert(name.clone(), factory);
                            } else {
                                field_defaults.insert(name.clone(), default.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    
    // Store __dataclass_fields__ as a tuple of (name, has_default, default_val) tuples
    let fields_info: Vec<PyObjectRef> = field_names.iter().map(|name| {
        let has_default = field_defaults.contains_key(name.as_str());
        let default_val = field_defaults.get(name.as_str()).cloned().unwrap_or_else(PyObject::none);
        PyObject::tuple(vec![
            PyObject::str_val(CompactString::from(name.as_str())),
            PyObject::bool_val(has_default),
            default_val,
        ])
    }).collect();
    
    // Store on the class
    if let PyObjectPayload::Class(cd) = &cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(CompactString::from("__dataclass_fields__"), PyObject::tuple(fields_info));
        // Mark it as a dataclass
        ns.insert(CompactString::from("__dataclass__"), PyObject::bool_val(true));
    }
    
    Ok(cls.clone())
}

// ── struct module ──


pub fn create_copy_module() -> PyObjectRef {
    make_module("copy", vec![
        ("copy", make_builtin(copy_copy)),
        ("deepcopy", make_builtin(copy_deepcopy)),
    ])
}

fn copy_copy(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("copy() requires 1 argument")); }
    shallow_copy(&args[0])
}

fn copy_deepcopy(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("deepcopy() requires 1 argument")); }
    deep_copy(&args[0])
}

fn shallow_copy(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::None | PyObjectPayload::Bool(_) | PyObjectPayload::Int(_)
        | PyObjectPayload::Float(_) | PyObjectPayload::Str(_) | PyObjectPayload::Bytes(_)
        | PyObjectPayload::FrozenSet(_) => Ok(obj.clone()),
        PyObjectPayload::Tuple(items) => Ok(PyObject::tuple(items.clone())),
        PyObjectPayload::List(items) => Ok(PyObject::list(items.read().clone())),
        PyObjectPayload::Dict(map) => Ok(PyObject::dict(map.read().clone())),
        PyObjectPayload::Set(set) => Ok(PyObject::set(set.read().clone())),
        PyObjectPayload::Instance(inst) => {
            // Create new instance with same class, shallow copy of attrs
            Ok(PyObject::wrap(PyObjectPayload::Instance(InstanceData {
                class: inst.class.clone(),
                attrs: Arc::new(RwLock::new(inst.attrs.read().clone())),
                dict_storage: inst.dict_storage.as_ref().map(|ds| Arc::new(RwLock::new(ds.read().clone()))),
            })))
        }
        _ => Ok(obj.clone()),
    }
}

fn deep_copy(obj: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &obj.payload {
        PyObjectPayload::None | PyObjectPayload::Bool(_) | PyObjectPayload::Int(_)
        | PyObjectPayload::Float(_) | PyObjectPayload::Str(_) | PyObjectPayload::Bytes(_)
        | PyObjectPayload::FrozenSet(_) => Ok(obj.clone()),
        PyObjectPayload::Tuple(items) => {
            let new_items: Result<Vec<_>, _> = items.iter().map(|x| deep_copy(x)).collect();
            Ok(PyObject::tuple(new_items?))
        }
        PyObjectPayload::List(items) => {
            let new_items: Result<Vec<_>, _> = items.read().iter().map(|x| deep_copy(x)).collect();
            Ok(PyObject::list(new_items?))
        }
        PyObjectPayload::Dict(map) => {
            let mut new_map = IndexMap::new();
            for (k, v) in map.read().iter() {
                new_map.insert(k.clone(), deep_copy(v)?);
            }
            Ok(PyObject::dict(new_map))
        }
        PyObjectPayload::Set(set) => {
            let mut new_set = IndexMap::new();
            for (k, v) in set.read().iter() {
                new_set.insert(k.clone(), deep_copy(v)?);
            }
            Ok(PyObject::set(new_set))
        }
        PyObjectPayload::Instance(inst) => {
            let mut new_attrs = IndexMap::new();
            for (k, v) in inst.attrs.read().iter() {
                new_attrs.insert(k.clone(), deep_copy(v)?);
            }
            Ok(PyObject::instance_with_attrs(inst.class.clone(), new_attrs))
        }
        _ => Ok(obj.clone()),
    }
}

// ── operator module ──


pub fn create_operator_module() -> PyObjectRef {
    make_module("operator", vec![
        ("add", make_builtin(|args| {
            check_args_min("add", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    return Ok(PyObject::int(a + b));
                }
            }
            if let (Ok(a), Ok(b)) = (args[0].to_float(), args[1].to_float()) {
                Ok(PyObject::float(a + b))
            } else {
                let a = args[0].py_to_string();
                let b = args[1].py_to_string();
                Ok(PyObject::str_val(CompactString::from(format!("{}{}", a, b))))
            }
        })),
        ("sub", make_builtin(|args| {
            check_args_min("sub", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    return Ok(PyObject::int(a - b));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a - b))
        })),
        ("mul", make_builtin(|args| {
            check_args_min("mul", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    return Ok(PyObject::int(a * b));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a * b))
        })),
        ("truediv", make_builtin(|args| {
            check_args_min("truediv", args, 2)?;
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            if b == 0.0 { return Err(PyException::zero_division_error("division by zero")); }
            Ok(PyObject::float(a / b))
        })),
        ("floordiv", make_builtin(|args| {
            check_args_min("floordiv", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b == 0 { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                    return Ok(PyObject::int(a.div_euclid(b)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            if b == 0.0 { return Err(PyException::zero_division_error("float floor division by zero")); }
            Ok(PyObject::float((a / b).floor()))
        })),
        ("mod_", make_builtin(|args| {
            check_args_min("mod_", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b == 0 { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                    return Ok(PyObject::int(a.rem_euclid(b)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a % b))
        })),
        // Also register as "mod" for getattr(operator, "mod") usage
        ("mod", make_builtin(|args| {
            check_args_min("mod", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b == 0 { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                    return Ok(PyObject::int(a.rem_euclid(b)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a % b))
        })),
        ("neg", make_builtin(|args| {
            check_args_min("neg", args, 1)?;
            if matches!(&args[0].payload, PyObjectPayload::Float(_)) {
                Ok(PyObject::float(-args[0].to_float()?))
            } else if let Ok(n) = args[0].to_int() {
                Ok(PyObject::int(-n))
            } else {
                Ok(PyObject::float(-args[0].to_float()?))
            }
        })),
        ("pow", make_builtin(|args| {
            check_args_min("pow", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b >= 0 {
                        return Ok(PyObject::int(a.pow(b as u32)));
                    }
                    return Ok(PyObject::float((a as f64).powf(b as f64)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a.powf(b)))
        })),
        ("pos", make_builtin(|args| {
            check_args_min("pos", args, 1)?;
            Ok(args[0].clone())
        })),
        ("not_", make_builtin(|args| {
            check_args_min("not_", args, 1)?;
            Ok(PyObject::bool_val(!args[0].is_truthy()))
        })),
        ("eq", make_builtin(|args| {
            check_args_min("eq", args, 2)?;
            args[0].compare(&args[1], CompareOp::Eq)
        })),
        ("ne", make_builtin(|args| {
            check_args_min("ne", args, 2)?;
            args[0].compare(&args[1], CompareOp::Ne)
        })),
        ("lt", make_builtin(|args| {
            check_args_min("lt", args, 2)?;
            args[0].compare(&args[1], CompareOp::Lt)
        })),
        ("le", make_builtin(|args| {
            check_args_min("le", args, 2)?;
            args[0].compare(&args[1], CompareOp::Le)
        })),
        ("gt", make_builtin(|args| {
            check_args_min("gt", args, 2)?;
            args[0].compare(&args[1], CompareOp::Gt)
        })),
        ("ge", make_builtin(|args| {
            check_args_min("ge", args, 2)?;
            args[0].compare(&args[1], CompareOp::Ge)
        })),
        ("abs", make_builtin(|args| {
            check_args_min("abs", args, 1)?;
            check_args("abs", args, 1)?;
            args[0].py_abs()
        })),
        ("contains", make_builtin(|args| {
            check_args_min("contains", args, 2)?;
            Ok(PyObject::bool_val(args[0].contains(&args[1])?))
        })),
        ("getitem", make_builtin(|args| {
            check_args_min("getitem", args, 2)?;
            match &args[0].payload {
                PyObjectPayload::List(items) => {
                    let idx = args[1].to_int()? as usize;
                    items.read().get(idx).cloned()
                        .ok_or_else(|| PyException::index_error("list index out of range"))
                }
                PyObjectPayload::Dict(map) => {
                    let key = args[1].to_hashable_key()?;
                    map.read().get(&key).cloned()
                        .ok_or_else(|| PyException::key_error(args[1].repr()))
                }
                PyObjectPayload::Tuple(items) => {
                    let idx = args[1].to_int()? as usize;
                    items.get(idx).cloned()
                        .ok_or_else(|| PyException::index_error("tuple index out of range"))
                }
                _ => Err(PyException::type_error("object is not subscriptable")),
            }
        })),
        ("itemgetter", make_builtin(|args| {
            check_args_min("itemgetter", args, 1)?;
            let keys: Vec<PyObjectRef> = args.to_vec();
            Ok(PyObject::native_closure("operator.itemgetter", move |call_args| {
                if call_args.is_empty() {
                    return Err(PyException::type_error("itemgetter expected 1 argument, got 0"));
                }
                let obj = &call_args[0];
                if keys.len() == 1 {
                    obj.get_item(&keys[0])
                } else {
                    let items: Vec<PyObjectRef> = keys.iter()
                        .map(|k| obj.get_item(k))
                        .collect::<PyResult<Vec<_>>>()?;
                    Ok(PyObject::tuple(items))
                }
            }))
        })),
        ("attrgetter", make_builtin(|args| {
            check_args_min("attrgetter", args, 1)?;
            let attr_names: Vec<String> = args.iter().map(|a| a.py_to_string()).collect();
            Ok(PyObject::native_closure("operator.attrgetter", move |call_args| {
                if call_args.is_empty() {
                    return Err(PyException::type_error("attrgetter expected 1 argument, got 0"));
                }
                let obj = &call_args[0];
                if attr_names.len() == 1 {
                    obj.get_attr(&attr_names[0])
                        .ok_or_else(|| PyException::attribute_error(format!(
                            "'{}' object has no attribute '{}'", obj.type_name(), attr_names[0]
                        )))
                } else {
                    let items: Vec<PyObjectRef> = attr_names.iter()
                        .map(|name| obj.get_attr(name).ok_or_else(|| PyException::attribute_error(
                            format!("'{}' object has no attribute '{}'", obj.type_name(), name)
                        )))
                        .collect::<PyResult<Vec<_>>>()?;
                    Ok(PyObject::tuple(items))
                }
            }))
        })),
        ("and_", make_builtin(|args| {
            check_args_min("and_", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a & b))
        })),
        ("or_", make_builtin(|args| {
            check_args_min("or_", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a | b))
        })),
        ("xor", make_builtin(|args| {
            check_args_min("xor", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a ^ b))
        })),
        ("lshift", make_builtin(|args| {
            check_args_min("lshift", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a << b))
        })),
        ("rshift", make_builtin(|args| {
            check_args_min("rshift", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a >> b))
        })),
        ("invert", make_builtin(|args| {
            check_args_min("invert", args, 1)?;
            let a = args[0].to_int()?;
            Ok(PyObject::int(!a))
        })),
        ("inv", make_builtin(|args| {
            check_args_min("inv", args, 1)?;
            let a = args[0].to_int()?;
            Ok(PyObject::int(!a))
        })),
        ("truth", make_builtin(|args| {
            check_args_min("truth", args, 1)?;
            Ok(PyObject::bool_val(args[0].is_truthy()))
        })),
        ("is_", make_builtin(|args| {
            check_args_min("is_", args, 2)?;
            Ok(PyObject::bool_val(std::sync::Arc::ptr_eq(&args[0], &args[1])))
        })),
        ("is_not", make_builtin(|args| {
            check_args_min("is_not", args, 2)?;
            Ok(PyObject::bool_val(!std::sync::Arc::ptr_eq(&args[0], &args[1])))
        })),
        ("index", make_builtin(|args| {
            check_args_min("index", args, 1)?;
            args[0].to_int().map(PyObject::int)
        })),
        ("setitem", make_builtin(|args| {
            check_args_min("setitem", args, 3)?;
            match &args[0].payload {
                PyObjectPayload::List(items) => {
                    let idx = args[1].to_int()? as usize;
                    let mut w = items.write();
                    if idx < w.len() {
                        w[idx] = args[2].clone();
                        Ok(PyObject::none())
                    } else {
                        Err(PyException::index_error("list assignment index out of range"))
                    }
                }
                PyObjectPayload::Dict(map) => {
                    let key = args[1].to_hashable_key()?;
                    map.write().insert(key, args[2].clone());
                    Ok(PyObject::none())
                }
                _ => Err(PyException::type_error("object does not support item assignment")),
            }
        })),
        ("delitem", make_builtin(|args| {
            check_args_min("delitem", args, 2)?;
            match &args[0].payload {
                PyObjectPayload::Dict(map) => {
                    let key = args[1].to_hashable_key()?;
                    map.write().shift_remove(&key);
                    Ok(PyObject::none())
                }
                _ => Err(PyException::type_error("object does not support item deletion")),
            }
        })),
        ("concat", make_builtin(|args| {
            check_args_min("concat", args, 2)?;
            args[0].add(&args[1])
        })),
        ("iadd", make_builtin(|args| {
            check_args_min("iadd", args, 2)?;
            args[0].add(&args[1])
        })),
        ("methodcaller", make_builtin(|args| {
            check_args_min("methodcaller", args, 1)?;
            let method_name = args[0].py_to_string();
            let extra_args: Vec<PyObjectRef> = if args.len() > 1 { args[1..].to_vec() } else { vec![] };
            Ok(PyObject::native_closure("operator.methodcaller", move |call_args| {
                if call_args.is_empty() {
                    return Err(PyException::type_error("methodcaller expected 1 argument, got 0"));
                }
                let obj = &call_args[0];
                let method = obj.get_attr(&method_name)
                    .ok_or_else(|| PyException::attribute_error(format!(
                        "'{}' object has no attribute '{}'", obj.type_name(), method_name
                    )))?;
                // We can't call through VM from native closure, so just return the method bound
                match &method.payload {
                    PyObjectPayload::NativeFunction { func, .. } => func(&extra_args),
                    PyObjectPayload::NativeClosure { func, .. } => func(&extra_args),
                    _ => Ok(method),
                }
            }))
        })),
        ("length_hint", make_builtin(|args| {
            check_args_min("length_hint", args, 1)?;
            let default = if args.len() > 1 { args[1].to_int().unwrap_or(0) } else { 0 };
            // Try __length_hint__ first
            if let Some(method) = args[0].get_attr("__length_hint__") {
                match &method.payload {
                    PyObjectPayload::NativeFunction { func, .. } => {
                        if let Ok(result) = func(&[args[0].clone()]) {
                            if let Ok(n) = result.to_int() {
                                return Ok(PyObject::int(n));
                            }
                        }
                    }
                    _ => {}
                }
            }
            // Try __len__
            if let Some(method) = args[0].get_attr("__len__") {
                match &method.payload {
                    PyObjectPayload::NativeFunction { func, .. } => {
                        if let Ok(result) = func(&[args[0].clone()]) {
                            if let Ok(n) = result.to_int() {
                                return Ok(PyObject::int(n));
                            }
                        }
                    }
                    _ => {}
                }
            }
            // Try len() directly
            match args[0].py_len() {
                Ok(n) => Ok(PyObject::int(n as i64)),
                Err(_) => Ok(PyObject::int(default)),
            }
        })),
    ])
}

// ── typing module (stub) ──


pub fn create_hashlib_module() -> PyObjectRef {
    make_module("hashlib", vec![
        ("md5", make_builtin(hashlib_md5)),
        ("sha1", make_builtin(hashlib_sha1)),
        ("sha256", make_builtin(hashlib_sha256)),
        ("sha512", make_builtin(hashlib_sha512)),
        ("sha224", make_builtin(hashlib_sha224)),
        ("sha384", make_builtin(hashlib_sha384)),
        ("new", make_builtin(hashlib_new)),
    ])
}

fn make_hash_object(name: &str, data: Vec<u8>, digest_hex: String, digest_bytes: Vec<u8>, block_size: i64, digest_size: i64) -> PyObjectRef {
    let class = PyObject::class(CompactString::from(name), vec![], IndexMap::new());
    let attrs = IndexMap::new();
    let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class: class.clone(),
        attrs: Arc::new(RwLock::new(attrs)),
        dict_storage: None,
    }));
    {
        let a = if let PyObjectPayload::Instance(ref d) = inst.payload { d.attrs.clone() } else { unreachable!() };
        let mut w = a.write();
        w.insert(CompactString::from("_hexdigest"), PyObject::str_val(CompactString::from(&digest_hex)));
        w.insert(CompactString::from("_digest"), PyObject::bytes(digest_bytes));
        w.insert(CompactString::from("_data"), PyObject::bytes(data));
        w.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(name)));
        w.insert(CompactString::from("block_size"), PyObject::int(block_size));
        w.insert(CompactString::from("digest_size"), PyObject::int(digest_size));
    }
    inst
}

fn hashlib_md5(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use md5::Md5;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Md5::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("md5", data, hex, result.to_vec(), 64, 16))
}

fn hashlib_sha1(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha1::Sha1;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha1::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha1", data, hex, result.to_vec(), 64, 20))
}

fn hashlib_sha256(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha2::Sha256;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha256", data, hex, result.to_vec(), 64, 32))
}

fn hashlib_sha224(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha2::Sha224;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha224::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha224", data, hex, result.to_vec(), 64, 28))
}

fn hashlib_sha384(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha2::Sha384;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha384::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha384", data, hex, result.to_vec(), 128, 48))
}

fn hashlib_sha512(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    use sha2::Sha512;
    use digest::Digest;
    let data = if args.is_empty() { vec![] } else { extract_bytes(&args[0])? };
    let mut hasher = Sha512::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hex = result.iter().map(|b| format!("{:02x}", b)).collect::<String>();
    Ok(make_hash_object("sha512", data, hex, result.to_vec(), 128, 64))
}

fn hashlib_new(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("hashlib.new() requires algorithm name")); }
    let name = match &args[0].payload {
        PyObjectPayload::Str(s) => s.to_string(),
        _ => return Err(PyException::type_error("algorithm name must be a string")),
    };
    let data_args = if args.len() > 1 { &args[1..] } else { &[] as &[PyObjectRef] };
    match name.as_str() {
        "md5" => hashlib_md5(data_args),
        "sha1" => hashlib_sha1(data_args),
        "sha256" => hashlib_sha256(data_args),
        "sha224" => hashlib_sha224(data_args),
        "sha384" => hashlib_sha384(data_args),
        "sha512" => hashlib_sha512(data_args),
        _ => Err(PyException::value_error(format!("unsupported hash type {}", name))),
    }
}

// ── copy module ──


pub fn create_logging_module() -> PyObjectRef {
    // Logging levels
    let debug_level = PyObject::int(10);
    let info_level = PyObject::int(20);
    let warning_level = PyObject::int(30);
    let error_level = PyObject::int(40);
    let critical_level = PyObject::int(50);

    // StreamHandler class — creates handler instance with stream ref and format/emit
    let stream_handler_cls = PyObject::class(CompactString::from("StreamHandler"), vec![], IndexMap::new());
    let sh_cls = stream_handler_cls.clone();
    let stream_handler_fn = PyObject::native_closure("StreamHandler", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(sh_cls.clone());
        let stream = if args.is_empty() { PyObject::none() } else { args[0].clone() };
        // Shared state for formatter and level
        let formatter_ref: Arc<RwLock<PyObjectRef>> = Arc::new(RwLock::new(PyObject::none()));
        let level_ref: Arc<RwLock<i64>> = Arc::new(RwLock::new(0));

        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(CompactString::from("stream"), stream.clone());
            attrs.insert(CompactString::from("level"), PyObject::int(0));
            attrs.insert(CompactString::from("formatter"), PyObject::none());

            let lr = level_ref.clone();
            attrs.insert(CompactString::from("setLevel"), PyObject::native_closure(
                "setLevel", move |args: &[PyObjectRef]| {
                    if let Some(v) = args.first() {
                        if let Some(n) = v.as_int() { *lr.write() = n; }
                    }
                    Ok(PyObject::none())
                }
            ));
            let fr = formatter_ref.clone();
            attrs.insert(CompactString::from("setFormatter"), PyObject::native_closure(
                "setFormatter", move |args: &[PyObjectRef]| {
                    if let Some(v) = args.first() {
                        *fr.write() = v.clone();
                    }
                    Ok(PyObject::none())
                }
            ));
            // emit(record) — write formatted message to stream or stderr
            let fr2 = formatter_ref.clone();
            let stream2 = stream.clone();
            attrs.insert(CompactString::from("emit"), PyObject::native_closure(
                "emit", move |args: &[PyObjectRef]| {
                    // args[0] may be handler (from logger dispatch) or record (direct call)
                    // Detect: if called with 2 args, args[0]=handler, args[1]=record
                    // If called with 1 arg, args[0]=record
                    let record = if args.len() >= 2 { &args[1] } else if !args.is_empty() { &args[0] } else {
                        return Ok(PyObject::none());
                    };

                    let msg = if let Some(m) = record.get_attr("message") {
                        m.py_to_string()
                    } else if let Some(m) = record.get_attr("msg") {
                        m.py_to_string()
                    } else {
                        record.py_to_string()
                    };

                    // Apply formatter if set
                    let fmt = fr2.read().clone();
                    let formatted = if !matches!(&fmt.payload, PyObjectPayload::None) {
                        if let Some(fmt_str) = fmt.get_attr("_fmt") {
                            let fs = fmt_str.py_to_string();
                            let mut result = fs.clone();
                            result = result.replace("%(message)s", &msg);
                            let levelname = if let Some(ln) = record.get_attr("levelname") {
                                ln.py_to_string()
                            } else { "INFO".to_string() };
                            let name = if let Some(n) = record.get_attr("name") {
                                n.py_to_string()
                            } else { "root".to_string() };
                            result = result.replace("%(levelname)s", &levelname);
                            result = result.replace("%(name)s", &name);
                            result
                        } else { msg.clone() }
                    } else { msg.clone() };

                    // Write to stream (directly to StringIO buffer)
                    if let PyObjectPayload::Instance(ref si) = stream2.payload {
                        let attrs_r = si.attrs.read();
                        if attrs_r.contains_key("__stringio__") {
                            drop(attrs_r);
                            let mut attrs_w = si.attrs.write();
                            let line = format!("{}\n", formatted);
                            if let Some(buf) = attrs_w.get("_buffer") {
                                let cur = buf.py_to_string();
                                attrs_w.insert(
                                    CompactString::from("_buffer"),
                                    PyObject::str_val(CompactString::from(format!("{}{}", cur, line))),
                                );
                            }
                            return Ok(PyObject::none());
                        }
                    }
                    eprintln!("{}", formatted);
                    Ok(PyObject::none())
                }
            ));
        }
        Ok(inst)
    });

    // FileHandler class — handler that writes to file
    let file_handler_cls = PyObject::class(CompactString::from("FileHandler"), vec![], IndexMap::new());
    let fh_cls = file_handler_cls.clone();
    let file_handler_fn = PyObject::native_closure("FileHandler", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(fh_cls.clone());
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let filename = if args.is_empty() {
                CompactString::from("")
            } else {
                CompactString::from(args[0].py_to_string())
            };
            attrs.insert(CompactString::from("baseFilename"), PyObject::str_val(filename));
            attrs.insert(CompactString::from("level"), PyObject::int(0));
            attrs.insert(CompactString::from("formatter"), PyObject::none());
            attrs.insert(CompactString::from("setLevel"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() >= 2 {
                    if let PyObjectPayload::Instance(ref d) = args[0].payload {
                        d.attrs.write().insert(CompactString::from("level"), args[1].clone());
                    }
                }
                Ok(PyObject::none())
            }));
            attrs.insert(CompactString::from("setFormatter"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() >= 2 {
                    if let PyObjectPayload::Instance(ref d) = args[0].payload {
                        d.attrs.write().insert(CompactString::from("formatter"), args[1].clone());
                    }
                }
                Ok(PyObject::none())
            }));
        }
        Ok(inst)
    });

    // Formatter(fmt) — stores format string, has format(record) method
    let formatter_cls = PyObject::class(CompactString::from("Formatter"), vec![], IndexMap::new());
    let fmt_cls = formatter_cls.clone();
    let formatter_fn = PyObject::native_closure("Formatter", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(fmt_cls.clone());
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let fmt_str = if args.is_empty() {
                CompactString::from("%(levelname)s:%(name)s:%(message)s")
            } else {
                CompactString::from(args[0].py_to_string())
            };
            attrs.insert(CompactString::from("_fmt"), PyObject::str_val(fmt_str));
            attrs.insert(CompactString::from("format"), make_builtin(|args: &[PyObjectRef]| {
                if args.len() >= 2 {
                    Ok(PyObject::str_val(CompactString::from(args[1].py_to_string())))
                } else {
                    Ok(PyObject::str_val(CompactString::from("")))
                }
            }));
        }
        Ok(inst)
    });

    // Handler base class
    let handler_cls = PyObject::class(CompactString::from("Handler"), vec![], IndexMap::new());
    let h_cls = handler_cls.clone();
    let handler_fn = PyObject::native_closure("Handler", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(h_cls.clone());
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            attrs.insert(CompactString::from("level"), PyObject::int(0));
            attrs.insert(CompactString::from("setLevel"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("setFormatter"), make_builtin(|_| Ok(PyObject::none())));
        }
        Ok(inst)
    });

    // basicConfig(**kwargs) — configure root logger
    let basic_config_fn = make_builtin(|args: &[PyObjectRef]| {
        // Accept kwargs as last dict arg from VM
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw_map) = &last.payload {
                let r = kw_map.read();
                // Extract level if present
                if let Some(_level) = r.get(&HashableKey::Str(CompactString::from("level"))) {
                    // In a real impl, would set root logger level
                }
                // Extract format if present
                if let Some(_format) = r.get(&HashableKey::Str(CompactString::from("format"))) {
                    // Would set root logger format
                }
            }
        }
        Ok(PyObject::none())
    });

    make_module("logging", vec![
        ("DEBUG", debug_level),
        ("INFO", info_level),
        ("WARNING", warning_level.clone()),
        ("ERROR", error_level),
        ("CRITICAL", critical_level),
        ("NOTSET", PyObject::int(0)),
        ("basicConfig", basic_config_fn),
        ("getLogger", make_builtin(logging_get_logger)),
        ("debug", make_builtin(|args| { logging_log(10, args) })),
        ("info", make_builtin(|args| { logging_log(20, args) })),
        ("warning", make_builtin(|args| { logging_log(30, args) })),
        ("error", make_builtin(|args| { logging_log(40, args) })),
        ("critical", make_builtin(|args| { logging_log(50, args) })),
        ("log", make_builtin(|args| {
            if args.len() >= 2 {
                let level = args[0].as_int().unwrap_or(20);
                logging_log(level, &args[1..])
            } else {
                Ok(PyObject::none())
            }
        })),
        ("StreamHandler", stream_handler_fn),
        ("FileHandler", file_handler_fn),
        ("Formatter", formatter_fn),
        ("Handler", handler_fn),
        ("Logger", make_builtin(logging_get_logger)),
    ])
}

fn logging_log(level: i64, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Ok(PyObject::none()); }
    let level_name = match level {
        10 => "DEBUG",
        20 => "INFO",
        30 => "WARNING",
        40 => "ERROR",
        50 => "CRITICAL",
        _ => "UNKNOWN",
    };
    let msg = args[0].py_to_string();
    eprintln!("{}:root:{}", level_name, msg);
    Ok(PyObject::none())
}

fn logging_get_logger(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let logger_name = if args.is_empty() || matches!(&args[0].payload, PyObjectPayload::None) {
        CompactString::from("root")
    } else {
        CompactString::from(args[0].py_to_string())
    };
    let mut ns = IndexMap::new();
    ns.insert(CompactString::from("name"), PyObject::str_val(logger_name.clone()));
    ns.insert(CompactString::from("level"), PyObject::int(30)); // WARNING default
    let handlers_list = PyObject::list(vec![]);
    ns.insert(CompactString::from("handlers"), handlers_list.clone());

    // Create log methods that capture the shared handlers list
    let make_log_method = |level: i64, level_name: &'static str, handlers: PyObjectRef, name: CompactString| -> PyObjectRef {
        PyObject::native_closure(level_name, move |args: &[PyObjectRef]| {
            if args.is_empty() { return Ok(PyObject::none()); }
            let msg = args[0].py_to_string();

            // Create a LogRecord-like instance
            let rec_cls = PyObject::class(CompactString::from("LogRecord"), vec![], IndexMap::new());
            let record = PyObject::instance(rec_cls);
            if let PyObjectPayload::Instance(ref rd) = record.payload {
                let mut ra = rd.attrs.write();
                ra.insert(CompactString::from("levelname"), PyObject::str_val(CompactString::from(level_name)));
                ra.insert(CompactString::from("levelno"), PyObject::int(level));
                ra.insert(CompactString::from("name"), PyObject::str_val(name.clone()));
                ra.insert(CompactString::from("message"), PyObject::str_val(CompactString::from(msg.clone())));
                ra.insert(CompactString::from("msg"), PyObject::str_val(CompactString::from(msg.clone())));
            }

            // Dispatch to handlers via shared list
            let mut dispatched = false;
            if let PyObjectPayload::List(items) = &handlers.payload {
                let items_r = items.read();
                for handler in items_r.iter() {
                    if let Some(emit_fn) = handler.get_attr("emit") {
                        match &emit_fn.payload {
                            PyObjectPayload::NativeFunction { func, .. } => {
                                let _ = func(&[handler.clone(), record.clone()]);
                                dispatched = true;
                            }
                            PyObjectPayload::NativeClosure { func, .. } => {
                                let _ = func(&[handler.clone(), record.clone()]);
                                dispatched = true;
                            }
                            _ => {}
                        }
                    }
                }
            }
            if !dispatched {
                eprintln!("{}:{}:{}", level_name, name, msg);
            }
            Ok(PyObject::none())
        })
    };

    ns.insert(CompactString::from("debug"), make_log_method(10, "DEBUG", handlers_list.clone(), logger_name.clone()));
    ns.insert(CompactString::from("info"), make_log_method(20, "INFO", handlers_list.clone(), logger_name.clone()));
    ns.insert(CompactString::from("warning"), make_log_method(30, "WARNING", handlers_list.clone(), logger_name.clone()));
    ns.insert(CompactString::from("error"), make_log_method(40, "ERROR", handlers_list.clone(), logger_name.clone()));
    ns.insert(CompactString::from("critical"), make_log_method(50, "CRITICAL", handlers_list.clone(), logger_name.clone()));

    ns.insert(CompactString::from("setLevel"), make_builtin(|_| Ok(PyObject::none())));
    // addHandler — push to shared handlers list
    let hl = handlers_list.clone();
    ns.insert(CompactString::from("addHandler"), PyObject::native_closure(
        "addHandler", move |args: &[PyObjectRef]| {
            if !args.is_empty() {
                if let PyObjectPayload::List(items) = &hl.payload {
                    items.write().push(args[0].clone());
                }
            }
            Ok(PyObject::none())
        }
    ));
    ns.insert(CompactString::from("removeHandler"), make_builtin(|_| Ok(PyObject::none())));
    let hl2 = handlers_list.clone();
    ns.insert(CompactString::from("hasHandlers"), PyObject::native_closure(
        "hasHandlers", move |_: &[PyObjectRef]| {
            if let PyObjectPayload::List(items) = &hl2.payload {
                return Ok(PyObject::bool_val(!items.read().is_empty()));
            }
            Ok(PyObject::bool_val(false))
        }
    ));
    ns.insert(CompactString::from("isEnabledFor"), make_builtin(|_| Ok(PyObject::bool_val(true))));
    ns.insert(CompactString::from("getEffectiveLevel"), make_builtin(|_| Ok(PyObject::int(30))));
    
    let cls = PyObject::class(CompactString::from("Logger"), vec![], IndexMap::new());
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(inst_data) = &inst.payload {
        let mut attrs = inst_data.attrs.write();
        for (k, v) in ns {
            attrs.insert(k, v);
        }
    }
    Ok(inst)
}

// ── subprocess module (basic) ──


pub fn create_warnings_module() -> PyObjectRef {
    // warn(message, category=UserWarning, stacklevel=1)
    let warn_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::none()); }
        let message = args[0].py_to_string();
        let category = if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
            let cat = &args[1];
            if let PyObjectPayload::Class(cd) = &cat.payload {
                cd.name.to_string()
            } else {
                cat.py_to_string()
            }
        } else {
            "UserWarning".to_string()
        };
        // Print warning in CPython format: filename:lineno: category: message
        eprintln!("<stdin>:1: {}: {}", category, message);
        Ok(PyObject::none())
    });

    // filterwarnings(action, message="", category=Warning, module="", lineno=0, append=False)
    let filter_warnings_fn = make_builtin(|_args: &[PyObjectRef]| {
        // Store filter — basic implementation accepts but doesn't enforce
        Ok(PyObject::none())
    });

    // simplefilter(action, category=Warning, append=False)
    let simple_filter_fn = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::none())
    });

    // catch_warnings(record=False) — context manager that saves/restores warning filters
    // When record=True, __enter__ returns a list that collects WarningMessage objects
    let catch_warnings_fn = make_builtin(|args: &[PyObjectRef]| {
        // Check for record=True (positional arg or kwarg)
        let record = if !args.is_empty() {
            args[0].is_truthy()
        } else {
            false
        };
        let cls = PyObject::class(CompactString::from("catch_warnings"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        let warning_list = PyObject::list(vec![]);
        attrs.insert(CompactString::from("_record"), PyObject::bool_val(record));
        attrs.insert(CompactString::from("_warnings"), warning_list.clone());
        if record {
            // __enter__ returns the warning list for `with ... as w:`
            let wl = warning_list.clone();
            attrs.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "catch_warnings.__enter__", move |_args: &[PyObjectRef]| {
                    Ok(wl.clone())
                }
            ));
        } else {
            attrs.insert(CompactString::from("__enter__"), PyObject::native_function(
                "catch_warnings.__enter__", |args: &[PyObjectRef]| {
                    if args.is_empty() { return Ok(PyObject::none()); }
                    Ok(args[0].clone())
                }
            ));
        }
        attrs.insert(CompactString::from("__exit__"), PyObject::native_function(
            "catch_warnings.__exit__", |_args: &[PyObjectRef]| {
                Ok(PyObject::bool_val(false))
            }
        ));
        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    make_module("warnings", vec![
        ("warn", warn_fn),
        ("filterwarnings", filter_warnings_fn),
        ("simplefilter", simple_filter_fn),
        ("resetwarnings", make_builtin(|_| Ok(PyObject::none()))),
        ("catch_warnings", catch_warnings_fn),
    ])
}

// ── decimal module (stub) ──


pub fn create_traceback_module() -> PyObjectRef {
    // format_exc() — return formatted exception string (empty when no active exception)
    let format_exc_fn = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::str_val(CompactString::from("")))
    });

    // format_exception(etype, value, tb) — format exception into list of strings
    let format_exception_fn = make_builtin(|args: &[PyObjectRef]| {
        let mut lines = Vec::new();
        if args.len() >= 2 {
            let etype = &args[0];
            let value = &args[1];
            let type_name = if let PyObjectPayload::Class(cd) = &etype.payload {
                cd.name.to_string()
            } else if let PyObjectPayload::ExceptionType(kind) = &etype.payload {
                format!("{:?}", kind)
            } else {
                etype.py_to_string()
            };
            let msg = value.py_to_string();
            if args.len() >= 3 && !matches!(&args[2].payload, PyObjectPayload::None) {
                lines.push(PyObject::str_val(CompactString::from("Traceback (most recent call last):\n")));
                lines.push(PyObject::str_val(CompactString::from("  File \"<unknown>\", line 0, in <module>\n")));
            }
            lines.push(PyObject::str_val(CompactString::from(
                format!("{}: {}\n", type_name, msg)
            )));
        }
        Ok(PyObject::list(lines))
    });

    // print_exc() — print exception info to stderr
    let print_exc_fn = make_builtin(|_args: &[PyObjectRef]| {
        eprintln!("NoneType: None");
        Ok(PyObject::none())
    });

    // format_tb(tb) — format traceback entries as list of strings
    let format_tb_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() || matches!(&args[0].payload, PyObjectPayload::None) {
            return Ok(PyObject::list(vec![]));
        }
        // Return a basic traceback entry
        Ok(PyObject::list(vec![
            PyObject::str_val(CompactString::from("  File \"<unknown>\", line 0, in <module>\n"))
        ]))
    });

    // extract_tb(tb) — extract FrameSummary-like tuples from traceback
    let extract_tb_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() || matches!(&args[0].payload, PyObjectPayload::None) {
            return Ok(PyObject::list(vec![]));
        }
        // Return list of (filename, lineno, name, line) tuples
        Ok(PyObject::list(vec![
            PyObject::tuple(vec![
                PyObject::str_val(CompactString::from("<unknown>")),
                PyObject::int(0),
                PyObject::str_val(CompactString::from("<module>")),
                PyObject::none(),
            ])
        ]))
    });

    make_module("traceback", vec![
        ("format_exc", format_exc_fn),
        ("print_exc", print_exc_fn),
        ("format_exception", format_exception_fn),
        ("print_stack", make_builtin(|_| Ok(PyObject::none()))),
        ("format_tb", format_tb_fn),
        ("extract_tb", extract_tb_fn),
    ])
}

// ── warnings module (stub) ──


pub fn create_inspect_module() -> PyObjectRef {
    make_module("inspect", vec![
        ("isfunction", make_builtin(|args| {
            check_args("inspect.isfunction", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Function(_))))
        })),
        ("isclass", make_builtin(|args| {
            check_args("inspect.isclass", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Class(_))))
        })),
        ("ismethod", make_builtin(|args| {
            check_args("inspect.ismethod", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::BoundMethod { .. })))
        })),
        ("ismodule", make_builtin(|args| {
            check_args("inspect.ismodule", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Module(_))))
        })),
        ("isbuiltin", make_builtin(|args| {
            check_args("inspect.isbuiltin", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::NativeFunction { .. } | PyObjectPayload::BuiltinFunction(_) | PyObjectPayload::BuiltinType(_))))
        })),
        ("getmembers", make_builtin(|args| {
            check_args("inspect.getmembers", args, 1)?;
            let dir_names = args[0].dir();
            let dir_list: Vec<PyObjectRef> = dir_names.into_iter().map(|n| PyObject::str_val(n)).collect();
            let names = PyObject::list(dir_list);
            let mut result = Vec::new();
            if let PyObjectPayload::List(items) = &names.payload {
                for item in items.read().iter() {
                    let name_str = item.py_to_string();
                    if let Some(val) = args[0].get_attr(&name_str) {
                        result.push(PyObject::tuple(vec![item.clone(), val]));
                    }
                }
            }
            Ok(PyObject::list(result))
        })),
    ])
}

// ── dis module (stub) ──


pub fn create_dis_module() -> PyObjectRef {
    make_module("dis", vec![
        ("dis", make_builtin(|_| { Ok(PyObject::none()) })),
    ])
}

// ── logging module ──


pub fn create_threading_module() -> PyObjectRef {
    // Thread class constructor — accepts target=, args=, kwargs=, daemon=
    let thread_cls = PyObject::class(CompactString::from("Thread"), vec![], IndexMap::new());
    let tc = thread_cls.clone();
    let thread_fn = PyObject::native_closure("Thread", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(tc.clone());
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            // Parse kwargs — VM passes kwargs dict as last arg
            let mut target = PyObject::none();
            let mut thread_args = PyObject::tuple(vec![]);
            let mut daemon = PyObject::bool_val(false);
            let mut name = PyObject::str_val(CompactString::from("Thread"));
            // Check for kwargs dict as last argument
            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(kw_map) = &last.payload {
                    let r = kw_map.read();
                    if let Some(t) = r.get(&HashableKey::Str(CompactString::from("target"))) {
                        target = t.clone();
                    }
                    if let Some(a) = r.get(&HashableKey::Str(CompactString::from("args"))) {
                        thread_args = a.clone();
                    }
                    if let Some(d) = r.get(&HashableKey::Str(CompactString::from("daemon"))) {
                        daemon = d.clone();
                    }
                    if let Some(n) = r.get(&HashableKey::Str(CompactString::from("name"))) {
                        name = n.clone();
                    }
                }
            }
            attrs.insert(CompactString::from("name"), name.clone());
            attrs.insert(CompactString::from("daemon"), daemon);

            // Shared state for thread lifecycle
            let alive = Arc::new(RwLock::new(false));
            let started = Arc::new(RwLock::new(false));

            // start() — call target(*args) synchronously (single-threaded interpreter)
            let tgt = target.clone();
            let targs = thread_args.clone();
            let a1 = alive.clone();
            let s1 = started.clone();
            attrs.insert(CompactString::from("start"), PyObject::native_closure(
                "start", move |_: &[PyObjectRef]| {
                    *s1.write() = true;
                    *a1.write() = true;
                    if !matches!(&tgt.payload, PyObjectPayload::None) {
                        let call_args: Vec<PyObjectRef> = match &targs.payload {
                            PyObjectPayload::Tuple(items) => items.clone(),
                            PyObjectPayload::List(items) => items.read().clone(),
                            _ => vec![],
                        };
                        match &tgt.payload {
                            PyObjectPayload::NativeFunction { func, .. } => { let _ = func(&call_args); }
                            PyObjectPayload::NativeClosure { func, .. } => { let _ = func(&call_args); }
                            _ => {
                                // Python function — defer to VM via thread-local
                                push_deferred_call(tgt.clone(), call_args);
                            }
                        }
                    }
                    *a1.write() = false;
                    Ok(PyObject::none())
                }
            ));
            attrs.insert(CompactString::from("join"), make_builtin(|_| Ok(PyObject::none())));
            let a2 = alive.clone();
            attrs.insert(CompactString::from("is_alive"), PyObject::native_closure(
                "is_alive", move |_: &[PyObjectRef]| {
                    Ok(PyObject::bool_val(*a2.read()))
                }
            ));
            let nm = name.clone();
            attrs.insert(CompactString::from("getName"), PyObject::native_closure(
                "getName", move |_: &[PyObjectRef]| {
                    Ok(nm.clone())
                }
            ));
            attrs.insert(CompactString::from("setDaemon"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("ident"), PyObject::none());
        }
        Ok(inst)
    });

    // Lock/RLock — context managers with acquire/release using shared state
    let lock_cls = PyObject::class(CompactString::from("Lock"), vec![], IndexMap::new());
    let lc = lock_cls.clone();
    let lock_fn = PyObject::native_closure("Lock", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(lc.clone());
        let locked = Arc::new(RwLock::new(false));
        let inst_ref = inst.clone(); // for __enter__ closure
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let l1 = locked.clone();
            attrs.insert(CompactString::from("acquire"), PyObject::native_closure(
                "acquire", move |_: &[PyObjectRef]| { *l1.write() = true; Ok(PyObject::bool_val(true)) }));
            let l2 = locked.clone();
            attrs.insert(CompactString::from("release"), PyObject::native_closure(
                "release", move |_: &[PyObjectRef]| { *l2.write() = false; Ok(PyObject::none()) }));
            let l3 = locked.clone();
            attrs.insert(CompactString::from("locked"), PyObject::native_closure(
                "locked", move |_: &[PyObjectRef]| { Ok(PyObject::bool_val(*l3.read())) }));
            let l4 = locked.clone();
            attrs.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "__enter__", move |_: &[PyObjectRef]| { *l4.write() = true; Ok(inst_ref.clone()) }));
            let l5 = locked.clone();
            attrs.insert(CompactString::from("__exit__"), PyObject::native_closure(
                "__exit__", move |_: &[PyObjectRef]| { *l5.write() = false; Ok(PyObject::bool_val(false)) }));
        }
        Ok(inst)
    });

    // Event — simple threading event using shared state
    let event_cls = PyObject::class(CompactString::from("Event"), vec![], IndexMap::new());
    let ec = event_cls.clone();
    let event_fn = PyObject::native_closure("Event", move |_args: &[PyObjectRef]| {
        let inst = PyObject::instance(ec.clone());
        let flag = Arc::new(RwLock::new(false));
        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            let f1 = flag.clone();
            attrs.insert(CompactString::from("set"), PyObject::native_closure(
                "set", move |_: &[PyObjectRef]| { *f1.write() = true; Ok(PyObject::none()) }));
            let f2 = flag.clone();
            attrs.insert(CompactString::from("clear"), PyObject::native_closure(
                "clear", move |_: &[PyObjectRef]| { *f2.write() = false; Ok(PyObject::none()) }));
            let f3 = flag.clone();
            attrs.insert(CompactString::from("is_set"), PyObject::native_closure(
                "is_set", move |_: &[PyObjectRef]| { Ok(PyObject::bool_val(*f3.read())) }));
            attrs.insert(CompactString::from("wait"), make_builtin(|_| Ok(PyObject::bool_val(true))));
        }
        Ok(inst)
    });

    make_module("threading", vec![
        ("Thread", thread_fn),
        ("Lock", lock_fn.clone()),
        ("RLock", lock_fn),
        ("Event", event_fn),
        ("Semaphore", make_builtin(|_| Ok(PyObject::none()))),
        ("BoundedSemaphore", make_builtin(|_| Ok(PyObject::none()))),
        ("Condition", make_builtin(|_| Ok(PyObject::none()))),
        ("Barrier", make_builtin(|_| Ok(PyObject::none()))),
        ("Timer", make_builtin(|_| Ok(PyObject::none()))),
        ("current_thread", make_builtin(|_| {
            let mut ns = IndexMap::new();
            ns.insert(CompactString::from("name"), PyObject::str_val(CompactString::from("MainThread")));
            ns.insert(CompactString::from("ident"), PyObject::int(1));
            ns.insert(CompactString::from("daemon"), PyObject::bool_val(false));
            ns.insert(CompactString::from("is_alive"), make_builtin(|_| Ok(PyObject::bool_val(true))));
            ns.insert(CompactString::from("getName"), make_builtin(|_| Ok(PyObject::str_val(CompactString::from("MainThread")))));
            let cls = PyObject::class(CompactString::from("Thread"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(i) = &inst.payload {
                let mut attrs = i.attrs.write();
                for (k, v) in ns { attrs.insert(k, v); }
            }
            Ok(inst)
        })),
        ("active_count", make_builtin(|_| Ok(PyObject::int(1)))),
        ("enumerate", make_builtin(|_| Ok(PyObject::list(vec![])))),
        ("main_thread", make_builtin(|_| Ok(PyObject::none()))),
        ("local", make_builtin(|_| {
            let cls = PyObject::class(CompactString::from("local"), vec![], IndexMap::new());
            Ok(PyObject::instance(cls))
        })),
    ])
}

// ── csv module (basic) ──


pub fn create_unittest_module() -> PyObjectRef {
    // Create TestCase class
    let mut tc_ns = IndexMap::new();
    tc_ns.insert(CompactString::from("__unittest_testcase__"), PyObject::bool_val(true));
    let test_case = PyObject::class(CompactString::from("TestCase"), vec![], tc_ns);

    make_module("unittest", vec![
        ("TestCase", test_case),
        ("main", make_builtin(|_| Ok(PyObject::none()))),
        ("TestSuite", make_builtin(|_| Ok(PyObject::none()))),
        ("TestLoader", make_builtin(|_| Ok(PyObject::none()))),
        ("TextTestRunner", make_builtin(|_| Ok(PyObject::none()))),
        ("skip", make_builtin(|_args| {
            // Return identity decorator
            Ok(make_builtin(|args| {
                if args.is_empty() { Ok(PyObject::none()) } else { Ok(args[0].clone()) }
            }))
        })),
        ("skipIf", make_builtin(|_| {
            Ok(make_builtin(|args| {
                if args.is_empty() { Ok(PyObject::none()) } else { Ok(args[0].clone()) }
            }))
        })),
        ("expectedFailure", make_builtin(|args| {
            if args.is_empty() { Ok(PyObject::none()) } else { Ok(args[0].clone()) }
        })),
    ])
}

// ── threading module (basic) ──


pub fn create_pprint_module() -> PyObjectRef {
    make_module("pprint", vec![
        ("pprint", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::none()); }
            println!("{}", args[0].py_to_string());
            Ok(PyObject::none())
        })),
        ("pformat", make_builtin(|args| {
            if args.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
            Ok(PyObject::str_val(CompactString::from(args[0].py_to_string())))
        })),
        ("PrettyPrinter", make_builtin(|_| Ok(PyObject::none()))),
    ])
}

// ── argparse module (basic) ──


pub fn create_argparse_module() -> PyObjectRef {
    // ArgumentParser class — functional constructor
    let ap_cls = PyObject::class(CompactString::from("ArgumentParser"), vec![], IndexMap::new());
    let apc = ap_cls.clone();
    let argument_parser_fn = PyObject::native_closure("ArgumentParser", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(apc.clone());
        // Shared argument storage between add_argument and parse_args
        let arg_defs: Arc<RwLock<Vec<(Vec<String>, IndexMap<CompactString, PyObjectRef>)>>> =
            Arc::new(RwLock::new(Vec::new()));

        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            // Store description, prog from kwargs
            let mut description = CompactString::from("");
            let mut prog = CompactString::from("");
            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(kw_map) = &last.payload {
                    let r = kw_map.read();
                    if let Some(d) = r.get(&HashableKey::Str(CompactString::from("description"))) {
                        description = CompactString::from(d.py_to_string());
                    }
                    if let Some(p) = r.get(&HashableKey::Str(CompactString::from("prog"))) {
                        prog = CompactString::from(p.py_to_string());
                    }
                }
            }
            attrs.insert(CompactString::from("description"), PyObject::str_val(description));
            attrs.insert(CompactString::from("prog"), PyObject::str_val(prog));

            // add_argument(*name_or_flags, **kwargs) — closure captures shared arg_defs
            let ad = arg_defs.clone();
            attrs.insert(CompactString::from("add_argument"), PyObject::native_closure(
                "add_argument", move |args: &[PyObjectRef]| {
                    let mut names: Vec<String> = Vec::new();
                    let mut kwargs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
                    for arg in args {
                        match &arg.payload {
                            PyObjectPayload::Str(s) => { names.push(s.to_string()); }
                            PyObjectPayload::Dict(kw_map) => {
                                let r = kw_map.read();
                                for (k, v) in r.iter() {
                                    if let HashableKey::Str(ks) = k {
                                        kwargs.insert(ks.clone(), v.clone());
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    ad.write().push((names, kwargs));
                    Ok(PyObject::none())
                }
            ));

            // parse_args(args=None) — closure captures shared arg_defs
            let pa = arg_defs.clone();
            attrs.insert(CompactString::from("parse_args"), PyObject::native_closure(
                "parse_args", move |_args: &[PyObjectRef]| {
                    let ns_cls = PyObject::class(CompactString::from("Namespace"), vec![], IndexMap::new());
                    let ns_inst = PyObject::instance(ns_cls);
                    // Set defaults from stored argument definitions
                    let defs = pa.read();
                    for (names, kwargs) in defs.iter() {
                        let dest = if let Some(d) = kwargs.get("dest") {
                            d.py_to_string()
                        } else {
                            // Prefer long option names (--verbose) over short (-v)
                            let long = names.iter().find(|n| n.starts_with("--"));
                            let chosen = long.or(names.first());
                            if let Some(n) = chosen {
                                n.trim_start_matches('-').replace('-', "_")
                            } else { continue; }
                        };
                        let default = kwargs.get("default").cloned().unwrap_or_else(PyObject::none);
                        if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                            nd.attrs.write().insert(CompactString::from(dest.as_str()), default);
                        }
                    }
                    Ok(ns_inst)
                }
            ));

            attrs.insert(CompactString::from("add_subparsers"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("add_mutually_exclusive_group"), make_builtin(|_| Ok(PyObject::none())));
        }
        Ok(inst)
    });

    // Namespace class
    let ns_cls = PyObject::class(CompactString::from("Namespace"), vec![], IndexMap::new());
    let nsc = ns_cls.clone();
    let namespace_fn = PyObject::native_closure("Namespace", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(nsc.clone());
        // Accept kwargs
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw_map) = &last.payload {
                if let PyObjectPayload::Instance(ref id) = inst.payload {
                    let mut attrs = id.attrs.write();
                    let r = kw_map.read();
                    for (k, v) in r.iter() {
                        if let HashableKey::Str(ks) = k {
                            attrs.insert(ks.clone(), v.clone());
                        }
                    }
                }
            }
        }
        Ok(inst)
    });

    make_module("argparse", vec![
        ("ArgumentParser", argument_parser_fn),
        ("Namespace", namespace_fn),
        ("Action", make_builtin(|_| Ok(PyObject::none()))),
        ("HelpFormatter", make_builtin(|_| Ok(PyObject::none()))),
        ("RawDescriptionHelpFormatter", make_builtin(|_| Ok(PyObject::none()))),
    ])
}

// ── datetime module ──


pub fn create_weakref_module() -> PyObjectRef {
    make_module("weakref", vec![
        ("ref", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("ref requires 1 argument")); }
            let referent = args[0].clone();
            let mut cls_ns = IndexMap::new();
            let ref2 = referent.clone();
            cls_ns.insert(CompactString::from("__call__"), PyObject::native_closure("weakref.__call__", move |_a| Ok(ref2.clone())));
            let cls = PyObject::class(CompactString::from("weakref"), vec![], cls_ns);
            let mut inst_attrs = IndexMap::new();
            if let PyObjectPayload::Instance(inst) = &referent.payload {
                let r = inst.attrs.read();
                for (k, v) in r.iter() {
                    inst_attrs.insert(k.clone(), v.clone());
                }
            }
            Ok(PyObject::instance_with_attrs(cls, inst_attrs))
        })),
        ("proxy", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("proxy requires 1 argument")); }
            Ok(args[0].clone())
        })),
        ("WeakValueDictionary", make_builtin(|_| Ok(PyObject::dict(IndexMap::new())))),
        ("WeakKeyDictionary", make_builtin(|_| Ok(PyObject::dict(IndexMap::new())))),
        ("WeakSet", make_builtin(|_| Ok(PyObject::set(IndexMap::new())))),
    ])
}

// ── gc module ──


pub fn create_gc_module() -> PyObjectRef {
    make_module("gc", vec![
        ("enable", make_builtin(|_| {
            ferrython_gc::enable();
            Ok(PyObject::none())
        })),
        ("disable", make_builtin(|_| {
            ferrython_gc::disable();
            Ok(PyObject::none())
        })),
        ("isenabled", make_builtin(|_| {
            Ok(PyObject::bool_val(ferrython_gc::is_enabled()))
        })),
        ("collect", make_builtin(|_| {
            let collected = ferrython_gc::collect();
            Ok(PyObject::int(collected as i64))
        })),
        ("get_threshold", make_builtin(|_| {
            let (g0, g1, g2) = ferrython_gc::get_threshold();
            Ok(PyObject::tuple(vec![
                PyObject::int(g0 as i64),
                PyObject::int(g1 as i64),
                PyObject::int(g2 as i64),
            ]))
        })),
        ("set_threshold", make_builtin(|args| {
            check_args_min("gc.set_threshold", args, 1)?;
            let g0 = args[0].as_int().ok_or_else(|| {
                PyException::type_error("threshold must be an integer")
            })? as u64;
            let g1 = args.get(1).and_then(|a| a.as_int()).unwrap_or(10) as u64;
            let g2 = args.get(2).and_then(|a| a.as_int()).unwrap_or(10) as u64;
            ferrython_gc::set_threshold(g0, g1, g2);
            Ok(PyObject::none())
        })),
        ("get_stats", make_builtin(|_| {
            let stats = ferrython_gc::get_stats();
            let entry = PyObject::dict({
                let mut m = IndexMap::new();
                m.insert(
                    HashableKey::Str(CompactString::from("collections")),
                    PyObject::int(stats.collections as i64),
                );
                m.insert(
                    HashableKey::Str(CompactString::from("collected")),
                    PyObject::int(0),
                );
                m.insert(
                    HashableKey::Str(CompactString::from("uncollectable")),
                    PyObject::int(0),
                );
                m
            });
            // CPython returns a list of 3 dicts, one per generation
            Ok(PyObject::list(vec![entry.clone(), entry.clone(), entry]))
        })),
        ("get_count", make_builtin(|_| {
            let stats = ferrython_gc::get_stats();
            Ok(PyObject::tuple(vec![
                PyObject::int(stats.allocations as i64),
                PyObject::int(0),
                PyObject::int(0),
            ]))
        })),
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

// ── errno module ──

pub fn create_errno_module() -> PyObjectRef {
    make_module("errno", vec![
        ("EPERM", PyObject::int(1)),
        ("ENOENT", PyObject::int(2)),
        ("ESRCH", PyObject::int(3)),
        ("EINTR", PyObject::int(4)),
        ("EIO", PyObject::int(5)),
        ("ENXIO", PyObject::int(6)),
        ("E2BIG", PyObject::int(7)),
        ("ENOEXEC", PyObject::int(8)),
        ("EBADF", PyObject::int(9)),
        ("ECHILD", PyObject::int(10)),
        ("EAGAIN", PyObject::int(11)),
        ("ENOMEM", PyObject::int(12)),
        ("EACCES", PyObject::int(13)),
        ("EFAULT", PyObject::int(14)),
        ("EBUSY", PyObject::int(16)),
        ("EEXIST", PyObject::int(17)),
        ("EXDEV", PyObject::int(18)),
        ("ENODEV", PyObject::int(19)),
        ("ENOTDIR", PyObject::int(20)),
        ("EISDIR", PyObject::int(21)),
        ("EINVAL", PyObject::int(22)),
        ("ENFILE", PyObject::int(23)),
        ("EMFILE", PyObject::int(24)),
        ("ENOTTY", PyObject::int(25)),
        ("EFBIG", PyObject::int(27)),
        ("ENOSPC", PyObject::int(28)),
        ("ESPIPE", PyObject::int(29)),
        ("EROFS", PyObject::int(30)),
        ("EMLINK", PyObject::int(31)),
        ("EPIPE", PyObject::int(32)),
        ("EDOM", PyObject::int(33)),
        ("ERANGE", PyObject::int(34)),
        ("EDEADLK", PyObject::int(35)),
        ("ENAMETOOLONG", PyObject::int(36)),
        ("ENOLCK", PyObject::int(37)),
        ("ENOSYS", PyObject::int(38)),
        ("ENOTEMPTY", PyObject::int(39)),
        ("ECONNREFUSED", PyObject::int(111)),
        ("ETIMEDOUT", PyObject::int(110)),
        ("errorcode", make_builtin(|_| {
            let mut map = IndexMap::new();
            let codes: Vec<(i64, &str)> = vec![
                (1, "EPERM"), (2, "ENOENT"), (13, "EACCES"), (17, "EEXIST"),
                (22, "EINVAL"), (32, "EPIPE"), (110, "ETIMEDOUT"), (111, "ECONNREFUSED"),
            ];
            for (num, name) in codes {
                map.insert(HashableKey::Int(PyInt::Small(num)), PyObject::str_val(CompactString::from(name)));
            }
            Ok(PyObject::dict(map))
        })),
    ])
}

// ── _thread module ──

pub fn create_thread_module() -> PyObjectRef {
    make_module("_thread", vec![
        ("allocate_lock", make_builtin(|_| {
            let cls = PyObject::class(CompactString::from("lock"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("_locked"), PyObject::bool_val(false));
                w.insert(CompactString::from("acquire"), make_builtin(|_| Ok(PyObject::bool_val(true))));
                w.insert(CompactString::from("release"), make_builtin(|_| Ok(PyObject::none())));
                w.insert(CompactString::from("locked"), make_builtin(|_| Ok(PyObject::bool_val(false))));
                w.insert(CompactString::from("__enter__"), make_builtin(|_| Ok(PyObject::bool_val(true))));
                w.insert(CompactString::from("__exit__"), make_builtin(|_| Ok(PyObject::none())));
            }
            Ok(inst)
        })),
        ("LockType", PyObject::class(CompactString::from("lock"), vec![], IndexMap::new())),
        ("start_new_thread", make_builtin(|_| Ok(PyObject::int(0)))),
        ("get_ident", make_builtin(|_| Ok(PyObject::int(1)))),
        ("stack_size", make_builtin(|_| Ok(PyObject::int(0)))),
        ("TIMEOUT_MAX", PyObject::float(f64::MAX)),
    ])
}
