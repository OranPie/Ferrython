//! Main compiler: walks the AST and emits bytecode into `CodeObject`s.

mod statements;
mod expressions;

use compact_str::CompactString;
use ferrython_ast::*;
use ferrython_bytecode::{CodeFlags, CodeObject, ConstantValue, Instruction, Opcode};
use rustc_hash::FxHashSet;

use crate::error::CompileError;
use crate::symbol_table::{self, Scope, ScopeType, SymbolScope};

pub(super) type Result<T> = std::result::Result<T, CompileError>;


/// Label index used for forward-jump patching.
#[derive(Debug, Clone, Copy)]
pub(super) struct Label(u32);

/// Tracks a loop context for break/continue.
#[derive(Debug, Clone)]
pub(super) struct LoopContext {
    /// Offset where `continue` should jump (loop header).
    pub(super) continue_target: u32,
    /// Labels that need patching when the loop ends (break targets).
    pub(super) break_labels: Vec<Label>,
}

/// Compile state for a single scope (module, function, class, comprehension).
pub(super) struct CompileUnit {
    pub(super) code: CodeObject,
    pub(super) scope: Scope,
    /// Stack of active loops for break/continue resolution.
    pub(super) loop_stack: Vec<LoopContext>,
    /// Whether this scope is a function body.
    pub(super) is_function: bool,
    /// Qualname prefix for nested scopes.
    pub(super) qualname_prefix: String,
    /// Index of the next child scope to consume.
    pub(super) child_scope_idx: usize,
}

impl CompileUnit {

    pub(super) fn new(
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
    pub(super) filename: String,
    /// Stack of compile units; the top is the current scope being compiled.
    pub(super) unit_stack: Vec<CompileUnit>,
    /// When true, top-level expression statements emit PrintExpr instead of PopTop.
    pub(super) interactive: bool,
}


impl Compiler {
    pub fn new(filename: impl Into<String>) -> Self {
        Self {
            filename: filename.into(),
            unit_stack: Vec::new(),
            interactive: false,
        }
    }

    /// Set interactive mode. In this mode, top-level expression statements
    /// emit `PrintExpr` instead of `PopTop`, and store the result in `_`.
    pub fn set_interactive(&mut self, interactive: bool) {
        self.interactive = interactive;
    }

    // ── scope helpers ───────────────────────────────────────────────

    pub(super) fn current_unit(&self) -> &CompileUnit {
        self.unit_stack.last().expect("no compile unit on stack")
    }

    pub(super) fn current_unit_mut(&mut self) -> &mut CompileUnit {
        self.unit_stack.last_mut().expect("no compile unit on stack")
    }

    #[allow(dead_code)]
    pub(super) fn code(&mut self) -> &mut CodeObject {
        &mut self.current_unit_mut().code
    }

    pub(super) fn emit(&mut self, instr: Instruction) -> u32 {
        self.current_unit_mut().code.emit(instr)
    }

    pub(super) fn emit_op(&mut self, op: Opcode) -> u32 {
        self.emit(Instruction::simple(op))
    }

    pub(super) fn emit_arg(&mut self, op: Opcode, arg: u32) -> u32 {
        self.emit(Instruction::new(op, arg))
    }

    pub(super) fn add_const(&mut self, value: ConstantValue) -> u32 {
        self.current_unit_mut().code.add_const(value)
    }

    pub(super) fn add_name(&mut self, name: &str) -> u32 {
        self.current_unit_mut().code.add_name(name)
    }

    pub(super) fn current_offset(&self) -> u32 {
        self.current_unit().code.current_offset()
    }

    // ── label / jump helpers ────────────────────────────────────────

    /// Emit a placeholder jump instruction; returns a Label to be patched later.
    pub(super) fn emit_jump(&mut self, op: Opcode) -> Label {
        let idx = self.emit_arg(op, 0); // placeholder
        Label(idx)
    }

    /// Patch a previously emitted jump to point at `target`.
    pub(super) fn patch_jump(&mut self, label: Label, target: u32) {
        self.current_unit_mut().code.instructions[label.0 as usize].arg = target;
    }

    /// Patch a jump to point at the current offset.
    pub(super) fn patch_jump_here(&mut self, label: Label) {
        let offset = self.current_offset();
        self.patch_jump(label, offset);
    }

    // ── name resolution ─────────────────────────────────────────────

    #[allow(dead_code)]
    pub(super) fn is_module_scope(&self) -> bool {
        self.current_unit().scope.scope_type == ScopeType::Module
    }

    pub(super) fn is_function_scope(&self) -> bool {
        self.current_unit().is_function
    }

    #[allow(dead_code)]
    pub(super) fn globals(&self) -> FxHashSet<&str> {
        self.current_unit().scope.global_names()
    }

    pub(super) fn is_local(&self, name: &str) -> bool {
        if let Some(sym) = self.current_unit().scope.lookup(name) {
            sym.scope == SymbolScope::Local
        } else {
            false
        }
    }

    pub(super) fn is_cell(&self, name: &str) -> bool {
        if let Some(sym) = self.current_unit().scope.lookup(name) {
            sym.scope == SymbolScope::Cell
        } else {
            false
        }
    }

    pub(super) fn is_free(&self, name: &str) -> bool {
        if let Some(sym) = self.current_unit().scope.lookup(name) {
            sym.scope == SymbolScope::Free
        } else {
            false
        }
    }

    pub(super) fn is_global(&self, name: &str) -> bool {
        if let Some(sym) = self.current_unit().scope.lookup(name) {
            sym.scope == SymbolScope::Global
        } else {
            false
        }
    }

    /// Find fast-local index for a name, adding it to varnames if needed.
    pub(super) fn varname_index(&mut self, name: &str) -> u32 {
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
    pub(super) fn deref_index(&mut self, name: &str) -> u32 {
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
    pub(super) fn load_name(&mut self, name: &str) {
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
    pub(super) fn store_name(&mut self, name: &str) {
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
    pub(super) fn delete_name(&mut self, name: &str) {
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

}
