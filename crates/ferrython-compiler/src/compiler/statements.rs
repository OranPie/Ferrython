//! Statement compilation methods for the Compiler.

use compact_str::CompactString;
use ferrython_ast::*;
use ferrython_bytecode::{ConstantValue, Opcode};

use super::{CleanupContext, Compiler, LoopContext, Result};
use crate::error::CompileError;

mod definitions;
mod imports;
mod targets;

impl Compiler {
    // ── statement compilation ───────────────────────────────────────

    pub(super) fn compile_body(&mut self, stmts: &[Statement]) -> Result<()> {
        for stmt in stmts {
            self.compile_statement(stmt)?;
        }
        Ok(())
    }

    pub(super) fn compile_statement(&mut self, stmt: &Statement) -> Result<()> {
        // Record line number
        let line = stmt.location.line;
        let offset = self.current_offset();
        self.current_unit_mut()
            .code
            .line_number_table
            .push((offset, line));

        match &stmt.node {
            StatementKind::Expr { value } => {
                self.compile_expression(value)?;
                if self.interactive && self.unit_stack.len() == 1 {
                    // Interactive: dup for _ assignment, print, then store _
                    self.emit_op(Opcode::DupTop);
                    self.emit_op(Opcode::PrintExpr);
                    let name_idx = self.add_name("_");
                    self.emit_arg(Opcode::StoreName, name_idx);
                } else {
                    self.emit_op(Opcode::PopTop);
                }
            }

            StatementKind::Assign { targets, value, .. } => {
                self.compile_expression(value)?;
                // For multiple targets like `a = b = expr`, dup the value
                for (i, target) in targets.iter().enumerate() {
                    if i < targets.len() - 1 {
                        self.emit_op(Opcode::DupTop);
                    }
                    self.compile_store_target(target)?;
                }
            }

            StatementKind::AugAssign { target, op, value } => {
                self.compile_aug_assign(target, *op, value)?;
            }

            StatementKind::AnnAssign {
                target,
                annotation,
                value,
                simple,
            } => {
                if let Some(val) = value {
                    self.compile_expression(val)?;
                    self.compile_store_target(target)?;
                }
                if self.current_unit().is_function {
                    return Ok(());
                }
                // Store annotation in __annotations__ dict: __annotations__[name] = annotation
                if *simple {
                    let ExpressionKind::Name { id: name, .. } = &target.node else {
                        return Ok(());
                    };
                    if self.future_annotations {
                        // PEP 563: store annotation as string constant
                        let ann_str = Self::annotation_to_string(annotation);
                        let idx = self.add_const(ConstantValue::Str(CompactString::from(ann_str)));
                        self.emit_arg(Opcode::LoadConst, idx);
                    } else {
                        self.compile_expression(annotation)?;
                    }
                    self.load_name("__annotations__"); // push __annotations__ dict
                    let key = self.mangle_name(name);
                    let name_idx =
                        self.add_const(ConstantValue::Str(CompactString::from(key.as_ref())));
                    self.emit_arg(Opcode::LoadConst, name_idx); // push key
                    self.emit_op(Opcode::StoreSubscr);
                }
            }

            StatementKind::Return { value } => {
                if !self.current_unit().is_function {
                    return Err(CompileError::ReturnOutsideFunction {
                        location: stmt.location,
                    });
                }
                if let Some(val) = value {
                    self.compile_expression(val)?;
                } else {
                    let none_idx = self.add_const(ConstantValue::None);
                    self.emit_arg(Opcode::LoadConst, none_idx);
                }
                self.emit_op(Opcode::ReturnValue);
            }

            StatementKind::Delete { targets } => {
                for target in targets {
                    self.compile_delete_target(target)?;
                }
            }

            StatementKind::Pass => {
                self.emit_op(Opcode::Nop);
            }

            StatementKind::Break => {
                let Some(loop_ctx) = self.current_unit().loop_stack.last().cloned() else {
                    return Err(CompileError::BreakOutsideLoop {
                        location: stmt.location,
                    });
                };
                let needs_finally_jump = self.emit_loop_control_cleanups(loop_ctx.cleanup_depth);
                if loop_ctx.is_for_loop && needs_finally_jump {
                    let cleanup_label = self.emit_jump(Opcode::JumpFinally);
                    let cleanup_target = self.current_offset();
                    self.patch_jump(cleanup_label, cleanup_target);
                    self.emit_op(Opcode::EndForLoop);
                    let label = self.emit_jump(Opcode::JumpAbsolute);
                    self.current_unit_mut()
                        .loop_stack
                        .last_mut()
                        .unwrap()
                        .break_labels
                        .push(label);
                } else {
                    // For `for` loops, pop the iterator and close generators.
                    if loop_ctx.is_for_loop {
                        self.emit_op(Opcode::EndForLoop);
                    }
                    let jump_op = if needs_finally_jump {
                        Opcode::JumpFinally
                    } else {
                        Opcode::JumpAbsolute
                    };
                    let label = self.emit_jump(jump_op);
                    self.current_unit_mut()
                        .loop_stack
                        .last_mut()
                        .unwrap()
                        .break_labels
                        .push(label);
                }
            }

            StatementKind::Continue => {
                let Some(loop_ctx) = self.current_unit().loop_stack.last().cloned() else {
                    return Err(CompileError::ContinueOutsideLoop {
                        location: stmt.location,
                    });
                };
                let needs_finally_jump = self.emit_loop_control_cleanups(loop_ctx.cleanup_depth);
                let jump_op = if needs_finally_jump {
                    Opcode::JumpFinally
                } else {
                    Opcode::JumpAbsolute
                };
                self.emit_arg(jump_op, loop_ctx.continue_target);
            }

            StatementKind::If { test, body, orelse } => {
                self.compile_if(test, body, orelse)?;
            }

            StatementKind::While { test, body, orelse } => {
                self.compile_while(test, body, orelse)?;
            }

            StatementKind::For {
                target,
                iter,
                body,
                orelse,
                is_async,
                ..
            } => {
                if *is_async {
                    self.compile_async_for(target, iter, body, orelse)?;
                } else {
                    self.compile_for(target, iter, body, orelse)?;
                }
            }

            StatementKind::FunctionDef {
                name,
                args,
                body,
                decorator_list,
                returns,
                is_async,
                ..
            } => {
                self.compile_function_def(
                    name,
                    args,
                    body,
                    decorator_list,
                    returns.as_deref(),
                    *is_async,
                    stmt.location,
                )?;
            }

            StatementKind::ClassDef {
                name,
                bases,
                keywords,
                body,
                decorator_list,
            } => {
                self.compile_class_def(name, bases, keywords, body, decorator_list, stmt.location)?;
            }

            StatementKind::Import { names } => {
                self.compile_import(names)?;
            }

            StatementKind::ImportFrom {
                module,
                names,
                level,
            } => {
                self.compile_import_from(module.as_deref(), names, *level)?;
            }

            StatementKind::Global { names } => {
                for name in names {
                    if let Some(sym) = self.current_unit().scope.symbols.get(name.as_str()) {
                        if sym.is_parameter {
                            return Err(CompileError::ParameterAndGlobal {
                                name: name.to_string(),
                                location: stmt.location,
                            });
                        }
                    }
                }
            }
            StatementKind::Nonlocal { names } => {
                for name in names {
                    if let Some(sym) = self.current_unit().scope.symbols.get(name.as_str()) {
                        if sym.is_parameter {
                            return Err(CompileError::ParameterAndNonlocal {
                                name: name.to_string(),
                                location: stmt.location,
                            });
                        }
                    }
                }
            }

            StatementKind::Raise { exc, cause } => {
                self.compile_raise(exc.as_deref(), cause.as_deref())?;
            }

            StatementKind::Try {
                body,
                handlers,
                orelse,
                finalbody,
            } => {
                if handlers.iter().any(|h| h.is_star) {
                    self.compile_try_star(body, handlers, orelse, finalbody)?;
                } else {
                    self.compile_try(body, handlers, orelse, finalbody)?;
                }
            }

            StatementKind::Assert { test, msg } => {
                self.compile_assert(test, msg.as_deref())?;
            }

            StatementKind::With {
                items,
                body,
                is_async,
                ..
            } => {
                if *is_async {
                    self.compile_async_with(items, body)?;
                } else {
                    self.compile_with(items, body)?;
                }
            }

            StatementKind::Match { subject, cases } => {
                self.compile_match(subject, cases)?;
            }
        }
        Ok(())
    }

    fn emit_loop_control_cleanups(&mut self, cleanup_depth: usize) -> bool {
        let cleanups: Vec<CleanupContext> = self.current_unit().cleanup_stack[cleanup_depth..]
            .iter()
            .rev()
            .copied()
            .collect();
        let mut needs_finally_jump = false;
        for cleanup in cleanups {
            match cleanup {
                CleanupContext::With => {
                    self.emit_op(Opcode::PopBlock);
                    self.emit_op(Opcode::BeginFinally);
                    self.emit_op(Opcode::WithCleanupStart);
                    self.emit_op(Opcode::WithCleanupFinish);
                    self.emit_op(Opcode::EndFinally);
                }
                CleanupContext::ExceptHandler => {
                    self.emit_op(Opcode::PopExcept);
                }
                CleanupContext::TryFinally => {
                    needs_finally_jump = true;
                }
                CleanupContext::FinallyBody => {
                    self.emit_op(Opcode::CancelFinally);
                }
            }
        }
        needs_finally_jump
    }

    // ── control flow compilation ────────────────────────────────────

    pub(super) fn compile_if(
        &mut self,
        test: &Expression,
        body: &[Statement],
        orelse: &[Statement],
    ) -> Result<()> {
        self.compile_expression(test)?;
        let else_label = self.emit_jump(Opcode::PopJumpIfFalse);

        self.compile_body(body)?;

        if orelse.is_empty() {
            self.patch_jump_here(else_label);
        } else {
            let end_label = self.emit_jump(Opcode::JumpForward);
            self.patch_jump_here(else_label);
            self.compile_body(orelse)?;
            self.patch_jump_here(end_label);
        }
        Ok(())
    }

    pub(super) fn compile_while(
        &mut self,
        test: &Expression,
        body: &[Statement],
        orelse: &[Statement],
    ) -> Result<()> {
        let loop_start = self.current_offset();

        self.compile_expression(test)?;
        let done_label = self.emit_jump(Opcode::PopJumpIfFalse);

        let cleanup_depth = self.current_unit().cleanup_stack.len();
        self.current_unit_mut().loop_stack.push(LoopContext {
            continue_target: loop_start,
            break_labels: Vec::new(),
            is_for_loop: false,
            cleanup_depth,
        });

        self.compile_body(body)?;
        self.emit_arg(Opcode::JumpAbsolute, loop_start);

        self.patch_jump_here(done_label);

        let loop_ctx = self.current_unit_mut().loop_stack.pop().unwrap();

        // Else clause runs if loop completed normally (no break)
        if !orelse.is_empty() {
            self.compile_body(orelse)?;
        }

        // Patch all break labels to after the else clause
        let after = self.current_offset();
        for label in loop_ctx.break_labels {
            self.patch_jump(label, after);
        }

        Ok(())
    }

    pub(super) fn compile_for(
        &mut self,
        target: &Expression,
        iter: &Expression,
        body: &[Statement],
        orelse: &[Statement],
    ) -> Result<()> {
        self.compile_expression(iter)?;
        self.emit_op(Opcode::GetIter);

        let loop_start = self.current_offset();
        let done_label = self.emit_jump(Opcode::ForIter);

        // Store the iteration value
        self.compile_store_target(target)?;

        let cleanup_depth = self.current_unit().cleanup_stack.len();
        self.current_unit_mut().loop_stack.push(LoopContext {
            continue_target: loop_start,
            break_labels: Vec::new(),
            is_for_loop: true,
            cleanup_depth,
        });

        self.compile_body(body)?;
        self.emit_arg(Opcode::JumpAbsolute, loop_start);

        self.patch_jump_here(done_label);

        let loop_ctx = self.current_unit_mut().loop_stack.pop().unwrap();

        if !orelse.is_empty() {
            self.compile_body(orelse)?;
        }

        let after = self.current_offset();
        for label in loop_ctx.break_labels {
            self.patch_jump(label, after);
        }

        Ok(())
    }

    // ── raise, try/except, with ─────────────────────────────────────

    pub(super) fn compile_raise(
        &mut self,
        exc: Option<&Expression>,
        cause: Option<&Expression>,
    ) -> Result<()> {
        let argc = match (exc, cause) {
            (None, _) => 0,
            (Some(e), None) => {
                self.compile_expression(e)?;
                1
            }
            (Some(e), Some(c)) => {
                self.compile_expression(e)?;
                self.compile_expression(c)?;
                2
            }
        };
        self.emit_arg(Opcode::RaiseVarargs, argc);
        Ok(())
    }

    pub(super) fn compile_try(
        &mut self,
        body: &[Statement],
        handlers: &[ExceptHandler],
        orelse: &[Statement],
        finalbody: &[Statement],
    ) -> Result<()> {
        let has_finally = !finalbody.is_empty();

        // If there's a finally clause, set up the outer try
        let finally_label = if has_finally {
            Some(self.emit_jump(Opcode::SetupFinally))
        } else {
            None
        };

        // Set up try/except
        let except_label = if !handlers.is_empty() {
            Some(self.emit_jump(Opcode::SetupExcept))
        } else {
            None
        };

        if has_finally {
            self.current_unit_mut()
                .cleanup_stack
                .push(CleanupContext::TryFinally);
        }

        // Compile try body
        self.compile_body(body)?;

        // Pop the except handler block
        if except_label.is_some() {
            self.emit_op(Opcode::PopBlock);
        }

        // Compile else clause (runs if no exception)
        if !orelse.is_empty() {
            self.compile_body(orelse)?;
        }

        // Jump past the except handlers
        let after_except_label = if !handlers.is_empty() {
            Some(self.emit_jump(Opcode::JumpForward))
        } else {
            None
        };

        // Except handler entry point
        if let Some(label) = except_label {
            self.patch_jump_here(label);
        }

        // Compile each except handler
        let mut handler_end_labels = Vec::new();
        for (i, handler) in handlers.iter().enumerate() {
            if let Some(ref typ) = handler.typ {
                // DupTop to keep the exception on stack for matching
                self.emit_op(Opcode::DupTop);
                self.compile_expression(typ)?;
                self.emit_arg(Opcode::CompareOp, 10); // exception match
                let no_match = self.emit_jump(Opcode::PopJumpIfFalse);

                // Matched: pop the exception
                self.emit_op(Opcode::PopTop);

                if let Some(ref name) = handler.name {
                    // Store exception in named variable
                    self.store_name(name);
                } else {
                    self.emit_op(Opcode::PopTop);
                }
                // Pop the traceback
                self.emit_op(Opcode::PopTop);

                // Compile handler body BEFORE PopExcept (matches CPython order)
                self.current_unit_mut()
                    .cleanup_stack
                    .push(CleanupContext::ExceptHandler);
                self.compile_body(&handler.body)?;
                self.current_unit_mut().cleanup_stack.pop();

                // Clean up: store None into handler var then delete it (prevents
                // exception→traceback reference cycles, matching CPython behavior)
                if handler.name.is_some() {
                    let name = handler.name.as_ref().unwrap();
                    let none_idx = self.add_const(ConstantValue::None);
                    self.emit_arg(Opcode::LoadConst, none_idx);
                    self.store_name(name);
                    self.delete_name(name);
                }

                self.emit_op(Opcode::PopExcept);
                handler_end_labels.push(self.emit_jump(Opcode::JumpForward));
                self.patch_jump_here(no_match);
            } else {
                // Bare except: catches everything
                self.emit_op(Opcode::PopTop);
                if let Some(ref name) = handler.name {
                    self.store_name(name);
                } else {
                    self.emit_op(Opcode::PopTop);
                }
                self.emit_op(Opcode::PopTop);

                // Compile handler body BEFORE PopExcept (matches CPython order)
                self.current_unit_mut()
                    .cleanup_stack
                    .push(CleanupContext::ExceptHandler);
                self.compile_body(&handler.body)?;
                self.current_unit_mut().cleanup_stack.pop();

                // Clean up handler variable (same as typed except path)
                if handler.name.is_some() {
                    let name = handler.name.as_ref().unwrap();
                    let none_idx = self.add_const(ConstantValue::None);
                    self.emit_arg(Opcode::LoadConst, none_idx);
                    self.store_name(name);
                    self.delete_name(name);
                }

                self.emit_op(Opcode::PopExcept);
                if i < handlers.len() - 1 {
                    handler_end_labels.push(self.emit_jump(Opcode::JumpForward));
                }
            }
        }

        // If no handler matched, re-raise
        if !handlers.is_empty() {
            self.emit_op(Opcode::EndFinally);
        }

        // Patch all handler end jumps
        if let Some(label) = after_except_label {
            self.patch_jump_here(label);
        }
        let after_handlers = self.current_offset();
        for label in handler_end_labels {
            self.patch_jump(label, after_handlers);
        }

        if has_finally {
            self.current_unit_mut().cleanup_stack.pop();
        }

        // Finally block
        if has_finally {
            self.emit_op(Opcode::PopBlock);
            self.emit_op(Opcode::BeginFinally);
            if let Some(label) = finally_label {
                self.patch_jump_here(label);
            }
            self.current_unit_mut()
                .cleanup_stack
                .push(CleanupContext::FinallyBody);
            self.compile_body(finalbody)?;
            self.current_unit_mut().cleanup_stack.pop();
            self.emit_op(Opcode::EndFinally);
        }

        Ok(())
    }

    /// Compile try/except* (PEP 654 — exception groups)
    pub(super) fn compile_try_star(
        &mut self,
        body: &[Statement],
        handlers: &[ExceptHandler],
        orelse: &[Statement],
        finalbody: &[Statement],
    ) -> Result<()> {
        let has_finally = !finalbody.is_empty();

        let finally_label = if has_finally {
            Some(self.emit_jump(Opcode::SetupFinally))
        } else {
            None
        };

        let except_label = self.emit_jump(Opcode::SetupExcept);

        if has_finally {
            self.current_unit_mut()
                .cleanup_stack
                .push(CleanupContext::TryFinally);
        }

        self.compile_body(body)?;
        self.emit_op(Opcode::PopBlock);

        if !orelse.is_empty() {
            self.compile_body(orelse)?;
        }

        let after_except = self.emit_jump(Opcode::JumpForward);

        self.patch_jump_here(except_label);

        // Stack: [traceback, value, type] with type on top
        let remain_var = CompactString::from("$exc_remain$");
        let remain_idx = self.varname_index(&remain_var);
        self.emit_op(Opcode::PopTop); // pop type
        self.emit_arg(Opcode::StoreFast, remain_idx); // store value
        self.emit_op(Opcode::PopTop); // pop traceback

        // For each except* handler: try to split, run handler body if matched
        let mut handler_end_labels = Vec::new();
        for handler in handlers {
            if let Some(ref typ) = handler.typ {
                // Skip if remainder is None
                self.emit_arg(Opcode::LoadFast, remain_idx);
                let none_idx = self.add_const(ConstantValue::None);
                self.emit_arg(Opcode::LoadConst, none_idx);
                self.emit_arg(Opcode::CompareOp, 8); // is
                let skip_handler = self.emit_jump(Opcode::PopJumpIfTrue);

                // Call $exc_remain$.split(Type) → (match, rest)
                self.load_name("getattr");
                self.emit_arg(Opcode::LoadFast, remain_idx);
                let split_str = self.add_const(ConstantValue::Str(CompactString::from("split")));
                self.emit_arg(Opcode::LoadConst, split_str);
                self.emit_arg(Opcode::CallFunction, 2);
                // Stack: split_method (bound)
                self.compile_expression(typ)?;
                self.emit_arg(Opcode::CallFunction, 1);
                // Stack: (match, rest) tuple — unpack
                self.emit_arg(Opcode::UnpackSequence, 2);
                let match_var =
                    CompactString::from(format!("$exc_match_{}$", handler.location.line));
                let match_idx = self.varname_index(&match_var);
                let rest_var = CompactString::from(format!("$exc_rest_{}$", handler.location.line));
                let rest_idx = self.varname_index(&rest_var);
                self.emit_arg(Opcode::StoreFast, match_idx);
                self.emit_arg(Opcode::StoreFast, rest_idx);

                // If match is None, skip handler body
                self.emit_arg(Opcode::LoadFast, match_idx);
                self.emit_arg(Opcode::LoadConst, none_idx);
                self.emit_arg(Opcode::CompareOp, 8); // is
                let no_match = self.emit_jump(Opcode::PopJumpIfTrue);

                // Match! Bind and execute handler body
                if let Some(ref name) = handler.name {
                    self.emit_arg(Opcode::LoadFast, match_idx);
                    self.store_name(name);
                }
                self.compile_body(&handler.body)?;
                // Clean up handler variable
                if let Some(ref name) = handler.name {
                    let none_c = self.add_const(ConstantValue::None);
                    self.emit_arg(Opcode::LoadConst, none_c);
                    self.store_name(name);
                    self.delete_name(name);
                }
                // Update remainder
                self.emit_arg(Opcode::LoadFast, rest_idx);
                self.emit_arg(Opcode::StoreFast, remain_idx);
                let after_handler = self.emit_jump(Opcode::JumpForward);

                self.patch_jump_here(no_match);
                // No match: remainder stays unchanged
                self.patch_jump_here(after_handler);
                self.patch_jump_here(skip_handler);
            }
        }

        // After all handlers: if remainder is not None, re-raise it
        self.emit_arg(Opcode::LoadFast, remain_idx);
        let none_idx2 = self.add_const(ConstantValue::None);
        self.emit_arg(Opcode::LoadConst, none_idx2);
        self.emit_arg(Opcode::CompareOp, 8); // is
        let all_done = self.emit_jump(Opcode::PopJumpIfTrue);
        self.emit_arg(Opcode::LoadFast, remain_idx);
        self.emit_arg(Opcode::RaiseVarargs, 1);
        self.patch_jump_here(all_done);
        self.emit_op(Opcode::PopExcept);

        handler_end_labels.push(self.emit_jump(Opcode::JumpForward));

        self.patch_jump_here(after_except);
        for label in handler_end_labels {
            self.patch_jump_here(label);
        }

        if has_finally {
            self.current_unit_mut().cleanup_stack.pop();
        }

        if has_finally {
            self.emit_op(Opcode::PopBlock);
            self.emit_op(Opcode::BeginFinally);
            if let Some(label) = finally_label {
                self.patch_jump_here(label);
            }
            self.current_unit_mut()
                .cleanup_stack
                .push(CleanupContext::FinallyBody);
            self.compile_body(finalbody)?;
            self.current_unit_mut().cleanup_stack.pop();
            self.emit_op(Opcode::EndFinally);
        }

        Ok(())
    }

    pub(super) fn compile_assert(
        &mut self,
        test: &Expression,
        msg: Option<&Expression>,
    ) -> Result<()> {
        self.compile_expression(test)?;
        let ok_label = self.emit_jump(Opcode::PopJumpIfTrue);

        // Load AssertionError
        self.load_name("AssertionError");

        if let Some(m) = msg {
            self.compile_expression(m)?;
            self.emit_arg(Opcode::CallFunction, 1);
        }

        self.emit_arg(Opcode::RaiseVarargs, 1);
        self.patch_jump_here(ok_label);
        Ok(())
    }

    pub(super) fn compile_with(&mut self, items: &[WithItem], body: &[Statement]) -> Result<()> {
        // Nested withs: `with a, b:` is equivalent to `with a: with b:`
        self.compile_with_item(items, 0, body)
    }

    pub(super) fn compile_with_item(
        &mut self,
        items: &[WithItem],
        idx: usize,
        body: &[Statement],
    ) -> Result<()> {
        if idx >= items.len() {
            return self.compile_body(body);
        }

        let item = &items[idx];

        // Evaluate the context expression
        self.compile_expression(&item.context_expr)?;

        // SETUP_WITH pushes __exit__ and the result of __enter__
        let cleanup_label = self.emit_jump(Opcode::SetupWith);

        // Store the __enter__ result if there's an `as` target
        if let Some(ref vars) = item.optional_vars {
            self.compile_store_target(vars)?;
        } else {
            self.emit_op(Opcode::PopTop);
        }

        // Compile inner withs or body
        self.current_unit_mut()
            .cleanup_stack
            .push(CleanupContext::With);
        self.compile_with_item(items, idx + 1, body)?;
        self.current_unit_mut().cleanup_stack.pop();

        // Normal exit
        self.emit_op(Opcode::PopBlock);
        self.emit_op(Opcode::BeginFinally);
        self.patch_jump_here(cleanup_label);
        self.emit_op(Opcode::WithCleanupStart);
        self.emit_op(Opcode::WithCleanupFinish);
        self.emit_op(Opcode::EndFinally);

        Ok(())
    }

    // ── async for ───────────────────────────────────────────────────

    pub(super) fn compile_async_for(
        &mut self,
        target: &Expression,
        iter: &Expression,
        body: &[Statement],
        orelse: &[Statement],
    ) -> Result<()> {
        // Compile iterator expression and get async iterator
        self.compile_expression(iter)?;
        self.emit_op(Opcode::GetAiter);

        let loop_start = self.current_offset();

        // Setup except handler for StopAsyncIteration → EndAsyncFor
        let except_label = self.emit_jump(Opcode::SetupExcept);

        // GET_ANEXT → GET_AWAITABLE → LOAD_CONST None → YIELD_FROM
        self.emit_op(Opcode::GetAnext);
        self.emit_op(Opcode::GetAwaitable);
        let none_idx = self.add_const(ConstantValue::None);
        self.emit_arg(Opcode::LoadConst, none_idx);
        self.emit_op(Opcode::YieldFrom);

        // Store iteration value
        self.compile_store_target(target)?;

        // Pop exception handler before body
        self.emit_op(Opcode::PopBlock);

        let cleanup_depth = self.current_unit().cleanup_stack.len();
        self.current_unit_mut().loop_stack.push(LoopContext {
            continue_target: loop_start,
            break_labels: Vec::new(),
            is_for_loop: true,
            cleanup_depth,
        });

        // Compile body
        self.compile_body(body)?;

        // Jump back to loop start
        self.emit_arg(Opcode::JumpAbsolute, loop_start);

        // Exception handler: END_ASYNC_FOR cleans up StopAsyncIteration
        self.patch_jump_here(except_label);
        self.emit_op(Opcode::EndAsyncFor);

        let loop_ctx = self.current_unit_mut().loop_stack.pop().unwrap();

        if !orelse.is_empty() {
            self.compile_body(orelse)?;
        }

        let after = self.current_offset();
        for label in loop_ctx.break_labels {
            self.patch_jump(label, after);
        }

        Ok(())
    }

    // ── async with ──────────────────────────────────────────────────

    pub(super) fn compile_async_with(
        &mut self,
        items: &[WithItem],
        body: &[Statement],
    ) -> Result<()> {
        self.compile_async_with_item(items, 0, body)
    }

    pub(super) fn compile_async_with_item(
        &mut self,
        items: &[WithItem],
        idx: usize,
        body: &[Statement],
    ) -> Result<()> {
        if idx >= items.len() {
            return self.compile_body(body);
        }

        let item = &items[idx];

        // Evaluate context expression
        self.compile_expression(&item.context_expr)?;

        // BEFORE_ASYNC_WITH calls __aenter__, pushes awaitable
        self.emit_op(Opcode::BeforeAsyncWith);

        // Await the __aenter__ result
        self.emit_op(Opcode::GetAwaitable);
        let none_idx = self.add_const(ConstantValue::None);
        self.emit_arg(Opcode::LoadConst, none_idx);
        self.emit_op(Opcode::YieldFrom);

        // SETUP_ASYNC_WITH sets up __aexit__ handler
        let cleanup_label = self.emit_jump(Opcode::SetupAsyncWith);

        // Store __aenter__ result if there's an `as` target
        if let Some(ref vars) = item.optional_vars {
            self.compile_store_target(vars)?;
        } else {
            self.emit_op(Opcode::PopTop);
        }

        // Compile inner withs or body
        self.current_unit_mut()
            .cleanup_stack
            .push(CleanupContext::With);
        self.compile_async_with_item(items, idx + 1, body)?;
        self.current_unit_mut().cleanup_stack.pop();

        // Normal exit: pop block, then run cleanup
        self.emit_op(Opcode::PopBlock);
        self.emit_op(Opcode::BeginFinally);
        self.patch_jump_here(cleanup_label);
        self.emit_op(Opcode::WithCleanupStart);
        self.emit_op(Opcode::WithCleanupFinish);
        self.emit_op(Opcode::EndFinally);

        Ok(())
    }
}
