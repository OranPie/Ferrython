use compact_str::CompactString;
use ferrython_core::object::{
    make_builtin, make_module, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use indexmap::IndexMap;

// ── cmd module ──

pub fn create_cmd_module() -> PyObjectRef {
    // Create Cmd as a proper class so it can be subclassed
    let cmd_cls = PyObject::class(CompactString::from("Cmd"), vec![], IndexMap::new());
    if let PyObjectPayload::Class(ref cd) = cmd_cls.payload {
        let mut ns = cd.namespace.write();
        ns.insert(
            CompactString::from("prompt"),
            PyObject::str_val(CompactString::from("(Cmd) ")),
        );
        ns.insert(CompactString::from("intro"), PyObject::none());
        ns.insert(
            CompactString::from("identchars"),
            PyObject::str_val(CompactString::from(
                "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_",
            )),
        );
        ns.insert(
            CompactString::from("ruler"),
            PyObject::str_val(CompactString::from("=")),
        );
        ns.insert(
            CompactString::from("lastcmd"),
            PyObject::str_val(CompactString::from("")),
        );
        ns.insert(
            CompactString::from("doc_leader"),
            PyObject::str_val(CompactString::from("")),
        );
        ns.insert(
            CompactString::from("doc_header"),
            PyObject::str_val(CompactString::from(
                "Documented commands (type help <topic>):",
            )),
        );
        ns.insert(
            CompactString::from("undoc_header"),
            PyObject::str_val(CompactString::from("Undocumented commands:")),
        );
        ns.insert(
            CompactString::from("misc_header"),
            PyObject::str_val(CompactString::from("Miscellaneous help topics:")),
        );
        ns.insert(
            CompactString::from("nohelp"),
            PyObject::str_val(CompactString::from("*** No help on %s")),
        );
        ns.insert(
            CompactString::from("use_rawinput"),
            PyObject::bool_val(true),
        );

        ns.insert(
            CompactString::from("cmdloop"),
            make_builtin(|args: &[PyObjectRef]| {
                // Basic cmdloop: read lines from stdin and dispatch
                let inst = if !args.is_empty() {
                    args[0].clone()
                } else {
                    return Ok(PyObject::none());
                };
                let prompt_attr = inst
                    .get_attr("prompt")
                    .map(|p| p.py_to_string())
                    .unwrap_or_else(|| "(Cmd) ".to_string());
                let intro = inst.get_attr("intro");

                if let Some(ref intro_obj) = intro {
                    if !matches!(&intro_obj.payload, PyObjectPayload::None) {
                        println!("{}", intro_obj.py_to_string());
                    }
                }

                loop {
                    eprint!("{}", prompt_attr);
                    let mut line = String::new();
                    match std::io::stdin().read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        _ => {}
                    }
                    let line = line.trim_end_matches('\n').trim_end_matches('\r');
                    if line == "EOF"
                        || line.is_empty()
                            && std::io::stdin().read_line(&mut String::new()).unwrap_or(0) == 0
                    {
                        break;
                    }

                    // Dispatch via onecmd
                    if let Some(onecmd_fn) = inst.get_attr("onecmd") {
                        match &onecmd_fn.payload {
                            PyObjectPayload::NativeFunction(nf) => {
                                match (nf.func)(&[PyObject::str_val(CompactString::from(line))]) {
                                    Ok(result) => {
                                        if result.is_truthy() {
                                            break;
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("{}", e.message);
                                    }
                                }
                            }
                            PyObjectPayload::NativeClosure(nc) => {
                                match (nc.func)(&[PyObject::str_val(CompactString::from(line))]) {
                                    Ok(result) => {
                                        if result.is_truthy() {
                                            break;
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("{}", e.message);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Ok(PyObject::none())
            }),
        );

        ns.insert(
            CompactString::from("parseline"),
            make_builtin(|args: &[PyObjectRef]| {
                let line = if args.len() > 1 {
                    args[1].py_to_string()
                } else if !args.is_empty() {
                    args[0].py_to_string()
                } else {
                    return Ok(PyObject::tuple(vec![
                        PyObject::none(),
                        PyObject::none(),
                        PyObject::str_val(CompactString::from("")),
                    ]));
                };
                let line = line.trim().to_string();
                if line.is_empty() {
                    return Ok(PyObject::tuple(vec![
                        PyObject::none(),
                        PyObject::none(),
                        PyObject::str_val(CompactString::from(line)),
                    ]));
                }
                // Find the command word
                let identchars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";
                let cmd_end = line
                    .find(|c: char| !identchars.contains(c))
                    .unwrap_or(line.len());
                let cmd = &line[..cmd_end];
                let rest = line[cmd_end..].trim_start();
                Ok(PyObject::tuple(vec![
                    PyObject::str_val(CompactString::from(cmd)),
                    PyObject::str_val(CompactString::from(rest)),
                    PyObject::str_val(CompactString::from(line)),
                ]))
            }),
        );

        ns.insert(
            CompactString::from("onecmd"),
            make_builtin(|args: &[PyObjectRef]| {
                let line = if args.len() > 1 {
                    args[1].py_to_string()
                } else if !args.is_empty() {
                    args[0].py_to_string()
                } else {
                    return Ok(PyObject::bool_val(false));
                };
                if line.trim().is_empty() {
                    return Ok(PyObject::bool_val(false));
                }
                // Parse cmd + args
                let trimmed = line.trim();
                let identchars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";
                let cmd_end = trimmed
                    .find(|c: char| !identchars.contains(c))
                    .unwrap_or(trimmed.len());
                let cmd = &trimmed[..cmd_end];
                let rest = trimmed[cmd_end..].trim_start();
                // Look for do_<cmd> method
                let self_obj = if args.len() > 1 {
                    &args[0]
                } else {
                    return Ok(PyObject::bool_val(false));
                };
                let method_name = format!("do_{}", cmd);
                if let Some(method) = self_obj.get_attr(&method_name) {
                    match &method.payload {
                        PyObjectPayload::NativeFunction(nf) => {
                            return (nf.func)(&[PyObject::str_val(CompactString::from(rest))]);
                        }
                        PyObjectPayload::NativeClosure(nc) => {
                            return (nc.func)(&[PyObject::str_val(CompactString::from(rest))]);
                        }
                        _ => {
                            ferrython_core::error::request_vm_call(
                                method.clone(),
                                vec![PyObject::str_val(CompactString::from(rest))],
                            );
                            return Ok(PyObject::bool_val(false));
                        }
                    }
                }
                eprintln!("*** Unknown syntax: {}", trimmed);
                Ok(PyObject::bool_val(false))
            }),
        );

        ns.insert(
            CompactString::from("precmd"),
            make_builtin(|args: &[PyObjectRef]| {
                // Default: return line unchanged
                if args.len() > 1 {
                    Ok(args[1].clone())
                } else if !args.is_empty() {
                    Ok(args[0].clone())
                } else {
                    Ok(PyObject::str_val(CompactString::from("")))
                }
            }),
        );

        ns.insert(
            CompactString::from("postcmd"),
            make_builtin(|args: &[PyObjectRef]| {
                // Default: return stop flag unchanged
                if args.len() > 1 {
                    Ok(args[1].clone())
                } else if !args.is_empty() {
                    Ok(args[0].clone())
                } else {
                    Ok(PyObject::bool_val(false))
                }
            }),
        );

        ns.insert(
            CompactString::from("preloop"),
            make_builtin(|_| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("postloop"),
            make_builtin(|_| Ok(PyObject::none())),
        );
        ns.insert(
            CompactString::from("emptyline"),
            make_builtin(|_| Ok(PyObject::none())),
        );

        ns.insert(
            CompactString::from("default"),
            make_builtin(|args: &[PyObjectRef]| {
                let line = if args.len() > 1 {
                    args[1].py_to_string()
                } else if !args.is_empty() {
                    args[0].py_to_string()
                } else {
                    String::new()
                };
                eprintln!("*** Unknown syntax: {}", line);
                Ok(PyObject::none())
            }),
        );

        ns.insert(
            CompactString::from("columnize"),
            make_builtin(|args: &[PyObjectRef]| {
                let list = if args.len() > 1 {
                    &args[1]
                } else if !args.is_empty() {
                    &args[0]
                } else {
                    return Ok(PyObject::none());
                };
                if let PyObjectPayload::List(ref items) = list.payload {
                    let items_r = items.read();
                    let strs: Vec<String> = items_r.iter().map(|i| i.py_to_string()).collect();
                    println!("{}", strs.join("  "));
                }
                Ok(PyObject::none())
            }),
        );
    }

    make_module("cmd", vec![("Cmd", cmd_cls)])
}
