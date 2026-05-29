//! configparser module implementation.

use compact_str::CompactString;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    check_args_min, make_module, new_fx_hashkey_map, FxHashKeyMap, PyCell, PyObject,
    PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use indexmap::IndexMap;
use std::rc::Rc;

// ── configparser module ──────────────────────────────────────────────
pub fn create_configparser_module() -> PyObjectRef {
    let unset = PyObject::instance(PyObject::class(
        CompactString::from("_UNSET"),
        vec![],
        IndexMap::new(),
    ));
    let basic_interpolation = PyObject::class(
        CompactString::from("BasicInterpolation"),
        vec![],
        IndexMap::new(),
    );
    let extended_interpolation = PyObject::class(
        CompactString::from("ExtendedInterpolation"),
        vec![],
        IndexMap::new(),
    );
    let legacy_interpolation = PyObject::class(
        CompactString::from("LegacyInterpolation"),
        vec![],
        IndexMap::new(),
    );

    // Build ConfigParser as a proper Class so subclasses inherit methods via MRO.
    let mut ns = IndexMap::new();

    // __init__: set up per-instance state
    ns.insert(
        CompactString::from("__init__"),
        PyObject::native_closure("ConfigParser.__init__", |args: &[PyObjectRef]| {
            if !args.is_empty() {
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    let kwargs = args
                        .last()
                        .filter(|arg| matches!(&arg.payload, PyObjectPayload::Dict(_)));
                    let kw_get = |name: &str| -> Option<PyObjectRef> {
                        let PyObjectPayload::Dict(d) = &kwargs?.payload else {
                            return None;
                        };
                        d.read()
                            .get(&HashableKey::str_key(CompactString::from(name)))
                            .cloned()
                    };
                    let mut w = inst.attrs.write();
                    w.insert(
                        CompactString::from("__configparser__"),
                        PyObject::bool_val(true),
                    );
                    w.insert(
                        CompactString::from("_sections"),
                        PyObject::dict(IndexMap::new()),
                    );
                    w.insert(
                        CompactString::from("_defaults"),
                        PyObject::dict(IndexMap::new()),
                    );
                    w.insert(
                        CompactString::from("_dict"),
                        kw_get("dict_type")
                            .unwrap_or_else(|| PyObject::builtin_type(CompactString::from("dict"))),
                    );
                    w.insert(
                        CompactString::from("default_section"),
                        kw_get("default_section")
                            .unwrap_or_else(|| PyObject::str_val(CompactString::from("DEFAULT"))),
                    );
                    w.insert(CompactString::from("converters"), default_converters_dict());
                    if let Some(defaults) = kw_get("defaults") {
                        if let PyObjectPayload::Dict(src) = &defaults.payload {
                            let mut defaults_map = IndexMap::new();
                            for (key, value) in src.read().iter() {
                                defaults_map.insert(
                                    HashableKey::str_key(CompactString::from(
                                        key.to_object().py_to_string().to_lowercase(),
                                    )),
                                    value.clone(),
                                );
                            }
                            w.insert(
                                CompactString::from("_defaults"),
                                PyObject::dict(defaults_map),
                            );
                        }
                    }
                    if let Some(converters) = kw_get("converters") {
                        if let PyObjectPayload::Dict(src) = &converters.payload {
                            if let Some(PyObjectPayload::Dict(dst)) =
                                w.get("converters").map(|obj| &obj.payload)
                            {
                                for (key, value) in src.read().iter() {
                                    dst.write().insert(key.clone(), value.clone());
                                }
                            }
                        }
                    }
                }
            }
            Ok(PyObject::none())
        }),
    );

    ns.insert(
        CompactString::from("read"),
        PyObject::native_function("ConfigParser.read", cp_read),
    );
    ns.insert(
        CompactString::from("read_string"),
        PyObject::native_function("ConfigParser.read_string", cp_read_string),
    );
    ns.insert(
        CompactString::from("get"),
        PyObject::native_function("ConfigParser.get", cp_get),
    );
    ns.insert(
        CompactString::from("getint"),
        PyObject::native_function("ConfigParser.getint", cp_getint),
    );
    ns.insert(
        CompactString::from("getfloat"),
        PyObject::native_function("ConfigParser.getfloat", cp_getfloat),
    );
    ns.insert(
        CompactString::from("getboolean"),
        PyObject::native_function("ConfigParser.getboolean", cp_getboolean),
    );
    ns.insert(
        CompactString::from("sections"),
        PyObject::native_function("ConfigParser.sections", cp_sections),
    );
    ns.insert(
        CompactString::from("has_section"),
        PyObject::native_function("ConfigParser.has_section", cp_has_section),
    );
    ns.insert(
        CompactString::from("has_option"),
        PyObject::native_function("ConfigParser.has_option", cp_has_option),
    );
    ns.insert(
        CompactString::from("options"),
        PyObject::native_function("ConfigParser.options", cp_options),
    );
    ns.insert(
        CompactString::from("items"),
        PyObject::native_function("ConfigParser.items", cp_items),
    );
    ns.insert(
        CompactString::from("defaults"),
        PyObject::native_function("ConfigParser.defaults", cp_defaults),
    );
    ns.insert(
        CompactString::from("set"),
        PyObject::native_function("ConfigParser.set", cp_set),
    );
    ns.insert(
        CompactString::from("add_section"),
        PyObject::native_function("ConfigParser.add_section", cp_add_section),
    );
    ns.insert(
        CompactString::from("remove_section"),
        PyObject::native_function("ConfigParser.remove_section", cp_remove_section),
    );
    ns.insert(
        CompactString::from("remove_option"),
        PyObject::native_function("ConfigParser.remove_option", cp_remove_option),
    );
    ns.insert(
        CompactString::from("write"),
        PyObject::native_function("ConfigParser.write", cp_write),
    );
    ns.insert(
        CompactString::from("__getitem__"),
        PyObject::native_function("ConfigParser.__getitem__", cp_getitem),
    );
    ns.insert(
        CompactString::from("__setitem__"),
        PyObject::native_function("ConfigParser.__setitem__", cp_setitem),
    );
    ns.insert(
        CompactString::from("__contains__"),
        PyObject::native_function("ConfigParser.__contains__", cp_contains),
    );
    ns.insert(
        CompactString::from("__iter__"),
        PyObject::native_function("ConfigParser.__iter__", cp_iter),
    );
    ns.insert(
        CompactString::from("__len__"),
        PyObject::native_function("ConfigParser.__len__", cp_len),
    );

    let configparser_class = PyObject::class(CompactString::from("ConfigParser"), vec![], ns);

    fn get_sections(obj: &PyObjectRef) -> Option<Rc<PyCell<FxHashKeyMap>>> {
        if let PyObjectPayload::Instance(inst) = &obj.payload {
            if let Some(sec) = inst.attrs.read().get("_sections") {
                if let PyObjectPayload::Dict(d) = &sec.payload {
                    return Some(d.clone());
                }
            }
        }
        None
    }

    fn parse_ini(content: &str) -> FxHashKeyMap {
        let mut sections: FxHashKeyMap = new_fx_hashkey_map();
        let mut current_section = CompactString::from("DEFAULT");
        let mut current_items: FxHashKeyMap = new_fx_hashkey_map();
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
                    sections.insert(
                        HashableKey::str_key(current_section.clone()),
                        PyObject::dict(current_items.clone()),
                    );
                    current_items.clear();
                }
                current_section = CompactString::from(&trimmed[1..trimmed.len() - 1]);
                last_key = None;
            } else if line.starts_with(' ') || line.starts_with('\t') {
                // Continuation line (multiline value): append to last key's value
                if let Some(ref key) = last_key {
                    let key_h = HashableKey::str_key(key.clone());
                    if let Some(existing) = current_items.get(&key_h) {
                        let prev = existing.py_to_string();
                        let combined = format!("{}\n{}", prev, trimmed);
                        current_items
                            .insert(key_h, PyObject::str_val(CompactString::from(&combined)));
                    }
                }
            } else if let Some(eq_pos) = trimmed.find('=') {
                let key = CompactString::from(trimmed[..eq_pos].trim());
                let val = trimmed[eq_pos + 1..].trim();
                current_items.insert(
                    HashableKey::str_key(key.clone()),
                    PyObject::str_val(CompactString::from(val)),
                );
                last_key = Some(key);
            } else if let Some(col_pos) = trimmed.find(':') {
                let key = CompactString::from(trimmed[..col_pos].trim());
                let val = trimmed[col_pos + 1..].trim();
                current_items.insert(
                    HashableKey::str_key(key.clone()),
                    PyObject::str_val(CompactString::from(val)),
                );
                last_key = Some(key);
            }
        }
        if !current_items.is_empty() {
            sections.insert(
                HashableKey::str_key(current_section),
                PyObject::dict(current_items),
            );
        }
        sections
    }

    /// Perform %(name)s interpolation on a value, resolving from section then defaults.
    fn interpolate_value(
        raw: &str,
        section_items: Option<&FxHashKeyMap>,
        defaults: Option<&FxHashKeyMap>,
        depth: usize,
    ) -> String {
        if depth > 10 {
            return raw.to_string();
        } // guard against infinite recursion
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
                        None => {
                            result.push_str("%(");
                            result.push_str(&var_name);
                            break;
                        }
                    }
                }
                // Skip the format char (usually 's')
                if chars.peek() == Some(&'s') {
                    chars.next();
                }
                // Look up the variable
                let var_key =
                    HashableKey::str_key(CompactString::from(var_name.to_lowercase().as_str()));
                let resolved = section_items
                    .and_then(|s| s.get(&var_key))
                    .or_else(|| defaults.and_then(|d| d.get(&var_key)));
                if let Some(val) = resolved {
                    let val_str = val.py_to_string();
                    result.push_str(&interpolate_value(
                        &val_str,
                        section_items,
                        defaults,
                        depth + 1,
                    ));
                } else {
                    result.push_str(&format!("%({})", var_name));
                }
            } else {
                result.push(ch);
            }
        }
        result
    }

    fn apply_parsed(obj: &PyObjectRef, parsed: FxHashKeyMap) {
        if let Some(secs) = get_sections(obj) {
            let mut w = secs.write();
            for (k, v) in &parsed {
                if k != &HashableKey::str_key(CompactString::from("DEFAULT")) {
                    w.insert(k.clone(), v.clone());
                }
            }
        }
        // Copy DEFAULT section items to _defaults for fallback inheritance
        if let Some(default_dict) =
            parsed.get(&HashableKey::str_key(CompactString::from("DEFAULT")))
        {
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
        if args.len() < 2 {
            return Err(PyException::type_error("add_section requires section name"));
        }
        let section = CompactString::from(args[1].py_to_string());
        if section == "DEFAULT" || section == "default" {
            return Err(PyException::value_error("Invalid section name: 'DEFAULT'"));
        }
        if let Some(secs) = get_sections(&args[0]) {
            let sec_key = HashableKey::str_key(section.clone());
            let mut w = secs.write();
            if w.contains_key(&sec_key) {
                return Err(PyException::runtime_error(format!(
                    "Section '{}' already exists",
                    section
                )));
            }
            w.insert(sec_key, PyObject::dict(IndexMap::new()));
        }
        Ok(PyObject::none())
    }

    fn cp_read(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("read requires filename"));
        }
        let path = args[1].py_to_string();
        let content = std::fs::read_to_string(&path)
            .map_err(|e| PyException::runtime_error(format!("Cannot read {}: {}", path, e)))?;
        let parsed = parse_ini(&content);
        apply_parsed(&args[0], parsed);
        Ok(PyObject::none())
    }

    fn cp_read_string(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Err(PyException::type_error("read_string requires string"));
        }
        let content = args[1].py_to_string();
        let parsed = parse_ini(&content);
        apply_parsed(&args[0], parsed);
        Ok(PyObject::none())
    }

    fn cp_get(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 3 {
            return Err(PyException::type_error("get requires section and option"));
        }
        let section = CompactString::from(args[1].py_to_string());
        let option = args[2].py_to_string().to_lowercase();
        let option_key = HashableKey::str_key(CompactString::from(&option));

        // Check for raw=True kwarg (skip interpolation)
        let raw = if args.len() > 3 {
            if let PyObjectPayload::Dict(kw) = &args[args.len() - 1].payload {
                kw.read()
                    .get(&HashableKey::str_key(CompactString::from("raw")))
                    .map(|v| v.is_truthy())
                    .unwrap_or(false)
            } else {
                false
            }
        } else {
            false
        };

        // Collect section items and defaults for interpolation
        let mut section_items_snap: Option<FxHashKeyMap> = None;
        let mut defaults_snap: Option<FxHashKeyMap> = None;
        let mut raw_val: Option<PyObjectRef> = None;

        // Check section first
        if let Some(secs) = get_sections(&args[0]) {
            let r = secs.read();
            if let Some(sec_dict) = r.get(&HashableKey::str_key(section.clone())) {
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
        Err(PyException::key_error(format!(
            "No option '{}' in section",
            option
        )))
    }

    fn cp_getint(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let val = cp_get(args)?;
        let s = val.py_to_string();
        let n: i64 = s
            .parse()
            .map_err(|_| PyException::value_error(format!("invalid int: {}", s)))?;
        Ok(PyObject::int(n))
    }

    fn cp_getfloat(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let val = cp_get(args)?;
        let s = val.py_to_string();
        let f: f64 = s
            .parse()
            .map_err(|_| PyException::value_error(format!("invalid float: {}", s)))?;
        Ok(PyObject::float(f))
    }

    fn cp_getboolean(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let val = cp_get(args)?;
        let s = val.py_to_string().to_lowercase();
        Ok(PyObject::bool_val(matches!(
            s.as_str(),
            "1" | "yes" | "true" | "on"
        )))
    }

    fn cp_sections(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Ok(PyObject::list(vec![]));
        }
        if let Some(secs) = get_sections(&args[0]) {
            let keys: Vec<PyObjectRef> = secs
                .read()
                .keys()
                .filter_map(|k| {
                    if let HashableKey::Str(s) = k {
                        Some(PyObject::str_val(s.to_compact_string()))
                    } else {
                        None
                    }
                })
                .collect();
            return Ok(PyObject::list(keys));
        }
        Ok(PyObject::list(vec![]))
    }

    fn cp_has_section(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Ok(PyObject::bool_val(false));
        }
        let section = CompactString::from(args[1].py_to_string());
        if let Some(secs) = get_sections(&args[0]) {
            return Ok(PyObject::bool_val(
                secs.read().contains_key(&HashableKey::str_key(section)),
            ));
        }
        Ok(PyObject::bool_val(false))
    }

    fn cp_has_option(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 3 {
            return Ok(PyObject::bool_val(false));
        }
        let section = CompactString::from(args[1].py_to_string());
        let option = CompactString::from(args[2].py_to_string());
        if let Some(secs) = get_sections(&args[0]) {
            let r = secs.read();
            if let Some(sec_dict) = r.get(&HashableKey::str_key(section)) {
                if let PyObjectPayload::Dict(d) = &sec_dict.payload {
                    return Ok(PyObject::bool_val(
                        d.read().contains_key(&HashableKey::str_key(option)),
                    ));
                }
            }
        }
        Ok(PyObject::bool_val(false))
    }

    fn cp_options(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            return Ok(PyObject::list(vec![]));
        }
        let section = CompactString::from(args[1].py_to_string());
        if let Some(secs) = get_sections(&args[0]) {
            let r = secs.read();
            if let Some(sec_dict) = r.get(&HashableKey::str_key(section)) {
                if let PyObjectPayload::Dict(d) = &sec_dict.payload {
                    let keys: Vec<PyObjectRef> = d
                        .read()
                        .keys()
                        .filter_map(|k| {
                            if let HashableKey::Str(s) = k {
                                Some(PyObject::str_val(s.to_compact_string()))
                            } else {
                                None
                            }
                        })
                        .collect();
                    return Ok(PyObject::list(keys));
                }
            }
        }
        Ok(PyObject::list(vec![]))
    }

    fn cp_items(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 2 {
            let mut items = Vec::new();
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                let default_section = inst
                    .attrs
                    .read()
                    .get("default_section")
                    .map(|v| v.py_to_string())
                    .unwrap_or_else(|| "DEFAULT".to_string());
                items.push(PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from(default_section)),
                    cp_getitem(&[
                        args[0].clone(),
                        PyObject::str_val(CompactString::from("DEFAULT")),
                    ])?,
                ]));
            }
            if let Some(secs) = get_sections(&args[0]) {
                for key in secs.read().keys() {
                    if let HashableKey::Str(section) = key {
                        let section_obj = PyObject::str_val(section.to_compact_string());
                        items.push(PyObject::tuple(vec![
                            section_obj.clone(),
                            cp_getitem(&[args[0].clone(), section_obj])?,
                        ]));
                    }
                }
            }
            return Ok(PyObject::list(items));
        }
        let section = CompactString::from(args[1].py_to_string());
        if let Some(secs) = get_sections(&args[0]) {
            let r = secs.read();
            if let Some(sec_dict) = r.get(&HashableKey::str_key(section)) {
                if let PyObjectPayload::Dict(d) = &sec_dict.payload {
                    let items: Vec<PyObjectRef> = d
                        .read()
                        .iter()
                        .map(|(k, v)| {
                            let key = if let HashableKey::Str(s) = k {
                                PyObject::str_val(s.to_compact_string())
                            } else {
                                PyObject::none()
                            };
                            PyObject::tuple(vec![key, v.clone()])
                        })
                        .collect();
                    return Ok(PyObject::list(items));
                }
            }
        }
        Ok(PyObject::list(vec![]))
    }

    fn cp_defaults(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Ok(PyObject::dict(IndexMap::new()));
        }
        if let PyObjectPayload::Instance(inst) = &args[0].payload {
            if let Some(defaults) = inst.attrs.read().get("_defaults") {
                return Ok(defaults.clone());
            }
        }
        Ok(PyObject::dict(IndexMap::new()))
    }

    fn cp_set(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 4 {
            return Err(PyException::type_error(
                "set requires section, option, value",
            ));
        }
        let section = CompactString::from(args[1].py_to_string());
        let option = CompactString::from(args[2].py_to_string().to_lowercase());
        let value = args[3].clone();
        if let Some(secs) = get_sections(&args[0]) {
            let mut w = secs.write();
            let sec_key = HashableKey::str_key(section.clone());
            if !w.contains_key(&sec_key) {
                w.insert(sec_key.clone(), PyObject::dict(IndexMap::new()));
            }
            if let Some(sec_dict) = w.get(&sec_key) {
                if let PyObjectPayload::Dict(d) = &sec_dict.payload {
                    d.write().insert(HashableKey::str_key(option), value);
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
        let defaults_snap: Option<FxHashKeyMap> =
            if let PyObjectPayload::Instance(inst) = &args[0].payload {
                inst.attrs.read().get("_defaults").and_then(|d| {
                    if let PyObjectPayload::Dict(dd) = &d.payload {
                        Some(dd.read().clone())
                    } else {
                        None
                    }
                })
            } else {
                None
            };

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
            let key = HashableKey::str_key(CompactString::from(&section));
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
                                    let interp = interpolate_value(
                                        &val_str,
                                        Some(&section_snap),
                                        Some(defs),
                                        0,
                                    );
                                    w.insert(
                                        k.clone(),
                                        PyObject::str_val(CompactString::from(&interp)),
                                    );
                                } else {
                                    w.insert(k.clone(), v.clone());
                                }
                            }
                        }
                        // Section values override defaults
                        for (k, v) in section_snap.iter() {
                            let val_str = v.py_to_string();
                            if val_str.contains("%(") {
                                let interp = interpolate_value(
                                    &val_str,
                                    Some(&section_snap),
                                    defaults_snap.as_ref(),
                                    0,
                                );
                                w.insert(
                                    k.clone(),
                                    PyObject::str_val(CompactString::from(&interp)),
                                );
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
                            let lk = HashableKey::str_key(CompactString::from(
                                k.to_object().py_to_string().to_lowercase(),
                            ));
                            dst.insert(
                                lk,
                                PyObject::str_val(CompactString::from(v.py_to_string())),
                            );
                        }
                    }
                    w.insert(CompactString::from("_defaults"), defaults);
                }
            }
            return Ok(PyObject::none());
        }
        if let Some(secs) = get_sections(&args[0]) {
            let sec_key = HashableKey::str_key(CompactString::from(&section));
            let sec_dict = PyObject::dict(IndexMap::new());
            // Copy keys from value dict
            if let PyObjectPayload::Dict(src_map) = &value.payload {
                if let PyObjectPayload::Dict(dst_map) = &sec_dict.payload {
                    let src = src_map.read();
                    let mut dst = dst_map.write();
                    for (k, v) in src.iter() {
                        let lk = HashableKey::str_key(CompactString::from(
                            k.to_object().py_to_string().to_lowercase(),
                        ));
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
            let key = HashableKey::str_key(CompactString::from(&section));
            return Ok(PyObject::bool_val(r.contains_key(&key)));
        }
        Ok(PyObject::bool_val(false))
    }

    fn cp_iter(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() {
            return Ok(PyObject::list(vec![]));
        }
        let mut keys = Vec::new();
        if let PyObjectPayload::Instance(inst) = &args[0].payload {
            let default_section = inst
                .attrs
                .read()
                .get("default_section")
                .map(|v| v.py_to_string())
                .unwrap_or_else(|| "DEFAULT".to_string());
            keys.push(PyObject::str_val(CompactString::from(default_section)));
        }
        if let Some(secs) = get_sections(&args[0]) {
            for key in secs.read().keys() {
                if let HashableKey::Str(section) = key {
                    keys.push(PyObject::str_val(section.to_compact_string()));
                }
            }
        }
        Ok(PyObject::list(keys))
    }

    fn cp_len(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        let section_count = get_sections(&args[0])
            .map(|sections| sections.read().len())
            .unwrap_or(0);
        Ok(PyObject::int(section_count as i64 + 1))
    }

    // remove_section
    fn cp_remove_section(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        check_args_min("remove_section", args, 2)?;
        let section = args[1].py_to_string();
        if let Some(secs) = get_sections(&args[0]) {
            let key = HashableKey::str_key(CompactString::from(&section));
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
            let key = HashableKey::str_key(CompactString::from(&section));
            if let Some(sec_dict) = r.get(&key) {
                if let PyObjectPayload::Dict(d) = &sec_dict.payload {
                    let opt_key = HashableKey::str_key(CompactString::from(&option));
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
                            output.push_str(&format!(
                                "{} = {}\n",
                                k.to_object().py_to_string(),
                                v.py_to_string()
                            ));
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
                        output.push_str(&format!(
                            "{} = {}\n",
                            k.to_object().py_to_string(),
                            v.py_to_string()
                        ));
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
                PyObjectPayload::NativeClosure(nc) => {
                    (nc.func)(&[text])?;
                }
                PyObjectPayload::NativeFunction(nf) => {
                    (nf.func)(&[text])?;
                }
                _ => {
                    // For bound methods or other callables, we can't invoke from stdlib.
                    // Fallback: write directly to StringIO buffer if possible
                    if let PyObjectPayload::Instance(inst) = &file_obj.payload {
                        if inst.attrs.read().contains_key("__stringio__") {
                            if let Some(w) = inst.attrs.read().get("write") {
                                if let PyObjectPayload::NativeClosure(nc) = &w.payload {
                                    let text2 =
                                        PyObject::str_val(CompactString::from(output.as_str()));
                                    (nc.func)(&[text2])?;
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(PyObject::none())
    }

    make_module(
        "configparser",
        vec![
            ("ConfigParser", configparser_class.clone()),
            ("RawConfigParser", configparser_class.clone()),
            ("SafeConfigParser", configparser_class),
            (
                "DEFAULTSECT",
                PyObject::str_val(CompactString::from("DEFAULT")),
            ),
            ("MAX_INTERPOLATION_DEPTH", PyObject::int(10)),
            ("_UNSET", unset),
            (
                "_default_dict",
                PyObject::builtin_type(CompactString::from("dict")),
            ),
            ("BasicInterpolation", basic_interpolation),
            ("ExtendedInterpolation", extended_interpolation),
            ("LegacyInterpolation", legacy_interpolation),
            (
                "Interpolation",
                PyObject::exception_type(ExceptionKind::ValueError),
            ),
            ("Error", PyObject::exception_type(ExceptionKind::Exception)),
            (
                "NoSectionError",
                PyObject::exception_type(ExceptionKind::KeyError),
            ),
            (
                "NoOptionError",
                PyObject::exception_type(ExceptionKind::KeyError),
            ),
            (
                "DuplicateSectionError",
                PyObject::exception_type(ExceptionKind::ValueError),
            ),
            (
                "DuplicateOptionError",
                PyObject::exception_type(ExceptionKind::ValueError),
            ),
            (
                "ParsingError",
                PyObject::exception_type(ExceptionKind::ValueError),
            ),
            (
                "MissingSectionHeaderError",
                PyObject::exception_type(ExceptionKind::ValueError),
            ),
            (
                "InterpolationError",
                PyObject::exception_type(ExceptionKind::ValueError),
            ),
            (
                "InterpolationMissingOptionError",
                PyObject::exception_type(ExceptionKind::ValueError),
            ),
            (
                "InterpolationSyntaxError",
                PyObject::exception_type(ExceptionKind::ValueError),
            ),
            (
                "InterpolationDepthError",
                PyObject::exception_type(ExceptionKind::ValueError),
            ),
            ("BOOLEAN_STATES", boolean_states_dict()),
            ("__all__", configparser_all()),
        ],
    )
}

fn default_converters_dict() -> PyObjectRef {
    let mut map = IndexMap::new();
    for name in ["boolean", "float", "int"] {
        map.insert(
            HashableKey::str_key(CompactString::from(name)),
            PyObject::none(),
        );
    }
    PyObject::dict(map)
}

fn boolean_states_dict() -> PyObjectRef {
    let mut map = IndexMap::new();
    for (name, value) in [
        ("1", true),
        ("yes", true),
        ("true", true),
        ("on", true),
        ("0", false),
        ("no", false),
        ("false", false),
        ("off", false),
    ] {
        map.insert(
            HashableKey::str_key(CompactString::from(name)),
            PyObject::bool_val(value),
        );
    }
    PyObject::dict(map)
}

fn configparser_all() -> PyObjectRef {
    PyObject::list(
        [
            "Error",
            "NoSectionError",
            "DuplicateSectionError",
            "DuplicateOptionError",
            "NoOptionError",
            "InterpolationError",
            "InterpolationDepthError",
            "InterpolationMissingOptionError",
            "InterpolationSyntaxError",
            "ParsingError",
            "MissingSectionHeaderError",
            "ConfigParser",
            "SafeConfigParser",
            "RawConfigParser",
            "DEFAULTSECT",
            "MAX_INTERPOLATION_DEPTH",
            "BasicInterpolation",
            "ExtendedInterpolation",
            "LegacyInterpolation",
        ]
        .into_iter()
        .map(|name| PyObject::str_val(CompactString::from(name)))
        .collect(),
    )
}
