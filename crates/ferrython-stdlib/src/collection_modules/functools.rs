use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyCell, 
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    InstanceData, PartialData,
    make_module, make_builtin,
    new_shared_fx,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;
use std::rc::Rc;

pub fn create_functools_module() -> PyObjectRef {
    make_module("functools", vec![
        ("reduce", PyObject::native_function("functools.reduce", functools_reduce)),
        ("partial", PyObject::native_function("functools.partial", functools_partial)),
        ("cmp_to_key", make_builtin(functools_cmp_to_key)),
        ("lru_cache", make_builtin(|args| {
            // lru_cache(func) or lru_cache(maxsize=N)(func) — returns a cached wrapper
            // Check for kwargs dict (trailing dict with 'maxsize' key)
            let mut extracted_maxsize: Option<Option<i64>> = None;
            if let Some(last) = args.last() {
                if let PyObjectPayload::Dict(d) = &last.payload {
                    let r = d.read();
                    if let Some(ms) = r.get(&HashableKey::Str(CompactString::from("maxsize"))) {
                        extracted_maxsize = Some(if matches!(ms.payload, PyObjectPayload::None) {
                            None
                        } else {
                            Some(ms.as_int().unwrap_or(128))
                        });
                    }
                }
            }
            if let Some(maxsize) = extracted_maxsize {
                // Called as lru_cache(maxsize=N) — return decorator
                return Ok(PyObject::native_closure("lru_cache", move |inner_args| {
                    if inner_args.is_empty() { return Ok(PyObject::none()); }
                    Ok(create_cached_function(inner_args[0].clone(), maxsize))
                }));
            }
            if args.is_empty() { 
                // @lru_cache() with no args — return decorator with default maxsize=128
                return Ok(PyObject::native_closure("lru_cache", move |inner_args| {
                    if inner_args.is_empty() { return Ok(PyObject::none()); }
                    Ok(create_cached_function(inner_args[0].clone(), Some(128)))
                }));
            }
            match &args[0].payload {
                PyObjectPayload::Function(_) | PyObjectPayload::NativeFunction { .. } 
                | PyObjectPayload::BuiltinFunction(_) | PyObjectPayload::NativeClosure(_) => {
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
        ("cache", make_builtin(|args| {
            // cache(func) — equivalent to lru_cache(maxsize=None)(func)
            if args.is_empty() {
                return Err(PyException::type_error("cache requires a callable argument"));
            }
            Ok(create_cached_function(args[0].clone(), None))
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
        ("singledispatch", make_builtin(functools_singledispatch)),
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
        // Constants used by wraps/update_wrapper
        ("WRAPPER_ASSIGNMENTS", PyObject::tuple(vec![
            PyObject::str_val(CompactString::from("__module__")),
            PyObject::str_val(CompactString::from("__name__")),
            PyObject::str_val(CompactString::from("__qualname__")),
            PyObject::str_val(CompactString::from("__annotations__")),
            PyObject::str_val(CompactString::from("__doc__")),
        ])),
        ("WRAPPER_UPDATES", PyObject::tuple(vec![
            PyObject::str_val(CompactString::from("__dict__")),
        ])),
        ("partialmethod", make_builtin(|args: &[PyObjectRef]| {
            // partialmethod(func, *args, **kwargs) — descriptor for partial on methods
            if args.is_empty() {
                return Err(PyException::type_error("partialmethod requires at least 1 argument"));
            }
            let func = args[0].clone();
            let bound_args: Vec<PyObjectRef> = args[1..].to_vec();
            let cls = PyObject::class(CompactString::from("partialmethod"), vec![], IndexMap::new());
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut w = d.attrs.write();
                w.insert(CompactString::from("func"), func.clone());
                w.insert(CompactString::from("args"), PyObject::tuple(bound_args.clone()));
                w.insert(CompactString::from("keywords"), PyObject::dict(IndexMap::new()));
                // __get__ descriptor — when accessed on instance, return bound partial
                w.insert(CompactString::from("__get__"), PyObject::native_closure(
                    "partialmethod.__get__",
                    move |get_args| {
                        let obj = get_args.first().cloned().unwrap_or(PyObject::none());
                        let f = func.clone();
                        let ba = bound_args.clone();
                        Ok(PyObject::native_closure("partialmethod.bound", move |call_args| {
                            let mut all_args = vec![obj.clone()];
                            all_args.extend(ba.iter().cloned());
                            all_args.extend(call_args.iter().cloned());
                            // We can't call the function directly from here without VM,
                            // so wrap as a callable that stores the info
                            Ok(PyObject::native_closure("_partial_call", {
                                let f2 = f.clone();
                                let args2 = all_args.clone();
                                move |extra| {
                                    let mut a = args2.clone();
                                    a.extend(extra.iter().cloned());
                                    // Return partial application info
                                    Ok(PyObject::tuple(a))
                                }
                            }))
                        }))
                    },
                ));
            }
            Ok(inst)
        })),
        ("_CacheInfo", {
            // namedtuple-like class: _CacheInfo(hits, misses, maxsize, currsize)
            let cls = PyObject::class(CompactString::from("_CacheInfo"), vec![], IndexMap::new());
            if let PyObjectPayload::Class(ref cd) = cls.payload {
                cd.namespace.write().insert(
                    CompactString::from("__init__"),
                    make_builtin(|args: &[PyObjectRef]| {
                        if args.is_empty() { return Ok(PyObject::none()); }
                        let inst = &args[0];
                        if let PyObjectPayload::Instance(ref d) = inst.payload {
                            let mut w = d.attrs.write();
                            w.insert(CompactString::from("hits"), args.get(1).cloned().unwrap_or_else(|| PyObject::int(0)));
                            w.insert(CompactString::from("misses"), args.get(2).cloned().unwrap_or_else(|| PyObject::int(0)));
                            w.insert(CompactString::from("maxsize"), args.get(3).cloned().unwrap_or_else(|| PyObject::none()));
                            w.insert(CompactString::from("currsize"), args.get(4).cloned().unwrap_or_else(|| PyObject::int(0)));
                        }
                        Ok(PyObject::none())
                    }),
                );
            }
            cls
        }),
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
    Ok(PyObject::wrap(PyObjectPayload::Partial(Box::new(PartialData {
        func,
        args: partial_args,
        kwargs: vec![],
    }))))
}

fn functools_reduce(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.len() < 2 { return Err(PyException::type_error("reduce() requires at least 2 arguments")); }
    let func = &args[0];
    let items = args[1].to_list()?;
    let mut acc = if args.len() > 2 {
        args[2].clone()
    } else if !items.is_empty() {
        items[0].clone()
    } else {
        return Err(PyException::type_error("reduce() of empty sequence with no initial value"));
    };
    let start_idx = if args.len() > 2 { 0 } else { 1 };
    for item in &items[start_idx..] {
        // Call func(acc, item) — dispatch to native or closure
        acc = match &func.payload {
            PyObjectPayload::NativeFunction { func: f, .. } => f(&[acc, item.clone()])?,
            PyObjectPayload::NativeClosure(nc) => (nc.func)(&[acc, item.clone()])?,
            PyObjectPayload::BoundMethod { method, .. } => {
                match &method.payload {
                    PyObjectPayload::NativeFunction { func: f, .. } => f(&[acc, item.clone()])?,
                    PyObjectPayload::NativeClosure(nc) => (nc.func)(&[acc, item.clone()])?,
                    _ => return Err(PyException::type_error(
                        "reduce(): Python-defined functions require VM dispatch — use operator module functions instead")),
                }
            }
            PyObjectPayload::Function(_) => {
                // Python functions can't be called directly from native code.
                // Set up a deferred call pattern using operator module for common cases.
                return Err(PyException::type_error(
                    "functools.reduce() with Python functions requires VM dispatch; use operator.add/mul or a lambda wrapping builtins"));
            }
            _ => return Err(PyException::type_error("reduce() arg 1 must be callable")),
        };
    }
    Ok(acc)
}

/// functools.cmp_to_key(cmp_func)
/// Returns a key function that wraps each value in a K object with __lt__/__eq__
/// that delegate to the comparison function. The comparison function itself is stored
/// on a marker so the VM can intercept and call it during sort.
fn functools_cmp_to_key(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("cmp_to_key() requires 1 argument"));
    }
    let cmp_func = args[0].clone();
    // Return a callable that wraps each value with the cmp function attached
    let mut ns = IndexMap::new();
    ns.insert(CompactString::from("__cmp_to_key_func__"), cmp_func);
    Ok(PyObject::class(CompactString::from("cmp_to_key"), vec![], ns))
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
    let cache_arc = Rc::new(PyCell::new(cache_dict));
    let attrs_arc = new_shared_fx();

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
            let h = hits; let m = misses; let ms = info_maxsize; let cs = currsize;
            w.insert(CompactString::from("__repr__"), PyObject::native_closure("CacheInfo.__repr__", move |_args| {
                let ms_str = match ms { Some(n) => n.to_string(), None => "None".to_string() };
                Ok(PyObject::str_val(CompactString::from(
                    format!("CacheInfo(hits={}, misses={}, maxsize={}, currsize={})", h, m, ms_str, cs)
                )))
            }));
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
    PyObjectRef::new(PyObject {
        payload: PyObjectPayload::Instance(InstanceData {
            class: cache_class,
            attrs: attrs_arc,
            is_special: true, dict_storage: None,
        }),
    })
}

/// functools.singledispatch — creates a single-dispatch generic function
/// The dispatcher Instance has __registry__ (dict of type→handler), __default__ (fallback),
/// and a __call__ NativeFunction intercepted at the VM level for actual dispatch.
fn functools_singledispatch(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Err(PyException::type_error("singledispatch requires 1 argument"));
    }
    let default_func = args[0].clone();

    // Build the dispatcher Instance
    let mut cls_ns = IndexMap::new();

    // register(type) → decorator that adds handler to __registry__
    cls_ns.insert(CompactString::from("register"), PyObject::native_function(
        "singledispatch.register", |_args| {
            // Stub — actual dispatch handled by VM intercept
            Err(PyException::type_error("singledispatch.register needs VM access"))
        },
    ));

    // __call__ — intercepted by VM for actual dispatch
    cls_ns.insert(CompactString::from("__call__"), PyObject::native_function(
        "singledispatch.__call__", |_args| {
            Err(PyException::type_error("singledispatch.__call__ should be VM-intercepted"))
        },
    ));

    let class = PyObject::class(CompactString::from("singledispatch"), vec![], cls_ns);
    let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class,
        attrs: new_shared_fx(),
        is_special: true, dict_storage: None,
    }));
    if let PyObjectPayload::Instance(ref d) = inst.payload {
        let mut w = d.attrs.write();
        w.insert(CompactString::from("__singledispatch__"), PyObject::bool_val(true));
        w.insert(CompactString::from("__default__"), default_func.clone());
        // __registry__ as a Dict
        let registry = PyObject::dict(IndexMap::new());
        if let PyObjectPayload::Dict(ref map) = registry.payload {
            map.write().insert(HashableKey::Str(CompactString::from("object")), default_func.clone());
        }
        w.insert(CompactString::from("__registry__"), registry);
        if let Some(name) = default_func.get_attr("__name__") {
            w.insert(CompactString::from("__name__"), name);
        }
    }
    Ok(inst)
}
