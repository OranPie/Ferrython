//! Execution frame for the Ferrython VM.

use compact_str::CompactString;
use ferrython_bytecode::CodeObject;
use ferrython_core::object::PyObjectRef;
use ferrython_core::types::{SharedConstantCache, SharedGlobals};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// A shared cell for closure variables.
pub type CellRef = Arc<RwLock<Option<PyObjectRef>>>;

/// Shared builtins map — built once, shared across all frames.
pub type SharedBuiltins = Arc<IndexMap<CompactString, PyObjectRef>>;

/// Global version counter — incremented on every StoreGlobal/DeleteGlobal.
/// LoadGlobal checks this to invalidate its per-frame cache.
static GLOBALS_VERSION: AtomicU64 = AtomicU64::new(0);

/// Bump the global version counter (called from StoreGlobal/DeleteGlobal).
#[inline]
pub fn bump_globals_version() {
    GLOBALS_VERSION.fetch_add(1, Ordering::Relaxed);
}

/// Read current globals version.
#[inline]
pub fn globals_version() -> u64 {
    GLOBALS_VERSION.load(Ordering::Relaxed)
}

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
    /// Per-frame inline cache for LoadGlobal: lazily allocated on first miss.
    pub global_cache: Option<Vec<Option<PyObjectRef>>>,
    /// The globals_version at which global_cache was populated.
    pub global_cache_version: u64,
    /// The dict returned by metaclass.__prepare__() (PEP 3115).
    /// When set, STORE_NAME in class scope also writes to this dict so that
    /// custom dict subclasses (e.g. enum._EnumDict) see every assignment.
    pub prepare_dict: Option<PyObjectRef>,
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
        let cells: Vec<CellRef> = if nc > 0 {
            (0..nc).map(|_| Arc::new(RwLock::new(None))).collect()
        } else { Vec::new() };
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
            global_cache: None,
            global_cache_version: u64::MAX, // force miss on first access
            prepare_dict: None,
        }
    }

    /// Create a frame reusing pooled vectors to avoid heap allocation.
    #[inline]
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
        let needed = (code.max_stack_size as usize).max(8);
        if stack.capacity() < needed {
            stack.reserve(needed - stack.capacity());
        }

        // Reuse a pooled locals vector or allocate new
        let mut locals = pool.take_locals();
        locals.clear();
        locals.resize(nl, None);

        let block_stack = pool.take_block_stack();

        let cells: Vec<CellRef> = if nc > 0 {
            (0..nc).map(|_| Arc::new(RwLock::new(None))).collect()
        } else { Vec::new() };
        Self {
            code, ip: 0,
            stack,
            block_stack,
            locals,
            local_names: IndexMap::new(),
            globals,
            builtins,
            cells,
            scope_kind: ScopeKind::Module,
            yielded: false,
            pending_return: None,
            constant_cache,
            global_cache: None,
            global_cache_version: u64::MAX,
            prepare_dict: None,
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
    #[inline]
    pub fn recycle(mut self, pool: &mut FramePool) {
        self.stack.clear();
        self.locals.clear();
        self.block_stack.clear();
        pool.return_stack(self.stack);
        pool.return_locals(self.locals);
        pool.return_block_stack(self.block_stack);
    }

    #[inline] pub fn push(&mut self, v: PyObjectRef) { self.stack.push(v); }
    #[inline] pub fn pop(&mut self) -> PyObjectRef { self.stack.pop().expect("stack underflow") }
    #[inline] pub fn peek(&self) -> &PyObjectRef { self.stack.last().expect("stack underflow") }

    /// Unchecked pop — caller guarantees stack is non-empty.
    #[inline(always)]
    pub unsafe fn pop_unchecked(&mut self) -> PyObjectRef {
        let new_len = self.stack.len() - 1;
        self.stack.set_len(new_len);
        std::ptr::read(self.stack.as_ptr().add(new_len))
    }

    /// Unchecked peek at TOS — caller guarantees stack is non-empty.
    #[inline(always)]
    pub unsafe fn peek_unchecked(&self) -> &PyObjectRef {
        self.stack.get_unchecked(self.stack.len() - 1)
    }

    /// Unchecked local get — caller guarantees idx < locals.len().
    #[inline(always)]
    pub unsafe fn get_local_unchecked(&self, idx: usize) -> Option<&PyObjectRef> {
        self.locals.get_unchecked(idx).as_ref()
    }

    /// Unchecked local set — caller guarantees idx < locals.len().
    #[inline(always)]
    pub unsafe fn set_local_unchecked(&mut self, idx: usize, v: PyObjectRef) {
        *self.locals.get_unchecked_mut(idx) = Some(v);
    }

    /// Replace TOS-1 with `val` and pop TOS in one operation (binary op result).
    /// Avoids separate pop() + last_mut().unwrap() bounds checks.
    /// SAFETY: caller guarantees stack has at least 2 elements.
    #[inline(always)]
    pub unsafe fn binary_op_result(&mut self, val: PyObjectRef) {
        let len = self.stack.len();
        // Overwrite TOS-1 with result (drops old value), then truncate
        *self.stack.get_unchecked_mut(len - 2) = val;
        self.stack.set_len(len - 1);
    }
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
const MAX_POOL_SIZE: usize = 32;

pub struct FramePool {
    stacks: Vec<Vec<PyObjectRef>>,
    locals: Vec<Vec<Option<PyObjectRef>>>,
    block_stacks: Vec<Vec<Block>>,
}

impl FramePool {
    pub fn new() -> Self {
        Self {
            stacks: Vec::with_capacity(MAX_POOL_SIZE),
            locals: Vec::with_capacity(MAX_POOL_SIZE),
            block_stacks: Vec::with_capacity(MAX_POOL_SIZE),
        }
    }

    fn take_stack(&mut self) -> Vec<PyObjectRef> {
        self.stacks.pop().unwrap_or_else(|| Vec::with_capacity(32))
    }

    fn take_locals(&mut self) -> Vec<Option<PyObjectRef>> {
        self.locals.pop().unwrap_or_default()
    }

    fn take_block_stack(&mut self) -> Vec<Block> {
        if let Some(mut bs) = self.block_stacks.pop() {
            bs.clear();
            bs
        } else {
            Vec::with_capacity(4)
        }
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

    fn return_block_stack(&mut self, v: Vec<Block>) {
        if self.block_stacks.len() < MAX_POOL_SIZE {
            self.block_stacks.push(v);
        }
    }
}
