use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::helpers::call_callable;
use ferrython_core::object::{
    make_builtin, make_module, new_fx_hashkey_map, BuiltinFn, CompareOp, FxBuildHasher, PyCell,
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, SharedFxAttrMap,
};
use ferrython_core::types::{hash_key_like_python, HashableKey};
use indexmap::IndexMap;
use std::cell::RefCell;
use std::rc::Rc;

fn is_python_keyword(name: &str) -> bool {
    matches!(
        name,
        "False"
            | "None"
            | "True"
            | "and"
            | "as"
            | "assert"
            | "async"
            | "await"
            | "break"
            | "class"
            | "continue"
            | "def"
            | "del"
            | "elif"
            | "else"
            | "except"
            | "finally"
            | "for"
            | "from"
            | "global"
            | "if"
            | "import"
            | "in"
            | "is"
            | "lambda"
            | "nonlocal"
            | "not"
            | "or"
            | "pass"
            | "raise"
            | "return"
            | "try"
            | "while"
            | "with"
            | "yield"
    )
}

fn is_valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first.is_alphabetic()) {
        return false;
    }
    chars.all(|c| c == '_' || c.is_alphanumeric())
}

fn native_method(class_name: &str, method_name: &str, f: BuiltinFn) -> PyObjectRef {
    PyObject::native_function(&format!("{class_name}.{method_name}"), f)
}

fn normalize_namedtuple_field_names(
    field_names: Vec<CompactString>,
    rename: bool,
) -> PyResult<Vec<CompactString>> {
    if !rename {
        let mut seen = std::collections::HashSet::new();
        for name in &field_names {
            let name_str = name.as_str();
            if name_str.starts_with('_') {
                return Err(PyException::value_error(format!(
                    "Field names cannot start with an underscore: {:?}",
                    name_str
                )));
            }
            if !is_valid_identifier(name_str) || is_python_keyword(name_str) {
                return Err(PyException::value_error(format!(
                    "Field names must be valid identifiers: {:?}",
                    name_str
                )));
            }
            if !seen.insert(name_str.to_string()) {
                return Err(PyException::value_error(format!(
                    "Encountered duplicate field name: {:?}",
                    name_str
                )));
            }
        }
        return Ok(field_names);
    }

    let mut seen = std::collections::HashSet::new();
    let mut renamed = Vec::with_capacity(field_names.len());
    for (index, name) in field_names.into_iter().enumerate() {
        let name_str = name.as_str();
        let invalid = !is_valid_identifier(name_str)
            || is_python_keyword(name_str)
            || name_str.starts_with('_')
            || seen.contains(name_str);
        if invalid {
            renamed.push(CompactString::from(format!("_{}", index)));
        } else {
            renamed.push(name.clone());
        }
        seen.insert(renamed[index].to_string());
    }
    Ok(renamed)
}

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

fn chainmap_make_instance(
    class_obj: PyObjectRef,
    maps: Vec<PyObjectRef>,
    set_parents: bool,
) -> PyResult<PyObjectRef> {
    let maps = if maps.is_empty() {
        vec![PyObject::dict(IndexMap::new())]
    } else {
        maps
    };
    let inst = PyObject::instance(class_obj.clone());
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(
            CompactString::from("__chainmap__"),
            PyObject::bool_val(true),
        );
        w.insert(CompactString::from("maps"), PyObject::list(maps.clone()));
        if set_parents {
            let parent_maps = if maps.len() > 1 {
                maps[1..].to_vec()
            } else {
                Vec::new()
            };
            let parents = chainmap_make_instance(class_obj, parent_maps, false)?;
            w.insert(CompactString::from("parents"), parents);
        }
    }
    Ok(inst)
}

fn chainmap_init_instance(inst: &PyObjectRef, maps: Vec<PyObjectRef>) -> PyResult<()> {
    let class_obj = if let PyObjectPayload::Instance(d) = &inst.payload {
        d.class.clone()
    } else {
        return Err(PyException::type_error("ChainMap expects an instance"));
    };
    let maps = if maps.is_empty() {
        vec![PyObject::dict(IndexMap::new())]
    } else {
        maps
    };
    if let PyObjectPayload::Instance(d) = &inst.payload {
        let mut w = d.attrs.write();
        w.insert(
            CompactString::from("__chainmap__"),
            PyObject::bool_val(true),
        );
        w.insert(CompactString::from("maps"), PyObject::list(maps.clone()));
        let parent_maps = if maps.len() > 1 {
            maps[1..].to_vec()
        } else {
            Vec::new()
        };
        let parents = chainmap_make_instance(class_obj, parent_maps, false)?;
        w.insert(CompactString::from("parents"), parents);
    }
    Ok(())
}

thread_local! {
    static NAMEDTUPLE_FIELD_DOCS: RefCell<Vec<PyObjectRef>> = RefCell::new(Vec::new());
    static NAMEDTUPLE_FIELD_CLASS: RefCell<Option<PyObjectRef>> = RefCell::new(None);
}

fn namedtuple_field_doc(index: usize) -> PyObjectRef {
    NAMEDTUPLE_FIELD_DOCS.with(|cache| {
        let mut cache = cache.borrow_mut();
        while cache.len() <= index {
            let next = cache.len();
            cache.push(PyObject::str_val(CompactString::from(format!(
                "Alias for field number {}",
                next
            ))));
        }
        cache[index].clone()
    })
}

fn namedtuple_field_class() -> PyObjectRef {
    NAMEDTUPLE_FIELD_CLASS.with(|cell| {
        if let Some(cls) = cell.borrow().as_ref() {
            return cls.clone();
        }
        let mut ns = IndexMap::new();
        ns.insert(
            CompactString::from("__get__"),
            PyObject::native_closure("namedtuple_field.__get__", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error(
                        "__get__ requires a descriptor object",
                    ));
                }
                let mut self_obj: Option<&PyObjectRef> = None;
                let mut obj: Option<&PyObjectRef> = None;
                for arg in args {
                    if self_obj.is_none() && arg.get_attr("__tuple_index__").is_some() {
                        self_obj = Some(arg);
                    } else if obj.is_none() {
                        obj = Some(arg);
                    }
                }
                let self_obj = self_obj.unwrap_or(&args[0]);
                let obj = if let Some(obj) = obj {
                    obj
                } else {
                    return Ok(self_obj.clone());
                };
                if matches!(&obj.payload, PyObjectPayload::None) {
                    return Ok(self_obj.clone());
                }
                let idx = self_obj
                    .get_attr("__tuple_index__")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0);
                if let Some(tup) = obj.get_attr("_tuple") {
                    if let PyObjectPayload::Tuple(items) = &tup.payload {
                        return Ok(items
                            .get(idx as usize)
                            .cloned()
                            .unwrap_or_else(PyObject::none));
                    }
                }
                if let Some(bv) = obj.get_attr("__builtin_value__") {
                    if let PyObjectPayload::Tuple(items) = &bv.payload {
                        return Ok(items
                            .get(idx as usize)
                            .cloned()
                            .unwrap_or_else(PyObject::none));
                    }
                }
                if let PyObjectPayload::Tuple(items) = &obj.payload {
                    return Ok(items
                        .get(idx as usize)
                        .cloned()
                        .unwrap_or_else(PyObject::none));
                }
                Err(PyException::attribute_error(
                    "namedtuple field descriptor has no backing tuple",
                ))
            }),
        );
        ns.insert(
            CompactString::from("__set__"),
            PyObject::native_closure("namedtuple_field.__set__", move |_args| {
                Err(PyException::attribute_error("can't set attribute"))
            }),
        );
        ns.insert(
            CompactString::from("__delete__"),
            PyObject::native_closure("namedtuple_field.__delete__", move |_args| {
                Err(PyException::attribute_error("can't delete attribute"))
            }),
        );
        ns.insert(
            CompactString::from("__reduce__"),
            PyObject::native_closure("namedtuple_field.__reduce__", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("__reduce__ requires self"));
                }
                let self_obj = &args[0];
                let idx = self_obj
                    .get_attr("__tuple_index__")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0);
                let field_name = self_obj
                    .get_attr("__field_name__")
                    .map(|v| v.py_to_string())
                    .unwrap_or_default();
                let doc = self_obj
                    .get_attr("__doc__")
                    .map(|v| v.py_to_string())
                    .unwrap_or_default();
                Ok(PyObject::tuple(vec![
                    PyObject::native_function(
                        "_namedtuple_field_rebuild",
                        namedtuple_rebuild_field,
                    ),
                    PyObject::tuple(vec![
                        PyObject::int(idx),
                        PyObject::str_val(CompactString::from(field_name)),
                        PyObject::str_val(CompactString::from(doc)),
                    ]),
                ]))
            }),
        );
        ns.insert(
            CompactString::from("__reduce_ex__"),
            PyObject::native_closure("namedtuple_field.__reduce_ex__", move |args| {
                if args.is_empty() {
                    return Err(PyException::type_error("__reduce_ex__ requires self"));
                }
                let self_obj = &args[0];
                let idx = self_obj
                    .get_attr("__tuple_index__")
                    .and_then(|v| v.as_int())
                    .unwrap_or(0);
                let field_name = self_obj
                    .get_attr("__field_name__")
                    .map(|v| v.py_to_string())
                    .unwrap_or_default();
                let doc = self_obj
                    .get_attr("__doc__")
                    .map(|v| v.py_to_string())
                    .unwrap_or_default();
                Ok(PyObject::tuple(vec![
                    PyObject::native_function(
                        "_namedtuple_field_rebuild",
                        namedtuple_rebuild_field,
                    ),
                    PyObject::tuple(vec![
                        PyObject::int(idx),
                        PyObject::str_val(CompactString::from(field_name)),
                        PyObject::str_val(CompactString::from(doc)),
                    ]),
                ]))
            }),
        );
        ns.insert(
            CompactString::from("__module__"),
            PyObject::str_val(CompactString::from("collections")),
        );
        let cls = PyObject::class(
            CompactString::from("namedtuple_field"),
            vec![PyObject::builtin_type(CompactString::from("object"))],
            ns,
        );
        *cell.borrow_mut() = Some(cls.clone());
        cls
    })
}

fn make_namedtuple_new_placeholder(defaults: Option<&[PyObjectRef]>) -> PyObjectRef {
    let mut ns = IndexMap::new();
    let defaults_obj = defaults
        .map(|vals| PyObject::tuple(vals.to_vec()))
        .unwrap_or_else(PyObject::none);
    ns.insert(CompactString::from("__defaults__"), defaults_obj);
    let cls = PyObject::class(
        CompactString::from("namedtuple_new"),
        vec![PyObject::builtin_type(CompactString::from("object"))],
        ns,
    );
    PyObject::instance(cls)
}

pub(crate) fn namedtuple_rebuild_field(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() != 3 {
        return Err(PyException::type_error(
            "namedtuple field rebuild requires 3 arguments",
        ));
    }
    let idx = args[0].to_int()? as usize;
    let field_name = args[1].py_to_string();
    let doc = args[2].py_to_string();
    let mut attrs = IndexMap::new();
    attrs.insert(
        CompactString::from("__tuple_index__"),
        PyObject::int(idx as i64),
    );
    attrs.insert(
        CompactString::from("__field_name__"),
        PyObject::str_val(CompactString::from(field_name)),
    );
    attrs.insert(
        CompactString::from("__doc__"),
        PyObject::str_val(CompactString::from(doc)),
    );
    Ok(PyObject::instance_with_attrs(
        namedtuple_field_class(),
        attrs,
    ))
}

pub(crate) fn namedtuple_rebuild_class(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() != 4 {
        return Err(PyException::type_error(
            "namedtuple class rebuild requires 4 arguments",
        ));
    }
    let typename = args[0].py_to_string();
    let field_names = args[1].to_list()?;
    let defaults = if matches!(&args[2].payload, PyObjectPayload::None) {
        None
    } else {
        Some(args[2].to_list()?)
    };
    let module = if matches!(&args[3].payload, PyObjectPayload::None) {
        None
    } else {
        Some(args[3].py_to_string())
    };

    let mut call_args = vec![PyObject::str_val(CompactString::from(typename))];
    call_args.push(PyObject::tuple(field_names));
    if defaults.is_some() || module.is_some() {
        let mut kw = IndexMap::new();
        if let Some(vals) = defaults {
            kw.insert(
                HashableKey::str_key(CompactString::from("defaults")),
                PyObject::tuple(vals),
            );
        }
        if let Some(module) = module {
            kw.insert(
                HashableKey::str_key(CompactString::from("module")),
                PyObject::str_val(CompactString::from(module)),
            );
        }
        call_args.push(PyObject::dict(kw));
    }
    collections_namedtuple(&call_args)
}

pub(crate) fn namedtuple_rebuild_instance(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() != 5 {
        return Err(PyException::type_error(
            "namedtuple instance rebuild requires 5 arguments",
        ));
    }
    let cls = namedtuple_rebuild_class(&args[..4])?;
    let values = args[4].to_list()?;
    let inst = PyObject::instance(cls);
    if let PyObjectPayload::Instance(ref data) = inst.payload {
        data.attrs
            .write()
            .insert(CompactString::from("_tuple"), PyObject::tuple(values));
    }
    Ok(inst)
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
            (
                "OrderedDict",
                PyObject::native_function("collections.OrderedDict", collections_ordered_dict),
            ),
            (
                "defaultdict",
                PyObject::native_function("collections.defaultdict", collections_defaultdict),
            ),
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

fn collections_ordered_dict(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // OrderedDict — same as dict but __eq__ compares order when both are OrderedDict
    let marker_key = HashableKey::str_key(CompactString::from("__ordered_dict__"));
    let mut pairs = Vec::new();
    if !args.is_empty() {
        let items = args[0].to_list()?;
        for item in items {
            if let PyObjectPayload::Tuple(t) = &item.payload {
                if t.len() == 2 {
                    pairs.push((t[0].clone(), t[1].clone()));
                }
            }
        }
    }
    let mut map = IndexMap::new();
    map.insert(marker_key, PyObject::bool_val(true));
    for (k, v) in pairs {
        let hk = k.to_hashable_key()?;
        map.insert(hk, v);
    }
    let od = PyObject::dict(map);

    // Install move_to_end method
    if let PyObjectPayload::Dict(dict_arc) = &od.payload {
        let d = dict_arc.clone();
        let move_fn = PyObject::native_closure("OrderedDict.move_to_end", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "move_to_end() requires a key argument",
                ));
            }
            let key = args[0].to_hashable_key()?;
            let last = if args.len() > 1 {
                args[1].is_truthy()
            } else {
                true
            };
            let mut map = d.write();
            if let Some(idx) = map.get_index_of(&key) {
                let target = if last { map.len() - 1 } else { 0 };
                map.move_index(idx, target);
                Ok(PyObject::none())
            } else {
                Err(PyException::key_error(format!("{:?}", args[0])))
            }
        });
        // Store move_to_end as a hidden dict key since dicts don't have instance attrs
        dict_arc.write().insert(
            HashableKey::str_key(CompactString::from("__move_to_end_fn__")),
            move_fn,
        );
    }

    Ok(od)
}

fn collections_defaultdict(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let factory = if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::None) {
        Some(args[0].clone())
    } else {
        None
    };

    let mut map = IndexMap::new();
    // Store factory as a special key
    if let Some(f) = factory {
        map.insert(
            HashableKey::str_key(CompactString::from("__defaultdict_factory__")),
            f,
        );
    }

    // If initial data provided
    if args.len() >= 2 {
        if let PyObjectPayload::Dict(src) = &args[1].payload {
            for (k, v) in src.read().iter() {
                map.insert(k.clone(), v.clone());
            }
        }
    }

    Ok(PyObject::dict(map))
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

fn make_counter_class() -> PyObjectRef {
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
fn collections_most_common(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
fn counter_elements(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
fn counter_update(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
fn counter_subtract(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
fn counter_total(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn counter_copy(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn counter_clear(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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

fn collections_namedtuple(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error(
            "namedtuple requires typename and field_names",
        ));
    }
    if args.len() > 3 {
        return Err(PyException::type_error(
            "namedtuple accepts at most 2 positional arguments",
        ));
    }
    let typename = args[0].py_to_string();
    if !is_valid_identifier(&typename) || is_python_keyword(&typename) {
        return Err(PyException::value_error(format!(
            "Type names must be valid identifiers: {:?}",
            typename
        )));
    }

    // Check for kwargs dict as last arg
    let kwargs_dict = if args.len() == 3 {
        if let PyObjectPayload::Dict(map) = &args[2].payload {
            Some(map.read().clone())
        } else {
            return Err(PyException::type_error(
                "namedtuple keyword arguments must be provided as a trailing dict",
            ));
        }
    } else {
        None
    };

    let rename = kwargs_dict
        .as_ref()
        .and_then(|kw| kw.get(&HashableKey::str_key(CompactString::from("rename"))))
        .map(|v| v.is_truthy())
        .unwrap_or(false);

    // Parse field names
    let raw_field_names: Vec<CompactString> = match &args[1].payload {
        PyObjectPayload::Str(s) => s
            .replace(',', " ")
            .split_whitespace()
            .map(|s| CompactString::from(s))
            .collect(),
        PyObjectPayload::List(_) => args[1]
            .to_list()?
            .iter()
            .map(|i| CompactString::from(i.py_to_string()))
            .collect(),
        PyObjectPayload::Tuple(items) => items
            .iter()
            .map(|i| CompactString::from(i.py_to_string()))
            .collect(),
        _ => args[1]
            .to_list()?
            .iter()
            .map(|i| CompactString::from(i.py_to_string()))
            .collect(),
    };

    let field_names = normalize_namedtuple_field_names(raw_field_names, rename)?;

    // Parse defaults from kwargs
    let defaults_obj = kwargs_dict
        .as_ref()
        .and_then(|kw| kw.get(&HashableKey::str_key(CompactString::from("defaults"))))
        .cloned();
    let defaults: Vec<PyObjectRef> = match defaults_obj {
        None => Vec::new(),
        Some(d) => {
            if matches!(&d.payload, PyObjectPayload::None) {
                Vec::new()
            } else {
                d.to_list()?
            }
        }
    };

    if defaults.len() > field_names.len() {
        return Err(PyException::type_error("Too many default values"));
    }

    let module_attr = kwargs_dict
        .as_ref()
        .and_then(|kw| kw.get(&HashableKey::str_key(CompactString::from("module"))))
        .cloned()
        .and_then(|module_obj| {
            if matches!(&module_obj.payload, PyObjectPayload::None) {
                None
            } else {
                Some(module_obj)
            }
        });
    let module_attr = module_attr.or_else(|| {
        crate::sys_modules::get_current_frame()
            .and_then(|frame| {
                frame
                    .get_attr("f_globals")
                    .and_then(|globals| globals.get_attr("__name__"))
            })
            .or_else(|| {
                crate::get_current_globals()
                    .and_then(|globals| globals.read().get("__name__").cloned())
            })
    });

    // Create a class with namespace containing field info
    let mut namespace = IndexMap::new();
    let fields_tuple = PyObject::tuple(
        field_names
            .iter()
            .map(|n| PyObject::str_val(n.clone()))
            .collect(),
    );
    namespace.insert(CompactString::from("_fields"), fields_tuple);
    namespace.insert(
        CompactString::from("__namedtuple__"),
        PyObject::bool_val(true),
    );
    namespace.insert(
        CompactString::from("__slots__"),
        PyObject::tuple(Vec::new()),
    );
    namespace.insert(
        CompactString::from("__doc__"),
        PyObject::str_val(CompactString::from(format!(
            "{}({})",
            typename,
            field_names
                .iter()
                .map(|n| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    );
    if let Some(module_attr) = module_attr {
        namespace.insert(CompactString::from("__module__"), module_attr);
    }

    // Build _field_defaults from defaults (right-aligned to fields)
    let mut defaults_map = IndexMap::new();
    if !defaults.is_empty() {
        let offset = field_names.len().saturating_sub(defaults.len());
        for (i, val) in defaults.iter().enumerate() {
            if let Some(name) = field_names.get(offset + i) {
                defaults_map.insert(HashableKey::str_key(name.clone()), val.clone());
            }
        }
    }
    namespace.insert(
        CompactString::from("_field_defaults"),
        PyObject::dict(defaults_map.clone()),
    );
    if let Some(tuple_getitem) =
        PyObject::builtin_type(CompactString::from("tuple")).get_attr("__getitem__")
    {
        namespace.insert(CompactString::from("__getitem__"), tuple_getitem);
    }

    // Store field indices
    for (i, name) in field_names.iter().enumerate() {
        namespace.insert(
            CompactString::from(format!("_field_idx_{}", name)),
            PyObject::int(i as i64),
        );
    }

    // Per-field descriptors are shared across namedtuple classes so pickling
    // and docstring identity behave consistently.
    let descriptor_class = namedtuple_field_class();

    for (i, name) in field_names.iter().enumerate() {
        let mut attrs = IndexMap::new();
        attrs.insert(
            CompactString::from("__tuple_index__"),
            PyObject::int(i as i64),
        );
        attrs.insert(
            CompactString::from("__field_name__"),
            PyObject::str_val(name.clone()),
        );
        attrs.insert(CompactString::from("__doc__"), namedtuple_field_doc(i));
        namespace.insert(
            name.clone(),
            PyObject::instance_with_attrs(descriptor_class.clone(), attrs),
        );
    }

    namespace.insert(
        CompactString::from("__new__"),
        make_namedtuple_new_placeholder(if defaults.is_empty() {
            None
        } else {
            Some(defaults.as_slice())
        }),
    );

    let cls = PyObject::class(
        CompactString::from(typename.as_str()),
        vec![PyObject::builtin_type(CompactString::from("tuple"))],
        namespace,
    );

    // _make classmethod: create instance from iterable (needs cls reference)
    let cls_ref = cls.clone();
    let field_names_clone = field_names.clone();
    let make_fn = PyObject::native_closure(
        "namedtuple._make",
        move |args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "_make() requires an iterable argument",
                ));
            }
            let items = args[0].to_list()?;
            if items.len() != field_names_clone.len() {
                return Err(PyException::type_error(format!(
                    "_make() takes {} arguments but {} were given",
                    field_names_clone.len(),
                    items.len()
                )));
            }
            let inst = PyObject::instance(cls_ref.clone());
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                attrs.insert(CompactString::from("_tuple"), PyObject::tuple(items));
            }
            Ok(inst)
        },
    );
    if let PyObjectPayload::Class(ref cd) = cls.payload {
        cd.namespace
            .write()
            .insert(CompactString::from("_make"), make_fn);
    }

    // _asdict: return OrderedDict of field_name → value
    let field_names_ad = field_names.clone();
    let asdict_fn = PyObject::native_closure(
        "namedtuple._asdict",
        move |args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
            if args.is_empty() {
                return Err(PyException::type_error("_asdict requires self"));
            }
            let self_obj = &args[0];
            let mut dict = IndexMap::new();
            if let Some(tup) = self_obj.get_attr("_tuple") {
                if let PyObjectPayload::Tuple(items) = &tup.payload {
                    for (name, val) in field_names_ad.iter().zip(items.iter()) {
                        dict.insert(HashableKey::str_key(name.clone()), val.clone());
                    }
                    return Ok(PyObject::dict(dict));
                }
            }
            for name in &field_names_ad {
                let val = self_obj
                    .get_attr(name.as_str())
                    .unwrap_or_else(PyObject::none);
                dict.insert(HashableKey::str_key(name.clone()), val);
            }
            Ok(PyObject::dict(dict))
        },
    );
    if let PyObjectPayload::Class(ref cd) = cls.payload {
        cd.namespace
            .write()
            .insert(CompactString::from("_asdict"), asdict_fn);
    }

    // _replace(**kwargs): return new namedtuple with specified fields replaced
    let cls_ref2 = cls.clone();
    let field_names_rep = field_names.clone();
    let replace_fn = PyObject::native_closure(
        "namedtuple._replace",
        move |args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
            if args.is_empty() {
                return Err(PyException::type_error("_replace requires self"));
            }
            let self_obj = &args[0];
            let kwargs = if args.len() > 1 {
                if let PyObjectPayload::Dict(ref map) = args[args.len() - 1].payload {
                    Some(map.read().clone())
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(ref kw) = kwargs {
                for (k, _) in kw.iter() {
                    if let HashableKey::Str(name) = k {
                        if !field_names_rep.iter().any(|f| f.as_str() == name.as_str()) {
                            return Err(PyException::value_error(format!(
                                "got an unexpected field name '{}'",
                                name
                            )));
                        }
                    }
                }
            }
            let inst = PyObject::instance(cls_ref2.clone());
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                let mut tuple_items = Vec::new();
                for name in &field_names_rep {
                    let val = if let Some(ref kw) = kwargs {
                        kw.get(&HashableKey::str_key(name.clone()))
                            .cloned()
                            .unwrap_or_else(|| {
                                self_obj
                                    .get_attr(name.as_str())
                                    .unwrap_or_else(PyObject::none)
                            })
                    } else {
                        self_obj
                            .get_attr(name.as_str())
                            .unwrap_or_else(PyObject::none)
                    };
                    tuple_items.push(val);
                }
                attrs.insert(CompactString::from("_tuple"), PyObject::tuple(tuple_items));
            }
            Ok(inst)
        },
    );
    if let PyObjectPayload::Class(ref cd) = cls.payload {
        cd.namespace
            .write()
            .insert(CompactString::from("_replace"), replace_fn);
    }

    let repr_field_names = field_names.clone();
    let repr_fn = PyObject::native_closure("namedtuple.__repr__", move |args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("__repr__ requires self"));
        }
        let self_obj = &args[0];
        let class_name = if let PyObjectPayload::Instance(inst) = &self_obj.payload {
            if let PyObjectPayload::Class(cd) = &inst.class.payload {
                cd.name.to_string()
            } else {
                typename.to_string()
            }
        } else {
            typename.to_string()
        };
        let mut parts = Vec::new();
        if let Some(tup) = self_obj.get_attr("_tuple") {
            if let PyObjectPayload::Tuple(items) = &tup.payload {
                for (name, val) in repr_field_names.iter().zip(items.iter()) {
                    parts.push(format!("{}={}", name, val.py_to_string()));
                }
                return Ok(PyObject::str_val(CompactString::from(format!(
                    "{}({})",
                    class_name,
                    parts.join(", ")
                ))));
            }
        }
        for name in &repr_field_names {
            let val = self_obj
                .get_attr(name.as_str())
                .unwrap_or_else(PyObject::none);
            parts.push(format!("{}={}", name, val.py_to_string()));
        }
        Ok(PyObject::str_val(CompactString::from(format!(
            "{}({})",
            class_name,
            parts.join(", ")
        ))))
    });
    let str_fn = repr_fn.clone();
    let hash_fn = PyObject::native_closure("namedtuple.__hash__", move |args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("__hash__ requires self"));
        }
        if let Some(tup) = args[0].get_attr("_tuple") {
            if let PyObjectPayload::Tuple(items) = &tup.payload {
                let mut h: u64 = 0x345678;
                let mult: u64 = 1_000_003;
                for item in items.iter() {
                    let item_hash = hash_key_like_python(&item.to_hashable_key()?);
                    h = h.wrapping_mul(mult) ^ item_hash as u64;
                }
                return Ok(PyObject::int(h as i64));
            }
        }
        Ok(PyObject::int(0))
    });
    let getnewargs_fn = PyObject::native_closure(
        "namedtuple.__getnewargs__",
        move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("__getnewargs__ requires self"));
            }
            Ok(args[0]
                .get_attr("_tuple")
                .unwrap_or_else(|| PyObject::tuple(Vec::new())))
        },
    );
    let reduce_fn =
        PyObject::native_closure("namedtuple.__reduce__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("__reduce__ requires self"));
            }
            let self_obj = &args[0];
            let cls = if let PyObjectPayload::Instance(inst) = &self_obj.payload {
                inst.class.clone()
            } else {
                return Err(PyException::type_error(
                    "__reduce__ requires a namedtuple instance",
                ));
            };
            let field_names = cls
                .get_attr("_fields")
                .and_then(|v| {
                    if let PyObjectPayload::Tuple(items) = &v.payload {
                        Some(items.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let defaults = cls.get_attr("_field_defaults").and_then(|v| {
                if let PyObjectPayload::Dict(map) = &v.payload {
                    Some(map.read().values().cloned().collect::<Vec<_>>())
                } else {
                    None
                }
            });
            let module = cls.get_attr("__module__").map(|m| m.py_to_string());
            let typename = if let PyObjectPayload::Class(cd) = &cls.payload {
                cd.name.to_string()
            } else {
                cls.py_to_string()
            };
            let tuple_obj = self_obj
                .get_attr("_tuple")
                .unwrap_or_else(|| PyObject::tuple(Vec::new()));
            let mut args_vec = vec![
                PyObject::str_val(CompactString::from(typename)),
                PyObject::tuple((*field_names).clone()),
                defaults.map(PyObject::tuple).unwrap_or_else(PyObject::none),
                module
                    .map(|m| PyObject::str_val(CompactString::from(m)))
                    .unwrap_or_else(PyObject::none),
                tuple_obj,
            ];
            Ok(PyObject::tuple(vec![
                PyObject::native_function(
                    "_namedtuple_rebuild_instance",
                    namedtuple_rebuild_instance,
                ),
                PyObject::tuple(args_vec.drain(..).collect()),
            ]))
        });
    let reduce_ex_fn =
        PyObject::native_closure("namedtuple.__reduce_ex__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("__reduce_ex__ requires self"));
            }
            let self_obj = &args[0];
            let cls = if let PyObjectPayload::Instance(inst) = &self_obj.payload {
                inst.class.clone()
            } else {
                return Err(PyException::type_error(
                    "__reduce_ex__ requires a namedtuple instance",
                ));
            };
            let field_names = cls
                .get_attr("_fields")
                .and_then(|v| {
                    if let PyObjectPayload::Tuple(items) = &v.payload {
                        Some(items.clone())
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let defaults = cls.get_attr("_field_defaults").and_then(|v| {
                if let PyObjectPayload::Dict(map) = &v.payload {
                    Some(map.read().values().cloned().collect::<Vec<_>>())
                } else {
                    None
                }
            });
            let module = cls.get_attr("__module__").map(|m| m.py_to_string());
            let typename = if let PyObjectPayload::Class(cd) = &cls.payload {
                cd.name.to_string()
            } else {
                cls.py_to_string()
            };
            let tuple_obj = self_obj
                .get_attr("_tuple")
                .unwrap_or_else(|| PyObject::tuple(Vec::new()));
            let mut args_vec = vec![
                PyObject::str_val(CompactString::from(typename)),
                PyObject::tuple((*field_names).clone()),
                defaults.map(PyObject::tuple).unwrap_or_else(PyObject::none),
                module
                    .map(|m| PyObject::str_val(CompactString::from(m)))
                    .unwrap_or_else(PyObject::none),
                tuple_obj,
            ];
            Ok(PyObject::tuple(vec![
                PyObject::native_function(
                    "_namedtuple_rebuild_instance",
                    namedtuple_rebuild_instance,
                ),
                PyObject::tuple(args_vec.drain(..).collect()),
            ]))
        });
    let len_fn = PyObject::native_closure("namedtuple.__len__", move |args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("__len__ requires self"));
        }
        if let Some(tup) = args[0].get_attr("_tuple") {
            if let PyObjectPayload::Tuple(items) = &tup.payload {
                return Ok(PyObject::int(items.len() as i64));
            }
        }
        Ok(PyObject::int(0))
    });

    if let PyObjectPayload::Class(ref cd) = cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(CompactString::from("__repr__"), repr_fn);
        ns.insert(CompactString::from("__str__"), str_fn);
        ns.insert(CompactString::from("__hash__"), hash_fn);
        ns.insert(CompactString::from("__getnewargs__"), getnewargs_fn);
        ns.insert(CompactString::from("__reduce__"), reduce_fn);
        ns.insert(CompactString::from("__reduce_ex__"), reduce_ex_fn);
        ns.insert(CompactString::from("__len__"), len_fn);
    }

    Ok(cls)
}

fn collections_deque(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Extract maxlen from last arg if it's a kwargs dict
    let has_trailing_kwargs =
        !args.is_empty() && matches!(&args[args.len() - 1].payload, PyObjectPayload::Dict(_));
    let kwargs_idx = if has_trailing_kwargs {
        args.len() - 1
    } else {
        args.len()
    };

    let items = if kwargs_idx == 0 || args.is_empty() {
        vec![]
    } else {
        args[0].to_list()?
    };

    // Extract maxlen from positional arg or trailing kwargs dict
    let maxlen = if has_trailing_kwargs {
        if let PyObjectPayload::Dict(map) = &args[args.len() - 1].payload {
            let map = map.read();
            map.get(&HashableKey::str_key(CompactString::from("maxlen")))
                .and_then(|v| {
                    if matches!(&v.payload, PyObjectPayload::None) {
                        None
                    } else {
                        Some(v.to_int().unwrap_or(0) as usize)
                    }
                })
        } else {
            None
        }
    } else if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
        Some(args[1].to_int()? as usize)
    } else {
        None
    };
    // Enforce maxlen on initial items
    let items = if let Some(ml) = maxlen {
        if items.len() > ml {
            items[items.len() - ml..].to_vec()
        } else {
            items
        }
    } else {
        items
    };

    let data = Rc::new(PyCell::new(items));

    // Build instance methods that share the data list
    let mut cls_ns = IndexMap::new();

    // append(x)
    let d = data.clone();
    let ml = maxlen;
    cls_ns.insert(
        CompactString::from("append"),
        PyObject::native_closure("deque.append", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("append requires argument"));
            }
            let mut w = d.write();
            w.push(args[0].clone());
            if let Some(m) = ml {
                while w.len() > m {
                    w.remove(0);
                }
            }
            Ok(PyObject::none())
        }),
    );

    // appendleft(x)
    let d = data.clone();
    let ml = maxlen;
    cls_ns.insert(
        CompactString::from("appendleft"),
        PyObject::native_closure("deque.appendleft", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("appendleft requires argument"));
            }
            let mut w = d.write();
            w.insert(0, args[0].clone());
            if let Some(m) = ml {
                while w.len() > m {
                    w.pop();
                }
            }
            Ok(PyObject::none())
        }),
    );

    // pop()
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("pop"),
        PyObject::native_closure("deque.pop", move |_: &[PyObjectRef]| {
            let mut w = d.write();
            w.pop()
                .ok_or_else(|| PyException::index_error("pop from an empty deque"))
        }),
    );

    // popleft()
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("popleft"),
        PyObject::native_closure("deque.popleft", move |_: &[PyObjectRef]| {
            let mut w = d.write();
            if w.is_empty() {
                return Err(PyException::index_error("pop from an empty deque"));
            }
            Ok(w.remove(0))
        }),
    );

    // extend(iterable)
    let d = data.clone();
    let ml = maxlen;
    cls_ns.insert(
        CompactString::from("extend"),
        PyObject::native_closure("deque.extend", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("extend requires argument"));
            }
            let items = args[0].to_list()?;
            let mut w = d.write();
            w.extend(items);
            if let Some(m) = ml {
                while w.len() > m {
                    w.remove(0);
                }
            }
            Ok(PyObject::none())
        }),
    );

    // extendleft(iterable)
    let d = data.clone();
    let ml = maxlen;
    cls_ns.insert(
        CompactString::from("extendleft"),
        PyObject::native_closure("deque.extendleft", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("extendleft requires argument"));
            }
            let items = args[0].to_list()?;
            let mut w = d.write();
            // CPython: appendleft each item in order — insert(0) naturally reverses
            for item in items.into_iter() {
                w.insert(0, item);
            }
            if let Some(m) = ml {
                while w.len() > m {
                    w.pop();
                }
            }
            Ok(PyObject::none())
        }),
    );

    // rotate(n=1)
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("rotate"),
        PyObject::native_closure("deque.rotate", move |args: &[PyObjectRef]| {
            let n = if args.is_empty() {
                1i64
            } else {
                args[0].to_int()?
            };
            let mut w = d.write();
            let len = w.len();
            if len == 0 {
                return Ok(PyObject::none());
            }
            let n = ((n % len as i64) + len as i64) as usize % len;
            if n > 0 {
                let split_point = len - n;
                let mut rotated = w[split_point..].to_vec();
                rotated.extend_from_slice(&w[..split_point]);
                *w = rotated;
            }
            Ok(PyObject::none())
        }),
    );

    // clear()
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("clear"),
        PyObject::native_closure("deque.clear", move |_: &[PyObjectRef]| {
            d.write().clear();
            Ok(PyObject::none())
        }),
    );

    // count(x)
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("count"),
        PyObject::native_closure("deque.count", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("count requires argument"));
            }
            let target = &args[0];
            let r = d.read();
            let c = r
                .iter()
                .filter(|item| {
                    item.compare(target, CompareOp::Eq)
                        .map(|v| v.is_truthy())
                        .unwrap_or(false)
                })
                .count();
            Ok(PyObject::int(c as i64))
        }),
    );

    // index(x)
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("index"),
        PyObject::native_closure("deque.index", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("index requires argument"));
            }
            let target = &args[0];
            let r = d.read();
            for (i, item) in r.iter().enumerate() {
                if item
                    .compare(target, CompareOp::Eq)
                    .map(|v| v.is_truthy())
                    .unwrap_or(false)
                {
                    return Ok(PyObject::int(i as i64));
                }
            }
            Err(PyException::value_error("value not in deque"))
        }),
    );

    // remove(x)
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("remove"),
        PyObject::native_closure("deque.remove", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("remove requires argument"));
            }
            let target = &args[0];
            let mut w = d.write();
            let pos = w.iter().position(|item| {
                item.compare(target, CompareOp::Eq)
                    .map(|v| v.is_truthy())
                    .unwrap_or(false)
            });
            match pos {
                Some(i) => {
                    w.remove(i);
                    Ok(PyObject::none())
                }
                None => Err(PyException::value_error("deque.remove(x): x not in deque")),
            }
        }),
    );

    // reverse()
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("reverse"),
        PyObject::native_closure("deque.reverse", move |_: &[PyObjectRef]| {
            d.write().reverse();
            Ok(PyObject::none())
        }),
    );

    // copy()
    let d = data.clone();
    let ml2 = maxlen;
    cls_ns.insert(
        CompactString::from("copy"),
        PyObject::native_closure("deque.copy", move |_: &[PyObjectRef]| {
            let items = d.read().clone();
            let mut new_args = vec![PyObject::list(items)];
            if let Some(m) = ml2 {
                new_args.push(PyObject::int(m as i64));
            }
            collections_deque(&new_args)
        }),
    );

    // __len__()
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("__len__"),
        PyObject::native_closure("deque.__len__", move |_: &[PyObjectRef]| {
            Ok(PyObject::int(d.read().len() as i64))
        }),
    );

    // __bool__()
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("__bool__"),
        PyObject::native_closure("deque.__bool__", move |_: &[PyObjectRef]| {
            Ok(PyObject::bool_val(!d.read().is_empty()))
        }),
    );

    // __repr__()
    let d = data.clone();
    let ml3 = maxlen;
    cls_ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("deque.__repr__", move |_: &[PyObjectRef]| {
            let r = d.read();
            let items_str: Vec<String> = r.iter().map(|i| i.py_to_string()).collect();
            let base = format!("deque([{}])", items_str.join(", "));
            if let Some(m) = ml3 {
                Ok(PyObject::str_val(CompactString::from(format!(
                    "deque([{}], maxlen={})",
                    items_str.join(", "),
                    m
                ))))
            } else {
                Ok(PyObject::str_val(CompactString::from(base)))
            }
        }),
    );

    // __iter__()
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("__iter__"),
        PyObject::native_closure("deque.__iter__", move |_: &[PyObjectRef]| {
            let snapshot = d.read().clone();
            Ok(PyObject::wrap(PyObjectPayload::Iterator(Rc::new(
                PyCell::new(ferrython_core::object::IteratorData::List {
                    items: snapshot,
                    index: 0,
                }),
            ))))
        }),
    );

    // __contains__(x) - needed for 'in' operator
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("__contains__"),
        PyObject::native_closure("deque.__contains__", move |args: &[PyObjectRef]| {
            // Called as unbound method: args = [self, value] or directly: args = [value]
            let target = if args.len() >= 2 {
                &args[1]
            } else if !args.is_empty() {
                &args[0]
            } else {
                return Ok(PyObject::bool_val(false));
            };
            let r = d.read();
            for item in r.iter() {
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

    // __getitem__(index)
    let d = data.clone();
    cls_ns.insert(
        CompactString::from("__getitem__"),
        PyObject::native_closure("deque.__getitem__", move |args: &[PyObjectRef]| {
            if args.is_empty() {
                return Err(PyException::type_error("__getitem__ requires index"));
            }
            // Called as unbound method: args = [self, index] or directly: args = [index]
            let idx_arg = if args.len() >= 2 { &args[1] } else { &args[0] };
            let idx = idx_arg.to_int()?;
            let r = d.read();
            let len = r.len() as i64;
            let actual = if idx < 0 { len + idx } else { idx };
            if actual < 0 || actual >= len {
                return Err(PyException::index_error("deque index out of range"));
            }
            Ok(r[actual as usize].clone())
        }),
    );

    let deque_cls = PyObject::class(CompactString::from("deque"), vec![], cls_ns);
    let inst = PyObject::instance(deque_cls);
    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("__deque__"), PyObject::bool_val(true));
        // Store a reference list for _data (closures share the backing Rc<PyCell> directly)
        attrs.insert(
            CompactString::from("_data"),
            PyObject::list(data.read().clone()),
        );
        attrs.insert(
            CompactString::from("__maxlen__"),
            match maxlen {
                Some(n) => PyObject::int(n as i64),
                None => PyObject::none(),
            },
        );
        // Also install instance methods directly on attrs for attribute access
        if let PyObjectPayload::Class(ref cd) = inst_data.class.payload {
            let ns = cd.namespace.read();
            for (name, val) in ns.iter() {
                if !name.starts_with("_data")
                    && !name.starts_with("__deque")
                    && !name.starts_with("__maxlen")
                {
                    attrs.insert(name.clone(), val.clone());
                }
            }
        }
    }
    Ok(inst)
}

fn make_chainmap_class() -> PyObjectRef {
    let mut ns = IndexMap::new();

    let maps_from_self = |self_obj: &PyObjectRef| -> PyResult<Vec<PyObjectRef>> {
        let maps = self_obj
            .get_attr("maps")
            .ok_or_else(|| PyException::type_error("ChainMap missing maps"))?;
        maps.to_list()
    };

    let build_builtin_value = |maps: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        let mut combined = IndexMap::new();
        for mapping in maps.iter().rev() {
            for key in mapping.to_list()? {
                let hk = key.to_hashable_key()?;
                let value = mapping.get_item(&key)?;
                combined.insert(hk, value);
            }
        }
        Ok(PyObject::dict(combined))
    };

    let lookup_in_maps =
        |maps: &[PyObjectRef], key: &PyObjectRef| -> PyResult<Option<PyObjectRef>> {
            for mapping in maps {
                match mapping.get_item(key) {
                    Ok(value) => return Ok(Some(value)),
                    Err(e) if e.kind == ExceptionKind::KeyError => continue,
                    Err(e) => return Err(e),
                }
            }
            Ok(None)
        };

    let unique_keys = |maps: &[PyObjectRef]| -> PyResult<Vec<PyObjectRef>> {
        let mut seen = IndexMap::<HashableKey, ()>::new();
        let mut keys = Vec::new();
        for mapping in maps.iter().rev() {
            let list = mapping.to_list()?;
            for key in list {
                let hk = HashableKey::from_object(&key)?;
                if seen.insert(hk, ()).is_none() {
                    keys.push(key);
                }
            }
        }
        Ok(keys)
    };

    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("ChainMap.__init__", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("ChainMap.__init__ requires self"));
            }
            let maps = if call_args.len() > 1 {
                call_args[1..].to_vec()
            } else {
                vec![PyObject::dict(IndexMap::new())]
            };
            chainmap_init_instance(&call_args[0], maps)?;
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__getitem__"),
        PyObject::native_closure("ChainMap.__getitem__", move |call_args| {
            if call_args.len() < 2 {
                return Err(PyException::type_error("__getitem__ requires key"));
            }
            let maps = maps_from_self(&call_args[0])?;
            if let Some(value) = lookup_in_maps(&maps, &call_args[1])? {
                return Ok(value);
            }
            if let Some(missing) = call_args[0].get_attr("__missing__") {
                return call_callable(&missing, &[call_args[1].clone()]);
            }
            Err(PyException::key_error(call_args[1].py_to_string()))
        }),
    );
    ns.insert(
        CompactString::from("__contains__"),
        PyObject::native_closure("ChainMap.__contains__", move |call_args| {
            if call_args.len() < 2 {
                return Err(PyException::type_error("__contains__ requires key"));
            }
            let maps = maps_from_self(&call_args[0])?;
            Ok(PyObject::bool_val(
                lookup_in_maps(&maps, &call_args[1])?.is_some(),
            ))
        }),
    );
    ns.insert(
        CompactString::from("__len__"),
        PyObject::native_closure("ChainMap.__len__", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("__len__ requires self"));
            }
            let maps = maps_from_self(&call_args[0])?;
            Ok(PyObject::int(unique_keys(&maps)?.len() as i64))
        }),
    );
    ns.insert(
        CompactString::from("__iter__"),
        PyObject::native_closure("ChainMap.__iter__", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("__iter__ requires self"));
            }
            let maps = maps_from_self(&call_args[0])?;
            let combined = build_builtin_value(&maps)?;
            if let PyObjectPayload::Dict(m) = &combined.payload {
                Ok(PyObject::wrap(PyObjectPayload::DictKeys(m.clone())))
            } else {
                Ok(PyObject::wrap(PyObjectPayload::DictKeys(Rc::new(
                    PyCell::new(new_fx_hashkey_map()),
                ))))
            }
        }),
    );
    ns.insert(
        CompactString::from("keys"),
        PyObject::native_closure("ChainMap.keys", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("keys requires self"));
            }
            let maps = maps_from_self(&call_args[0])?;
            let combined = build_builtin_value(&maps)?;
            if let PyObjectPayload::Dict(m) = &combined.payload {
                Ok(PyObject::wrap(PyObjectPayload::DictKeys(m.clone())))
            } else {
                Ok(PyObject::wrap(PyObjectPayload::DictKeys(Rc::new(
                    PyCell::new(new_fx_hashkey_map()),
                ))))
            }
        }),
    );
    ns.insert(
        CompactString::from("values"),
        PyObject::native_closure("ChainMap.values", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("values requires self"));
            }
            let maps = maps_from_self(&call_args[0])?;
            let combined = build_builtin_value(&maps)?;
            if let PyObjectPayload::Dict(m) = &combined.payload {
                Ok(PyObject::wrap(PyObjectPayload::DictValues(m.clone())))
            } else {
                Ok(PyObject::wrap(PyObjectPayload::DictValues(Rc::new(
                    PyCell::new(new_fx_hashkey_map()),
                ))))
            }
        }),
    );
    ns.insert(
        CompactString::from("items"),
        PyObject::native_closure("ChainMap.items", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("items requires self"));
            }
            let maps = maps_from_self(&call_args[0])?;
            let combined = build_builtin_value(&maps)?;
            if let PyObjectPayload::Dict(m) = &combined.payload {
                Ok(PyObject::wrap(PyObjectPayload::DictItems(m.clone())))
            } else {
                Ok(PyObject::wrap(PyObjectPayload::DictItems(Rc::new(
                    PyCell::new(new_fx_hashkey_map()),
                ))))
            }
        }),
    );
    ns.insert(
        CompactString::from("get"),
        PyObject::native_closure("ChainMap.get", move |call_args| {
            if call_args.len() < 2 {
                return Err(PyException::type_error("get requires key"));
            }
            let maps = maps_from_self(&call_args[0])?;
            let default = call_args.get(2).cloned().unwrap_or_else(PyObject::none);
            Ok(lookup_in_maps(&maps, &call_args[1])?.unwrap_or(default))
        }),
    );
    ns.insert(
        CompactString::from("__eq__"),
        PyObject::native_closure("ChainMap.__eq__", move |call_args| {
            if call_args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let self_maps = maps_from_self(&call_args[0])?;
            let self_value = build_builtin_value(&self_maps)?;
            let other_value = if let Ok(other_maps) = maps_from_self(&call_args[1]) {
                build_builtin_value(&other_maps)?
            } else {
                call_args[1].clone()
            };
            self_value.compare(&other_value, CompareOp::Eq)
        }),
    );
    ns.insert(
        CompactString::from("__ne__"),
        PyObject::native_closure("ChainMap.__ne__", move |call_args| {
            if call_args.len() < 2 {
                return Ok(PyObject::bool_val(true));
            }
            let self_maps = maps_from_self(&call_args[0])?;
            let self_value = build_builtin_value(&self_maps)?;
            let other_value = if let Ok(other_maps) = maps_from_self(&call_args[1]) {
                build_builtin_value(&other_maps)?
            } else {
                call_args[1].clone()
            };
            self_value.compare(&other_value, CompareOp::Ne)
        }),
    );
    ns.insert(
        CompactString::from("__setitem__"),
        PyObject::native_closure("ChainMap.__setitem__", move |call_args| {
            if call_args.len() < 3 {
                return Err(PyException::type_error(
                    "__setitem__ requires key and value",
                ));
            }
            let key = &call_args[1];
            let value = &call_args[2];
            let hk = HashableKey::from_object(key)?;
            let maps = maps_from_self(&call_args[0])?;
            if let Some(first) = maps.first() {
                if let PyObjectPayload::Dict(dict) = &first.payload {
                    dict.write().insert(hk, value.clone());
                }
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__delitem__"),
        PyObject::native_closure("ChainMap.__delitem__", move |call_args| {
            if call_args.len() < 2 {
                return Err(PyException::type_error("__delitem__ requires key"));
            }
            let key = &call_args[1];
            let hk = HashableKey::from_object(key)?;
            let maps = maps_from_self(&call_args[0])?;
            if let Some(first) = maps.first() {
                if let PyObjectPayload::Dict(dict) = &first.payload {
                    if dict.write().shift_remove(&hk).is_none() {
                        return Err(PyException::key_error(&key.py_to_string()));
                    }
                    return Ok(PyObject::none());
                }
            }
            Err(PyException::key_error(&call_args[1].py_to_string()))
        }),
    );
    ns.insert(
        CompactString::from("__repr__"),
        PyObject::native_closure("ChainMap.__repr__", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("__repr__ requires self"));
            }
            let mut parts = Vec::new();
            let maps = maps_from_self(&call_args[0])?;
            for m in &maps {
                parts.push(m.py_to_string());
            }
            let class_name = if let PyObjectPayload::Instance(inst) = &call_args[0].payload {
                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                    cd.name.to_string()
                } else {
                    "ChainMap".to_string()
                }
            } else {
                "ChainMap".to_string()
            };
            Ok(PyObject::str_val(CompactString::from(format!(
                "{}({})",
                class_name,
                parts.join(", ")
            ))))
        }),
    );

    let copy_fn = PyObject::native_closure("ChainMap.copy", move |call_args| {
        if call_args.is_empty() {
            return Err(PyException::type_error("copy requires self"));
        }
        let maps = maps_from_self(&call_args[0])?;
        let mut new_maps = Vec::with_capacity(maps.len());
        if let Some(first) = maps.first() {
            let copied = match &first.payload {
                PyObjectPayload::Dict(dict) => PyObject::dict(dict.read().clone()),
                _ => first.clone(),
            };
            new_maps.push(copied);
            new_maps.extend(maps.iter().skip(1).cloned());
        }
        let class_obj = if let PyObjectPayload::Instance(inst) = &call_args[0].payload {
            inst.class.clone()
        } else {
            PyObject::builtin_type(CompactString::from("object"))
        };
        chainmap_make_instance(class_obj, new_maps, true)
    });
    ns.insert(CompactString::from("copy"), copy_fn.clone());
    ns.insert(CompactString::from("__copy__"), copy_fn);
    ns.insert(
        CompactString::from("new_child"),
        PyObject::native_closure("ChainMap.new_child", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("new_child requires self"));
            }
            let child_map = if call_args.len() > 1 {
                call_args[1].clone()
            } else {
                PyObject::dict(IndexMap::new())
            };
            let maps = maps_from_self(&call_args[0])?;
            let mut new_maps = vec![child_map];
            new_maps.extend(maps.into_iter());
            let class_obj = if let PyObjectPayload::Instance(inst) = &call_args[0].payload {
                inst.class.clone()
            } else {
                PyObject::builtin_type(CompactString::from("object"))
            };
            chainmap_make_instance(class_obj, new_maps, true)
        }),
    );
    ns.insert(
        CompactString::from("pop"),
        PyObject::native_closure("ChainMap.pop", move |call_args| {
            if call_args.len() < 2 {
                return Err(PyException::type_error("pop requires key"));
            }
            let self_obj = &call_args[0];
            let key = &call_args[1];
            let default = call_args.get(2).cloned();
            let maps = maps_from_self(self_obj)?;
            if let Some(first) = maps.first() {
                if let PyObjectPayload::Dict(dict) = &first.payload {
                    let hk = HashableKey::from_object(key)?;
                    if let Some(v) = dict.write().shift_remove(&hk) {
                        return Ok(v);
                    }
                }
            }
            default.ok_or_else(|| PyException::key_error(key.py_to_string()))
        }),
    );
    ns.insert(
        CompactString::from("popitem"),
        PyObject::native_closure("ChainMap.popitem", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("popitem requires self"));
            }
            let maps = maps_from_self(&call_args[0])?;
            if let Some(first) = maps.first() {
                if let PyObjectPayload::Dict(dict) = &first.payload {
                    let mut w = dict.write();
                    if let Some((k, v)) = w.iter().next().map(|(k, v)| (k.clone(), v.clone())) {
                        w.shift_remove(&k);
                        return Ok(PyObject::tuple(vec![k.to_object(), v]));
                    }
                }
            }
            Err(PyException::key_error("popitem(): dictionary is empty"))
        }),
    );
    ns.insert(
        CompactString::from("clear"),
        PyObject::native_closure("ChainMap.clear", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("clear requires self"));
            }
            let maps = maps_from_self(&call_args[0])?;
            if let Some(first) = maps.first() {
                if let PyObjectPayload::Dict(dict) = &first.payload {
                    dict.write().clear();
                }
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__reduce__"),
        PyObject::native_closure("ChainMap.__reduce__", move |call_args| {
            if call_args.is_empty() {
                return Err(PyException::type_error("__reduce__ requires self"));
            }
            let self_obj = &call_args[0];
            let maps = maps_from_self(self_obj)?;
            let class_obj = if let PyObjectPayload::Instance(inst) = &self_obj.payload {
                inst.class.clone()
            } else {
                PyObject::builtin_type(CompactString::from("object"))
            };
            Ok(PyObject::tuple(vec![class_obj, PyObject::tuple(maps)]))
        }),
    );

    PyObject::class(CompactString::from("ChainMap"), vec![], ns)
}

fn copy_instance_attrs(src_attrs: &SharedFxAttrMap, dst_attrs: &SharedFxAttrMap, skip: &[&str]) {
    let src = src_attrs.read();
    let mut dst = dst_attrs.write();
    for (name, value) in src.iter() {
        if skip.iter().any(|s| *s == name.as_str()) {
            continue;
        }
        dst.insert(name.clone(), value.clone());
    }
}

fn build_userdict_copy(
    data: &PyObjectRef,
    owner_class: PyObjectRef,
    src_attrs: &SharedFxAttrMap,
) -> PyResult<PyObjectRef> {
    let new_data = if let PyObjectPayload::Dict(m) = &data.payload {
        PyObject::dict(m.read().clone())
    } else {
        PyObject::dict_from_pairs(vec![])
    };
    let new_inst = PyObject::instance(owner_class);
    if let PyObjectPayload::Instance(ref dst_inst) = new_inst.payload {
        dst_inst
            .attrs
            .write()
            .insert(CompactString::from("data"), new_data.clone());
        install_dict_methods(&dst_inst.attrs, &new_data, dst_inst.class.clone());
        copy_instance_attrs(
            src_attrs,
            &dst_inst.attrs,
            &[
                "data",
                "keys",
                "values",
                "items",
                "get",
                "pop",
                "setdefault",
                "update",
                "copy",
                "clear",
            ],
        );
    }
    Ok(new_inst)
}

fn build_userlist_copy(
    data: &PyObjectRef,
    owner_class: PyObjectRef,
    src_attrs: &SharedFxAttrMap,
) -> PyResult<PyObjectRef> {
    let new_data = if let PyObjectPayload::List(items) = &data.payload {
        PyObject::list(items.read().clone())
    } else {
        PyObject::list(vec![])
    };
    let new_inst = PyObject::instance(owner_class);
    if let PyObjectPayload::Instance(ref dst_inst) = new_inst.payload {
        dst_inst
            .attrs
            .write()
            .insert(CompactString::from("data"), new_data.clone());
        install_list_methods(&dst_inst.attrs, &new_data, dst_inst.class.clone());
        copy_instance_attrs(
            src_attrs,
            &dst_inst.attrs,
            &[
                "data", "append", "extend", "insert", "pop", "remove", "clear", "reverse", "count",
                "index", "sort", "copy",
            ],
        );
    }
    Ok(new_inst)
}

// --- UserDict / UserList / UserString ---

fn make_user_dict_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        native_method("UserDict", "__init__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("UserDict.__init__ requires self"));
            }
            let inst = &args[0];
            let data = if args.len() > 1 {
                if let PyObjectPayload::Dict(d) = &args[1].payload {
                    PyObject::wrap(PyObjectPayload::Dict(Rc::new(PyCell::new(
                        d.read().clone(),
                    ))))
                } else {
                    PyObject::dict_from_pairs(vec![])
                }
            } else {
                PyObject::dict_from_pairs(vec![])
            };
            if let PyObjectPayload::Instance(d) = &inst.payload {
                d.attrs
                    .write()
                    .insert(CompactString::from("data"), data.clone());
                // Install instance methods that directly operate on the data
                install_dict_methods(&d.attrs, &data, d.class.clone());
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__getitem__"),
        native_method("UserDict", "__getitem__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected key"));
            }
            let data = get_user_data(&args[0], "data")?;
            data.get_item(&args[1])
        }),
    );
    ns.insert(
        CompactString::from("__setitem__"),
        native_method("UserDict", "__setitem__", |args| {
            if args.len() < 3 {
                return Err(PyException::type_error("expected key and value"));
            }
            let data = get_user_data(&args[0], "data")?;
            if let PyObjectPayload::Dict(d) = &data.payload {
                let key = HashableKey::from_object(&args[1])?;
                d.write().insert(key, args[2].clone());
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__delitem__"),
        native_method("UserDict", "__delitem__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected key"));
            }
            let data = get_user_data(&args[0], "data")?;
            if let PyObjectPayload::Dict(d) = &data.payload {
                let key = HashableKey::from_object(&args[1])?;
                if d.write().shift_remove(&key).is_none() {
                    return Err(PyException::key_error(args[1].py_to_string()));
                }
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__len__"),
        native_method("UserDict", "__len__", |args| {
            let data = get_user_data(&args[0], "data")?;
            Ok(PyObject::int(data.py_len()? as i64))
        }),
    );
    ns.insert(
        CompactString::from("__contains__"),
        native_method("UserDict", "__contains__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected key"));
            }
            let data = get_user_data(&args[0], "data")?;
            if let PyObjectPayload::Dict(d) = &data.payload {
                let key = HashableKey::from_object(&args[1])?;
                Ok(PyObject::bool_val(d.read().contains_key(&key)))
            } else {
                Ok(PyObject::bool_val(false))
            }
        }),
    );
    ns.insert(
        CompactString::from("__repr__"),
        native_method("UserDict", "__repr__", |args| {
            let data = get_user_data(&args[0], "data")?;
            Ok(PyObject::str_val(CompactString::from(data.py_to_string())))
        }),
    );
    ns.insert(
        CompactString::from("__iter__"),
        native_method("UserDict", "__iter__", |args| {
            let data = get_user_data(&args[0], "data")?;
            data.get_iter()
        }),
    );
    ns.insert(
        CompactString::from("__eq__"),
        native_method("UserDict", "__eq__", |args| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let data = get_user_data(&args[0], "data")?;
            let other_data = if let Ok(od) = get_user_data(&args[1], "data") {
                od
            } else {
                args[1].clone()
            };
            if let (PyObjectPayload::Dict(a), PyObjectPayload::Dict(b)) =
                (&data.payload, &other_data.payload)
            {
                let ra = a.read();
                let rb = b.read();
                if ra.len() != rb.len() {
                    return Ok(PyObject::bool_val(false));
                }
                for (k, v) in ra.iter() {
                    match rb.get(k) {
                        Some(ov)
                            if v.compare(ov, CompareOp::Eq)
                                .map_or(false, |r| r.is_truthy()) => {}
                        _ => return Ok(PyObject::bool_val(false)),
                    }
                }
                Ok(PyObject::bool_val(true))
            } else {
                Ok(PyObject::bool_val(false))
            }
        }),
    );
    ns.insert(
        CompactString::from("__bool__"),
        native_method("UserDict", "__bool__", |args| {
            let data = get_user_data(&args[0], "data")?;
            Ok(PyObject::bool_val(data.py_len()? > 0))
        }),
    );
    ns.insert(
        CompactString::from("__or__"),
        native_method("UserDict", "__or__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected other"));
            }
            let data = get_user_data(&args[0], "data")?;
            let mut merged = IndexMap::new();
            if let PyObjectPayload::Dict(d) = &data.payload {
                for (k, v) in d.read().iter() {
                    merged.insert(k.clone(), v.clone());
                }
            }
            let other = if let Ok(od) = get_user_data(&args[1], "data") {
                od
            } else {
                args[1].clone()
            };
            if let PyObjectPayload::Dict(d) = &other.payload {
                for (k, v) in d.read().iter() {
                    merged.insert(k.clone(), v.clone());
                }
            }
            Ok(PyObject::dict(merged))
        }),
    );
    ns.insert(
        CompactString::from("__copy__"),
        native_method("UserDict", "__copy__", copy_userdict_instance),
    );
    ns.insert(
        CompactString::from("__ior__"),
        native_method("UserDict", "__ior__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected other"));
            }
            let data = get_user_data(&args[0], "data")?;
            let other = if let Ok(od) = get_user_data(&args[1], "data") {
                od
            } else {
                args[1].clone()
            };
            if let (PyObjectPayload::Dict(dst), PyObjectPayload::Dict(src)) =
                (&data.payload, &other.payload)
            {
                let mut w = dst.write();
                for (k, v) in src.read().iter() {
                    w.insert(k.clone(), v.clone());
                }
            }
            Ok(args[0].clone())
        }),
    );
    PyObject::class(CompactString::from("UserDict"), vec![], ns)
}

fn install_dict_methods(attrs: &SharedFxAttrMap, data: &PyObjectRef, owner_class: PyObjectRef) {
    let map = if let PyObjectPayload::Dict(m) = &data.payload {
        m.clone()
    } else {
        return;
    };
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("keys"),
        PyObject::native_closure("keys", move |_| {
            Ok(PyObject::wrap(PyObjectPayload::DictKeys(m.clone())))
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("values"),
        PyObject::native_closure("values", move |_| {
            Ok(PyObject::wrap(PyObjectPayload::DictValues(m.clone())))
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("items"),
        PyObject::native_closure("items", move |_| {
            Ok(PyObject::wrap(PyObjectPayload::DictItems(m.clone())))
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("get"),
        PyObject::native_closure("get", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "get() requires at least 1 argument",
                ));
            }
            let key = args[0].to_hashable_key()?;
            let default = if args.len() >= 2 {
                args[1].clone()
            } else {
                PyObject::none()
            };
            Ok(m.read().get(&key).cloned().unwrap_or(default))
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("pop"),
        PyObject::native_closure("pop", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "pop() requires at least 1 argument",
                ));
            }
            let key = args[0].to_hashable_key()?;
            let default = if args.len() >= 2 {
                Some(args[1].clone())
            } else {
                None
            };
            match m.write().shift_remove(&key) {
                Some(v) => Ok(v),
                None => default.ok_or_else(|| PyException::key_error(args[0].py_to_string())),
            }
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("setdefault"),
        PyObject::native_closure("setdefault", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error(
                    "setdefault() requires at least 1 argument",
                ));
            }
            let key = args[0].to_hashable_key()?;
            let default = if args.len() >= 2 {
                args[1].clone()
            } else {
                PyObject::none()
            };
            let mut w = m.write();
            if let Some(v) = w.get(&key) {
                return Ok(v.clone());
            }
            w.insert(key, default.clone());
            Ok(default)
        }),
    );
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("update"),
        PyObject::native_closure("update", move |args| {
            if !args.is_empty() {
                if let PyObjectPayload::Dict(other) = &args[0].payload {
                    let mut w = m.write();
                    for (k, v) in other.read().iter() {
                        w.insert(k.clone(), v.clone());
                    }
                }
            }
            Ok(PyObject::none())
        }),
    );
    let m = map.clone();
    attrs.write().insert(CompactString::from("copy"), {
        let attrs = attrs.clone();
        let data = data.clone();
        let owner_class = owner_class.clone();
        PyObject::native_closure("copy", move |_| {
            build_userdict_copy(&data, owner_class.clone(), &attrs)
        })
    });
    let m = map.clone();
    attrs.write().insert(
        CompactString::from("clear"),
        PyObject::native_closure("clear", move |_| {
            m.write().clear();
            Ok(PyObject::none())
        }),
    );
}

fn copy_userdict_instance(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("UserDict.__copy__ requires self"));
    }
    let self_obj = &args[0];
    let inst = if let PyObjectPayload::Instance(inst) = &self_obj.payload {
        inst
    } else {
        return Err(PyException::type_error(
            "UserDict.__copy__ requires an instance",
        ));
    };
    let data = get_user_data(self_obj, "data")?;
    build_userdict_copy(&data, inst.class.clone(), &inst.attrs)
}

fn copy_userlist_instance(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("UserList.__copy__ requires self"));
    }
    let self_obj = &args[0];
    let inst = if let PyObjectPayload::Instance(inst) = &self_obj.payload {
        inst
    } else {
        return Err(PyException::type_error(
            "UserList.__copy__ requires an instance",
        ));
    };
    let data = get_user_data(self_obj, "data")?;
    build_userlist_copy(&data, inst.class.clone(), &inst.attrs)
}

fn make_user_list_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        native_method("UserList", "__init__", |args| {
            if args.is_empty() {
                return Err(PyException::type_error("UserList.__init__ requires self"));
            }
            let inst = &args[0];
            let data = if args.len() > 1 {
                let items = args[1].to_list()?;
                PyObject::list(items)
            } else {
                PyObject::list(vec![])
            };
            if let PyObjectPayload::Instance(d) = &inst.payload {
                d.attrs
                    .write()
                    .insert(CompactString::from("data"), data.clone());
                install_list_methods(&d.attrs, &data, d.class.clone());
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__getitem__"),
        native_method("UserList", "__getitem__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected index"));
            }
            let data = get_user_data(&args[0], "data")?;
            data.get_item(&args[1])
        }),
    );
    ns.insert(
        CompactString::from("__setitem__"),
        native_method("UserList", "__setitem__", |args| {
            if args.len() < 3 {
                return Err(PyException::type_error("expected index and value"));
            }
            let data = get_user_data(&args[0], "data")?;
            if let PyObjectPayload::List(l) = &data.payload {
                let idx = args[1].to_int()? as i64;
                let mut w = l.write();
                let len = w.len() as i64;
                let i = if idx < 0 {
                    (len + idx).max(0) as usize
                } else {
                    idx as usize
                };
                if i < w.len() {
                    w[i] = args[2].clone();
                } else {
                    return Err(PyException::index_error(
                        "list assignment index out of range",
                    ));
                }
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__len__"),
        native_method("UserList", "__len__", |args| {
            let data = get_user_data(&args[0], "data")?;
            Ok(PyObject::int(data.py_len()? as i64))
        }),
    );
    ns.insert(
        CompactString::from("__contains__"),
        native_method("UserList", "__contains__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected item"));
            }
            let data = get_user_data(&args[0], "data")?;
            if let PyObjectPayload::List(l) = &data.payload {
                let target = &args[1];
                Ok(PyObject::bool_val(l.read().iter().any(|x| {
                    x.compare(target, CompareOp::Eq)
                        .map_or(false, |v| v.is_truthy())
                })))
            } else {
                Ok(PyObject::bool_val(false))
            }
        }),
    );
    ns.insert(
        CompactString::from("__repr__"),
        native_method("UserList", "__repr__", |args| {
            let data = get_user_data(&args[0], "data")?;
            Ok(PyObject::str_val(CompactString::from(data.py_to_string())))
        }),
    );
    ns.insert(
        CompactString::from("__iter__"),
        native_method("UserList", "__iter__", |args| {
            let data = get_user_data(&args[0], "data")?;
            data.get_iter()
        }),
    );
    ns.insert(
        CompactString::from("__delitem__"),
        native_method("UserList", "__delitem__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected index"));
            }
            let data = get_user_data(&args[0], "data")?;
            if let PyObjectPayload::List(l) = &data.payload {
                let idx = args[1].to_int()? as i64;
                let mut w = l.write();
                let len = w.len() as i64;
                let i = if idx < 0 {
                    (len + idx).max(0) as usize
                } else {
                    idx as usize
                };
                if i < w.len() {
                    w.remove(i);
                    Ok(PyObject::none())
                } else {
                    Err(PyException::index_error(
                        "list assignment index out of range",
                    ))
                }
            } else {
                Err(PyException::type_error("expected list data"))
            }
        }),
    );
    ns.insert(
        CompactString::from("__add__"),
        native_method("UserList", "__add__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected other"));
            }
            let data = get_user_data(&args[0], "data")?;
            let other = if let Ok(od) = get_user_data(&args[1], "data") {
                od
            } else {
                args[1].clone()
            };
            let mut items = data.to_list()?;
            items.extend(other.to_list()?);
            Ok(PyObject::list(items))
        }),
    );
    ns.insert(
        CompactString::from("__iadd__"),
        native_method("UserList", "__iadd__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected other"));
            }
            let data = get_user_data(&args[0], "data")?;
            let other = if let Ok(od) = get_user_data(&args[1], "data") {
                od
            } else {
                args[1].clone()
            };
            if let PyObjectPayload::List(l) = &data.payload {
                l.write().extend(other.to_list()?);
            }
            Ok(args[0].clone())
        }),
    );
    ns.insert(
        CompactString::from("__mul__"),
        native_method("UserList", "__mul__", |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected int"));
            }
            let data = get_user_data(&args[0], "data")?;
            let n = args[1].to_int()?.max(0) as usize;
            let items = data.to_list()?;
            let mut result = Vec::with_capacity(items.len() * n);
            for _ in 0..n {
                result.extend(items.iter().cloned());
            }
            Ok(PyObject::list(result))
        }),
    );
    ns.insert(
        CompactString::from("__copy__"),
        native_method("UserList", "__copy__", copy_userlist_instance),
    );
    ns.insert(
        CompactString::from("__eq__"),
        native_method("UserList", "__eq__", |args| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let data = get_user_data(&args[0], "data")?;
            let other = if let Ok(od) = get_user_data(&args[1], "data") {
                od
            } else {
                args[1].clone()
            };
            if let (PyObjectPayload::List(a), PyObjectPayload::List(b)) =
                (&data.payload, &other.payload)
            {
                let ra = a.read();
                let rb = b.read();
                if ra.len() != rb.len() {
                    return Ok(PyObject::bool_val(false));
                }
                for (x, y) in ra.iter().zip(rb.iter()) {
                    if !x.compare(y, CompareOp::Eq).map_or(false, |v| v.is_truthy()) {
                        return Ok(PyObject::bool_val(false));
                    }
                }
                Ok(PyObject::bool_val(true))
            } else {
                Ok(PyObject::bool_val(false))
            }
        }),
    );
    ns.insert(
        CompactString::from("__bool__"),
        native_method("UserList", "__bool__", |args| {
            let data = get_user_data(&args[0], "data")?;
            Ok(PyObject::bool_val(data.py_len()? > 0))
        }),
    );
    PyObject::class(CompactString::from("UserList"), vec![], ns)
}

fn install_list_methods(attrs: &SharedFxAttrMap, data: &PyObjectRef, owner_class: PyObjectRef) {
    if !matches!(&data.payload, PyObjectPayload::List(_)) {
        return;
    }
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("append"),
        PyObject::native_closure("append", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("append() requires 1 argument"));
            }
            if let PyObjectPayload::List(items) = &l.payload {
                items.write().push(args[0].clone());
            }
            Ok(PyObject::none())
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("extend"),
        PyObject::native_closure("extend", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("extend() requires 1 argument"));
            }
            let new_items = args[0].to_list()?;
            if let PyObjectPayload::List(items) = &l.payload {
                items.write().extend(new_items);
            }
            Ok(PyObject::none())
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("insert"),
        PyObject::native_closure("insert", move |args| {
            if args.len() < 2 {
                return Err(PyException::type_error("insert() requires 2 arguments"));
            }
            let idx = args[0].to_int()? as usize;
            if let PyObjectPayload::List(items) = &l.payload {
                let mut w = items.write();
                let idx = idx.min(w.len());
                w.insert(idx, args[1].clone());
            }
            Ok(PyObject::none())
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("pop"),
        PyObject::native_closure("pop", move |args| {
            if let PyObjectPayload::List(items) = &l.payload {
                let mut w = items.write();
                if w.is_empty() {
                    return Err(PyException::index_error("pop from empty list"));
                }
                let idx = if !args.is_empty() {
                    let i = args[0].to_int()? as i64;
                    let len = w.len() as i64;
                    (if i < 0 {
                        (len + i).max(0)
                    } else {
                        i.min(len - 1)
                    }) as usize
                } else {
                    w.len() - 1
                };
                if idx < w.len() {
                    Ok(w.remove(idx))
                } else {
                    Err(PyException::index_error("pop index out of range"))
                }
            } else {
                Err(PyException::type_error("not a list"))
            }
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("remove"),
        PyObject::native_closure("remove", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("remove() requires 1 argument"));
            }
            if let PyObjectPayload::List(items) = &l.payload {
                let mut w = items.write();
                let target = &args[0];
                if let Some(pos) = w.iter().position(|x| {
                    x.compare(target, CompareOp::Eq)
                        .map_or(false, |v| v.is_truthy())
                }) {
                    w.remove(pos);
                    Ok(PyObject::none())
                } else {
                    Err(PyException::value_error("list.remove(x): x not in list"))
                }
            } else {
                Err(PyException::type_error("not a list"))
            }
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("clear"),
        PyObject::native_closure("clear", move |_| {
            if let PyObjectPayload::List(items) = &l.payload {
                items.write().clear();
            }
            Ok(PyObject::none())
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("reverse"),
        PyObject::native_closure("reverse", move |_| {
            if let PyObjectPayload::List(items) = &l.payload {
                items.write().reverse();
            }
            Ok(PyObject::none())
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("count"),
        PyObject::native_closure("count", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("count() requires 1 argument"));
            }
            let target = &args[0];
            if let PyObjectPayload::List(items) = &l.payload {
                let count = items
                    .read()
                    .iter()
                    .filter(|x| {
                        x.compare(target, CompareOp::Eq)
                            .map_or(false, |v| v.is_truthy())
                    })
                    .count();
                Ok(PyObject::int(count as i64))
            } else {
                Ok(PyObject::int(0))
            }
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("index"),
        PyObject::native_closure("index", move |args| {
            if args.is_empty() {
                return Err(PyException::type_error("index() requires 1 argument"));
            }
            let target = &args[0];
            if let PyObjectPayload::List(items) = &l.payload {
                let r = items.read();
                for (i, x) in r.iter().enumerate() {
                    if x.compare(target, CompareOp::Eq)
                        .map_or(false, |v| v.is_truthy())
                    {
                        return Ok(PyObject::int(i as i64));
                    }
                }
            }
            Err(PyException::value_error("x not in list"))
        }),
    );
    let l = data.clone();
    attrs.write().insert(
        CompactString::from("sort"),
        PyObject::native_closure("sort", move |_| {
            if let PyObjectPayload::List(items) = &l.payload {
                let mut w = items.write();
                let mut sorted: Vec<_> = w.drain(..).collect();
                sorted.sort_by(|a, b| {
                    a.compare(b, CompareOp::Lt)
                        .map_or(std::cmp::Ordering::Equal, |v| {
                            if v.is_truthy() {
                                std::cmp::Ordering::Less
                            } else {
                                std::cmp::Ordering::Greater
                            }
                        })
                });
                *w = sorted;
            }
            Ok(PyObject::none())
        }),
    );
    let l = data.clone();
    attrs.write().insert(CompactString::from("copy"), {
        let attrs = attrs.clone();
        let data = data.clone();
        let owner_class = owner_class.clone();
        PyObject::native_closure("copy", move |_| {
            build_userlist_copy(&data, owner_class.clone(), &attrs)
        })
    });
}

fn make_user_string_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(
        CompactString::from("__init__"),
        make_builtin(|args| {
            if args.is_empty() {
                return Err(PyException::type_error("UserString.__init__ requires self"));
            }
            let inst = &args[0];
            let data = if args.len() > 1 {
                PyObject::str_val(CompactString::from(args[1].py_to_string()))
            } else {
                PyObject::str_val(CompactString::from(""))
            };
            if let PyObjectPayload::Instance(d) = &inst.payload {
                d.attrs
                    .write()
                    .insert(CompactString::from("data"), data.clone());
                install_string_methods(&d.attrs, &data);
            }
            Ok(PyObject::none())
        }),
    );
    ns.insert(
        CompactString::from("__str__"),
        make_builtin(|args| get_user_data(&args[0], "data")),
    );
    ns.insert(
        CompactString::from("__repr__"),
        make_builtin(|args| {
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("");
            Ok(PyObject::str_val(CompactString::from(format!("'{}'", s))))
        }),
    );
    ns.insert(
        CompactString::from("__len__"),
        make_builtin(|args| {
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("");
            Ok(PyObject::int(s.len() as i64))
        }),
    );
    ns.insert(
        CompactString::from("__contains__"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected item"));
            }
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("");
            let sub = args[1].py_to_string();
            Ok(PyObject::bool_val(s.contains(&*sub)))
        }),
    );
    ns.insert(
        CompactString::from("__add__"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected other"));
            }
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("").to_string();
            let other = args[1].py_to_string();
            Ok(PyObject::str_val(CompactString::from(format!(
                "{}{}",
                s, other
            ))))
        }),
    );
    ns.insert(
        CompactString::from("__eq__"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Ok(PyObject::bool_val(false));
            }
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("");
            let other = args[1].py_to_string();
            Ok(PyObject::bool_val(s == other.as_str()))
        }),
    );
    ns.insert(
        CompactString::from("__getitem__"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected index"));
            }
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("");
            let idx = args[1].to_int()? as i64;
            let len = s.chars().count() as i64;
            let i = if idx < 0 {
                (len + idx).max(0) as usize
            } else {
                idx as usize
            };
            match s.chars().nth(i) {
                Some(c) => Ok(PyObject::str_val(CompactString::from(c.to_string()))),
                None => Err(PyException::index_error("string index out of range")),
            }
        }),
    );
    ns.insert(
        CompactString::from("__iter__"),
        make_builtin(|args| {
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("").to_string();
            let chars: Vec<PyObjectRef> = s
                .chars()
                .map(|c| PyObject::str_val(CompactString::from(c.to_string())))
                .collect();
            Ok(PyObject::list(chars))
        }),
    );
    ns.insert(
        CompactString::from("__mul__"),
        make_builtin(|args| {
            if args.len() < 2 {
                return Err(PyException::type_error("expected int"));
            }
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("");
            let n = args[1].to_int()?.max(0) as usize;
            Ok(PyObject::str_val(CompactString::from(s.repeat(n))))
        }),
    );
    ns.insert(
        CompactString::from("__bool__"),
        make_builtin(|args| {
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("");
            Ok(PyObject::bool_val(!s.is_empty()))
        }),
    );
    ns.insert(
        CompactString::from("__hash__"),
        make_builtin(|args| {
            let data = get_user_data(&args[0], "data")?;
            let s = data.as_str().unwrap_or("");
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            s.hash(&mut hasher);
            Ok(PyObject::int(hasher.finish() as i64))
        }),
    );
    PyObject::class(CompactString::from("UserString"), vec![], ns)
}

fn install_string_methods(attrs: &SharedFxAttrMap, data: &PyObjectRef) {
    let s_val = data.as_str().unwrap_or("").to_string();

    macro_rules! str_method {
        ($attrs:expr, $name:expr, $s:expr, $body:expr) => {{
            let captured = $s.clone();
            $attrs.write().insert(
                CompactString::from($name),
                PyObject::native_closure($name, move |args| {
                    let s = &captured;
                    #[allow(clippy::redundant_closure_call)]
                    ($body)(s, args)
                }),
            );
        }};
    }

    str_method!(attrs, "upper", s_val, |s: &String,
                                        _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::str_val(CompactString::from(s.to_uppercase())))
    });
    str_method!(attrs, "lower", s_val, |s: &String,
                                        _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::str_val(CompactString::from(s.to_lowercase())))
    });
    str_method!(attrs, "strip", s_val, |s: &String,
                                        _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::str_val(CompactString::from(s.trim())))
    });
    str_method!(attrs, "lstrip", s_val, |s: &String,
                                         _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::str_val(CompactString::from(s.trim_start())))
    });
    str_method!(attrs, "rstrip", s_val, |s: &String,
                                         _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::str_val(CompactString::from(s.trim_end())))
    });
    str_method!(attrs, "title", s_val, |s: &String,
                                        _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        let mut title = String::with_capacity(s.len());
        let mut capitalize_next = true;
        for c in s.chars() {
            if c.is_whitespace() || !c.is_alphanumeric() {
                capitalize_next = true;
                title.push(c);
            } else if capitalize_next {
                title.extend(c.to_uppercase());
                capitalize_next = false;
            } else {
                title.extend(c.to_lowercase());
            }
        }
        Ok(PyObject::str_val(CompactString::from(title)))
    });
    str_method!(attrs, "capitalize", s_val, |s: &String,
                                             _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        let mut chars = s.chars();
        let cap = match chars.next() {
            None => String::new(),
            Some(c) => c.to_uppercase().to_string() + &chars.as_str().to_lowercase(),
        };
        Ok(PyObject::str_val(CompactString::from(cap)))
    });
    str_method!(attrs, "swapcase", s_val, |s: &String,
                                           _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        let swapped: String = s
            .chars()
            .map(|c| {
                if c.is_uppercase() {
                    c.to_lowercase().to_string()
                } else if c.is_lowercase() {
                    c.to_uppercase().to_string()
                } else {
                    c.to_string()
                }
            })
            .collect();
        Ok(PyObject::str_val(CompactString::from(swapped)))
    });
    str_method!(attrs, "split", s_val, |s: &String,
                                        args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        let parts: Vec<PyObjectRef> =
            if args.is_empty() || matches!(args[0].payload, PyObjectPayload::None) {
                s.split_whitespace()
                    .map(|p| PyObject::str_from_utf8_slice(p.as_bytes()))
                    .collect()
            } else if let PyObjectPayload::Str(sr) = &args[0].payload {
                s.split(sr.as_str())
                    .map(|p| PyObject::str_from_utf8_slice(p.as_bytes()))
                    .collect()
            } else {
                let sep = args[0].py_to_string();
                s.split(&*sep)
                    .map(|p| PyObject::str_from_utf8_slice(p.as_bytes()))
                    .collect()
            };
        Ok(PyObject::list(parts))
    });
    str_method!(attrs, "rsplit", s_val, |s: &String,
                                         args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        let parts: Vec<PyObjectRef> =
            if args.is_empty() || matches!(args[0].payload, PyObjectPayload::None) {
                s.split_whitespace()
                    .rev()
                    .map(|p| PyObject::str_from_utf8_slice(p.as_bytes()))
                    .collect()
            } else if let PyObjectPayload::Str(sr) = &args[0].payload {
                s.rsplit(sr.as_str())
                    .map(|p| PyObject::str_from_utf8_slice(p.as_bytes()))
                    .collect()
            } else {
                let sep = args[0].py_to_string();
                s.rsplit(&*sep)
                    .map(|p| PyObject::str_from_utf8_slice(p.as_bytes()))
                    .collect()
            };
        Ok(PyObject::list(parts))
    });
    str_method!(attrs, "replace", s_val, |s: &String,
                                          args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "replace() requires at least 2 arguments",
            ));
        }
        let result = match (&args[0].payload, &args[1].payload) {
            (PyObjectPayload::Str(old_s), PyObjectPayload::Str(new_s)) => {
                s.replace(old_s.as_str(), new_s.as_str())
            }
            _ => {
                let old = args[0].py_to_string();
                let new = args[1].py_to_string();
                s.replace(&*old, &*new)
            }
        };
        Ok(PyObject::str_val(CompactString::from(result)))
    });
    str_method!(attrs, "find", s_val, |s: &String,
                                       args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("find() requires 1 argument"));
        }
        let idx = if let PyObjectPayload::Str(sr) = &args[0].payload {
            s.find(sr.as_str())
        } else {
            let sub = args[0].py_to_string();
            s.find(&*sub)
        };
        Ok(PyObject::int(idx.map_or(-1, |i| i as i64)))
    });
    str_method!(attrs, "rfind", s_val, |s: &String,
                                        args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("rfind() requires 1 argument"));
        }
        let idx = if let PyObjectPayload::Str(sr) = &args[0].payload {
            s.rfind(sr.as_str())
        } else {
            let sub = args[0].py_to_string();
            s.rfind(&*sub)
        };
        Ok(PyObject::int(idx.map_or(-1, |i| i as i64)))
    });
    str_method!(attrs, "count", s_val, |s: &String,
                                        args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("count() requires 1 argument"));
        }
        let n = if let PyObjectPayload::Str(sr) = &args[0].payload {
            s.matches(sr.as_str()).count()
        } else {
            let sub = args[0].py_to_string();
            s.matches(&*sub).count()
        };
        Ok(PyObject::int(n as i64))
    });
    str_method!(attrs, "startswith", s_val, |s: &String,
                                             args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("startswith() requires 1 argument"));
        }
        let result = if let PyObjectPayload::Str(sr) = &args[0].payload {
            s.starts_with(sr.as_str())
        } else {
            let prefix = args[0].py_to_string();
            s.starts_with(&*prefix)
        };
        Ok(PyObject::bool_val(result))
    });
    str_method!(attrs, "endswith", s_val, |s: &String,
                                           args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("endswith() requires 1 argument"));
        }
        let result = if let PyObjectPayload::Str(sr) = &args[0].payload {
            s.ends_with(sr.as_str())
        } else {
            let suffix = args[0].py_to_string();
            s.ends_with(&*suffix)
        };
        Ok(PyObject::bool_val(result))
    });
    str_method!(attrs, "join", s_val, |s: &String,
                                       args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("join() requires 1 argument"));
        }
        // Direct access to list/tuple data via data_ptr — avoids to_list() Vec clone
        let (items_slice, _owned): (&[PyObjectRef], Option<Vec<PyObjectRef>>) =
            match &args[0].payload {
                PyObjectPayload::List(v) => {
                    let vec = unsafe { &*v.data_ptr() };
                    (vec.as_slice(), None)
                }
                PyObjectPayload::Tuple(v) => (&**v, None),
                _ => {
                    let list = args[0].to_list()?;
                    // Need owned Vec to live long enough — store it and take slice
                    (
                        unsafe { std::slice::from_raw_parts(list.as_ptr(), list.len()) },
                        Some(list),
                    )
                }
            };
        if items_slice.is_empty() {
            return Ok(PyObject::str_val(CompactString::new("")));
        }
        // Single-allocation join: pre-compute total length, then build
        let sep_len = s.len();
        let mut total_len = 0usize;
        for (i, item) in items_slice.iter().enumerate() {
            if i > 0 {
                total_len += sep_len;
            }
            if let PyObjectPayload::Str(sr) = &item.payload {
                total_len += sr.as_str().len();
            } else {
                total_len += item.py_to_string().len();
            }
        }
        let mut result = String::with_capacity(total_len);
        for (i, item) in items_slice.iter().enumerate() {
            if i > 0 {
                result.push_str(s);
            }
            if let PyObjectPayload::Str(sr) = &item.payload {
                result.push_str(sr.as_str());
            } else {
                result.push_str(&item.py_to_string());
            }
        }
        Ok(PyObject::str_from_utf8_slice(result.as_bytes()))
    });
    str_method!(attrs, "isalpha", s_val, |s: &String,
                                          _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::bool_val(
            !s.is_empty() && s.chars().all(|c| c.is_alphabetic()),
        ))
    });
    str_method!(attrs, "isdigit", s_val, |s: &String,
                                          _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::bool_val(
            !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()),
        ))
    });
    str_method!(attrs, "isalnum", s_val, |s: &String,
                                          _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::bool_val(
            !s.is_empty() && s.chars().all(|c| c.is_alphanumeric()),
        ))
    });
    str_method!(attrs, "isspace", s_val, |s: &String,
                                          _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::bool_val(
            !s.is_empty() && s.chars().all(|c| c.is_whitespace()),
        ))
    });
    str_method!(attrs, "isupper", s_val, |s: &String,
                                          _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::bool_val(
            s.chars().any(|c| c.is_uppercase()) && !s.chars().any(|c| c.is_lowercase()),
        ))
    });
    str_method!(attrs, "islower", s_val, |s: &String,
                                          _args: &[PyObjectRef]|
     -> PyResult<PyObjectRef> {
        Ok(PyObject::bool_val(
            s.chars().any(|c| c.is_lowercase()) && !s.chars().any(|c| c.is_uppercase()),
        ))
    });
}

fn get_user_data(obj: &PyObjectRef, attr: &str) -> PyResult<PyObjectRef> {
    if let PyObjectPayload::Instance(d) = &obj.payload {
        if let Some(v) = d.attrs.read().get(attr) {
            return Ok(v.clone());
        }
    }
    Err(PyException::attribute_error(format!(
        "'{}' object has no attribute '{}'",
        obj.type_name(),
        attr
    )))
}

/// _count_elements(mapping, iterable) — C accelerator for Counter.__init__
fn count_elements(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
