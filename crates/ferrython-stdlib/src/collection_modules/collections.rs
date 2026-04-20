use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyCell,
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin,
    CompareOp, SharedFxAttrMap,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

pub fn create_collections_module() -> PyObjectRef {
    let abc_module = crate::type_modules::create_collections_abc_module();

    // Deprecated aliases (Python 3.3-3.9 compat — removed in 3.10 but many packages still use them)
    let get_abc_attr = |name: &str| -> PyObjectRef {
        abc_module.get_attr(name).unwrap_or(PyObject::none())
    };
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

    make_module("collections", vec![
        ("abc", abc_module),
        ("OrderedDict", PyObject::native_function("collections.OrderedDict", collections_ordered_dict)),
        ("defaultdict", PyObject::native_function("collections.defaultdict", collections_defaultdict)),
        ("Counter", PyObject::native_function("collections.Counter", collections_counter)),
        ("namedtuple", make_builtin(collections_namedtuple)),
        ("deque", PyObject::native_function("collections.deque", collections_deque)),
        ("most_common", make_builtin(collections_most_common)),
        ("ChainMap", PyObject::native_function("collections.ChainMap", collections_chainmap)),
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
    ])
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
                return Err(PyException::type_error("move_to_end() requires a key argument"));
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

fn is_counter_internal_key(k: &HashableKey) -> bool {
    matches!(k, HashableKey::Str(s) if s.as_str() == "__defaultdict_factory__" || s.as_str() == "__counter__")
}

/// counter_elements(counter) -> list of elements repeated by their counts
fn counter_elements(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("counter_elements requires a Counter")); }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let r = map.read();
        let mut result = Vec::new();
        for (k, v) in r.iter() {
            if is_counter_internal_key(k) { continue; }
            let count = v.as_int().unwrap_or(0);
            for _ in 0..count {
                result.push(k.to_object());
            }
        }
        Ok(PyObject::list(result))
    } else {
        Err(PyException::type_error("counter_elements requires a Counter"))
    }
}

/// counter_update(counter, iterable_or_dict) -> None (mutates counter in-place)
fn counter_update(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("counter_update requires counter and data")); }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let mut w = map.write();
        if let PyObjectPayload::Dict(other) = &args[1].payload {
            let r = other.read();
            for (k, v) in r.iter() {
                if is_counter_internal_key(k) { continue; }
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
        Err(PyException::type_error("counter_update requires a Counter as first argument"))
    }
}

/// counter_subtract(counter, iterable_or_dict) -> None (mutates counter)
fn counter_subtract(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("counter_subtract requires counter and data")); }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let mut w = map.write();
        if let PyObjectPayload::Dict(other) = &args[1].payload {
            let r = other.read();
            for (k, v) in r.iter() {
                if is_counter_internal_key(k) { continue; }
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
        Err(PyException::type_error("counter_subtract requires a Counter"))
    }
}

/// counter_total(counter) -> int (sum of all counts)
fn counter_total(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("counter_total requires a Counter")); }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let r = map.read();
        let total: i64 = r.iter()
            .filter(|(k, _)| !is_counter_internal_key(k))
            .map(|(_, v)| v.as_int().unwrap_or(0))
            .sum();
        Ok(PyObject::int(total))
    } else {
        Err(PyException::type_error("counter_total requires a Counter"))
    }
}

fn counter_copy(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("counter_copy requires a Counter")); }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let r = map.read();
        Ok(PyObject::dict(r.clone()))
    } else {
        Err(PyException::type_error("counter_copy requires a Counter"))
    }
}

fn counter_clear(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("counter_clear requires a Counter")); }
    if let PyObjectPayload::Dict(map) = &args[0].payload {
        let mut w = map.write();
        let factory = w.get(&HashableKey::str_key(CompactString::from("__defaultdict_factory__"))).cloned();
        let marker = w.get(&HashableKey::str_key(CompactString::from("__counter__"))).cloned();
        w.clear();
        if let Some(f) = factory { w.insert(HashableKey::str_key(CompactString::from("__defaultdict_factory__")), f); }
        if let Some(m) = marker { w.insert(HashableKey::str_key(CompactString::from("__counter__")), m); }
    }
    Ok(PyObject::none())
}

fn collections_namedtuple(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("namedtuple requires typename and field_names"));
    }
    let typename = args[0].py_to_string();
    
    // Check for kwargs dict as last arg
    let (_field_args_end, kwargs_dict) = if args.len() > 2 {
        if let PyObjectPayload::Dict(map) = &args[args.len() - 1].payload {
            (args.len() - 1, Some(map.read().clone()))
        } else {
            (args.len(), None)
        }
    } else {
        (args.len(), None)
    };
    
    // Parse field names
    let field_names: Vec<CompactString> = match &args[1].payload {
        PyObjectPayload::Str(s) => {
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
    
    // Parse defaults from kwargs
    let defaults: Vec<PyObjectRef> = kwargs_dict.as_ref()
        .and_then(|kw| kw.get(&HashableKey::str_key(CompactString::from("defaults"))))
        .and_then(|d| d.to_list().ok())
        .unwrap_or_default();
    
    // Create a class with namespace containing field info
    let mut namespace = IndexMap::new();
    let fields_tuple = PyObject::tuple(
        field_names.iter().map(|n| PyObject::str_val(n.clone())).collect()
    );
    namespace.insert(CompactString::from("_fields"), fields_tuple);
    namespace.insert(CompactString::from("__namedtuple__"), PyObject::bool_val(true));
    
    // Build _field_defaults from defaults (right-aligned to fields)
    if !defaults.is_empty() {
        let mut defaults_map = IndexMap::new();
        let offset = field_names.len().saturating_sub(defaults.len());
        for (i, val) in defaults.iter().enumerate() {
            if let Some(name) = field_names.get(offset + i) {
                defaults_map.insert(
                    HashableKey::str_key(name.clone()),
                    val.clone(),
                );
            }
        }
        namespace.insert(
            CompactString::from("_field_defaults"),
            PyObject::dict(defaults_map),
        );
    }
    
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

    // _asdict: return OrderedDict of field_name → value
    let field_names_ad = field_names.clone();
    let asdict_fn = PyObject::native_closure(
        "namedtuple._asdict",
        move |args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
            if args.is_empty() { return Err(PyException::type_error("_asdict requires self")); }
            let self_obj = &args[0];
            let mut dict = IndexMap::new();
            for name in &field_names_ad {
                let val = self_obj.get_attr(name.as_str()).unwrap_or_else(PyObject::none);
                dict.insert(HashableKey::str_key(name.clone()), val);
            }
            Ok(PyObject::dict(dict))
        }
    );
    if let PyObjectPayload::Class(ref cd) = cls.payload {
        cd.namespace.write().insert(CompactString::from("_asdict"), asdict_fn);
    }

    // _replace(**kwargs): return new namedtuple with specified fields replaced
    let cls_ref2 = cls.clone();
    let field_names_rep = field_names.clone();
    let replace_fn = PyObject::native_closure(
        "namedtuple._replace",
        move |args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
            if args.is_empty() { return Err(PyException::type_error("_replace requires self")); }
            let self_obj = &args[0];
            let kwargs = if args.len() > 1 {
                if let PyObjectPayload::Dict(ref map) = args[args.len() - 1].payload {
                    Some(map.read().clone())
                } else { None }
            } else { None };
            let inst = PyObject::instance(cls_ref2.clone());
            if let PyObjectPayload::Instance(ref data) = inst.payload {
                let mut attrs = data.attrs.write();
                let mut tuple_items = Vec::new();
                for name in &field_names_rep {
                    let val = if let Some(ref kw) = kwargs {
                        kw.get(&HashableKey::str_key(name.clone()))
                            .cloned()
                            .unwrap_or_else(|| self_obj.get_attr(name.as_str()).unwrap_or_else(PyObject::none))
                    } else {
                        self_obj.get_attr(name.as_str()).unwrap_or_else(PyObject::none)
                    };
                    attrs.insert(name.clone(), val.clone());
                    tuple_items.push(val);
                }
                attrs.insert(CompactString::from("_tuple"), PyObject::tuple(tuple_items));
            }
            Ok(inst)
        }
    );
    if let PyObjectPayload::Class(ref cd) = cls.payload {
        cd.namespace.write().insert(CompactString::from("_replace"), replace_fn);
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
            map.get(&HashableKey::str_key(CompactString::from("maxlen")))
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
    
    let data = Rc::new(PyCell::new(items));
    
    // Build instance methods that share the data list
    let mut cls_ns = IndexMap::new();
    
    // append(x)
    let d = data.clone();
    let ml = maxlen;
    cls_ns.insert(CompactString::from("append"), PyObject::native_closure(
        "deque.append", move |args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("append requires argument")); }
            let mut w = d.write();
            w.push(args[0].clone());
            if let Some(m) = ml { while w.len() > m { w.remove(0); } }
            Ok(PyObject::none())
        }
    ));
    
    // appendleft(x)
    let d = data.clone();
    let ml = maxlen;
    cls_ns.insert(CompactString::from("appendleft"), PyObject::native_closure(
        "deque.appendleft", move |args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("appendleft requires argument")); }
            let mut w = d.write();
            w.insert(0, args[0].clone());
            if let Some(m) = ml { while w.len() > m { w.pop(); } }
            Ok(PyObject::none())
        }
    ));
    
    // pop()
    let d = data.clone();
    cls_ns.insert(CompactString::from("pop"), PyObject::native_closure(
        "deque.pop", move |_: &[PyObjectRef]| {
            let mut w = d.write();
            w.pop().ok_or_else(|| PyException::index_error("pop from an empty deque"))
        }
    ));
    
    // popleft()
    let d = data.clone();
    cls_ns.insert(CompactString::from("popleft"), PyObject::native_closure(
        "deque.popleft", move |_: &[PyObjectRef]| {
            let mut w = d.write();
            if w.is_empty() { return Err(PyException::index_error("pop from an empty deque")); }
            Ok(w.remove(0))
        }
    ));
    
    // extend(iterable)
    let d = data.clone();
    let ml = maxlen;
    cls_ns.insert(CompactString::from("extend"), PyObject::native_closure(
        "deque.extend", move |args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("extend requires argument")); }
            let items = args[0].to_list()?;
            let mut w = d.write();
            w.extend(items);
            if let Some(m) = ml { while w.len() > m { w.remove(0); } }
            Ok(PyObject::none())
        }
    ));
    
    // extendleft(iterable)
    let d = data.clone();
    let ml = maxlen;
    cls_ns.insert(CompactString::from("extendleft"), PyObject::native_closure(
        "deque.extendleft", move |args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("extendleft requires argument")); }
            let items = args[0].to_list()?;
            let mut w = d.write();
            // CPython: appendleft each item in order — insert(0) naturally reverses
            for item in items.into_iter() {
                w.insert(0, item);
            }
            if let Some(m) = ml { while w.len() > m { w.pop(); } }
            Ok(PyObject::none())
        }
    ));
    
    // rotate(n=1)
    let d = data.clone();
    cls_ns.insert(CompactString::from("rotate"), PyObject::native_closure(
        "deque.rotate", move |args: &[PyObjectRef]| {
            let n = if args.is_empty() { 1i64 } else { args[0].to_int()? };
            let mut w = d.write();
            let len = w.len();
            if len == 0 { return Ok(PyObject::none()); }
            let n = ((n % len as i64) + len as i64) as usize % len;
            if n > 0 {
                let split_point = len - n;
                let mut rotated = w[split_point..].to_vec();
                rotated.extend_from_slice(&w[..split_point]);
                *w = rotated;
            }
            Ok(PyObject::none())
        }
    ));
    
    // clear()
    let d = data.clone();
    cls_ns.insert(CompactString::from("clear"), PyObject::native_closure(
        "deque.clear", move |_: &[PyObjectRef]| {
            d.write().clear();
            Ok(PyObject::none())
        }
    ));
    
    // count(x)
    let d = data.clone();
    cls_ns.insert(CompactString::from("count"), PyObject::native_closure(
        "deque.count", move |args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("count requires argument")); }
            let target = &args[0];
            let r = d.read();
            let c = r.iter().filter(|item| {
                item.compare(target, CompareOp::Eq).map(|v| v.is_truthy()).unwrap_or(false)
            }).count();
            Ok(PyObject::int(c as i64))
        }
    ));
    
    // index(x)
    let d = data.clone();
    cls_ns.insert(CompactString::from("index"), PyObject::native_closure(
        "deque.index", move |args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("index requires argument")); }
            let target = &args[0];
            let r = d.read();
            for (i, item) in r.iter().enumerate() {
                if item.compare(target, CompareOp::Eq).map(|v| v.is_truthy()).unwrap_or(false) { return Ok(PyObject::int(i as i64)); }
            }
            Err(PyException::value_error("value not in deque"))
        }
    ));
    
    // remove(x)
    let d = data.clone();
    cls_ns.insert(CompactString::from("remove"), PyObject::native_closure(
        "deque.remove", move |args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("remove requires argument")); }
            let target = &args[0];
            let mut w = d.write();
            let pos = w.iter().position(|item| {
                item.compare(target, CompareOp::Eq).map(|v| v.is_truthy()).unwrap_or(false)
            });
            match pos {
                Some(i) => { w.remove(i); Ok(PyObject::none()) }
                None => Err(PyException::value_error("deque.remove(x): x not in deque"))
            }
        }
    ));
    
    // reverse()
    let d = data.clone();
    cls_ns.insert(CompactString::from("reverse"), PyObject::native_closure(
        "deque.reverse", move |_: &[PyObjectRef]| {
            d.write().reverse();
            Ok(PyObject::none())
        }
    ));
    
    // copy()
    let d = data.clone();
    let ml2 = maxlen;
    cls_ns.insert(CompactString::from("copy"), PyObject::native_closure(
        "deque.copy", move |_: &[PyObjectRef]| {
            let items = d.read().clone();
            let mut new_args = vec![PyObject::list(items)];
            if let Some(m) = ml2 { new_args.push(PyObject::int(m as i64)); }
            collections_deque(&new_args)
        }
    ));
    
    // __len__()
    let d = data.clone();
    cls_ns.insert(CompactString::from("__len__"), PyObject::native_closure(
        "deque.__len__", move |_: &[PyObjectRef]| {
            Ok(PyObject::int(d.read().len() as i64))
        }
    ));
    
    // __bool__()
    let d = data.clone();
    cls_ns.insert(CompactString::from("__bool__"), PyObject::native_closure(
        "deque.__bool__", move |_: &[PyObjectRef]| {
            Ok(PyObject::bool_val(!d.read().is_empty()))
        }
    ));
    
    // __repr__()
    let d = data.clone();
    let ml3 = maxlen;
    cls_ns.insert(CompactString::from("__repr__"), PyObject::native_closure(
        "deque.__repr__", move |_: &[PyObjectRef]| {
            let r = d.read();
            let items_str: Vec<String> = r.iter().map(|i| i.py_to_string()).collect();
            let base = format!("deque([{}])", items_str.join(", "));
            if let Some(m) = ml3 {
                Ok(PyObject::str_val(CompactString::from(format!("deque([{}], maxlen={})", items_str.join(", "), m))))
            } else {
                Ok(PyObject::str_val(CompactString::from(base)))
            }
        }
    ));
    
    // __iter__()
    let d = data.clone();
    cls_ns.insert(CompactString::from("__iter__"), PyObject::native_closure(
        "deque.__iter__", move |_: &[PyObjectRef]| {
            let snapshot = d.read().clone();
            Ok(PyObject::wrap(PyObjectPayload::Iterator(
                Rc::new(PyCell::new(ferrython_core::object::IteratorData::List {
                    items: snapshot,
                    index: 0,
                }))
            )))
        }
    ));

    // __eq__ — element-wise comparison with identity check
    let d_eq = data.clone();
    cls_ns.insert(CompactString::from("__eq__"), PyObject::native_closure(
        "deque.__eq__", move |args: &[PyObjectRef]| {
            if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
            let other = &args[1];
            // Other must also be a deque (Instance with __deque__ marker)
            if let PyObjectPayload::Instance(other_inst) = &other.payload {
                if !other_inst.attrs.read().contains_key("__deque__") {
                    return Ok(PyObject::not_implemented());
                }
            } else {
                return Ok(PyObject::not_implemented());
            }
            // Get other's internal data via closure capture? No, we need to read from the other deque.
            // Read the other deque's _data from its attrs or use its class __iter__
            let other_data_list = if let PyObjectPayload::Instance(other_inst) = &other.payload {
                other_inst.attrs.read().get("_data").cloned()
            } else { None };
            // Use closure-captured data for self
            let self_data = d_eq.read();
            // Get other data elements
            let other_elems: Vec<PyObjectRef> = if let Some(ref od) = other_data_list {
                if let PyObjectPayload::List(list) = &od.payload {
                    list.read().clone()
                } else { return Ok(PyObject::bool_val(false)); }
            } else { return Ok(PyObject::bool_val(false)); };
            if self_data.len() != other_elems.len() {
                return Ok(PyObject::bool_val(false));
            }
            for (x, y) in self_data.iter().zip(other_elems.iter()) {
                // Identity check first (CPython: PyObject_RichCompareBool)
                if PyObjectRef::ptr_eq(x, y) { continue; }
                if !x.compare(y, CompareOp::Eq).map_or(false, |r| r.is_truthy()) {
                    return Ok(PyObject::bool_val(false));
                }
            }
            Ok(PyObject::bool_val(true))
        }
    ));
    
    // __contains__(x) - needed for 'in' operator
    let d = data.clone();
    cls_ns.insert(CompactString::from("__contains__"), PyObject::native_closure(
        "deque.__contains__", move |args: &[PyObjectRef]| {
            // Called as unbound method: args = [self, value] or directly: args = [value]
            let target = if args.len() >= 2 { &args[1] } else if !args.is_empty() { &args[0] } else { return Ok(PyObject::bool_val(false)); };
            let r = d.read();
            for item in r.iter() {
                // Identity check first (needed for NaN: nan is nan → True, nan == nan → False)
                if PyObjectRef::ptr_eq(item, target) {
                    return Ok(PyObject::bool_val(true));
                }
                if item.compare(target, CompareOp::Eq).map(|v| v.is_truthy()).unwrap_or(false) {
                    return Ok(PyObject::bool_val(true));
                }
            }
            Ok(PyObject::bool_val(false))
        }
    ));
    
    // __getitem__(index)
    let d = data.clone();
    cls_ns.insert(CompactString::from("__getitem__"), PyObject::native_closure(
        "deque.__getitem__", move |args: &[PyObjectRef]| {
            if args.is_empty() { return Err(PyException::type_error("__getitem__ requires index")); }
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
        }
    ));
    
    let deque_cls = PyObject::class(
        CompactString::from("deque"),
        vec![],
        cls_ns,
    );
    let inst = PyObject::instance(deque_cls);
    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("__deque__"), PyObject::bool_val(true));
        // Store a reference list for _data (closures share the backing Rc<PyCell> directly)
        attrs.insert(CompactString::from("_data"), PyObject::list(data.read().clone()));
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
                if !name.starts_with("_data") && !name.starts_with("__deque") && !name.starts_with("__maxlen") {
                    attrs.insert(name.clone(), val.clone());
                }
            }
        }
    }
    Ok(inst)
}

fn collections_chainmap(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let maps: Vec<PyObjectRef> = args.to_vec();
    // Create a class with __getitem__/__setitem__ that delegates to the maps list
    let maps_list = PyObject::list(maps.clone());
    let mut ns = IndexMap::new();

    // __getitem__ — walk maps in order
    let ml = maps_list.clone();
    ns.insert(CompactString::from("__getitem__"), PyObject::native_closure(
        "ChainMap.__getitem__", move |call_args| {
            if call_args.len() < 2 { return Err(PyException::type_error("__getitem__ requires key")); }
            let key = &call_args[1];
            let hk = HashableKey::from_object(key)?;
            if let PyObjectPayload::List(list) = &ml.payload {
                for m in list.read().iter() {
                    if let PyObjectPayload::Dict(dict) = &m.payload {
                        if let Some(val) = dict.read().get(&hk) {
                            return Ok(val.clone());
                        }
                    }
                }
            }
            Err(PyException::key_error(&key.py_to_string()))
        }
    ));
    // __setitem__ — write to first map
    let ml2 = maps_list.clone();
    ns.insert(CompactString::from("__setitem__"), PyObject::native_closure(
        "ChainMap.__setitem__", move |call_args| {
            if call_args.len() < 3 { return Err(PyException::type_error("__setitem__ requires key and value")); }
            let key = &call_args[1];
            let value = &call_args[2];
            let hk = HashableKey::from_object(key)?;
            if let PyObjectPayload::List(list) = &ml2.payload {
                let r = list.read();
                if let Some(first) = r.first() {
                    if let PyObjectPayload::Dict(dict) = &first.payload {
                        dict.write().insert(hk, value.clone());
                    }
                }
            }
            Ok(PyObject::none())
        }
    ));
    // __delitem__ — delete from first map
    let ml3 = maps_list.clone();
    ns.insert(CompactString::from("__delitem__"), PyObject::native_closure(
        "ChainMap.__delitem__", move |call_args| {
            if call_args.len() < 2 { return Err(PyException::type_error("__delitem__ requires key")); }
            let key = &call_args[1];
            let hk = HashableKey::from_object(key)?;
            if let PyObjectPayload::List(list) = &ml3.payload {
                let r = list.read();
                if let Some(first) = r.first() {
                    if let PyObjectPayload::Dict(dict) = &first.payload {
                        if dict.write().shift_remove(&hk).is_none() {
                            return Err(PyException::key_error(&key.py_to_string()));
                        }
                        return Ok(PyObject::none());
                    }
                }
            }
            Err(PyException::key_error(&call_args[1].py_to_string()))
        }
    ));
    // __contains__
    let ml4 = maps_list.clone();
    ns.insert(CompactString::from("__contains__"), PyObject::native_closure(
        "ChainMap.__contains__", move |call_args| {
            if call_args.len() < 2 { return Ok(PyObject::bool_val(false)); }
            let hk = HashableKey::from_object(&call_args[1])?;
            if let PyObjectPayload::List(list) = &ml4.payload {
                for m in list.read().iter() {
                    if let PyObjectPayload::Dict(dict) = &m.payload {
                        if dict.read().contains_key(&hk) {
                            return Ok(PyObject::bool_val(true));
                        }
                    }
                }
            }
            Ok(PyObject::bool_val(false))
        }
    ));
    // __len__ — count unique keys
    let ml5 = maps_list.clone();
    ns.insert(CompactString::from("__len__"), PyObject::native_closure(
        "ChainMap.__len__", move |_call_args| {
            let mut all_keys = IndexMap::new();
            if let PyObjectPayload::List(list) = &ml5.payload {
                for m in list.read().iter() {
                    if let PyObjectPayload::Dict(dict) = &m.payload {
                        for (k, _) in dict.read().iter() {
                            all_keys.entry(k.clone()).or_insert(());
                        }
                    }
                }
            }
            Ok(PyObject::int(all_keys.len() as i64))
        }
    ));
    // __repr__
    let ml6 = maps_list.clone();
    ns.insert(CompactString::from("__repr__"), PyObject::native_closure(
        "ChainMap.__repr__", move |_call_args| {
            let mut parts = Vec::new();
            if let PyObjectPayload::List(list) = &ml6.payload {
                for m in list.read().iter() {
                    parts.push(m.py_to_string());
                }
            }
            Ok(PyObject::str_val(CompactString::from(format!("ChainMap({})", parts.join(", ")))))
        }
    ));
    // keys — all unique keys across maps
    let ml7 = maps_list.clone();
    ns.insert(CompactString::from("keys"), PyObject::native_closure(
        "ChainMap.keys", move |_call_args| {
            let mut all_keys = IndexMap::<HashableKey, ()>::new();
            if let PyObjectPayload::List(list) = &ml7.payload {
                for m in list.read().iter() {
                    if let PyObjectPayload::Dict(dict) = &m.payload {
                        for (k, _) in dict.read().iter() {
                            all_keys.entry(k.clone()).or_insert(());
                        }
                    }
                }
            }
            let keys: Vec<PyObjectRef> = all_keys.keys().map(|k| k.to_object()).collect();
            Ok(PyObject::list(keys))
        }
    ));
    // values — values for unique keys (first occurrence wins)
    let ml8 = maps_list.clone();
    ns.insert(CompactString::from("values"), PyObject::native_closure(
        "ChainMap.values", move |_call_args| {
            let mut seen = IndexMap::<HashableKey, PyObjectRef>::new();
            if let PyObjectPayload::List(list) = &ml8.payload {
                for m in list.read().iter() {
                    if let PyObjectPayload::Dict(dict) = &m.payload {
                        for (k, v) in dict.read().iter() {
                            seen.entry(k.clone()).or_insert_with(|| v.clone());
                        }
                    }
                }
            }
            Ok(PyObject::list(seen.values().cloned().collect()))
        }
    ));
    // items — (key, value) pairs for unique keys
    let ml9 = maps_list.clone();
    ns.insert(CompactString::from("items"), PyObject::native_closure(
        "ChainMap.items", move |_call_args| {
            let mut seen = IndexMap::<HashableKey, PyObjectRef>::new();
            if let PyObjectPayload::List(list) = &ml9.payload {
                for m in list.read().iter() {
                    if let PyObjectPayload::Dict(dict) = &m.payload {
                        for (k, v) in dict.read().iter() {
                            seen.entry(k.clone()).or_insert_with(|| v.clone());
                        }
                    }
                }
            }
            let items: Vec<PyObjectRef> = seen.iter()
                .map(|(k, v)| PyObject::tuple(vec![k.to_object(), v.clone()]))
                .collect();
            Ok(PyObject::list(items))
        }
    ));
    // get(key, default=None)
    let ml10 = maps_list.clone();
    ns.insert(CompactString::from("get"), PyObject::native_closure(
        "ChainMap.get", move |call_args| {
            if call_args.len() < 2 { return Err(PyException::type_error("get requires key")); }
            let key = &call_args[1];
            let default = call_args.get(2).cloned().unwrap_or_else(PyObject::none);
            let hk = HashableKey::from_object(key)?;
            if let PyObjectPayload::List(list) = &ml10.payload {
                for m in list.read().iter() {
                    if let PyObjectPayload::Dict(dict) = &m.payload {
                        if let Some(val) = dict.read().get(&hk) {
                            return Ok(val.clone());
                        }
                    }
                }
            }
            Ok(default)
        }
    ));
    // __iter__ — iterate unique keys
    let ml11 = maps_list.clone();
    ns.insert(CompactString::from("__iter__"), PyObject::native_closure(
        "ChainMap.__iter__", move |_call_args| {
            let mut all_keys = IndexMap::<HashableKey, ()>::new();
            if let PyObjectPayload::List(list) = &ml11.payload {
                for m in list.read().iter() {
                    if let PyObjectPayload::Dict(dict) = &m.payload {
                        for (k, _) in dict.read().iter() {
                            all_keys.entry(k.clone()).or_insert(());
                        }
                    }
                }
            }
            let keys: Vec<PyObjectRef> = all_keys.keys().map(|k| k.to_object()).collect();
            Ok(PyObject::list(keys))
        }
    ));

    let cls = PyObject::class(CompactString::from("ChainMap"), vec![], ns);
    let inst = PyObject::instance(cls.clone());
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("__chainmap__"), PyObject::bool_val(true));
        w.insert(CompactString::from("maps"), maps_list.clone());
        // new_child(m=None)
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
        if maps.len() > 1 {
            let parents_val = collections_chainmap(&maps[1..])?;
            w.insert(CompactString::from("parents"), parents_val);
        }
    }
    Ok(inst)
}

// --- UserDict / UserList / UserString ---

fn make_user_dict_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(CompactString::from("__init__"), make_builtin(|args| {
        if args.is_empty() { return Err(PyException::type_error("UserDict.__init__ requires self")); }
        let inst = &args[0];
        let data = if args.len() > 1 {
            if let PyObjectPayload::Dict(d) = &args[1].payload {
                PyObject::wrap(PyObjectPayload::Dict(Rc::new(PyCell::new(d.read().clone()))))
            } else {
                PyObject::dict_from_pairs(vec![])
            }
        } else {
            PyObject::dict_from_pairs(vec![])
        };
        if let PyObjectPayload::Instance(d) = &inst.payload {
            d.attrs.write().insert(CompactString::from("data"), data.clone());
            // Install instance methods that directly operate on the data
            install_dict_methods(&d.attrs, &data);
        }
        Ok(PyObject::none())
    }));
    ns.insert(CompactString::from("__getitem__"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("expected key")); }
        let data = get_user_data(&args[0], "data")?;
        data.get_item(&args[1])
    }));
    ns.insert(CompactString::from("__setitem__"), make_builtin(|args| {
        if args.len() < 3 { return Err(PyException::type_error("expected key and value")); }
        let data = get_user_data(&args[0], "data")?;
        if let PyObjectPayload::Dict(d) = &data.payload {
            let key = HashableKey::from_object(&args[1])?;
            d.write().insert(key, args[2].clone());
        }
        Ok(PyObject::none())
    }));
    ns.insert(CompactString::from("__delitem__"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("expected key")); }
        let data = get_user_data(&args[0], "data")?;
        if let PyObjectPayload::Dict(d) = &data.payload {
            let key = HashableKey::from_object(&args[1])?;
            if d.write().shift_remove(&key).is_none() {
                return Err(PyException::key_error(args[1].py_to_string()));
            }
        }
        Ok(PyObject::none())
    }));
    ns.insert(CompactString::from("__len__"), make_builtin(|args| {
        let data = get_user_data(&args[0], "data")?;
        Ok(PyObject::int(data.py_len()? as i64))
    }));
    ns.insert(CompactString::from("__contains__"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("expected key")); }
        let data = get_user_data(&args[0], "data")?;
        if let PyObjectPayload::Dict(d) = &data.payload {
            let key = HashableKey::from_object(&args[1])?;
            Ok(PyObject::bool_val(d.read().contains_key(&key)))
        } else {
            Ok(PyObject::bool_val(false))
        }
    }));
    ns.insert(CompactString::from("__repr__"), make_builtin(|args| {
        let data = get_user_data(&args[0], "data")?;
        Ok(PyObject::str_val(CompactString::from(data.py_to_string())))
    }));
    ns.insert(CompactString::from("__iter__"), make_builtin(|args| {
        let data = get_user_data(&args[0], "data")?;
        data.get_iter()
    }));
    ns.insert(CompactString::from("__eq__"), make_builtin(|args| {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
        let data = get_user_data(&args[0], "data")?;
        let other_data = if let Ok(od) = get_user_data(&args[1], "data") { od } else { args[1].clone() };
        if let (PyObjectPayload::Dict(a), PyObjectPayload::Dict(b)) = (&data.payload, &other_data.payload) {
            let ra = a.read();
            let rb = b.read();
            if ra.len() != rb.len() { return Ok(PyObject::bool_val(false)); }
            for (k, v) in ra.iter() {
                match rb.get(k) {
                    Some(ov) if v.compare(ov, CompareOp::Eq).map_or(false, |r| r.is_truthy()) => {}
                    _ => return Ok(PyObject::bool_val(false)),
                }
            }
            Ok(PyObject::bool_val(true))
        } else {
            Ok(PyObject::bool_val(false))
        }
    }));
    ns.insert(CompactString::from("__bool__"), make_builtin(|args| {
        let data = get_user_data(&args[0], "data")?;
        Ok(PyObject::bool_val(data.py_len()? > 0))
    }));
    ns.insert(CompactString::from("__or__"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("expected other")); }
        let data = get_user_data(&args[0], "data")?;
        let mut merged = IndexMap::new();
        if let PyObjectPayload::Dict(d) = &data.payload {
            for (k, v) in d.read().iter() { merged.insert(k.clone(), v.clone()); }
        }
        let other = if let Ok(od) = get_user_data(&args[1], "data") { od } else { args[1].clone() };
        if let PyObjectPayload::Dict(d) = &other.payload {
            for (k, v) in d.read().iter() { merged.insert(k.clone(), v.clone()); }
        }
        Ok(PyObject::dict(merged))
    }));
    ns.insert(CompactString::from("__ior__"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("expected other")); }
        let data = get_user_data(&args[0], "data")?;
        let other = if let Ok(od) = get_user_data(&args[1], "data") { od } else { args[1].clone() };
        if let (PyObjectPayload::Dict(dst), PyObjectPayload::Dict(src)) = (&data.payload, &other.payload) {
            let mut w = dst.write();
            for (k, v) in src.read().iter() { w.insert(k.clone(), v.clone()); }
        }
        Ok(args[0].clone())
    }));
    PyObject::class(CompactString::from("UserDict"), vec![], ns)
}

fn install_dict_methods(attrs: &SharedFxAttrMap, data: &PyObjectRef) {
    let map = if let PyObjectPayload::Dict(m) = &data.payload { m.clone() } else { return; };
    let m = map.clone();
    attrs.write().insert(CompactString::from("keys"), PyObject::native_closure("keys", move |_| {
        Ok(PyObject::wrap(PyObjectPayload::DictKeys(m.clone())))
    }));
    let m = map.clone();
    attrs.write().insert(CompactString::from("values"), PyObject::native_closure("values", move |_| {
        Ok(PyObject::wrap(PyObjectPayload::DictValues(m.clone())))
    }));
    let m = map.clone();
    attrs.write().insert(CompactString::from("items"), PyObject::native_closure("items", move |_| {
        Ok(PyObject::wrap(PyObjectPayload::DictItems(m.clone())))
    }));
    let m = map.clone();
    attrs.write().insert(CompactString::from("get"), PyObject::native_closure("get", move |args| {
        if args.is_empty() { return Err(PyException::type_error("get() requires at least 1 argument")); }
        let key = args[0].to_hashable_key()?;
        let default = if args.len() >= 2 { args[1].clone() } else { PyObject::none() };
        Ok(m.read().get(&key).cloned().unwrap_or(default))
    }));
    let m = map.clone();
    attrs.write().insert(CompactString::from("pop"), PyObject::native_closure("pop", move |args| {
        if args.is_empty() { return Err(PyException::type_error("pop() requires at least 1 argument")); }
        let key = args[0].to_hashable_key()?;
        let default = if args.len() >= 2 { Some(args[1].clone()) } else { None };
        match m.write().shift_remove(&key) {
            Some(v) => Ok(v),
            None => default.ok_or_else(|| PyException::key_error(args[0].py_to_string())),
        }
    }));
    let m = map.clone();
    attrs.write().insert(CompactString::from("setdefault"), PyObject::native_closure("setdefault", move |args| {
        if args.is_empty() { return Err(PyException::type_error("setdefault() requires at least 1 argument")); }
        let key = args[0].to_hashable_key()?;
        let default = if args.len() >= 2 { args[1].clone() } else { PyObject::none() };
        let mut w = m.write();
        if let Some(v) = w.get(&key) { return Ok(v.clone()); }
        w.insert(key, default.clone());
        Ok(default)
    }));
    let m = map.clone();
    attrs.write().insert(CompactString::from("update"), PyObject::native_closure("update", move |args| {
        if !args.is_empty() {
            if let PyObjectPayload::Dict(other) = &args[0].payload {
                let mut w = m.write();
                for (k, v) in other.read().iter() { w.insert(k.clone(), v.clone()); }
            }
        }
        Ok(PyObject::none())
    }));
    let m = map.clone();
    attrs.write().insert(CompactString::from("copy"), PyObject::native_closure("copy", move |_| {
        Ok(PyObject::dict(m.read().clone()))
    }));
    let m = map.clone();
    attrs.write().insert(CompactString::from("clear"), PyObject::native_closure("clear", move |_| {
        m.write().clear();
        Ok(PyObject::none())
    }));
}

fn make_user_list_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(CompactString::from("__init__"), make_builtin(|args| {
        if args.is_empty() { return Err(PyException::type_error("UserList.__init__ requires self")); }
        let inst = &args[0];
        let data = if args.len() > 1 {
            let items = args[1].to_list()?;
            PyObject::list(items)
        } else {
            PyObject::list(vec![])
        };
        if let PyObjectPayload::Instance(d) = &inst.payload {
            d.attrs.write().insert(CompactString::from("data"), data.clone());
            install_list_methods(&d.attrs, &data);
        }
        Ok(PyObject::none())
    }));
    ns.insert(CompactString::from("__getitem__"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("expected index")); }
        let data = get_user_data(&args[0], "data")?;
        data.get_item(&args[1])
    }));
    ns.insert(CompactString::from("__setitem__"), make_builtin(|args| {
        if args.len() < 3 { return Err(PyException::type_error("expected index and value")); }
        let data = get_user_data(&args[0], "data")?;
        if let PyObjectPayload::List(l) = &data.payload {
            let idx = args[1].to_int()? as i64;
            let mut w = l.write();
            let len = w.len() as i64;
            let i = if idx < 0 { (len + idx).max(0) as usize } else { idx as usize };
            if i < w.len() {
                w[i] = args[2].clone();
            } else {
                return Err(PyException::index_error("list assignment index out of range"));
            }
        }
        Ok(PyObject::none())
    }));
    ns.insert(CompactString::from("__len__"), make_builtin(|args| {
        let data = get_user_data(&args[0], "data")?;
        Ok(PyObject::int(data.py_len()? as i64))
    }));
    ns.insert(CompactString::from("__contains__"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("expected item")); }
        let data = get_user_data(&args[0], "data")?;
        if let PyObjectPayload::List(l) = &data.payload {
            let target = &args[1];
            Ok(PyObject::bool_val(l.read().iter().any(|x| {
                x.compare(target, CompareOp::Eq).map_or(false, |v| v.is_truthy())
            })))
        } else {
            Ok(PyObject::bool_val(false))
        }
    }));
    ns.insert(CompactString::from("__repr__"), make_builtin(|args| {
        let data = get_user_data(&args[0], "data")?;
        Ok(PyObject::str_val(CompactString::from(data.py_to_string())))
    }));
    ns.insert(CompactString::from("__iter__"), make_builtin(|args| {
        let data = get_user_data(&args[0], "data")?;
        data.get_iter()
    }));
    ns.insert(CompactString::from("__delitem__"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("expected index")); }
        let data = get_user_data(&args[0], "data")?;
        if let PyObjectPayload::List(l) = &data.payload {
            let idx = args[1].to_int()? as i64;
            let mut w = l.write();
            let len = w.len() as i64;
            let i = if idx < 0 { (len + idx).max(0) as usize } else { idx as usize };
            if i < w.len() {
                w.remove(i);
                Ok(PyObject::none())
            } else {
                Err(PyException::index_error("list assignment index out of range"))
            }
        } else {
            Err(PyException::type_error("expected list data"))
        }
    }));
    ns.insert(CompactString::from("__add__"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("expected other")); }
        let data = get_user_data(&args[0], "data")?;
        let other = if let Ok(od) = get_user_data(&args[1], "data") { od } else { args[1].clone() };
        let mut items = data.to_list()?;
        items.extend(other.to_list()?);
        Ok(PyObject::list(items))
    }));
    ns.insert(CompactString::from("__iadd__"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("expected other")); }
        let data = get_user_data(&args[0], "data")?;
        let other = if let Ok(od) = get_user_data(&args[1], "data") { od } else { args[1].clone() };
        if let PyObjectPayload::List(l) = &data.payload {
            l.write().extend(other.to_list()?);
        }
        Ok(args[0].clone())
    }));
    ns.insert(CompactString::from("__mul__"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("expected int")); }
        let data = get_user_data(&args[0], "data")?;
        let n = args[1].to_int()?.max(0) as usize;
        let items = data.to_list()?;
        let mut result = Vec::with_capacity(items.len() * n);
        for _ in 0..n { result.extend(items.iter().cloned()); }
        Ok(PyObject::list(result))
    }));
    ns.insert(CompactString::from("__eq__"), make_builtin(|args| {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
        let data = get_user_data(&args[0], "data")?;
        let other = if let Ok(od) = get_user_data(&args[1], "data") { od } else { args[1].clone() };
        if let (PyObjectPayload::List(a), PyObjectPayload::List(b)) = (&data.payload, &other.payload) {
            let ra = a.read();
            let rb = b.read();
            if ra.len() != rb.len() { return Ok(PyObject::bool_val(false)); }
            for (x, y) in ra.iter().zip(rb.iter()) {
                if !x.compare(y, CompareOp::Eq).map_or(false, |v| v.is_truthy()) {
                    return Ok(PyObject::bool_val(false));
                }
            }
            Ok(PyObject::bool_val(true))
        } else {
            Ok(PyObject::bool_val(false))
        }
    }));
    ns.insert(CompactString::from("__bool__"), make_builtin(|args| {
        let data = get_user_data(&args[0], "data")?;
        Ok(PyObject::bool_val(data.py_len()? > 0))
    }));
    PyObject::class(CompactString::from("UserList"), vec![], ns)
}

fn install_list_methods(attrs: &SharedFxAttrMap, data: &PyObjectRef) {
    if !matches!(&data.payload, PyObjectPayload::List(_)) { return; }
    let l = data.clone();
    attrs.write().insert(CompactString::from("append"), PyObject::native_closure("append", move |args| {
        if args.is_empty() { return Err(PyException::type_error("append() requires 1 argument")); }
        if let PyObjectPayload::List(items) = &l.payload { items.write().push(args[0].clone()); }
        Ok(PyObject::none())
    }));
    let l = data.clone();
    attrs.write().insert(CompactString::from("extend"), PyObject::native_closure("extend", move |args| {
        if args.is_empty() { return Err(PyException::type_error("extend() requires 1 argument")); }
        let new_items = args[0].to_list()?;
        if let PyObjectPayload::List(items) = &l.payload { items.write().extend(new_items); }
        Ok(PyObject::none())
    }));
    let l = data.clone();
    attrs.write().insert(CompactString::from("insert"), PyObject::native_closure("insert", move |args| {
        if args.len() < 2 { return Err(PyException::type_error("insert() requires 2 arguments")); }
        let idx = args[0].to_int()? as usize;
        if let PyObjectPayload::List(items) = &l.payload {
            let mut w = items.write();
            let idx = idx.min(w.len());
            w.insert(idx, args[1].clone());
        }
        Ok(PyObject::none())
    }));
    let l = data.clone();
    attrs.write().insert(CompactString::from("pop"), PyObject::native_closure("pop", move |args| {
        if let PyObjectPayload::List(items) = &l.payload {
            let mut w = items.write();
            if w.is_empty() { return Err(PyException::index_error("pop from empty list")); }
            let idx = if !args.is_empty() {
                let i = args[0].to_int()? as i64;
                let len = w.len() as i64;
                (if i < 0 { (len + i).max(0) } else { i.min(len - 1) }) as usize
            } else { w.len() - 1 };
            if idx < w.len() { Ok(w.remove(idx)) } else { Err(PyException::index_error("pop index out of range")) }
        } else { Err(PyException::type_error("not a list")) }
    }));
    let l = data.clone();
    attrs.write().insert(CompactString::from("remove"), PyObject::native_closure("remove", move |args| {
        if args.is_empty() { return Err(PyException::type_error("remove() requires 1 argument")); }
        if let PyObjectPayload::List(items) = &l.payload {
            let mut w = items.write();
            let target = &args[0];
            if let Some(pos) = w.iter().position(|x| x.compare(target, CompareOp::Eq).map_or(false, |v| v.is_truthy())) {
                w.remove(pos);
                Ok(PyObject::none())
            } else {
                Err(PyException::value_error("list.remove(x): x not in list"))
            }
        } else { Err(PyException::type_error("not a list")) }
    }));
    let l = data.clone();
    attrs.write().insert(CompactString::from("clear"), PyObject::native_closure("clear", move |_| {
        if let PyObjectPayload::List(items) = &l.payload { items.write().clear(); }
        Ok(PyObject::none())
    }));
    let l = data.clone();
    attrs.write().insert(CompactString::from("reverse"), PyObject::native_closure("reverse", move |_| {
        if let PyObjectPayload::List(items) = &l.payload { items.write().reverse(); }
        Ok(PyObject::none())
    }));
    let l = data.clone();
    attrs.write().insert(CompactString::from("count"), PyObject::native_closure("count", move |args| {
        if args.is_empty() { return Err(PyException::type_error("count() requires 1 argument")); }
        let target = &args[0];
        if let PyObjectPayload::List(items) = &l.payload {
            let count = items.read().iter().filter(|x| x.compare(target, CompareOp::Eq).map_or(false, |v| v.is_truthy())).count();
            Ok(PyObject::int(count as i64))
        } else { Ok(PyObject::int(0)) }
    }));
    let l = data.clone();
    attrs.write().insert(CompactString::from("index"), PyObject::native_closure("index", move |args| {
        if args.is_empty() { return Err(PyException::type_error("index() requires 1 argument")); }
        let target = &args[0];
        if let PyObjectPayload::List(items) = &l.payload {
            let r = items.read();
            for (i, x) in r.iter().enumerate() {
                if x.compare(target, CompareOp::Eq).map_or(false, |v| v.is_truthy()) {
                    return Ok(PyObject::int(i as i64));
                }
            }
        }
        Err(PyException::value_error("x not in list"))
    }));
    let l = data.clone();
    attrs.write().insert(CompactString::from("sort"), PyObject::native_closure("sort", move |_| {
        if let PyObjectPayload::List(items) = &l.payload {
            let mut w = items.write();
            let mut sorted: Vec<_> = w.drain(..).collect();
            sorted.sort_by(|a, b| a.compare(b, CompareOp::Lt).map_or(std::cmp::Ordering::Equal, |v| if v.is_truthy() { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater }));
            *w = sorted;
        }
        Ok(PyObject::none())
    }));
    let l = data.clone();
    attrs.write().insert(CompactString::from("copy"), PyObject::native_closure("copy", move |_| {
        if let PyObjectPayload::List(items) = &l.payload {
            Ok(PyObject::list(items.read().clone()))
        } else { Ok(PyObject::list(vec![])) }
    }));
}

fn make_user_string_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(CompactString::from("__init__"), make_builtin(|args| {
        if args.is_empty() { return Err(PyException::type_error("UserString.__init__ requires self")); }
        let inst = &args[0];
        let data = if args.len() > 1 {
            PyObject::str_val(CompactString::from(args[1].py_to_string()))
        } else {
            PyObject::str_val(CompactString::from(""))
        };
        if let PyObjectPayload::Instance(d) = &inst.payload {
            d.attrs.write().insert(CompactString::from("data"), data.clone());
            install_string_methods(&d.attrs, &data);
        }
        Ok(PyObject::none())
    }));
    ns.insert(CompactString::from("__str__"), make_builtin(|args| {
        get_user_data(&args[0], "data")
    }));
    ns.insert(CompactString::from("__repr__"), make_builtin(|args| {
        let data = get_user_data(&args[0], "data")?;
        let s = data.as_str().unwrap_or("");
        Ok(PyObject::str_val(CompactString::from(format!("'{}'", s))))
    }));
    ns.insert(CompactString::from("__len__"), make_builtin(|args| {
        let data = get_user_data(&args[0], "data")?;
        let s = data.as_str().unwrap_or("");
        Ok(PyObject::int(s.len() as i64))
    }));
    ns.insert(CompactString::from("__contains__"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("expected item")); }
        let data = get_user_data(&args[0], "data")?;
        let s = data.as_str().unwrap_or("");
        let sub = args[1].py_to_string();
        Ok(PyObject::bool_val(s.contains(&*sub)))
    }));
    ns.insert(CompactString::from("__add__"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("expected other")); }
        let data = get_user_data(&args[0], "data")?;
        let s = data.as_str().unwrap_or("").to_string();
        let other = args[1].py_to_string();
        Ok(PyObject::str_val(CompactString::from(format!("{}{}", s, other))))
    }));
    ns.insert(CompactString::from("__eq__"), make_builtin(|args| {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
        let data = get_user_data(&args[0], "data")?;
        let s = data.as_str().unwrap_or("");
        let other = args[1].py_to_string();
        Ok(PyObject::bool_val(s == other.as_str()))
    }));
    ns.insert(CompactString::from("__getitem__"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("expected index")); }
        let data = get_user_data(&args[0], "data")?;
        let s = data.as_str().unwrap_or("");
        let idx = args[1].to_int()? as i64;
        let len = s.chars().count() as i64;
        let i = if idx < 0 { (len + idx).max(0) as usize } else { idx as usize };
        match s.chars().nth(i) {
            Some(c) => Ok(PyObject::str_val(CompactString::from(c.to_string()))),
            None => Err(PyException::index_error("string index out of range")),
        }
    }));
    ns.insert(CompactString::from("__iter__"), make_builtin(|args| {
        let data = get_user_data(&args[0], "data")?;
        let s = data.as_str().unwrap_or("").to_string();
        let chars: Vec<PyObjectRef> = s.chars()
            .map(|c| PyObject::str_val(CompactString::from(c.to_string())))
            .collect();
        Ok(PyObject::list(chars))
    }));
    ns.insert(CompactString::from("__mul__"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("expected int")); }
        let data = get_user_data(&args[0], "data")?;
        let s = data.as_str().unwrap_or("");
        let n = args[1].to_int()?.max(0) as usize;
        Ok(PyObject::str_val(CompactString::from(s.repeat(n))))
    }));
    ns.insert(CompactString::from("__bool__"), make_builtin(|args| {
        let data = get_user_data(&args[0], "data")?;
        let s = data.as_str().unwrap_or("");
        Ok(PyObject::bool_val(!s.is_empty()))
    }));
    ns.insert(CompactString::from("__hash__"), make_builtin(|args| {
        let data = get_user_data(&args[0], "data")?;
        let s = data.as_str().unwrap_or("");
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        s.hash(&mut hasher);
        Ok(PyObject::int(hasher.finish() as i64))
    }));
    ns.insert(CompactString::from("encode"), make_builtin(|args| {
        let data = get_user_data(&args[0], "data")?;
        // Delegate to data.encode(...) via str method dispatch
        if let Some(enc_fn) = data.get_attr("encode") {
            ferrython_core::object::helpers::call_callable(&enc_fn, &args[1..])
        } else {
            Err(PyException::type_error("str has no encode method"))
        }
    }));
    ns.insert(CompactString::from("__mod__"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("expected value")); }
        let data = get_user_data(&args[0], "data")?;
        // Delegate to str.__mod__
        data.modulo(&args[1])
    }));
    ns.insert(CompactString::from("__rmod__"), make_builtin(|args| {
        if args.len() < 2 { return Err(PyException::type_error("expected value")); }
        let data = get_user_data(&args[0], "data")?;
        // __rmod__: other % self => other % self.data
        args[1].modulo(&data)
    }));
    PyObject::class(CompactString::from("UserString"), vec![], ns)
}

fn install_string_methods(attrs: &SharedFxAttrMap, data: &PyObjectRef) {
    let s_val = data.as_str().unwrap_or("").to_string();

    macro_rules! str_method {
        ($attrs:expr, $name:expr, $s:expr, $body:expr) => {{
            let captured = $s.clone();
            $attrs.write().insert(CompactString::from($name), PyObject::native_closure($name, move |args| {
                let s = &captured;
                #[allow(clippy::redundant_closure_call)]
                ($body)(s, args)
            }));
        }};
    }

    str_method!(attrs, "upper", s_val, |s: &String, _args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        Ok(PyObject::str_val(CompactString::from(s.to_uppercase())))
    });
    str_method!(attrs, "lower", s_val, |s: &String, _args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        Ok(PyObject::str_val(CompactString::from(s.to_lowercase())))
    });
    str_method!(attrs, "strip", s_val, |s: &String, _args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        Ok(PyObject::str_val(CompactString::from(s.trim())))
    });
    str_method!(attrs, "lstrip", s_val, |s: &String, _args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        Ok(PyObject::str_val(CompactString::from(s.trim_start())))
    });
    str_method!(attrs, "rstrip", s_val, |s: &String, _args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        Ok(PyObject::str_val(CompactString::from(s.trim_end())))
    });
    str_method!(attrs, "title", s_val, |s: &String, _args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
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
    str_method!(attrs, "capitalize", s_val, |s: &String, _args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        let mut chars = s.chars();
        let cap = match chars.next() {
            None => String::new(),
            Some(c) => c.to_uppercase().to_string() + &chars.as_str().to_lowercase(),
        };
        Ok(PyObject::str_val(CompactString::from(cap)))
    });
    str_method!(attrs, "swapcase", s_val, |s: &String, _args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        let swapped: String = s.chars().map(|c| {
            if c.is_uppercase() { c.to_lowercase().to_string() }
            else if c.is_lowercase() { c.to_uppercase().to_string() }
            else { c.to_string() }
        }).collect();
        Ok(PyObject::str_val(CompactString::from(swapped)))
    });
    str_method!(attrs, "split", s_val, |s: &String, args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        let parts: Vec<PyObjectRef> = if args.is_empty() || matches!(args[0].payload, PyObjectPayload::None) {
            s.split_whitespace().map(|p| PyObject::str_from_utf8_slice(p.as_bytes())).collect()
        } else if let PyObjectPayload::Str(sr) = &args[0].payload {
            s.split(sr.as_str()).map(|p| PyObject::str_from_utf8_slice(p.as_bytes())).collect()
        } else {
            let sep = args[0].py_to_string();
            s.split(&*sep).map(|p| PyObject::str_from_utf8_slice(p.as_bytes())).collect()
        };
        Ok(PyObject::list(parts))
    });
    str_method!(attrs, "rsplit", s_val, |s: &String, args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        let parts: Vec<PyObjectRef> = if args.is_empty() || matches!(args[0].payload, PyObjectPayload::None) {
            s.split_whitespace().rev().map(|p| PyObject::str_from_utf8_slice(p.as_bytes())).collect()
        } else if let PyObjectPayload::Str(sr) = &args[0].payload {
            s.rsplit(sr.as_str()).map(|p| PyObject::str_from_utf8_slice(p.as_bytes())).collect()
        } else {
            let sep = args[0].py_to_string();
            s.rsplit(&*sep).map(|p| PyObject::str_from_utf8_slice(p.as_bytes())).collect()
        };
        Ok(PyObject::list(parts))
    });
    str_method!(attrs, "replace", s_val, |s: &String, args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("replace() requires at least 2 arguments")); }
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
    str_method!(attrs, "find", s_val, |s: &String, args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("find() requires 1 argument")); }
        let idx = if let PyObjectPayload::Str(sr) = &args[0].payload {
            s.find(sr.as_str())
        } else {
            let sub = args[0].py_to_string();
            s.find(&*sub)
        };
        Ok(PyObject::int(idx.map_or(-1, |i| i as i64)))
    });
    str_method!(attrs, "rfind", s_val, |s: &String, args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("rfind() requires 1 argument")); }
        let idx = if let PyObjectPayload::Str(sr) = &args[0].payload {
            s.rfind(sr.as_str())
        } else {
            let sub = args[0].py_to_string();
            s.rfind(&*sub)
        };
        Ok(PyObject::int(idx.map_or(-1, |i| i as i64)))
    });
    str_method!(attrs, "count", s_val, |s: &String, args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("count() requires 1 argument")); }
        let n = if let PyObjectPayload::Str(sr) = &args[0].payload {
            s.matches(sr.as_str()).count()
        } else {
            let sub = args[0].py_to_string();
            s.matches(&*sub).count()
        };
        Ok(PyObject::int(n as i64))
    });
    str_method!(attrs, "startswith", s_val, |s: &String, args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("startswith() requires 1 argument")); }
        let result = if let PyObjectPayload::Str(sr) = &args[0].payload {
            s.starts_with(sr.as_str())
        } else {
            let prefix = args[0].py_to_string();
            s.starts_with(&*prefix)
        };
        Ok(PyObject::bool_val(result))
    });
    str_method!(attrs, "endswith", s_val, |s: &String, args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("endswith() requires 1 argument")); }
        let result = if let PyObjectPayload::Str(sr) = &args[0].payload {
            s.ends_with(sr.as_str())
        } else {
            let suffix = args[0].py_to_string();
            s.ends_with(&*suffix)
        };
        Ok(PyObject::bool_val(result))
    });
    str_method!(attrs, "join", s_val, |s: &String, args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("join() requires 1 argument")); }
        // Direct access to list/tuple data via data_ptr — avoids to_list() Vec clone
        let (items_slice, _owned): (&[PyObjectRef], Option<Vec<PyObjectRef>>) = match &args[0].payload {
            PyObjectPayload::List(v) => {
                let vec = unsafe { &*v.data_ptr() };
                (vec.as_slice(), None)
            }
            PyObjectPayload::Tuple(v) => (&**v, None),
            _ => {
                let list = args[0].to_list()?;
                // Need owned Vec to live long enough — store it and take slice
                (unsafe { std::slice::from_raw_parts(list.as_ptr(), list.len()) }, Some(list))
            }
        };
        if items_slice.is_empty() {
            return Ok(PyObject::str_val(CompactString::new("")));
        }
        // Single-allocation join: pre-compute total length, then build
        let sep_len = s.len();
        let mut total_len = 0usize;
        for (i, item) in items_slice.iter().enumerate() {
            if i > 0 { total_len += sep_len; }
            if let PyObjectPayload::Str(sr) = &item.payload {
                total_len += sr.as_str().len();
            } else {
                total_len += item.py_to_string().len();
            }
        }
        let mut result = String::with_capacity(total_len);
        for (i, item) in items_slice.iter().enumerate() {
            if i > 0 { result.push_str(s); }
            if let PyObjectPayload::Str(sr) = &item.payload {
                result.push_str(sr.as_str());
            } else {
                result.push_str(&item.py_to_string());
            }
        }
        Ok(PyObject::str_from_utf8_slice(result.as_bytes()))
    });
    str_method!(attrs, "isalpha", s_val, |s: &String, _args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        Ok(PyObject::bool_val(!s.is_empty() && s.chars().all(|c| c.is_alphabetic())))
    });
    str_method!(attrs, "isdigit", s_val, |s: &String, _args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        Ok(PyObject::bool_val(!s.is_empty() && s.chars().all(|c| c.is_ascii_digit())))
    });
    str_method!(attrs, "isalnum", s_val, |s: &String, _args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        Ok(PyObject::bool_val(!s.is_empty() && s.chars().all(|c| c.is_alphanumeric())))
    });
    str_method!(attrs, "isspace", s_val, |s: &String, _args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        Ok(PyObject::bool_val(!s.is_empty() && s.chars().all(|c| c.is_whitespace())))
    });
    str_method!(attrs, "isupper", s_val, |s: &String, _args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        Ok(PyObject::bool_val(s.chars().any(|c| c.is_uppercase()) && !s.chars().any(|c| c.is_lowercase())))
    });
    str_method!(attrs, "islower", s_val, |s: &String, _args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        Ok(PyObject::bool_val(s.chars().any(|c| c.is_lowercase()) && !s.chars().any(|c| c.is_uppercase())))
    });
}

fn get_user_data(obj: &PyObjectRef, attr: &str) -> PyResult<PyObjectRef> {
    if let PyObjectPayload::Instance(d) = &obj.payload {
        if let Some(v) = d.attrs.read().get(attr) {
            return Ok(v.clone());
        }
    }
    Err(PyException::attribute_error(format!("'{}' object has no attribute '{}'", obj.type_name(), attr)))
}

/// _count_elements(mapping, iterable) — C accelerator for Counter.__init__
fn count_elements(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("_count_elements requires 2 arguments"));
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
