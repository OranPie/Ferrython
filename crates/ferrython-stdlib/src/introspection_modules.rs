//! Introspection stdlib modules (warnings, traceback, inspect, dis)

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectPayload, PyObjectRef, PyObjectMethods,
    make_module, make_builtin, check_args, check_args_min,
    InstanceData,
};
use ferrython_core::types::HashableKey;
use ferrython_bytecode::CodeFlags;
use indexmap::IndexMap;

// ── subprocess module (basic) ──


pub fn create_warnings_module() -> PyObjectRef {
    use std::sync::atomic::{AtomicBool, Ordering};
    use parking_lot::RwLock;

    // Global filter list: Vec<(action, message_pattern, category, module_pattern, lineno)>
    // Actions: "default", "always", "ignore", "error", "module", "once"
    type FilterEntry = (String, String, String, String, i64);
    static FILTERS: std::sync::LazyLock<RwLock<Vec<FilterEntry>>> =
        std::sync::LazyLock::new(|| RwLock::new(vec![
            ("default".into(), "".into(), "Warning".into(), "".into(), 0),
        ]));
    static ONCE_SEEN: std::sync::LazyLock<RwLock<std::collections::HashSet<String>>> =
        std::sync::LazyLock::new(|| RwLock::new(std::collections::HashSet::new()));

    static RECORDING: AtomicBool = AtomicBool::new(false);
    static RECORD_LIST: std::sync::LazyLock<RwLock<Option<PyObjectRef>>> =
        std::sync::LazyLock::new(|| RwLock::new(None));

    fn match_filter(_action: &str, msg: &str, cat: &str, module: &str, filter: &FilterEntry) -> bool {
        let (_, msg_pat, cat_pat, mod_pat, lineno) = filter;
        if !msg_pat.is_empty() && !msg.contains(msg_pat.as_str()) { return false; }
        if !cat_pat.is_empty() && cat_pat != "Warning" && cat != cat_pat { return false; }
        if !mod_pat.is_empty() && !module.contains(mod_pat.as_str()) { return false; }
        if *lineno != 0 { return false; }
        true
    }

    fn get_kwarg(args: &[PyObjectRef], key: &str) -> Option<PyObjectRef> {
        for arg in args {
            if let PyObjectPayload::Dict(kw_map) = &arg.payload {
                let r = kw_map.read();
                if let Some(v) = r.get(&HashableKey::Str(CompactString::from(key))) {
                    return Some(v.clone());
                }
            }
        }
        None
    }

    fn get_kwarg_str(args: &[PyObjectRef], key: &str, default: &str) -> String {
        get_kwarg(args, key).map(|v| v.py_to_string()).unwrap_or_else(|| default.to_string())
    }

    fn resolve_category_name(args: &[PyObjectRef]) -> (String, PyObjectRef) {
        let from_kwarg = get_kwarg(args, "category");
        let cat_obj = from_kwarg.or_else(|| {
            if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::Dict(_) | PyObjectPayload::None) {
                Some(args[1].clone())
            } else {
                None
            }
        });
        if let Some(cat) = cat_obj {
            let name = if let PyObjectPayload::Class(cd) = &cat.payload { cd.name.to_string() }
                       else { cat.py_to_string() };
            (name, cat)
        } else {
            let cls = PyObject::class(CompactString::from("UserWarning"), vec![], IndexMap::new());
            ("UserWarning".to_string(), cls)
        }
    }

    // warn(message, category=UserWarning, stacklevel=1)
    let warn_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::none()); }
        let message = args[0].py_to_string();
        let (category, category_cls) = resolve_category_name(args);
        let _stacklevel = get_kwarg(args, "stacklevel")
            .and_then(|v| v.as_int())
            .unwrap_or(1);

        let module = "<stdin>";
        let action = {
            let filters = FILTERS.read();
            let mut found = "default".to_string();
            for f in filters.iter() {
                if match_filter(&f.0, &message, &category, module, f) {
                    found = f.0.clone();
                    break;
                }
            }
            found
        };

        match action.as_str() {
            "ignore" => return Ok(PyObject::none()),
            "error" => {
                return Err(PyException::runtime_error(&format!("{}: {}", category, message)));
            }
            "once" => {
                let key = format!("{}:{}:{}", message, category, module);
                let mut seen = ONCE_SEEN.write();
                if seen.contains(&key) { return Ok(PyObject::none()); }
                seen.insert(key);
            }
            _ => {}
        }

        if RECORDING.load(Ordering::Relaxed) {
            let guard = RECORD_LIST.read();
            if let Some(ref list_obj) = *guard {
                let cls = PyObject::class(CompactString::from("WarningMessage"), vec![], IndexMap::new());
                let mut attrs = IndexMap::new();
                attrs.insert(CompactString::from("message"), args[0].clone());
                attrs.insert(CompactString::from("category"), category_cls.clone());
                attrs.insert(CompactString::from("filename"), PyObject::str_val(CompactString::from("<stdin>")));
                attrs.insert(CompactString::from("lineno"), PyObject::int(1));
                let warning_obj = PyObject::instance_with_attrs(cls, attrs);
                if let PyObjectPayload::List(items) = &list_obj.payload {
                    items.write().push(warning_obj);
                }
            }
        } else {
            eprintln!("<stdin>:1: {}: {}", category, message);
        }
        Ok(PyObject::none())
    });

    // filterwarnings(action, message="", category=Warning, module="", lineno=0, append=False)
    let filter_warnings_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("filterwarnings", args, 1)?;
        let action = args[0].py_to_string();
        let message = get_kwarg_str(args, "message", "");
        let category = get_kwarg(args, "category")
            .map(|cat| {
                if let PyObjectPayload::Class(cd) = &cat.payload { cd.name.to_string() }
                else { cat.py_to_string() }
            })
            .unwrap_or_else(|| "Warning".to_string());
        let module = get_kwarg_str(args, "module", "");
        let lineno = get_kwarg(args, "lineno")
            .and_then(|v| v.as_int())
            .unwrap_or(0);
        let append = get_kwarg(args, "append").map(|v| v.is_truthy()).unwrap_or(false);

        let non_dict_args: Vec<_> = args[1..].iter()
            .filter(|a| !matches!(a.payload, PyObjectPayload::Dict(_)))
            .collect();
        let message = if !message.is_empty() { message }
            else if !non_dict_args.is_empty() { non_dict_args[0].py_to_string() }
            else { String::new() };

        let entry = (action, message, category, module, lineno);
        let mut filters = FILTERS.write();
        if append { filters.push(entry); } else { filters.insert(0, entry); }
        Ok(PyObject::none())
    });

    // simplefilter(action, category=Warning, lineno=0, append=False)
    let simple_filter_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("simplefilter", args, 1)?;
        let action = args[0].py_to_string();
        let category = get_kwarg(args, "category")
            .map(|cat| {
                if let PyObjectPayload::Class(cd) = &cat.payload { cd.name.to_string() }
                else { cat.py_to_string() }
            })
            .unwrap_or_else(|| {
                let non_dict: Vec<_> = args[1..].iter()
                    .filter(|a| !matches!(a.payload, PyObjectPayload::Dict(_)))
                    .collect();
                if !non_dict.is_empty() && !matches!(non_dict[0].payload, PyObjectPayload::None) {
                    if let PyObjectPayload::Class(cd) = &non_dict[0].payload { cd.name.to_string() }
                    else { non_dict[0].py_to_string() }
                } else {
                    "Warning".to_string()
                }
            });
        let lineno = get_kwarg(args, "lineno")
            .and_then(|v| v.as_int())
            .unwrap_or(0);
        let append = get_kwarg(args, "append").map(|v| v.is_truthy()).unwrap_or(false);

        let entry = (action, String::new(), category, String::new(), lineno);
        let mut filters = FILTERS.write();
        if append { filters.push(entry); } else { filters.insert(0, entry); }
        Ok(PyObject::none())
    });

    // resetwarnings()
    let reset_fn = make_builtin(|_args: &[PyObjectRef]| {
        let mut filters = FILTERS.write();
        filters.clear();
        filters.push(("default".into(), "".into(), "Warning".into(), "".into(), 0));
        ONCE_SEEN.write().clear();
        Ok(PyObject::none())
    });

    let filters_list = PyObject::list(vec![]);

    // catch_warnings(record=False)
    let catch_warnings_fn = make_builtin(|args: &[PyObjectRef]| {
        let record = get_kwarg(args, "record").map(|v| v.is_truthy()).unwrap_or_else(|| {
            if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::Dict(_)) {
                args[0].is_truthy()
            } else {
                false
            }
        });

        let cls = PyObject::class(CompactString::from("catch_warnings"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        let warning_list = PyObject::list(vec![]);
        attrs.insert(CompactString::from("_record"), PyObject::bool_val(record));
        attrs.insert(CompactString::from("_warnings"), warning_list.clone());

        // Save filter state for restore on __exit__
        let saved_filters: Vec<FilterEntry> = FILTERS.read().clone();

        if record {
            let wl = warning_list.clone();
            let enter_list = warning_list.clone();
            attrs.insert(CompactString::from("__enter__"), PyObject::native_closure(
                "catch_warnings.__enter__", move |_args: &[PyObjectRef]| {
                    RECORDING.store(true, Ordering::Relaxed);
                    *RECORD_LIST.write() = Some(enter_list.clone());
                    Ok(wl.clone())
                }
            ));
        } else {
            attrs.insert(CompactString::from("__enter__"), PyObject::native_function(
                "catch_warnings.__enter__", |_args: &[PyObjectRef]| {
                    Ok(PyObject::none())
                }
            ));
        }

        attrs.insert(CompactString::from("__exit__"), PyObject::native_closure(
            "catch_warnings.__exit__", move |_args: &[PyObjectRef]| {
                RECORDING.store(false, Ordering::Relaxed);
                *RECORD_LIST.write() = None;
                // Restore previous filters
                *FILTERS.write() = saved_filters.clone();
                Ok(PyObject::bool_val(false))
            }
        ));

        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    // showwarning(message, category, filename, lineno, file=None, line=None)
    let showwarning_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("showwarning", args, 4)?;
        let message = args[0].py_to_string();
        let category = args[1].py_to_string();
        let filename = args[2].py_to_string();
        let lineno = args[3].as_int().unwrap_or(0);
        let _file = if args.len() > 4 { Some(args[4].clone()) } else { None };
        let _line = if args.len() > 5 { Some(args[5].clone()) } else { None };
        eprintln!("{}:{}: {}: {}", filename, lineno, category, message);
        Ok(PyObject::none())
    });

    // formatwarning(message, category, filename, lineno, line=None)
    let formatwarning_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("formatwarning", args, 4)?;
        let message = args[0].py_to_string();
        let category = args[1].py_to_string();
        let filename = args[2].py_to_string();
        let lineno = args[3].as_int().unwrap_or(0);
        let result = format!("{}:{}: {}: {}\n", filename, lineno, category, message);
        Ok(PyObject::str_val(CompactString::from(result)))
    });

    // _filters_mutated() — no-op, called by some 3rd-party libs
    let filters_mutated_fn = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::none())
    });

    // Warning category classes
    fn make_warning_class(name: &str, base_name: &str) -> PyObjectRef {
        let base = PyObject::class(CompactString::from(base_name), vec![], IndexMap::new());
        let mut ns = IndexMap::new();
        let cls_name = name.to_string();
        ns.insert(CompactString::from("__name__"), PyObject::str_val(CompactString::from(name)));
        ns.insert(CompactString::from("__init__"), PyObject::native_closure(
            &format!("{}.__init__", name), {
                let cls_name = cls_name.clone();
                move |args: &[PyObjectRef]| {
                    if args.is_empty() { return Ok(PyObject::none()); }
                    if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                        let mut w = inst.attrs.write();
                        let msg = if args.len() > 1 { args[1].py_to_string() } else { String::new() };
                        w.insert(CompactString::from("args"), PyObject::tuple(
                            args[1..].iter().filter(|a| !matches!(a.payload, PyObjectPayload::Dict(_))).cloned().collect()
                        ));
                        w.insert(CompactString::from("message"), PyObject::str_val(CompactString::from(&msg)));
                        let _ = &cls_name;
                    }
                    Ok(PyObject::none())
                }
            }
        ));
        let cls_name_repr = name.to_string();
        ns.insert(CompactString::from("__repr__"), PyObject::native_closure(
            &format!("{}.__repr__", name), move |args: &[PyObjectRef]| {
                if !args.is_empty() {
                    if let Some(msg) = args[0].get_attr("message") {
                        let s = msg.py_to_string();
                        if !s.is_empty() {
                            return Ok(PyObject::str_val(CompactString::from(format!("{}('{}')", cls_name_repr, s))));
                        }
                    }
                }
                Ok(PyObject::str_val(CompactString::from(format!("{}()", cls_name_repr))))
            }
        ));
        PyObject::class(CompactString::from(name), vec![base], ns)
    }

    let warning_cls = PyObject::class(CompactString::from("Warning"), vec![], {
        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("__name__"), PyObject::str_val(CompactString::from("Warning")));
        ns
    });
    let user_warning = make_warning_class("UserWarning", "Warning");
    let deprecation_warning = make_warning_class("DeprecationWarning", "Warning");
    let future_warning = make_warning_class("FutureWarning", "Warning");
    let runtime_warning = make_warning_class("RuntimeWarning", "Warning");
    let syntax_warning = make_warning_class("SyntaxWarning", "Warning");
    let resource_warning = make_warning_class("ResourceWarning", "Warning");
    let pending_deprecation_warning = make_warning_class("PendingDeprecationWarning", "Warning");
    let import_warning = make_warning_class("ImportWarning", "Warning");
    let unicode_warning = make_warning_class("UnicodeWarning", "Warning");
    let bytes_warning = make_warning_class("BytesWarning", "Warning");

    make_module("warnings", vec![
        ("warn", warn_fn),
        ("filterwarnings", filter_warnings_fn),
        ("simplefilter", simple_filter_fn),
        ("resetwarnings", reset_fn),
        ("catch_warnings", catch_warnings_fn),
        ("showwarning", showwarning_fn),
        ("formatwarning", formatwarning_fn),
        ("_filters_mutated", filters_mutated_fn),
        ("filters", filters_list),
        ("Warning", warning_cls),
        ("UserWarning", user_warning),
        ("DeprecationWarning", deprecation_warning),
        ("FutureWarning", future_warning),
        ("RuntimeWarning", runtime_warning),
        ("SyntaxWarning", syntax_warning),
        ("ResourceWarning", resource_warning),
        ("PendingDeprecationWarning", pending_deprecation_warning),
        ("ImportWarning", import_warning),
        ("UnicodeWarning", unicode_warning),
        ("BytesWarning", bytes_warning),
    ])
}

// ── decimal module (stub) ──


pub fn create_traceback_module() -> PyObjectRef {
    // Delegate to the dedicated ferrython-traceback crate
    ferrython_traceback::create_traceback_module()
}

// ── inspect module ──

pub fn create_inspect_module() -> PyObjectRef {
    // Shared _empty sentinel used by Parameter and Signature
    let empty_cls = PyObject::class(CompactString::from("_empty"), vec![], {
        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("__repr__"), PyObject::native_function(
            "_empty.__repr__", |_args: &[PyObjectRef]| {
                Ok(PyObject::str_val(CompactString::from("<class 'inspect._empty'>")))
            }
        ));
        ns.insert(CompactString::from("__bool__"), PyObject::native_function(
            "_empty.__bool__", |_args: &[PyObjectRef]| {
                Ok(PyObject::bool_val(false))
            }
        ));
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
        ns.insert(CompactString::from("POSITIONAL_OR_KEYWORD"), PyObject::int(1));
        ns.insert(CompactString::from("VAR_POSITIONAL"), PyObject::int(2));
        ns.insert(CompactString::from("KEYWORD_ONLY"), PyObject::int(3));
        ns.insert(CompactString::from("VAR_KEYWORD"), PyObject::int(4));
        ns.insert(CompactString::from("__repr__"), PyObject::native_function(
            "Parameter.__repr__", |args: &[PyObjectRef]| {
                if args.is_empty() { return Ok(PyObject::str_val(CompactString::from("<Parameter>"))); }
                let obj = &args[0];
                let name = obj.get_attr("name").map(|n| n.py_to_string()).unwrap_or_default();
                let kind = obj.get_attr("kind").and_then(|k| k.as_int()).unwrap_or(1);
                let mut s = match kind {
                    2 => format!("*{}", name),
                    4 => format!("**{}", name),
                    _ => name.clone(),
                };
                if kind != 2 && kind != 4 {
                    if let Some(ann) = obj.get_attr("annotation") {
                        if !is_empty(&ann) { s = format!("{}: {}", s, ann.py_to_string()); }
                    }
                    if let Some(default) = obj.get_attr("default") {
                        if !is_empty(&default) { s = format!("{} = {}", s, default.repr()); }
                    }
                }
                Ok(PyObject::str_val(CompactString::from(format!("<Parameter \"{}\">", s))))
            }
        ));
        ns.insert(CompactString::from("__str__"), PyObject::native_function(
            "Parameter.__str__", |args: &[PyObjectRef]| {
                if args.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
                let obj = &args[0];
                let name = obj.get_attr("name").map(|n| n.py_to_string()).unwrap_or_default();
                let kind = obj.get_attr("kind").and_then(|k| k.as_int()).unwrap_or(1);
                let s = match kind {
                    2 => format!("*{}", name),
                    4 => format!("**{}", name),
                    _ => {
                        let mut s = name;
                        if let Some(ann) = obj.get_attr("annotation") {
                            if !is_empty(&ann) { s = format!("{}: {}", s, ann.py_to_string()); }
                        }
                        if let Some(default) = obj.get_attr("default") {
                            if !is_empty(&default) { s = format!("{} = {}", s, default.repr()); }
                        }
                        s
                    }
                };
                Ok(PyObject::str_val(CompactString::from(s)))
            }
        ));
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
            w.insert(CompactString::from("annotation"), annotation.unwrap_or_else(|| empty.clone()));
            // replace() → return self copy (simplified)
            let p_ref = p.clone();
            w.insert(CompactString::from("replace"), PyObject::native_closure("Parameter.replace", move |_args: &[PyObjectRef]| {
                Ok(p_ref.clone())
            }));
        }
        p
    }

    // Helper: extract (params_map, keys, return_annotation) from a callable
    fn extract_params(
        func: &PyObjectRef,
        param_cls: &PyObjectRef,
        empty: &PyObjectRef,
    ) -> (IndexMap<HashableKey, PyObjectRef>, Vec<String>, PyObjectRef) {
        let mut params_map: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
        let mut keys = Vec::new();
        let mut ret_ann = empty.clone();

        if let PyObjectPayload::Function(f) = &func.payload {
            let ac = f.code.arg_count as usize;
            let kwc = f.code.kwonlyarg_count as usize;
            let n_defaults = f.defaults.len();
            let has_varargs = f.code.flags.contains(CodeFlags::VARARGS);
            let has_varkw = f.code.flags.contains(CodeFlags::VARKEYWORDS);
            let varargs_idx = if has_varargs { Some(ac) } else { None };
            let kw_start = ac + if has_varargs { 1 } else { 0 };
            let varkw_idx = if has_varkw { Some(kw_start + kwc) } else { None };

            // Positional params
            for i in 0..ac.min(f.code.varnames.len()) {
                let name = &f.code.varnames[i];
                let default = if n_defaults > 0 && i >= ac - n_defaults {
                    f.defaults[i - (ac - n_defaults)].clone()
                } else {
                    empty.clone()
                };
                let ann = f.annotations.get(name).cloned();
                let p = make_param(param_cls, empty, name, 1, default, ann);
                params_map.insert(HashableKey::Str(name.clone()), p);
                keys.push(name.to_string());
            }

            // *args
            if let Some(idx) = varargs_idx {
                if idx < f.code.varnames.len() {
                    let name = &f.code.varnames[idx];
                    let ann = f.annotations.get(name).cloned();
                    let p = make_param(param_cls, empty, name, 2, empty.clone(), ann);
                    params_map.insert(HashableKey::Str(name.clone()), p);
                    keys.push(name.to_string());
                }
            }

            // Keyword-only params
            for i in 0..kwc {
                let idx = kw_start + i;
                if idx >= f.code.varnames.len() { break; }
                let name = &f.code.varnames[idx];
                let default = f.kw_defaults.get(name).cloned().unwrap_or_else(|| empty.clone());
                let ann = f.annotations.get(name).cloned();
                let p = make_param(param_cls, empty, name, 3, default, ann);
                params_map.insert(HashableKey::Str(name.clone()), p);
                keys.push(name.to_string());
            }

            // **kwargs
            if let Some(idx) = varkw_idx {
                if idx < f.code.varnames.len() {
                    let name = &f.code.varnames[idx];
                    let ann = f.annotations.get(name).cloned();
                    let p = make_param(param_cls, empty, name, 4, empty.clone(), ann);
                    params_map.insert(HashableKey::Str(name.clone()), p);
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
    fn sig_to_string(params_map: &IndexMap<HashableKey, PyObjectRef>, keys: &[String]) -> String {
        let mut parts = Vec::new();
        let mut has_varargs = false;
        let mut has_kwonly = false;
        for k in keys {
            if let Some(p) = params_map.get(&HashableKey::Str(CompactString::from(k.as_str()))) {
                if let PyObjectPayload::Instance(ref pinst) = p.payload {
                    let kind = pinst.attrs.read().get("kind").and_then(|v| v.as_int()).unwrap_or(1);
                    if kind == 2 { has_varargs = true; }
                    if kind == 3 { has_kwonly = true; }
                }
            }
        }
        let needs_bare_star = has_kwonly && !has_varargs;
        let mut bare_star_inserted = false;
        for k in keys {
            if let Some(p) = params_map.get(&HashableKey::Str(CompactString::from(k.as_str()))) {
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
    let signature_fn = PyObject::native_closure("inspect.signature", move |args: &[PyObjectRef]| {
        check_args_min("inspect.signature", args, 1)?;

        let (params_map, keys, ret_ann) = extract_params(&args[0], &param_cls_for_sig, &empty_for_sig);
        let sig_str = sig_to_string(&params_map, &keys);

        let sig = PyObject::instance(sig_cls_for_sig.clone());
        if let PyObjectPayload::Instance(ref inst) = sig.payload {
            let mut w = inst.attrs.write();
            w.insert(CompactString::from("parameters"), PyObject::dict(params_map.clone()));
            w.insert(CompactString::from("return_annotation"), ret_ann);

            // __contains__
            let keys_c = keys.clone();
            w.insert(CompactString::from("__contains__"), PyObject::native_closure("Signature.__contains__", move |a| {
                if a.is_empty() { return Ok(PyObject::bool_val(false)); }
                let needle = a[0].py_to_string();
                Ok(PyObject::bool_val(keys_c.iter().any(|k| k == &needle)))
            }));

            // __str__ / __repr__
            let s1 = sig_str.clone();
            let s2 = sig_str.clone();
            w.insert(CompactString::from("__str__"), PyObject::native_closure("Signature.__str__", move |_a| {
                Ok(PyObject::str_val(CompactString::from(&s1)))
            }));
            w.insert(CompactString::from("__repr__"), PyObject::native_closure("Signature.__repr__", move |_a| {
                Ok(PyObject::str_val(CompactString::from(format!("<Signature {}>", s2))))
            }));

            // bind(*args, **kwargs) → BoundArguments
            let pm_bind = params_map.clone();
            let keys_bind = keys.clone();
            w.insert(CompactString::from("bind"), PyObject::native_closure("Signature.bind", move |call_args: &[PyObjectRef]| {
                do_bind(&pm_bind, &keys_bind, call_args, false)
            }));

            // bind_partial(*args, **kwargs) → BoundArguments
            let pm_bp = params_map.clone();
            let keys_bp = keys.clone();
            w.insert(CompactString::from("bind_partial"), PyObject::native_closure("Signature.bind_partial", move |call_args: &[PyObjectRef]| {
                do_bind(&pm_bp, &keys_bp, call_args, true)
            }));

            // replace(**kwargs) → new Signature (returns self with updated attrs)
            let sig_ref = sig.clone();
            w.insert(CompactString::from("replace"), PyObject::native_closure("Signature.replace", move |args: &[PyObjectRef]| {
                // For now, return a copy of self — full kwarg handling would need
                // parameters= and return_annotation= support
                Ok(sig_ref.clone())
            }));
        }
        Ok(sig)
    });

    // Shared bind logic for Signature.bind / bind_partial
    fn do_bind(
        params_map: &IndexMap<HashableKey, PyObjectRef>,
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

        let mut arguments: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
        let mut pos_idx = 0;

        for key_name in keys {
            let p = match params_map.get(&HashableKey::Str(CompactString::from(key_name.as_str()))) {
                Some(p) => p,
                None => continue,
            };
            let kind = if let PyObjectPayload::Instance(ref inst) = p.payload {
                inst.attrs.read().get("kind").and_then(|v| v.as_int()).unwrap_or(1)
            } else { 1 };

            match kind {
                2 => {
                    // VAR_POSITIONAL: consume remaining positional args
                    let rest: Vec<PyObjectRef> = positional_args[pos_idx..].to_vec();
                    pos_idx = positional_args.len();
                    arguments.insert(HashableKey::Str(CompactString::from(key_name.as_str())), PyObject::tuple(rest));
                }
                4 => {
                    // VAR_KEYWORD: consume remaining keyword args
                    let mut d: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
                    // Only include kwargs not already consumed
                    let bound_keys: std::collections::HashSet<String> = arguments.keys()
                        .filter_map(|k| if let HashableKey::Str(s) = k { Some(s.to_string()) } else { None })
                        .collect();
                    for (kn, kv) in &kw_args {
                        if !bound_keys.contains(kn) && !keys.contains(kn) {
                            d.insert(HashableKey::Str(CompactString::from(kn.as_str())), kv.clone());
                        }
                    }
                    arguments.insert(HashableKey::Str(CompactString::from(key_name.as_str())), PyObject::dict(d));
                }
                _ => {
                    // POSITIONAL_ONLY, POSITIONAL_OR_KEYWORD, KEYWORD_ONLY
                    if let Some(kv) = kw_args.get(key_name) {
                        arguments.insert(HashableKey::Str(CompactString::from(key_name.as_str())), kv.clone());
                    } else if pos_idx < positional_args.len() && kind != 3 {
                        arguments.insert(HashableKey::Str(CompactString::from(key_name.as_str())), positional_args[pos_idx].clone());
                        pos_idx += 1;
                    } else {
                        // Check for default
                        let has_default = if let PyObjectPayload::Instance(ref inst) = p.payload {
                            let attrs = inst.attrs.read();
                            attrs.get("default").map(|d| !is_empty(d)).unwrap_or(false)
                        } else { false };
                        if has_default {
                            if let PyObjectPayload::Instance(ref inst) = p.payload {
                                let attrs = inst.attrs.read();
                                if let Some(d) = attrs.get("default") {
                                    arguments.insert(HashableKey::Str(CompactString::from(key_name.as_str())), d.clone());
                                }
                            }
                        } else if !partial {
                            return Err(PyException::type_error(
                                format!("missing a required argument: '{}'", key_name)
                            ));
                        }
                    }
                }
            }
        }

        // Build BoundArguments object
        let ba_cls = PyObject::class(CompactString::from("BoundArguments"), vec![], IndexMap::new());
        let mut ba_attrs = IndexMap::new();
        ba_attrs.insert(CompactString::from("arguments"), PyObject::dict(arguments.clone()));
        let args_list: Vec<PyObjectRef> = arguments.values().cloned().collect();
        ba_attrs.insert(CompactString::from("args"), PyObject::tuple(args_list));
        let mut kw_dict: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
        for (k, v) in &arguments {
            kw_dict.insert(k.clone(), v.clone());
        }
        ba_attrs.insert(CompactString::from("kwargs"), PyObject::dict(kw_dict));
        ba_attrs.insert(CompactString::from("apply_defaults"), PyObject::native_function(
            "apply_defaults", |_: &[PyObjectRef]| Ok(PyObject::none())
        ));
        ba_attrs.insert(CompactString::from("signature"), PyObject::none());
        Ok(PyObject::instance_with_attrs(ba_cls, ba_attrs))
    }

    // ── getcallargs ──
    let param_cls_gc = param_cls.clone();
    let empty_gc = empty_sentinel.clone();
    let getcallargs_fn = PyObject::native_closure("inspect.getcallargs", move |args: &[PyObjectRef]| {
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

        let mut result: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
        let mut pos_idx = 0;

        for key_name in &keys {
            let p = match params_map.get(&HashableKey::Str(CompactString::from(key_name.as_str()))) {
                Some(p) => p, None => continue,
            };
            let kind = if let PyObjectPayload::Instance(ref inst) = p.payload {
                inst.attrs.read().get("kind").and_then(|v| v.as_int()).unwrap_or(1)
            } else { 1 };

            match kind {
                2 => {
                    let rest: Vec<PyObjectRef> = positional[pos_idx..].to_vec();
                    pos_idx = positional.len();
                    result.insert(HashableKey::Str(CompactString::from(key_name.as_str())), PyObject::tuple(rest));
                }
                4 => {
                    let mut d: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
                    let bound: std::collections::HashSet<String> = result.keys()
                        .filter_map(|k| if let HashableKey::Str(s) = k { Some(s.to_string()) } else { None })
                        .collect();
                    for (kn, kv) in &kwargs {
                        if !bound.contains(kn) && !keys.contains(kn) {
                            d.insert(HashableKey::Str(CompactString::from(kn.as_str())), kv.clone());
                        }
                    }
                    result.insert(HashableKey::Str(CompactString::from(key_name.as_str())), PyObject::dict(d));
                }
                _ => {
                    if let Some(kv) = kwargs.get(key_name) {
                        result.insert(HashableKey::Str(CompactString::from(key_name.as_str())), kv.clone());
                    } else if pos_idx < positional.len() && kind != 3 {
                        result.insert(HashableKey::Str(CompactString::from(key_name.as_str())), positional[pos_idx].clone());
                        pos_idx += 1;
                    } else {
                        let default_val = if let PyObjectPayload::Instance(ref inst) = p.payload {
                            let attrs = inst.attrs.read();
                            attrs.get("default").filter(|d| !is_empty(d)).cloned()
                        } else { None };
                        if let Some(d) = default_val {
                            result.insert(HashableKey::Str(CompactString::from(key_name.as_str())), d);
                        } else {
                            return Err(PyException::type_error(
                                format!("missing a required argument: '{}'", key_name)
                            ));
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
                if idx < code.varnames.len() { Some(PyObject::str_val(code.varnames[idx].clone())) } else { None }
            } else { None };

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
                if idx < code.varnames.len() { Some(PyObject::str_val(code.varnames[idx].clone())) } else { None }
            } else { None };

            let defaults = if pf.defaults.is_empty() { PyObject::tuple(vec![]) } else { PyObject::tuple(pf.defaults.clone()) };

            let cls = PyObject::class(CompactString::from("FullArgSpec"), vec![], IndexMap::new());
            if let PyObjectPayload::Class(ref cd) = cls.payload {
                let mut ns = cd.namespace.write();
                ns.insert(CompactString::from("__getitem__"), PyObject::native_closure(
                    "FullArgSpec.__getitem__", |args: &[PyObjectRef]| {
                        if args.len() < 2 { return Err(PyException::type_error("__getitem__ requires key")); }
                        let key = args[1].py_to_string();
                        match args[0].get_attr(&key) {
                            Some(v) => Ok(v),
                            None => Err(PyException::key_error(key)),
                        }
                    }));
            }
            let inst = PyObject::instance(cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut a = d.attrs.write();
                a.insert(CompactString::from("args"), PyObject::list(positional));
                a.insert(CompactString::from("varargs"), varargs.unwrap_or_else(PyObject::none));
                a.insert(CompactString::from("varkw"), varkw.unwrap_or_else(PyObject::none));
                a.insert(CompactString::from("defaults"), defaults);
                a.insert(CompactString::from("kwonlyargs"), PyObject::list(kwonly));
                a.insert(CompactString::from("kwonlydefaults"), if pf.kw_defaults.is_empty() {
                    PyObject::none()
                } else {
                    let mut kw_dict: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
                    for (k, v) in &pf.kw_defaults {
                        kw_dict.insert(HashableKey::Str(k.clone()), v.clone());
                    }
                    PyObject::dict(kw_dict)
                });
                let mut ann_map: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
                for (k, v) in &pf.annotations {
                    ann_map.insert(HashableKey::Str(k.clone()), v.clone());
                }
                a.insert(CompactString::from("annotations"), PyObject::dict(ann_map));
            }
            Ok(inst)
        } else {
            Err(PyException::type_error("unsupported callable"))
        }
    });

    make_module("inspect", vec![
        // ── Type-checking predicates ──
        ("isfunction", make_builtin(|args| {
            check_args("inspect.isfunction", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Function(_))))
        })),
        ("isclass", make_builtin(|args| {
            check_args("inspect.isclass", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload,
                PyObjectPayload::Class(_) | PyObjectPayload::BuiltinType(_))))
        })),
        ("ismethod", make_builtin(|args| {
            check_args("inspect.ismethod", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::BoundMethod { .. })))
        })),
        ("ismodule", make_builtin(|args| {
            check_args("inspect.ismodule", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Module(_))))
        })),
        ("isbuiltin", make_builtin(|args| {
            check_args("inspect.isbuiltin", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload,
                PyObjectPayload::NativeFunction { .. } | PyObjectPayload::BuiltinFunction(_) | PyObjectPayload::BuiltinType(_))))
        })),
        ("isgenerator", make_builtin(|args| {
            check_args("inspect.isgenerator", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Generator(_))))
        })),
        ("isgeneratorfunction", make_builtin(|args| {
            check_args("inspect.isgeneratorfunction", args, 1)?;
            if let PyObjectPayload::Function(f) = &args[0].payload {
                Ok(PyObject::bool_val(f.code.flags.contains(CodeFlags::GENERATOR)))
            } else { Ok(PyObject::bool_val(false)) }
        })),
        ("iscoroutine", make_builtin(|args| {
            check_args("inspect.iscoroutine", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Coroutine(_))))
        })),
        ("iscoroutinefunction", make_builtin(|args| {
            check_args("inspect.iscoroutinefunction", args, 1)?;
            if let PyObjectPayload::Function(pf) = &args[0].payload {
                Ok(PyObject::bool_val(pf.code.flags.contains(CodeFlags::COROUTINE)))
            } else { Ok(PyObject::bool_val(false)) }
        })),
        ("isroutine", make_builtin(|args| {
            check_args("inspect.isroutine", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload,
                PyObjectPayload::Function(_) | PyObjectPayload::BoundMethod { .. } |
                PyObjectPayload::NativeFunction { .. } | PyObjectPayload::NativeClosure(_) |
                PyObjectPayload::BuiltinBoundMethod { .. } | PyObjectPayload::BuiltinFunction(_))))
        })),
        ("isabstract", make_builtin(|args| {
            check_args("inspect.isabstract", args, 1)?;
            Ok(PyObject::bool_val(args[0].get_attr("__abstractmethods__").is_some()))
        })),
        ("isasyncgen", make_builtin(|args| {
            check_args("inspect.isasyncgen", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::AsyncGenerator(_))))
        })),
        ("isasyncgenfunction", make_builtin(|args| {
            check_args("inspect.isasyncgenfunction", args, 1)?;
            if let PyObjectPayload::Function(pf) = &args[0].payload {
                Ok(PyObject::bool_val(pf.code.flags.contains(CodeFlags::ASYNC_GENERATOR)))
            } else { Ok(PyObject::bool_val(false)) }
        })),
        ("isawaitable", make_builtin(|args| {
            check_args("inspect.isawaitable", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload,
                PyObjectPayload::Coroutine(_) | PyObjectPayload::BuiltinAwaitable(_))))
        })),
        ("isdatadescriptor", make_builtin(|args| {
            check_args("inspect.isdatadescriptor", args, 1)?;
            Ok(PyObject::bool_val(args[0].get_attr("__get__").is_some() && args[0].get_attr("__set__").is_some()))
        })),

        // ── Member introspection ──
        ("getmembers", make_builtin(|args| {
            check_args_min("inspect.getmembers", args, 1)?;
            let dir_names = args[0].dir();
            let mut result = Vec::new();
            for n in &dir_names {
                if let Some(val) = args[0].get_attr(n.as_str()) {
                    result.push(PyObject::tuple(vec![PyObject::str_val(n.clone()), val]));
                }
            }
            Ok(PyObject::list(result))
        })),
        ("getdoc", make_builtin(|args| {
            check_args("inspect.getdoc", args, 1)?;
            match args[0].get_attr("__doc__") {
                Some(doc) if !matches!(&doc.payload, PyObjectPayload::None) => {
                    let s = doc.py_to_string();
                    let lines: Vec<&str> = s.lines().collect();
                    if lines.is_empty() { return Ok(PyObject::none()); }
                    let min_indent = lines.iter().skip(1)
                        .filter(|l| !l.trim().is_empty())
                        .map(|l| l.len() - l.trim_start().len())
                        .min().unwrap_or(0);
                    let mut result = String::from(lines[0].trim());
                    for line in &lines[1..] {
                        result.push('\n');
                        if line.len() > min_indent { result.push_str(&line[min_indent..]); }
                        else { result.push_str(line.trim()); }
                    }
                    let cleaned: String = result.lines().map(|l| l.trim_end()).collect::<Vec<_>>().join("\n");
                    Ok(PyObject::str_val(CompactString::from(cleaned.trim_end())))
                }
                _ => Ok(PyObject::none()),
            }
        })),
        ("getmodule", make_builtin(|args| {
            check_args("inspect.getmodule", args, 1)?;
            Ok(args[0].get_attr("__module__").unwrap_or_else(PyObject::none))
        })),
        ("getfile", make_builtin(|args| {
            check_args("inspect.getfile", args, 1)?;
            if let PyObjectPayload::Function(f) = &args[0].payload {
                return Ok(PyObject::str_val(f.code.filename.clone()));
            }
            if let PyObjectPayload::Module(m) = &args[0].payload {
                if let Some(file) = m.attrs.read().get("__file__").cloned() { return Ok(file); }
            }
            Err(PyException::type_error("could not get file for object"))
        })),
        ("getsourcefile", make_builtin(|args| {
            check_args("inspect.getsourcefile", args, 1)?;
            let filename = if let PyObjectPayload::Function(f) = &args[0].payload {
                Some(f.code.filename.clone())
            } else if let PyObjectPayload::Module(m) = &args[0].payload {
                m.attrs.read().get("__file__").map(|f| CompactString::from(f.py_to_string()))
            } else { None };
            match filename {
                Some(f) if f.ends_with(".py") => Ok(PyObject::str_val(f)),
                Some(_) => Ok(PyObject::none()),
                None => Err(PyException::type_error("could not find source file")),
            }
        })),
        ("getsource", make_builtin(|args| {
            check_args("inspect.getsource", args, 1)?;
            let filename = match &args[0].payload {
                PyObjectPayload::Function(f) => f.code.filename.clone(),
                PyObjectPayload::Module(m) => {
                    if let Some(f) = m.attrs.read().get("__file__") {
                        CompactString::from(f.py_to_string())
                    } else { return Err(PyException::runtime_error("could not find source")); }
                }
                _ => return Err(PyException::runtime_error("could not find source")),
            };
            match std::fs::read_to_string(filename.as_str()) {
                Ok(src) => {
                    if let PyObjectPayload::Function(f) = &args[0].payload {
                        let lines: Vec<&str> = src.lines().collect();
                        let start = (f.code.first_line_number as usize).saturating_sub(1);
                        if start < lines.len() {
                            let indent = lines[start].len() - lines[start].trim_start().len();
                            let mut end = start + 1;
                            while end < lines.len() {
                                let line = lines[end];
                                if line.trim().is_empty() { end += 1; continue; }
                                let li = line.len() - line.trim_start().len();
                                if li <= indent { break; }
                                end += 1;
                            }
                            return Ok(PyObject::str_val(CompactString::from(lines[start..end].join("\n"))));
                        }
                    }
                    Ok(PyObject::str_val(CompactString::from(src)))
                }
                Err(_) => Err(PyException::runtime_error("could not read source file")),
            }
        })),
        ("getsourcelines", make_builtin(|args| {
            check_args("inspect.getsourcelines", args, 1)?;
            let filename = match &args[0].payload {
                PyObjectPayload::Function(f) => Some((f.code.filename.clone(), f.code.first_line_number)),
                _ => None,
            };
            if let Some((fname, lineno)) = filename {
                match std::fs::read_to_string(fname.as_str()) {
                    Ok(src) => {
                        let all_lines: Vec<&str> = src.lines().collect();
                        let start = (lineno as usize).saturating_sub(1);
                        if start >= all_lines.len() { return Err(PyException::runtime_error("could not find source lines")); }
                        let base_indent = all_lines[start].len() - all_lines[start].trim_start().len();
                        let mut end = start + 1;
                        while end < all_lines.len() {
                            let line = all_lines[end];
                            if line.trim().is_empty() { end += 1; continue; }
                            let indent = line.len() - line.trim_start().len();
                            if indent <= base_indent { break; }
                            end += 1;
                        }
                        let lines: Vec<PyObjectRef> = all_lines[start..end].iter()
                            .map(|l| PyObject::str_val(CompactString::from(format!("{}\n", l))))
                            .collect();
                        Ok(PyObject::tuple(vec![PyObject::list(lines), PyObject::int(lineno as i64)]))
                    }
                    Err(_) => Err(PyException::runtime_error("could not read source")),
                }
            } else { Err(PyException::runtime_error("could not find source lines")) }
        })),

        // ── Signature & Parameter ──
        ("signature", signature_fn),
        ("getcallargs", getcallargs_fn),
        ("getfullargspec", getfullargspec_fn),
        ("Parameter", param_cls),
        ("Signature", sig_cls),

        // ── MRO & argspec ──
        ("getmro", make_builtin(|args| {
            check_args("inspect.getmro", args, 1)?;
            if let PyObjectPayload::Class(cd) = &args[0].payload {
                let mut mro = vec![args[0].clone()];
                mro.extend(cd.mro.iter().cloned());
                Ok(PyObject::tuple(mro))
            } else if let Some(mro) = args[0].get_attr("__mro__") {
                Ok(mro)
            } else {
                Ok(PyObject::tuple(vec![args[0].clone()]))
            }
        })),
        ("getargspec", make_builtin(|args| {
            check_args("inspect.getargspec", args, 1)?;
            if let PyObjectPayload::Function(pf) = &args[0].payload {
                let code = &pf.code;
                let ac = code.arg_count as usize;
                let has_varargs = code.flags.contains(CodeFlags::VARARGS);
                let has_varkw = code.flags.contains(CodeFlags::VARKEYWORDS);
                let mut positional = Vec::new();
                for i in 0..ac {
                    if i < code.varnames.len() { positional.push(PyObject::str_val(code.varnames[i].clone())); }
                }
                let varargs = if has_varargs && ac < code.varnames.len() {
                    PyObject::str_val(code.varnames[ac].clone())
                } else { PyObject::none() };
                let kw_start = ac + if has_varargs { 1 } else { 0 };
                let kwc = code.kwonlyarg_count as usize;
                let varkw = if has_varkw && kw_start + kwc < code.varnames.len() {
                    PyObject::str_val(code.varnames[kw_start + kwc].clone())
                } else { PyObject::none() };
                let defaults = if pf.defaults.is_empty() { PyObject::none() } else { PyObject::tuple(pf.defaults.clone()) };
                Ok(PyObject::tuple(vec![PyObject::list(positional), varargs, varkw, defaults]))
            } else { Err(PyException::type_error("unsupported callable")) }
        })),
        ("classify_class_attrs", make_builtin(|args| {
            check_args("inspect.classify_class_attrs", args, 1)?;
            Ok(PyObject::list(vec![]))
        })),

        // ── Source inspection utilities ──
        ("cleandoc", make_builtin(|args| {
            check_args("inspect.cleandoc", args, 1)?;
            let doc = args[0].py_to_string();
            let lines: Vec<&str> = doc.lines().collect();
            if lines.is_empty() { return Ok(PyObject::str_val(CompactString::from(""))); }
            let mut start = 0;
            while start < lines.len() && lines[start].trim().is_empty() { start += 1; }
            let mut end = lines.len();
            while end > start && lines[end - 1].trim().is_empty() { end -= 1; }
            if start >= end { return Ok(PyObject::str_val(CompactString::from(""))); }
            let min_indent = lines[start..end].iter()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.len() - l.trim_start().len())
                .min().unwrap_or(0);
            let result: Vec<&str> = lines[start..end].iter()
                .map(|l| if l.len() >= min_indent { &l[min_indent..] } else { l.trim() })
                .collect();
            Ok(PyObject::str_val(CompactString::from(result.join("\n"))))
        })),
        ("unwrap", make_builtin(|args| {
            check_args("inspect.unwrap", args, 1)?;
            let mut func = args[0].clone();
            for _ in 0..100 {
                if let Some(wrapped) = func.get_attr("__wrapped__") { func = wrapped; } else { break; }
            }
            Ok(func)
        })),

        // ── Frame introspection ──
        ("getattr_static", make_builtin(|args| {
            // getattr_static(obj, name[, default]) — like getattr but no descriptor protocol
            if args.is_empty() || args.len() < 2 {
                return Err(PyException::type_error("getattr_static() requires at least 2 arguments"));
            }
            let name_str = args[1].py_to_string();
            if let Some(v) = args[0].get_attr(&name_str) {
                Ok(v)
            } else if args.len() >= 3 {
                Ok(args[2].clone())
            } else {
                Err(PyException::attribute_error(format!(
                    "'{}' object has no attribute '{}'", args[0].type_name(), name_str)))
            }
        })),
        ("currentframe", make_builtin(|_args| {
            let cls = PyObject::class(CompactString::from("frame"), vec![], IndexMap::new());
            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("f_lineno"), PyObject::int(0));
            attrs.insert(CompactString::from("f_code"), {
                let code_cls = PyObject::class(CompactString::from("code"), vec![], IndexMap::new());
                let mut code_attrs = IndexMap::new();
                code_attrs.insert(CompactString::from("co_filename"), PyObject::str_val(CompactString::from("<unknown>")));
                code_attrs.insert(CompactString::from("co_name"), PyObject::str_val(CompactString::from("<module>")));
                code_attrs.insert(CompactString::from("co_firstlineno"), PyObject::int(0));
                PyObject::instance_with_attrs(code_cls, code_attrs)
            });
            attrs.insert(CompactString::from("f_locals"), PyObject::dict(IndexMap::new()));
            attrs.insert(CompactString::from("f_globals"), PyObject::dict(IndexMap::new()));
            attrs.insert(CompactString::from("f_back"), PyObject::none());
            Ok(PyObject::instance_with_attrs(cls, attrs))
        })),
        ("stack", make_builtin(|_| {
            let cls = PyObject::class(CompactString::from("FrameInfo"), vec![], IndexMap::new());
            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("filename"), PyObject::str_val(CompactString::from("<unknown>")));
            attrs.insert(CompactString::from("lineno"), PyObject::int(0));
            attrs.insert(CompactString::from("function"), PyObject::str_val(CompactString::from("<module>")));
            attrs.insert(CompactString::from("code_context"), PyObject::none());
            attrs.insert(CompactString::from("index"), PyObject::none());
            let frame_info = PyObject::instance_with_attrs(cls, attrs);
            Ok(PyObject::list(vec![frame_info]))
        })),

        // ── Constants ──
        ("CO_VARARGS", PyObject::int(0x04)),
        ("CO_VARKEYWORDS", PyObject::int(0x08)),
        ("TPFLAGS_IS_ABSTRACT", PyObject::int(1 << 20)),
    ])
}

// ── dis module ──

pub fn create_dis_module() -> PyObjectRef {
    use ferrython_bytecode::code::ConstantValue;
    use ferrython_bytecode::opcode::Opcode;

    fn dis_dis(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("dis() requires a function argument"));
        }
        let obj = &args[0];
        let code: std::sync::Arc<ferrython_bytecode::CodeObject> = match &obj.payload {
            PyObjectPayload::Function(pf) => std::sync::Arc::clone(&pf.code),
            PyObjectPayload::Code(c) => std::sync::Arc::clone(c),
            PyObjectPayload::Str(s) => {
                // Auto-compile string source code, like CPython
                let source = s.as_str();
                match ferrython_parser::parse(source, "<dis>") {
                    Ok(module) => match ferrython_compiler::compile(&module, "<dis>") {
                        Ok(c) => std::sync::Arc::new(c),
                        Err(e) => return Err(PyException::type_error(
                            format!("could not compile source: {}", e)
                        )),
                    },
                    Err(e) => return Err(PyException::type_error(
                        format!("could not parse source: {:?}", e)
                    )),
                }
            }
            _ => return Err(PyException::type_error(
                format!("don't know how to disassemble {} objects", obj.type_name())
            )),
        };
        let output = disassemble_code_to_string(&code, 0);
        // Resolve file= keyword argument from trailing kwargs dict or positional arg
        let mut file_obj: Option<PyObjectRef> = None;
        if args.len() >= 2 {
            let last = &args[args.len() - 1];
            // kwargs packed as trailing dict by VM
            if let PyObjectPayload::Dict(map) = &last.payload {
                let r = map.read();
                if let Some(f) = r.get(&HashableKey::Str(CompactString::from("file"))) {
                    file_obj = Some(f.clone());
                }
            }
            // Also accept positional file-like object
            if file_obj.is_none() {
                if let PyObjectPayload::Instance(_) = &last.payload {
                    file_obj = Some(last.clone());
                }
            }
        }
        let mut written = false;
        if let Some(ref fobj) = file_obj {
            if let PyObjectPayload::Instance(ref inst) = fobj.payload {
                if let Some(write_fn) = inst.attrs.read().get("write").cloned() {
                    match &write_fn.payload {
                        PyObjectPayload::NativeFunction { func, .. } => {
                            func(&[PyObject::str_val(CompactString::from(output.as_str()))])?;
                            written = true;
                        }
                        PyObjectPayload::NativeClosure(nc) => {
                            (nc.func)(&[PyObject::str_val(CompactString::from(output.as_str()))])?;
                            written = true;
                        }
                        _ => {}
                    }
                }
            }
        }
        if !written {
            print!("{}", output);
        }
        Ok(PyObject::none())
    }

    fn disassemble_code_to_string(code: &ferrython_bytecode::CodeObject, indent: usize) -> String {
        let mut output = String::new();
        let pad = " ".repeat(indent);
        // Find line number for each instruction using lnotab
        let last_lineno = code.first_line_number;
        let mut line_for_offset: Vec<u32> = Vec::with_capacity(code.instructions.len());
        {
            let mut line = code.first_line_number;
            let mut lnotab_idx = 0;
            for i in 0..code.instructions.len() {
                while lnotab_idx + 1 < code.line_number_table.len() {
                    let (off, ln) = code.line_number_table[lnotab_idx];
                    if i >= off as usize {
                        line = ln;
                        lnotab_idx += 1;
                    } else {
                        break;
                    }
                }
                line_for_offset.push(line);
            }
        }

        let mut prev_line = 0u32;
        for (i, instr) in code.instructions.iter().enumerate() {
            let lineno = if i < line_for_offset.len() { line_for_offset[i] } else { last_lineno };
            let line_str = if lineno != prev_line {
                prev_line = lineno;
                format!("{:>4}", lineno)
            } else {
                "    ".to_string()
            };

            let arg_desc = format_dis_arg(code, instr.op, instr.arg);
            use std::fmt::Write;
            let _ = writeln!(output, "{}{} {:>6} {:<24} {}", pad, line_str, i * 2, format!("{:?}", instr.op), arg_desc);
        }

        // Recurse into nested code objects
        for c in &code.constants {
            if let ConstantValue::Code(nested) = c {
                output.push('\n');
                use std::fmt::Write;
                let _ = writeln!(output, "{}Disassembly of <code object {} at ...>:", pad, nested.name);
                output.push_str(&disassemble_code_to_string(nested, indent + 2));
            }
        }
        output
    }

    fn format_dis_arg(code: &ferrython_bytecode::CodeObject, op: Opcode, arg: u32) -> String {
        match op {
            Opcode::LoadConst => {
                if let Some(c) = code.constants.get(arg as usize) {
                    match c {
                        ConstantValue::Str(s) => format!("{:<4} ('{}')", arg, if s.len() > 30 { &s[..27] } else { s }),
                        ConstantValue::Integer(n) => format!("{:<4} ({})", arg, n),
                        ConstantValue::Float(f) => format!("{:<4} ({})", arg, f),
                        ConstantValue::None => format!("{:<4} (None)", arg),
                        ConstantValue::Bool(b) => format!("{:<4} ({})", arg, b),
                        ConstantValue::Code(c) => format!("{:<4} (<code object {}>)", arg, c.name),
                        ConstantValue::Tuple(t) => format!("{:<4} (tuple/{})", arg, t.len()),
                        _ => format!("{}", arg),
                    }
                } else { format!("{}", arg) }
            }
            Opcode::LoadName | Opcode::StoreName | Opcode::DeleteName
            | Opcode::LoadGlobal | Opcode::StoreGlobal | Opcode::DeleteGlobal
            | Opcode::LoadAttr | Opcode::StoreAttr | Opcode::DeleteAttr
            | Opcode::ImportName | Opcode::ImportFrom => {
                if let Some(n) = code.names.get(arg as usize) {
                    format!("{:<4} ({})", arg, n)
                } else { format!("{}", arg) }
            }
            Opcode::LoadFast | Opcode::StoreFast | Opcode::DeleteFast => {
                if let Some(n) = code.varnames.get(arg as usize) {
                    format!("{:<4} ({})", arg, n)
                } else { format!("{}", arg) }
            }
            Opcode::LoadDeref | Opcode::StoreDeref | Opcode::LoadClosure => {
                let nc = code.cellvars.len();
                let idx = arg as usize;
                if idx < nc {
                    code.cellvars.get(idx).map_or(format!("{}", arg), |n| format!("{:<4} (cell: {})", arg, n))
                } else {
                    code.freevars.get(idx - nc).map_or(format!("{}", arg), |n| format!("{:<4} (free: {})", arg, n))
                }
            }
            Opcode::CompareOp => {
                let op_name = match arg {
                    0 => "<", 1 => "<=", 2 => "==", 3 => "!=", 4 => ">", 5 => ">=",
                    6 => "in", 7 => "not in", 8 => "is", 9 => "is not",
                    10 => "exception match", _ => "?",
                };
                format!("{:<4} ({})", arg, op_name)
            }
            Opcode::JumpAbsolute | Opcode::JumpForward
            | Opcode::PopJumpIfTrue | Opcode::PopJumpIfFalse
            | Opcode::JumpIfTrueOrPop | Opcode::JumpIfFalseOrPop
            | Opcode::SetupExcept | Opcode::SetupFinally
            | Opcode::ForIter => {
                format!("{:<4} (to {})", arg, arg)
            }
            _ => {
                if arg != 0 { format!("{}", arg) } else { String::new() }
            }
        }
    }

    // code_info(x) — return formatted information about a code object
    fn dis_code_info(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Err(PyException::type_error("code_info() requires argument"));
        }
        let code: std::sync::Arc<ferrython_bytecode::CodeObject> = match &args[0].payload {
            PyObjectPayload::Function(pf) => std::sync::Arc::clone(&pf.code),
            PyObjectPayload::Code(c) => std::sync::Arc::clone(c),
            _ => return Err(PyException::type_error("don't know how to get code info")),
        };
        let mut info = String::new();
        info.push_str(&format!("Name:              {}\n", code.name));
        info.push_str(&format!("Filename:          {}\n", code.filename));
        info.push_str(&format!("Argument count:    {}\n", code.arg_count));
        info.push_str(&format!("Kw-only arguments: {}\n", code.kwonlyarg_count));
        info.push_str(&format!("Number of locals:  {}\n", code.varnames.len()));
        info.push_str(&format!("Stack size:        {}\n", code.instructions.len()));
        info.push_str(&format!("Flags:             0x{:04x}\n", code.flags));
        if !code.constants.is_empty() {
            info.push_str("Constants:\n");
            for (i, c) in code.constants.iter().enumerate() {
                let repr = match c {
                    ConstantValue::Str(s) => format!("'{}'", s),
                    ConstantValue::Integer(n) => format!("{}", n),
                    ConstantValue::Float(f) => format!("{}", f),
                    ConstantValue::None => "None".to_string(),
                    ConstantValue::Bool(b) => format!("{}", b),
                    ConstantValue::Code(c) => format!("<code object {}>", c.name),
                    _ => "...".to_string(),
                };
                info.push_str(&format!("   {}: {}\n", i, repr));
            }
        }
        if !code.names.is_empty() {
            info.push_str("Names:\n");
            for (i, n) in code.names.iter().enumerate() {
                info.push_str(&format!("   {}: {}\n", i, n));
            }
        }
        if !code.varnames.is_empty() {
            info.push_str("Variable names:\n");
            for (i, v) in code.varnames.iter().enumerate() {
                info.push_str(&format!("   {}: {}\n", i, v));
            }
        }
        Ok(PyObject::str_val(CompactString::from(info)))
    }

    // Instruction namedtuple-like class
    let instruction_cls = {
        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("__init__"), make_builtin(|_| Ok(PyObject::none())));
        PyObject::class(CompactString::from("Instruction"), vec![], ns)
    };

    // Bytecode(x) — iterable of Instruction objects
    let bytecode_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("Bytecode() requires argument"));
        }
        let code: std::sync::Arc<ferrython_bytecode::CodeObject> = match &args[0].payload {
            PyObjectPayload::Function(pf) => std::sync::Arc::clone(&pf.code),
            PyObjectPayload::Code(c) => std::sync::Arc::clone(c),
            _ => return Err(PyException::type_error("don't know how to disassemble")),
        };
        let mut instructions = Vec::new();
        for (i, instr) in code.instructions.iter().enumerate() {
            let opname = format!("{:?}", instr.op);
            let arg_desc = format_dis_arg(&code, instr.op, instr.arg);
            let inst_cls = PyObject::class(CompactString::from("Instruction"), vec![], IndexMap::new());
            let inst = PyObject::instance(inst_cls);
            if let PyObjectPayload::Instance(ref d) = inst.payload {
                let mut attrs = d.attrs.write();
                attrs.insert(CompactString::from("opcode"), PyObject::int(instr.op as i64));
                attrs.insert(CompactString::from("opname"), PyObject::str_val(CompactString::from(&opname)));
                attrs.insert(CompactString::from("arg"), PyObject::int(instr.arg as i64));
                attrs.insert(CompactString::from("argval"), PyObject::str_val(CompactString::from(&arg_desc)));
                attrs.insert(CompactString::from("offset"), PyObject::int((i * 2) as i64));
                attrs.insert(CompactString::from("is_jump_target"), PyObject::bool_val(false));
            }
            instructions.push(inst);
        }
        Ok(PyObject::list(instructions))
    });

    // show_code(x) — print code_info to stdout
    let show_code_fn = make_builtin(|args: &[PyObjectRef]| {
        let info = dis_code_info(args)?;
        println!("{}", info.py_to_string());
        Ok(PyObject::none())
    });

    make_module("dis", vec![
        ("dis", make_builtin(dis_dis)),
        ("disassemble", make_builtin(dis_dis)),
        ("code_info", make_builtin(dis_code_info)),
        ("show_code", show_code_fn),
        ("Bytecode", bytecode_fn),
        ("Instruction", instruction_cls),
    ])
}

// ── ast module ──

pub fn create_ast_module() -> PyObjectRef {
    let parse_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("ast.parse() requires source code argument"));
        }
        let source = args[0].py_to_string();
        let mut filename = "<string>".to_string();
        let mut mode = "exec".to_string();
        // Handle positional args
        for (i, arg) in args.iter().enumerate().skip(1) {
            // Check if it's a kwargs dict (trailing dict convention)
            if let PyObjectPayload::Dict(map) = &arg.payload {
                let r = map.read();
                for (k, v) in r.iter() {
                    match k.to_object().py_to_string().as_str() {
                        "filename" => filename = v.py_to_string(),
                        "mode" => mode = v.py_to_string(),
                        _ => {}
                    }
                }
            } else if i == 1 {
                filename = arg.py_to_string();
            } else if i == 2 {
                mode = arg.py_to_string();
            }
        }
        match mode.as_str() {
            "eval" => {
                match ferrython_parser::parse_expression(&source, &filename) {
                    Ok(expr) => {
                        let body = expr_to_pyobject(&expr);
                        let cls = PyObject::class(CompactString::from("Expression"), vec![], IndexMap::new());
                        let inst = PyObject::instance(cls);
                        set_node_attr(&inst, "body", body);
                        set_node_fields(&inst, &["body"]);
                        // Store source for compile() support
                        if let PyObjectPayload::Instance(ref data) = inst.payload {
                            let mut a = data.attrs.write();
                            a.insert(CompactString::from("__source__"), PyObject::str_val(CompactString::from(&source)));
                            a.insert(CompactString::from("__filename__"), PyObject::str_val(CompactString::from(&filename)));
                            a.insert(CompactString::from("__mode__"), PyObject::str_val(CompactString::from("eval")));
                        }
                        Ok(inst)
                    }
                    Err(e) => Err(PyException::syntax_error(format!("{}", e))),
                }
            }
            _ => {
                match ferrython_parser::parse(&source, &filename) {
                    Ok(module) => {
                        let obj = module_to_pyobject(&module);
                        // Store source for compile() support
                        if let PyObjectPayload::Instance(inst) = &obj.payload {
                            let mut a = inst.attrs.write();
                            a.insert(CompactString::from("__source__"), PyObject::str_val(CompactString::from(&source)));
                            a.insert(CompactString::from("__filename__"), PyObject::str_val(CompactString::from(&filename)));
                            a.insert(CompactString::from("__mode__"), PyObject::str_val(CompactString::from(&mode)));
                        }
                        Ok(obj)
                    }
                    Err(e) => Err(PyException::syntax_error(format!("{}", e))),
                }
            }
        }
    });

    let dump_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("ast.dump() requires node argument"));
        }
        let indent = if args.len() > 1 {
            args[1].as_int().map(|n| n as usize)
        } else {
            None
        };
        let include_attributes = args.len() > 2 && args[2].is_truthy();
        let result = dump_node(&args[0], indent, include_attributes, 0);
        Ok(PyObject::str_val(CompactString::from(result)))
    });

    let literal_eval_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("ast.literal_eval() requires string argument"));
        }
        let s = args[0].py_to_string();
        let trimmed = s.trim();
        if trimmed == "None" { return Ok(PyObject::none()); }
        if trimmed == "True" { return Ok(PyObject::bool_val(true)); }
        if trimmed == "False" { return Ok(PyObject::bool_val(false)); }
        if let Ok(n) = trimmed.parse::<i64>() { return Ok(PyObject::int(n)); }
        if let Ok(f) = trimmed.parse::<f64>() { return Ok(PyObject::float(f)); }
        if (trimmed.starts_with('"') && trimmed.ends_with('"')) || (trimmed.starts_with('\'') && trimmed.ends_with('\'')) {
            return Ok(PyObject::str_val(CompactString::from(&trimmed[1..trimmed.len()-1])));
        }
        // Use the real parser for complex literals
        match ferrython_parser::parse_expression(trimmed, "<literal_eval>") {
            Ok(expr) => eval_const_expr(&expr),
            Err(_) => Err(PyException::value_error(format!("malformed node or string: {}", trimmed))),
        }
    });

    let walk_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("ast.walk() requires node argument"));
        }
        let mut nodes = Vec::new();
        collect_ast_nodes(&args[0], &mut nodes);
        Ok(PyObject::list(nodes))
    });

    let get_docstring_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("ast.get_docstring() requires node argument"));
        }
        if let Some(body) = args[0].get_attr("body") {
            if let PyObjectPayload::List(items) = &body.payload {
                let items = items.read();
                if let Some(first) = items.first() {
                    let type_name = first.type_name();
                    if type_name == "Expr" {
                        if let Some(value) = first.get_attr("value") {
                            if value.type_name() == "Constant" {
                                if let Some(val) = value.get_attr("value") {
                                    if let PyObjectPayload::Str(s) = &val.payload {
                                        return Ok(PyObject::str_val(s.clone()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(PyObject::none())
    });

    let fix_missing_locations_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("ast.fix_missing_locations() requires node argument"));
        }
        // Walk all nodes and set missing lineno/col_offset to 1/0
        let mut nodes = Vec::new();
        collect_ast_nodes(&args[0], &mut nodes);
        for node in &nodes {
            if node.get_attr("lineno").is_none() {
                set_node_attr(node, "lineno", PyObject::int(1));
            }
            if node.get_attr("col_offset").is_none() {
                set_node_attr(node, "col_offset", PyObject::int(0));
            }
        }
        Ok(args[0].clone())
    });

    let increment_lineno_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 {
            return Err(PyException::type_error("ast.increment_lineno() requires node and n"));
        }
        let n = args[1].as_int().unwrap_or(1);
        let mut nodes = Vec::new();
        collect_ast_nodes(&args[0], &mut nodes);
        for node in &nodes {
            if let Some(lineno) = node.get_attr("lineno") {
                if let Some(line) = lineno.as_int() {
                    set_node_attr(node, "lineno", PyObject::int(line + n));
                }
            }
        }
        Ok(args[0].clone())
    });

    let iter_child_nodes_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("ast.iter_child_nodes() requires node argument"));
        }
        let children = get_child_nodes(&args[0]);
        Ok(PyObject::list(children))
    });

    let make_node_type = |name: &str, fields: &[&str]| -> PyObjectRef {
        let cls = get_or_create_ast_class(name);
        if let PyObjectPayload::Class(cd) = &cls.payload {
            let field_strs: Vec<PyObjectRef> = fields.iter()
                .map(|f| PyObject::str_val(CompactString::from(*f)))
                .collect();
            let mut ns = cd.namespace.write();
            ns.insert(CompactString::from("_fields"), PyObject::tuple(field_strs));
            // CPython AST nodes define _attributes for source location info
            ns.insert(CompactString::from("_attributes"), PyObject::tuple(vec![
                PyObject::str_val(CompactString::from("lineno")),
                PyObject::str_val(CompactString::from("col_offset")),
                PyObject::str_val(CompactString::from("end_lineno")),
                PyObject::str_val(CompactString::from("end_col_offset")),
            ]));
        }
        cls
    };

    // copy_location(new_node, old_node)
    let copy_location_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 {
            return Err(PyException::type_error("copy_location() requires new_node and old_node"));
        }
        let new_node = &args[0];
        let old_node = &args[1];
        for attr in &["lineno", "col_offset", "end_lineno", "end_col_offset"] {
            if let Some(val) = old_node.get_attr(attr) {
                if let PyObjectPayload::Instance(ref d) = new_node.payload {
                    d.attrs.write().insert(CompactString::from(*attr), val);
                }
            }
        }
        Ok(new_node.clone())
    });

    // unparse(node) — convert AST back to source code (simplified)
    let unparse_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("unparse() requires a node argument"));
        }
        let src = ast_unparse(&args[0]);
        Ok(PyObject::str_val(CompactString::from(src)))
    });

    make_module("ast", vec![
        ("parse", parse_fn),
        ("dump", dump_fn),
        ("literal_eval", literal_eval_fn),
        ("walk", walk_fn),
        ("get_docstring", get_docstring_fn),
        ("fix_missing_locations", fix_missing_locations_fn),
        ("increment_lineno", increment_lineno_fn),
        ("iter_child_nodes", iter_child_nodes_fn),
        ("copy_location", copy_location_fn),
        ("unparse", unparse_fn),
        // Node types (with ASDL field definitions for positional arg mapping)
        ("Module", make_node_type("Module", &["body", "type_ignores"])),
        ("Expression", make_node_type("Expression", &["body"])),
        ("Interactive", make_node_type("Interactive", &["body"])),
        ("FunctionDef", make_node_type("FunctionDef", &["name", "args", "body", "decorator_list", "returns"])),
        ("AsyncFunctionDef", make_node_type("AsyncFunctionDef", &["name", "args", "body", "decorator_list", "returns"])),
        ("ClassDef", make_node_type("ClassDef", &["name", "bases", "keywords", "body", "decorator_list"])),
        ("Return", make_node_type("Return", &["value"])),
        ("Assign", make_node_type("Assign", &["targets", "value"])),
        ("AugAssign", make_node_type("AugAssign", &["target", "op", "value"])),
        ("AnnAssign", make_node_type("AnnAssign", &["target", "annotation", "value", "simple"])),
        ("For", make_node_type("For", &["target", "iter", "body", "orelse"])),
        ("AsyncFor", make_node_type("AsyncFor", &["target", "iter", "body", "orelse"])),
        ("While", make_node_type("While", &["test", "body", "orelse"])),
        ("If", make_node_type("If", &["test", "body", "orelse"])),
        ("With", make_node_type("With", &["items", "body"])),
        ("AsyncWith", make_node_type("AsyncWith", &["items", "body"])),
        ("Raise", make_node_type("Raise", &["exc", "cause"])),
        ("Try", make_node_type("Try", &["body", "handlers", "orelse", "finalbody"])),
        ("Import", make_node_type("Import", &["names"])),
        ("ImportFrom", make_node_type("ImportFrom", &["module", "names", "level"])),
        ("Global", make_node_type("Global", &["names"])),
        ("Nonlocal", make_node_type("Nonlocal", &["names"])),
        ("Delete", make_node_type("Delete", &["targets"])),
        ("Assert", make_node_type("Assert", &["test", "msg"])),
        ("Expr", make_node_type("Expr", &["value"])),
        ("Name", make_node_type("Name", &["id", "ctx"])),
        ("Constant", make_node_type("Constant", &["value", "kind"])),
        ("BinOp", make_node_type("BinOp", &["left", "op", "right"])),
        ("UnaryOp", make_node_type("UnaryOp", &["op", "operand"])),
        ("BoolOp", make_node_type("BoolOp", &["op", "values"])),
        ("Compare", make_node_type("Compare", &["left", "ops", "comparators"])),
        ("Call", make_node_type("Call", &["func", "args", "keywords"])),
        ("Attribute", make_node_type("Attribute", &["value", "attr", "ctx"])),
        ("Subscript", make_node_type("Subscript", &["value", "slice", "ctx"])),
        ("Starred", make_node_type("Starred", &["value", "ctx"])),
        ("List", make_node_type("List", &["elts", "ctx"])),
        ("Tuple", make_node_type("Tuple", &["elts", "ctx"])),
        ("Dict", make_node_type("Dict", &["keys", "values"])),
        ("Set", make_node_type("Set", &["elts"])),
        ("Lambda", make_node_type("Lambda", &["args", "body"])),
        ("IfExp", make_node_type("IfExp", &["test", "body", "orelse"])),
        ("ListComp", make_node_type("ListComp", &["elt", "generators"])),
        ("SetComp", make_node_type("SetComp", &["elt", "generators"])),
        ("DictComp", make_node_type("DictComp", &["key", "value", "generators"])),
        ("GeneratorExp", make_node_type("GeneratorExp", &["elt", "generators"])),
        ("Yield", make_node_type("Yield", &["value"])),
        ("YieldFrom", make_node_type("YieldFrom", &["value"])),
        ("Await", make_node_type("Await", &["value"])),
        ("FormattedValue", make_node_type("FormattedValue", &["value", "conversion", "format_spec"])),
        ("JoinedStr", make_node_type("JoinedStr", &["values"])),
        ("NamedExpr", make_node_type("NamedExpr", &["target", "value"])),
        ("Slice", make_node_type("Slice", &["lower", "upper", "step"])),
        ("Pass", make_node_type("Pass", &[])),
        ("Break", make_node_type("Break", &[])),
        ("Continue", make_node_type("Continue", &[])),
        ("ExceptHandler", make_node_type("ExceptHandler", &["type", "name", "body"])),
        // Context types
        ("Load", make_node_type("Load", &[])),
        ("Store", make_node_type("Store", &[])),
        ("Del", make_node_type("Del", &[])),
        // Operator types
        ("Add", make_node_type("Add", &[])),
        ("Sub", make_node_type("Sub", &[])),
        ("Mult", make_node_type("Mult", &[])),
        ("Div", make_node_type("Div", &[])),
        ("Mod", make_node_type("Mod", &[])),
        ("Pow", make_node_type("Pow", &[])),
        ("LShift", make_node_type("LShift", &[])),
        ("RShift", make_node_type("RShift", &[])),
        ("BitOr", make_node_type("BitOr", &[])),
        ("BitXor", make_node_type("BitXor", &[])),
        ("BitAnd", make_node_type("BitAnd", &[])),
        ("FloorDiv", make_node_type("FloorDiv", &[])),
        ("MatMult", make_node_type("MatMult", &[])),
        ("And", make_node_type("And", &[])),
        ("Or", make_node_type("Or", &[])),
        ("Invert", make_node_type("Invert", &[])),
        ("Not", make_node_type("Not", &[])),
        ("UAdd", make_node_type("UAdd", &[])),
        ("USub", make_node_type("USub", &[])),
        ("Eq", make_node_type("Eq", &[])),
        ("NotEq", make_node_type("NotEq", &[])),
        ("Lt", make_node_type("Lt", &[])),
        ("LtE", make_node_type("LtE", &[])),
        ("Gt", make_node_type("Gt", &[])),
        ("GtE", make_node_type("GtE", &[])),
        ("Is", make_node_type("Is", &[])),
        ("IsNot", make_node_type("IsNot", &[])),
        ("In", make_node_type("In", &[])),
        ("NotIn", make_node_type("NotIn", &[])),
        // Misc
        ("arguments", make_node_type("arguments", &["posonlyargs", "args", "vararg", "kwonlyargs", "kw_defaults", "kwarg", "defaults"])),
        ("arg", make_node_type("arg", &["arg", "annotation"])),
        ("keyword", make_node_type("keyword", &["arg", "value"])),
        ("alias", make_node_type("alias", &["name", "asname"])),
        ("withitem", make_node_type("withitem", &["context_expr", "optional_vars"])),
        ("comprehension", make_node_type("comprehension", &["target", "iter", "ifs", "is_async"])),
        ("PyCF_ONLY_AST", PyObject::int(1024)),
        ("AST", make_node_type("AST", &[])),
    ])
}

// ── AST conversion helpers ──

fn set_node_attr(obj: &PyObjectRef, name: &str, value: PyObjectRef) {
    if let PyObjectPayload::Instance(ref d) = obj.payload {
        d.attrs.write().insert(CompactString::from(name), value);
    }
}

fn set_node_fields(obj: &PyObjectRef, fields: &[&str]) {
    let flds: Vec<PyObjectRef> = fields.iter()
        .map(|f| PyObject::str_val(CompactString::from(*f)))
        .collect();
    set_node_attr(obj, "_fields", PyObject::tuple(flds));
}

fn set_location(obj: &PyObjectRef, loc: &ferrython_ast::SourceLocation) {
    set_node_attr(obj, "lineno", PyObject::int(loc.line as i64));
    set_node_attr(obj, "col_offset", PyObject::int(loc.column as i64));
    set_node_attr(obj, "end_lineno", match loc.end_line {
        Some(l) => PyObject::int(l as i64),
        None => PyObject::none(),
    });
    set_node_attr(obj, "end_col_offset", match loc.end_column {
        Some(c) => PyObject::int(c as i64),
        None => PyObject::none(),
    });
}

fn make_ast_node(type_name: &str) -> PyObjectRef {
    let cls = get_or_create_ast_class(type_name);
    PyObject::instance(cls)
}

/// Get or create a shared AST class, so isinstance(ast.parse(...), ast.Module) works
fn get_or_create_ast_class(name: &str) -> PyObjectRef {
    use std::sync::Mutex;
    use std::collections::HashMap;
    use std::sync::LazyLock;
    static AST_CLASSES: LazyLock<Mutex<HashMap<String, PyObjectRef>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let mut map = AST_CLASSES.lock().unwrap();
    if let Some(cls) = map.get(name) {
        return cls.clone();
    }
    let cls = PyObject::class(CompactString::from(name), vec![], IndexMap::new());
    map.insert(name.to_string(), cls.clone());
    cls
}

/// Fix expression context to Store (for assignment targets) or Del (for delete targets)
fn fix_ctx(node: &PyObjectRef, ctx_name: &str) {
    set_node_attr(node, "ctx", make_ast_node(ctx_name));
    // Recursively fix children for starred, tuple, list
    let type_name = node.type_name();
    if type_name == "Tuple" || type_name == "List" {
        if let Some(elts) = node.get_attr("elts") {
            if let PyObjectPayload::List(items) = &elts.payload {
                for item in items.read().iter() {
                    fix_ctx(item, ctx_name);
                }
            }
        }
    } else if type_name == "Starred" {
        if let Some(val) = node.get_attr("value") {
            fix_ctx(&val, ctx_name);
        }
    }
}

fn module_to_pyobject(module: &ferrython_ast::Module) -> PyObjectRef {
    match module {
        ferrython_ast::Module::Module { body, type_ignores: _ } => {
            let node = make_ast_node("Module");
            let body_list: Vec<PyObjectRef> = body.iter().map(stmt_to_pyobject).collect();
            set_node_attr(&node, "body", PyObject::list(body_list));
            set_node_attr(&node, "type_ignores", PyObject::list(vec![]));
            set_node_fields(&node, &["body", "type_ignores"]);
            node
        }
        ferrython_ast::Module::Interactive { body } => {
            let node = make_ast_node("Interactive");
            let body_list: Vec<PyObjectRef> = body.iter().map(stmt_to_pyobject).collect();
            set_node_attr(&node, "body", PyObject::list(body_list));
            set_node_fields(&node, &["body"]);
            node
        }
        ferrython_ast::Module::Expression { body } => {
            let node = make_ast_node("Expression");
            set_node_attr(&node, "body", expr_to_pyobject(body));
            set_node_fields(&node, &["body"]);
            node
        }
    }
}

fn stmt_to_pyobject(stmt: &ferrython_ast::Statement) -> PyObjectRef {
    use ferrython_ast::StatementKind::*;
    let node = match &stmt.node {
        FunctionDef { name, args, body, decorator_list, returns, is_async, .. } => {
            let type_name = if *is_async { "AsyncFunctionDef" } else { "FunctionDef" };
            let n = make_ast_node(type_name);
            set_node_attr(&n, "name", PyObject::str_val(name.clone()));
            set_node_attr(&n, "args", args_to_pyobject(args));
            set_node_attr(&n, "body", PyObject::list(body.iter().map(stmt_to_pyobject).collect()));
            set_node_attr(&n, "decorator_list", PyObject::list(decorator_list.iter().map(expr_to_pyobject).collect()));
            set_node_attr(&n, "returns", match returns {
                Some(r) => expr_to_pyobject(r),
                None => PyObject::none(),
            });
            set_node_fields(&n, &["name", "args", "body", "decorator_list", "returns"]);
            n
        }
        ClassDef { name, bases, keywords, body, decorator_list } => {
            let n = make_ast_node("ClassDef");
            set_node_attr(&n, "name", PyObject::str_val(name.clone()));
            set_node_attr(&n, "bases", PyObject::list(bases.iter().map(expr_to_pyobject).collect()));
            set_node_attr(&n, "keywords", PyObject::list(keywords.iter().map(keyword_to_pyobject).collect()));
            set_node_attr(&n, "body", PyObject::list(body.iter().map(stmt_to_pyobject).collect()));
            set_node_attr(&n, "decorator_list", PyObject::list(decorator_list.iter().map(expr_to_pyobject).collect()));
            set_node_fields(&n, &["name", "bases", "keywords", "body", "decorator_list"]);
            n
        }
        Return { value } => {
            let n = make_ast_node("Return");
            set_node_attr(&n, "value", match value {
                Some(v) => expr_to_pyobject(v),
                None => PyObject::none(),
            });
            set_node_fields(&n, &["value"]);
            n
        }
        Delete { targets } => {
            let n = make_ast_node("Delete");
            let target_nodes: Vec<PyObjectRef> = targets.iter().map(|t| {
                let node = expr_to_pyobject(t);
                fix_ctx(&node, "Del");
                node
            }).collect();
            set_node_attr(&n, "targets", PyObject::list(target_nodes));
            set_node_fields(&n, &["targets"]);
            n
        }
        Assign { targets, value, .. } => {
            let n = make_ast_node("Assign");
            let target_nodes: Vec<PyObjectRef> = targets.iter().map(|t| {
                let node = expr_to_pyobject(t);
                fix_ctx(&node, "Store");
                node
            }).collect();
            set_node_attr(&n, "targets", PyObject::list(target_nodes));
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_fields(&n, &["targets", "value"]);
            n
        }
        AugAssign { target, op, value } => {
            let n = make_ast_node("AugAssign");
            let tgt = expr_to_pyobject(target);
            fix_ctx(&tgt, "Store");
            set_node_attr(&n, "target", tgt);
            set_node_attr(&n, "op", operator_to_pyobject(*op));
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_fields(&n, &["target", "op", "value"]);
            n
        }
        AnnAssign { target, annotation, value, simple } => {
            let n = make_ast_node("AnnAssign");
            let tgt = expr_to_pyobject(target);
            fix_ctx(&tgt, "Store");
            set_node_attr(&n, "target", tgt);
            set_node_attr(&n, "annotation", expr_to_pyobject(annotation));
            set_node_attr(&n, "value", match value {
                Some(v) => expr_to_pyobject(v),
                None => PyObject::none(),
            });
            set_node_attr(&n, "simple", PyObject::bool_val(*simple));
            set_node_fields(&n, &["target", "annotation", "value", "simple"]);
            n
        }
        For { target, iter, body, orelse, is_async, .. } => {
            let type_name = if *is_async { "AsyncFor" } else { "For" };
            let n = make_ast_node(type_name);
            let tgt = expr_to_pyobject(target);
            fix_ctx(&tgt, "Store");
            set_node_attr(&n, "target", tgt);
            set_node_attr(&n, "iter", expr_to_pyobject(iter));
            set_node_attr(&n, "body", PyObject::list(body.iter().map(stmt_to_pyobject).collect()));
            set_node_attr(&n, "orelse", PyObject::list(orelse.iter().map(stmt_to_pyobject).collect()));
            set_node_fields(&n, &["target", "iter", "body", "orelse"]);
            n
        }
        While { test, body, orelse } => {
            let n = make_ast_node("While");
            set_node_attr(&n, "test", expr_to_pyobject(test));
            set_node_attr(&n, "body", PyObject::list(body.iter().map(stmt_to_pyobject).collect()));
            set_node_attr(&n, "orelse", PyObject::list(orelse.iter().map(stmt_to_pyobject).collect()));
            set_node_fields(&n, &["test", "body", "orelse"]);
            n
        }
        If { test, body, orelse } => {
            let n = make_ast_node("If");
            set_node_attr(&n, "test", expr_to_pyobject(test));
            set_node_attr(&n, "body", PyObject::list(body.iter().map(stmt_to_pyobject).collect()));
            set_node_attr(&n, "orelse", PyObject::list(orelse.iter().map(stmt_to_pyobject).collect()));
            set_node_fields(&n, &["test", "body", "orelse"]);
            n
        }
        With { items, body, is_async, .. } => {
            let type_name = if *is_async { "AsyncWith" } else { "With" };
            let n = make_ast_node(type_name);
            set_node_attr(&n, "items", PyObject::list(items.iter().map(withitem_to_pyobject).collect()));
            set_node_attr(&n, "body", PyObject::list(body.iter().map(stmt_to_pyobject).collect()));
            set_node_fields(&n, &["items", "body"]);
            n
        }
        Raise { exc, cause } => {
            let n = make_ast_node("Raise");
            set_node_attr(&n, "exc", match exc { Some(e) => expr_to_pyobject(e), None => PyObject::none() });
            set_node_attr(&n, "cause", match cause { Some(c) => expr_to_pyobject(c), None => PyObject::none() });
            set_node_fields(&n, &["exc", "cause"]);
            n
        }
        Try { body, handlers, orelse, finalbody } => {
            let n = make_ast_node("Try");
            set_node_attr(&n, "body", PyObject::list(body.iter().map(stmt_to_pyobject).collect()));
            set_node_attr(&n, "handlers", PyObject::list(handlers.iter().map(except_handler_to_pyobject).collect()));
            set_node_attr(&n, "orelse", PyObject::list(orelse.iter().map(stmt_to_pyobject).collect()));
            set_node_attr(&n, "finalbody", PyObject::list(finalbody.iter().map(stmt_to_pyobject).collect()));
            set_node_fields(&n, &["body", "handlers", "orelse", "finalbody"]);
            n
        }
        Assert { test, msg } => {
            let n = make_ast_node("Assert");
            set_node_attr(&n, "test", expr_to_pyobject(test));
            set_node_attr(&n, "msg", match msg { Some(m) => expr_to_pyobject(m), None => PyObject::none() });
            set_node_fields(&n, &["test", "msg"]);
            n
        }
        Import { names } => {
            let n = make_ast_node("Import");
            set_node_attr(&n, "names", PyObject::list(names.iter().map(alias_to_pyobject).collect()));
            set_node_fields(&n, &["names"]);
            n
        }
        ImportFrom { module, names, level } => {
            let n = make_ast_node("ImportFrom");
            set_node_attr(&n, "module", match module {
                Some(m) => PyObject::str_val(m.clone()),
                None => PyObject::none(),
            });
            set_node_attr(&n, "names", PyObject::list(names.iter().map(alias_to_pyobject).collect()));
            set_node_attr(&n, "level", PyObject::int(*level as i64));
            set_node_fields(&n, &["module", "names", "level"]);
            n
        }
        Global { names } => {
            let n = make_ast_node("Global");
            set_node_attr(&n, "names", PyObject::list(names.iter().map(|s| PyObject::str_val(s.clone())).collect()));
            set_node_fields(&n, &["names"]);
            n
        }
        Nonlocal { names } => {
            let n = make_ast_node("Nonlocal");
            set_node_attr(&n, "names", PyObject::list(names.iter().map(|s| PyObject::str_val(s.clone())).collect()));
            set_node_fields(&n, &["names"]);
            n
        }
        Expr { value } => {
            let n = make_ast_node("Expr");
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_fields(&n, &["value"]);
            n
        }
        Pass => { let n = make_ast_node("Pass"); set_node_fields(&n, &[]); n }
        Break => { let n = make_ast_node("Break"); set_node_fields(&n, &[]); n }
        Continue => { let n = make_ast_node("Continue"); set_node_fields(&n, &[]); n }
        Match { subject, cases } => {
            let n = make_ast_node("Match");
            set_node_attr(&n, "subject", expr_to_pyobject(subject));
            let case_nodes: Vec<PyObjectRef> = cases.iter().map(|c| {
                let cn = make_ast_node("match_case");
                set_node_attr(&cn, "body", PyObject::list(c.body.iter().map(stmt_to_pyobject).collect()));
                set_node_attr(&cn, "guard", match &c.guard {
                    Some(g) => expr_to_pyobject(g),
                    None => PyObject::none(),
                });
                set_node_fields(&cn, &["pattern", "guard", "body"]);
                cn
            }).collect();
            set_node_attr(&n, "cases", PyObject::list(case_nodes));
            set_node_fields(&n, &["subject", "cases"]);
            n
        }
    };
    set_location(&node, &stmt.location);
    node
}

fn expr_to_pyobject(expr: &ferrython_ast::Expression) -> PyObjectRef {
    use ferrython_ast::ExpressionKind::*;
    let node = match &expr.node {
        BoolOp { op, values } => {
            let n = make_ast_node("BoolOp");
            set_node_attr(&n, "op", boolop_to_pyobject(*op));
            set_node_attr(&n, "values", PyObject::list(values.iter().map(expr_to_pyobject).collect()));
            set_node_fields(&n, &["op", "values"]);
            n
        }
        NamedExpr { target, value } => {
            let n = make_ast_node("NamedExpr");
            set_node_attr(&n, "target", expr_to_pyobject(target));
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_fields(&n, &["target", "value"]);
            n
        }
        BinOp { left, op, right } => {
            let n = make_ast_node("BinOp");
            set_node_attr(&n, "left", expr_to_pyobject(left));
            set_node_attr(&n, "op", operator_to_pyobject(*op));
            set_node_attr(&n, "right", expr_to_pyobject(right));
            set_node_fields(&n, &["left", "op", "right"]);
            n
        }
        UnaryOp { op, operand } => {
            let n = make_ast_node("UnaryOp");
            set_node_attr(&n, "op", unaryop_to_pyobject(*op));
            set_node_attr(&n, "operand", expr_to_pyobject(operand));
            set_node_fields(&n, &["op", "operand"]);
            n
        }
        Lambda { args, body } => {
            let n = make_ast_node("Lambda");
            set_node_attr(&n, "args", args_to_pyobject(args));
            set_node_attr(&n, "body", expr_to_pyobject(body));
            set_node_fields(&n, &["args", "body"]);
            n
        }
        IfExp { test, body, orelse } => {
            let n = make_ast_node("IfExp");
            set_node_attr(&n, "test", expr_to_pyobject(test));
            set_node_attr(&n, "body", expr_to_pyobject(body));
            set_node_attr(&n, "orelse", expr_to_pyobject(orelse));
            set_node_fields(&n, &["test", "body", "orelse"]);
            n
        }
        Dict { keys, values } => {
            let n = make_ast_node("Dict");
            let key_list: Vec<PyObjectRef> = keys.iter().map(|k| match k {
                Some(e) => expr_to_pyobject(e),
                None => PyObject::none(),
            }).collect();
            set_node_attr(&n, "keys", PyObject::list(key_list));
            set_node_attr(&n, "values", PyObject::list(values.iter().map(expr_to_pyobject).collect()));
            set_node_fields(&n, &["keys", "values"]);
            n
        }
        Set { elts } => {
            let n = make_ast_node("Set");
            set_node_attr(&n, "elts", PyObject::list(elts.iter().map(expr_to_pyobject).collect()));
            set_node_fields(&n, &["elts"]);
            n
        }
        ListComp { elt, generators } => {
            let n = make_ast_node("ListComp");
            set_node_attr(&n, "elt", expr_to_pyobject(elt));
            set_node_attr(&n, "generators", PyObject::list(generators.iter().map(comprehension_to_pyobject).collect()));
            set_node_fields(&n, &["elt", "generators"]);
            n
        }
        SetComp { elt, generators } => {
            let n = make_ast_node("SetComp");
            set_node_attr(&n, "elt", expr_to_pyobject(elt));
            set_node_attr(&n, "generators", PyObject::list(generators.iter().map(comprehension_to_pyobject).collect()));
            set_node_fields(&n, &["elt", "generators"]);
            n
        }
        DictComp { key, value, generators } => {
            let n = make_ast_node("DictComp");
            set_node_attr(&n, "key", expr_to_pyobject(key));
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_attr(&n, "generators", PyObject::list(generators.iter().map(comprehension_to_pyobject).collect()));
            set_node_fields(&n, &["key", "value", "generators"]);
            n
        }
        GeneratorExp { elt, generators } => {
            let n = make_ast_node("GeneratorExp");
            set_node_attr(&n, "elt", expr_to_pyobject(elt));
            set_node_attr(&n, "generators", PyObject::list(generators.iter().map(comprehension_to_pyobject).collect()));
            set_node_fields(&n, &["elt", "generators"]);
            n
        }
        Await { value } => {
            let n = make_ast_node("Await");
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_fields(&n, &["value"]);
            n
        }
        Yield { value } => {
            let n = make_ast_node("Yield");
            set_node_attr(&n, "value", match value { Some(v) => expr_to_pyobject(v), None => PyObject::none() });
            set_node_fields(&n, &["value"]);
            n
        }
        YieldFrom { value } => {
            let n = make_ast_node("YieldFrom");
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_fields(&n, &["value"]);
            n
        }
        Compare { left, ops, comparators } => {
            let n = make_ast_node("Compare");
            set_node_attr(&n, "left", expr_to_pyobject(left));
            set_node_attr(&n, "ops", PyObject::list(ops.iter().map(|o| cmpop_to_pyobject(*o)).collect()));
            set_node_attr(&n, "comparators", PyObject::list(comparators.iter().map(expr_to_pyobject).collect()));
            set_node_fields(&n, &["left", "ops", "comparators"]);
            n
        }
        Call { func, args, keywords } => {
            let n = make_ast_node("Call");
            set_node_attr(&n, "func", expr_to_pyobject(func));
            set_node_attr(&n, "args", PyObject::list(args.iter().map(expr_to_pyobject).collect()));
            set_node_attr(&n, "keywords", PyObject::list(keywords.iter().map(keyword_to_pyobject).collect()));
            set_node_fields(&n, &["func", "args", "keywords"]);
            n
        }
        FormattedValue { value, conversion, format_spec } => {
            let n = make_ast_node("FormattedValue");
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_attr(&n, "conversion", match conversion {
                Some(c) => PyObject::int(*c as i64),
                None => PyObject::int(-1),
            });
            set_node_attr(&n, "format_spec", match format_spec {
                Some(s) => expr_to_pyobject(s),
                None => PyObject::none(),
            });
            set_node_fields(&n, &["value", "conversion", "format_spec"]);
            n
        }
        JoinedStr { values } => {
            let n = make_ast_node("JoinedStr");
            set_node_attr(&n, "values", PyObject::list(values.iter().map(expr_to_pyobject).collect()));
            set_node_fields(&n, &["values"]);
            n
        }
        Constant { value } => {
            let n = make_ast_node("Constant");
            set_node_attr(&n, "value", constant_to_pyobject(value));
            set_node_fields(&n, &["value"]);
            n
        }
        Attribute { value, attr, ctx } => {
            let n = make_ast_node("Attribute");
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_attr(&n, "attr", PyObject::str_val(attr.clone()));
            set_node_attr(&n, "ctx", ctx_to_pyobject(*ctx));
            set_node_fields(&n, &["value", "attr", "ctx"]);
            n
        }
        Subscript { value, slice, ctx } => {
            let n = make_ast_node("Subscript");
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_attr(&n, "slice", expr_to_pyobject(slice));
            set_node_attr(&n, "ctx", ctx_to_pyobject(*ctx));
            set_node_fields(&n, &["value", "slice", "ctx"]);
            n
        }
        Starred { value, ctx } => {
            let n = make_ast_node("Starred");
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_attr(&n, "ctx", ctx_to_pyobject(*ctx));
            set_node_fields(&n, &["value", "ctx"]);
            n
        }
        Name { id, ctx } => {
            let n = make_ast_node("Name");
            set_node_attr(&n, "id", PyObject::str_val(id.clone()));
            set_node_attr(&n, "ctx", ctx_to_pyobject(*ctx));
            set_node_fields(&n, &["id", "ctx"]);
            n
        }
        List { elts, ctx } => {
            let n = make_ast_node("List");
            set_node_attr(&n, "elts", PyObject::list(elts.iter().map(expr_to_pyobject).collect()));
            set_node_attr(&n, "ctx", ctx_to_pyobject(*ctx));
            set_node_fields(&n, &["elts", "ctx"]);
            n
        }
        Tuple { elts, ctx } => {
            let n = make_ast_node("Tuple");
            set_node_attr(&n, "elts", PyObject::list(elts.iter().map(expr_to_pyobject).collect()));
            set_node_attr(&n, "ctx", ctx_to_pyobject(*ctx));
            set_node_fields(&n, &["elts", "ctx"]);
            n
        }
        Slice { lower, upper, step } => {
            let n = make_ast_node("Slice");
            set_node_attr(&n, "lower", match lower { Some(e) => expr_to_pyobject(e), None => PyObject::none() });
            set_node_attr(&n, "upper", match upper { Some(e) => expr_to_pyobject(e), None => PyObject::none() });
            set_node_attr(&n, "step", match step { Some(e) => expr_to_pyobject(e), None => PyObject::none() });
            set_node_fields(&n, &["lower", "upper", "step"]);
            n
        }
    };
    set_location(&node, &expr.location);
    node
}

fn constant_to_pyobject(c: &ferrython_ast::Constant) -> PyObjectRef {
    match c {
        ferrython_ast::Constant::None => PyObject::none(),
        ferrython_ast::Constant::Bool(b) => PyObject::bool_val(*b),
        ferrython_ast::Constant::Int(i) => match i {
            ferrython_ast::BigInt::Small(n) => PyObject::int(*n),
            ferrython_ast::BigInt::Big(b) => PyObject::big_int(b.as_ref().clone()),
        },
        ferrython_ast::Constant::Float(f) => PyObject::float(*f),
        ferrython_ast::Constant::Complex { real, imag } => {
            PyObject::complex(*real, *imag)
        }
        ferrython_ast::Constant::Str(s) => PyObject::str_val(s.clone()),
        ferrython_ast::Constant::Bytes(b) => PyObject::bytes(b.clone()),
        ferrython_ast::Constant::Ellipsis => PyObject::ellipsis(),
    }
}

fn operator_to_pyobject(op: ferrython_ast::Operator) -> PyObjectRef {
    use ferrython_ast::Operator::*;
    make_ast_node(match op {
        Add => "Add", Sub => "Sub", Mult => "Mult", MatMult => "MatMult",
        Div => "Div", Mod => "Mod", Pow => "Pow", LShift => "LShift",
        RShift => "RShift", BitOr => "BitOr", BitXor => "BitXor",
        BitAnd => "BitAnd", FloorDiv => "FloorDiv",
    })
}

fn boolop_to_pyobject(op: ferrython_ast::BoolOperator) -> PyObjectRef {
    make_ast_node(match op {
        ferrython_ast::BoolOperator::And => "And",
        ferrython_ast::BoolOperator::Or => "Or",
    })
}

fn unaryop_to_pyobject(op: ferrython_ast::UnaryOperator) -> PyObjectRef {
    use ferrython_ast::UnaryOperator::*;
    make_ast_node(match op {
        Invert => "Invert", Not => "Not", UAdd => "UAdd", USub => "USub",
    })
}

fn cmpop_to_pyobject(op: ferrython_ast::CompareOperator) -> PyObjectRef {
    use ferrython_ast::CompareOperator::*;
    make_ast_node(match op {
        Eq => "Eq", NotEq => "NotEq", Lt => "Lt", LtE => "LtE",
        Gt => "Gt", GtE => "GtE", Is => "Is", IsNot => "IsNot",
        In => "In", NotIn => "NotIn",
    })
}

fn ctx_to_pyobject(ctx: ferrython_ast::ExprContext) -> PyObjectRef {
    make_ast_node(match ctx {
        ferrython_ast::ExprContext::Load => "Load",
        ferrython_ast::ExprContext::Store => "Store",
        ferrython_ast::ExprContext::Del => "Del",
    })
}

fn args_to_pyobject(args: &ferrython_ast::Arguments) -> PyObjectRef {
    let n = make_ast_node("arguments");
    set_node_attr(&n, "posonlyargs", PyObject::list(args.posonlyargs.iter().map(arg_to_pyobject).collect()));
    set_node_attr(&n, "args", PyObject::list(args.args.iter().map(arg_to_pyobject).collect()));
    set_node_attr(&n, "vararg", match &args.vararg {
        Some(a) => arg_to_pyobject(a),
        None => PyObject::none(),
    });
    set_node_attr(&n, "kwonlyargs", PyObject::list(args.kwonlyargs.iter().map(arg_to_pyobject).collect()));
    set_node_attr(&n, "kw_defaults", PyObject::list(args.kw_defaults.iter().map(|d| match d {
        Some(e) => expr_to_pyobject(e),
        None => PyObject::none(),
    }).collect()));
    set_node_attr(&n, "kwarg", match &args.kwarg {
        Some(a) => arg_to_pyobject(a),
        None => PyObject::none(),
    });
    set_node_attr(&n, "defaults", PyObject::list(args.defaults.iter().map(expr_to_pyobject).collect()));
    set_node_fields(&n, &["posonlyargs", "args", "vararg", "kwonlyargs", "kw_defaults", "kwarg", "defaults"]);
    n
}

fn arg_to_pyobject(arg: &ferrython_ast::Arg) -> PyObjectRef {
    let n = make_ast_node("arg");
    set_node_attr(&n, "arg", PyObject::str_val(arg.arg.clone()));
    set_node_attr(&n, "annotation", match &arg.annotation {
        Some(a) => expr_to_pyobject(a),
        None => PyObject::none(),
    });
    set_location(&n, &arg.location);
    set_node_fields(&n, &["arg", "annotation"]);
    n
}

fn keyword_to_pyobject(kw: &ferrython_ast::Keyword) -> PyObjectRef {
    let n = make_ast_node("keyword");
    set_node_attr(&n, "arg", match &kw.arg {
        Some(a) => PyObject::str_val(a.clone()),
        None => PyObject::none(),
    });
    set_node_attr(&n, "value", expr_to_pyobject(&kw.value));
    set_location(&n, &kw.location);
    set_node_fields(&n, &["arg", "value"]);
    n
}

fn alias_to_pyobject(alias: &ferrython_ast::Alias) -> PyObjectRef {
    let n = make_ast_node("alias");
    set_node_attr(&n, "name", PyObject::str_val(alias.name.clone()));
    set_node_attr(&n, "asname", match &alias.asname {
        Some(a) => PyObject::str_val(a.clone()),
        None => PyObject::none(),
    });
    set_location(&n, &alias.location);
    set_node_fields(&n, &["name", "asname"]);
    n
}

fn withitem_to_pyobject(item: &ferrython_ast::WithItem) -> PyObjectRef {
    let n = make_ast_node("withitem");
    set_node_attr(&n, "context_expr", expr_to_pyobject(&item.context_expr));
    set_node_attr(&n, "optional_vars", match &item.optional_vars {
        Some(v) => expr_to_pyobject(v),
        None => PyObject::none(),
    });
    set_node_fields(&n, &["context_expr", "optional_vars"]);
    n
}

fn except_handler_to_pyobject(handler: &ferrython_ast::ExceptHandler) -> PyObjectRef {
    let n = make_ast_node("ExceptHandler");
    set_node_attr(&n, "type", match &handler.typ {
        Some(t) => expr_to_pyobject(t),
        None => PyObject::none(),
    });
    set_node_attr(&n, "name", match &handler.name {
        Some(nm) => PyObject::str_val(nm.clone()),
        None => PyObject::none(),
    });
    set_node_attr(&n, "body", PyObject::list(handler.body.iter().map(stmt_to_pyobject).collect()));
    set_location(&n, &handler.location);
    set_node_fields(&n, &["type", "name", "body"]);
    n
}

fn comprehension_to_pyobject(comp: &ferrython_ast::Comprehension) -> PyObjectRef {
    let n = make_ast_node("comprehension");
    set_node_attr(&n, "target", expr_to_pyobject(&comp.target));
    set_node_attr(&n, "iter", expr_to_pyobject(&comp.iter));
    set_node_attr(&n, "ifs", PyObject::list(comp.ifs.iter().map(expr_to_pyobject).collect()));
    set_node_attr(&n, "is_async", PyObject::bool_val(comp.is_async));
    set_node_fields(&n, &["target", "iter", "ifs", "is_async"]);
    n
}

/// Evaluate a constant expression for ast.literal_eval
fn eval_const_expr(expr: &ferrython_ast::Expression) -> PyResult<PyObjectRef> {
    use ferrython_ast::ExpressionKind::*;
    match &expr.node {
        Constant { value } => Ok(constant_to_pyobject(value)),
        List { elts, .. } => {
            let items: Result<Vec<_>, _> = elts.iter().map(eval_const_expr).collect();
            Ok(PyObject::list(items?))
        }
        Tuple { elts, .. } => {
            let items: Result<Vec<_>, _> = elts.iter().map(eval_const_expr).collect();
            Ok(PyObject::tuple(items?))
        }
        Set { elts } => {
            let items: Result<Vec<_>, _> = elts.iter().map(eval_const_expr).collect();
            let items = items?;
            let mut map = IndexMap::new();
            for item in &items {
                if let Ok(key) = ferrython_core::types::HashableKey::from_object(item) {
                    map.insert(key, item.clone());
                }
            }
            Ok(PyObject::frozenset(map))
        }
        Dict { keys, values } => {
            let mut map = IndexMap::new();
            for (k, v) in keys.iter().zip(values.iter()) {
                let val = eval_const_expr(v)?;
                if let Some(key_expr) = k {
                    let key_obj = eval_const_expr(key_expr)?;
                    if let Ok(hk) = ferrython_core::types::HashableKey::from_object(&key_obj) {
                        map.insert(hk, val);
                    }
                }
            }
            Ok(PyObject::dict(map))
        }
        UnaryOp { op, operand } => {
            let val = eval_const_expr(operand)?;
            match op {
                ferrython_ast::UnaryOperator::USub => {
                    if let Some(n) = val.as_int() { return Ok(PyObject::int(-n)); }
                    if let PyObjectPayload::Float(f) = &val.payload { return Ok(PyObject::float(-f)); }
                    Err(PyException::value_error("malformed node or string"))
                }
                ferrython_ast::UnaryOperator::UAdd => Ok(val),
                _ => Err(PyException::value_error("malformed node or string")),
            }
        }
        BinOp { left, op, right } => {
            // Only allow Add/Sub for complex number literals: 1+2j, 1-2j
            let l = eval_const_expr(left)?;
            let r = eval_const_expr(right)?;
            match op {
                ferrython_ast::Operator::Add | ferrython_ast::Operator::Sub => {
                    // Must be numeric
                    if l.as_int().is_some() || matches!(&l.payload, PyObjectPayload::Float(_)) {
                        if r.as_int().is_some() || matches!(&r.payload, PyObjectPayload::Float(_)) {
                            return Err(PyException::value_error("malformed node or string"));
                        }
                    }
                    Err(PyException::value_error("malformed node or string"))
                }
                _ => Err(PyException::value_error("malformed node or string")),
            }
        }
        _ => Err(PyException::value_error("malformed node or string")),
    }
}

/// ast.dump() — recursively dump an AST node to string
fn dump_node(obj: &PyObjectRef, indent: Option<usize>, include_attrs: bool, depth: usize) -> String {
    let type_name = obj.type_name();
    // Get _fields to know which attributes to dump
    let fields = obj.get_attr("_fields");
    if fields.is_none() {
        // Not an AST node — dump as value
        return format_value(obj);
    }
    let fields = fields.unwrap();
    let field_names: Vec<String> = if let PyObjectPayload::Tuple(items) = &fields.payload {
        items.iter().map(|f| f.py_to_string()).collect()
    } else {
        vec![]
    };

    let mut parts: Vec<String> = Vec::new();
    for name in &field_names {
        if let Some(val) = obj.get_attr(name) {
            let val_str = dump_value(&val, indent, include_attrs, depth + 1);
            parts.push(format!("{}={}", name, val_str));
        }
    }

    if include_attrs {
        for attr in &["lineno", "col_offset", "end_lineno", "end_col_offset"] {
            if let Some(val) = obj.get_attr(attr) {
                if !matches!(&val.payload, PyObjectPayload::None) {
                    parts.push(format!("{}={}", attr, format_value(&val)));
                }
            }
        }
    }

    if let Some(indent_size) = indent {
        if parts.is_empty() {
            format!("{}()", type_name)
        } else {
            let indent_str = " ".repeat(indent_size * (depth + 1));
            let inner = parts.iter()
                .map(|p| format!("{}{}", indent_str, p))
                .collect::<Vec<_>>()
                .join(",\n");
            format!("{}(\n{})", type_name, inner)
        }
    } else {
        format!("{}({})", type_name, parts.join(", "))
    }
}

fn dump_value(obj: &PyObjectRef, indent: Option<usize>, include_attrs: bool, depth: usize) -> String {
    // Check if it's an AST node (has _fields)
    if obj.get_attr("_fields").is_some() {
        return dump_node(obj, indent, include_attrs, depth);
    }
    // Check if it's a list of AST nodes
    if let PyObjectPayload::List(items) = &obj.payload {
        let items = items.read();
        if items.is_empty() {
            return "[]".to_string();
        }
        let inner: Vec<String> = items.iter()
            .map(|item| dump_value(item, indent, include_attrs, depth))
            .collect();
        if let Some(indent_size) = indent {
            let indent_str = " ".repeat(indent_size * (depth + 1));
            let entries = inner.iter()
                .map(|e| format!("{}{}", indent_str, e))
                .collect::<Vec<_>>()
                .join(",\n");
            format!("[\n{}]", entries)
        } else {
            format!("[{}]", inner.join(", "))
        }
    } else {
        format_value(obj)
    }
}

fn format_value(obj: &PyObjectRef) -> String {
    match &obj.payload {
        PyObjectPayload::None => "None".to_string(),
        PyObjectPayload::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        PyObjectPayload::Int(_) => obj.py_to_string(),
        PyObjectPayload::Float(f) => format!("{}", f),
        PyObjectPayload::Str(s) => format!("'{}'", s),
        PyObjectPayload::Bytes(b) => format!("b{:?}", String::from_utf8_lossy(b)),
        _ => obj.py_to_string(),
    }
}

/// Collect all AST nodes recursively for ast.walk()
fn collect_ast_nodes(obj: &PyObjectRef, nodes: &mut Vec<PyObjectRef>) {
    if obj.get_attr("_fields").is_none() { return; }
    nodes.push(obj.clone());
    if let Some(fields) = obj.get_attr("_fields") {
        if let PyObjectPayload::Tuple(field_names) = &fields.payload {
            for fname in field_names.iter() {
                let name = fname.py_to_string();
                if let Some(val) = obj.get_attr(&name) {
                    if val.get_attr("_fields").is_some() {
                        collect_ast_nodes(&val, nodes);
                    } else if let PyObjectPayload::List(items) = &val.payload {
                        for item in items.read().iter() {
                            collect_ast_nodes(item, nodes);
                        }
                    }
                }
            }
        }
    }
}

/// Get immediate child nodes for ast.iter_child_nodes()
fn get_child_nodes(obj: &PyObjectRef) -> Vec<PyObjectRef> {
    let mut children = Vec::new();
    if let Some(fields) = obj.get_attr("_fields") {
        if let PyObjectPayload::Tuple(field_names) = &fields.payload {
            for fname in field_names.iter() {
                let name = fname.py_to_string();
                if let Some(val) = obj.get_attr(&name) {
                    if val.get_attr("_fields").is_some() {
                        children.push(val);
                    } else if let PyObjectPayload::List(items) = &val.payload {
                        for item in items.read().iter() {
                            if item.get_attr("_fields").is_some() {
                                children.push(item.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    children
}

/// Public API for compile() to use when compiling programmatically-built AST
pub fn ast_unparse_module(node: &PyObjectRef) -> String {
    ast_unparse(node)
}

/// Simplified AST unparse — convert AST node back to Python source
fn ast_unparse(node: &PyObjectRef) -> String {
    // Use the class name (type_name()) which is "Module", "Assign", etc.
    let type_name = node.type_name().to_string();
    match type_name.as_str() {
        "Module" => {
            if let Some(body) = node.get_attr("body") {
                if let PyObjectPayload::List(items) = &body.payload {
                    return items.read().iter().map(|s| ast_unparse(s)).collect::<Vec<_>>().join("\n");
                }
            }
            String::new()
        }
        "Assign" => {
            let targets = node.get_attr("targets").map(|t| {
                if let PyObjectPayload::List(items) = &t.payload {
                    items.read().iter().map(|t| ast_unparse(t)).collect::<Vec<_>>().join(", ")
                } else { ast_unparse(&t) }
            }).unwrap_or_default();
            let value = node.get_attr("value").map(|v| ast_unparse(&v)).unwrap_or_default();
            format!("{} = {}", targets, value)
        }
        "Name" => node.get_attr("id").map(|i| i.py_to_string()).unwrap_or_default(),
        "Constant" => {
            if let Some(v) = node.get_attr("value") {
                match &v.payload {
                    PyObjectPayload::Str(s) => format!("'{}'", s),
                    PyObjectPayload::None => "None".to_string(),
                    PyObjectPayload::Bool(b) => if *b { "True" } else { "False" }.to_string(),
                    _ => v.py_to_string(),
                }
            } else { "None".to_string() }
        }
        "BinOp" => {
            let left = node.get_attr("left").map(|l| ast_unparse(&l)).unwrap_or_default();
            let right = node.get_attr("right").map(|r| ast_unparse(&r)).unwrap_or_default();
            let op = node.get_attr("op").map(|o| {
                let op_type = o.type_name().to_string();
                match op_type.as_str() {
                    "Add" => "+", "Sub" => "-", "Mult" => "*", "Div" => "/",
                    "Mod" => "%", "Pow" => "**", "FloorDiv" => "//",
                    "LShift" => "<<", "RShift" => ">>",
                    "BitOr" => "|", "BitXor" => "^", "BitAnd" => "&",
                    "MatMult" => "@",
                    _ => "?",
                }.to_string()
            }).unwrap_or_else(|| "+".to_string());
            format!("{} {} {}", left, op, right)
        }
        "Return" => {
            let val = node.get_attr("value").map(|v| ast_unparse(&v)).unwrap_or_default();
            if val.is_empty() { "return".to_string() } else { format!("return {}", val) }
        }
        "Expr" => node.get_attr("value").map(|v| ast_unparse(&v)).unwrap_or_default(),
        "Call" => {
            let func = node.get_attr("func").map(|f| ast_unparse(&f)).unwrap_or_default();
            let args_str = node.get_attr("args").map(|a| {
                if let PyObjectPayload::List(items) = &a.payload {
                    items.read().iter().map(|a| ast_unparse(a)).collect::<Vec<_>>().join(", ")
                } else { String::new() }
            }).unwrap_or_default();
            format!("{}({})", func, args_str)
        }
        "Attribute" => {
            let val = node.get_attr("value").map(|v| ast_unparse(&v)).unwrap_or_default();
            let attr = node.get_attr("attr").map(|a| a.py_to_string()).unwrap_or_default();
            format!("{}.{}", val, attr)
        }
        "ListComp" => {
            let elt = node.get_attr("elt").map(|e| ast_unparse(&e)).unwrap_or_default();
            let gens = unparse_generators(node);
            format!("[{} {}]", elt, gens)
        }
        "SetComp" => {
            let elt = node.get_attr("elt").map(|e| ast_unparse(&e)).unwrap_or_default();
            let gens = unparse_generators(node);
            format!("{{{} {}}}", elt, gens)
        }
        "DictComp" => {
            let key = node.get_attr("key").map(|k| ast_unparse(&k)).unwrap_or_default();
            let value = node.get_attr("value").map(|v| ast_unparse(&v)).unwrap_or_default();
            let gens = unparse_generators(node);
            format!("{{{}: {} {}}}", key, value, gens)
        }
        "GeneratorExp" => {
            let elt = node.get_attr("elt").map(|e| ast_unparse(&e)).unwrap_or_default();
            let gens = unparse_generators(node);
            format!("({} {})", elt, gens)
        }
        "List" => {
            let elts = unparse_list_attr(node, "elts");
            format!("[{}]", elts)
        }
        "Tuple" => {
            let elts = unparse_list_attr(node, "elts");
            if elts.contains(',') || elts.is_empty() { format!("({})", elts) }
            else { format!("({},)", elts) }
        }
        "Set" => {
            let elts = unparse_list_attr(node, "elts");
            format!("{{{}}}", elts)
        }
        "Dict" => {
            let keys = node.get_attr("keys").and_then(|k| if let PyObjectPayload::List(items) = &k.payload {
                Some(items.read().iter().map(|k| ast_unparse(k)).collect::<Vec<_>>())
            } else { None }).unwrap_or_default();
            let values = node.get_attr("values").and_then(|v| if let PyObjectPayload::List(items) = &v.payload {
                Some(items.read().iter().map(|v| ast_unparse(v)).collect::<Vec<_>>())
            } else { None }).unwrap_or_default();
            let pairs: Vec<String> = keys.iter().zip(values.iter()).map(|(k, v)| format!("{}: {}", k, v)).collect();
            format!("{{{}}}", pairs.join(", "))
        }
        "Subscript" => {
            let val = node.get_attr("value").map(|v| ast_unparse(&v)).unwrap_or_default();
            let slice = node.get_attr("slice").map(|s| ast_unparse(&s)).unwrap_or_default();
            format!("{}[{}]", val, slice)
        }
        "Index" => node.get_attr("value").map(|v| ast_unparse(&v)).unwrap_or_default(),
        "Slice" => {
            let lower = node.get_attr("lower").map(|l| ast_unparse(&l)).unwrap_or_default();
            let upper = node.get_attr("upper").map(|u| ast_unparse(&u)).unwrap_or_default();
            let step = node.get_attr("step").map(|s| ast_unparse(&s)).unwrap_or_default();
            if step.is_empty() { format!("{}:{}", lower, upper) }
            else { format!("{}:{}:{}", lower, upper, step) }
        }
        "UnaryOp" => {
            let operand = node.get_attr("operand").map(|o| ast_unparse(&o)).unwrap_or_default();
            let op = node.get_attr("op").map(|o| {
                match o.type_name().to_string().as_str() {
                    "UAdd" => "+", "USub" => "-", "Not" => "not ", "Invert" => "~",
                    _ => "?",
                }.to_string()
            }).unwrap_or_default();
            format!("{}{}", op, operand)
        }
        "BoolOp" => {
            let op = node.get_attr("op").map(|o| {
                match o.type_name().to_string().as_str() {
                    "And" => " and ", "Or" => " or ", _ => " ? ",
                }.to_string()
            }).unwrap_or_else(|| " ? ".to_string());
            let values = unparse_list_attr(node, "values");
            values.split(", ").collect::<Vec<_>>().join(&op)
        }
        "Compare" => {
            let left = node.get_attr("left").map(|l| ast_unparse(&l)).unwrap_or_default();
            let ops = node.get_attr("ops").and_then(|o| if let PyObjectPayload::List(items) = &o.payload {
                Some(items.read().iter().map(|o| {
                    match o.type_name().to_string().as_str() {
                        "Eq" => "==", "NotEq" => "!=", "Lt" => "<", "LtE" => "<=",
                        "Gt" => ">", "GtE" => ">=", "Is" => "is", "IsNot" => "is not",
                        "In" => "in", "NotIn" => "not in", _ => "?",
                    }.to_string()
                }).collect::<Vec<_>>())
            } else { None }).unwrap_or_default();
            let comparators = node.get_attr("comparators").and_then(|c| if let PyObjectPayload::List(items) = &c.payload {
                Some(items.read().iter().map(|c| ast_unparse(c)).collect::<Vec<_>>())
            } else { None }).unwrap_or_default();
            let mut s = left;
            for (op, comp) in ops.iter().zip(comparators.iter()) {
                s = format!("{} {} {}", s, op, comp);
            }
            s
        }
        "IfExp" => {
            let body = node.get_attr("body").map(|b| ast_unparse(&b)).unwrap_or_default();
            let test = node.get_attr("test").map(|t| ast_unparse(&t)).unwrap_or_default();
            let orelse = node.get_attr("orelse").map(|o| ast_unparse(&o)).unwrap_or_default();
            format!("{} if {} else {}", body, test, orelse)
        }
        "Lambda" => {
            let body = node.get_attr("body").map(|b| ast_unparse(&b)).unwrap_or_default();
            let args_node = node.get_attr("args");
            let params = args_node.map(|a| unparse_arguments(&a)).unwrap_or_default();
            format!("lambda {}: {}", params, body)
        }
        "FunctionDef" | "AsyncFunctionDef" => {
            let name = node.get_attr("name").map(|n| n.py_to_string()).unwrap_or_default();
            let args_node = node.get_attr("args");
            let params = args_node.map(|a| unparse_arguments(&a)).unwrap_or_default();
            let body = node.get_attr("body").and_then(|b| if let PyObjectPayload::List(items) = &b.payload {
                Some(items.read().iter().map(|s| format!("    {}", ast_unparse(s))).collect::<Vec<_>>().join("\n"))
            } else { None }).unwrap_or_else(|| "    pass".to_string());
            let prefix = if type_name == "AsyncFunctionDef" { "async def" } else { "def" };
            format!("{} {}({}):\n{}", prefix, name, params, body)
        }
        "ClassDef" => {
            let name = node.get_attr("name").map(|n| n.py_to_string()).unwrap_or_default();
            let bases = unparse_list_attr(node, "bases");
            let body = node.get_attr("body").and_then(|b| if let PyObjectPayload::List(items) = &b.payload {
                Some(items.read().iter().map(|s| format!("    {}", ast_unparse(s))).collect::<Vec<_>>().join("\n"))
            } else { None }).unwrap_or_else(|| "    pass".to_string());
            if bases.is_empty() { format!("class {}:\n{}", name, body) }
            else { format!("class {}({}):\n{}", name, bases, body) }
        }
        "If" => {
            let test = node.get_attr("test").map(|t| ast_unparse(&t)).unwrap_or_default();
            let body = node.get_attr("body").and_then(|b| if let PyObjectPayload::List(items) = &b.payload {
                Some(items.read().iter().map(|s| format!("    {}", ast_unparse(s))).collect::<Vec<_>>().join("\n"))
            } else { None }).unwrap_or_default();
            format!("if {}:\n{}", test, body)
        }
        "For" | "AsyncFor" => {
            let target = node.get_attr("target").map(|t| ast_unparse(&t)).unwrap_or_default();
            let iter_val = node.get_attr("iter").map(|i| ast_unparse(&i)).unwrap_or_default();
            let body = node.get_attr("body").and_then(|b| if let PyObjectPayload::List(items) = &b.payload {
                Some(items.read().iter().map(|s| format!("    {}", ast_unparse(s))).collect::<Vec<_>>().join("\n"))
            } else { None }).unwrap_or_default();
            let prefix = if type_name == "AsyncFor" { "async for" } else { "for" };
            format!("{} {} in {}:\n{}", prefix, target, iter_val, body)
        }
        "While" => {
            let test = node.get_attr("test").map(|t| ast_unparse(&t)).unwrap_or_default();
            let body = node.get_attr("body").and_then(|b| if let PyObjectPayload::List(items) = &b.payload {
                Some(items.read().iter().map(|s| format!("    {}", ast_unparse(s))).collect::<Vec<_>>().join("\n"))
            } else { None }).unwrap_or_default();
            format!("while {}:\n{}", test, body)
        }
        "Import" => {
            let names = node.get_attr("names").and_then(|n| if let PyObjectPayload::List(items) = &n.payload {
                Some(items.read().iter().map(|alias| {
                    let name = alias.get_attr("name").map(|n| n.py_to_string()).unwrap_or_default();
                    let asname = alias.get_attr("asname").map(|a| a.py_to_string());
                    if let Some(a) = asname { if a != "None" { return format!("{} as {}", name, a); } }
                    name
                }).collect::<Vec<_>>())
            } else { None }).unwrap_or_default();
            format!("import {}", names.join(", "))
        }
        "ImportFrom" => {
            let module = node.get_attr("module").map(|m| m.py_to_string()).unwrap_or_default();
            let names = node.get_attr("names").and_then(|n| if let PyObjectPayload::List(items) = &n.payload {
                Some(items.read().iter().map(|alias| {
                    let name = alias.get_attr("name").map(|n| n.py_to_string()).unwrap_or_default();
                    let asname = alias.get_attr("asname").map(|a| a.py_to_string());
                    if let Some(a) = asname { if a != "None" { return format!("{} as {}", name, a); } }
                    name
                }).collect::<Vec<_>>())
            } else { None }).unwrap_or_default();
            format!("from {} import {}", module, names.join(", "))
        }
        "Raise" => {
            let exc = node.get_attr("exc").map(|e| ast_unparse(&e));
            let cause = node.get_attr("cause").map(|c| ast_unparse(&c));
            match (exc, cause) {
                (Some(e), Some(c)) => format!("raise {} from {}", e, c),
                (Some(e), None) => format!("raise {}", e),
                _ => "raise".to_string(),
            }
        }
        "Try" => {
            let body = node.get_attr("body").and_then(|b| if let PyObjectPayload::List(items) = &b.payload {
                Some(items.read().iter().map(|s| format!("    {}", ast_unparse(s))).collect::<Vec<_>>().join("\n"))
            } else { None }).unwrap_or_default();
            format!("try:\n{}", body)
        }
        "With" | "AsyncWith" => {
            let items_str = node.get_attr("items").and_then(|it| if let PyObjectPayload::List(items) = &it.payload {
                Some(items.read().iter().map(|w| {
                    let ctx = w.get_attr("context_expr").map(|c| ast_unparse(&c)).unwrap_or_default();
                    let var = w.get_attr("optional_vars").map(|v| ast_unparse(&v));
                    if let Some(v) = var { format!("{} as {}", ctx, v) } else { ctx }
                }).collect::<Vec<_>>().join(", "))
            } else { None }).unwrap_or_default();
            let prefix = if type_name == "AsyncWith" { "async with" } else { "with" };
            format!("{} {}:", prefix, items_str)
        }
        "Pass" => "pass".to_string(),
        "Break" => "break".to_string(),
        "Continue" => "continue".to_string(),
        "Delete" => {
            let targets = unparse_list_attr(node, "targets");
            format!("del {}", targets)
        }
        "Assert" => {
            let test = node.get_attr("test").map(|t| ast_unparse(&t)).unwrap_or_default();
            let msg = node.get_attr("msg").map(|m| ast_unparse(&m));
            match msg {
                Some(m) => format!("assert {}, {}", test, m),
                None => format!("assert {}", test),
            }
        }
        "AugAssign" => {
            let target = node.get_attr("target").map(|t| ast_unparse(&t)).unwrap_or_default();
            let value = node.get_attr("value").map(|v| ast_unparse(&v)).unwrap_or_default();
            let op = node.get_attr("op").map(|o| {
                match o.type_name().to_string().as_str() {
                    "Add" => "+=", "Sub" => "-=", "Mult" => "*=", "Div" => "/=",
                    "Mod" => "%=", "Pow" => "**=", "FloorDiv" => "//=",
                    "LShift" => "<<=", "RShift" => ">>=",
                    "BitOr" => "|=", "BitXor" => "^=", "BitAnd" => "&=",
                    _ => "?=",
                }.to_string()
            }).unwrap_or_else(|| "?=".to_string());
            format!("{} {} {}", target, op, value)
        }
        "Starred" => {
            let val = node.get_attr("value").map(|v| ast_unparse(&v)).unwrap_or_default();
            format!("*{}", val)
        }
        "JoinedStr" => {
            // f-string
            let values = node.get_attr("values").and_then(|v| if let PyObjectPayload::List(items) = &v.payload {
                Some(items.read().iter().map(|v| {
                    let tn = v.type_name().to_string();
                    if tn == "FormattedValue" {
                        let inner = v.get_attr("value").map(|iv| ast_unparse(&iv)).unwrap_or_default();
                        format!("{{{}}}", inner)
                    } else {
                        ast_unparse(v)
                    }
                }).collect::<Vec<_>>())
            } else { None }).unwrap_or_default();
            format!("f'{}'", values.join(""))
        }
        "FormattedValue" => {
            let val = node.get_attr("value").map(|v| ast_unparse(&v)).unwrap_or_default();
            format!("{{{}}}", val)
        }
        "Num" => node.get_attr("n").map(|n| n.py_to_string()).unwrap_or_else(|| "0".to_string()),
        "Str" => node.get_attr("s").map(|s| format!("'{}'", s.py_to_string())).unwrap_or_else(|| "''".to_string()),
        "NameConstant" => node.get_attr("value").map(|v| v.py_to_string()).unwrap_or_else(|| "None".to_string()),
        "Yield" => {
            let val = node.get_attr("value").map(|v| ast_unparse(&v));
            match val { Some(v) => format!("yield {}", v), None => "yield".to_string() }
        }
        "YieldFrom" => {
            let val = node.get_attr("value").map(|v| ast_unparse(&v)).unwrap_or_default();
            format!("yield from {}", val)
        }
        "Await" => {
            let val = node.get_attr("value").map(|v| ast_unparse(&v)).unwrap_or_default();
            format!("await {}", val)
        }
        "Global" => {
            let names = unparse_list_attr(node, "names");
            format!("global {}", names)
        }
        "Nonlocal" => {
            let names = unparse_list_attr(node, "names");
            format!("nonlocal {}", names)
        }
        _ => format!("<{}>", type_name),
    }
}

fn unparse_generators(node: &PyObjectRef) -> String {
    node.get_attr("generators").and_then(|g| if let PyObjectPayload::List(items) = &g.payload {
        Some(items.read().iter().map(|comp| {
            let target = comp.get_attr("target").map(|t| ast_unparse(&t)).unwrap_or_default();
            let iter_val = comp.get_attr("iter").map(|i| ast_unparse(&i)).unwrap_or_default();
            let ifs = comp.get_attr("ifs").and_then(|i| if let PyObjectPayload::List(conds) = &i.payload {
                let conds: Vec<String> = conds.read().iter().map(|c| format!(" if {}", ast_unparse(c))).collect();
                Some(conds.join(""))
            } else { None }).unwrap_or_default();
            let is_async = comp.get_attr("is_async").map(|a| a.py_to_string() == "1" || a.py_to_string() == "True").unwrap_or(false);
            let prefix = if is_async { "async for" } else { "for" };
            format!("{} {} in {}{}", prefix, target, iter_val, ifs)
        }).collect::<Vec<_>>().join(" "))
    } else { None }).unwrap_or_default()
}

fn unparse_list_attr(node: &PyObjectRef, attr: &str) -> String {
    node.get_attr(attr).and_then(|a| if let PyObjectPayload::List(items) = &a.payload {
        Some(items.read().iter().map(|i| ast_unparse(i)).collect::<Vec<_>>().join(", "))
    } else { None }).unwrap_or_default()
}

fn unparse_arguments(args_node: &PyObjectRef) -> String {
    let mut parts = Vec::new();
    if let Some(args_list) = args_node.get_attr("args") {
        if let PyObjectPayload::List(items) = &args_list.payload {
            for arg in items.read().iter() {
                let name = arg.get_attr("arg").map(|a| a.py_to_string()).unwrap_or_default();
                parts.push(name);
            }
        }
    }
    parts.join(", ")
}

// ── linecache module ──

pub fn create_linecache_module() -> PyObjectRef {
    let getline_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 {
            return Err(PyException::type_error("linecache.getline requires filename and lineno"));
        }
        let filename = args[0].py_to_string();
        let lineno = match &args[1].payload {
            PyObjectPayload::Int(n) => n.to_i64().unwrap_or(0) as usize,
            _ => 0,
        };
        // Try to read the file and get the line
        match std::fs::read_to_string(&filename) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                if lineno > 0 && lineno <= lines.len() {
                    Ok(PyObject::str_val(CompactString::from(format!("{}\n", lines[lineno - 1]))))
                } else {
                    Ok(PyObject::str_val(CompactString::from("")))
                }
            }
            Err(_) => Ok(PyObject::str_val(CompactString::from(""))),
        }
    });

    let getlines_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("linecache.getlines requires filename"));
        }
        let filename = args[0].py_to_string();
        match std::fs::read_to_string(&filename) {
            Ok(content) => {
                let lines: Vec<PyObjectRef> = content.lines()
                    .map(|l| PyObject::str_val(CompactString::from(format!("{}\n", l))))
                    .collect();
                Ok(PyObject::list(lines))
            }
            Err(_) => Ok(PyObject::list(vec![])),
        }
    });

    let clearcache_fn = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::none())
    });

    let checkcache_fn = make_builtin(|_args: &[PyObjectRef]| {
        Ok(PyObject::none())
    });

    make_module("linecache", vec![
        ("getline", getline_fn),
        ("getlines", getlines_fn),
        ("clearcache", clearcache_fn),
        ("checkcache", checkcache_fn),
        ("cache", PyObject::dict(IndexMap::new())),
    ])
}

// ── token module ──

pub fn create_token_module() -> PyObjectRef {
    make_module("token", vec![
        ("ENDMARKER", PyObject::int(0)),
        ("NAME", PyObject::int(1)),
        ("NUMBER", PyObject::int(2)),
        ("STRING", PyObject::int(3)),
        ("NEWLINE", PyObject::int(4)),
        ("INDENT", PyObject::int(5)),
        ("DEDENT", PyObject::int(6)),
        ("LPAR", PyObject::int(7)),
        ("RPAR", PyObject::int(8)),
        ("LSQB", PyObject::int(9)),
        ("RSQB", PyObject::int(10)),
        ("COLON", PyObject::int(11)),
        ("COMMA", PyObject::int(12)),
        ("SEMI", PyObject::int(13)),
        ("PLUS", PyObject::int(14)),
        ("MINUS", PyObject::int(15)),
        ("STAR", PyObject::int(16)),
        ("SLASH", PyObject::int(17)),
        ("VBAR", PyObject::int(18)),
        ("AMPER", PyObject::int(19)),
        ("LESS", PyObject::int(20)),
        ("GREATER", PyObject::int(21)),
        ("EQUAL", PyObject::int(22)),
        ("DOT", PyObject::int(23)),
        ("PERCENT", PyObject::int(24)),
        ("LBRACE", PyObject::int(25)),
        ("RBRACE", PyObject::int(26)),
        ("EQEQUAL", PyObject::int(27)),
        ("NOTEQUAL", PyObject::int(28)),
        ("LESSEQUAL", PyObject::int(29)),
        ("GREATEREQUAL", PyObject::int(30)),
        ("TILDE", PyObject::int(31)),
        ("CIRCUMFLEX", PyObject::int(32)),
        ("LEFTSHIFT", PyObject::int(33)),
        ("RIGHTSHIFT", PyObject::int(34)),
        ("DOUBLESTAR", PyObject::int(35)),
        ("PLUSEQUAL", PyObject::int(36)),
        ("MINEQUAL", PyObject::int(37)),
        ("STAREQUAL", PyObject::int(38)),
        ("SLASHEQUAL", PyObject::int(39)),
        ("PERCENTEQUAL", PyObject::int(40)),
        ("AMPEREQUAL", PyObject::int(41)),
        ("VBAREQUAL", PyObject::int(42)),
        ("CIRCUMFLEXEQUAL", PyObject::int(43)),
        ("LEFTSHIFTEQUAL", PyObject::int(44)),
        ("RIGHTSHIFTEQUAL", PyObject::int(45)),
        ("DOUBLESTAREQUAL", PyObject::int(46)),
        ("DOUBLESLASH", PyObject::int(47)),
        ("DOUBLESLASHEQUAL", PyObject::int(48)),
        ("AT", PyObject::int(49)),
        ("ATEQUAL", PyObject::int(50)),
        ("RARROW", PyObject::int(51)),
        ("ELLIPSIS", PyObject::int(52)),
        ("COLONEQUAL", PyObject::int(53)),
        ("OP", PyObject::int(54)),
        ("COMMENT", PyObject::int(55)),
        ("NL", PyObject::int(56)),
        ("ERRORTOKEN", PyObject::int(57)),
        ("ENCODING", PyObject::int(62)),
        ("NT_OFFSET", PyObject::int(256)),
        ("tok_name", {
            let mut map = IndexMap::new();
            for (i, name) in [(0,"ENDMARKER"),(1,"NAME"),(2,"NUMBER"),(3,"STRING"),(4,"NEWLINE"),
                (5,"INDENT"),(6,"DEDENT"),(54,"OP"),(55,"COMMENT"),(56,"NL"),(57,"ERRORTOKEN"),(62,"ENCODING")].iter() {
                map.insert(ferrython_core::types::HashableKey::Int(ferrython_core::types::PyInt::Small(*i)), 
                    PyObject::str_val(CompactString::from(*name)));
            }
            PyObject::dict(map)
        }),
    ])
}

/// Helper: create a simple namespace-like instance with the given attrs.
fn make_ns(cls_name: &str, attrs: IndexMap<CompactString, PyObjectRef>) -> PyObjectRef {
    use std::sync::Arc;
    use parking_lot::RwLock;
    let cls = PyObject::class(CompactString::from(cls_name), vec![], IndexMap::new());
    PyObject::wrap(PyObjectPayload::Instance(InstanceData {
        class: cls,
        attrs: Arc::new(RwLock::new(attrs)),
        is_special: true, dict_storage: None,
    }))
}

// ── tokenize module ──

pub fn create_tokenize_module() -> PyObjectRef {
    fn tokenize_string(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        check_args("generate_tokens", args, 1)?;
        // args[0] should be a readline callable; collect all lines first
        let source = if let PyObjectPayload::NativeClosure(nc) = &args[0].payload {
            let mut lines = String::new();
            loop {
                let line = (nc.func)(&[])?;
                let s = line.py_to_string();
                if s.is_empty() { break; }
                lines.push_str(&s);
            }
            lines
        } else {
            // Fallback: treat as string source
            args[0].py_to_string()
        };
        let mut tokens = Vec::new();
        let mut indent_stack: Vec<usize> = vec![0];

        for (lineno, line) in source.lines().enumerate() {
            let lineno = lineno + 1;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                tokens.push(make_token_info(61, "", (lineno, 0), (lineno, line.len()), line));
                continue;
            }
            if trimmed.starts_with('#') {
                tokens.push(make_token_info(60, trimmed, (lineno, 0), (lineno, line.len()), line));
                continue;
            }
            // Count leading indent
            let indent = line.len() - line.trim_start().len();
            let prev = *indent_stack.last().unwrap_or(&0);
            if indent > prev {
                indent_stack.push(indent);
                tokens.push(make_token_info(5, "", (lineno, 0), (lineno, indent), line)); // INDENT
            } else {
                while indent < *indent_stack.last().unwrap_or(&0) {
                    indent_stack.pop();
                    tokens.push(make_token_info(6, "", (lineno, 0), (lineno, 0), line)); // DEDENT
                }
            }
            let mut col = 0;
            let chars: Vec<char> = line.chars().collect();
            while col < chars.len() {
                if chars[col].is_whitespace() { col += 1; continue; }
                if chars[col] == '#' {
                    // Inline comment — consume rest of line
                    let comment: String = chars[col..].iter().collect();
                    tokens.push(make_token_info(60, &comment, (lineno, col), (lineno, chars.len()), line));
                    let _ = chars.len(); // col unused
                    break;
                }
                let start_col = col;
                if chars[col].is_alphabetic() || chars[col] == '_' {
                    while col < chars.len() && (chars[col].is_alphanumeric() || chars[col] == '_') { col += 1; }
                    let word: String = chars[start_col..col].iter().collect();
                    tokens.push(make_token_info(1, &word, (lineno, start_col), (lineno, col), line));
                } else if chars[col].is_ascii_digit() {
                    while col < chars.len() && (chars[col].is_ascii_digit() || chars[col] == '.' || chars[col] == 'e' || chars[col] == 'E' || chars[col] == '_') { col += 1; }
                    let num: String = chars[start_col..col].iter().collect();
                    tokens.push(make_token_info(2, &num, (lineno, start_col), (lineno, col), line));
                } else if chars[col] == '"' || chars[col] == '\'' {
                    let quote = chars[col];
                    col += 1;
                    while col < chars.len() && chars[col] != quote { col += 1; }
                    if col < chars.len() { col += 1; }
                    let s: String = chars[start_col..col].iter().collect();
                    tokens.push(make_token_info(3, &s, (lineno, start_col), (lineno, col), line));
                } else {
                    // Multi-character operators
                    let mut end = col + 1;
                    if end < chars.len() {
                        let two: String = chars[col..end+1].iter().collect();
                        if matches!(two.as_str(), "==" | "!=" | "<=" | ">=" | "**" | "//" | "<<" | ">>" | "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" | "->" | ":=") {
                            end += 1;
                            // Check for 3-char ops
                            if end < chars.len() {
                                let three: String = chars[col..end+1].iter().collect();
                                if matches!(three.as_str(), "**=" | "//=" | "<<=" | ">>=") {
                                    end += 1;
                                }
                            }
                        }
                    }
                    let op: String = chars[col..end].iter().collect();
                    col = end;
                    tokens.push(make_token_info(54, &op, (lineno, start_col), (lineno, col), line));
                }
            }
            tokens.push(make_token_info(4, "\n", (lineno, line.len()), (lineno, line.len()+1), line));
        }
        // Emit remaining DEDENT tokens
        while indent_stack.len() > 1 {
            indent_stack.pop();
            tokens.push(make_token_info(6, "", (0, 0), (0, 0), ""));
        }
        tokens.push(make_token_info(0, "", (0, 0), (0, 0), ""));
        Ok(PyObject::list(tokens))
    }

    fn make_token_info(type_id: i64, string: &str, start: (usize, usize), end: (usize, usize), line: &str) -> PyObjectRef {
        PyObject::tuple(vec![
            PyObject::int(type_id),
            PyObject::str_val(CompactString::from(string)),
            PyObject::tuple(vec![PyObject::int(start.0 as i64), PyObject::int(start.1 as i64)]),
            PyObject::tuple(vec![PyObject::int(end.0 as i64), PyObject::int(end.1 as i64)]),
            PyObject::str_val(CompactString::from(line)),
        ])
    }

    fn tokenize_open(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        check_args("open", args, 1)?;
        let filename = args[0].py_to_string();
        let content = std::fs::read_to_string(filename.as_str())
            .map_err(|e| PyException::os_error(format!("{}", e)))?;
        Ok(PyObject::str_val(CompactString::from(content)))
    }

    fn detect_encoding(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        Ok(PyObject::tuple(vec![
            PyObject::str_val(CompactString::from("utf-8")),
            PyObject::list(vec![]),
        ]))
    }

    // Build tok_name mapping (same as token.tok_name)
    let tok_name_entries: Vec<(i64, &str)> = vec![
        (0, "ENDMARKER"), (1, "NAME"), (2, "NUMBER"), (3, "STRING"),
        (4, "NEWLINE"), (5, "INDENT"), (6, "DEDENT"),
        (54, "OP"), (59, "ERRORTOKEN"), (60, "COMMENT"),
        (61, "NL"), (62, "ENCODING"),
    ];
    let mut tok_name_map = IndexMap::new();
    for (id, name) in &tok_name_entries {
        tok_name_map.insert(HashableKey::Int(ferrython_core::types::PyInt::Small(*id)), PyObject::str_val(CompactString::from(*name)));
    }
    let tok_name = PyObject::dict(tok_name_map);

    // TokenInfo namedtuple-like class
    let mut ti_ns = IndexMap::new();
    ti_ns.insert(CompactString::from("_fields"), PyObject::tuple(vec![
        PyObject::str_val(CompactString::from("type")),
        PyObject::str_val(CompactString::from("string")),
        PyObject::str_val(CompactString::from("start")),
        PyObject::str_val(CompactString::from("end")),
        PyObject::str_val(CompactString::from("line")),
    ]));
    let token_info_cls = PyObject::class(CompactString::from("TokenInfo"), vec![], ti_ns);

    make_module("tokenize", vec![
        ("generate_tokens", make_builtin(tokenize_string)),
        ("tokenize", make_builtin(tokenize_string)),
        ("open", make_builtin(tokenize_open)),
        ("detect_encoding", make_builtin(detect_encoding)),
        ("tok_name", tok_name),
        ("TokenInfo", token_info_cls),
        ("ENDMARKER", PyObject::int(0)),
        ("NAME", PyObject::int(1)),
        ("NUMBER", PyObject::int(2)),
        ("STRING", PyObject::int(3)),
        ("NEWLINE", PyObject::int(4)),
        ("INDENT", PyObject::int(5)),
        ("DEDENT", PyObject::int(6)),
        ("OP", PyObject::int(54)),
        ("COMMENT", PyObject::int(60)),
        ("NL", PyObject::int(61)),
        ("ENCODING", PyObject::int(62)),
        ("ERRORTOKEN", PyObject::int(59)),
        ("EXACT_TOKEN_TYPES", PyObject::dict(IndexMap::new())),
    ])
}

// ── symtable module ──

pub fn create_symtable_module() -> PyObjectRef {
    fn symtable_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        check_args_min("symtable", args, 2)?;
        let source = args[0].py_to_string();
        let filename = args[1].py_to_string();
        let kind = if args.len() > 2 { args[2].py_to_string() } else { "exec".to_string() };

        let mut symbols: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
        let mut locals: Vec<String> = Vec::new();
        let mut globals: Vec<String> = Vec::new();

        for line in source.lines() {
            let trimmed = line.trim();
            if let Some(pos) = trimmed.find('=') {
                if pos > 0 && !trimmed[..pos].contains('(') && !trimmed[..pos].contains('[') {
                    let name = trimmed[..pos].trim();
                    if name.chars().all(|c| c.is_alphanumeric() || c == '_') && !name.is_empty() {
                        if !locals.contains(&name.to_string()) {
                            locals.push(name.to_string());
                        }
                    }
                }
            }
            if trimmed.starts_with("global ") {
                for name in trimmed[7..].split(',') {
                    let name = name.trim().to_string();
                    if !globals.contains(&name) { globals.push(name); }
                }
            }
            if trimmed.starts_with("def ") || trimmed.starts_with("class ") {
                let rest = if trimmed.starts_with("def ") { &trimmed[4..] } else { &trimmed[6..] };
                if let Some(name_end) = rest.find(|c: char| !c.is_alphanumeric() && c != '_') {
                    let name = rest[..name_end].to_string();
                    if !locals.contains(&name) { locals.push(name); }
                }
            }
        }

        for name in &locals {
            let mut ns = IndexMap::new();
            ns.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(name.as_str())));
            ns.insert(CompactString::from("is_local"), PyObject::bool_val(true));
            ns.insert(CompactString::from("is_global"), PyObject::bool_val(globals.contains(name)));
            ns.insert(CompactString::from("is_referenced"), PyObject::bool_val(true));
            ns.insert(CompactString::from("is_imported"), PyObject::bool_val(false));
            ns.insert(CompactString::from("is_parameter"), PyObject::bool_val(false));
            ns.insert(CompactString::from("is_free"), PyObject::bool_val(false));
            ns.insert(CompactString::from("is_assigned"), PyObject::bool_val(true));
            ns.insert(CompactString::from("is_namespace"), PyObject::bool_val(false));
            symbols.insert(CompactString::from(name.as_str()), make_ns("Symbol", ns));
        }

        let mut st_ns = IndexMap::new();
        st_ns.insert(CompactString::from("name"), PyObject::str_val(CompactString::from("top")));
        st_ns.insert(CompactString::from("type"), PyObject::str_val(CompactString::from(kind.as_str())));
        st_ns.insert(CompactString::from("filename"), PyObject::str_val(CompactString::from(filename.as_str())));
        st_ns.insert(CompactString::from("get_symbols"),
            PyObject::native_closure("SymbolTable.get_symbols", {
                let syms = symbols.clone();
                move |_: &[PyObjectRef]| {
                    Ok(PyObject::list(syms.values().cloned().collect()))
                }
            })
        );
        st_ns.insert(CompactString::from("lookup"),
            PyObject::native_closure("SymbolTable.lookup", {
                let syms = symbols.clone();
                move |args: &[PyObjectRef]| {
                    check_args("lookup", args, 1)?;
                    let name = args[0].py_to_string();
                    match syms.get(name.as_str()) {
                        Some(sym) => Ok(sym.clone()),
                        None => Err(PyException::key_error(format!("symbol '{}' not found", name))),
                    }
                }
            })
        );
        st_ns.insert(CompactString::from("get_identifiers"),
            PyObject::native_closure("SymbolTable.get_identifiers", {
                let syms = symbols;
                move |_: &[PyObjectRef]| {
                    Ok(PyObject::list(syms.keys().map(|k| PyObject::str_val(k.clone())).collect()))
                }
            })
        );
        st_ns.insert(CompactString::from("has_children"), PyObject::bool_val(false));
        st_ns.insert(CompactString::from("get_children"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::list(vec![]))),
        );
        Ok(make_ns("SymbolTable", st_ns))
    }

    fn symbol_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        check_args("Symbol", args, 1)?;
        let name = args[0].py_to_string();
        let mut ns = IndexMap::new();
        ns.insert(CompactString::from("name"), PyObject::str_val(CompactString::from(name.as_str())));
        ns.insert(CompactString::from("is_local"), PyObject::bool_val(false));
        ns.insert(CompactString::from("is_global"), PyObject::bool_val(false));
        ns.insert(CompactString::from("is_referenced"), PyObject::bool_val(false));
        ns.insert(CompactString::from("is_imported"), PyObject::bool_val(false));
        ns.insert(CompactString::from("is_parameter"), PyObject::bool_val(false));
        ns.insert(CompactString::from("is_free"), PyObject::bool_val(false));
        ns.insert(CompactString::from("is_assigned"), PyObject::bool_val(false));
        ns.insert(CompactString::from("is_namespace"), PyObject::bool_val(false));
        Ok(make_ns("Symbol", ns))
    }

    make_module("symtable", vec![
        ("symtable", make_builtin(symtable_fn)),
        ("Symbol", make_builtin(symbol_fn)),
        ("DEF_GLOBAL", PyObject::int(1)),
        ("DEF_LOCAL", PyObject::int(2)),
        ("DEF_PARAM", PyObject::int(4)),
        ("DEF_IMPORT", PyObject::int(8)),
        ("DEF_FREE", PyObject::int(16)),
        ("DEF_FREE_CLASS", PyObject::int(32)),
        ("DEF_BOUND", PyObject::int(64)),
    ])
}

// ── PyObject AST → Rust AST converter ──────────────────────────────────
// Converts Python AST objects (from `ast.parse()` or programmatic construction)
// directly into ferrython_ast types, bypassing source code roundtrip.
// This is necessary because werkzeug and other libs use invalid-identifier
// names (e.g. `<builder:...>`, `.self`) that cannot survive unparse→reparse.

use ferrython_ast::{
    Module as AstModule, Statement, StatementKind, Expression as AstExpression,
    ExpressionKind, Constant as AstConstant, BigInt as AstBigInt,
    Operator, BoolOperator, UnaryOperator, CompareOperator, ExprContext,
    Arguments as AstArguments, Arg as AstArg, Keyword as AstKeyword,
    Alias as AstAlias, WithItem as AstWithItem, ExceptHandler as AstExceptHandler,
    Comprehension as AstComprehension, SourceLocation,
};

/// Convert a PyObject AST Module into a ferrython_ast Module for compilation.
pub fn pyobj_ast_to_module(node: &PyObjectRef) -> Result<AstModule, String> {
    let type_name = node.type_name().to_string();
    match type_name.as_str() {
        "Module" => {
            let body = convert_stmt_list(node, "body")?;
            Ok(AstModule::Module {
                body,
                type_ignores: Vec::new(),
            })
        }
        "Expression" => {
            let body_expr = node.get_attr("body")
                .ok_or_else(|| "Expression node missing 'body'".to_string())?;
            let expr = convert_expr(&body_expr)?;
            Ok(AstModule::Expression {
                body: Box::new(expr),
            })
        }
        "Interactive" => {
            let body = convert_stmt_list(node, "body")?;
            Ok(AstModule::Interactive { body })
        }
        _ => Err(format!("Expected Module/Expression/Interactive, got {}", type_name)),
    }
}

fn loc_from_node(node: &PyObjectRef) -> SourceLocation {
    let line = node.get_attr("lineno")
        .and_then(|v| v.to_int().map(|i| i as u32).ok())
        .unwrap_or(1);
    let col = node.get_attr("col_offset")
        .and_then(|v| v.to_int().map(|i| i as u32).ok())
        .unwrap_or(0);
    let end_line = node.get_attr("end_lineno")
        .and_then(|v| v.to_int().map(|i| i as u32).ok());
    let end_col = node.get_attr("end_col_offset")
        .and_then(|v| v.to_int().map(|i| i as u32).ok());
    let mut loc = SourceLocation::new(line, col);
    if let (Some(el), Some(ec)) = (end_line, end_col) {
        loc = loc.with_end(el, ec);
    }
    loc
}

fn get_list_attr(node: &PyObjectRef, attr: &str) -> Vec<PyObjectRef> {
    node.get_attr(attr).map(|v| {
        if let PyObjectPayload::List(items) = &v.payload {
            items.read().clone()
        } else if matches!(&v.payload, PyObjectPayload::None) {
            Vec::new()
        } else {
            vec![v]
        }
    }).unwrap_or_default()
}

fn get_str_attr(node: &PyObjectRef, attr: &str) -> CompactString {
    node.get_attr(attr)
        .map(|v| CompactString::from(v.py_to_string()))
        .unwrap_or_default()
}

fn get_optional_str(node: &PyObjectRef, attr: &str) -> Option<CompactString> {
    node.get_attr(attr).and_then(|v| {
        if matches!(&v.payload, PyObjectPayload::None) {
            None
        } else {
            Some(CompactString::from(v.py_to_string()))
        }
    })
}

fn convert_stmt_list(parent: &PyObjectRef, attr: &str) -> Result<Vec<Statement>, String> {
    let items = get_list_attr(parent, attr);
    items.iter().map(convert_stmt).collect()
}

fn convert_expr_list(parent: &PyObjectRef, attr: &str) -> Result<Vec<AstExpression>, String> {
    let items = get_list_attr(parent, attr);
    items.iter().map(convert_expr).collect()
}

fn convert_optional_expr(parent: &PyObjectRef, attr: &str) -> Result<Option<Box<AstExpression>>, String> {
    match parent.get_attr(attr) {
        Some(v) if !matches!(&v.payload, PyObjectPayload::None) => {
            Ok(Some(Box::new(convert_expr(&v)?)))
        }
        _ => Ok(None),
    }
}

fn convert_stmt(node: &PyObjectRef) -> Result<Statement, String> {
    let type_name = node.type_name().to_string();
    let location = loc_from_node(node);
    let kind = match type_name.as_str() {
        "FunctionDef" | "AsyncFunctionDef" => {
            let name = get_str_attr(node, "name");
            let args = node.get_attr("args")
                .map(|a| convert_arguments(&a))
                .unwrap_or_else(|| Ok(AstArguments::empty()))?;
            let body = convert_stmt_list(node, "body")?;
            let decorator_list = convert_expr_list(node, "decorator_list")?;
            let returns = convert_optional_expr(node, "returns")?;
            StatementKind::FunctionDef {
                name,
                args: Box::new(args),
                body,
                decorator_list,
                returns,
                type_comment: None,
                is_async: type_name == "AsyncFunctionDef",
            }
        }
        "ClassDef" => {
            let name = get_str_attr(node, "name");
            let bases = convert_expr_list(node, "bases")?;
            let keywords = convert_keyword_list(node, "keywords")?;
            let body = convert_stmt_list(node, "body")?;
            let decorator_list = convert_expr_list(node, "decorator_list")?;
            StatementKind::ClassDef { name, bases, keywords, body, decorator_list }
        }
        "Return" => {
            let value = convert_optional_expr(node, "value")?;
            StatementKind::Return { value }
        }
        "Delete" => {
            let targets = convert_expr_list(node, "targets")?;
            StatementKind::Delete { targets }
        }
        "Assign" => {
            let targets = convert_expr_list(node, "targets")?;
            let value_node = node.get_attr("value")
                .ok_or_else(|| "Assign missing 'value'".to_string())?;
            let value = Box::new(convert_expr(&value_node)?);
            StatementKind::Assign { targets, value, type_comment: None }
        }
        "AugAssign" => {
            let target_node = node.get_attr("target")
                .ok_or_else(|| "AugAssign missing 'target'".to_string())?;
            let target = Box::new(convert_expr(&target_node)?);
            let op = node.get_attr("op")
                .map(|o| convert_operator(&o))
                .unwrap_or(Operator::Add);
            let value_node = node.get_attr("value")
                .ok_or_else(|| "AugAssign missing 'value'".to_string())?;
            let value = Box::new(convert_expr(&value_node)?);
            StatementKind::AugAssign { target, op, value }
        }
        "AnnAssign" => {
            let target_node = node.get_attr("target")
                .ok_or_else(|| "AnnAssign missing 'target'".to_string())?;
            let target = Box::new(convert_expr(&target_node)?);
            let ann_node = node.get_attr("annotation")
                .ok_or_else(|| "AnnAssign missing 'annotation'".to_string())?;
            let annotation = Box::new(convert_expr(&ann_node)?);
            let value = convert_optional_expr(node, "value")?;
            let simple = node.get_attr("simple")
                .and_then(|v| v.to_int().ok())
                .map(|i| i != 0)
                .unwrap_or(true);
            StatementKind::AnnAssign { target, annotation, value, simple }
        }
        "For" | "AsyncFor" => {
            let target_node = node.get_attr("target")
                .ok_or_else(|| "For missing 'target'".to_string())?;
            let target = Box::new(convert_expr(&target_node)?);
            let iter_node = node.get_attr("iter")
                .ok_or_else(|| "For missing 'iter'".to_string())?;
            let iter_expr = Box::new(convert_expr(&iter_node)?);
            let body = convert_stmt_list(node, "body")?;
            let orelse = convert_stmt_list(node, "orelse")?;
            StatementKind::For {
                target, iter: iter_expr, body, orelse,
                type_comment: None,
                is_async: type_name == "AsyncFor",
            }
        }
        "While" => {
            let test_node = node.get_attr("test")
                .ok_or_else(|| "While missing 'test'".to_string())?;
            let test = Box::new(convert_expr(&test_node)?);
            let body = convert_stmt_list(node, "body")?;
            let orelse = convert_stmt_list(node, "orelse")?;
            StatementKind::While { test, body, orelse }
        }
        "If" => {
            let test_node = node.get_attr("test")
                .ok_or_else(|| "If missing 'test'".to_string())?;
            let test = Box::new(convert_expr(&test_node)?);
            let body = convert_stmt_list(node, "body")?;
            let orelse = convert_stmt_list(node, "orelse")?;
            StatementKind::If { test, body, orelse }
        }
        "With" | "AsyncWith" => {
            let items = get_list_attr(node, "items");
            let with_items: Vec<AstWithItem> = items.iter().map(|item| {
                let ctx_node = item.get_attr("context_expr")
                    .ok_or_else(|| "WithItem missing 'context_expr'".to_string())?;
                let context_expr = convert_expr(&ctx_node)?;
                let optional_vars = convert_optional_expr(item, "optional_vars")?;
                Ok(AstWithItem { context_expr, optional_vars })
            }).collect::<Result<_, String>>()?;
            let body = convert_stmt_list(node, "body")?;
            StatementKind::With {
                items: with_items,
                body,
                type_comment: None,
                is_async: type_name == "AsyncWith",
            }
        }
        "Raise" => {
            let exc = convert_optional_expr(node, "exc")?;
            let cause = convert_optional_expr(node, "cause")?;
            StatementKind::Raise { exc, cause }
        }
        "Try" | "TryStar" => {
            let body = convert_stmt_list(node, "body")?;
            let handlers = get_list_attr(node, "handlers");
            let except_handlers: Vec<AstExceptHandler> = handlers.iter().map(|h| {
                let typ = convert_optional_expr(h, "type")?;
                let name = get_optional_str(h, "name");
                let handler_body = convert_stmt_list(h, "body")?;
                Ok(AstExceptHandler {
                    typ, name, body: handler_body,
                    location: loc_from_node(h),
                    is_star: type_name == "TryStar",
                })
            }).collect::<Result<_, String>>()?;
            let orelse = convert_stmt_list(node, "orelse")?;
            let finalbody = convert_stmt_list(node, "finalbody")?;
            StatementKind::Try { body, handlers: except_handlers, orelse, finalbody }
        }
        "Assert" => {
            let test_node = node.get_attr("test")
                .ok_or_else(|| "Assert missing 'test'".to_string())?;
            let test = Box::new(convert_expr(&test_node)?);
            let msg = convert_optional_expr(node, "msg")?;
            StatementKind::Assert { test, msg }
        }
        "Import" => {
            let names = convert_alias_list(node, "names")?;
            StatementKind::Import { names }
        }
        "ImportFrom" => {
            let module = get_optional_str(node, "module");
            let names = convert_alias_list(node, "names")?;
            let level = node.get_attr("level")
                .and_then(|v| v.to_int().ok())
                .unwrap_or(0) as u32;
            StatementKind::ImportFrom { module, names, level }
        }
        "Global" => {
            let names = get_list_attr(node, "names")
                .iter().map(|n| CompactString::from(n.py_to_string())).collect();
            StatementKind::Global { names }
        }
        "Nonlocal" => {
            let names = get_list_attr(node, "names")
                .iter().map(|n| CompactString::from(n.py_to_string())).collect();
            StatementKind::Nonlocal { names }
        }
        "Expr" => {
            let value_node = node.get_attr("value")
                .ok_or_else(|| "Expr missing 'value'".to_string())?;
            let value = Box::new(convert_expr(&value_node)?);
            StatementKind::Expr { value }
        }
        "Pass" => StatementKind::Pass,
        "Break" => StatementKind::Break,
        "Continue" => StatementKind::Continue,
        _ => return Err(format!("Unknown statement type: {}", type_name)),
    };
    Ok(Statement { node: kind, location })
}

fn convert_expr(node: &PyObjectRef) -> Result<AstExpression, String> {
    let type_name = node.type_name().to_string();
    let location = loc_from_node(node);
    let kind = match type_name.as_str() {
        "BoolOp" => {
            let op = node.get_attr("op")
                .map(|o| convert_bool_op(&o))
                .unwrap_or(BoolOperator::And);
            let values = convert_expr_list(node, "values")?;
            ExpressionKind::BoolOp { op, values }
        }
        "NamedExpr" => {
            let target_node = node.get_attr("target")
                .ok_or_else(|| "NamedExpr missing 'target'".to_string())?;
            let value_node = node.get_attr("value")
                .ok_or_else(|| "NamedExpr missing 'value'".to_string())?;
            ExpressionKind::NamedExpr {
                target: Box::new(convert_expr(&target_node)?),
                value: Box::new(convert_expr(&value_node)?),
            }
        }
        "BinOp" => {
            let left_node = node.get_attr("left")
                .ok_or_else(|| "BinOp missing 'left'".to_string())?;
            let right_node = node.get_attr("right")
                .ok_or_else(|| "BinOp missing 'right'".to_string())?;
            let op = node.get_attr("op")
                .map(|o| convert_operator(&o))
                .unwrap_or(Operator::Add);
            ExpressionKind::BinOp {
                left: Box::new(convert_expr(&left_node)?),
                op,
                right: Box::new(convert_expr(&right_node)?),
            }
        }
        "UnaryOp" => {
            let op = node.get_attr("op")
                .map(|o| convert_unary_op(&o))
                .unwrap_or(UnaryOperator::UAdd);
            let operand_node = node.get_attr("operand")
                .ok_or_else(|| "UnaryOp missing 'operand'".to_string())?;
            ExpressionKind::UnaryOp {
                op,
                operand: Box::new(convert_expr(&operand_node)?),
            }
        }
        "Lambda" => {
            let args = node.get_attr("args")
                .map(|a| convert_arguments(&a))
                .unwrap_or_else(|| Ok(AstArguments::empty()))?;
            let body_node = node.get_attr("body")
                .ok_or_else(|| "Lambda missing 'body'".to_string())?;
            ExpressionKind::Lambda {
                args: Box::new(args),
                body: Box::new(convert_expr(&body_node)?),
            }
        }
        "IfExp" => {
            let test = node.get_attr("test")
                .ok_or_else(|| "IfExp missing 'test'".to_string())?;
            let body = node.get_attr("body")
                .ok_or_else(|| "IfExp missing 'body'".to_string())?;
            let orelse = node.get_attr("orelse")
                .ok_or_else(|| "IfExp missing 'orelse'".to_string())?;
            ExpressionKind::IfExp {
                test: Box::new(convert_expr(&test)?),
                body: Box::new(convert_expr(&body)?),
                orelse: Box::new(convert_expr(&orelse)?),
            }
        }
        "Dict" => {
            let keys_raw = get_list_attr(node, "keys");
            let values_raw = get_list_attr(node, "values");
            let keys: Vec<Option<AstExpression>> = keys_raw.iter().map(|k| {
                if matches!(&k.payload, PyObjectPayload::None) {
                    Ok(None)
                } else {
                    convert_expr(k).map(Some)
                }
            }).collect::<Result<_, String>>()?;
            let values: Vec<AstExpression> = values_raw.iter()
                .map(|v| convert_expr(v)).collect::<Result<_, String>>()?;
            ExpressionKind::Dict { keys, values }
        }
        "Set" => {
            let elts = convert_expr_list(node, "elts")?;
            ExpressionKind::Set { elts }
        }
        "ListComp" => {
            let elt_node = node.get_attr("elt")
                .ok_or_else(|| "ListComp missing 'elt'".to_string())?;
            let generators = convert_comprehension_list(node)?;
            ExpressionKind::ListComp {
                elt: Box::new(convert_expr(&elt_node)?),
                generators,
            }
        }
        "SetComp" => {
            let elt_node = node.get_attr("elt")
                .ok_or_else(|| "SetComp missing 'elt'".to_string())?;
            let generators = convert_comprehension_list(node)?;
            ExpressionKind::SetComp {
                elt: Box::new(convert_expr(&elt_node)?),
                generators,
            }
        }
        "DictComp" => {
            let key_node = node.get_attr("key")
                .ok_or_else(|| "DictComp missing 'key'".to_string())?;
            let value_node = node.get_attr("value")
                .ok_or_else(|| "DictComp missing 'value'".to_string())?;
            let generators = convert_comprehension_list(node)?;
            ExpressionKind::DictComp {
                key: Box::new(convert_expr(&key_node)?),
                value: Box::new(convert_expr(&value_node)?),
                generators,
            }
        }
        "GeneratorExp" => {
            let elt_node = node.get_attr("elt")
                .ok_or_else(|| "GeneratorExp missing 'elt'".to_string())?;
            let generators = convert_comprehension_list(node)?;
            ExpressionKind::GeneratorExp {
                elt: Box::new(convert_expr(&elt_node)?),
                generators,
            }
        }
        "Await" => {
            let value_node = node.get_attr("value")
                .ok_or_else(|| "Await missing 'value'".to_string())?;
            ExpressionKind::Await {
                value: Box::new(convert_expr(&value_node)?),
            }
        }
        "Yield" => {
            let value = convert_optional_expr(node, "value")?;
            ExpressionKind::Yield { value: value.map(|b| *b).map(Box::new) }
        }
        "YieldFrom" => {
            let value_node = node.get_attr("value")
                .ok_or_else(|| "YieldFrom missing 'value'".to_string())?;
            ExpressionKind::YieldFrom {
                value: Box::new(convert_expr(&value_node)?),
            }
        }
        "Compare" => {
            let left_node = node.get_attr("left")
                .ok_or_else(|| "Compare missing 'left'".to_string())?;
            let ops_raw = get_list_attr(node, "ops");
            let ops: Vec<CompareOperator> = ops_raw.iter()
                .map(|o| convert_compare_op(o)).collect();
            let comparators = convert_expr_list(node, "comparators")?;
            ExpressionKind::Compare {
                left: Box::new(convert_expr(&left_node)?),
                ops,
                comparators,
            }
        }
        "Call" => {
            let func_node = node.get_attr("func")
                .ok_or_else(|| "Call missing 'func'".to_string())?;
            let args = convert_expr_list(node, "args")?;
            let keywords = convert_keyword_list(node, "keywords")?;
            ExpressionKind::Call {
                func: Box::new(convert_expr(&func_node)?),
                args,
                keywords,
            }
        }
        "FormattedValue" => {
            let value_node = node.get_attr("value")
                .ok_or_else(|| "FormattedValue missing 'value'".to_string())?;
            let conversion = node.get_attr("conversion")
                .and_then(|v| v.to_int().ok())
                .and_then(|c| if c < 0 { None } else { char::from_u32(c as u32) });
            let format_spec = convert_optional_expr(node, "format_spec")?;
            ExpressionKind::FormattedValue {
                value: Box::new(convert_expr(&value_node)?),
                conversion,
                format_spec,
            }
        }
        "JoinedStr" => {
            let values = convert_expr_list(node, "values")?;
            ExpressionKind::JoinedStr { values }
        }
        "Constant" => {
            let value = node.get_attr("value")
                .map(|v| convert_constant(&v))
                .unwrap_or(AstConstant::None);
            ExpressionKind::Constant { value }
        }
        "Attribute" => {
            let value_node = node.get_attr("value")
                .ok_or_else(|| "Attribute missing 'value'".to_string())?;
            let attr = get_str_attr(node, "attr");
            let ctx = node.get_attr("ctx")
                .map(|c| convert_expr_context(&c))
                .unwrap_or(ExprContext::Load);
            ExpressionKind::Attribute {
                value: Box::new(convert_expr(&value_node)?),
                attr,
                ctx,
            }
        }
        "Subscript" => {
            let value_node = node.get_attr("value")
                .ok_or_else(|| "Subscript missing 'value'".to_string())?;
            let slice_node = node.get_attr("slice")
                .ok_or_else(|| "Subscript missing 'slice'".to_string())?;
            let ctx = node.get_attr("ctx")
                .map(|c| convert_expr_context(&c))
                .unwrap_or(ExprContext::Load);
            ExpressionKind::Subscript {
                value: Box::new(convert_expr(&value_node)?),
                slice: Box::new(convert_expr(&slice_node)?),
                ctx,
            }
        }
        "Starred" => {
            let value_node = node.get_attr("value")
                .ok_or_else(|| "Starred missing 'value'".to_string())?;
            let ctx = node.get_attr("ctx")
                .map(|c| convert_expr_context(&c))
                .unwrap_or(ExprContext::Load);
            ExpressionKind::Starred {
                value: Box::new(convert_expr(&value_node)?),
                ctx,
            }
        }
        "Name" => {
            let id = get_str_attr(node, "id");
            let ctx = node.get_attr("ctx")
                .map(|c| convert_expr_context(&c))
                .unwrap_or(ExprContext::Load);
            ExpressionKind::Name { id, ctx }
        }
        "List" => {
            let elts = convert_expr_list(node, "elts")?;
            let ctx = node.get_attr("ctx")
                .map(|c| convert_expr_context(&c))
                .unwrap_or(ExprContext::Load);
            ExpressionKind::List { elts, ctx }
        }
        "Tuple" => {
            let elts = convert_expr_list(node, "elts")?;
            let ctx = node.get_attr("ctx")
                .map(|c| convert_expr_context(&c))
                .unwrap_or(ExprContext::Load);
            ExpressionKind::Tuple { elts, ctx }
        }
        "Slice" => {
            let lower = convert_optional_expr(node, "lower")?;
            let upper = convert_optional_expr(node, "upper")?;
            let step = convert_optional_expr(node, "step")?;
            ExpressionKind::Slice { lower, upper, step }
        }
        _ => {
            return Err(format!("Unknown expression type: {}", type_name));
        }
    };
    Ok(AstExpression { node: kind, location })
}

fn convert_constant(val: &PyObjectRef) -> AstConstant {
    use ferrython_core::types::PyInt;
    match &val.payload {
        PyObjectPayload::None => AstConstant::None,
        PyObjectPayload::Bool(b) => AstConstant::Bool(*b),
        PyObjectPayload::Int(pi) => match pi {
            PyInt::Small(i) => AstConstant::Int(AstBigInt::Small(*i)),
            PyInt::Big(bi) => AstConstant::Int(AstBigInt::Big(bi.clone())),
        },
        PyObjectPayload::Float(f) => AstConstant::Float(*f),
        PyObjectPayload::Str(s) => AstConstant::Str(CompactString::from(s.as_str())),
        PyObjectPayload::Bytes(b) => AstConstant::Bytes(b.clone()),
        _ => {
            let s = val.py_to_string();
            if let Ok(i) = s.parse::<i64>() {
                AstConstant::Int(AstBigInt::Small(i))
            } else if let Ok(f) = s.parse::<f64>() {
                AstConstant::Float(f)
            } else {
                AstConstant::Str(CompactString::from(s))
            }
        }
    }
}

fn convert_operator(node: &PyObjectRef) -> Operator {
    match node.type_name().to_string().as_str() {
        "Add" => Operator::Add,
        "Sub" => Operator::Sub,
        "Mult" => Operator::Mult,
        "Div" => Operator::Div,
        "Mod" => Operator::Mod,
        "Pow" => Operator::Pow,
        "LShift" => Operator::LShift,
        "RShift" => Operator::RShift,
        "BitOr" => Operator::BitOr,
        "BitXor" => Operator::BitXor,
        "BitAnd" => Operator::BitAnd,
        "FloorDiv" => Operator::FloorDiv,
        "MatMult" => Operator::MatMult,
        _ => Operator::Add,
    }
}

fn convert_bool_op(node: &PyObjectRef) -> BoolOperator {
    match node.type_name().to_string().as_str() {
        "And" => BoolOperator::And,
        "Or" => BoolOperator::Or,
        _ => BoolOperator::And,
    }
}

fn convert_unary_op(node: &PyObjectRef) -> UnaryOperator {
    match node.type_name().to_string().as_str() {
        "Invert" => UnaryOperator::Invert,
        "Not" => UnaryOperator::Not,
        "UAdd" => UnaryOperator::UAdd,
        "USub" => UnaryOperator::USub,
        _ => UnaryOperator::UAdd,
    }
}

fn convert_compare_op(node: &PyObjectRef) -> CompareOperator {
    match node.type_name().to_string().as_str() {
        "Eq" => CompareOperator::Eq,
        "NotEq" => CompareOperator::NotEq,
        "Lt" => CompareOperator::Lt,
        "LtE" => CompareOperator::LtE,
        "Gt" => CompareOperator::Gt,
        "GtE" => CompareOperator::GtE,
        "Is" => CompareOperator::Is,
        "IsNot" => CompareOperator::IsNot,
        "In" => CompareOperator::In,
        "NotIn" => CompareOperator::NotIn,
        _ => CompareOperator::Eq,
    }
}

fn convert_expr_context(node: &PyObjectRef) -> ExprContext {
    match node.type_name().to_string().as_str() {
        "Store" => ExprContext::Store,
        "Del" => ExprContext::Del,
        _ => ExprContext::Load,
    }
}

fn convert_arguments(node: &PyObjectRef) -> Result<AstArguments, String> {
    let posonlyargs = convert_arg_list(node, "posonlyargs")?;
    let args = convert_arg_list(node, "args")?;
    let vararg = node.get_attr("vararg").and_then(|v| {
        if matches!(&v.payload, PyObjectPayload::None) { None }
        else { Some(convert_arg(&v)) }
    }).transpose()?;
    let kwonlyargs = convert_arg_list(node, "kwonlyargs")?;
    let kw_defaults = get_list_attr(node, "kw_defaults").iter().map(|d| {
        if matches!(&d.payload, PyObjectPayload::None) {
            Ok(None)
        } else {
            convert_expr(d).map(Some)
        }
    }).collect::<Result<Vec<_>, String>>()?;
    let kwarg = node.get_attr("kwarg").and_then(|v| {
        if matches!(&v.payload, PyObjectPayload::None) { None }
        else { Some(convert_arg(&v)) }
    }).transpose()?;
    let defaults = convert_expr_list(node, "defaults")?;
    Ok(AstArguments { posonlyargs, args, vararg, kwonlyargs, kw_defaults, kwarg, defaults })
}

fn convert_arg_list(parent: &PyObjectRef, attr: &str) -> Result<Vec<AstArg>, String> {
    get_list_attr(parent, attr).iter().map(convert_arg).collect()
}

fn convert_arg(node: &PyObjectRef) -> Result<AstArg, String> {
    let arg = get_str_attr(node, "arg");
    let annotation = convert_optional_expr(node, "annotation")?;
    Ok(AstArg {
        arg,
        annotation,
        type_comment: None,
        location: loc_from_node(node),
    })
}

fn convert_keyword_list(parent: &PyObjectRef, attr: &str) -> Result<Vec<AstKeyword>, String> {
    get_list_attr(parent, attr).iter().map(|k| {
        let arg = get_optional_str(k, "arg");
        let value_node = k.get_attr("value")
            .ok_or_else(|| "keyword missing 'value'".to_string())?;
        Ok(AstKeyword {
            arg,
            value: convert_expr(&value_node)?,
            location: loc_from_node(k),
        })
    }).collect()
}

fn convert_alias_list(parent: &PyObjectRef, attr: &str) -> Result<Vec<AstAlias>, String> {
    get_list_attr(parent, attr).iter().map(|a| {
        let name = get_str_attr(a, "name");
        let asname = get_optional_str(a, "asname");
        Ok(AstAlias { name, asname, location: loc_from_node(a) })
    }).collect()
}

fn convert_comprehension_list(parent: &PyObjectRef) -> Result<Vec<AstComprehension>, String> {
    get_list_attr(parent, "generators").iter().map(|g| {
        let target_node = g.get_attr("target")
            .ok_or_else(|| "comprehension missing 'target'".to_string())?;
        let iter_node = g.get_attr("iter")
            .ok_or_else(|| "comprehension missing 'iter'".to_string())?;
        let ifs = convert_expr_list(g, "ifs")?;
        let is_async = g.get_attr("is_async")
            .and_then(|v| v.to_int().ok())
            .map(|i| i != 0)
            .unwrap_or(false);
        Ok(AstComprehension {
            target: convert_expr(&target_node)?,
            iter: convert_expr(&iter_node)?,
            ifs,
            is_async,
        })
    }).collect()
}
