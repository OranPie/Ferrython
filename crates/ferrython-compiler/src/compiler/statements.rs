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
                // For `for` loops, pop the iterator and close generators
                if self.current_unit().loop_stack.last().unwrap().is_for_loop {
                    self.emit_op(Opcode::EndForLoop);
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

            StatementKind::With { items, body, is_async, .. } => {
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

        // Set the first line number from the def statement location
        self.current_unit_mut().code.first_line_number = location.line;

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

        // Emit SetupAnnotations if the function body has annotation assignments
        if Self::has_annotations(body) {
            self.emit_op(Opcode::SetupAnnotations);
            // Ensure __annotations__ is registered as a local variable so
            // load_name() compiles to LoadFast instead of LoadGlobal
            let unit = self.current_unit_mut();
            let ann_name = CompactString::from("__annotations__");
            if !unit.code.varnames.iter().any(|v| v == &ann_name) {
                unit.code.varnames.push(ann_name);
            }
        }

        // Compile the function body
        // Extract docstring: if first statement is a string literal, store as first constant
        if let Some(first) = body.first() {
            if let StatementKind::Expr { value } = &first.node {
                if let ExpressionKind::Constant { value: Constant::Str(doc) } = &value.node {
                    // Ensure docstring is the first constant in the code object
                    let unit = self.current_unit_mut();
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
        let all_args: Vec<&Arg> = args.posonlyargs.iter()
            .chain(args.args.iter())
            .chain(args.vararg.iter())
            .chain(args.kwonlyargs.iter())
            .chain(args.kwarg.iter())
            .collect();
        let mut ann_count: u32 = 0;
        for arg in &all_args {
            if let Some(ref annotation) = arg.annotation {
                let key_idx = self.add_const(ConstantValue::Str(arg.arg.clone()));
                self.emit_arg(Opcode::LoadConst, key_idx);
                self.compile_expression(annotation)?;
                ann_count += 1;
            }
        }
        if let Some(ret) = returns {
            let key_idx = self.add_const(ConstantValue::Str("return".into()));
            self.emit_arg(Opcode::LoadConst, key_idx);
            self.compile_expression(ret)?;
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
        let code_idx = self.add_const(ConstantValue::Code(std::sync::Arc::new(func_code)));
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
        } else {
            format!("{}.{}", qualname_prefix, name)
        };

        let mut class_unit =
            CompileUnit::new(name, &self.filename, child_scope, false, qualname.clone());
        class_unit.code.flags = CodeFlags::empty();
        // The class body function takes __locals__ as implicit first arg
        class_unit.code.arg_count = 0;
        class_unit.class_name = Some(name.to_string());
        class_unit.code.first_line_number = location.line;
        self.unit_stack.push(class_unit);

        // __name__ = qualname
        let qname_idx = self.add_const(ConstantValue::Str(qualname.clone().into()));
        self.emit_arg(Opcode::LoadConst, qname_idx);
        self.store_name("__qualname__");

        // Setup annotations dict for the class body
        self.emit_op(Opcode::SetupAnnotations);

        // Extract docstring from first statement if it's a string literal
        if let Some(first) = body.first() {
            if let StatementKind::Expr { value } = &first.node {
                if let ExpressionKind::Constant { value: Constant::Str(doc) } = &value.node {
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
        let code_idx = self.add_const(ConstantValue::Code(std::sync::Arc::new(class_code)));
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

            if alias.asname.is_some() && alias.name.contains('.') {
                // `import a.b.c as X` — ImportName with None fromlist returns
                // top-level `a`, then walk IMPORT_FROM chain to reach `a.b.c`.
                let parts: Vec<&str> = alias.name.split('.').collect();
                let none_idx = self.add_const(ConstantValue::None);
                self.emit_arg(Opcode::LoadConst, none_idx);

                let name_idx = self.add_name(&alias.name);
                self.emit_arg(Opcode::ImportName, name_idx);

                // Walk IMPORT_FROM for parts[1..] to reach the deepest submodule
                // Stack after ImportName: [a]
                // Each iteration: IMPORT_FROM x -> [parent, x], RotTwo -> [x, parent], PopTop -> [x]
                for part in &parts[1..] {
                    let from_idx = self.add_name(part);
                    self.emit_arg(Opcode::ImportFrom, from_idx);
                    self.emit_op(Opcode::RotTwo);
                    self.emit_op(Opcode::PopTop);
                }

                self.store_name(alias.asname.as_ref().unwrap());
            } else {
                // Regular import: `import a.b.c` stores `a`, `import foo` stores `foo`
                let none_idx = self.add_const(ConstantValue::None);
                self.emit_arg(Opcode::LoadConst, none_idx);

                let name_idx = self.add_name(&alias.name);
                self.emit_arg(Opcode::ImportName, name_idx);

                if let Some(ref asname) = alias.asname {
                    self.store_name(asname);
                } else {
                    let top = alias.name.split('.').next().unwrap_or(&alias.name);
                    if alias.name.contains('.') {
                        self.store_name(top);
                    } else {
                        self.store_name(&alias.name);
                    }
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

                // Compile handler body BEFORE PopExcept (matches CPython order)
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
                self.compile_body(&handler.body)?;

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

        self.current_unit_mut().loop_stack.push(LoopContext {
            continue_target: loop_start,
            break_labels: Vec::new(),
            is_for_loop: true,
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
        self.compile_async_with_item(items, idx + 1, body)?;

        // Normal exit: pop block, then run cleanup
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
                let mangled = self.mangle_name(attr);
                let attr_idx = self.add_name(&mangled);
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
                let mangled = self.mangle_name(attr);
                let attr_idx = self.add_name(&mangled);
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
                let mangled = self.mangle_name(attr);
                let attr_idx = self.add_name(&mangled);
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

    // ── match/case compilation (Python 3.10+) ───────────────────────
    //
    // Strategy: compile match/case into equivalent if-elif chain.
    // The subject is evaluated once and stored in a temp variable.
    // Each case pattern is compiled to a test expression.

    pub(super) fn compile_match(
        &mut self,
        subject: &Expression,
        cases: &[MatchCase],
    ) -> Result<()> {
        // Evaluate subject and store in a temp variable
        let temp = CompactString::from("$match_subject$");
        let temp_idx = self.varname_index(&temp);
        self.compile_expression(subject)?;
        self.emit_arg(Opcode::StoreFast, temp_idx);

        let mut end_labels = Vec::new();

        for case in cases {
            // Compile pattern test
            let is_wildcard = self.compile_pattern_test(&case.pattern, temp_idx)?;

            let skip_label = if !is_wildcard {
                Some(self.emit_jump(Opcode::PopJumpIfFalse))
            } else {
                None
            };

            // Bind pattern captures BEFORE guard (guard may reference captured names)
            self.compile_pattern_bindings(&case.pattern, temp_idx)?;

            // Compile guard if present
            let guard_label = if let Some(guard) = &case.guard {
                self.compile_expression(guard)?;
                Some(self.emit_jump(Opcode::PopJumpIfFalse))
            } else {
                None
            };

            // Compile body
            self.compile_body(&case.body)?;

            // Jump to end of match
            end_labels.push(self.emit_jump(Opcode::JumpForward));

            // Patch skip labels
            if let Some(label) = guard_label {
                self.patch_jump_here(label);
            }
            if let Some(label) = skip_label {
                self.patch_jump_here(label);
            }
        }

        // Patch all end labels
        for label in end_labels {
            self.patch_jump_here(label);
        }

        Ok(())
    }

    /// Compile a pattern test — pushes True/False on stack.
    /// Returns true if the pattern always matches (wildcard/capture).
    fn compile_pattern_test(
        &mut self,
        pattern: &Pattern,
        subject_idx: u32,
    ) -> Result<bool> {
        match pattern {
            Pattern::MatchWildcard => {
                // Always matches — push True
                let idx = self.add_const(ConstantValue::Bool(true));
                self.emit_arg(Opcode::LoadConst, idx);
                Ok(true)
            }
            Pattern::MatchCapture { .. } => {
                // Always matches — push True (binding happens later)
                let idx = self.add_const(ConstantValue::Bool(true));
                self.emit_arg(Opcode::LoadConst, idx);
                Ok(true)
            }
            Pattern::MatchLiteral { value } | Pattern::MatchValue { value } => {
                // subject == value
                self.emit_arg(Opcode::LoadFast, subject_idx);
                self.compile_expression(value)?;
                self.emit_arg(Opcode::CompareOp, 2); // == operator
                Ok(false)
            }
            Pattern::MatchSequence { patterns } => {
                self.compile_sequence_pattern_test(patterns, subject_idx)
            }
            Pattern::MatchMapping { keys, patterns, .. } => {
                self.compile_mapping_pattern_test(keys, patterns, subject_idx)
            }
            Pattern::MatchClass { cls, patterns, kwd_attrs, kwd_patterns } => {
                self.compile_class_pattern_test(cls, patterns, kwd_attrs, kwd_patterns, subject_idx)
            }
            Pattern::MatchOr { patterns } => {
                // Any pattern can match
                let mut end_labels = Vec::new();
                for (i, pat) in patterns.iter().enumerate() {
                    let is_wc = self.compile_pattern_test(pat, subject_idx)?;
                    if is_wc {
                        // Always true — skip the rest
                        if i < patterns.len() - 1 {
                            // Pop remaining patterns
                        }
                        return Ok(true);
                    }
                    if i < patterns.len() - 1 {
                        // If this one matched, jump to end with True
                        let dup_label = self.emit_jump(Opcode::PopJumpIfTrue);
                        end_labels.push(dup_label);
                    }
                }
                // Last pattern result is on stack
                let done = self.emit_jump(Opcode::JumpForward);
                // Patch PopJumpIfTrue labels — they jump here with True already popped
                // Need to push True
                for label in &end_labels {
                    self.patch_jump_here(*label);
                }
                if !end_labels.is_empty() {
                    let idx = self.add_const(ConstantValue::Bool(true));
                    self.emit_arg(Opcode::LoadConst, idx);
                }
                self.patch_jump_here(done);
                Ok(false)
            }
            Pattern::MatchAs { pattern, .. } => {
                // Test the inner pattern (binding happens in compile_pattern_bindings)
                if let Some(inner) = pattern {
                    self.compile_pattern_test(inner, subject_idx)
                } else {
                    // `as name` with no inner pattern = wildcard
                    let idx = self.add_const(ConstantValue::Bool(true));
                    self.emit_arg(Opcode::LoadConst, idx);
                    Ok(true)
                }
            }
            Pattern::MatchStar { .. } => {
                // Star patterns only valid inside sequences — shouldn't hit here at top level
                let idx = self.add_const(ConstantValue::Bool(true));
                self.emit_arg(Opcode::LoadConst, idx);
                Ok(true)
            }
        }
    }

    fn compile_sequence_pattern_test(
        &mut self,
        patterns: &[Pattern],
        subject_idx: u32,
    ) -> Result<bool> {
        // isinstance(subject, (list, tuple))
        // Build: isinstance(subject, (list, tuple))
        self.load_name("isinstance");
        self.emit_arg(Opcode::LoadFast, subject_idx);
        self.load_name("list");
        self.load_name("tuple");
        self.emit_arg(Opcode::BuildTuple, 2);
        self.emit_arg(Opcode::CallFunction, 2);

        let fail_label = self.emit_jump(Opcode::PopJumpIfFalse);

        // Check len
        let has_star = patterns.iter().any(|p| matches!(p, Pattern::MatchStar { .. }));
        let fixed_count = patterns.iter().filter(|p| !matches!(p, Pattern::MatchStar { .. })).count();

        self.load_name("len");
        self.emit_arg(Opcode::LoadFast, subject_idx);
        self.emit_arg(Opcode::CallFunction, 1);

        let count_idx = self.add_const(ConstantValue::Integer(fixed_count as i64));
        self.emit_arg(Opcode::LoadConst, count_idx);
        if has_star {
            self.emit_arg(Opcode::CompareOp, 5); // >=
        } else {
            self.emit_arg(Opcode::CompareOp, 2); // ==
        }

        let len_fail = self.emit_jump(Opcode::PopJumpIfFalse);

        // Check each element pattern
        let mut elem_fails = Vec::new();
        let mut elem_idx = 0u32;
        for pat in patterns {
            if matches!(pat, Pattern::MatchStar { .. }) {
                // Skip star patterns in testing — they match the rest
                continue;
            }
            // Create temp for element
            let elem_temp = CompactString::from(format!("$match_elem_{}$", elem_idx));
            let elem_temp_idx = self.varname_index(&elem_temp);
            self.emit_arg(Opcode::LoadFast, subject_idx);
            let idx_const = self.add_const(ConstantValue::Integer(elem_idx as i64));
            self.emit_arg(Opcode::LoadConst, idx_const);
            self.emit_op(Opcode::BinarySubscr);
            self.emit_arg(Opcode::StoreFast, elem_temp_idx);

            let is_wc = self.compile_pattern_test(pat, elem_temp_idx)?;
            if !is_wc {
                elem_fails.push(self.emit_jump(Opcode::PopJumpIfFalse));
            } else {
                self.emit_op(Opcode::PopTop);
            }
            elem_idx += 1;
        }

        // All passed — push True
        let true_idx = self.add_const(ConstantValue::Bool(true));
        self.emit_arg(Opcode::LoadConst, true_idx);
        let done = self.emit_jump(Opcode::JumpForward);

        // Failure paths
        self.patch_jump_here(fail_label);
        self.patch_jump_here(len_fail);
        for label in elem_fails {
            self.patch_jump_here(label);
        }
        let false_idx = self.add_const(ConstantValue::Bool(false));
        self.emit_arg(Opcode::LoadConst, false_idx);
        self.patch_jump_here(done);

        Ok(false)
    }

    fn compile_mapping_pattern_test(
        &mut self,
        keys: &[Expression],
        patterns: &[Pattern],
        subject_idx: u32,
    ) -> Result<bool> {
        // isinstance(subject, dict)
        self.load_name("isinstance");
        self.emit_arg(Opcode::LoadFast, subject_idx);
        self.load_name("dict");
        self.emit_arg(Opcode::CallFunction, 2);

        let fail_label = self.emit_jump(Opcode::PopJumpIfFalse);

        // Check each key exists and value matches
        let mut kv_fails = Vec::new();
        for (i, (key, pat)) in keys.iter().zip(patterns.iter()).enumerate() {
            // Check key in subject
            self.compile_expression(key)?;
            self.emit_arg(Opcode::LoadFast, subject_idx);
            self.emit_arg(Opcode::CompareOp, 6); // in

            kv_fails.push(self.emit_jump(Opcode::PopJumpIfFalse));

            // Get value, store temp, test pattern
            let val_temp = CompactString::from(format!("$match_val_{}$", i));
            let val_temp_idx = self.varname_index(&val_temp);
            self.emit_arg(Opcode::LoadFast, subject_idx);
            self.compile_expression(key)?;
            self.emit_op(Opcode::BinarySubscr);
            self.emit_arg(Opcode::StoreFast, val_temp_idx);

            let is_wc = self.compile_pattern_test(pat, val_temp_idx)?;
            if !is_wc {
                kv_fails.push(self.emit_jump(Opcode::PopJumpIfFalse));
            } else {
                self.emit_op(Opcode::PopTop);
            }
        }

        let true_idx = self.add_const(ConstantValue::Bool(true));
        self.emit_arg(Opcode::LoadConst, true_idx);
        let done = self.emit_jump(Opcode::JumpForward);

        self.patch_jump_here(fail_label);
        for label in kv_fails {
            self.patch_jump_here(label);
        }
        let false_idx = self.add_const(ConstantValue::Bool(false));
        self.emit_arg(Opcode::LoadConst, false_idx);
        self.patch_jump_here(done);

        Ok(false)
    }

    fn compile_class_pattern_test(
        &mut self,
        cls: &Expression,
        patterns: &[Pattern],
        kwd_attrs: &[CompactString],
        kwd_patterns: &[Pattern],
        subject_idx: u32,
    ) -> Result<bool> {
        // isinstance(subject, cls)
        self.load_name("isinstance");
        self.emit_arg(Opcode::LoadFast, subject_idx);
        self.compile_expression(cls)?;
        self.emit_arg(Opcode::CallFunction, 2);

        if patterns.is_empty() && kwd_attrs.is_empty() {
            return Ok(false);
        }

        let fail_label = self.emit_jump(Opcode::PopJumpIfFalse);

        let mut attr_fails = Vec::new();

        // Positional patterns: for builtin types (int, str, float, bytes, bool),
        // the single positional arg captures the subject value itself.
        // For user classes, positional args map via __match_args__.
        for (i, pat) in patterns.iter().enumerate() {
            if matches!(pat, Pattern::MatchWildcard) {
                continue;
            }
            let pos_temp = CompactString::from(format!("$match_pos_{}$", i));
            let pos_temp_idx = self.varname_index(&pos_temp);
            // For single positional arg on builtin types, bind to subject directly
            self.emit_arg(Opcode::LoadFast, subject_idx);
            self.emit_arg(Opcode::StoreFast, pos_temp_idx);

            let is_wc = self.compile_pattern_test(pat, pos_temp_idx)?;
            if !is_wc {
                attr_fails.push(self.emit_jump(Opcode::PopJumpIfFalse));
            } else {
                self.emit_op(Opcode::PopTop);
            }
        }

        // Keyword attribute patterns
        for (attr, pat) in kwd_attrs.iter().zip(kwd_patterns.iter()) {
            let attr_temp = CompactString::from(format!("$match_attr_{}$", attr));
            let attr_temp_idx = self.varname_index(&attr_temp);

            self.emit_arg(Opcode::LoadFast, subject_idx);
            let attr_name = self.add_name(attr);
            self.emit_arg(Opcode::LoadAttr, attr_name);
            self.emit_arg(Opcode::StoreFast, attr_temp_idx);

            let is_wc = self.compile_pattern_test(pat, attr_temp_idx)?;
            if !is_wc {
                attr_fails.push(self.emit_jump(Opcode::PopJumpIfFalse));
            } else {
                self.emit_op(Opcode::PopTop);
            }
        }

        let true_idx = self.add_const(ConstantValue::Bool(true));
        self.emit_arg(Opcode::LoadConst, true_idx);
        let done = self.emit_jump(Opcode::JumpForward);

        self.patch_jump_here(fail_label);
        for label in attr_fails {
            self.patch_jump_here(label);
        }
        let false_idx = self.add_const(ConstantValue::Bool(false));
        self.emit_arg(Opcode::LoadConst, false_idx);
        self.patch_jump_here(done);

        Ok(false)
    }

    /// Compile pattern bindings — store captured names after a successful match.
    fn compile_pattern_bindings(
        &mut self,
        pattern: &Pattern,
        subject_idx: u32,
    ) -> Result<()> {
        match pattern {
            Pattern::MatchCapture { name } => {
                self.emit_arg(Opcode::LoadFast, subject_idx);
                self.store_name(name);
            }
            Pattern::MatchAs { pattern, name } => {
                if let Some(inner) = pattern {
                    self.compile_pattern_bindings(inner, subject_idx)?;
                }
                if let Some(name) = name {
                    self.emit_arg(Opcode::LoadFast, subject_idx);
                    self.store_name(name);
                }
            }
            Pattern::MatchOr { patterns } => {
                // Bind from the first pattern that could match (simplified)
                if let Some(first) = patterns.first() {
                    self.compile_pattern_bindings(first, subject_idx)?;
                }
            }
            Pattern::MatchSequence { patterns } => {
                let mut elem_idx = 0u32;
                for pat in patterns {
                    if matches!(pat, Pattern::MatchStar { .. }) {
                        continue;
                    }
                    let elem_temp = CompactString::from(format!("$match_elem_{}$", elem_idx));
                    let elem_temp_idx = self.varname_index(&elem_temp);
                    self.compile_pattern_bindings(pat, elem_temp_idx)?;
                    elem_idx += 1;
                }
            }
            Pattern::MatchMapping { keys, patterns, rest } => {
                for (i, pat) in patterns.iter().enumerate() {
                    let val_temp = CompactString::from(format!("$match_val_{}$", i));
                    let val_temp_idx = self.varname_index(&val_temp);
                    self.compile_pattern_bindings(pat, val_temp_idx)?;
                }
                if let Some(rest_name) = rest {
                    // Bind remaining dict items — simplified: bind entire subject
                    self.emit_arg(Opcode::LoadFast, subject_idx);
                    self.store_name(rest_name);
                }
            }
            Pattern::MatchClass { kwd_attrs, kwd_patterns, patterns, .. } => {
                // Positional pattern bindings: bind subject to captured names
                for (i, pat) in patterns.iter().enumerate() {
                    let pos_temp = CompactString::from(format!("$match_pos_{}$", i));
                    let pos_temp_idx = self.varname_index(&pos_temp);
                    self.compile_pattern_bindings(pat, pos_temp_idx)?;
                }
                // Keyword attribute bindings
                for (attr, pat) in kwd_attrs.iter().zip(kwd_patterns.iter()) {
                    let attr_temp = CompactString::from(format!("$match_attr_{}$", attr));
                    let attr_temp_idx = self.varname_index(&attr_temp);
                    self.compile_pattern_bindings(pat, attr_temp_idx)?;
                }
            }
            Pattern::MatchWildcard | Pattern::MatchLiteral { .. }
            | Pattern::MatchValue { .. } | Pattern::MatchStar { .. } => {}
        }
        Ok(())
    }

}
