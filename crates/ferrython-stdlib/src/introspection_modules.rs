//! Introspection stdlib modules (warnings, traceback, inspect, dis)

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectPayload, PyObjectRef, PyObjectMethods,
    make_module, make_builtin, check_args, check_args_min,
    InstanceData,
};
use ferrython_core::types::HashableKey;
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
    // Track "once" warnings: set of (message, category, module)
    static ONCE_SEEN: std::sync::LazyLock<RwLock<std::collections::HashSet<String>>> =
        std::sync::LazyLock::new(|| RwLock::new(std::collections::HashSet::new()));

    // Global recording state: when catch_warnings(record=True) is active,
    // warn() appends to this list instead of printing to stderr.
    static RECORDING: AtomicBool = AtomicBool::new(false);
    static RECORD_LIST: std::sync::LazyLock<RwLock<Option<PyObjectRef>>> =
        std::sync::LazyLock::new(|| RwLock::new(None));

    fn match_filter(action: &str, msg: &str, cat: &str, module: &str, filter: &FilterEntry) -> bool {
        let (_, msg_pat, cat_pat, mod_pat, lineno) = filter;
        if !msg_pat.is_empty() && !msg.contains(msg_pat.as_str()) { return false; }
        if !cat_pat.is_empty() && cat_pat != "Warning" && cat != cat_pat { return false; }
        if !mod_pat.is_empty() && !module.contains(mod_pat.as_str()) { return false; }
        if *lineno != 0 { return false; } // lineno filtering not supported
        let _ = action;
        true
    }

    // warn(message, category=UserWarning, stacklevel=1)
    let warn_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() { return Ok(PyObject::none()); }
        let message = args[0].py_to_string();
        // category can be positional arg[1] or in kwargs dict
        let category = get_kwarg(args, "category")
            .map(|cat| {
                if let PyObjectPayload::Class(cd) = &cat.payload { cd.name.to_string() }
                else { cat.py_to_string() }
            })
            .unwrap_or_else(|| {
                // Check positional arg[1] if it's not a dict
                if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::Dict(_) | PyObjectPayload::None) {
                    let cat = &args[1];
                    if let PyObjectPayload::Class(cd) = &cat.payload { cd.name.to_string() }
                    else { cat.py_to_string() }
                } else {
                    "UserWarning".to_string()
                }
            });

        // Check filters
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
                if seen.contains(&key) {
                    return Ok(PyObject::none());
                }
                seen.insert(key);
            }
            _ => {} // "default", "always", "module" — show the warning
        }

        if RECORDING.load(Ordering::Relaxed) {
            let guard = RECORD_LIST.read();
            if let Some(ref list_obj) = *guard {
                let cls = PyObject::class(CompactString::from("WarningMessage"), vec![], IndexMap::new());
                let mut attrs = IndexMap::new();
                attrs.insert(CompactString::from("message"), args[0].clone());
                attrs.insert(CompactString::from("category"), PyObject::str_val(CompactString::from(&category)));
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

    // Helper to extract kwarg from a kwargs dict
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
            .and_then(|v| v.as_int().map(|i| i))
            .unwrap_or(0);
        let append = get_kwarg(args, "append").map(|v| v.is_truthy()).unwrap_or(false);

        // Also handle positional args (after the action, skipping dict kwargs)
        let non_dict_args: Vec<_> = args[1..].iter()
            .filter(|a| !matches!(a.payload, PyObjectPayload::Dict(_)))
            .collect();
        let message = if !message.is_empty() { message }
            else if !non_dict_args.is_empty() { non_dict_args[0].py_to_string() }
            else { String::new() };

        let entry = (action, message, category, module, lineno);
        let mut filters = FILTERS.write();
        if append {
            filters.push(entry);
        } else {
            filters.insert(0, entry);
        }
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
                // Positional: first non-dict after action
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
            .and_then(|v| v.as_int().map(|i| i))
            .unwrap_or(0);
        let append = get_kwarg(args, "append").map(|v| v.is_truthy()).unwrap_or(false);

        let entry = (action, String::new(), category, String::new(), lineno);
        let mut filters = FILTERS.write();
        if append {
            filters.push(entry);
        } else {
            filters.insert(0, entry);
        }
        Ok(PyObject::none())
    });

    // resetwarnings()
    let reset_fn = make_builtin(|_args: &[PyObjectRef]| {
        FILTERS.write().clear();
        FILTERS.write().push(("default".into(), "".into(), "Warning".into(), "".into(), 0));
        ONCE_SEEN.write().clear();
        Ok(PyObject::none())
    });

    // Build the initial filters list as a Python list for the `filters` attribute
    let filters_list = PyObject::list(vec![]);

    // catch_warnings(record=False)
    let catch_warnings_fn = make_builtin(|args: &[PyObjectRef]| {
        let record = if !args.is_empty() { args[0].is_truthy() } else { false };

        let cls = PyObject::class(CompactString::from("catch_warnings"), vec![], IndexMap::new());
        let mut attrs = IndexMap::new();
        let warning_list = PyObject::list(vec![]);
        attrs.insert(CompactString::from("_record"), PyObject::bool_val(record));
        attrs.insert(CompactString::from("_warnings"), warning_list.clone());

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
                "catch_warnings.__enter__", |args: &[PyObjectRef]| {
                    if args.is_empty() { return Ok(PyObject::none()); }
                    Ok(args[0].clone())
                }
            ));
        }

        attrs.insert(CompactString::from("__exit__"), PyObject::native_function(
            "catch_warnings.__exit__", |_args: &[PyObjectRef]| {
                RECORDING.store(false, Ordering::Relaxed);
                *RECORD_LIST.write() = None;
                Ok(PyObject::bool_val(false))
            }
        ));

        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    make_module("warnings", vec![
        ("warn", warn_fn),
        ("filterwarnings", filter_warnings_fn),
        ("simplefilter", simple_filter_fn),
        ("resetwarnings", reset_fn),
        ("catch_warnings", catch_warnings_fn),
        ("filters", filters_list),
    ])
}

// ── decimal module (stub) ──


pub fn create_traceback_module() -> PyObjectRef {
    // Delegate to the dedicated ferrython-traceback crate
    ferrython_traceback::create_traceback_module()
}

// ── warnings module (stub) ──


pub fn create_inspect_module() -> PyObjectRef {
    make_module("inspect", vec![
        ("isfunction", make_builtin(|args| {
            check_args("inspect.isfunction", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Function(_))))
        })),
        ("isclass", make_builtin(|args| {
            check_args("inspect.isclass", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload,
                PyObjectPayload::Class(_) | PyObjectPayload::BuiltinType(_)
            )))
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
                Ok(PyObject::bool_val(f.code.flags.contains(ferrython_bytecode::code::CodeFlags::GENERATOR)))
            } else {
                Ok(PyObject::bool_val(false))
            }
        })),
        ("iscoroutine", make_builtin(|args| {
            check_args("inspect.iscoroutine", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload, PyObjectPayload::Coroutine(_))))
        })),
        ("iscoroutinefunction", make_builtin(|args| {
            check_args("inspect.iscoroutinefunction", args, 1)?;
            if let PyObjectPayload::Function(f) = &args[0].payload {
                Ok(PyObject::bool_val(f.code.flags.contains(ferrython_bytecode::code::CodeFlags::COROUTINE)))
            } else {
                Ok(PyObject::bool_val(false))
            }
        })),
        ("isroutine", make_builtin(|args| {
            check_args("inspect.isroutine", args, 1)?;
            Ok(PyObject::bool_val(matches!(&args[0].payload,
                PyObjectPayload::Function(_) | PyObjectPayload::BoundMethod { .. } |
                PyObjectPayload::NativeFunction { .. } | PyObjectPayload::NativeClosure { .. } |
                PyObjectPayload::BuiltinBoundMethod { .. } | PyObjectPayload::BuiltinFunction(_))))
        })),
        ("isabstract", make_builtin(|args| {
            check_args("inspect.isabstract", args, 1)?;
            Ok(PyObject::bool_val(args[0].get_attr("__abstractmethods__").is_some()))
        })),
        ("getmembers", make_builtin(|args| {
            check_args_min("inspect.getmembers", args, 1)?;
            let dir_names = args[0].dir();
            let mut result = Vec::new();
            for n in &dir_names {
                if let Some(val) = args[0].get_attr(n.as_str()) {
                    // If predicate provided (args[1]), filter
                    if args.len() > 1 {
                        // Can't call VM functions from native — skip predicate filter
                    }
                    result.push(PyObject::tuple(vec![PyObject::str_val(n.clone()), val]));
                }
            }
            Ok(PyObject::list(result))
        })),
        ("getdoc", make_builtin(|args| {
            check_args("inspect.getdoc", args, 1)?;
            Ok(args[0].get_attr("__doc__").unwrap_or_else(PyObject::none))
        })),
        ("getfile", make_builtin(|args| {
            check_args("inspect.getfile", args, 1)?;
            if let PyObjectPayload::Function(f) = &args[0].payload {
                return Ok(PyObject::str_val(f.code.filename.clone()));
            }
            if let PyObjectPayload::Module(m) = &args[0].payload {
                if let Some(file) = m.attrs.read().get("__file__").cloned() {
                    return Ok(file);
                }
            }
            Err(PyException::type_error("could not get file for object"))
        })),
        ("getmodule", make_builtin(|args| {
            check_args("inspect.getmodule", args, 1)?;
            Ok(args[0].get_attr("__module__").unwrap_or_else(PyObject::none))
        })),
        ("signature", make_builtin(|args| {
            check_args("inspect.signature", args, 1)?;
            // Build a Signature object with .parameters (OrderedDict of Parameter objects)
            let sig_cls = PyObject::class(CompactString::from("Signature"), vec![], IndexMap::new());
            let param_cls = PyObject::class(CompactString::from("Parameter"), vec![], IndexMap::new());

            let mut params_map = IndexMap::new();

            if let PyObjectPayload::Function(f) = &args[0].payload {
                let ac = f.code.arg_count as usize;
                let kwc = f.code.kwonlyarg_count as usize;
                let total = ac + kwc;
                let n_defaults = f.defaults.len();

                for (i, name) in f.code.varnames.iter().take(total).enumerate() {
                    let p = PyObject::instance(param_cls.clone());
                    if let PyObjectPayload::Instance(ref inst) = p.payload {
                        let mut w = inst.attrs.write();
                        w.insert(CompactString::from("name"), PyObject::str_val(name.clone()));
                        // Determine kind
                        let kind = if i < ac { 1 } else { 3 }; // POSITIONAL_OR_KEYWORD=1, KEYWORD_ONLY=3
                        w.insert(CompactString::from("kind"), PyObject::int(kind));
                        // Default
                        if i < ac && i >= ac - n_defaults {
                            w.insert(CompactString::from("default"), f.defaults[i - (ac - n_defaults)].clone());
                        } else if i >= ac {
                            let kw_name = name.clone();
                            if let Some(kw_def) = f.kw_defaults.get(&kw_name) {
                                w.insert(CompactString::from("default"), kw_def.clone());
                            } else {
                                w.insert(CompactString::from("default"), PyObject::instance(
                                    PyObject::class(CompactString::from("_empty"), vec![], IndexMap::new())
                                ));
                            }
                        } else {
                            w.insert(CompactString::from("default"), PyObject::instance(
                                PyObject::class(CompactString::from("_empty"), vec![], IndexMap::new())
                            ));
                        }
                        // Annotation
                        if let Some(ann) = f.annotations.get(name) {
                            w.insert(CompactString::from("annotation"), ann.clone());
                        }
                    }
                    params_map.insert(
                        HashableKey::Str(name.clone()),
                        p,
                    );
                }

                // *args
                if f.code.flags.contains(ferrython_bytecode::code::CodeFlags::VARARGS) && total < f.code.varnames.len() {
                    let name = &f.code.varnames[total];
                    let p = PyObject::instance(param_cls.clone());
                    if let PyObjectPayload::Instance(ref inst) = p.payload {
                        let mut w = inst.attrs.write();
                        w.insert(CompactString::from("name"), PyObject::str_val(name.clone()));
                        w.insert(CompactString::from("kind"), PyObject::int(2)); // VAR_POSITIONAL
                    }
                    params_map.insert(HashableKey::Str(name.clone()), p);
                }

                // **kwargs
                if f.code.flags.contains(ferrython_bytecode::code::CodeFlags::VARKEYWORDS) {
                    let kw_idx = total + if f.code.flags.contains(ferrython_bytecode::code::CodeFlags::VARARGS) { 1 } else { 0 };
                    if kw_idx < f.code.varnames.len() {
                        let name = &f.code.varnames[kw_idx];
                        let p = PyObject::instance(param_cls.clone());
                        if let PyObjectPayload::Instance(ref inst) = p.payload {
                            let mut w = inst.attrs.write();
                            w.insert(CompactString::from("name"), PyObject::str_val(name.clone()));
                            w.insert(CompactString::from("kind"), PyObject::int(4)); // VAR_KEYWORD
                        }
                        params_map.insert(HashableKey::Str(name.clone()), p);
                    }
                }
            }

            let sig = PyObject::instance(sig_cls);
            if let PyObjectPayload::Instance(ref inst) = sig.payload {
                let mut w = inst.attrs.write();
                let params = PyObject::dict(params_map.clone());
                w.insert(CompactString::from("parameters"), params);

                // __contains__ — check if parameter name is in signature
                let keys: Vec<String> = params_map.keys()
                    .filter_map(|k| if let HashableKey::Str(s) = k { Some(s.to_string()) } else { None })
                    .collect();
                let keys2 = keys.clone();
                w.insert(CompactString::from("__contains__"), PyObject::native_closure("__contains__", move |args| {
                    if args.is_empty() { return Ok(PyObject::bool_val(false)); }
                    let needle = args[0].py_to_string();
                    Ok(PyObject::bool_val(keys.iter().any(|k| k == &needle)))
                }));

                // __str__ / __repr__ — "(a, b, *args, **kwargs)" format
                let sig_str = {
                    let mut parts = Vec::new();
                    for k in &keys2 {
                        if let Some(p) = params_map.get(&HashableKey::Str(CompactString::from(k.as_str()))) {
                            if let PyObjectPayload::Instance(ref pinst) = p.payload {
                                let attrs = pinst.attrs.read();
                                let kind = attrs.get("kind").and_then(|v| v.as_int()).unwrap_or(1);
                                match kind {
                                    2 => parts.push(format!("*{}", k)),
                                    4 => parts.push(format!("**{}", k)),
                                    _ => {
                                        if let Some(default) = attrs.get("default") {
                                            if let PyObjectPayload::Instance(ref di) = default.payload {
                                                if let PyObjectPayload::Class(ref dc) = di.class.payload {
                                                    if dc.name.as_str() == "_empty" {
                                                        parts.push(k.to_string());
                                                        continue;
                                                    }
                                                }
                                            }
                                            parts.push(format!("{}={}", k, default.repr()));
                                        } else {
                                            parts.push(k.to_string());
                                        }
                                    }
                                }
                            } else {
                                parts.push(k.to_string());
                            }
                        }
                    }
                    format!("({})", parts.join(", "))
                };
                let sig_str2 = sig_str.clone();
                w.insert(CompactString::from("__str__"), PyObject::native_closure("__str__", move |_args| {
                    Ok(PyObject::str_val(CompactString::from(&sig_str)))
                }));
                w.insert(CompactString::from("__repr__"), PyObject::native_closure("__repr__", move |_args| {
                    Ok(PyObject::str_val(CompactString::from(format!("<Signature {}>", sig_str2))))
                }));
            }
            Ok(sig)
        })),
        ("getfullargspec", make_builtin(|args| {
            check_args("inspect.getfullargspec", args, 1)?;
            if let PyObjectPayload::Function(f) = &args[0].payload {
                let ac = f.code.arg_count as usize;
                let kwc = f.code.kwonlyarg_count as usize;
                let arg_names: Vec<PyObjectRef> = f.code.varnames.iter()
                    .take(ac)
                    .map(|v| PyObject::str_val(v.clone()))
                    .collect();
                let kwonly_names: Vec<PyObjectRef> = f.code.varnames.iter()
                    .skip(ac)
                    .take(kwc)
                    .map(|v| PyObject::str_val(v.clone()))
                    .collect();
                // Return a FullArgSpec-like namedtuple as dict for simplicity
                let mut map = IndexMap::new();
                map.insert(HashableKey::Str(CompactString::from("args")), PyObject::list(arg_names));
                map.insert(HashableKey::Str(CompactString::from("varargs")), PyObject::none());
                map.insert(HashableKey::Str(CompactString::from("varkw")), PyObject::none());
                map.insert(HashableKey::Str(CompactString::from("defaults")),
                    if f.defaults.is_empty() { PyObject::none() } else { PyObject::tuple(f.defaults.clone()) });
                map.insert(HashableKey::Str(CompactString::from("kwonlyargs")), PyObject::list(kwonly_names));
                map.insert(HashableKey::Str(CompactString::from("kwonlydefaults")),
                    if f.kw_defaults.is_empty() { PyObject::none() }
                    else {
                        let mut kw_map = IndexMap::new();
                        for (k, v) in &f.kw_defaults {
                            kw_map.insert(HashableKey::Str(k.clone()), v.clone());
                        }
                        PyObject::dict(kw_map)
                    });
                map.insert(HashableKey::Str(CompactString::from("annotations")), PyObject::dict(IndexMap::new()));
                Ok(PyObject::dict(map))
            } else {
                Err(PyException::type_error("unsupported callable"))
            }
        })),
        // Parameter and Signature classes (simplified placeholders for compatibility)
        ("Parameter", PyObject::class(CompactString::from("Parameter"), vec![], IndexMap::new())),
        ("Signature", PyObject::class(CompactString::from("Signature"), vec![], IndexMap::new())),
        ("getsource", make_builtin(|args| {
            check_args("inspect.getsource", args, 1)?;
            let filename = match &args[0].payload {
                PyObjectPayload::Function(f) => f.code.filename.clone(),
                PyObjectPayload::Module(m) => {
                    if let Some(f) = m.attrs.read().get("__file__") {
                        CompactString::from(f.py_to_string())
                    } else {
                        return Err(PyException::runtime_error("could not find source"));
                    }
                }
                _ => return Err(PyException::runtime_error("could not find source")),
            };
            match std::fs::read_to_string(filename.as_str()) {
                Ok(src) => {
                    // For functions, extract from first_line_number
                    if let PyObjectPayload::Function(f) = &args[0].payload {
                        let lines: Vec<&str> = src.lines().collect();
                        let start = (f.code.first_line_number as usize).saturating_sub(1);
                        if start < lines.len() {
                            // Find the end of the function by indentation
                            let indent = lines[start].len() - lines[start].trim_start().len();
                            let mut end = start + 1;
                            while end < lines.len() {
                                let line = lines[end];
                                if line.trim().is_empty() { end += 1; continue; }
                                let line_indent = line.len() - line.trim_start().len();
                                if line_indent <= indent && !line.trim().is_empty() { break; }
                                end += 1;
                            }
                            let func_src = lines[start..end].join("\n");
                            return Ok(PyObject::str_val(CompactString::from(func_src)));
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
                        let lines: Vec<PyObjectRef> = src.lines()
                            .skip(lineno.saturating_sub(1) as usize)
                            .take_while(|l| !l.is_empty() || l.trim().is_empty())
                            .map(|l| PyObject::str_val(CompactString::from(format!("{}\n", l))))
                            .collect();
                        Ok(PyObject::tuple(vec![PyObject::list(lines), PyObject::int(lineno as i64)]))
                    }
                    Err(_) => Err(PyException::runtime_error("could not read source")),
                }
            } else {
                Err(PyException::runtime_error("could not find source lines"))
            }
        })),
        ("currentframe", make_builtin(|_| Ok(PyObject::none()))),
        ("stack", make_builtin(|_| Ok(PyObject::list(vec![])))),
        ("getmro", make_builtin(|args| {
            check_args("inspect.getmro", args, 1)?;
            if let PyObjectPayload::Class(cd) = &args[0].payload {
                let mut mro = vec![args[0].clone()];
                mro.extend(cd.mro.iter().cloned());
                Ok(PyObject::tuple(mro))
            } else {
                Err(PyException::type_error("argument is not a class"))
            }
        })),
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
            _ => return Err(PyException::type_error(
                format!("don't know how to disassemble {} objects", obj.type_name())
            )),
        };
        disassemble_code(&code, 0);
        Ok(PyObject::none())
    }

    fn disassemble_code(code: &ferrython_bytecode::CodeObject, indent: usize) {
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
            println!("{}{} {:>6} {:<24} {}", pad, line_str, i * 2, format!("{:?}", instr.op), arg_desc);
        }

        // Recurse into nested code objects
        for c in &code.constants {
            if let ConstantValue::Code(nested) = c {
                println!();
                println!("{}Disassembly of <code object {} at ...>:", pad, nested.name);
                disassemble_code(nested, indent + 2);
            }
        }
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

    make_module("dis", vec![
        ("dis", make_builtin(dis_dis)),
        ("disassemble", make_builtin(dis_dis)),
    ])
}

// ── ast module ──

pub fn create_ast_module() -> PyObjectRef {
    // Basic AST module — provides parse() and dump() for introspection
    let parse_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("ast.parse() requires source code argument"));
        }
        let _source = args[0].py_to_string();
        // Create a Module AST node (simplified)
        let cls = PyObject::class(CompactString::from("Module"), vec![], IndexMap::new());
        let inst = PyObject::instance(cls);
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("body"), PyObject::list(vec![]));
            w.insert(CompactString::from("type_ignores"), PyObject::list(vec![]));
            w.insert(CompactString::from("_fields"), PyObject::tuple(vec![
                PyObject::str_val(CompactString::from("body")),
                PyObject::str_val(CompactString::from("type_ignores")),
            ]));
        }
        Ok(inst)
    });

    let dump_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("ast.dump() requires node argument"));
        }
        // Simple dump — show the type name and fields
        let type_name = args[0].type_name();
        Ok(PyObject::str_val(CompactString::from(format!("{}()", type_name))))
    });

    let literal_eval_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("ast.literal_eval() requires string argument"));
        }
        let s = args[0].py_to_string();
        let trimmed = s.trim();
        // Handle basic literals
        if trimmed == "None" { return Ok(PyObject::none()); }
        if trimmed == "True" { return Ok(PyObject::bool_val(true)); }
        if trimmed == "False" { return Ok(PyObject::bool_val(false)); }
        if let Ok(n) = trimmed.parse::<i64>() { return Ok(PyObject::int(n)); }
        if let Ok(f) = trimmed.parse::<f64>() { return Ok(PyObject::float(f)); }
        if (trimmed.starts_with('"') && trimmed.ends_with('"')) || (trimmed.starts_with('\'') && trimmed.ends_with('\'')) {
            return Ok(PyObject::str_val(CompactString::from(&trimmed[1..trimmed.len()-1])));
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            // Simple list literal parsing
            let inner = &trimmed[1..trimmed.len()-1];
            if inner.trim().is_empty() {
                return Ok(PyObject::list(vec![]));
            }
            let items: Vec<PyObjectRef> = inner.split(',')
                .map(|s| {
                    let s = s.trim();
                    if let Ok(n) = s.parse::<i64>() { PyObject::int(n) }
                    else if let Ok(f) = s.parse::<f64>() { PyObject::float(f) }
                    else if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
                        PyObject::str_val(CompactString::from(&s[1..s.len()-1]))
                    } else {
                        PyObject::str_val(CompactString::from(s))
                    }
                }).collect();
            return Ok(PyObject::list(items));
        }
        if trimmed.starts_with('(') && trimmed.ends_with(')') {
            let inner = &trimmed[1..trimmed.len()-1];
            if inner.trim().is_empty() {
                return Ok(PyObject::tuple(vec![]));
            }
            let items: Vec<PyObjectRef> = inner.split(',')
                .filter(|s| !s.trim().is_empty())
                .map(|s| {
                    let s = s.trim();
                    if let Ok(n) = s.parse::<i64>() { PyObject::int(n) }
                    else if let Ok(f) = s.parse::<f64>() { PyObject::float(f) }
                    else { PyObject::str_val(CompactString::from(s)) }
                }).collect();
            return Ok(PyObject::tuple(items));
        }
        if trimmed.starts_with('{') && trimmed.ends_with('}') {
            let inner = &trimmed[1..trimmed.len()-1];
            if inner.trim().is_empty() {
                return Ok(PyObject::dict(IndexMap::new()));
            }
            // Basic dict literal — only handles simple key:value pairs
            let mut map = IndexMap::new();
            for pair in inner.split(',') {
                if let Some((k, v)) = pair.split_once(':') {
                    let k = k.trim().trim_matches(|c| c == '\'' || c == '"');
                    let v = v.trim();
                    let val = if let Ok(n) = v.parse::<i64>() { PyObject::int(n) }
                    else if let Ok(f) = v.parse::<f64>() { PyObject::float(f) }
                    else { PyObject::str_val(CompactString::from(v.trim_matches(|c| c == '\'' || c == '"'))) };
                    map.insert(ferrython_core::types::HashableKey::Str(CompactString::from(k)), val);
                }
            }
            return Ok(PyObject::dict(map));
        }
        Err(PyException::value_error(format!("malformed node or string: {}", trimmed)))
    });

    // AST node type constructors (stubs)
    let make_node_type = |name: &str| -> PyObjectRef {
        let n = name.to_string();
        PyObject::native_closure(&format!("ast.{}", n), move |_args: &[PyObjectRef]| {
            let cls = PyObject::class(CompactString::from(&n), vec![], IndexMap::new());
            Ok(PyObject::instance(cls))
        })
    };

    make_module("ast", vec![
        ("parse", parse_fn),
        ("dump", dump_fn),
        ("literal_eval", literal_eval_fn),
        // Node types
        ("Module", make_node_type("Module")),
        ("Expression", make_node_type("Expression")),
        ("Interactive", make_node_type("Interactive")),
        ("FunctionDef", make_node_type("FunctionDef")),
        ("AsyncFunctionDef", make_node_type("AsyncFunctionDef")),
        ("ClassDef", make_node_type("ClassDef")),
        ("Return", make_node_type("Return")),
        ("Assign", make_node_type("Assign")),
        ("AugAssign", make_node_type("AugAssign")),
        ("AnnAssign", make_node_type("AnnAssign")),
        ("For", make_node_type("For")),
        ("While", make_node_type("While")),
        ("If", make_node_type("If")),
        ("With", make_node_type("With")),
        ("Raise", make_node_type("Raise")),
        ("Try", make_node_type("Try")),
        ("Import", make_node_type("Import")),
        ("ImportFrom", make_node_type("ImportFrom")),
        ("Expr", make_node_type("Expr")),
        ("Name", make_node_type("Name")),
        ("Constant", make_node_type("Constant")),
        ("BinOp", make_node_type("BinOp")),
        ("UnaryOp", make_node_type("UnaryOp")),
        ("BoolOp", make_node_type("BoolOp")),
        ("Compare", make_node_type("Compare")),
        ("Call", make_node_type("Call")),
        ("Attribute", make_node_type("Attribute")),
        ("Subscript", make_node_type("Subscript")),
        ("Starred", make_node_type("Starred")),
        ("List", make_node_type("List")),
        ("Tuple", make_node_type("Tuple")),
        ("Dict", make_node_type("Dict")),
        ("Set", make_node_type("Set")),
        ("Lambda", make_node_type("Lambda")),
        ("IfExp", make_node_type("IfExp")),
        ("ListComp", make_node_type("ListComp")),
        ("SetComp", make_node_type("SetComp")),
        ("DictComp", make_node_type("DictComp")),
        ("GeneratorExp", make_node_type("GeneratorExp")),
        ("Yield", make_node_type("Yield")),
        ("YieldFrom", make_node_type("YieldFrom")),
        ("Await", make_node_type("Await")),
        ("Pass", make_node_type("Pass")),
        ("Break", make_node_type("Break")),
        ("Continue", make_node_type("Continue")),
        // Load/Store/Del contexts
        ("Load", make_node_type("Load")),
        ("Store", make_node_type("Store")),
        ("Del", make_node_type("Del")),
        // PyCF compile flags
        ("PyCF_ONLY_AST", PyObject::int(1024)),
    ])
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
        dict_storage: None,
    }))
}

// ── tokenize module ──

pub fn create_tokenize_module() -> PyObjectRef {
    fn tokenize_string(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        check_args("generate_tokens", args, 1)?;
        let source = args[0].py_to_string();
        let mut tokens = Vec::new();

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
            let mut col = 0;
            let chars: Vec<char> = line.chars().collect();
            while col < chars.len() {
                if chars[col].is_whitespace() { col += 1; continue; }
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
                    let op: String = chars[start_col..start_col+1].iter().collect();
                    col += 1;
                    tokens.push(make_token_info(54, &op, (lineno, start_col), (lineno, col), line));
                }
            }
            tokens.push(make_token_info(4, "\n", (lineno, line.len()), (lineno, line.len()+1), line));
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

    make_module("tokenize", vec![
        ("generate_tokens", make_builtin(tokenize_string)),
        ("open", make_builtin(tokenize_open)),
        ("detect_encoding", make_builtin(detect_encoding)),
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
