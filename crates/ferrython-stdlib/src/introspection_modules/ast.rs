use super::*;
mod nodes;
pub(crate) use nodes::ast_empty_fields_node_names;
use nodes::*;
mod to_py;
pub use to_py::module_ast_to_pyobject;
use to_py::*;
mod tools;
use tools::*;
mod unparse;
pub use unparse::ast_unparse_module;
use unparse::*;

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
            ns.insert(
                CompactString::from("__module__"),
                PyObject::str_val(CompactString::from("ast")),
            );
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
