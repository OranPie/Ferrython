//! Execution frame for the Ferrython VM.

use compact_str::CompactString;
use ferrython_bytecode::CodeObject;
use ferrython_core::object::{ PyCell, FxAttrMap, PyObjectRef};
use ferrython_core::types::{SharedConstantCache, SharedGlobals};
use indexmap::IndexMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::rc::Rc;

/// A shared cell for closure variables.
pub type CellRef = Rc<PyCell<Option<PyObjectRef>>>;

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

#[derive(Debug, Clone, Copy)]
pub struct Block {
    pub kind: BlockKind,
    pub handler: usize,
    pub stack_level: usize,
}

/// Fixed-capacity inline block stack (avoids Vec heap allocation).
/// Python rarely nests more than 8 blocks; overflow spills to a Vec.
const BLOCK_STACK_INLINE: usize = 8;

#[derive(Debug, Clone)]
pub struct BlockStack {
    inline: [std::mem::MaybeUninit<Block>; BLOCK_STACK_INLINE],
    len: u8,
    overflow: Option<Vec<Block>>,
}

impl BlockStack {
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            inline: [std::mem::MaybeUninit::uninit(); BLOCK_STACK_INLINE],
            len: 0,
            overflow: None,
        }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0 && self.overflow.as_ref().map_or(true, |v| v.is_empty())
    }

    #[inline(always)]
    pub fn push(&mut self, block: Block) {
        if (self.len as usize) < BLOCK_STACK_INLINE {
            unsafe { self.inline[self.len as usize].write(block); }
            self.len += 1;
        } else {
            self.overflow.get_or_insert_with(|| Vec::with_capacity(4)).push(block);
        }
    }

    #[inline(always)]
    pub fn pop(&mut self) -> Option<Block> {
        if let Some(ref mut ov) = self.overflow {
            if let Some(b) = ov.pop() {
                return Some(b);
            }
        }
        if self.len > 0 {
            self.len -= 1;
            Some(unsafe { self.inline[self.len as usize].assume_init() })
        } else {
            None
        }
    }

    #[inline(always)]
    pub fn last(&self) -> Option<&Block> {
        if let Some(ref ov) = self.overflow {
            if let Some(b) = ov.last() {
                return Some(b);
            }
        }
        if self.len > 0 {
            Some(unsafe { self.inline[(self.len - 1) as usize].assume_init_ref() })
        } else {
            None
        }
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &Block> {
        let inline_slice = unsafe {
            std::slice::from_raw_parts(self.inline.as_ptr() as *const Block, self.len as usize)
        };
        let overflow_slice = self.overflow.as_deref().unwrap_or(&[]);
        inline_slice.iter().chain(overflow_slice.iter())
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.len = 0;
        if let Some(ref mut ov) = self.overflow {
            ov.clear();
        }
    }
}

pub struct Frame {
    pub code: Arc<CodeObject>,
    pub ip: usize,
    pub stack: Vec<PyObjectRef>,
    pub block_stack: BlockStack,
    pub locals: Vec<Option<PyObjectRef>>,
    /// Boxed to reduce Frame size (~48→8 bytes). Only allocated for class/module scope.
    pub local_names: Option<Box<FxAttrMap>>,
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
    /// Arc-wrapped so recursive frames can share the cache cheaply.
    pub global_cache: Option<Arc<Vec<Option<PyObjectRef>>>>,
    /// The globals_version at which global_cache was populated.
    pub global_cache_version: u64,
    /// The dict returned by metaclass.__prepare__() (PEP 3115).
    /// When set, STORE_NAME in class scope also writes to this dict so that
    /// custom dict subclasses (e.g. enum._EnumDict) see every assignment.
    pub prepare_dict: Option<PyObjectRef>,
    /// True when code/globals/builtins/constant_cache were borrowed (not ref-counted)
    /// from the parent frame via new_recursive(). recycle() must not drop those Arcs.
    pub(crate) borrowed_env: bool,
    /// Per-frame inline cache for class-level attribute lookups (LoadMethod, LoadAttr).
    /// Direct-mapped by instruction pointer: slot = ip % IC_SLOTS.
    /// Each entry: (ip, class_version, cached_value). On hit, skip vtable/MRO lookup.
    /// Lazily allocated on first IC miss to avoid overhead for functions without attr lookups.
    pub(crate) attr_ic: Option<Box<AttrInlineCache>>,
}

/// Fixed-size direct-mapped inline cache for class attribute lookups.
/// Stores (instruction_pointer, class_version, value) tuples.
/// Slot = ip % ATTR_IC_SLOTS. Collisions evict the old entry.
pub const ATTR_IC_SLOTS: usize = 32;

#[derive(Clone)]
pub struct AttrInlineCache {
    ips: [u32; ATTR_IC_SLOTS],
    versions: [u64; ATTR_IC_SLOTS],
    values: [Option<PyObjectRef>; ATTR_IC_SLOTS],
}

impl AttrInlineCache {
    pub const fn empty() -> Self {
        Self {
            ips: [u32::MAX; ATTR_IC_SLOTS],
            versions: [0; ATTR_IC_SLOTS],
            values: [const { None }; ATTR_IC_SLOTS],
        }
    }

    #[inline(always)]
    fn slot(ip: u32) -> usize {
        // Multiplicative hash for better distribution across 32 slots
        (ip.wrapping_mul(2654435761) >> 27) as usize
    }

    #[inline(always)]
    pub fn lookup(&self, ip: u32, class_version: u64) -> Option<&PyObjectRef> {
        let slot = Self::slot(ip);
        unsafe {
            if *self.ips.get_unchecked(slot) == ip
                && *self.versions.get_unchecked(slot) == class_version
            {
                self.values.get_unchecked(slot).as_ref()
            } else {
                None
            }
        }
    }

    #[inline(always)]
    pub fn insert(&mut self, ip: u32, class_version: u64, value: PyObjectRef) {
        let slot = Self::slot(ip);
        unsafe {
            *self.ips.get_unchecked_mut(slot) = ip;
            *self.versions.get_unchecked_mut(slot) = class_version;
            *self.values.get_unchecked_mut(slot) = Some(value);
        }
    }

    #[inline]
    pub fn clear(&mut self) {
        self.ips = [u32::MAX; ATTR_IC_SLOTS];
        self.versions = [0; ATTR_IC_SLOTS];
        self.values = [const { None }; ATTR_IC_SLOTS];
    }
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
            (0..nc).map(|_| Rc::new(PyCell::new(None))).collect()
        } else { Vec::new() };
        Self {
            code, ip: 0,
            stack: Vec::with_capacity(32),
            block_stack: BlockStack::new(),
            locals: vec![None; nl],
            local_names: None,
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
            borrowed_env: false,
            attr_ic: None,
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

        let cells: Vec<CellRef> = if nc > 0 {
            (0..nc).map(|_| Rc::new(PyCell::new(None))).collect()
        } else { Vec::new() };
        Self {
            code, ip: 0,
            stack,
            block_stack: BlockStack::new(),
            locals,
            local_names: None,
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
            borrowed_env: false,
            attr_ic: None,
        }
    }

    /// Create a frame for closure functions, reusing pooled vectors.
    /// Takes closure cells directly to avoid allocating and then replacing freevars.
    #[inline]
    pub fn new_closure_from_pool(
        code: Arc<CodeObject>,
        globals: SharedGlobals,
        builtins: SharedBuiltins,
        constant_cache: SharedConstantCache,
        closure: &[CellRef],
        pool: &mut FramePool,
    ) -> Self {
        let nl = code.varnames.len();
        let n_cellvars = code.cellvars.len();
        let n_freevars = code.freevars.len();

        let mut stack = pool.take_stack();
        stack.clear();
        let needed = (code.max_stack_size as usize).max(8);
        if stack.capacity() < needed {
            stack.reserve(needed - stack.capacity());
        }

        let mut locals = pool.take_locals();
        locals.clear();
        locals.resize(nl, None);

        // Build cells: allocate only for cellvars, clone from closure for freevars
        let nc = n_cellvars + n_freevars;
        let cells: Vec<CellRef> = if nc > 0 {
            let mut cells = Vec::with_capacity(nc);
            for _ in 0..n_cellvars {
                cells.push(Rc::new(PyCell::new(None)));
            }
            for (i, cell) in closure.iter().enumerate() {
                if i < n_freevars {
                    cells.push(cell.clone());
                }
            }
            cells
        } else { Vec::new() };

        Self {
            code, ip: 0,
            stack,
            block_stack: BlockStack::new(),
            locals,
            local_names: None,
            globals,
            builtins,
            cells,
            scope_kind: ScopeKind::Function,
            yielded: false,
            pending_return: None,
            constant_cache,
            global_cache: None,
            global_cache_version: u64::MAX,
            prepare_dict: None,
            borrowed_env: false,
            attr_ic: None,
        }
    }

    /// Lightweight frame construction for recursive calls to the same function.
    /// SAFETY: Borrows the parent's code/globals/builtins/constant_cache without
    /// incrementing their reference counts (saves 4 atomic operations). The
    /// caller MUST ensure the parent outlives this frame — guaranteed by the
    /// iterative call-stack design where child frames are always popped before
    /// the parent. recycle_borrowed() must be used instead of recycle().
    #[inline(always)]
    pub unsafe fn new_recursive(
        parent: &Frame,
        pool: &mut FramePool,
    ) -> Self {
        let nl = parent.code.varnames.len();

        let mut stack = pool.take_stack();
        stack.clear();

        let mut locals = pool.take_locals();
        // Fast path: if pooled locals already has the right length and all None
        // (guaranteed by recycle()), skip clear+resize entirely
        if locals.len() != nl {
            locals.clear();
            locals.resize(nl, None);
        }
        // else: locals is already len=nl with all None from recycle()

        // Bitwise-copy Arcs without incrementing refcount — parent keeps them alive
        Self {
            code: std::ptr::read(&parent.code),
            ip: 0,
            stack,
            block_stack: BlockStack::new(),
            locals,
            local_names: None,
            globals: std::ptr::read(&parent.globals),
            builtins: std::ptr::read(&parent.builtins),
            cells: Vec::new(),
            scope_kind: ScopeKind::Function,
            yielded: false,
            pending_return: None,
            constant_cache: std::ptr::read(&parent.constant_cache),
            global_cache: std::ptr::read(&parent.global_cache),
            global_cache_version: parent.global_cache_version,
            prepare_dict: None,
            borrowed_env: true,
            attr_ic: None,
        }
    }
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
    /// If borrowed_env is set (created via new_recursive), uses ManuallyDrop
    /// to skip decrementing the borrowed Arc refcounts.
    #[inline]
    pub fn recycle(mut self, pool: &mut FramePool) {
        self.stack.clear();
        self.locals.clear();
        // block_stack is inline — just drop it (no pooling needed)

        if self.borrowed_env {
            let md = std::mem::ManuallyDrop::new(self);
            unsafe {
                let stack = std::ptr::read(&md.stack);
                let locals = std::ptr::read(&md.locals);
                // attr_ic is cloned (not borrowed) — must drop to free cached PyObjectRefs
                let _attr_ic = std::ptr::read(&md.attr_ic);
                // block_stack is inline, no need to extract
                pool.return_stack(stack);
                pool.return_locals(locals);
            }
        } else {
            pool.return_stack(self.stack);
            pool.return_locals(self.locals);
        }
    }

    #[inline] pub fn push(&mut self, v: PyObjectRef) { self.stack.push(v); }
    #[inline] pub fn pop(&mut self) -> PyObjectRef { self.stack.pop().expect("stack underflow") }
    #[inline] pub fn peek(&self) -> &PyObjectRef { self.stack.last().expect("stack underflow") }

    /// Unchecked push — caller guarantees stack has capacity.
    /// Stack capacity is pre-allocated (32) and grows automatically; for typical code
    /// this avoids the branch in Vec::push checking capacity.
    #[inline(always)]
    pub unsafe fn push_unchecked(&mut self, v: PyObjectRef) {
        let len = self.stack.len();
        debug_assert!(len < self.stack.capacity());
        std::ptr::write(self.stack.as_mut_ptr().add(len), v);
        self.stack.set_len(len + 1);
    }

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
        // Read TOS out (takes ownership → dropped at scope end)
        let _tos = std::ptr::read(self.stack.as_ptr().add(len - 1));
        // Overwrite TOS-1 with result (assignment drops old TOS-1 value)
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
        self.local_names.as_ref().and_then(|m| m.get(name).cloned())
            .or_else(|| self.globals.read().get(name).cloned())
            .or_else(|| self.builtins.get(name).cloned())
    }
    pub fn store_name(&mut self, name: CompactString, value: PyObjectRef) {
        self.local_names.get_or_insert_with(|| Box::new(FxAttrMap::default())).insert(name, value);
    }
    /// Get a value from local_names (class/module namespace).
    #[inline]
    pub fn local_names_get(&self, name: &str) -> Option<PyObjectRef> {
        self.local_names.as_ref().and_then(|m| m.get(name).cloned())
    }
    /// Check if local_names contains a key.
    #[inline]
    pub fn local_names_contains_key(&self, name: &str) -> bool {
        self.local_names.as_ref().map_or(false, |m| m.contains_key(name))
    }
    /// Remove a key from local_names.
    #[inline]
    pub fn local_names_remove(&mut self, name: &str) -> Option<PyObjectRef> {
        self.local_names.as_mut().and_then(|m| m.shift_remove(name))
    }
    /// Insert into local_names (allocates if needed).
    #[inline]
    pub fn local_names_insert(&mut self, name: CompactString, value: PyObjectRef) {
        self.local_names.get_or_insert_with(|| Box::new(FxAttrMap::default())).insert(name, value);
    }
    /// Iterate over local_names. Returns empty iter if None.
    #[inline]
    pub fn local_names_iter(&self) -> impl Iterator<Item = (&CompactString, &PyObjectRef)> {
        self.local_names.as_ref().into_iter().flat_map(|m| m.iter())
    }
    /// Take ownership of local_names (for class creation).
    #[inline]
    pub fn take_local_names(&mut self) -> FxAttrMap {
        self.local_names.take().map(|b| *b).unwrap_or_default()
    }
}

/// Pool of reusable vectors to reduce allocation overhead on function calls.
const MAX_POOL_SIZE: usize = 32;

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

    #[inline(always)]
    fn take_stack(&mut self) -> Vec<PyObjectRef> {
        self.stacks.pop().unwrap_or_else(|| Vec::with_capacity(32))
    }

    #[inline(always)]
    fn take_locals(&mut self) -> Vec<Option<PyObjectRef>> {
        self.locals.pop().unwrap_or_default()
    }

    #[inline(always)]
    fn return_stack(&mut self, v: Vec<PyObjectRef>) {
        if self.stacks.len() < MAX_POOL_SIZE {
            self.stacks.push(v);
        }
    }

    #[inline(always)]
    fn return_locals(&mut self, v: Vec<Option<PyObjectRef>>) {
        if self.locals.len() < MAX_POOL_SIZE {
            self.locals.push(v);
        }
    }
}
