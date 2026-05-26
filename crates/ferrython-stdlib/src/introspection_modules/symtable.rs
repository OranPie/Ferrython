use super::*;

/// Helper: create a simple namespace-like instance with the given attrs.
fn make_ns(cls_name: &str, attrs: IndexMap<CompactString, PyObjectRef>) -> PyObjectRef {
    let cls = PyObject::class(CompactString::from(cls_name), vec![], IndexMap::new());
    let class_flags = InstanceData::compute_flags(&cls);
    PyObject::wrap(PyObjectPayload::Instance(std::mem::ManuallyDrop::new(
        Box::new(InstanceData {
            class: cls,
            attrs: to_shared_fx(attrs),
            is_special: true,
            dict_storage: None,
            class_flags,
            finalizer_state: std::cell::Cell::new(0),
        }),
    )))
}

// ── symtable module ──

pub fn create_symtable_module() -> PyObjectRef {
    fn symtable_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        check_args_min("symtable", args, 2)?;
        let source = args[0].py_to_string();
        let filename = args[1].py_to_string();
        let kind = if args.len() > 2 {
            args[2].py_to_string()
        } else {
            "exec".to_string()
        };

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
                    if !globals.contains(&name) {
                        globals.push(name);
                    }
                }
            }
            if trimmed.starts_with("def ") || trimmed.starts_with("class ") {
                let rest = if trimmed.starts_with("def ") {
                    &trimmed[4..]
                } else {
                    &trimmed[6..]
                };
                if let Some(name_end) = rest.find(|c: char| !c.is_alphanumeric() && c != '_') {
                    let name = rest[..name_end].to_string();
                    if !locals.contains(&name) {
                        locals.push(name);
                    }
                }
            }
        }

        for name in &locals {
            let mut ns = IndexMap::new();
            ns.insert(
                CompactString::from("name"),
                PyObject::str_val(CompactString::from(name.as_str())),
            );
            ns.insert(CompactString::from("is_local"), PyObject::bool_val(true));
            ns.insert(
                CompactString::from("is_global"),
                PyObject::bool_val(globals.contains(name)),
            );
            ns.insert(
                CompactString::from("is_referenced"),
                PyObject::bool_val(true),
            );
            ns.insert(
                CompactString::from("is_imported"),
                PyObject::bool_val(false),
            );
            ns.insert(
                CompactString::from("is_parameter"),
                PyObject::bool_val(false),
            );
            ns.insert(CompactString::from("is_free"), PyObject::bool_val(false));
            ns.insert(CompactString::from("is_assigned"), PyObject::bool_val(true));
            ns.insert(
                CompactString::from("is_namespace"),
                PyObject::bool_val(false),
            );
            symbols.insert(CompactString::from(name.as_str()), make_ns("Symbol", ns));
        }

        let mut st_ns = IndexMap::new();
        st_ns.insert(
            CompactString::from("name"),
            PyObject::str_val(CompactString::from("top")),
        );
        st_ns.insert(
            CompactString::from("type"),
            PyObject::str_val(CompactString::from(kind.as_str())),
        );
        st_ns.insert(
            CompactString::from("filename"),
            PyObject::str_val(CompactString::from(filename.as_str())),
        );
        st_ns.insert(
            CompactString::from("get_symbols"),
            PyObject::native_closure("SymbolTable.get_symbols", {
                let syms = symbols.clone();
                move |_: &[PyObjectRef]| Ok(PyObject::list(syms.values().cloned().collect()))
            }),
        );
        st_ns.insert(
            CompactString::from("lookup"),
            PyObject::native_closure("SymbolTable.lookup", {
                let syms = symbols.clone();
                move |args: &[PyObjectRef]| {
                    check_args("lookup", args, 1)?;
                    let name = args[0].py_to_string();
                    match syms.get(name.as_str()) {
                        Some(sym) => Ok(sym.clone()),
                        None => Err(PyException::key_error(format!(
                            "symbol '{}' not found",
                            name
                        ))),
                    }
                }
            }),
        );
        st_ns.insert(
            CompactString::from("get_identifiers"),
            PyObject::native_closure("SymbolTable.get_identifiers", {
                let syms = symbols;
                move |_: &[PyObjectRef]| {
                    Ok(PyObject::list(
                        syms.keys().map(|k| PyObject::str_val(k.clone())).collect(),
                    ))
                }
            }),
        );
        st_ns.insert(
            CompactString::from("has_children"),
            PyObject::bool_val(false),
        );
        st_ns.insert(
            CompactString::from("get_children"),
            make_builtin(|_: &[PyObjectRef]| Ok(PyObject::list(vec![]))),
        );
        Ok(make_ns("SymbolTable", st_ns))
    }

    fn symbol_fn(args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        check_args("Symbol", args, 1)?;
        let name = args[0].py_to_string();
        let mut ns = IndexMap::new();
        ns.insert(
            CompactString::from("name"),
            PyObject::str_val(CompactString::from(name.as_str())),
        );
        ns.insert(CompactString::from("is_local"), PyObject::bool_val(false));
        ns.insert(CompactString::from("is_global"), PyObject::bool_val(false));
        ns.insert(
            CompactString::from("is_referenced"),
            PyObject::bool_val(false),
        );
        ns.insert(
            CompactString::from("is_imported"),
            PyObject::bool_val(false),
        );
        ns.insert(
            CompactString::from("is_parameter"),
            PyObject::bool_val(false),
        );
        ns.insert(CompactString::from("is_free"), PyObject::bool_val(false));
        ns.insert(
            CompactString::from("is_assigned"),
            PyObject::bool_val(false),
        );
        ns.insert(
            CompactString::from("is_namespace"),
            PyObject::bool_val(false),
        );
        Ok(make_ns("Symbol", ns))
    }

    make_module(
        "symtable",
        vec![
            ("symtable", make_builtin(symtable_fn)),
            ("Symbol", make_builtin(symbol_fn)),
            ("DEF_GLOBAL", PyObject::int(1)),
            ("DEF_LOCAL", PyObject::int(2)),
            ("DEF_PARAM", PyObject::int(4)),
            ("DEF_IMPORT", PyObject::int(8)),
            ("DEF_FREE", PyObject::int(16)),
            ("DEF_FREE_CLASS", PyObject::int(32)),
            ("DEF_BOUND", PyObject::int(64)),
        ],
    )
}
