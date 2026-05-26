use super::nodes::*;
use super::*;

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

pub(super) fn module_to_pyobject(module: &ferrython_ast::Module) -> PyObjectRef {
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

pub(super) fn expr_to_pyobject(expr: &ferrython_ast::Expression) -> PyObjectRef {
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

pub(super) fn constant_to_pyobject(c: &ferrython_ast::Constant) -> PyObjectRef {
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
