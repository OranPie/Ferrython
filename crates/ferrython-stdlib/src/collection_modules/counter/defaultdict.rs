use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::helpers::call_callable;
use ferrython_core::object::{
    FxBuildHasher, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

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

pub(in crate::collection_modules) fn make_defaultdict_class() -> PyObjectRef {
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
