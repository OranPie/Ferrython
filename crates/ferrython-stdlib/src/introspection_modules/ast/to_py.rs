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

mod helpers;
mod statement;

pub(super) use helpers::constant_to_pyobject;
use helpers::*;
use statement::stmt_to_pyobject;

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
