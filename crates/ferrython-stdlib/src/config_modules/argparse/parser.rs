use super::*;

fn call_argparse_hidden_method(
    public_name: &str,
    hidden_name: &str,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    check_args_min(public_name, args, 1)?;
    let method = if let PyObjectPayload::Instance(inst) = &args[0].payload {
        inst.attrs.read().get(hidden_name).cloned()
    } else {
        None
    };
    let Some(method) = method else {
        return Err(PyException::attribute_error(format!(
            "'{}' object has no attribute '{}'",
            args[0].type_name(),
            public_name
        )));
    };
    match &method.payload {
        PyObjectPayload::NativeClosure(nc) => (nc.func)(&args[1..]),
        PyObjectPayload::NativeFunction(nf) => (nf.func)(&args[1..]),
        _ => Err(PyException::type_error(format!(
            "{} internal method is not callable",
            public_name
        ))),
    }
}

pub(super) fn argparse_add_argument(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    call_argparse_hidden_method("add_argument", "__argparse_add_argument__", args)
}

pub(super) fn argparse_parse_args_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    call_argparse_hidden_method("parse_args", "__argparse_parse_args__", args)
}

pub(super) fn argparse_parse_known_args_method(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    call_argparse_hidden_method("parse_known_args", "__argparse_parse_known_args__", args)
}

pub(super) fn argparse_print_help(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    call_argparse_hidden_method("print_help", "__argparse_print_help__", args)
}

pub(super) fn argparse_add_subparsers(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    call_argparse_hidden_method("add_subparsers", "__argparse_add_subparsers__", args)
}

pub(super) fn argparse_set_defaults(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    call_argparse_hidden_method("set_defaults", "__argparse_set_defaults__", args)
}

pub(super) fn argparse_get_default(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    call_argparse_hidden_method("get_default", "__argparse_get_default__", args)
}

pub(super) fn argparse_add_mutually_exclusive_group(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    call_argparse_hidden_method(
        "add_mutually_exclusive_group",
        "__argparse_add_mutually_exclusive_group__",
        args,
    )
}

pub(super) fn argparse_add_argument_group(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    check_args_min("add_argument_group", args, 1)?;
    let parser = args[0].clone();
    let group_cls = PyObject::class(
        CompactString::from("_ArgumentGroup"),
        vec![],
        IndexMap::new(),
    );
    let group = PyObject::instance(group_cls);
    if let PyObjectPayload::Instance(group_data) = &group.payload {
        let parser_ref = parser;
        group_data.attrs.write().insert(
            CompactString::from("add_argument"),
            PyObject::native_closure("ArgumentGroup.add_argument", move |group_args| {
                let method = if let PyObjectPayload::Instance(inst) = &parser_ref.payload {
                    inst.attrs.read().get("__argparse_add_argument__").cloned()
                } else {
                    None
                };
                let Some(method) = method else {
                    return Err(PyException::attribute_error(
                        "parser has no add_argument method",
                    ));
                };
                match &method.payload {
                    PyObjectPayload::NativeClosure(nc) => (nc.func)(group_args),
                    PyObjectPayload::NativeFunction(nf) => (nf.func)(group_args),
                    _ => Err(PyException::type_error("add_argument is not callable")),
                }
            }),
        );
    }
    Ok(group)
}

pub(super) fn argparse_exit(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let status = args.get(1).cloned().unwrap_or_else(|| PyObject::int(0));
    Err(PyException::system_exit(status))
}

pub(super) fn argparse_error(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    Err(PyException::system_exit(PyObject::int(2)))
}

pub(super) fn argparse_argument_parser_repr(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    if args.is_empty() {
        return Ok(PyObject::str_val(CompactString::from(
            "ArgumentParser(prog='', usage=None, description=None, formatter_class=<class 'HelpFormatter'>, conflict_handler='error', add_help=True)",
        )));
    }
    let prog = args[0]
        .get_attr("prog")
        .map(|v| v.repr())
        .unwrap_or_else(|| "''".to_string());
    let description = args[0]
        .get_attr("description")
        .filter(|v| !v.py_to_string().is_empty())
        .map(|v| v.repr())
        .unwrap_or_else(|| "None".to_string());
    Ok(PyObject::str_val(CompactString::from(format!(
        "ArgumentParser(prog={}, usage=None, description={}, formatter_class=<class 'HelpFormatter'>, conflict_handler='error', add_help=True)",
        prog, description
    ))))
}

/// Create a fully-featured ArgumentParser instance.
/// `ap_cls` is the shared class object for ArgumentParser.
/// `args` are passed through for prog/description extraction.

/// Create a fully-featured ArgumentParser instance.
/// `ap_cls` is the shared class object for ArgumentParser.
/// `args` are passed through for prog/description extraction.
pub(super) fn create_argument_parser(
    ap_cls: &PyObjectRef,
    ns_cls: &PyObjectRef,
    args: &[PyObjectRef],
) -> PyResult<PyObjectRef> {
    let inst = PyObject::instance(ap_cls.clone());
    init_argument_parser(&inst, ap_cls, ns_cls, args)?;
    Ok(inst)
}

pub(super) fn init_argument_parser(
    inst: &PyObjectRef,
    ap_cls: &PyObjectRef,
    ns_cls: &PyObjectRef,
    args: &[PyObjectRef],
) -> PyResult<()> {
    let arg_defs: Rc<PyCell<Vec<(Vec<String>, IndexMap<CompactString, PyObjectRef>)>>> =
        Rc::new(PyCell::new(Vec::new()));

    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();
        attrs.insert(CompactString::from("__namespace_class__"), ns_cls.clone());
        let mut description = CompactString::from("");
        let mut prog = CompactString::from("");
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw_map) = &last.payload {
                let r = kw_map.read();
                if let Some(d) = r.get(&HashableKey::str_key(CompactString::from("description"))) {
                    description = CompactString::from(d.py_to_string());
                }
                if let Some(p) = r.get(&HashableKey::str_key(CompactString::from("prog"))) {
                    prog = CompactString::from(p.py_to_string());
                }
            }
        }
        // Also use first positional string as prog for add_parser("name") calls
        if prog.is_empty() {
            if let Some(first) = args.first() {
                if let PyObjectPayload::Str(s) = &first.payload {
                    prog = s.to_compact_string();
                }
            }
        }
        attrs.insert(
            CompactString::from("description"),
            PyObject::str_val(description),
        );
        attrs.insert(CompactString::from("prog"), PyObject::str_val(prog.clone()));

        let ad = arg_defs.clone();
        attrs.insert(
            CompactString::from("__argparse_add_argument__"),
            PyObject::native_closure("add_argument", move |args: &[PyObjectRef]| {
                let mut names: Vec<String> = Vec::new();
                let mut kwargs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
                for arg in args {
                    match &arg.payload {
                        PyObjectPayload::Str(s) => {
                            names.push(s.to_string());
                        }
                        PyObjectPayload::Dict(kw_map) => {
                            let r = kw_map.read();
                            for (k, v) in r.iter() {
                                if let HashableKey::Str(ks) = k {
                                    kwargs.insert(ks.to_compact_string(), v.clone());
                                }
                            }
                        }
                        _ => {}
                    }
                }
                ad.write().push((names, kwargs));
                Ok(PyObject::none())
            }),
        );

        // parse_args(args=None)
        let pa = arg_defs.clone();
        let pa_inst = inst.clone();
        attrs.insert(
            CompactString::from("__argparse_parse_args__"),
            PyObject::native_closure("parse_args", move |call_args: &[PyObjectRef]| {
                argparse_parse_args(&pa, call_args, false, Some(&pa_inst)).map(|(ns, _)| ns)
            }),
        );

        // parse_known_args(args=None)
        let pka = arg_defs.clone();
        let pka_inst = inst.clone();
        attrs.insert(
            CompactString::from("__argparse_parse_known_args__"),
            PyObject::native_closure("parse_known_args", move |call_args: &[PyObjectRef]| {
                let (ns, remaining) = argparse_parse_args(&pka, call_args, true, Some(&pka_inst))?;
                let rem_list: Vec<PyObjectRef> = remaining
                    .into_iter()
                    .map(|s| PyObject::str_val(CompactString::from(s)))
                    .collect();
                Ok(PyObject::tuple(vec![ns, PyObject::list(rem_list)]))
            }),
        );

        // print_help() / format_help()
        let ph = arg_defs.clone();
        let prog_c = prog;
        attrs.insert(
            CompactString::from("__argparse_print_help__"),
            PyObject::native_closure("print_help", move |_| {
                let defs = ph.read();
                println!(
                    "usage: {}",
                    if prog_c.is_empty() {
                        "prog"
                    } else {
                        prog_c.as_str()
                    }
                );
                if !defs.is_empty() {
                    println!("\npositional arguments:");
                    for (names, kw) in defs.iter() {
                        if names.iter().all(|n| !n.starts_with('-')) {
                            let help = kw.get("help").map(|h| h.py_to_string()).unwrap_or_default();
                            println!("  {:20} {}", names.join(", "), help);
                        }
                    }
                    println!("\noptions:");
                    println!("  {:20} show this help message and exit", "-h, --help");
                    for (names, kw) in defs.iter() {
                        if names.iter().any(|n| n.starts_with('-')) {
                            let help = kw.get("help").map(|h| h.py_to_string()).unwrap_or_default();
                            println!("  {:20} {}", names.join(", "), help);
                        }
                    }
                }
                Ok(PyObject::none())
            }),
        );

        // ── add_subparsers(dest=None) ──
        // Returns a _SubParsersAction with .add_parser(name) → child ArgumentParser
        let inst_ref = inst.clone();
        let apc2 = ap_cls.clone();
        let nsc2 = ns_cls.clone();
        attrs.insert(
            CompactString::from("__argparse_add_subparsers__"),
            PyObject::native_closure("add_subparsers", move |sp_args: &[PyObjectRef]| {
                // Extract dest kwarg (default "subcommand")
                let mut dest = CompactString::from("subcommand");
                if let Some(last) = sp_args.last() {
                    if let PyObjectPayload::Dict(kw) = &last.payload {
                        let r = kw.read();
                        if let Some(d) = r.get(&HashableKey::str_key(CompactString::from("dest"))) {
                            dest = CompactString::from(d.py_to_string());
                        }
                    }
                }
                // Store subparser registry on the parent instance
                let registry: Rc<PyCell<IndexMap<CompactString, PyObjectRef>>> =
                    Rc::new(PyCell::new(IndexMap::new()));
                if let PyObjectPayload::Instance(ref id) = inst_ref.payload {
                    let mut wa = id.attrs.write();
                    wa.insert(
                        CompactString::from("__subparsers_dest__"),
                        PyObject::str_val(dest),
                    );
                    // Store the registry Arc as a native closure that returns it (bridge)
                    let reg_c = registry.clone();
                    wa.insert(
                        CompactString::from("__subparsers_registry__"),
                        PyObject::native_closure("__subparsers_registry__", move |_| {
                            let r = reg_c.read();
                            let items: Vec<PyObjectRef> = r
                                .iter()
                                .map(|(k, v)| {
                                    PyObject::tuple(vec![PyObject::str_val(k.clone()), v.clone()])
                                })
                                .collect();
                            Ok(PyObject::list(items))
                        }),
                    );
                }
                // Return _SubParsersAction with add_parser method
                let sp_cls = PyObject::class(
                    CompactString::from("_SubParsersAction"),
                    vec![],
                    IndexMap::new(),
                );
                let sp_inst = PyObject::instance(sp_cls);
                if let PyObjectPayload::Instance(ref sp_data) = sp_inst.payload {
                    let mut sa = sp_data.attrs.write();
                    let reg = registry.clone();
                    let apc3 = apc2.clone();
                    let nsc3 = nsc2.clone();
                    sa.insert(
                        CompactString::from("add_parser"),
                        PyObject::native_closure("add_parser", move |ap_args: &[PyObjectRef]| {
                            check_args_min("add_parser", ap_args, 1)?;
                            let name = CompactString::from(ap_args[0].py_to_string());
                            // Create a full child ArgumentParser by calling the factory
                            let child = create_argument_parser(&apc3, &nsc3, ap_args)?;
                            reg.write().insert(name, child.clone());
                            Ok(child)
                        }),
                    );
                }
                Ok(sp_inst)
            }),
        );

        // ── set_defaults(**kwargs) ──
        // Stores parser-level defaults that override per-argument defaults
        // but are themselves overridden by actual command-line values.
        let sd_inst = inst.clone();
        attrs.insert(
            CompactString::from("__argparse_set_defaults__"),
            PyObject::native_closure("set_defaults", move |sd_args: &[PyObjectRef]| {
                if let Some(last) = sd_args.last() {
                    if let PyObjectPayload::Dict(kw_map) = &last.payload {
                        if let PyObjectPayload::Instance(ref id) = sd_inst.payload {
                            let mut wa = id.attrs.write();
                            let defaults_obj = wa
                                .entry(CompactString::from("__defaults__"))
                                .or_insert_with(|| PyObject::dict(IndexMap::new()))
                                .clone();
                            if let PyObjectPayload::Dict(dd) = &defaults_obj.payload {
                                let mut d = dd.write();
                                let r = kw_map.read();
                                for (k, v) in r.iter() {
                                    d.insert(k.clone(), v.clone());
                                }
                            }
                        }
                    }
                }
                Ok(PyObject::none())
            }),
        );

        // ── get_default(dest) ──
        let gd_inst = inst.clone();
        attrs.insert(
            CompactString::from("__argparse_get_default__"),
            PyObject::native_closure("get_default", move |gd_args: &[PyObjectRef]| {
                if gd_args.is_empty() {
                    return Ok(PyObject::none());
                }
                let key = gd_args[0].py_to_string();
                if let PyObjectPayload::Instance(ref id) = gd_inst.payload {
                    let ra = id.attrs.read();
                    if let Some(defaults_obj) = ra.get("__defaults__") {
                        if let PyObjectPayload::Dict(dd) = &defaults_obj.payload {
                            let dr = dd.read();
                            if let Some(v) =
                                dr.get(&HashableKey::str_key(CompactString::from(key.as_str())))
                            {
                                return Ok(v.clone());
                            }
                        }
                    }
                }
                Ok(PyObject::none())
            }),
        );

        // ── add_mutually_exclusive_group(required=False) ──
        let ad3 = arg_defs.clone();
        let meg_inst_ref = inst.clone();
        attrs.insert(
            CompactString::from("__argparse_add_mutually_exclusive_group__"),
            PyObject::native_closure(
                "add_mutually_exclusive_group",
                move |meg_args: &[PyObjectRef]| {
                    let mut required = false;
                    if let Some(last) = meg_args.last() {
                        if let PyObjectPayload::Dict(kw) = &last.payload {
                            let r = kw.read();
                            if let Some(req) =
                                r.get(&HashableKey::str_key(CompactString::from("required")))
                            {
                                required = req.is_truthy();
                            }
                        }
                    }
                    let group_dests: Rc<PyCell<Vec<PyObjectRef>>> =
                        Rc::new(PyCell::new(Vec::new()));
                    let meg_cls = PyObject::class(
                        CompactString::from("_MutuallyExclusiveGroup"),
                        vec![],
                        IndexMap::new(),
                    );
                    let meg_inst = PyObject::instance(meg_cls);
                    if let PyObjectPayload::Instance(ref gd) = meg_inst.payload {
                        let mut ga = gd.attrs.write();
                        ga.insert(
                            CompactString::from("required"),
                            PyObject::bool_val(required),
                        );
                        // Store __dests__ as a shared list that gets updated by add_argument
                        let dests_list = PyObject::list(vec![]);
                        ga.insert(CompactString::from("__dests__"), dests_list.clone());
                        let ad4 = ad3.clone();
                        let gd_c = group_dests.clone();
                        let dests_ref = dests_list;
                        ga.insert(
                            CompactString::from("add_argument"),
                            PyObject::native_closure(
                                "add_argument",
                                move |args: &[PyObjectRef]| {
                                    let mut names: Vec<String> = Vec::new();
                                    let mut kwargs: IndexMap<CompactString, PyObjectRef> =
                                        IndexMap::new();
                                    for arg in args {
                                        match &arg.payload {
                                            PyObjectPayload::Str(s) => {
                                                names.push(s.to_string());
                                            }
                                            PyObjectPayload::Dict(kw_map) => {
                                                let r = kw_map.read();
                                                for (k, v) in r.iter() {
                                                    if let HashableKey::Str(ks) = k {
                                                        kwargs.insert(
                                                            ks.to_compact_string(),
                                                            v.clone(),
                                                        );
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    let dest = if let Some(d) = kwargs.get("dest") {
                                        d.py_to_string()
                                    } else {
                                        let long = names.iter().find(|n| n.starts_with("--"));
                                        let chosen = long.or(names.first());
                                        chosen
                                            .map(|n| n.trim_start_matches('-').replace('-', "_"))
                                            .unwrap_or_default()
                                    };
                                    // Track dest in shared list for mutual exclusion check
                                    if let PyObjectPayload::List(items) = &dests_ref.payload {
                                        items.write().push(PyObject::str_val(CompactString::from(
                                            dest.as_str(),
                                        )));
                                    }
                                    gd_c.write().push(PyObject::str_val(CompactString::from(
                                        dest.as_str(),
                                    )));
                                    ad4.write().push((names, kwargs));
                                    Ok(PyObject::none())
                                },
                            ),
                        );
                    }
                    // Register group on parser instance for enforcement
                    if let PyObjectPayload::Instance(ref pid) = meg_inst_ref.payload {
                        let mut pa = pid.attrs.write();
                        let groups = pa
                            .entry(CompactString::from("__mutually_exclusive_groups__"))
                            .or_insert_with(|| PyObject::list(vec![]));
                        if let PyObjectPayload::List(items) = &groups.payload {
                            items.write().push(meg_inst.clone());
                        }
                    }
                    Ok(meg_inst)
                },
            ),
        );
    }
    Ok(())
}
// End of create_argument_parser
