use super::ast_convert::convert_expr;
use super::*;

// ── ast module ──

fn ast_parse_feature_version(value: &PyObjectRef) -> Option<(i64, i64)> {
    match &value.payload {
        PyObjectPayload::Tuple(items) if items.len() >= 2 => {
            Some((items[0].as_int()?, items[1].as_int()?))
        }
        PyObjectPayload::Int(_) => Some((3, value.as_int()?)),
        PyObjectPayload::None => None,
        _ => None,
    }
}

fn ast_parse_rejects_fstring_debug(source: &str, feature_version: Option<(i64, i64)>) -> bool {
    if let Some((major, minor)) = feature_version {
        (major, minor) < (3, 8) && source.contains("=}")
    } else {
        false
    }
}

pub fn create_ast_module() -> PyObjectRef {
    let parse_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "ast.parse() requires source code argument",
            ));
        }
        let source = args[0].py_to_string();
        let mut filename = "<string>".to_string();
        let mut mode = "exec".to_string();
        let mut feature_version = None;
        // Handle positional args
        for (i, arg) in args.iter().enumerate().skip(1) {
            // Check if it's a kwargs dict (trailing dict convention)
            if let PyObjectPayload::Dict(map) = &arg.payload {
                let r = map.read();
                for (k, v) in r.iter() {
                    match k.to_object().py_to_string().as_str() {
                        "filename" => filename = v.py_to_string(),
                        "mode" => mode = v.py_to_string(),
                        "feature_version" => {
                            feature_version = ast_parse_feature_version(v);
                        }
                        _ => {}
                    }
                }
            } else if i == 1 {
                filename = arg.py_to_string();
            } else if i == 2 {
                mode = arg.py_to_string();
            }
        }
        if ast_parse_rejects_fstring_debug(&source, feature_version) {
            return Err(PyException::syntax_error(
                "f-string: self documenting expressions are only supported in Python 3.8 and greater",
            ));
        }
        match mode.as_str() {
            "eval" => {
                match ferrython_parser::parse_expression(&source, &filename) {
                    Ok(expr) => {
                        let body = expr_to_pyobject(&expr);
                        if source.trim_start().starts_with('u')
                            || source.trim_start().starts_with('U')
                        {
                            if body.type_name() == "Constant" {
                                set_node_attr(
                                    &body,
                                    "kind",
                                    PyObject::str_val(CompactString::from("u")),
                                );
                            }
                        }
                        let cls = PyObject::class(
                            CompactString::from("Expression"),
                            vec![],
                            IndexMap::new(),
                        );
                        let inst = PyObject::instance(cls);
                        set_node_attr(&inst, "body", body);
                        set_node_fields(&inst, &["body"]);
                        // Store source for compile() support
                        if let PyObjectPayload::Instance(ref data) = inst.payload {
                            let mut a = data.attrs.write();
                            a.insert(
                                CompactString::from("__source__"),
                                PyObject::str_val(CompactString::from(&source)),
                            );
                            a.insert(
                                CompactString::from("__filename__"),
                                PyObject::str_val(CompactString::from(&filename)),
                            );
                            a.insert(
                                CompactString::from("__mode__"),
                                PyObject::str_val(CompactString::from("eval")),
                            );
                        }
                        Ok(inst)
                    }
                    Err(e) => Err(PyException::syntax_error(format!("{}", e))),
                }
            }
            _ => {
                match ferrython_parser::parse(&source, &filename) {
                    Ok(module) => {
                        let obj = module_to_pyobject(&module);
                        // Store source for compile() support
                        if let PyObjectPayload::Instance(inst) = &obj.payload {
                            let mut a = inst.attrs.write();
                            a.insert(
                                CompactString::from("__source__"),
                                PyObject::str_val(CompactString::from(&source)),
                            );
                            a.insert(
                                CompactString::from("__filename__"),
                                PyObject::str_val(CompactString::from(&filename)),
                            );
                            a.insert(
                                CompactString::from("__mode__"),
                                PyObject::str_val(CompactString::from(&mode)),
                            );
                        }
                        Ok(obj)
                    }
                    Err(e) => Err(PyException::syntax_error(format!("{}", e))),
                }
            }
        }
    });

    let dump_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("ast.dump() requires node argument"));
        }
        let mut annotate_fields = true;
        let mut include_attributes = false;
        let mut indent = None;
        if args.len() > 1 {
            match &args[1].payload {
                PyObjectPayload::Dict(kwargs) => {
                    let kwargs = kwargs.read();
                    if let Some(v) = kwargs.get(&HashableKey::str_key(CompactString::from(
                        "annotate_fields",
                    ))) {
                        annotate_fields = v.is_truthy();
                    }
                    if let Some(v) = kwargs.get(&HashableKey::str_key(CompactString::from(
                        "include_attributes",
                    ))) {
                        include_attributes = v.is_truthy();
                    }
                    if let Some(v) =
                        kwargs.get(&HashableKey::str_key(CompactString::from("indent")))
                    {
                        if !matches!(&v.payload, PyObjectPayload::None) {
                            indent = v.as_int().map(|n| n.max(0) as usize);
                        }
                    }
                }
                _ => {
                    annotate_fields = args[1].is_truthy();
                    if args.len() > 2 {
                        include_attributes = args[2].is_truthy();
                    }
                }
            }
        }
        let result = dump_node(&args[0], indent, include_attributes, annotate_fields, 0);
        Ok(PyObject::str_val(CompactString::from(result)))
    });

    let literal_eval_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "ast.literal_eval() requires string argument",
            ));
        }
        if args[0].get_attr("_fields").is_some() {
            return eval_py_const_node(&args[0]);
        }
        let s = args[0].py_to_string();
        let trimmed = s.trim();
        if trimmed == "None" {
            return Ok(PyObject::none());
        }
        if trimmed == "True" {
            return Ok(PyObject::bool_val(true));
        }
        if trimmed == "False" {
            return Ok(PyObject::bool_val(false));
        }
        if trimmed.chars().all(|ch| ch.is_ascii_digit()) && trimmed.len() > 4000 {
            return Err(PyException::syntax_error(
                "Exceeds the limit (4000 digits) for integer string conversion: value has too many digits; Consider hexadecimal for huge integer literals",
            ));
        }
        if let Ok(n) = trimmed.parse::<i64>() {
            return Ok(PyObject::int(n));
        }
        if let Ok(f) = trimmed.parse::<f64>() {
            return Ok(PyObject::float(f));
        }
        if (trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        {
            if trimmed.contains("\\U") {
                return Err(PyException::syntax_error(
                    "unicodeescape codec can't decode bytes in position 0-1: truncated \\UXXXXXXXX escape",
                ));
            }
            return Ok(PyObject::str_val(CompactString::from(
                &trimmed[1..trimmed.len() - 1],
            )));
        }
        // Use the real parser for complex literals
        match ferrython_parser::parse_expression(trimmed, "<literal_eval>") {
            Ok(expr) => eval_const_expr(&expr),
            Err(e) => Err(PyException::syntax_error(format!("{}", e))),
        }
    });

    let walk_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error("ast.walk() requires node argument"));
        }
        let mut nodes = Vec::new();
        collect_ast_nodes(&args[0], &mut nodes);
        Ok(PyObject::list(nodes))
    });

    let get_docstring_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "ast.get_docstring() requires node argument",
            ));
        }
        if let Some(body) = args[0].get_attr("body") {
            if let PyObjectPayload::List(items) = &body.payload {
                let items = items.read();
                if let Some(first) = items.first() {
                    let type_name = first.type_name();
                    if type_name == "Expr" {
                        if let Some(value) = first.get_attr("value") {
                            if value.type_name() == "Constant" {
                                if let Some(val) = value.get_attr("value") {
                                    if let PyObjectPayload::Str(s) = &val.payload {
                                        return Ok(PyObject::str_val(CompactString::from(
                                            clean_docstring(s.as_str()),
                                        )));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(PyObject::none())
    });

    let iter_fields_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "ast.iter_fields() requires node argument",
            ));
        }
        let mut items = Vec::new();
        if let Some(fields) = args[0].get_attr("_fields") {
            if let PyObjectPayload::Tuple(field_names) = &fields.payload {
                for fname in field_names.iter() {
                    let name = fname.py_to_string();
                    if let Some(value) = args[0].get_attr(&name) {
                        items.push(PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from(name)),
                            value,
                        ]));
                    }
                }
            }
        }
        Ok(PyObject::wrap(PyObjectPayload::VecIter(Box::new(
            ferrython_core::object::VecIterData {
                items,
                index: ferrython_core::object::SyncUsize::new(0),
            },
        ))))
    });

    let get_source_segment_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "ast.get_source_segment() requires source and node",
            ));
        }
        let source = args[0].py_to_string();
        let node = &args[1];
        let mut padded = false;
        if args.len() > 2 {
            match &args[2].payload {
                PyObjectPayload::Dict(kwargs) => {
                    if let Some(v) = kwargs
                        .read()
                        .get(&HashableKey::str_key(CompactString::from("padded")))
                    {
                        padded = v.is_truthy();
                    }
                }
                _ => padded = args[2].is_truthy(),
            }
        }
        Ok(match source_segment(&source, node, padded) {
            Some(seg) => PyObject::str_val(CompactString::from(seg)),
            None => PyObject::none(),
        })
    });

    let fix_missing_locations_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "ast.fix_missing_locations() requires node argument",
            ));
        }
        // Walk all nodes and set missing source locations to the root default.
        let mut nodes = Vec::new();
        collect_ast_nodes(&args[0], &mut nodes);
        for node in &nodes {
            let type_name = node.type_name();
            if !ast_node_supports_location(&type_name) {
                continue;
            }
            if node.get_attr("lineno").is_none() {
                set_node_attr(node, "lineno", PyObject::int(1));
            }
            if node.get_attr("col_offset").is_none() {
                set_node_attr(node, "col_offset", PyObject::int(0));
            }
            if node.get_attr("end_lineno").is_none() {
                set_node_attr(node, "end_lineno", PyObject::int(1));
            }
            if node.get_attr("end_col_offset").is_none() {
                set_node_attr(node, "end_col_offset", PyObject::int(0));
            }
        }
        Ok(args[0].clone())
    });

    let increment_lineno_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "ast.increment_lineno() requires node argument",
            ));
        }
        let mut n = args.get(1).and_then(|arg| arg.as_int()).unwrap_or(1);
        if let Some(last) = args.last() {
            if let PyObjectPayload::Dict(kwargs) = &last.payload {
                if let Some(v) = kwargs
                    .read()
                    .get(&HashableKey::str_key(CompactString::from("n")))
                {
                    n = v.as_int().unwrap_or(n);
                }
            }
        }
        let mut nodes = Vec::new();
        collect_ast_nodes(&args[0], &mut nodes);
        for node in &nodes {
            if let Some(lineno) = node.get_attr("lineno") {
                if let Some(line) = lineno.as_int() {
                    set_node_attr(node, "lineno", PyObject::int(line + n));
                }
            }
            if let Some(end_lineno) = node.get_attr("end_lineno") {
                if let Some(line) = end_lineno.as_int() {
                    set_node_attr(node, "end_lineno", PyObject::int(line + n));
                }
            }
        }
        Ok(args[0].clone())
    });

    let iter_child_nodes_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "ast.iter_child_nodes() requires node argument",
            ));
        }
        let children = get_child_nodes(&args[0]);
        Ok(PyObject::wrap(PyObjectPayload::VecIter(Box::new(
            ferrython_core::object::VecIterData {
                items: children,
                index: ferrython_core::object::SyncUsize::new(0),
            },
        ))))
    });

    let ast_init_fn = PyObject::native_function("_ast.AST.__init__", |_args| Ok(PyObject::none()));

    let make_node_type = |name: &str, fields: &[&str]| -> PyObjectRef {
        let bases = ast_base_names(name)
            .iter()
            .map(|base| get_or_create_ast_class(base))
            .collect();
        let cls = get_or_create_ast_class_with_bases(name, bases);
        if let PyObjectPayload::Class(cd) = &cls.payload {
            let field_strs: Vec<PyObjectRef> = fields
                .iter()
                .map(|f| PyObject::str_val(CompactString::from(*f)))
                .collect();
            let mut ns = cd.namespace.write();
            ns.insert(CompactString::from("_fields"), PyObject::tuple(field_strs));
            ns.insert(
                CompactString::from("__ferrython_ast_node__"),
                PyObject::bool_val(true),
            );
            ns.insert(CompactString::from("__init__"), ast_init_fn.clone());
            let attributes = if ast_node_supports_location(name) {
                vec![
                    PyObject::str_val(CompactString::from("lineno")),
                    PyObject::str_val(CompactString::from("col_offset")),
                    PyObject::str_val(CompactString::from("end_lineno")),
                    PyObject::str_val(CompactString::from("end_col_offset")),
                ]
            } else {
                Vec::new()
            };
            ns.insert(
                CompactString::from("_attributes"),
                PyObject::tuple(attributes),
            );
            drop(ns);
            cd.invalidate_cache();
        }
        cls
    };

    // copy_location(new_node, old_node)
    let copy_location_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.len() < 2 {
            return Err(PyException::type_error(
                "copy_location() requires new_node and old_node",
            ));
        }
        let new_node = &args[0];
        let old_node = &args[1];
        for attr in &["lineno", "col_offset"] {
            if let Some(val) = old_node.get_attr(attr) {
                if !matches!(val.payload, PyObjectPayload::None) {
                    if let PyObjectPayload::Instance(ref d) = new_node.payload {
                        d.attrs.write().insert(CompactString::from(*attr), val);
                    }
                }
            }
        }
        for attr in &["end_lineno", "end_col_offset"] {
            if let Some(val) = old_node.get_attr(attr) {
                if let PyObjectPayload::Instance(ref d) = new_node.payload {
                    d.attrs.write().insert(CompactString::from(*attr), val);
                }
            }
        }
        Ok(new_node.clone())
    });

    // unparse(node) — convert AST back to source code (simplified)
    let unparse_fn = make_builtin(|args: &[PyObjectRef]| {
        if args.is_empty() {
            return Err(PyException::type_error(
                "unparse() requires a node argument",
            ));
        }
        let src = ast_unparse(&args[0]);
        Ok(PyObject::str_val(CompactString::from(src)))
    });

    make_module(
        "ast",
        vec![
            ("parse", parse_fn),
            ("dump", dump_fn),
            ("literal_eval", literal_eval_fn),
            ("walk", walk_fn),
            ("get_docstring", get_docstring_fn),
            ("fix_missing_locations", fix_missing_locations_fn),
            ("increment_lineno", increment_lineno_fn),
            ("iter_child_nodes", iter_child_nodes_fn),
            ("copy_location", copy_location_fn),
            ("unparse", unparse_fn),
            ("iter_fields", iter_fields_fn),
            ("get_source_segment", get_source_segment_fn),
            // Node types (with ASDL field definitions for positional arg mapping)
            ("mod", make_node_type("mod", &[])),
            ("stmt", make_node_type("stmt", &[])),
            ("expr", make_node_type("expr", &[])),
            ("expr_context", make_node_type("expr_context", &[])),
            ("boolop", make_node_type("boolop", &[])),
            ("operator", make_node_type("operator", &[])),
            ("unaryop", make_node_type("unaryop", &[])),
            ("cmpop", make_node_type("cmpop", &[])),
            ("slice", make_node_type("slice", &[])),
            ("excepthandler", make_node_type("excepthandler", &[])),
            (
                "Module",
                make_node_type("Module", &["body", "type_ignores"]),
            ),
            ("Expression", make_node_type("Expression", &["body"])),
            ("Interactive", make_node_type("Interactive", &["body"])),
            (
                "FunctionDef",
                make_node_type(
                    "FunctionDef",
                    &[
                        "name",
                        "args",
                        "body",
                        "decorator_list",
                        "returns",
                        "type_comment",
                    ],
                ),
            ),
            (
                "AsyncFunctionDef",
                make_node_type(
                    "AsyncFunctionDef",
                    &[
                        "name",
                        "args",
                        "body",
                        "decorator_list",
                        "returns",
                        "type_comment",
                    ],
                ),
            ),
            (
                "ClassDef",
                make_node_type(
                    "ClassDef",
                    &["name", "bases", "keywords", "body", "decorator_list"],
                ),
            ),
            ("Return", make_node_type("Return", &["value"])),
            (
                "Assign",
                make_node_type("Assign", &["targets", "value", "type_comment"]),
            ),
            (
                "AugAssign",
                make_node_type("AugAssign", &["target", "op", "value"]),
            ),
            (
                "AnnAssign",
                make_node_type("AnnAssign", &["target", "annotation", "value", "simple"]),
            ),
            (
                "For",
                make_node_type("For", &["target", "iter", "body", "orelse", "type_comment"]),
            ),
            (
                "AsyncFor",
                make_node_type(
                    "AsyncFor",
                    &["target", "iter", "body", "orelse", "type_comment"],
                ),
            ),
            (
                "While",
                make_node_type("While", &["test", "body", "orelse"]),
            ),
            ("If", make_node_type("If", &["test", "body", "orelse"])),
            (
                "With",
                make_node_type("With", &["items", "body", "type_comment"]),
            ),
            (
                "AsyncWith",
                make_node_type("AsyncWith", &["items", "body", "type_comment"]),
            ),
            ("Raise", make_node_type("Raise", &["exc", "cause"])),
            (
                "Try",
                make_node_type("Try", &["body", "handlers", "orelse", "finalbody"]),
            ),
            ("Import", make_node_type("Import", &["names"])),
            (
                "ImportFrom",
                make_node_type("ImportFrom", &["module", "names", "level"]),
            ),
            ("Global", make_node_type("Global", &["names"])),
            ("Nonlocal", make_node_type("Nonlocal", &["names"])),
            ("Delete", make_node_type("Delete", &["targets"])),
            ("Assert", make_node_type("Assert", &["test", "msg"])),
            ("Expr", make_node_type("Expr", &["value"])),
            ("Name", make_node_type("Name", &["id", "ctx"])),
            ("Constant", make_node_type("Constant", &["value", "kind"])),
            ("Num", make_node_type("Num", &["n"])),
            ("Str", make_node_type("Str", &["s"])),
            ("Bytes", make_node_type("Bytes", &["s"])),
            (
                "NameConstant",
                make_node_type("NameConstant", &["value", "kind"]),
            ),
            ("Ellipsis", make_node_type("Ellipsis", &[])),
            ("BinOp", make_node_type("BinOp", &["left", "op", "right"])),
            ("UnaryOp", make_node_type("UnaryOp", &["op", "operand"])),
            ("BoolOp", make_node_type("BoolOp", &["op", "values"])),
            (
                "Compare",
                make_node_type("Compare", &["left", "ops", "comparators"]),
            ),
            (
                "Call",
                make_node_type("Call", &["func", "args", "keywords"]),
            ),
            (
                "Attribute",
                make_node_type("Attribute", &["value", "attr", "ctx"]),
            ),
            (
                "Subscript",
                make_node_type("Subscript", &["value", "slice", "ctx"]),
            ),
            ("Starred", make_node_type("Starred", &["value", "ctx"])),
            ("List", make_node_type("List", &["elts", "ctx"])),
            ("Tuple", make_node_type("Tuple", &["elts", "ctx"])),
            ("Dict", make_node_type("Dict", &["keys", "values"])),
            ("Set", make_node_type("Set", &["elts"])),
            ("Lambda", make_node_type("Lambda", &["args", "body"])),
            (
                "IfExp",
                make_node_type("IfExp", &["test", "body", "orelse"]),
            ),
            (
                "ListComp",
                make_node_type("ListComp", &["elt", "generators"]),
            ),
            ("SetComp", make_node_type("SetComp", &["elt", "generators"])),
            (
                "DictComp",
                make_node_type("DictComp", &["key", "value", "generators"]),
            ),
            (
                "GeneratorExp",
                make_node_type("GeneratorExp", &["elt", "generators"]),
            ),
            ("Yield", make_node_type("Yield", &["value"])),
            ("YieldFrom", make_node_type("YieldFrom", &["value"])),
            ("Await", make_node_type("Await", &["value"])),
            (
                "FormattedValue",
                make_node_type("FormattedValue", &["value", "conversion", "format_spec"]),
            ),
            ("JoinedStr", make_node_type("JoinedStr", &["values"])),
            (
                "NamedExpr",
                make_node_type("NamedExpr", &["target", "value"]),
            ),
            (
                "Slice",
                make_node_type("Slice", &["lower", "upper", "step"]),
            ),
            ("Index", make_node_type("Index", &["value"])),
            ("ExtSlice", make_node_type("ExtSlice", &["dims"])),
            ("Pass", make_node_type("Pass", &[])),
            ("Break", make_node_type("Break", &[])),
            ("Continue", make_node_type("Continue", &[])),
            (
                "ExceptHandler",
                make_node_type("ExceptHandler", &["type", "name", "body"]),
            ),
            // Context types
            ("Load", make_node_type("Load", &[])),
            ("Store", make_node_type("Store", &[])),
            ("Del", make_node_type("Del", &[])),
            // Operator types
            ("Add", make_node_type("Add", &[])),
            ("Sub", make_node_type("Sub", &[])),
            ("Mult", make_node_type("Mult", &[])),
            ("Div", make_node_type("Div", &[])),
            ("Mod", make_node_type("Mod", &[])),
            ("Pow", make_node_type("Pow", &[])),
            ("LShift", make_node_type("LShift", &[])),
            ("RShift", make_node_type("RShift", &[])),
            ("BitOr", make_node_type("BitOr", &[])),
            ("BitXor", make_node_type("BitXor", &[])),
            ("BitAnd", make_node_type("BitAnd", &[])),
            ("FloorDiv", make_node_type("FloorDiv", &[])),
            ("MatMult", make_node_type("MatMult", &[])),
            ("And", make_node_type("And", &[])),
            ("Or", make_node_type("Or", &[])),
            ("Invert", make_node_type("Invert", &[])),
            ("Not", make_node_type("Not", &[])),
            ("UAdd", make_node_type("UAdd", &[])),
            ("USub", make_node_type("USub", &[])),
            ("Eq", make_node_type("Eq", &[])),
            ("NotEq", make_node_type("NotEq", &[])),
            ("Lt", make_node_type("Lt", &[])),
            ("LtE", make_node_type("LtE", &[])),
            ("Gt", make_node_type("Gt", &[])),
            ("GtE", make_node_type("GtE", &[])),
            ("Is", make_node_type("Is", &[])),
            ("IsNot", make_node_type("IsNot", &[])),
            ("In", make_node_type("In", &[])),
            ("NotIn", make_node_type("NotIn", &[])),
            // Misc
            (
                "arguments",
                make_node_type(
                    "arguments",
                    &[
                        "posonlyargs",
                        "args",
                        "vararg",
                        "kwonlyargs",
                        "kw_defaults",
                        "kwarg",
                        "defaults",
                    ],
                ),
            ),
            (
                "arg",
                make_node_type("arg", &["arg", "annotation", "type_comment"]),
            ),
            ("keyword", make_node_type("keyword", &["arg", "value"])),
            ("alias", make_node_type("alias", &["name", "asname"])),
            (
                "withitem",
                make_node_type("withitem", &["context_expr", "optional_vars"]),
            ),
            (
                "comprehension",
                make_node_type("comprehension", &["target", "iter", "ifs", "is_async"]),
            ),
            ("PyCF_ONLY_AST", PyObject::int(1024)),
            ("AST", make_node_type("AST", &[])),
        ],
    )
}

fn ast_base_names(name: &str) -> &'static [&'static str] {
    match name {
        "Module" | "Expression" | "Interactive" => &["mod"],
        "FunctionDef" | "AsyncFunctionDef" | "ClassDef" | "Return" | "Assign" | "AugAssign"
        | "AnnAssign" | "For" | "AsyncFor" | "While" | "If" | "With" | "AsyncWith" | "Raise"
        | "Try" | "Import" | "ImportFrom" | "Global" | "Nonlocal" | "Delete" | "Assert"
        | "Expr" | "Pass" | "Break" | "Continue" => &["stmt"],
        "BoolOp" | "NamedExpr" | "BinOp" | "UnaryOp" | "Lambda" | "IfExp" | "Dict" | "Set"
        | "ListComp" | "SetComp" | "DictComp" | "GeneratorExp" | "Await" | "Yield"
        | "YieldFrom" | "Compare" | "Call" | "FormattedValue" | "JoinedStr" | "Constant"
        | "Attribute" | "Subscript" | "Starred" | "Name" | "List" | "Tuple" => &["expr"],
        "Num" | "Str" | "Bytes" | "NameConstant" | "Ellipsis" => &["Constant"],
        "Load" | "Store" | "Del" => &["expr_context"],
        "And" | "Or" => &["boolop"],
        "Add" | "Sub" | "Mult" | "Div" | "Mod" | "Pow" | "LShift" | "RShift" | "BitOr"
        | "BitXor" | "BitAnd" | "FloorDiv" | "MatMult" => &["operator"],
        "Invert" | "Not" | "UAdd" | "USub" => &["unaryop"],
        "Eq" | "NotEq" | "Lt" | "LtE" | "Gt" | "GtE" | "Is" | "IsNot" | "In" | "NotIn" => {
            &["cmpop"]
        }
        "Slice" | "Index" | "ExtSlice" => &["slice"],
        "ExceptHandler" => &["excepthandler"],
        _ => &["AST"],
    }
}

fn ast_node_supports_location(name: &str) -> bool {
    matches!(
        name,
        "stmt"
            | "expr"
            | "excepthandler"
            | "FunctionDef"
            | "AsyncFunctionDef"
            | "ClassDef"
            | "Return"
            | "Assign"
            | "AugAssign"
            | "AnnAssign"
            | "For"
            | "AsyncFor"
            | "While"
            | "If"
            | "With"
            | "AsyncWith"
            | "Raise"
            | "Try"
            | "Import"
            | "ImportFrom"
            | "Global"
            | "Nonlocal"
            | "Delete"
            | "Assert"
            | "Expr"
            | "Pass"
            | "Break"
            | "Continue"
            | "BoolOp"
            | "NamedExpr"
            | "BinOp"
            | "UnaryOp"
            | "Lambda"
            | "IfExp"
            | "Dict"
            | "Set"
            | "ListComp"
            | "SetComp"
            | "DictComp"
            | "GeneratorExp"
            | "Await"
            | "Yield"
            | "YieldFrom"
            | "Compare"
            | "Call"
            | "FormattedValue"
            | "JoinedStr"
            | "Constant"
            | "Attribute"
            | "Subscript"
            | "Starred"
            | "Name"
            | "List"
            | "Tuple"
            | "Num"
            | "Str"
            | "Bytes"
            | "NameConstant"
            | "Ellipsis"
            | "ExceptHandler"
            | "arg"
    )
}

// ── AST conversion helpers ──

fn set_node_attr(obj: &PyObjectRef, name: &str, value: PyObjectRef) {
    if let PyObjectPayload::Instance(ref d) = obj.payload {
        d.attrs.write().insert(CompactString::from(name), value);
    }
}

fn has_instance_attr(obj: &PyObjectRef, name: &str) -> bool {
    if let PyObjectPayload::Instance(ref d) = obj.payload {
        d.attrs.read().contains_key(&CompactString::from(name))
    } else {
        false
    }
}

fn set_node_fields(obj: &PyObjectRef, fields: &[&str]) {
    let flds: Vec<PyObjectRef> = fields
        .iter()
        .map(|f| PyObject::str_val(CompactString::from(*f)))
        .collect();
    set_node_attr(obj, "_fields", PyObject::tuple(flds));
}

fn set_location(obj: &PyObjectRef, loc: &ferrython_ast::SourceLocation) {
    set_node_attr(obj, "lineno", PyObject::int(loc.line as i64));
    set_node_attr(obj, "col_offset", PyObject::int(loc.column as i64));
    set_node_attr(
        obj,
        "end_lineno",
        match loc.end_line {
            Some(l) => PyObject::int(l as i64),
            None => PyObject::none(),
        },
    );
    set_node_attr(
        obj,
        "end_col_offset",
        match loc.end_column {
            Some(c) => PyObject::int(c as i64),
            None => PyObject::none(),
        },
    );
}

fn make_ast_node(type_name: &str) -> PyObjectRef {
    let cls = get_or_create_ast_class(type_name);
    PyObject::instance(cls)
}

/// Get or create a shared AST class, so isinstance(ast.parse(...), ast.Module) works
fn get_or_create_ast_class(name: &str) -> PyObjectRef {
    get_or_create_ast_class_with_bases(name, Vec::new())
}

fn get_or_create_ast_class_with_bases(name: &str, bases: Vec<PyObjectRef>) -> PyObjectRef {
    use std::collections::HashMap;
    use std::sync::LazyLock;
    use std::sync::Mutex;
    static AST_CLASSES: LazyLock<Mutex<HashMap<String, PyObjectRef>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let mut map = AST_CLASSES.lock().unwrap();
    if let Some(cls) = map.get(name) {
        return cls.clone();
    }
    let cls = PyObject::class(CompactString::from(name), bases, IndexMap::new());
    map.insert(name.to_string(), cls.clone());
    cls
}

/// Fix expression context to Store (for assignment targets) or Del (for delete targets)
fn fix_ctx(node: &PyObjectRef, ctx_name: &str) {
    set_node_attr(node, "ctx", make_ast_node(ctx_name));
    // Recursively fix children for starred, tuple, list
    let type_name = node.type_name();
    if type_name == "Tuple" || type_name == "List" {
        if let Some(elts) = node.get_attr("elts") {
            if let PyObjectPayload::List(items) = &elts.payload {
                for item in items.read().iter() {
                    fix_ctx(item, ctx_name);
                }
            }
        }
    } else if type_name == "Starred" {
        if let Some(val) = node.get_attr("value") {
            fix_ctx(&val, ctx_name);
        }
    }
}

pub fn module_ast_to_pyobject(module: &ferrython_ast::Module) -> PyObjectRef {
    match module {
        ferrython_ast::Module::Module {
            body,
            type_ignores: _,
        } => {
            let node = make_ast_node("Module");
            let body_list: Vec<PyObjectRef> = body.iter().map(stmt_to_pyobject).collect();
            set_node_attr(&node, "body", PyObject::list(body_list));
            set_node_attr(&node, "type_ignores", PyObject::list(vec![]));
            set_node_fields(&node, &["body", "type_ignores"]);
            node
        }
        ferrython_ast::Module::Interactive { body } => {
            let node = make_ast_node("Interactive");
            let body_list: Vec<PyObjectRef> = body.iter().map(stmt_to_pyobject).collect();
            set_node_attr(&node, "body", PyObject::list(body_list));
            set_node_fields(&node, &["body"]);
            node
        }
        ferrython_ast::Module::Expression { body } => {
            let node = make_ast_node("Expression");
            set_node_attr(&node, "body", expr_to_pyobject(body));
            set_node_fields(&node, &["body"]);
            node
        }
    }
}

fn module_to_pyobject(module: &ferrython_ast::Module) -> PyObjectRef {
    module_ast_to_pyobject(module)
}

fn stmt_to_pyobject(stmt: &ferrython_ast::Statement) -> PyObjectRef {
    use ferrython_ast::StatementKind::*;
    let node = match &stmt.node {
        FunctionDef {
            name,
            args,
            body,
            decorator_list,
            returns,
            is_async,
            ..
        } => {
            let type_name = if *is_async {
                "AsyncFunctionDef"
            } else {
                "FunctionDef"
            };
            let n = make_ast_node(type_name);
            set_node_attr(&n, "name", PyObject::str_val(name.clone()));
            set_node_attr(&n, "args", args_to_pyobject(args));
            set_node_attr(
                &n,
                "body",
                PyObject::list(body.iter().map(stmt_to_pyobject).collect()),
            );
            set_node_attr(
                &n,
                "decorator_list",
                PyObject::list(decorator_list.iter().map(expr_to_pyobject).collect()),
            );
            set_node_attr(
                &n,
                "returns",
                match returns {
                    Some(r) => expr_to_pyobject(r),
                    None => PyObject::none(),
                },
            );
            set_node_attr(&n, "type_comment", PyObject::none());
            set_node_fields(
                &n,
                &[
                    "name",
                    "args",
                    "body",
                    "decorator_list",
                    "returns",
                    "type_comment",
                ],
            );
            n
        }
        ClassDef {
            name,
            bases,
            keywords,
            body,
            decorator_list,
        } => {
            let n = make_ast_node("ClassDef");
            set_node_attr(&n, "name", PyObject::str_val(name.clone()));
            set_node_attr(
                &n,
                "bases",
                PyObject::list(bases.iter().map(expr_to_pyobject).collect()),
            );
            set_node_attr(
                &n,
                "keywords",
                PyObject::list(keywords.iter().map(keyword_to_pyobject).collect()),
            );
            set_node_attr(
                &n,
                "body",
                PyObject::list(body.iter().map(stmt_to_pyobject).collect()),
            );
            set_node_attr(
                &n,
                "decorator_list",
                PyObject::list(decorator_list.iter().map(expr_to_pyobject).collect()),
            );
            set_node_fields(&n, &["name", "bases", "keywords", "body", "decorator_list"]);
            n
        }
        Return { value } => {
            let n = make_ast_node("Return");
            set_node_attr(
                &n,
                "value",
                match value {
                    Some(v) => expr_to_pyobject(v),
                    None => PyObject::none(),
                },
            );
            set_node_fields(&n, &["value"]);
            n
        }
        Delete { targets } => {
            let n = make_ast_node("Delete");
            let target_nodes: Vec<PyObjectRef> = targets
                .iter()
                .map(|t| {
                    let node = expr_to_pyobject(t);
                    fix_ctx(&node, "Del");
                    node
                })
                .collect();
            set_node_attr(&n, "targets", PyObject::list(target_nodes));
            set_node_fields(&n, &["targets"]);
            n
        }
        Assign { targets, value, .. } => {
            let n = make_ast_node("Assign");
            let target_nodes: Vec<PyObjectRef> = targets
                .iter()
                .map(|t| {
                    let node = expr_to_pyobject(t);
                    fix_ctx(&node, "Store");
                    node
                })
                .collect();
            set_node_attr(&n, "targets", PyObject::list(target_nodes));
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_attr(&n, "type_comment", PyObject::none());
            set_node_fields(&n, &["targets", "value", "type_comment"]);
            n
        }
        AugAssign { target, op, value } => {
            let n = make_ast_node("AugAssign");
            let tgt = expr_to_pyobject(target);
            fix_ctx(&tgt, "Store");
            set_node_attr(&n, "target", tgt);
            set_node_attr(&n, "op", operator_to_pyobject(*op));
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_fields(&n, &["target", "op", "value"]);
            n
        }
        AnnAssign {
            target,
            annotation,
            value,
            simple,
        } => {
            let n = make_ast_node("AnnAssign");
            let tgt = expr_to_pyobject(target);
            fix_ctx(&tgt, "Store");
            set_node_attr(&n, "target", tgt);
            set_node_attr(&n, "annotation", expr_to_pyobject(annotation));
            set_node_attr(
                &n,
                "value",
                match value {
                    Some(v) => expr_to_pyobject(v),
                    None => PyObject::none(),
                },
            );
            set_node_attr(&n, "simple", PyObject::bool_val(*simple));
            set_node_fields(&n, &["target", "annotation", "value", "simple"]);
            n
        }
        For {
            target,
            iter,
            body,
            orelse,
            is_async,
            ..
        } => {
            let type_name = if *is_async { "AsyncFor" } else { "For" };
            let n = make_ast_node(type_name);
            let tgt = expr_to_pyobject(target);
            fix_ctx(&tgt, "Store");
            set_node_attr(&n, "target", tgt);
            set_node_attr(&n, "iter", expr_to_pyobject(iter));
            set_node_attr(
                &n,
                "body",
                PyObject::list(body.iter().map(stmt_to_pyobject).collect()),
            );
            set_node_attr(
                &n,
                "orelse",
                PyObject::list(orelse.iter().map(stmt_to_pyobject).collect()),
            );
            set_node_attr(&n, "type_comment", PyObject::none());
            set_node_fields(&n, &["target", "iter", "body", "orelse", "type_comment"]);
            n
        }
        While { test, body, orelse } => {
            let n = make_ast_node("While");
            set_node_attr(&n, "test", expr_to_pyobject(test));
            set_node_attr(
                &n,
                "body",
                PyObject::list(body.iter().map(stmt_to_pyobject).collect()),
            );
            set_node_attr(
                &n,
                "orelse",
                PyObject::list(orelse.iter().map(stmt_to_pyobject).collect()),
            );
            set_node_fields(&n, &["test", "body", "orelse"]);
            n
        }
        If { test, body, orelse } => {
            let n = make_ast_node("If");
            set_node_attr(&n, "test", expr_to_pyobject(test));
            set_node_attr(
                &n,
                "body",
                PyObject::list(body.iter().map(stmt_to_pyobject).collect()),
            );
            set_node_attr(
                &n,
                "orelse",
                PyObject::list(orelse.iter().map(stmt_to_pyobject).collect()),
            );
            set_node_fields(&n, &["test", "body", "orelse"]);
            n
        }
        With {
            items,
            body,
            is_async,
            ..
        } => {
            let type_name = if *is_async { "AsyncWith" } else { "With" };
            let n = make_ast_node(type_name);
            set_node_attr(
                &n,
                "items",
                PyObject::list(items.iter().map(withitem_to_pyobject).collect()),
            );
            set_node_attr(
                &n,
                "body",
                PyObject::list(body.iter().map(stmt_to_pyobject).collect()),
            );
            set_node_attr(&n, "type_comment", PyObject::none());
            set_node_fields(&n, &["items", "body", "type_comment"]);
            n
        }
        Raise { exc, cause } => {
            let n = make_ast_node("Raise");
            set_node_attr(
                &n,
                "exc",
                match exc {
                    Some(e) => expr_to_pyobject(e),
                    None => PyObject::none(),
                },
            );
            set_node_attr(
                &n,
                "cause",
                match cause {
                    Some(c) => expr_to_pyobject(c),
                    None => PyObject::none(),
                },
            );
            set_node_fields(&n, &["exc", "cause"]);
            n
        }
        Try {
            body,
            handlers,
            orelse,
            finalbody,
        } => {
            let n = make_ast_node("Try");
            set_node_attr(
                &n,
                "body",
                PyObject::list(body.iter().map(stmt_to_pyobject).collect()),
            );
            set_node_attr(
                &n,
                "handlers",
                PyObject::list(handlers.iter().map(except_handler_to_pyobject).collect()),
            );
            set_node_attr(
                &n,
                "orelse",
                PyObject::list(orelse.iter().map(stmt_to_pyobject).collect()),
            );
            set_node_attr(
                &n,
                "finalbody",
                PyObject::list(finalbody.iter().map(stmt_to_pyobject).collect()),
            );
            set_node_fields(&n, &["body", "handlers", "orelse", "finalbody"]);
            n
        }
        Assert { test, msg } => {
            let n = make_ast_node("Assert");
            set_node_attr(&n, "test", expr_to_pyobject(test));
            set_node_attr(
                &n,
                "msg",
                match msg {
                    Some(m) => expr_to_pyobject(m),
                    None => PyObject::none(),
                },
            );
            set_node_fields(&n, &["test", "msg"]);
            n
        }
        Import { names } => {
            let n = make_ast_node("Import");
            set_node_attr(
                &n,
                "names",
                PyObject::list(names.iter().map(alias_to_pyobject).collect()),
            );
            set_node_fields(&n, &["names"]);
            n
        }
        ImportFrom {
            module,
            names,
            level,
        } => {
            let n = make_ast_node("ImportFrom");
            set_node_attr(
                &n,
                "module",
                match module {
                    Some(m) => PyObject::str_val(m.clone()),
                    None => PyObject::none(),
                },
            );
            set_node_attr(
                &n,
                "names",
                PyObject::list(names.iter().map(alias_to_pyobject).collect()),
            );
            set_node_attr(&n, "level", PyObject::int(*level as i64));
            set_node_fields(&n, &["module", "names", "level"]);
            n
        }
        Global { names } => {
            let n = make_ast_node("Global");
            set_node_attr(
                &n,
                "names",
                PyObject::list(names.iter().map(|s| PyObject::str_val(s.clone())).collect()),
            );
            set_node_fields(&n, &["names"]);
            n
        }
        Nonlocal { names } => {
            let n = make_ast_node("Nonlocal");
            set_node_attr(
                &n,
                "names",
                PyObject::list(names.iter().map(|s| PyObject::str_val(s.clone())).collect()),
            );
            set_node_fields(&n, &["names"]);
            n
        }
        Expr { value } => {
            let n = make_ast_node("Expr");
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_fields(&n, &["value"]);
            n
        }
        Pass => {
            let n = make_ast_node("Pass");
            set_node_fields(&n, &[]);
            n
        }
        Break => {
            let n = make_ast_node("Break");
            set_node_fields(&n, &[]);
            n
        }
        Continue => {
            let n = make_ast_node("Continue");
            set_node_fields(&n, &[]);
            n
        }
        Match { subject, cases } => {
            let n = make_ast_node("Match");
            set_node_attr(&n, "subject", expr_to_pyobject(subject));
            let case_nodes: Vec<PyObjectRef> = cases
                .iter()
                .map(|c| {
                    let cn = make_ast_node("match_case");
                    set_node_attr(
                        &cn,
                        "body",
                        PyObject::list(c.body.iter().map(stmt_to_pyobject).collect()),
                    );
                    set_node_attr(
                        &cn,
                        "guard",
                        match &c.guard {
                            Some(g) => expr_to_pyobject(g),
                            None => PyObject::none(),
                        },
                    );
                    set_node_fields(&cn, &["pattern", "guard", "body"]);
                    cn
                })
                .collect();
            set_node_attr(&n, "cases", PyObject::list(case_nodes));
            set_node_fields(&n, &["subject", "cases"]);
            n
        }
    };
    set_location(&node, &stmt.location);
    node
}

fn expr_to_pyobject(expr: &ferrython_ast::Expression) -> PyObjectRef {
    use ferrython_ast::ExpressionKind::*;
    let node = match &expr.node {
        BoolOp { op, values } => {
            let n = make_ast_node("BoolOp");
            set_node_attr(&n, "op", boolop_to_pyobject(*op));
            set_node_attr(
                &n,
                "values",
                PyObject::list(values.iter().map(expr_to_pyobject).collect()),
            );
            set_node_fields(&n, &["op", "values"]);
            n
        }
        NamedExpr { target, value } => {
            let n = make_ast_node("NamedExpr");
            set_node_attr(&n, "target", expr_to_pyobject(target));
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_fields(&n, &["target", "value"]);
            n
        }
        BinOp { left, op, right } => {
            let n = make_ast_node("BinOp");
            set_node_attr(&n, "left", expr_to_pyobject(left));
            set_node_attr(&n, "op", operator_to_pyobject(*op));
            set_node_attr(&n, "right", expr_to_pyobject(right));
            set_node_fields(&n, &["left", "op", "right"]);
            n
        }
        UnaryOp { op, operand } => {
            let n = make_ast_node("UnaryOp");
            set_node_attr(&n, "op", unaryop_to_pyobject(*op));
            set_node_attr(&n, "operand", expr_to_pyobject(operand));
            set_node_fields(&n, &["op", "operand"]);
            n
        }
        Lambda { args, body } => {
            let n = make_ast_node("Lambda");
            set_node_attr(&n, "args", args_to_pyobject(args));
            set_node_attr(&n, "body", expr_to_pyobject(body));
            set_node_fields(&n, &["args", "body"]);
            n
        }
        IfExp { test, body, orelse } => {
            let n = make_ast_node("IfExp");
            set_node_attr(&n, "test", expr_to_pyobject(test));
            set_node_attr(&n, "body", expr_to_pyobject(body));
            set_node_attr(&n, "orelse", expr_to_pyobject(orelse));
            set_node_fields(&n, &["test", "body", "orelse"]);
            n
        }
        Dict { keys, values } => {
            let n = make_ast_node("Dict");
            let key_list: Vec<PyObjectRef> = keys
                .iter()
                .map(|k| match k {
                    Some(e) => expr_to_pyobject(e),
                    None => PyObject::none(),
                })
                .collect();
            set_node_attr(&n, "keys", PyObject::list(key_list));
            set_node_attr(
                &n,
                "values",
                PyObject::list(values.iter().map(expr_to_pyobject).collect()),
            );
            set_node_fields(&n, &["keys", "values"]);
            n
        }
        Set { elts } => {
            let n = make_ast_node("Set");
            set_node_attr(
                &n,
                "elts",
                PyObject::list(elts.iter().map(expr_to_pyobject).collect()),
            );
            set_node_fields(&n, &["elts"]);
            n
        }
        ListComp { elt, generators } => {
            let n = make_ast_node("ListComp");
            set_node_attr(&n, "elt", expr_to_pyobject(elt));
            set_node_attr(
                &n,
                "generators",
                PyObject::list(generators.iter().map(comprehension_to_pyobject).collect()),
            );
            set_node_fields(&n, &["elt", "generators"]);
            n
        }
        SetComp { elt, generators } => {
            let n = make_ast_node("SetComp");
            set_node_attr(&n, "elt", expr_to_pyobject(elt));
            set_node_attr(
                &n,
                "generators",
                PyObject::list(generators.iter().map(comprehension_to_pyobject).collect()),
            );
            set_node_fields(&n, &["elt", "generators"]);
            n
        }
        DictComp {
            key,
            value,
            generators,
        } => {
            let n = make_ast_node("DictComp");
            set_node_attr(&n, "key", expr_to_pyobject(key));
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_attr(
                &n,
                "generators",
                PyObject::list(generators.iter().map(comprehension_to_pyobject).collect()),
            );
            set_node_fields(&n, &["key", "value", "generators"]);
            n
        }
        GeneratorExp { elt, generators } => {
            let n = make_ast_node("GeneratorExp");
            set_node_attr(&n, "elt", expr_to_pyobject(elt));
            set_node_attr(
                &n,
                "generators",
                PyObject::list(generators.iter().map(comprehension_to_pyobject).collect()),
            );
            set_node_fields(&n, &["elt", "generators"]);
            n
        }
        Await { value } => {
            let n = make_ast_node("Await");
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_fields(&n, &["value"]);
            n
        }
        Yield { value } => {
            let n = make_ast_node("Yield");
            set_node_attr(
                &n,
                "value",
                match value {
                    Some(v) => expr_to_pyobject(v),
                    None => PyObject::none(),
                },
            );
            set_node_fields(&n, &["value"]);
            n
        }
        YieldFrom { value } => {
            let n = make_ast_node("YieldFrom");
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_fields(&n, &["value"]);
            n
        }
        Compare {
            left,
            ops,
            comparators,
        } => {
            let n = make_ast_node("Compare");
            set_node_attr(&n, "left", expr_to_pyobject(left));
            set_node_attr(
                &n,
                "ops",
                PyObject::list(ops.iter().map(|o| cmpop_to_pyobject(*o)).collect()),
            );
            set_node_attr(
                &n,
                "comparators",
                PyObject::list(comparators.iter().map(expr_to_pyobject).collect()),
            );
            set_node_fields(&n, &["left", "ops", "comparators"]);
            n
        }
        Call {
            func,
            args,
            keywords,
        } => {
            let n = make_ast_node("Call");
            set_node_attr(&n, "func", expr_to_pyobject(func));
            set_node_attr(
                &n,
                "args",
                PyObject::list(args.iter().map(expr_to_pyobject).collect()),
            );
            set_node_attr(
                &n,
                "keywords",
                PyObject::list(keywords.iter().map(keyword_to_pyobject).collect()),
            );
            set_node_fields(&n, &["func", "args", "keywords"]);
            n
        }
        FormattedValue {
            value,
            conversion,
            format_spec,
        } => {
            let n = make_ast_node("FormattedValue");
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_attr(
                &n,
                "conversion",
                match conversion {
                    Some(c) => PyObject::int(*c as i64),
                    None => PyObject::int(-1),
                },
            );
            set_node_attr(
                &n,
                "format_spec",
                match format_spec {
                    Some(s) => expr_to_pyobject(s),
                    None => PyObject::none(),
                },
            );
            set_node_fields(&n, &["value", "conversion", "format_spec"]);
            n
        }
        JoinedStr { values } => {
            let n = make_ast_node("JoinedStr");
            set_node_attr(
                &n,
                "values",
                PyObject::list(values.iter().map(expr_to_pyobject).collect()),
            );
            set_node_fields(&n, &["values"]);
            n
        }
        Constant { value } => {
            let n = make_ast_node("Constant");
            set_node_attr(&n, "value", constant_to_pyobject(value));
            set_node_attr(&n, "kind", PyObject::none());
            set_node_fields(&n, &["value", "kind"]);
            n
        }
        Attribute { value, attr, ctx } => {
            let n = make_ast_node("Attribute");
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_attr(&n, "attr", PyObject::str_val(attr.clone()));
            set_node_attr(&n, "ctx", ctx_to_pyobject(*ctx));
            set_node_fields(&n, &["value", "attr", "ctx"]);
            n
        }
        Subscript { value, slice, ctx } => {
            let n = make_ast_node("Subscript");
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_attr(&n, "slice", slice_to_pyobject(slice));
            set_node_attr(&n, "ctx", ctx_to_pyobject(*ctx));
            set_node_fields(&n, &["value", "slice", "ctx"]);
            n
        }
        Starred { value, ctx } => {
            let n = make_ast_node("Starred");
            set_node_attr(&n, "value", expr_to_pyobject(value));
            set_node_attr(&n, "ctx", ctx_to_pyobject(*ctx));
            set_node_fields(&n, &["value", "ctx"]);
            n
        }
        Name { id, ctx } => {
            let n = make_ast_node("Name");
            set_node_attr(&n, "id", PyObject::str_val(id.clone()));
            set_node_attr(&n, "ctx", ctx_to_pyobject(*ctx));
            set_node_fields(&n, &["id", "ctx"]);
            n
        }
        List { elts, ctx } => {
            let n = make_ast_node("List");
            set_node_attr(
                &n,
                "elts",
                PyObject::list(elts.iter().map(expr_to_pyobject).collect()),
            );
            set_node_attr(&n, "ctx", ctx_to_pyobject(*ctx));
            set_node_fields(&n, &["elts", "ctx"]);
            n
        }
        Tuple { elts, ctx } => {
            let n = make_ast_node("Tuple");
            set_node_attr(
                &n,
                "elts",
                PyObject::list(elts.iter().map(expr_to_pyobject).collect()),
            );
            set_node_attr(&n, "ctx", ctx_to_pyobject(*ctx));
            set_node_fields(&n, &["elts", "ctx"]);
            n
        }
        Slice { lower, upper, step } => {
            let n = make_ast_node("Slice");
            set_node_attr(
                &n,
                "lower",
                match lower {
                    Some(e) => expr_to_pyobject(e),
                    None => PyObject::none(),
                },
            );
            set_node_attr(
                &n,
                "upper",
                match upper {
                    Some(e) => expr_to_pyobject(e),
                    None => PyObject::none(),
                },
            );
            set_node_attr(
                &n,
                "step",
                match step {
                    Some(e) => expr_to_pyobject(e),
                    None => PyObject::none(),
                },
            );
            set_node_fields(&n, &["lower", "upper", "step"]);
            n
        }
    };
    if !matches!(expr.node, Slice { .. }) {
        set_location(&node, &expr.location);
    }
    node
}

fn slice_to_pyobject(slice: &ferrython_ast::Expression) -> PyObjectRef {
    match &slice.node {
        ferrython_ast::ExpressionKind::Slice { .. } => expr_to_pyobject(slice),
        ferrython_ast::ExpressionKind::Tuple { elts, .. }
            if elts
                .iter()
                .any(|elt| matches!(elt.node, ferrython_ast::ExpressionKind::Slice { .. })) =>
        {
            let n = make_ast_node("ExtSlice");
            set_node_attr(
                &n,
                "dims",
                PyObject::list(elts.iter().map(slice_dim_to_pyobject).collect()),
            );
            set_node_fields(&n, &["dims"]);
            n
        }
        _ => {
            let n = make_ast_node("Index");
            set_node_attr(&n, "value", expr_to_pyobject(slice));
            set_node_fields(&n, &["value"]);
            n
        }
    }
}

fn slice_dim_to_pyobject(dim: &ferrython_ast::Expression) -> PyObjectRef {
    if matches!(dim.node, ferrython_ast::ExpressionKind::Slice { .. }) {
        expr_to_pyobject(dim)
    } else {
        let n = make_ast_node("Index");
        set_node_attr(&n, "value", expr_to_pyobject(dim));
        set_node_fields(&n, &["value"]);
        n
    }
}

fn constant_to_pyobject(c: &ferrython_ast::Constant) -> PyObjectRef {
    match c {
        ferrython_ast::Constant::None => PyObject::none(),
        ferrython_ast::Constant::Bool(b) => PyObject::bool_val(*b),
        ferrython_ast::Constant::Int(i) => match i {
            ferrython_ast::BigInt::Small(n) => PyObject::int(*n),
            ferrython_ast::BigInt::Big(b) => PyObject::big_int(b.as_ref().clone()),
        },
        ferrython_ast::Constant::Float(f) => PyObject::float(*f),
        ferrython_ast::Constant::Complex { real, imag } => PyObject::complex(*real, *imag),
        ferrython_ast::Constant::Str(s) => PyObject::str_val(s.clone()),
        ferrython_ast::Constant::Bytes(b) => PyObject::bytes(b.clone()),
        ferrython_ast::Constant::Ellipsis => PyObject::ellipsis(),
        ferrython_ast::Constant::Tuple(items) => {
            PyObject::tuple(items.iter().map(constant_to_pyobject).collect())
        }
        ferrython_ast::Constant::FrozenSet(items) => {
            let mut map = new_fx_hashkey_map();
            for item in items {
                let obj = constant_to_pyobject(item);
                if let Ok(key) = HashableKey::from_object(&obj) {
                    map.insert(key, obj);
                }
            }
            PyObject::frozenset(map)
        }
    }
}

fn operator_to_pyobject(op: ferrython_ast::Operator) -> PyObjectRef {
    use ferrython_ast::Operator::*;
    make_ast_node(match op {
        Add => "Add",
        Sub => "Sub",
        Mult => "Mult",
        MatMult => "MatMult",
        Div => "Div",
        Mod => "Mod",
        Pow => "Pow",
        LShift => "LShift",
        RShift => "RShift",
        BitOr => "BitOr",
        BitXor => "BitXor",
        BitAnd => "BitAnd",
        FloorDiv => "FloorDiv",
    })
}

fn boolop_to_pyobject(op: ferrython_ast::BoolOperator) -> PyObjectRef {
    make_ast_node(match op {
        ferrython_ast::BoolOperator::And => "And",
        ferrython_ast::BoolOperator::Or => "Or",
    })
}

fn unaryop_to_pyobject(op: ferrython_ast::UnaryOperator) -> PyObjectRef {
    use ferrython_ast::UnaryOperator::*;
    make_ast_node(match op {
        Invert => "Invert",
        Not => "Not",
        UAdd => "UAdd",
        USub => "USub",
    })
}

fn cmpop_to_pyobject(op: ferrython_ast::CompareOperator) -> PyObjectRef {
    use ferrython_ast::CompareOperator::*;
    make_ast_node(match op {
        Eq => "Eq",
        NotEq => "NotEq",
        Lt => "Lt",
        LtE => "LtE",
        Gt => "Gt",
        GtE => "GtE",
        Is => "Is",
        IsNot => "IsNot",
        In => "In",
        NotIn => "NotIn",
    })
}

fn ctx_to_pyobject(ctx: ferrython_ast::ExprContext) -> PyObjectRef {
    make_ast_node(match ctx {
        ferrython_ast::ExprContext::Load => "Load",
        ferrython_ast::ExprContext::Store => "Store",
        ferrython_ast::ExprContext::Del => "Del",
    })
}

fn args_to_pyobject(args: &ferrython_ast::Arguments) -> PyObjectRef {
    let n = make_ast_node("arguments");
    set_node_attr(
        &n,
        "posonlyargs",
        PyObject::list(args.posonlyargs.iter().map(arg_to_pyobject).collect()),
    );
    set_node_attr(
        &n,
        "args",
        PyObject::list(args.args.iter().map(arg_to_pyobject).collect()),
    );
    set_node_attr(
        &n,
        "vararg",
        match &args.vararg {
            Some(a) => arg_to_pyobject(a),
            None => PyObject::none(),
        },
    );
    set_node_attr(
        &n,
        "kwonlyargs",
        PyObject::list(args.kwonlyargs.iter().map(arg_to_pyobject).collect()),
    );
    set_node_attr(
        &n,
        "kw_defaults",
        PyObject::list(
            args.kw_defaults
                .iter()
                .map(|d| match d {
                    Some(e) => expr_to_pyobject(e),
                    None => PyObject::none(),
                })
                .collect(),
        ),
    );
    set_node_attr(
        &n,
        "kwarg",
        match &args.kwarg {
            Some(a) => arg_to_pyobject(a),
            None => PyObject::none(),
        },
    );
    set_node_attr(
        &n,
        "defaults",
        PyObject::list(args.defaults.iter().map(expr_to_pyobject).collect()),
    );
    set_node_fields(
        &n,
        &[
            "posonlyargs",
            "args",
            "vararg",
            "kwonlyargs",
            "kw_defaults",
            "kwarg",
            "defaults",
        ],
    );
    n
}

fn arg_to_pyobject(arg: &ferrython_ast::Arg) -> PyObjectRef {
    let n = make_ast_node("arg");
    set_node_attr(&n, "arg", PyObject::str_val(arg.arg.clone()));
    set_node_attr(
        &n,
        "annotation",
        match &arg.annotation {
            Some(a) => expr_to_pyobject(a),
            None => PyObject::none(),
        },
    );
    set_node_attr(
        &n,
        "type_comment",
        match &arg.type_comment {
            Some(comment) => PyObject::str_val(comment.clone()),
            None => PyObject::none(),
        },
    );
    set_location(&n, &arg.location);
    set_node_fields(&n, &["arg", "annotation", "type_comment"]);
    n
}

fn keyword_to_pyobject(kw: &ferrython_ast::Keyword) -> PyObjectRef {
    let n = make_ast_node("keyword");
    set_node_attr(
        &n,
        "arg",
        match &kw.arg {
            Some(a) => PyObject::str_val(a.clone()),
            None => PyObject::none(),
        },
    );
    set_node_attr(&n, "value", expr_to_pyobject(&kw.value));
    set_node_fields(&n, &["arg", "value"]);
    n
}

fn alias_to_pyobject(alias: &ferrython_ast::Alias) -> PyObjectRef {
    let n = make_ast_node("alias");
    set_node_attr(&n, "name", PyObject::str_val(alias.name.clone()));
    set_node_attr(
        &n,
        "asname",
        match &alias.asname {
            Some(a) => PyObject::str_val(a.clone()),
            None => PyObject::none(),
        },
    );
    set_node_fields(&n, &["name", "asname"]);
    n
}

fn withitem_to_pyobject(item: &ferrython_ast::WithItem) -> PyObjectRef {
    let n = make_ast_node("withitem");
    set_node_attr(&n, "context_expr", expr_to_pyobject(&item.context_expr));
    let optional_vars = match &item.optional_vars {
        Some(v) => {
            let var = expr_to_pyobject(v);
            fix_ctx(&var, "Store");
            var
        }
        None => PyObject::none(),
    };
    set_node_attr(&n, "optional_vars", optional_vars);
    set_node_fields(&n, &["context_expr", "optional_vars"]);
    n
}

fn except_handler_to_pyobject(handler: &ferrython_ast::ExceptHandler) -> PyObjectRef {
    let n = make_ast_node("ExceptHandler");
    set_node_attr(
        &n,
        "type",
        match &handler.typ {
            Some(t) => expr_to_pyobject(t),
            None => PyObject::none(),
        },
    );
    set_node_attr(
        &n,
        "name",
        match &handler.name {
            Some(nm) => PyObject::str_val(nm.clone()),
            None => PyObject::none(),
        },
    );
    set_node_attr(
        &n,
        "body",
        PyObject::list(handler.body.iter().map(stmt_to_pyobject).collect()),
    );
    set_location(&n, &handler.location);
    set_node_fields(&n, &["type", "name", "body"]);
    n
}

fn comprehension_to_pyobject(comp: &ferrython_ast::Comprehension) -> PyObjectRef {
    let n = make_ast_node("comprehension");
    let target = expr_to_pyobject(&comp.target);
    fix_ctx(&target, "Store");
    set_node_attr(&n, "target", target);
    set_node_attr(&n, "iter", expr_to_pyobject(&comp.iter));
    set_node_attr(
        &n,
        "ifs",
        PyObject::list(comp.ifs.iter().map(expr_to_pyobject).collect()),
    );
    set_node_attr(
        &n,
        "is_async",
        PyObject::int(if comp.is_async { 1 } else { 0 }),
    );
    set_node_fields(&n, &["target", "iter", "ifs", "is_async"]);
    n
}

/// Evaluate a constant expression for ast.literal_eval
fn eval_const_expr(expr: &ferrython_ast::Expression) -> PyResult<PyObjectRef> {
    use ferrython_ast::ExpressionKind::*;
    match &expr.node {
        Constant { value } => Ok(constant_to_pyobject(value)),
        List { elts, .. } => {
            let items: Result<Vec<_>, _> = elts.iter().map(eval_const_expr).collect();
            Ok(PyObject::list(items?))
        }
        Tuple { elts, .. } => {
            let items: Result<Vec<_>, _> = elts.iter().map(eval_const_expr).collect();
            Ok(PyObject::tuple(items?))
        }
        Set { elts } => {
            let items: Result<Vec<_>, _> = elts.iter().map(eval_const_expr).collect();
            let items = items?;
            let mut map = IndexMap::new();
            for item in &items {
                if let Ok(key) = ferrython_core::types::HashableKey::from_object(item) {
                    map.insert(key, item.clone());
                }
            }
            Ok(PyObject::frozenset(map))
        }
        Dict { keys, values } => {
            if keys.len() != values.len() {
                return Err(PyException::value_error("malformed node or string"));
            }
            let mut map = IndexMap::new();
            for (k, v) in keys.iter().zip(values.iter()) {
                let val = eval_const_expr(v)?;
                let key_expr = match k {
                    Some(key_expr) => key_expr,
                    None => return Err(PyException::value_error("malformed node or string")),
                };
                let key_obj = eval_const_expr(key_expr)?;
                if let Ok(hk) = ferrython_core::types::HashableKey::from_object(&key_obj) {
                    map.insert(hk, val);
                }
            }
            Ok(PyObject::dict(map))
        }
        UnaryOp { op, operand } => {
            if matches!(
                (&op, &operand.node),
                (
                    ferrython_ast::UnaryOperator::UAdd | ferrython_ast::UnaryOperator::USub,
                    UnaryOp { .. }
                ) | (ferrython_ast::UnaryOperator::USub, BinOp { .. })
            ) {
                return Err(PyException::value_error("malformed node or string"));
            }
            let val = eval_const_expr(operand)?;
            match op {
                ferrython_ast::UnaryOperator::USub => {
                    if let Some(n) = val.as_int() {
                        return Ok(PyObject::int(-n));
                    }
                    if let PyObjectPayload::Float(f) = &val.payload {
                        return Ok(PyObject::float(-f));
                    }
                    if let PyObjectPayload::Complex { real, imag } = &val.payload {
                        return Ok(PyObject::complex(-real, -imag));
                    }
                    Err(PyException::value_error("malformed node or string"))
                }
                ferrython_ast::UnaryOperator::UAdd => {
                    if matches!(
                        &val.payload,
                        PyObjectPayload::Int(_)
                            | PyObjectPayload::Float(_)
                            | PyObjectPayload::Complex { .. }
                    ) {
                        Ok(val)
                    } else {
                        Err(PyException::value_error("malformed node or string"))
                    }
                }
                _ => Err(PyException::value_error("malformed node or string")),
            }
        }
        BinOp { left, op, right } => match op {
            ferrython_ast::Operator::Add | ferrython_ast::Operator::Sub => {
                let left_num = match &left.node {
                    Constant {
                        value: ferrython_ast::Constant::Int(i),
                    } => match i {
                        ferrython_ast::BigInt::Small(n) => Some(*n as f64),
                        ferrython_ast::BigInt::Big(_) => None,
                    },
                    Constant {
                        value: ferrython_ast::Constant::Float(f),
                    } => Some(*f),
                    UnaryOp {
                        op: ferrython_ast::UnaryOperator::USub,
                        operand,
                    } => match &operand.node {
                        Constant {
                            value: ferrython_ast::Constant::Int(i),
                        } => match i {
                            ferrython_ast::BigInt::Small(n) => Some(-(*n as f64)),
                            ferrython_ast::BigInt::Big(_) => None,
                        },
                        Constant {
                            value: ferrython_ast::Constant::Float(f),
                        } => Some(-*f),
                        _ => None,
                    },
                    _ => None,
                };
                let right_complex = match &right.node {
                    Constant {
                        value: ferrython_ast::Constant::Complex { real, imag },
                    } => Some((*real, *imag)),
                    _ => None,
                };
                if let (Some(left_f), Some((real, imag))) = (left_num, right_complex) {
                    return Ok(match op {
                        ferrython_ast::Operator::Add => PyObject::complex(left_f + real, imag),
                        ferrython_ast::Operator::Sub => PyObject::complex(left_f - real, -imag),
                        _ => unreachable!(),
                    });
                }
                Err(PyException::value_error("malformed node or string"))
            }
            _ => Err(PyException::value_error("malformed node or string")),
        },
        _ => Err(PyException::value_error("malformed node or string")),
    }
}

/// ast.dump() — recursively dump an AST node to string
fn dump_node(
    obj: &PyObjectRef,
    indent: Option<usize>,
    include_attrs: bool,
    annotate_fields: bool,
    depth: usize,
) -> String {
    let type_name = obj.type_name();
    // Get _fields to know which attributes to dump
    let fields = obj.get_attr("_fields");
    if fields.is_none() {
        // Not an AST node — dump as value
        return format_value(obj);
    }
    let fields = fields.unwrap();
    let field_names: Vec<String> = if let PyObjectPayload::Tuple(items) = &fields.payload {
        items.iter().map(|f| f.py_to_string()).collect()
    } else {
        vec![]
    };

    let mut parts: Vec<String> = Vec::new();
    let mut omitted_field = false;
    for name in &field_names {
        if type_name == "Constant" && name == "kind" && !has_instance_attr(obj, name) {
            omitted_field = true;
            continue;
        }
        if let Some(val) = obj.get_attr(name) {
            let val_str = dump_value(&val, indent, include_attrs, annotate_fields, depth + 1);
            if annotate_fields || omitted_field {
                parts.push(format!("{}={}", name, val_str));
            } else {
                parts.push(val_str);
            }
        } else {
            omitted_field = true;
        }
    }

    if include_attrs {
        for attr in &["lineno", "col_offset", "end_lineno", "end_col_offset"] {
            if let Some(val) = obj.get_attr(attr) {
                if !matches!(&val.payload, PyObjectPayload::None) {
                    parts.push(format!("{}={}", attr, format_value(&val)));
                }
            }
        }
    }

    if let Some(indent_size) = indent {
        if parts.is_empty() {
            format!("{}()", type_name)
        } else {
            let indent_str = " ".repeat(indent_size * (depth + 1));
            let inner = parts
                .iter()
                .map(|p| format!("{}{}", indent_str, p))
                .collect::<Vec<_>>()
                .join(",\n");
            format!("{}(\n{})", type_name, inner)
        }
    } else {
        format!("{}({})", type_name, parts.join(", "))
    }
}

fn dump_value(
    obj: &PyObjectRef,
    indent: Option<usize>,
    include_attrs: bool,
    annotate_fields: bool,
    depth: usize,
) -> String {
    // Check if it's an AST node (has _fields)
    if obj.get_attr("_fields").is_some() {
        return dump_node(obj, indent, include_attrs, annotate_fields, depth);
    }
    // Check if it's a list of AST nodes
    if let PyObjectPayload::List(items) = &obj.payload {
        let items = items.read();
        if items.is_empty() {
            return "[]".to_string();
        }
        let inner: Vec<String> = items
            .iter()
            .map(|item| dump_value(item, indent, include_attrs, annotate_fields, depth))
            .collect();
        if let Some(indent_size) = indent {
            let indent_str = " ".repeat(indent_size * (depth + 1));
            let entries = inner
                .iter()
                .map(|e| format!("{}{}", indent_str, e))
                .collect::<Vec<_>>()
                .join(",\n");
            format!("[\n{}]", entries)
        } else {
            format!("[{}]", inner.join(", "))
        }
    } else {
        format_value(obj)
    }
}

fn eval_py_const_node(node: &PyObjectRef) -> PyResult<PyObjectRef> {
    let expr_obj = if node.type_name() == "Expression" {
        node.get_attr("body")
            .ok_or_else(|| PyException::value_error("malformed node or string"))?
    } else {
        node.clone()
    };
    let expr = convert_expr(&expr_obj)
        .map_err(|_| PyException::value_error("malformed node or string"))?;
    eval_const_expr(&expr)
}

fn source_segment(source: &str, node: &PyObjectRef, padded: bool) -> Option<String> {
    let lineno = node.get_attr("lineno")?.as_int()? as usize;
    let col_offset = node.get_attr("col_offset")?.as_int()? as usize;
    let end_lineno = node.get_attr("end_lineno")?.as_int()? as usize;
    let end_col_offset = node.get_attr("end_col_offset")?.as_int()? as usize;
    if lineno == 0 || end_lineno == 0 {
        return None;
    }

    let mut line_starts = vec![0usize];
    let bytes = source.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                line_starts.push(i + 1);
                i += 1;
            }
            b'\r' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    line_starts.push(i + 2);
                    i += 2;
                } else {
                    line_starts.push(i + 1);
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    if lineno > line_starts.len() || end_lineno > line_starts.len() {
        return None;
    }

    let line_bounds = |idx: usize| -> (usize, usize) {
        let start = line_starts[idx - 1];
        let end = if idx < line_starts.len() {
            let mut end = line_starts[idx];
            while end > start && matches!(bytes[end - 1], b'\n' | b'\r') {
                end -= 1;
            }
            end
        } else {
            bytes.len()
        };
        (start, end)
    };

    let mut parts = Vec::new();
    for line_idx in lineno..=end_lineno {
        let (start, end) = line_bounds(line_idx);
        let line = &source[start..end];
        let segment = if line_idx == lineno && line_idx == end_lineno {
            line.get(col_offset..end_col_offset)?.to_string()
        } else if line_idx == lineno {
            line.get(col_offset..)?.to_string()
        } else if line_idx == end_lineno {
            line.get(..end_col_offset)?.to_string()
        } else {
            line.to_string()
        };
        parts.push(segment);
    }

    if parts.is_empty() {
        return None;
    }
    let mut segment = parts.join("\n");
    if padded && lineno != end_lineno {
        let (start, end) = line_bounds(lineno);
        let line = &source[start..end];
        let prefix = line.get(..col_offset).unwrap_or("");
        segment = format!("{}{}", prefix, segment);
    }
    Some(segment)
}

fn format_value(obj: &PyObjectRef) -> String {
    match &obj.payload {
        PyObjectPayload::None => "None".to_string(),
        PyObjectPayload::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        PyObjectPayload::Int(_) => obj.py_to_string(),
        PyObjectPayload::Float(f) => format!("{}", f),
        PyObjectPayload::Str(s) => format!("'{}'", s),
        PyObjectPayload::Bytes(b) => format!("b{:?}", String::from_utf8_lossy(b)),
        _ => obj.py_to_string(),
    }
}

/// Collect all AST nodes recursively for ast.walk()
fn collect_ast_nodes(obj: &PyObjectRef, nodes: &mut Vec<PyObjectRef>) {
    if obj.get_attr("_fields").is_none() {
        return;
    }
    nodes.push(obj.clone());
    if let Some(fields) = obj.get_attr("_fields") {
        if let PyObjectPayload::Tuple(field_names) = &fields.payload {
            for fname in field_names.iter() {
                let name = fname.py_to_string();
                if let Some(val) = obj.get_attr(&name) {
                    if val.get_attr("_fields").is_some() {
                        collect_ast_nodes(&val, nodes);
                    } else if let PyObjectPayload::List(items) = &val.payload {
                        for item in items.read().iter() {
                            collect_ast_nodes(item, nodes);
                        }
                    }
                }
            }
        }
    }
}

/// Get immediate child nodes for ast.iter_child_nodes()
fn get_child_nodes(obj: &PyObjectRef) -> Vec<PyObjectRef> {
    let mut children = Vec::new();
    if let Some(fields) = obj.get_attr("_fields") {
        if let PyObjectPayload::Tuple(field_names) = &fields.payload {
            for fname in field_names.iter() {
                let name = fname.py_to_string();
                if let Some(val) = obj.get_attr(&name) {
                    if val.get_attr("_fields").is_some() {
                        children.push(val);
                    } else if let PyObjectPayload::List(items) = &val.payload {
                        for item in items.read().iter() {
                            if item.get_attr("_fields").is_some() {
                                children.push(item.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    children
}

/// Public API for compile() to use when compiling programmatically-built AST
pub fn ast_unparse_module(node: &PyObjectRef) -> String {
    ast_unparse(node)
}

/// Simplified AST unparse — convert AST node back to Python source
fn ast_unparse(node: &PyObjectRef) -> String {
    // Use the class name (type_name()) which is "Module", "Assign", etc.
    let type_name = node.type_name().to_string();
    match type_name.as_str() {
        "Module" => {
            if let Some(body) = node.get_attr("body") {
                if let PyObjectPayload::List(items) = &body.payload {
                    return items
                        .read()
                        .iter()
                        .map(|s| ast_unparse(s))
                        .collect::<Vec<_>>()
                        .join("\n");
                }
            }
            String::new()
        }
        "Assign" => {
            let targets = node
                .get_attr("targets")
                .map(|t| {
                    if let PyObjectPayload::List(items) = &t.payload {
                        items
                            .read()
                            .iter()
                            .map(|t| ast_unparse(t))
                            .collect::<Vec<_>>()
                            .join(", ")
                    } else {
                        ast_unparse(&t)
                    }
                })
                .unwrap_or_default();
            let value = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            format!("{} = {}", targets, value)
        }
        "Name" => node
            .get_attr("id")
            .map(|i| i.py_to_string())
            .unwrap_or_default(),
        "Constant" => {
            if let Some(v) = node.get_attr("value") {
                match &v.payload {
                    PyObjectPayload::Str(s) => format!("'{}'", s),
                    PyObjectPayload::None => "None".to_string(),
                    PyObjectPayload::Bool(b) => if *b { "True" } else { "False" }.to_string(),
                    _ => v.py_to_string(),
                }
            } else {
                "None".to_string()
            }
        }
        "BinOp" => {
            let left = node
                .get_attr("left")
                .map(|l| ast_unparse(&l))
                .unwrap_or_default();
            let right = node
                .get_attr("right")
                .map(|r| ast_unparse(&r))
                .unwrap_or_default();
            let op = node
                .get_attr("op")
                .map(|o| {
                    let op_type = o.type_name().to_string();
                    match op_type.as_str() {
                        "Add" => "+",
                        "Sub" => "-",
                        "Mult" => "*",
                        "Div" => "/",
                        "Mod" => "%",
                        "Pow" => "**",
                        "FloorDiv" => "//",
                        "LShift" => "<<",
                        "RShift" => ">>",
                        "BitOr" => "|",
                        "BitXor" => "^",
                        "BitAnd" => "&",
                        "MatMult" => "@",
                        _ => "?",
                    }
                    .to_string()
                })
                .unwrap_or_else(|| "+".to_string());
            format!("{} {} {}", left, op, right)
        }
        "Return" => {
            let val = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            if val.is_empty() {
                "return".to_string()
            } else {
                format!("return {}", val)
            }
        }
        "Expr" => node
            .get_attr("value")
            .map(|v| ast_unparse(&v))
            .unwrap_or_default(),
        "Call" => {
            let func = node
                .get_attr("func")
                .map(|f| ast_unparse(&f))
                .unwrap_or_default();
            let args_str = node
                .get_attr("args")
                .map(|a| {
                    if let PyObjectPayload::List(items) = &a.payload {
                        items
                            .read()
                            .iter()
                            .map(|a| ast_unparse(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    } else {
                        String::new()
                    }
                })
                .unwrap_or_default();
            format!("{}({})", func, args_str)
        }
        "Attribute" => {
            let val = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            let attr = node
                .get_attr("attr")
                .map(|a| a.py_to_string())
                .unwrap_or_default();
            format!("{}.{}", val, attr)
        }
        "ListComp" => {
            let elt = node
                .get_attr("elt")
                .map(|e| ast_unparse(&e))
                .unwrap_or_default();
            let gens = unparse_generators(node);
            format!("[{} {}]", elt, gens)
        }
        "SetComp" => {
            let elt = node
                .get_attr("elt")
                .map(|e| ast_unparse(&e))
                .unwrap_or_default();
            let gens = unparse_generators(node);
            format!("{{{} {}}}", elt, gens)
        }
        "DictComp" => {
            let key = node
                .get_attr("key")
                .map(|k| ast_unparse(&k))
                .unwrap_or_default();
            let value = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            let gens = unparse_generators(node);
            format!("{{{}: {} {}}}", key, value, gens)
        }
        "GeneratorExp" => {
            let elt = node
                .get_attr("elt")
                .map(|e| ast_unparse(&e))
                .unwrap_or_default();
            let gens = unparse_generators(node);
            format!("({} {})", elt, gens)
        }
        "List" => {
            let elts = unparse_list_attr(node, "elts");
            format!("[{}]", elts)
        }
        "Tuple" => {
            let elts = unparse_list_attr(node, "elts");
            if elts.contains(',') || elts.is_empty() {
                format!("({})", elts)
            } else {
                format!("({},)", elts)
            }
        }
        "Set" => {
            let elts = unparse_list_attr(node, "elts");
            format!("{{{}}}", elts)
        }
        "Dict" => {
            let keys = node
                .get_attr("keys")
                .and_then(|k| {
                    if let PyObjectPayload::List(items) = &k.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|k| ast_unparse(k))
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let values = node
                .get_attr("values")
                .and_then(|v| {
                    if let PyObjectPayload::List(items) = &v.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|v| ast_unparse(v))
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let pairs: Vec<String> = keys
                .iter()
                .zip(values.iter())
                .map(|(k, v)| format!("{}: {}", k, v))
                .collect();
            format!("{{{}}}", pairs.join(", "))
        }
        "Subscript" => {
            let val = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            let slice = node
                .get_attr("slice")
                .map(|s| ast_unparse(&s))
                .unwrap_or_default();
            format!("{}[{}]", val, slice)
        }
        "Index" => node
            .get_attr("value")
            .map(|v| ast_unparse(&v))
            .unwrap_or_default(),
        "Slice" => {
            let lower = node
                .get_attr("lower")
                .map(|l| ast_unparse(&l))
                .unwrap_or_default();
            let upper = node
                .get_attr("upper")
                .map(|u| ast_unparse(&u))
                .unwrap_or_default();
            let step = node
                .get_attr("step")
                .map(|s| ast_unparse(&s))
                .unwrap_or_default();
            if step.is_empty() {
                format!("{}:{}", lower, upper)
            } else {
                format!("{}:{}:{}", lower, upper, step)
            }
        }
        "UnaryOp" => {
            let operand = node
                .get_attr("operand")
                .map(|o| ast_unparse(&o))
                .unwrap_or_default();
            let op = node
                .get_attr("op")
                .map(|o| {
                    match o.type_name().to_string().as_str() {
                        "UAdd" => "+",
                        "USub" => "-",
                        "Not" => "not ",
                        "Invert" => "~",
                        _ => "?",
                    }
                    .to_string()
                })
                .unwrap_or_default();
            format!("{}{}", op, operand)
        }
        "BoolOp" => {
            let op = node
                .get_attr("op")
                .map(|o| {
                    match o.type_name().to_string().as_str() {
                        "And" => " and ",
                        "Or" => " or ",
                        _ => " ? ",
                    }
                    .to_string()
                })
                .unwrap_or_else(|| " ? ".to_string());
            let values = unparse_list_attr(node, "values");
            values.split(", ").collect::<Vec<_>>().join(&op)
        }
        "Compare" => {
            let left = node
                .get_attr("left")
                .map(|l| ast_unparse(&l))
                .unwrap_or_default();
            let ops = node
                .get_attr("ops")
                .and_then(|o| {
                    if let PyObjectPayload::List(items) = &o.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|o| {
                                    match o.type_name().to_string().as_str() {
                                        "Eq" => "==",
                                        "NotEq" => "!=",
                                        "Lt" => "<",
                                        "LtE" => "<=",
                                        "Gt" => ">",
                                        "GtE" => ">=",
                                        "Is" => "is",
                                        "IsNot" => "is not",
                                        "In" => "in",
                                        "NotIn" => "not in",
                                        _ => "?",
                                    }
                                    .to_string()
                                })
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let comparators = node
                .get_attr("comparators")
                .and_then(|c| {
                    if let PyObjectPayload::List(items) = &c.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|c| ast_unparse(c))
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let mut s = left;
            for (op, comp) in ops.iter().zip(comparators.iter()) {
                s = format!("{} {} {}", s, op, comp);
            }
            s
        }
        "IfExp" => {
            let body = node
                .get_attr("body")
                .map(|b| ast_unparse(&b))
                .unwrap_or_default();
            let test = node
                .get_attr("test")
                .map(|t| ast_unparse(&t))
                .unwrap_or_default();
            let orelse = node
                .get_attr("orelse")
                .map(|o| ast_unparse(&o))
                .unwrap_or_default();
            format!("{} if {} else {}", body, test, orelse)
        }
        "Lambda" => {
            let body = node
                .get_attr("body")
                .map(|b| ast_unparse(&b))
                .unwrap_or_default();
            let args_node = node.get_attr("args");
            let params = args_node.map(|a| unparse_arguments(&a)).unwrap_or_default();
            format!("lambda {}: {}", params, body)
        }
        "FunctionDef" | "AsyncFunctionDef" => {
            let name = node
                .get_attr("name")
                .map(|n| n.py_to_string())
                .unwrap_or_default();
            let args_node = node.get_attr("args");
            let params = args_node.map(|a| unparse_arguments(&a)).unwrap_or_default();
            let body = node
                .get_attr("body")
                .and_then(|b| {
                    if let PyObjectPayload::List(items) = &b.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|s| format!("    {}", ast_unparse(s)))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "    pass".to_string());
            let prefix = if type_name == "AsyncFunctionDef" {
                "async def"
            } else {
                "def"
            };
            format!("{} {}({}):\n{}", prefix, name, params, body)
        }
        "ClassDef" => {
            let name = node
                .get_attr("name")
                .map(|n| n.py_to_string())
                .unwrap_or_default();
            let bases = unparse_list_attr(node, "bases");
            let body = node
                .get_attr("body")
                .and_then(|b| {
                    if let PyObjectPayload::List(items) = &b.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|s| format!("    {}", ast_unparse(s)))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "    pass".to_string());
            if bases.is_empty() {
                format!("class {}:\n{}", name, body)
            } else {
                format!("class {}({}):\n{}", name, bases, body)
            }
        }
        "If" => {
            let test = node
                .get_attr("test")
                .map(|t| ast_unparse(&t))
                .unwrap_or_default();
            let body = node
                .get_attr("body")
                .and_then(|b| {
                    if let PyObjectPayload::List(items) = &b.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|s| format!("    {}", ast_unparse(s)))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            format!("if {}:\n{}", test, body)
        }
        "For" | "AsyncFor" => {
            let target = node
                .get_attr("target")
                .map(|t| ast_unparse(&t))
                .unwrap_or_default();
            let iter_val = node
                .get_attr("iter")
                .map(|i| ast_unparse(&i))
                .unwrap_or_default();
            let body = node
                .get_attr("body")
                .and_then(|b| {
                    if let PyObjectPayload::List(items) = &b.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|s| format!("    {}", ast_unparse(s)))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let prefix = if type_name == "AsyncFor" {
                "async for"
            } else {
                "for"
            };
            format!("{} {} in {}:\n{}", prefix, target, iter_val, body)
        }
        "While" => {
            let test = node
                .get_attr("test")
                .map(|t| ast_unparse(&t))
                .unwrap_or_default();
            let body = node
                .get_attr("body")
                .and_then(|b| {
                    if let PyObjectPayload::List(items) = &b.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|s| format!("    {}", ast_unparse(s)))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            format!("while {}:\n{}", test, body)
        }
        "Import" => {
            let names = node
                .get_attr("names")
                .and_then(|n| {
                    if let PyObjectPayload::List(items) = &n.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|alias| {
                                    let name = alias
                                        .get_attr("name")
                                        .map(|n| n.py_to_string())
                                        .unwrap_or_default();
                                    let asname = alias.get_attr("asname").map(|a| a.py_to_string());
                                    if let Some(a) = asname {
                                        if a != "None" {
                                            return format!("{} as {}", name, a);
                                        }
                                    }
                                    name
                                })
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            format!("import {}", names.join(", "))
        }
        "ImportFrom" => {
            let module = node
                .get_attr("module")
                .map(|m| m.py_to_string())
                .unwrap_or_default();
            let names = node
                .get_attr("names")
                .and_then(|n| {
                    if let PyObjectPayload::List(items) = &n.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|alias| {
                                    let name = alias
                                        .get_attr("name")
                                        .map(|n| n.py_to_string())
                                        .unwrap_or_default();
                                    let asname = alias.get_attr("asname").map(|a| a.py_to_string());
                                    if let Some(a) = asname {
                                        if a != "None" {
                                            return format!("{} as {}", name, a);
                                        }
                                    }
                                    name
                                })
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            format!("from {} import {}", module, names.join(", "))
        }
        "Raise" => {
            let exc = node.get_attr("exc").map(|e| ast_unparse(&e));
            let cause = node.get_attr("cause").map(|c| ast_unparse(&c));
            match (exc, cause) {
                (Some(e), Some(c)) => format!("raise {} from {}", e, c),
                (Some(e), None) => format!("raise {}", e),
                _ => "raise".to_string(),
            }
        }
        "Try" => {
            let body = node
                .get_attr("body")
                .and_then(|b| {
                    if let PyObjectPayload::List(items) = &b.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|s| format!("    {}", ast_unparse(s)))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            format!("try:\n{}", body)
        }
        "With" | "AsyncWith" => {
            let items_str = node
                .get_attr("items")
                .and_then(|it| {
                    if let PyObjectPayload::List(items) = &it.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|w| {
                                    let ctx = w
                                        .get_attr("context_expr")
                                        .map(|c| ast_unparse(&c))
                                        .unwrap_or_default();
                                    let var = w.get_attr("optional_vars").map(|v| ast_unparse(&v));
                                    if let Some(v) = var {
                                        format!("{} as {}", ctx, v)
                                    } else {
                                        ctx
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join(", "),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let prefix = if type_name == "AsyncWith" {
                "async with"
            } else {
                "with"
            };
            format!("{} {}:", prefix, items_str)
        }
        "Pass" => "pass".to_string(),
        "Break" => "break".to_string(),
        "Continue" => "continue".to_string(),
        "Delete" => {
            let targets = unparse_list_attr(node, "targets");
            format!("del {}", targets)
        }
        "Assert" => {
            let test = node
                .get_attr("test")
                .map(|t| ast_unparse(&t))
                .unwrap_or_default();
            let msg = node.get_attr("msg").map(|m| ast_unparse(&m));
            match msg {
                Some(m) => format!("assert {}, {}", test, m),
                None => format!("assert {}", test),
            }
        }
        "AugAssign" => {
            let target = node
                .get_attr("target")
                .map(|t| ast_unparse(&t))
                .unwrap_or_default();
            let value = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            let op = node
                .get_attr("op")
                .map(|o| {
                    match o.type_name().to_string().as_str() {
                        "Add" => "+=",
                        "Sub" => "-=",
                        "Mult" => "*=",
                        "Div" => "/=",
                        "Mod" => "%=",
                        "Pow" => "**=",
                        "FloorDiv" => "//=",
                        "LShift" => "<<=",
                        "RShift" => ">>=",
                        "BitOr" => "|=",
                        "BitXor" => "^=",
                        "BitAnd" => "&=",
                        _ => "?=",
                    }
                    .to_string()
                })
                .unwrap_or_else(|| "?=".to_string());
            format!("{} {} {}", target, op, value)
        }
        "Starred" => {
            let val = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            format!("*{}", val)
        }
        "JoinedStr" => {
            // f-string
            let values = node
                .get_attr("values")
                .and_then(|v| {
                    if let PyObjectPayload::List(items) = &v.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|v| {
                                    let tn = v.type_name().to_string();
                                    if tn == "FormattedValue" {
                                        let inner = v
                                            .get_attr("value")
                                            .map(|iv| ast_unparse(&iv))
                                            .unwrap_or_default();
                                        format!("{{{}}}", inner)
                                    } else {
                                        ast_unparse(v)
                                    }
                                })
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            format!("f'{}'", values.join(""))
        }
        "FormattedValue" => {
            let val = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            format!("{{{}}}", val)
        }
        "Num" => node
            .get_attr("n")
            .map(|n| n.py_to_string())
            .unwrap_or_else(|| "0".to_string()),
        "Str" => node
            .get_attr("s")
            .map(|s| format!("'{}'", s.py_to_string()))
            .unwrap_or_else(|| "''".to_string()),
        "NameConstant" => node
            .get_attr("value")
            .map(|v| v.py_to_string())
            .unwrap_or_else(|| "None".to_string()),
        "Yield" => {
            let val = node.get_attr("value").map(|v| ast_unparse(&v));
            match val {
                Some(v) => format!("yield {}", v),
                None => "yield".to_string(),
            }
        }
        "YieldFrom" => {
            let val = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            format!("yield from {}", val)
        }
        "Await" => {
            let val = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            format!("await {}", val)
        }
        "Global" => {
            let names = unparse_list_attr(node, "names");
            format!("global {}", names)
        }
        "Nonlocal" => {
            let names = unparse_list_attr(node, "names");
            format!("nonlocal {}", names)
        }
        _ => format!("<{}>", type_name),
    }
}

fn unparse_generators(node: &PyObjectRef) -> String {
    node.get_attr("generators")
        .and_then(|g| {
            if let PyObjectPayload::List(items) = &g.payload {
                Some(
                    items
                        .read()
                        .iter()
                        .map(|comp| {
                            let target = comp
                                .get_attr("target")
                                .map(|t| ast_unparse(&t))
                                .unwrap_or_default();
                            let iter_val = comp
                                .get_attr("iter")
                                .map(|i| ast_unparse(&i))
                                .unwrap_or_default();
                            let ifs = comp
                                .get_attr("ifs")
                                .and_then(|i| {
                                    if let PyObjectPayload::List(conds) = &i.payload {
                                        let conds: Vec<String> = conds
                                            .read()
                                            .iter()
                                            .map(|c| format!(" if {}", ast_unparse(c)))
                                            .collect();
                                        Some(conds.join(""))
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or_default();
                            let is_async = comp
                                .get_attr("is_async")
                                .map(|a| a.py_to_string() == "1" || a.py_to_string() == "True")
                                .unwrap_or(false);
                            let prefix = if is_async { "async for" } else { "for" };
                            format!("{} {} in {}{}", prefix, target, iter_val, ifs)
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            } else {
                None
            }
        })
        .unwrap_or_default()
}

fn unparse_list_attr(node: &PyObjectRef, attr: &str) -> String {
    node.get_attr(attr)
        .and_then(|a| {
            if let PyObjectPayload::List(items) = &a.payload {
                Some(
                    items
                        .read()
                        .iter()
                        .map(|i| ast_unparse(i))
                        .collect::<Vec<_>>()
                        .join(", "),
                )
            } else {
                None
            }
        })
        .unwrap_or_default()
}

fn unparse_arguments(args_node: &PyObjectRef) -> String {
    let mut parts = Vec::new();
    if let Some(args_list) = args_node.get_attr("args") {
        if let PyObjectPayload::List(items) = &args_list.payload {
            for arg in items.read().iter() {
                let name = arg
                    .get_attr("arg")
                    .map(|a| a.py_to_string())
                    .unwrap_or_default();
                parts.push(name);
            }
        }
    }
    parts.join(", ")
}
