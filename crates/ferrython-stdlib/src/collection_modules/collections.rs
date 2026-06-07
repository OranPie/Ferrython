use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    make_builtin, make_module, FxHashKeyMap, PyCell, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::cell::RefCell;

use super::chainmap::make_chainmap_class;
use super::counter::{
    collections_most_common, count_elements, counter_clear, counter_copy, counter_elements,
    counter_subtract, counter_total, counter_update, make_counter_class, make_defaultdict_class,
};
use super::deque::collections_deque;
use super::namedtuple::{
    collections_namedtuple, namedtuple_field_class, namedtuple_rebuild_class,
    namedtuple_rebuild_field, namedtuple_rebuild_instance,
};
use super::user_types::{make_user_dict_class, make_user_list_class, make_user_string_class};

thread_local! {
    static ORDERED_DICT_REPR_GUARD: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

pub fn create_collections_module() -> PyObjectRef {
    let abc_module = crate::type_modules::create_collections_abc_module();

    // Deprecated aliases (Python 3.3-3.9 compat — removed in 3.10 but many packages still use them)
    let get_abc_attr =
        |name: &str| -> PyObjectRef { abc_module.get_attr(name).unwrap_or(PyObject::none()) };
    let mapping = get_abc_attr("Mapping");
    let mutable_mapping = get_abc_attr("MutableMapping");
    let sequence = get_abc_attr("Sequence");
    let mutable_sequence = get_abc_attr("MutableSequence");
    let set_abc = get_abc_attr("Set");
    let mutable_set = get_abc_attr("MutableSet");
    let iterable = get_abc_attr("Iterable");
    let iterator = get_abc_attr("Iterator");
    let callable = get_abc_attr("Callable");
    let sized = get_abc_attr("Sized");
    let container = get_abc_attr("Container");
    let hashable = get_abc_attr("Hashable");
    let namedtuple_field = namedtuple_field_class();
    let namedtuple_rebuild_field_fn =
        PyObject::native_function("_namedtuple_field_rebuild", namedtuple_rebuild_field);
    let namedtuple_rebuild_class_fn =
        PyObject::native_function("_namedtuple_rebuild_class", namedtuple_rebuild_class);
    let namedtuple_rebuild_instance_fn =
        PyObject::native_function("_namedtuple_rebuild_instance", namedtuple_rebuild_instance);

    make_module(
        "collections",
        vec![
            ("abc", abc_module),
            ("OrderedDict", make_ordered_dict_class()),
            ("defaultdict", make_defaultdict_class()),
            ("Counter", make_counter_class()),
            ("namedtuple", make_builtin(collections_namedtuple)),
            ("namedtuple_field", namedtuple_field),
            ("_namedtuple_field_rebuild", namedtuple_rebuild_field_fn),
            ("_namedtuple_rebuild_class", namedtuple_rebuild_class_fn),
            (
                "_namedtuple_rebuild_instance",
                namedtuple_rebuild_instance_fn,
            ),
            (
                "deque",
                PyObject::native_function("collections.deque", collections_deque),
            ),
            ("most_common", make_builtin(collections_most_common)),
            ("ChainMap", make_chainmap_class()),
            ("UserDict", make_user_dict_class()),
            ("UserList", make_user_list_class()),
            ("UserString", make_user_string_class()),
            // Deprecated ABC aliases for compat
            ("Mapping", mapping),
            ("MutableMapping", mutable_mapping),
            ("Sequence", sequence),
            ("MutableSequence", mutable_sequence),
            ("Set", set_abc),
            ("MutableSet", mutable_set),
            ("Iterable", iterable),
            ("Iterator", iterator),
            ("Callable", callable),
            ("Sized", sized),
            ("Container", container),
            ("Hashable", hashable),
            // Counter helper functions
            ("counter_elements", make_builtin(counter_elements)),
            ("counter_update", make_builtin(counter_update)),
            ("counter_subtract", make_builtin(counter_subtract)),
            ("counter_total", make_builtin(counter_total)),
            ("counter_copy", make_builtin(counter_copy)),
            ("counter_clear", make_builtin(counter_clear)),
            // _count_elements(mapping, iterable) — C accelerator for Counter
            ("_count_elements", make_builtin(count_elements)),
        ],
    )
}

fn ordered_dict_storage(obj: &PyObjectRef) -> PyResult<std::rc::Rc<PyCell<FxHashKeyMap>>> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        if let Some(storage) = inst.dict_storage.as_ref() {
            return Ok(storage.clone());
        }
    }
    Err(PyException::type_error(
        "OrderedDict method requires an instance",
    ))
}

fn ordered_dict_marker_key() -> HashableKey {
    HashableKey::str_key(CompactString::from("__ordered_dict__"))
}

fn ordered_dict_broken_key() -> HashableKey {
    HashableKey::str_key(CompactString::from("__ordered_dict_broken__"))
}

fn ordered_dict_check_consistent(storage: &std::rc::Rc<PyCell<FxHashKeyMap>>) -> PyResult<()> {
    if storage.read().contains_key(&ordered_dict_broken_key()) {
        return Err(PyException::key_error("OrderedDict mutated by dict method"));
    }
    Ok(())
}

fn ordered_dict_kwarg(args: &[PyObjectRef], name: &str) -> Option<PyObjectRef> {
    let PyObjectPayload::Dict(map) = &args.last()?.payload else {
        return None;
    };
    let read = map.read();
    if !read
        .get(&HashableKey::str_key(CompactString::from(
            "__ordered_dict_kwargs__",
        )))
        .map(|v| v.is_truthy())
        .unwrap_or(false)
    {
        return None;
    }
    read.get(&HashableKey::str_key(CompactString::from(name)))
        .cloned()
}

fn ordered_dict_entries_from_obj(obj: &PyObjectRef) -> PyResult<Vec<(HashableKey, PyObjectRef)>> {
    match &obj.payload {
        PyObjectPayload::Dict(map) => Ok(map
            .read()
            .iter()
            .filter(|(k, _)| !ferrython_core::object::is_hidden_dict_key(k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()),
        PyObjectPayload::Instance(inst) => {
            if let Some(storage) = inst.dict_storage.as_ref() {
                return Ok(storage
                    .read()
                    .iter()
                    .filter(|(k, _)| !ferrython_core::object::is_hidden_dict_key(k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect());
            }
            if let Some(keys_method) = obj.get_attr("keys") {
                let keys_obj = ferrython_core::object::call_callable(&keys_method, &[])?;
                let keys = keys_obj.to_list()?;
                let mut out = Vec::new();
                for key_obj in keys {
                    let value = obj.get_item(&key_obj)?;
                    out.push((key_obj.to_hashable_key()?, value));
                }
                return Ok(out);
            }
            ordered_dict_pairs_from_iterable(obj)
        }
        _ => ordered_dict_pairs_from_iterable(obj),
    }
}

fn ordered_dict_pairs_from_iterable(
    obj: &PyObjectRef,
) -> PyResult<Vec<(HashableKey, PyObjectRef)>> {
    let mut out = Vec::new();
    for item in obj.to_list()? {
        let pair = item.to_list()?;
        if pair.len() != 2 {
            return Err(PyException::value_error(
                "dictionary update sequence element has length other than 2",
            ));
        }
        out.push((pair[0].to_hashable_key()?, pair[1].clone()));
    }
    Ok(out)
}

fn ordered_dict_parse_args(
    args: &[PyObjectRef],
) -> PyResult<(
    PyObjectRef,
    Vec<PyObjectRef>,
    IndexMap<CompactString, PyObjectRef>,
)> {
    if args.is_empty() {
        return Err(PyException::type_error("OrderedDict method requires self"));
    }
    let self_obj = args[0].clone();
    let mut pos_args = Vec::new();
    let mut kwargs = IndexMap::new();
    if args.len() >= 2 {
        let last_is_kwargs = if let PyObjectPayload::Dict(map) = &args[args.len() - 1].payload {
            map.read()
                .get(&HashableKey::str_key(CompactString::from(
                    "__ordered_dict_kwargs__",
                )))
                .map(|v| v.is_truthy())
                .unwrap_or(false)
        } else {
            false
        };
        if last_is_kwargs {
            if args.len() > 3 {
                return Err(PyException::type_error(
                    "OrderedDict expected at most one positional argument",
                ));
            }
            if args.len() == 3 {
                pos_args.push(args[1].clone());
            }
            if let PyObjectPayload::Dict(map) = &args[args.len() - 1].payload {
                for (k, v) in map.read().iter() {
                    if let HashableKey::Str(name) = k {
                        if name.as_str() == "__ordered_dict_kwargs__" {
                            continue;
                        }
                        kwargs.insert(name.to_compact_string(), v.clone());
                    }
                }
            }
            return Ok((self_obj, pos_args, kwargs));
        }
        if args.len() > 2 {
            return Err(PyException::type_error(
                "OrderedDict expected at most one positional argument",
            ));
        }
        pos_args.push(args[1].clone());
    }
    Ok((self_obj, pos_args, kwargs))
}

fn ordered_dict_update_from_parts(
    self_obj: &PyObjectRef,
    pos_args: &[PyObjectRef],
    kwargs: IndexMap<CompactString, PyObjectRef>,
) -> PyResult<()> {
    if pos_args.len() > 1 {
        return Err(PyException::type_error(
            "OrderedDict expected at most one positional argument",
        ));
    }
    let storage = ordered_dict_storage(self_obj)?;
    storage
        .write()
        .entry(ordered_dict_marker_key())
        .or_insert_with(|| PyObject::bool_val(true));
    if let Some(src) = pos_args.first() {
        for (k, v) in ordered_dict_entries_from_obj(src)? {
            storage.write().insert(k, v);
        }
    }
    for (k, v) in kwargs {
        storage.write().insert(HashableKey::str_key(k), v);
    }
    Ok(())
}

fn ordered_dict_instance_attrs(obj: &PyObjectRef) -> PyResult<Vec<(CompactString, PyObjectRef)>> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        return Ok(inst
            .attrs
            .read()
            .iter()
            .filter(|(k, _)| k.as_str() != "__ordered_dict__")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect());
    }
    Err(PyException::type_error(
        "OrderedDict method requires an instance",
    ))
}

fn ordered_dict_items(obj: &PyObjectRef) -> PyResult<Vec<(PyObjectRef, PyObjectRef)>> {
    let storage = ordered_dict_storage(obj)?;
    ordered_dict_check_consistent(&storage)?;
    Ok(storage
        .read()
        .iter()
        .filter(|(k, _)| !ferrython_core::object::is_hidden_dict_key(k))
        .map(|(k, v)| (k.to_object(), v.clone()))
        .collect())
}

struct OrderedDictReprGuard {
    ptr: usize,
}

impl OrderedDictReprGuard {
    fn enter(obj: &PyObjectRef) -> Option<Self> {
        let ptr = PyObjectRef::as_ptr(obj) as usize;
        ORDERED_DICT_REPR_GUARD.with(|active| {
            let mut active = active.borrow_mut();
            if active.contains(&ptr) {
                None
            } else {
                active.push(ptr);
                Some(Self { ptr })
            }
        })
    }
}

impl Drop for OrderedDictReprGuard {
    fn drop(&mut self) {
        ORDERED_DICT_REPR_GUARD.with(|active| {
            let mut active = active.borrow_mut();
            if let Some(pos) = active.iter().rposition(|ptr| *ptr == self.ptr) {
                active.remove(pos);
            }
        });
    }
}

fn make_ordered_dict_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__module__"),
        PyObject::str_val(CompactString::from("collections")),
    );
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("OrderedDict.__init__", move |args| {
            let (self_obj, pos_args, kwargs) = ordered_dict_parse_args(args)?;
            ordered_dict_update_from_parts(&self_obj, &pos_args, kwargs)?;
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("update"),
        PyObject::native_closure("OrderedDict.update", move |args| {
            let (self_obj, pos_args, kwargs) = ordered_dict_parse_args(args)?;
            ordered_dict_update_from_parts(&self_obj, &pos_args, kwargs)?;
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("clear"),
        PyObject::native_closure("OrderedDict.clear", move |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("clear() takes no arguments"));
            }
            let storage = ordered_dict_storage(&args[0])?;
            storage
                .write()
                .retain(|key, _| ferrython_core::object::is_hidden_dict_key(key));
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("move_to_end"),
        PyObject::native_closure("OrderedDict.move_to_end", move |args| {
            if args.len() < 2 || args.len() > 3 {
                return Err(PyException::type_error(
                    "move_to_end() expected key and optional last argument",
                ));
            }
            let key = args[1].to_hashable_key()?;
            let last = ordered_dict_kwarg(args, "last")
                .or_else(|| args.get(2).cloned())
                .map(|v| v.is_truthy())
                .unwrap_or(true);
            let storage = ordered_dict_storage(&args[0])?;
            ordered_dict_check_consistent(&storage)?;
            let mut map = storage.write();
            let Some(idx) = map.get_index_of(&key) else {
                return Err(PyException::key_error_value(args[1].clone()));
            };
            let target = if last {
                map.len().saturating_sub(1)
            } else {
                map.iter()
                    .position(|(key, _)| !ferrython_core::object::is_hidden_dict_key(key))
                    .unwrap_or(0)
            };
            map.move_index(idx, target);
            drop(map);
            ferrython_core::object::mark_dict_storage_mutated(&storage);
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__eq__"),
        PyObject::native_closure("OrderedDict.__eq__", move |args| {
            if args.len() != 2 {
                return Err(PyException::type_error("__eq__ requires self and other"));
            }
            let left = ordered_dict_items(&args[0])?;
            if let Ok(right) = ordered_dict_items(&args[1]) {
                if left.len() != right.len() {
                    return Ok(PyObject::bool_val(false));
                }
                for ((lk, lv), (rk, rv)) in left.iter().zip(right.iter()) {
                    if lk.to_hashable_key()? != rk.to_hashable_key()?
                        || PyObjectMethods::compare(lv, rv, ferrython_core::object::CompareOp::Ne)?
                            .is_truthy()
                    {
                        return Ok(PyObject::bool_val(false));
                    }
                }
                return Ok(PyObject::bool_val(true));
            }
            Ok(PyObject::bool_val(
                PyObjectMethods::compare(
                    &args[0],
                    &args[1],
                    ferrython_core::object::CompareOp::Eq,
                )?
                .is_truthy(),
            ))
        }),
    );
    ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("OrderedDict.__repr__", move |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__repr__ takes no arguments"));
            }
            let Some(_guard) = OrderedDictReprGuard::enter(&args[0]) else {
                return Ok(PyObject::str_val(CompactString::from("...")));
            };
            let items = ordered_dict_items(&args[0])?;
            if items.is_empty() {
                return Ok(PyObject::str_val(CompactString::from("OrderedDict()")));
            }
            let body = items
                .into_iter()
                .map(|(k, v)| {
                    let value_repr = if matches!(&v.payload, PyObjectPayload::Instance(_))
                        && PyObjectRef::as_ptr(&v) == PyObjectRef::as_ptr(&args[0])
                    {
                        "...".to_string()
                    } else {
                        v.repr()
                    };
                    format!("({}, {})", k.repr(), value_repr)
                })
                .collect::<Vec<_>>()
                .join(", ");
            Ok(PyObject::str_val(CompactString::from(format!(
                "OrderedDict([{}])",
                body
            ))))
        }),
    );
    ns.insert(
        CompactString::from("copy"),
        PyObject::native_closure("OrderedDict.copy", move |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("copy() takes no arguments"));
            }
            let class = if let PyObjectPayload::Instance(inst) = &args[0].payload {
                inst.class.clone()
            } else {
                return Err(PyException::type_error("copy() requires an instance"));
            };
            let result = PyObject::instance(class);
            if let Ok(dst) = ordered_dict_storage(&result) {
                let mut w = dst.write();
                w.insert(ordered_dict_marker_key(), PyObject::bool_val(true));
                for (k, v) in ordered_dict_items(&args[0])? {
                    w.insert(k.to_hashable_key()?, v);
                }
            }
            if let PyObjectPayload::Instance(result_inst) = &result.payload {
                for (k, v) in ordered_dict_instance_attrs(&args[0])? {
                    result_inst.attrs.write().insert(k, v);
                }
            }
            Ok(result)
        }),
    );
    ns.insert(
        CompactString::from("__reduce__"),
        PyObject::native_closure("OrderedDict.__reduce__", move |args| {
            if args.len() != 1 {
                return Err(PyException::type_error("__reduce__ requires self"));
            }
            let class = if let PyObjectPayload::Instance(inst) = &args[0].payload {
                inst.class.clone()
            } else {
                return Err(PyException::type_error("__reduce__ requires an instance"));
            };
            let items = ordered_dict_items(&args[0])?
                .into_iter()
                .map(|(k, v)| PyObject::list(vec![k, v]))
                .collect();
            let state_pairs = ordered_dict_instance_attrs(&args[0])?;
            let state = if state_pairs.is_empty() {
                PyObject::none()
            } else {
                let mut map = FxHashKeyMap::default();
                for (k, v) in state_pairs {
                    map.insert(HashableKey::str_key(k), v);
                }
                PyObject::dict(map)
            };
            Ok(PyObject::tuple(vec![
                class,
                PyObject::tuple(vec![PyObject::list(items)]),
                state,
            ]))
        }),
    );
    ns.insert(
        CompactString::from("fromkeys"),
        PyObject::wrap(PyObjectPayload::ClassMethod(PyObject::native_closure(
            "OrderedDict.fromkeys",
            move |args| {
                if args.len() < 2 || args.len() > 3 {
                    return Err(PyException::type_error(
                        "fromkeys() requires iterable and optional value",
                    ));
                }
                let class = args[0].clone();
                let iterable = &args[1];
                let value = if let Some(third) = args.get(2) {
                    if let PyObjectPayload::Dict(map) = &third.payload {
                        if map
                            .read()
                            .get(&HashableKey::str_key(CompactString::from(
                                "__ordered_dict_kwargs__",
                            )))
                            .map(|v| v.is_truthy())
                            .unwrap_or(false)
                        {
                            map.read()
                                .get(&HashableKey::str_key(CompactString::from("value")))
                                .cloned()
                                .unwrap_or_else(PyObject::none)
                        } else {
                            third.clone()
                        }
                    } else {
                        third.clone()
                    }
                } else {
                    PyObject::none()
                };
                let result = PyObject::instance(class);
                let storage = ordered_dict_storage(&result)?;
                let mut w = storage.write();
                w.insert(ordered_dict_marker_key(), PyObject::bool_val(true));
                for item in iterable.to_list()? {
                    w.insert(item.to_hashable_key()?, value.clone());
                }
                Ok(result)
            },
        ))),
    );
    PyObject::class(
        CompactString::from("OrderedDict"),
        vec![PyObject::builtin_type(CompactString::from("dict"))],
        ns,
    )
}
