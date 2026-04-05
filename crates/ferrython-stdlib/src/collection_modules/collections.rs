use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    InstanceData,
    make_module, make_builtin,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

pub fn create_collections_module() -> PyObjectRef {
    make_module("collections", vec![
        ("OrderedDict", make_builtin(collections_ordered_dict)),
        ("defaultdict", make_builtin(collections_defaultdict)),
        ("Counter", make_builtin(collections_counter)),
        ("namedtuple", make_builtin(collections_namedtuple)),
        ("deque", make_builtin(collections_deque)),
        ("most_common", make_builtin(collections_most_common)),
        ("ChainMap", PyObject::native_function("ChainMap", collections_chainmap)),
    ])
}

fn collections_ordered_dict(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // OrderedDict is just a regular dict in Python 3.7+
    if args.is_empty() {
        Ok(PyObject::dict_from_pairs(vec![]))
    } else {
        let items = args[0].to_list()?;
        let mut pairs = Vec::new();
        for item in items {
            if let PyObjectPayload::Tuple(t) = &item.payload {
                if t.len() == 2 {
                    pairs.push((t[0].clone(), t[1].clone()));
                }
            }
        }
        Ok(PyObject::dict_from_pairs(pairs))
    }
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
            HashableKey::Str(CompactString::from("__defaultdict_factory__")),
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
    let factory_key = HashableKey::Str(CompactString::from("__defaultdict_factory__"));
    let counter_marker = HashableKey::Str(CompactString::from("__counter__"));
    
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
            if !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__") {
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

/// Standalone most_common(counter_dict, n?) — also available as Counter.most_common()
fn collections_most_common(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("most_common() requires a Counter argument"));
    }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let r = map.read();
        let mut pairs: Vec<(HashableKey, i64)> = r.iter()
            .filter(|(k, _)| !matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__"))
            .map(|(k, v)| (k.clone(), v.as_int().unwrap_or(0)))
            .collect();
        pairs.sort_by(|a, b| b.1.cmp(&a.1));
        let n = if args.len() > 1 { args[1].as_int().unwrap_or(pairs.len() as i64) as usize } else { pairs.len() };
        let result: Vec<PyObjectRef> = pairs.into_iter().take(n)
            .map(|(k, v)| PyObject::tuple(vec![k.to_object(), PyObject::int(v)]))
            .collect();
        Ok(PyObject::list(result))
    } else {
        Err(PyException::type_error("most_common() argument must be a Counter"))
    }
}

fn collections_namedtuple(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("namedtuple requires typename and field_names"));
    }
    let typename = args[0].py_to_string();
    
    // Parse field names
    let field_names: Vec<CompactString> = match &args[1].payload {
        PyObjectPayload::Str(s) => {
            // "x y" or "x, y" style
            s.replace(',', " ").split_whitespace()
                .map(|s| CompactString::from(s))
                .collect()
        }
        PyObjectPayload::List(_) => {
            args[1].to_list()?.iter().map(|i| CompactString::from(i.py_to_string())).collect()
        }
        PyObjectPayload::Tuple(items) => {
            items.iter().map(|i| CompactString::from(i.py_to_string())).collect()
        }
        _ => {
            args[1].to_list()?.iter().map(|i| CompactString::from(i.py_to_string())).collect()
        }
    };
    
    // Create a class with namespace containing field info
    let mut namespace = IndexMap::new();
    // Store field names for __init__ and indexing
    let fields_tuple = PyObject::tuple(
        field_names.iter().map(|n| PyObject::str_val(n.clone())).collect()
    );
    namespace.insert(CompactString::from("_fields"), fields_tuple);
    namespace.insert(CompactString::from("__namedtuple__"), PyObject::bool_val(true));
    
    // Store field indices  
    for (i, name) in field_names.iter().enumerate() {
        namespace.insert(
            CompactString::from(format!("_field_idx_{}", name)),
            PyObject::int(i as i64)
        );
    }

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
                return Err(PyException::type_error("_make() requires an iterable argument"));
            }
            let items = args[0].to_list()?;
            let inst = PyObject::instance(cls_ref.clone());
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                for (i, name) in field_names_clone.iter().enumerate() {
                    let val = items.get(i).cloned().unwrap_or_else(PyObject::none);
                    attrs.insert(name.clone(), val);
                }
                attrs.insert(CompactString::from("_tuple"), PyObject::tuple(items));
            }
            Ok(inst)
        }
    );
    if let PyObjectPayload::Class(ref cd) = cls.payload {
        cd.namespace.write().insert(CompactString::from("_make"), make_fn);
    }

    Ok(cls)
}

fn collections_deque(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Extract maxlen from last arg if it's a kwargs dict
    let has_trailing_kwargs = !args.is_empty() && matches!(&args[args.len() - 1].payload, PyObjectPayload::Dict(_));
    let kwargs_idx = if has_trailing_kwargs { args.len() - 1 } else { args.len() };
    
    let items = if kwargs_idx == 0 || args.is_empty() {
        vec![]
    } else {
        args[0].to_list()?
    };
    
    // Extract maxlen from positional arg or trailing kwargs dict
    let maxlen = if has_trailing_kwargs {
        if let PyObjectPayload::Dict(map) = &args[args.len() - 1].payload {
            let map = map.read();
            map.get(&HashableKey::Str(CompactString::from("maxlen")))
                .and_then(|v| if matches!(&v.payload, PyObjectPayload::None) { None } else { Some(v.to_int().unwrap_or(0) as usize) })
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
    let deque_cls = PyObject::class(
        CompactString::from("deque"),
        vec![],
        IndexMap::new(),
    );
    let inst = PyObject::instance(deque_cls);
    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("__deque__"), PyObject::bool_val(true));
        attrs.insert(CompactString::from("_data"), PyObject::list(items));
        attrs.insert(
            CompactString::from("__maxlen__"),
            match maxlen {
                Some(n) => PyObject::int(n as i64),
                None => PyObject::none(),
            },
        );
    }
    Ok(inst)
}

fn collections_chainmap(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let maps: Vec<PyObjectRef> = args.to_vec();
    let mut merged = IndexMap::new();
    for m in maps.iter().rev() {
        if let PyObjectPayload::Dict(dict) = &m.payload {
            let rd = dict.read();
            for (k, v) in rd.iter() {
                merged.insert(k.clone(), v.clone());
            }
        }
    }
    let cls = PyObject::class(CompactString::from("ChainMap"), vec![], IndexMap::new());
    let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class: cls,
        attrs: Arc::new(RwLock::new(IndexMap::new())),
        dict_storage: Some(Arc::new(RwLock::new(merged))),
    }));
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("__chainmap__"), PyObject::bool_val(true));
        let maps_list = PyObject::list(maps.clone());
        w.insert(CompactString::from("maps"), maps_list);
        // new_child(m=None) — creates new ChainMap with m followed by current maps
        let captured_maps = maps.clone();
        w.insert(CompactString::from("new_child"), PyObject::native_closure(
            "ChainMap.new_child", move |call_args| {
                let child_map = if !call_args.is_empty() {
                    call_args[0].clone()
                } else {
                    PyObject::dict(IndexMap::new())
                };
                let mut new_maps = vec![child_map];
                new_maps.extend(captured_maps.iter().cloned());
                collections_chainmap(&new_maps)
            }
        ));
        // parents — eagerly create ChainMap of all maps except first
        // Only compute if we have maps, to avoid infinite recursion
        if maps.len() > 1 {
            let parents_val = collections_chainmap(&maps[1..])?;
            w.insert(CompactString::from("parents"), parents_val);
        }
    }
    Ok(inst)
}
