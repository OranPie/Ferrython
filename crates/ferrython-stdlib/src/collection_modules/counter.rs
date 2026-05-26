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

fn mapping_entries_from_obj(obj: &PyObjectRef) -> PyResult<Vec<(HashableKey, PyObjectRef)>> {
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
            let items = obj.to_list()?;
            let mut out = Vec::new();
            for item in items {
                let pair = item.to_list()?;
                if pair.len() == 2 {
                    out.push((pair[0].to_hashable_key()?, pair[1].clone()));
                }
            }
            Ok(out)
        }
        _ => {
            let items = obj.to_list()?;
            let mut out = Vec::new();
            for item in items {
                let pair = item.to_list()?;
                if pair.len() == 2 {
                    out.push((pair[0].to_hashable_key()?, pair[1].clone()));
                }
            }
            Ok(out)
        }
    }
}

fn defaultdict_storage(
    obj: &PyObjectRef,
) -> PyResult<Rc<PyCell<IndexMap<HashableKey, PyObjectRef, FxBuildHasher>>>> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        if let Some(storage) = inst.dict_storage.as_ref() {
            return Ok(storage.clone());
        }
    }
    Err(PyException::type_error(
        "defaultdict method requires an instance",
    ))
}

fn defaultdict_factory(obj: &PyObjectRef) -> PyObjectRef {
    obj.get_attr("default_factory")
        .unwrap_or_else(PyObject::none)
}

fn set_defaultdict_factory(obj: &PyObjectRef, factory: PyObjectRef) -> PyResult<()> {
    let storage = defaultdict_storage(obj)?;
    if matches!(&factory.payload, PyObjectPayload::None) {
        storage
            .write()
            .shift_remove(&HashableKey::str_key(CompactString::from(
                "__defaultdict_factory__",
            )));
    } else {
        storage.write().insert(
            HashableKey::str_key(CompactString::from("__defaultdict_factory__")),
            factory.clone(),
        );
    }
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        inst.attrs
            .write()
            .insert(CompactString::from("default_factory"), factory);
    }
    Ok(())
}

fn defaultdict_repr_string(obj: &PyObjectRef) -> PyResult<String> {
    let storage = defaultdict_storage(obj)?;
    let factory = defaultdict_factory(obj);
    let factory_repr = factory.repr();
    let dict_repr = {
        let read = storage.read();
        let mut parts = Vec::new();
        for (k, v) in read.iter() {
            if ferrython_core::object::is_hidden_dict_key(k) {
                continue;
            }
            parts.push(format!("{}: {}", k.to_object().repr(), v.repr()));
        }
        format!("{{{}}}", parts.join(", "))
    };
    let name = if let PyObjectPayload::Instance(inst) = &obj.payload {
        if let PyObjectPayload::Class(cd) = &inst.class.payload {
            cd.name.as_str().to_string()
        } else {
            "defaultdict".to_string()
        }
    } else {
        "defaultdict".to_string()
    };
    Ok(format!("{}({}, {})", name, factory_repr, dict_repr))
}

fn deepcopy_defaultdict_value(value: &PyObjectRef) -> PyObjectRef {
    match &value.payload {
        PyObjectPayload::List(items) => PyObject::list(
            items
                .read()
                .iter()
                .map(deepcopy_defaultdict_value)
                .collect(),
        ),
        PyObjectPayload::Tuple(items) => {
            PyObject::tuple(items.iter().map(deepcopy_defaultdict_value).collect())
        }
        PyObjectPayload::Dict(map) => {
            let mut copied = IndexMap::new();
            for (k, v) in map.read().iter() {
                copied.insert(k.clone(), deepcopy_defaultdict_value(v));
            }
            PyObject::dict(copied)
        }
        PyObjectPayload::Instance(inst) if inst.dict_storage.is_some() => {
            let result = PyObject::instance(inst.class.clone());
            if let (Some(src), PyObjectPayload::Instance(dst_inst)) =
                (inst.dict_storage.as_ref(), &result.payload)
            {
                if let Some(dst) = dst_inst.dict_storage.as_ref() {
                    for (k, v) in src.read().iter() {
                        dst.write().insert(k.clone(), deepcopy_defaultdict_value(v));
                    }
                }
            }
            result
        }
        _ => value.clone(),
    }
}

fn defaultdict_init_from_args(
    self_obj: &PyObjectRef,
    args: &[PyObjectRef],
    kwargs: IndexMap<CompactString, PyObjectRef>,
) -> PyResult<()> {
    let factory = args.first().cloned().unwrap_or_else(PyObject::none);
    if !matches!(&factory.payload, PyObjectPayload::None) && !factory.is_callable() {
        return Err(PyException::type_error(
            "first argument must be callable or None",
        ));
    }
    set_defaultdict_factory(self_obj, factory)?;
    let storage = defaultdict_storage(self_obj)?;
    if let Some(source) = args.get(1) {
        for (k, v) in mapping_entries_from_obj(source)? {
            storage.write().insert(k, v);
        }
    }
    for (k, v) in kwargs {
        storage.write().insert(HashableKey::str_key(k), v);
    }
    Ok(())
}

fn extract_trailing_kwargs(
    args: &[PyObjectRef],
) -> (Vec<PyObjectRef>, IndexMap<CompactString, PyObjectRef>) {
    let mut pos_args = args.to_vec();
    let mut kwargs = IndexMap::new();
    let marker = HashableKey::str_key(CompactString::from("__defaultdict_kwargs__"));
    if let Some(last) = pos_args.last() {
        if let PyObjectPayload::Dict(map) = &last.payload {
            let read = map.read();
            if !read.get(&marker).map(|v| v.is_truthy()).unwrap_or(false) {
                return (pos_args, kwargs);
            }
            for (k, v) in read.iter() {
                if *k == marker {
                    continue;
                }
                if let HashableKey::Str(name) = k {
                    kwargs.insert(name.to_compact_string(), v.clone());
                }
            }
            drop(read);
            pos_args.pop();
        }
    }
    (pos_args, kwargs)
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

pub(super) fn make_defaultdict_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__module__"),
        PyObject::str_val(CompactString::from("collections")),
    );
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("defaultdict.__init__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("__init__ requires self"));
            }
            let (pos_args, kwargs) = extract_trailing_kwargs(&args[1..]);
            if pos_args.len() > 2 {
                return Err(PyException::type_error(
                    "defaultdict expected at most 2 arguments",
                ));
            }
            defaultdict_init_from_args(&args[0], &pos_args, kwargs)?;
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__missing__"),
        PyObject::native_closure("defaultdict.__missing__", move |args: &[PyObjectRef]| {
            if args.len() != 2 {
                return Err(PyException::type_error("__missing__ requires self and key"));
            }
            let factory = defaultdict_factory(&args[0]);
            if matches!(&factory.payload, PyObjectPayload::None) {
                return Err(PyException::key_error_value(args[1].clone()));
            }
            let value = call_callable(&factory, &[])?;
            let storage = defaultdict_storage(&args[0])?;
            storage
                .write()
                .insert(args[1].to_hashable_key()?, value.clone());
            Ok(value)
        }),
    );
    ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("defaultdict.__repr__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("__repr__ requires self"));
            }
            let ptr = PyObjectRef::as_ptr(&args[0]) as usize;
            if !ferrython_core::object::repr_enter(ptr) {
                let name = if let PyObjectPayload::Instance(inst) = &args[0].payload {
                    if let PyObjectPayload::Class(cd) = &inst.class.payload {
                        cd.name.as_str().to_string()
                    } else {
                        "defaultdict".to_string()
                    }
                } else {
                    "defaultdict".to_string()
                };
                return Ok(PyObject::str_val(CompactString::from(format!(
                    "{}(..., {{}})",
                    name
                ))));
            }
            let result = defaultdict_repr_string(&args[0]);
            ferrython_core::object::repr_leave(ptr);
            Ok(PyObject::str_val(CompactString::from(result?)))
        }),
    );
    ns.insert(
        CompactString::from("copy"),
        PyObject::native_closure("defaultdict.copy", move |args: &[PyObjectRef]| {
            if args.len() != 1 {
                return Err(PyException::type_error("copy() takes no arguments"));
            }
            let class = if let PyObjectPayload::Instance(inst) = &args[0].payload {
                inst.class.clone()
            } else {
                return Err(PyException::type_error("copy requires a defaultdict"));
            };
            let result = PyObject::instance(class);
            set_defaultdict_factory(&result, defaultdict_factory(&args[0]))?;
            let src = defaultdict_storage(&args[0])?;
            let dst = defaultdict_storage(&result)?;
            for (k, v) in src.read().iter() {
                dst.write().insert(k.clone(), v.clone());
            }
            Ok(result)
        }),
    );
    ns.insert(
        CompactString::from("__copy__"),
        PyObject::native_closure("defaultdict.__copy__", move |args: &[PyObjectRef]| {
            let copy = args
                .get(0)
                .and_then(|obj| obj.get_attr("copy"))
                .ok_or_else(|| PyException::type_error("__copy__ requires self"))?;
            call_callable(&copy, &[])
        }),
    );
    ns.insert(
        CompactString::from("__deepcopy__"),
        PyObject::native_closure("defaultdict.__deepcopy__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("__deepcopy__ requires self"));
            }
            let class = if let PyObjectPayload::Instance(inst) = &args[0].payload {
                inst.class.clone()
            } else {
                return Err(PyException::type_error(
                    "__deepcopy__ requires a defaultdict",
                ));
            };
            let result = PyObject::instance(class);
            set_defaultdict_factory(&result, defaultdict_factory(&args[0]))?;
            let src = defaultdict_storage(&args[0])?;
            let dst = defaultdict_storage(&result)?;
            for (k, v) in src.read().iter() {
                let copied = if ferrython_core::object::is_hidden_dict_key(k) {
                    v.clone()
                } else {
                    deepcopy_defaultdict_value(v)
                };
                dst.write().insert(k.clone(), copied);
            }
            Ok(result)
        }),
    );
    ns.insert(
        CompactString::from("__reduce__"),
        PyObject::native_closure("defaultdict.__reduce__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("__reduce__ requires self"));
            }
            let class = if let PyObjectPayload::Instance(inst) = &args[0].payload {
                inst.class.clone()
            } else {
                return Err(PyException::type_error("__reduce__ requires a defaultdict"));
            };
            let factory = defaultdict_factory(&args[0]);
            let ctor_args = if matches!(&factory.payload, PyObjectPayload::None) {
                PyObject::tuple(vec![])
            } else {
                PyObject::tuple(vec![factory])
            };
            let mut map = IndexMap::new();
            for (k, v) in defaultdict_storage(&args[0])?.read().iter() {
                if !ferrython_core::object::is_hidden_dict_key(k) {
                    map.insert(k.clone(), v.clone());
                }
            }
            Ok(PyObject::tuple(vec![
                class,
                ctor_args,
                PyObject::none(),
                PyObject::none(),
                PyObject::dict(map),
            ]))
        }),
    );
    ns.insert(
        CompactString::from("__reduce_ex__"),
        ns.get("__reduce__").cloned().unwrap(),
    );

    PyObject::class(
        CompactString::from("defaultdict"),
        vec![PyObject::builtin_type(CompactString::from("dict"))],
        ns,
    )
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

/// Standalone most_common(counter_dict, n?) — also available as Counter.most_common()
pub(super) fn collections_most_common(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "most_common() requires a Counter argument",
        ));
    }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let r = map.read();
        let mut pairs: Vec<(HashableKey, i64)> = r.iter()
            .filter(|(k, _)| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__"))
            .map(|(k, v)| (k.clone(), v.as_int().unwrap_or(0)))
            .collect();
        pairs.sort_by(|a, b| b.1.cmp(&a.1));
        let n = if args.len() > 1 {
            args[1].as_int().unwrap_or(pairs.len() as i64) as usize
        } else {
            pairs.len()
        };
        let result: Vec<PyObjectRef> = pairs
            .into_iter()
            .take(n)
            .map(|(k, v)| PyObject::tuple(vec![k.to_object(), PyObject::int(v)]))
            .collect();
        Ok(PyObject::list(result))
    } else {
        Err(PyException::type_error(
            "most_common() argument must be a Counter",
        ))
    }
}

fn is_counter_internal_key(k: &HashableKey) -> bool {
    matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__")
}

/// counter_elements(counter) -> list of elements repeated by their counts
pub(super) fn counter_elements(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error(
            "counter_elements requires a Counter",
        ));
    }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let r = map.read();
        let mut result = Vec::new();
        for (k, v) in r.iter() {
            if is_counter_internal_key(k) {
                continue;
            }
            let count = v.as_int().unwrap_or(0);
            for _ in 0..count {
                result.push(k.to_object());
            }
        }
        Ok(PyObject::list(result))
    } else {
        Err(PyException::type_error(
            "counter_elements requires a Counter",
        ))
    }
}

/// counter_update(counter, iterable_or_dict) -> None (mutates counter in-place)
pub(super) fn counter_update(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "counter_update requires counter and data",
        ));
    }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let mut w = map.write();
        if let PyObjectPayload::Dict(other) = &args[1].payload {
            let r = other.read();
            for (k, v) in r.iter() {
                if is_counter_internal_key(k) {
                    continue;
                }
                let existing = w.get(k).and_then(|x| x.as_int()).unwrap_or(0);
                let add = v.as_int().unwrap_or(0);
                w.insert(k.clone(), PyObject::int(existing + add));
            }
        } else {
            let items = args[1].to_list()?;
            for item in &items {
                let key = item.to_hashable_key()?;
                let existing = w.get(&key).and_then(|x| x.as_int()).unwrap_or(0);
                w.insert(key, PyObject::int(existing + 1));
            }
        }
        Ok(PyObject::none())
    } else {
        Err(PyException::type_error(
            "counter_update requires a Counter as first argument",
        ))
    }
}

/// counter_subtract(counter, iterable_or_dict) -> None (mutates counter)
pub(super) fn counter_subtract(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "counter_subtract requires counter and data",
        ));
    }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let mut w = map.write();
        if let PyObjectPayload::Dict(other) = &args[1].payload {
            let r = other.read();
            for (k, v) in r.iter() {
                if is_counter_internal_key(k) {
                    continue;
                }
                let existing = w.get(k).and_then(|x| x.as_int()).unwrap_or(0);
                let sub = v.as_int().unwrap_or(0);
                w.insert(k.clone(), PyObject::int(existing - sub));
            }
        } else {
            let items = args[1].to_list()?;
            for item in &items {
                let key = item.to_hashable_key()?;
                let existing = w.get(&key).and_then(|x| x.as_int()).unwrap_or(0);
                w.insert(key, PyObject::int(existing - 1));
            }
        }
        Ok(PyObject::none())
    } else {
        Err(PyException::type_error(
            "counter_subtract requires a Counter",
        ))
    }
}

/// counter_total(counter) -> int (sum of all counts)
pub(super) fn counter_total(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("counter_total requires a Counter"));
    }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let r = map.read();
        let total: i64 = r
            .iter()
            .filter(|(k, _)| !is_counter_internal_key(k))
            .map(|(_, v)| v.as_int().unwrap_or(0))
            .sum();
        Ok(PyObject::int(total))
    } else {
        Err(PyException::type_error("counter_total requires a Counter"))
    }
}

pub(super) fn counter_copy(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("counter_copy requires a Counter"));
    }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let r = map.read();
        Ok(PyObject::dict(r.clone()))
    } else {
        Err(PyException::type_error("counter_copy requires a Counter"))
    }
}

pub(super) fn counter_clear(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("counter_clear requires a Counter"));
    }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let mut w = map.write();
        let factory = w
            .get(&HashableKey::str_key(CompactString::from(
                "__defaultdict_factory__",
            )))
            .cloned();
        let marker = w
            .get(&HashableKey::str_key(CompactString::from("__counter__")))
            .cloned();
        w.clear();
        if let Some(f) = factory {
            w.insert(
                HashableKey::str_key(CompactString::from("__defaultdict_factory__")),
                f,
            );
        }
        if let Some(m) = marker {
            w.insert(HashableKey::str_key(CompactString::from("__counter__")), m);
        }
    }
    Ok(PyObject::none())
}

/// _count_elements(mapping, iterable) — C accelerator for Counter.__init__
pub(super) fn count_elements(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "_count_elements requires 2 arguments",
        ));
    }
    let mapping = &args[0];
    let iterable = &args[1];
    let items = iterable.to_list()?;
    for item in items {
        let key_str = item.py_to_string();
        let key = HashableKey::str_key(CompactString::from(key_str.as_str()));
        if let PyObjectPayload::Dict(map) = &mapping.payload {
            let current = {
                let r = map.read();
                r.get(&key).cloned()
            };
            let new_val = match current {
                Some(v) => {
                    let n = v.to_int().unwrap_or(0) + 1;
                    PyObject::int(n)
                }
                None => PyObject::int(1),
            };
            map.write().insert(key, new_val);
        }
    }
    Ok(PyObject::none())
}
