use super::*;

/// Public API for compile() to use when compiling programmatically-built AST
pub fn ast_unparse_module(node: &PyObjectRef) -> String {
    ast_unparse(node)
}

/// Simplified AST unparse — convert AST node back to Python source
pub(super) fn ast_unparse(node: &PyObjectRef) -> String {
    // Use the class name (type_name()) which is "Module", "Assign", etc.
    let type_name = node.type_name().to_string();
    match type_name.as_str() {
        "Module" => {
            if let Some(body) = node.get_attr("body") {
                if let PyObjectPayload::List(items) = &body.payload {
                    return items
                        .read()
                        .iter()
                        .map(|s| ast_unparse(s))
                        .collect::<Vec<_>>()
                        .join("\n");
                }
            }
            String::new()
        }
        "Assign" => {
            let targets = node
                .get_attr("targets")
                .map(|t| {
                    if let PyObjectPayload::List(items) = &t.payload {
                        items
                            .read()
                            .iter()
                            .map(|t| ast_unparse(t))
                            .collect::<Vec<_>>()
                            .join(", ")
                    } else {
                        ast_unparse(&t)
                    }
                })
                .unwrap_or_default();
            let value = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            format!("{} = {}", targets, value)
        }
        "Name" => node
            .get_attr("id")
            .map(|i| i.py_to_string())
            .unwrap_or_default(),
        "Constant" => {
            if let Some(v) = node.get_attr("value") {
                match &v.payload {
                    PyObjectPayload::Str(s) => format!("'{}'", s),
                    PyObjectPayload::None => "None".to_string(),
                    PyObjectPayload::Bool(b) => if *b { "True" } else { "False" }.to_string(),
                    _ => v.py_to_string(),
                }
            } else {
                "None".to_string()
            }
        }
        "BinOp" => {
            let left = node
                .get_attr("left")
                .map(|l| ast_unparse(&l))
                .unwrap_or_default();
            let right = node
                .get_attr("right")
                .map(|r| ast_unparse(&r))
                .unwrap_or_default();
            let op = node
                .get_attr("op")
                .map(|o| {
                    let op_type = o.type_name().to_string();
                    match op_type.as_str() {
                        "Add" => "+",
                        "Sub" => "-",
                        "Mult" => "*",
                        "Div" => "/",
                        "Mod" => "%",
                        "Pow" => "**",
                        "FloorDiv" => "//",
                        "LShift" => "<<",
                        "RShift" => ">>",
                        "BitOr" => "|",
                        "BitXor" => "^",
                        "BitAnd" => "&",
                        "MatMult" => "@",
                        _ => "?",
                    }
                    .to_string()
                })
                .unwrap_or_else(|| "+".to_string());
            format!("{} {} {}", left, op, right)
        }
        "Return" => {
            let val = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            if val.is_empty() {
                "return".to_string()
            } else {
                format!("return {}", val)
            }
        }
        "Expr" => node
            .get_attr("value")
            .map(|v| ast_unparse(&v))
            .unwrap_or_default(),
        "Call" => {
            let func = node
                .get_attr("func")
                .map(|f| ast_unparse(&f))
                .unwrap_or_default();
            let args_str = node
                .get_attr("args")
                .map(|a| {
                    if let PyObjectPayload::List(items) = &a.payload {
                        items
                            .read()
                            .iter()
                            .map(|a| ast_unparse(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    } else {
                        String::new()
                    }
                })
                .unwrap_or_default();
            format!("{}({})", func, args_str)
        }
        "Attribute" => {
            let val = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            let attr = node
                .get_attr("attr")
                .map(|a| a.py_to_string())
                .unwrap_or_default();
            format!("{}.{}", val, attr)
        }
        "ListComp" => {
            let elt = node
                .get_attr("elt")
                .map(|e| ast_unparse(&e))
                .unwrap_or_default();
            let gens = unparse_generators(node);
            format!("[{} {}]", elt, gens)
        }
        "SetComp" => {
            let elt = node
                .get_attr("elt")
                .map(|e| ast_unparse(&e))
                .unwrap_or_default();
            let gens = unparse_generators(node);
            format!("{{{} {}}}", elt, gens)
        }
        "DictComp" => {
            let key = node
                .get_attr("key")
                .map(|k| ast_unparse(&k))
                .unwrap_or_default();
            let value = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            let gens = unparse_generators(node);
            format!("{{{}: {} {}}}", key, value, gens)
        }
        "GeneratorExp" => {
            let elt = node
                .get_attr("elt")
                .map(|e| ast_unparse(&e))
                .unwrap_or_default();
            let gens = unparse_generators(node);
            format!("({} {})", elt, gens)
        }
        "List" => {
            let elts = unparse_list_attr(node, "elts");
            format!("[{}]", elts)
        }
        "Tuple" => {
            let elts = unparse_list_attr(node, "elts");
            if elts.contains(',') || elts.is_empty() {
                format!("({})", elts)
            } else {
                format!("({},)", elts)
            }
        }
        "Set" => {
            let elts = unparse_list_attr(node, "elts");
            format!("{{{}}}", elts)
        }
        "Dict" => {
            let keys = node
                .get_attr("keys")
                .and_then(|k| {
                    if let PyObjectPayload::List(items) = &k.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|k| ast_unparse(k))
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let values = node
                .get_attr("values")
                .and_then(|v| {
                    if let PyObjectPayload::List(items) = &v.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|v| ast_unparse(v))
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let pairs: Vec<String> = keys
                .iter()
                .zip(values.iter())
                .map(|(k, v)| format!("{}: {}", k, v))
                .collect();
            format!("{{{}}}", pairs.join(", "))
        }
        "Subscript" => {
            let val = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            let slice = node
                .get_attr("slice")
                .map(|s| ast_unparse(&s))
                .unwrap_or_default();
            format!("{}[{}]", val, slice)
        }
        "Index" => node
            .get_attr("value")
            .map(|v| ast_unparse(&v))
            .unwrap_or_default(),
        "Slice" => {
            let lower = node
                .get_attr("lower")
                .map(|l| ast_unparse(&l))
                .unwrap_or_default();
            let upper = node
                .get_attr("upper")
                .map(|u| ast_unparse(&u))
                .unwrap_or_default();
            let step = node
                .get_attr("step")
                .map(|s| ast_unparse(&s))
                .unwrap_or_default();
            if step.is_empty() {
                format!("{}:{}", lower, upper)
            } else {
                format!("{}:{}:{}", lower, upper, step)
            }
        }
        "UnaryOp" => {
            let operand = node
                .get_attr("operand")
                .map(|o| ast_unparse(&o))
                .unwrap_or_default();
            let op = node
                .get_attr("op")
                .map(|o| {
                    match o.type_name().to_string().as_str() {
                        "UAdd" => "+",
                        "USub" => "-",
                        "Not" => "not ",
                        "Invert" => "~",
                        _ => "?",
                    }
                    .to_string()
                })
                .unwrap_or_default();
            format!("{}{}", op, operand)
        }
        "BoolOp" => {
            let op = node
                .get_attr("op")
                .map(|o| {
                    match o.type_name().to_string().as_str() {
                        "And" => " and ",
                        "Or" => " or ",
                        _ => " ? ",
                    }
                    .to_string()
                })
                .unwrap_or_else(|| " ? ".to_string());
            let values = unparse_list_attr(node, "values");
            values.split(", ").collect::<Vec<_>>().join(&op)
        }
        "Compare" => {
            let left = node
                .get_attr("left")
                .map(|l| ast_unparse(&l))
                .unwrap_or_default();
            let ops = node
                .get_attr("ops")
                .and_then(|o| {
                    if let PyObjectPayload::List(items) = &o.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|o| {
                                    match o.type_name().to_string().as_str() {
                                        "Eq" => "==",
                                        "NotEq" => "!=",
                                        "Lt" => "<",
                                        "LtE" => "<=",
                                        "Gt" => ">",
                                        "GtE" => ">=",
                                        "Is" => "is",
                                        "IsNot" => "is not",
                                        "In" => "in",
                                        "NotIn" => "not in",
                                        _ => "?",
                                    }
                                    .to_string()
                                })
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let comparators = node
                .get_attr("comparators")
                .and_then(|c| {
                    if let PyObjectPayload::List(items) = &c.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|c| ast_unparse(c))
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let mut s = left;
            for (op, comp) in ops.iter().zip(comparators.iter()) {
                s = format!("{} {} {}", s, op, comp);
            }
            s
        }
        "IfExp" => {
            let body = node
                .get_attr("body")
                .map(|b| ast_unparse(&b))
                .unwrap_or_default();
            let test = node
                .get_attr("test")
                .map(|t| ast_unparse(&t))
                .unwrap_or_default();
            let orelse = node
                .get_attr("orelse")
                .map(|o| ast_unparse(&o))
                .unwrap_or_default();
            format!("{} if {} else {}", body, test, orelse)
        }
        "Lambda" => {
            let body = node
                .get_attr("body")
                .map(|b| ast_unparse(&b))
                .unwrap_or_default();
            let args_node = node.get_attr("args");
            let params = args_node.map(|a| unparse_arguments(&a)).unwrap_or_default();
            format!("lambda {}: {}", params, body)
        }
        "FunctionDef" | "AsyncFunctionDef" => {
            let name = node
                .get_attr("name")
                .map(|n| n.py_to_string())
                .unwrap_or_default();
            let args_node = node.get_attr("args");
            let params = args_node.map(|a| unparse_arguments(&a)).unwrap_or_default();
            let body = node
                .get_attr("body")
                .and_then(|b| {
                    if let PyObjectPayload::List(items) = &b.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|s| format!("    {}", ast_unparse(s)))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "    pass".to_string());
            let prefix = if type_name == "AsyncFunctionDef" {
                "async def"
            } else {
                "def"
            };
            format!("{} {}({}):\n{}", prefix, name, params, body)
        }
        "ClassDef" => {
            let name = node
                .get_attr("name")
                .map(|n| n.py_to_string())
                .unwrap_or_default();
            let bases = unparse_list_attr(node, "bases");
            let body = node
                .get_attr("body")
                .and_then(|b| {
                    if let PyObjectPayload::List(items) = &b.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|s| format!("    {}", ast_unparse(s)))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "    pass".to_string());
            if bases.is_empty() {
                format!("class {}:\n{}", name, body)
            } else {
                format!("class {}({}):\n{}", name, bases, body)
            }
        }
        "If" => {
            let test = node
                .get_attr("test")
                .map(|t| ast_unparse(&t))
                .unwrap_or_default();
            let body = node
                .get_attr("body")
                .and_then(|b| {
                    if let PyObjectPayload::List(items) = &b.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|s| format!("    {}", ast_unparse(s)))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            format!("if {}:\n{}", test, body)
        }
        "For" | "AsyncFor" => {
            let target = node
                .get_attr("target")
                .map(|t| ast_unparse(&t))
                .unwrap_or_default();
            let iter_val = node
                .get_attr("iter")
                .map(|i| ast_unparse(&i))
                .unwrap_or_default();
            let body = node
                .get_attr("body")
                .and_then(|b| {
                    if let PyObjectPayload::List(items) = &b.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|s| format!("    {}", ast_unparse(s)))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let prefix = if type_name == "AsyncFor" {
                "async for"
            } else {
                "for"
            };
            format!("{} {} in {}:\n{}", prefix, target, iter_val, body)
        }
        "While" => {
            let test = node
                .get_attr("test")
                .map(|t| ast_unparse(&t))
                .unwrap_or_default();
            let body = node
                .get_attr("body")
                .and_then(|b| {
                    if let PyObjectPayload::List(items) = &b.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|s| format!("    {}", ast_unparse(s)))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            format!("while {}:\n{}", test, body)
        }
        "Import" => {
            let names = node
                .get_attr("names")
                .and_then(|n| {
                    if let PyObjectPayload::List(items) = &n.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|alias| {
                                    let name = alias
                                        .get_attr("name")
                                        .map(|n| n.py_to_string())
                                        .unwrap_or_default();
                                    let asname = alias.get_attr("asname").map(|a| a.py_to_string());
                                    if let Some(a) = asname {
                                        if a != "None" {
                                            return format!("{} as {}", name, a);
                                        }
                                    }
                                    name
                                })
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            format!("import {}", names.join(", "))
        }
        "ImportFrom" => {
            let module = node
                .get_attr("module")
                .map(|m| m.py_to_string())
                .unwrap_or_default();
            let names = node
                .get_attr("names")
                .and_then(|n| {
                    if let PyObjectPayload::List(items) = &n.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|alias| {
                                    let name = alias
                                        .get_attr("name")
                                        .map(|n| n.py_to_string())
                                        .unwrap_or_default();
                                    let asname = alias.get_attr("asname").map(|a| a.py_to_string());
                                    if let Some(a) = asname {
                                        if a != "None" {
                                            return format!("{} as {}", name, a);
                                        }
                                    }
                                    name
                                })
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            format!("from {} import {}", module, names.join(", "))
        }
        "Raise" => {
            let exc = node.get_attr("exc").map(|e| ast_unparse(&e));
            let cause = node.get_attr("cause").map(|c| ast_unparse(&c));
            match (exc, cause) {
                (Some(e), Some(c)) => format!("raise {} from {}", e, c),
                (Some(e), None) => format!("raise {}", e),
                _ => "raise".to_string(),
            }
        }
        "Try" => {
            let body = node
                .get_attr("body")
                .and_then(|b| {
                    if let PyObjectPayload::List(items) = &b.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|s| format!("    {}", ast_unparse(s)))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            format!("try:\n{}", body)
        }
        "With" | "AsyncWith" => {
            let items_str = node
                .get_attr("items")
                .and_then(|it| {
                    if let PyObjectPayload::List(items) = &it.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|w| {
                                    let ctx = w
                                        .get_attr("context_expr")
                                        .map(|c| ast_unparse(&c))
                                        .unwrap_or_default();
                                    let var = w.get_attr("optional_vars").map(|v| ast_unparse(&v));
                                    if let Some(v) = var {
                                        format!("{} as {}", ctx, v)
                                    } else {
                                        ctx
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join(", "),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            let prefix = if type_name == "AsyncWith" {
                "async with"
            } else {
                "with"
            };
            format!("{} {}:", prefix, items_str)
        }
        "Pass" => "pass".to_string(),
        "Break" => "break".to_string(),
        "Continue" => "continue".to_string(),
        "Delete" => {
            let targets = unparse_list_attr(node, "targets");
            format!("del {}", targets)
        }
        "Assert" => {
            let test = node
                .get_attr("test")
                .map(|t| ast_unparse(&t))
                .unwrap_or_default();
            let msg = node.get_attr("msg").map(|m| ast_unparse(&m));
            match msg {
                Some(m) => format!("assert {}, {}", test, m),
                None => format!("assert {}", test),
            }
        }
        "AugAssign" => {
            let target = node
                .get_attr("target")
                .map(|t| ast_unparse(&t))
                .unwrap_or_default();
            let value = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            let op = node
                .get_attr("op")
                .map(|o| {
                    match o.type_name().to_string().as_str() {
                        "Add" => "+=",
                        "Sub" => "-=",
                        "Mult" => "*=",
                        "Div" => "/=",
                        "Mod" => "%=",
                        "Pow" => "**=",
                        "FloorDiv" => "//=",
                        "LShift" => "<<=",
                        "RShift" => ">>=",
                        "BitOr" => "|=",
                        "BitXor" => "^=",
                        "BitAnd" => "&=",
                        _ => "?=",
                    }
                    .to_string()
                })
                .unwrap_or_else(|| "?=".to_string());
            format!("{} {} {}", target, op, value)
        }
        "Starred" => {
            let val = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            format!("*{}", val)
        }
        "JoinedStr" => {
            // f-string
            let values = node
                .get_attr("values")
                .and_then(|v| {
                    if let PyObjectPayload::List(items) = &v.payload {
                        Some(
                            items
                                .read()
                                .iter()
                                .map(|v| {
                                    let tn = v.type_name().to_string();
                                    if tn == "FormattedValue" {
                                        let inner = v
                                            .get_attr("value")
                                            .map(|iv| ast_unparse(&iv))
                                            .unwrap_or_default();
                                        format!("{{{}}}", inner)
                                    } else {
                                        ast_unparse(v)
                                    }
                                })
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            format!("f'{}'", values.join(""))
        }
        "FormattedValue" => {
            let val = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            format!("{{{}}}", val)
        }
        "Num" => node
            .get_attr("n")
            .map(|n| n.py_to_string())
            .unwrap_or_else(|| "0".to_string()),
        "Str" => node
            .get_attr("s")
            .map(|s| format!("'{}'", s.py_to_string()))
            .unwrap_or_else(|| "''".to_string()),
        "NameConstant" => node
            .get_attr("value")
            .map(|v| v.py_to_string())
            .unwrap_or_else(|| "None".to_string()),
        "Yield" => {
            let val = node.get_attr("value").map(|v| ast_unparse(&v));
            match val {
                Some(v) => format!("yield {}", v),
                None => "yield".to_string(),
            }
        }
        "YieldFrom" => {
            let val = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            format!("yield from {}", val)
        }
        "Await" => {
            let val = node
                .get_attr("value")
                .map(|v| ast_unparse(&v))
                .unwrap_or_default();
            format!("await {}", val)
        }
        "Global" => {
            let names = unparse_list_attr(node, "names");
            format!("global {}", names)
        }
        "Nonlocal" => {
            let names = unparse_list_attr(node, "names");
            format!("nonlocal {}", names)
        }
        _ => format!("<{}>", type_name),
    }
}

fn unparse_generators(node: &PyObjectRef) -> String {
    node.get_attr("generators")
        .and_then(|g| {
            if let PyObjectPayload::List(items) = &g.payload {
                Some(
                    items
                        .read()
                        .iter()
                        .map(|comp| {
                            let target = comp
                                .get_attr("target")
                                .map(|t| ast_unparse(&t))
                                .unwrap_or_default();
                            let iter_val = comp
                                .get_attr("iter")
                                .map(|i| ast_unparse(&i))
                                .unwrap_or_default();
                            let ifs = comp
                                .get_attr("ifs")
                                .and_then(|i| {
                                    if let PyObjectPayload::List(conds) = &i.payload {
                                        let conds: Vec<String> = conds
                                            .read()
                                            .iter()
                                            .map(|c| format!(" if {}", ast_unparse(c)))
                                            .collect();
                                        Some(conds.join(""))
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or_default();
                            let is_async = comp
                                .get_attr("is_async")
                                .map(|a| a.py_to_string() == "1" || a.py_to_string() == "True")
                                .unwrap_or(false);
                            let prefix = if is_async { "async for" } else { "for" };
                            format!("{} {} in {}{}", prefix, target, iter_val, ifs)
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            } else {
                None
            }
        })
        .unwrap_or_default()
}

fn unparse_list_attr(node: &PyObjectRef, attr: &str) -> String {
    node.get_attr(attr)
        .and_then(|a| {
            if let PyObjectPayload::List(items) = &a.payload {
                Some(
                    items
                        .read()
                        .iter()
                        .map(|i| ast_unparse(i))
                        .collect::<Vec<_>>()
                        .join(", "),
                )
            } else {
                None
            }
        })
        .unwrap_or_default()
}

fn unparse_arguments(args_node: &PyObjectRef) -> String {
    let mut parts = Vec::new();
    if let Some(args_list) = args_node.get_attr("args") {
        if let PyObjectPayload::List(items) = &args_list.payload {
            for arg in items.read().iter() {
                let name = arg
                    .get_attr("arg")
                    .map(|a| a.py_to_string())
                    .unwrap_or_default();
                parts.push(name);
            }
        }
    }
    parts.join(", ")
}
