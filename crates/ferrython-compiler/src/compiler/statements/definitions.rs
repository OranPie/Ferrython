//! Function, class, and annotation statement helpers.

use compact_str::CompactString;
use ferrython_ast::*;
use ferrython_bytecode::{CodeFlags, CodeObject, ConstantValue, Opcode};

use super::super::expressions::body_contains_yield;
use super::super::{CompileUnit, Compiler, Result};
use crate::symbol_table::Scope;

impl Compiler {
    fn ensure_name(list: &mut Vec<CompactString>, name: &str) {
        if !list.iter().any(|item| item.as_str() == name) {
            list.push(CompactString::from(name));
        }
    }

    fn ensure_enclosing_class_cell(&mut self, name: &str) -> bool {
        let Some(class_idx) = self
            .unit_stack
            .iter()
            .rposition(|unit| unit.class_name.is_some())
        else {
            return false;
        };
        Self::ensure_name(&mut self.unit_stack[class_idx].code.cellvars, name);
        for idx in class_idx + 1..self.unit_stack.len() {
            Self::ensure_name(&mut self.unit_stack[idx].code.freevars, name);
            self.unit_stack[idx].code.flags |= CodeFlags::NESTED;
        }
        true
    }

    fn class_body_reads_class(body: &[Statement]) -> bool {
        body.iter().any(Self::statement_reads_class)
    }

    fn statement_reads_class(stmt: &Statement) -> bool {
        match &stmt.node {
            StatementKind::FunctionDef {
                decorator_list,
                returns,
                args,
                ..
            } => {
                decorator_list.iter().any(Self::expression_reads_class)
                    || returns
                        .as_ref()
                        .map(|expr| Self::expression_reads_class(expr))
                        .unwrap_or(false)
                    || Self::arguments_read_class(args)
            }
            StatementKind::ClassDef {
                bases,
                keywords,
                decorator_list,
                ..
            } => {
                bases.iter().any(Self::expression_reads_class)
                    || keywords
                        .iter()
                        .any(|kw| Self::expression_reads_class(&kw.value))
                    || decorator_list.iter().any(Self::expression_reads_class)
            }
            StatementKind::Return { value } => value
                .as_ref()
                .map(|expr| Self::expression_reads_class(expr))
                .unwrap_or(false),
            StatementKind::Delete { targets } => targets.iter().any(Self::expression_reads_class),
            StatementKind::Assign { targets, value, .. } => {
                Self::expression_reads_class(value)
                    || targets.iter().any(Self::expression_reads_class)
            }
            StatementKind::AugAssign { target, value, .. } => {
                Self::expression_reads_class(target) || Self::expression_reads_class(value)
            }
            StatementKind::AnnAssign {
                target,
                annotation,
                value,
                ..
            } => {
                Self::expression_reads_class(target)
                    || Self::expression_reads_class(annotation)
                    || value
                        .as_ref()
                        .map(|expr| Self::expression_reads_class(expr))
                        .unwrap_or(false)
            }
            StatementKind::For {
                target,
                iter,
                body,
                orelse,
                ..
            } => {
                Self::expression_reads_class(target)
                    || Self::expression_reads_class(iter)
                    || body.iter().any(Self::statement_reads_class)
                    || orelse.iter().any(Self::statement_reads_class)
            }
            StatementKind::While { test, body, orelse } => {
                Self::expression_reads_class(test)
                    || body.iter().any(Self::statement_reads_class)
                    || orelse.iter().any(Self::statement_reads_class)
            }
            StatementKind::If { test, body, orelse } => {
                Self::expression_reads_class(test)
                    || body.iter().any(Self::statement_reads_class)
                    || orelse.iter().any(Self::statement_reads_class)
            }
            StatementKind::With { items, body, .. } => {
                items.iter().any(|item| {
                    Self::expression_reads_class(&item.context_expr)
                        || item
                            .optional_vars
                            .as_ref()
                            .map(|expr| Self::expression_reads_class(expr))
                            .unwrap_or(false)
                }) || body.iter().any(Self::statement_reads_class)
            }
            StatementKind::Raise { exc, cause } => {
                exc.as_ref()
                    .map(|expr| Self::expression_reads_class(expr))
                    .unwrap_or(false)
                    || cause
                        .as_ref()
                        .map(|expr| Self::expression_reads_class(expr))
                        .unwrap_or(false)
            }
            StatementKind::Try {
                body,
                handlers,
                orelse,
                finalbody,
            } => {
                body.iter().any(Self::statement_reads_class)
                    || handlers.iter().any(|handler| {
                        handler
                            .typ
                            .as_ref()
                            .map(|expr| Self::expression_reads_class(expr))
                            .unwrap_or(false)
                            || handler.body.iter().any(Self::statement_reads_class)
                    })
                    || orelse.iter().any(Self::statement_reads_class)
                    || finalbody.iter().any(Self::statement_reads_class)
            }
            StatementKind::Assert { test, msg } => {
                Self::expression_reads_class(test)
                    || msg
                        .as_ref()
                        .map(|expr| Self::expression_reads_class(expr))
                        .unwrap_or(false)
            }
            StatementKind::Expr { value } => Self::expression_reads_class(value),
            StatementKind::Match { subject, cases } => {
                Self::expression_reads_class(subject)
                    || cases.iter().any(|case| {
                        case.guard
                            .as_ref()
                            .map(Self::expression_reads_class)
                            .unwrap_or(false)
                            || case.body.iter().any(Self::statement_reads_class)
                    })
            }
            StatementKind::Import { .. }
            | StatementKind::ImportFrom { .. }
            | StatementKind::Global { .. }
            | StatementKind::Nonlocal { .. }
            | StatementKind::Pass
            | StatementKind::Break
            | StatementKind::Continue => false,
        }
    }

    fn arguments_read_class(args: &Arguments) -> bool {
        args.defaults.iter().any(Self::expression_reads_class)
            || args
                .kw_defaults
                .iter()
                .flatten()
                .any(Self::expression_reads_class)
            || args
                .posonlyargs
                .iter()
                .chain(args.args.iter())
                .chain(args.kwonlyargs.iter())
                .chain(args.vararg.iter())
                .chain(args.kwarg.iter())
                .any(|arg| {
                    arg.annotation
                        .as_ref()
                        .map(|expr| Self::expression_reads_class(expr))
                        .unwrap_or(false)
                })
    }

    fn expression_reads_class(expr: &Expression) -> bool {
        match &expr.node {
            ExpressionKind::Name { id, ctx } => {
                id.as_str() == "__class__" && matches!(ctx, ExprContext::Load)
            }
            ExpressionKind::BoolOp { values, .. } => {
                values.iter().any(Self::expression_reads_class)
            }
            ExpressionKind::NamedExpr { target, value } => {
                Self::expression_reads_class(target) || Self::expression_reads_class(value)
            }
            ExpressionKind::BinOp { left, right, .. } => {
                Self::expression_reads_class(left) || Self::expression_reads_class(right)
            }
            ExpressionKind::UnaryOp { operand, .. } => Self::expression_reads_class(operand),
            ExpressionKind::Lambda { args, .. } => Self::arguments_read_class(args),
            ExpressionKind::IfExp { test, body, orelse } => {
                Self::expression_reads_class(test)
                    || Self::expression_reads_class(body)
                    || Self::expression_reads_class(orelse)
            }
            ExpressionKind::Dict { keys, values } => {
                keys.iter().flatten().any(Self::expression_reads_class)
                    || values.iter().any(Self::expression_reads_class)
            }
            ExpressionKind::Set { elts }
            | ExpressionKind::List { elts, .. }
            | ExpressionKind::Tuple { elts, .. } => elts.iter().any(Self::expression_reads_class),
            ExpressionKind::ListComp { generators, .. }
            | ExpressionKind::SetComp { generators, .. }
            | ExpressionKind::GeneratorExp { generators, .. } => generators
                .first()
                .map(|gen| Self::expression_reads_class(&gen.iter))
                .unwrap_or(false),
            ExpressionKind::DictComp { generators, .. } => generators
                .first()
                .map(|gen| Self::expression_reads_class(&gen.iter))
                .unwrap_or(false),
            ExpressionKind::Await { value }
            | ExpressionKind::YieldFrom { value }
            | ExpressionKind::Starred { value, .. } => Self::expression_reads_class(value),
            ExpressionKind::Yield { value } => value
                .as_ref()
                .map(|expr| Self::expression_reads_class(expr))
                .unwrap_or(false),
            ExpressionKind::Compare {
                left, comparators, ..
            } => {
                Self::expression_reads_class(left)
                    || comparators.iter().any(Self::expression_reads_class)
            }
            ExpressionKind::Call {
                func,
                args,
                keywords,
            } => {
                Self::expression_reads_class(func)
                    || args.iter().any(Self::expression_reads_class)
                    || keywords
                        .iter()
                        .any(|kw| Self::expression_reads_class(&kw.value))
            }
            ExpressionKind::FormattedValue {
                value, format_spec, ..
            } => {
                Self::expression_reads_class(value)
                    || format_spec
                        .as_ref()
                        .map(|expr| Self::expression_reads_class(expr))
                        .unwrap_or(false)
            }
            ExpressionKind::JoinedStr { values } => values.iter().any(Self::expression_reads_class),
            ExpressionKind::Attribute { value, .. } => Self::expression_reads_class(value),
            ExpressionKind::Subscript { value, slice, .. } => {
                Self::expression_reads_class(value) || Self::expression_reads_class(slice)
            }
            ExpressionKind::Slice { lower, upper, step } => {
                lower
                    .as_ref()
                    .map(|expr| Self::expression_reads_class(expr))
                    .unwrap_or(false)
                    || upper
                        .as_ref()
                        .map(|expr| Self::expression_reads_class(expr))
                        .unwrap_or(false)
                    || step
                        .as_ref()
                        .map(|expr| Self::expression_reads_class(expr))
                        .unwrap_or(false)
            }
            ExpressionKind::Constant { .. } => false,
        }
    }

    // ── function definition ─────────────────────────────────────────

    pub(super) fn compile_function_def(
        &mut self,
        name: &str,
        args: &Arguments,
        body: &[Statement],
        decorator_list: &[Expression],
        returns: Option<&Expression>,
        is_async: bool,
        location: SourceLocation,
    ) -> Result<()> {
        // Compile decorators first (they are called in reverse order)
        for dec in decorator_list {
            self.compile_expression(dec)?;
        }

        // Compile default argument values in the enclosing scope
        let num_defaults = args.defaults.len();
        if num_defaults > 0 {
            for default in &args.defaults {
                self.compile_expression(default)?;
            }
            self.emit_arg(Opcode::BuildTuple, num_defaults as u32);
        }

        // Compile keyword-only defaults as a dict
        let kw_defaults: Vec<_> = args
            .kw_defaults
            .iter()
            .zip(args.kwonlyargs.iter())
            .filter(|(d, _)| d.is_some())
            .collect();
        let has_kw_defaults = !kw_defaults.is_empty();
        if has_kw_defaults {
            for (default, arg) in &kw_defaults {
                let key = CompactString::from(self.mangle_name(arg.arg.as_str()).as_ref());
                let key_idx = self.add_const(ConstantValue::Str(key));
                self.emit_arg(Opcode::LoadConst, key_idx);
                self.compile_expression(default.as_ref().unwrap())?;
            }
            self.emit_arg(Opcode::BuildMap, kw_defaults.len() as u32);
        }

        // Build child code object
        let child_scope = self.current_unit_mut().take_child_scope();
        let qualname_prefix = &self.current_unit().qualname_prefix;
        let qualname = if self.is_explicit_global(name) || qualname_prefix.is_empty() {
            name.to_string()
        } else if self.current_unit().is_function && !qualname_prefix.ends_with(".<locals>") {
            format!("{}.<locals>.{}", qualname_prefix, name)
        } else {
            format!("{}.{}", qualname_prefix, name)
        };

        self.push_function_unit(name, child_scope, &qualname)?;

        // Set the first line number from the def statement location
        self.current_unit_mut().code.first_line_number = location.line;
        let posonly_names: Vec<CompactString> = args
            .posonlyargs
            .iter()
            .map(|arg| CompactString::from(self.mangle_name(arg.arg.as_str()).as_ref()))
            .collect();
        let positional_names: Vec<CompactString> = args
            .args
            .iter()
            .map(|arg| CompactString::from(self.mangle_name(arg.arg.as_str()).as_ref()))
            .collect();
        let vararg_name = args
            .vararg
            .as_ref()
            .map(|arg| CompactString::from(self.mangle_name(arg.arg.as_str()).as_ref()));
        let kwonly_names: Vec<CompactString> = args
            .kwonlyargs
            .iter()
            .map(|arg| CompactString::from(self.mangle_name(arg.arg.as_str()).as_ref()))
            .collect();
        let kwarg_name = args
            .kwarg
            .as_ref()
            .map(|arg| CompactString::from(self.mangle_name(arg.arg.as_str()).as_ref()));

        // Set up argument info on the code object
        {
            let unit = self.current_unit_mut();
            unit.code.arg_count = (args.posonlyargs.len() + args.args.len()) as u32;
            unit.code.posonlyarg_count = args.posonlyargs.len() as u32;
            unit.code.kwonlyarg_count = args.kwonlyargs.len() as u32;

            // Add parameters as varnames
            for name in &posonly_names {
                let name_str = name.as_str();
                let varnames = &unit.code.varnames;
                if !varnames.iter().any(|v| v.as_str() == name_str) {
                    unit.code.varnames.push(name.clone());
                }
            }
            for name in &positional_names {
                let name_str = name.as_str();
                let varnames = &unit.code.varnames;
                if !varnames.iter().any(|v| v.as_str() == name_str) {
                    unit.code.varnames.push(name.clone());
                }
            }
            if let Some(ref name) = vararg_name {
                unit.code.flags |= CodeFlags::VARARGS;
                let name_str = name.as_str();
                let varnames = &unit.code.varnames;
                if !varnames.iter().any(|v| v.as_str() == name_str) {
                    unit.code.varnames.push(name.clone());
                }
            }
            for name in &kwonly_names {
                let name_str = name.as_str();
                let varnames = &unit.code.varnames;
                if !varnames.iter().any(|v| v.as_str() == name_str) {
                    unit.code.varnames.push(name.clone());
                }
            }
            if let Some(ref name) = kwarg_name {
                unit.code.flags |= CodeFlags::VARKEYWORDS;
                let name_str = name.as_str();
                let varnames = &unit.code.varnames;
                if !varnames.iter().any(|v| v.as_str() == name_str) {
                    unit.code.varnames.push(name.clone());
                }
            }

            if is_async {
                unit.code.flags |= CodeFlags::COROUTINE;
            }
        }

        // Compile the function body
        // Extract docstring: if first statement is a string literal, store as first constant
        if let Some(first) = body.first() {
            if let StatementKind::Expr { value } = &first.node {
                if let ExpressionKind::Constant {
                    value: Constant::Str(doc),
                } = &value.node
                {
                    // Ensure docstring is the first constant in the code object
                    let unit = self.current_unit_mut();
                    unit.code.docstring = Some(doc.clone());
                    let doc_const = ConstantValue::Str(doc.clone());
                    if unit.code.constants.is_empty() || unit.code.constants[0] != doc_const {
                        unit.code.constants.insert(0, doc_const);
                    }
                }
            }
        }
        self.compile_body(body)?;

        // Check if the function body contains yield — if so, mark as generator
        if body_contains_yield(body) {
            self.current_unit_mut().code.flags |= CodeFlags::GENERATOR;
        }

        // Ensure function returns None if no explicit return
        let none_idx = self.add_const(ConstantValue::None);
        self.emit_arg(Opcode::LoadConst, none_idx);
        self.emit_op(Opcode::ReturnValue);

        let func_code = self.pop_function_unit();

        // Build annotations dict from arg annotations and return type
        let all_args: Vec<&Arg> = args
            .posonlyargs
            .iter()
            .chain(args.args.iter())
            .chain(args.vararg.iter())
            .chain(args.kwonlyargs.iter())
            .chain(args.kwarg.iter())
            .collect();
        let mut ann_count: u32 = 0;
        for arg in &all_args {
            if let Some(ref annotation) = arg.annotation {
                let key = CompactString::from(self.mangle_name(arg.arg.as_str()).as_ref());
                let key_idx = self.add_const(ConstantValue::Str(key));
                self.emit_arg(Opcode::LoadConst, key_idx);
                if self.future_annotations {
                    let ann_str = Self::annotation_to_string(annotation);
                    let idx = self.add_const(ConstantValue::Str(CompactString::from(ann_str)));
                    self.emit_arg(Opcode::LoadConst, idx);
                } else {
                    self.compile_expression(annotation)?;
                }
                ann_count += 1;
            }
        }
        if let Some(ret) = returns {
            let key_idx = self.add_const(ConstantValue::Str("return".into()));
            self.emit_arg(Opcode::LoadConst, key_idx);
            if self.future_annotations {
                let ann_str = Self::annotation_to_string(ret);
                let idx = self.add_const(ConstantValue::Str(CompactString::from(ann_str)));
                self.emit_arg(Opcode::LoadConst, idx);
            } else {
                self.compile_expression(ret)?;
            }
            ann_count += 1;
        }
        let has_annotations = ann_count > 0;
        if has_annotations {
            self.emit_arg(Opcode::BuildMap, ann_count);
        }

        // If the function has free variables, emit closure
        let has_closure = !func_code.freevars.is_empty();
        if has_closure {
            for freevar in &func_code.freevars {
                let idx = self.deref_index(freevar.as_str());
                self.emit_arg(Opcode::LoadClosure, idx);
            }
            let n = func_code.freevars.len() as u32;
            self.emit_arg(Opcode::BuildTuple, n);
        }

        // Load the code object as a constant
        let code_idx = self.add_const(ConstantValue::Code(std::rc::Rc::new(func_code)));
        self.emit_arg(Opcode::LoadConst, code_idx);

        // Load the qualified name
        let qname_idx = self.add_const(ConstantValue::Str(qualname.into()));
        self.emit_arg(Opcode::LoadConst, qname_idx);

        // Determine MAKE_FUNCTION flags
        let mut make_fn_flags: u32 = 0;
        if num_defaults > 0 {
            make_fn_flags |= 0x01;
        }
        if has_kw_defaults {
            make_fn_flags |= 0x02;
        }
        if has_annotations {
            make_fn_flags |= 0x04;
        }
        if has_closure {
            make_fn_flags |= 0x08;
        }
        self.emit_arg(Opcode::MakeFunction, make_fn_flags);

        // Apply decorators in reverse order
        for _ in decorator_list {
            self.emit_arg(Opcode::CallFunction, 1);
        }

        // Store the function name
        self.store_name(name);

        Ok(())
    }

    pub(in crate::compiler) fn push_function_unit(
        &mut self,
        name: &str,
        scope: Scope,
        qualname: &str,
    ) -> Result<()> {
        let mut unit = CompileUnit::new(name, &self.filename, scope, true, qualname.to_string());
        unit.code.qualname = CompactString::from(qualname);
        self.unit_stack.push(unit);
        Ok(())
    }

    pub(in crate::compiler) fn pop_function_unit(&mut self) -> CodeObject {
        let unit = self.unit_stack.pop().unwrap();
        let mut code = unit.code;
        code.num_locals = code.varnames.len() as u32;
        code
    }

    // ── class definition ────────────────────────────────────────────

    pub(super) fn compile_class_def(
        &mut self,
        name: &str,
        bases: &[Expression],
        keywords: &[Keyword],
        body: &[Statement],
        decorator_list: &[Expression],
        location: SourceLocation,
    ) -> Result<()> {
        // Compile decorators
        for dec in decorator_list {
            self.compile_expression(dec)?;
        }

        // LOAD_BUILD_CLASS
        self.emit_op(Opcode::LoadBuildClass);

        // Compile class body into a sub-CodeObject
        let child_scope = self.current_unit_mut().take_child_scope();
        let qualname_prefix = &self.current_unit().qualname_prefix;
        let qualname = if qualname_prefix.is_empty() {
            name.to_string()
        } else if self.current_unit().is_function && !qualname_prefix.ends_with(".<locals>") {
            format!("{}.<locals>.{}", qualname_prefix, name)
        } else {
            format!("{}.{}", qualname_prefix, name)
        };

        let mut class_unit =
            CompileUnit::new(name, &self.filename, child_scope, false, qualname.clone());
        class_unit.code.flags = CodeFlags::empty();
        if Self::class_body_reads_class(body) && self.ensure_enclosing_class_cell("__class__") {
            Self::ensure_name(&mut class_unit.code.freevars, "__class__");
            class_unit.code.flags |= CodeFlags::NESTED;
        }
        // The class body function takes __locals__ as implicit first arg
        class_unit.code.arg_count = 0;
        class_unit.class_name = Some(name.to_string());
        class_unit.code.first_line_number = location.line;
        self.unit_stack.push(class_unit);

        // __name__ = qualname
        let qname_idx = self.add_const(ConstantValue::Str(qualname.clone().into()));
        self.emit_arg(Opcode::LoadConst, qname_idx);
        self.store_name("__qualname__");

        if Self::has_annotations(body) {
            self.emit_op(Opcode::SetupAnnotations);
        }

        // Extract docstring from first statement if it's a string literal
        if let Some(first) = body.first() {
            if let StatementKind::Expr { value } = &first.node {
                if let ExpressionKind::Constant {
                    value: Constant::Str(doc),
                } = &value.node
                {
                    let doc_idx = self.add_const(ConstantValue::Str(doc.clone()));
                    self.emit_arg(Opcode::LoadConst, doc_idx);
                    self.store_name("__doc__");
                }
            }
        }

        // Compile the class body
        self.compile_body(body)?;

        // Return None from the class body
        let none_idx = self.add_const(ConstantValue::None);
        self.emit_arg(Opcode::LoadConst, none_idx);
        self.emit_op(Opcode::ReturnValue);

        let class_code = self.pop_function_unit();

        // Check if the class body needs closure cells
        let has_freevars = !class_code.freevars.is_empty();
        let num_freevars = class_code.freevars.len();

        // Emit closure cells BEFORE loading code/qualname (push order matters)
        if has_freevars {
            // For each freevar in the class code, emit LoadClosure from the parent scope
            for freevar_name in &class_code.freevars.clone() {
                let parent_has_cell = self
                    .current_unit()
                    .code
                    .cellvars
                    .iter()
                    .any(|v| v == freevar_name);
                let parent_has_free = self
                    .current_unit()
                    .code
                    .freevars
                    .iter()
                    .any(|v| v == freevar_name);
                if self.current_unit().is_function {
                    if !parent_has_cell && !parent_has_free {
                        Self::ensure_name(&mut self.current_unit_mut().code.cellvars, freevar_name);
                    }
                } else if !parent_has_cell && !parent_has_free {
                    Self::ensure_name(&mut self.current_unit_mut().code.freevars, freevar_name);
                    self.current_unit_mut().code.flags |= CodeFlags::NESTED;
                }
                // Find the cell index in the current (parent) scope
                let unit = self.current_unit();
                let cell_idx = unit
                    .code
                    .cellvars
                    .iter()
                    .position(|v| v == freevar_name)
                    .or_else(|| {
                        unit.code
                            .freevars
                            .iter()
                            .position(|v| v == freevar_name)
                            .map(|i| i + unit.code.cellvars.len())
                    });
                if let Some(idx) = cell_idx {
                    self.emit_arg(Opcode::LoadClosure, idx as u32);
                }
            }
            self.emit_arg(Opcode::BuildTuple, num_freevars as u32);
        }

        // Load the class body code object
        let code_idx = self.add_const(ConstantValue::Code(std::rc::Rc::new(class_code)));
        self.emit_arg(Opcode::LoadConst, code_idx);

        // Load qualname for MAKE_FUNCTION
        let qname_const = self.add_const(ConstantValue::Str(qualname.into()));
        self.emit_arg(Opcode::LoadConst, qname_const);

        // MAKE_FUNCTION with closure flag if needed
        let make_fn_flags = if has_freevars { 0x08 } else { 0 };
        self.emit_arg(Opcode::MakeFunction, make_fn_flags);

        // Load class name as string arg
        let name_idx = self.add_const(ConstantValue::Str(name.into()));
        self.emit_arg(Opcode::LoadConst, name_idx);

        // Compile base classes
        for base in bases {
            self.compile_expression(base)?;
        }

        // Compile keyword args (e.g., metaclass=...)
        let num_kw = keywords.iter().filter(|k| k.arg.is_some()).count();
        for kw in keywords {
            if let Some(ref arg_name) = kw.arg {
                self.compile_expression(&kw.value)?;
                let _ = arg_name; // keyword arg names passed via CALL_FUNCTION_KW
            }
        }

        let total_args = 2 + bases.len() as u32; // func + name + bases
        if num_kw > 0 {
            // Build a tuple of keyword argument names
            let kw_names: Vec<ConstantValue> = keywords
                .iter()
                .filter_map(|k| k.arg.as_ref().map(|a| ConstantValue::Str(a.clone())))
                .collect();
            let kw_tuple_idx = self.add_const(ConstantValue::Tuple(kw_names));
            self.emit_arg(Opcode::LoadConst, kw_tuple_idx);
            self.emit_arg(Opcode::CallFunctionKw, total_args + num_kw as u32);
        } else {
            self.emit_arg(Opcode::CallFunction, total_args);
        }

        // Apply decorators in reverse order
        for _ in decorator_list {
            self.emit_arg(Opcode::CallFunction, 1);
        }

        // Store the class
        self.store_name(name);

        Ok(())
    }

    fn constant_to_annotation(value: &Constant) -> String {
        match value {
            Constant::Str(s) => format!("'{}'", s),
            Constant::Int(n) => match n {
                BigInt::Small(v) => v.to_string(),
                BigInt::Big(v) => v.to_string(),
            },
            Constant::Float(f) => f.to_string(),
            Constant::Complex { real, imag } => format!("{}+{}j", real, imag),
            Constant::None => "None".to_string(),
            Constant::Bool(true) => "True".to_string(),
            Constant::Bool(false) => "False".to_string(),
            Constant::Ellipsis => "...".to_string(),
            Constant::Bytes(_) => "b'...'".to_string(),
            Constant::Tuple(items) => {
                let parts: Vec<String> = items.iter().map(Self::constant_to_annotation).collect();
                if parts.len() == 1 {
                    format!("({},)", parts[0])
                } else {
                    format!("({})", parts.join(", "))
                }
            }
            Constant::FrozenSet(_) => "frozenset(...)".to_string(),
        }
    }

    /// Convert an annotation expression AST to its source-code string representation.
    /// Used by PEP 563 (`from __future__ import annotations`) to store annotations as strings.
    pub(super) fn annotation_to_string(expr: &Expression) -> String {
        match &expr.node {
            ExpressionKind::Name { id, .. } => id.to_string(),
            ExpressionKind::Attribute { value, attr, .. } => {
                format!("{}.{}", Self::annotation_to_string(value), attr)
            }
            ExpressionKind::Subscript { value, slice, .. } => {
                format!(
                    "{}[{}]",
                    Self::annotation_to_string(value),
                    Self::annotation_to_string(slice)
                )
            }
            ExpressionKind::Tuple { elts, .. } => {
                let parts: Vec<String> = elts.iter().map(Self::annotation_to_string).collect();
                parts.join(", ")
            }
            ExpressionKind::List { elts, .. } => {
                let parts: Vec<String> = elts.iter().map(Self::annotation_to_string).collect();
                format!("[{}]", parts.join(", "))
            }
            ExpressionKind::Constant { value } => Self::constant_to_annotation(value),
            ExpressionKind::BinOp { left, op, right } => {
                let op_str = match op {
                    Operator::BitOr => "|",
                    Operator::Add => "+",
                    Operator::Sub => "-",
                    Operator::Mult => "*",
                    Operator::Div => "/",
                    _ => "|",
                };
                format!(
                    "{} {} {}",
                    Self::annotation_to_string(left),
                    op_str,
                    Self::annotation_to_string(right)
                )
            }
            ExpressionKind::Call {
                func,
                args,
                keywords,
            } => {
                let mut parts: Vec<String> = args.iter().map(Self::annotation_to_string).collect();
                for kw in keywords {
                    if let Some(ref key) = kw.arg {
                        parts.push(format!("{}={}", key, Self::annotation_to_string(&kw.value)));
                    } else {
                        parts.push(format!("**{}", Self::annotation_to_string(&kw.value)));
                    }
                }
                format!("{}({})", Self::annotation_to_string(func), parts.join(", "))
            }
            ExpressionKind::UnaryOp { op, operand } => {
                let op_str = match op {
                    UnaryOperator::Not => "not ",
                    UnaryOperator::USub => "-",
                    UnaryOperator::UAdd => "+",
                    UnaryOperator::Invert => "~",
                };
                format!("{}{}", op_str, Self::annotation_to_string(operand))
            }
            ExpressionKind::BoolOp { op, values } => {
                let op_str = match op {
                    BoolOperator::And => " and ",
                    BoolOperator::Or => " or ",
                };
                let parts: Vec<String> = values.iter().map(Self::annotation_to_string).collect();
                parts.join(op_str)
            }
            ExpressionKind::IfExp { test, body, orelse } => {
                format!(
                    "{} if {} else {}",
                    Self::annotation_to_string(body),
                    Self::annotation_to_string(test),
                    Self::annotation_to_string(orelse)
                )
            }
            ExpressionKind::Starred { value, .. } => {
                format!("*{}", Self::annotation_to_string(value))
            }
            _ => "...".to_string(), // Fallback for unsupported expression types
        }
    }
}
