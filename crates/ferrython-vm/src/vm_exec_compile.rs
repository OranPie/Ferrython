//! Exec/eval/compile helpers and syntax error conversion.

use crate::frame::ScopeKind;
use crate::VirtualMachine;
use compact_str::CompactString;
use ferrython_ast::{
    CompareOperator, Constant, Expression, ExpressionKind, Module as AstModule, Statement,
    StatementKind,
};
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    FxAttrMap, PyCell, PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef,
};
use ferrython_core::types::HashableKey;
use std::rc::Rc;

fn validate_single_input(
    module: &AstModule,
    filename: &str,
    source_ends_with_newline: bool,
) -> PyResult<()> {
    fn is_compound_statement(stmt: &Statement) -> bool {
        matches!(
            &stmt.node,
            StatementKind::FunctionDef { .. }
                | StatementKind::ClassDef { .. }
                | StatementKind::For { .. }
                | StatementKind::While { .. }
                | StatementKind::If { .. }
                | StatementKind::With { .. }
                | StatementKind::Try { .. }
                | StatementKind::Match { .. }
        )
    }

    let body = match module {
        AstModule::Module { body, .. } | AstModule::Interactive { body } => body,
        AstModule::Expression { .. } => return Ok(()),
    };
    let Some(first) = body.first() else {
        return Ok(());
    };
    let first_line = first.location.line;
    if body.iter().any(|stmt| stmt.location.line != first_line) {
        return Err(build_syntax_exception(
            ExceptionKind::SyntaxError,
            "multiple statements found while compiling a single statement",
            filename,
            first_line as i64,
            first.location.column as i64 + 1,
        ));
    }
    if is_compound_statement(first)
        && first.location.end_line == Some(first.location.line)
        && !source_ends_with_newline
    {
        return Err(build_syntax_exception(
            ExceptionKind::SyntaxError,
            "invalid syntax",
            filename,
            first_line as i64,
            first.location.column as i64,
        ));
    }
    Ok(())
}

fn validate_ast_compile_mode(module: &AstModule, mode: &str) -> PyResult<()> {
    let ok = matches!(
        (module, mode),
        (AstModule::Module { .. }, "exec")
            | (AstModule::Interactive { .. }, "single")
            | (AstModule::Expression { .. }, "eval")
    );
    if ok {
        Ok(())
    } else {
        Err(PyException::type_error(
            "expected AST matching compile mode".to_string(),
        ))
    }
}

fn warn_compile_filename_deprecated(vm: &mut VirtualMachine) -> PyResult<()> {
    let warnings = vm.import_module_simple("warnings", 0)?;
    let warn = warnings
        .get_attr("warn")
        .ok_or_else(|| PyException::attribute_error("module 'warnings' has no attribute 'warn'"))?;
    let category = warnings.get_attr("DeprecationWarning").ok_or_else(|| {
        PyException::attribute_error("module 'warnings' has no attribute 'DeprecationWarning'")
    })?;
    vm.call_object(
        warn,
        vec![
            PyObject::str_val(CompactString::from(
                "path should be string, bytes, or os.PathLike, not bytearray",
            )),
            category,
        ],
    )?;
    Ok(())
}

fn warn_invalid_escape(vm: &mut VirtualMachine, filename: &str, escape: char) -> PyResult<()> {
    let warnings = vm.import_module_simple("warnings", 0)?;
    let warn = warnings.get_attr("warn_explicit").ok_or_else(|| {
        PyException::attribute_error("module 'warnings' has no attribute 'warn_explicit'")
    })?;
    let category = warnings.get_attr("DeprecationWarning").ok_or_else(|| {
        PyException::attribute_error("module 'warnings' has no attribute 'DeprecationWarning'")
    })?;
    let message = CompactString::from(format!("invalid escape sequence \\{}", escape));
    let result = vm.call_object(
        warn,
        vec![
            PyObject::str_val(message.clone()),
            category,
            PyObject::str_val(CompactString::from(filename)),
            PyObject::int(1),
        ],
    );
    match result {
        Ok(_) => Ok(()),
        Err(err) if err.kind == ExceptionKind::RuntimeError => Err(build_syntax_exception(
            ExceptionKind::SyntaxError,
            &format!("invalid escape sequence \\{}", escape),
            filename,
            1,
            1,
        )),
        Err(err) => Err(err),
    }
}

fn warn_identity_literal(vm: &mut VirtualMachine, filename: &str) -> PyResult<()> {
    let warnings = vm.import_module_simple("warnings", 0)?;
    let warn = warnings.get_attr("warn_explicit").ok_or_else(|| {
        PyException::attribute_error("module 'warnings' has no attribute 'warn_explicit'")
    })?;
    let category = warnings.get_attr("SyntaxWarning").ok_or_else(|| {
        PyException::attribute_error("module 'warnings' has no attribute 'SyntaxWarning'")
    })?;
    let result = vm.call_object(
        warn,
        vec![
            PyObject::str_val(CompactString::from(
                "\"is\" with a literal. Did you mean \"==\"?",
            )),
            category,
            PyObject::str_val(CompactString::from(filename)),
            PyObject::int(1),
        ],
    );
    match result {
        Ok(_) => Ok(()),
        Err(err) if err.kind == ExceptionKind::RuntimeError => Err(build_syntax_exception(
            ExceptionKind::SyntaxError,
            "\"is\" with a literal. Did you mean \"==\"?",
            filename,
            1,
            1,
        )),
        Err(err) => Err(err),
    }
}

fn parse_with_compile_warnings(
    vm: &mut VirtualMachine,
    source: &str,
    filename: &str,
    expression: bool,
) -> PyResult<AstModule> {
    if let Some(ch) = first_deprecated_escape(source) {
        warn_invalid_escape(vm, filename, ch)?;
    }
    let parsed = if expression {
        ferrython_parser::parse_expression(source, filename).map(|expr| AstModule::Expression {
            body: Box::new(expr),
        })
    } else {
        ferrython_parser::parse(source, filename)
    };
    let module = parsed.map_err(|e| parse_error_to_syntax_exc(filename, e))?;
    if module_has_identity_literal(&module) {
        warn_identity_literal(vm, filename)?;
    }
    Ok(module)
}

fn module_has_identity_literal(module: &AstModule) -> bool {
    match module {
        AstModule::Module { body, .. } | AstModule::Interactive { body } => {
            body.iter().any(stmt_has_identity_literal)
        }
        AstModule::Expression { body } => expr_has_identity_literal(body),
    }
}

fn stmt_has_identity_literal(stmt: &Statement) -> bool {
    match &stmt.node {
        StatementKind::Expr { value } => expr_has_identity_literal(value),
        StatementKind::Assign { targets, value, .. } => {
            targets.iter().any(expr_has_identity_literal) || expr_has_identity_literal(value)
        }
        StatementKind::AnnAssign {
            target,
            annotation,
            value,
            ..
        } => {
            expr_has_identity_literal(target)
                || expr_has_identity_literal(annotation)
                || value
                    .as_ref()
                    .map(|expr| expr_has_identity_literal(expr))
                    .unwrap_or(false)
        }
        StatementKind::AugAssign { target, value, .. } => {
            expr_has_identity_literal(target) || expr_has_identity_literal(value)
        }
        StatementKind::Return { value } => value
            .as_ref()
            .map(|expr| expr_has_identity_literal(expr))
            .unwrap_or(false),
        StatementKind::If { test, body, orelse } | StatementKind::While { test, body, orelse } => {
            expr_has_identity_literal(test)
                || body.iter().any(stmt_has_identity_literal)
                || orelse.iter().any(stmt_has_identity_literal)
        }
        StatementKind::For {
            target,
            iter,
            body,
            orelse,
            ..
        } => {
            expr_has_identity_literal(target)
                || expr_has_identity_literal(iter)
                || body.iter().any(stmt_has_identity_literal)
                || orelse.iter().any(stmt_has_identity_literal)
        }
        StatementKind::With { items, body, .. } => {
            items.iter().any(|item| {
                expr_has_identity_literal(&item.context_expr)
                    || item
                        .optional_vars
                        .as_ref()
                        .map(|expr| expr_has_identity_literal(expr))
                        .unwrap_or(false)
            }) || body.iter().any(stmt_has_identity_literal)
        }
        StatementKind::Try {
            body,
            handlers,
            orelse,
            finalbody,
        } => {
            body.iter().any(stmt_has_identity_literal)
                || handlers.iter().any(|handler| {
                    handler
                        .typ
                        .as_ref()
                        .map(|expr| expr_has_identity_literal(expr))
                        .unwrap_or(false)
                        || handler.body.iter().any(stmt_has_identity_literal)
                })
                || orelse.iter().any(stmt_has_identity_literal)
                || finalbody.iter().any(stmt_has_identity_literal)
        }
        StatementKind::FunctionDef {
            body,
            decorator_list,
            returns,
            ..
        } => {
            decorator_list.iter().any(expr_has_identity_literal)
                || returns
                    .as_ref()
                    .map(|expr| expr_has_identity_literal(expr))
                    .unwrap_or(false)
                || body.iter().any(stmt_has_identity_literal)
        }
        StatementKind::ClassDef {
            bases,
            keywords,
            body,
            decorator_list,
            ..
        } => {
            bases.iter().any(expr_has_identity_literal)
                || keywords
                    .iter()
                    .any(|kw| expr_has_identity_literal(&kw.value))
                || decorator_list.iter().any(expr_has_identity_literal)
                || body.iter().any(stmt_has_identity_literal)
        }
        StatementKind::Raise { exc, cause } => {
            exc.as_ref()
                .map(|expr| expr_has_identity_literal(expr))
                .unwrap_or(false)
                || cause
                    .as_ref()
                    .map(|expr| expr_has_identity_literal(expr))
                    .unwrap_or(false)
        }
        StatementKind::Assert { test, msg } => {
            expr_has_identity_literal(test)
                || msg
                    .as_ref()
                    .map(|expr| expr_has_identity_literal(expr))
                    .unwrap_or(false)
        }
        _ => false,
    }
}

fn expr_has_identity_literal(expr: &Expression) -> bool {
    match &expr.node {
        ExpressionKind::Compare {
            left,
            ops,
            comparators,
        } => {
            let mut previous = left.as_ref();
            for (op, comparator) in ops.iter().zip(comparators.iter()) {
                if matches!(op, CompareOperator::Is | CompareOperator::IsNot)
                    && (is_warning_literal(previous) || is_warning_literal(comparator))
                {
                    return true;
                }
                previous = comparator;
            }
            expr_has_identity_literal(left) || comparators.iter().any(expr_has_identity_literal)
        }
        ExpressionKind::BoolOp { values, .. } => values.iter().any(expr_has_identity_literal),
        ExpressionKind::BinOp { left, right, .. } => {
            expr_has_identity_literal(left) || expr_has_identity_literal(right)
        }
        ExpressionKind::UnaryOp { operand, .. } => expr_has_identity_literal(operand),
        ExpressionKind::Lambda { body, .. } => expr_has_identity_literal(body),
        ExpressionKind::IfExp { test, body, orelse } => {
            expr_has_identity_literal(test)
                || expr_has_identity_literal(body)
                || expr_has_identity_literal(orelse)
        }
        ExpressionKind::Dict { keys, values } => {
            keys.iter().flatten().any(expr_has_identity_literal)
                || values.iter().any(expr_has_identity_literal)
        }
        ExpressionKind::Set { elts }
        | ExpressionKind::List { elts, .. }
        | ExpressionKind::Tuple { elts, .. } => elts.iter().any(expr_has_identity_literal),
        ExpressionKind::ListComp { elt, generators }
        | ExpressionKind::SetComp { elt, generators }
        | ExpressionKind::GeneratorExp { elt, generators } => {
            expr_has_identity_literal(elt)
                || generators.iter().any(|gen| {
                    expr_has_identity_literal(&gen.target)
                        || expr_has_identity_literal(&gen.iter)
                        || gen.ifs.iter().any(expr_has_identity_literal)
                })
        }
        ExpressionKind::DictComp {
            key,
            value,
            generators,
        } => {
            expr_has_identity_literal(key)
                || expr_has_identity_literal(value)
                || generators.iter().any(|gen| {
                    expr_has_identity_literal(&gen.target)
                        || expr_has_identity_literal(&gen.iter)
                        || gen.ifs.iter().any(expr_has_identity_literal)
                })
        }
        ExpressionKind::Await { value }
        | ExpressionKind::Yield { value: Some(value) }
        | ExpressionKind::YieldFrom { value } => expr_has_identity_literal(value),
        ExpressionKind::Call {
            func,
            args,
            keywords,
        } => {
            expr_has_identity_literal(func)
                || args.iter().any(expr_has_identity_literal)
                || keywords
                    .iter()
                    .any(|kw| expr_has_identity_literal(&kw.value))
        }
        ExpressionKind::FormattedValue {
            value, format_spec, ..
        } => {
            expr_has_identity_literal(value)
                || format_spec
                    .as_ref()
                    .map(|expr| expr_has_identity_literal(expr))
                    .unwrap_or(false)
        }
        ExpressionKind::JoinedStr { values } => values.iter().any(expr_has_identity_literal),
        ExpressionKind::Attribute { value, .. }
        | ExpressionKind::Subscript { value, .. }
        | ExpressionKind::Starred { value, .. } => expr_has_identity_literal(value),
        ExpressionKind::Slice { lower, upper, step } => {
            lower
                .as_ref()
                .map(|expr| expr_has_identity_literal(expr))
                .unwrap_or(false)
                || upper
                    .as_ref()
                    .map(|expr| expr_has_identity_literal(expr))
                    .unwrap_or(false)
                || step
                    .as_ref()
                    .map(|expr| expr_has_identity_literal(expr))
                    .unwrap_or(false)
        }
        _ => false,
    }
}

fn is_warning_literal(expr: &Expression) -> bool {
    matches!(
        &expr.node,
        ExpressionKind::Constant {
            value: Constant::Int(_)
                | Constant::Float(_)
                | Constant::Complex { .. }
                | Constant::Str(_)
                | Constant::Bytes(_)
        }
    )
}

fn first_deprecated_escape(source: &str) -> Option<char> {
    let mut chars = source.chars().peekable();
    let mut in_string = false;
    let mut raw = false;
    let mut bytes = false;
    let mut quote = '\0';
    let mut triple = false;
    while let Some(c) = chars.next() {
        if !in_string {
            raw = false;
            bytes = false;
            let mut prefix = String::new();
            let mut probe = c;
            if matches!(probe, 'r' | 'R' | 'b' | 'B' | 'u' | 'U') {
                prefix.push(probe);
                if let Some(next) = chars.peek().copied() {
                    if matches!(next, 'r' | 'R' | 'b' | 'B') {
                        prefix.push(chars.next().unwrap());
                        probe = chars.peek().copied().unwrap_or('\0');
                    } else {
                        probe = next;
                    }
                }
            }
            if probe == '\'' || probe == '"' {
                if !prefix.is_empty() && c != probe {
                    chars.next();
                }
                in_string = true;
                raw = prefix.chars().any(|p| matches!(p, 'r' | 'R'));
                bytes = prefix.chars().any(|p| matches!(p, 'b' | 'B'));
                quote = probe;
                triple = chars.peek() == Some(&quote) && {
                    let mut iter = chars.clone();
                    iter.next();
                    iter.peek() == Some(&quote)
                };
                if triple {
                    chars.next();
                    chars.next();
                }
            }
            continue;
        }
        if c == quote {
            if triple {
                if chars.peek() == Some(&quote) {
                    let mut iter = chars.clone();
                    iter.next();
                    if iter.peek() == Some(&quote) {
                        chars.next();
                        chars.next();
                        in_string = false;
                    }
                }
            } else {
                in_string = false;
            }
            continue;
        }
        if c == '\\' {
            if let Some(next) = chars.next() {
                if !raw && !is_valid_escape_start(next, bytes) {
                    return Some(next);
                }
            }
        }
    }
    None
}

fn is_valid_escape_start(c: char, bytes: bool) -> bool {
    matches!(
        c,
        '\n' | '\r' | '"' | '\'' | '\\' | 'a' | 'b' | 'f' | 'n' | 'r' | 't' | 'v' | 'x' | '0'..='7'
    ) || (!bytes && matches!(c, 'u' | 'U' | 'N'))
}

fn compile_filename_arg(obj: &PyObjectRef) -> PyResult<String> {
    match &obj.payload {
        PyObjectPayload::Str(s) => {
            if s.as_str().contains('\0') {
                return Err(PyException::value_error(
                    "source code string cannot contain null bytes",
                ));
            }
            Ok(s.as_str().to_owned())
        }
        PyObjectPayload::Bytes(bytes) | PyObjectPayload::ByteArray(bytes) => {
            std::str::from_utf8(bytes)
                .map(|s| s.to_string())
                .map_err(|_| {
                    PyException::value_error("source code string cannot contain null bytes")
                })
        }
        PyObjectPayload::Instance(_) if obj.get_attr("__memoryview__").is_some() => {
            if let Some(base) = obj.get_attr("obj") {
                match &base.payload {
                    PyObjectPayload::Bytes(bytes) | PyObjectPayload::ByteArray(bytes) => {
                        return std::str::from_utf8(bytes)
                            .map(|s| s.to_string())
                            .map_err(|_| {
                                PyException::value_error(
                                    "source code string cannot contain null bytes",
                                )
                            });
                    }
                    _ => {}
                }
            }
            Ok(obj.py_to_string())
        }
        PyObjectPayload::List(_) => Err(PyException::type_error(
            "compile() arg 2 must be a string, bytes or os.PathLike object",
        )),
        _ => Ok(obj.py_to_string()),
    }
}

fn compile_filename_warns(obj: &PyObjectRef) -> bool {
    matches!(&obj.payload, PyObjectPayload::ByteArray(_))
        || matches!(&obj.payload, PyObjectPayload::Instance(_))
            && obj.get_attr("__memoryview__").is_some()
}

fn source_arg_to_string(
    obj: &PyObjectRef,
    filename: &str,
    type_error_message: &str,
) -> PyResult<String> {
    match &obj.payload {
        PyObjectPayload::Str(s) => {
            if s.as_str().contains('\0') {
                return Err(PyException::value_error(
                    "source code string cannot contain null bytes",
                ));
            }
            Ok(s.as_str().to_owned())
        }
        PyObjectPayload::Bytes(bytes) | PyObjectPayload::ByteArray(bytes) => {
            decode_source_bytes(bytes, filename)
        }
        PyObjectPayload::Instance(_) if obj.get_attr("__memoryview__").is_some() => {
            if let Some(base) = obj.get_attr("obj") {
                if let PyObjectPayload::Bytes(bytes) | PyObjectPayload::ByteArray(bytes) =
                    &base.payload
                {
                    return decode_source_bytes(bytes, filename);
                }
            }
            Err(PyException::type_error(type_error_message))
        }
        _ => Err(PyException::type_error(type_error_message)),
    }
}

fn decode_source_bytes(bytes: &[u8], filename: &str) -> PyResult<String> {
    if bytes.contains(&0) {
        return Err(PyException::value_error(
            "source code string cannot contain null bytes",
        ));
    }
    let encoding = source_encoding(bytes);
    match encoding.as_str() {
        "utf-8" | "utf8" => std::str::from_utf8(bytes)
            .map(|s| s.to_string())
            .map_err(|_| {
                build_syntax_exception(
                    ExceptionKind::SyntaxError,
                    "invalid or missing encoding declaration",
                    filename,
                    1,
                    0,
                )
            }),
        "latin-1" | "latin1" | "iso-8859-1" | "iso8859-1" => {
            Ok(bytes.iter().map(|b| char::from(*b)).collect())
        }
        "iso-8859-15" | "iso8859-15" | "iso_8859_15" | "latin-9" | "latin9" => {
            Ok(bytes.iter().map(|b| decode_iso_8859_15_byte(*b)).collect())
        }
        _ => Err(build_syntax_exception(
            ExceptionKind::SyntaxError,
            &format!("unknown encoding: {}", encoding),
            filename,
            0,
            0,
        )),
    }
}

fn source_encoding(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("");
    if let Some(encoding) = coding_cookie(first) {
        return normalize_encoding(&encoding);
    }
    let first_prefix = first.trim_start_matches('\u{feff}').trim_start();
    if first_prefix.is_empty() || first_prefix.starts_with('#') {
        if let Some(second) = lines.next() {
            if let Some(encoding) = coding_cookie(second) {
                return normalize_encoding(&encoding);
            }
        }
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return "utf-8".to_string();
    }
    "utf-8".to_string()
}

fn coding_cookie(line: &str) -> Option<String> {
    let line = line.trim_start();
    if !(line.starts_with('#') || line.starts_with("\u{feff}#")) {
        return None;
    }
    let marker = "coding";
    let idx = line.find(marker)?;
    let mut rest = &line[idx + marker.len()..];
    rest = rest.trim_start();
    if rest.starts_with(':') || rest.starts_with('=') {
        rest = &rest[1..];
    } else {
        return None;
    }
    rest = rest.trim_start();
    let end = rest
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.'))
        .unwrap_or(rest.len());
    if end == 0 {
        None
    } else {
        Some(rest[..end].to_string())
    }
}

fn normalize_encoding(encoding: &str) -> String {
    encoding
        .trim()
        .to_ascii_lowercase()
        .replace('_', "-")
        .replace('.', "-")
}

fn decode_iso_8859_15_byte(byte: u8) -> char {
    match byte {
        0xA4 => '\u{20AC}',
        0xA6 => '\u{0160}',
        0xA8 => '\u{0161}',
        0xB4 => '\u{017D}',
        0xB8 => '\u{017E}',
        0xBC => '\u{0152}',
        0xBD => '\u{0153}',
        0xBE => '\u{0178}',
        _ => char::from(byte),
    }
}

impl VirtualMachine {
    // ── exec/eval/compile helpers (moved from vm_call.rs) ──

    fn code_requires_closure(builtin: &str) -> PyException {
        PyException::type_error(format!(
            "{}() code object passed to {}() may not contain free variables",
            builtin, builtin
        ))
    }

    fn caller_locals_for_exec(&mut self) -> PyObjectRef {
        let Some(frame) = self.call_stack.last() else {
            return PyObject::dict(ferrython_core::object::new_fx_hashkey_map());
        };
        if let Some(locals) = &frame.exec_locals {
            return locals.clone();
        }
        if matches!(frame.scope_kind, ScopeKind::Module) {
            return PyObject::wrap(PyObjectPayload::InstanceDict(frame.globals.clone()));
        }
        if matches!(frame.scope_kind, ScopeKind::Class) {
            let local_names = self.call_stack.last_mut().unwrap().ensure_local_names();
            return PyObject::wrap(PyObjectPayload::InstanceDict(local_names));
        }
        PyObject::dict(self.frame_locals_map(frame))
    }

    pub(crate) fn builtin_exec(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() || args.len() > 3 {
            return Err(PyException::type_error("exec() takes 1 to 3 arguments"));
        }
        let code = if let PyObjectPayload::Code(co) = &args[0].payload {
            Rc::clone(co)
        } else {
            let code_str = source_arg_to_string(
                &args[0],
                "<string>",
                "exec() arg 1 must be a string, bytes or code object",
            )?;
            let module = ferrython_parser::parse(&code_str, "<string>")
                .map_err(|e| PyException::syntax_error(format!("exec: {}", e)))?;
            Rc::new(
                ferrython_compiler::compile(&module, "<string>")
                    .map_err(|e| PyException::syntax_error(format!("exec: {}", e)))?,
            )
        };
        if matches!(&args[0].payload, PyObjectPayload::Code(_)) && !code.freevars.is_empty() {
            return Err(Self::code_requires_closure("exec"));
        }
        if args.len() >= 2 {
            // Accept both Dict and InstanceDict (returned by globals())
            if let PyObjectPayload::InstanceDict(ref shared_globals) = args[1].payload {
                self.ensure_exec_builtins_in_attrmap(shared_globals);
                // InstanceDict: execute directly with the shared attr map
                let locals_dict = if args.len() >= 3 {
                    Some(&args[2])
                } else {
                    None
                };
                if let Some(ld) = locals_dict {
                    self.execute_with_globals_and_locals_obj(
                        code,
                        shared_globals.clone(),
                        Some(ld.clone()),
                        Some(args[1].clone()),
                    )?;
                } else {
                    self.execute_with_globals_and_locals_obj(
                        code,
                        shared_globals.clone(),
                        None,
                        Some(args[1].clone()),
                    )?;
                }
            } else if let PyObjectPayload::Dict(ref map) = args[1].payload {
                self.ensure_exec_builtins_in_dict(map);
                let shared = Rc::new(PyCell::new(
                    map.read()
                        .iter()
                        .filter_map(|(k, v)| {
                            if let HashableKey::Str(s) = k {
                                Some((s.to_compact_string(), v.clone()))
                            } else {
                                None
                            }
                        })
                        .collect(),
                ));
                let exec_locals = if args.len() >= 3 {
                    Some(args[2].clone())
                } else {
                    None
                };
                self.execute_with_globals_and_locals_obj(
                    code,
                    shared.clone(),
                    exec_locals,
                    Some(args[1].clone()),
                )?;
                if args.len() >= 3 {
                    let results = shared.read();
                    let mut gm = map.write();
                    for (k, v) in results.iter() {
                        gm.insert(HashableKey::str_key(k.clone()), v.clone());
                    }
                } else {
                    // No separate locals — write everything back to globals
                    let results = shared.read();
                    let mut m = map.write();
                    for (k, v) in results.iter() {
                        m.insert(HashableKey::str_key(k.clone()), v.clone());
                    }
                }
            } else {
                return Err(PyException::type_error("exec() globals must be a dict"));
            }
        } else {
            let globals = self.call_stack.last().unwrap().globals.clone();
            let locals = self.caller_locals_for_exec();
            self.execute_with_globals_and_locals_obj(code, globals, Some(locals), None)?;
        }
        Ok(PyObject::none())
    }

    fn ensure_exec_builtins_in_attrmap(&mut self, globals: &Rc<PyCell<FxAttrMap>>) {
        if globals.read().contains_key("__builtins__") {
            return;
        }
        if let Some(builtins_mod) = self.builtins_module() {
            globals
                .write()
                .insert(CompactString::from("__builtins__"), builtins_mod);
        }
    }

    fn ensure_exec_builtins_in_dict(
        &mut self,
        globals: &Rc<PyCell<ferrython_core::object::FxHashKeyMap>>,
    ) {
        let key = HashableKey::str_key(CompactString::from("__builtins__"));
        if globals.read().contains_key(&key) {
            return;
        }
        if let Some(builtins_mod) = self.builtins_module() {
            globals.write().insert(key, builtins_mod);
        }
    }

    pub(crate) fn exec_locals_get(
        &mut self,
        locals_obj: &PyObjectRef,
        name: &str,
    ) -> PyResult<Option<PyObjectRef>> {
        let key = PyObject::str_val(CompactString::from(name));
        match &locals_obj.payload {
            PyObjectPayload::Dict(map) => Ok(map
                .read()
                .get(&HashableKey::str_key(CompactString::from(name)))
                .cloned()),
            PyObjectPayload::InstanceDict(map) => Ok(map.read().get(name).cloned()),
            PyObjectPayload::Instance(inst) => {
                if let Some(ref ds) = inst.dict_storage {
                    if Self::class_has_user_override(&inst.class, "__getitem__") {
                        let Some(getitem) = locals_obj.get_attr("__getitem__") else {
                            return Err(PyException::type_error("exec() locals must be a mapping"));
                        };
                        return match self.call_object(getitem, vec![key]) {
                            Ok(value) => Ok(Some(value)),
                            Err(e) if e.kind == ExceptionKind::KeyError => Ok(None),
                            Err(e) => Err(e),
                        };
                    }
                    let hk = self.vm_to_hashable_key(&key)?;
                    Ok(ds.read().get(&hk).cloned())
                } else {
                    let Some(getitem) = locals_obj.get_attr("__getitem__") else {
                        return Err(PyException::type_error("exec() locals must be a mapping"));
                    };
                    match self.call_object(getitem, vec![key]) {
                        Ok(value) => Ok(Some(value)),
                        Err(e) if e.kind == ExceptionKind::KeyError => Ok(None),
                        Err(e) => Err(e),
                    }
                }
            }
            _ => {
                let Some(getitem) = locals_obj.get_attr("__getitem__") else {
                    return Err(PyException::type_error("exec() locals must be a mapping"));
                };
                match self.call_object(getitem, vec![key]) {
                    Ok(value) => Ok(Some(value)),
                    Err(e) if e.kind == ExceptionKind::KeyError => Ok(None),
                    Err(e) => Err(e),
                }
            }
        }
    }

    pub(crate) fn exec_locals_set(
        &mut self,
        locals_obj: &PyObjectRef,
        name: &str,
        value: PyObjectRef,
    ) -> PyResult<()> {
        match &locals_obj.payload {
            PyObjectPayload::Dict(map) => {
                map.write()
                    .insert(HashableKey::str_key(CompactString::from(name)), value);
                Ok(())
            }
            PyObjectPayload::InstanceDict(map) => {
                map.write().insert(CompactString::from(name), value);
                Ok(())
            }
            PyObjectPayload::Instance(inst) => {
                let key = PyObject::str_val(CompactString::from(name));
                if let Some(ref ds) = inst.dict_storage {
                    if Self::class_has_user_override(&inst.class, "__setitem__") {
                        let Some(setitem) = locals_obj.get_attr("__setitem__") else {
                            return Err(PyException::type_error("exec() locals must be a mapping"));
                        };
                        self.call_object(setitem, vec![key, value])?;
                        return Ok(());
                    }
                    let hk = self.vm_to_hashable_key(&key)?;
                    ds.write().insert(hk, value);
                    Ok(())
                } else {
                    let Some(setitem) = locals_obj.get_attr("__setitem__") else {
                        return Err(PyException::type_error("exec() locals must be a mapping"));
                    };
                    self.call_object(setitem, vec![key, value])?;
                    Ok(())
                }
            }
            _ => {
                let Some(setitem) = locals_obj.get_attr("__setitem__") else {
                    return Err(PyException::type_error("exec() locals must be a mapping"));
                };
                let key = PyObject::str_val(CompactString::from(name));
                self.call_object(setitem, vec![key, value])?;
                Ok(())
            }
        }
    }

    pub(crate) fn exec_locals_keys(
        &mut self,
        locals_obj: &PyObjectRef,
    ) -> PyResult<Vec<PyObjectRef>> {
        match &locals_obj.payload {
            PyObjectPayload::Dict(map) => Ok(map.read().keys().map(|k| k.to_object()).collect()),
            PyObjectPayload::InstanceDict(map) => Ok(map
                .read()
                .keys()
                .map(|k| PyObject::str_val(k.clone()))
                .collect()),
            PyObjectPayload::Instance(inst) => {
                if let Some(ref ds) = inst.dict_storage {
                    if let Some(keys_method) = locals_obj.get_attr("keys") {
                        let keys_obj = self.call_object(keys_method, vec![])?;
                        return self.collect_iterable(&keys_obj);
                    }
                    Ok(ds.read().keys().map(|k| k.to_object()).collect())
                } else {
                    let Some(keys_method) = locals_obj.get_attr("keys") else {
                        return Err(PyException::type_error("exec() locals must be a mapping"));
                    };
                    let keys_obj = self.call_object(keys_method, vec![])?;
                    self.collect_iterable(&keys_obj)
                }
            }
            _ => {
                let Some(keys_method) = locals_obj.get_attr("keys") else {
                    return Err(PyException::type_error("exec() locals must be a mapping"));
                };
                let keys_obj = self.call_object(keys_method, vec![])?;
                self.collect_iterable(&keys_obj)
            }
        }
    }

    fn merge_dict_into_attrmap(dict_obj: &PyObjectRef, target: &mut FxAttrMap) {
        if let PyObjectPayload::Dict(ref lmap) = dict_obj.payload {
            let lm = lmap.read();
            for (k, v) in lm.iter() {
                let key_str = match k {
                    HashableKey::Str(s) => s.to_compact_string(),
                    _ => CompactString::from(format!("{:?}", k)),
                };
                target.insert(key_str, v.clone());
            }
        } else if let PyObjectPayload::InstanceDict(ref imap) = dict_obj.payload {
            let im = imap.read();
            for (k, v) in im.iter() {
                target.insert(k.clone(), v.clone());
            }
        }
    }

    fn write_back_locals(
        locals_obj: &PyObjectRef,
        results: &FxAttrMap,
        original_global_keys: &[CompactString],
    ) {
        if let PyObjectPayload::Dict(ref lmap) = locals_obj.payload {
            let mut lm = lmap.write();
            for (k, v) in results.iter() {
                if !original_global_keys.contains(k)
                    || lm.contains_key(&HashableKey::str_key(k.clone()))
                {
                    lm.insert(HashableKey::str_key(k.clone()), v.clone());
                }
            }
        } else if let PyObjectPayload::InstanceDict(ref imap) = locals_obj.payload {
            let mut im = imap.write();
            for (k, v) in results.iter() {
                if !original_global_keys.contains(k) || im.contains_key(k) {
                    im.insert(k.clone(), v.clone());
                }
            }
        }
    }

    pub(crate) fn builtin_eval(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.is_empty() || args.len() > 3 {
            return Err(PyException::type_error("eval() takes 1 to 3 arguments"));
        }
        // Accept either a string or a code object (from compile())
        let code = if let PyObjectPayload::Code(co) = &args[0].payload {
            Rc::clone(co)
        } else {
            let code_str = source_arg_to_string(
                &args[0],
                "<string>",
                "eval() arg 1 must be a string, bytes or code object",
            )?;
            let wrapped = format!("__eval_result__ = ({})", code_str);
            let module = parse_with_compile_warnings(self, &wrapped, "<string>", false)?;
            Rc::new(
                ferrython_compiler::compile(&module, "<string>")
                    .map_err(|e| PyException::syntax_error(format!("eval: {}", e)))?,
            )
        };
        let is_code_obj = matches!(&args[0].payload, PyObjectPayload::Code(_));
        if is_code_obj && !code.freevars.is_empty() {
            return Err(Self::code_requires_closure("eval"));
        }
        if args.len() >= 2 && !matches!(&args[1].payload, PyObjectPayload::None) {
            // Extract globals as FxAttrMap, whether Dict or InstanceDict
            let (new_globals, globals_source) =
                if let PyObjectPayload::InstanceDict(ref shared_g) = args[1].payload {
                    let g = shared_g.read().clone();
                    (g, Some(args[1].clone()))
                } else if let PyObjectPayload::Dict(ref globs_map) = args[1].payload {
                    let mut ng = FxAttrMap::default();
                    let gm = globs_map.read();
                    for (k, v) in gm.iter() {
                        let key_str = match k {
                            HashableKey::Str(s) => s.to_compact_string(),
                            _ => CompactString::from(format!("{:?}", k)),
                        };
                        ng.insert(key_str, v.clone());
                    }
                    (ng, Some(args[1].clone()))
                } else {
                    return Err(PyException::type_error("eval() globals must be a dict"));
                };

            let mut exec_globals = new_globals;

            // Check if we have a separate locals dict (args[2] that is not None)
            let has_separate_locals =
                args.len() >= 3 && !matches!(&args[2].payload, PyObjectPayload::None);

            // Merge locals entries into globals for name resolution
            let original_global_keys: std::collections::HashSet<CompactString> =
                exec_globals.keys().cloned().collect();
            if has_separate_locals {
                Self::merge_dict_into_attrmap(&args[2], &mut exec_globals);
            }

            let shared = Rc::new(PyCell::new(exec_globals));
            let exec_result = self.execute_with_globals(code, shared.clone())?;

            // Check for __eval_result__ (compile(mode='eval') wrapping)
            let eval_result = shared.read().get("__eval_result__").cloned();

            // Write results back to the appropriate dicts
            let results = shared.read();
            if has_separate_locals {
                // Write back globals
                if let PyObjectPayload::InstanceDict(ref sg) =
                    globals_source.as_ref().unwrap().payload
                {
                    let mut gm = sg.write();
                    for (k, v) in results.iter() {
                        if original_global_keys.contains(k) {
                            gm.insert(k.clone(), v.clone());
                        }
                    }
                } else if let PyObjectPayload::Dict(ref globs_map) =
                    globals_source.as_ref().unwrap().payload
                {
                    let mut gm = globs_map.write();
                    for (k, v) in results.iter() {
                        if original_global_keys.contains(k) {
                            gm.insert(HashableKey::str_key(k.clone()), v.clone());
                        }
                    }
                }
                // Write back locals
                let ogk: Vec<CompactString> = original_global_keys.into_iter().collect();
                Self::write_back_locals(&args[2], &results, &ogk);
            } else {
                // No separate locals: write everything back to globals
                if let PyObjectPayload::InstanceDict(ref sg) =
                    globals_source.as_ref().unwrap().payload
                {
                    let mut gm = sg.write();
                    for (k, v) in results.iter() {
                        gm.insert(k.clone(), v.clone());
                    }
                } else if let PyObjectPayload::Dict(ref globs_map) =
                    globals_source.as_ref().unwrap().payload
                {
                    let mut gm = globs_map.write();
                    for (k, v) in results.iter() {
                        gm.insert(HashableKey::str_key(k.clone()), v.clone());
                    }
                }
            }
            drop(results);

            if let Some(val) = eval_result {
                return Ok(val);
            }
            if is_code_obj {
                return Ok(exec_result);
            }
            Ok(PyObject::none())
        } else {
            let globals = self.call_stack.last().unwrap().globals.clone();
            // Merge in locals so names defined in the enclosing function scope are visible to eval().
            // Only needed for non-module frames; at module level, locals == globals.
            let is_module = matches!(
                self.call_stack.last().unwrap().scope_kind,
                ScopeKind::Module
            );
            let shared = if is_module {
                globals
            } else {
                let mut merged = globals.read().clone();
                let frame = self.call_stack.last().unwrap();
                for (i, name) in frame.code.varnames.iter().enumerate() {
                    if let Some(Some(val)) = frame.locals.get(i) {
                        merged.insert(name.clone(), val.clone());
                    }
                }
                for (k, v) in frame.local_names_snapshot() {
                    merged.insert(k.clone(), v.clone());
                }
                for (i, name) in frame
                    .code
                    .cellvars
                    .iter()
                    .chain(frame.code.freevars.iter())
                    .enumerate()
                {
                    if let Some(cell) = frame.cells.get(i) {
                        if let Some(val) = cell.read().as_ref() {
                            merged.insert(name.clone(), val.clone());
                        }
                    }
                }
                Rc::new(PyCell::new(merged))
            };
            let exec_result = self.execute_with_globals(code, shared.clone())?;
            // Check for __eval_result__
            if let Some(val) = shared.read().get("__eval_result__").cloned() {
                return Ok(val);
            }
            if is_code_obj {
                return Ok(exec_result);
            }
            let result = shared
                .read()
                .get("__eval_result__")
                .cloned()
                .unwrap_or_else(PyObject::none);
            Ok(result)
        }
    }

    pub(crate) fn builtin_compile(&mut self, args: &[PyObjectRef]) -> PyResult<PyObjectRef> {
        if args.len() < 3 {
            return Err(PyException::type_error(
                "compile() requires at least 3 arguments",
            ));
        }
        let filename = compile_filename_arg(&args[1])?;
        if compile_filename_warns(&args[1]) {
            warn_compile_filename_deprecated(self)?;
        }
        let mode = args[2].py_to_string();
        let flags = if args.len() > 3 && !matches!(&args[3].payload, PyObjectPayload::Dict(_)) {
            args[3].as_int().unwrap_or(0)
        } else if let Some(dict) = args.iter().find_map(|arg| {
            if let PyObjectPayload::Dict(map) = &arg.payload {
                Some(map)
            } else {
                None
            }
        }) {
            dict.read()
                .get(&HashableKey::str_key(CompactString::from("flags")))
                .and_then(|v| v.as_int())
                .unwrap_or(0)
        } else {
            0
        };
        let only_ast = (flags & 1024) != 0;

        // Check if the argument is an AST object (Instance), not a string or bytes-like source.
        let is_ast_obj = matches!(&args[0].payload, PyObjectPayload::Instance(_))
            && args[0].get_attr("__memoryview__").is_none();

        if is_ast_obj {
            match ferrython_stdlib::pyobj_ast_to_module(&args[0]) {
                Ok(module) => {
                    validate_ast_compile_mode(&module, &mode)?;
                    let code = ferrython_compiler::compile(&module, &filename)
                        .map_err(|e| compile_ast_error_to_value_exc(&filename, e))?;
                    return Ok(PyObject::wrap(PyObjectPayload::Code(std::rc::Rc::new(
                        code,
                    ))));
                }
                Err(_e) => {
                    if let Some(message) = _e.strip_prefix("TypeError: ") {
                        return Err(PyException::type_error(message));
                    }
                    if let Some(message) = _e.strip_prefix("ValueError: ") {
                        return Err(PyException::value_error(message));
                    }
                    return Err(PyException::type_error(_e));
                }
            }
        }

        // String or bytes source code
        let source = source_arg_to_string(
            &args[0],
            &filename,
            "compile() arg 1 must be a string, bytes, or AST object",
        )?;
        let source_ends_with_newline = source.ends_with(['\n', '\r']);
        if only_ast {
            return match mode.as_str() {
                "eval" => parse_with_compile_warnings(self, &source, &filename, true)
                    .map(|module| ferrython_stdlib::module_ast_to_pyobject(&module)),
                "single" => {
                    let module = parse_with_compile_warnings(self, &source, &filename, false)?;
                    validate_single_input(&module, &filename, source_ends_with_newline)?;
                    Ok(match module {
                        ferrython_ast::Module::Module { body, .. } => {
                            ferrython_stdlib::module_ast_to_pyobject(
                                &ferrython_ast::Module::Interactive { body },
                            )
                        }
                        ferrython_ast::Module::Interactive { body } => {
                            ferrython_stdlib::module_ast_to_pyobject(
                                &ferrython_ast::Module::Interactive { body },
                            )
                        }
                        ferrython_ast::Module::Expression { body } => {
                            ferrython_stdlib::module_ast_to_pyobject(
                                &ferrython_ast::Module::Expression { body },
                            )
                        }
                    })
                }
                _ => parse_with_compile_warnings(self, &source, &filename, false)
                    .map(|module| ferrython_stdlib::module_ast_to_pyobject(&module)),
            };
        }
        let effective_source = if mode == "eval" {
            format!("__eval_result__ = ({})", source)
        } else {
            source
        };
        let module = parse_with_compile_warnings(self, &effective_source, &filename, false)?;
        let module = if mode == "single" {
            validate_single_input(&module, &filename, source_ends_with_newline)?;
            match module {
                ferrython_ast::Module::Module { body, .. } => {
                    ferrython_ast::Module::Interactive { body }
                }
                other => other,
            }
        } else {
            module
        };
        let code = ferrython_compiler::compile(&module, &filename)
            .map_err(|e| compile_error_to_syntax_exc(&filename, e))?;
        Ok(PyObject::wrap(PyObjectPayload::Code(std::rc::Rc::new(
            code,
        ))))
    }
}

/// Convert a parser `ParseError` into a `SyntaxError` (or `IndentationError`)
/// `PyException` carrying `.filename`, `.lineno`, `.offset`, `.msg` attributes.
pub(crate) fn parse_error_to_syntax_exc(
    filename: &str,
    e: ferrython_parser::ParseError,
) -> PyException {
    let (kind, msg) = match &e.kind {
        ferrython_parser::ParseErrorKind::IndentationError(m) => {
            (ExceptionKind::IndentationError, m.to_string())
        }
        ferrython_parser::ParseErrorKind::TabError => (
            ExceptionKind::TabError,
            "inconsistent use of tabs and spaces in indentation".to_string(),
        ),
        _ => (ExceptionKind::SyntaxError, format!("{}", e.kind)),
    };
    let lineno = e.span.start_line as i64;
    let offset = (e.span.start_col as i64) + 1;
    build_syntax_exception(kind, &msg, filename, lineno, offset)
}

/// Convert a compiler `CompileError` into a `SyntaxError` `PyException`
/// carrying `.filename`, `.lineno`, `.offset`, `.msg` attributes.
pub(crate) fn compile_error_to_syntax_exc(
    filename: &str,
    e: ferrython_compiler::CompileError,
) -> PyException {
    use ferrython_compiler::CompileError;
    let (msg, loc) = match &e {
        CompileError::SyntaxError { message, location } => (message.clone(), Some(*location)),
        CompileError::Unsupported { feature, location } => {
            (format!("unsupported: {}", feature), Some(*location))
        }
        CompileError::InvalidAssignTarget { location } => {
            ("cannot assign to expression".to_string(), Some(*location))
        }
        CompileError::BreakOutsideLoop { location } => {
            ("'break' outside loop".to_string(), Some(*location))
        }
        CompileError::ContinueOutsideLoop { location } => (
            "'continue' not properly in loop".to_string(),
            Some(*location),
        ),
        CompileError::ReturnOutsideFunction { location } => {
            ("'return' outside function".to_string(), Some(*location))
        }
        CompileError::YieldOutsideFunction { location } => {
            ("'yield' outside function".to_string(), Some(*location))
        }
        CompileError::CannotDeleteCall { location } => {
            ("cannot delete function call".to_string(), Some(*location))
        }
        CompileError::CannotDeleteLiteral { location } => {
            ("cannot delete literal".to_string(), Some(*location))
        }
        CompileError::CannotDeleteExpression { location } => {
            ("cannot delete expression".to_string(), Some(*location))
        }
        CompileError::ParameterAndGlobal { name, location } => (
            format!("name '{}' is parameter and global", name),
            Some(*location),
        ),
        CompileError::ParameterAndNonlocal { name, location } => (
            format!("name '{}' is parameter and nonlocal", name),
            Some(*location),
        ),
        CompileError::NameError { message } => (message.clone(), None),
        CompileError::Internal(s) => (s.clone(), None),
        CompileError::InvalidAst { message } => {
            return build_syntax_exception(ExceptionKind::ValueError, message, filename, 1, 0);
        }
    };
    let (lineno, offset) = match loc {
        Some(l) => (l.line as i64, (l.column as i64) + 1),
        None => (1, 0),
    };
    build_syntax_exception(ExceptionKind::SyntaxError, &msg, filename, lineno, offset)
}

pub(crate) fn compile_ast_error_to_value_exc(
    filename: &str,
    e: ferrython_compiler::CompileError,
) -> PyException {
    use ferrython_compiler::CompileError;
    match &e {
        CompileError::InvalidAssignTarget { .. } => build_syntax_exception(
            ExceptionKind::ValueError,
            "expression which can't be assigned to in Store context",
            filename,
            1,
            0,
        ),
        CompileError::InvalidAst { message } => {
            build_syntax_exception(ExceptionKind::ValueError, message, filename, 1, 0)
        }
        _ => compile_error_to_syntax_exc(filename, e),
    }
}

fn build_syntax_exception(
    kind: ExceptionKind,
    msg: &str,
    filename: &str,
    lineno: i64,
    offset: i64,
) -> PyException {
    let instance = PyObject::exception_instance(kind, CompactString::from(msg));
    if let PyObjectPayload::ExceptionInstance(ref ei) = instance.payload {
        let mut w = ei.ensure_attrs().write();
        w.insert(
            CompactString::from("filename"),
            PyObject::str_val(CompactString::from(filename)),
        );
        w.insert(CompactString::from("lineno"), PyObject::int(lineno));
        w.insert(CompactString::from("offset"), PyObject::int(offset));
        w.insert(
            CompactString::from("msg"),
            PyObject::str_val(CompactString::from(msg)),
        );
        w.insert(CompactString::from("text"), PyObject::none());
    }
    PyException::with_original(kind, CompactString::from(msg), instance)
}
