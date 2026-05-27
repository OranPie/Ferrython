use super::*;

/// Core argument parsing logic — shared by parse_args and parse_known_args.
/// `parser_inst` is the ArgumentParser instance (needed for subparser delegation).
pub(super) fn argparse_parse_args(
    arg_defs: &Rc<PyCell<Vec<(Vec<String>, IndexMap<CompactString, PyObjectRef>)>>>,
    call_args: &[PyObjectRef],
    allow_unknown: bool,
    parser_inst: Option<&PyObjectRef>,
) -> PyResult<(PyObjectRef, Vec<String>)> {
    let ns_cls = parser_inst
        .and_then(|p| {
            if let PyObjectPayload::Instance(pid) = &p.payload {
                pid.attrs.read().get("__namespace_class__").cloned()
            } else {
                None
            }
        })
        .unwrap_or_else(create_argparse_namespace_class);
    let ns_inst =
        if call_args.len() >= 2 && matches!(call_args[1].payload, PyObjectPayload::Instance(_)) {
            call_args[1].clone()
        } else {
            PyObject::instance(ns_cls)
        };
    let defs = arg_defs.read();

    let arg_strings: Vec<String> = if !call_args.is_empty()
        && !matches!(&call_args[0].payload, PyObjectPayload::None)
    {
        match &call_args[0].payload {
            PyObjectPayload::List(items) => items.read().iter().map(|a| a.py_to_string()).collect(),
            PyObjectPayload::Tuple(items) => items.iter().map(|a| a.py_to_string()).collect(),
            PyObjectPayload::Dict(d) => {
                let dr = d.read();
                if let Some(v) = dr.get(&HashableKey::str_key(CompactString::from("args"))) {
                    if let PyObjectPayload::List(items) = &v.payload {
                        items.read().iter().map(|a| a.py_to_string()).collect()
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                }
            }
            _ => vec![],
        }
    } else {
        // Default to sys.argv[1:] like CPython
        crate::get_argv().into_iter().skip(1).collect()
    };

    // Classify defs into positional and optional
    let mut positional_defs: Vec<(String, &IndexMap<CompactString, PyObjectRef>)> = Vec::new();
    let mut optional_defs: Vec<(Vec<String>, String, &IndexMap<CompactString, PyObjectRef>)> =
        Vec::new();

    for (names, kwargs) in defs.iter() {
        let dest = if let Some(d) = kwargs.get("dest") {
            d.py_to_string()
        } else {
            let long = names.iter().find(|n| n.starts_with("--"));
            let chosen = long.or(names.first());
            if let Some(n) = chosen {
                n.trim_start_matches('-').replace('-', "_")
            } else {
                continue;
            }
        };
        if names.iter().all(|n| !n.starts_with('-')) {
            positional_defs.push((dest, kwargs));
        } else {
            optional_defs.push((names.clone(), dest, kwargs));
        }
    }

    // Set defaults
    if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
        // Positional args: set defaults (None unless explicitly specified)
        // This ensures optional positionals (nargs="?", "*") have the attribute in namespace.
        for (dest, kwargs) in &positional_defs {
            let default = kwargs
                .get("default")
                .cloned()
                .unwrap_or_else(PyObject::none);
            nd.attrs
                .write()
                .insert(CompactString::from(dest.as_str()), default);
        }
        for (_, dest, kwargs) in &optional_defs {
            let action = kwargs.get("action").map(|a| a.py_to_string());
            let default = if action.as_deref() == Some("store_true")
                && !kwargs.contains_key("default")
            {
                PyObject::bool_val(false)
            } else if action.as_deref() == Some("store_false") && !kwargs.contains_key("default") {
                PyObject::bool_val(true)
            } else if action.as_deref() == Some("count") && !kwargs.contains_key("default") {
                PyObject::int(0)
            } else {
                kwargs
                    .get("default")
                    .cloned()
                    .unwrap_or_else(PyObject::none)
            };
            nd.attrs
                .write()
                .insert(CompactString::from(dest.as_str()), default);
        }
    }

    // Apply set_defaults() values — these override per-argument defaults
    // but will themselves be overridden by actual command-line values below.
    if let Some(pinst) = parser_inst {
        if let PyObjectPayload::Instance(ref pid) = pinst.payload {
            // Pre-populate the subparser dest with None so it's always present
            // even when no subcommand is given.
            if let Some(sp_dest) = pid.attrs.read().get("__subparsers_dest__").cloned() {
                let dest_str = sp_dest.py_to_string();
                if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                    let mut wa = nd.attrs.write();
                    wa.entry(CompactString::from(dest_str.as_str()))
                        .or_insert_with(PyObject::none);
                }
            }
            if let Some(defaults_obj) = pid.attrs.read().get("__defaults__").cloned() {
                if let PyObjectPayload::Dict(dd) = &defaults_obj.payload {
                    let dr = dd.read();
                    if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                        let mut wa = nd.attrs.write();
                        for (k, v) in dr.iter() {
                            if let HashableKey::Str(ks) = k {
                                wa.insert(ks.to_compact_string(), v.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    // Handle -h / --help: print help and exit.
    // Only at this parser level if --help appears before any subcommand.
    let first_help_pos = arg_strings.iter().position(|a| a == "-h" || a == "--help");
    let first_positional_pos = arg_strings.iter().position(|a| !a.starts_with('-'));
    let help_at_this_level = first_help_pos
        .map(|hp| first_positional_pos.map_or(true, |pp| hp <= pp))
        .unwrap_or(false);
    if help_at_this_level {
        if let Some(pinst) = parser_inst {
            if let Some(ph_fn) = pinst.get_attr("__argparse_print_help__") {
                let _ = match &ph_fn.payload {
                    PyObjectPayload::NativeClosure(nc) => (nc.func)(&[]),
                    PyObjectPayload::NativeFunction(nf) => (nf.func)(&[]),
                    _ => Ok(PyObject::none()),
                };
            }
        }
        return Err(PyException::system_exit(PyObject::int(0)));
    }

    fn convert_value(
        val_str: &str,
        kwargs: &IndexMap<CompactString, PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        // Validate choices
        if let Some(choices) = kwargs.get("choices") {
            if let Ok(choice_list) = choices.to_list() {
                let valid = choice_list.iter().any(|c| c.py_to_string() == val_str);
                if !valid {
                    let valid_strs: Vec<String> =
                        choice_list.iter().map(|c| c.py_to_string()).collect();
                    return Err(PyException::runtime_error(format!(
                        "argument: invalid choice: '{}' (choose from {:?})",
                        val_str, valid_strs
                    )));
                }
            }
        }
        if let Some(type_obj) = kwargs.get("type") {
            // Check if it's a callable (NativeFunction/NativeClosure/Class)
            match &type_obj.payload {
                PyObjectPayload::NativeFunction(nf) => {
                    return (nf.func)(&[PyObject::str_val(CompactString::from(val_str))]);
                }
                PyObjectPayload::NativeClosure(nc) => {
                    return (nc.func)(&[PyObject::str_val(CompactString::from(val_str))]);
                }
                PyObjectPayload::Class(_) | PyObjectPayload::BuiltinType(_) => {
                    // Handle builtin type names
                    let name = type_obj.py_to_string();
                    return Ok(match name.as_str() {
                        "int" | "<class 'int'>" => {
                            PyObject::int(val_str.parse::<i64>().map_err(|_| {
                                PyException::value_error(format!(
                                    "invalid literal for int(): '{}'",
                                    val_str
                                ))
                            })?)
                        }
                        "float" | "<class 'float'>" => {
                            PyObject::float(val_str.parse::<f64>().map_err(|_| {
                                PyException::value_error(format!(
                                    "could not convert string to float: '{}'",
                                    val_str
                                ))
                            })?)
                        }
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
            if let Some(i) = n.as_int() {
                i.to_string()
            } else {
                n.py_to_string()
            }
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
                        nd.attrs
                            .write()
                            .insert(CompactString::from(dest.as_str()), val);
                    }
                    pos_idx += 1;
                } else if allow_unknown {
                    remaining.push(arg_strings[i].clone());
                }
                i += 1;
            }
            break;
        } else if arg.starts_with('-') {
            if let Some((_, dest, kwargs)) = optional_defs
                .iter()
                .find(|(names, _, _)| names.iter().any(|n| n == arg))
            {
                let action = kwargs.get("action").map(|a| a.py_to_string());
                let nargs = get_nargs(kwargs);
                if action.as_deref() == Some("store_true") {
                    if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                        nd.attrs
                            .write()
                            .insert(CompactString::from(dest.as_str()), PyObject::bool_val(true));
                    }
                } else if action.as_deref() == Some("store_false") {
                    if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                        nd.attrs.write().insert(
                            CompactString::from(dest.as_str()),
                            PyObject::bool_val(false),
                        );
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
                                    wa.insert(
                                        CompactString::from(dest.as_str()),
                                        PyObject::list(list),
                                    );
                                } else {
                                    wa.insert(
                                        CompactString::from(dest.as_str()),
                                        PyObject::list(vec![val]),
                                    );
                                }
                            } else {
                                wa.insert(
                                    CompactString::from(dest.as_str()),
                                    PyObject::list(vec![val]),
                                );
                            }
                        }
                    }
                } else {
                    // Consume values based on nargs
                    match nargs.as_deref() {
                        Some("*") | Some("+") => {
                            let mut vals = Vec::new();
                            while i + 1 < arg_strings.len() && !arg_strings[i + 1].starts_with('-')
                            {
                                i += 1;
                                vals.push(convert_value(&arg_strings[i], kwargs)?);
                            }
                            if nargs.as_deref() == Some("+") && vals.is_empty() {
                                return Err(PyException::runtime_error(format!(
                                    "argument {}: expected at least one argument",
                                    arg
                                )));
                            }
                            if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                                nd.attrs.write().insert(
                                    CompactString::from(dest.as_str()),
                                    PyObject::list(vals),
                                );
                            }
                        }
                        Some("?") => {
                            if i + 1 < arg_strings.len() && !arg_strings[i + 1].starts_with('-') {
                                i += 1;
                                let val = convert_value(&arg_strings[i], kwargs)?;
                                if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                                    nd.attrs
                                        .write()
                                        .insert(CompactString::from(dest.as_str()), val);
                                }
                            } else {
                                let const_val =
                                    kwargs.get("const").cloned().unwrap_or_else(PyObject::none);
                                if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                                    nd.attrs
                                        .write()
                                        .insert(CompactString::from(dest.as_str()), const_val);
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
                                nd.attrs.write().insert(
                                    CompactString::from(dest.as_str()),
                                    PyObject::list(vals),
                                );
                            }
                        }
                        _ => {
                            i += 1;
                            if i < arg_strings.len() {
                                let val = convert_value(&arg_strings[i], kwargs)?;
                                if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                                    nd.attrs
                                        .write()
                                        .insert(CompactString::from(dest.as_str()), val);
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
                            PyObjectPayload::NativeClosure(nc) => (nc.func)(&[]),
                            PyObjectPayload::NativeFunction(nf) => (nf.func)(&[]),
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
                                            if let PyObjectPayload::Instance(ref nd) =
                                                ns_inst.payload
                                            {
                                                nd.attrs.write().insert(
                                                    CompactString::from(sp_dest.as_str()),
                                                    PyObject::str_val(CompactString::from(
                                                        arg.as_str(),
                                                    )),
                                                );
                                            }
                                            // Delegate remaining args to child parser's parse_args
                                            let child_args: Vec<PyObjectRef> = arg_strings[i + 1..]
                                                .iter()
                                                .map(|s| {
                                                    PyObject::str_val(CompactString::from(
                                                        s.as_str(),
                                                    ))
                                                })
                                                .collect();
                                            if let Some(parse_fn) =
                                                child_parser.get_attr("__argparse_parse_args__")
                                            {
                                                let child_ns = match &parse_fn.payload {
                                                    PyObjectPayload::NativeClosure(nc) => {
                                                        (nc.func)(&[PyObject::list(child_args)])?
                                                    }
                                                    PyObjectPayload::NativeFunction(nf) => {
                                                        (nf.func)(&[PyObject::list(child_args)])?
                                                    }
                                                    _ => {
                                                        return Err(PyException::runtime_error(
                                                            "parse_args not callable",
                                                        ))
                                                    }
                                                };
                                                // Merge child namespace into parent
                                                if let PyObjectPayload::Instance(ref child_data) =
                                                    child_ns.payload
                                                {
                                                    let cr = child_data.attrs.read();
                                                    if let PyObjectPayload::Instance(ref nd) =
                                                        ns_inst.payload
                                                    {
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
                            while i + 1 < arg_strings.len() && !arg_strings[i + 1].starts_with('-')
                            {
                                i += 1;
                                vals.push(convert_value(&arg_strings[i], kwargs)?);
                            }
                            if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                                nd.attrs.write().insert(
                                    CompactString::from(dest.as_str()),
                                    PyObject::list(vals),
                                );
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
                                nd.attrs.write().insert(
                                    CompactString::from(dest.as_str()),
                                    PyObject::list(vals),
                                );
                            }
                            pos_idx += 1;
                        }
                        _ => {
                            let val = convert_value(arg, kwargs)?;
                            if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                                nd.attrs
                                    .write()
                                    .insert(CompactString::from(dest.as_str()), val);
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
                                return Err(PyException::runtime_error(format!(
                                    "the following arguments are required: {}",
                                    names.first().unwrap_or(&dest.clone())
                                )));
                            }
                        }
                    }
                }
            }
        }
        // Check required positional arguments (nargs not * or ?)
        for (dest, kwargs) in positional_defs
            .iter()
            .take(pos_idx.max(positional_defs.len()))
        {
            if pos_idx
                <= positional_defs
                    .iter()
                    .position(|d| d.0 == *dest)
                    .unwrap_or(0)
            {
                let nargs = get_nargs(kwargs);
                match nargs.as_deref() {
                    Some("*") | Some("?") => {} // optional
                    _ => {
                        // This positional was not provided
                        return Err(PyException::runtime_error(format!(
                            "the following arguments are required: {}",
                            dest
                        )));
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
                                let required =
                                    ga.get("required").map(|v| v.is_truthy()).unwrap_or(false);
                                let dests: Vec<String> = ga
                                    .get("__dests__")
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
                                                && !matches!(
                                                    v.payload,
                                                    PyObjectPayload::Bool(false)
                                                )
                                            {
                                                count += 1;
                                            }
                                        }
                                    }
                                }
                                if count > 1 {
                                    return Err(PyException::runtime_error(format!(
                                        "argument: not allowed with argument (mutually exclusive)"
                                    )));
                                }
                                if required && count == 0 {
                                    return Err(PyException::runtime_error(format!(
                                        "one of the arguments {} is required",
                                        dests
                                            .iter()
                                            .map(|d| format!("--{}", d))
                                            .collect::<Vec<_>>()
                                            .join(" ")
                                    )));
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
