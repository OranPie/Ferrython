//! Lambda and comprehension expression compilation helpers.

use compact_str::CompactString;
use ferrython_ast::*;
use ferrython_bytecode::{CodeFlags, ConstantValue, Opcode};

use super::super::{Compiler, Result};
impl Compiler {
    // ── lambda ──────────────────────────────────────────────────────

    pub(super) fn compile_lambda(
        &mut self,
        args: &Arguments,
        body: &Expression,
        location: SourceLocation,
    ) -> Result<()> {
        // Compile defaults in enclosing scope
        let num_defaults = args.defaults.len();
        if num_defaults > 0 {
            for default in &args.defaults {
                self.compile_expression(default)?;
            }
            self.emit_arg(Opcode::BuildTuple, num_defaults as u32);
        }

        let kw_defaults: Vec<_> = args
            .kw_defaults
            .iter()
            .zip(args.kwonlyargs.iter())
            .filter(|(d, _)| d.is_some())
            .collect();
        let has_kw_defaults = !kw_defaults.is_empty();
        if has_kw_defaults {
            for (default, arg) in &kw_defaults {
                let key_idx = self.add_const(ConstantValue::Str(arg.arg.clone()));
                self.emit_arg(Opcode::LoadConst, key_idx);
                self.compile_expression(default.as_ref().unwrap())?;
            }
            self.emit_arg(Opcode::BuildMap, kw_defaults.len() as u32);
        }

        let child_scope = self.current_unit_mut().take_child_scope();
        let qualname_prefix = &self.current_unit().qualname_prefix;
        let qualname = if qualname_prefix.is_empty() {
            "<lambda>".to_string()
        } else {
            format!("{}.<lambda>", qualname_prefix)
        };

        self.push_function_unit("<lambda>", child_scope, &qualname)?;

        // Set the first line number from the lambda location
        self.current_unit_mut().code.first_line_number = location.line;

        // Set up argument info
        {
            let unit = self.current_unit_mut();
            unit.code.arg_count = (args.posonlyargs.len() + args.args.len()) as u32;
            unit.code.posonlyarg_count = args.posonlyargs.len() as u32;
            unit.code.kwonlyarg_count = args.kwonlyargs.len() as u32;

            for arg in &args.posonlyargs {
                if !unit
                    .code
                    .varnames
                    .iter()
                    .any(|v| v.as_str() == arg.arg.as_str())
                {
                    unit.code.varnames.push(arg.arg.clone());
                }
            }
            for arg in &args.args {
                if !unit
                    .code
                    .varnames
                    .iter()
                    .any(|v| v.as_str() == arg.arg.as_str())
                {
                    unit.code.varnames.push(arg.arg.clone());
                }
            }
            if let Some(ref vararg) = args.vararg {
                unit.code.flags |= CodeFlags::VARARGS;
                if !unit
                    .code
                    .varnames
                    .iter()
                    .any(|v| v.as_str() == vararg.arg.as_str())
                {
                    unit.code.varnames.push(vararg.arg.clone());
                }
            }
            for arg in &args.kwonlyargs {
                if !unit
                    .code
                    .varnames
                    .iter()
                    .any(|v| v.as_str() == arg.arg.as_str())
                {
                    unit.code.varnames.push(arg.arg.clone());
                }
            }
            if let Some(ref kwarg) = args.kwarg {
                unit.code.flags |= CodeFlags::VARKEYWORDS;
                if !unit
                    .code
                    .varnames
                    .iter()
                    .any(|v| v.as_str() == kwarg.arg.as_str())
                {
                    unit.code.varnames.push(kwarg.arg.clone());
                }
            }
        }

        // Compile the lambda body and return it
        self.compile_expression(body)?;
        self.emit_op(Opcode::ReturnValue);

        let func_code = self.pop_function_unit();

        // If the lambda has free variables, emit closure
        let has_closure = !func_code.freevars.is_empty();
        if has_closure {
            for freevar in &func_code.freevars {
                let idx = self.deref_index(freevar.as_str());
                self.emit_arg(Opcode::LoadClosure, idx);
            }
            let n = func_code.freevars.len() as u32;
            self.emit_arg(Opcode::BuildTuple, n);
        }

        let code_idx = self.add_const(ConstantValue::Code(std::rc::Rc::new(func_code)));
        self.emit_arg(Opcode::LoadConst, code_idx);

        let qname_idx = self.add_const(ConstantValue::Str(qualname.into()));
        self.emit_arg(Opcode::LoadConst, qname_idx);

        let mut make_fn_flags: u32 = 0;
        if num_defaults > 0 {
            make_fn_flags |= 0x01;
        }
        if has_kw_defaults {
            make_fn_flags |= 0x02;
        }
        if has_closure {
            make_fn_flags |= 0x08;
        }
        self.emit_arg(Opcode::MakeFunction, make_fn_flags);

        Ok(())
    }

    // ── comprehensions ──────────────────────────────────────────────

    pub(super) fn compile_comprehension(
        &mut self,
        name: &str,
        elt: &Expression,
        value: Option<&Expression>,
        generators: &[Comprehension],
        kind: ComprehensionKind,
    ) -> Result<()> {
        // CPython semantics: the first generator's iterable is evaluated in
        // the enclosing scope BEFORE the comprehension scope is entered.
        // We must compile it first to match symbol table child ordering.
        self.compile_expression(&generators[0].iter)?;
        self.emit_op(Opcode::GetIter);
        // Stack: [iter]

        let child_scope = self.current_unit_mut().take_child_scope();
        let qualname_prefix = &self.current_unit().qualname_prefix;
        let qualname = if qualname_prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}.{}", qualname_prefix, name)
        };

        // Compile the comprehension function body
        self.push_function_unit(name, child_scope, &qualname)?;

        {
            let unit = self.current_unit_mut();
            unit.code.arg_count = 1;
            unit.code.varnames.push(".0".into());
            if matches!(kind, ComprehensionKind::Generator) {
                unit.code.flags |= CodeFlags::GENERATOR;
            }
        }

        match kind {
            ComprehensionKind::List => {
                self.emit_arg(Opcode::BuildList, 0);
            }
            ComprehensionKind::Set => {
                self.emit_arg(Opcode::BuildSet, 0);
            }
            ComprehensionKind::Dict => {
                self.emit_arg(Opcode::BuildMap, 0);
            }
            ComprehensionKind::Generator => {}
        }

        self.compile_comprehension_generators(generators, 0, elt, value, kind)?;

        match kind {
            ComprehensionKind::Generator => {
                let none_idx = self.add_const(ConstantValue::None);
                self.emit_arg(Opcode::LoadConst, none_idx);
            }
            _ => {}
        }
        self.emit_op(Opcode::ReturnValue);

        let comp_code = self.pop_function_unit();

        // If the comprehension has free variables, emit closure
        let has_closure = !comp_code.freevars.is_empty();
        if has_closure {
            for freevar in &comp_code.freevars {
                let idx = self.deref_index(freevar.as_str());
                self.emit_arg(Opcode::LoadClosure, idx);
            }
            let n = comp_code.freevars.len() as u32;
            self.emit_arg(Opcode::BuildTuple, n);
        }

        // Back in the enclosing scope — emit function creation
        let code_idx = self.add_const(ConstantValue::Code(std::rc::Rc::new(comp_code)));
        self.emit_arg(Opcode::LoadConst, code_idx);

        let qname_idx = self.add_const(ConstantValue::Str(qualname.into()));
        self.emit_arg(Opcode::LoadConst, qname_idx);

        self.emit_arg(Opcode::MakeFunction, if has_closure { 0x08 } else { 0 });

        // Stack: [iter, fn] — need [fn, iter] for CallFunction
        self.emit_op(Opcode::RotTwo);

        // Call fn(iter)
        self.emit_arg(Opcode::CallFunction, 1);

        Ok(())
    }

    pub(super) fn compile_comprehension_generators(
        &mut self,
        generators: &[Comprehension],
        idx: usize,
        elt: &Expression,
        value: Option<&Expression>,
        kind: ComprehensionKind,
    ) -> Result<()> {
        let gen = &generators[idx];

        // Load the iterator (first one comes from argument, rest are created here)
        if idx == 0 {
            let iter_idx = self.varname_index(".0");
            self.emit_arg(Opcode::LoadFast, iter_idx);
        } else {
            self.compile_expression(&gen.iter)?;
            self.emit_op(Opcode::GetIter);
        }

        let loop_start = self.current_offset();
        let done_label = self.emit_jump(Opcode::ForIter);

        // Store the iteration variable
        self.compile_store_target(&gen.target)?;

        // Compile filter conditions
        let mut skip_labels = Vec::new();
        for cond in &gen.ifs {
            self.compile_expression(cond)?;
            skip_labels.push(self.emit_jump(Opcode::PopJumpIfFalse));
        }

        if idx + 1 < generators.len() {
            self.compile_comprehension_generators(generators, idx + 1, elt, value, kind)?;
        } else {
            match kind {
                ComprehensionKind::List => {
                    self.compile_expression(elt)?;
                    self.emit_arg(Opcode::ListAppend, (generators.len() + 1) as u32);
                }
                ComprehensionKind::Set => {
                    self.compile_expression(elt)?;
                    self.emit_arg(Opcode::SetAdd, (generators.len() + 1) as u32);
                }
                ComprehensionKind::Dict => {
                    self.compile_expression(elt)?;
                    self.compile_expression(value.unwrap())?;
                    self.emit_arg(Opcode::MapAdd, (generators.len() + 1) as u32);
                }
                ComprehensionKind::Generator => {
                    self.compile_expression(elt)?;
                    self.emit_op(Opcode::YieldValue);
                    self.emit_op(Opcode::PopTop);
                }
            }
        }

        let cont_target = self.current_offset();
        for label in skip_labels {
            self.patch_jump(label, cont_target);
        }

        self.emit_arg(Opcode::JumpAbsolute, loop_start);
        self.patch_jump_here(done_label);

        Ok(())
    }

    /// Compile an async comprehension inline (without a separate coroutine function).
    /// Uses a temp variable for the result container to avoid stack corruption
    /// from SetupExcept/EndAsyncFor.
    pub(super) fn compile_async_comprehension_inline(
        &mut self,
        elt: &Expression,
        value: Option<&Expression>,
        generators: &[Comprehension],
        kind: ComprehensionKind,
    ) -> Result<()> {
        // Build the empty result container and store in temp var
        let result_temp = CompactString::from("$async_comp_result$");
        let result_idx = self.varname_index(&result_temp);
        match kind {
            ComprehensionKind::List => self.emit_arg(Opcode::BuildList, 0),
            ComprehensionKind::Set => self.emit_arg(Opcode::BuildSet, 0),
            ComprehensionKind::Dict => self.emit_arg(Opcode::BuildMap, 0),
            ComprehensionKind::Generator => 0,
        };
        self.emit_arg(Opcode::StoreFast, result_idx);

        self.compile_async_comp_generator(generators, 0, elt, value, kind, result_idx)?;

        // Load result back onto stack
        self.emit_arg(Opcode::LoadFast, result_idx);
        Ok(())
    }

    fn compile_async_comp_generator(
        &mut self,
        generators: &[Comprehension],
        idx: usize,
        elt: &Expression,
        value: Option<&Expression>,
        kind: ComprehensionKind,
        result_idx: u32,
    ) -> Result<()> {
        let gen = &generators[idx];

        self.compile_expression(&gen.iter)?;
        if gen.is_async {
            self.emit_op(Opcode::GetAiter);
        } else {
            self.emit_op(Opcode::GetIter);
        }

        let loop_start = self.current_offset();

        if gen.is_async {
            let except_label = self.emit_jump(Opcode::SetupExcept);
            self.emit_op(Opcode::GetAnext);
            self.emit_op(Opcode::GetAwaitable);
            let none_idx = self.add_const(ConstantValue::None);
            self.emit_arg(Opcode::LoadConst, none_idx);
            self.emit_op(Opcode::YieldFrom);
            self.compile_store_target(&gen.target)?;
            self.emit_op(Opcode::PopBlock);

            let mut skip_labels = Vec::new();
            for cond in &gen.ifs {
                self.compile_expression(cond)?;
                skip_labels.push(self.emit_jump(Opcode::PopJumpIfFalse));
            }

            if idx + 1 < generators.len() {
                self.compile_async_comp_generator(
                    generators,
                    idx + 1,
                    elt,
                    value,
                    kind,
                    result_idx,
                )?;
            } else {
                // Innermost: load result, append element, store back
                match kind {
                    ComprehensionKind::List => {
                        self.emit_arg(Opcode::LoadFast, result_idx);
                        let append_name = self.add_name("append");
                        self.emit_arg(Opcode::LoadAttr, append_name);
                        self.compile_expression(elt)?;
                        self.emit_arg(Opcode::CallFunction, 1);
                        self.emit_op(Opcode::PopTop);
                    }
                    ComprehensionKind::Set => {
                        self.emit_arg(Opcode::LoadFast, result_idx);
                        let add_name = self.add_name("add");
                        self.emit_arg(Opcode::LoadAttr, add_name);
                        self.compile_expression(elt)?;
                        self.emit_arg(Opcode::CallFunction, 1);
                        self.emit_op(Opcode::PopTop);
                    }
                    ComprehensionKind::Dict => {
                        // StoreSubscr stack: TOS=key, TOS1=obj, TOS2=value
                        self.compile_expression(value.unwrap())?; // value
                        self.emit_arg(Opcode::LoadFast, result_idx); // dict
                        self.compile_expression(elt)?; // key
                        self.emit_op(Opcode::StoreSubscr);
                    }
                    ComprehensionKind::Generator => {
                        self.compile_expression(elt)?;
                        self.emit_op(Opcode::YieldValue);
                        self.emit_op(Opcode::PopTop);
                    }
                }
            }

            let cont_target = self.current_offset();
            for label in skip_labels {
                self.patch_jump(label, cont_target);
            }
            self.emit_arg(Opcode::JumpAbsolute, loop_start);
            self.patch_jump_here(except_label);
            self.emit_op(Opcode::EndAsyncFor);
        } else {
            let done_label = self.emit_jump(Opcode::ForIter);
            self.compile_store_target(&gen.target)?;

            let mut skip_labels = Vec::new();
            for cond in &gen.ifs {
                self.compile_expression(cond)?;
                skip_labels.push(self.emit_jump(Opcode::PopJumpIfFalse));
            }

            if idx + 1 < generators.len() {
                self.compile_async_comp_generator(
                    generators,
                    idx + 1,
                    elt,
                    value,
                    kind,
                    result_idx,
                )?;
            } else {
                match kind {
                    ComprehensionKind::List => {
                        self.emit_arg(Opcode::LoadFast, result_idx);
                        let append_name = self.add_name("append");
                        self.emit_arg(Opcode::LoadAttr, append_name);
                        self.compile_expression(elt)?;
                        self.emit_arg(Opcode::CallFunction, 1);
                        self.emit_op(Opcode::PopTop);
                    }
                    ComprehensionKind::Set => {
                        self.emit_arg(Opcode::LoadFast, result_idx);
                        let add_name = self.add_name("add");
                        self.emit_arg(Opcode::LoadAttr, add_name);
                        self.compile_expression(elt)?;
                        self.emit_arg(Opcode::CallFunction, 1);
                        self.emit_op(Opcode::PopTop);
                    }
                    ComprehensionKind::Dict => {
                        self.compile_expression(value.unwrap())?; // value
                        self.emit_arg(Opcode::LoadFast, result_idx); // dict
                        self.compile_expression(elt)?; // key
                        self.emit_op(Opcode::StoreSubscr);
                    }
                    ComprehensionKind::Generator => {
                        self.compile_expression(elt)?;
                        self.emit_op(Opcode::YieldValue);
                        self.emit_op(Opcode::PopTop);
                    }
                }
            }

            let cont_target = self.current_offset();
            for label in skip_labels {
                self.patch_jump(label, cont_target);
            }
            self.emit_arg(Opcode::JumpAbsolute, loop_start);
            self.patch_jump_here(done_label);
        }

        Ok(())
    }
}

/// What kind of comprehension we're compiling.
#[derive(Debug, Clone, Copy)]
pub(in crate::compiler) enum ComprehensionKind {
    List,
    Set,
    Dict,
    Generator,
}

/// Map a comparison operator to the CPython compare op argument.

pub(in crate::compiler) fn compare_op_arg(op: CompareOperator) -> u32 {
    match op {
        CompareOperator::Lt => 0,
        CompareOperator::LtE => 1,
        CompareOperator::Eq => 2,
        CompareOperator::NotEq => 3,
        CompareOperator::Gt => 4,
        CompareOperator::GtE => 5,
        CompareOperator::In => 6,
        CompareOperator::NotIn => 7,
        CompareOperator::Is => 8,
        CompareOperator::IsNot => 9,
    }
}

/// Check if a function body (list of statements) contains any `yield` or `yield from` expressions.
/// Only checks the direct body — does NOT recurse into nested function/class defs.
pub(in crate::compiler) fn body_contains_yield(stmts: &[Statement]) -> bool {
    for stmt in stmts {
        if stmt_contains_yield(stmt) {
            return true;
        }
    }
    false
}

pub(in crate::compiler::expressions) fn stmt_contains_yield(stmt: &Statement) -> bool {
    match &stmt.node {
        StatementKind::Expr { value } => expr_contains_yield(value),
        StatementKind::Return { value } => value.as_ref().map_or(false, |v| expr_contains_yield(v)),
        StatementKind::Assign { value, .. } => expr_contains_yield(value),
        StatementKind::AnnAssign { value, .. } => {
            value.as_ref().map_or(false, |v| expr_contains_yield(v))
        }
        StatementKind::AugAssign { value, .. } => expr_contains_yield(value),
        StatementKind::If { test, body, orelse } => {
            expr_contains_yield(test) || body_contains_yield(body) || body_contains_yield(orelse)
        }
        StatementKind::While { test, body, orelse } => {
            expr_contains_yield(test) || body_contains_yield(body) || body_contains_yield(orelse)
        }
        StatementKind::For {
            body, orelse, iter, ..
        } => expr_contains_yield(iter) || body_contains_yield(body) || body_contains_yield(orelse),
        StatementKind::Try {
            body,
            handlers,
            orelse,
            finalbody,
        } => {
            body_contains_yield(body)
                || body_contains_yield(orelse)
                || body_contains_yield(finalbody)
                || handlers.iter().any(|h| body_contains_yield(&h.body))
        }
        StatementKind::With { body, .. } => body_contains_yield(body),
        // Do NOT recurse into nested function/class definitions
        StatementKind::FunctionDef { .. } | StatementKind::ClassDef { .. } => false,
        _ => false,
    }
}

pub(in crate::compiler::expressions) fn expr_contains_yield(expr: &Expression) -> bool {
    match &expr.node {
        ExpressionKind::Yield { .. } | ExpressionKind::YieldFrom { .. } => true,
        ExpressionKind::BinOp { left, right, .. } => {
            expr_contains_yield(left) || expr_contains_yield(right)
        }
        ExpressionKind::UnaryOp { operand, .. } => expr_contains_yield(operand),
        ExpressionKind::BoolOp { values, .. } => values.iter().any(|v| expr_contains_yield(v)),
        ExpressionKind::Call {
            func,
            args,
            keywords,
        } => {
            expr_contains_yield(func)
                || args.iter().any(|a| expr_contains_yield(a))
                || keywords.iter().any(|k| expr_contains_yield(&k.value))
        }
        ExpressionKind::IfExp { test, body, orelse } => {
            expr_contains_yield(test) || expr_contains_yield(body) || expr_contains_yield(orelse)
        }
        ExpressionKind::Tuple { elts, .. }
        | ExpressionKind::List { elts, .. }
        | ExpressionKind::Set { elts } => elts.iter().any(|e| expr_contains_yield(e)),
        ExpressionKind::Dict { keys, values } => {
            keys.iter()
                .any(|k| k.as_ref().map_or(false, |k| expr_contains_yield(k)))
                || values.iter().any(|v| expr_contains_yield(v))
        }
        ExpressionKind::Attribute { value, .. } => expr_contains_yield(value),
        ExpressionKind::Subscript { value, slice, .. } => {
            expr_contains_yield(value) || expr_contains_yield(slice)
        }
        ExpressionKind::Compare {
            left, comparators, ..
        } => expr_contains_yield(left) || comparators.iter().any(|c| expr_contains_yield(c)),
        _ => false,
    }
}
