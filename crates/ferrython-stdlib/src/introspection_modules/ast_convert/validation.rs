use super::*;

pub(super) fn require_load_context(expr: &AstExpression) -> Result<(), String> {
    match &expr.node {
        ExpressionKind::BoolOp { values, .. } => {
            for value in values {
                require_load_context(value)?;
            }
        }
        ExpressionKind::NamedExpr { target, value } => {
            require_store_context(target)?;
            require_load_context(value)?;
        }
        ExpressionKind::BinOp { left, right, .. } => {
            require_load_context(left)?;
            require_load_context(right)?;
        }
        ExpressionKind::UnaryOp { operand, .. } => require_load_context(operand)?,
        ExpressionKind::Lambda { args, body } => {
            validate_arguments(args)?;
            require_load_context(body)?;
        }
        ExpressionKind::IfExp { test, body, orelse } => {
            require_load_context(test)?;
            require_load_context(body)?;
            require_load_context(orelse)?;
        }
        ExpressionKind::Dict { keys, values } => {
            for key in keys.iter().flatten() {
                require_load_context(key)?;
            }
            for value in values {
                require_load_context(value)?;
            }
        }
        ExpressionKind::Set { elts } => {
            for elt in elts {
                require_load_context(elt)?;
            }
        }
        ExpressionKind::ListComp { elt, generators }
        | ExpressionKind::SetComp { elt, generators }
        | ExpressionKind::GeneratorExp { elt, generators } => {
            require_load_context(elt)?;
            validate_comprehensions(generators)?;
        }
        ExpressionKind::DictComp {
            key,
            value,
            generators,
        } => {
            require_load_context(key)?;
            require_load_context(value)?;
            validate_comprehensions(generators)?;
        }
        ExpressionKind::Await { value } => require_load_context(value)?,
        ExpressionKind::Yield { value } => {
            if let Some(value) = value {
                require_load_context(value)?;
            }
        }
        ExpressionKind::YieldFrom { value } => require_load_context(value)?,
        ExpressionKind::Compare {
            left, comparators, ..
        } => {
            require_load_context(left)?;
            for comparator in comparators {
                require_load_context(comparator)?;
            }
        }
        ExpressionKind::Call {
            func,
            args,
            keywords,
        } => {
            require_load_context(func)?;
            for arg in args {
                require_load_context(arg)?;
            }
            for keyword in keywords {
                require_load_context(&keyword.value)?;
            }
        }
        ExpressionKind::FormattedValue {
            value, format_spec, ..
        } => {
            require_load_context(value)?;
            if let Some(format_spec) = format_spec {
                require_load_context(format_spec)?;
            }
        }
        ExpressionKind::JoinedStr { values } => {
            for value in values {
                require_load_context(value)?;
            }
        }
        ExpressionKind::Attribute { value, ctx, .. } => {
            require_context(*ctx, ExprContext::Load)?;
            require_load_context(value)?;
        }
        ExpressionKind::Subscript {
            value, slice, ctx, ..
        } => {
            require_context(*ctx, ExprContext::Load)?;
            require_load_context(value)?;
            require_load_context(slice)?;
        }
        ExpressionKind::Starred { value, ctx } => {
            require_context(*ctx, ExprContext::Load)?;
            require_load_context(value)?;
        }
        ExpressionKind::Name { ctx, .. } => require_context(*ctx, ExprContext::Load)?,
        ExpressionKind::List { elts, ctx } | ExpressionKind::Tuple { elts, ctx } => {
            require_context(*ctx, ExprContext::Load)?;
            for elt in elts {
                require_load_context(elt)?;
            }
        }
        ExpressionKind::Slice { lower, upper, step } => {
            if let Some(lower) = lower {
                require_load_context(lower)?;
            }
            if let Some(upper) = upper {
                require_load_context(upper)?;
            }
            if let Some(step) = step {
                require_load_context(step)?;
            }
        }
        ExpressionKind::Constant { .. } => {}
    }
    Ok(())
}

pub(super) fn require_store_context(expr: &AstExpression) -> Result<(), String> {
    match &expr.node {
        ExpressionKind::Name { ctx, .. } => require_context(*ctx, ExprContext::Store)?,
        ExpressionKind::Attribute { value, ctx, .. } => {
            require_context(*ctx, ExprContext::Store)?;
            require_load_context(value)?;
        }
        ExpressionKind::Subscript {
            value, slice, ctx, ..
        } => {
            require_context(*ctx, ExprContext::Store)?;
            require_load_context(value)?;
            require_load_context(slice)?;
        }
        ExpressionKind::Starred { value, ctx } => {
            require_context(*ctx, ExprContext::Store)?;
            require_store_context(value)?;
        }
        ExpressionKind::List { elts, ctx } | ExpressionKind::Tuple { elts, ctx } => {
            require_context(*ctx, ExprContext::Store)?;
            for elt in elts {
                require_store_context(elt)?;
            }
        }
        _ => {
            return Err(value_error(
                "expression which can't be assigned to in Store context",
            ));
        }
    }
    Ok(())
}

pub(super) fn require_del_context(expr: &AstExpression) -> Result<(), String> {
    match &expr.node {
        ExpressionKind::Name { ctx, .. } => require_context(*ctx, ExprContext::Del)?,
        ExpressionKind::Attribute { value, ctx, .. } => {
            require_context(*ctx, ExprContext::Del)?;
            require_load_context(value)?;
        }
        ExpressionKind::Subscript {
            value, slice, ctx, ..
        } => {
            require_context(*ctx, ExprContext::Del)?;
            require_load_context(value)?;
            require_load_context(slice)?;
        }
        ExpressionKind::List { elts, ctx } | ExpressionKind::Tuple { elts, ctx } => {
            require_context(*ctx, ExprContext::Del)?;
            for elt in elts {
                require_del_context(elt)?;
            }
        }
        _ => return Err(value_error("expression must have Del context")),
    }
    Ok(())
}

fn require_context(actual: ExprContext, expected: ExprContext) -> Result<(), String> {
    if actual == expected {
        return Ok(());
    }
    let expected = match expected {
        ExprContext::Load => "Load",
        ExprContext::Store => "Store",
        ExprContext::Del => "Del",
    };
    Err(value_error(&format!(
        "expression must have {} context",
        expected
    )))
}

pub(super) fn validate_arguments(args: &AstArguments) -> Result<(), String> {
    if args.defaults.len() > args.posonlyargs.len() + args.args.len() {
        return Err(value_error("more positional defaults than args"));
    }
    if args.kwonlyargs.len() != args.kw_defaults.len() {
        return Err(value_error(
            "length of kwonlyargs is not the same as kw_defaults",
        ));
    }

    for arg in args.posonlyargs.iter().chain(args.args.iter()) {
        if let Some(annotation) = &arg.annotation {
            require_load_context(annotation)?;
        }
    }
    if let Some(vararg) = &args.vararg {
        if let Some(annotation) = &vararg.annotation {
            require_load_context(annotation)?;
        }
    }
    for arg in &args.kwonlyargs {
        if let Some(annotation) = &arg.annotation {
            require_load_context(annotation)?;
        }
    }
    if let Some(kwarg) = &args.kwarg {
        if let Some(annotation) = &kwarg.annotation {
            require_load_context(annotation)?;
        }
    }
    for default in &args.defaults {
        require_load_context(default)?;
    }
    for default in args.kw_defaults.iter().flatten() {
        require_load_context(default)?;
    }
    Ok(())
}

pub(super) fn validate_comprehensions(generators: &[AstComprehension]) -> Result<(), String> {
    ensure_non_empty(generators, "comprehension with no generators")?;
    for generator in generators {
        require_store_context(&generator.target)?;
        require_load_context(&generator.iter)?;
        for if_expr in &generator.ifs {
            require_load_context(if_expr)?;
        }
    }
    Ok(())
}

pub(super) fn validate_sequence_context(
    elts: &[AstExpression],
    ctx: ExprContext,
) -> Result<(), String> {
    match ctx {
        ExprContext::Load => {
            for elt in elts {
                require_load_context(elt)?;
            }
        }
        ExprContext::Store => {
            for elt in elts {
                require_store_context(elt)?;
            }
        }
        ExprContext::Del => {
            for elt in elts {
                require_del_context(elt)?;
            }
        }
    }
    Ok(())
}
