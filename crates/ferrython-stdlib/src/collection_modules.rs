//! Collection and functional stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    IteratorData, InstanceData, CompareOp,
    make_module, make_builtin,
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
        ("ChainMap", make_builtin(collections_chainmap)),
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

    // _make classmethod: create instance from iterable
    namespace.insert(CompactString::from("_make"), PyObject::native_function(
        "namedtuple._make",
        |_args: &[PyObjectRef]| -> PyResult<PyObjectRef> {
            // _make is dispatched via BuiltinBoundMethod; handled in builtins
            Ok(PyObject::none())
        }
    ));
    
    let cls = PyObject::class(
        CompactString::from(typename.as_str()),
        vec![],
        namespace,
    );
    
    Ok(cls)
}

fn collections_deque(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let items = if args.is_empty() {
        vec![]
    } else {
        args[0].to_list()?
    };
    // Extract maxlen from positional arg or trailing kwargs dict
    let maxlen = if args.len() >= 2 {
        if let PyObjectPayload::Dict(map) = &args[args.len() - 1].payload {
            let map = map.read();
            map.get(&HashableKey::Str(CompactString::from("maxlen")))
                .and_then(|v| if matches!(&v.payload, PyObjectPayload::None) { None } else { Some(v.to_int().unwrap_or(0) as usize) })
        } else if !matches!(&args[1].payload, PyObjectPayload::None) {
            Some(args[1].to_int()? as usize)
        } else {
            None
        }
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
    // ChainMap(*maps) — group multiple dicts for first-found lookup
    let maps: Vec<PyObjectRef> = args.to_vec();
    let mut merged = IndexMap::new();
    // Iterate in reverse: last map has lowest priority
    for m in maps.iter().rev() {
        if let PyObjectPayload::Dict(dict) = &m.payload {
            let rd = dict.read();
            for (k, v) in rd.iter() {
                merged.insert(k.clone(), v.clone());
            }
        }
    }
    // Store as a dict subclass with __chainmap__ marker and .maps attribute
    let cls = PyObject::class(CompactString::from("ChainMap"), vec![], IndexMap::new());
    let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class: cls,
        attrs: Arc::new(RwLock::new(IndexMap::new())),
        dict_storage: Some(Arc::new(RwLock::new(merged))),
    }));
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("__chainmap__"), PyObject::bool_val(true));
        w.insert(CompactString::from("maps"), PyObject::list(maps));
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
            // Stub — just wrap the function in a property-like
            if args.is_empty() { return Err(PyException::type_error("cached_property requires 1 argument")); }
            Ok(PyObject::wrap(PyObjectPayload::Property {
                fget: Some(args[0].clone()),
                fset: None,
                fdel: None,
            }))
        })),
        ("total_ordering", make_builtin(functools_total_ordering)),
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
        ("accumulate", make_builtin(itertools_accumulate)),
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
    if args.len() < 2 {
        return Err(PyException::type_error("zip_longest requires at least 2 arguments"));
    }
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
    if items.is_empty() { return Ok(PyObject::list(vec![])); }
    // Optional binary function as second arg
    let func = if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
        Some(args[1].clone())
    } else {
        None
    };
    let mut result = Vec::new();
    let mut acc = items[0].clone();
    result.push(acc.clone());
    for item in &items[1..] {
        acc = if let Some(ref f) = func {
            // Try calling the function directly (works for NativeFunction/NativeClosure)
            match &f.payload {
                PyObjectPayload::NativeFunction { func: nf, .. } => nf(&[acc, item.clone()])?,
                PyObjectPayload::NativeClosure { func: nf, .. } => nf(&[acc, item.clone()])?,
                _ => {
                    // Fallback to addition for Python functions (needs VM dispatch)
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
    let mut attrs = IndexMap::new();
    attrs.insert(CompactString::from("__wrapped__"), func);
    attrs.insert(CompactString::from("_cache"), PyObject::dict(cache_dict));
    PyObject::instance_with_attrs(cache_class, attrs)
}

// ── queue module ──

pub fn create_queue_module() -> PyObjectRef {
    make_module("queue", vec![
        ("Queue", PyObject::native_function("Queue", |args| create_queue_instance("Queue", args))),
        ("LifoQueue", PyObject::native_function("LifoQueue", |args| create_queue_instance("LifoQueue", args))),
        ("PriorityQueue", PyObject::native_function("PriorityQueue", |args| create_queue_instance("PriorityQueue", args))),
        ("Empty", PyObject::class(CompactString::from("Empty"), vec![], IndexMap::new())),
        ("Full", PyObject::class(CompactString::from("Full"), vec![], IndexMap::new())),
    ])
}

fn create_queue_instance(kind: &str, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let maxsize = if !args.is_empty() { args[0].to_int().unwrap_or(0) } else { 0 };
    let class = PyObject::class(CompactString::from(kind), vec![], IndexMap::new());
    let inst = PyObject::instance(class);
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("__queue__"), PyObject::str_val(CompactString::from(kind)));
        w.insert(CompactString::from("_items"), PyObject::list(vec![]));
        w.insert(CompactString::from("maxsize"), PyObject::int(maxsize));
    }
    Ok(inst)
}
