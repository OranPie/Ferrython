use super::*;

pub(super) fn ast_base_names(name: &str) -> &'static [&'static str] {
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

pub(super) fn ast_node_supports_location(name: &str) -> bool {
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

pub(super) fn set_node_attr(obj: &PyObjectRef, name: &str, value: PyObjectRef) {
    if let PyObjectPayload::Instance(ref d) = obj.payload {
        d.attrs.write().insert(CompactString::from(name), value);
    }
}

pub(super) fn has_instance_attr(obj: &PyObjectRef, name: &str) -> bool {
    if let PyObjectPayload::Instance(ref d) = obj.payload {
        d.attrs.read().contains_key(&CompactString::from(name))
    } else {
        false
    }
}

pub(super) fn set_node_fields(obj: &PyObjectRef, fields: &[&str]) {
    let flds: Vec<PyObjectRef> = fields
        .iter()
        .map(|f| PyObject::str_val(CompactString::from(*f)))
        .collect();
    set_node_attr(obj, "_fields", PyObject::tuple(flds));
}

pub(super) fn set_location(obj: &PyObjectRef, loc: &ferrython_ast::SourceLocation) {
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

pub(super) fn make_ast_node(type_name: &str) -> PyObjectRef {
    let cls = get_or_create_ast_class(type_name);
    PyObject::instance(cls)
}

/// Get or create a shared AST class, so isinstance(ast.parse(...), ast.Module) works
pub(super) fn get_or_create_ast_class(name: &str) -> PyObjectRef {
    get_or_create_ast_class_with_bases(name, Vec::new())
}

pub(super) fn get_or_create_ast_class_with_bases(
    name: &str,
    bases: Vec<PyObjectRef>,
) -> PyObjectRef {
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
pub(super) fn fix_ctx(node: &PyObjectRef, ctx_name: &str) {
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
