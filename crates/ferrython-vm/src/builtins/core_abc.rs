//! isinstance/issubclass and structural ABC helpers.

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    call_callable, check_args, lookup_in_class_mro, property_field, PyObject, PyObjectMethods,
    PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;

const CLASSINFO_RECURSION_LIMIT: usize = 1000;

pub(super) fn builtin_isinstance(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("isinstance", args, 2)?;
    Ok(PyObject::bool_val(is_instance_of_result(
        &args[0], &args[1], 0,
    )?))
}

fn ast_constant_value(obj: &PyObjectRef) -> Option<Option<PyObjectRef>> {
    let PyObjectPayload::Instance(inst) = &obj.payload else {
        return None;
    };
    let is_constant = match &inst.class.payload {
        PyObjectPayload::Class(cd) => {
            cd.name.as_str() == "Constant"
                || cd.mro.iter().any(|base| {
                    matches!(&base.payload, PyObjectPayload::Class(bcd) if bcd.name.as_str() == "Constant")
                })
        }
        _ => false,
    };
    if !is_constant {
        return None;
    }
    Some(inst.attrs.read().get("value").cloned())
}

fn ast_constant_matches_legacy(value: &PyObjectRef, legacy: &str) -> bool {
    match legacy {
        "Num" => matches!(
            &value.payload,
            PyObjectPayload::Int(_) | PyObjectPayload::Float(_) | PyObjectPayload::Complex { .. }
        ),
        "Str" => {
            matches!(&value.payload, PyObjectPayload::Str(_))
                || matches!(
                    &value.payload,
                    PyObjectPayload::Instance(inst)
                        if inst
                            .attrs
                            .read()
                            .get("__builtin_value__")
                            .map(|v| matches!(&v.payload, PyObjectPayload::Str(_)))
                            .unwrap_or(false)
                )
        }
        "Bytes" => {
            matches!(&value.payload, PyObjectPayload::Bytes(_))
                || matches!(
                    &value.payload,
                    PyObjectPayload::Instance(inst)
                        if inst
                            .attrs
                            .read()
                            .get("__builtin_value__")
                            .map(|v| matches!(&v.payload, PyObjectPayload::Bytes(_)))
                            .unwrap_or(false)
                )
        }
        "NameConstant" => {
            matches!(
                &value.payload,
                PyObjectPayload::None | PyObjectPayload::Bool(_)
            )
        }
        "Ellipsis" => matches!(&value.payload, PyObjectPayload::Ellipsis),
        _ => false,
    }
}

fn classinfo_recursion_error() -> PyException {
    PyException::recursion_error("maximum recursion depth exceeded in __instancecheck__")
}

fn descriptor_attr(obj: &PyObjectRef, name: &str) -> PyResult<Option<PyObjectRef>> {
    let PyObjectPayload::Instance(inst) = &obj.payload else {
        return Ok(obj.get_attr(name));
    };
    if let Some(attr) = lookup_in_class_mro(&inst.class, name) {
        if let Some(getter) = property_field(&attr, "fget") {
            if matches!(&getter.payload, PyObjectPayload::None) {
                return Err(PyException::attribute_error(format!(
                    "unreadable attribute '{}'",
                    name
                )));
            }
            return Ok(Some(call_callable(&getter, &[obj.clone()])?));
        }
    }
    Ok(obj.get_attr(name))
}

fn type_bases(obj: &PyObjectRef, depth: usize) -> PyResult<Option<Vec<PyObjectRef>>> {
    if depth > CLASSINFO_RECURSION_LIMIT {
        return Err(classinfo_recursion_error());
    }
    match &obj.payload {
        PyObjectPayload::Class(cd) => Ok(Some(cd.bases.clone())),
        PyObjectPayload::BuiltinType(name) | PyObjectPayload::BuiltinFunction(name) => {
            if name.as_str() == "object" {
                Ok(Some(vec![]))
            } else if name.as_str() == "bool" {
                Ok(Some(vec![PyObject::builtin_type(CompactString::from(
                    "int",
                ))]))
            } else {
                Ok(Some(vec![PyObject::builtin_type(CompactString::from(
                    "object",
                ))]))
            }
        }
        PyObjectPayload::ExceptionType(kind) => {
            let parent = match kind {
                ExceptionKind::BaseException => {
                    Some(PyObject::builtin_type(CompactString::from("object")))
                }
                ExceptionKind::Exception => {
                    Some(PyObject::exception_type(ExceptionKind::BaseException))
                }
                _ => Some(PyObject::exception_type(ExceptionKind::Exception)),
            };
            Ok(Some(parent.map_or_else(Vec::new, |p| vec![p])))
        }
        PyObjectPayload::NativeFunction(nf)
            if native_function_class_name(nf.name.as_str()).is_some() =>
        {
            Ok(Some(vec![PyObject::builtin_type(CompactString::from(
                "object",
            ))]))
        }
        _ => match descriptor_attr(obj, "__bases__")? {
            Some(bases) => match &bases.payload {
                PyObjectPayload::Tuple(items) => Ok(Some(items.iter().cloned().collect())),
                _ => Err(PyException::type_error("__bases__ must be tuple")),
            },
            None => Ok(None),
        },
    }
}

fn is_subclass_result(sub: &PyObjectRef, sup: &PyObjectRef, depth: usize) -> PyResult<bool> {
    if depth > CLASSINFO_RECURSION_LIMIT {
        return Err(classinfo_recursion_error());
    }
    if let PyObjectPayload::Tuple(types) = &sup.payload {
        for t in types.iter() {
            if is_subclass_result(sub, t, depth + 1)? {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    match (&sub.payload, &sup.payload) {
        (PyObjectPayload::ExceptionType(a), PyObjectPayload::ExceptionType(b)) => {
            return Ok(a == b || is_exception_subclass(a, b));
        }
        (PyObjectPayload::Class(_), PyObjectPayload::Class(_))
        | (PyObjectPayload::Class(_), PyObjectPayload::ExceptionType(_))
        | (PyObjectPayload::BuiltinType(_), PyObjectPayload::BuiltinType(_))
        | (PyObjectPayload::Class(_), PyObjectPayload::BuiltinType(_))
        | (PyObjectPayload::BuiltinType(_), PyObjectPayload::Class(_)) => {
            if let (PyObjectPayload::Class(_), PyObjectPayload::Class(_)) =
                (&sub.payload, &sup.payload)
            {
                if class_abc_virtual_match(sub, sup) {
                    return Ok(true);
                }
            }
            if class_blocks_structural_abc(sub, sup) {
                return Ok(false);
            }
            if check_subclass(sub, sup) {
                return Ok(true);
            }
        }
        (PyObjectPayload::NativeFunction(nf), PyObjectPayload::Class(sup_cd)) => {
            if let Some(class_name) = native_function_class_name(nf.name.as_str()) {
                if abc_builtin_type_names(sup_cd.name.as_str())
                    .iter()
                    .any(|builtin| *builtin == class_name)
                {
                    return Ok(true);
                }
            }
        }
        (PyObjectPayload::BuiltinFunction(name), PyObjectPayload::Class(sup_cd)) => {
            if abc_builtin_type_names(sup_cd.name.as_str())
                .iter()
                .any(|builtin| *builtin == name.as_str())
            {
                return Ok(true);
            }
        }
        _ => {}
    }
    let Some(sub_bases) = type_bases(sub, depth + 1)? else {
        return Err(PyException::type_error(
            "issubclass() arg 1 must be a class",
        ));
    };
    if type_bases(sup, depth + 1)?.is_none() {
        return Err(PyException::type_error(
            "issubclass() arg 2 must be a class or tuple of classes",
        ));
    }
    if PyObjectRef::ptr_eq(sub, sup) {
        return Ok(true);
    }
    if let (PyObjectPayload::Class(_), PyObjectPayload::Class(_)) = (&sub.payload, &sup.payload) {
        if class_abc_virtual_match(sub, sup) {
            return Ok(true);
        }
    }
    if class_blocks_structural_abc(sub, sup) {
        return Ok(false);
    }
    for base in sub_bases {
        if PyObjectRef::ptr_eq(&base, sup) || is_subclass_result(&base, sup, depth + 1)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn is_instance_of_result(obj: &PyObjectRef, cls: &PyObjectRef, depth: usize) -> PyResult<bool> {
    if depth > CLASSINFO_RECURSION_LIMIT {
        return Err(classinfo_recursion_error());
    }
    if let PyObjectPayload::Tuple(types) = &cls.payload {
        for t in types.iter() {
            if is_instance_of_result(obj, t, depth + 1)? {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    // Handle PEP 604 union types: isinstance(x, int | str)
    if let Some(union_flag) = cls.get_attr("__union_params__") {
        if union_flag.is_truthy() {
            if let Some(args_tuple) = cls.get_attr("__args__") {
                if let PyObjectPayload::Tuple(types) = &args_tuple.payload {
                    for t in types.iter() {
                        if is_instance_of_result(obj, t, depth + 1)? {
                            return Ok(true);
                        }
                    }
                    return Ok(false);
                }
            }
        }
    }
    if is_instance_of(obj, cls) {
        return Ok(true);
    }
    let inst_class = descriptor_attr(obj, "__class__")?;
    if let Some(inst_class) = inst_class {
        match type_bases(&inst_class, depth + 1) {
            Ok(Some(_)) => {}
            Ok(None) => return Ok(false),
            Err(err) if err.kind == ExceptionKind::AttributeError => return Ok(false),
            Err(err) => return Err(err),
        }
        match type_bases(cls, depth + 1) {
            Ok(Some(_)) => {}
            Ok(None)
            | Err(PyException {
                kind: ExceptionKind::AttributeError,
                ..
            }) => {
                return Err(PyException::type_error(
                    "isinstance() arg 2 must be a type or tuple of types",
                ));
            }
            Err(err) => return Err(err),
        }
        match is_subclass_result(&inst_class, cls, depth + 1) {
            Ok(value) => return Ok(value),
            Err(err) if err.kind == ExceptionKind::AttributeError => {
                return Err(PyException::type_error(
                    "isinstance() arg 2 must be a type or tuple of types",
                ));
            }
            Err(err) => return Err(err),
        }
    }
    match type_bases(cls, depth + 1) {
        Ok(Some(_)) => {}
        Ok(None)
        | Err(PyException {
            kind: ExceptionKind::AttributeError,
            ..
        }) => {
            return Err(PyException::type_error(
                "isinstance() arg 2 must be a type or tuple of types",
            ));
        }
        Err(err) => return Err(err),
    }
    Ok(false)
}

/// Check if obj is an instance of cls (including inheritance).
pub(crate) fn is_instance_of(obj: &PyObjectRef, cls: &PyObjectRef) -> bool {
    match &cls.payload {
        PyObjectPayload::BuiltinFunction(type_name) | PyObjectPayload::BuiltinType(type_name) => {
            // Everything is an instance of object
            if type_name.as_str() == "object" {
                return true;
            }
            let obj_type = obj.type_name();
            if obj_type == type_name.as_str() {
                return true;
            }
            // Built-in subtype relationships: bool is subclass of int
            if type_name.as_str() == "int" && obj_type == "bool" {
                return true;
            }
            // IntEnum members are also int instances
            if type_name.as_str() == "int" {
                if let PyObjectPayload::Instance(inst) = &obj.payload {
                    if let PyObjectPayload::Class(cd) = &inst.class.payload {
                        let ns = cd.namespace.read();
                        if ns.contains_key("__int_enum__") || ns.contains_key("_value_") {
                            for base in &cd.bases {
                                if class_is_subclass_of(base, "IntEnum")
                                    || class_is_subclass_of(base, "int")
                                {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
            // collections.abc structural checks (duck typing)
            if check_abc_structural(obj, type_name.as_str()) {
                return true;
            }
            // Check user-defined classes that inherit from builtins
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                return class_is_subclass_of(&inst.class, type_name.as_str());
            }
            false
        }
        PyObjectPayload::Class(target_cd) => {
            if obj.type_name() == target_cd.name.as_str() {
                return true;
            }
            if matches!(
                target_cd.name.as_str(),
                "Num" | "Str" | "Bytes" | "NameConstant" | "Ellipsis"
            ) {
                if let Some(value) = ast_constant_value(obj) {
                    return value
                        .as_ref()
                        .map(|v| ast_constant_matches_legacy(v, target_cd.name.as_str()))
                        .unwrap_or(false);
                }
            }
            // Check _abc_builtin_types registry (collections.abc uses this)
            let obj_type = obj.type_name();
            if let Some(registry) = target_cd.namespace.read().get("_abc_builtin_types") {
                if let PyObjectPayload::Set(set) = &registry.payload {
                    let key = HashableKey::str_key(CompactString::from(obj_type));
                    if set.read().contains_key(&key) {
                        return true;
                    }
                }
            }
            if let Some(obj_class) = match &obj.payload {
                PyObjectPayload::Instance(inst) => Some(inst.class.clone()),
                PyObjectPayload::Class(_) => Some(obj.clone()),
                _ => None,
            } {
                if class_blocks_structural_abc(&obj_class, cls) {
                    return false;
                }
            }
            // Check collections.abc structural typing for Class-based ABCs
            if check_abc_structural(obj, target_cd.name.as_str()) {
                return true;
            }
            // Check _abc_registry for ABCMeta.register() virtual subclasses
            // Walk the class and its bases (MRO) to find registries
            if let Some(obj_class) = match &obj.payload {
                PyObjectPayload::Instance(inst) => Some(inst.class.clone()),
                PyObjectPayload::Class(_) => Some(obj.clone()),
                _ => None,
            } {
                if class_abc_virtual_match(&obj_class, cls) {
                    return true;
                }
            }
            // runtime_checkable Protocol check
            if let Some(flag) = target_cd.namespace.read().get("_is_runtime_checkable") {
                if flag.is_truthy() {
                    if let Some(attrs) = target_cd.namespace.read().get("__protocol_attrs__") {
                        if let PyObjectPayload::Tuple(required) = &attrs.payload {
                            return required.iter().all(|attr_name| {
                                let name = attr_name.py_to_string();
                                obj.get_attr(&name).is_some()
                            });
                        }
                    }
                }
            }
            // User-defined class check: walk the instance's class MRO
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                class_has_exact_base(&inst.class, cls)
            } else if let PyObjectPayload::Class(obj_cd) = &obj.payload {
                // Metaclass check: isinstance(MyClass, Meta) where Meta is a metaclass
                if let Some(ref mcs) = obj_cd.metaclass {
                    if let PyObjectPayload::Class(mcs_cd) = &mcs.payload {
                        if mcs_cd.name == target_cd.name {
                            return true;
                        }
                        // Check MRO of the metaclass
                        return class_is_subclass_of(mcs, &target_cd.name);
                    }
                }
                // All classes are instances of 'type'
                target_cd.name.as_str() == "type"
            } else {
                false
            }
        }
        PyObjectPayload::ExceptionType(kind) => {
            // Check if obj is an exception instance of this type
            if let PyObjectPayload::ExceptionInstance(ei) = &obj.payload {
                if &ei.kind == kind {
                    return true;
                }
                // Check exception hierarchy
                return exception_is_subclass_of(ei.kind, &format!("{:?}", kind));
            }
            // Check if obj is a user-defined class instance that inherits from this exception
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                let kind_name = format!("{:?}", kind);
                return class_is_subclass_of(&inst.class, &kind_name);
            }
            false
        }
        // NativeFunction/NativeClosure used as constructor (e.g., ChainMap, OrderedDict):
        // Check if the instance's class name matches
        PyObjectPayload::NativeFunction(nf) => {
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    let cls_name = cd.name.as_str();
                    if !nf.name.is_empty() && cls_name == nf.name.as_str() {
                        return true;
                    }
                    return class_is_subclass_of(&inst.class, nf.name.as_str());
                }
            }
            false
        }
        PyObjectPayload::NativeClosure(nc) => {
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    let cls_name = cd.name.as_str();
                    if !nc.name.is_empty() && cls_name == nc.name.as_str() {
                        return true;
                    }
                    return class_is_subclass_of(&inst.class, nc.name.as_str());
                }
            }
            false
        }
        _ => false,
    }
}
pub(crate) fn class_is_subclass_of(cls: &PyObjectRef, target_name: &str) -> bool {
    match &cls.payload {
        PyObjectPayload::Class(cd) => {
            if cd.name.as_str() == target_name {
                return true;
            }
            for base in &cd.bases {
                if class_is_subclass_of(base, target_name) {
                    return true;
                }
            }
            false
        }
        // Handle builtin type bases (e.g., class MyList(list))
        PyObjectPayload::BuiltinFunction(name) | PyObjectPayload::BuiltinType(name) => {
            name.as_str() == target_name
        }
        // Handle exception type bases (e.g., class AppError(Exception))
        PyObjectPayload::ExceptionType(kind) => {
            let kind_name = format!("{:?}", kind);
            if kind_name == target_name {
                return true;
            }
            // Walk up the exception hierarchy
            exception_is_subclass_of(*kind, target_name)
        }
        _ => false,
    }
}

/// Check if an ExceptionKind is a subclass of a target by name.
fn exception_is_subclass_of(kind: ExceptionKind, target_name: &str) -> bool {
    if let Some(target_kind) = ExceptionKind::from_name(target_name) {
        is_exception_subclass(&kind, &target_kind)
    } else {
        false
    }
}

/// Structural (duck-type) check for collections.abc ABCs.
fn abc_required_methods(abc_name: &str) -> &'static [&'static str] {
    match abc_name {
        "Hashable" => &["__hash__"],
        "Iterable" => &["__iter__"],
        "Iterator" => &["__iter__", "__next__"],
        "Reversible" => &["__iter__", "__reversed__"],
        "Generator" => &["__iter__", "__next__", "send", "throw", "close"],
        "Sized" => &["__len__"],
        "Container" => &["__contains__"],
        "Callable" => &["__call__"],
        "Collection" => &["__len__", "__iter__", "__contains__"],
        "Sequence" => &["__contains__", "__iter__", "__getitem__", "__len__"],
        "MutableSequence" => &[
            "__contains__",
            "__iter__",
            "__getitem__",
            "__len__",
            "__setitem__",
            "__delitem__",
            "insert",
        ],
        "Set" => &["__contains__", "__iter__", "__len__"],
        "MutableSet" => &["__contains__", "__iter__", "__len__", "add", "discard"],
        "Mapping" => &["__getitem__", "__iter__", "__len__"],
        "MutableMapping" => &[
            "__getitem__",
            "__iter__",
            "__len__",
            "__setitem__",
            "__delitem__",
        ],
        "Awaitable" => &["__await__"],
        "Coroutine" => &["send", "throw", "close", "__await__"],
        "AsyncIterable" => &["__aiter__"],
        "AsyncIterator" => &["__aiter__", "__anext__"],
        "AsyncGenerator" => &["__aiter__", "__anext__", "asend", "athrow"],
        _ => &[],
    }
}

fn abc_builtin_type_names(abc_name: &str) -> &'static [&'static str] {
    match abc_name {
        "Hashable" => &[
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
        "Iterable" => &[
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
            "list_iterator",
            "tuple_iterator",
            "dict_keyiterator",
            "dict_valueiterator",
            "dict_itemiterator",
            "set_iterator",
        ],
        "Iterator" => &[
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
            "set_iterator",
            "list_reverseiterator",
        ],
        "Reversible" => &[
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
        "Sized" => &[
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
            "dict_keyiterator",
            "dict_valueiterator",
            "dict_itemiterator",
        ],
        "Container" => &[
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
            "dict_keyiterator",
            "dict_itemiterator",
        ],
        "Collection" => &[
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
            "dict_keyiterator",
            "dict_valueiterator",
            "dict_itemiterator",
        ],
        "Sequence" => &[
            "list",
            "tuple",
            "str",
            "bytes",
            "bytearray",
            "range",
            "memoryview",
        ],
        "MutableSequence" => &["list", "bytearray", "deque"],
        "ByteString" => &["bytes", "bytearray"],
        "Set" => &["set", "frozenset", "dict_keys", "dict_items"],
        "MutableSet" => &["set"],
        "Mapping" => &["dict", "Counter", "UserDict"],
        "MutableMapping" => &["dict", "Counter", "UserDict"],
        "Callable" => &["function", "method", "type"],
        "Awaitable" => &["coroutine"],
        "Coroutine" => &["coroutine"],
        "AsyncIterable" => &["async_generator"],
        "AsyncIterator" => &["async_generator"],
        "AsyncGenerator" => &["async_generator"],
        "Number" | "Complex" => &["int", "bool", "float", "complex"],
        "Real" => &["int", "bool", "float"],
        "Rational" | "Integral" => &["int", "bool"],
        _ => &[],
    }
}

fn class_lookup_abc_method_blocking(cls: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    if let PyObjectPayload::Class(cd) = &cls.payload {
        if let Some(attr) = cd.namespace.read().get(name) {
            return Some(attr.clone());
        }
        for base in &cd.mro {
            if matches!(&base.payload, PyObjectPayload::BuiltinType(name) if name.as_str() == "object")
                && name != "__hash__"
            {
                continue;
            }
            if let Some(attr) = class_lookup_abc_method_blocking(base, name) {
                return Some(attr);
            } else if !matches!(&base.payload, PyObjectPayload::Class(_)) {
                if let Some(attr) = base.get_attr(name) {
                    return Some(attr);
                }
            }
        }
        for base in &cd.bases {
            if matches!(&base.payload, PyObjectPayload::BuiltinType(type_name) if type_name.as_str() == "object")
                && name != "__hash__"
            {
                continue;
            }
            if let Some(attr) = class_lookup_abc_method_blocking(base, name) {
                return Some(attr);
            }
        }
    } else {
        return cls.get_attr(name);
    }
    None
}

fn class_blocks_structural_abc(sub: &PyObjectRef, sup: &PyObjectRef) -> bool {
    let (PyObjectPayload::Class(sub_cd), PyObjectPayload::Class(sup_cd)) =
        (&sub.payload, &sup.payload)
    else {
        return false;
    };
    if !is_known_structural_abc(sup_cd.name.as_str()) {
        return false;
    }
    let ns = sub_cd.namespace.read();
    abc_required_methods(sup_cd.name.as_str())
        .iter()
        .any(|name| {
            ns.get(*name)
                .map(|attr| matches!(&attr.payload, PyObjectPayload::None))
                .unwrap_or(false)
        })
}

fn classes_for_abc_registry(cls: &PyObjectRef) -> Vec<PyObjectRef> {
    let mut classes = vec![cls.clone()];
    if let PyObjectPayload::Class(cd) = &cls.payload {
        classes.extend(cd.bases.iter().cloned());
        classes.extend(cd.mro.iter().cloned());
    }
    classes
}

fn abc_registry_contains(abc: &PyObjectRef, sub: &PyObjectRef) -> bool {
    let Some(registry) = abc.get_attr("_abc_registry") else {
        return false;
    };
    let PyObjectPayload::Dict(map) = &registry.payload else {
        return false;
    };
    map.read().iter().any(|(key, _)| match key {
        HashableKey::Identity(_, registered) => {
            PyObjectRef::ptr_eq(registered, sub) || check_subclass(sub, registered)
        }
        _ => false,
    })
}

fn abc_registry_or_subclass_registry_contains(abc: &PyObjectRef, sub: &PyObjectRef) -> bool {
    if abc_registry_contains(abc, sub) {
        return true;
    }
    let PyObjectPayload::Class(cd) = &abc.payload else {
        return false;
    };
    let subclasses: Vec<_> = cd
        .subclasses
        .read()
        .iter()
        .filter_map(|weak| weak.upgrade())
        .collect();
    subclasses
        .iter()
        .any(|child| abc_registry_or_subclass_registry_contains(child, sub))
}

fn class_abc_virtual_match(sub: &PyObjectRef, sup: &PyObjectRef) -> bool {
    classes_for_abc_registry(sup)
        .iter()
        .any(|abc| abc_registry_or_subclass_registry_contains(abc, sub))
}

fn class_has_exact_base(cls: &PyObjectRef, target: &PyObjectRef) -> bool {
    if PyObjectRef::ptr_eq(cls, target) {
        return true;
    }
    let PyObjectPayload::Class(cd) = &cls.payload else {
        return false;
    };
    cd.mro.iter().any(|base| PyObjectRef::ptr_eq(base, target))
        || cd
            .bases
            .iter()
            .any(|base| PyObjectRef::ptr_eq(base, target) || class_has_exact_base(base, target))
}

fn native_function_class_name(name: &str) -> Option<&'static str> {
    match name {
        "range" => Some("range"),
        "collections.deque" => Some("deque"),
        _ => None,
    }
}

fn object_has_abc_method(obj: &PyObjectRef, name: &str) -> bool {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        return class_lookup_abc_method_blocking(&inst.class, name)
            .map(|attr| !matches!(&attr.payload, PyObjectPayload::None))
            .unwrap_or(false);
    }
    obj.get_attr(name)
        .map(|attr| !matches!(&attr.payload, PyObjectPayload::None))
        .unwrap_or(false)
}

fn class_has_abc_method(cls: &PyObjectRef, name: &str) -> bool {
    class_lookup_abc_method_blocking(cls, name)
        .map(|attr| !matches!(&attr.payload, PyObjectPayload::None))
        .unwrap_or(false)
}

fn abc_hashable_blocked_type(name: &str) -> bool {
    matches!(name, "bytearray" | "dict" | "list" | "set")
}

fn is_known_structural_abc(name: &str) -> bool {
    matches!(
        name,
        "Hashable"
            | "Iterable"
            | "Iterator"
            | "Reversible"
            | "Generator"
            | "Sized"
            | "Container"
            | "Callable"
            | "Collection"
            | "Sequence"
            | "MutableSequence"
            | "Set"
            | "MutableSet"
            | "Mapping"
            | "MutableMapping"
            | "ByteString"
            | "Awaitable"
            | "Coroutine"
            | "AsyncIterable"
            | "AsyncIterator"
            | "AsyncGenerator"
            | "AbstractContextManager"
            | "Number"
            | "Complex"
            | "Real"
            | "Rational"
            | "Integral"
    )
}

fn check_abc_structural_class(cls: &PyObjectRef, abc_name: &str) -> bool {
    if !is_known_structural_abc(abc_name) {
        return false;
    }
    if let PyObjectPayload::Class(cd) = &cls.payload {
        if abc_builtin_type_names(abc_name)
            .iter()
            .any(|builtin| *builtin == cd.name.as_str())
        {
            return true;
        }
        if let Some(base_name) = &cd.builtin_base_name {
            if abc_builtin_type_names(abc_name)
                .iter()
                .any(|builtin| *builtin == base_name.as_str())
            {
                return true;
            }
        }
        let blocked_hashable = abc_hashable_blocked_type(cd.name.as_str())
            || cd
                .builtin_base_name
                .as_ref()
                .map(|name| abc_hashable_blocked_type(name.as_str()))
                .unwrap_or(false);
        match abc_name {
            "Hashable" => {
                if blocked_hashable {
                    false
                } else {
                    class_has_abc_method(cls, "__hash__")
                }
            }
            "Callable" => class_has_abc_method(cls, "__call__"),
            "Sequence" | "MutableSequence" | "ByteString" | "Set" | "MutableSet" | "Mapping"
            | "MutableMapping" => false,
            _ => abc_required_methods(abc_name)
                .iter()
                .all(|name| class_has_abc_method(cls, name)),
        }
    } else {
        false
    }
}

fn check_abc_structural(obj: &PyObjectRef, abc_name: &str) -> bool {
    if !is_known_structural_abc(abc_name) {
        return false;
    }
    if abc_builtin_type_names(abc_name)
        .iter()
        .any(|builtin| *builtin == obj.type_name())
    {
        return true;
    }
    match abc_name {
        "ByteString" => matches!(obj.type_name(), "bytes" | "bytearray"),
        "Callable" => object_has_abc_method(obj, "__call__"),
        "Hashable" => {
            if abc_hashable_blocked_type(obj.type_name()) {
                false
            } else {
                object_has_abc_method(obj, "__hash__")
            }
        }
        "Number" | "Complex" => {
            matches!(obj.type_name(), "int" | "float" | "complex" | "bool")
        }
        "Real" => {
            matches!(obj.type_name(), "int" | "float" | "bool")
        }
        "Rational" | "Integral" => {
            matches!(obj.type_name(), "int" | "bool")
        }
        "Sequence" | "MutableSequence" | "Set" | "MutableSet" | "Mapping" | "MutableMapping" => {
            false
        }
        _ => abc_required_methods(abc_name)
            .iter()
            .all(|name| object_has_abc_method(obj, name)),
    }
}

pub(super) fn builtin_issubclass(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args("issubclass", args, 2)?;
    match is_subclass_result(&args[0], &args[1], 0) {
        Ok(value) => Ok(PyObject::bool_val(value)),
        Err(err) if err.kind == ExceptionKind::AttributeError => Err(PyException::type_error(
            "issubclass() arg 2 must be a class or tuple of classes",
        )),
        Err(err) => Err(err),
    }
}

pub(crate) fn check_subclass(sub: &PyObjectRef, sup: &PyObjectRef) -> bool {
    match (&sub.payload, &sup.payload) {
        (PyObjectPayload::Class(sub_cd), PyObjectPayload::Class(sup_cd)) => {
            if PyObjectRef::ptr_eq(sub, sup) {
                return true;
            }
            if let Some(registered) = sub_cd.namespace.read().get("__abc_registered__") {
                if PyObjectRef::ptr_eq(registered, sup) || check_subclass(registered, sup) {
                    return true;
                }
            }
            // Walk full MRO
            for base in &sub_cd.mro {
                if PyObjectRef::ptr_eq(base, sup) {
                    return true;
                }
            }
            // Also check direct bases
            for base in &sub_cd.bases {
                if PyObjectRef::ptr_eq(base, sup) || check_subclass(base, sup) {
                    return true;
                }
            }
            if let Some(registry) = sup_cd.namespace.read().get("_abc_builtin_types") {
                if let PyObjectPayload::Set(set) = &registry.payload {
                    let sub_name = HashableKey::str_key(CompactString::from(sub_cd.name.as_str()));
                    if set.read().contains_key(&sub_name) {
                        return true;
                    }
                    if let Some(base_name) = &sub_cd.builtin_base_name {
                        let base_key =
                            HashableKey::str_key(CompactString::from(base_name.as_str()));
                        if set.read().contains_key(&base_key) {
                            return true;
                        }
                    }
                }
            }
            // Check _abc_registry for virtual subclass registration
            if class_abc_virtual_match(sub, sup) {
                return true;
            }
            if class_blocks_structural_abc(sub, sup) {
                return false;
            }
            if check_abc_structural_class(sub, sup_cd.name.as_str()) {
                return true;
            }
            false
        }
        // Class inheriting from ExceptionType (e.g. class MyError(ValueError))
        (PyObjectPayload::Class(sub_cd), PyObjectPayload::ExceptionType(target_kind)) => {
            let _target_name = format!("{:?}", target_kind);
            // Check bases: is any base an ExceptionType matching target?
            for base in &sub_cd.bases {
                if let PyObjectPayload::ExceptionType(bk) = &base.payload {
                    if bk == target_kind {
                        return true;
                    }
                    // Check exception hierarchy
                    if is_exception_subclass(bk, target_kind) {
                        return true;
                    }
                }
                // Recursively check class bases
                if check_subclass(base, sup) {
                    return true;
                }
            }
            false
        }
        (PyObjectPayload::ExceptionType(a), PyObjectPayload::ExceptionType(b)) => {
            a == b || is_exception_subclass(a, b)
        }
        // BuiltinType subclass (bool is subclass of int)
        (PyObjectPayload::BuiltinType(a), PyObjectPayload::BuiltinType(b)) => {
            a == b || (a.as_str() == "bool" && b.as_str() == "int") || b.as_str() == "object"
            // everything is a subclass of object
        }
        // Any type is subclass of object
        (_, PyObjectPayload::BuiltinType(b)) if b.as_str() == "object" => true,
        // Class checking against BuiltinType: walk MRO for matching BuiltinType
        (PyObjectPayload::Class(sub_cd), PyObjectPayload::BuiltinType(target)) => {
            for base in &sub_cd.mro {
                if let PyObjectPayload::BuiltinType(bt) = &base.payload {
                    if bt == target {
                        return true;
                    }
                }
            }
            for base in &sub_cd.bases {
                if let PyObjectPayload::BuiltinType(bt) = &base.payload {
                    if bt == target {
                        return true;
                    }
                }
            }
            false
        }
        // BuiltinType vs ABC Class: check _abc_builtin_types registry
        (PyObjectPayload::BuiltinType(type_name), PyObjectPayload::Class(sup_cd)) => {
            if abc_builtin_type_names(sup_cd.name.as_str())
                .iter()
                .any(|builtin| *builtin == type_name.as_str())
            {
                return true;
            }
            if let Some(registry) = sup_cd.namespace.read().get("_abc_builtin_types") {
                if let PyObjectPayload::Set(set) = &registry.payload {
                    let key = HashableKey::str_key(CompactString::from(type_name.as_str()));
                    return set.read().contains_key(&key);
                }
            }
            false
        }
        _ => false,
    }
}

/// Check if exception kind `child` is a subclass of `parent` in the hierarchy.
pub(crate) fn is_exception_subclass(child: &ExceptionKind, parent: &ExceptionKind) -> bool {
    child.is_subclass_of(parent)
}
