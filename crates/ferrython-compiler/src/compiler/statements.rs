//! Statement compilation methods for the Compiler.

use compact_str::CompactString;
use ferrython_ast::*;
use ferrython_bytecode::{CodeFlags, CodeObject, ConstantValue, Opcode};

use crate::error::CompileError;
use crate::symbol_table::Scope;
use super::{Compiler, CompileUnit, LoopContext, Result};
use super::expressions::body_contains_yield;

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

            StatementKind::AnnAssign { target, annotation, value, .. } => {
                if let Some(val) = value {
                    self.compile_expression(val)?;
                    self.compile_store_target(target)?;
                }
                // Store annotation in __annotations__ dict: __annotations__[name] = annotation
                if let ExpressionKind::Name { id: name, .. } = &target.node {
                    // Stack needs: value(annotation), obj(__annotations__), key(name_str)
                    self.compile_expression(annotation)?;  // push annotation value
                    self.load_name("__annotations__");      // push __annotations__ dict
                    let name_idx = self.add_const(ConstantValue::Str(CompactString::from(name.as_str())));
                    self.emit_arg(Opcode::LoadConst, name_idx);  // push key
                    self.emit_op(Opcode::StoreSubscr);
                }
            }

            StatementKind::Return { value } => {
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
                if self.current_unit().loop_stack.is_empty() {
                    return Err(CompileError::BreakOutsideLoop {
                        location: stmt.location,
                    });
                }
                // For `for` loops, pop the iterator off the stack before jumping
                if self.current_unit().loop_stack.last().unwrap().is_for_loop {
                    self.emit_op(Opcode::PopTop);
                }
                let label = self.emit_jump(Opcode::JumpAbsolute);
                self.current_unit_mut()
                    .loop_stack
                    .last_mut()
                    .unwrap()
                    .break_labels
                    .push(label);
            }

            StatementKind::Continue => {
                let loop_ctx = self.current_unit().loop_stack.last();
                match loop_ctx {
                    Some(ctx) => {
                        let target = ctx.continue_target;
                        self.emit_arg(Opcode::JumpAbsolute, target);
                    }
                    None => {
                        return Err(CompileError::ContinueOutsideLoop {
                            location: stmt.location,
                        });
                    }
                }
            }

            StatementKind::If {
                test,
                body,
                orelse,
            } => {
                self.compile_if(test, body, orelse)?;
            }

            StatementKind::While {
                test,
                body,
                orelse,
            } => {
                self.compile_while(test, body, orelse)?;
            }

            StatementKind::For {
                target,
                iter,
                body,
                orelse,
                ..
            } => {
                self.compile_for(target, iter, body, orelse)?;
            }

            StatementKind::FunctionDef {
                name,
                args,
                body,
                decorator_list,
                returns: _,
                is_async,
                ..
            } => {
                self.compile_function_def(
                    name,
                    args,
                    body,
                    decorator_list,
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
                self.compile_class_def(name, bases, keywords, body, decorator_list)?;
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

            StatementKind::Global { .. } | StatementKind::Nonlocal { .. } => {
                // Handled by symbol table; no bytecode emitted.
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
                self.compile_try(body, handlers, orelse, finalbody)?;
            }

            StatementKind::Assert { test, msg } => {
                self.compile_assert(test, msg.as_deref())?;
            }

            StatementKind::With { items, body, .. } => {
                self.compile_with(items, body)?;
            }
        }
        Ok(())
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

        self.current_unit_mut().loop_stack.push(LoopContext {
            continue_target: loop_start,
            break_labels: Vec::new(),
            is_for_loop: false,
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

        self.current_unit_mut().loop_stack.push(LoopContext {
            continue_target: loop_start,
            break_labels: Vec::new(),
            is_for_loop: true,
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

    // ── function definition ─────────────────────────────────────────

    pub(super) fn compile_function_def(
        &mut self,
        name: &str,
        args: &Arguments,
        body: &[Statement],
        decorator_list: &[Expression],
        is_async: bool,
        _location: SourceLocation,
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
                let key_idx = self.add_const(ConstantValue::Str(arg.arg.clone()));
                self.emit_arg(Opcode::LoadConst, key_idx);
                self.compile_expression(default.as_ref().unwrap())?;
            }
            self.emit_arg(Opcode::BuildMap, kw_defaults.len() as u32);
        }

        // Build child code object
        let child_scope = self.current_unit_mut().take_child_scope();
        let qualname_prefix = &self.current_unit().qualname_prefix;
        let qualname = if qualname_prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}.{}", qualname_prefix, name)
        };

        self.push_function_unit(name, child_scope, &qualname)?;

        // Set up argument info on the code object
        {
            let unit = self.current_unit_mut();
            unit.code.arg_count =
                (args.posonlyargs.len() + args.args.len()) as u32;
            unit.code.posonlyarg_count = args.posonlyargs.len() as u32;
            unit.code.kwonlyarg_count = args.kwonlyargs.len() as u32;

            // Add parameters as varnames
            for arg in &args.posonlyargs {
                let name_str = arg.arg.as_str();
                let varnames = &unit.code.varnames;
                if !varnames.iter().any(|v| v.as_str() == name_str) {
                    unit.code.varnames.push(arg.arg.clone());
                }
            }
            for arg in &args.args {
                let name_str = arg.arg.as_str();
                let varnames = &unit.code.varnames;
                if !varnames.iter().any(|v| v.as_str() == name_str) {
                    unit.code.varnames.push(arg.arg.clone());
                }
            }
            if let Some(ref vararg) = args.vararg {
                unit.code.flags |= CodeFlags::VARARGS;
                let name_str = vararg.arg.as_str();
                let varnames = &unit.code.varnames;
                if !varnames.iter().any(|v| v.as_str() == name_str) {
                    unit.code.varnames.push(vararg.arg.clone());
                }
            }
            for arg in &args.kwonlyargs {
                let name_str = arg.arg.as_str();
                let varnames = &unit.code.varnames;
                if !varnames.iter().any(|v| v.as_str() == name_str) {
                    unit.code.varnames.push(arg.arg.clone());
                }
            }
            if let Some(ref kwarg) = args.kwarg {
                unit.code.flags |= CodeFlags::VARKEYWORDS;
                let name_str = kwarg.arg.as_str();
                let varnames = &unit.code.varnames;
                if !varnames.iter().any(|v| v.as_str() == name_str) {
                    unit.code.varnames.push(kwarg.arg.clone());
                }
            }

            if is_async {
                unit.code.flags |= CodeFlags::COROUTINE;
            }
        }

        // Compile the function body
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
        let code_idx = self.add_const(ConstantValue::Code(Box::new(func_code)));
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

    pub(super) fn push_function_unit(
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

    pub(super) fn pop_function_unit(&mut self) -> CodeObject {
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
        } else {
            format!("{}.{}", qualname_prefix, name)
        };

        let mut class_unit =
            CompileUnit::new(name, &self.filename, child_scope, false, qualname.clone());
        class_unit.code.flags = CodeFlags::empty();
        // The class body function takes __locals__ as implicit first arg
        class_unit.code.arg_count = 0;
        self.unit_stack.push(class_unit);

        // __name__ = qualname
        let qname_idx = self.add_const(ConstantValue::Str(qualname.clone().into()));
        self.emit_arg(Opcode::LoadConst, qname_idx);
        self.store_name("__qualname__");

        // Setup annotations dict for the class body
        self.emit_op(Opcode::SetupAnnotations);

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
                // Find the cell index in the current (parent) scope
                let unit = self.current_unit();
                let cell_idx = unit.code.cellvars.iter().position(|v| v == freevar_name)
                    .or_else(|| {
                        unit.code.freevars.iter().position(|v| v == freevar_name)
                            .map(|i| i + unit.code.cellvars.len())
                    });
                if let Some(idx) = cell_idx {
                    self.emit_arg(Opcode::LoadClosure, idx as u32);
                }
            }
            self.emit_arg(Opcode::BuildTuple, num_freevars as u32);
        }

        // Load the class body code object
        let code_idx = self.add_const(ConstantValue::Code(Box::new(class_code)));
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
            self.emit_arg(
                Opcode::CallFunctionKw,
                total_args + num_kw as u32,
            );
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

    // ── import compilation ──────────────────────────────────────────

    pub(super) fn compile_import(&mut self, names: &[Alias]) -> Result<()> {
        for alias in names {
            // Push level (0 for absolute import)
            let zero_idx = self.add_const(ConstantValue::Integer(0));
            self.emit_arg(Opcode::LoadConst, zero_idx);

            // Push fromlist (None for regular import)
            let none_idx = self.add_const(ConstantValue::None);
            self.emit_arg(Opcode::LoadConst, none_idx);

            let name_idx = self.add_name(&alias.name);
            self.emit_arg(Opcode::ImportName, name_idx);

            if let Some(ref asname) = alias.asname {
                self.store_name(asname);
            } else {
                // For `import a.b.c`, we need to store `a` (the top-level module)
                let top = alias.name.split('.').next().unwrap_or(&alias.name);
                if alias.name.contains('.') {
                    // Import of dotted name: store the top-level module
                    self.store_name(top);
                } else {
                    self.store_name(&alias.name);
                }
            }
        }
        Ok(())
    }

    pub(super) fn compile_import_from(
        &mut self,
        module: Option<&str>,
        names: &[Alias],
        level: u32,
    ) -> Result<()> {
        // Push level
        let level_idx = self.add_const(ConstantValue::Integer(level as i64));
        self.emit_arg(Opcode::LoadConst, level_idx);

        // Build fromlist tuple
        if names.len() == 1 && names[0].name.as_str() == "*" {
            let star_idx = self.add_const(ConstantValue::Tuple(vec![ConstantValue::Str(
                "*".into(),
            )]));
            self.emit_arg(Opcode::LoadConst, star_idx);
        } else {
            let from_names: Vec<ConstantValue> = names
                .iter()
                .map(|a| ConstantValue::Str(a.name.clone()))
                .collect();
            let tuple_idx = self.add_const(ConstantValue::Tuple(from_names));
            self.emit_arg(Opcode::LoadConst, tuple_idx);
        }

        let mod_name = module.unwrap_or("");
        let mod_idx = self.add_name(mod_name);
        self.emit_arg(Opcode::ImportName, mod_idx);

        if names.len() == 1 && names[0].name.as_str() == "*" {
            self.emit_op(Opcode::ImportStar);
        } else {
            for alias in names {
                let from_idx = self.add_name(&alias.name);
                self.emit_arg(Opcode::ImportFrom, from_idx);
                let store_as = alias.asname.as_deref().unwrap_or(&alias.name);
                self.store_name(store_as);
            }
            // Pop the module left by ImportName
            self.emit_op(Opcode::PopTop);
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

                self.emit_op(Opcode::PopExcept);
                self.compile_body(&handler.body)?;

                // Clean up: store None into handler var then delete it (prevents
                // exception→traceback reference cycles, matching CPython behavior)
                if handler.name.is_some() {
                    let name = handler.name.as_ref().unwrap();
                    let none_idx = self.add_const(ConstantValue::None);
                    self.emit_arg(Opcode::LoadConst, none_idx);
                    self.store_name(name);
                    self.delete_name(name);
                }

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

                self.emit_op(Opcode::PopExcept);
                self.compile_body(&handler.body)?;

                // Clean up handler variable (same as typed except path)
                if handler.name.is_some() {
                    let name = handler.name.as_ref().unwrap();
                    let none_idx = self.add_const(ConstantValue::None);
                    self.emit_arg(Opcode::LoadConst, none_idx);
                    self.store_name(name);
                    self.delete_name(name);
                }

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

        // Finally block
        if has_finally {
            self.emit_op(Opcode::PopBlock);
            self.emit_op(Opcode::BeginFinally);
            if let Some(label) = finally_label {
                self.patch_jump_here(label);
            }
            self.compile_body(finalbody)?;
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

    pub(super) fn compile_with(
        &mut self,
        items: &[WithItem],
        body: &[Statement],
    ) -> Result<()> {
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
        self.compile_with_item(items, idx + 1, body)?;

        // Normal exit
        self.emit_op(Opcode::PopBlock);
        self.emit_op(Opcode::BeginFinally);
        self.patch_jump_here(cleanup_label);
        self.emit_op(Opcode::WithCleanupStart);
        self.emit_op(Opcode::WithCleanupFinish);
        self.emit_op(Opcode::EndFinally);

        Ok(())
    }

    // ── augmented assignment ────────────────────────────────────────

    pub(super) fn compile_aug_assign(
        &mut self,
        target: &Expression,
        op: Operator,
        value: &Expression,
    ) -> Result<()> {
        match &target.node {
            ExpressionKind::Name { id, .. } => {
                self.load_name(id);
                self.compile_expression(value)?;
                self.emit_inplace_op(op);
                self.store_name(id);
            }
            ExpressionKind::Attribute { value: obj, attr, .. } => {
                self.compile_expression(obj)?;
                self.emit_op(Opcode::DupTop);
                let attr_idx = self.add_name(attr);
                self.emit_arg(Opcode::LoadAttr, attr_idx);
                self.compile_expression(value)?;
                self.emit_inplace_op(op);
                self.emit_op(Opcode::RotTwo);
                self.emit_arg(Opcode::StoreAttr, attr_idx);
            }
            ExpressionKind::Subscript {
                value: obj,
                slice,
                ..
            } => {
                self.compile_expression(obj)?;
                self.compile_expression(slice)?;
                self.emit_op(Opcode::DupTopTwo);
                self.emit_op(Opcode::BinarySubscr);
                self.compile_expression(value)?;
                self.emit_inplace_op(op);
                self.emit_op(Opcode::RotThree);
                self.emit_op(Opcode::StoreSubscr);
            }
            _ => {
                return Err(CompileError::InvalidAssignTarget {
                    location: target.location,
                });
            }
        }
        Ok(())
    }

    pub(super) fn emit_inplace_op(&mut self, op: Operator) {
        let opcode = match op {
            Operator::Add => Opcode::InplaceAdd,
            Operator::Sub => Opcode::InplaceSubtract,
            Operator::Mult => Opcode::InplaceMultiply,
            Operator::MatMult => Opcode::InplaceMatrixMultiply,
            Operator::Div => Opcode::InplaceTrueDivide,
            Operator::FloorDiv => Opcode::InplaceFloorDivide,
            Operator::Mod => Opcode::InplaceModulo,
            Operator::Pow => Opcode::InplacePower,
            Operator::LShift => Opcode::InplaceLshift,
            Operator::RShift => Opcode::InplaceRshift,
            Operator::BitOr => Opcode::InplaceOr,
            Operator::BitXor => Opcode::InplaceXor,
            Operator::BitAnd => Opcode::InplaceAnd,
        };
        self.emit_op(opcode);
    }

    // ── store / delete targets ──────────────────────────────────────

    pub(super) fn compile_store_target(&mut self, target: &Expression) -> Result<()> {
        match &target.node {
            ExpressionKind::Name { id, .. } => {
                self.store_name(id);
            }
            ExpressionKind::Attribute { value, attr, .. } => {
                self.compile_expression(value)?;
                let attr_idx = self.add_name(attr);
                self.emit_arg(Opcode::StoreAttr, attr_idx);
            }
            ExpressionKind::Subscript { value, slice, .. } => {
                self.compile_expression(value)?;
                self.compile_expression(slice)?;
                self.emit_op(Opcode::StoreSubscr);
            }
            ExpressionKind::Tuple { elts, .. } | ExpressionKind::List { elts, .. } => {
                // Check for starred element
                let star_count = elts.iter().filter(|e| {
                    matches!(e.node, ExpressionKind::Starred { .. })
                }).count();

                if star_count > 1 {
                    return Err(CompileError::syntax(
                        "multiple starred expressions in assignment",
                        target.location,
                    ));
                }

                let star_idx = elts.iter().position(|e| {
                    matches!(e.node, ExpressionKind::Starred { .. })
                });

                if let Some(star) = star_idx {
                    let before = star as u32;
                    let after = (elts.len() - star - 1) as u32;
                    self.emit_arg(Opcode::UnpackEx, before | (after << 8));
                } else {
                    self.emit_arg(Opcode::UnpackSequence, elts.len() as u32);
                }

                for elt in elts {
                    if let ExpressionKind::Starred { value, .. } = &elt.node {
                        self.compile_store_target(value)?;
                    } else {
                        self.compile_store_target(elt)?;
                    }
                }
            }
            ExpressionKind::Starred { value, .. } => {
                self.compile_store_target(value)?;
            }
            _ => {
                return Err(CompileError::InvalidAssignTarget {
                    location: target.location,
                });
            }
        }
        Ok(())
    }

    pub(super) fn compile_delete_target(&mut self, target: &Expression) -> Result<()> {
        match &target.node {
            ExpressionKind::Name { id, .. } => {
                self.delete_name(id);
            }
            ExpressionKind::Attribute { value, attr, .. } => {
                self.compile_expression(value)?;
                let attr_idx = self.add_name(attr);
                self.emit_arg(Opcode::DeleteAttr, attr_idx);
            }
            ExpressionKind::Subscript { value, slice, .. } => {
                self.compile_expression(value)?;
                self.compile_expression(slice)?;
                self.emit_op(Opcode::DeleteSubscr);
            }
            ExpressionKind::Tuple { elts, .. } | ExpressionKind::List { elts, .. } => {
                for elt in elts {
                    self.compile_delete_target(elt)?;
                }
            }
            _ => {
                return Err(CompileError::InvalidAssignTarget {
                    location: target.location,
                });
            }
        }
        Ok(())
    }

}
