use super::*;

pub(super) fn slice_to_pyobject(slice: &ferrython_ast::Expression) -> PyObjectRef {
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

pub(super) fn slice_dim_to_pyobject(dim: &ferrython_ast::Expression) -> PyObjectRef {
    if matches!(dim.node, ferrython_ast::ExpressionKind::Slice { .. }) {
        expr_to_pyobject(dim)
    } else {
        let n = make_ast_node("Index");
        set_node_attr(&n, "value", expr_to_pyobject(dim));
        set_node_fields(&n, &["value"]);
        n
    }
}

pub(in crate::introspection_modules::ast) fn constant_to_pyobject(
    c: &ferrython_ast::Constant,
) -> PyObjectRef {
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

pub(super) fn operator_to_pyobject(op: ferrython_ast::Operator) -> PyObjectRef {
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

pub(super) fn boolop_to_pyobject(op: ferrython_ast::BoolOperator) -> PyObjectRef {
    make_ast_node(match op {
        ferrython_ast::BoolOperator::And => "And",
        ferrython_ast::BoolOperator::Or => "Or",
    })
}

pub(super) fn unaryop_to_pyobject(op: ferrython_ast::UnaryOperator) -> PyObjectRef {
    use ferrython_ast::UnaryOperator::*;
    make_ast_node(match op {
        Invert => "Invert",
        Not => "Not",
        UAdd => "UAdd",
        USub => "USub",
    })
}

pub(super) fn cmpop_to_pyobject(op: ferrython_ast::CompareOperator) -> PyObjectRef {
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

pub(super) fn ctx_to_pyobject(ctx: ferrython_ast::ExprContext) -> PyObjectRef {
    make_ast_node(match ctx {
        ferrython_ast::ExprContext::Load => "Load",
        ferrython_ast::ExprContext::Store => "Store",
        ferrython_ast::ExprContext::Del => "Del",
    })
}

pub(super) fn args_to_pyobject(args: &ferrython_ast::Arguments) -> PyObjectRef {
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

pub(super) fn arg_to_pyobject(arg: &ferrython_ast::Arg) -> PyObjectRef {
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

pub(super) fn keyword_to_pyobject(kw: &ferrython_ast::Keyword) -> PyObjectRef {
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

pub(super) fn alias_to_pyobject(alias: &ferrython_ast::Alias) -> PyObjectRef {
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

pub(super) fn withitem_to_pyobject(item: &ferrython_ast::WithItem) -> PyObjectRef {
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

pub(super) fn except_handler_to_pyobject(handler: &ferrython_ast::ExceptHandler) -> PyObjectRef {
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

pub(super) fn comprehension_to_pyobject(comp: &ferrython_ast::Comprehension) -> PyObjectRef {
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
