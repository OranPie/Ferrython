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

mod abc;
mod enum_module;
mod types_module;
mod typing;

pub use abc::create_abc_module;
pub use enum_module::create_enum_module;
pub use types_module::create_types_module;
pub use typing::create_typing_module;

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
