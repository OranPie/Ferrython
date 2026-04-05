use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    InstanceData,
    make_module, make_builtin,
    CompareOp,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

pub fn create_collections_module() -> PyObjectRef {
    make_module("collections", vec![
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
        // Counter helper functions (usable as Counter.elements(c), Counter.update(c, other), etc.)
        ("counter_elements", make_builtin(counter_elements)),
        ("counter_update", make_builtin(counter_update)),
        ("counter_subtract", make_builtin(counter_subtract)),
        ("counter_total", make_builtin(counter_total)),
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
    
    let data = Arc::new(RwLock::new(items));
    
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
            for item in items.into_iter().rev() {
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
                Arc::new(std::sync::Mutex::new(ferrython_core::object::IteratorData::List {
                    items: snapshot,
                    index: 0,
                }))
            )))
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
            let idx = args[0].to_int()?;
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
        // Share the same Arc so mutations through closures are visible via _data
        attrs.insert(CompactString::from("_data"), PyObject::wrap(PyObjectPayload::List(data.clone())));
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
        is_special: true, dict_storage: Some(Arc::new(RwLock::new(merged))),
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

// --- UserDict / UserList / UserString ---

fn make_user_dict_class() -> PyObjectRef {
    let mut ns = IndexMap::new();
    ns.insert(CompactString::from("__init__"), make_builtin(|args| {
        if args.is_empty() { return Err(PyException::type_error("UserDict.__init__ requires self")); }
        let inst = &args[0];
        let data = if args.len() > 1 {
            if let PyObjectPayload::Dict(d) = &args[1].payload {
                PyObject::wrap(PyObjectPayload::Dict(Arc::new(RwLock::new(d.read().clone()))))
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
    PyObject::class(CompactString::from("UserDict"), vec![], ns)
}

fn install_dict_methods(attrs: &Arc<RwLock<IndexMap<CompactString, PyObjectRef>>>, data: &PyObjectRef) {
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
    PyObject::class(CompactString::from("UserList"), vec![], ns)
}

fn install_list_methods(attrs: &Arc<RwLock<IndexMap<CompactString, PyObjectRef>>>, data: &PyObjectRef) {
    let list = if let PyObjectPayload::List(l) = &data.payload { l.clone() } else { return; };
    let l = list.clone();
    attrs.write().insert(CompactString::from("append"), PyObject::native_closure("append", move |args| {
        if args.is_empty() { return Err(PyException::type_error("append() requires 1 argument")); }
        l.write().push(args[0].clone());
        Ok(PyObject::none())
    }));
    let l = list.clone();
    attrs.write().insert(CompactString::from("extend"), PyObject::native_closure("extend", move |args| {
        if args.is_empty() { return Err(PyException::type_error("extend() requires 1 argument")); }
        let items = args[0].to_list()?;
        l.write().extend(items);
        Ok(PyObject::none())
    }));
    let l = list.clone();
    attrs.write().insert(CompactString::from("insert"), PyObject::native_closure("insert", move |args| {
        if args.len() < 2 { return Err(PyException::type_error("insert() requires 2 arguments")); }
        let idx = args[0].to_int()? as usize;
        let mut w = l.write();
        let idx = idx.min(w.len());
        w.insert(idx, args[1].clone());
        Ok(PyObject::none())
    }));
    let l = list.clone();
    attrs.write().insert(CompactString::from("pop"), PyObject::native_closure("pop", move |args| {
        let mut w = l.write();
        if w.is_empty() { return Err(PyException::index_error("pop from empty list")); }
        let idx = if !args.is_empty() {
            let i = args[0].to_int()? as i64;
            let len = w.len() as i64;
            (if i < 0 { (len + i).max(0) } else { i.min(len - 1) }) as usize
        } else { w.len() - 1 };
        if idx < w.len() { Ok(w.remove(idx)) } else { Err(PyException::index_error("pop index out of range")) }
    }));
    let l = list.clone();
    attrs.write().insert(CompactString::from("remove"), PyObject::native_closure("remove", move |args| {
        if args.is_empty() { return Err(PyException::type_error("remove() requires 1 argument")); }
        let mut w = l.write();
        let target = &args[0];
        if let Some(pos) = w.iter().position(|x| x.compare(target, CompareOp::Eq).map_or(false, |v| v.is_truthy())) {
            w.remove(pos);
            Ok(PyObject::none())
        } else {
            Err(PyException::value_error("list.remove(x): x not in list"))
        }
    }));
    let l = list.clone();
    attrs.write().insert(CompactString::from("clear"), PyObject::native_closure("clear", move |_| {
        l.write().clear();
        Ok(PyObject::none())
    }));
    let l = list.clone();
    attrs.write().insert(CompactString::from("reverse"), PyObject::native_closure("reverse", move |_| {
        l.write().reverse();
        Ok(PyObject::none())
    }));
    let l = list.clone();
    attrs.write().insert(CompactString::from("count"), PyObject::native_closure("count", move |args| {
        if args.is_empty() { return Err(PyException::type_error("count() requires 1 argument")); }
        let target = &args[0];
        let count = l.read().iter().filter(|x| x.compare(target, CompareOp::Eq).map_or(false, |v| v.is_truthy())).count();
        Ok(PyObject::int(count as i64))
    }));
    let l = list.clone();
    attrs.write().insert(CompactString::from("index"), PyObject::native_closure("index", move |args| {
        if args.is_empty() { return Err(PyException::type_error("index() requires 1 argument")); }
        let target = &args[0];
        let r = l.read();
        for (i, x) in r.iter().enumerate() {
            if x.compare(target, CompareOp::Eq).map_or(false, |v| v.is_truthy()) {
                return Ok(PyObject::int(i as i64));
            }
        }
        Err(PyException::value_error("x not in list"))
    }));
    let l = list.clone();
    attrs.write().insert(CompactString::from("sort"), PyObject::native_closure("sort", move |_| {
        let mut w = l.write();
        let mut items: Vec<_> = w.drain(..).collect();
        items.sort_by(|a, b| a.compare(b, CompareOp::Lt).map_or(std::cmp::Ordering::Equal, |v| if v.is_truthy() { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater }));
        *w = items;
        Ok(PyObject::none())
    }));
    let l = list.clone();
    attrs.write().insert(CompactString::from("copy"), PyObject::native_closure("copy", move |_| {
        Ok(PyObject::list(l.read().clone()))
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
    PyObject::class(CompactString::from("UserString"), vec![], ns)
}

fn install_string_methods(attrs: &Arc<RwLock<IndexMap<CompactString, PyObjectRef>>>, data: &PyObjectRef) {
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
            s.split_whitespace().map(|p| PyObject::str_val(CompactString::from(p))).collect()
        } else {
            let sep = args[0].py_to_string();
            s.split(&*sep).map(|p| PyObject::str_val(CompactString::from(p))).collect()
        };
        Ok(PyObject::list(parts))
    });
    str_method!(attrs, "rsplit", s_val, |s: &String, args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        let parts: Vec<PyObjectRef> = if args.is_empty() || matches!(args[0].payload, PyObjectPayload::None) {
            s.split_whitespace().rev().map(|p| PyObject::str_val(CompactString::from(p))).collect()
        } else {
            let sep = args[0].py_to_string();
            s.rsplit(&*sep).map(|p| PyObject::str_val(CompactString::from(p))).collect()
        };
        Ok(PyObject::list(parts))
    });
    str_method!(attrs, "replace", s_val, |s: &String, args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("replace() requires at least 2 arguments")); }
        let old = args[0].py_to_string();
        let new = args[1].py_to_string();
        Ok(PyObject::str_val(CompactString::from(s.replace(&*old, &*new))))
    });
    str_method!(attrs, "find", s_val, |s: &String, args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("find() requires 1 argument")); }
        let sub = args[0].py_to_string();
        Ok(PyObject::int(s.find(&*sub).map_or(-1, |i| i as i64)))
    });
    str_method!(attrs, "rfind", s_val, |s: &String, args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("rfind() requires 1 argument")); }
        let sub = args[0].py_to_string();
        Ok(PyObject::int(s.rfind(&*sub).map_or(-1, |i| i as i64)))
    });
    str_method!(attrs, "count", s_val, |s: &String, args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("count() requires 1 argument")); }
        let sub = args[0].py_to_string();
        Ok(PyObject::int(s.matches(&*sub).count() as i64))
    });
    str_method!(attrs, "startswith", s_val, |s: &String, args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("startswith() requires 1 argument")); }
        let prefix = args[0].py_to_string();
        Ok(PyObject::bool_val(s.starts_with(&*prefix)))
    });
    str_method!(attrs, "endswith", s_val, |s: &String, args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("endswith() requires 1 argument")); }
        let suffix = args[0].py_to_string();
        Ok(PyObject::bool_val(s.ends_with(&*suffix)))
    });
    str_method!(attrs, "join", s_val, |s: &String, args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
        if args.is_empty() { return Err(PyException::type_error("join() requires 1 argument")); }
        let items = args[0].to_list()?;
        let strs: Vec<String> = items.iter().map(|x| x.py_to_string()).collect();
        Ok(PyObject::str_val(CompactString::from(strs.join(s))))
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
