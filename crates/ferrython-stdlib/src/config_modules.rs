//! Configuration and argument parsing stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
    make_module, make_builtin, check_args_min,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

// ── argparse module (basic) ──

/// Create a fully-featured ArgumentParser instance.
/// `ap_cls` is the shared class object for ArgumentParser.
/// `args` are passed through for prog/description extraction.
fn create_argument_parser(ap_cls: &PyObjectRef, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
    let inst = PyObject::instance(ap_cls.clone());
    let arg_defs: Arc<RwLock<Vec<(Vec<String>, IndexMap<CompactString, PyObjectRef>)>>> =
        Arc::new(RwLock::new(Vec::new()));

    if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
        let mut attrs = inst_data.attrs.write();
        let mut description = CompactString::from("");
        let mut prog = CompactString::from("");
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw_map) = &last.payload {
                let r = kw_map.read();
                if let Some(d) = r.get(&HashableKey::Str(CompactString::from("description"))) {
                    description = CompactString::from(d.py_to_string());
                }
                if let Some(p) = r.get(&HashableKey::Str(CompactString::from("prog"))) {
                    prog = CompactString::from(p.py_to_string());
                }
            }
        }
        // Also use first positional string as prog for add_parser("name") calls
        if prog.is_empty() {
            if let Some(first) = args.first() {
                if let PyObjectPayload::Str(s) = &first.payload {
                    prog = s.clone();
                }
            }
        }
        attrs.insert(CompactString::from("description"), PyObject::str_val(description));
        attrs.insert(CompactString::from("prog"), PyObject::str_val(prog.clone()));

            let ad = arg_defs.clone();
            attrs.insert(CompactString::from("add_argument"), PyObject::native_closure(
                "add_argument", move |args: &[PyObjectRef]| {
                    let mut names: Vec<String> = Vec::new();
                    let mut kwargs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
                    for arg in args {
                        match &arg.payload {
                            PyObjectPayload::Str(s) => { names.push(s.to_string()); }
                            PyObjectPayload::Dict(kw_map) => {
                                let r = kw_map.read();
                                for (k, v) in r.iter() {
                                    if let HashableKey::Str(ks) = k {
                                        kwargs.insert(ks.clone(), v.clone());
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    ad.write().push((names, kwargs));
                    Ok(PyObject::none())
                }
            ));

            // parse_args(args=None)
            let pa = arg_defs.clone();
            let pa_inst = inst.clone();
            attrs.insert(CompactString::from("parse_args"), PyObject::native_closure(
                "parse_args", move |call_args: &[PyObjectRef]| {
                    argparse_parse_args(&pa, call_args, false, Some(&pa_inst)).map(|(ns, _)| ns)
                }
            ));

            // parse_known_args(args=None)
            let pka = arg_defs.clone();
            let pka_inst = inst.clone();
            attrs.insert(CompactString::from("parse_known_args"), PyObject::native_closure(
                "parse_known_args", move |call_args: &[PyObjectRef]| {
                    let (ns, remaining) = argparse_parse_args(&pka, call_args, true, Some(&pka_inst))?;
                    let rem_list: Vec<PyObjectRef> = remaining.into_iter()
                        .map(|s| PyObject::str_val(CompactString::from(s)))
                        .collect();
                    Ok(PyObject::tuple(vec![ns, PyObject::list(rem_list)]))
                }
            ));

            // print_help() / format_help()
            let ph = arg_defs.clone();
            let prog_c = prog;
            attrs.insert(CompactString::from("print_help"), PyObject::native_closure(
                "print_help", move |_| {
                    let defs = ph.read();
                    println!("usage: {}", if prog_c.is_empty() { "prog" } else { prog_c.as_str() });
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
                }
            ));

            // ── add_subparsers(dest=None) ──
            // Returns a _SubParsersAction with .add_parser(name) → child ArgumentParser
            let inst_ref = inst.clone();
            let apc2 = ap_cls.clone();
            attrs.insert(CompactString::from("add_subparsers"), PyObject::native_closure(
                "add_subparsers", move |sp_args: &[PyObjectRef]| {
                    // Extract dest kwarg (default "subcommand")
                    let mut dest = CompactString::from("subcommand");
                    if let Some(last) = sp_args.last() {
                        if let PyObjectPayload::Dict(kw) = &last.payload {
                            let r = kw.read();
                            if let Some(d) = r.get(&HashableKey::Str(CompactString::from("dest"))) {
                                dest = CompactString::from(d.py_to_string());
                            }
                        }
                    }
                    // Store subparser registry on the parent instance
                    let registry: Arc<RwLock<IndexMap<CompactString, PyObjectRef>>> =
                        Arc::new(RwLock::new(IndexMap::new()));
                    if let PyObjectPayload::Instance(ref id) = inst_ref.payload {
                        let mut wa = id.attrs.write();
                        wa.insert(CompactString::from("__subparsers_dest__"), PyObject::str_val(dest));
                        // Store the registry Arc as a native closure that returns it (bridge)
                        let reg_c = registry.clone();
                        wa.insert(CompactString::from("__subparsers_registry__"), PyObject::native_closure(
                            "__subparsers_registry__", move |_| {
                                let r = reg_c.read();
                                let items: Vec<PyObjectRef> = r.iter()
                                    .map(|(k, v)| PyObject::tuple(vec![PyObject::str_val(k.clone()), v.clone()]))
                                    .collect();
                                Ok(PyObject::list(items))
                            }
                        ));
                    }
                    // Return _SubParsersAction with add_parser method
                    let sp_cls = PyObject::class(CompactString::from("_SubParsersAction"), vec![], IndexMap::new());
                    let sp_inst = PyObject::instance(sp_cls);
                    if let PyObjectPayload::Instance(ref sp_data) = sp_inst.payload {
                        let mut sa = sp_data.attrs.write();
                        let reg = registry.clone();
                        let apc3 = apc2.clone();
                        sa.insert(CompactString::from("add_parser"), PyObject::native_closure(
                            "add_parser", move |ap_args: &[PyObjectRef]| {
                                check_args_min("add_parser", ap_args, 1)?;
                                let name = CompactString::from(ap_args[0].py_to_string());
                                // Create a full child ArgumentParser by calling the factory
                                let child = create_argument_parser(&apc3, ap_args)?;
                                reg.write().insert(name, child.clone());
                                Ok(child)
                            }
                        ));
                    }
                    Ok(sp_inst)
                }
            ));

            // ── add_mutually_exclusive_group(required=False) ──
            let ad3 = arg_defs.clone();
            let meg_inst_ref = inst.clone();
            attrs.insert(CompactString::from("add_mutually_exclusive_group"), PyObject::native_closure(
                "add_mutually_exclusive_group", move |meg_args: &[PyObjectRef]| {
                    let mut required = false;
                    if let Some(last) = meg_args.last() {
                        if let PyObjectPayload::Dict(kw) = &last.payload {
                            let r = kw.read();
                            if let Some(req) = r.get(&HashableKey::Str(CompactString::from("required"))) {
                                required = req.is_truthy();
                            }
                        }
                    }
                    let group_dests: Arc<RwLock<Vec<PyObjectRef>>> = Arc::new(RwLock::new(Vec::new()));
                    let meg_cls = PyObject::class(CompactString::from("_MutuallyExclusiveGroup"), vec![], IndexMap::new());
                    let meg_inst = PyObject::instance(meg_cls);
                    if let PyObjectPayload::Instance(ref gd) = meg_inst.payload {
                        let mut ga = gd.attrs.write();
                        ga.insert(CompactString::from("required"), PyObject::bool_val(required));
                        // Store __dests__ as a shared list that gets updated by add_argument
                        let dests_list = PyObject::list(vec![]);
                        ga.insert(CompactString::from("__dests__"), dests_list.clone());
                        let ad4 = ad3.clone();
                        let gd_c = group_dests.clone();
                        let dests_ref = dests_list;
                        ga.insert(CompactString::from("add_argument"), PyObject::native_closure(
                            "add_argument", move |args: &[PyObjectRef]| {
                                let mut names: Vec<String> = Vec::new();
                                let mut kwargs: IndexMap<CompactString, PyObjectRef> = IndexMap::new();
                                for arg in args {
                                    match &arg.payload {
                                        PyObjectPayload::Str(s) => { names.push(s.to_string()); }
                                        PyObjectPayload::Dict(kw_map) => {
                                            let r = kw_map.read();
                                            for (k, v) in r.iter() {
                                                if let HashableKey::Str(ks) = k {
                                                    kwargs.insert(ks.clone(), v.clone());
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
                                    chosen.map(|n| n.trim_start_matches('-').replace('-', "_")).unwrap_or_default()
                                };
                                // Track dest in shared list for mutual exclusion check
                                if let PyObjectPayload::List(items) = &dests_ref.payload {
                                    items.write().push(PyObject::str_val(CompactString::from(dest.as_str())));
                                }
                                gd_c.write().push(PyObject::str_val(CompactString::from(dest.as_str())));
                                ad4.write().push((names, kwargs));
                                Ok(PyObject::none())
                            }
                        ));
                    }
                    // Register group on parser instance for enforcement
                    if let PyObjectPayload::Instance(ref pid) = meg_inst_ref.payload {
                        let mut pa = pid.attrs.write();
                        let groups = pa.entry(CompactString::from("__mutually_exclusive_groups__"))
                            .or_insert_with(|| PyObject::list(vec![]));
                        if let PyObjectPayload::List(items) = &groups.payload {
                            items.write().push(meg_inst.clone());
                        }
                    }
                    Ok(meg_inst)
                }
            ));
        }
        Ok(inst)
    }
    // End of create_argument_parser

    pub fn create_argparse_module() -> PyObjectRef {
        let ap_cls = PyObject::class(CompactString::from("ArgumentParser"), vec![], IndexMap::new());
        let apc = ap_cls.clone();
        let argument_parser_fn = PyObject::native_closure("ArgumentParser", move |args: &[PyObjectRef]| {
            create_argument_parser(&apc, args)
        });
    let nsc = PyObject::class(CompactString::from("Namespace"), vec![], IndexMap::new());
    let namespace_fn = PyObject::native_closure("Namespace", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(nsc.clone());
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kw_map) = &last.payload {
                if let PyObjectPayload::Instance(ref id) = inst.payload {
                    let mut attrs = id.attrs.write();
                    let r = kw_map.read();
                    for (k, v) in r.iter() {
                        if let HashableKey::Str(ks) = k {
                            attrs.insert(ks.clone(), v.clone());
                        }
                    }
                }
            }
        }
        Ok(inst)
    });

    make_module("argparse", vec![
        ("ArgumentParser", argument_parser_fn),
        ("Namespace", namespace_fn),
        ("Action", make_builtin(|_| Ok(PyObject::none()))),
        ("HelpFormatter", make_builtin(|_| Ok(PyObject::none()))),
        ("RawDescriptionHelpFormatter", make_builtin(|_| Ok(PyObject::none()))),
        ("RawTextHelpFormatter", make_builtin(|_| Ok(PyObject::none()))),
    ])
}

/// Core argument parsing logic — shared by parse_args and parse_known_args.
/// `parser_inst` is the ArgumentParser instance (needed for subparser delegation).
fn argparse_parse_args(
    arg_defs: &Arc<RwLock<Vec<(Vec<String>, IndexMap<CompactString, PyObjectRef>)>>>,
    call_args: &[PyObjectRef],
    allow_unknown: bool,
    parser_inst: Option<&PyObjectRef>,
) -> PyResult<(PyObjectRef, Vec<String>)> {
    let ns_cls = PyObject::class(CompactString::from("Namespace"), vec![], IndexMap::new());
    let ns_inst = PyObject::instance(ns_cls);
    let defs = arg_defs.read();

    let arg_strings: Vec<String> = if !call_args.is_empty() {
        match &call_args[0].payload {
            PyObjectPayload::List(items) => items.read().iter().map(|a| a.py_to_string()).collect(),
            PyObjectPayload::Tuple(items) => items.iter().map(|a| a.py_to_string()).collect(),
            PyObjectPayload::Dict(d) => {
                let dr = d.read();
                if let Some(v) = dr.get(&HashableKey::Str(CompactString::from("args"))) {
                    if let PyObjectPayload::List(items) = &v.payload {
                        items.read().iter().map(|a| a.py_to_string()).collect()
                    } else { vec![] }
                } else { vec![] }
            }
            _ => vec![],
        }
    } else { vec![] };

    // Classify defs into positional and optional
    let mut positional_defs: Vec<(String, &IndexMap<CompactString, PyObjectRef>)> = Vec::new();
    let mut optional_defs: Vec<(Vec<String>, String, &IndexMap<CompactString, PyObjectRef>)> = Vec::new();

    for (names, kwargs) in defs.iter() {
        let dest = if let Some(d) = kwargs.get("dest") {
            d.py_to_string()
        } else {
            let long = names.iter().find(|n| n.starts_with("--"));
            let chosen = long.or(names.first());
            if let Some(n) = chosen { n.trim_start_matches('-').replace('-', "_") } else { continue; }
        };
        if names.iter().all(|n| !n.starts_with('-')) {
            positional_defs.push((dest, kwargs));
        } else {
            optional_defs.push((names.clone(), dest, kwargs));
        }
    }

    // Set defaults
    if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
        for (_, kwargs) in &positional_defs {
            if let Some(def) = kwargs.get("default") {
                // Will be overwritten if arg provided
                let _ = def;
            }
        }
        for (_, dest, kwargs) in &optional_defs {
            let action = kwargs.get("action").map(|a| a.py_to_string());
            let default = if action.as_deref() == Some("store_true") && !kwargs.contains_key("default") {
                PyObject::bool_val(false)
            } else if action.as_deref() == Some("store_false") && !kwargs.contains_key("default") {
                PyObject::bool_val(true)
            } else if action.as_deref() == Some("count") && !kwargs.contains_key("default") {
                PyObject::int(0)
            } else {
                kwargs.get("default").cloned().unwrap_or_else(PyObject::none)
            };
            nd.attrs.write().insert(CompactString::from(dest.as_str()), default);
        }
    }

    fn convert_value(val_str: &str, kwargs: &IndexMap<CompactString, PyObjectRef>) -> PyResult<PyObjectRef> {
        // Validate choices
        if let Some(choices) = kwargs.get("choices") {
            if let Ok(choice_list) = choices.to_list() {
                let valid = choice_list.iter().any(|c| c.py_to_string() == val_str);
                if !valid {
                    let valid_strs: Vec<String> = choice_list.iter().map(|c| c.py_to_string()).collect();
                    return Err(PyException::runtime_error(
                        format!("argument: invalid choice: '{}' (choose from {:?})", val_str, valid_strs)
                    ));
                }
            }
        }
        if let Some(type_obj) = kwargs.get("type") {
            // Check if it's a callable (NativeFunction/NativeClosure/Class)
            match &type_obj.payload {
                PyObjectPayload::NativeFunction { func, .. } => {
                    return func(&[PyObject::str_val(CompactString::from(val_str))]);
                }
                PyObjectPayload::NativeClosure { func, .. } => {
                    return func(&[PyObject::str_val(CompactString::from(val_str))]);
                }
                PyObjectPayload::Class(_) | PyObjectPayload::BuiltinType(_) => {
                    // Handle builtin type names
                    let name = type_obj.py_to_string();
                    return Ok(match name.as_str() {
                        "int" | "<class 'int'>" => PyObject::int(val_str.parse::<i64>().map_err(|_|
                            PyException::value_error(format!("invalid literal for int(): '{}'", val_str)))?),
                        "float" | "<class 'float'>" => PyObject::float(val_str.parse::<f64>().map_err(|_|
                            PyException::value_error(format!("could not convert string to float: '{}'", val_str)))?),
                        _ => PyObject::str_val(CompactString::from(val_str)),
                    });
                }
                _ => {
                    let type_str = type_obj.py_to_string();
                    return Ok(match type_str.as_str() {
                        "int" => PyObject::int(val_str.parse::<i64>().unwrap_or(0)),
                        "float" => PyObject::float(val_str.parse::<f64>().unwrap_or(0.0)),
                        _ => PyObject::str_val(CompactString::from(val_str)),
                    });
                }
            }
        }
        Ok(PyObject::str_val(CompactString::from(val_str)))
    }

    fn get_nargs(kwargs: &IndexMap<CompactString, PyObjectRef>) -> Option<String> {
        kwargs.get("nargs").map(|n| {
            if let Some(i) = n.as_int() { i.to_string() }
            else { n.py_to_string() }
        })
    }

    let mut pos_idx = 0usize;
    let mut i = 0usize;
    let mut remaining = Vec::new();
    while i < arg_strings.len() {
        let arg = &arg_strings[i];
        if arg == "--" {
            // Everything after -- is positional
            i += 1;
            while i < arg_strings.len() {
                if pos_idx < positional_defs.len() {
                    let (dest, kwargs) = &positional_defs[pos_idx];
                    let val = convert_value(&arg_strings[i], kwargs)?;
                    if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                        nd.attrs.write().insert(CompactString::from(dest.as_str()), val);
                    }
                    pos_idx += 1;
                } else if allow_unknown {
                    remaining.push(arg_strings[i].clone());
                }
                i += 1;
            }
            break;
        } else if arg.starts_with('-') {
            if let Some((_, dest, kwargs)) = optional_defs.iter().find(|(names, _, _)| {
                names.iter().any(|n| n == arg)
            }) {
                let action = kwargs.get("action").map(|a| a.py_to_string());
                let nargs = get_nargs(kwargs);
                if action.as_deref() == Some("store_true") {
                    if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                        nd.attrs.write().insert(CompactString::from(dest.as_str()), PyObject::bool_val(true));
                    }
                } else if action.as_deref() == Some("store_false") {
                    if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                        nd.attrs.write().insert(CompactString::from(dest.as_str()), PyObject::bool_val(false));
                    }
                } else if action.as_deref() == Some("count") {
                    if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                        let mut wa = nd.attrs.write();
                        let cur = wa.get(dest.as_str()).and_then(|v| v.as_int()).unwrap_or(0);
                        wa.insert(CompactString::from(dest.as_str()), PyObject::int(cur + 1));
                    }
                } else if action.as_deref() == Some("append") {
                    i += 1;
                    if i < arg_strings.len() {
                        let val = convert_value(&arg_strings[i], kwargs)?;
                        if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                            let mut wa = nd.attrs.write();
                            let cur = wa.get(dest.as_str()).cloned();
                            if let Some(cur_val) = cur {
                                if let Ok(mut list) = cur_val.to_list() {
                                    list.push(val);
                                    wa.insert(CompactString::from(dest.as_str()), PyObject::list(list));
                                } else {
                                    wa.insert(CompactString::from(dest.as_str()), PyObject::list(vec![val]));
                                }
                            } else {
                                wa.insert(CompactString::from(dest.as_str()), PyObject::list(vec![val]));
                            }
                        }
                    }
                } else {
                    // Consume values based on nargs
                    match nargs.as_deref() {
                        Some("*") | Some("+") => {
                            let mut vals = Vec::new();
                            while i + 1 < arg_strings.len() && !arg_strings[i + 1].starts_with('-') {
                                i += 1;
                                vals.push(convert_value(&arg_strings[i], kwargs)?);
                            }
                            if nargs.as_deref() == Some("+") && vals.is_empty() {
                                return Err(PyException::runtime_error(
                                    format!("argument {}: expected at least one argument", arg)
                                ));
                            }
                            if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                                nd.attrs.write().insert(CompactString::from(dest.as_str()), PyObject::list(vals));
                            }
                        }
                        Some("?") => {
                            if i + 1 < arg_strings.len() && !arg_strings[i + 1].starts_with('-') {
                                i += 1;
                                let val = convert_value(&arg_strings[i], kwargs)?;
                                if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                                    nd.attrs.write().insert(CompactString::from(dest.as_str()), val);
                                }
                            } else {
                                let const_val = kwargs.get("const").cloned().unwrap_or_else(PyObject::none);
                                if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                                    nd.attrs.write().insert(CompactString::from(dest.as_str()), const_val);
                                }
                            }
                        }
                        Some(n_str) if n_str.parse::<usize>().is_ok() => {
                            let count = n_str.parse::<usize>().unwrap();
                            let mut vals = Vec::new();
                            for _ in 0..count {
                                i += 1;
                                if i < arg_strings.len() {
                                    vals.push(convert_value(&arg_strings[i], kwargs)?);
                                }
                            }
                            if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                                nd.attrs.write().insert(CompactString::from(dest.as_str()), PyObject::list(vals));
                            }
                        }
                        _ => {
                            i += 1;
                            if i < arg_strings.len() {
                                let val = convert_value(&arg_strings[i], kwargs)?;
                                if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                                    nd.attrs.write().insert(CompactString::from(dest.as_str()), val);
                                }
                            }
                        }
                    }
                }
            } else if allow_unknown {
                remaining.push(arg.clone());
            }
        } else {
            // Positional argument — first check if this is a subparser command
            let mut handled_as_subparser = false;
            if let Some(pinst) = parser_inst {
                if let PyObjectPayload::Instance(ref pid) = pinst.payload {
                    let pr = pid.attrs.read();
                    if let (Some(dest_obj), Some(reg_fn)) = (
                        pr.get("__subparsers_dest__"),
                        pr.get("__subparsers_registry__"),
                    ) {
                        let sp_dest = dest_obj.py_to_string();
                        // Call the registry function to get (name, parser) tuples
                        let registry_list = match &reg_fn.payload {
                            PyObjectPayload::NativeClosure { func, .. } => func(&[]),
                            PyObjectPayload::NativeFunction { func, .. } => func(&[]),
                            _ => Err(PyException::runtime_error("bad subparser registry")),
                        };
                        if let Ok(rlist) = registry_list {
                            if let PyObjectPayload::List(items_lock) = &rlist.payload {
                                let items = items_lock.read();
                                for item in items.iter() {
                                    if let PyObjectPayload::Tuple(tup) = &item.payload {
                                        if tup.len() == 2 && tup[0].py_to_string() == *arg {
                                            let child_parser = &tup[1];
                                            // Set dest on namespace
                                            if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                                                nd.attrs.write().insert(
                                                    CompactString::from(sp_dest.as_str()),
                                                    PyObject::str_val(CompactString::from(arg.as_str())),
                                                );
                                            }
                                            // Delegate remaining args to child parser's parse_args
                                            let child_args: Vec<PyObjectRef> = arg_strings[i + 1..]
                                                .iter()
                                                .map(|s| PyObject::str_val(CompactString::from(s.as_str())))
                                                .collect();
                                            if let Some(parse_fn) = child_parser.get_attr("parse_args") {
                                                let child_ns = match &parse_fn.payload {
                                                    PyObjectPayload::NativeClosure { func, .. } => func(&[PyObject::list(child_args)])?,
                                                    PyObjectPayload::NativeFunction { func, .. } => func(&[PyObject::list(child_args)])?,
                                                    _ => return Err(PyException::runtime_error("parse_args not callable")),
                                                };
                                                // Merge child namespace into parent
                                                if let PyObjectPayload::Instance(ref child_data) = child_ns.payload {
                                                    let cr = child_data.attrs.read();
                                                    if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                                                        let mut wa = nd.attrs.write();
                                                        for (k, v) in cr.iter() {
                                                            wa.insert(k.clone(), v.clone());
                                                        }
                                                    }
                                                }
                                            }
                                            handled_as_subparser = true;
                                            i = arg_strings.len();
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if !handled_as_subparser {
            if pos_idx < positional_defs.len() {
                let (dest, kwargs) = &positional_defs[pos_idx];
                let nargs = get_nargs(kwargs);
                match nargs.as_deref() {
                    Some("*") | Some("+") => {
                        let mut vals = vec![convert_value(arg, kwargs)?];
                        while i + 1 < arg_strings.len() && !arg_strings[i + 1].starts_with('-') {
                            i += 1;
                            vals.push(convert_value(&arg_strings[i], kwargs)?);
                        }
                        if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                            nd.attrs.write().insert(CompactString::from(dest.as_str()), PyObject::list(vals));
                        }
                        pos_idx += 1;
                    }
                    Some(n_str) if n_str.parse::<usize>().is_ok() => {
                        let count = n_str.parse::<usize>().unwrap();
                        let mut vals = vec![convert_value(arg, kwargs)?];
                        for _ in 1..count {
                            i += 1;
                            if i < arg_strings.len() {
                                vals.push(convert_value(&arg_strings[i], kwargs)?);
                            }
                        }
                        if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                            nd.attrs.write().insert(CompactString::from(dest.as_str()), PyObject::list(vals));
                        }
                        pos_idx += 1;
                    }
                    _ => {
                        let val = convert_value(arg, kwargs)?;
                        if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                            nd.attrs.write().insert(CompactString::from(dest.as_str()), val);
                        }
                        pos_idx += 1;
                    }
                }
            } else if allow_unknown {
                remaining.push(arg.clone());
            }
            } // end if !handled_as_subparser
        }
        i += 1;
    }

    // Check required optional arguments
    if !allow_unknown {
        for (names, dest, kwargs) in &optional_defs {
            if let Some(req) = kwargs.get("required") {
                if req.is_truthy() {
                    if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                        let attrs = nd.attrs.read();
                        if let Some(v) = attrs.get(dest.as_str()) {
                            if matches!(v.payload, PyObjectPayload::None) {
                                return Err(PyException::runtime_error(
                                    format!("the following arguments are required: {}", names.first().unwrap_or(&dest.clone()))
                                ));
                            }
                        }
                    }
                }
            }
        }
        // Check required positional arguments (nargs not * or ?)
        for (dest, kwargs) in positional_defs.iter().take(pos_idx.max(positional_defs.len())) {
            if pos_idx <= positional_defs.iter().position(|d| d.0 == *dest).unwrap_or(0) {
                let nargs = get_nargs(kwargs);
                match nargs.as_deref() {
                    Some("*") | Some("?") => {} // optional
                    _ => {
                        // This positional was not provided
                        return Err(PyException::runtime_error(
                            format!("the following arguments are required: {}", dest)
                        ));
                    }
                }
            }
        }
        // Enforce mutually exclusive groups
        if let Some(pinst) = parser_inst {
            if let PyObjectPayload::Instance(ref pid) = pinst.payload {
                let pr = pid.attrs.read();
                if let Some(meg_list) = pr.get("__mutually_exclusive_groups__") {
                    if let Ok(groups) = meg_list.to_list() {
                        for group in &groups {
                            if let PyObjectPayload::Instance(ref gdata) = group.payload {
                                let ga = gdata.attrs.read();
                                let required = ga.get("required")
                                    .map(|v| v.is_truthy()).unwrap_or(false);
                                let dests: Vec<String> = ga.get("__dests__")
                                    .and_then(|v| v.to_list().ok())
                                    .map(|l| l.iter().map(|i| i.py_to_string()).collect())
                                    .unwrap_or_default();
                                // Count how many were set
                                let mut count = 0;
                                if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                                    let attrs = nd.attrs.read();
                                    for d in &dests {
                                        if let Some(v) = attrs.get(d.as_str()) {
                                            if !matches!(v.payload, PyObjectPayload::None)
                                                && !matches!(v.payload, PyObjectPayload::Bool(false)) {
                                                count += 1;
                                            }
                                        }
                                    }
                                }
                                if count > 1 {
                                    return Err(PyException::runtime_error(
                                        format!("argument: not allowed with argument (mutually exclusive)")
                                    ));
                                }
                                if required && count == 0 {
                                    return Err(PyException::runtime_error(
                                        format!("one of the arguments {} is required",
                                            dests.iter().map(|d| format!("--{}", d)).collect::<Vec<_>>().join(" "))
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok((ns_inst, remaining))
}

// ── configparser module ──────────────────────────────────────────────
pub fn create_configparser_module() -> PyObjectRef {

    // Build ConfigParser as a proper Class so subclasses inherit methods via MRO.
    let mut ns = IndexMap::new();

    // __init__: set up per-instance state
    ns.insert(CompactString::from("__init__"), make_builtin(|args: &[PyObjectRef]| {
        if !args.is_empty() {
            if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                let mut w = inst.attrs.write();
                w.insert(CompactString::from("__configparser__"), PyObject::bool_val(true));
                w.insert(CompactString::from("_sections"), PyObject::dict(IndexMap::new()));
                w.insert(CompactString::from("_defaults"), PyObject::dict(IndexMap::new()));
            }
        }
        Ok(PyObject::none())
    }));

    ns.insert(CompactString::from("read"), make_builtin(cp_read));
    ns.insert(CompactString::from("read_string"), make_builtin(cp_read_string));
    ns.insert(CompactString::from("get"), make_builtin(cp_get));
    ns.insert(CompactString::from("getint"), make_builtin(cp_getint));
    ns.insert(CompactString::from("getfloat"), make_builtin(cp_getfloat));
    ns.insert(CompactString::from("getboolean"), make_builtin(cp_getboolean));
    ns.insert(CompactString::from("sections"), make_builtin(cp_sections));
    ns.insert(CompactString::from("has_section"), make_builtin(cp_has_section));
    ns.insert(CompactString::from("has_option"), make_builtin(cp_has_option));
    ns.insert(CompactString::from("options"), make_builtin(cp_options));
    ns.insert(CompactString::from("items"), make_builtin(cp_items));
    ns.insert(CompactString::from("set"), make_builtin(cp_set));
    ns.insert(CompactString::from("add_section"), make_builtin(cp_add_section));
    ns.insert(CompactString::from("remove_section"), make_builtin(cp_remove_section));
    ns.insert(CompactString::from("remove_option"), make_builtin(cp_remove_option));
    ns.insert(CompactString::from("write"), make_builtin(cp_write));
    ns.insert(CompactString::from("__getitem__"), make_builtin(cp_getitem));
    ns.insert(CompactString::from("__setitem__"), make_builtin(cp_setitem));
    ns.insert(CompactString::from("__contains__"), make_builtin(cp_contains));

    let configparser_class = PyObject::class(CompactString::from("ConfigParser"), vec![], ns);

    fn get_sections(obj: &PyObjectRef) -> Option<Arc<RwLock<IndexMap<HashableKey, PyObjectRef>>>> {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            if let Some(sec) = inst.attrs.read().get("_sections") {
                if let PyObjectPayload::Dict(d) = &sec.payload {
                    return Some(d.clone());
                }
            }
        }
        None
    }

    fn parse_ini(content: &str) -> IndexMap<HashableKey, PyObjectRef> {
        let mut sections: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
        let mut current_section = CompactString::from("DEFAULT");
        let mut current_items: IndexMap<HashableKey, PyObjectRef> = IndexMap::new();
        let mut last_key: Option<CompactString> = None;

        for line in content.lines() {
            // Blank lines or comments
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
                last_key = None;
                continue;
            }
            // Section header
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                if !current_items.is_empty() {
                    sections.insert(HashableKey::Str(current_section.clone()), PyObject::dict(current_items.clone()));
                    current_items.clear();
                }
                current_section = CompactString::from(&trimmed[1..trimmed.len()-1]);
                last_key = None;
            } else if line.starts_with(' ') || line.starts_with('\t') {
                // Continuation line (multiline value): append to last key's value
                if let Some(ref key) = last_key {
                    let key_h = HashableKey::Str(key.clone());
                    if let Some(existing) = current_items.get(&key_h) {
                        let prev = existing.py_to_string();
                        let combined = format!("{}\n{}", prev, trimmed);
                        current_items.insert(key_h, PyObject::str_val(CompactString::from(&combined)));
                    }
                }
            } else if let Some(eq_pos) = trimmed.find('=') {
                let key = CompactString::from(trimmed[..eq_pos].trim());
                let val = trimmed[eq_pos+1..].trim();
                current_items.insert(HashableKey::Str(key.clone()), PyObject::str_val(CompactString::from(val)));
                last_key = Some(key);
            } else if let Some(col_pos) = trimmed.find(':') {
                let key = CompactString::from(trimmed[..col_pos].trim());
                let val = trimmed[col_pos+1..].trim();
                current_items.insert(HashableKey::Str(key.clone()), PyObject::str_val(CompactString::from(val)));
                last_key = Some(key);
            }
        }
        if !current_items.is_empty() {
            sections.insert(HashableKey::Str(current_section), PyObject::dict(current_items));
        }
        sections
    }

    /// Perform %(name)s interpolation on a value, resolving from section then defaults.
    fn interpolate_value(
        raw: &str,
        section_items: Option<&IndexMap<HashableKey, PyObjectRef>>,
        defaults: Option<&IndexMap<HashableKey, PyObjectRef>>,
        depth: usize,
    ) -> String {
        if depth > 10 { return raw.to_string(); } // guard against infinite recursion
        let mut result = String::new();
        let mut chars = raw.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '%' && chars.peek() == Some(&'(') {
                chars.next(); // skip '('
                let mut var_name = String::new();
                loop {
                    match chars.next() {
                        Some(')') => break,
                        Some(c) => var_name.push(c),
                        None => { result.push_str("%("); result.push_str(&var_name); break; }
                    }
                }
                // Skip the format char (usually 's')
                if chars.peek() == Some(&'s') { chars.next(); }
                // Look up the variable
                let var_key = HashableKey::Str(CompactString::from(var_name.to_lowercase().as_str()));
                let resolved = section_items
                    .and_then(|s| s.get(&var_key))
                    .or_else(|| defaults.and_then(|d| d.get(&var_key)));
                if let Some(val) = resolved {
                    let val_str = val.py_to_string();
                    result.push_str(&interpolate_value(&val_str, section_items, defaults, depth + 1));
                } else {
                    result.push_str(&format!("%({})", var_name));
                }
            } else {
                result.push(ch);
            }
        }
        result
    }

    fn apply_parsed(obj: &PyObjectRef, parsed: IndexMap<HashableKey, PyObjectRef>) {
        if let Some(secs) = get_sections(obj) {
            let mut w = secs.write();
            for (k, v) in &parsed {
                if k != &HashableKey::Str(CompactString::from("DEFAULT")) {
                    w.insert(k.clone(), v.clone());
                }
            }
        }
        // Copy DEFAULT section items to _defaults for fallback inheritance
        if let Some(default_dict) = parsed.get(&HashableKey::Str(CompactString::from("DEFAULT"))) {
            if let PyObjectPayload::Instance(inst) = &obj.payload {
                if let PyObjectPayload::Dict(src) = &default_dict.payload {
                    let mut attrs_w = inst.attrs.write();
                    // Merge into _defaults
                    if let Some(defs) = attrs_w.get("_defaults") {
                        if let PyObjectPayload::Dict(d) = &defs.payload {
                            let mut dw = d.write();
                            for (k, v) in src.read().iter() {
                                dw.insert(k.clone(), v.clone());
                            }
                            return;
                        }
                    }
                    // If _defaults doesn't exist yet, create it
                    attrs_w.insert(CompactString::from("_defaults"), default_dict.clone());
                }
            }
        }
    }

    fn cp_add_section(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("add_section requires section name")); }
        let section = CompactString::from(args[1].py_to_string());
        if section == "DEFAULT" || section == "default" {
            return Err(PyException::value_error("Invalid section name: 'DEFAULT'"));
        }
        if let Some(secs) = get_sections(&args[0]) {
            let sec_key = HashableKey::Str(section.clone());
            let mut w = secs.write();
            if w.contains_key(&sec_key) {
                return Err(PyException::runtime_error(format!("Section '{}' already exists", section)));
            }
            w.insert(sec_key, PyObject::dict(IndexMap::new()));
        }
        Ok(PyObject::none())
    }

    fn cp_read(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("read requires filename")); }
        let path = args[1].py_to_string();
        let content = std::fs::read_to_string(&path).map_err(|e|
            PyException::runtime_error(format!("Cannot read {}: {}", path, e)))?;
        let parsed = parse_ini(&content);
        apply_parsed(&args[0], parsed);
        Ok(PyObject::none())
    }

    fn cp_read_string(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("read_string requires string")); }
        let content = args[1].py_to_string();
        let parsed = parse_ini(&content);
        apply_parsed(&args[0], parsed);
        Ok(PyObject::none())
    }

    fn cp_get(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 3 { return Err(PyException::type_error("get requires section and option")); }
        let section = CompactString::from(args[1].py_to_string());
        let option = args[2].py_to_string().to_lowercase();
        let option_key = HashableKey::Str(CompactString::from(&option));

        // Check for raw=True kwarg (skip interpolation)
        let raw = if args.len() > 3 {
            if let PyObjectPayload::Dict(kw) = &args[args.len()-1].payload {
                kw.read().get(&HashableKey::Str(CompactString::from("raw")))
                    .map(|v| v.is_truthy()).unwrap_or(false)
            } else { false }
        } else { false };

        // Collect section items and defaults for interpolation
        let mut section_items_snap: Option<IndexMap<HashableKey, PyObjectRef>> = None;
        let mut defaults_snap: Option<IndexMap<HashableKey, PyObjectRef>> = None;
        let mut raw_val: Option<PyObjectRef> = None;

        // Check section first
        if let Some(secs) = get_sections(&args[0]) {
            let r = secs.read();
            if let Some(sec_dict) = r.get(&HashableKey::Str(section.clone())) {
                if let PyObjectPayload::Dict(d) = &sec_dict.payload {
                    let dr = d.read();
                    if let Some(val) = dr.get(&option_key) {
                        raw_val = Some(val.clone());
                    } else {
                        // Case-insensitive fallback
                        for (k, v) in dr.iter() {
                            if k.to_object().py_to_string().to_lowercase() == option {
                                raw_val = Some(v.clone());
                                break;
                            }
                        }
                    }
                    section_items_snap = Some(dr.clone());
                }
            }
        }
        // Check DEFAULT
        if let PyObjectPayload::Instance(inst) = &args[0].payload {
            if let Some(defaults) = inst.attrs.read().get("_defaults") {
                if let PyObjectPayload::Dict(d) = &defaults.payload {
                    let dr = d.read();
                    if raw_val.is_none() {
                        if let Some(val) = dr.get(&option_key) {
                            raw_val = Some(val.clone());
                        } else {
                            for (k, v) in dr.iter() {
                                if k.to_object().py_to_string().to_lowercase() == option {
                                    raw_val = Some(v.clone());
                                    break;
                                }
                            }
                        }
                    }
                    defaults_snap = Some(dr.clone());
                }
            }
        }

        if let Some(val) = raw_val {
            if raw {
                return Ok(val);
            }
            // Apply %(name)s interpolation
            let val_str = val.py_to_string();
            if val_str.contains("%(") {
                let interpolated = interpolate_value(
                    &val_str,
                    section_items_snap.as_ref(),
                    defaults_snap.as_ref(),
                    0,
                );
                return Ok(PyObject::str_val(CompactString::from(&interpolated)));
            }
            return Ok(val);
        }
        if args.len() > 3 && !matches!(&args[3].payload, PyObjectPayload::Dict(_)) {
            return Ok(args[3].clone()); // fallback
        }
        Err(PyException::key_error(format!("No option '{}' in section", option)))
    }

    fn cp_getint(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let val = cp_get(args)?;
        let s = val.py_to_string();
        let n: i64 = s.parse().map_err(|_| PyException::value_error(format!("invalid int: {}", s)))?;
        Ok(PyObject::int(n))
    }

    fn cp_getfloat(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let val = cp_get(args)?;
        let s = val.py_to_string();
        let f: f64 = s.parse().map_err(|_| PyException::value_error(format!("invalid float: {}", s)))?;
        Ok(PyObject::float(f))
    }

    fn cp_getboolean(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let val = cp_get(args)?;
        let s = val.py_to_string().to_lowercase();
        Ok(PyObject::bool_val(matches!(s.as_str(), "1" | "yes" | "true" | "on")))
    }

    fn cp_sections(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() { return Ok(PyObject::list(vec![])); }
        if let Some(secs) = get_sections(&args[0]) {
            let keys: Vec<PyObjectRef> = secs.read().keys()
                .filter_map(|k| if let HashableKey::Str(s) = k { Some(PyObject::str_val(s.clone())) } else { None })
                .collect();
            return Ok(PyObject::list(keys));
        }
        Ok(PyObject::list(vec![]))
    }

    fn cp_has_section(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Ok(PyObject::bool_val(false)); }
        let section = CompactString::from(args[1].py_to_string());
        if let Some(secs) = get_sections(&args[0]) {
            return Ok(PyObject::bool_val(secs.read().contains_key(&HashableKey::Str(section))));
        }
        Ok(PyObject::bool_val(false))
    }

    fn cp_has_option(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 3 { return Ok(PyObject::bool_val(false)); }
        let section = CompactString::from(args[1].py_to_string());
        let option = CompactString::from(args[2].py_to_string());
        if let Some(secs) = get_sections(&args[0]) {
            let r = secs.read();
            if let Some(sec_dict) = r.get(&HashableKey::Str(section)) {
                if let PyObjectPayload::Dict(d) = &sec_dict.payload {
                    return Ok(PyObject::bool_val(d.read().contains_key(&HashableKey::Str(option))));
                }
            }
        }
        Ok(PyObject::bool_val(false))
    }

    fn cp_options(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Ok(PyObject::list(vec![])); }
        let section = CompactString::from(args[1].py_to_string());
        if let Some(secs) = get_sections(&args[0]) {
            let r = secs.read();
            if let Some(sec_dict) = r.get(&HashableKey::Str(section)) {
                if let PyObjectPayload::Dict(d) = &sec_dict.payload {
                    let keys: Vec<PyObjectRef> = d.read().keys()
                        .filter_map(|k| if let HashableKey::Str(s) = k { Some(PyObject::str_val(s.clone())) } else { None })
                        .collect();
                    return Ok(PyObject::list(keys));
                }
            }
        }
        Ok(PyObject::list(vec![]))
    }

    fn cp_items(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Ok(PyObject::list(vec![])); }
        let section = CompactString::from(args[1].py_to_string());
        if let Some(secs) = get_sections(&args[0]) {
            let r = secs.read();
            if let Some(sec_dict) = r.get(&HashableKey::Str(section)) {
                if let PyObjectPayload::Dict(d) = &sec_dict.payload {
                    let items: Vec<PyObjectRef> = d.read().iter()
                        .map(|(k, v)| {
                            let key = if let HashableKey::Str(s) = k { PyObject::str_val(s.clone()) } else { PyObject::none() };
                            PyObject::tuple(vec![key, v.clone()])
                        }).collect();
                    return Ok(PyObject::list(items));
                }
            }
        }
        Ok(PyObject::list(vec![]))
    }

    fn cp_set(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 4 { return Err(PyException::type_error("set requires section, option, value")); }
        let section = CompactString::from(args[1].py_to_string());
        let option = CompactString::from(args[2].py_to_string().to_lowercase());
        let value = args[3].clone();
        if let Some(secs) = get_sections(&args[0]) {
            let mut w = secs.write();
            let sec_key = HashableKey::Str(section.clone());
            if !w.contains_key(&sec_key) {
                w.insert(sec_key.clone(), PyObject::dict(IndexMap::new()));
            }
            if let Some(sec_dict) = w.get(&sec_key) {
                if let PyObjectPayload::Dict(d) = &sec_dict.payload {
                    d.write().insert(HashableKey::Str(option), value);
                }
            }
        }
        Ok(PyObject::none())
    }

    // __getitem__: config["section"] returns a section proxy dict with interpolation
    fn cp_getitem(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        check_args_min("ConfigParser.__getitem__", args, 2)?;
        let section = args[1].py_to_string();

        // Collect defaults snapshot for interpolation
        let defaults_snap: Option<IndexMap<HashableKey, PyObjectRef>> =
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                inst.attrs.read().get("_defaults")
                    .and_then(|d| if let PyObjectPayload::Dict(dd) = &d.payload {
                        Some(dd.read().clone())
                    } else { None })
            } else { None };

        if section == "DEFAULT" {
            if let Some(snap) = &defaults_snap {
                // Return interpolated defaults
                let result = IndexMap::new();
                let result_dict = PyObject::dict(result);
                if let PyObjectPayload::Dict(rd) = &result_dict.payload {
                    let mut w = rd.write();
                    for (k, v) in snap.iter() {
                        let val_str = v.py_to_string();
                        if val_str.contains("%(") {
                            let interp = interpolate_value(&val_str, Some(snap), None, 0);
                            w.insert(k.clone(), PyObject::str_val(CompactString::from(&interp)));
                        } else {
                            w.insert(k.clone(), v.clone());
                        }
                    }
                }
                return Ok(result_dict);
            }
            return Ok(PyObject::dict(IndexMap::new()));
        }
        if let Some(secs) = get_sections(&args[0]) {
            let r = secs.read();
            let key = HashableKey::Str(CompactString::from(&section));
            if let Some(sec_dict) = r.get(&key) {
                // Apply interpolation: merge section items + defaults, interpolate all values
                if let PyObjectPayload::Dict(d) = &sec_dict.payload {
                    let section_snap = d.read().clone();
                    let result = IndexMap::new();
                    let result_dict = PyObject::dict(result);
                    if let PyObjectPayload::Dict(rd) = &result_dict.payload {
                        let mut w = rd.write();
                        // Include defaults as fallback
                        if let Some(defs) = &defaults_snap {
                            for (k, v) in defs.iter() {
                                let val_str = v.py_to_string();
                                if val_str.contains("%(") {
                                    let interp = interpolate_value(&val_str, Some(&section_snap), Some(defs), 0);
                                    w.insert(k.clone(), PyObject::str_val(CompactString::from(&interp)));
                                } else {
                                    w.insert(k.clone(), v.clone());
                                }
                            }
                        }
                        // Section values override defaults
                        for (k, v) in section_snap.iter() {
                            let val_str = v.py_to_string();
                            if val_str.contains("%(") {
                                let interp = interpolate_value(&val_str, Some(&section_snap), defaults_snap.as_ref(), 0);
                                w.insert(k.clone(), PyObject::str_val(CompactString::from(&interp)));
                            } else {
                                w.insert(k.clone(), v.clone());
                            }
                        }
                    }
                    return Ok(result_dict);
                }
                return Ok(sec_dict.clone());
            }
        }
        Err(PyException::key_error(format!("'{}'", section)))
    }

    // __setitem__: config["section"] = {"key": "val", ...}
    fn cp_setitem(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        check_args_min("ConfigParser.__setitem__", args, 3)?;
        let section = args[1].py_to_string();
        let value = &args[2];
        if section == "DEFAULT" {
            // Set defaults
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                let mut w = inst.attrs.write();
                // Copy keys from value dict
                if let PyObjectPayload::Dict(d) = &value.payload {
                    let defaults = PyObject::dict(IndexMap::new());
                    if let PyObjectPayload::Dict(dd) = &defaults.payload {
                        let src = d.read();
                        let mut dst = dd.write();
                        for (k, v) in src.iter() {
                            let lk = HashableKey::Str(CompactString::from(k.to_object().py_to_string().to_lowercase()));
                            dst.insert(lk, PyObject::str_val(CompactString::from(v.py_to_string())));
                        }
                    }
                    w.insert(CompactString::from("_defaults"), defaults);
                }
            }
            return Ok(PyObject::none());
        }
        if let Some(secs) = get_sections(&args[0]) {
            let sec_key = HashableKey::Str(CompactString::from(&section));
            let sec_dict = PyObject::dict(IndexMap::new());
            // Copy keys from value dict
            if let PyObjectPayload::Dict(src_map) = &value.payload {
                if let PyObjectPayload::Dict(dst_map) = &sec_dict.payload {
                    let src = src_map.read();
                    let mut dst = dst_map.write();
                    for (k, v) in src.iter() {
                        let lk = HashableKey::Str(CompactString::from(k.to_object().py_to_string().to_lowercase()));
                        dst.insert(lk, PyObject::str_val(CompactString::from(v.py_to_string())));
                    }
                }
            }
            secs.write().insert(sec_key, sec_dict);
        }
        Ok(PyObject::none())
    }

    // __contains__: "section" in config
    fn cp_contains(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        check_args_min("ConfigParser.__contains__", args, 2)?;
        let section = args[1].py_to_string();
        if section == "DEFAULT" {
            return Ok(PyObject::bool_val(true));
        }
        if let Some(secs) = get_sections(&args[0]) {
            let r = secs.read();
            let key = HashableKey::Str(CompactString::from(&section));
            return Ok(PyObject::bool_val(r.contains_key(&key)));
        }
        Ok(PyObject::bool_val(false))
    }

    // remove_section
    fn cp_remove_section(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        check_args_min("remove_section", args, 2)?;
        let section = args[1].py_to_string();
        if let Some(secs) = get_sections(&args[0]) {
            let key = HashableKey::Str(CompactString::from(&section));
            let existed = secs.write().swap_remove(&key).is_some();
            return Ok(PyObject::bool_val(existed));
        }
        Ok(PyObject::bool_val(false))
    }

    // remove_option
    fn cp_remove_option(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        check_args_min("remove_option", args, 3)?;
        let section = args[1].py_to_string();
        let option = args[2].py_to_string();
        if let Some(secs) = get_sections(&args[0]) {
            let r = secs.read();
            let key = HashableKey::Str(CompactString::from(&section));
            if let Some(sec_dict) = r.get(&key) {
                if let PyObjectPayload::Dict(d) = &sec_dict.payload {
                    let opt_key = HashableKey::Str(CompactString::from(&option));
                    let existed = d.write().swap_remove(&opt_key).is_some();
                    return Ok(PyObject::bool_val(existed));
                }
            }
        }
        Ok(PyObject::bool_val(false))
    }

    // write: write config to a file-like object
    fn cp_write(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        check_args_min("write", args, 2)?;
        let mut output = String::new();
        // Write defaults
        if let PyObjectPayload::Instance(inst) = &args[0].payload {
            if let Some(defaults) = inst.attrs.read().get("_defaults") {
                if let PyObjectPayload::Dict(d) = &defaults.payload {
                    let r = d.read();
                    if !r.is_empty() {
                        output.push_str("[DEFAULT]\n");
                        for (k, v) in r.iter() {
                            output.push_str(&format!("{} = {}\n", k.to_object().py_to_string(), v.py_to_string()));
                        }
                        output.push('\n');
                    }
                }
            }
        }
        if let Some(secs) = get_sections(&args[0]) {
            let r = secs.read();
            for (sec_key, sec_val) in r.iter() {
                output.push_str(&format!("[{}]\n", sec_key.to_object().py_to_string()));
                if let PyObjectPayload::Dict(d) = &sec_val.payload {
                    for (k, v) in d.read().iter() {
                        output.push_str(&format!("{} = {}\n", k.to_object().py_to_string(), v.py_to_string()));
                    }
                }
                output.push('\n');
            }
        }
        // Call write on the file-like object
        let file_obj = &args[1];
        if let Some(write_fn) = file_obj.get_attr("write") {
            let text = PyObject::str_val(CompactString::from(output.as_str()));
            match &write_fn.payload {
                PyObjectPayload::NativeClosure { func, .. } => {
                    func(&[text])?;
                }
                PyObjectPayload::NativeFunction { func, .. } => {
                    func(&[text])?;
                }
                _ => {
                    // For bound methods or other callables, we can't invoke from stdlib.
                    // Fallback: write directly to StringIO buffer if possible
                    if let PyObjectPayload::Instance(inst) = &file_obj.payload {
                        if inst.attrs.read().contains_key("__stringio__") {
                            if let Some(w) = inst.attrs.read().get("write") {
                                if let PyObjectPayload::NativeClosure { func, .. } = &w.payload {
                                    let text2 = PyObject::str_val(CompactString::from(output.as_str()));
                                    func(&[text2])?;
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(PyObject::none())
    }

    make_module("configparser", vec![
        ("ConfigParser", configparser_class.clone()),
        ("RawConfigParser", configparser_class.clone()),
        ("SafeConfigParser", configparser_class),
    ])
}
