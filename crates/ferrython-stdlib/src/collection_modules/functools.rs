use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    InstanceData,
    make_module, make_builtin,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::sync::Arc;

pub fn create_functools_module() -> PyObjectRef {
    make_module("functools", vec![
        ("reduce", PyObject::native_function("functools.reduce", functools_reduce)),
        ("partial", PyObject::native_function("functools.partial", functools_partial)),
        ("cmp_to_key", make_builtin(functools_cmp_to_key)),
        ("lru_cache", make_builtin(|args| {
            // lru_cache(func) or lru_cache(maxsize=N)(func) — returns a cached wrapper
            if args.is_empty() { 
                // @lru_cache() with no args — return decorator with default maxsize=128
                return Ok(PyObject::native_closure("lru_cache", move |inner_args| {
                    if inner_args.is_empty() { return Ok(PyObject::none()); }
                    Ok(create_cached_function(inner_args[0].clone(), Some(128)))
                }));
            }
            match &args[0].payload {
                PyObjectPayload::Function(_) | PyObjectPayload::NativeFunction { .. } 
                | PyObjectPayload::BuiltinFunction(_) | PyObjectPayload::NativeClosure { .. } => {
                    // @lru_cache directly on function — default maxsize=128
                    Ok(create_cached_function(args[0].clone(), Some(128)))
                }
                _ => {
                    // Called with maxsize parameter — extract it and return decorator
                    let maxsize = if matches!(&args[0].payload, PyObjectPayload::None) {
                        None // unlimited
                    } else {
                        Some(args[0].as_int().unwrap_or(128))
                    };
                    Ok(PyObject::native_closure("lru_cache", move |inner_args| {
                        if inner_args.is_empty() { return Ok(PyObject::none()); }
                        Ok(create_cached_function(inner_args[0].clone(), maxsize))
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

/// Create a cached wrapper function for lru_cache.
/// Returns an Instance with __wrapped__ (original func) and _cache (dict).
/// The VM intercepts __call__ on instances with _cache to implement caching.
/// `maxsize`: Some(n) for bounded cache, None for unlimited.
fn create_cached_function(func: PyObjectRef, maxsize: Option<i64>) -> PyObjectRef {
    let cache_class = PyObject::class(
        CompactString::from("_lru_wrapper"),
        vec![],
        IndexMap::new(),
    );
    let cache_dict: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
    let cache_arc = Arc::new(parking_lot::RwLock::new(cache_dict));
    let attrs_map: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
    let attrs_arc = Arc::new(parking_lot::RwLock::new(attrs_map));

    // Build cache_info closure that reads _hits/_misses from attrs
    let info_attrs = attrs_arc.clone();
    let info_cache = cache_arc.clone();
    let info_maxsize = maxsize;
    let cache_info_fn = PyObject::native_closure("cache_info", move |_args| {
        let r = info_attrs.read();
        let hits = r.get(&CompactString::from("_hits")).and_then(|v| v.as_int()).unwrap_or(0);
        let misses = r.get(&CompactString::from("_misses")).and_then(|v| v.as_int()).unwrap_or(0);
        let currsize = info_cache.read().len() as i64;
        let info_class = PyObject::class(CompactString::from("CacheInfo"), vec![], IndexMap::new());
        let info = PyObject::instance(info_class);
        if let PyObjectPayload::Instance(ref d) = info.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("hits"), PyObject::int(hits));
            w.insert(CompactString::from("misses"), PyObject::int(misses));
            w.insert(CompactString::from("maxsize"), match info_maxsize {
                Some(n) => PyObject::int(n),
                None => PyObject::none(),
            });
            w.insert(CompactString::from("currsize"), PyObject::int(currsize));
        }
        Ok(info)
    });

    // Build cache_clear closure that actually clears the cache and resets counters
    let clear_attrs = attrs_arc.clone();
    let clear_cache = cache_arc.clone();
    let cache_clear_fn = PyObject::native_closure("cache_clear", move |_args| {
        clear_cache.write().clear();
        let mut w = clear_attrs.write();
        w.insert(CompactString::from("_hits"), PyObject::int(0));
        w.insert(CompactString::from("_misses"), PyObject::int(0));
        Ok(PyObject::none())
    });

    // Store the cache as a Dict PyObject wrapping our shared Arc
    let cache_obj = PyObject::wrap(PyObjectPayload::Dict(cache_arc));

    {
        let mut w = attrs_arc.write();
        w.insert(CompactString::from("__wrapped__"), func);
        w.insert(CompactString::from("_cache"), cache_obj);
        w.insert(CompactString::from("_hits"), PyObject::int(0));
        w.insert(CompactString::from("_misses"), PyObject::int(0));
        w.insert(CompactString::from("_maxsize"), match maxsize {
            Some(n) => PyObject::int(n),
            None => PyObject::int(-1), // -1 signals unlimited in VM
        });
        w.insert(CompactString::from("cache_info"), cache_info_fn);
        w.insert(CompactString::from("cache_clear"), cache_clear_fn);
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
