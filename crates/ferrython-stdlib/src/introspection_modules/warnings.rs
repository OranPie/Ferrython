use super::*;

type WarningFilterEntry = (String, String, String, String, i64);

static WARNING_FILTERS: std::sync::LazyLock<RwLock<Vec<WarningFilterEntry>>> =
    std::sync::LazyLock::new(|| {
        RwLock::new(vec![(
            "default".into(),
            "".into(),
            "Warning".into(),
            "".into(),
            0,
        )])
    });
static WARNING_ONCE_SEEN: std::sync::LazyLock<RwLock<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(|| RwLock::new(std::collections::HashSet::new()));
static WARNING_RECORD_STACK: std::sync::LazyLock<RwLock<Vec<Option<PyObjectRef>>>> =
    std::sync::LazyLock::new(|| RwLock::new(Vec::new()));

fn normalize_warning_category_name(raw: &str) -> String {
    let mut name = raw.trim();
    if let Some(start) = name.find('\'') {
        if let Some(end) = name[start + 1..].find('\'') {
            name = &name[start + 1..start + 1 + end];
        }
    } else if let Some(start) = name.find('"') {
        if let Some(end) = name[start + 1..].find('"') {
            name = &name[start + 1..start + 1 + end];
        }
    }
    name.trim()
        .trim_start_matches("class ")
        .trim_matches('<')
        .trim_matches('>')
        .rsplit('.')
        .next()
        .unwrap_or(name)
        .trim_matches('\'')
        .trim_matches('"')
        .to_string()
}

fn warning_category_name(obj: &PyObjectRef) -> String {
    let raw = if let PyObjectPayload::Class(cd) = &obj.payload {
        cd.name.to_string()
    } else {
        obj.py_to_string()
    };
    normalize_warning_category_name(&raw)
}

pub(crate) fn emit_deprecation_warning(message: &str) {
    emit_warning("DeprecationWarning", message);
}

fn emit_warning_at(
    category: &str,
    category_cls: PyObjectRef,
    message: &str,
    filename: &str,
    lineno: i64,
) {
    {
        let guard = WARNING_RECORD_STACK.read();
        if let Some(Some(list_obj)) = guard.last() {
            let cls = PyObject::class(
                CompactString::from("WarningMessage"),
                vec![],
                IndexMap::new(),
            );
            let mut attrs = IndexMap::new();
            attrs.insert(
                CompactString::from("message"),
                PyObject::str_val(CompactString::from(message)),
            );
            attrs.insert(CompactString::from("category"), category_cls);
            attrs.insert(
                CompactString::from("filename"),
                PyObject::str_val(CompactString::from(filename)),
            );
            attrs.insert(CompactString::from("lineno"), PyObject::int(lineno));
            let warning_obj = PyObject::instance_with_attrs(cls, attrs);
            if let PyObjectPayload::List(items) = &list_obj.payload {
                items.write().push(warning_obj);
            }
            return;
        }
    }
    eprintln!("{}:{}: {}: {}", filename, lineno, category, message);
}

fn emit_warning(category: &str, message: &str) {
    let category_cls = PyObject::class(CompactString::from(category), vec![], IndexMap::new());
    emit_warning_at(category, category_cls, message, "<stdin>", 1);
}

// ── warnings module ──

pub fn create_warnings_module() -> PyObjectRef {
    fn warnings_recorder(storage: PyObjectRef) -> PyObjectRef {
        let cls = PyObject::class(
            CompactString::from("_WarningsRecorder"),
            vec![PyObject::builtin_type_by_name("list")],
            IndexMap::new(),
        );
        let mut attrs = IndexMap::new();
        attrs.insert(CompactString::from("__builtin_value__"), storage);
        PyObject::instance_with_attrs(cls, attrs)
    }

    fn match_filter(
        _action: &str,
        msg: &str,
        cat: &str,
        module: &str,
        filter: &WarningFilterEntry,
    ) -> bool {
        let (_, msg_pat, cat_pat, mod_pat, lineno) = filter;
        if !msg_pat.is_empty() && !msg.contains(msg_pat.as_str()) {
            return false;
        }
        let cat = normalize_warning_category_name(cat);
        let cat_pat = normalize_warning_category_name(cat_pat);
        if !cat_pat.is_empty() && cat_pat != "Warning" && cat != cat_pat {
            return false;
        }
        if !mod_pat.is_empty() && !module.contains(mod_pat.as_str()) {
            return false;
        }
        if *lineno != 0 {
            return false;
        }
        true
    }

    fn get_kwarg(args: &[PyObjectRef], key: &str) -> Option<PyObjectRef> {
        for arg in args {
            if let PyObjectPayload::Dict(kw_map) = &arg.payload {
                let r = kw_map.read();
                if let Some(v) = r.get(&HashableKey::str_key(CompactString::from(key))) {
                    return Some(v.clone());
                }
            }
        }
        None
    }

    fn get_kwarg_str(args: &[PyObjectRef], key: &str, default: &str) -> String {
        get_kwarg(args, key)
            .map(|v| v.py_to_string())
            .unwrap_or_else(|| default.to_string())
    }

    fn resolve_category_name(args: &[PyObjectRef]) -> (String, PyObjectRef) {
        let from_kwarg = get_kwarg(args, "category");
        let cat_obj = from_kwarg.or_else(|| {
            if args.len() >= 2
                && !matches!(
                    &args[1].payload,
                    PyObjectPayload::Dict(_) | PyObjectPayload::None
                )
            {
                Some(args[1].clone())
            } else {
                None
            }
        });
        if let Some(cat) = cat_obj {
            (warning_category_name(&cat), cat)
        } else {
            let cls = PyObject::class(CompactString::from("UserWarning"), vec![], IndexMap::new());
            ("UserWarning".to_string(), cls)
        }
    }

    // warn(message, category=UserWarning, stacklevel=1)
    let warn_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Ok(PyObject::none());
        }
        let message = args[0].py_to_string();
        let (category, category_cls) = resolve_category_name(args);
        let _stacklevel = get_kwarg(args, "stacklevel")
            .and_then(|v| v.as_int())
            .unwrap_or(1);

        let module = "<stdin>";
        let action = {
            let filters = WARNING_FILTERS.read();
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
                return Err(PyException::runtime_error(&format!(
                    "{}: {}",
                    category, message
                )));
            }
            "once" => {
                let key = format!("{}:{}:{}", message, category, module);
                let mut seen = WARNING_ONCE_SEEN.write();
                if seen.contains(&key) {
                    return Ok(PyObject::none());
                }
                seen.insert(key);
            }
            _ => {}
        }

        emit_warning_at(&category, category_cls, &message, "<stdin>", 1);
        Ok(PyObject::none())
    });

    // warn_explicit(message, category, filename, lineno, ...)
    let warn_explicit_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("warn_explicit", args, 4)?;
        let message = args[0].py_to_string();
        let category = warning_category_name(&args[1]);
        let category_cls = args[1].clone();
        let filename = args[2].py_to_string();
        let lineno = args[3].as_int().unwrap_or(1);
        let action = {
            let filters = WARNING_FILTERS.read();
            let mut found = "default".to_string();
            for f in filters.iter() {
                if match_filter(&f.0, &message, &category, &filename, f) {
                    found = f.0.clone();
                    break;
                }
            }
            found
        };
        match action.as_str() {
            "ignore" => return Ok(PyObject::none()),
            "error" => {
                return Err(PyException::runtime_error(&format!(
                    "{}: {}",
                    category, message
                )));
            }
            "once" => {
                let key = format!("{}:{}:{}", message, category, filename);
                let mut seen = WARNING_ONCE_SEEN.write();
                if seen.contains(&key) {
                    return Ok(PyObject::none());
                }
                seen.insert(key);
            }
            _ => {}
        }
        emit_warning_at(&category, category_cls, &message, &filename, lineno);
        Ok(PyObject::none())
    });

    // filterwarnings(action, message="", category=Warning, module="", lineno=0, append=False)
    let filter_warnings_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("filterwarnings", args, 1)?;
        let action = args[0].py_to_string();
        let message = get_kwarg_str(args, "message", "");
        let category = get_kwarg(args, "category")
            .map(|cat| warning_category_name(&cat))
            .unwrap_or_else(|| "Warning".to_string());
        let module = get_kwarg_str(args, "module", "");
        let lineno = get_kwarg(args, "lineno")
            .and_then(|v| v.as_int())
            .unwrap_or(0);
        let append = get_kwarg(args, "append")
            .map(|v| v.is_truthy())
            .unwrap_or(false);

        let non_dict_args: Vec<_> = args[1..]
            .iter()
            .filter(|a| !matches!(a.payload, PyObjectPayload::Dict(_)))
            .collect();
        let message = if !message.is_empty() {
            message
        } else if !non_dict_args.is_empty() {
            non_dict_args[0].py_to_string()
        } else {
            String::new()
        };

        let entry = (action, message, category, module, lineno);
        let mut filters = WARNING_FILTERS.write();
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
            .map(|cat| warning_category_name(&cat))
            .unwrap_or_else(|| {
                let non_dict: Vec<_> = args[1..]
                    .iter()
                    .filter(|a| !matches!(a.payload, PyObjectPayload::Dict(_)))
                    .collect();
                if !non_dict.is_empty() && !matches!(non_dict[0].payload, PyObjectPayload::None) {
                    warning_category_name(non_dict[0])
                } else {
                    "Warning".to_string()
                }
            });
        let lineno = get_kwarg(args, "lineno")
            .and_then(|v| v.as_int())
            .unwrap_or(0);
        let append = get_kwarg(args, "append")
            .map(|v| v.is_truthy())
            .unwrap_or(false);

        let entry = (action, String::new(), category, String::new(), lineno);
        let mut filters = WARNING_FILTERS.write();
        if append {
            filters.push(entry);
        } else {
            filters.insert(0, entry);
        }
        Ok(PyObject::none())
    });

    // resetwarnings()
    let reset_fn = make_builtin(|_args: &[PyObjectRef]| {
        let mut filters = WARNING_FILTERS.write();
        filters.clear();
        filters.push(("default".into(), "".into(), "Warning".into(), "".into(), 0));
        WARNING_ONCE_SEEN.write().clear();
        Ok(PyObject::none())
    });

    let filters_list = PyObject::list(vec![]);

    // catch_warnings(record=False)
    let catch_warnings_fn = make_builtin(|args: &[PyObjectRef]| {
        let record = get_kwarg(args, "record")
            .map(|v| v.is_truthy())
            .unwrap_or_else(|| {
                if !args.is_empty() && !matches!(&args[0].payload, PyObjectPayload::Dict(_)) {
                    args[0].is_truthy()
                } else {
                    false
                }
            });

        let cls = PyObject::class(
            CompactString::from("catch_warnings"),
            vec![],
            IndexMap::new(),
        );
        let mut attrs = IndexMap::new();
        let warning_list = PyObject::list(vec![]);
        let warning_recorder = if record {
            Some(warnings_recorder(warning_list.clone()))
        } else {
            None
        };
        attrs.insert(CompactString::from("_record"), PyObject::bool_val(record));
        attrs.insert(
            CompactString::from("_warnings"),
            warning_recorder
                .as_ref()
                .cloned()
                .unwrap_or_else(|| warning_list.clone()),
        );

        // Save filter state for restore on __exit__
        let saved_filters: Vec<WarningFilterEntry> = WARNING_FILTERS.read().clone();

        if record {
            let enter_list = warning_list.clone();
            let enter_recorder = warning_recorder
                .as_ref()
                .cloned()
                .unwrap_or_else(|| enter_list.clone());
            attrs.insert(
                CompactString::from("__enter__"),
                PyObject::native_closure(
                    "catch_warnings.__enter__",
                    move |_args: &[PyObjectRef]| {
                        WARNING_RECORD_STACK.write().push(Some(enter_list.clone()));
                        Ok(enter_recorder.clone())
                    },
                ),
            );
        } else {
            attrs.insert(
                CompactString::from("__enter__"),
                PyObject::native_function("catch_warnings.__enter__", |_args: &[PyObjectRef]| {
                    WARNING_RECORD_STACK.write().push(None);
                    Ok(PyObject::none())
                }),
            );
        }

        attrs.insert(
            CompactString::from("__exit__"),
            PyObject::native_closure("catch_warnings.__exit__", move |_args: &[PyObjectRef]| {
                WARNING_RECORD_STACK.write().pop();
                // Restore previous filters
                *WARNING_FILTERS.write() = saved_filters.clone();
                Ok(PyObject::bool_val(false))
            }),
        );

        Ok(PyObject::instance_with_attrs(cls, attrs))
    });

    // showwarning(message, category, filename, lineno, file=None, line=None)
    let showwarning_fn = make_builtin(|args: &[PyObjectRef]| {
        check_args_min("showwarning", args, 4)?;
        let message = args[0].py_to_string();
        let category = args[1].py_to_string();
        let filename = args[2].py_to_string();
        let lineno = args[3].as_int().unwrap_or(0);
        let _file = if args.len() > 4 {
            Some(args[4].clone())
        } else {
            None
        };
        let _line = if args.len() > 5 {
            Some(args[5].clone())
        } else {
            None
        };
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
    let filters_mutated_fn = make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none()));

    // Warning category classes
    fn make_warning_class(name: &str, base_name: &str) -> PyObjectRef {
        let base = PyObject::class(CompactString::from(base_name), vec![], IndexMap::new());
        let mut ns = IndexMap::new();
        let cls_name = name.to_string();
        ns.insert(
            CompactString::from("__name__"),
            PyObject::str_val(CompactString::from(name)),
        );
        ns.insert(
            CompactString::from("__init__"),
            PyObject::native_closure(&format!("{}.__init__", name), {
                let cls_name = cls_name.clone();
                move |args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Ok(PyObject::none());
                    }
                    if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                        let mut w = inst.attrs.write();
                        let msg = if args.len() > 1 {
                            args[1].py_to_string()
                        } else {
                            String::new()
                        };
                        w.insert(
                            CompactString::from("args"),
                            PyObject::tuple(
                                args[1..]
                                    .iter()
                                    .filter(|a| !matches!(a.payload, PyObjectPayload::Dict(_)))
                                    .cloned()
                                    .collect(),
                            ),
                        );
                        w.insert(
                            CompactString::from("message"),
                            PyObject::str_val(CompactString::from(&msg)),
                        );
                        let _ = &cls_name;
                    }
                    Ok(PyObject::none())
                }
            }),
        );
        let cls_name_repr = name.to_string();
        ns.insert(
            CompactString::from("__repr__"),
            PyObject::native_closure(
                &format!("{}.__repr__", name),
                move |args: &[PyObjectRef]| {
                    if !args.is_empty() {
                        if let Some(msg) = args[0].get_attr("message") {
                            let s = msg.py_to_string();
                            if !s.is_empty() {
                                return Ok(PyObject::str_val(CompactString::from(format!(
                                    "{}('{}')",
                                    cls_name_repr, s
                                ))));
                            }
                        }
                    }
                    Ok(PyObject::str_val(CompactString::from(format!(
                        "{}()",
                        cls_name_repr
                    ))))
                },
            ),
        );
        PyObject::class(CompactString::from(name), vec![base], ns)
    }

    let warning_cls = PyObject::class(CompactString::from("Warning"), vec![], {
        let mut ns = IndexMap::new();
        ns.insert(
            CompactString::from("__name__"),
            PyObject::str_val(CompactString::from("Warning")),
        );
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

    make_module(
        "warnings",
        vec![
            ("warn", warn_fn),
            ("warn_explicit", warn_explicit_fn),
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
        ],
    )
}
