//! Execution frame for the Ferrython VM.

use compact_str::CompactString;
use ferrython_bytecode::CodeObject;
use ferrython_core::object::PyObjectRef;
use ferrython_core::types::SharedGlobals;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

/// A shared cell for closure variables.
pub type CellRef = Arc<RwLock<Option<PyObjectRef>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind { Loop, Except, Finally, With, ExceptHandler }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind { Module, Function, Class }

#[derive(Debug, Clone)]
pub struct Block {
    pub kind: BlockKind,
    pub handler: usize,
    pub stack_level: usize,
}

pub struct Frame {
    pub code: CodeObject,
    pub ip: usize,
    pub stack: Vec<PyObjectRef>,
    pub block_stack: Vec<Block>,
    pub locals: Vec<Option<PyObjectRef>>,
    pub local_names: IndexMap<CompactString, PyObjectRef>,
    pub globals: SharedGlobals,
    pub builtins: IndexMap<CompactString, PyObjectRef>,
    /// Cell and free variables. Indices 0..cellvars.len() are cell vars,
    /// cellvars.len()..cellvars.len()+freevars.len() are free vars.
    pub cells: Vec<CellRef>,
    pub scope_kind: ScopeKind,
    /// Set to true when a YieldValue instruction is executed.
    pub yielded: bool,
}

impl Frame {
    pub fn new(
        code: CodeObject,
        globals: SharedGlobals,
        builtins: IndexMap<CompactString, PyObjectRef>,
    ) -> Self {
        let nl = code.varnames.len();
        let nc = code.cellvars.len() + code.freevars.len();
        let cells: Vec<CellRef> = (0..nc).map(|_| Arc::new(RwLock::new(None))).collect();
        Self {
            code, ip: 0,
            stack: Vec::with_capacity(32),
            block_stack: Vec::new(),
            locals: vec![None; nl],
            local_names: IndexMap::new(),
            globals,
            builtins,
            cells,
            scope_kind: ScopeKind::Module,
            yielded: false,
        }
    }
    #[inline] pub fn push(&mut self, v: PyObjectRef) { self.stack.push(v); }
    #[inline] pub fn pop(&mut self) -> PyObjectRef { self.stack.pop().expect("stack underflow") }
    #[inline] pub fn peek(&self) -> &PyObjectRef { self.stack.last().expect("stack underflow") }
    pub fn get_local(&self, idx: usize) -> Option<&PyObjectRef> { self.locals[idx].as_ref() }
    pub fn set_local(&mut self, idx: usize, v: PyObjectRef) { self.locals[idx] = Some(v); }
    pub fn push_block(&mut self, kind: BlockKind, handler: usize) {
        self.block_stack.push(Block { kind, handler, stack_level: self.stack.len() });
    }
    pub fn pop_block(&mut self) -> Option<Block> { self.block_stack.pop() }
    pub fn load_name(&self, name: &str) -> Option<PyObjectRef> {
        self.local_names.get(name).cloned()
            .or_else(|| self.globals.read().get(name).cloned())
            .or_else(|| self.builtins.get(name).cloned())
    }
    pub fn store_name(&mut self, name: CompactString, value: PyObjectRef) {
        self.local_names.insert(name, value);
    }
}
