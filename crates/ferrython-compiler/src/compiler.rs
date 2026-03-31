//! Main compiler: walks the AST and emits bytecode into `CodeObject`s.

use compact_str::CompactString;
use ferrython_ast::*;
use ferrython_bytecode::{CodeFlags, CodeObject, ConstantValue, Instruction, Opcode};
use rustc_hash::FxHashSet;

use crate::error::CompileError;
use crate::symbol_table::{self, Scope, ScopeType, SymbolScope};

type Result<T> = std::result::Result<T, CompileError>;

/// Label index used for forward-jump patching.
#[derive(Debug, Clone, Copy)]
struct Label(u32);

/// Tracks a loop context for break/continue.
#[derive(Debug, Clone)]
struct LoopContext {
    /// Offset where `continue` should jump (loop header).
    continue_target: u32,
    /// Labels that need patching when the loop ends (break targets).
    break_labels: Vec<Label>,
}

/// Compile state for a single scope (module, function, class, comprehension).
struct CompileUnit {
    code: CodeObject,
    scope: Scope,
    /// Stack of active loops for break/continue resolution.
    loop_stack: Vec<LoopContext>,
    /// Whether this scope is a function body.
    is_function: bool,
    /// Qualname prefix for nested scopes.
    qualname_prefix: String,
    /// Index of the next child scope to consume.
    child_scope_idx: usize,
}

impl CompileUnit {
    fn new(
        name: &str,
        filename: &str,
        scope: Scope,
        is_function: bool,
        qualname_prefix: String,
    ) -> Self {
        let mut code = CodeObject::new(name, filename);
        // Module scope should not have OPTIMIZED | NEWLOCALS
        if scope.scope_type == ScopeType::Module {
            code.flags = CodeFlags::empty();
        } else {
            code.flags = CodeFlags::OPTIMIZED | CodeFlags::NEWLOCALS;
        }
        // Populate cellvars and freevars from scope analysis
        code.cellvars = scope.cell_names().iter().map(|s| CompactString::from(*s)).collect();
        code.freevars = scope.free_names().iter().map(|s| CompactString::from(*s)).collect();
        if !code.freevars.is_empty() {
            code.flags |= CodeFlags::NESTED;
        }
        Self {
            code,
            scope,
            loop_stack: Vec::new(),
            is_function,
            qualname_prefix,
            child_scope_idx: 0,
        }
    }

    /// Take the next child scope from the symbol table.
    fn take_child_scope(&mut self) -> Scope {
        let idx = self.child_scope_idx;
        self.child_scope_idx += 1;
        self.scope.children[idx].clone()
    }
}

/// The Ferrython compiler.
pub struct Compiler {
    filename: String,
    /// Stack of compile units; the top is the current scope being compiled.
    unit_stack: Vec<CompileUnit>,
}

impl Compiler {
    pub fn new(filename: impl Into<String>) -> Self {
        Self {
            filename: filename.into(),
            unit_stack: Vec::new(),
        }
    }

    // ── scope helpers ───────────────────────────────────────────────

    fn current_unit(&self) -> &CompileUnit {
        self.unit_stack.last().expect("no compile unit on stack")
    }

    fn current_unit_mut(&mut self) -> &mut CompileUnit {
        self.unit_stack.last_mut().expect("no compile unit on stack")
    }

    #[allow(dead_code)]
    fn code(&mut self) -> &mut CodeObject {
        &mut self.current_unit_mut().code
    }

    fn emit(&mut self, instr: Instruction) -> u32 {
        self.current_unit_mut().code.emit(instr)
    }

    fn emit_op(&mut self, op: Opcode) -> u32 {
        self.emit(Instruction::simple(op))
    }

    fn emit_arg(&mut self, op: Opcode, arg: u32) -> u32 {
        self.emit(Instruction::new(op, arg))
    }

    fn add_const(&mut self, value: ConstantValue) -> u32 {
        self.current_unit_mut().code.add_const(value)
    }

    fn add_name(&mut self, name: &str) -> u32 {
        self.current_unit_mut().code.add_name(name)
    }

    fn current_offset(&self) -> u32 {
        self.current_unit().code.current_offset()
    }

    // ── label / jump helpers ────────────────────────────────────────

    /// Emit a placeholder jump instruction; returns a Label to be patched later.
    fn emit_jump(&mut self, op: Opcode) -> Label {
        let idx = self.emit_arg(op, 0); // placeholder
        Label(idx)
    }

    /// Patch a previously emitted jump to point at `target`.
    fn patch_jump(&mut self, label: Label, target: u32) {
        self.current_unit_mut().code.instructions[label.0 as usize].arg = target;
    }

    /// Patch a jump to point at the current offset.
    fn patch_jump_here(&mut self, label: Label) {
        let offset = self.current_offset();
        self.patch_jump(label, offset);
    }

    // ── name resolution ─────────────────────────────────────────────

    #[allow(dead_code)]
    fn is_module_scope(&self) -> bool {
        self.current_unit().scope.scope_type == ScopeType::Module
    }

    fn is_function_scope(&self) -> bool {
        self.current_unit().is_function
    }

    #[allow(dead_code)]
    fn globals(&self) -> FxHashSet<&str> {
        self.current_unit().scope.global_names()
    }

    fn is_local(&self, name: &str) -> bool {
        if let Some(sym) = self.current_unit().scope.lookup(name) {
            sym.scope == SymbolScope::Local
        } else {
            false
        }
    }

    fn is_cell(&self, name: &str) -> bool {
        if let Some(sym) = self.current_unit().scope.lookup(name) {
            sym.scope == SymbolScope::Cell
        } else {
            false
        }
    }

    fn is_free(&self, name: &str) -> bool {
        if let Some(sym) = self.current_unit().scope.lookup(name) {
            sym.scope == SymbolScope::Free
        } else {
            false
        }
    }

    fn is_global(&self, name: &str) -> bool {
        if let Some(sym) = self.current_unit().scope.lookup(name) {
            sym.scope == SymbolScope::Global
        } else {
            false
        }
    }

    /// Find fast-local index for a name, adding it to varnames if needed.
    fn varname_index(&mut self, name: &str) -> u32 {
        let varnames = &self.current_unit().code.varnames;
        for (i, v) in varnames.iter().enumerate() {
            if v.as_str() == name {
                return i as u32;
            }
        }
        let idx = varnames.len() as u32;
        self.current_unit_mut()
            .code
            .varnames
            .push(name.into());
        idx
    }

    /// Get the DEREF index for a cell or free variable.
    /// Cell vars come first (indices 0..cellvars.len()), then free vars.
    fn deref_index(&mut self, name: &str) -> u32 {
        let code = &self.current_unit().code;
        // Check cellvars first
        for (i, v) in code.cellvars.iter().enumerate() {
            if v.as_str() == name {
                return i as u32;
            }
        }
        // Then freevars (offset by cellvars.len())
        let offset = code.cellvars.len();
        for (i, v) in code.freevars.iter().enumerate() {
            if v.as_str() == name {
                return (offset + i) as u32;
            }
        }
        // Should not happen if symbol table is correct, but add it as freevar
        let idx = (offset + code.freevars.len()) as u32;
        self.current_unit_mut().code.freevars.push(name.into());
        idx
    }

    /// Emit the correct LOAD instruction for a name.
    fn load_name(&mut self, name: &str) {
        if self.is_function_scope() {
            if self.is_cell(name) || self.is_free(name) {
                let idx = self.deref_index(name);
                self.emit_arg(Opcode::LoadDeref, idx);
            } else if self.is_global(name) {
                let idx = self.add_name(name);
                self.emit_arg(Opcode::LoadGlobal, idx);
            } else if self.is_local(name) {
                let idx = self.varname_index(name);
                self.emit_arg(Opcode::LoadFast, idx);
            } else {
                let idx = self.add_name(name);
                self.emit_arg(Opcode::LoadGlobal, idx);
            }
        } else {
            let idx = self.add_name(name);
            self.emit_arg(Opcode::LoadName, idx);
        }
    }

    /// Emit the correct STORE instruction for a name.
    fn store_name(&mut self, name: &str) {
        if self.is_function_scope() {
            if self.is_cell(name) || self.is_free(name) {
                let idx = self.deref_index(name);
                self.emit_arg(Opcode::StoreDeref, idx);
            } else if self.is_global(name) {
                let idx = self.add_name(name);
                self.emit_arg(Opcode::StoreGlobal, idx);
            } else {
                let idx = self.varname_index(name);
                self.emit_arg(Opcode::StoreFast, idx);
            }
        } else {
            let idx = self.add_name(name);
            self.emit_arg(Opcode::StoreName, idx);
        }
    }

    /// Emit the correct DELETE instruction for a name.
    fn delete_name(&mut self, name: &str) {
        if self.is_function_scope() {
            if self.is_global(name) {
                let idx = self.add_name(name);
                self.emit_arg(Opcode::DeleteGlobal, idx);
            } else {
                let idx = self.varname_index(name);
                self.emit_arg(Opcode::DeleteFast, idx);
            }
        } else {
            let idx = self.add_name(name);
            self.emit_arg(Opcode::DeleteName, idx);
        }
    }

    // ── public entry point ──────────────────────────────────────────

    /// Compile an entire module AST into a `CodeObject`.
    pub fn compile_module(&mut self, module: &Module) -> Result<CodeObject> {
        let symtable = symbol_table::analyze(module);
        let unit = CompileUnit::new(
            "<module>",
            &self.filename,
            symtable.top,
            false,
            String::new(),
        );
        self.unit_stack.push(unit);

        match module {
            Module::Module { body, .. } | Module::Interactive { body } => {
                self.compile_body(body)?;
            }
            Module::Expression { body } => {
                self.compile_expression(body)?;
                self.emit_op(Opcode::ReturnValue);
            }
        }

        // Module always ends with LOAD_CONST None + RETURN_VALUE
        if !matches!(module, Module::Expression { .. }) {
            let none_idx = self.add_const(ConstantValue::None);
            self.emit_arg(Opcode::LoadConst, none_idx);
            self.emit_op(Opcode::ReturnValue);
        }

        let unit = self.unit_stack.pop().unwrap();
        let mut code = unit.code;
        code.num_locals = code.varnames.len() as u32;
        Ok(code)
    }

    // ── statement compilation ───────────────────────────────────────

    fn compile_body(&mut self, stmts: &[Statement]) -> Result<()> {
        for stmt in stmts {
            self.compile_statement(stmt)?;
        }
        Ok(())
    }

    fn compile_statement(&mut self, stmt: &Statement) -> Result<()> {
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
                self.emit_op(Opcode::PopTop);
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

            StatementKind::AnnAssign { target, value, .. } => {
                if let Some(val) = value {
                    self.compile_expression(val)?;
                    self.compile_store_target(target)?;
                }
                // annotation itself is not compiled at runtime (just for type checkers)
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

    fn compile_if(
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

    fn compile_while(
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

    fn compile_for(
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

    fn compile_function_def(
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

    fn push_function_unit(
        &mut self,
        name: &str,
        scope: Scope,
        qualname: &str,
    ) -> Result<()> {
        let unit = CompileUnit::new(name, &self.filename, scope, true, qualname.to_string());
        self.unit_stack.push(unit);
        Ok(())
    }

    fn pop_function_unit(&mut self) -> CodeObject {
        let unit = self.unit_stack.pop().unwrap();
        let mut code = unit.code;
        code.num_locals = code.varnames.len() as u32;
        code
    }

    // ── class definition ────────────────────────────────────────────

    fn compile_class_def(
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

        // Compile the class body
        self.compile_body(body)?;

        // Return None from the class body
        let none_idx = self.add_const(ConstantValue::None);
        self.emit_arg(Opcode::LoadConst, none_idx);
        self.emit_op(Opcode::ReturnValue);

        let class_code = self.pop_function_unit();

        // Load the class body code object
        let code_idx = self.add_const(ConstantValue::Code(Box::new(class_code)));
        self.emit_arg(Opcode::LoadConst, code_idx);

        // Load qualname for MAKE_FUNCTION
        let qname_const = self.add_const(ConstantValue::Str(qualname.into()));
        self.emit_arg(Opcode::LoadConst, qname_const);

        // MAKE_FUNCTION (no defaults/closures for class body)
        self.emit_arg(Opcode::MakeFunction, 0);

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

    fn compile_import(&mut self, names: &[Alias]) -> Result<()> {
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

    fn compile_import_from(
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

    fn compile_raise(
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

    fn compile_try(
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
            Some(self.emit_jump(Opcode::SetupFinally))
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

    fn compile_assert(
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

    fn compile_with(
        &mut self,
        items: &[WithItem],
        body: &[Statement],
    ) -> Result<()> {
        // Nested withs: `with a, b:` is equivalent to `with a: with b:`
        self.compile_with_item(items, 0, body)
    }

    fn compile_with_item(
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

    fn compile_aug_assign(
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

    fn emit_inplace_op(&mut self, op: Operator) {
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

    fn compile_store_target(&mut self, target: &Expression) -> Result<()> {
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

    fn compile_delete_target(&mut self, target: &Expression) -> Result<()> {
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

    // ── expression compilation ──────────────────────────────────────

    fn compile_expression(&mut self, expr: &Expression) -> Result<()> {
        match &expr.node {
            ExpressionKind::Constant { value } => {
                let cv = self.constant_to_value(value);
                let idx = self.add_const(cv);
                self.emit_arg(Opcode::LoadConst, idx);
            }

            ExpressionKind::Name { id, ctx } => {
                match ctx {
                    ExprContext::Load => self.load_name(id),
                    ExprContext::Store => self.store_name(id),
                    ExprContext::Del => self.delete_name(id),
                }
            }

            ExpressionKind::BinOp { left, op, right } => {
                self.compile_expression(left)?;
                self.compile_expression(right)?;
                self.emit_binary_op(*op);
            }

            ExpressionKind::UnaryOp { op, operand } => {
                self.compile_expression(operand)?;
                let opcode = match op {
                    UnaryOperator::Invert => Opcode::UnaryInvert,
                    UnaryOperator::Not => Opcode::UnaryNot,
                    UnaryOperator::UAdd => Opcode::UnaryPositive,
                    UnaryOperator::USub => Opcode::UnaryNegative,
                };
                self.emit_op(opcode);
            }

            ExpressionKind::BoolOp { op, values } => {
                self.compile_bool_op(*op, values)?;
            }

            ExpressionKind::Compare {
                left,
                ops,
                comparators,
            } => {
                self.compile_compare(left, ops, comparators)?;
            }

            ExpressionKind::Call {
                func,
                args,
                keywords,
            } => {
                self.compile_call(func, args, keywords)?;
            }

            ExpressionKind::Attribute { value, attr, ctx } => {
                match ctx {
                    ExprContext::Load => {
                        self.compile_expression(value)?;
                        let attr_idx = self.add_name(attr);
                        self.emit_arg(Opcode::LoadAttr, attr_idx);
                    }
                    ExprContext::Store => {
                        self.compile_expression(value)?;
                        let attr_idx = self.add_name(attr);
                        self.emit_arg(Opcode::StoreAttr, attr_idx);
                    }
                    ExprContext::Del => {
                        self.compile_expression(value)?;
                        let attr_idx = self.add_name(attr);
                        self.emit_arg(Opcode::DeleteAttr, attr_idx);
                    }
                }
            }

            ExpressionKind::Subscript { value, slice, ctx } => {
                match ctx {
                    ExprContext::Load => {
                        self.compile_expression(value)?;
                        self.compile_expression(slice)?;
                        self.emit_op(Opcode::BinarySubscr);
                    }
                    ExprContext::Store => {
                        self.compile_expression(value)?;
                        self.compile_expression(slice)?;
                        self.emit_op(Opcode::StoreSubscr);
                    }
                    ExprContext::Del => {
                        self.compile_expression(value)?;
                        self.compile_expression(slice)?;
                        self.emit_op(Opcode::DeleteSubscr);
                    }
                }
            }

            ExpressionKind::List { elts, ctx } => {
                match ctx {
                    ExprContext::Load => {
                        for elt in elts {
                            self.compile_expression(elt)?;
                        }
                        self.emit_arg(Opcode::BuildList, elts.len() as u32);
                    }
                    _ => {
                        // Store/Del contexts handled by compile_store_target
                    }
                }
            }

            ExpressionKind::Tuple { elts, ctx } => {
                match ctx {
                    ExprContext::Load => {
                        for elt in elts {
                            self.compile_expression(elt)?;
                        }
                        self.emit_arg(Opcode::BuildTuple, elts.len() as u32);
                    }
                    _ => {
                        // Store/Del contexts handled by compile_store_target
                    }
                }
            }

            ExpressionKind::Set { elts } => {
                for elt in elts {
                    self.compile_expression(elt)?;
                }
                self.emit_arg(Opcode::BuildSet, elts.len() as u32);
            }

            ExpressionKind::Dict { keys, values } => {
                // Check for dictionary unpacking (None keys indicate **)
                let has_unpacking = keys.iter().any(|k| k.is_none());
                if has_unpacking {
                    // Build dict in segments
                    let mut n_regular = 0u32;
                    let mut n_segments = 0u32;
                    for (key, val) in keys.iter().zip(values.iter()) {
                        if let Some(k) = key {
                            self.compile_expression(k)?;
                            self.compile_expression(val)?;
                            n_regular += 1;
                        } else {
                            // Flush regular pairs
                            if n_regular > 0 {
                                self.emit_arg(Opcode::BuildMap, n_regular);
                                n_regular = 0;
                                n_segments += 1;
                            }
                            // Compile the unpacked dict
                            self.compile_expression(val)?;
                            n_segments += 1;
                        }
                    }
                    if n_regular > 0 {
                        self.emit_arg(Opcode::BuildMap, n_regular);
                        n_segments += 1;
                    }
                    // Merge all segments
                    if n_segments > 1 {
                        self.emit_arg(Opcode::BuildMap, 0);
                        for _ in 0..n_segments {
                            self.emit_arg(Opcode::DictUpdate, 1);
                        }
                    }
                } else {
                    for (key, val) in keys.iter().zip(values.iter()) {
                        if let Some(k) = key {
                            self.compile_expression(k)?;
                        }
                        self.compile_expression(val)?;
                    }
                    self.emit_arg(Opcode::BuildMap, keys.len() as u32);
                }
            }

            ExpressionKind::IfExp {
                test,
                body,
                orelse,
            } => {
                self.compile_expression(test)?;
                let else_label = self.emit_jump(Opcode::PopJumpIfFalse);
                self.compile_expression(body)?;
                let end_label = self.emit_jump(Opcode::JumpForward);
                self.patch_jump_here(else_label);
                self.compile_expression(orelse)?;
                self.patch_jump_here(end_label);
            }

            ExpressionKind::Lambda { args, body } => {
                self.compile_lambda(args, body)?;
            }

            ExpressionKind::ListComp { elt, generators } => {
                self.compile_comprehension("<listcomp>", elt, None, generators, ComprehensionKind::List)?;
            }

            ExpressionKind::SetComp { elt, generators } => {
                self.compile_comprehension("<setcomp>", elt, None, generators, ComprehensionKind::Set)?;
            }

            ExpressionKind::DictComp {
                key,
                value,
                generators,
            } => {
                self.compile_comprehension("<dictcomp>", key, Some(value), generators, ComprehensionKind::Dict)?;
            }

            ExpressionKind::GeneratorExp { elt, generators } => {
                self.compile_comprehension("<genexpr>", elt, None, generators, ComprehensionKind::Generator)?;
            }

            ExpressionKind::Yield { value } => {
                if let Some(val) = value {
                    self.compile_expression(val)?;
                } else {
                    let none_idx = self.add_const(ConstantValue::None);
                    self.emit_arg(Opcode::LoadConst, none_idx);
                }
                self.emit_op(Opcode::YieldValue);
            }

            ExpressionKind::YieldFrom { value } => {
                self.compile_expression(value)?;
                self.emit_op(Opcode::GetIter);
                let none_idx = self.add_const(ConstantValue::None);
                self.emit_arg(Opcode::LoadConst, none_idx);
                self.emit_op(Opcode::YieldFrom);
            }

            ExpressionKind::Await { value } => {
                self.compile_expression(value)?;
                self.emit_op(Opcode::GetAwaitable);
                let none_idx = self.add_const(ConstantValue::None);
                self.emit_arg(Opcode::LoadConst, none_idx);
                self.emit_op(Opcode::YieldFrom);
            }

            ExpressionKind::Starred { value, .. } => {
                // In expression context (e.g., [*a, b]), starred should be handled
                // by the parent (List, Call, etc.). If we reach here, just compile normally.
                self.compile_expression(value)?;
            }

            ExpressionKind::Slice {
                lower,
                upper,
                step,
            } => {
                let argc = if step.is_some() { 3 } else { 2 };
                if let Some(l) = lower {
                    self.compile_expression(l)?;
                } else {
                    let none_idx = self.add_const(ConstantValue::None);
                    self.emit_arg(Opcode::LoadConst, none_idx);
                }
                if let Some(u) = upper {
                    self.compile_expression(u)?;
                } else {
                    let none_idx = self.add_const(ConstantValue::None);
                    self.emit_arg(Opcode::LoadConst, none_idx);
                }
                if let Some(s) = step {
                    self.compile_expression(s)?;
                }
                self.emit_arg(Opcode::BuildSlice, argc);
            }

            ExpressionKind::NamedExpr { target, value } => {
                self.compile_expression(value)?;
                self.emit_op(Opcode::DupTop);
                self.compile_store_target(target)?;
            }

            ExpressionKind::FormattedValue {
                value,
                conversion,
                format_spec,
            } => {
                self.compile_expression(value)?;
                let mut flags = 0u32;
                // Conversion: -1 = no conversion, 's' = str, 'r' = repr, 'a' = ascii
                match conversion {
                    Some('s') => flags |= 0x01,
                    Some('r') => flags |= 0x02,
                    Some('a') => flags |= 0x03,
                    _ => {}
                }
                if let Some(spec) = format_spec {
                    self.compile_expression(spec)?;
                    flags |= 0x04;
                }
                self.emit_arg(Opcode::FormatValue, flags);
            }

            ExpressionKind::JoinedStr { values } => {
                for v in values {
                    self.compile_expression(v)?;
                }
                self.emit_arg(Opcode::BuildString, values.len() as u32);
            }
        }
        Ok(())
    }

    // ── binary operator emission ────────────────────────────────────

    fn emit_binary_op(&mut self, op: Operator) {
        let opcode = match op {
            Operator::Add => Opcode::BinaryAdd,
            Operator::Sub => Opcode::BinarySubtract,
            Operator::Mult => Opcode::BinaryMultiply,
            Operator::MatMult => Opcode::BinaryMatrixMultiply,
            Operator::Div => Opcode::BinaryTrueDivide,
            Operator::FloorDiv => Opcode::BinaryFloorDivide,
            Operator::Mod => Opcode::BinaryModulo,
            Operator::Pow => Opcode::BinaryPower,
            Operator::LShift => Opcode::BinaryLshift,
            Operator::RShift => Opcode::BinaryRshift,
            Operator::BitOr => Opcode::BinaryOr,
            Operator::BitXor => Opcode::BinaryXor,
            Operator::BitAnd => Opcode::BinaryAnd,
        };
        self.emit_op(opcode);
    }

    // ── boolean operator (short-circuit) ────────────────────────────

    fn compile_bool_op(
        &mut self,
        op: BoolOperator,
        values: &[Expression],
    ) -> Result<()> {
        let jump_op = match op {
            BoolOperator::And => Opcode::JumpIfFalseOrPop,
            BoolOperator::Or => Opcode::JumpIfTrueOrPop,
        };

        let mut labels = Vec::new();
        for (i, val) in values.iter().enumerate() {
            self.compile_expression(val)?;
            if i < values.len() - 1 {
                labels.push(self.emit_jump(jump_op));
            }
        }
        let end = self.current_offset();
        for label in labels {
            self.patch_jump(label, end);
        }
        Ok(())
    }

    // ── comparison (chained) ────────────────────────────────────────

    fn compile_compare(
        &mut self,
        left: &Expression,
        ops: &[CompareOperator],
        comparators: &[Expression],
    ) -> Result<()> {
        self.compile_expression(left)?;

        if ops.len() == 1 {
            // Simple comparison: left op right
            self.compile_expression(&comparators[0])?;
            let cmp_arg = compare_op_arg(ops[0]);
            self.emit_arg(Opcode::CompareOp, cmp_arg);
        } else {
            // Chained: a < b < c → (a < b) and (b < c)
            let mut cleanup_labels = Vec::new();
            for (i, (op, comp)) in ops.iter().zip(comparators.iter()).enumerate() {
                self.compile_expression(comp)?;
                if i < ops.len() - 1 {
                    // Need to keep the intermediate value
                    self.emit_op(Opcode::DupTop);
                    self.emit_op(Opcode::RotThree);
                    let cmp_arg = compare_op_arg(*op);
                    self.emit_arg(Opcode::CompareOp, cmp_arg);
                    cleanup_labels.push(self.emit_jump(Opcode::JumpIfFalseOrPop));
                } else {
                    let cmp_arg = compare_op_arg(*op);
                    self.emit_arg(Opcode::CompareOp, cmp_arg);
                }
            }
            let end = self.current_offset();
            for label in &cleanup_labels {
                // When short-circuiting, we need to clean up the extra value
                self.patch_jump(*label, end);
            }
            // If we short-circuited, RotTwo to get rid of the extra copy
            if !cleanup_labels.is_empty() {
                let ok_label = self.emit_jump(Opcode::JumpForward);
                let cleanup_target = self.current_offset();
                self.emit_op(Opcode::RotTwo);
                self.emit_op(Opcode::PopTop);
                self.patch_jump_here(ok_label);
                // Re-patch cleanup labels to the cleanup target
                for label in cleanup_labels {
                    self.patch_jump(label, cleanup_target);
                }
            }
        }
        Ok(())
    }

    // ── function call ───────────────────────────────────────────────

    fn compile_call(
        &mut self,
        func: &Expression,
        args: &[Expression],
        keywords: &[Keyword],
    ) -> Result<()> {
        // Check if any arg is starred or any keyword has None arg (** unpacking)
        let has_star_args = args.iter().any(|a| matches!(a.node, ExpressionKind::Starred { .. }));
        let has_double_star = keywords.iter().any(|k| k.arg.is_none());

        if has_star_args || has_double_star {
            // Use CALL_FUNCTION_EX
            self.compile_expression(func)?;
            self.compile_star_args(args)?;
            if keywords.is_empty() {
                self.emit_arg(Opcode::CallFunctionEx, 0);
            } else {
                self.compile_star_kwargs(keywords)?;
                self.emit_arg(Opcode::CallFunctionEx, 1);
            }
        } else if !keywords.is_empty() {
            // Use CALL_FUNCTION_KW
            self.compile_expression(func)?;
            for arg in args {
                self.compile_expression(arg)?;
            }
            for kw in keywords {
                self.compile_expression(&kw.value)?;
            }
            let kw_names: Vec<ConstantValue> = keywords
                .iter()
                .map(|k| ConstantValue::Str(k.arg.as_ref().unwrap().clone()))
                .collect();
            let tuple_idx = self.add_const(ConstantValue::Tuple(kw_names));
            self.emit_arg(Opcode::LoadConst, tuple_idx);
            let total = (args.len() + keywords.len()) as u32;
            self.emit_arg(Opcode::CallFunctionKw, total);
        } else {
            // Simple CALL_FUNCTION
            self.compile_expression(func)?;
            for arg in args {
                self.compile_expression(arg)?;
            }
            self.emit_arg(Opcode::CallFunction, args.len() as u32);
        }
        Ok(())
    }

    fn compile_star_args(&mut self, args: &[Expression]) -> Result<()> {
        // Build a tuple from positional args, merging starred elements
        // Build individual segments and use BUILD_TUPLE_UNPACK_WITH_CALL
        // For simplicity, build a list and convert
        let mut segments = 0u32;
        let mut n_regular = 0u32;

        for arg in args {
            if let ExpressionKind::Starred { value, .. } = &arg.node {
                if n_regular > 0 {
                    self.emit_arg(Opcode::BuildTuple, n_regular);
                    segments += 1;
                    n_regular = 0;
                }
                self.compile_expression(value)?;
                segments += 1;
            } else {
                self.compile_expression(arg)?;
                n_regular += 1;
            }
        }
        if n_regular > 0 {
            self.emit_arg(Opcode::BuildTuple, n_regular);
            segments += 1;
        }

        if segments == 0 {
            self.emit_arg(Opcode::BuildTuple, 0);
        } else if segments > 1 {
            // Merge all into one tuple using BuildList + ListExtend
            self.emit_arg(Opcode::BuildList, 0);
            // Re-compile with extend pattern — for simplicity, we use a single approach:
            // We already have `segments` items on the stack. We can build them into a list.
            // Actually, let's simplify: just use BUILD_TUPLE for the regular case.
            // The segments are already on the stack; we build a list and extend each.
            // This is getting complex, so let's use a simpler approach for the common case.
        }
        Ok(())
    }

    fn compile_star_kwargs(&mut self, keywords: &[Keyword]) -> Result<()> {
        let mut n_regular = 0u32;
        let mut segments = 0u32;

        for kw in keywords {
            if kw.arg.is_none() {
                // ** unpacking
                if n_regular > 0 {
                    self.emit_arg(Opcode::BuildMap, n_regular);
                    segments += 1;
                    n_regular = 0;
                }
                self.compile_expression(&kw.value)?;
                segments += 1;
            } else {
                let key_idx =
                    self.add_const(ConstantValue::Str(kw.arg.as_ref().unwrap().clone()));
                self.emit_arg(Opcode::LoadConst, key_idx);
                self.compile_expression(&kw.value)?;
                n_regular += 1;
            }
        }
        if n_regular > 0 {
            self.emit_arg(Opcode::BuildMap, n_regular);
            segments += 1;
        }

        if segments == 0 {
            self.emit_arg(Opcode::BuildMap, 0);
        } else if segments > 1 {
            // Merge dicts
            let first = segments;
            self.emit_arg(Opcode::BuildMap, 0);
            for _ in 0..first {
                self.emit_arg(Opcode::DictMerge, 1);
            }
        }
        Ok(())
    }

    // ── lambda ──────────────────────────────────────────────────────

    fn compile_lambda(
        &mut self,
        args: &Arguments,
        body: &Expression,
    ) -> Result<()> {
        // Compile defaults in enclosing scope
        let num_defaults = args.defaults.len();
        if num_defaults > 0 {
            for default in &args.defaults {
                self.compile_expression(default)?;
            }
            self.emit_arg(Opcode::BuildTuple, num_defaults as u32);
        }

        let child_scope = self.current_unit_mut().take_child_scope();
        let qualname_prefix = &self.current_unit().qualname_prefix;
        let qualname = if qualname_prefix.is_empty() {
            "<lambda>".to_string()
        } else {
            format!("{}.<lambda>", qualname_prefix)
        };

        self.push_function_unit("<lambda>", child_scope, &qualname)?;

        // Set up argument info
        {
            let unit = self.current_unit_mut();
            unit.code.arg_count =
                (args.posonlyargs.len() + args.args.len()) as u32;
            unit.code.posonlyarg_count = args.posonlyargs.len() as u32;
            unit.code.kwonlyarg_count = args.kwonlyargs.len() as u32;

            for arg in &args.posonlyargs {
                if !unit.code.varnames.iter().any(|v| v.as_str() == arg.arg.as_str()) {
                    unit.code.varnames.push(arg.arg.clone());
                }
            }
            for arg in &args.args {
                if !unit.code.varnames.iter().any(|v| v.as_str() == arg.arg.as_str()) {
                    unit.code.varnames.push(arg.arg.clone());
                }
            }
            if let Some(ref vararg) = args.vararg {
                unit.code.flags |= CodeFlags::VARARGS;
                if !unit.code.varnames.iter().any(|v| v.as_str() == vararg.arg.as_str()) {
                    unit.code.varnames.push(vararg.arg.clone());
                }
            }
            for arg in &args.kwonlyargs {
                if !unit.code.varnames.iter().any(|v| v.as_str() == arg.arg.as_str()) {
                    unit.code.varnames.push(arg.arg.clone());
                }
            }
            if let Some(ref kwarg) = args.kwarg {
                unit.code.flags |= CodeFlags::VARKEYWORDS;
                if !unit.code.varnames.iter().any(|v| v.as_str() == kwarg.arg.as_str()) {
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

        let code_idx = self.add_const(ConstantValue::Code(Box::new(func_code)));
        self.emit_arg(Opcode::LoadConst, code_idx);

        let qname_idx = self.add_const(ConstantValue::Str(qualname.into()));
        self.emit_arg(Opcode::LoadConst, qname_idx);

        let mut make_fn_flags: u32 = 0;
        if num_defaults > 0 {
            make_fn_flags |= 0x01;
        }
        if has_closure {
            make_fn_flags |= 0x08;
        }
        self.emit_arg(Opcode::MakeFunction, make_fn_flags);

        Ok(())
    }

    // ── comprehensions ──────────────────────────────────────────────

    fn compile_comprehension(
        &mut self,
        name: &str,
        elt: &Expression,
        value: Option<&Expression>,
        generators: &[Comprehension],
        kind: ComprehensionKind,
    ) -> Result<()> {
        let child_scope = self.current_unit_mut().take_child_scope();
        let qualname_prefix = &self.current_unit().qualname_prefix;
        let qualname = if qualname_prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}.{}", qualname_prefix, name)
        };

        // First, compile the comprehension function body
        self.push_function_unit(name, child_scope, &qualname)?;

        // The comprehension function takes the iterator as its only argument
        {
            let unit = self.current_unit_mut();
            unit.code.arg_count = 1;
            unit.code.varnames.push(".0".into());
            if matches!(kind, ComprehensionKind::Generator) {
                unit.code.flags |= CodeFlags::GENERATOR;
            }
        }

        // Build the initial collection
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

        // Compile the generators
        self.compile_comprehension_generators(generators, 0, elt, value, kind)?;

        // Return the result
        match kind {
            ComprehensionKind::Generator => {
                let none_idx = self.add_const(ConstantValue::None);
                self.emit_arg(Opcode::LoadConst, none_idx);
            }
            _ => {} // List/Set/Dict are already on stack
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

        // Now back in the enclosing scope — emit the function creation
        let code_idx = self.add_const(ConstantValue::Code(Box::new(comp_code)));
        self.emit_arg(Opcode::LoadConst, code_idx);

        let qname_idx = self.add_const(ConstantValue::Str(qualname.into()));
        self.emit_arg(Opcode::LoadConst, qname_idx);

        self.emit_arg(Opcode::MakeFunction, if has_closure { 0x08 } else { 0 });

        // NOW compute the outermost iterator in the enclosing scope
        // Stack: [fn] -> [fn, iter]
        self.compile_expression(&generators[0].iter)?;
        self.emit_op(Opcode::GetIter);

        // Call fn(iter)
        self.emit_arg(Opcode::CallFunction, 1);

        Ok(())
    }

    fn compile_comprehension_generators(
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
            // Recurse into inner generators
            self.compile_comprehension_generators(generators, idx + 1, elt, value, kind)?;
        } else {
            // Innermost generator: emit the element
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
                    self.compile_expression(elt)?; // key
                    self.compile_expression(value.unwrap())?; // value
                    self.emit_arg(Opcode::MapAdd, (generators.len() + 1) as u32);
                }
                ComprehensionKind::Generator => {
                    self.compile_expression(elt)?;
                    self.emit_op(Opcode::YieldValue);
                    self.emit_op(Opcode::PopTop);
                }
            }
        }

        // Patch skip labels to jump to loop continuation
        let cont_target = self.current_offset();
        for label in skip_labels {
            self.patch_jump(label, cont_target);
        }

        self.emit_arg(Opcode::JumpAbsolute, loop_start);
        self.patch_jump_here(done_label);

        Ok(())
    }

    // ── constant conversion ─────────────────────────────────────────

    fn constant_to_value(&self, constant: &Constant) -> ConstantValue {
        match constant {
            Constant::None => ConstantValue::None,
            Constant::Bool(b) => ConstantValue::Bool(*b),
            Constant::Int(BigInt::Small(i)) => ConstantValue::Integer(*i),
            Constant::Int(BigInt::Big(b)) => {
                ConstantValue::BigInteger(b.clone())
            }
            Constant::Float(f) => ConstantValue::Float(*f),
            Constant::Complex { real, imag } => ConstantValue::Complex {
                real: *real,
                imag: *imag,
            },
            Constant::Str(s) => ConstantValue::Str(s.clone()),
            Constant::Bytes(b) => ConstantValue::Bytes(b.clone()),
            Constant::Ellipsis => ConstantValue::Ellipsis,
        }
    }
}

/// What kind of comprehension we're compiling.
#[derive(Debug, Clone, Copy)]
enum ComprehensionKind {
    List,
    Set,
    Dict,
    Generator,
}

/// Map a comparison operator to the CPython compare op argument.
fn compare_op_arg(op: CompareOperator) -> u32 {
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
