use super::*;

// ── PyObject AST → Rust AST converter ──────────────────────────────────
// Converts Python AST objects (from `ast.parse()` or programmatic construction)
// directly into ferrython_ast types, bypassing source code roundtrip.
// This is necessary because werkzeug and other libs use invalid-identifier
// names (e.g. `<builder:...>`, `.self`) that cannot survive unparse→reparse.

use ferrython_ast::{
    Alias as AstAlias, Arg as AstArg, Arguments as AstArguments, BigInt as AstBigInt, BoolOperator,
    CompareOperator, Comprehension as AstComprehension, Constant as AstConstant,
    ExceptHandler as AstExceptHandler, ExprContext, Expression as AstExpression, ExpressionKind,
    Keyword as AstKeyword, Module as AstModule, Operator, SourceLocation, Statement, StatementKind,
    UnaryOperator, WithItem as AstWithItem,
};

/// Convert a PyObject AST Module into a ferrython_ast Module for compilation.
pub fn pyobj_ast_to_module(node: &PyObjectRef) -> Result<AstModule, String> {
    let type_name = node.type_name().to_string();
    match type_name.as_str() {
        "Module" => {
            let body = convert_stmt_list(node, "body")?;
            Ok(AstModule::Module {
                body,
                type_ignores: Vec::new(),
            })
        }
        "Expression" => {
            let body_expr = node
                .get_attr("body")
                .ok_or_else(|| "Expression node missing 'body'".to_string())?;
            let expr = convert_expr(&body_expr)?;
            require_load_context(&expr)?;
            Ok(AstModule::Expression {
                body: Box::new(expr),
            })
        }
        "Interactive" => {
            let body = convert_stmt_list(node, "body")?;
            Ok(AstModule::Interactive { body })
        }
        _ => Err(format!(
            "Expected Module/Expression/Interactive, got {}",
            type_name
        )),
    }
}

fn loc_from_node(node: &PyObjectRef) -> SourceLocation {
    let line = node.get_attr("lineno").and_then(|v| match &v.payload {
        PyObjectPayload::None => Some(0),
        _ => v.to_int().map(|i| i as u32).ok(),
    });
    let col = node.get_attr("col_offset").and_then(|v| match &v.payload {
        PyObjectPayload::None => Some(0),
        _ => v.to_int().map(|i| i as u32).ok(),
    });
    let end_line = node
        .get_attr("end_lineno")
        .and_then(|v| v.to_int().map(|i| i as u32).ok());
    let end_col = node
        .get_attr("end_col_offset")
        .and_then(|v| v.to_int().map(|i| i as u32).ok());
    let mut loc = SourceLocation::new(line.unwrap_or(1), col.unwrap_or(0));
    if let (Some(el), Some(ec)) = (end_line, end_col) {
        loc = loc.with_end(el, ec);
    }
    loc
}

fn get_list_attr(node: &PyObjectRef, attr: &str) -> Vec<PyObjectRef> {
    node.get_attr(attr)
        .map(|v| {
            if let PyObjectPayload::List(items) = &v.payload {
                items.read().clone()
            } else if matches!(&v.payload, PyObjectPayload::None) {
                Vec::new()
            } else {
                vec![v]
            }
        })
        .unwrap_or_default()
}

fn value_error(message: &str) -> String {
    format!("ValueError: {}", message)
}

fn type_error(message: &str) -> String {
    format!("TypeError: {}", message)
}

fn get_checked_list_attr(node: &PyObjectRef, attr: &str) -> Result<Vec<PyObjectRef>, String> {
    let items = get_list_attr(node, attr);
    if items
        .iter()
        .any(|item| matches!(&item.payload, PyObjectPayload::None))
    {
        return Err(value_error("None disallowed"));
    }
    Ok(items)
}

fn ensure_non_empty<T>(items: &[T], message: &str) -> Result<(), String> {
    if items.is_empty() {
        Err(value_error(message))
    } else {
        Ok(())
    }
}

fn get_str_attr(node: &PyObjectRef, attr: &str) -> CompactString {
    node.get_attr(attr)
        .map(|v| CompactString::from(v.py_to_string()))
        .unwrap_or_default()
}

fn get_identifier_attr(node: &PyObjectRef, attr: &str) -> Result<CompactString, String> {
    match node.get_attr(attr) {
        Some(v) => match &v.payload {
            PyObjectPayload::Str(s) => Ok(CompactString::from(s.as_str())),
            _ => Err(type_error("identifier must be of type str")),
        },
        None => Ok(CompactString::from("")),
    }
}

fn get_optional_str(node: &PyObjectRef, attr: &str) -> Option<CompactString> {
    node.get_attr(attr).and_then(|v| {
        if matches!(&v.payload, PyObjectPayload::None) {
            None
        } else {
            Some(CompactString::from(v.py_to_string()))
        }
    })
}

fn convert_stmt_list(parent: &PyObjectRef, attr: &str) -> Result<Vec<Statement>, String> {
    let items = get_checked_list_attr(parent, attr)?;
    items.iter().map(convert_stmt).collect()
}

fn convert_expr_list(parent: &PyObjectRef, attr: &str) -> Result<Vec<AstExpression>, String> {
    let items = get_checked_list_attr(parent, attr)?;
    items.iter().map(convert_expr).collect()
}

fn require_load_context(expr: &AstExpression) -> Result<(), String> {
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

fn require_store_context(expr: &AstExpression) -> Result<(), String> {
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

fn require_del_context(expr: &AstExpression) -> Result<(), String> {
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

fn validate_arguments(args: &AstArguments) -> Result<(), String> {
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

fn validate_comprehensions(generators: &[AstComprehension]) -> Result<(), String> {
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

fn validate_sequence_context(elts: &[AstExpression], ctx: ExprContext) -> Result<(), String> {
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

fn convert_optional_expr(
    parent: &PyObjectRef,
    attr: &str,
) -> Result<Option<Box<AstExpression>>, String> {
    match parent.get_attr(attr) {
        Some(v) if !matches!(&v.payload, PyObjectPayload::None) => {
            Ok(Some(Box::new(convert_expr(&v)?)))
        }
        _ => Ok(None),
    }
}

fn convert_stmt(node: &PyObjectRef) -> Result<Statement, String> {
    let type_name = node.type_name().to_string();
    let location = loc_from_node(node);
    let kind = match type_name.as_str() {
        "FunctionDef" | "AsyncFunctionDef" => {
            let name = get_str_attr(node, "name");
            let args = node
                .get_attr("args")
                .map(|a| convert_arguments(&a))
                .unwrap_or_else(|| Ok(AstArguments::empty()))?;
            validate_arguments(&args)?;
            let body = convert_stmt_list(node, "body")?;
            ensure_non_empty(&body, &format!("empty body on {}", type_name))?;
            let decorator_list = convert_expr_list(node, "decorator_list")?;
            for decorator in &decorator_list {
                require_load_context(decorator)?;
            }
            let returns = convert_optional_expr(node, "returns")?;
            if let Some(returns) = &returns {
                require_load_context(returns)?;
            }
            StatementKind::FunctionDef {
                name,
                args: Box::new(args),
                body,
                decorator_list,
                returns,
                type_comment: None,
                is_async: type_name == "AsyncFunctionDef",
            }
        }
        "ClassDef" => {
            let name = get_str_attr(node, "name");
            let bases = convert_expr_list(node, "bases")?;
            for base in &bases {
                require_load_context(base)?;
            }
            let keywords = convert_keyword_list(node, "keywords")?;
            let body = convert_stmt_list(node, "body")?;
            ensure_non_empty(&body, "empty body on ClassDef")?;
            let decorator_list = convert_expr_list(node, "decorator_list")?;
            for decorator in &decorator_list {
                require_load_context(decorator)?;
            }
            StatementKind::ClassDef {
                name,
                bases,
                keywords,
                body,
                decorator_list,
            }
        }
        "Return" => {
            let value = convert_optional_expr(node, "value")?;
            if let Some(value) = &value {
                require_load_context(value)?;
            }
            StatementKind::Return { value }
        }
        "Delete" => {
            let targets = convert_expr_list(node, "targets")?;
            ensure_non_empty(&targets, "empty targets on Delete")?;
            for target in &targets {
                require_del_context(target)?;
            }
            StatementKind::Delete { targets }
        }
        "Assign" => {
            let targets = convert_expr_list(node, "targets")?;
            ensure_non_empty(&targets, "empty targets on Assign")?;
            let value_node = node
                .get_attr("value")
                .ok_or_else(|| "Assign missing 'value'".to_string())?;
            let value = Box::new(convert_expr(&value_node)?);
            for target in &targets {
                require_store_context(target)?;
            }
            require_load_context(&value)?;
            StatementKind::Assign {
                targets,
                value,
                type_comment: None,
            }
        }
        "AugAssign" => {
            let target_node = node
                .get_attr("target")
                .ok_or_else(|| "AugAssign missing 'target'".to_string())?;
            let target = Box::new(convert_expr(&target_node)?);
            let op = node
                .get_attr("op")
                .map(|o| convert_operator(&o))
                .unwrap_or(Operator::Add);
            let value_node = node
                .get_attr("value")
                .ok_or_else(|| "AugAssign missing 'value'".to_string())?;
            let value = Box::new(convert_expr(&value_node)?);
            require_store_context(&target)?;
            require_load_context(&value)?;
            StatementKind::AugAssign { target, op, value }
        }
        "AnnAssign" => {
            let target_node = node
                .get_attr("target")
                .ok_or_else(|| "AnnAssign missing 'target'".to_string())?;
            let target = Box::new(convert_expr(&target_node)?);
            let ann_node = node
                .get_attr("annotation")
                .ok_or_else(|| "AnnAssign missing 'annotation'".to_string())?;
            let annotation = Box::new(convert_expr(&ann_node)?);
            let value = convert_optional_expr(node, "value")?;
            require_store_context(&target)?;
            require_load_context(&annotation)?;
            if let Some(value) = &value {
                require_load_context(value)?;
            }
            let simple = node
                .get_attr("simple")
                .and_then(|v| v.to_int().ok())
                .map(|i| i != 0)
                .unwrap_or(true);
            StatementKind::AnnAssign {
                target,
                annotation,
                value,
                simple,
            }
        }
        "For" | "AsyncFor" => {
            let target_node = node
                .get_attr("target")
                .ok_or_else(|| "For missing 'target'".to_string())?;
            let target = Box::new(convert_expr(&target_node)?);
            let iter_node = node
                .get_attr("iter")
                .ok_or_else(|| "For missing 'iter'".to_string())?;
            let iter_expr = Box::new(convert_expr(&iter_node)?);
            let body = convert_stmt_list(node, "body")?;
            ensure_non_empty(&body, &format!("empty body on {}", type_name))?;
            let orelse = convert_stmt_list(node, "orelse")?;
            require_store_context(&target)?;
            require_load_context(&iter_expr)?;
            StatementKind::For {
                target,
                iter: iter_expr,
                body,
                orelse,
                type_comment: None,
                is_async: type_name == "AsyncFor",
            }
        }
        "While" => {
            let test_node = node
                .get_attr("test")
                .ok_or_else(|| "While missing 'test'".to_string())?;
            let test = Box::new(convert_expr(&test_node)?);
            let body = convert_stmt_list(node, "body")?;
            ensure_non_empty(&body, "empty body on While")?;
            let orelse = convert_stmt_list(node, "orelse")?;
            require_load_context(&test)?;
            StatementKind::While { test, body, orelse }
        }
        "If" => {
            let test_node = node
                .get_attr("test")
                .ok_or_else(|| "If missing 'test'".to_string())?;
            let test = Box::new(convert_expr(&test_node)?);
            let body = convert_stmt_list(node, "body")?;
            ensure_non_empty(&body, "empty body on If")?;
            let orelse = convert_stmt_list(node, "orelse")?;
            require_load_context(&test)?;
            StatementKind::If { test, body, orelse }
        }
        "With" | "AsyncWith" => {
            let items = get_checked_list_attr(node, "items")?;
            ensure_non_empty(&items, &format!("empty items on {}", type_name))?;
            let with_items: Vec<AstWithItem> = items
                .iter()
                .map(|item| {
                    let ctx_node = item
                        .get_attr("context_expr")
                        .ok_or_else(|| "WithItem missing 'context_expr'".to_string())?;
                    let context_expr = convert_expr(&ctx_node)?;
                    let optional_vars = convert_optional_expr(item, "optional_vars")?;
                    require_load_context(&context_expr)?;
                    if let Some(optional_vars) = &optional_vars {
                        require_store_context(optional_vars)?;
                    }
                    Ok(AstWithItem {
                        context_expr,
                        optional_vars,
                    })
                })
                .collect::<Result<_, String>>()?;
            let body = convert_stmt_list(node, "body")?;
            ensure_non_empty(&body, &format!("empty body on {}", type_name))?;
            StatementKind::With {
                items: with_items,
                body,
                type_comment: None,
                is_async: type_name == "AsyncWith",
            }
        }
        "Raise" => {
            let exc = convert_optional_expr(node, "exc")?;
            let cause = convert_optional_expr(node, "cause")?;
            if exc.is_none() && cause.is_some() {
                return Err(value_error("Raise with cause but no exception"));
            }
            if let Some(exc) = &exc {
                require_load_context(exc)?;
            }
            if let Some(cause) = &cause {
                require_load_context(cause)?;
            }
            StatementKind::Raise { exc, cause }
        }
        "Try" | "TryStar" => {
            let body = convert_stmt_list(node, "body")?;
            ensure_non_empty(&body, &format!("empty body on {}", type_name))?;
            let handlers = get_checked_list_attr(node, "handlers")?;
            let except_handlers: Vec<AstExceptHandler> = handlers
                .iter()
                .map(|h| {
                    let typ = convert_optional_expr(h, "type")?;
                    let name = get_optional_str(h, "name");
                    let handler_body = convert_stmt_list(h, "body")?;
                    ensure_non_empty(&handler_body, "empty body on ExceptHandler")?;
                    if let Some(typ) = &typ {
                        require_load_context(typ)?;
                    }
                    Ok(AstExceptHandler {
                        typ,
                        name,
                        body: handler_body,
                        location: loc_from_node(h),
                        is_star: type_name == "TryStar",
                    })
                })
                .collect::<Result<_, String>>()?;
            let orelse = convert_stmt_list(node, "orelse")?;
            let finalbody = convert_stmt_list(node, "finalbody")?;
            if except_handlers.is_empty() && finalbody.is_empty() {
                return Err(value_error("Try has neither except handlers nor finalbody"));
            }
            if except_handlers.is_empty() && !orelse.is_empty() {
                return Err(value_error("Try has orelse but no except handlers"));
            }
            StatementKind::Try {
                body,
                handlers: except_handlers,
                orelse,
                finalbody,
            }
        }
        "Assert" => {
            let test_node = node
                .get_attr("test")
                .ok_or_else(|| "Assert missing 'test'".to_string())?;
            let test = Box::new(convert_expr(&test_node)?);
            let msg = convert_optional_expr(node, "msg")?;
            require_load_context(&test)?;
            if let Some(msg) = &msg {
                require_load_context(msg)?;
            }
            StatementKind::Assert { test, msg }
        }
        "Import" => {
            let names = convert_alias_list(node, "names")?;
            ensure_non_empty(&names, "empty names on Import")?;
            StatementKind::Import { names }
        }
        "ImportFrom" => {
            let module = get_optional_str(node, "module");
            let names = convert_alias_list(node, "names")?;
            ensure_non_empty(&names, "empty names on ImportFrom")?;
            if node
                .get_attr("lineno")
                .map(|v| matches!(&v.payload, PyObjectPayload::None))
                .unwrap_or(false)
            {
                return Err("ValueError: invalid integer value: None".to_string());
            }
            let level = match node.get_attr("level") {
                Some(v) if matches!(&v.payload, PyObjectPayload::None) => 0,
                Some(v) => {
                    let level = v
                        .to_int()
                        .map_err(|_| format!("invalid integer value: {}", v.py_to_string()))?;
                    if level < 0 {
                        return Err(value_error("Negative ImportFrom level"));
                    }
                    level as u32
                }
                None => 0,
            };
            StatementKind::ImportFrom {
                module,
                names,
                level,
            }
        }
        "Global" => {
            let names = get_list_attr(node, "names")
                .iter()
                .map(|n| CompactString::from(n.py_to_string()))
                .collect::<Vec<_>>();
            ensure_non_empty(&names, "empty names on Global")?;
            StatementKind::Global { names }
        }
        "Nonlocal" => {
            let names = get_list_attr(node, "names")
                .iter()
                .map(|n| CompactString::from(n.py_to_string()))
                .collect::<Vec<_>>();
            ensure_non_empty(&names, "empty names on Nonlocal")?;
            StatementKind::Nonlocal { names }
        }
        "Expr" => {
            let value_node = node
                .get_attr("value")
                .ok_or_else(|| "Expr missing 'value'".to_string())?;
            let value = Box::new(convert_expr(&value_node)?);
            require_load_context(&value)?;
            StatementKind::Expr { value }
        }
        "Pass" => StatementKind::Pass,
        "Break" => StatementKind::Break,
        "Continue" => StatementKind::Continue,
        _ => return Err(format!("Unknown statement type: {}", type_name)),
    };
    Ok(Statement {
        node: kind,
        location,
    })
}

pub(super) fn convert_expr(node: &PyObjectRef) -> Result<AstExpression, String> {
    let type_name = node.type_name().to_string();
    let location = loc_from_node(node);
    let kind = match type_name.as_str() {
        "Num" | "Str" | "Bytes" | "NameConstant" | "Ellipsis" => {
            let value = if type_name == "Num" {
                node.get_attr("n")
            } else if type_name == "Str" || type_name == "Bytes" {
                node.get_attr("s")
            } else if type_name == "Ellipsis" {
                Some(PyObject::ellipsis())
            } else {
                node.get_attr("value")
            }
            .map(|v| {
                if type_name == "Num" && is_ast_constant_builtin_subclass(&v) {
                    return Err("TypeError: invalid type in Num".to_string());
                }
                Ok(convert_constant(&v))
            })
            .transpose()?
            .unwrap_or(AstConstant::None);
            ExpressionKind::Constant { value }
        }
        "BoolOp" => {
            let op = node
                .get_attr("op")
                .map(|o| convert_bool_op(&o))
                .unwrap_or(BoolOperator::And);
            let values = convert_expr_list(node, "values")?;
            if values.len() < 2 {
                return Err(value_error("BoolOp with less than 2 values"));
            }
            for value in &values {
                require_load_context(value)?;
            }
            ExpressionKind::BoolOp { op, values }
        }
        "NamedExpr" => {
            let target_node = node
                .get_attr("target")
                .ok_or_else(|| "NamedExpr missing 'target'".to_string())?;
            let value_node = node
                .get_attr("value")
                .ok_or_else(|| "NamedExpr missing 'value'".to_string())?;
            let target = Box::new(convert_expr(&target_node)?);
            let value = Box::new(convert_expr(&value_node)?);
            require_store_context(&target)?;
            require_load_context(&value)?;
            ExpressionKind::NamedExpr { target, value }
        }
        "BinOp" => {
            let left_node = node
                .get_attr("left")
                .ok_or_else(|| "BinOp missing 'left'".to_string())?;
            let right_node = node
                .get_attr("right")
                .ok_or_else(|| "BinOp missing 'right'".to_string())?;
            let op = node
                .get_attr("op")
                .map(|o| convert_operator(&o))
                .unwrap_or(Operator::Add);
            let left = Box::new(convert_expr(&left_node)?);
            let right = Box::new(convert_expr(&right_node)?);
            require_load_context(&left)?;
            require_load_context(&right)?;
            ExpressionKind::BinOp { left, op, right }
        }
        "UnaryOp" => {
            let op = node
                .get_attr("op")
                .map(|o| convert_unary_op(&o))
                .unwrap_or(UnaryOperator::UAdd);
            let operand_node = node
                .get_attr("operand")
                .ok_or_else(|| "UnaryOp missing 'operand'".to_string())?;
            let operand = Box::new(convert_expr(&operand_node)?);
            require_load_context(&operand)?;
            ExpressionKind::UnaryOp { op, operand }
        }
        "Lambda" => {
            let args = node
                .get_attr("args")
                .map(|a| convert_arguments(&a))
                .unwrap_or_else(|| Ok(AstArguments::empty()))?;
            validate_arguments(&args)?;
            let body_node = node
                .get_attr("body")
                .ok_or_else(|| "Lambda missing 'body'".to_string())?;
            let body = Box::new(convert_expr(&body_node)?);
            require_load_context(&body)?;
            ExpressionKind::Lambda {
                args: Box::new(args),
                body,
            }
        }
        "IfExp" => {
            let test = node
                .get_attr("test")
                .ok_or_else(|| "IfExp missing 'test'".to_string())?;
            let body = node
                .get_attr("body")
                .ok_or_else(|| "IfExp missing 'body'".to_string())?;
            let orelse = node
                .get_attr("orelse")
                .ok_or_else(|| "IfExp missing 'orelse'".to_string())?;
            let test = Box::new(convert_expr(&test)?);
            let body = Box::new(convert_expr(&body)?);
            let orelse = Box::new(convert_expr(&orelse)?);
            require_load_context(&test)?;
            require_load_context(&body)?;
            require_load_context(&orelse)?;
            ExpressionKind::IfExp { test, body, orelse }
        }
        "Dict" => {
            let keys_raw = get_list_attr(node, "keys");
            let values_raw = get_checked_list_attr(node, "values")?;
            if keys_raw.len() != values_raw.len() {
                return Err(value_error(
                    "Dict doesn't have same number of keys as values",
                ));
            }
            let keys: Vec<Option<AstExpression>> = keys_raw
                .iter()
                .map(|k| {
                    if matches!(&k.payload, PyObjectPayload::None) {
                        Ok(None)
                    } else {
                        convert_expr(k).map(Some)
                    }
                })
                .collect::<Result<_, String>>()?;
            let values: Vec<AstExpression> = values_raw
                .iter()
                .map(|v| convert_expr(v))
                .collect::<Result<_, String>>()?;
            for key in keys.iter().flatten() {
                require_load_context(key)?;
            }
            for value in &values {
                require_load_context(value)?;
            }
            ExpressionKind::Dict { keys, values }
        }
        "Set" => {
            let elts = convert_expr_list(node, "elts")?;
            for elt in &elts {
                require_load_context(elt)?;
            }
            ExpressionKind::Set { elts }
        }
        "ListComp" => {
            let elt_node = node
                .get_attr("elt")
                .ok_or_else(|| "ListComp missing 'elt'".to_string())?;
            let generators = convert_comprehension_list(node)?;
            ensure_non_empty(&generators, "comprehension with no generators")?;
            let elt = Box::new(convert_expr(&elt_node)?);
            require_load_context(&elt)?;
            ExpressionKind::ListComp { elt, generators }
        }
        "SetComp" => {
            let elt_node = node
                .get_attr("elt")
                .ok_or_else(|| "SetComp missing 'elt'".to_string())?;
            let generators = convert_comprehension_list(node)?;
            ensure_non_empty(&generators, "comprehension with no generators")?;
            let elt = Box::new(convert_expr(&elt_node)?);
            require_load_context(&elt)?;
            ExpressionKind::SetComp { elt, generators }
        }
        "DictComp" => {
            let key_node = node
                .get_attr("key")
                .ok_or_else(|| "DictComp missing 'key'".to_string())?;
            let value_node = node
                .get_attr("value")
                .ok_or_else(|| "DictComp missing 'value'".to_string())?;
            let generators = convert_comprehension_list(node)?;
            ensure_non_empty(&generators, "comprehension with no generators")?;
            let key = Box::new(convert_expr(&key_node)?);
            let value = Box::new(convert_expr(&value_node)?);
            require_load_context(&key)?;
            require_load_context(&value)?;
            ExpressionKind::DictComp {
                key,
                value,
                generators,
            }
        }
        "GeneratorExp" => {
            let elt_node = node
                .get_attr("elt")
                .ok_or_else(|| "GeneratorExp missing 'elt'".to_string())?;
            let generators = convert_comprehension_list(node)?;
            ensure_non_empty(&generators, "comprehension with no generators")?;
            let elt = Box::new(convert_expr(&elt_node)?);
            require_load_context(&elt)?;
            ExpressionKind::GeneratorExp { elt, generators }
        }
        "Await" => {
            let value_node = node
                .get_attr("value")
                .ok_or_else(|| "Await missing 'value'".to_string())?;
            let value = Box::new(convert_expr(&value_node)?);
            require_load_context(&value)?;
            ExpressionKind::Await { value }
        }
        "Yield" => {
            let value = convert_optional_expr(node, "value")?;
            if let Some(value) = &value {
                require_load_context(value)?;
            }
            ExpressionKind::Yield {
                value: value.map(|b| *b).map(Box::new),
            }
        }
        "YieldFrom" => {
            let value_node = node
                .get_attr("value")
                .ok_or_else(|| "YieldFrom missing 'value'".to_string())?;
            if matches!(&value_node.payload, PyObjectPayload::None) {
                return Err(value_error("field value is required for YieldFrom"));
            }
            let value = Box::new(convert_expr(&value_node)?);
            require_load_context(&value)?;
            ExpressionKind::YieldFrom { value }
        }
        "Compare" => {
            let left_node = node
                .get_attr("left")
                .ok_or_else(|| "Compare missing 'left'".to_string())?;
            let ops_raw = get_list_attr(node, "ops");
            let ops: Vec<CompareOperator> = ops_raw.iter().map(|o| convert_compare_op(o)).collect();
            let comparators = convert_expr_list(node, "comparators")?;
            if comparators.is_empty() {
                return Err(value_error("no comparators"));
            }
            if ops.len() != comparators.len() {
                return Err(value_error("different number of comparators and operands"));
            }
            let left = Box::new(convert_expr(&left_node)?);
            require_load_context(&left)?;
            for comparator in &comparators {
                require_load_context(comparator)?;
            }
            ExpressionKind::Compare {
                left,
                ops,
                comparators,
            }
        }
        "Call" => {
            let func_node = node
                .get_attr("func")
                .ok_or_else(|| "Call missing 'func'".to_string())?;
            let args = convert_expr_list(node, "args")?;
            let keywords = convert_keyword_list(node, "keywords")?;
            let func = Box::new(convert_expr(&func_node)?);
            require_load_context(&func)?;
            for arg in &args {
                require_load_context(arg)?;
            }
            ExpressionKind::Call {
                func,
                args,
                keywords,
            }
        }
        "FormattedValue" => {
            let value_node = node
                .get_attr("value")
                .ok_or_else(|| "FormattedValue missing 'value'".to_string())?;
            let value = Box::new(convert_expr(&value_node)?);
            require_load_context(&value)?;
            let conversion = node
                .get_attr("conversion")
                .and_then(|v| v.to_int().ok())
                .and_then(|c| {
                    if c < 0 {
                        None
                    } else {
                        char::from_u32(c as u32)
                    }
                });
            let format_spec = convert_optional_expr(node, "format_spec")?;
            if let Some(format_spec) = &format_spec {
                require_load_context(format_spec)?;
            }
            ExpressionKind::FormattedValue {
                value,
                conversion,
                format_spec,
            }
        }
        "JoinedStr" => {
            let values = convert_expr_list(node, "values")?;
            for value in &values {
                require_load_context(value)?;
            }
            ExpressionKind::JoinedStr { values }
        }
        "Constant" => {
            let value = if let Some(value) = node.get_attr("value") {
                validate_ast_constant_value(&value).map_err(|msg| format!("TypeError: {}", msg))?;
                convert_constant(&value)
            } else {
                AstConstant::None
            };
            ExpressionKind::Constant { value }
        }
        "Attribute" => {
            let value_node = node
                .get_attr("value")
                .ok_or_else(|| "Attribute missing 'value'".to_string())?;
            let attr = get_str_attr(node, "attr");
            let ctx = node
                .get_attr("ctx")
                .map(|c| convert_expr_context(&c))
                .unwrap_or(ExprContext::Load);
            let value = Box::new(convert_expr(&value_node)?);
            require_load_context(&value)?;
            ExpressionKind::Attribute { value, attr, ctx }
        }
        "Subscript" => {
            let value_node = node
                .get_attr("value")
                .ok_or_else(|| "Subscript missing 'value'".to_string())?;
            let slice_node = node
                .get_attr("slice")
                .ok_or_else(|| "Subscript missing 'slice'".to_string())?;
            let slice_node = if slice_node.type_name() == "Index" {
                slice_node
                    .get_attr("value")
                    .ok_or_else(|| "Index missing 'value'".to_string())?
            } else {
                slice_node
            };
            let ctx = node
                .get_attr("ctx")
                .map(|c| convert_expr_context(&c))
                .unwrap_or(ExprContext::Load);
            let value = Box::new(convert_expr(&value_node)?);
            let slice = Box::new(convert_expr(&slice_node)?);
            require_load_context(&value)?;
            require_load_context(&slice)?;
            ExpressionKind::Subscript { value, slice, ctx }
        }
        "Starred" => {
            let value_node = node
                .get_attr("value")
                .ok_or_else(|| "Starred missing 'value'".to_string())?;
            let ctx = node
                .get_attr("ctx")
                .map(|c| convert_expr_context(&c))
                .unwrap_or(ExprContext::Load);
            ExpressionKind::Starred {
                value: Box::new(convert_expr(&value_node)?),
                ctx,
            }
        }
        "Name" => {
            let id = get_identifier_attr(node, "id")?;
            if matches!(id.as_str(), "True" | "False" | "None") {
                return Err(value_error(&format!(
                    "Name node can't be used with '{}' constant",
                    id
                )));
            }
            let ctx = node
                .get_attr("ctx")
                .map(|c| convert_expr_context(&c))
                .unwrap_or(ExprContext::Load);
            ExpressionKind::Name { id, ctx }
        }
        "List" => {
            let elts = convert_expr_list(node, "elts")?;
            let ctx = node
                .get_attr("ctx")
                .map(|c| convert_expr_context(&c))
                .unwrap_or(ExprContext::Load);
            validate_sequence_context(&elts, ctx)?;
            ExpressionKind::List { elts, ctx }
        }
        "Tuple" => {
            let elts = convert_expr_list(node, "elts")?;
            let ctx = node
                .get_attr("ctx")
                .map(|c| convert_expr_context(&c))
                .unwrap_or(ExprContext::Load);
            validate_sequence_context(&elts, ctx)?;
            ExpressionKind::Tuple { elts, ctx }
        }
        "Slice" => {
            let lower = convert_optional_expr(node, "lower")?;
            let upper = convert_optional_expr(node, "upper")?;
            let step = convert_optional_expr(node, "step")?;
            if let Some(lower) = &lower {
                require_load_context(lower)?;
            }
            if let Some(upper) = &upper {
                require_load_context(upper)?;
            }
            if let Some(step) = &step {
                require_load_context(step)?;
            }
            ExpressionKind::Slice { lower, upper, step }
        }
        "Index" => {
            let value = node
                .get_attr("value")
                .ok_or_else(|| "Index missing 'value'".to_string())?;
            return convert_expr(&value);
        }
        "ExtSlice" => {
            let dims = convert_expr_list(node, "dims")?;
            ensure_non_empty(&dims, "empty dims on ExtSlice")?;
            for dim in &dims {
                require_load_context(dim)?;
            }
            ExpressionKind::Tuple {
                elts: dims,
                ctx: ExprContext::Load,
            }
        }
        "expr" => {
            return Err(type_error(
                "expected some sort of expr, but got <_ast.expr object>",
            ));
        }
        _ => {
            return Err(format!("Unknown expression type: {}", type_name));
        }
    };
    Ok(AstExpression {
        node: kind,
        location,
        outer_location: location,
    })
}

fn is_ast_constant_builtin_subclass(val: &PyObjectRef) -> bool {
    if let PyObjectPayload::Instance(inst) = &val.payload {
        return inst.attrs.read().contains_key("__builtin_value__");
    }
    false
}

fn validate_ast_constant_value(val: &PyObjectRef) -> Result<(), String> {
    match &val.payload {
        PyObjectPayload::None
        | PyObjectPayload::Bool(_)
        | PyObjectPayload::Int(_)
        | PyObjectPayload::Float(_)
        | PyObjectPayload::Complex { .. }
        | PyObjectPayload::Str(_)
        | PyObjectPayload::Bytes(_)
        | PyObjectPayload::Ellipsis => Ok(()),
        PyObjectPayload::Tuple(items) => {
            for item in items.iter() {
                validate_ast_constant_value(item)?;
            }
            Ok(())
        }
        PyObjectPayload::FrozenSet(items) => {
            for item in items.values() {
                validate_ast_constant_value(item)?;
            }
            Ok(())
        }
        _ => Err(format!(
            "got an invalid type in Constant: {}",
            val.type_name()
        )),
    }
}

fn convert_constant(val: &PyObjectRef) -> AstConstant {
    use ferrython_core::types::PyInt;
    match &val.payload {
        PyObjectPayload::None => AstConstant::None,
        PyObjectPayload::Bool(b) => AstConstant::Bool(*b),
        PyObjectPayload::Int(pi) => match pi {
            PyInt::Small(i) => AstConstant::Int(AstBigInt::Small(*i)),
            PyInt::Big(bi) => AstConstant::Int(AstBigInt::Big(bi.clone())),
        },
        PyObjectPayload::Float(f) => AstConstant::Float(*f),
        PyObjectPayload::Complex { real, imag } => AstConstant::Complex {
            real: *real,
            imag: *imag,
        },
        PyObjectPayload::Str(s) => AstConstant::Str(CompactString::from(s.as_str())),
        PyObjectPayload::Bytes(b) => AstConstant::Bytes((**b).clone()),
        PyObjectPayload::Ellipsis => AstConstant::Ellipsis,
        PyObjectPayload::Tuple(items) => {
            AstConstant::Tuple(items.iter().map(|item| convert_constant(item)).collect())
        }
        PyObjectPayload::FrozenSet(items) => {
            AstConstant::FrozenSet(items.values().map(|item| convert_constant(item)).collect())
        }
        PyObjectPayload::Instance(inst) => {
            if let Some(value) = inst.attrs.read().get("__builtin_value__").cloned() {
                return convert_constant(&value);
            }
            AstConstant::Str(CompactString::from(val.py_to_string()))
        }
        _ => {
            let s = val.py_to_string();
            if let Ok(i) = s.parse::<i64>() {
                AstConstant::Int(AstBigInt::Small(i))
            } else if let Ok(f) = s.parse::<f64>() {
                AstConstant::Float(f)
            } else {
                AstConstant::Str(CompactString::from(s))
            }
        }
    }
}

fn convert_operator(node: &PyObjectRef) -> Operator {
    match node.type_name().to_string().as_str() {
        "Add" => Operator::Add,
        "Sub" => Operator::Sub,
        "Mult" => Operator::Mult,
        "Div" => Operator::Div,
        "Mod" => Operator::Mod,
        "Pow" => Operator::Pow,
        "LShift" => Operator::LShift,
        "RShift" => Operator::RShift,
        "BitOr" => Operator::BitOr,
        "BitXor" => Operator::BitXor,
        "BitAnd" => Operator::BitAnd,
        "FloorDiv" => Operator::FloorDiv,
        "MatMult" => Operator::MatMult,
        _ => Operator::Add,
    }
}

fn convert_bool_op(node: &PyObjectRef) -> BoolOperator {
    match node.type_name().to_string().as_str() {
        "And" => BoolOperator::And,
        "Or" => BoolOperator::Or,
        _ => BoolOperator::And,
    }
}

fn convert_unary_op(node: &PyObjectRef) -> UnaryOperator {
    match node.type_name().to_string().as_str() {
        "Invert" => UnaryOperator::Invert,
        "Not" => UnaryOperator::Not,
        "UAdd" => UnaryOperator::UAdd,
        "USub" => UnaryOperator::USub,
        _ => UnaryOperator::UAdd,
    }
}

fn convert_compare_op(node: &PyObjectRef) -> CompareOperator {
    match node.type_name().to_string().as_str() {
        "Eq" => CompareOperator::Eq,
        "NotEq" => CompareOperator::NotEq,
        "Lt" => CompareOperator::Lt,
        "LtE" => CompareOperator::LtE,
        "Gt" => CompareOperator::Gt,
        "GtE" => CompareOperator::GtE,
        "Is" => CompareOperator::Is,
        "IsNot" => CompareOperator::IsNot,
        "In" => CompareOperator::In,
        "NotIn" => CompareOperator::NotIn,
        _ => CompareOperator::Eq,
    }
}

fn convert_expr_context(node: &PyObjectRef) -> ExprContext {
    match node.type_name().to_string().as_str() {
        "Store" => ExprContext::Store,
        "Del" => ExprContext::Del,
        _ => ExprContext::Load,
    }
}

fn convert_arguments(node: &PyObjectRef) -> Result<AstArguments, String> {
    let posonlyargs = convert_arg_list(node, "posonlyargs")?;
    let args = convert_arg_list(node, "args")?;
    let vararg = node
        .get_attr("vararg")
        .and_then(|v| {
            if matches!(&v.payload, PyObjectPayload::None) {
                None
            } else {
                Some(convert_arg(&v))
            }
        })
        .transpose()?;
    let kwonlyargs = convert_arg_list(node, "kwonlyargs")?;
    let kw_defaults = get_list_attr(node, "kw_defaults")
        .iter()
        .map(|d| {
            if matches!(&d.payload, PyObjectPayload::None) {
                Ok(None)
            } else {
                convert_expr(d).map(Some)
            }
        })
        .collect::<Result<Vec<_>, String>>()?;
    let kwarg = node
        .get_attr("kwarg")
        .and_then(|v| {
            if matches!(&v.payload, PyObjectPayload::None) {
                None
            } else {
                Some(convert_arg(&v))
            }
        })
        .transpose()?;
    let defaults = convert_expr_list(node, "defaults")?;
    Ok(AstArguments {
        posonlyargs,
        args,
        vararg,
        kwonlyargs,
        kw_defaults,
        kwarg,
        defaults,
    })
}

fn convert_arg_list(parent: &PyObjectRef, attr: &str) -> Result<Vec<AstArg>, String> {
    get_checked_list_attr(parent, attr)?
        .iter()
        .map(convert_arg)
        .collect()
}

fn convert_arg(node: &PyObjectRef) -> Result<AstArg, String> {
    let arg = get_str_attr(node, "arg");
    let annotation = convert_optional_expr(node, "annotation")?;
    if let Some(annotation) = &annotation {
        require_load_context(annotation)?;
    }
    Ok(AstArg {
        arg,
        annotation,
        type_comment: None,
        location: loc_from_node(node),
    })
}

fn convert_keyword_list(parent: &PyObjectRef, attr: &str) -> Result<Vec<AstKeyword>, String> {
    get_checked_list_attr(parent, attr)?
        .iter()
        .map(|k| {
            let arg = get_optional_str(k, "arg");
            let value_node = k
                .get_attr("value")
                .ok_or_else(|| "keyword missing 'value'".to_string())?;
            let value = convert_expr(&value_node)?;
            require_load_context(&value)?;
            Ok(AstKeyword {
                arg,
                value,
                location: loc_from_node(k),
            })
        })
        .collect()
}

fn convert_alias_list(parent: &PyObjectRef, attr: &str) -> Result<Vec<AstAlias>, String> {
    get_checked_list_attr(parent, attr)?
        .iter()
        .map(|a| {
            let name = get_str_attr(a, "name");
            let asname = get_optional_str(a, "asname");
            Ok(AstAlias {
                name,
                asname,
                location: loc_from_node(a),
            })
        })
        .collect()
}

fn convert_comprehension_list(parent: &PyObjectRef) -> Result<Vec<AstComprehension>, String> {
    get_checked_list_attr(parent, "generators")?
        .iter()
        .map(|g| {
            let target_node = g
                .get_attr("target")
                .ok_or_else(|| "comprehension missing 'target'".to_string())?;
            let iter_node = g
                .get_attr("iter")
                .ok_or_else(|| "comprehension missing 'iter'".to_string())?;
            let ifs = convert_expr_list(g, "ifs")?;
            let is_async = g
                .get_attr("is_async")
                .and_then(|v| v.to_int().ok())
                .map(|i| i != 0)
                .unwrap_or(false);
            let target = convert_expr(&target_node)?;
            let iter = convert_expr(&iter_node)?;
            require_store_context(&target)?;
            require_load_context(&iter)?;
            for if_expr in &ifs {
                require_load_context(if_expr)?;
            }
            Ok(AstComprehension {
                target,
                iter,
                ifs,
                is_async,
            })
        })
        .collect()
}
