use super::*;

// ── html.parser module ──

pub fn create_html_parser_module() -> PyObjectRef {
    // Build HTMLParser as a proper Class so subclasses inherit methods via MRO.
    let mut ns = IndexMap::new();

    // __init__: set up per-instance state
    ns.insert(
        CompactString::from("__init__"),
        make_builtin(|args: &[PyObjectRef]| {
            // args[0] is self
            if !args.is_empty() {
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    let mut w = inst.attrs.write();
                    w.insert(
                        CompactString::from("_data_buf"),
                        PyObject::str_val(CompactString::from("")),
                    );
                    w.insert(
                        CompactString::from("_pos"),
                        PyObject::tuple(vec![PyObject::int(1), PyObject::int(0)]),
                    );
                }
            }
            Ok(PyObject::none())
        }),
    );

    // feed(self, data): parse HTML data and invoke callbacks
    ns.insert(
        CompactString::from("feed"),
        make_builtin(|args: &[PyObjectRef]| {
            check_args_min("HTMLParser.feed", args, 2)?;
            let _self_obj = &args[0];
            let data = args[1].py_to_string();

            // Store raw data
            if let PyObjectPayload::Instance(ref inst) = _self_obj.payload {
                let existing = inst
                    .attrs
                    .read()
                    .get("_data_buf")
                    .cloned()
                    .map(|v| v.py_to_string())
                    .unwrap_or_default();
                inst.attrs.write().insert(
                    CompactString::from("_data_buf"),
                    PyObject::str_val(CompactString::from(format!("{}{}", existing, data))),
                );
            }

            // Simple HTML tag parsing — extract tags and invoke callbacks
            // Store callback requests as a list for the VM to process
            let mut pending = Vec::new();
            let chars: Vec<char> = data.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                if chars[i] == '<' {
                    // Find closing >
                    if let Some(end) = chars[i..].iter().position(|&c| c == '>') {
                        let tag_content: String = chars[i + 1..i + end].iter().collect();
                        let tag_content = tag_content.trim().to_string();
                        if tag_content.starts_with('/') {
                            // End tag
                            let tag_name = tag_content[1..].trim().to_lowercase();
                            pending.push(("handle_endtag", tag_name, Vec::new()));
                        } else if tag_content.starts_with('!') {
                            // Comment or declaration
                            if tag_content.starts_with("!--") {
                                let comment = tag_content
                                    .strip_prefix("!--")
                                    .unwrap_or("")
                                    .strip_suffix("--")
                                    .unwrap_or(&tag_content[3..])
                                    .to_string();
                                pending.push(("handle_comment", comment, Vec::new()));
                            } else {
                                let decl = tag_content[1..].to_string();
                                pending.push(("handle_decl", decl, Vec::new()));
                            }
                        } else {
                            // Start tag: parse name and attributes
                            let parts: Vec<&str> =
                                tag_content.splitn(2, char::is_whitespace).collect();
                            let tag_name = parts[0].trim_end_matches('/').to_lowercase();
                            let mut attrs = Vec::new();
                            if parts.len() > 1 {
                                // Simple attribute parsing
                                let attr_str = parts[1].trim_end_matches('/');
                                for attr in attr_str.split_whitespace() {
                                    if let Some(eq_pos) = attr.find('=') {
                                        let k = &attr[..eq_pos];
                                        let v =
                                            attr[eq_pos + 1..].trim_matches('"').trim_matches('\'');
                                        attrs.push((k.to_string(), v.to_string()));
                                    } else {
                                        attrs.push((attr.to_string(), String::new()));
                                    }
                                }
                            }
                            if tag_content.ends_with('/') {
                                // Self-closing: check if subclass overrides handle_startendtag
                                // If not, fall back to handle_starttag + handle_endtag (CPython behavior)
                                pending.push(("handle_startendtag_or_split", tag_name, attrs));
                            } else {
                                pending.push(("handle_starttag", tag_name, attrs));
                            }
                        }
                        i += end + 1;
                    } else {
                        i += 1;
                    }
                } else if chars[i] == '&' {
                    // Entity or character reference
                    if let Some(semi) = chars[i..].iter().position(|&c| c == ';') {
                        let ref_content: String = chars[i + 1..i + semi].iter().collect();
                        if ref_content.starts_with('#') {
                            // Character reference: &#65; or &#x41;
                            pending.push((
                                "handle_charref",
                                ref_content[1..].to_string(),
                                Vec::new(),
                            ));
                        } else {
                            // Named entity: &amp; etc.
                            pending.push(("handle_entityref", ref_content.clone(), Vec::new()));
                        }
                        i += semi + 1;
                    } else {
                        // No semicolon found, treat as text
                        pending.push(("handle_data", "&".to_string(), Vec::new()));
                        i += 1;
                    }
                } else {
                    // Text data
                    let start = i;
                    while i < chars.len() && chars[i] != '<' && chars[i] != '&' {
                        i += 1;
                    }
                    let text: String = chars[start..i].iter().collect();
                    if !text.is_empty() {
                        pending.push(("handle_data", text, Vec::new()));
                    }
                }
            }

            // Queue callbacks via pending_vm_call mechanism
            // Since we can't call Python methods from a NativeClosure, store them for the VM
            if let PyObjectPayload::Instance(ref inst) = _self_obj.payload {
                let mut callback_list = Vec::new();

                // Helper: find method in instance attrs first, then class (MRO)
                let find_method = |name: &str| -> Option<(PyObjectRef, bool)> {
                    // Instance attrs (user-bound methods)
                    if let Some(m) = inst.attrs.read().get(&CompactString::from(name)).cloned() {
                        return Some((m, false)); // false = no self prepend
                    }
                    // Class namespace (inherited)
                    if let PyObjectPayload::Class(cd) = &inst.class.payload {
                        if let Some(m) =
                            cd.namespace.read().get(&CompactString::from(name)).cloned()
                        {
                            return Some((m, true)); // true = needs self prepend
                        }
                    }
                    None
                };

                let make_attr_list = |attrs: &[(String, String)]| -> PyObjectRef {
                    let items: Vec<PyObjectRef> = attrs
                        .iter()
                        .map(|(k, v)| {
                            PyObject::tuple(vec![
                                PyObject::str_val(CompactString::from(k.as_str())),
                                PyObject::str_val(CompactString::from(v.as_str())),
                            ])
                        })
                        .collect();
                    PyObject::list(items)
                };

                for (method_name, arg, attrs) in &pending {
                    if *method_name == "handle_startendtag_or_split" {
                        // Check if subclass overrides handle_startendtag
                        let has_override = if let Some(_m) = inst
                            .attrs
                            .read()
                            .get(&CompactString::from("handle_startendtag"))
                            .cloned()
                        {
                            true
                        } else {
                            // Check if class override differs from HTMLParser base
                            false
                        };
                        if has_override {
                            if let Some((m, needs_self)) = find_method("handle_startendtag") {
                                let mut call_args = if needs_self {
                                    vec![_self_obj.clone()]
                                } else {
                                    vec![]
                                };
                                call_args
                                    .push(PyObject::str_val(CompactString::from(arg.as_str())));
                                call_args.push(make_attr_list(attrs));
                                callback_list.push((m, call_args));
                            }
                        } else {
                            // Split into handle_starttag + handle_endtag
                            if let Some((m, needs_self)) = find_method("handle_starttag") {
                                let mut call_args = if needs_self {
                                    vec![_self_obj.clone()]
                                } else {
                                    vec![]
                                };
                                call_args
                                    .push(PyObject::str_val(CompactString::from(arg.as_str())));
                                call_args.push(make_attr_list(attrs));
                                callback_list.push((m, call_args));
                            }
                            if let Some((m, needs_self)) = find_method("handle_endtag") {
                                let mut call_args = if needs_self {
                                    vec![_self_obj.clone()]
                                } else {
                                    vec![]
                                };
                                call_args
                                    .push(PyObject::str_val(CompactString::from(arg.as_str())));
                                callback_list.push((m, call_args));
                            }
                        }
                        continue;
                    }

                    let is_tag_method =
                        *method_name == "handle_starttag" || *method_name == "handle_startendtag";

                    if let Some((m, needs_self)) = find_method(method_name) {
                        let mut call_args = if needs_self {
                            vec![_self_obj.clone()]
                        } else {
                            vec![]
                        };
                        call_args.push(PyObject::str_val(CompactString::from(arg.as_str())));
                        if is_tag_method {
                            call_args.push(make_attr_list(attrs));
                        }
                        callback_list.push((m, call_args));
                    }
                }
                // Store callbacks for the VM to process
                for (func, call_args) in callback_list {
                    crate::concurrency_modules::push_deferred_call(func, call_args);
                }
            }

            Ok(PyObject::none())
        }),
    );

    // close(self)
    ns.insert(
        CompactString::from("close"),
        make_builtin(|args: &[PyObjectRef]| {
            if !args.is_empty() {
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    inst.attrs.write().insert(
                        CompactString::from("_data_buf"),
                        PyObject::str_val(CompactString::from("")),
                    );
                }
            }
            Ok(PyObject::none())
        }),
    );

    // reset(self)
    ns.insert(
        CompactString::from("reset"),
        make_builtin(|args: &[PyObjectRef]| {
            if !args.is_empty() {
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    inst.attrs.write().insert(
                        CompactString::from("_data_buf"),
                        PyObject::str_val(CompactString::from("")),
                    );
                }
            }
            Ok(PyObject::none())
        }),
    );

    // getpos(self)
    ns.insert(
        CompactString::from("getpos"),
        make_builtin(|args: &[PyObjectRef]| {
            if !args.is_empty() {
                if let PyObjectPayload::Instance(ref inst) = args[0].payload {
                    if let Some(pos) = inst.attrs.read().get("_pos").cloned() {
                        return Ok(pos);
                    }
                }
            }
            Ok(PyObject::tuple(vec![PyObject::int(1), PyObject::int(0)]))
        }),
    );

    // Callback stubs (no-ops by default, subclasses override)
    for name in &[
        "handle_starttag",
        "handle_endtag",
        "handle_data",
        "handle_comment",
        "handle_decl",
        "handle_pi",
        "handle_entityref",
        "handle_charref",
    ] {
        ns.insert(
            CompactString::from(*name),
            make_builtin(|_args: &[PyObjectRef]| Ok(PyObject::none())),
        );
    }

    // handle_startendtag default: calls handle_starttag + handle_endtag (CPython behavior)
    ns.insert(
        CompactString::from("handle_startendtag"),
        make_builtin(|_args: &[PyObjectRef]| {
            // Default: just no-op. The real delegation happens in the feed loop
            // where we check for user-override of handle_startendtag and fall back
            // to handle_starttag + handle_endtag if not overridden.
            Ok(PyObject::none())
        }),
    );

    let html_parser_class = PyObject::class(CompactString::from("HTMLParser"), vec![], ns);

    make_module("html.parser", vec![("HTMLParser", html_parser_class)])
}
