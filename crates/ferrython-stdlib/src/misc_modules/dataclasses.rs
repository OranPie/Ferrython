use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    check_args, make_builtin, make_module, new_fx_hashkey_map, repr_enter, repr_leave, ClassData,
    FxHashKeyMap, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

// ── dataclasses module ──

pub fn create_dataclasses_module() -> PyObjectRef {
    make_module(
        "dataclasses",
        vec![
            ("dataclass", make_builtin(dataclass_decorator)),
            (
                "field",
                make_builtin(|args| {
                    // field(default=..., default_factory=..., compare=..., init=..., repr=..., ...)
                    // kwargs passed as trailing dict by VM
                    let mut compare = true;
                    let mut init = true;
                    let mut repr_flag = true;
                    let mut hash_flag: Option<bool> = None;
                    let mut default_val: Option<PyObjectRef> = None;
                    let mut factory_val: Option<PyObjectRef> = None;

                    if let Some(last) = args.last() {
                        if let PyObjectPayload::Dict(kw_map) = &last.payload {
                            let r = kw_map.read();
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("compare")))
                            {
                                compare = v.is_truthy();
                            }
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("init")))
                            {
                                init = v.is_truthy();
                            }
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("repr")))
                            {
                                repr_flag = v.is_truthy();
                            }
                            if let Some(v) =
                                r.get(&HashableKey::str_key(CompactString::from("hash")))
                            {
                                if !matches!(&v.payload, PyObjectPayload::None) {
                                    hash_flag = Some(v.is_truthy());
                                }
                            }
                            if let Some(f) = r.get(&HashableKey::str_key(CompactString::from(
                                "default_factory",
                            ))) {
                                factory_val = Some(f.clone());
                            }
                            if let Some(d) =
                                r.get(&HashableKey::str_key(CompactString::from("default")))
                            {
                                default_val = Some(d.clone());
                            }
                        }
                    }
                    // Return a field sentinel Module with all metadata
                    let mut attrs = IndexMap::new();
                    attrs.insert(
                        CompactString::from("__field_compare__"),
                        PyObject::bool_val(compare),
                    );
                    attrs.insert(
                        CompactString::from("__field_init__"),
                        PyObject::bool_val(init),
                    );
                    attrs.insert(
                        CompactString::from("__field_repr__"),
                        PyObject::bool_val(repr_flag),
                    );
                    attrs.insert(CompactString::from("repr"), PyObject::bool_val(repr_flag));
                    attrs.insert(CompactString::from("init"), PyObject::bool_val(init));
                    attrs.insert(CompactString::from("compare"), PyObject::bool_val(compare));
                    attrs.insert(
                        CompactString::from("hash"),
                        match hash_flag {
                            Some(v) => PyObject::bool_val(v),
                            None => PyObject::none(),
                        },
                    );
                    attrs.insert(
                        CompactString::from("metadata"),
                        PyObject::dict(IndexMap::new()),
                    );
                    attrs.insert(CompactString::from("kw_only"), PyObject::bool_val(false));
                    if let Some(factory) = factory_val {
                        attrs.insert(CompactString::from("__field_factory__"), factory.clone());
                        attrs.insert(CompactString::from("default_factory"), factory);
                        attrs.insert(CompactString::from("default"), PyObject::none());
                    } else if let Some(default) = default_val {
                        attrs.insert(CompactString::from("__field_default__"), default.clone());
                        attrs.insert(CompactString::from("default"), default);
                        attrs.insert(CompactString::from("default_factory"), PyObject::none());
                    } else {
                        attrs.insert(CompactString::from("default"), PyObject::none());
                        attrs.insert(CompactString::from("default_factory"), PyObject::none());
                    }
                    Ok(PyObject::module_with_attrs(
                        CompactString::from("_field"),
                        attrs,
                    ))
                }),
            ),
            (
                "asdict",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("asdict requires 1 argument"));
                    }
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        // Use __dataclass_fields__ to get fields in order
                        if let Some(class) = inst
                            .attrs
                            .read()
                            .get("__class__")
                            .cloned()
                            .or_else(|| Some(inst.class.clone()))
                        {
                            if let Some(fields) = class.get_attr("__dataclass_fields__") {
                                if let PyObjectPayload::Dict(field_dict) = &fields.payload {
                                    let dict = field_dict.read();
                                    let attrs = inst.attrs.read();
                                    let mut map = IndexMap::new();
                                    for (k, _v) in dict.iter() {
                                        if let HashableKey::Str(name) = k {
                                            if let Some(v) = attrs.get(name.as_str()) {
                                                map.insert(
                                                    HashableKey::str_key(name.to_compact_string()),
                                                    v.clone(),
                                                );
                                            }
                                        }
                                    }
                                    return Ok(PyObject::dict(map));
                                }
                            }
                        }
                        // Fallback: all non-_ attrs
                        let attrs = inst.attrs.read();
                        let mut map = IndexMap::new();
                        for (k, v) in attrs.iter() {
                            if !k.starts_with('_') {
                                map.insert(HashableKey::str_key(k.clone()), v.clone());
                            }
                        }
                        Ok(PyObject::dict(map))
                    } else {
                        Err(PyException::type_error(
                            "asdict() should be called on dataclass instances",
                        ))
                    }
                }),
            ),
            (
                "astuple",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Err(PyException::type_error("astuple requires 1 argument"));
                    }
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        if let Some(class) = inst
                            .attrs
                            .read()
                            .get("__class__")
                            .cloned()
                            .or_else(|| Some(inst.class.clone()))
                        {
                            if let Some(fields) = class.get_attr("__dataclass_fields__") {
                                if let PyObjectPayload::Dict(field_dict) = &fields.payload {
                                    let dict = field_dict.read();
                                    let attrs = inst.attrs.read();
                                    let items: Vec<_> = dict
                                        .keys()
                                        .filter_map(|k| {
                                            if let HashableKey::Str(name) = k {
                                                attrs.get(name.as_str()).cloned()
                                            } else {
                                                None
                                            }
                                        })
                                        .collect();
                                    return Ok(PyObject::tuple(items));
                                }
                            }
                        }
                        let attrs = inst.attrs.read();
                        let items: Vec<_> = attrs.values().cloned().collect();
                        Ok(PyObject::tuple(items))
                    } else {
                        Err(PyException::type_error(
                            "astuple() should be called on dataclass instances",
                        ))
                    }
                }),
            ),
            (
                "fields",
                make_builtin(|args| {
                    // fields(instance_or_class) -> tuple of Field objects
                    if args.is_empty() {
                        return Err(PyException::type_error("fields requires 1 argument"));
                    }
                    let cls = match &args[0].payload {
                        PyObjectPayload::Class(_) => args[0].clone(),
                        PyObjectPayload::Instance(inst) => inst.class.clone(),
                        _ => {
                            return Err(PyException::type_error(
                                "fields() argument must be a dataclass or instance",
                            ))
                        }
                    };
                    if let Some(fields_data) = cls.get_attr("__dataclass_fields__") {
                        if let PyObjectPayload::Dict(field_dict) = &fields_data.payload {
                            let dict = field_dict.read();
                            let field_objs: Vec<PyObjectRef> = dict.values().cloned().collect();
                            return Ok(PyObject::tuple(field_objs));
                        }
                    }
                    Ok(PyObject::tuple(vec![]))
                }),
            ),
            (
                "replace",
                make_builtin(|args| {
                    // replace(instance, **kwargs)
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "replace requires at least 1 argument",
                        ));
                    }
                    let instance = &args[0];
                    if let PyObjectPayload::Instance(inst) = &instance.payload {
                        let cls = inst.class.clone();
                        // Clone all attrs
                        let mut new_attrs: IndexMap<CompactString, PyObjectRef> = inst
                            .attrs
                            .read()
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        // Apply kwargs overrides
                        if args.len() > 1 {
                            if let PyObjectPayload::Dict(kw_map) = &args[1].payload {
                                for (k, v) in kw_map.read().iter() {
                                    if let HashableKey::Str(name) = k {
                                        new_attrs.insert(name.to_compact_string(), v.clone());
                                    }
                                }
                            }
                        }
                        Ok(PyObject::instance_with_attrs(cls, new_attrs))
                    } else {
                        Err(PyException::type_error(
                            "replace() argument must be a dataclass instance",
                        ))
                    }
                }),
            ),
            (
                "is_dataclass",
                make_builtin(|args| {
                    if args.is_empty() {
                        return Ok(PyObject::bool_val(false));
                    }
                    let obj = &args[0];
                    match &obj.payload {
                        PyObjectPayload::Class(_) => Ok(PyObject::bool_val(
                            obj.get_attr("__dataclass_fields__").is_some(),
                        )),
                        PyObjectPayload::Instance(inst) => Ok(PyObject::bool_val(
                            inst.class.get_attr("__dataclass_fields__").is_some(),
                        )),
                        _ => Ok(PyObject::bool_val(false)),
                    }
                }),
            ),
            (
                "make_dataclass",
                make_builtin(|args| {
                    // make_dataclass(cls_name, fields, *, bases=()) -> class
                    if args.is_empty() {
                        return Err(PyException::type_error("make_dataclass requires cls_name"));
                    }
                    let cls_name = args[0].py_to_string();
                    let field_list = if args.len() > 1 {
                        args[1].to_list()?
                    } else {
                        vec![]
                    };
                    let mut ns = IndexMap::new();
                    let mut annotations = IndexMap::new();
                    // Parse field specs: can be "name", ("name", type), or ("name", type, field(...))
                    for f in &field_list {
                        let items = f.to_list().unwrap_or_else(|_| vec![f.clone()]);
                        let name = items.first().map(|v| v.py_to_string()).unwrap_or_default();
                        if name.is_empty() {
                            continue;
                        }
                        annotations.insert(
                            HashableKey::str_key(CompactString::from(name.as_str())),
                            if items.len() > 1 {
                                items[1].clone()
                            } else {
                                PyObject::none()
                            },
                        );
                        // If a field(...) default is provided as 3rd element, set as class attr
                        if items.len() > 2 {
                            ns.insert(CompactString::from(name.as_str()), items[2].clone());
                        }
                    }
                    ns.insert(
                        CompactString::from("__annotations__"),
                        PyObject::dict(annotations),
                    );
                    let cls = PyObject::class(CompactString::from(cls_name.as_str()), vec![], ns);
                    // Apply the dataclass transform to generate __init__, __repr__, __eq__
                    dataclass_apply(&cls, true, false, false, true, false, false)
                }),
            ),
            (
                "FrozenInstanceError",
                PyObject::exception_type(ferrython_core::error::ExceptionKind::AttributeError),
            ),
            (
                "InitVar",
                make_builtin(|args: &[PyObjectRef]| {
                    // InitVar acts as a type marker for dataclass fields
                    let cls =
                        PyObject::class(CompactString::from("InitVar"), vec![], IndexMap::new());
                    let inst = PyObject::instance(cls);
                    if let PyObjectPayload::Instance(ref d) = inst.payload {
                        let mut attrs = d.attrs.write();
                        attrs.insert(CompactString::from("__initvar__"), PyObject::bool_val(true));
                        if !args.is_empty() {
                            attrs.insert(CompactString::from("type"), args[0].clone());
                        }
                    }
                    Ok(inst)
                }),
            ),
        ],
    )
}

fn dataclass_decorator(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // @dataclass() — called with empty parens, return decorator with defaults
    if args.is_empty() {
        return Ok(PyObject::native_closure(
            "dataclass",
            move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Err(PyException::type_error("dataclass requires 1 argument"));
                }
                dataclass_apply(&args[0], true, false, false, true, false, false)
            },
        ));
    }
    let cls = &args[0];

    // If called as @dataclass(eq=True, ...) the first arg is kwargs dict, not a class.
    if !matches!(&cls.payload, PyObjectPayload::Class(_)) {
        let mut eq = true;
        let mut order = false;
        let mut frozen = false;
        let mut repr = true;
        let mut unsafe_hash = false;
        let mut slots = false;
        if let PyObjectPayload::Dict(map) = &cls.payload {
            let m = map.read();
            if let Some(v) = m.get(&HashableKey::str_key(CompactString::from("eq"))) {
                eq = v.is_truthy();
            }
            if let Some(v) = m.get(&HashableKey::str_key(CompactString::from("order"))) {
                order = v.is_truthy();
            }
            if let Some(v) = m.get(&HashableKey::str_key(CompactString::from("frozen"))) {
                frozen = v.is_truthy();
            }
            if let Some(v) = m.get(&HashableKey::str_key(CompactString::from("repr"))) {
                repr = v.is_truthy();
            }
            if let Some(v) = m.get(&HashableKey::str_key(CompactString::from("unsafe_hash"))) {
                unsafe_hash = v.is_truthy();
            }
            if let Some(v) = m.get(&HashableKey::str_key(CompactString::from("slots"))) {
                slots = v.is_truthy();
            }
        }
        return Ok(PyObject::native_closure(
            "dataclass",
            move |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Err(PyException::type_error("dataclass requires 1 argument"));
                }
                dataclass_apply(&args[0], eq, order, frozen, repr, unsafe_hash, slots)
            },
        ));
    }

    dataclass_apply(cls, true, false, false, true, false, false)
}

/// Call a default_factory callable or clone a static default value.
/// Handles NativeFunction, NativeClosure, BuiltinType (dict/list/set/tuple/frozenset),
/// Function (Python lambda/def), and Class (user-defined types).
fn call_factory_or_clone(default: &PyObjectRef) -> PyResult<PyObjectRef> {
    match &default.payload {
        PyObjectPayload::NativeFunction(nf) => (nf.func)(&[]),
        PyObjectPayload::NativeClosure(nc) => (nc.func)(&[]),
        PyObjectPayload::BuiltinType(name) => {
            // Common builtin types: dict() → {}, list() → [], set() → set(), etc.
            match name.as_str() {
                "dict" => Ok(PyObject::dict(IndexMap::new())),
                "list" => Ok(PyObject::list(vec![])),
                "set" => Ok(PyObject::set(IndexMap::new())),
                "tuple" => Ok(PyObject::tuple(vec![])),
                "frozenset" => Ok(PyObject::frozenset(IndexMap::new())),
                "str" => Ok(PyObject::str_val(CompactString::new(""))),
                "int" => Ok(PyObject::int(0)),
                "float" => Ok(PyObject::float(0.0)),
                "bool" => Ok(PyObject::bool_val(false)),
                "bytes" => Ok(PyObject::bytes(vec![])),
                "bytearray" => Ok(PyObject::bytearray(vec![])),
                _ => Ok(default.clone()),
            }
        }
        // BuiltinFunction holds a name string, not callable — skip
        _ => Ok(default.clone()),
    }
}

fn dataclass_apply(
    cls: &PyObjectRef,
    eq: bool,
    order: bool,
    frozen: bool,
    repr: bool,
    unsafe_hash: bool,
    slots: bool,
) -> PyResult<PyObjectRef> {
    // Get annotations to discover fields — walk MRO for inherited dataclass fields
    let mut field_names: Vec<CompactString> = Vec::new();
    let mut field_defaults: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    let mut field_types: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    let mut compare_fields: Vec<CompactString> = Vec::new();
    let mut init_fields: Vec<CompactString> = Vec::new();
    let mut repr_fields: Vec<CompactString> = Vec::new();

    if let PyObjectPayload::Class(cd) = &cls.payload {
        // Collect fields from base classes first (MRO order), then own class
        let mut all_classes: Vec<PyObjectRef> = cd.bases.iter().rev().cloned().collect();
        all_classes.push(cls.clone());

        for base_cls in &all_classes {
            if let PyObjectPayload::Class(bcd) = &base_cls.payload {
                let ns = bcd.namespace.read();
                if let Some(annotations) = ns.get("__annotations__") {
                    if let PyObjectPayload::Dict(ann_map) = &annotations.payload {
                        for (k, v) in ann_map.read().iter() {
                            if let HashableKey::Str(name) = k {
                                let name = name.to_compact_string();
                                if !field_names.contains(&name) {
                                    field_names.push(name.clone());
                                }
                                field_types.insert(name.clone(), v.clone());
                                let mut compare = true;
                                let mut init = true;
                                let mut field_repr = true;

                                if let Some(default) = ns.get(name.as_str()) {
                                    if let PyObjectPayload::Module(md) = &default.payload {
                                        let mod_attrs = md.attrs.read();
                                        if let Some(cmp_flag) = mod_attrs.get("__field_compare__") {
                                            compare = cmp_flag.is_truthy();
                                        }
                                        if let Some(init_flag) = mod_attrs.get("__field_init__") {
                                            init = init_flag.is_truthy();
                                        }
                                        if let Some(repr_flag) = mod_attrs.get("__field_repr__") {
                                            field_repr = repr_flag.is_truthy();
                                        }
                                        if let Some(factory) = mod_attrs.get("__field_factory__") {
                                            field_defaults.insert(name.clone(), factory.clone());
                                        } else if let Some(default_val) =
                                            mod_attrs.get("__field_default__")
                                        {
                                            field_defaults
                                                .insert(name.clone(), default_val.clone());
                                        }
                                    } else {
                                        field_defaults.insert(name.clone(), default.clone());
                                    }
                                }
                                if compare {
                                    compare_fields.push(name.clone());
                                }
                                if init {
                                    init_fields.push(name.clone());
                                }
                                if field_repr {
                                    repr_fields.push(name.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Store __dataclass_fields__ as dict mapping field name → Field-like object
    // CPython stores Field objects; we use Module objects with the same key attributes.
    let mut fields_dict: FxHashKeyMap = new_fx_hashkey_map();
    for name in &field_names {
        let has_default = field_defaults.contains_key(name.as_str());
        let default_val = field_defaults
            .get(name.as_str())
            .cloned()
            .unwrap_or_else(PyObject::none);
        let init_flag = init_fields.contains(name);
        let compare_flag = compare_fields.contains(name);
        let repr_flag = repr_fields.contains(name);
        let type_val = field_types
            .get(name)
            .cloned()
            .unwrap_or_else(PyObject::none);
        // Create a Field-like object with standard dataclass Field attributes
        let mut field_attrs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
        field_attrs.insert(CompactString::from("name"), PyObject::str_val(name.clone()));
        field_attrs.insert(CompactString::from("type"), type_val);
        field_attrs.insert(CompactString::from("default"), default_val.clone());
        field_attrs.insert(CompactString::from("default_factory"), PyObject::none());
        field_attrs.insert(
            CompactString::from("__has_default__"),
            PyObject::bool_val(has_default),
        );
        field_attrs.insert(CompactString::from("init"), PyObject::bool_val(init_flag));
        field_attrs.insert(CompactString::from("repr"), PyObject::bool_val(repr_flag));
        field_attrs.insert(
            CompactString::from("compare"),
            PyObject::bool_val(compare_flag),
        );
        field_attrs.insert(CompactString::from("hash"), PyObject::none());
        field_attrs.insert(
            CompactString::from("metadata"),
            PyObject::dict(IndexMap::new()),
        );
        field_attrs.insert(CompactString::from("kw_only"), PyObject::bool_val(false));
        field_attrs.insert(
            CompactString::from("_field_type"),
            PyObject::str_val(CompactString::from("_FIELD")),
        );
        let field_obj = PyObject::module_with_attrs(CompactString::from("Field"), field_attrs);
        fields_dict.insert(HashableKey::str_key(name.clone()), field_obj);
    }

    if let PyObjectPayload::Class(cd) = &cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(
            CompactString::from("__dataclass_fields__"),
            PyObject::dict(fields_dict),
        );
        ns.insert(
            CompactString::from("__dataclass__"),
            PyObject::bool_val(true),
        );

        // slots=True: add __slots__ tuple and restrict attribute assignment
        if slots {
            let slot_names: Vec<PyObjectRef> = field_names
                .iter()
                .map(|n| PyObject::str_val(n.clone()))
                .collect();
            ns.insert(
                CompactString::from("__slots__"),
                PyObject::tuple(slot_names),
            );
            // Add __setattr__ that restricts to declared slots + dataclass internals
            let allowed: Vec<CompactString> = field_names.clone();
            ns.insert(
                CompactString::from("__setattr__"),
                PyObject::native_closure("__setattr__", move |args: &[PyObjectRef]| {
                    if args.len() < 3 {
                        return Err(PyException::type_error("__setattr__ requires 3 arguments"));
                    }
                    let attr_name = args[1].py_to_string();
                    if !allowed.iter().any(|f| f.as_str() == attr_name)
                        && !attr_name.starts_with("__")
                    {
                        return Err(PyException::attribute_error(format!(
                            "'{}' object has no attribute '{}'",
                            "object", attr_name
                        )));
                    }
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        inst.attrs
                            .write()
                            .insert(CompactString::from(attr_name), args[2].clone());
                    }
                    Ok(PyObject::none())
                }),
            );
        }

        // Generate __init__ for all dataclasses (frozen and non-frozen),
        // but only if the class doesn't already define __init__ (CPython _set_new_attribute behavior)
        if !ns.contains_key("__init__") {
            let init_field_names = init_fields.clone();
            let init_field_defaults = field_defaults.clone();
            let cls_for_init = cls.clone();
            ns.insert(
                CompactString::from("__init__"),
                PyObject::native_closure("__init__", move |args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error("__init__ requires self"));
                    }
                    let self_obj = &args[0];
                    // Detect trailing kwargs dict (VM packs kwargs as last arg for NativeClosure).
                    // Only check when arg count doesn't match field count exactly —
                    // if we have exactly the right number of positional args, they ARE positional
                    // (avoids treating a user dict arg like {"a":1} as kwargs).
                    let n_args_excl_self = args.len() - 1;
                    let trailing_kwargs: Option<FxHashKeyMap> =
                        if n_args_excl_self != init_field_names.len() && args.len() >= 2 {
                            if let PyObjectPayload::Dict(map) = &args[args.len() - 1].payload {
                                Some(map.read().clone())
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                    let pos_end = if trailing_kwargs.is_some() {
                        args.len() - 1
                    } else {
                        args.len()
                    };
                    if let PyObjectPayload::Instance(inst) = &self_obj.payload {
                        let mut attrs = inst.attrs.write();
                        let mut pos = 1; // skip self
                        for fname in &init_field_names {
                            // Try positional arg first, then kwargs, then defaults
                            let value = if pos < pos_end {
                                args[pos].clone()
                            } else if let Some(ref kw) = trailing_kwargs {
                                if let Some(v) = kw.get(&HashableKey::str_key(fname.clone())) {
                                    v.clone()
                                } else if let Some(default) =
                                    init_field_defaults.get(fname.as_str())
                                {
                                    call_factory_or_clone(default)?
                                } else {
                                    return Err(PyException::type_error(format!(
                                        "__init__() missing required argument: '{}'",
                                        fname
                                    )));
                                }
                            } else if let Some(default) = init_field_defaults.get(fname.as_str()) {
                                call_factory_or_clone(default)?
                            } else {
                                return Err(PyException::type_error(format!(
                                    "__init__() missing required argument: '{}'",
                                    fname
                                )));
                            };
                            attrs.insert(fname.clone(), value);
                            pos += 1;
                        }
                    }
                    // Call __post_init__ if defined (CPython does this in generated __init__)
                    if let PyObjectPayload::Class(cd) = &cls_for_init.payload {
                        if let Some(post_init) = cd.namespace.read().get("__post_init__") {
                            ferrython_core::error::request_vm_call(
                                post_init.clone(),
                                vec![self_obj.clone()],
                            );
                        }
                    }
                    Ok(PyObject::none())
                }),
            );
        }

        // Generate __setattr__ and __delattr__ for frozen=True
        if frozen {
            ns.insert(
                CompactString::from("__dataclass_frozen__"),
                PyObject::bool_val(true),
            );

            // Raise FrozenInstanceError on frozen field assignment, allow other attrs
            let frozen_field_names: Vec<CompactString> = field_names.clone();
            ns.insert(
                CompactString::from("__setattr__"),
                PyObject::native_closure("__setattr__", move |args: &[PyObjectRef]| {
                    // args: self, name, value
                    if args.len() < 3 {
                        return Err(PyException::type_error("__setattr__ requires 3 arguments"));
                    }
                    let attr_name = args[1].py_to_string();
                    if frozen_field_names.iter().any(|f| f.as_str() == attr_name) {
                        return Err(PyException::attribute_error(format!(
                            "cannot assign to field '{}'",
                            attr_name
                        )));
                    }
                    // Allow non-field attributes (e.g., subclass __init__ setting new attrs)
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        inst.attrs
                            .write()
                            .insert(CompactString::from(attr_name), args[2].clone());
                    }
                    Ok(PyObject::none())
                }),
            );
            let frozen_del_names: Vec<CompactString> = field_names.clone();
            ns.insert(
                CompactString::from("__delattr__"),
                PyObject::native_closure("__delattr__", move |args: &[PyObjectRef]| {
                    if args.len() < 2 {
                        return Err(PyException::type_error("__delattr__ requires 2 arguments"));
                    }
                    let attr_name = args[1].py_to_string();
                    if frozen_del_names.iter().any(|f| f.as_str() == attr_name) {
                        return Err(PyException::attribute_error(format!(
                            "cannot delete field '{}'",
                            attr_name
                        )));
                    }
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        inst.attrs.write().swap_remove(attr_name.as_str());
                    }
                    Ok(PyObject::none())
                }),
            );
        }

        // Generate __repr__ if repr=True (default)
        if repr {
            let fields_for_repr = repr_fields.clone();
            let cls_ref = cls.clone();
            ns.insert(
                CompactString::from("__repr__"),
                PyObject::native_closure("__repr__", move |args: &[PyObjectRef]| {
                    check_args("__repr__", args, 1)?;
                    let ptr = PyObjectRef::as_ptr(&args[0]) as usize;
                    if !repr_enter(ptr) {
                        return Ok(PyObject::str_val(CompactString::from("...")));
                    }
                    let cls_name = if let PyObjectPayload::Class(cd) = &cls_ref.payload {
                        cd.name.clone()
                    } else {
                        CompactString::from("???")
                    };
                    let mut parts = Vec::new();
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        let attrs = inst.attrs.read();
                        for f in &fields_for_repr {
                            let val = attrs
                                .get(f.as_str())
                                .cloned()
                                .unwrap_or_else(PyObject::none);
                            let val_repr = val.repr();
                            parts.push(format!("{}={}", f, val_repr));
                        }
                    }
                    let rendered = PyObject::str_val(CompactString::from(format!(
                        "{}({})",
                        cls_name,
                        parts.join(", ")
                    )));
                    repr_leave(ptr);
                    Ok(rendered)
                }),
            );
        }

        // Generate __eq__ if eq=True (default)
        if eq {
            let fields_for_eq = compare_fields.clone();
            ns.insert(
                CompactString::from("__eq__"),
                PyObject::native_closure("__eq__", move |args: &[PyObjectRef]| {
                    check_args("__eq__", args, 2)?;
                    let (a, b) = (&args[0], &args[1]);
                    // Must be same type
                    if !same_class(a, b) {
                        return Ok(PyObject::not_implemented());
                    }
                    let tup_a = extract_compare_tuple(a, &fields_for_eq);
                    let tup_b = extract_compare_tuple(b, &fields_for_eq);
                    tup_a.compare(&tup_b, ferrython_core::object::CompareOp::Eq)
                }),
            );

            let fields_for_ne = compare_fields.clone();
            ns.insert(
                CompactString::from("__ne__"),
                PyObject::native_closure("__ne__", move |args: &[PyObjectRef]| {
                    check_args("__ne__", args, 2)?;
                    let (a, b) = (&args[0], &args[1]);
                    if !same_class(a, b) {
                        return Ok(PyObject::not_implemented());
                    }
                    let tup_a = extract_compare_tuple(a, &fields_for_ne);
                    let tup_b = extract_compare_tuple(b, &fields_for_ne);
                    tup_a.compare(&tup_b, ferrython_core::object::CompareOp::Ne)
                }),
            );
        }

        // Generate __hash__
        // CPython: if eq=True and frozen=True, generate __hash__
        //          if eq=True and frozen=False, set __hash__ = None (unhashable)
        //          if unsafe_hash=True, always generate __hash__
        if unsafe_hash || (eq && frozen) {
            let fields_for_hash = compare_fields.clone();
            ns.insert(
                CompactString::from("__hash__"),
                PyObject::native_closure("__hash__", move |args: &[PyObjectRef]| {
                    check_args("__hash__", args, 1)?;
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::{Hash, Hasher};
                    if let PyObjectPayload::Instance(inst) = &args[0].payload {
                        let attrs = inst.attrs.read();
                        let vals: Vec<PyObjectRef> = fields_for_hash
                            .iter()
                            .map(|f| {
                                attrs
                                    .get(f.as_str())
                                    .cloned()
                                    .unwrap_or_else(PyObject::none)
                            })
                            .collect();
                        let tup = PyObject::tuple(vals);
                        let hk = tup.to_hashable_key()?;
                        let mut hasher = DefaultHasher::new();
                        hk.hash(&mut hasher);
                        Ok(PyObject::int(hasher.finish() as i64))
                    } else {
                        Ok(PyObject::int(0))
                    }
                }),
            );
        } else if eq {
            // eq=True, frozen=False → unhashable (like CPython)
            ns.insert(CompactString::from("__hash__"), PyObject::none());
        }

        // Generate ordering methods if order=True
        if order {
            let fields_for_lt = compare_fields.clone();
            ns.insert(
                CompactString::from("__lt__"),
                PyObject::native_closure("__lt__", move |args: &[PyObjectRef]| {
                    check_args("__lt__", args, 2)?;
                    let (a, b) = (&args[0], &args[1]);
                    let tup_a = extract_compare_tuple(a, &fields_for_lt);
                    let tup_b = extract_compare_tuple(b, &fields_for_lt);
                    tup_a.compare(&tup_b, ferrython_core::object::CompareOp::Lt)
                }),
            );

            let fields_for_le = compare_fields.clone();
            ns.insert(
                CompactString::from("__le__"),
                PyObject::native_closure("__le__", move |args: &[PyObjectRef]| {
                    check_args("__le__", args, 2)?;
                    let (a, b) = (&args[0], &args[1]);
                    let tup_a = extract_compare_tuple(a, &fields_for_le);
                    let tup_b = extract_compare_tuple(b, &fields_for_le);
                    tup_a.compare(&tup_b, ferrython_core::object::CompareOp::Le)
                }),
            );

            let fields_for_gt = compare_fields.clone();
            ns.insert(
                CompactString::from("__gt__"),
                PyObject::native_closure("__gt__", move |args: &[PyObjectRef]| {
                    check_args("__gt__", args, 2)?;
                    let (a, b) = (&args[0], &args[1]);
                    let tup_a = extract_compare_tuple(a, &fields_for_gt);
                    let tup_b = extract_compare_tuple(b, &fields_for_gt);
                    tup_a.compare(&tup_b, ferrython_core::object::CompareOp::Gt)
                }),
            );

            let fields_for_ge = compare_fields.clone();
            ns.insert(
                CompactString::from("__ge__"),
                PyObject::native_closure("__ge__", move |args: &[PyObjectRef]| {
                    check_args("__ge__", args, 2)?;
                    let (a, b) = (&args[0], &args[1]);
                    let tup_a = extract_compare_tuple(a, &fields_for_ge);
                    let tup_b = extract_compare_tuple(b, &fields_for_ge);
                    tup_a.compare(&tup_b, ferrython_core::object::CompareOp::Ge)
                }),
            );
        }
    }

    // Invalidate vtable so the inline class instantiation uses namespace lookup.
    // The decorator added __init__ (and possibly __eq__/__repr__/etc.) AFTER class creation,
    // so the vtable is stale and must be cleared.
    if let PyObjectPayload::Class(cd) = &cls.payload {
        cd.invalidate_cache();
        // Update has_setattr flag if frozen (decorator added __setattr__ after creation)
        if frozen {
            // Safety: we have unique logical ownership during class creation; no other
            // thread or Rc observer reads has_setattr concurrently with this write.
            unsafe {
                let cd_ptr = &**cd as *const ClassData as *mut ClassData;
                (*cd_ptr).has_setattr = true;
                // Also update cached instance_flags so new instances respect __setattr__
                (*cd_ptr).instance_flags |= ferrython_core::object::CLASS_FLAG_HAS_SETATTR;
            }
        }
    }

    Ok(cls.clone())
}

/// Extract a comparison tuple from a dataclass instance for ordering.
fn extract_compare_tuple(obj: &PyObjectRef, fields: &[CompactString]) -> PyObjectRef {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        let attrs = inst.attrs.read();
        let vals: Vec<PyObjectRef> = fields
            .iter()
            .map(|f| {
                attrs
                    .get(f.as_str())
                    .cloned()
                    .unwrap_or_else(PyObject::none)
            })
            .collect();
        PyObject::tuple(vals)
    } else {
        PyObject::tuple(vec![])
    }
}

/// Check if two instances share the same class (by Arc pointer identity).
fn same_class(a: &PyObjectRef, b: &PyObjectRef) -> bool {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Instance(ia), PyObjectPayload::Instance(ib)) => {
            PyObjectRef::ptr_eq(&ia.class, &ib.class)
        }
        _ => false,
    }
}
