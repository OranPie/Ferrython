use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyResult};
use ferrython_core::object::{
    ExceptionInstanceData, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;

pub(super) fn hashable_key_to_pyobj(k: &HashableKey) -> PyObjectRef {
    match k {
        HashableKey::Str(s) => PyObject::str_val(s.to_compact_string()),
        HashableKey::Int(n) => PyObject::int(n.to_i64().unwrap_or(0)),
        HashableKey::Float(f) => PyObject::float(f.0),
        HashableKey::Bool(b) => PyObject::bool_val(*b),
        HashableKey::Identity(_, obj) | HashableKey::Custom { object: obj, .. } => {
            if let PyObjectPayload::Class(cd) = &obj.payload {
                PyObject::str_val(cd.name.clone())
            } else {
                PyObject::str_val(CompactString::from(format!("{:?}", k)))
            }
        }
        _ => PyObject::str_val(CompactString::from(format!("{:?}", k))),
    }
}

fn decimal_signal_from_name(obj: &PyObjectRef, name: &str) -> Option<PyObjectRef> {
    let PyObjectPayload::Instance(inst) = &obj.payload else {
        return None;
    };
    let PyObjectPayload::Class(cd) = &inst.class.payload else {
        return None;
    };
    if cd.name.as_str() != "Context" {
        return None;
    }
    let module = inst.class.get_attr("__module__")?;
    let module_attrs = match &module.payload {
        PyObjectPayload::Module(data) => data.attrs.read().get(name).cloned(),
        _ => None,
    };
    module_attrs.filter(|value| matches!(&value.payload, PyObjectPayload::Class(_)))
}

fn decimal_signal_key_from_existing_state(obj: &PyObjectRef, name: &str) -> Option<HashableKey> {
    let PyObjectPayload::Instance(inst) = &obj.payload else {
        return None;
    };
    for attr_name in ["flags", "traps"] {
        let Some(existing) = inst.attrs.read().get(attr_name).cloned() else {
            continue;
        };
        let PyObjectPayload::Dict(map) = &existing.payload else {
            continue;
        };
        for key in map.read().keys() {
            let signal = match key {
                HashableKey::Identity(_, signal) => Some(signal),
                HashableKey::Custom { object, .. } => Some(object),
                _ => None,
            };
            if let Some(signal) = signal {
                if let PyObjectPayload::Class(cd) = &signal.payload {
                    if cd.name.as_str() == name {
                        return Some(key.clone());
                    }
                }
            }
        }
    }
    let PyObjectPayload::Instance(inst) = &obj.payload else {
        return None;
    };
    let module_name = inst
        .class
        .get_attr("__module__")
        .map(|module| module.py_to_string())
        .unwrap_or_else(|| "decimal".to_string());
    let module = lookup_loaded_module(&module_name).or_else(|| crate::load_module(&module_name))?;
    let default_context = module.get_attr("DefaultContext")?;
    for attr_name in ["flags", "traps"] {
        let Some(existing) = default_context.get_attr(attr_name) else {
            continue;
        };
        let PyObjectPayload::Dict(map) = &existing.payload else {
            continue;
        };
        for key in map.read().keys() {
            let signal = match key {
                HashableKey::Identity(_, signal) => Some(signal),
                HashableKey::Custom { object, .. } => Some(object),
                _ => None,
            };
            if let Some(signal) = signal {
                if let PyObjectPayload::Class(cd) = &signal.payload {
                    if cd.name.as_str() == name {
                        return Some(key.clone());
                    }
                }
            }
        }
    }
    None
}

fn lookup_loaded_module(name: &str) -> Option<PyObjectRef> {
    let sys = crate::get_current_sys_module()?;
    let modules = sys.get_attr("modules")?;
    let PyObjectPayload::Dict(map) = &modules.payload else {
        return None;
    };
    map.read()
        .get(&HashableKey::str_key(CompactString::from(name)))
        .cloned()
        .filter(|obj| !matches!(&obj.payload, PyObjectPayload::None))
}

fn normalize_decimal_signal_dict(
    obj: &PyObjectRef,
    name: &str,
    value: &PyObjectRef,
) -> PyObjectRef {
    if !matches!(name, "flags" | "traps") {
        return value.clone();
    }
    let PyObjectPayload::Dict(map) = &value.payload else {
        return value.clone();
    };
    let mut normalized = IndexMap::new();
    for (key, item) in map.read().iter() {
        let normalized_key = match key {
            HashableKey::Str(s) => decimal_signal_key_from_existing_state(obj, s.as_str())
                .or_else(|| {
                    decimal_signal_from_name(obj, s.as_str())
                        .and_then(|signal| signal.to_hashable_key().ok())
                })
                .unwrap_or_else(|| key.clone()),
            _ => key.clone(),
        };
        normalized.insert(normalized_key, item.clone());
    }
    PyObject::dict(normalized)
}

pub(super) fn format_float_repr(f: f64) -> String {
    if f.is_nan() {
        return "nan".to_string();
    }
    if f.is_infinite() {
        return if f > 0.0 {
            "inf".to_string()
        } else {
            "-inf".to_string()
        };
    }
    let s = format!("{}", f);
    if s.contains('.') || s.contains('e') || s.contains('E') {
        s
    } else {
        format!("{}.0", s)
    }
}

pub(super) fn operator_reduce_target(name: &str) -> Option<&'static str> {
    match name {
        "operator.attrgetter" => Some("attrgetter"),
        "operator.itemgetter" => Some("itemgetter"),
        "operator.methodcaller" => Some("methodcaller"),
        _ => None,
    }
}

pub(super) fn pickle_exception_instance(
    kind: ExceptionKind,
    args: Vec<PyObjectRef>,
) -> PyObjectRef {
    let message = args
        .first()
        .map(|arg| CompactString::from(arg.py_to_string()))
        .unwrap_or_else(|| CompactString::from(""));
    let inst = PyObject::exception_instance_with_args(kind, message, args.clone());

    if kind.is_subclass_of(&ExceptionKind::SyntaxError) {
        if let PyObjectPayload::ExceptionInstance(ei) = &inst.payload {
            let mut attrs = ei.ensure_attrs().write();
            attrs.insert(
                CompactString::from("msg"),
                args.first().cloned().unwrap_or_else(PyObject::none),
            );
            attrs.insert(CompactString::from("filename"), PyObject::none());
            attrs.insert(CompactString::from("lineno"), PyObject::none());
            attrs.insert(CompactString::from("offset"), PyObject::none());
            attrs.insert(CompactString::from("text"), PyObject::none());
            attrs.insert(CompactString::from("print_file_and_line"), PyObject::none());
            if args.len() == 2 {
                if let PyObjectPayload::Tuple(items) = &args[1].payload {
                    if items.len() >= 4 {
                        attrs.insert(CompactString::from("filename"), items[0].clone());
                        attrs.insert(CompactString::from("lineno"), items[1].clone());
                        attrs.insert(CompactString::from("offset"), items[2].clone());
                        attrs.insert(CompactString::from("text"), items[3].clone());
                    }
                }
            }
        }
    }

    if kind.is_subclass_of(&ExceptionKind::OSError) && args.len() >= 2 {
        if let PyObjectPayload::ExceptionInstance(ei) = &inst.payload {
            let mut attrs = ei.ensure_attrs().write();
            if args.len() >= 3 {
                attrs.insert(
                    CompactString::from("args"),
                    PyObject::tuple(vec![args[0].clone(), args[1].clone()]),
                );
            }
            attrs.insert(CompactString::from("errno"), args[0].clone());
            attrs.insert(CompactString::from("strerror"), args[1].clone());
            attrs.insert(
                CompactString::from("filename"),
                args.get(2).cloned().unwrap_or_else(PyObject::none),
            );
            attrs.insert(
                CompactString::from("filename2"),
                args.get(4).cloned().unwrap_or_else(PyObject::none),
            );
            if kind == ExceptionKind::BlockingIOError
                && args.len() >= 3
                && args[2].as_int().is_some()
            {
                attrs.insert(CompactString::from("characters_written"), args[2].clone());
            }
        }
    }

    match kind {
        ExceptionKind::UnicodeEncodeError | ExceptionKind::UnicodeDecodeError
            if args.len() >= 5 =>
        {
            if let PyObjectPayload::ExceptionInstance(ei) = &inst.payload {
                let mut attrs = ei.ensure_attrs().write();
                attrs.insert(CompactString::from("encoding"), args[0].clone());
                let object = if kind == ExceptionKind::UnicodeDecodeError {
                    match &args[1].payload {
                        PyObjectPayload::ByteArray(bytes) => PyObject::bytes((**bytes).clone()),
                        _ => args[1].clone(),
                    }
                } else {
                    args[1].clone()
                };
                attrs.insert(CompactString::from("object"), object);
                attrs.insert(CompactString::from("start"), args[2].clone());
                attrs.insert(CompactString::from("end"), args[3].clone());
                attrs.insert(CompactString::from("reason"), args[4].clone());
            }
        }
        ExceptionKind::UnicodeTranslateError if args.len() >= 4 => {
            if let PyObjectPayload::ExceptionInstance(ei) = &inst.payload {
                let mut attrs = ei.ensure_attrs().write();
                attrs.insert(CompactString::from("object"), args[0].clone());
                attrs.insert(CompactString::from("start"), args[1].clone());
                attrs.insert(CompactString::from("end"), args[2].clone());
                attrs.insert(CompactString::from("reason"), args[3].clone());
            }
        }
        _ => {}
    }

    if kind.is_subclass_of(&ExceptionKind::ImportError) {
        if let PyObjectPayload::ExceptionInstance(ei) = &inst.payload {
            let mut attrs = ei.ensure_attrs().write();
            attrs.insert(CompactString::from("args"), PyObject::tuple(args.clone()));
            attrs.insert(
                CompactString::from("msg"),
                args.first().cloned().unwrap_or_else(PyObject::none),
            );
            attrs.insert(CompactString::from("name"), PyObject::none());
            attrs.insert(CompactString::from("path"), PyObject::none());
        }
    }

    inst
}

pub(super) fn exception_pickle_state(ei: &ExceptionInstanceData) -> Option<PyObjectRef> {
    if !ei.kind.is_subclass_of(&ExceptionKind::ImportError) {
        return None;
    }

    let attrs = ei.get_attrs()?;
    let attrs = attrs.read();
    let mut pairs = Vec::new();
    for key in ["name", "path"] {
        if let Some(value) = attrs.get(key) {
            if !matches!(value.payload, PyObjectPayload::None) {
                pairs.push((PyObject::str_val(CompactString::from(key)), value.clone()));
            }
        }
    }
    if pairs.is_empty() {
        None
    } else {
        Some(PyObject::dict_from_pairs(pairs))
    }
}

pub(super) fn pkl_apply_state(obj: &PyObjectRef, state: &PyObjectRef) -> PyResult<()> {
    if let PyObjectPayload::Instance(inst) = &obj.payload {
        if matches!(&inst.class.payload, PyObjectPayload::Class(cd)
            if cd.name.as_str() == "partial"
                || cd.mro.iter().any(|base| matches!(&base.payload, PyObjectPayload::Class(base_cd)
                    if base_cd.name.as_str() == "partial")))
        {
            if let PyObjectPayload::Tuple(items) = &state.payload {
                if items.len() == 4 {
                    let mut attrs = inst.attrs.write();
                    attrs.clear();
                    attrs.insert(CompactString::from("func"), items[0].clone());
                    attrs.insert(CompactString::from("args"), items[1].clone());
                    attrs.insert(
                        CompactString::from("keywords"),
                        if matches!(items[2].payload, PyObjectPayload::None) {
                            PyObject::dict(IndexMap::new())
                        } else {
                            items[2].clone()
                        },
                    );
                    if let PyObjectPayload::Dict(namespace) = &items[3].payload {
                        for (key, value) in namespace.read().iter() {
                            if let HashableKey::Str(name) = key {
                                attrs.insert(name.to_compact_string(), value.clone());
                            }
                        }
                    }
                    return Ok(());
                }
            }
        }
    }

    let PyObjectPayload::Dict(map) = &state.payload else {
        return Ok(());
    };

    if let PyObjectPayload::Instance(inst) = &obj.payload {
        let map_r = map.read();
        let has_deque_storage = map_r
            .get(&HashableKey::str_key(CompactString::from("_data")))
            .is_some();
        let has_deque_marker = map_r
            .get(&HashableKey::str_key(CompactString::from("__deque__")))
            .is_some();
        if has_deque_storage || has_deque_marker {
            let restored_items = map_r
                .get(&HashableKey::str_key(CompactString::from("_data")))
                .and_then(|value| value.to_list().ok())
                .unwrap_or_default();
            let restored_maxlen = map_r
                .get(&HashableKey::str_key(CompactString::from("__maxlen__")))
                .cloned()
                .unwrap_or_else(PyObject::none);
            let storage = PyObject::deque_storage(restored_items);
            let mut attrs = inst.attrs.write();
            attrs.insert(CompactString::from("__deque__"), PyObject::bool_val(true));
            attrs.insert(CompactString::from("_data"), storage.clone());
            attrs.insert(CompactString::from("__builtin_value__"), storage);
            attrs.insert(CompactString::from("__maxlen__"), restored_maxlen);
            for (key, value) in map_r.iter() {
                let HashableKey::Str(name) = key else {
                    continue;
                };
                if matches!(
                    name.as_str(),
                    "__deque__" | "_data" | "__builtin_value__" | "__maxlen__"
                ) {
                    continue;
                }
                attrs.insert(name.to_compact_string(), value.clone());
            }
            return Ok(());
        }
        if matches!(&inst.class.payload, PyObjectPayload::Class(cd) if cd.name.as_str() == "Counter")
        {
            if let Some(dst) = inst.dict_storage.as_ref() {
                let mut storage = dst.write();
                for (key, value) in map.read().iter() {
                    if let HashableKey::Str(name) = key {
                        if name.as_str() == "__counter_kwargs__" {
                            continue;
                        }
                    }
                    storage.insert(key.clone(), value.clone());
                }
            }
            return Ok(());
        }
    }

    for (key, value) in map.read().iter() {
        let HashableKey::Str(name) = key else {
            continue;
        };
        match &obj.payload {
            PyObjectPayload::Instance(inst) => {
                let value = normalize_decimal_signal_dict(obj, name.as_str(), value);
                inst.attrs.write().insert(name.to_compact_string(), value);
            }
            PyObjectPayload::ExceptionInstance(ei) => {
                ei.ensure_attrs()
                    .write()
                    .insert(name.to_compact_string(), value.clone());
            }
            _ => {}
        }
    }
    Ok(())
}
