//! Collection and functional stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    IteratorData, InstanceData, CompareOp,
    make_module, make_builtin, check_args, check_args_min,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::{Arc, Mutex};

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


pub fn create_functools_module() -> PyObjectRef {
    make_module("functools", vec![
        ("reduce", PyObject::native_function("functools.reduce", functools_reduce)),
        ("partial", PyObject::native_function("functools.partial", functools_partial)),
        ("cmp_to_key", make_builtin(functools_cmp_to_key)),
        ("lru_cache", make_builtin(|args| {
            // lru_cache(func) or lru_cache(maxsize=N)(func) — returns a cached wrapper
            if args.is_empty() { 
                // @lru_cache() with no args — return decorator
                return Ok(make_builtin(|args| {
                    if args.is_empty() { return Ok(PyObject::none()); }
                    Ok(create_cached_function(args[0].clone()))
                }));
            }
            match &args[0].payload {
                PyObjectPayload::Function(_) | PyObjectPayload::NativeFunction { .. } 
                | PyObjectPayload::BuiltinFunction(_) => {
                    // @lru_cache directly on function
                    Ok(create_cached_function(args[0].clone()))
                }
                _ => {
                    // Called with maxsize parameter — return decorator
                    Ok(make_builtin(|args| {
                        if args.is_empty() { return Ok(PyObject::none()); }
                        Ok(create_cached_function(args[0].clone()))
                    }))
                }
            }
        })),
        ("wraps", PyObject::native_function("functools.wraps", |args| {
            // wraps(wrapped) returns a decorator that copies __name__, __doc__, etc.
            if args.is_empty() { return Ok(PyObject::none()); }
            let wrapped = args[0].clone();
            let decorator = PyObject::native_closure("functools.wraps.decorator", move |wrapper_args| {
                if wrapper_args.is_empty() { return Ok(PyObject::none()); }
                let wrapper = &wrapper_args[0];
                // Copy __name__, __doc__, __wrapped__ from wrapped to wrapper
                let copy_attr = |attr_name: &str| {
                    if let Some(val) = wrapped.get_attr(attr_name) {
                        if let PyObjectPayload::Instance(ref d) = wrapper.payload {
                            d.attrs.write().insert(CompactString::from(attr_name), val);
                        } else if let PyObjectPayload::Function(ref fd) = wrapper.payload {
                            fd.attrs.write().insert(CompactString::from(attr_name), val);
                        }
                    }
                };
                copy_attr("__name__");
                copy_attr("__doc__");
                copy_attr("__module__");
                copy_attr("__qualname__");
                copy_attr("__dict__");
                // Store __wrapped__ reference
                if let PyObjectPayload::Instance(ref d) = wrapper.payload {
                    d.attrs.write().insert(CompactString::from("__wrapped__"), wrapped.clone());
                } else if let PyObjectPayload::Function(ref fd) = wrapper.payload {
                    fd.attrs.write().insert(CompactString::from("__wrapped__"), wrapped.clone());
                }
                Ok(wrapper.clone())
            });
            Ok(decorator)
        })),
        ("cached_property", make_builtin(|args| {
            if args.is_empty() { return Err(PyException::type_error("cached_property requires 1 argument")); }
            let func = args[0].clone();
            // Create a marker instance that the descriptor protocol recognizes
            let cls = PyObject::class(CompactString::from("cached_property"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref id) = inst.payload {
                let mut attrs = id.attrs.write();
                attrs.insert(CompactString::from("__cached_property_func__"), func.clone());
                // Store the function name for the attr name
                let name = func.get_attr("__name__")
                    .map(|n| n.py_to_string())
                    .unwrap_or_else(|| "cached".to_string());
                attrs.insert(CompactString::from("__name__"), PyObject::str_val(CompactString::from(name)));
            }
            Ok(inst)
        })),
        ("total_ordering", make_builtin(functools_total_ordering)),
        ("singledispatch", make_builtin(|args| {
            // Stub — return the function unchanged to enable imports
            if args.is_empty() { return Err(PyException::type_error("singledispatch requires 1 argument")); }
            Ok(args[0].clone())
        })),
        ("update_wrapper", PyObject::native_function("functools.update_wrapper", |args| {
            // update_wrapper(wrapper, wrapped) — copy attrs from wrapped to wrapper
            if args.len() < 2 { return Err(PyException::type_error("update_wrapper requires at least 2 arguments")); }
            let wrapper = &args[0];
            let wrapped = &args[1];
            let copy_attr = |attr_name: &str| {
                if let Some(val) = wrapped.get_attr(attr_name) {
                    if let PyObjectPayload::Instance(ref d) = wrapper.payload {
                        d.attrs.write().insert(CompactString::from(attr_name), val);
                    } else if let PyObjectPayload::Function(ref fd) = wrapper.payload {
                        fd.attrs.write().insert(CompactString::from(attr_name), val);
                    }
                }
            };
            copy_attr("__name__");
            copy_attr("__doc__");
            copy_attr("__module__");
            copy_attr("__qualname__");
            copy_attr("__dict__");
            // Store __wrapped__ reference
            if let PyObjectPayload::Instance(ref d) = wrapper.payload {
                d.attrs.write().insert(CompactString::from("__wrapped__"), wrapped.clone());
            } else if let PyObjectPayload::Function(ref fd) = wrapper.payload {
                fd.attrs.write().insert(CompactString::from("__wrapped__"), wrapped.clone());
            }
            Ok(wrapper.clone())
        })),
    ])
}

fn functools_total_ordering(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("total_ordering requires 1 argument")); }
    let cls = &args[0];
    if let PyObjectPayload::Class(cd) = &cls.payload {
        let has = |name: &str| -> bool {
            cd.namespace.read().contains_key(name)
        };
        // Determine the root method
        let root = if has("__lt__") { "__lt__" }
            else if has("__gt__") { "__gt__" }
            else if has("__le__") { "__le__" }
            else if has("__ge__") { "__ge__" }
            else { return Ok(args[0].clone()); };

        let mut ns = cd.namespace.write();
        // Set marker for VM to derive missing comparisons
        ns.insert(CompactString::from("__total_ordering_root__"),
            PyObject::str_val(CompactString::from(root)));
    }
    Ok(args[0].clone())
}

fn functools_partial(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("partial() requires at least 1 argument")); }
    let func = args[0].clone();
    let partial_args = if args.len() > 1 { args[1..].to_vec() } else { vec![] };
    Ok(PyObject::wrap(PyObjectPayload::Partial {
        func,
        args: partial_args,
        kwargs: vec![],
    }))
}

fn functools_reduce(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("reduce() requires at least 2 arguments")); }
    let func = args[0].clone();
    let items = args[1].to_list()?;
    let acc = if args.len() > 2 {
        args[2].clone()
    } else if !items.is_empty() {
        items[0].clone()
    } else {
        return Err(PyException::type_error("reduce() of empty sequence with no initial value"));
    };
    let start_idx = if args.len() > 2 { 0 } else { 1 };
    for item in &items[start_idx..] {
        // Call func(acc, item) — but we're a builtin, so we can't easily call Python funcs here.
        // This would need VM access; for now we'll return a stub error.
        let _ = func;
        let _ = item;
        return Err(PyException::type_error("functools.reduce not fully implemented yet"));
    }
    Ok(acc)
}

/// functools.cmp_to_key(cmp_func)
/// Returns a key class whose instances compare using the given comparison function.
/// Since native functions can't call Python functions, we create a wrapper instance
/// that stores both the comparison function and the wrapped value.
fn functools_cmp_to_key(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("cmp_to_key() requires 1 argument"));
    }
    let cmp_func = args[0].clone();
    // Create a key class that wraps values and stores the comparison function
    let mut namespace = IndexMap::new();
    namespace.insert(CompactString::from("__cmp_to_key__"), PyObject::bool_val(true));
    namespace.insert(CompactString::from("_cmp_func"), cmp_func);
    let key_cls = PyObject::class(
        CompactString::from("cmp_to_key"),
        vec![],
        namespace,
    );
    Ok(key_cls)
}


pub fn create_itertools_module() -> PyObjectRef {
    // chain is a callable object with a from_iterable class method attribute
    let chain_class = PyObject::class(
        CompactString::from("chain"),
        vec![],
        IndexMap::new(),
    );
    let chain_inst = PyObject::instance(chain_class);
    if let PyObjectPayload::Instance(ref d) = chain_inst.payload {
        let mut attrs = d.attrs.write();
        attrs.insert(CompactString::from("__call__"), make_builtin(itertools_chain));
        attrs.insert(CompactString::from("from_iterable"), make_builtin(itertools_chain_from_iterable));
        attrs.insert(CompactString::from("__itertools_chain__"), PyObject::bool_val(true));
    }

    make_module("itertools", vec![
        ("count", make_builtin(itertools_count)),
        ("chain", chain_inst),
        ("repeat", make_builtin(itertools_repeat)),
        ("cycle", make_builtin(itertools_cycle)),
        ("islice", PyObject::native_function("itertools.islice", itertools_islice)),
        ("zip_longest", make_builtin(itertools_zip_longest)),
        ("product", make_builtin(itertools_product)),
        ("accumulate", PyObject::native_function("itertools.accumulate", itertools_accumulate)),
        ("dropwhile", make_builtin(itertools_dropwhile)),
        ("takewhile", make_builtin(itertools_takewhile)),
        ("combinations", make_builtin(itertools_combinations)),
        ("combinations_with_replacement", make_builtin(itertools_combinations_with_replacement)),
        ("permutations", make_builtin(itertools_permutations)),
        ("groupby", PyObject::native_function("itertools.groupby", itertools_groupby)),
        ("filterfalse", PyObject::native_function("itertools.filterfalse", itertools_filterfalse)),
        ("compress", make_builtin(itertools_compress)),
        ("tee", make_builtin(itertools_tee)),
        ("starmap", PyObject::native_function("itertools.starmap", itertools_starmap)),
    ])
}

fn itertools_count(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let start = if args.is_empty() { 0i64 } else { args[0].to_int()? };
    let step = if args.len() >= 2 { args[1].to_int()? } else { 1 };
    // Return a list-based iterator with a large range (lazy would be better, but this works)
    let items: Vec<PyObjectRef> = (0..1000).map(|i| PyObject::int(start + i * step)).collect();
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(
        IteratorData::List { items, index: 0 }
    )))))
}

fn itertools_chain(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let mut all_items = Vec::new();
    for arg in args {
        let items = arg.to_list()?;
        all_items.extend(items);
    }
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(
        IteratorData::List { items: all_items, index: 0 }
    )))))
}

fn itertools_repeat(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("repeat() missing required argument"));
    }
    let item = args[0].clone();
    let count = if args.len() >= 2 { args[1].to_int()? as usize } else { 100 };
    let items: Vec<PyObjectRef> = std::iter::repeat(item).take(count).collect();
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(
        IteratorData::List { items, index: 0 }
    )))))
}

fn itertools_cycle(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("cycle() missing required argument"));
    }
    let base = args[0].to_list()?;
    if base.is_empty() {
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(
            IteratorData::List { items: vec![], index: 0 }
        )))));
    }
    // Materialize a reasonable number of cycles
    let mut items = Vec::new();
    for _ in 0..1000 {
        items.extend(base.iter().cloned());
    }
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(
        IteratorData::List { items, index: 0 }
    )))))
}

fn itertools_islice(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 {
        return Err(PyException::type_error("islice() requires at least 2 arguments"));
    }
    let items = args[0].to_list()?;
    let (start, stop, step) = if args.len() == 2 {
        (0usize, args[1].to_int()? as usize, 1usize)
    } else if args.len() == 3 {
        let s = if matches!(&args[1].payload, PyObjectPayload::None) { 0 } else { args[1].to_int()? as usize };
        (s, args[2].to_int()? as usize, 1usize)
    } else {
        let s = if matches!(&args[1].payload, PyObjectPayload::None) { 0 } else { args[1].to_int()? as usize };
        let step = if matches!(&args[3].payload, PyObjectPayload::None) { 1 } else { args[3].to_int()? as usize };
        (s, args[2].to_int()? as usize, step)
    };
    let result: Vec<PyObjectRef> = items.into_iter()
        .skip(start)
        .take(stop - start)
        .step_by(step.max(1))
        .collect();
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(
        IteratorData::List { items: result, index: 0 }
    )))))
}

fn itertools_zip_longest(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // Check for trailing kwargs dict (from kw dispatch)
    let mut fillvalue = PyObject::none();
    let iter_args = if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(map) = &last.payload {
            let map_r = map.read();
            if let Some(fv) = map_r.get(&HashableKey::Str(CompactString::from("fillvalue"))) {
                fillvalue = fv.clone();
            }
            &args[..args.len() - 1]
        } else {
            args
        }
    } else {
        args
    };
    let lists: Vec<Vec<PyObjectRef>> = iter_args.iter()
        .map(|a| a.to_list())
        .collect::<Result<Vec<_>, _>>()?;
    let max_len = lists.iter().map(|l| l.len()).max().unwrap_or(0);
    let mut result = Vec::new();
    for i in 0..max_len {
        let tuple: Vec<PyObjectRef> = lists.iter()
            .map(|l| l.get(i).cloned().unwrap_or_else(|| fillvalue.clone()))
            .collect();
        result.push(PyObject::tuple(tuple));
    }
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(
        IteratorData::List { items: result, index: 0 }
    )))))
}

fn itertools_product(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(
            IteratorData::List { items: vec![PyObject::tuple(vec![])], index: 0 }
        )))));
    }
    // Check for trailing kwargs dict with repeat=
    let (pos_args, repeat) = if let Some(last) = args.last() {
        if let PyObjectPayload::Dict(map) = &last.payload {
            let map = map.read();
            let r = map.get(&HashableKey::Str(CompactString::from("repeat")))
                .and_then(|v| v.as_int().map(|n| n as usize))
                .unwrap_or(1);
            (&args[..args.len() - 1], r)
        } else {
            (args, 1)
        }
    } else {
        (args, 1)
    };
    let mut lists: Vec<Vec<PyObjectRef>> = pos_args.iter()
        .map(|a| a.to_list())
        .collect::<Result<Vec<_>, _>>()?;
    // Apply repeat: duplicate the iterables
    if repeat > 1 {
        let base = lists.clone();
        for _ in 1..repeat {
            lists.extend(base.clone());
        }
    }
    let mut result = vec![vec![]];
    for lst in &lists {
        let mut new_result = Vec::new();
        for prefix in &result {
            for item in lst {
                let mut combo = prefix.clone();
                combo.push(item.clone());
                new_result.push(combo);
            }
        }
        result = new_result;
    }
    let items: Vec<PyObjectRef> = result.into_iter()
        .map(|combo| PyObject::tuple(combo))
        .collect();
    Ok(PyObject::wrap(PyObjectPayload::Iterator(Arc::new(Mutex::new(
        IteratorData::List { items, index: 0 }
    )))))
}

fn itertools_accumulate(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("accumulate requires an iterable")); }
    let items = args[0].to_list()?;
    // Optional binary function as second arg
    let func = if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
        Some(args[1].clone())
    } else {
        None
    };
    // Optional initial value as third arg
    let initial = if args.len() >= 3 && !matches!(&args[2].payload, PyObjectPayload::None) {
        Some(args[2].clone())
    } else {
        None
    };
    let has_initial = initial.is_some();
    if items.is_empty() {
        return if let Some(init) = initial {
            Ok(PyObject::list(vec![init]))
        } else {
            Ok(PyObject::list(vec![]))
        };
    }
    let mut result = Vec::new();
    let mut acc = if let Some(init) = initial {
        result.push(init.clone());
        init
    } else {
        result.push(items[0].clone());
        items[0].clone()
    };
    let iter_items = if has_initial { &items[..] } else { &items[1..] };
    for item in iter_items {
        acc = if let Some(ref f) = func {
            match &f.payload {
                PyObjectPayload::NativeFunction { func: nf, .. } => nf(&[acc, item.clone()])?,
                PyObjectPayload::NativeClosure { func: nf, .. } => nf(&[acc, item.clone()])?,
                _ => {
                    let a = acc.to_float().unwrap_or(acc.as_int().unwrap_or(0) as f64);
                    let b = item.to_float().unwrap_or(item.as_int().unwrap_or(0) as f64);
                    let sum = a + b;
                    if acc.as_int().is_some() && item.as_int().is_some() {
                        PyObject::int(sum as i64)
                    } else {
                        PyObject::float(sum)
                    }
                }
            }
        } else {
            let a = acc.to_float().unwrap_or(acc.as_int().unwrap_or(0) as f64);
            let b = item.to_float().unwrap_or(item.as_int().unwrap_or(0) as f64);
            let sum = a + b;
            if acc.as_int().is_some() && item.as_int().is_some() {
                PyObject::int(sum as i64)
            } else {
                PyObject::float(sum)
            }
        };
        result.push(acc.clone());
    }
    Ok(PyObject::list(result))
}

fn itertools_dropwhile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("dropwhile requires predicate and iterable")); }
    let func = args[0].clone();
    let source = args[1].get_iter()?;
    Ok(PyObject::wrap(PyObjectPayload::Iterator(
        Arc::new(std::sync::Mutex::new(IteratorData::DropWhile { func, source, dropping: true }))
    )))
}

fn itertools_takewhile(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("takewhile requires predicate and iterable")); }
    let func = args[0].clone();
    let source = args[1].get_iter()?;
    Ok(PyObject::wrap(PyObjectPayload::Iterator(
        Arc::new(std::sync::Mutex::new(IteratorData::TakeWhile { func, source, done: false }))
    )))
}

fn itertools_combinations(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("combinations requires iterable and r")); }
    let items = args[0].to_list()?;
    let r = args[1].as_int().unwrap_or(2) as usize;
    let n = items.len();
    if r > n { return Ok(PyObject::list(vec![])); }
    let mut result = Vec::new();
    let mut indices: Vec<usize> = (0..r).collect();
    result.push(PyObject::tuple(indices.iter().map(|&i| items[i].clone()).collect()));
    loop {
        let mut i_opt = None;
        for i in (0..r).rev() {
            if indices[i] != i + n - r {
                i_opt = Some(i);
                break;
            }
        }
        let i = match i_opt { Some(i) => i, None => break };
        indices[i] += 1;
        for j in (i + 1)..r {
            indices[j] = indices[j - 1] + 1;
        }
        result.push(PyObject::tuple(indices.iter().map(|&idx| items[idx].clone()).collect()));
    }
    Ok(PyObject::list(result))
}

fn itertools_combinations_with_replacement(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("combinations_with_replacement requires iterable and r")); }
    let items = args[0].to_list()?;
    let r = args[1].as_int().unwrap_or(2) as usize;
    let n = items.len();
    if n == 0 && r > 0 { return Ok(PyObject::list(vec![])); }
    if r == 0 { return Ok(PyObject::list(vec![PyObject::tuple(vec![])])); }
    let mut result = Vec::new();
    let mut indices: Vec<usize> = vec![0; r];
    result.push(PyObject::tuple(indices.iter().map(|&i| items[i].clone()).collect()));
    loop {
        let mut i_opt = None;
        for i in (0..r).rev() {
            if indices[i] != n - 1 {
                i_opt = Some(i);
                break;
            }
        }
        let i = match i_opt { Some(i) => i, None => break };
        let new_val = indices[i] + 1;
        for j in i..r {
            indices[j] = new_val;
        }
        result.push(PyObject::tuple(indices.iter().map(|&idx| items[idx].clone()).collect()));
    }
    Ok(PyObject::list(result))
}

fn itertools_permutations(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("permutations requires iterable")); }
    let items = args[0].to_list()?;
    let r = if args.len() > 1 { args[1].as_int().unwrap_or(items.len() as i64) as usize } else { items.len() };
    let n = items.len();
    if r > n { return Ok(PyObject::list(vec![])); }
    let mut result = Vec::new();
    let mut indices: Vec<usize> = (0..n).collect();
    let mut cycles: Vec<usize> = (0..r).map(|i| n - i).collect();
    result.push(PyObject::tuple(indices[..r].iter().map(|&i| items[i].clone()).collect()));
    'outer: loop {
        for i in (0..r).rev() {
            cycles[i] -= 1;
            if cycles[i] == 0 {
                let tmp = indices[i];
                for j in i..n-1 { indices[j] = indices[j+1]; }
                indices[n-1] = tmp;
                cycles[i] = n - i;
                if i == 0 { break 'outer; }
            } else {
                let j = n - cycles[i];
                indices.swap(i, j);
                result.push(PyObject::tuple(indices[..r].iter().map(|&idx| items[idx].clone()).collect()));
                continue 'outer;
            }
        }
        break;
    }
    Ok(PyObject::list(result))
}

fn itertools_groupby(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("groupby requires iterable")); }
    let items = args[0].to_list()?;
    if items.is_empty() { return Ok(PyObject::list(vec![])); }
    // Optional key function (second arg)
    let key_fn = if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
        Some(args[1].clone())
    } else {
        None
    };
    // For stdlib groupby without VM-level key call, use identity (py_to_string) for grouping
    let get_key = |item: &PyObjectRef| -> String {
        item.py_to_string()
    };
    let mut result = Vec::new();
    let first_key = get_key(&items[0]);
    let mut current_key_str = first_key;
    let mut current_key_obj = if key_fn.is_some() { items[0].clone() } else { items[0].clone() };
    let mut current_group = vec![items[0].clone()];
    for item in &items[1..] {
        let k = get_key(item);
        if k == current_key_str {
            current_group.push(item.clone());
        } else {
            result.push(PyObject::tuple(vec![
                current_key_obj.clone(),
                PyObject::list(current_group),
            ]));
            current_key_str = k;
            current_key_obj = item.clone();
            current_group = vec![item.clone()];
        }
    }
    result.push(PyObject::tuple(vec![
        current_key_obj,
        PyObject::list(current_group),
    ]));
    let _ = key_fn; // key_fn requires VM-level calls; identity grouping for now
    Ok(PyObject::list(result))
}

fn itertools_chain_from_iterable(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("chain.from_iterable requires iterable")); }
    let outer = args[0].to_list()?;
    let mut result = Vec::new();
    for inner in &outer {
        let items = inner.to_list()?;
        result.extend(items);
    }
    Ok(PyObject::list(result))
}

fn itertools_compress(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("compress requires data and selectors")); }
    let data = args[0].to_list()?;
    let selectors = args[1].to_list()?;
    let mut result = Vec::new();
    for (d, s) in data.iter().zip(selectors.iter()) {
        if s.is_truthy() {
            result.push(d.clone());
        }
    }
    Ok(PyObject::list(result))
}

fn itertools_tee(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() { return Err(PyException::type_error("tee requires iterable")); }
    let items = args[0].to_list()?;
    let n = if args.len() > 1 { args[1].as_int().unwrap_or(2) } else { 2 };
    let copies: Vec<PyObjectRef> = (0..n).map(|_| PyObject::list(items.clone())).collect();
    Ok(PyObject::tuple(copies))
}

fn itertools_filterfalse(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // filterfalse(predicate, iterable) — VM-intercepted for callable predicates
    if args.len() < 2 { return Err(PyException::type_error("filterfalse requires predicate and iterable")); }
    let items = args[1].to_list()?;
    // If predicate is None, filter out truthy values
    if matches!(args[0].payload, PyObjectPayload::None) {
        let result: Vec<PyObjectRef> = items.into_iter().filter(|x| !x.is_truthy()).collect();
        return Ok(PyObject::list(result));
    }
    // For NativeFunction/BuiltinFn predicates, call directly
    Err(PyException::type_error("filterfalse with callable predicate requires VM dispatch"))
}

fn itertools_starmap(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    // starmap(func, iterable) — VM-intercepted for callable functions
    if args.len() < 2 { return Err(PyException::type_error("starmap requires function and iterable")); }
    Err(PyException::type_error("starmap requires VM dispatch"))
}

/// Create a cached wrapper function for lru_cache.
/// Returns an Instance with __wrapped__ (original func) and _cache (dict).
/// The VM intercepts __call__ on instances with _cache to implement caching.
fn create_cached_function(func: PyObjectRef) -> PyObjectRef {
    let cache_class = PyObject::class(
        CompactString::from("_lru_wrapper"),
        vec![],
        IndexMap::new(),
    );
    let cache_dict: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
    let attrs_map: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    let attrs_arc = Arc::new(parking_lot::RwLock::new(attrs_map));

    // Build cache_info closure that reads _hits/_misses from attrs
    let info_attrs = attrs_arc.clone();
    let cache_info_fn = PyObject::native_closure("cache_info", move |_args| {
        let r = info_attrs.read();
        let hits = r.get(&CompactString::from("_hits")).and_then(|v| v.as_int()).unwrap_or(0);
        let misses = r.get(&CompactString::from("_misses")).and_then(|v| v.as_int()).unwrap_or(0);
        let currsize = if let Some(cache) = r.get(&CompactString::from("_cache")) {
            if let PyObjectPayload::Dict(d) = &cache.payload { d.read().len() as i64 } else { 0 }
        } else { 0 };
        let info_class = PyObject::class(CompactString::from("CacheInfo"), vec![], IndexMap::new());
        let info = PyObject::instance(info_class);
        if let PyObjectPayload::Instance(ref d) = info.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("hits"), PyObject::int(hits));
            w.insert(CompactString::from("misses"), PyObject::int(misses));
            w.insert(CompactString::from("maxsize"), PyObject::int(128));
            w.insert(CompactString::from("currsize"), PyObject::int(currsize));
        }
        Ok(info)
    });

    {
        let mut w = attrs_arc.write();
        w.insert(CompactString::from("__wrapped__"), func);
        w.insert(CompactString::from("_cache"), PyObject::dict(cache_dict));
        w.insert(CompactString::from("_hits"), PyObject::int(0));
        w.insert(CompactString::from("_misses"), PyObject::int(0));
        w.insert(CompactString::from("cache_info"), cache_info_fn);
        w.insert(CompactString::from("cache_clear"), PyObject::native_function("cache_clear", |_args| {
            Ok(PyObject::none())
        }));
    }

    // Build the Instance manually with the shared attrs Arc
    Arc::new(PyObject {
        payload: PyObjectPayload::Instance(InstanceData {
            class: cache_class,
            attrs: attrs_arc,
            dict_storage: None,
        }),
    })
}

// ── queue module ──

pub fn create_queue_module() -> PyObjectRef {
    // Queue constructor
    let queue_fn = PyObject::native_closure("Queue", move |args: &[PyObjectRef]| {
        create_queue_instance_full("Queue", args)
    });
    // LifoQueue constructor
    let lifo_fn = PyObject::native_closure("LifoQueue", move |args: &[PyObjectRef]| {
        create_queue_instance_full("LifoQueue", args)
    });
    // PriorityQueue constructor
    let prio_fn = PyObject::native_closure("PriorityQueue", move |args: &[PyObjectRef]| {
        create_queue_instance_full("PriorityQueue", args)
    });

    make_module("queue", vec![
        ("Queue", queue_fn),
        ("LifoQueue", lifo_fn),
        ("PriorityQueue", prio_fn),
        ("Empty", PyObject::class(CompactString::from("Empty"), vec![], IndexMap::new())),
        ("Full", PyObject::class(CompactString::from("Full"), vec![], IndexMap::new())),
    ])
}

fn create_queue_instance_full(kind: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let maxsize = if !args.is_empty() { args[0].to_int().unwrap_or(0) } else { 0 };
    let class = PyObject::class(CompactString::from(kind), vec![], IndexMap::new());
    let inst = PyObject::instance(class);
    let items: Arc<RwLock<Vec<PyObjectRef>>> = Arc::new(RwLock::new(Vec::new()));
    let unfinished = Arc::new(Mutex::new(0i64));
    let is_lifo = kind == "LifoQueue";
    let is_priority = kind == "PriorityQueue";

    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("__queue__"), PyObject::str_val(CompactString::from(kind)));
        w.insert(CompactString::from("maxsize"), PyObject::int(maxsize));

        // put(item)
        let it1 = items.clone();
        let uf1 = unfinished.clone();
        let ms1 = maxsize;
        w.insert(CompactString::from("put"), PyObject::native_closure(
            "put", move |a: &[PyObjectRef]| {
                if a.is_empty() { return Err(PyException::type_error("put() requires 1 argument")); }
                let mut v = it1.write();
                if ms1 > 0 && v.len() as i64 >= ms1 {
                    return Err(PyException::runtime_error("queue is full"));
                }
                if is_priority {
                    // Insert in sorted order (min-heap via sorted Vec)
                    let item = a[0].clone();
                    let pos = v.iter().position(|x| {
                        // Compare: try numeric first, then string
                        if let (Ok(a_val), Ok(b_val)) = (item.to_float(), x.to_float()) {
                            a_val < b_val
                        } else {
                            item.py_to_string() < x.py_to_string()
                        }
                    }).unwrap_or(v.len());
                    v.insert(pos, item);
                } else {
                    v.push(a[0].clone());
                }
                *uf1.lock().unwrap() += 1;
                Ok(PyObject::none())
            }));

        // put_nowait(item) — same as put for single-threaded
        let it1b = items.clone();
        let uf1b = unfinished.clone();
        let ms1b = maxsize;
        w.insert(CompactString::from("put_nowait"), PyObject::native_closure(
            "put_nowait", move |a: &[PyObjectRef]| {
                if a.is_empty() { return Err(PyException::type_error("put_nowait() requires 1 argument")); }
                let mut v = it1b.write();
                if ms1b > 0 && v.len() as i64 >= ms1b {
                    return Err(PyException::runtime_error("queue is full"));
                }
                v.push(a[0].clone());
                *uf1b.lock().unwrap() += 1;
                Ok(PyObject::none())
            }));

        // get()
        let it2 = items.clone();
        w.insert(CompactString::from("get"), PyObject::native_closure(
            "get", move |_: &[PyObjectRef]| {
                let mut v = it2.write();
                if v.is_empty() {
                    return Err(PyException::runtime_error("queue is empty"));
                }
                if is_lifo {
                    Ok(v.pop().unwrap())
                } else {
                    Ok(v.remove(0))
                }
            }));

        // get_nowait() — same as get for single-threaded
        let it2b = items.clone();
        w.insert(CompactString::from("get_nowait"), PyObject::native_closure(
            "get_nowait", move |_: &[PyObjectRef]| {
                let mut v = it2b.write();
                if v.is_empty() {
                    return Err(PyException::runtime_error("queue is empty"));
                }
                if is_lifo {
                    Ok(v.pop().unwrap())
                } else {
                    Ok(v.remove(0))
                }
            }));

        // qsize()
        let it3 = items.clone();
        w.insert(CompactString::from("qsize"), PyObject::native_closure(
            "qsize", move |_: &[PyObjectRef]| {
                Ok(PyObject::int(it3.read().len() as i64))
            }));

        // empty()
        let it4 = items.clone();
        w.insert(CompactString::from("empty"), PyObject::native_closure(
            "empty", move |_: &[PyObjectRef]| {
                Ok(PyObject::bool_val(it4.read().is_empty()))
            }));

        // full()
        let it5 = items.clone();
        let ms2 = maxsize;
        w.insert(CompactString::from("full"), PyObject::native_closure(
            "full", move |_: &[PyObjectRef]| {
                if ms2 <= 0 { return Ok(PyObject::bool_val(false)); }
                Ok(PyObject::bool_val(it5.read().len() as i64 >= ms2))
            }));

        // task_done()
        let uf2 = unfinished.clone();
        w.insert(CompactString::from("task_done"), PyObject::native_closure(
            "task_done", move |_: &[PyObjectRef]| {
                let mut u = uf2.lock().unwrap();
                if *u <= 0 {
                    return Err(PyException::value_error("task_done() called too many times"));
                }
                *u -= 1;
                Ok(PyObject::none())
            }));

        // join() — blocks until all tasks done; stub returns immediately
        let uf3 = unfinished.clone();
        w.insert(CompactString::from("join"), PyObject::native_closure(
            "join", move |_: &[PyObjectRef]| {
                // In single-threaded context, just return
                drop(uf3.lock().unwrap());
                Ok(PyObject::none())
            }));

        // _items for backwards compat
        let it6 = items.clone();
        w.insert(CompactString::from("_items"), PyObject::native_closure(
            "_items", move |_: &[PyObjectRef]| {
                Ok(PyObject::list(it6.read().clone()))
            }));
    }
    Ok(inst)
}

// ── array module ─────────────────────────────────────────────────────
pub fn create_array_module() -> PyObjectRef {
    make_module("array", vec![
        ("array", make_builtin(array_array)),
        ("typecodes", PyObject::str_val(CompactString::from("bBuhHiIlLqQfd"))),
    ])
}

fn array_array(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("array() requires at least 1 argument"));
    }
    let typecode = args[0].py_to_string();
    if typecode.len() != 1 || !"bBuhHiIlLqQfd".contains(&typecode) {
        return Err(PyException::value_error(format!(
            "bad typecode (must be b, B, u, h, H, i, I, l, L, q, Q, f, or d): '{}'", typecode
        )));
    }
    let items = if args.len() > 1 {
        args[1].to_list()?
    } else {
        vec![]
    };
    // Store as list with typecode metadata
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("typecode"), PyObject::str_val(CompactString::from(&typecode)));
    attrs.insert(CompactString::from("_data"), PyObject::list(items));
    attrs.insert(CompactString::from("__array__"), PyObject::bool_val(true));
    Ok(PyObject::instance_with_attrs(
        PyObject::str_val(CompactString::from("array")),
        attrs,
    ))
}

// ── operator module ──


pub fn create_operator_module() -> PyObjectRef {
    make_module("operator", vec![
        ("add", make_builtin(|args| {
            check_args_min("add", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    return Ok(PyObject::int(a + b));
                }
            }
            if let (Ok(a), Ok(b)) = (args[0].to_float(), args[1].to_float()) {
                Ok(PyObject::float(a + b))
            } else {
                let a = args[0].py_to_string();
                let b = args[1].py_to_string();
                Ok(PyObject::str_val(CompactString::from(format!("{}{}", a, b))))
            }
        })),
        ("sub", make_builtin(|args| {
            check_args_min("sub", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    return Ok(PyObject::int(a - b));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a - b))
        })),
        ("mul", make_builtin(|args| {
            check_args_min("mul", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    return Ok(PyObject::int(a * b));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a * b))
        })),
        ("truediv", make_builtin(|args| {
            check_args_min("truediv", args, 2)?;
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            if b == 0.0 { return Err(PyException::zero_division_error("division by zero")); }
            Ok(PyObject::float(a / b))
        })),
        ("floordiv", make_builtin(|args| {
            check_args_min("floordiv", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b == 0 { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                    return Ok(PyObject::int(a.div_euclid(b)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            if b == 0.0 { return Err(PyException::zero_division_error("float floor division by zero")); }
            Ok(PyObject::float((a / b).floor()))
        })),
        ("mod_", make_builtin(|args| {
            check_args_min("mod_", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b == 0 { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                    return Ok(PyObject::int(a.rem_euclid(b)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a % b))
        })),
        // Also register as "mod" for getattr(operator, "mod") usage
        ("mod", make_builtin(|args| {
            check_args_min("mod", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b == 0 { return Err(PyException::zero_division_error("integer division or modulo by zero")); }
                    return Ok(PyObject::int(a.rem_euclid(b)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a % b))
        })),
        ("neg", make_builtin(|args| {
            check_args_min("neg", args, 1)?;
            if matches!(&args[0].payload, PyObjectPayload::Float(_)) {
                Ok(PyObject::float(-args[0].to_float()?))
            } else if let Ok(n) = args[0].to_int() {
                Ok(PyObject::int(-n))
            } else {
                Ok(PyObject::float(-args[0].to_float()?))
            }
        })),
        ("pow", make_builtin(|args| {
            check_args_min("pow", args, 2)?;
            let either_float = matches!(&args[0].payload, PyObjectPayload::Float(_)) || matches!(&args[1].payload, PyObjectPayload::Float(_));
            if !either_float {
                if let (Ok(a), Ok(b)) = (args[0].to_int(), args[1].to_int()) {
                    if b >= 0 {
                        return Ok(PyObject::int(a.pow(b as u32)));
                    }
                    return Ok(PyObject::float((a as f64).powf(b as f64)));
                }
            }
            let a = args[0].to_float()?;
            let b = args[1].to_float()?;
            Ok(PyObject::float(a.powf(b)))
        })),
        ("pos", make_builtin(|args| {
            check_args_min("pos", args, 1)?;
            Ok(args[0].clone())
        })),
        ("not_", make_builtin(|args| {
            check_args_min("not_", args, 1)?;
            Ok(PyObject::bool_val(!args[0].is_truthy()))
        })),
        ("eq", make_builtin(|args| {
            check_args_min("eq", args, 2)?;
            args[0].compare(&args[1], CompareOp::Eq)
        })),
        ("ne", make_builtin(|args| {
            check_args_min("ne", args, 2)?;
            args[0].compare(&args[1], CompareOp::Ne)
        })),
        ("lt", make_builtin(|args| {
            check_args_min("lt", args, 2)?;
            args[0].compare(&args[1], CompareOp::Lt)
        })),
        ("le", make_builtin(|args| {
            check_args_min("le", args, 2)?;
            args[0].compare(&args[1], CompareOp::Le)
        })),
        ("gt", make_builtin(|args| {
            check_args_min("gt", args, 2)?;
            args[0].compare(&args[1], CompareOp::Gt)
        })),
        ("ge", make_builtin(|args| {
            check_args_min("ge", args, 2)?;
            args[0].compare(&args[1], CompareOp::Ge)
        })),
        ("abs", make_builtin(|args| {
            check_args_min("abs", args, 1)?;
            check_args("abs", args, 1)?;
            args[0].py_abs()
        })),
        ("contains", make_builtin(|args| {
            check_args_min("contains", args, 2)?;
            Ok(PyObject::bool_val(args[0].contains(&args[1])?))
        })),
        ("getitem", make_builtin(|args| {
            check_args_min("getitem", args, 2)?;
            match &args[0].payload {
                PyObjectPayload::List(items) => {
                    let idx = args[1].to_int()? as usize;
                    items.read().get(idx).cloned()
                        .ok_or_else(|| PyException::index_error("list index out of range"))
                }
                PyObjectPayload::Dict(map) => {
                    let key = args[1].to_hashable_key()?;
                    map.read().get(&key).cloned()
                        .ok_or_else(|| PyException::key_error(args[1].repr()))
                }
                PyObjectPayload::Tuple(items) => {
                    let idx = args[1].to_int()? as usize;
                    items.get(idx).cloned()
                        .ok_or_else(|| PyException::index_error("tuple index out of range"))
                }
                _ => Err(PyException::type_error("object is not subscriptable")),
            }
        })),
        ("itemgetter", make_builtin(|args| {
            check_args_min("itemgetter", args, 1)?;
            let keys: Vec<PyObjectRef> = args.to_vec();
            Ok(PyObject::native_closure("operator.itemgetter", move |call_args| {
                if call_args.is_empty() {
                    return Err(PyException::type_error("itemgetter expected 1 argument, got 0"));
                }
                let obj = &call_args[0];
                if keys.len() == 1 {
                    obj.get_item(&keys[0])
                } else {
                    let items: Vec<PyObjectRef> = keys.iter()
                        .map(|k| obj.get_item(k))
                        .collect::<PyResult<Vec<_>>>()?;
                    Ok(PyObject::tuple(items))
                }
            }))
        })),
        ("attrgetter", make_builtin(|args| {
            check_args_min("attrgetter", args, 1)?;
            let attr_names: Vec<String> = args.iter().map(|a| a.py_to_string()).collect();
            Ok(PyObject::native_closure("operator.attrgetter", move |call_args| {
                if call_args.is_empty() {
                    return Err(PyException::type_error("attrgetter expected 1 argument, got 0"));
                }
                let obj = &call_args[0];
                if attr_names.len() == 1 {
                    obj.get_attr(&attr_names[0])
                        .ok_or_else(|| PyException::attribute_error(format!(
                            "'{}' object has no attribute '{}'", obj.type_name(), attr_names[0]
                        )))
                } else {
                    let items: Vec<PyObjectRef> = attr_names.iter()
                        .map(|name| obj.get_attr(name).ok_or_else(|| PyException::attribute_error(
                            format!("'{}' object has no attribute '{}'", obj.type_name(), name)
                        )))
                        .collect::<PyResult<Vec<_>>>()?;
                    Ok(PyObject::tuple(items))
                }
            }))
        })),
        ("and_", make_builtin(|args| {
            check_args_min("and_", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a & b))
        })),
        ("or_", make_builtin(|args| {
            check_args_min("or_", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a | b))
        })),
        ("xor", make_builtin(|args| {
            check_args_min("xor", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a ^ b))
        })),
        ("lshift", make_builtin(|args| {
            check_args_min("lshift", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a << b))
        })),
        ("rshift", make_builtin(|args| {
            check_args_min("rshift", args, 2)?;
            let a = args[0].to_int()?;
            let b = args[1].to_int()?;
            Ok(PyObject::int(a >> b))
        })),
        ("invert", make_builtin(|args| {
            check_args_min("invert", args, 1)?;
            let a = args[0].to_int()?;
            Ok(PyObject::int(!a))
        })),
        ("inv", make_builtin(|args| {
            check_args_min("inv", args, 1)?;
            let a = args[0].to_int()?;
            Ok(PyObject::int(!a))
        })),
        ("truth", make_builtin(|args| {
            check_args_min("truth", args, 1)?;
            Ok(PyObject::bool_val(args[0].is_truthy()))
        })),
        ("is_", make_builtin(|args| {
            check_args_min("is_", args, 2)?;
            Ok(PyObject::bool_val(std::sync::Arc::ptr_eq(&args[0], &args[1])))
        })),
        ("is_not", make_builtin(|args| {
            check_args_min("is_not", args, 2)?;
            Ok(PyObject::bool_val(!std::sync::Arc::ptr_eq(&args[0], &args[1])))
        })),
        ("index", make_builtin(|args| {
            check_args_min("index", args, 1)?;
            args[0].to_int().map(PyObject::int)
        })),
        ("setitem", make_builtin(|args| {
            check_args_min("setitem", args, 3)?;
            match &args[0].payload {
                PyObjectPayload::List(items) => {
                    let idx = args[1].to_int()? as usize;
                    let mut w = items.write();
                    if idx < w.len() {
                        w[idx] = args[2].clone();
                        Ok(PyObject::none())
                    } else {
                        Err(PyException::index_error("list assignment index out of range"))
                    }
                }
                PyObjectPayload::Dict(map) => {
                    let key = args[1].to_hashable_key()?;
                    map.write().insert(key, args[2].clone());
                    Ok(PyObject::none())
                }
                _ => Err(PyException::type_error("object does not support item assignment")),
            }
        })),
        ("delitem", make_builtin(|args| {
            check_args_min("delitem", args, 2)?;
            match &args[0].payload {
                PyObjectPayload::Dict(map) => {
                    let key = args[1].to_hashable_key()?;
                    map.write().shift_remove(&key);
                    Ok(PyObject::none())
                }
                _ => Err(PyException::type_error("object does not support item deletion")),
            }
        })),
        ("concat", make_builtin(|args| {
            check_args_min("concat", args, 2)?;
            args[0].add(&args[1])
        })),
        ("iadd", make_builtin(|args| {
            check_args_min("iadd", args, 2)?;
            args[0].add(&args[1])
        })),
        ("methodcaller", make_builtin(|args| {
            check_args_min("methodcaller", args, 1)?;
            let method_name = args[0].py_to_string();
            let extra_args: Vec<PyObjectRef> = if args.len() > 1 { args[1..].to_vec() } else { vec![] };
            Ok(PyObject::native_closure("operator.methodcaller", move |call_args| {
                if call_args.is_empty() {
                    return Err(PyException::type_error("methodcaller expected 1 argument, got 0"));
                }
                let obj = &call_args[0];
                let method = obj.get_attr(&method_name)
                    .ok_or_else(|| PyException::attribute_error(format!(
                        "'{}' object has no attribute '{}'", obj.type_name(), method_name
                    )))?;
                // Build full args: [obj, ...extra_args]
                let mut full_args = vec![obj.clone()];
                full_args.extend(extra_args.iter().cloned());
                match &method.payload {
                    PyObjectPayload::NativeFunction { func, .. } => func(&full_args),
                    PyObjectPayload::NativeClosure { func, .. } => func(&full_args),
                    PyObjectPayload::BuiltinBoundMethod { receiver, method_name, .. } => {
                        // Try to resolve common methods without VM
                        let result_str = match method_name.as_str() {
                            "upper" => Some(receiver.py_to_string().to_uppercase()),
                            "lower" => Some(receiver.py_to_string().to_lowercase()),
                            "strip" => Some(receiver.py_to_string().trim().to_string()),
                            "lstrip" => Some(receiver.py_to_string().trim_start().to_string()),
                            "rstrip" => Some(receiver.py_to_string().trim_end().to_string()),
                            "title" => {
                                let s = receiver.py_to_string();
                                let mut result = String::with_capacity(s.len());
                                let mut capitalize_next = true;
                                for c in s.chars() {
                                    if c.is_alphanumeric() {
                                        if capitalize_next { result.extend(c.to_uppercase()); capitalize_next = false; }
                                        else { result.extend(c.to_lowercase()); }
                                    } else { result.push(c); capitalize_next = true; }
                                }
                                Some(result)
                            }
                            "capitalize" => {
                                let s = receiver.py_to_string();
                                let mut chars = s.chars();
                                Some(match chars.next() {
                                    None => String::new(),
                                    Some(f) => f.to_uppercase().collect::<String>() + &chars.collect::<String>().to_lowercase(),
                                })
                            }
                            "swapcase" => {
                                let s = receiver.py_to_string();
                                Some(s.chars().map(|c| {
                                    if c.is_uppercase() { c.to_lowercase().collect::<String>() }
                                    else { c.to_uppercase().collect::<String>() }
                                }).collect())
                            }
                            _ => None,
                        };
                        if let Some(s) = result_str {
                            Ok(PyObject::str_val(CompactString::from(s)))
                        } else {
                            // Can't dispatch BuiltinBoundMethod from NativeClosure — return method ref
                            Ok(method)
                        }
                    }
                    PyObjectPayload::BoundMethod { receiver, method: meth, .. } => {
                        match &meth.payload {
                            PyObjectPayload::NativeFunction { func, .. } => {
                                let mut bound_args = vec![receiver.clone()];
                                bound_args.extend(extra_args.iter().cloned());
                                func(&bound_args)
                            }
                            _ => Ok(method),
                        }
                    }
                    _ => Ok(method),
                }
            }))
        })),
        ("length_hint", make_builtin(|args| {
            check_args_min("length_hint", args, 1)?;
            let default = if args.len() > 1 { args[1].to_int().unwrap_or(0) } else { 0 };
            // Try __length_hint__ first
            if let Some(method) = args[0].get_attr("__length_hint__") {
                match &method.payload {
                    PyObjectPayload::NativeFunction { func, .. } => {
                        if let Ok(result) = func(&[args[0].clone()]) {
                            if let Ok(n) = result.to_int() {
                                return Ok(PyObject::int(n));
                            }
                        }
                    }
                    _ => {}
                }
            }
            // Try __len__
            if let Some(method) = args[0].get_attr("__len__") {
                match &method.payload {
                    PyObjectPayload::NativeFunction { func, .. } => {
                        if let Ok(result) = func(&[args[0].clone()]) {
                            if let Ok(n) = result.to_int() {
                                return Ok(PyObject::int(n));
                            }
                        }
                    }
                    _ => {}
                }
            }
            // Try len() directly
            match args[0].py_len() {
                Ok(n) => Ok(PyObject::int(n as i64)),
                Err(_) => Ok(PyObject::int(default)),
            }
        })),
    ])
}