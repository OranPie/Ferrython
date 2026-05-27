use super::*;

pub(super) fn stmt_to_pyobject(stmt: &ferrython_ast::Statement) -> PyObjectRef {
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
