use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::helpers::call_callable;
use ferrython_core::object::{
    make_builtin, make_module, FxBuildHasher, PyCell, PyObject, PyObjectMethods, PyObjectPayload,
    PyObjectRef,
};
use ferrython_core::types::{hash_key_like_python, HashableKey};
use indexmap::IndexMap;
use std::cell::RefCell;
use std::rc::Rc;

use super::chainmap::make_chainmap_class;
use super::deque::collections_deque;
use super::user_types::{make_user_dict_class, make_user_list_class, make_user_string_class};

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

fn make_defaultdict_class() -> PyObjectRef {
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
