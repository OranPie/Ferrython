use super::*;

mod frame;
mod mro_argspec;
mod predicates;
mod source;

use frame::{inspect_currentframe, inspect_stack};
use mro_argspec::{inspect_classify_class_attrs, inspect_getargspec, inspect_getmro};
use predicates::{
    inspect_getgeneratorstate, inspect_isabstract, inspect_isasyncgen, inspect_isasyncgenfunction,
    inspect_isawaitable, inspect_isbuiltin, inspect_isclass, inspect_iscoroutine,
    inspect_iscoroutinefunction, inspect_isdatadescriptor, inspect_isfunction, inspect_isgenerator,
    inspect_isgeneratorfunction, inspect_ismethod, inspect_ismodule, inspect_isroutine,
};
use source::{
    inspect_cleandoc, inspect_getattr_static, inspect_getdoc, inspect_getfile, inspect_getmembers,
    inspect_getmodule, inspect_getsource, inspect_getsourcefile, inspect_getsourcelines,
    inspect_unwrap,
};

// ── inspect module ──

pub fn create_inspect_module() -> PyObjectRef {
    // Shared _empty sentinel used by Parameter and Signature
    let empty_cls = PyObject::class(CompactString::from("_empty"), vec![], {
        let mut ns = IndexMap::new();
        ns.insert(
            CompactString::from("__repr__"),
            PyObject::native_function("_empty.__repr__", |_args: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from(
                    "<class 'inspect._empty'>",
                )))
            }),
        );
        ns.insert(
            CompactString::from("__bool__"),
            PyObject::native_function("_empty.__bool__", |_args: &[PyObjectRef]| {
                Ok(PyObject::bool_val(false))
            }),
        );
        ns
    });
    let empty_sentinel = PyObject::instance(empty_cls.clone());

    fn is_empty(obj: &PyObjectRef) -> bool {
        if let PyObjectPayload::Instance(ref inst) = obj.payload {
            if let PyObjectPayload::Class(ref dc) = inst.class.payload {
                return dc.name.as_str() == "_empty";
            }
        }
        false
    }

    // ── Parameter class ──
    let param_cls = {
        let empty = empty_sentinel.clone();
        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("empty"), empty.clone());
        ns.insert(CompactString::from("POSITIONAL_ONLY"), PyObject::int(0));
        ns.insert(
            CompactString::from("POSITIONAL_OR_KEYWORD"),
            PyObject::int(1),
        );
        ns.insert(CompactString::from("VAR_POSITIONAL"), PyObject::int(2));
        ns.insert(CompactString::from("KEYWORD_ONLY"), PyObject::int(3));
        ns.insert(CompactString::from("VAR_KEYWORD"), PyObject::int(4));
        ns.insert(
            CompactString::from("__repr__"),
            PyObject::native_function("Parameter.__repr__", |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(PyObject::str_val(CompactString::from("<Parameter>")));
                }
                let obj = &args[0];
                let name = obj
                    .get_attr("name")
                    .map(|n| n.py_to_string())
                    .unwrap_or_default();
                let kind = obj.get_attr("kind").and_then(|k| k.as_int()).unwrap_or(1);
                let mut s = match kind {
                    2 => format!("*{}", name),
                    4 => format!("**{}", name),
                    _ => name.clone(),
                };
                if kind != 2 && kind != 4 {
                    if let Some(ann) = obj.get_attr("annotation") {
                        if !is_empty(&ann) {
                            s = format!("{}: {}", s, ann.py_to_string());
                        }
                    }
                    if let Some(default) = obj.get_attr("default") {
                        if !is_empty(&default) {
                            s = format!("{} = {}", s, default.repr());
                        }
                    }
                }
                Ok(PyObject::str_val(CompactString::from(format!(
                    "<Parameter \"{}\">",
                    s
                ))))
            }),
        );
        ns.insert(
            CompactString::from("__str__"),
            PyObject::native_function("Parameter.__str__", |args: &[PyObjectRef]| {
                if args.is_empty() {
                    return Ok(PyObject::str_val(CompactString::from("")));
                }
                let obj = &args[0];
                let name = obj
                    .get_attr("name")
                    .map(|n| n.py_to_string())
                    .unwrap_or_default();
                let kind = obj.get_attr("kind").and_then(|k| k.as_int()).unwrap_or(1);
                let s = match kind {
                    2 => format!("*{}", name),
                    4 => format!("**{}", name),
                    _ => {
                        let mut s = name;
                        if let Some(ann) = obj.get_attr("annotation") {
                            if !is_empty(&ann) {
                                s = format!("{}: {}", s, ann.py_to_string());
                            }
                        }
                        if let Some(default) = obj.get_attr("default") {
                            if !is_empty(&default) {
                                s = format!("{} = {}", s, default.repr());
                            }
                        }
                        s
                    }
                };
                Ok(PyObject::str_val(CompactString::from(s)))
            }),
        );
        PyObject::class(CompactString::from("Parameter"), vec![], ns)
    };

    // Helper: build a Parameter instance
    fn make_param(
        param_cls: &PyObjectRef,
        empty: &PyObjectRef,
        name: &CompactString,
        kind: i64,
        default: PyObjectRef,
        annotation: Option<PyObjectRef>,
    ) -> PyObjectRef {
        let p = PyObject::instance(param_cls.clone());
        if let PyObjectPayload::Instance(ref inst) = p.payload {
            let mut w = inst.attrs.write();
            w.insert(CompactString::from("name"), PyObject::str_val(name.clone()));
            w.insert(CompactString::from("kind"), PyObject::int(kind));
            w.insert(CompactString::from("default"), default);
            w.insert(
                CompactString::from("annotation"),
                annotation.unwrap_or_else(|| empty.clone()),
            );
            // replace() → return self copy (simplified)
            let p_ref = p.clone();
            w.insert(
                CompactString::from("replace"),
                PyObject::native_closure("Parameter.replace", move |_args: &[PyObjectRef]| {
                    Ok(p_ref.clone())
                }),
            );
        }
        p
    }

    // Helper: extract (params_map, keys, return_annotation) from a callable
    fn extract_params(
        func: &PyObjectRef,
        param_cls: &PyObjectRef,
        empty: &PyObjectRef,
    ) -> (FxHashKeyMap, Vec<String>, PyObjectRef) {
        let mut params_map: FxHashKeyMap = new_fx_hashkey_map();
        let mut keys = Vec::new();
        let mut ret_ann = empty.clone();

        if let PyObjectPayload::Function(f) = &func.payload {
            let ac = f.code.arg_count as usize;
            let kwc = f.code.kwonlyarg_count as usize;
            let defaults = f.defaults.read();
            let kw_defaults = f.kw_defaults.read();
            let n_defaults = defaults.len();
            let has_varargs = f.code.flags.contains(CodeFlags::VARARGS);
            let has_varkw = f.code.flags.contains(CodeFlags::VARKEYWORDS);
            let varargs_idx = if has_varargs { Some(ac) } else { None };
            let kw_start = ac + if has_varargs { 1 } else { 0 };
            let varkw_idx = if has_varkw {
                Some(kw_start + kwc)
            } else {
                None
            };

            // Positional params
            for i in 0..ac.min(f.code.varnames.len()) {
                let name = &f.code.varnames[i];
                let default = if n_defaults > 0 && i >= ac - n_defaults {
                    defaults[i - (ac - n_defaults)].clone()
                } else {
                    empty.clone()
                };
                let ann = f.annotations.get(name).cloned();
                let p = make_param(param_cls, empty, name, 1, default, ann);
                params_map.insert(HashableKey::str_key(name.clone()), p);
                keys.push(name.to_string());
            }

            // *args
            if let Some(idx) = varargs_idx {
                if idx < f.code.varnames.len() {
                    let name = &f.code.varnames[idx];
                    let ann = f.annotations.get(name).cloned();
                    let p = make_param(param_cls, empty, name, 2, empty.clone(), ann);
                    params_map.insert(HashableKey::str_key(name.clone()), p);
                    keys.push(name.to_string());
                }
            }

            // Keyword-only params
            for i in 0..kwc {
                let idx = kw_start + i;
                if idx >= f.code.varnames.len() {
                    break;
                }
                let name = &f.code.varnames[idx];
                let default = kw_defaults
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| empty.clone());
                let ann = f.annotations.get(name).cloned();
                let p = make_param(param_cls, empty, name, 3, default, ann);
                params_map.insert(HashableKey::str_key(name.clone()), p);
                keys.push(name.to_string());
            }

            // **kwargs
            if let Some(idx) = varkw_idx {
                if idx < f.code.varnames.len() {
                    let name = &f.code.varnames[idx];
                    let ann = f.annotations.get(name).cloned();
                    let p = make_param(param_cls, empty, name, 4, empty.clone(), ann);
                    params_map.insert(HashableKey::str_key(name.clone()), p);
                    keys.push(name.to_string());
                }
            }

            if let Some(r) = f.annotations.get("return") {
                ret_ann = r.clone();
            }
        }
        (params_map, keys, ret_ann)
    }

    // Helper: build signature string from params_map
    fn sig_to_string(params_map: &FxHashKeyMap, keys: &[String]) -> String {
        let mut parts = Vec::new();
        let mut has_varargs = false;
        let mut has_kwonly = false;
        for k in keys {
            if let Some(p) = params_map.get(&HashableKey::str_key(CompactString::from(k.as_str())))
            {
                if let PyObjectPayload::Instance(ref pinst) = p.payload {
                    let kind = pinst
                        .attrs
                        .read()
                        .get("kind")
                        .and_then(|v| v.as_int())
                        .unwrap_or(1);
                    if kind == 2 {
                        has_varargs = true;
                    }
                    if kind == 3 {
                        has_kwonly = true;
                    }
                }
            }
        }
        let needs_bare_star = has_kwonly && !has_varargs;
        let mut bare_star_inserted = false;
        for k in keys {
            if let Some(p) = params_map.get(&HashableKey::str_key(CompactString::from(k.as_str())))
            {
                if let PyObjectPayload::Instance(ref pinst) = p.payload {
                    let attrs = pinst.attrs.read();
                    let kind = attrs.get("kind").and_then(|v| v.as_int()).unwrap_or(1);
                    if kind == 3 && needs_bare_star && !bare_star_inserted {
                        parts.push("*".to_string());
                        bare_star_inserted = true;
                    }
                    match kind {
                        2 => parts.push(format!("*{}", k)),
                        4 => parts.push(format!("**{}", k)),
                        _ => {
                            let mut part = k.to_string();
                            if let Some(ann) = attrs.get("annotation") {
                                if !is_empty(ann) {
                                    part = format!("{}: {}", part, ann.py_to_string());
                                }
                            }
                            if let Some(default) = attrs.get("default") {
                                if !is_empty(default) {
                                    part = format!("{} = {}", part, default.repr());
                                }
                            }
                            parts.push(part);
                        }
                    }
                } else {
                    parts.push(k.to_string());
                }
            }
        }
        format!("({})", parts.join(", "))
    }

    // ── Signature class ──
    let sig_cls = {
        let empty = empty_sentinel.clone();
        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("empty"), empty);
        PyObject::class(CompactString::from("Signature"), vec![], ns)
    };

    // Build signature function
    let sig_cls_for_sig = sig_cls.clone();
    let param_cls_for_sig = param_cls.clone();
    let empty_for_sig = empty_sentinel.clone();
    let signature_fn =
        PyObject::native_closure("inspect.signature", move |args: &[PyObjectRef]| {
            check_args_min("inspect.signature", args, 1)?;

            let (params_map, keys, ret_ann) =
                extract_params(&args[0], &param_cls_for_sig, &empty_for_sig);
            let sig_str = sig_to_string(&params_map, &keys);

            let sig = PyObject::instance(sig_cls_for_sig.clone());
            if let PyObjectPayload::Instance(ref inst) = sig.payload {
                let mut w = inst.attrs.write();
                w.insert(
                    CompactString::from("parameters"),
                    PyObject::dict(params_map.clone()),
                );
                w.insert(CompactString::from("return_annotation"), ret_ann);

                // __contains__
                let keys_c = keys.clone();
                w.insert(
                    CompactString::from("__contains__"),
                    PyObject::native_closure("Signature.__contains__", move |a| {
                        if a.is_empty() {
                            return Ok(PyObject::bool_val(false));
                        }
                        let needle = a[0].py_to_string();
                        Ok(PyObject::bool_val(keys_c.iter().any(|k| k == &needle)))
                    }),
                );

                // __str__ / __repr__
                let s1 = sig_str.clone();
                let s2 = sig_str.clone();
                w.insert(
                    CompactString::from("__str__"),
                    PyObject::native_closure("Signature.__str__", move |_a| {
                        Ok(PyObject::str_val(CompactString::from(&s1)))
                    }),
                );
                w.insert(
                    CompactString::from("__repr__"),
                    PyObject::native_closure("Signature.__repr__", move |_a| {
                        Ok(PyObject::str_val(CompactString::from(format!(
                            "<Signature {}>",
                            s2
                        ))))
                    }),
                );

                // bind(*args, **kwargs) → BoundArguments
                let pm_bind = params_map.clone();
                let keys_bind = keys.clone();
                w.insert(
                    CompactString::from("bind"),
                    PyObject::native_closure(
                        "Signature.bind",
                        move |call_args: &[PyObjectRef]| {
                            do_bind(&pm_bind, &keys_bind, call_args, false)
                        },
                    ),
                );

                // bind_partial(*args, **kwargs) → BoundArguments
                let pm_bp = params_map.clone();
                let keys_bp = keys.clone();
                w.insert(
                    CompactString::from("bind_partial"),
                    PyObject::native_closure(
                        "Signature.bind_partial",
                        move |call_args: &[PyObjectRef]| do_bind(&pm_bp, &keys_bp, call_args, true),
                    ),
                );

                // replace(**kwargs) → new Signature (returns self with updated attrs)
                let sig_ref = sig.clone();
                w.insert(
                    CompactString::from("replace"),
                    PyObject::native_closure(
                        "Signature.replace",
                        move |_args: &[PyObjectRef]| {
                            // For now, return a copy of self — full kwarg handling would need
                            // parameters= and return_annotation= support
                            Ok(sig_ref.clone())
                        },
                    ),
                );
            }
            Ok(sig)
        });

    // Shared bind logic for Signature.bind / bind_partial
    fn do_bind(
        params_map: &FxHashKeyMap,
        keys: &[String],
        call_args: &[PyObjectRef],
        partial: bool,
    ) -> PyResult<PyObjectRef> {
        let mut positional_args: Vec<PyObjectRef> = Vec::new();
        let mut kw_args: IndexMap<String, PyObjectRef> = IndexMap::new();

        // Separate positional from keyword args
        for arg in call_args {
            if let PyObjectPayload::Dict(kw_map) = &arg.payload {
                let r = kw_map.read();
                for (k, v) in r.iter() {
                    if let HashableKey::Str(s) = k {
                        kw_args.insert(s.to_string(), v.clone());
                    }
                }
            } else {
                positional_args.push(arg.clone());
            }
        }

        let mut arguments: FxHashKeyMap = new_fx_hashkey_map();
        let mut pos_idx = 0;

        for key_name in keys {
            let p = match params_map.get(&HashableKey::str_key(CompactString::from(
                key_name.as_str(),
            ))) {
                Some(p) => p,
                None => continue,
            };
            let kind = if let PyObjectPayload::Instance(ref inst) = p.payload {
                inst.attrs
                    .read()
                    .get("kind")
                    .and_then(|v| v.as_int())
                    .unwrap_or(1)
            } else {
                1
            };

            match kind {
                2 => {
                    // VAR_POSITIONAL: consume remaining positional args
                    let rest: Vec<PyObjectRef> = positional_args[pos_idx..].to_vec();
                    pos_idx = positional_args.len();
                    arguments.insert(
                        HashableKey::str_key(CompactString::from(key_name.as_str())),
                        PyObject::tuple(rest),
                    );
                }
                4 => {
                    // VAR_KEYWORD: consume remaining keyword args
                    let mut d: FxHashKeyMap = new_fx_hashkey_map();
                    // Only include kwargs not already consumed
                    let bound_keys: std::collections::HashSet<String> = arguments
                        .keys()
                        .filter_map(|k| {
                            if let HashableKey::Str(s) = k {
                                Some(s.to_string())
                            } else {
                                None
                            }
                        })
                        .collect();
                    for (kn, kv) in &kw_args {
                        if !bound_keys.contains(kn) && !keys.contains(kn) {
                            d.insert(
                                HashableKey::str_key(CompactString::from(kn.as_str())),
                                kv.clone(),
                            );
                        }
                    }
                    arguments.insert(
                        HashableKey::str_key(CompactString::from(key_name.as_str())),
                        PyObject::dict(d),
                    );
                }
                _ => {
                    // POSITIONAL_ONLY, POSITIONAL_OR_KEYWORD, KEYWORD_ONLY
                    if let Some(kv) = kw_args.get(key_name) {
                        arguments.insert(
                            HashableKey::str_key(CompactString::from(key_name.as_str())),
                            kv.clone(),
                        );
                    } else if pos_idx < positional_args.len() && kind != 3 {
                        arguments.insert(
                            HashableKey::str_key(CompactString::from(key_name.as_str())),
                            positional_args[pos_idx].clone(),
                        );
                        pos_idx += 1;
                    } else {
                        // Check for default
                        let has_default = if let PyObjectPayload::Instance(ref inst) = p.payload {
                            let attrs = inst.attrs.read();
                            attrs.get("default").map(|d| !is_empty(d)).unwrap_or(false)
                        } else {
                            false
                        };
                        if has_default {
                            if let PyObjectPayload::Instance(ref inst) = p.payload {
                                let attrs = inst.attrs.read();
                                if let Some(d) = attrs.get("default") {
                                    arguments.insert(
                                        HashableKey::str_key(CompactString::from(
                                            key_name.as_str(),
                                        )),
                                        d.clone(),
                                    );
                                }
                            }
                        } else if !partial {
                            return Err(PyException::type_error(format!(
                                "missing a required argument: '{}'",
                                key_name
                            )));
                        }
                    }
                }
            }
        }

        // Build BoundArguments object
        let ba_cls = PyObject::class(
            CompactString::from("BoundArguments"),
            vec![],
            IndexMap::new(),
        );
        let mut ba_attrs = IndexMap::new();
        ba_attrs.insert(
            CompactString::from("arguments"),
            PyObject::dict(arguments.clone()),
        );
        let args_list: Vec<PyObjectRef> = arguments.values().cloned().collect();
        ba_attrs.insert(CompactString::from("args"), PyObject::tuple(args_list));
        let mut kw_dict: FxHashKeyMap = new_fx_hashkey_map();
        for (k, v) in &arguments {
            kw_dict.insert(k.clone(), v.clone());
        }
        ba_attrs.insert(CompactString::from("kwargs"), PyObject::dict(kw_dict));
        ba_attrs.insert(
            CompactString::from("apply_defaults"),
            PyObject::native_function("apply_defaults", |_: &[PyObjectRef]| Ok(PyObject::none())),
        );
        ba_attrs.insert(CompactString::from("signature"), PyObject::none());
        Ok(PyObject::instance_with_attrs(ba_cls, ba_attrs))
    }

    // ── getcallargs ──
    let param_cls_gc = param_cls.clone();
    let empty_gc = empty_sentinel.clone();
    let getcallargs_fn =
        PyObject::native_closure("inspect.getcallargs", move |args: &[PyObjectRef]| {
            check_args_min("inspect.getcallargs", args, 1)?;
            let func = &args[0];
            let call_args = &args[1..];

            let (params_map, keys, _) = extract_params(func, &param_cls_gc, &empty_gc);

            // Separate positional from keyword args in call_args
            let mut positional: Vec<PyObjectRef> = Vec::new();
            let mut kwargs: IndexMap<String, PyObjectRef> = IndexMap::new();
            for a in call_args {
                if let PyObjectPayload::Dict(kw_map) = &a.payload {
                    let r = kw_map.read();
                    for (k, v) in r.iter() {
                        if let HashableKey::Str(s) = k {
                            kwargs.insert(s.to_string(), v.clone());
                        }
                    }
                } else {
                    positional.push(a.clone());
                }
            }

            let mut result: FxHashKeyMap = new_fx_hashkey_map();
            let mut pos_idx = 0;

            for key_name in &keys {
                let p = match params_map.get(&HashableKey::str_key(CompactString::from(
                    key_name.as_str(),
                ))) {
                    Some(p) => p,
                    None => continue,
                };
                let kind = if let PyObjectPayload::Instance(ref inst) = p.payload {
                    inst.attrs
                        .read()
                        .get("kind")
                        .and_then(|v| v.as_int())
                        .unwrap_or(1)
                } else {
                    1
                };

                match kind {
                    2 => {
                        let rest: Vec<PyObjectRef> = positional[pos_idx..].to_vec();
                        pos_idx = positional.len();
                        result.insert(
                            HashableKey::str_key(CompactString::from(key_name.as_str())),
                            PyObject::tuple(rest),
                        );
                    }
                    4 => {
                        let mut d: FxHashKeyMap = new_fx_hashkey_map();
                        let bound: std::collections::HashSet<String> = result
                            .keys()
                            .filter_map(|k| {
                                if let HashableKey::Str(s) = k {
                                    Some(s.to_string())
                                } else {
                                    None
                                }
                            })
                            .collect();
                        for (kn, kv) in &kwargs {
                            if !bound.contains(kn) && !keys.contains(kn) {
                                d.insert(
                                    HashableKey::str_key(CompactString::from(kn.as_str())),
                                    kv.clone(),
                                );
                            }
                        }
                        result.insert(
                            HashableKey::str_key(CompactString::from(key_name.as_str())),
                            PyObject::dict(d),
                        );
                    }
                    _ => {
                        if let Some(kv) = kwargs.get(key_name) {
                            result.insert(
                                HashableKey::str_key(CompactString::from(key_name.as_str())),
                                kv.clone(),
                            );
                        } else if pos_idx < positional.len() && kind != 3 {
                            result.insert(
                                HashableKey::str_key(CompactString::from(key_name.as_str())),
                                positional[pos_idx].clone(),
                            );
                            pos_idx += 1;
                        } else {
                            let default_val = if let PyObjectPayload::Instance(ref inst) = p.payload
                            {
                                let attrs = inst.attrs.read();
                                attrs.get("default").filter(|d| !is_empty(d)).cloned()
                            } else {
                                None
                            };
                            if let Some(d) = default_val {
                                result.insert(
                                    HashableKey::str_key(CompactString::from(key_name.as_str())),
                                    d,
                                );
                            } else {
                                return Err(PyException::type_error(format!(
                                    "missing a required argument: '{}'",
                                    key_name
                                )));
                            }
                        }
                    }
                }
            }
            Ok(PyObject::dict(result))
        });

    // ── getfullargspec ──
    let getfullargspec_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args("inspect.getfullargspec", args, 1)?;
        let func = &args[0];
        if let PyObjectPayload::Function(pf) = &func.payload {
            let code = &pf.code;
            let ac = code.arg_count as usize;
            let has_varargs = code.flags.contains(CodeFlags::VARARGS);
            let has_varkw = code.flags.contains(CodeFlags::VARKEYWORDS);
            let kwonly_count = code.kwonlyarg_count as usize;

            let mut positional = Vec::new();
            for i in 0..ac {
                if i < code.varnames.len() {
                    positional.push(PyObject::str_val(code.varnames[i].clone()));
                }
            }
            let varargs = if has_varargs {
                let idx = ac;
                if idx < code.varnames.len() {
                    Some(PyObject::str_val(code.varnames[idx].clone()))
                } else {
                    None
                }
            } else {
                None
            };

            let kw_start = ac + if has_varargs { 1 } else { 0 };
            let mut kwonly = Vec::new();
            for i in 0..kwonly_count {
                let idx = kw_start + i;
                if idx < code.varnames.len() {
                    kwonly.push(PyObject::str_val(code.varnames[idx].clone()));
                }
            }
            let varkw = if has_varkw {
                let idx = kw_start + kwonly_count;
                if idx < code.varnames.len() {
                    Some(PyObject::str_val(code.varnames[idx].clone()))
                } else {
                    None
                }
            } else {
                None
            };

            let defaults_guard = pf.defaults.read();
            let defaults = if defaults_guard.is_empty() {
                PyObject::tuple(vec![])
            } else {
                PyObject::tuple(defaults_guard.clone())
            };
            let kw_defaults_guard = pf.kw_defaults.read();

            let cls = PyObject::class(CompactString::from("FullArgSpec"), vec![], IndexMap::new());
            if let PyObjectPayload::Class(ref cd) = cls.payload {
                let mut ns = cd.namespace.write();
                ns.insert(
                    CompactString::from("__getitem__"),
                    PyObject::native_closure(
                        "FullArgSpec.__getitem__",
                        |args: &[PyObjectRef]| {
                            if args.len() < 2 {
                                return Err(PyException::type_error("__getitem__ requires key"));
                            }
                            let key = args[1].py_to_string();
                            match args[0].get_attr(&key) {
                                Some(v) => Ok(v),
                                None => Err(PyException::key_error(key)),
                            }
                        },
                    ),
                );
            }
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut a = d.attrs.write();
                a.insert(CompactString::from("args"), PyObject::list(positional));
                a.insert(
                    CompactString::from("varargs"),
                    varargs.unwrap_or_else(PyObject::none),
                );
                a.insert(
                    CompactString::from("varkw"),
                    varkw.unwrap_or_else(PyObject::none),
                );
                a.insert(CompactString::from("defaults"), defaults);
                a.insert(CompactString::from("kwonlyargs"), PyObject::list(kwonly));
                a.insert(
                    CompactString::from("kwonlydefaults"),
                    if kw_defaults_guard.is_empty() {
                        PyObject::none()
                    } else {
                        let mut kw_dict: FxHashKeyMap = new_fx_hashkey_map();
                        for (k, v) in kw_defaults_guard.iter() {
                            kw_dict.insert(HashableKey::str_key(k.clone()), v.clone());
                        }
                        PyObject::dict(kw_dict)
                    },
                );
                let mut ann_map: FxHashKeyMap = new_fx_hashkey_map();
                for (k, v) in &pf.annotations {
                    ann_map.insert(HashableKey::str_key(k.clone()), v.clone());
                }
                a.insert(CompactString::from("annotations"), PyObject::dict(ann_map));
            }
            Ok(inst)
        } else {
            Err(PyException::type_error("unsupported callable"))
        }
    });

    make_module(
        "inspect",
        vec![
            // ── Type-checking predicates ──
            ("isfunction", make_builtin(inspect_isfunction)),
            ("isclass", make_builtin(inspect_isclass)),
            ("ismethod", make_builtin(inspect_ismethod)),
            ("ismodule", make_builtin(inspect_ismodule)),
            ("isbuiltin", make_builtin(inspect_isbuiltin)),
            ("isgenerator", make_builtin(inspect_isgenerator)),
            ("getgeneratorstate", make_builtin(inspect_getgeneratorstate)),
            (
                "isgeneratorfunction",
                make_builtin(inspect_isgeneratorfunction),
            ),
            ("iscoroutine", make_builtin(inspect_iscoroutine)),
            (
                "iscoroutinefunction",
                make_builtin(inspect_iscoroutinefunction),
            ),
            ("isroutine", make_builtin(inspect_isroutine)),
            ("isabstract", make_builtin(inspect_isabstract)),
            ("isasyncgen", make_builtin(inspect_isasyncgen)),
            (
                "isasyncgenfunction",
                make_builtin(inspect_isasyncgenfunction),
            ),
            ("isawaitable", make_builtin(inspect_isawaitable)),
            ("isdatadescriptor", make_builtin(inspect_isdatadescriptor)),
            // ── Member introspection ──
            ("getmembers", make_builtin(inspect_getmembers)),
            ("getdoc", make_builtin(inspect_getdoc)),
            ("getmodule", make_builtin(inspect_getmodule)),
            ("getfile", make_builtin(inspect_getfile)),
            ("getsourcefile", make_builtin(inspect_getsourcefile)),
            ("getsource", make_builtin(inspect_getsource)),
            ("getsourcelines", make_builtin(inspect_getsourcelines)),
            // ── Signature & Parameter ──
            ("signature", signature_fn),
            ("getcallargs", getcallargs_fn),
            ("getfullargspec", getfullargspec_fn),
            ("Parameter", param_cls),
            ("Signature", sig_cls),
            // ── MRO & argspec ──
            ("getmro", make_builtin(inspect_getmro)),
            ("getargspec", make_builtin(inspect_getargspec)),
            (
                "classify_class_attrs",
                make_builtin(inspect_classify_class_attrs),
            ),
            // ── Source inspection utilities ──
            ("cleandoc", make_builtin(inspect_cleandoc)),
            ("unwrap", make_builtin(inspect_unwrap)),
            // ── Frame introspection ──
            ("getattr_static", make_builtin(inspect_getattr_static)),
            (
                "currentframe",
                PyObject::native_function("inspect.currentframe", inspect_currentframe),
            ),
            (
                "stack",
                PyObject::native_function("inspect.stack", inspect_stack),
            ),
            // ── Constants ──
            ("CO_OPTIMIZED", PyObject::int(0x01)),
            ("CO_NEWLOCALS", PyObject::int(0x02)),
            ("CO_VARARGS", PyObject::int(0x04)),
            ("CO_VARKEYWORDS", PyObject::int(0x08)),
            ("CO_NESTED", PyObject::int(0x10)),
            ("CO_GENERATOR", PyObject::int(0x20)),
            ("CO_NOFREE", PyObject::int(0x40)),
            ("CO_COROUTINE", PyObject::int(0x80)),
            ("CO_ITERABLE_COROUTINE", PyObject::int(0x100)),
            ("CO_ASYNC_GENERATOR", PyObject::int(0x200)),
            ("TPFLAGS_IS_ABSTRACT", PyObject::int(1 << 20)),
            (
                "GEN_CREATED",
                PyObject::str_val(CompactString::from("GEN_CREATED")),
            ),
            (
                "GEN_RUNNING",
                PyObject::str_val(CompactString::from("GEN_RUNNING")),
            ),
            (
                "GEN_SUSPENDED",
                PyObject::str_val(CompactString::from("GEN_SUSPENDED")),
            ),
            (
                "GEN_CLOSED",
                PyObject::str_val(CompactString::from("GEN_CLOSED")),
            ),
        ],
    )
}
