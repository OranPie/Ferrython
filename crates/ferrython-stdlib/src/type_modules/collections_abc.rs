use super::*;

mod generators;
mod helpers;
mod mapping_views;
mod sequences;
mod sets;

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
            PyObjectPayload::Class(cd) => {
                PyObjectRef::ptr_eq(&inst.class, set_cls)
                    || PyObjectRef::ptr_eq(&inst.class, mutable_set_cls)
                    || cd.mro.iter().any(|base| {
                        PyObjectRef::ptr_eq(base, set_cls)
                            || PyObjectRef::ptr_eq(base, mutable_set_cls)
                    })
            }
            _ => false,
        },
        _ => false,
    }
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
            "set_iterator",
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
            "set_iterator",
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
            "dict_keyiterator",
            "dict_valueiterator",
            "dict_itemiterator",
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
    sequences::add_sequence_methods(&sequence_cls, &mutable_sequence_cls);
    sets::add_set_methods(&set_cls, &mutable_set_cls);

    generators::add_generator_methods(
        &generator_cls,
        &coroutine_cls,
        &async_iterator_cls,
        &async_generator_cls,
    );

    mapping_views::add_mapping_view_methods(
        &mapping_view_cls,
        &keys_view_cls,
        &items_view_cls,
        &values_view_cls,
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
