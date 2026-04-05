//! Execution frame for the Ferrython VM.

use compact_str::CompactString;
use ferrython_bytecode::CodeObject;
use ferrython_core::object::PyObjectRef;
use ferrython_core::types::{SharedConstantCache, SharedGlobals};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;

/// A shared cell for closure variables.
pub type CellRef = Arc<RwLock<Option<PyObjectRef>>>;

/// Shared builtins map — built once, shared across all frames.
pub type SharedBuiltins = Arc<IndexMap<CompactString, PyObjectRef>>;

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
    pub code: Arc<CodeObject>,
    pub ip: usize,
    pub stack: Vec<PyObjectRef>,
    pub block_stack: Vec<Block>,
    pub locals: Vec<Option<PyObjectRef>>,
    pub local_names: IndexMap<CompactString, PyObjectRef>,
    pub globals: SharedGlobals,
    pub builtins: SharedBuiltins,
    /// Cell and free variables. Indices 0..cellvars.len() are cell vars,
    /// cellvars.len()..cellvars.len()+freevars.len() are free vars.
    pub cells: Vec<CellRef>,
    pub scope_kind: ScopeKind,
    /// Set to true when a YieldValue instruction is executed.
    pub yielded: bool,
    /// Pending return value when unwinding through finally blocks.
    pub pending_return: Option<PyObjectRef>,
    /// Pre-boxed constants — shared from PyFunction or built for module-level code.
    pub constant_cache: SharedConstantCache,
}

impl Frame {
    /// Create a frame for a function call with a pre-built shared constant cache.
    pub fn new_with_cache(
        code: Arc<CodeObject>,
        globals: SharedGlobals,
        builtins: SharedBuiltins,
        constant_cache: SharedConstantCache,
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
            pending_return: None,
            constant_cache,
        }
    }

    /// Create a frame reusing pooled vectors to avoid heap allocation.
    pub fn new_from_pool(
        code: Arc<CodeObject>,
        globals: SharedGlobals,
        builtins: SharedBuiltins,
        constant_cache: SharedConstantCache,
        pool: &mut FramePool,
    ) -> Self {
        let nl = code.varnames.len();
        let nc = code.cellvars.len() + code.freevars.len();

        // Reuse a pooled stack vector or allocate new
        let mut stack = pool.take_stack();
        stack.clear();
        if stack.capacity() < 32 {
            stack.reserve(32 - stack.capacity());
        }

        // Reuse a pooled locals vector or allocate new
        let mut locals = pool.take_locals();
        locals.clear();
        locals.resize(nl, None);

        let cells: Vec<CellRef> = (0..nc).map(|_| Arc::new(RwLock::new(None))).collect();
        Self {
            code, ip: 0,
            stack,
            block_stack: Vec::new(),
            locals,
            local_names: IndexMap::new(),
            globals,
            builtins,
            cells,
            scope_kind: ScopeKind::Module,
            yielded: false,
            pending_return: None,
            constant_cache,
        }
    }

    /// Create a frame for module-level code (builds its own constant cache).
    pub fn new(
        code: Arc<CodeObject>,
        globals: SharedGlobals,
        builtins: SharedBuiltins,
    ) -> Self {
        use ferrython_core::types::PyFunction;
        let constant_cache = Arc::new(PyFunction::build_constant_cache(&code));
        Self::new_with_cache(code, globals, builtins, constant_cache)
    }

    /// Return the stack and locals vectors to the pool for reuse.
    pub fn recycle(mut self, pool: &mut FramePool) {
        self.stack.clear();
        self.locals.clear();
        pool.return_stack(self.stack);
        pool.return_locals(self.locals);
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

/// Pool of reusable vectors to reduce allocation overhead on function calls.
const MAX_POOL_SIZE: usize = 16;

pub struct FramePool {
    stacks: Vec<Vec<PyObjectRef>>,
    locals: Vec<Vec<Option<PyObjectRef>>>,
}

impl FramePool {
    pub fn new() -> Self {
        Self {
            stacks: Vec::with_capacity(MAX_POOL_SIZE),
            locals: Vec::with_capacity(MAX_POOL_SIZE),
        }
    }

    fn take_stack(&mut self) -> Vec<PyObjectRef> {
        self.stacks.pop().unwrap_or_else(|| Vec::with_capacity(32))
    }

    fn take_locals(&mut self) -> Vec<Option<PyObjectRef>> {
        self.locals.pop().unwrap_or_default()
    }

    fn return_stack(&mut self, v: Vec<PyObjectRef>) {
        if self.stacks.len() < MAX_POOL_SIZE {
            self.stacks.push(v);
        }
    }

    fn return_locals(&mut self, v: Vec<Option<PyObjectRef>>) {
        if self.locals.len() < MAX_POOL_SIZE {
            self.locals.push(v);
        }
    }
}
