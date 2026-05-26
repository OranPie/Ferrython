use super::super::ast_convert::convert_expr;
use super::*;

/// Evaluate a constant expression for ast.literal_eval
pub(super) fn eval_const_expr(expr: &ferrython_ast::Expression) -> PyResult<PyObjectRef> {
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
pub(super) fn dump_node(
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

pub(super) fn eval_py_const_node(node: &PyObjectRef) -> PyResult<PyObjectRef> {
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

pub(super) fn source_segment(source: &str, node: &PyObjectRef, padded: bool) -> Option<String> {
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
pub(super) fn collect_ast_nodes(obj: &PyObjectRef, nodes: &mut Vec<PyObjectRef>) {
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
pub(super) fn get_child_nodes(obj: &PyObjectRef) -> Vec<PyObjectRef> {
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
