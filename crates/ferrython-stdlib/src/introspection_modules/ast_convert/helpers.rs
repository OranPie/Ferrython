use super::*;

pub(super) fn is_ast_constant_builtin_subclass(val: &PyObjectRef) -> bool {
    if let PyObjectPayload::Instance(inst) = &val.payload {
        return inst.attrs.read().contains_key("__builtin_value__");
    }
    false
}

pub(super) fn validate_ast_constant_value(val: &PyObjectRef) -> Result<(), String> {
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

pub(super) fn convert_constant(val: &PyObjectRef) -> AstConstant {
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

pub(super) fn convert_operator(node: &PyObjectRef) -> Operator {
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

pub(super) fn convert_bool_op(node: &PyObjectRef) -> BoolOperator {
    match node.type_name().to_string().as_str() {
        "And" => BoolOperator::And,
        "Or" => BoolOperator::Or,
        _ => BoolOperator::And,
    }
}

pub(super) fn convert_unary_op(node: &PyObjectRef) -> UnaryOperator {
    match node.type_name().to_string().as_str() {
        "Invert" => UnaryOperator::Invert,
        "Not" => UnaryOperator::Not,
        "UAdd" => UnaryOperator::UAdd,
        "USub" => UnaryOperator::USub,
        _ => UnaryOperator::UAdd,
    }
}

pub(super) fn convert_compare_op(node: &PyObjectRef) -> CompareOperator {
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

pub(super) fn convert_expr_context(node: &PyObjectRef) -> ExprContext {
    match node.type_name().to_string().as_str() {
        "Store" => ExprContext::Store,
        "Del" => ExprContext::Del,
        _ => ExprContext::Load,
    }
}

pub(super) fn convert_arguments(node: &PyObjectRef) -> Result<AstArguments, String> {
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

pub(super) fn convert_arg_list(parent: &PyObjectRef, attr: &str) -> Result<Vec<AstArg>, String> {
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

pub(super) fn convert_keyword_list(
    parent: &PyObjectRef,
    attr: &str,
) -> Result<Vec<AstKeyword>, String> {
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

pub(super) fn convert_alias_list(
    parent: &PyObjectRef,
    attr: &str,
) -> Result<Vec<AstAlias>, String> {
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

pub(super) fn convert_comprehension_list(
    parent: &PyObjectRef,
) -> Result<Vec<AstComprehension>, String> {
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
