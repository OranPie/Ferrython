//! `collections.defaultdict` and `collections.Counter` implementation.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::helpers::call_callable;
use ferrython_core::object::{
    FxBuildHasher, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

mod defaultdict;
mod standalone;

pub(super) use defaultdict::make_defaultdict_class;
pub(super) use standalone::{
    collections_most_common, count_elements, counter_clear, counter_copy, counter_elements,
    counter_subtract, counter_total, counter_update,
};

fn counter_internal_marker_key() -> HashableKey {
    HashableKey::str_key(CompactString::from("__counter_kwargs__"))
}

fn counter_instance_storage(
    obj: &PyObjectRef,
) -> Option<Rc<PyCell<IndexMap<HashableKey, PyObjectRef, FxBuildHasher>>>> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        inst.dict_storage.clone()
    } else {
        None
    }
}

fn counter_extract_kwargs(
    args: &[PyObjectRef],
) -> PyResult<(
    PyObjectRef,
    Vec<PyObjectRef>,
    IndexMap<CompactString, PyObjectRef>,
)> {
    if args.is_empty() {
        return Err(PyException::type_error("Counter method requires self"));
    }
    let self_obj = args[0].clone();
    let mut pos_args = Vec::new();
    let mut kwds = IndexMap::new();

    if args.len() >= 2 {
        let last_is_marker_kwargs =
            if let PyObjectPayload::Dict(map) = &args[args.len() - 1].payload {
                let r = map.read();
                r.get(&counter_internal_marker_key())
                    .map(|v| v.is_truthy())
                    .unwrap_or(false)
            } else {
                false
            };

        if last_is_marker_kwargs {
            if args.len() > 3 {
                return Err(PyException::type_error(
                    "Counter methods accept at most one positional argument",
                ));
            }
            if args.len() == 3 {
                pos_args.push(args[1].clone());
            }
            if let PyObjectPayload::Dict(map) = &args[args.len() - 1].payload {
                for (k, v) in map.read().iter() {
                    if *k == counter_internal_marker_key() {
                        continue;
                    }
                    if let HashableKey::Str(name) = k {
                        kwds.insert(name.to_compact_string(), v.clone());
                    }
                }
            }
            return Ok((self_obj, pos_args, kwds));
        }

        if args.len() > 2 {
            return Err(PyException::type_error(
                "Counter methods accept at most one positional argument",
            ));
        }
        pos_args.push(args[1].clone());
    }

    Ok((self_obj, pos_args, kwds))
}

fn counter_count_from_source(
    self_obj: &PyObjectRef,
    source: &PyObjectRef,
    subtract: bool,
) -> PyResult<()> {
    let get_method = self_obj.get_attr("get");
    let set_method = self_obj.get_attr("__setitem__");
    let storage = counter_instance_storage(self_obj);

    let apply_value = |key: PyObjectRef, delta: Option<PyObjectRef>| -> PyResult<()> {
        let current = if let Some(ref method) = get_method {
            call_callable(method, &[key.clone(), PyObject::int(0)])?
        } else if let Some(ref ds) = storage {
            ds.read()
                .get(&key.to_hashable_key()?)
                .cloned()
                .unwrap_or_else(|| PyObject::int(0))
        } else {
            PyObject::int(0)
        };
        if matches!(
            &delta.as_ref().map(|d| &d.payload),
            Some(PyObjectPayload::None)
        ) {
            if let Some(ref method) = set_method {
                return call_callable(method, &[key, PyObject::none()]).map(|_| ());
            }
            if let Some(ref ds) = storage {
                ds.write().insert(key.to_hashable_key()?, PyObject::none());
                return Ok(());
            }
            return Ok(());
        }
        let step = delta.and_then(|d| d.to_int().ok()).unwrap_or(0);
        let current_n = current.to_int().unwrap_or(0);
        let next = if subtract {
            PyObject::int(current_n - step)
        } else {
            PyObject::int(current_n + step)
        };
        if let Some(ref method) = set_method {
            call_callable(method, &[key, next]).map(|_| ())
        } else if let Some(ref ds) = storage {
            ds.write().insert(key.to_hashable_key()?, next);
            Ok(())
        } else {
            Ok(())
        }
    };

    match &source.payload {
        PyObjectPayload::Dict(map) => {
            for (k, v) in map.read().iter() {
                if let HashableKey::Str(s) = k {
                    if s.as_str() == "__counter_kwargs__" {
                        continue;
                    }
                }
                apply_value(k.to_object(), Some(v.clone()))?;
            }
        }
        PyObjectPayload::Instance(inst) if inst.dict_storage.is_some() => {
            if let Some(storage) = inst.dict_storage.as_ref() {
                for (k, v) in storage.read().iter() {
                    apply_value(k.to_object(), Some(v.clone()))?;
                }
            }
        }
        _ => {
            let items = source.to_list()?;
            for item in items {
                apply_value(item, Some(PyObject::int(1)))?;
            }
        }
    }
    Ok(())
}

fn counter_apply_kwds(
    self_obj: &PyObjectRef,
    kwds: IndexMap<CompactString, PyObjectRef>,
    subtract: bool,
) -> PyResult<()> {
    let get_method = self_obj.get_attr("get");
    let set_method = self_obj.get_attr("__setitem__");
    let storage = counter_instance_storage(self_obj);
    for (k, v) in kwds {
        let key_obj = PyObject::str_val(k.clone());
        let current = if let Some(ref method) = get_method {
            call_callable(method, &[key_obj.clone(), PyObject::int(0)])?
        } else if let Some(ref ds) = storage {
            ds.read()
                .get(&key_obj.to_hashable_key()?)
                .cloned()
                .unwrap_or_else(|| PyObject::int(0))
        } else {
            PyObject::int(0)
        };
        if matches!(&v.payload, PyObjectPayload::None) {
            if let Some(ref method) = set_method {
                call_callable(method, &[key_obj, PyObject::none()])?;
            } else if let Some(ref ds) = storage {
                ds.write()
                    .insert(key_obj.to_hashable_key()?, PyObject::none());
            }
            continue;
        }
        let step = v.to_int().unwrap_or(0);
        let current_n = current.to_int().unwrap_or(0);
        let next = if subtract {
            PyObject::int(current_n - step)
        } else {
            PyObject::int(current_n + step)
        };
        if let Some(ref method) = set_method {
            call_callable(method, &[key_obj, next])?;
        } else if let Some(ref ds) = storage {
            ds.write().insert(key_obj.to_hashable_key()?, next);
        }
    }
    Ok(())
}

fn counter_clone_like(
    self_obj: &PyObjectRef,
    filter: impl Fn(&HashableKey, &PyObjectRef) -> bool,
) -> PyResult<PyObjectRef> {
    let class = if let PyObjectPayload::Instance(inst) = &self_obj.payload {
        inst.class.clone()
    } else {
        return Err(PyException::type_error(
            "Counter operation requires an instance",
        ));
    };
    let result = PyObject::instance(class);
    if let Some(dst) = counter_instance_storage(&result) {
        let mut w = dst.write();
        if let Some(src) = counter_instance_storage(self_obj) {
            for (k, v) in src.read().iter() {
                if matches!(k, HashableKey::Str(s) if s.as_str() == "__counter_kwargs__") {
                    continue;
                }
                if filter(&k, v) {
                    w.insert(k.clone(), v.clone());
                }
            }
        }
    }
    Ok(result)
}

fn counter_most_common_items(obj: &PyObjectRef) -> Vec<(HashableKey, PyObjectRef)> {
    let mut pairs = Vec::new();
    if let Some(storage) = counter_instance_storage(obj) {
        for (k, v) in storage.read().iter() {
            if matches!(k, HashableKey::Str(s) if s.as_str() == "__counter_kwargs__") {
                continue;
            }
            pairs.push((k.clone(), v.clone()));
        }
    }
    pairs
}

fn collections_counter(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let int_factory = PyObject::builtin_type(CompactString::from("int"));
    let factory_key = HashableKey::str_key(CompactString::from("__defaultdict_factory__"));
    let counter_marker = HashableKey::str_key(CompactString::from("__counter__"));

    if args.is_empty() {
        let mut map = IndexMap::new();
        map.insert(factory_key, int_factory);
        map.insert(counter_marker, PyObject::bool_val(true));
        return Ok(PyObject::dict(map));
    }
    // Handle dict input: Counter({"red": 4, "blue": 2})
    if let PyObjectPayload::Dict(m) = &args[0].payload {
        let src = m.read();
        let mut map = IndexMap::new();
        map.insert(factory_key, int_factory);
        map.insert(counter_marker, PyObject::bool_val(true));
        for (k, v) in src.iter() {
            if !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__")
            {
                map.insert(k.clone(), v.clone());
            }
        }
        return Ok(PyObject::dict(map));
    }
    let items = args[0].to_list()?;
    let mut counts: IndexMap<HashableKey, i64> = IndexMap::new();
    for item in &items {
        let key = item.to_hashable_key()?;
        *counts.entry(key).or_insert(0) += 1;
    }
    let mut map = IndexMap::new();
    map.insert(factory_key, int_factory);
    map.insert(counter_marker, PyObject::bool_val(true));
    for (k, v) in counts {
        map.insert(k.clone(), PyObject::int(v));
    }
    Ok(PyObject::dict(map))
}

pub(super) fn make_counter_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__missing__"),
        PyObject::native_closure("Counter.__missing__", move |_args| Ok(PyObject::int(0))),
    );

    ns.insert(
        CompactString::from("__contains__"),
        PyObject::native_closure("Counter.__contains__", move |args: &[PyObjectRef]| {
            let (self_obj, pos_args, _) = counter_extract_kwargs(args)?;
            if pos_args.len() != 1 {
                return Err(PyException::type_error(
                    "__contains__() takes exactly one key argument",
                ));
            }
            if let Some(storage) = counter_instance_storage(&self_obj) {
                let hk = pos_args[0].to_hashable_key()?;
                return Ok(PyObject::bool_val(storage.read().contains_key(&hk)));
            }
            Ok(PyObject::bool_val(false))
        }),
    );

    ns.insert(
        CompactString::from("__delitem__"),
        PyObject::native_closure("Counter.__delitem__", move |args: &[PyObjectRef]| {
            let (self_obj, pos_args, _) = counter_extract_kwargs(args)?;
            if pos_args.len() != 1 {
                return Err(PyException::type_error(
                    "__delitem__() takes exactly one key argument",
                ));
            }
            if let Some(storage) = counter_instance_storage(&self_obj) {
                let hk = pos_args[0].to_hashable_key()?;
                storage.write().shift_remove(&hk);
            }
            Ok(PyObject::none())
        }),
    );

    ns.insert(
        CompactString::from("__hash__"),
        PyObject::native_closure("Counter.__hash__", move |_args: &[PyObjectRef]| {
            Err(PyException::type_error("unhashable type: 'Counter'"))
        }),
    );

    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("Counter.__init__", move |args: &[PyObjectRef]| {
            let (self_obj, pos_args, kwds) = counter_extract_kwargs(args)?;
            if let Some(src) = pos_args.get(0) {
                counter_count_from_source(&self_obj, src, false)?;
            }
            if !kwds.is_empty() {
                counter_apply_kwds(&self_obj, kwds, false)?;
            }
            Ok(PyObject::none())
        }),
    );

    ns.insert(
        CompactString::from("update"),
        PyObject::native_closure("Counter.update", move |args: &[PyObjectRef]| {
            let (self_obj, pos_args, kwds) = counter_extract_kwargs(args)?;
            if let Some(src) = pos_args.get(0) {
                counter_count_from_source(&self_obj, src, false)?;
            }
            if !kwds.is_empty() {
                counter_apply_kwds(&self_obj, kwds, false)?;
            }
            Ok(PyObject::none())
        }),
    );

    ns.insert(
        CompactString::from("subtract"),
        PyObject::native_closure("Counter.subtract", move |args: &[PyObjectRef]| {
            let (self_obj, pos_args, kwds) = counter_extract_kwargs(args)?;
            if let Some(src) = pos_args.get(0) {
                counter_count_from_source(&self_obj, src, true)?;
            }
            if !kwds.is_empty() {
                counter_apply_kwds(&self_obj, kwds, true)?;
            }
            Ok(PyObject::none())
        }),
    );

    ns.insert(
        CompactString::from("most_common"),
        PyObject::native_closure("Counter.most_common", move |args: &[PyObjectRef]| {
            let (self_obj, pos_args, _) = counter_extract_kwargs(args)?;
            if pos_args.len() > 1 {
                return Err(PyException::type_error(
                    "most_common() takes at most 1 positional argument",
                ));
            }
            let mut pairs = counter_most_common_items(&self_obj);
            pairs.sort_by(|a, b| {
                let a_n = a.1.to_int().unwrap_or(0);
                let b_n = b.1.to_int().unwrap_or(0);
                b_n.cmp(&a_n)
            });
            let limit = if let Some(n) = pos_args.get(0) {
                Some(n.to_int().unwrap_or(0).max(0) as usize)
            } else {
                None
            };
            let items = pairs
                .into_iter()
                .map(|(k, v)| PyObject::tuple(vec![k.to_object(), v]))
                .collect::<Vec<_>>();
            Ok(PyObject::list(match limit {
                Some(n) => items.into_iter().take(n).collect(),
                None => items,
            }))
        }),
    );

    ns.insert(
        CompactString::from("elements"),
        PyObject::native_closure("Counter.elements", move |args: &[PyObjectRef]| {
            let (self_obj, pos_args, _) = counter_extract_kwargs(args)?;
            if !pos_args.is_empty() {
                return Err(PyException::type_error(
                    "elements() takes no positional arguments",
                ));
            }
            let mut items = Vec::new();
            for (k, v) in counter_most_common_items(&self_obj) {
                let count = v.to_int().unwrap_or(0);
                for _ in 0..count.max(0) {
                    items.push(k.to_object());
                }
            }
            Ok(PyObject::list(items))
        }),
    );

    ns.insert(
        CompactString::from("total"),
        PyObject::native_closure("Counter.total", move |args: &[PyObjectRef]| {
            let (self_obj, pos_args, _) = counter_extract_kwargs(args)?;
            if !pos_args.is_empty() {
                return Err(PyException::type_error(
                    "total() takes no positional arguments",
                ));
            }
            let total: i64 = counter_most_common_items(&self_obj)
                .into_iter()
                .map(|(_, v)| v.to_int().unwrap_or(0))
                .sum();
            Ok(PyObject::int(total))
        }),
    );

    ns.insert(
        CompactString::from("copy"),
        PyObject::native_closure("Counter.copy", move |args: &[PyObjectRef]| {
            let (self_obj, pos_args, _) = counter_extract_kwargs(args)?;
            if !pos_args.is_empty() {
                return Err(PyException::type_error(
                    "copy() takes no positional arguments",
                ));
            }
            let class = if let PyObjectPayload::Instance(inst) = &self_obj.payload {
                inst.class.clone()
            } else {
                return Err(PyException::type_error("Counter.copy requires an instance"));
            };
            let result = PyObject::instance(class);
            if let Some(dst) = counter_instance_storage(&result) {
                let mut w = dst.write();
                for (k, v) in counter_most_common_items(&self_obj) {
                    w.insert(k, v);
                }
            }
            Ok(result)
        }),
    );

    ns.insert(
        CompactString::from("__deepcopy__"),
        PyObject::native_closure("Counter.__deepcopy__", move |args: &[PyObjectRef]| {
            let (self_obj, pos_args, _) = counter_extract_kwargs(args)?;
            if pos_args.len() > 1 {
                return Err(PyException::type_error(
                    "__deepcopy__ takes at most one argument",
                ));
            }
            let class = if let PyObjectPayload::Instance(inst) = &self_obj.payload {
                inst.class.clone()
            } else {
                return Err(PyException::type_error(
                    "Counter.__deepcopy__ requires an instance",
                ));
            };
            let result = PyObject::instance(class);
            if let Some(dst) = counter_instance_storage(&result) {
                let mut w = dst.write();
                for (k, v) in counter_most_common_items(&self_obj) {
                    w.insert(k, v);
                }
            }
            Ok(result)
        }),
    );

    ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("Counter.__repr__", move |args: &[PyObjectRef]| {
            let (self_obj, pos_args, _) = counter_extract_kwargs(args)?;
            if !pos_args.is_empty() {
                return Err(PyException::type_error(
                    "__repr__ takes no positional arguments",
                ));
            }
            let mut pairs = counter_most_common_items(&self_obj);
            pairs.sort_by(|a, b| {
                let a_n = a.1.to_int().unwrap_or(0);
                let b_n = b.1.to_int().unwrap_or(0);
                b_n.cmp(&a_n)
            });
            if pairs.is_empty() {
                return Ok(PyObject::str_val(CompactString::from("Counter()")));
            }
            let items = pairs
                .into_iter()
                .map(|(k, v)| format!("{}: {}", k.to_object().repr(), v.repr()))
                .collect::<Vec<_>>()
                .join(", ");
            Ok(PyObject::str_val(CompactString::from(format!(
                "Counter({{{}}})",
                items
            ))))
        }),
    );

    let binary_op =
        |name: &'static str, combine: fn(i64, i64) -> Option<i64>, in_place: bool| -> PyObjectRef {
            PyObject::native_closure(name, move |args: &[PyObjectRef]| {
                let (self_obj, pos_args, _) = counter_extract_kwargs(args)?;
                if pos_args.len() != 1 {
                    return Err(PyException::type_error(format!(
                        "{} requires one Counter argument",
                        name
                    )));
                }
                let other = &pos_args[0];
                let class = if let PyObjectPayload::Instance(inst) = &self_obj.payload {
                    inst.class.clone()
                } else {
                    return Err(PyException::type_error(
                        "Counter operation requires an instance",
                    ));
                };
                let left_items = counter_most_common_items(&self_obj);
                let mut right_items: IndexMap<HashableKey, i64> = IndexMap::new();
                for (k, v) in counter_most_common_items(other) {
                    right_items.insert(k, v.to_int().unwrap_or(0));
                }

                let mut build_result =
                    |target: &Rc<PyCell<IndexMap<HashableKey, PyObjectRef, FxBuildHasher>>>| {
                        let mut w = target.write();
                        for (k, v) in left_items.iter() {
                            let a = v.to_int().unwrap_or(0);
                            let b = right_items.shift_remove(k).unwrap_or(0);
                            if let Some(next) = combine(a, b) {
                                w.insert(k.clone(), PyObject::int(next));
                            }
                        }
                        for (k, b) in right_items.iter() {
                            if let Some(next) = combine(0, *b) {
                                w.insert(k.clone(), PyObject::int(next));
                            }
                        }
                    };

                if !in_place {
                    let result = PyObject::instance(class.clone());
                    if let Some(dst) = counter_instance_storage(&result) {
                        build_result(&dst);
                    }
                    return Ok(result);
                }

                let dst = counter_instance_storage(&self_obj).ok_or_else(|| {
                    PyException::type_error("Counter operation requires a Counter")
                })?;
                {
                    let mut w = dst.write();
                    w.clear();
                }
                build_result(&dst);
                Ok(self_obj.clone())
            })
        };

    ns.insert(
        CompactString::from("__add__"),
        binary_op(
            "__add__",
            |a, b| {
                let n = a + b;
                if n > 0 {
                    Some(n)
                } else {
                    None
                }
            },
            false,
        ),
    );
    ns.insert(
        CompactString::from("__sub__"),
        binary_op(
            "__sub__",
            |a, b| {
                let n = a - b;
                if n > 0 {
                    Some(n)
                } else {
                    None
                }
            },
            false,
        ),
    );
    ns.insert(
        CompactString::from("__or__"),
        binary_op(
            "__or__",
            |a, b| {
                let n = a.max(b);
                if n > 0 {
                    Some(n)
                } else {
                    None
                }
            },
            false,
        ),
    );
    ns.insert(
        CompactString::from("__and__"),
        binary_op(
            "__and__",
            |a, b| {
                let n = a.min(b);
                if n > 0 {
                    Some(n)
                } else {
                    None
                }
            },
            false,
        ),
    );
    ns.insert(
        CompactString::from("__iadd__"),
        binary_op(
            "__iadd__",
            |a, b| {
                let n = a + b;
                if n > 0 {
                    Some(n)
                } else {
                    None
                }
            },
            true,
        ),
    );
    ns.insert(
        CompactString::from("__isub__"),
        binary_op(
            "__isub__",
            |a, b| {
                let n = a - b;
                if n > 0 {
                    Some(n)
                } else {
                    None
                }
            },
            true,
        ),
    );
    ns.insert(
        CompactString::from("__ior__"),
        binary_op(
            "__ior__",
            |a, b| {
                let n = a.max(b);
                if n > 0 {
                    Some(n)
                } else {
                    None
                }
            },
            true,
        ),
    );
    ns.insert(
        CompactString::from("__iand__"),
        binary_op(
            "__iand__",
            |a, b| {
                let n = a.min(b);
                if n > 0 {
                    Some(n)
                } else {
                    None
                }
            },
            true,
        ),
    );

    ns.insert(
        CompactString::from("__pos__"),
        PyObject::native_closure("Counter.__pos__", move |args: &[PyObjectRef]| {
            let (self_obj, pos_args, _) = counter_extract_kwargs(args)?;
            if !pos_args.is_empty() {
                return Err(PyException::type_error(
                    "__pos__ takes no positional arguments",
                ));
            }
            let class = if let PyObjectPayload::Instance(inst) = &self_obj.payload {
                inst.class.clone()
            } else {
                return Err(PyException::type_error(
                    "Counter operation requires an instance",
                ));
            };
            let result = PyObject::instance(class);
            if let Some(dst) = counter_instance_storage(&result) {
                let mut w = dst.write();
                for (k, v) in counter_most_common_items(&self_obj) {
                    let n = v.to_int().unwrap_or(0);
                    if n > 0 {
                        w.insert(k, PyObject::int(n));
                    }
                }
            }
            Ok(result)
        }),
    );

    ns.insert(
        CompactString::from("__neg__"),
        PyObject::native_closure("Counter.__neg__", move |args: &[PyObjectRef]| {
            let (self_obj, pos_args, _) = counter_extract_kwargs(args)?;
            if !pos_args.is_empty() {
                return Err(PyException::type_error(
                    "__neg__ takes no positional arguments",
                ));
            }
            let class = if let PyObjectPayload::Instance(inst) = &self_obj.payload {
                inst.class.clone()
            } else {
                return Err(PyException::type_error(
                    "Counter operation requires an instance",
                ));
            };
            let result = PyObject::instance(class);
            if let Some(dst) = counter_instance_storage(&result) {
                let mut w = dst.write();
                for (k, v) in counter_most_common_items(&self_obj) {
                    let n = v.to_int().unwrap_or(0);
                    if n < 0 {
                        w.insert(k, PyObject::int(-n));
                    }
                }
            }
            Ok(result)
        }),
    );

    ns.insert(
        CompactString::from("__getstate__"),
        PyObject::native_closure("Counter.__getstate__", move |args: &[PyObjectRef]| {
            let (self_obj, pos_args, _) = counter_extract_kwargs(args)?;
            if !pos_args.is_empty() {
                return Err(PyException::type_error(
                    "__getstate__ takes no positional arguments",
                ));
            }
            let mut map = IndexMap::new();
            for (k, v) in counter_most_common_items(&self_obj) {
                map.insert(k, v);
            }
            Ok(PyObject::dict(map))
        }),
    );

    ns.insert(
        CompactString::from("fromkeys"),
        PyObject::native_function("Counter.fromkeys", |_args| {
            Err(PyException::not_implemented_error(
                "Counter.fromkeys() is undefined",
            ))
        }),
    );

    PyObject::class(
        CompactString::from("Counter"),
        vec![PyObject::builtin_type(CompactString::from("dict"))],
        ns,
    )
}
