//! Configuration and argument parsing stdlib modules

use compact_str::CompactString;
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, InstanceData,
    make_module, make_builtin, check_args_min,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

// ── argparse module (basic) ──


pub fn create_argparse_module() -> PyObjectRef {
    // ArgumentParser class — functional constructor
    let ap_cls = PyObject::class(CompactString::from("ArgumentParser"), vec![], IndexMap::new());
    let apc = ap_cls.clone();
    let argument_parser_fn = PyObject::native_closure("ArgumentParser", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(apc.clone());
        // Shared argument storage between add_argument and parse_args
        let arg_defs: Arc<RwLock<Vec<(Vec<String>, IndexMap<CompactString, PyObjectRef>)>>> =
            Arc::new(RwLock::new(Vec::new()));

        if let PyObjectPayload::Instance(ref inst_data) = inst.payload {
            let mut attrs = inst_data.attrs.write();
            // Store description, prog from kwargs
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
            attrs.insert(CompactString::from("description"), PyObject::str_val(description));
            attrs.insert(CompactString::from("prog"), PyObject::str_val(prog));

            // add_argument(*name_or_flags, **kwargs) — closure captures shared arg_defs
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

            // parse_args(args=None) — closure captures shared arg_defs
            let pa = arg_defs.clone();
            attrs.insert(CompactString::from("parse_args"), PyObject::native_closure(
                "parse_args", move |_args: &[PyObjectRef]| {
                    let ns_cls = PyObject::class(CompactString::from("Namespace"), vec![], IndexMap::new());
                    let ns_inst = PyObject::instance(ns_cls);
                    // Set defaults from stored argument definitions
                    let defs = pa.read();
                    for (names, kwargs) in defs.iter() {
                        let dest = if let Some(d) = kwargs.get("dest") {
                            d.py_to_string()
                        } else {
                            // Prefer long option names (--verbose) over short (-v)
                            let long = names.iter().find(|n| n.starts_with("--"));
                            let chosen = long.or(names.first());
                            if let Some(n) = chosen {
                                n.trim_start_matches('-').replace('-', "_")
                            } else { continue; }
                        };
                        let default = kwargs.get("default").cloned().unwrap_or_else(PyObject::none);
                        if let PyObjectPayload::Instance(ref nd) = ns_inst.payload {
                            nd.attrs.write().insert(CompactString::from(dest.as_str()), default);
                        }
                    }
                    Ok(ns_inst)
                }
            ));

            attrs.insert(CompactString::from("add_subparsers"), make_builtin(|_| Ok(PyObject::none())));
            attrs.insert(CompactString::from("add_mutually_exclusive_group"), make_builtin(|_| Ok(PyObject::none())));
        }
        Ok(inst)
    });

    // Namespace class
    let ns_cls = PyObject::class(CompactString::from("Namespace"), vec![], IndexMap::new());
    let nsc = ns_cls.clone();
    let namespace_fn = PyObject::native_closure("Namespace", move |args: &[PyObjectRef]| {
        let inst = PyObject::instance(nsc.clone());
        // Accept kwargs
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
    ])
}

// ── configparser module ──────────────────────────────────────────────
pub fn create_configparser_module() -> PyObjectRef {

    fn configparser_new(_args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let mut ns = IndexMap::new();
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
        ns.insert(CompactString::from("remove_section"), make_builtin(cp_remove_section));
        ns.insert(CompactString::from("remove_option"), make_builtin(cp_remove_option));
        ns.insert(CompactString::from("write"), make_builtin(cp_write));
        ns.insert(CompactString::from("__getitem__"), make_builtin(cp_getitem));
        ns.insert(CompactString::from("__setitem__"), make_builtin(cp_setitem));
        ns.insert(CompactString::from("__contains__"), make_builtin(cp_contains));
        let class = PyObject::class(CompactString::from("ConfigParser"), vec![], ns);
        let inst = PyObject::wrap(PyObjectPayload::Instance(InstanceData {
            class,
            attrs: Arc::new(RwLock::new(IndexMap::new())),
            dict_storage: None,
        }));
        // Store sections as a dict of dicts
        if let PyObjectPayload::Instance(ref d) = inst.payload {
            let mut w = d.attrs.write();
            w.insert(CompactString::from("__configparser__"), PyObject::bool_val(true));
            w.insert(CompactString::from("_sections"), PyObject::dict(IndexMap::new()));
            w.insert(CompactString::from("_defaults"), PyObject::dict(IndexMap::new()));
        }
        Ok(inst)
    }

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

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') { continue; }
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                if !current_items.is_empty() {
                    sections.insert(HashableKey::Str(current_section.clone()), PyObject::dict(current_items.clone()));
                    current_items.clear();
                }
                current_section = CompactString::from(&trimmed[1..trimmed.len()-1]);
            } else if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim();
                let val = trimmed[eq_pos+1..].trim();
                current_items.insert(HashableKey::Str(CompactString::from(key)), PyObject::str_val(CompactString::from(val)));
            } else if let Some(eq_pos) = trimmed.find(':') {
                let key = trimmed[..eq_pos].trim();
                let val = trimmed[eq_pos+1..].trim();
                current_items.insert(HashableKey::Str(CompactString::from(key)), PyObject::str_val(CompactString::from(val)));
            }
        }
        if !current_items.is_empty() {
            sections.insert(HashableKey::Str(current_section), PyObject::dict(current_items));
        }
        sections
    }

    fn cp_read(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("read requires filename")); }
        let path = args[1].py_to_string();
        let content = std::fs::read_to_string(&path).map_err(|e|
            PyException::runtime_error(format!("Cannot read {}: {}", path, e)))?;
        if let Some(secs) = get_sections(&args[0]) {
            let parsed = parse_ini(&content);
            let mut w = secs.write();
            for (k, v) in parsed { w.insert(k, v); }
        }
        Ok(PyObject::none())
    }

    fn cp_read_string(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 { return Err(PyException::type_error("read_string requires string")); }
        let content = args[1].py_to_string();
        if let Some(secs) = get_sections(&args[0]) {
            let parsed = parse_ini(&content);
            let mut w = secs.write();
            for (k, v) in parsed { w.insert(k, v); }
        }
        Ok(PyObject::none())
    }

    fn cp_get(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 3 { return Err(PyException::type_error("get requires section and option")); }
        let section = CompactString::from(args[1].py_to_string());
        let option = args[2].py_to_string().to_lowercase();
        let option_key = HashableKey::Str(CompactString::from(&option));
        // Check section first
        if let Some(secs) = get_sections(&args[0]) {
            let r = secs.read();
            if let Some(sec_dict) = r.get(&HashableKey::Str(section.clone())) {
                if let PyObjectPayload::Dict(d) = &sec_dict.payload {
                    let dr = d.read();
                    if let Some(val) = dr.get(&option_key) {
                        return Ok(val.clone());
                    }
                    // Try case-insensitive fallback
                    for (k, v) in dr.iter() {
                        if k.to_object().py_to_string().to_lowercase() == option {
                            return Ok(v.clone());
                        }
                    }
                }
            }
        }
        // Check DEFAULT
        if let PyObjectPayload::Instance(inst) = &args[0].payload {
            if let Some(defaults) = inst.attrs.read().get("_defaults") {
                if let PyObjectPayload::Dict(d) = &defaults.payload {
                    let dr = d.read();
                    if let Some(val) = dr.get(&option_key) {
                        return Ok(val.clone());
                    }
                    for (k, v) in dr.iter() {
                        if k.to_object().py_to_string().to_lowercase() == option {
                            return Ok(v.clone());
                        }
                    }
                }
            }
        }
        if args.len() > 3 { return Ok(args[3].clone()); } // fallback
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

    // __getitem__: config["section"] returns a section proxy dict
    fn cp_getitem(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        check_args_min("ConfigParser.__getitem__", args, 2)?;
        let section = args[1].py_to_string();
        if section == "DEFAULT" {
            // Return defaults dict
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                if let Some(d) = inst.attrs.read().get("_defaults") {
                    return Ok(d.clone());
                }
            }
            return Ok(PyObject::dict(IndexMap::new()));
        }
        if let Some(secs) = get_sections(&args[0]) {
            let r = secs.read();
            let key = HashableKey::Str(CompactString::from(&section));
            if let Some(sec_dict) = r.get(&key) {
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

    // write: write config to a file-like object (simplified: just returns string)
    fn cp_write(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        check_args_min("write", args, 2)?;
        // For now, just build the string and try to write to the file-like arg
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
        // Try to call write on the file-like object
        if let Some(write_fn) = args[1].get_attr("write") {
            // Just store output — we can't call from stdlib
            // Instead, write directly if it's a StringIO
        }
        Ok(PyObject::str_val(CompactString::from(output)))
    }

    make_module("configparser", vec![
        ("ConfigParser", make_builtin(configparser_new)),
        ("RawConfigParser", make_builtin(configparser_new)),
        ("SafeConfigParser", make_builtin(configparser_new)),
    ])
}
