//! `collections.namedtuple` implementation and pickle rebuild helpers.

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::{hash_key_like_python, HashableKey};
use indexmap::IndexMap;
use std::cell::RefCell;

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

pub(super) fn namedtuple_field_class() -> PyObjectRef {
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

pub(super) fn collections_namedtuple(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
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
