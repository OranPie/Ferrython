//! The main virtual machine — executes bytecode instructions.

/// Unchecked push to frame.stack — only borrows stack field, not entire Frame.
/// SAFETY: caller guarantees stack has capacity (stack pre-allocated to max_stack_size).
macro_rules! spush {
    ($frame:expr, $val:expr) => {
        unsafe {
            let stack = &mut $frame.stack;
            let len = stack.len();
            debug_assert!(len < stack.capacity());
            std::ptr::write(stack.as_mut_ptr().add(len), $val);
            stack.set_len(len + 1);
        }
    };
}

/// Unchecked pop from frame.stack — only borrows stack field, not entire Frame.
/// SAFETY: caller guarantees stack is non-empty.
macro_rules! spop {
    ($frame:expr) => {
        unsafe {
            let stack = &mut $frame.stack;
            let new_len = stack.len() - 1;
            stack.set_len(new_len);
            std::ptr::read(stack.as_ptr().add(new_len))
        }
    };
}

/// Unchecked peek at TOS — only borrows stack field immutably.
macro_rules! speek {
    ($frame:expr) => {
        unsafe { $frame.stack.get_unchecked($frame.stack.len() - 1) }
    };
}

/// Unchecked local read — only borrows locals field, not entire Frame.
macro_rules! slocal {
    ($frame:expr, $idx:expr) => {
        unsafe { $frame.locals.get_unchecked($idx).as_ref() }
    };
}

/// Unchecked local write — only borrows locals field, not entire Frame.
macro_rules! sset_local {
    ($frame:expr, $idx:expr, $val:expr) => {
        unsafe { *$frame.locals.get_unchecked_mut($idx) = Some($val) }
    };
}

/// Unchecked stack index read — only borrows stack field immutably.
macro_rules! sget {
    ($frame:expr, $idx:expr) => {
        unsafe { $frame.stack.get_unchecked($idx) }
    };
}

/// Fast path: end profiling + continue to next instruction.
/// Eliminates Ok(None) construction + result match for hot opcodes.
macro_rules! hot_ok {
    ($profiling:expr, $profiler:expr, $op:expr) => {{
        if $profiling { $profiler.end_instruction($op); }
        continue;
    }};
}

/// Instruction chaining: if the next instruction is JumpAbsolute, consume it inline.
/// Saves one dispatch cycle per for-loop iteration.
macro_rules! chain_jump {
    ($frame:expr, $instr_base:expr, $instr_count:expr) => {
        let next_ip = $frame.ip;
        if next_ip < $instr_count {
            let next = unsafe { *$instr_base.add(next_ip) };
            if next.op == Opcode::JumpAbsolute {
                $frame.ip = next.arg as usize;
            }
        }
    };
}

/// Fast path with instruction chaining: chain JumpAbsolute, end profiling, continue.
/// Use in superinstructions that commonly appear before JumpAbsolute in for-loops.
macro_rules! hot_ok_chain {
    ($profiling:expr, $profiler:expr, $op:expr, $frame:expr, $instr_base:expr, $instr_count:expr) => {{
        chain_jump!($frame, $instr_base, $instr_count);
        if $profiling { $profiler.end_instruction($op); }
        continue;
    }};
}

/// Re-derive frame_ptr, instr_base, instr_count after call_stack modification.
/// SAFETY: call_stack must be non-empty.
macro_rules! rederive_frame {
    ($self_:expr, $frame_ptr:expr, $instr_base:expr, $instr_count:expr) => {
        unsafe {
            $frame_ptr = $self_.call_stack.as_mut_ptr().add($self_.call_stack.len() - 1);
            let f = &*$frame_ptr;
            $instr_base = f.code.instructions.as_ptr();
            $instr_count = f.code.instructions.len();
        }
    };
}

/// Chain-skip POP_TOP after void method calls: when a method returns None and the
/// next instruction is POP_TOP (expression statement), skip pushing None entirely.
/// Saves: 1 Rc clone (PyObject::none()), 1 push, 1 dispatch cycle, 1 pop.
macro_rules! chain_pop_none {
    ($frame:expr, $instr_base:expr, $instr_count:expr, $profiling:expr, $profiler:expr, $op:expr) => {{
        let next_ip = $frame.ip;
        if next_ip < $instr_count {
            if unsafe { (*$instr_base.add(next_ip)).op } == Opcode::PopTop {
                $frame.ip = next_ip + 1;
                if $profiling { $profiler.end_instruction($op); }
                continue;
            }
        }
        spush!($frame, PyObject::none());
        if $profiling { $profiler.end_instruction($op); }
        continue;
    }};
}


use crate::builtins;
use crate::frame::{AttrInlineCache, BlockKind, Frame, FramePool, SharedBuiltins};
use compact_str::CompactString;
use ferrython_bytecode::code::{CodeObject, CodeFlags, ConstantValue};
use ferrython_bytecode::opcode::{Instruction, Opcode};
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{ new_fx_hashkey_map, PyCell, 
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, IteratorData,
    lookup_in_class_mro, SyncI64, SyncUsize, FxAttrMap, is_hidden_dict_key,
    CLASS_FLAG_HAS_GETATTRIBUTE, CLASS_FLAG_HAS_DESCRIPTORS, CLASS_FLAG_HAS_SETATTR, CLASS_FLAG_HAS_SLOTS,
};
use ferrython_core::types::{BorrowedIntKey, BorrowedStrKey, HashableKey, PyInt, SharedGlobals};
use ferrython_debug::{ExecutionProfiler, BreakpointManager};
use indexmap::IndexMap;
use std::sync::OnceLock;
use std::rc::Rc;

/// Shared builtins for spawning thread VMs without re-initializing.
static SHARED_BUILTINS: OnceLock<SharedBuiltins> = OnceLock::new();

// ── Interned method name singletons ──
// Module-level statics for hot method names — enables pointer-identity comparison
// in CallMethodPopTop to skip string comparison in the fast path.
macro_rules! define_interned {
    ($id:ident, $s:literal) => {
        static $id: OnceLock<PyObjectRef> = OnceLock::new();
    };
}
define_interned!(N_APPEND, "append");
define_interned!(N_POP, "pop");
define_interned!(N_GET, "get");
define_interned!(N_SET, "set");
define_interned!(N_ADD, "add");
define_interned!(N_STRIP, "strip");
define_interned!(N_LSTRIP, "lstrip");
define_interned!(N_RSTRIP, "rstrip");
define_interned!(N_LOWER, "lower");
define_interned!(N_UPPER, "upper");
define_interned!(N_STARTSWITH, "startswith");
define_interned!(N_ENDSWITH, "endswith");
define_interned!(N_EXTEND, "extend");
define_interned!(N_INSERT, "insert");
define_interned!(N_REMOVE, "remove");
define_interned!(N_SORT, "sort");
define_interned!(N_REVERSE, "reverse");
define_interned!(N_COPY, "copy");
define_interned!(N_CLEAR, "clear");
define_interned!(N_UPDATE, "update");
define_interned!(N_ITEMS, "items");
define_interned!(N_KEYS, "keys");
define_interned!(N_VALUES, "values");
define_interned!(N_JOIN, "join");
define_interned!(N_SPLIT, "split");
define_interned!(N_REPLACE, "replace");
define_interned!(N_FIND, "find");
define_interned!(N_RFIND, "rfind");
define_interned!(N_INDEX, "index");
define_interned!(N_COUNT, "count");
define_interned!(N_FORMAT, "format");
define_interned!(N_ENCODE, "encode");
define_interned!(N_DECODE, "decode");
define_interned!(N_WRITE, "write");
define_interned!(N_READ, "read");
define_interned!(N_CLOSE, "close");

#[inline(always)]
fn init_interned<'a>(lock: &'a OnceLock<PyObjectRef>, name: &str) -> &'a PyObjectRef {
    lock.get_or_init(|| PyObjectRef::new_immortal(PyObject {
        payload: PyObjectPayload::Str(CompactString::from(name))
    }))
}

/// Cached PyObjectRef for common method names — avoids heap allocation in LoadMethod
/// for builtin type method calls. Each entry is allocated once (OnceLock) and
/// subsequent uses are just pointer clones (immortal, no refcount).
#[inline]
fn cached_method_name(name: &str) -> Option<PyObjectRef> {
    match name {
        "append" => Some(init_interned(&N_APPEND, "append").clone()),
        "pop" => Some(init_interned(&N_POP, "pop").clone()),
        "get" => Some(init_interned(&N_GET, "get").clone()),
        "set" => Some(init_interned(&N_SET, "set").clone()),
        "add" => Some(init_interned(&N_ADD, "add").clone()),
        "strip" => Some(init_interned(&N_STRIP, "strip").clone()),
        "lstrip" => Some(init_interned(&N_LSTRIP, "lstrip").clone()),
        "rstrip" => Some(init_interned(&N_RSTRIP, "rstrip").clone()),
        "lower" => Some(init_interned(&N_LOWER, "lower").clone()),
        "upper" => Some(init_interned(&N_UPPER, "upper").clone()),
        "startswith" => Some(init_interned(&N_STARTSWITH, "startswith").clone()),
        "endswith" => Some(init_interned(&N_ENDSWITH, "endswith").clone()),
        "extend" => Some(init_interned(&N_EXTEND, "extend").clone()),
        "insert" => Some(init_interned(&N_INSERT, "insert").clone()),
        "remove" => Some(init_interned(&N_REMOVE, "remove").clone()),
        "sort" => Some(init_interned(&N_SORT, "sort").clone()),
        "reverse" => Some(init_interned(&N_REVERSE, "reverse").clone()),
        "copy" => Some(init_interned(&N_COPY, "copy").clone()),
        "clear" => Some(init_interned(&N_CLEAR, "clear").clone()),
        "update" => Some(init_interned(&N_UPDATE, "update").clone()),
        "items" => Some(init_interned(&N_ITEMS, "items").clone()),
        "keys" => Some(init_interned(&N_KEYS, "keys").clone()),
        "values" => Some(init_interned(&N_VALUES, "values").clone()),
        "join" => Some(init_interned(&N_JOIN, "join").clone()),
        "split" => Some(init_interned(&N_SPLIT, "split").clone()),
        "replace" => Some(init_interned(&N_REPLACE, "replace").clone()),
        "find" => Some(init_interned(&N_FIND, "find").clone()),
        "rfind" => Some(init_interned(&N_RFIND, "rfind").clone()),
        "index" => Some(init_interned(&N_INDEX, "index").clone()),
        "count" => Some(init_interned(&N_COUNT, "count").clone()),
        "format" => Some(init_interned(&N_FORMAT, "format").clone()),
        "encode" => Some(init_interned(&N_ENCODE, "encode").clone()),
        "decode" => Some(init_interned(&N_DECODE, "decode").clone()),
        "write" => Some(init_interned(&N_WRITE, "write").clone()),
        "read" => Some(init_interned(&N_READ, "read").clone()),
        "close" => Some(init_interned(&N_CLOSE, "close").clone()),
        _ => None,
    }
}

/// Fast pointer-identity check: is this PyObjectRef the interned "append" name?
#[inline(always)]
fn is_interned_append(obj: &PyObjectRef) -> bool {
    N_APPEND.get().map_or(false, |c| PyObjectRef::ptr_eq(obj, c))
}

/// Fast pointer-identity check: is this PyObjectRef the interned "pop" name?
#[inline(always)]
fn is_interned_pop(obj: &PyObjectRef) -> bool {
    N_POP.get().map_or(false, |c| PyObjectRef::ptr_eq(obj, c))
}

/// Callback registered with ferrython-core to spawn Python functions on real OS threads.
fn spawn_python_thread_impl(
    func: PyObjectRef,
    args: Vec<PyObjectRef>,
) -> std::thread::JoinHandle<()> {
    let builtins = SHARED_BUILTINS
        .get()
        .expect("SHARED_BUILTINS not initialized")
        .clone();
    std::thread::spawn(move || {
        let mut vm = VirtualMachine::new_for_thread(builtins);
        let _ = vm.call_function_standalone(func, args);
    })
}

/// The Ferrython virtual machine.
pub struct VirtualMachine {
    pub(crate) call_stack: Vec<Frame>,
    pub(crate) builtins: SharedBuiltins,
    pub(crate) modules: IndexMap<CompactString, PyObjectRef>,
    /// Currently active exception being handled (for bare `raise` re-raise).
    pub(crate) active_exception: Option<PyException>,
    /// Reference to the sys.modules dict for synchronization.
    pub(crate) sys_modules_dict: Option<PyObjectRef>,
    /// Execution profiler (disabled by default — zero overhead when off).
    pub profiler: ExecutionProfiler,
    /// Breakpoint manager for debugger support.
    pub breakpoints: BreakpointManager,
    /// Pool of reusable frame vectors to reduce allocation.
    pub(crate) frame_pool: FramePool,
    /// Cached recursion limit (avoids thread-local access on every call).
    pub(crate) recursion_limit: usize,
}

impl VirtualMachine {
    pub fn new() -> Self {
        let builtins = SharedBuiltins(Rc::new(builtins::init_builtins()));
        // Register the thread spawn callback so the stdlib can spawn real OS
        // threads for Python function targets.  Uses the shared builtins.
        {
            SHARED_BUILTINS.get_or_init(|| builtins.clone());
            ferrython_core::error::register_thread_spawn(spawn_python_thread_impl);
        }
        // Register generator frame drop callback (core crate can't know Frame type)
        ferrython_core::object::register_gen_frame_drop(crate::vm_helpers::drop_generator_frame);
        Self {
            call_stack: Vec::with_capacity(64),
            builtins,
            modules: IndexMap::new(),
            active_exception: None,
            sys_modules_dict: None,
            profiler: ExecutionProfiler::new(),
            breakpoints: BreakpointManager::new(),
            frame_pool: FramePool::new(),
            recursion_limit: ferrython_stdlib::get_recursion_limit() as usize,
        }
    }

    /// Create a lightweight VM for use in a spawned thread.
    /// Shares the same builtins map (Arc) so builtin lookup is free.
    pub fn new_for_thread(builtins: SharedBuiltins) -> Self {
        Self {
            call_stack: Vec::with_capacity(64),
            builtins,
            modules: IndexMap::new(),
            active_exception: None,
            sys_modules_dict: None,
            profiler: ExecutionProfiler::new(),
            breakpoints: BreakpointManager::new(),
            frame_pool: FramePool::new(),
            recursion_limit: ferrython_stdlib::get_recursion_limit() as usize,
        }
    }

    /// Get a clone of the builtins Arc for passing to thread VMs.
    pub fn shared_builtins(&self) -> SharedBuiltins {
        self.builtins.clone()
    }

    /// Execute a Python function object with arguments on this VM.
    /// Used by thread spawning to run Python-defined thread targets.
    pub fn call_function_standalone(
        &mut self,
        func: PyObjectRef,
        args: Vec<PyObjectRef>,
    ) -> PyResult<PyObjectRef> {
        self.install_hash_eq_dispatch();
        self.call_object(func, args)
    }

    /// Create a new empty shared globals map.
    pub fn new_globals() -> SharedGlobals {
        Rc::new(PyCell::new(FxAttrMap::default()))
    }

    /// Execute a code object (module-level).
    pub fn execute(&mut self, code: CodeObject) -> PyResult<PyObjectRef> {
        self.install_hash_eq_dispatch();
        let globals = Rc::new(PyCell::new(FxAttrMap::default()));
        // Set __name__ = "__main__" for top-level scripts
        {
            let mut g = globals.write();
            g.insert(
                CompactString::from("__name__"),
                PyObject::str_val(CompactString::from("__main__")),
            );
            // Store __file__ if the code has a filename
            if !code.filename.is_empty() {
                g.insert(
                    CompactString::from("__file__"),
                    PyObject::str_val(code.filename.clone()),
                );
            }
            // In CPython, __builtins__ is available in every module's globals.
            // In __main__, it is the builtins module itself.
            if let Some(builtins_mod) = ferrython_stdlib::load_module("builtins") {
                g.insert(CompactString::from("__builtins__"), builtins_mod);
            }
        }
        // Register __main__ in sys.modules so that sys.modules["__main__"] works
        let main_attrs = {
            let g = globals.read();
            let mut attrs = IndexMap::new();
            for (k, v) in g.iter() {
                attrs.insert(k.clone(), v.clone());
            }
            attrs
        };
        let main_mod = PyObject::module_with_attrs(CompactString::from("__main__"), main_attrs);
        self.modules.insert(CompactString::from("__main__"), main_mod.clone());
        // If sys_modules_dict is already initialized, update it too
        if let Some(ref sys_mod_dict) = self.sys_modules_dict {
            if let PyObjectPayload::Dict(ref d) = sys_mod_dict.payload {
                d.write().insert(
                    HashableKey::str_key(CompactString::from("__main__")),
                    main_mod,
                );
            }
        }
        self.execute_with_globals(Rc::new(code), globals)
    }

    /// Execute a code object with shared globals (for REPL).
    pub fn execute_with_globals(&mut self, code: Rc<CodeObject>, globals: SharedGlobals) -> PyResult<PyObjectRef> {
        self.install_hash_eq_dispatch();
        let stack_depth = self.call_stack.len();
        let frame = Frame::new(code, globals.clone(), self.builtins.clone());
        self.call_stack.push(frame);
        let result = self.run_frame();
        // Clean up call stack: pop back to the expected depth.
        // On error, nested frames may remain; drain them to prevent
        // state pollution in subsequent REPL executions.
        while self.call_stack.len() > stack_depth {
            if let Some(frame) = self.call_stack.pop() {
                // Sync cell variables back to globals for the outermost frame only
                if self.call_stack.len() == stack_depth && result.is_ok() {
                    if !frame.code.cellvars.is_empty() {
                        let mut g = globals.write();
                        for (i, name) in frame.code.cellvars.iter().enumerate() {
                            if let Some(cell) = frame.cells.get(i) {
                                if let Some(val) = cell.read().as_ref() {
                                    g.insert(name.clone(), val.clone());
                                }
                            }
                        }
                    }
                }
                frame.recycle(&mut self.frame_pool);
            }
        }
        // Also clear the operand stack of any leftover values from errors
        if result.is_err() {
            // Reset any pending exception state
        }
        result
    }

    /// Cold helper: generate NameError for unbound locals. Marked #[cold] #[inline(never)]
    /// to keep format!() code out of the hot dispatch loop's I-cache footprint.
    #[cold]
    #[inline(never)]
    fn err_unbound_local(varnames: &[compact_str::CompactString], idx: usize) -> Result<Option<PyObjectRef>, PyException> {
        Err(PyException::name_error(format!(
            "local variable '{}' referenced before assignment",
            varnames.get(idx).map(|s| s.as_str()).unwrap_or("?")
        )))
    }

    /// Cold helper: generate NameError for unresolved names.
    #[cold]
    #[inline(never)]
    fn err_name_not_found(name: &str) -> Result<Option<PyObjectRef>, PyException> {
        Err(PyException::name_error(format!("name '{}' is not defined", name)))
    }

    /// Cold helper: generate NameError with a custom message.
    #[cold]
    #[inline(never)]
    fn err_name_error_msg(msg: String) -> Result<Option<PyObjectRef>, PyException> {
        Err(PyException::name_error(msg))
    }

    /// Execute a code object as a function call with arguments.
    pub(crate) fn run_frame(&mut self) -> PyResult<PyObjectRef> {
        // Register active_exception pointer for lazy sys.exc_info() reads
        ferrython_core::error::register_active_exc_ptr(&self.active_exception as *const _);

        let profiling = self.profiler.is_enabled();
        // Use fast atomic flags instead of thread-local RefCell access.
        // These are ~1ns (atomic load) vs ~15ns (thread-local + RefCell borrow + Option clone).
        let mut has_trace = ferrython_stdlib::is_trace_active();
        let mut has_profile = ferrython_stdlib::is_profile_active();

        // Fire "call" event at frame entry
        if has_trace {
            // Update the thread-local current frame for sys._getframe()
            let frame_obj = self.make_trace_frame();
            ferrython_stdlib::set_current_frame(Some(frame_obj));
            self.fire_trace_event("call", PyObject::none());
        }
        if has_profile {
            self.fire_profile_event("call", PyObject::none());
        }

        // Track initial call stack depth for iterative call/return.
        // When CallFunction/CallMethod push a child frame, the loop continues
        // executing it. When ReturnValue fires above initial_depth, we pop the
        // child and push the result to the parent — no recursive run_frame().
        let initial_depth = self.call_stack.len();

        let mut last_line: u32 = 0;
        // Re-check trace/profile periodically (every 64 opcodes) instead of
        // calling thread-local get_trace_func() on every single iteration.
        let mut trace_check_counter: u8 = 0;

        // Cache frame pointer to avoid re-deriving from call_stack every iteration.
        // Also cache instruction base pointer and count to eliminate Rc + Vec deref
        // on every dispatch (frame → Rc<CodeObject> → Vec<Instruction> = 2 pointer chases).
        // Hot opcodes `continue` without touching call_stack, so cached pointers stay valid.
        // Cold paths re-derive via rederive_frame!() after any call_stack modification.
        // SAFETY: call_stack is non-empty (we just pushed the initial frame).
        let mut frame_ptr: *mut crate::frame::Frame = unsafe {
            let cs_len = self.call_stack.len();
            self.call_stack.as_mut_ptr().add(cs_len - 1)
        };
        let mut instr_base: *const Instruction;
        let mut instr_count: usize;
        unsafe {
            let f = &*frame_ptr;
            instr_base = f.code.instructions.as_ptr();
            instr_count = f.code.instructions.len();
        }

        loop {
            // Only check trace/profile state when trace might be active, to avoid
            // even the counter decrement overhead in the common non-tracing case.
            if has_trace || has_profile {
                if trace_check_counter == 0 {
                    trace_check_counter = 63;
                    has_trace = ferrython_stdlib::is_trace_active();
                    has_profile = ferrython_stdlib::is_profile_active();
                } else {
                    trace_check_counter -= 1;
                }
            }

            // When tracing, separate borrows needed for fire_trace_event(&mut self).
            // When NOT tracing (common case), single mutable borrow is cheaper.
            if has_trace {
                let frame = self.call_stack.last().unwrap();
                let ip = frame.ip;
                if ip >= frame.code.instructions.len() { return Ok(PyObject::none()); }
                let current_line = Self::ip_to_line(&frame.code, ip);
                let fire_line = current_line != last_line;
                if fire_line { last_line = current_line; }
                self.call_stack.last_mut().unwrap().ip = ip + 1;
                if fire_line { self.fire_trace_event("line", PyObject::none()); }
                // Re-derive all cached pointers: fire_trace_event may call Python code
                rederive_frame!(self, frame_ptr, instr_base, instr_count);
            }

            // SAFETY: frame_ptr is re-derived after any call_stack modification.
            // Hot opcodes `continue` without modifying call_stack, keeping frame_ptr valid.
            let frame = unsafe { &mut *frame_ptr };

            let ip = frame.ip;
            let instr = if !has_trace {
                if ip >= instr_count { return Ok(PyObject::none()); }
                // SAFETY: bounds check above guarantees ip < instr_count
                let instr = unsafe { *instr_base.add(ip) };
                frame.ip = ip + 1;
                instr
            } else {
                // Tracing path already advanced ip above; read the previous instruction
                let prev_ip = ip.wrapping_sub(1);
                if prev_ip >= instr_count { return Ok(PyObject::none()); }
                unsafe { *instr_base.add(prev_ip) }
            };

            if profiling { self.profiler.start_instruction(instr.op); }

            // Inline the hottest opcodes to avoid execute_one dispatch overhead
            let result = match instr.op {
                Opcode::LoadFast => {
                    let idx = instr.arg as usize;
                    // SAFETY: compiler guarantees idx < locals.len(); stack pre-allocated
                    match slocal!(frame, idx) {
                        Some(val) => { spush!(frame, val.clone()); hot_ok!(profiling, self.profiler, instr.op) }
                        None => Self::err_unbound_local(&frame.code.varnames, idx),
                    }
                }
                Opcode::StoreFast => {
                    // SAFETY: stack non-empty (compiler guarantees), idx < locals.len()
                    let val = spop!(frame);
                    sset_local!(frame, instr.arg as usize, val);
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                // Fused StoreFast + JumpAbsolute — saves one dispatch per loop iteration
                Opcode::StoreFastJumpAbsolute => {
                    let store_idx = (instr.arg >> 16) as usize;
                    let jump_target = (instr.arg & 0xFFFF) as usize;
                    let val = spop!(frame);
                    sset_local!(frame, store_idx, val);
                    frame.ip = jump_target;
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                Opcode::LoadConst => {
                    // SAFETY: compiler guarantees arg < constant_cache.len(); stack pre-allocated
                    let obj = unsafe { frame.constant_cache.get_unchecked(instr.arg as usize).clone() };
                    spush!(frame, obj);
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                // ── Superinstructions: fused opcode pairs ──
                Opcode::LoadFastLoadFast => {
                    let idx1 = (instr.arg >> 16) as usize;
                    let idx2 = (instr.arg & 0xFFFF) as usize;
                    // SAFETY: compiler guarantees indices < locals.len()
                    let a = slocal!(frame, idx1).cloned();
                    let b = slocal!(frame, idx2).cloned();
                    match (a, b) {
                        (Some(a), Some(b)) => {
                            spush!(frame, a); spush!(frame, b);
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (None, _) => Self::err_unbound_local(&frame.code.varnames, idx1),
                        (_, None) => Self::err_unbound_local(&frame.code.varnames, idx2),
                    }
                }
                Opcode::LoadFastLoadConst => {
                    let fast_idx = (instr.arg >> 16) as usize;
                    let const_idx = (instr.arg & 0xFFFF) as usize;
                    // SAFETY: compiler guarantees indices valid
                    match slocal!(frame, fast_idx) {
                        Some(val) => {
                            spush!(frame, val.clone());
                            let c = unsafe { frame.constant_cache.get_unchecked(const_idx) }.clone();
                            spush!(frame, c);
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        None => Self::err_unbound_local(&frame.code.varnames, fast_idx),
                    }
                }
                Opcode::StoreFastLoadFast => {
                    let store_idx = (instr.arg >> 16) as usize;
                    let load_idx = (instr.arg & 0xFFFF) as usize;
                    // SAFETY: stack non-empty, indices < locals.len()
                    let val = spop!(frame);
                    sset_local!(frame, store_idx, val);
                    match slocal!(frame, load_idx) {
                        Some(val) => { spush!(frame, val.clone()); hot_ok!(profiling, self.profiler, instr.op) }
                        None => Self::err_unbound_local(&frame.code.varnames, load_idx),
                    }
                }
                // 3-way superinstruction: LoadFast + LoadConst + BinarySubtract
                Opcode::LoadFastLoadConstBinarySub => {
                    let fast_idx = (instr.arg >> 16) as usize;
                    let const_idx = (instr.arg & 0xFFFF) as usize;
                    // SAFETY: compiler guarantees indices valid
                    match slocal!(frame, fast_idx) {
                        Some(local) => {
                            let c = unsafe { frame.constant_cache.get_unchecked(const_idx) };
                            match (&local.payload, &c.payload) {
                                (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                    let result = match x.checked_sub(*y) {
                                        Some(r) => PyObject::int(r),
                                        None => {
                                            use num_bigint::BigInt;
                                            PyObject::big_int(BigInt::from(*x) - BigInt::from(*y))
                                        }
                                    };
                                    spush!(frame, result);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                    spush!(frame, PyObject::float(*x - *y));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                _ => {
                                    // Fallback: push both and let execute_one handle BinarySub
                                    spush!(frame, local.clone());
                                    spush!(frame, c.clone());
                                    self.execute_one(ferrython_bytecode::Instruction::new(
                                        Opcode::BinarySubtract, 0))
                                }
                            }
                        }
                        None => Self::err_unbound_local(&frame.code.varnames, fast_idx),
                    }
                }
                // 3-way superinstruction: LoadFast + LoadConst + BinaryAdd
                Opcode::LoadFastLoadConstBinaryAdd => {
                    let fast_idx = (instr.arg >> 16) as usize;
                    let const_idx = (instr.arg & 0xFFFF) as usize;
                    match slocal!(frame, fast_idx) {
                        Some(local) => {
                            let c = unsafe { frame.constant_cache.get_unchecked(const_idx) };
                            match (&local.payload, &c.payload) {
                                (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                    let result = match x.checked_add(*y) {
                                        Some(r) => PyObject::int(r),
                                        None => {
                                            use num_bigint::BigInt;
                                            PyObject::big_int(BigInt::from(*x) + BigInt::from(*y))
                                        }
                                    };
                                    spush!(frame, result);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                    spush!(frame, PyObject::float(*x + *y));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
                                    spush!(frame, PyObject::float(*x as f64 + *y));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
                                    spush!(frame, PyObject::float(*x + *y as f64));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                _ => {
                                    spush!(frame, local.clone());
                                    spush!(frame, c.clone());
                                    self.execute_one(ferrython_bytecode::Instruction::new(
                                        Opcode::BinaryAdd, 0))
                                }
                            }
                        }
                        None => Self::err_unbound_local(&frame.code.varnames, fast_idx),
                    }
                }
                Opcode::LoadFastLoadFastBinaryAdd => {
                    let idx1 = (instr.arg >> 16) as usize;
                    let idx2 = (instr.arg & 0xFFFF) as usize;
                    // Borrow locals without cloning — only clone on fallback
                    let a = slocal!(frame, idx1);
                    let b = slocal!(frame, idx2);
                    match (a, b) {
                        (Some(a), Some(b)) => {
                            match (&a.payload, &b.payload) {
                                (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                    let (x, y) = (*x, *y);
                                    let result = match x.checked_add(y) {
                                        Some(r) => PyObject::int(r),
                                        None => {
                                            use num_bigint::BigInt;
                                            PyObject::big_int(BigInt::from(x) + BigInt::from(y))
                                        }
                                    };
                                    spush!(frame, result);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                    let r = *x + *y;
                                    spush!(frame, PyObject::float(r));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                _ => {
                                    let (ac, bc) = (a.clone(), b.clone());
                                    spush!(frame, ac);
                                    spush!(frame, bc);
                                    self.execute_one(ferrython_bytecode::Instruction::new(
                                        Opcode::BinaryAdd, 0))
                                }
                            }
                        }
                        (None, _) | (_, None) => Err(PyException::name_error(
                            String::from("local variable referenced before assignment"))),
                    }
                }
                // 4-way fused: load two locals, add, store result — no stack touch
                Opcode::LoadFastLoadFastBinaryAddStoreFast => {
                    let idx1 = (instr.arg >> 16) as usize;
                    let idx2 = ((instr.arg >> 8) & 0xFF) as usize;
                    let dest = (instr.arg & 0xFF) as usize;
                    let a = slocal!(frame, idx1);
                    let b = slocal!(frame, idx2);
                    match (a, b) {
                        (Some(a), Some(b)) => {
                            match (&a.payload, &b.payload) {
                                (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                    let (x, y) = (*x, *y);
                                    if let Some(r) = x.checked_add(y) {
                                        // Try in-place mutation if dest holds sole reference
                                        let dest_slot = unsafe { frame.locals.get_unchecked_mut(dest) };
                                        if let Some(ref mut arc) = dest_slot {
                                            if let Some(obj) = PyObjectRef::get_mut(arc) {
                                                obj.payload = PyObjectPayload::Int(PyInt::Small(r));
                                                hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                                            }
                                        }
                                        *dest_slot = Some(PyObject::int(r));
                                    } else {
                                        use num_bigint::BigInt;
                                        let result = PyObject::big_int(BigInt::from(x) + BigInt::from(y));
                                        sset_local!(frame, dest, result);
                                    }
                                    hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                                }
                                (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                    let r = *x + *y;
                                    // Try in-place mutation — huge win for float loops
                                    let dest_slot = unsafe { frame.locals.get_unchecked_mut(dest) };
                                    if let Some(ref mut arc) = dest_slot {
                                        if let Some(obj) = PyObjectRef::get_mut(arc) {
                                            obj.payload = PyObjectPayload::Float(r);
                                            hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                                        }
                                    }
                                    *dest_slot = Some(PyObject::float(r));
                                    hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                                }
                                (PyObjectPayload::Str(x), PyObjectPayload::Str(y)) => {
                                    let mut s = String::with_capacity(x.len() + y.len());
                                    s.push_str(x);
                                    s.push_str(y);
                                    sset_local!(frame, dest, PyObject::str_val(CompactString::from(s)));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                _ => {
                                    let (ac, bc) = (a.clone(), b.clone());
                                    spush!(frame, ac);
                                    spush!(frame, bc);
                                    let r = self.execute_one(ferrython_bytecode::Instruction::new(
                                        Opcode::BinaryAdd, 0));
                                    // Re-borrow frame after execute_one
                                    if r.is_ok() {
                                        let cs_len2 = self.call_stack.len();
                                        let frame2 = unsafe { self.call_stack.get_unchecked_mut(cs_len2 - 1) };
                                        if !frame2.stack.is_empty() {
                                            let val = frame2.stack.pop().unwrap();
                                            unsafe { frame2.set_local_unchecked(dest, val) };
                                        }
                                    }
                                    r
                                }
                            }
                        }
                        (None, _) | (_, None) => Err(PyException::name_error(
                            String::from("local variable referenced before assignment"))),
                    }
                }
                // 4-way fused: load local + const, add, store — no stack touch
                Opcode::LoadFastLoadConstBinaryAddStoreFast => {
                    let local_idx = (instr.arg >> 16) as usize;
                    let const_idx = ((instr.arg >> 8) & 0xFF) as usize;
                    let dest = (instr.arg & 0xFF) as usize;
                    let a = slocal!(frame, local_idx);
                    let c = unsafe { frame.constant_cache.get_unchecked(const_idx) };
                    match a {
                        Some(a) => {
                            match (&a.payload, &c.payload) {
                                (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                    let (x, y) = (*x, *y);
                                    if let Some(r) = x.checked_add(y) {
                                        let dest_slot = unsafe { frame.locals.get_unchecked_mut(dest) };
                                        if let Some(ref mut arc) = dest_slot {
                                            if let Some(obj) = PyObjectRef::get_mut(arc) {
                                                obj.payload = PyObjectPayload::Int(PyInt::Small(r));
                                                hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                                            }
                                        }
                                        *dest_slot = Some(PyObject::int(r));
                                    } else {
                                        use num_bigint::BigInt;
                                        let result = PyObject::big_int(BigInt::from(x) + BigInt::from(y));
                                        sset_local!(frame, dest, result);
                                    }
                                    hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                                }
                                (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                    let r = *x + *y;
                                    let dest_slot = unsafe { frame.locals.get_unchecked_mut(dest) };
                                    if let Some(ref mut arc) = dest_slot {
                                        if let Some(obj) = PyObjectRef::get_mut(arc) {
                                            obj.payload = PyObjectPayload::Float(r);
                                            hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                                        }
                                    }
                                    *dest_slot = Some(PyObject::float(r));
                                    hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                                }
                                (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
                                    let r = *x as f64 + *y;
                                    let dest_slot = unsafe { frame.locals.get_unchecked_mut(dest) };
                                    if let Some(ref mut arc) = dest_slot {
                                        if let Some(obj) = PyObjectRef::get_mut(arc) {
                                            obj.payload = PyObjectPayload::Float(r);
                                            hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                                        }
                                    }
                                    *dest_slot = Some(PyObject::float(r));
                                    hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                                }
                                (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
                                    let r = *x + *y as f64;
                                    let dest_slot = unsafe { frame.locals.get_unchecked_mut(dest) };
                                    if let Some(ref mut arc) = dest_slot {
                                        if let Some(obj) = PyObjectRef::get_mut(arc) {
                                            obj.payload = PyObjectPayload::Float(r);
                                            hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                                        }
                                    }
                                    *dest_slot = Some(PyObject::float(r));
                                    hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                                }
                                (PyObjectPayload::Str(_), PyObjectPayload::Str(rhs)) => {
                                    // String concat in-place: s = s + "a" (like CPython)
                                    let rhs_clone: CompactString = rhs.clone();
                                    // NLL: a, c borrows are dead after the clone above
                                    if local_idx == dest {
                                        let locals_ptr = frame.locals.as_mut_ptr();
                                        let dest_slot = unsafe { &mut *locals_ptr.add(dest) };
                                        if let Some(mut arc) = dest_slot.take() {
                                            if let Some(obj) = PyObjectRef::get_mut(&mut arc) {
                                                if let PyObjectPayload::Str(ref mut s) = obj.payload {
                                                    s.push_str(&rhs_clone);
                                                    *dest_slot = Some(arc);
                                                    hot_ok!(profiling, self.profiler, instr.op)
                                                }
                                            }
                                            // In-place failed (extra refs), allocate new
                                            let new_s = if let PyObjectPayload::Str(ref x) = arc.payload {
                                                let mut s = String::with_capacity(x.len() + rhs_clone.len());
                                                s.push_str(x);
                                                s.push_str(&rhs_clone);
                                                CompactString::from(s)
                                            } else { unreachable!() };
                                            *dest_slot = Some(PyObject::str_val(new_s));
                                            hot_ok!(profiling, self.profiler, instr.op)
                                        }
                                    }
                                    // local_idx != dest: fall through to generic path
                                    let a = slocal!(frame, local_idx);
                                    let c = unsafe { frame.constant_cache.get_unchecked(const_idx) };
                                    if let Some(a) = a {
                                        let (ac, cc) = (a.clone(), c.clone());
                                        spush!(frame, ac);
                                        spush!(frame, cc);
                                        let r = self.execute_one(ferrython_bytecode::Instruction::new(
                                            Opcode::BinaryAdd, 0));
                                        if r.is_ok() {
                                            let cs_len2 = self.call_stack.len();
                                            let frame2 = unsafe { self.call_stack.get_unchecked_mut(cs_len2 - 1) };
                                            if !frame2.stack.is_empty() {
                                                let val = frame2.stack.pop().unwrap();
                                                unsafe { frame2.set_local_unchecked(dest, val) };
                                            }
                                        }
                                        r
                                    } else {
                                        Err(PyException::name_error(
                                            String::from("local variable referenced before assignment")))
                                    }
                                }
                                _ => {
                                    let (ac, cc) = (a.clone(), c.clone());
                                    spush!(frame, ac);
                                    spush!(frame, cc);
                                    let r = self.execute_one(ferrython_bytecode::Instruction::new(
                                        Opcode::BinaryAdd, 0));
                                    if r.is_ok() {
                                        let cs_len2 = self.call_stack.len();
                                        let frame2 = unsafe { self.call_stack.get_unchecked_mut(cs_len2 - 1) };
                                        if !frame2.stack.is_empty() {
                                            let val = frame2.stack.pop().unwrap();
                                            unsafe { frame2.set_local_unchecked(dest, val) };
                                        }
                                    }
                                    r
                                }
                            }
                        }
                        None => Err(PyException::name_error(
                            String::from("local variable referenced before assignment"))),
                    }
                }
                // Fused LoadFast + LoadConst + BinaryMul + StoreFast (x = x * c)
                Opcode::LoadFastLoadConstBinaryMulStoreFast => {
                    let fast_idx = (instr.arg >> 16) as usize;
                    let const_idx = ((instr.arg >> 8) & 0xFF) as usize;
                    let dest = (instr.arg & 0xFF) as usize;
                    let a = slocal!(frame, fast_idx);
                    let c = unsafe { frame.constant_cache.get_unchecked(const_idx) };
                    match a {
                        Some(a) => {
                            match (&a.payload, &c.payload) {
                                (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                    let (x, y) = (*x, *y);
                                    if let Some(r) = x.checked_mul(y) {
                                        let dest_slot = unsafe { frame.locals.get_unchecked_mut(dest) };
                                        if let Some(ref mut arc) = dest_slot {
                                            if let Some(obj) = PyObjectRef::get_mut(arc) {
                                                obj.payload = PyObjectPayload::Int(PyInt::Small(r));
                                                hot_ok!(profiling, self.profiler, instr.op)
                                            }
                                        }
                                        *dest_slot = Some(PyObject::int(r));
                                    } else {
                                        use num_bigint::BigInt;
                                        let result = PyObject::big_int(BigInt::from(x) * BigInt::from(y));
                                        sset_local!(frame, dest, result);
                                    }
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                    let r = *x * *y;
                                    let dest_slot = unsafe { frame.locals.get_unchecked_mut(dest) };
                                    if let Some(ref mut arc) = dest_slot {
                                        if let Some(obj) = PyObjectRef::get_mut(arc) {
                                            obj.payload = PyObjectPayload::Float(r);
                                            hot_ok!(profiling, self.profiler, instr.op)
                                        }
                                    }
                                    *dest_slot = Some(PyObject::float(r));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
                                    let r = *x as f64 * *y;
                                    let dest_slot = unsafe { frame.locals.get_unchecked_mut(dest) };
                                    if let Some(ref mut arc) = dest_slot {
                                        if let Some(obj) = PyObjectRef::get_mut(arc) {
                                            obj.payload = PyObjectPayload::Float(r);
                                            hot_ok!(profiling, self.profiler, instr.op)
                                        }
                                    }
                                    *dest_slot = Some(PyObject::float(r));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
                                    let r = *x * *y as f64;
                                    let dest_slot = unsafe { frame.locals.get_unchecked_mut(dest) };
                                    if let Some(ref mut arc) = dest_slot {
                                        if let Some(obj) = PyObjectRef::get_mut(arc) {
                                            obj.payload = PyObjectPayload::Float(r);
                                            hot_ok!(profiling, self.profiler, instr.op)
                                        }
                                    }
                                    *dest_slot = Some(PyObject::float(r));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                _ => {
                                    spush!(frame, a.clone());
                                    spush!(frame, c.clone());
                                    let r = self.execute_one(ferrython_bytecode::Instruction::new(
                                        Opcode::BinaryMultiply, 0));
                                    if r.is_ok() {
                                        let cs_len2 = self.call_stack.len();
                                        let frame2 = unsafe { self.call_stack.get_unchecked_mut(cs_len2 - 1) };
                                        if !frame2.stack.is_empty() {
                                            let val = frame2.stack.pop().unwrap();
                                            unsafe { frame2.set_local_unchecked(dest, val) };
                                        }
                                    }
                                    r
                                }
                            }
                        }
                        None => Err(PyException::name_error(
                            String::from("local variable referenced before assignment"))),
                    }
                }
                // Fused LoadFast + LoadConst + BinarySub + StoreFast (x = x - 1)
                Opcode::LoadFastLoadConstBinarySubStoreFast => {
                    let fast_idx = (instr.arg >> 16) as usize;
                    let const_idx = ((instr.arg >> 8) & 0xFF) as usize;
                    let dest = (instr.arg & 0xFF) as usize;
                    let a = slocal!(frame, fast_idx);
                    let c = unsafe { frame.constant_cache.get_unchecked(const_idx) };
                    match a {
                        Some(a) => {
                            match (&a.payload, &c.payload) {
                                (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                    let (x, y) = (*x, *y);
                                    if let Some(r) = x.checked_sub(y) {
                                        let dest_slot = unsafe { frame.locals.get_unchecked_mut(dest) };
                                        if let Some(ref mut arc) = dest_slot {
                                            if let Some(obj) = PyObjectRef::get_mut(arc) {
                                                obj.payload = PyObjectPayload::Int(PyInt::Small(r));
                                                hot_ok!(profiling, self.profiler, instr.op)
                                            }
                                        }
                                        *dest_slot = Some(PyObject::int(r));
                                    } else {
                                        use num_bigint::BigInt;
                                        let result = PyObject::big_int(BigInt::from(x) - BigInt::from(y));
                                        sset_local!(frame, dest, result);
                                    }
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                    let r = *x - *y;
                                    let dest_slot = unsafe { frame.locals.get_unchecked_mut(dest) };
                                    if let Some(ref mut arc) = dest_slot {
                                        if let Some(obj) = PyObjectRef::get_mut(arc) {
                                            obj.payload = PyObjectPayload::Float(r);
                                            hot_ok!(profiling, self.profiler, instr.op)
                                        }
                                    }
                                    *dest_slot = Some(PyObject::float(r));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
                                    let r = *x as f64 - *y;
                                    let dest_slot = unsafe { frame.locals.get_unchecked_mut(dest) };
                                    if let Some(ref mut arc) = dest_slot {
                                        if let Some(obj) = PyObjectRef::get_mut(arc) {
                                            obj.payload = PyObjectPayload::Float(r);
                                            hot_ok!(profiling, self.profiler, instr.op)
                                        }
                                    }
                                    *dest_slot = Some(PyObject::float(r));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
                                    let r = *x - *y as f64;
                                    let dest_slot = unsafe { frame.locals.get_unchecked_mut(dest) };
                                    if let Some(ref mut arc) = dest_slot {
                                        if let Some(obj) = PyObjectRef::get_mut(arc) {
                                            obj.payload = PyObjectPayload::Float(r);
                                            hot_ok!(profiling, self.profiler, instr.op)
                                        }
                                    }
                                    *dest_slot = Some(PyObject::float(r));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                _ => {
                                    spush!(frame, a.clone());
                                    spush!(frame, c.clone());
                                    let r = self.execute_one(ferrython_bytecode::Instruction::new(
                                        Opcode::BinarySubtract, 0));
                                    if r.is_ok() {
                                        let cs_len2 = self.call_stack.len();
                                        let frame2 = unsafe { self.call_stack.get_unchecked_mut(cs_len2 - 1) };
                                        if !frame2.stack.is_empty() {
                                            let val = frame2.stack.pop().unwrap();
                                            unsafe { frame2.set_local_unchecked(dest, val) };
                                        }
                                    }
                                    r
                                }
                            }
                        }
                        None => Err(PyException::name_error(
                            String::from("local variable referenced before assignment"))),
                    }
                }
                // 6-way fused: x = (x * c1) % c2 — zero stack touch, in-place mutation
                Opcode::LoadFastMulModStoreFast => {
                    let local_idx = (instr.arg >> 24) as usize;
                    let const1_idx = ((instr.arg >> 16) & 0xFF) as usize;
                    let const2_idx = ((instr.arg >> 8) & 0xFF) as usize;
                    let dest = (instr.arg & 0xFF) as usize;
                    let a = slocal!(frame, local_idx);
                    let c1 = unsafe { frame.constant_cache.get_unchecked(const1_idx) };
                    let c2 = unsafe { frame.constant_cache.get_unchecked(const2_idx) };
                    match a {
                        Some(a) => {
                            match (&a.payload, &c1.payload, &c2.payload) {
                                (PyObjectPayload::Int(PyInt::Small(x)),
                                 PyObjectPayload::Int(PyInt::Small(m)),
                                 PyObjectPayload::Int(PyInt::Small(d))) if *d != 0 => {
                                    let (x, m, d) = (*x, *m, *d);
                                    // Compute (x * m) % d with Python semantics
                                    if let Some(product) = x.checked_mul(m) {
                                        // Python modulo: result has same sign as divisor
                                        let r = ((product % d) + d) % d;
                                        let dest_slot = unsafe { frame.locals.get_unchecked_mut(dest) };
                                        if let Some(ref mut arc) = dest_slot {
                                            if let Some(obj) = PyObjectRef::get_mut(arc) {
                                                obj.payload = PyObjectPayload::Int(PyInt::Small(r));
                                                hot_ok!(profiling, self.profiler, instr.op)
                                            }
                                        }
                                        *dest_slot = Some(PyObject::int(r));
                                    } else {
                                        use num_bigint::BigInt;
                                        let product = BigInt::from(x) * BigInt::from(m);
                                        let d_big = BigInt::from(d);
                                        let r = ((&product % &d_big) + &d_big) % &d_big;
                                        let result = PyObject::big_int(r);
                                        sset_local!(frame, dest, result);
                                    }
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                (PyObjectPayload::Float(x),
                                 PyObjectPayload::Float(m),
                                 PyObjectPayload::Float(d)) if *d != 0.0 => {
                                    let product = *x * *m;
                                    let r = product - (product / *d).floor() * *d;
                                    let dest_slot = unsafe { frame.locals.get_unchecked_mut(dest) };
                                    if let Some(ref mut arc) = dest_slot {
                                        if let Some(obj) = PyObjectRef::get_mut(arc) {
                                            obj.payload = PyObjectPayload::Float(r);
                                            hot_ok!(profiling, self.profiler, instr.op)
                                        }
                                    }
                                    *dest_slot = Some(PyObject::float(r));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                _ => {
                                    // Fallback: push operands on stack, execute mul+mod manually
                                    // Clone values we need before any borrows
                                    let a_clone = a.clone();
                                    let c1_clone = c1.clone();
                                    let c2_clone = c2.clone();
                                    spush!(frame, a_clone);
                                    spush!(frame, c1_clone);
                                    let r = self.execute_one(ferrython_bytecode::Instruction::new(
                                        Opcode::BinaryMultiply, 0));
                                    if r.is_ok() {
                                        let cs_len2 = self.call_stack.len();
                                        let frame2 = unsafe { self.call_stack.get_unchecked_mut(cs_len2 - 1) };
                                        spush!(frame2, c2_clone);
                                        let r2 = self.execute_one(ferrython_bytecode::Instruction::new(
                                            Opcode::BinaryModulo, 0));
                                        if r2.is_ok() {
                                            let cs_len3 = self.call_stack.len();
                                            let frame3 = unsafe { self.call_stack.get_unchecked_mut(cs_len3 - 1) };
                                            if !frame3.stack.is_empty() {
                                                let val = frame3.stack.pop().unwrap();
                                                unsafe { frame3.set_local_unchecked(dest, val) };
                                            }
                                        }
                                        r2
                                    } else {
                                        r
                                    }
                                }
                            }
                        }
                        None => Err(PyException::name_error(
                            String::from("local variable referenced before assignment"))),
                    }
                }
                // Fused LoadFast + LoadConst + BinaryMul (pushes result, no store)
                Opcode::LoadFastLoadConstBinaryMul => {
                    let fast_idx = (instr.arg >> 16) as usize;
                    let const_idx = (instr.arg & 0xFFFF) as usize;
                    let a = slocal!(frame, fast_idx);
                    let c = unsafe { frame.constant_cache.get_unchecked(const_idx) };
                    match a {
                        Some(a) => {
                            match (&a.payload, &c.payload) {
                                (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                    let result = match x.checked_mul(*y) {
                                        Some(r) => PyObject::int(r),
                                        None => {
                                            use num_bigint::BigInt;
                                            PyObject::big_int(BigInt::from(*x) * BigInt::from(*y))
                                        }
                                    };
                                    spush!(frame, result);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                    spush!(frame, PyObject::float(*x * *y));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
                                    spush!(frame, PyObject::float(*x as f64 * *y));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
                                    spush!(frame, PyObject::float(*x * *y as f64));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                _ => {
                                    spush!(frame, a.clone());
                                    spush!(frame, c.clone());
                                    self.execute_one(ferrython_bytecode::Instruction::new(
                                        Opcode::BinaryMultiply, 0))
                                }
                            }
                        }
                        None => Err(PyException::name_error(
                            String::from("local variable referenced before assignment"))),
                    }
                }
                Opcode::PopTop => {
                    // SAFETY: stack non-empty for well-formed bytecode
                    drop(spop!(frame));
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                Opcode::PopTopJumpAbsolute => {
                    drop(spop!(frame));
                    frame.ip = instr.arg as usize;
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                Opcode::DupTop => {
                    // SAFETY: stack non-empty; stack pre-allocated
                    let v = unsafe { frame.peek_unchecked() }.clone();
                    spush!(frame, v);
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                Opcode::RotTwo => {
                    let len = frame.stack.len();
                    unsafe { frame.stack.as_mut_ptr().add(len - 1).swap(frame.stack.as_mut_ptr().add(len - 2)) };
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                // RotThree and DupTopTwo: cold, delegate to execute_one
                Opcode::RotThree | Opcode::DupTopTwo => self.execute_one(instr),
                Opcode::Nop => hot_ok!(profiling, self.profiler, instr.op),
                // Inline GetIter for common types
                Opcode::GetIter => {
                    let obj = speek!(frame);
                    match &obj.payload {
                        PyObjectPayload::Iterator(_) | PyObjectPayload::RangeIter { .. } | PyObjectPayload::VecIter(_) | PyObjectPayload::RefIter { .. } | PyObjectPayload::Generator(_) => hot_ok!(profiling, self.profiler, instr.op),
                        PyObjectPayload::List(_) | PyObjectPayload::Tuple(_) | PyObjectPayload::Dict(_) | PyObjectPayload::MappingProxy(_) | PyObjectPayload::DictKeys(_) => {
                            let iter = PyObject::wrap(PyObjectPayload::RefIter {
                                source: obj.clone(), index: SyncUsize::new(0)
                            });
                            let len = frame.stack.len();
                            unsafe { *frame.stack.get_unchecked_mut(len - 1) = iter };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => self.execute_one(instr),
                    }
                }
                // Inline ForIter for Range/List (hot in `for i in range(n)`)
                Opcode::ForIter => {
                    // SAFETY: stack non-empty (iterator on TOS)
                    let iter = unsafe { frame.peek_unchecked() };
                    // Lock-free fast path for RangeIter
                    if let PyObjectPayload::RangeIter { current, stop, step } = &iter.payload {
                        let cur = current.get();
                        let done = if *step > 0 { cur >= *stop } else { cur <= *stop };
                        if done {
                            drop(spop!(frame));
                            frame.ip = instr.arg as usize;
                        } else {
                            let v = PyObject::int(cur);
                            current.set(cur + *step);
                            spush!(frame, v);
                        }
                        hot_ok!(profiling, self.profiler, instr.op)
                    } else if let PyObjectPayload::VecIter(data) = &iter.payload {
                        let idx = data.index.get();
                        if idx < data.items.len() {
                            let v = data.items[idx].clone();
                            data.index.set(idx + 1);
                            spush!(frame, v);
                        } else {
                            drop(spop!(frame));
                            frame.ip = instr.arg as usize;
                        }
                        hot_ok!(profiling, self.profiler, instr.op)
                    } else if let PyObjectPayload::RefIter { source, index } = &iter.payload {
                        let idx = index.get();
                        let item = match &source.payload {
                            PyObjectPayload::List(cell) => {
                                let items = unsafe { &*cell.data_ptr() };
                                if idx < items.len() { Some(items[idx].clone()) } else { None }
                            }
                            PyObjectPayload::Tuple(items) => {
                                if idx < items.len() { Some(items[idx].clone()) } else { None }
                            }
                            PyObjectPayload::Dict(cell) | PyObjectPayload::MappingProxy(cell) | PyObjectPayload::DictKeys(cell) => {
                                let map = unsafe { &*cell.data_ptr() };
                                if idx < map.len() {
                                    Some(map.get_index(idx).unwrap().0.to_object())
                                } else { None }
                            }
                            PyObjectPayload::DictValues(cell) => {
                                let map = unsafe { &*cell.data_ptr() };
                                if idx < map.len() {
                                    Some(map.get_index(idx).unwrap().1.clone())
                                } else { None }
                            }
                            PyObjectPayload::DictItems(cell) => {
                                let map = unsafe { &*cell.data_ptr() };
                                if idx < map.len() {
                                    let (k, v) = map.get_index(idx).unwrap();
                                    Some(PyObject::tuple(vec![k.to_object(), v.clone()]))
                                } else { None }
                            }
                            _ => None,
                        };
                        if let Some(v) = item {
                            index.set(idx + 1);
                            spush!(frame, v);
                        } else {
                            drop(spop!(frame));
                            frame.ip = instr.arg as usize;
                        }
                        hot_ok!(profiling, self.profiler, instr.op)
                    } else if let PyObjectPayload::Iterator(ref iter_data) = iter.payload {
                        let mut data = iter_data.write();
                        match &mut *data {
                            IteratorData::Range { current, stop, step } => {
                                let done = if *step > 0 { *current >= *stop } else { *current <= *stop };
                                if done {
                                    drop(data);
                                    drop(spop!(frame));
                                    frame.ip = instr.arg as usize;
                                } else {
                                    let v = PyObject::int(*current);
                                    *current += *step;
                                    drop(data);
                                    spush!(frame, v);
                                }
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            IteratorData::List { items, index } => {
                                if *index < items.len() {
                                    let v = items[*index].clone();
                                    *index += 1;
                                    drop(data);
                                    spush!(frame, v);
                                } else {
                                    drop(data);
                                    drop(spop!(frame));
                                    frame.ip = instr.arg as usize;
                                }
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            IteratorData::Tuple { items, index } => {
                                if *index < items.len() {
                                    let v = items[*index].clone();
                                    *index += 1;
                                    drop(data);
                                    spush!(frame, v);
                                } else {
                                    drop(data);
                                    drop(spop!(frame));
                                    frame.ip = instr.arg as usize;
                                }
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            IteratorData::Enumerate { source, index, cached_tuple } => {
                                let idx = *index;
                                // Direct RefIter+List/Tuple advance (avoids advance_source_inline overhead)
                                let val_opt: Option<Option<PyObjectRef>> = if let PyObjectPayload::RefIter { source: ref src, index: ref src_idx } = source.payload {
                                    let si = src_idx.get();
                                    match &src.payload {
                                        PyObjectPayload::List(cell) => {
                                            let items = unsafe { &*cell.data_ptr() };
                                            if si < items.len() {
                                                src_idx.set(si + 1);
                                                Some(Some(items[si].clone()))
                                            } else { Some(None) }
                                        }
                                        PyObjectPayload::Tuple(items) => {
                                            if si < items.len() {
                                                src_idx.set(si + 1);
                                                Some(Some(items[si].clone()))
                                            } else { Some(None) }
                                        }
                                        _ => None,
                                    }
                                } else if let PyObjectPayload::VecIter(ref vd) = source.payload {
                                    let si = vd.index.get();
                                    if si < vd.items.len() {
                                        vd.index.set(si + 1);
                                        Some(Some(vd.items[si].clone()))
                                    } else { Some(None) }
                                } else { None };

                                match val_opt {
                                    Some(Some(val)) => {
                                        *index = idx + 1;
                                        let idx_obj = PyObject::int(idx);
                                        // CPython-style tuple reuse: mutate cached tuple in-place
                                        let tuple = if let Some(ref mut cached) = cached_tuple {
                                            if let Some(obj) = PyObjectRef::get_mut(cached) {
                                                if let PyObjectPayload::Tuple(ref mut items) = obj.payload {
                                                    items[0] = idx_obj;
                                                    items[1] = val;
                                                    cached.clone()
                                                } else {
                                                    let t = PyObject::tuple(vec![idx_obj, val]);
                                                    *cached = t.clone();
                                                    t
                                                }
                                            } else {
                                                let t = PyObject::tuple(vec![idx_obj, val]);
                                                *cached = t.clone();
                                                t
                                            }
                                        } else {
                                            let t = PyObject::tuple(vec![idx_obj, val]);
                                            *cached_tuple = Some(t.clone());
                                            t
                                        };
                                        drop(data);
                                        spush!(frame, tuple);
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    }
                                    Some(None) => {
                                        drop(data);
                                        drop(spop!(frame));
                                        frame.ip = instr.arg as usize;
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    }
                                    None => {
                                        // Fallback to advance_source_inline for other source types
                                        match Self::advance_source_inline(source) {
                                            Some(Some(val)) => {
                                                *index = idx + 1;
                                                let idx_obj = PyObject::int(idx);
                                                let tuple = if let Some(ref mut cached) = cached_tuple {
                                                    if let Some(obj) = PyObjectRef::get_mut(cached) {
                                                        if let PyObjectPayload::Tuple(ref mut items) = obj.payload {
                                                            items[0] = idx_obj;
                                                            items[1] = val;
                                                            cached.clone()
                                                        } else {
                                                            let t = PyObject::tuple(vec![idx_obj, val]);
                                                            *cached = t.clone();
                                                            t
                                                        }
                                                    } else {
                                                        let t = PyObject::tuple(vec![idx_obj, val]);
                                                        *cached = t.clone();
                                                        t
                                                    }
                                                } else {
                                                    let t = PyObject::tuple(vec![idx_obj, val]);
                                                    *cached_tuple = Some(t.clone());
                                                    t
                                                };
                                                drop(data);
                                                spush!(frame, tuple);
                                                hot_ok!(profiling, self.profiler, instr.op)
                                            }
                                            Some(None) => {
                                                drop(data);
                                                drop(spop!(frame));
                                                frame.ip = instr.arg as usize;
                                                hot_ok!(profiling, self.profiler, instr.op)
                                            }
                                            None => {
                                                drop(data);
                                                self.execute_one(instr)
                                            }
                                        }
                                    }
                                }
                            }
                            IteratorData::Zip { sources, strict, cached_tuple, items_buf } => {
                                let is_strict = *strict;
                                let n = sources.len();

                                // ── 2-source fast path (most common: zip(a, b)) ──
                                if n == 2 {
                                    // Inline RefIter+List advancement: avoids 4 enum matches
                                    // in advance_source_inline per iteration.
                                    let (v0, v1) = 'zip_inline: {
                                        if let (
                                            PyObjectPayload::RefIter { source: ref src0, index: ref idx0 },
                                            PyObjectPayload::RefIter { source: ref src1, index: ref idx1 },
                                        ) = (&sources[0].payload, &sources[1].payload) {
                                            if let (
                                                PyObjectPayload::List(ref cell0),
                                                PyObjectPayload::List(ref cell1),
                                            ) = (&src0.payload, &src1.payload) {
                                                let items0 = unsafe { &*cell0.data_ptr() };
                                                let items1 = unsafe { &*cell1.data_ptr() };
                                                let i0 = idx0.get();
                                                let i1 = idx1.get();
                                                if i0 < items0.len() && i1 < items1.len() {
                                                    idx0.set(i0 + 1);
                                                    idx1.set(i1 + 1);
                                                    break 'zip_inline (Some(Some(items0[i0].clone())), Some(Some(items1[i1].clone())));
                                                } else {
                                                    let e0 = if i0 >= items0.len() { Some(None) } else { Some(Some(items0[i0].clone())) };
                                                    let e1 = if i1 >= items1.len() { Some(None) } else { Some(Some(items1[i1].clone())) };
                                                    if e0.as_ref().map_or(false, |v| v.is_some()) { idx0.set(i0 + 1); }
                                                    if e1.as_ref().map_or(false, |v| v.is_some()) { idx1.set(i1 + 1); }
                                                    break 'zip_inline (e0, e1);
                                                }
                                            }
                                        }
                                        // Fallback to generic advance
                                        (Self::advance_source_inline(&sources[0]),
                                         Self::advance_source_inline(&sources[1]))
                                    };
                                    match (v0, v1) {
                                        (Some(Some(a)), Some(Some(b))) => {
                                            // Both sources yielded — reuse cached tuple
                                            let tuple = if let Some(ref mut cached) = cached_tuple {
                                                if let Some(obj) = PyObjectRef::get_mut(cached) {
                                                    if let PyObjectPayload::Tuple(ref mut items) = obj.payload {
                                                        items[0] = a;
                                                        items[1] = b;
                                                        cached.clone()
                                                    } else {
                                                        let t = PyObject::tuple(vec![a, b]);
                                                        *cached = t.clone();
                                                        t
                                                    }
                                                } else {
                                                    let t = PyObject::tuple(vec![a, b]);
                                                    *cached = t.clone();
                                                    t
                                                }
                                            } else {
                                                let t = PyObject::tuple(vec![a, b]);
                                                *cached_tuple = Some(t.clone());
                                                t
                                            };
                                            drop(data);
                                            spush!(frame, tuple);
                                            hot_ok!(profiling, self.profiler, instr.op)
                                        }
                                        (Some(None), Some(None)) if is_strict => {
                                            drop(data);
                                            drop(spop!(frame));
                                            frame.ip = instr.arg as usize;
                                            hot_ok!(profiling, self.profiler, instr.op)
                                        }
                                        (Some(None), _) | (_, Some(None)) if is_strict => {
                                            drop(data);
                                            return Err(PyException::value_error(
                                                "zip() has arguments with different lengths"));
                                        }
                                        (Some(None), _) | (_, Some(None)) => {
                                            drop(data);
                                            drop(spop!(frame));
                                            frame.ip = instr.arg as usize;
                                            hot_ok!(profiling, self.profiler, instr.op)
                                        }
                                        _ => {
                                            drop(data);
                                            self.execute_one(instr)
                                        }
                                    }
                                } else {
                                // ── General N-source path (reuse items_buf) ──
                                items_buf.clear();
                                let mut all_ok = true;
                                let mut exhausted_count = 0usize;
                                let mut needs_vm = false;
                                for src in sources.iter() {
                                    match Self::advance_source_inline(src) {
                                        Some(Some(val)) => items_buf.push(val),
                                        Some(None) => {
                                            exhausted_count += 1;
                                            if is_strict {
                                                items_buf.push(PyObject::none());
                                            } else {
                                                all_ok = false;
                                                break;
                                            }
                                        }
                                        None => {
                                            needs_vm = true;
                                            break;
                                        }
                                    }
                                }
                                if needs_vm {
                                    items_buf.clear();
                                    drop(data);
                                    self.execute_one(instr)
                                } else {
                                    if !all_ok || (is_strict && exhausted_count > 0 && exhausted_count == n) {
                                        if is_strict && exhausted_count > 0 && exhausted_count != n {
                                            items_buf.clear();
                                            drop(data);
                                            return Err(PyException::value_error(
                                                "zip() has arguments with different lengths"));
                                        }
                                        items_buf.clear();
                                        drop(data);
                                        drop(spop!(frame));
                                        frame.ip = instr.arg as usize;
                                    } else {
                                        // Tuple reuse
                                        let buf_len = items_buf.len();
                                        let tuple = if buf_len == n {
                                            if let Some(ref mut cached) = cached_tuple {
                                                if let Some(obj) = PyObjectRef::get_mut(cached) {
                                                    if let PyObjectPayload::Tuple(ref mut items) = obj.payload {
                                                        if items.len() == n {
                                                            for (i, v) in items_buf.drain(..).enumerate() {
                                                                items[i] = v;
                                                            }
                                                            cached.clone()
                                                        } else {
                                                            let buf: Vec<_> = items_buf.drain(..).collect();
                                                            let t = PyObject::tuple(buf);
                                                            *cached = t.clone();
                                                            t
                                                        }
                                                    } else {
                                                        let buf: Vec<_> = items_buf.drain(..).collect();
                                                        let t = PyObject::tuple(buf);
                                                        *cached = t.clone();
                                                        t
                                                    }
                                                } else {
                                                    let buf: Vec<_> = items_buf.drain(..).collect();
                                                    let t = PyObject::tuple(buf);
                                                    *cached = t.clone();
                                                    t
                                                }
                                            } else {
                                                let buf: Vec<_> = items_buf.drain(..).collect();
                                                let t = PyObject::tuple(buf);
                                                *cached_tuple = Some(t.clone());
                                                t
                                            }
                                        } else {
                                            let buf: Vec<_> = items_buf.drain(..).collect();
                                            PyObject::tuple(buf)
                                        };
                                        drop(data);
                                        spush!(frame, tuple);
                                    }
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                } // close else (N-source path)
                            }
                            IteratorData::DictEntries { source, index, cached_tuple } => {
                                let map = unsafe { &*source.data_ptr() };
                                // Skip hidden keys
                                while *index < map.len() {
                                    let (hk, _) = map.get_index(*index).unwrap();
                                    if !is_hidden_dict_key(hk) { break; }
                                    *index += 1;
                                }
                                if *index < map.len() {
                                    let (hk, v) = map.get_index(*index).unwrap();
                                    let k = hk.to_object();
                                    let v = v.clone();
                                    *index += 1;
                                    // Reuse cached tuple when consumer has dropped its ref (refcount == 1)
                                    let tuple = if let Some(ref ct) = cached_tuple {
                                        if PyObjectRef::strong_count(ct) == 1 {
                                            // Mutate in-place via raw pointer: only cached_tuple holds a ref
                                            unsafe {
                                                let obj_ptr = PyObjectRef::as_ptr(ct) as *mut PyObject;
                                                if let PyObjectPayload::Tuple(ref mut items) = (*obj_ptr).payload {
                                                    items[0] = k;
                                                    items[1] = v;
                                                }
                                            }
                                            ct.clone()
                                        } else {
                                            let t = PyObject::tuple(vec![k, v]);
                                            *cached_tuple = Some(t.clone());
                                            t
                                        }
                                    } else {
                                        let t = PyObject::tuple(vec![k, v]);
                                        *cached_tuple = Some(t.clone());
                                        t
                                    };
                                    drop(data);
                                    spush!(frame, tuple);
                                } else {
                                    drop(data);
                                    drop(spop!(frame));
                                    frame.ip = instr.arg as usize;
                                }
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            IteratorData::DictKeys { keys, index } => {
                                if *index < keys.len() {
                                    let obj = keys[*index].clone();
                                    *index += 1;
                                    drop(data);
                                    spush!(frame, obj);
                                } else {
                                    drop(data);
                                    drop(spop!(frame));
                                    frame.ip = instr.arg as usize;
                                }
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            _ => {
                                drop(data);
                                self.execute_one(instr)
                            }
                        }
                    } else if let PyObjectPayload::Generator(ref gen_arc) = iter.payload {
                        // Inline generator iteration: avoids execute_one dispatch +
                        // StopIteration exception allocation on generator completion.
                        let gen_arc = gen_arc.clone();
                        match self.resume_generator_for_iter(&gen_arc) {
                            Ok(Some(value)) => {
                                // Re-derive frame after resume (call_stack may have reallocated)
                                rederive_frame!(self, frame_ptr, instr_base, instr_count);
                                let frame = unsafe { &mut *frame_ptr };
                                spush!(frame, value);
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            Ok(None) => {
                                // Generator exhausted — no exception needed
                                rederive_frame!(self, frame_ptr, instr_base, instr_count);
                                let frame = unsafe { &mut *frame_ptr };
                                drop(spop!(frame)); // remove generator from stack
                                frame.ip = instr.arg as usize;
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            Err(e) => Err(e),
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                // ForIter + StoreFast fused: store directly to local, no stack push/pop
                Opcode::ForIterStoreFast => {
                    let jump_target = (instr.arg >> 16) as usize;
                    let store_idx = (instr.arg & 0xFFFF) as usize;
                    let iter = unsafe { frame.peek_unchecked() };
                    // Lock-free fast path for RangeIter
                    if let PyObjectPayload::RangeIter { current, stop, step } = &iter.payload {
                        let cur = current.get();
                        let done = if *step > 0 { cur >= *stop } else { cur <= *stop };
                        if done {
                            drop(spop!(frame));
                            frame.ip = jump_target;
                        } else {
                            current.set(cur + *step);
                            // Try in-place mutation if dest holds sole reference
                            let dest_slot = unsafe { frame.locals.get_unchecked_mut(store_idx) };
                            if let Some(ref mut arc) = dest_slot {
                                if let Some(obj) = PyObjectRef::get_mut(arc) {
                                    obj.payload = PyObjectPayload::Int(PyInt::Small(cur));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                            }
                            *dest_slot = Some(PyObject::int(cur));
                        }
                        hot_ok!(profiling, self.profiler, instr.op)
                    } else if let PyObjectPayload::VecIter(data) = &iter.payload {
                        let idx = data.index.get();
                        if idx < data.items.len() {
                            let obj = data.items[idx].clone();
                            data.index.set(idx + 1);
                            sset_local!(frame, store_idx, obj);
                        } else {
                            drop(spop!(frame));
                            frame.ip = jump_target;
                        }
                        hot_ok!(profiling, self.profiler, instr.op)
                    } else if let PyObjectPayload::RefIter { source, index } = &iter.payload {
                        let idx = index.get();
                        let item = match &source.payload {
                            PyObjectPayload::List(cell) => {
                                let items = unsafe { &*cell.data_ptr() };
                                if idx < items.len() { Some(items[idx].clone()) } else { None }
                            }
                            PyObjectPayload::Tuple(items) => {
                                if idx < items.len() { Some(items[idx].clone()) } else { None }
                            }
                            PyObjectPayload::Dict(cell) | PyObjectPayload::MappingProxy(cell) | PyObjectPayload::DictKeys(cell) => {
                                let map = unsafe { &*cell.data_ptr() };
                                if idx < map.len() {
                                    Some(map.get_index(idx).unwrap().0.to_object())
                                } else { None }
                            }
                            PyObjectPayload::DictValues(cell) => {
                                let map = unsafe { &*cell.data_ptr() };
                                if idx < map.len() {
                                    Some(map.get_index(idx).unwrap().1.clone())
                                } else { None }
                            }
                            PyObjectPayload::DictItems(cell) => {
                                let map = unsafe { &*cell.data_ptr() };
                                if idx < map.len() {
                                    let (k, v) = map.get_index(idx).unwrap();
                                    Some(PyObject::tuple(vec![k.to_object(), v.clone()]))
                                } else { None }
                            }
                            _ => None,
                        };
                        if let Some(v) = item {
                            index.set(idx + 1);
                            sset_local!(frame, store_idx, v);
                        } else {
                            drop(spop!(frame));
                            frame.ip = jump_target;
                        }
                        hot_ok!(profiling, self.profiler, instr.op)
                    } else if let PyObjectPayload::Iterator(ref iter_data) = iter.payload {
                        let mut data = iter_data.write();
                        match &mut *data {
                            IteratorData::Range { current, stop, step } => {
                                let done = if *step > 0 { *current >= *stop } else { *current <= *stop };
                                if done {
                                    drop(data);
                                    drop(spop!(frame));
                                    frame.ip = jump_target;
                                } else {
                                    let v = PyObject::int(*current);
                                    *current += *step;
                                    drop(data);
                                    sset_local!(frame, store_idx, v);
                                }
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            IteratorData::List { items, index } => {
                                if *index < items.len() {
                                    let v = items[*index].clone();
                                    *index += 1;
                                    drop(data);
                                    sset_local!(frame, store_idx, v);
                                } else {
                                    drop(data);
                                    drop(spop!(frame));
                                    frame.ip = jump_target;
                                }
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            IteratorData::Tuple { items, index } => {
                                if *index < items.len() {
                                    let v = items[*index].clone();
                                    *index += 1;
                                    drop(data);
                                    sset_local!(frame, store_idx, v);
                                } else {
                                    drop(data);
                                    drop(spop!(frame));
                                    frame.ip = jump_target;
                                }
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            IteratorData::DictKeys { keys, index } => {
                                if *index < keys.len() {
                                    let obj = keys[*index].clone();
                                    *index += 1;
                                    drop(data);
                                    sset_local!(frame, store_idx, obj);
                                } else {
                                    drop(data);
                                    drop(spop!(frame));
                                    frame.ip = jump_target;
                                }
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            _ => {
                                // Fallback: execute as ForIter, then the StoreFast
                                drop(data);
                                let for_instr = ferrython_bytecode::Instruction::new(
                                    Opcode::ForIter, jump_target as u32);
                                self.execute_one(for_instr)?;
                                let frame = self.call_stack.last_mut().unwrap();
                                // If ForIter didn't jump (value was pushed), store it
                                if frame.ip != jump_target {
                                    let v = spop!(frame);
                                    sset_local!(frame, store_idx, v);
                                }
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                        }
                    } else if let PyObjectPayload::Generator(ref gen_arc) = iter.payload {
                        // Inline generator in ForIterStoreFast
                        let gen_arc = gen_arc.clone();
                        match self.resume_generator_for_iter(&gen_arc) {
                            Ok(Some(value)) => {
                                rederive_frame!(self, frame_ptr, instr_base, instr_count);
                                let frame = unsafe { &mut *frame_ptr };
                                sset_local!(frame, store_idx, value);
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            Ok(None) => {
                                rederive_frame!(self, frame_ptr, instr_base, instr_count);
                                let frame = unsafe { &mut *frame_ptr };
                                drop(spop!(frame)); // remove generator
                                frame.ip = jump_target;
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            Err(e) => Err(e),
                        }
                    } else {
                        // Fallback for non-iterator types
                        let for_instr = ferrython_bytecode::Instruction::new(
                            Opcode::ForIter, jump_target as u32);
                        self.execute_one(for_instr)?;
                        let frame = self.call_stack.last_mut().unwrap();
                        if frame.ip != jump_target {
                            let v = spop!(frame);
                            sset_local!(frame, store_idx, v);
                        }
                        hot_ok!(profiling, self.profiler, instr.op)
                    }
                }
                // Inline ReturnValue: fast path when no finally blocks are active
                Opcode::ReturnValue => {
                    if frame.block_stack.is_empty() {
                        // SAFETY: stack non-empty for well-formed bytecode
                        let val = spop!(frame);
                        // __init__ must return None — check here so Err flows
                        // through the normal exception unwind (try/except catches it)
                        if frame.discard_return && !matches!(&val.payload, PyObjectPayload::None) {
                            Err(PyException::type_error(
                                "__init__() should return None".to_string()
                            ))
                        } else {
                            Ok(Some(val))
                        }
                    } else if frame.block_stack.iter().any(|b| b.kind() == BlockKind::Finally) {
                        self.execute_one(instr)
                    } else {
                        // SAFETY: stack non-empty for well-formed bytecode
                        let val = spop!(frame);
                        if frame.discard_return && !matches!(&val.payload, PyObjectPayload::None) {
                            Err(PyException::type_error(
                                "__init__() should return None".to_string()
                            ))
                        } else {
                            Ok(Some(val))
                        }
                    }
                }
                // Fused LoadFast + ReturnValue — common `return x` pattern
                Opcode::LoadFastReturnValue => {
                    if frame.block_stack.is_empty() {
                        match slocal!(frame, instr.arg as usize) {
                            Some(val) => {
                                let val = val.clone();
                                if frame.discard_return && !matches!(&val.payload, PyObjectPayload::None) {
                                    Err(PyException::type_error(
                                        "__init__() should return None".to_string()
                                    ))
                                } else {
                                    Ok(Some(val))
                                }
                            }
                            None => Self::err_unbound_local(&frame.code.varnames, instr.arg as usize),
                        }
                    } else {
                        // Fallback: push to stack and use normal ReturnValue
                        match slocal!(frame, instr.arg as usize) {
                            Some(val) => {
                                spush!(frame, val.clone());
                                self.execute_one(ferrython_bytecode::Instruction::new(
                                    Opcode::ReturnValue, 0))
                            }
                            None => Self::err_unbound_local(&frame.code.varnames, instr.arg as usize),
                        }
                    }
                }
                // Fused LoadConst + ReturnValue — common `return 0`, `return None`
                Opcode::LoadConstReturnValue => {
                    if frame.block_stack.is_empty() {
                        let val = unsafe { frame.constant_cache.get_unchecked(instr.arg as usize) };
                        if frame.discard_return && !matches!(&val.payload, PyObjectPayload::None) {
                            Err(PyException::type_error(
                                "__init__() should return None".to_string()
                            ))
                        } else {
                            Ok(Some(val.clone()))
                        }
                    } else {
                        let val = unsafe { frame.constant_cache.get_unchecked(instr.arg as usize) };
                        spush!(frame, val.clone());
                        self.execute_one(ferrython_bytecode::Instruction::new(
                            Opcode::ReturnValue, 0))
                    }
                }

                // Fused LoadConst + StoreFast — common in initialization (`x = 0`, `s = ""`)
                Opcode::LoadConstStoreFast => {
                    let const_idx = (instr.arg >> 16) as usize;
                    let store_idx = (instr.arg & 0xFFFF) as usize;
                    let const_ref = unsafe { frame.constant_cache.get_unchecked(const_idx) };
                    // In-place mutation: if dest holds same type with refcount 1, overwrite payload
                    let dest_slot = unsafe { frame.locals.get_unchecked_mut(store_idx) };
                    if let Some(ref mut arc) = dest_slot {
                        if let Some(obj) = PyObjectRef::get_mut(arc) {
                            match (&const_ref.payload, &mut obj.payload) {
                                (PyObjectPayload::Int(src), PyObjectPayload::Int(dst)) => {
                                    *dst = src.clone();
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                (PyObjectPayload::Bool(src), PyObjectPayload::Bool(dst)) => {
                                    *dst = *src;
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                (PyObjectPayload::None, PyObjectPayload::None) => {
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                (PyObjectPayload::Float(src), PyObjectPayload::Float(dst)) => {
                                    *dst = *src;
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                _ => {}
                            }
                        }
                    }
                    // Fallback: clone constant
                    let val = const_ref.clone();
                    *unsafe { frame.locals.get_unchecked_mut(store_idx) } = Some(val);
                    hot_ok!(profiling, self.profiler, instr.op)
                }

                // Inline int+int for BinaryAdd (hot in arithmetic loops)
                Opcode::BinaryAdd | Opcode::InplaceAdd => {
                    let len = frame.stack.len();
                    // SAFETY: well-formed bytecode guarantees stack depth >= 2
                    let a = sget!(frame, len - 2);
                    let b = sget!(frame, len - 1);
                    match (&a.payload, &b.payload) {
                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                            let result = match x.checked_add(*y) {
                                Some(r) => PyObject::int(r),
                                None => {
                                    use num_bigint::BigInt;
                                    PyObject::big_int(BigInt::from(*x) + BigInt::from(*y))
                                }
                            };
                            unsafe { frame.binary_op_result(result) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                            let r = *x + *y;
                            unsafe { frame.binary_op_result(PyObject::float(r)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
                            let r = *x as f64 + *y;
                            unsafe { frame.binary_op_result(PyObject::float(r)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
                            let r = *x + *y as f64;
                            unsafe { frame.binary_op_result(PyObject::float(r)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Str(_x), PyObjectPayload::Str(y)) => {
                            // CPython-style in-place string resize: if the LHS string has
                            // refcount 2 (one on stack, one in a local) and the next
                            // instruction is STORE_FAST, clear the local to get refcount 1,
                            // then do push_str in place — avoids allocation per concat.
                            let rhs: CompactString = y.clone(); // cheap for small strings (inline)
                            let len = frame.stack.len();
                            unsafe {
                                let _b_arc = std::ptr::read(frame.stack.as_ptr().add(len - 1));
                                let mut a_arc = std::ptr::read(frame.stack.as_ptr().add(len - 2));
                                frame.stack.set_len(len - 2);
                                drop(_b_arc);
                                // If refcount == 2, try to clear the local to get refcount 1
                                if PyObjectRef::strong_count(&a_arc) == 2 && frame.ip < frame.code.instructions.len() {
                                    let next = *frame.code.instructions.get_unchecked(frame.ip);
                                    let store_idx_opt = match next.op {
                                        Opcode::StoreFast => Some(next.arg as usize),
                                        Opcode::StoreFastLoadFast | Opcode::StoreFastJumpAbsolute
                                            => Some((next.arg >> 16) as usize),
                                        Opcode::LoadConstStoreFast => Some((next.arg & 0xFFFF) as usize),
                                        _ => None,
                                    };
                                    if let Some(store_idx) = store_idx_opt {
                                        let slot = frame.locals.get_unchecked_mut(store_idx);
                                        if let Some(ref existing) = slot {
                                            if PyObjectRef::ptr_eq(existing, &a_arc) {
                                                *slot = None; // drop ref → refcount becomes 1
                                            }
                                        }
                                    }
                                }
                                // Now try in-place mutation
                                if let Some(obj) = PyObjectRef::get_mut(&mut a_arc) {
                                    if let PyObjectPayload::Str(ref mut s) = obj.payload {
                                        s.push_str(&rhs);
                                        frame.stack.push(a_arc);
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    }
                                }
                                // Fallback: allocate new string
                                let new_s = if let PyObjectPayload::Str(ref x) = a_arc.payload {
                                    let mut s = String::with_capacity(x.len() + rhs.len());
                                    s.push_str(x);
                                    s.push_str(&rhs);
                                    CompactString::from(s)
                                } else { unreachable!() };
                                frame.stack.push(PyObject::str_val(new_s));
                            }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::List(x), PyObjectPayload::List(y)) => {
                            let mut items = unsafe { &*x.data_ptr() }.clone();
                            items.extend(unsafe { &*y.data_ptr() }.iter().cloned());
                            unsafe { frame.binary_op_result(PyObject::list(items)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Tuple(x), PyObjectPayload::Tuple(y)) => {
                            let mut items = x.to_vec();
                            items.extend(y.iter().cloned());
                            unsafe { frame.binary_op_result(PyObject::tuple(items)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => self.execute_one(instr),
                    }
                }
                // Inline int/float subtract and multiply (hot in numeric code)
                Opcode::BinarySubtract | Opcode::InplaceSubtract => {
                    let len = frame.stack.len();
                    let a = sget!(frame, len - 2);
                    let b = sget!(frame, len - 1);
                    match (&a.payload, &b.payload) {
                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                            let result = match x.checked_sub(*y) {
                                Some(r) => PyObject::int(r),
                                None => {
                                    use num_bigint::BigInt;
                                    PyObject::big_int(BigInt::from(*x) - BigInt::from(*y))
                                }
                            };
                            unsafe { frame.binary_op_result(result) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                            unsafe { frame.binary_op_result(PyObject::float(*x - *y)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
                            unsafe { frame.binary_op_result(PyObject::float(*x as f64 - *y)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
                            unsafe { frame.binary_op_result(PyObject::float(*x - *y as f64)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => self.execute_one(instr),
                    }
                }
                Opcode::BinaryMultiply | Opcode::InplaceMultiply => {
                    let len = frame.stack.len();
                    let a = sget!(frame, len - 2);
                    let b = sget!(frame, len - 1);
                    match (&a.payload, &b.payload) {
                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                            let result = match x.checked_mul(*y) {
                                Some(r) => PyObject::int(r),
                                None => {
                                    use num_bigint::BigInt;
                                    PyObject::big_int(BigInt::from(*x) * BigInt::from(*y))
                                }
                            };
                            unsafe { frame.binary_op_result(result) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                            unsafe { frame.binary_op_result(PyObject::float(*x * *y)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
                            unsafe { frame.binary_op_result(PyObject::float(*x as f64 * *y)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
                            unsafe { frame.binary_op_result(PyObject::float(*x * *y as f64)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => self.execute_one(instr),
                    }
                }
                Opcode::BinaryModulo | Opcode::InplaceModulo => {
                    let len = frame.stack.len();
                    let a = sget!(frame, len - 2);
                    let b = sget!(frame, len - 1);
                    match (&a.payload, &b.payload) {
                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) if *y != 0 => {
                            // Python modulo: result has same sign as divisor
                            // Fast path: both non-negative → single modulo
                            let r = if *x >= 0 && *y > 0 {
                                *x % *y
                            } else {
                                ((*x % *y) + *y) % *y
                            };
                            unsafe { frame.binary_op_result(PyObject::int(r)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) if *y != 0.0 => {
                            let r = *x - (*x / *y).floor() * *y;
                            unsafe { frame.binary_op_result(PyObject::float(r)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => self.execute_one(instr),
                    }
                }
                Opcode::BinaryFloorDivide | Opcode::InplaceFloorDivide => {
                    let len = frame.stack.len();
                    let a = sget!(frame, len - 2);
                    let b = sget!(frame, len - 1);
                    match (&a.payload, &b.payload) {
                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) if *y != 0 => {
                            // Python floor division: round towards -infinity
                            let (d, m) = (x.div_euclid(*y), x.rem_euclid(*y));
                            let r = if m != 0 && (*x ^ *y) < 0 { d - 1 } else { d };
                            unsafe { frame.binary_op_result(PyObject::int(r)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) if *y != 0.0 => {
                            unsafe { frame.binary_op_result(PyObject::float((*x / *y).floor())) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => self.execute_one(instr),
                    }
                }
                Opcode::BinaryTrueDivide | Opcode::InplaceTrueDivide => {
                    let len = frame.stack.len();
                    let a = sget!(frame, len - 2);
                    let b = sget!(frame, len - 1);
                    match (&a.payload, &b.payload) {
                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) if *y != 0 => {
                            unsafe { frame.binary_op_result(PyObject::float(*x as f64 / *y as f64)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) if *y != 0.0 => {
                            unsafe { frame.binary_op_result(PyObject::float(*x / *y)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) if *y != 0.0 => {
                            unsafe { frame.binary_op_result(PyObject::float(*x as f64 / *y)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) if *y != 0 => {
                            unsafe { frame.binary_op_result(PyObject::float(*x / *y as f64)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => self.execute_one(instr),
                    }
                }
                // Inline int comparisons (hot in for-loop range iteration)
                Opcode::CompareOp if instr.arg <= 5 => {
                    let len = frame.stack.len();
                    // SAFETY: well-formed bytecode guarantees stack depth >= 2
                    let a = sget!(frame, len - 2);
                    let b = sget!(frame, len - 1);
                    // Arc pointer equality fast-path: same object → equal
                    if (instr.arg == 2 || instr.arg == 3) && PyObjectRef::ptr_eq(a, b) {
                        let result = instr.arg == 2; // Eq=true, Ne=false
                        unsafe { frame.binary_op_result(PyObject::bool_val(result)) };
                        hot_ok!(profiling, self.profiler, instr.op)
                    } else {
                    match (&a.payload, &b.payload) {
                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                            let result = match instr.arg {
                                0 => x < y,  // Lt
                                1 => x <= y, // Le
                                2 => x == y, // Eq
                                3 => x != y, // Ne
                                4 => x > y,  // Gt
                                _ => x >= y, // Ge (5)
                            };
                            unsafe { frame.binary_op_result(PyObject::bool_val(result)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                            let (xv, yv) = (*x, *y);
                            let result = match instr.arg {
                                0 => xv < yv,
                                1 => xv <= yv,
                                2 => xv == yv,
                                3 => xv != yv,
                                4 => xv > yv,
                                _ => xv >= yv,
                            };
                            unsafe { frame.binary_op_result(PyObject::bool_val(result)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        // String equality (hot for dict lookups, isinstance checks)
                        (PyObjectPayload::Str(x), PyObjectPayload::Str(y)) if instr.arg == 2 || instr.arg == 3 => {
                            let eq = x == y;
                            let result = if instr.arg == 2 { eq } else { !eq };
                            unsafe { frame.binary_op_result(PyObject::bool_val(result)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => self.execute_one(instr),
                    }
                    }
                }
                // Inline is/is not comparisons (CompareOp arg 8/9)
                Opcode::CompareOp if instr.arg == 8 || instr.arg == 9 => {
                    let len = frame.stack.len();
                    // SAFETY: well-formed bytecode guarantees stack depth >= 2
                    let a = sget!(frame, len - 2);
                    let b = sget!(frame, len - 1);
                    let same = PyObjectRef::ptr_eq(a, b)
                        || matches!((&a.payload, &b.payload),
                            (PyObjectPayload::BuiltinType(at), PyObjectPayload::BuiltinType(bt)) if at == bt)
                        || matches!((&a.payload, &b.payload),
                            (PyObjectPayload::ExceptionType(at), PyObjectPayload::ExceptionType(bt)) if at == bt);
                    let result = if instr.arg == 8 { same } else { !same };
                    unsafe { frame.binary_op_result(PyObject::bool_val(result)) };
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                // Inline 'in' / 'not in' for dict, set, list, tuple, str (CompareOp arg 6/7)
                Opcode::CompareOp if instr.arg == 6 || instr.arg == 7 => {
                    let len = frame.stack.len();
                    let needle = sget!(frame, len - 2);
                    let haystack = sget!(frame, len - 1);
                    let found = match &haystack.payload {
                        PyObjectPayload::Dict(map) => {
                            let r = unsafe { &*map.data_ptr() };
                            let found = match &needle.payload {
                                PyObjectPayload::Str(s) => Some(r.contains_key(&BorrowedStrKey(s.as_str()))),
                                PyObjectPayload::Int(PyInt::Small(n)) => Some(r.contains_key(&BorrowedIntKey(*n))),
                                PyObjectPayload::Bool(b) => Some(r.contains_key(&BorrowedIntKey(*b as i64))),
                                _ => None,
                            };
                            found
                        }
                        PyObjectPayload::Set(items) => {
                            let r = unsafe { &*items.data_ptr() };
                            match &needle.payload {
                                PyObjectPayload::Str(s) => Some(r.contains_key(&BorrowedStrKey(s.as_str()))),
                                PyObjectPayload::Int(PyInt::Small(n)) => Some(r.contains_key(&BorrowedIntKey(*n))),
                                PyObjectPayload::Bool(b) => Some(r.contains_key(&BorrowedIntKey(*b as i64))),
                                _ => {
                                    if let Ok(hk) = HashableKey::from_object(needle) {
                                        Some(r.contains_key(&hk))
                                    } else { None }
                                }
                            }
                        }
                        PyObjectPayload::List(items) => {
                            let items = unsafe { &*items.data_ptr() };
                            Some(items.iter().any(|x| {
                                match (&x.payload, &needle.payload) {
                                    (PyObjectPayload::Int(PyInt::Small(a)), PyObjectPayload::Int(PyInt::Small(b))) => a == b,
                                    (PyObjectPayload::Str(a), PyObjectPayload::Str(b)) => a == b,
                                    (PyObjectPayload::Bool(a), PyObjectPayload::Bool(b)) => a == b,
                                    (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => a == b,
                                    (PyObjectPayload::None, PyObjectPayload::None) => true,
                                    (PyObjectPayload::Tuple(a), PyObjectPayload::Tuple(b)) => {
                                        a.len() == b.len() && a.iter().zip(b.iter()).all(|(ai, bi)| {
                                            ferrython_core::object::helpers::partial_cmp_objects(ai, bi) == Some(std::cmp::Ordering::Equal)
                                        })
                                    }
                                    _ => PyObjectRef::ptr_eq(x, needle),
                                }
                            }))
                        }
                        PyObjectPayload::Tuple(items) => {
                            Some(items.iter().any(|x| {
                                match (&x.payload, &needle.payload) {
                                    (PyObjectPayload::Int(PyInt::Small(a)), PyObjectPayload::Int(PyInt::Small(b))) => a == b,
                                    (PyObjectPayload::Str(a), PyObjectPayload::Str(b)) => a == b,
                                    (PyObjectPayload::Bool(a), PyObjectPayload::Bool(b)) => a == b,
                                    (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => a == b,
                                    (PyObjectPayload::None, PyObjectPayload::None) => true,
                                    (PyObjectPayload::Tuple(a), PyObjectPayload::Tuple(b)) => {
                                        a.len() == b.len() && a.iter().zip(b.iter()).all(|(ai, bi)| {
                                            ferrython_core::object::helpers::partial_cmp_objects(ai, bi) == Some(std::cmp::Ordering::Equal)
                                        })
                                    }
                                    _ => PyObjectRef::ptr_eq(x, needle),
                                }
                            }))
                        }
                        PyObjectPayload::Str(haystack_s) => {
                            if let PyObjectPayload::Str(needle_s) = &needle.payload {
                                Some(haystack_s.contains(needle_s.as_str()))
                            } else { None }
                        }
                        _ => None,
                    };
                    if let Some(is_in) = found {
                        let result = if instr.arg == 6 { is_in } else { !is_in };
                        unsafe { frame.binary_op_result(PyObject::bool_val(result)) };
                        hot_ok!(profiling, self.profiler, instr.op)
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline LoadGlobal: check per-frame cache, then globals, then builtins
                Opcode::LoadGlobal => {
                    let idx = instr.arg as usize;
                    let ver = crate::frame::globals_version();
                    // Fast path: cache hit
                    if frame.global_cache_version == ver {
                        if let Some(ref cache) = frame.global_cache {
                            // SAFETY: compiler guarantees idx < code.names.len() == cache.len()
                            if let Some(ref v) = unsafe { cache.get_unchecked(idx) } {
                                spush!(frame, v.clone());
                                hot_ok!(profiling, self.profiler, instr.op)
                            } else {
                                self.execute_one(instr) // miss — fall through to full handler
                            }
                        } else {
                            self.execute_one(instr)
                        }
                    } else {
                        self.execute_one(instr) // version mismatch
                    }
                }
                // Inline PopJumpIfFalse for primitive types (hot in conditionals/loops)
                Opcode::PopJumpIfFalse => {
                    // SAFETY: stack non-empty for well-formed bytecode
                    let v = spop!(frame);
                    match &v.payload {
                        PyObjectPayload::Bool(b) => {
                            if !b { frame.ip = instr.arg as usize; }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        PyObjectPayload::None => {
                            frame.ip = instr.arg as usize;
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        PyObjectPayload::Int(PyInt::Small(n)) => {
                            if *n == 0 { frame.ip = instr.arg as usize; }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        PyObjectPayload::Str(s) => {
                            if s.is_empty() { frame.ip = instr.arg as usize; }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        PyObjectPayload::List(items) => {
                            if unsafe { &*items.data_ptr() }.is_empty() { frame.ip = instr.arg as usize; }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        PyObjectPayload::Tuple(items) => {
                            if items.is_empty() { frame.ip = instr.arg as usize; }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        PyObjectPayload::Dict(map) => {
                            if unsafe { &*map.data_ptr() }.is_empty() { frame.ip = instr.arg as usize; }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        PyObjectPayload::Float(f) => {
                            if *f == 0.0 { frame.ip = instr.arg as usize; }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => {
                            if !self.vm_is_truthy(&v)? {
                                let cs_len = self.call_stack.len();
                                unsafe { self.call_stack.get_unchecked_mut(cs_len - 1) }.ip = instr.arg as usize;
                            }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                    }
                }
                Opcode::PopJumpIfTrue => {
                    // SAFETY: stack non-empty for well-formed bytecode
                    let v = spop!(frame);
                    match &v.payload {
                        PyObjectPayload::Bool(b) => {
                            if *b { frame.ip = instr.arg as usize; }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        PyObjectPayload::None => hot_ok!(profiling, self.profiler, instr.op),
                        PyObjectPayload::Int(PyInt::Small(n)) => {
                            if *n != 0 { frame.ip = instr.arg as usize; }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        PyObjectPayload::Str(s) => {
                            if !s.is_empty() { frame.ip = instr.arg as usize; }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        PyObjectPayload::List(items) => {
                            if !unsafe { &*items.data_ptr() }.is_empty() { frame.ip = instr.arg as usize; }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        PyObjectPayload::Tuple(items) => {
                            if !items.is_empty() { frame.ip = instr.arg as usize; }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        PyObjectPayload::Dict(map) => {
                            if !unsafe { &*map.data_ptr() }.is_empty() { frame.ip = instr.arg as usize; }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        PyObjectPayload::Float(f) => {
                            if *f != 0.0 { frame.ip = instr.arg as usize; }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => {
                            if self.vm_is_truthy(&v)? {
                                let cs_len = self.call_stack.len();
                                unsafe { self.call_stack.get_unchecked_mut(cs_len - 1) }.ip = instr.arg as usize;
                            }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                    }
                }
                // Inline unconditional jumps (trivial but saves dispatch)
                Opcode::JumpForward | Opcode::JumpAbsolute => {
                    frame.ip = instr.arg as usize;
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                // Inline try/except block setup/teardown (very cheap, called every iteration in try loops)
                Opcode::SetupExcept => {
                    frame.push_block(crate::frame::BlockKind::Except, instr.arg as usize);
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                Opcode::SetupFinally => {
                    frame.push_block(crate::frame::BlockKind::Finally, instr.arg as usize);
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                Opcode::PopBlock => {
                    frame.pop_block();
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                Opcode::PopExcept => {
                    frame.pop_block();
                    self.active_exception = None;
                    // sys.exc_info() reads through active_exception pointer — clearing
                    // active_exception is sufficient, no TLS clear needed
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                // Inline RaiseVarargs(1) for the common case: raise ExceptionInstance
                Opcode::RaiseVarargs if instr.arg == 1 => {
                    let tos = unsafe { frame.peek_unchecked() };
                    match &tos.payload {
                        PyObjectPayload::ExceptionInstance(ei) => {
                            let kind = ei.kind;
                            let msg = ei.message.clone();
                            let orig = tos.clone();
                            frame.pop();
                            Err(PyException::with_original(kind, msg, orig))
                        }
                        PyObjectPayload::ExceptionType(kind) => {
                            let kind = *kind;
                            frame.pop();
                            Err(PyException::new(kind, ""))
                        }
                        _ => self.exec_exception_ops(instr)
                    }
                }
                Opcode::BeginFinally => {
                    spush!(frame, PyObject::none());
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                // EndFinally fast path: TOS is None → no exception, no pending return
                Opcode::EndFinally => {
                    if frame.pending_return.is_none() && !frame.stack.is_empty() {
                        if matches!(unsafe { frame.peek_unchecked() }.payload, PyObjectPayload::None) {
                            let _ = spop!(frame);
                            hot_ok!(profiling, self.profiler, instr.op)
                        } else {
                            self.execute_one(instr)
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline UnaryNot for primitive types
                Opcode::UnaryNot => {
                    let v = speek!(frame);
                    let fast = match &v.payload {
                        PyObjectPayload::Bool(b) => Some(!b),
                        PyObjectPayload::Int(PyInt::Small(n)) => Some(*n == 0),
                        PyObjectPayload::None => Some(true),
                        PyObjectPayload::Float(f) => Some(*f == 0.0),
                        PyObjectPayload::Str(s) => Some(s.is_empty()),
                        PyObjectPayload::List(items) => Some(unsafe { &*items.data_ptr() }.is_empty()),
                        PyObjectPayload::Tuple(items) => Some(items.is_empty()),
                        PyObjectPayload::Dict(map) => Some(unsafe { &*map.data_ptr() }.is_empty()),
                        _ => None,
                    };
                    if let Some(r) = fast {
                        let len = frame.stack.len();
                        unsafe { *frame.stack.get_unchecked_mut(len - 1) = PyObject::bool_val(r) };
                        hot_ok!(profiling, self.profiler, instr.op)
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline UnaryNegative for int/float
                Opcode::UnaryNegative => {
                    let v = speek!(frame);
                    let fast = match &v.payload {
                        PyObjectPayload::Int(PyInt::Small(n)) => {
                            Some(match n.checked_neg() {
                                Some(r) => PyObject::int(r),
                                None => {
                                    use num_bigint::BigInt;
                                    PyObject::big_int(-BigInt::from(*n))
                                }
                            })
                        }
                        PyObjectPayload::Float(f) => Some(PyObject::float(-f)),
                        PyObjectPayload::Bool(b) => Some(PyObject::int(if *b { -1 } else { 0 })),
                        _ => None,
                    };
                    if let Some(r) = fast {
                        let len = frame.stack.len();
                        unsafe { *frame.stack.get_unchecked_mut(len - 1) = r };
                        hot_ok!(profiling, self.profiler, instr.op)
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline BinaryPower for int/float fast paths
                Opcode::BinaryPower | Opcode::InplacePower => {
                    let len = frame.stack.len();
                    let a = sget!(frame, len - 2);
                    let b = sget!(frame, len - 1);
                    match (&a.payload, &b.payload) {
                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) if *y >= 0 && *y <= 63 => {
                            let mut r: i64 = 1;
                            let mut overflow = false;
                            let base = *x;
                            let exp = *y;
                            for _ in 0..exp {
                                match r.checked_mul(base) {
                                    Some(v) => r = v,
                                    None => { overflow = true; break; }
                                }
                            }
                            if !overflow {
                                unsafe { frame.binary_op_result(PyObject::int(r)) };
                                hot_ok!(profiling, self.profiler, instr.op)
                            } else {
                                self.execute_one(instr)
                            }
                        }
                        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                            unsafe { frame.binary_op_result(PyObject::float(x.powf(*y))) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
                            unsafe { frame.binary_op_result(PyObject::float(x.powi(*y as i32))) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
                            unsafe { frame.binary_op_result(PyObject::float((*x as f64).powf(*y))) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => self.execute_one(instr),
                    }
                }
                // Inline bitwise ops for int fast paths
                Opcode::BinaryAnd | Opcode::InplaceAnd => {
                    let len = frame.stack.len();
                    let a = sget!(frame, len - 2);
                    let b = sget!(frame, len - 1);
                    match (&a.payload, &b.payload) {
                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                            unsafe { frame.binary_op_result(PyObject::int(*x & *y)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Bool(x), PyObjectPayload::Bool(y)) => {
                            unsafe { frame.binary_op_result(PyObject::bool_val(*x && *y)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => self.execute_one(instr),
                    }
                }
                Opcode::BinaryOr | Opcode::InplaceOr => {
                    let len = frame.stack.len();
                    let a = sget!(frame, len - 2);
                    let b = sget!(frame, len - 1);
                    match (&a.payload, &b.payload) {
                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                            unsafe { frame.binary_op_result(PyObject::int(*x | *y)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Bool(x), PyObjectPayload::Bool(y)) => {
                            unsafe { frame.binary_op_result(PyObject::bool_val(*x || *y)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => self.execute_one(instr),
                    }
                }
                Opcode::BinaryXor | Opcode::InplaceXor => {
                    let len = frame.stack.len();
                    let a = sget!(frame, len - 2);
                    let b = sget!(frame, len - 1);
                    match (&a.payload, &b.payload) {
                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                            unsafe { frame.binary_op_result(PyObject::int(*x ^ *y)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        (PyObjectPayload::Bool(x), PyObjectPayload::Bool(y)) => {
                            unsafe { frame.binary_op_result(PyObject::bool_val(*x ^ *y)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => self.execute_one(instr),
                    }
                }
                Opcode::BinaryLshift | Opcode::InplaceLshift => {
                    let len = frame.stack.len();
                    let a = sget!(frame, len - 2);
                    let b = sget!(frame, len - 1);
                    match (&a.payload, &b.payload) {
                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y)))
                            if *y >= 0 && *y < 64 =>
                        {
                            unsafe { frame.binary_op_result(PyObject::int(*x << *y)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => self.execute_one(instr),
                    }
                }
                Opcode::BinaryRshift | Opcode::InplaceRshift => {
                    let len = frame.stack.len();
                    let a = sget!(frame, len - 2);
                    let b = sget!(frame, len - 1);
                    match (&a.payload, &b.payload) {
                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y)))
                            if *y >= 0 && *y < 64 =>
                        {
                            unsafe { frame.binary_op_result(PyObject::int(*x >> *y)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => self.execute_one(instr),
                    }
                }
                // Inline LoadDeref — lock-free closure variable fast path
                // SAFETY: single-threaded interpreter, no concurrent cell writes
                Opcode::LoadDeref => {
                    let idx = instr.arg as usize;
                    let val = unsafe { &*frame.cells[idx].data_ptr() };
                    if let Some(v) = val {
                        unsafe { frame.push_unchecked(v.clone()) };
                        hot_ok!(profiling, self.profiler, instr.op)
                    } else {
                        self.execute_one(instr)
                    }
                }
                // StoreDeref: lock-free write closure var
                Opcode::StoreDeref => {
                    let value = spop!(frame);
                    let idx = instr.arg as usize;
                    unsafe { *frame.cells[idx].data_ptr() = Some(value) };
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                // Inline BuildTuple for small counts (0–4)
                Opcode::BuildTuple => {
                    let count = instr.arg as usize;
                    match count {
                        0 => {
                            unsafe { frame.push_unchecked(PyObject::tuple(vec![])) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        1 => {
                            let a = spop!(frame);
                            unsafe { frame.push_unchecked(PyObject::tuple(vec![a])) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        2 => {
                            let b = spop!(frame);
                            let a = spop!(frame);
                            unsafe { frame.push_unchecked(PyObject::tuple(vec![a, b])) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        3 => {
                            let c = spop!(frame);
                            let b = spop!(frame);
                            let a = spop!(frame);
                            unsafe { frame.push_unchecked(PyObject::tuple(vec![a, b, c])) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => {
                            let start = frame.stack.len() - count;
                            let items = frame.stack.split_off(start);
                            unsafe { frame.push_unchecked(PyObject::tuple(items)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                    }
                }
                // Inline BuildList for small counts (0–3)
                Opcode::BuildList => {
                    let count = instr.arg as usize;
                    match count {
                        0 => {
                            unsafe { frame.push_unchecked(PyObject::list(vec![])) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        1 => {
                            let a = spop!(frame);
                            unsafe { frame.push_unchecked(PyObject::list(vec![a])) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => {
                            let start = frame.stack.len() - count;
                            let items = frame.stack.split_off(start);
                            unsafe { frame.push_unchecked(PyObject::list(items)) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                    }
                }
                // Inline FormatValue for common primitives (int, str, float, bool, None)
                Opcode::FormatValue => {
                    let has_fmt_spec = instr.arg & 0x04 != 0;
                    let conversion = (instr.arg & 0x03) as u8;
                    if !has_fmt_spec && (conversion == 0 || conversion == 1) {
                        let val = speek!(frame);
                        // Format value to string fragment (without allocating PyObject yet)
                        let fast_str = match &val.payload {
                            PyObjectPayload::Str(s) => Some(s.clone()),
                            PyObjectPayload::Int(PyInt::Small(n)) => {
                                let mut buf = itoa::Buffer::new();
                                Some(CompactString::from(buf.format(*n)))
                            }
                            PyObjectPayload::Float(f) => {
                                let mut buf = ryu::Buffer::new();
                                Some(CompactString::from(buf.format(*f)))
                            }
                            PyObjectPayload::Bool(b) => Some(CompactString::from(if *b { "True" } else { "False" })),
                            PyObjectPayload::None => Some(CompactString::from("None")),
                            _ => None,
                        };
                        if let Some(s) = fast_str {
                            let next_ip = frame.ip;
                            let instr_len = frame.code.instructions.len();
                            // Look-ahead fusion: avoid intermediate PyObject + dispatch cycles
                            if next_ip < instr_len {
                                let next = unsafe { *frame.code.instructions.get_unchecked(next_ip) };
                                // Pattern 1: FORMAT_VALUE + BUILD_STRING 1 → skip BUILD_STRING
                                // f"{val}" — BUILD_STRING 1 is a no-op
                                if next.op == Opcode::BuildString && next.arg == 1 {
                                    let len = frame.stack.len();
                                    unsafe { *frame.stack.get_unchecked_mut(len - 1) = PyObject::str_val(s) };
                                    frame.ip = next_ip + 1; // skip BUILD_STRING
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                // Pattern 2: FORMAT_VALUE + BUILD_STRING 2 → fuse prefix + val
                                // f"prefix{val}" — stack has [prefix, val], concat directly
                                if next.op == Opcode::BuildString && next.arg == 2 {
                                    let stack_len = frame.stack.len();
                                    let prefix_obj = sget!(frame, stack_len - 2);
                                    if let PyObjectPayload::Str(prefix) = &prefix_obj.payload {
                                        let total = prefix.len() + s.len();
                                        let mut result = String::with_capacity(total);
                                        result.push_str(prefix.as_str());
                                        result.push_str(s.as_str());
                                        unsafe {
                                            std::ptr::drop_in_place(frame.stack.as_mut_ptr().add(stack_len - 2));
                                            std::ptr::drop_in_place(frame.stack.as_mut_ptr().add(stack_len - 1));
                                            frame.stack.set_len(stack_len - 2);
                                        }
                                        spush!(frame, PyObject::str_val(CompactString::from(result)));
                                        frame.ip = next_ip + 1; // skip BUILD_STRING
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    }
                                }
                                // Pattern 3 & 4: LOAD_CONST + BUILD_STRING 2 or 3
                                if next_ip + 1 < instr_len && next.op == Opcode::LoadConst {
                                    let next2 = unsafe { *frame.code.instructions.get_unchecked(next_ip + 1) };
                                    let suffix_const = &frame.code.constants[next.arg as usize];
                                    if let ConstantValue::Str(suffix) = suffix_const {
                                        // Pattern 3: FORMAT_VALUE + LOAD_CONST + BUILD_STRING 2
                                        // f"{val}suffix" — fuse val + suffix
                                        if next2.op == Opcode::BuildString && next2.arg == 2 {
                                            let total = s.len() + suffix.len();
                                            let mut result = String::with_capacity(total);
                                            result.push_str(s.as_str());
                                            result.push_str(suffix.as_str());
                                            let len = frame.stack.len();
                                            unsafe { *frame.stack.get_unchecked_mut(len - 1) = PyObject::str_val(CompactString::from(result)) };
                                            frame.ip = next_ip + 2; // skip LOAD_CONST + BUILD_STRING
                                            hot_ok!(profiling, self.profiler, instr.op)
                                        }
                                        // Pattern 4: FORMAT_VALUE + LOAD_CONST + BUILD_STRING 3
                                        // f"prefix{val}suffix" — fuse prefix + val + suffix
                                        if next2.op == Opcode::BuildString && next2.arg == 3 {
                                            let stack_len = frame.stack.len();
                                            let prefix_obj = sget!(frame, stack_len - 2);
                                            if let PyObjectPayload::Str(prefix) = &prefix_obj.payload {
                                                let total = prefix.len() + s.len() + suffix.len();
                                                let mut result = String::with_capacity(total);
                                                result.push_str(prefix.as_str());
                                                result.push_str(s.as_str());
                                                result.push_str(suffix.as_str());
                                                unsafe {
                                                    std::ptr::drop_in_place(frame.stack.as_mut_ptr().add(stack_len - 2));
                                                    std::ptr::drop_in_place(frame.stack.as_mut_ptr().add(stack_len - 1));
                                                    frame.stack.set_len(stack_len - 2);
                                                }
                                                spush!(frame, PyObject::str_val(CompactString::from(result)));
                                                frame.ip = next_ip + 2; // skip LOAD_CONST + BUILD_STRING
                                                hot_ok!(profiling, self.profiler, instr.op)
                                            }
                                        }
                                    }
                                }
                            }
                            // Normal path: replace TOS with formatted Str
                            let len = frame.stack.len();
                            unsafe { *frame.stack.get_unchecked_mut(len - 1) = PyObject::str_val(s) };
                            hot_ok!(profiling, self.profiler, instr.op)
                        } else {
                            self.execute_one(instr)
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline BuildString for the all-str fast path
                Opcode::BuildString => {
                    let count = instr.arg as usize;
                    if count <= 1 {
                        // 0 or 1 items — trivial
                        if count == 0 {
                            spush!(frame, PyObject::str_val(CompactString::from("")));
                        }
                        // count == 1 → already a string from FormatValue, nothing to do
                        hot_ok!(profiling, self.profiler, instr.op)
                    } else {
                        let start = frame.stack.len() - count;
                        let mut total_len = 0usize;
                        let mut all_str = true;
                        for i in start..frame.stack.len() {
                            if let PyObjectPayload::Str(s) = &frame.stack[i].payload {
                                total_len += s.len();
                            } else {
                                all_str = false;
                                break;
                            }
                        }
                        if all_str {
                            let mut result = String::with_capacity(total_len);
                            for i in start..frame.stack.len() {
                                if let PyObjectPayload::Str(s) = &frame.stack[i].payload {
                                    result.push_str(s.as_str());
                                }
                            }
                            frame.stack.truncate(start);
                            spush!(frame, PyObject::str_val(CompactString::from(result)));
                            hot_ok!(profiling, self.profiler, instr.op)
                        } else {
                            self.execute_one(instr)
                        }
                    }
                }
                // Inline UnpackSequence for tuple fast path
                Opcode::UnpackSequence => {
                    let count = instr.arg as usize;
                    // Pop first, match once — push back only in rare fallback case
                    let top = spop!(frame);
                    match &top.payload {
                        PyObjectPayload::Tuple(items) if items.len() == count => {
                            unsafe {
                                let stack = &mut frame.stack;
                                stack.reserve(count);
                                let base = stack.as_mut_ptr().add(stack.len());
                                for i in 0..count {
                                    std::ptr::write(base.add(i), items[count - 1 - i].clone());
                                }
                                stack.set_len(stack.len() + count);
                            }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        PyObjectPayload::List(cell) => {
                            let list = unsafe { &*cell.data_ptr() };
                            if list.len() == count {
                                unsafe {
                                    let stack = &mut frame.stack;
                                    stack.reserve(count);
                                    let base = stack.as_mut_ptr().add(stack.len());
                                    for i in 0..count {
                                        std::ptr::write(base.add(i), list[count - 1 - i].clone());
                                    }
                                    stack.set_len(stack.len() + count);
                                }
                                hot_ok!(profiling, self.profiler, instr.op)
                            } else {
                                spush!(frame, top);
                                self.execute_one(instr)
                            }
                        }
                        _ => {
                            spush!(frame, top);
                            self.execute_one(instr)
                        }
                    }
                }
                Opcode::BuildMap => self.execute_one(instr),
                // Inline CallFunction fast path for simple Python function calls
                Opcode::CallFunction => {
                    let arg_count = instr.arg as usize;
                    let stack_len = frame.stack.len();
                    let func_idx = stack_len - 1 - arg_count;
                    // Single payload check: determine both is_simple and is_recursive
                    // call_kind: 0=slow, 1=simple, 2=recursive, 3=trivial, 4=closure
                    let call_kind = if let PyObjectPayload::Function(pf) = &sget!(frame, func_idx).payload {
                        if pf.is_simple && pf.code.arg_count as usize == arg_count {
                            // Trivial function: body is just `LoadConst X; ReturnValue`
                            // or fused `LoadConstReturnValue X`
                            // Skip frame creation entirely — just push the constant.
                            if (pf.code.instructions.len() == 2
                                && pf.code.instructions[0].op == Opcode::LoadConst
                                && pf.code.instructions[1].op == Opcode::ReturnValue)
                                || (pf.code.instructions.len() == 1
                                    && pf.code.instructions[0].op == Opcode::LoadConstReturnValue)
                            { 3u8 }
                            else if Rc::ptr_eq(&pf.code, &frame.code) { 2u8 } else { 1 }
                        } else if pf.code.arg_count as usize == arg_count
                            && pf.code.kwonlyarg_count == 0
                            && !pf.code.flags.contains(CodeFlags::VARARGS)
                            && !pf.code.flags.contains(CodeFlags::VARKEYWORDS)
                            && !pf.code.flags.contains(CodeFlags::GENERATOR)
                            && !pf.code.flags.contains(CodeFlags::COROUTINE)
                        { 4u8 } // closure or cell function — fast path with cell setup
                        else { 0 }
                    } else { 0 };
                    if call_kind == 3 {
                        // Trivial function: inline the return constant
                        let const_idx = if let PyObjectPayload::Function(pf) = &sget!(frame, func_idx).payload {
                            pf.code.instructions[0].arg as usize
                        } else { unreachable!() };
                        let ret_val = if let PyObjectPayload::Function(pf) = &sget!(frame, func_idx).payload {
                            pf.constant_cache[const_idx].clone()
                        } else { unreachable!() };
                        // Drop function + args from stack, push return value
                        unsafe {
                            let base = frame.stack.as_ptr();
                            for i in 0..=arg_count {
                                let _ = std::ptr::read(base.add(func_idx + i));
                            }
                            frame.stack.set_len(func_idx);
                        }
                        spush!(frame, ret_val);
                        hot_ok!(profiling, self.profiler, instr.op)
                    } else if call_kind > 0 {
                        let args_start = func_idx + 1;
                        // ── Mini-interpreter: inline base-case returns ──
                        // Pattern: LoadFastCompareConstJump → LoadFast → ReturnValue
                        // Skips frame creation entirely for leaf returns (e.g., fib base case)
                        let mut mini_result: Option<PyObjectRef> = None;
                        // Trivial closure: body is just LOAD_DEREF X; RETURN_VALUE
                        // Skip frame creation, directly read the cell
                        if call_kind == 4 {
                            if let PyObjectPayload::Function(pf) = &sget!(frame, func_idx).payload {
                                let instrs = &pf.code.instructions;
                                if instrs.len() == 2
                                    && instrs[0].op == Opcode::LoadDeref
                                    && instrs[1].op == Opcode::ReturnValue
                                {
                                    let cell_idx = instrs[0].arg as usize;
                                    let n_cell = pf.code.cellvars.len();
                                    if cell_idx >= n_cell && cell_idx - n_cell < pf.closure.len() {
                                        let cell = &pf.closure[cell_idx - n_cell];
                                        if let Some(val) = unsafe { &*cell.data_ptr() } {
                                            mini_result = Some(val.clone());
                                        }
                                    }
                                } else if instrs.len() == 1
                                    && instrs[0].op == Opcode::LoadConstReturnValue
                                {
                                    // Closure that returns a constant
                                    mini_result = Some(pf.constant_cache[instrs[0].arg as usize].clone());
                                }
                            }
                        } else if call_kind == 1 && arg_count > 0 {
                            // Trivial simple-function inlining for functions with args
                            if let PyObjectPayload::Function(pf) = &sget!(frame, func_idx).payload {
                                let instrs = &pf.code.instructions;
                                match instrs.len() {
                                    1 => match instrs[0].op {
                                        // def f(a): return a
                                        Opcode::LoadFastReturnValue => {
                                            let li = instrs[0].arg as usize;
                                            if li < arg_count {
                                                mini_result = Some(sget!(frame, args_start + li).clone());
                                            }
                                        }
                                        // def f(a): return CONST
                                        Opcode::LoadConstReturnValue => {
                                            mini_result = Some(pf.constant_cache[instrs[0].arg as usize].clone());
                                        }
                                        _ => {}
                                    }
                                    2 => {
                                        // def add(a, b): return a + b (fused)
                                        if instrs[0].op == Opcode::LoadFastLoadFastBinaryAdd
                                            && instrs[1].op == Opcode::ReturnValue
                                        {
                                            let ai = (instrs[0].arg >> 16) as usize;
                                            let bi = (instrs[0].arg & 0xFFFF) as usize;
                                            if ai < arg_count && bi < arg_count {
                                                let a = sget!(frame, args_start + ai);
                                                let b = sget!(frame, args_start + bi);
                                                mini_result = match (&a.payload, &b.payload) {
                                                    (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                                        match x.checked_add(*y) {
                                                            Some(r) => Some(PyObject::int(r)),
                                                            None => {
                                                                use num_bigint::BigInt;
                                                                Some(PyObject::big_int(BigInt::from(*x) + BigInt::from(*y)))
                                                            }
                                                        }
                                                    }
                                                    (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => Some(PyObject::float(*x + *y)),
                                                    (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => Some(PyObject::float(*x as f64 + *y)),
                                                    (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => Some(PyObject::float(*x + *y as f64)),
                                                    _ => None,
                                                };
                                            }
                                        }
                                        // def f(a): return a (unfused)
                                        else if instrs[0].op == Opcode::LoadFast && instrs[1].op == Opcode::ReturnValue {
                                            let li = instrs[0].arg as usize;
                                            if li < arg_count {
                                                mini_result = Some(sget!(frame, args_start + li).clone());
                                            }
                                        }
                                    }
                                    3 => {
                                        // def sub(a, b): return a - b
                                        if instrs[0].op == Opcode::LoadFastLoadFast
                                            && instrs[2].op == Opcode::ReturnValue
                                        {
                                            let ai = (instrs[0].arg >> 16) as usize;
                                            let bi = (instrs[0].arg & 0xFFFF) as usize;
                                            if ai < arg_count && bi < arg_count {
                                                let a = sget!(frame, args_start + ai);
                                                let b = sget!(frame, args_start + bi);
                                                if instrs[1].op == Opcode::BinarySubtract {
                                                    mini_result = match (&a.payload, &b.payload) {
                                                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                                            match x.checked_sub(*y) {
                                                                Some(r) => Some(PyObject::int(r)),
                                                                None => {
                                                                    use num_bigint::BigInt;
                                                                    Some(PyObject::big_int(BigInt::from(*x) - BigInt::from(*y)))
                                                                }
                                                            }
                                                        }
                                                        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => Some(PyObject::float(*x - *y)),
                                                        _ => None,
                                                    };
                                                } else if instrs[1].op == Opcode::BinaryMultiply {
                                                    mini_result = match (&a.payload, &b.payload) {
                                                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                                            match x.checked_mul(*y) {
                                                                Some(r) => Some(PyObject::int(r)),
                                                                None => {
                                                                    use num_bigint::BigInt;
                                                                    Some(PyObject::big_int(BigInt::from(*x) * BigInt::from(*y)))
                                                                }
                                                            }
                                                        }
                                                        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => Some(PyObject::float(*x * *y)),
                                                        _ => None,
                                                    };
                                                }
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        } else if call_kind == 2 {
                            let instrs = &frame.code.instructions;
                            if instrs.len() > 2
                                && instrs[0].op == Opcode::LoadFastCompareConstJump
                            {
                                let packed = instrs[0].arg;
                                let cmp_op = packed >> 28;
                                let local_idx = ((packed >> 20) & 0xFF) as usize;
                                let const_idx = ((packed >> 12) & 0xFF) as usize;
                                if local_idx < arg_count {
                                    let arg_ref = sget!(frame, args_start + local_idx);
                                    let const_ref = unsafe { frame.constant_cache.get_unchecked(const_idx) };
                                    let cmp_result = match (&arg_ref.payload, &const_ref.payload) {
                                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                            match cmp_op {
                                                0 => Some(*x < *y), 1 => Some(*x <= *y),
                                                2 => Some(*x == *y), 3 => Some(*x != *y),
                                                4 => Some(*x > *y), 5 => Some(*x >= *y),
                                                _ => None,
                                            }
                                        }
                                        _ => None,
                                    };
                                    if let Some(cmp_val) = cmp_result {
                                        if cmp_val {
                                            // True branch: next instruction is return
                                            if instrs[1].op == Opcode::LoadFastReturnValue {
                                                let ret_local = instrs[1].arg as usize;
                                                mini_result = Some(if ret_local < arg_count {
                                                    sget!(frame, args_start + ret_local).clone()
                                                } else { PyObject::none() });
                                            } else if instrs[1].op == Opcode::LoadFast
                                                && instrs[2].op == Opcode::ReturnValue
                                            {
                                                let ret_local = instrs[1].arg as usize;
                                                mini_result = Some(if ret_local < arg_count {
                                                    sget!(frame, args_start + ret_local).clone()
                                                } else { PyObject::none() });
                                            } else if instrs[1].op == Opcode::LoadConstReturnValue {
                                                mini_result = Some(unsafe {
                                                    frame.constant_cache.get_unchecked(instrs[1].arg as usize).clone()
                                                });
                                            }
                                        } else if !cmp_val {
                                            let jt = (packed & 0xFFF) as usize;
                                            if jt < instrs.len() {
                                                if instrs[jt].op == Opcode::LoadFastReturnValue {
                                                    let ret_local = instrs[jt].arg as usize;
                                                    mini_result = Some(if ret_local < arg_count {
                                                        sget!(frame, args_start + ret_local).clone()
                                                    } else { PyObject::none() });
                                                } else if instrs[jt].op == Opcode::LoadFast
                                                    && jt + 1 < instrs.len()
                                                    && instrs[jt + 1].op == Opcode::ReturnValue
                                                {
                                                    let ret_local = instrs[jt].arg as usize;
                                                    mini_result = Some(if ret_local < arg_count {
                                                        sget!(frame, args_start + ret_local).clone()
                                                    } else { PyObject::none() });
                                                } else if instrs[jt].op == Opcode::LoadConstReturnValue {
                                                    mini_result = Some(unsafe {
                                                        frame.constant_cache.get_unchecked(instrs[jt].arg as usize).clone()
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(ret_val) = mini_result {
                            // Base case resolved without frame creation
                            unsafe {
                                let base = frame.stack.as_ptr();
                                for i in 0..=arg_count {
                                    let _ = std::ptr::read(base.add(func_idx + i));
                                }
                                frame.stack.set_len(func_idx);
                            }
                            spush!(frame, ret_val);
                            hot_ok!(profiling, self.profiler, instr.op)
                        } else {
                        // Normal path: create frame
                        let borrowed_func = call_kind == 1;
                        let mut new_frame = if call_kind == 2 {
                            // SAFETY: parent frame outlives child in iterative dispatch
                            unsafe { Frame::new_recursive(frame, &mut self.frame_pool) }
                        } else if call_kind == 4 {
                            // Closure call: use optimized constructor that takes cells directly
                            let (code, globals, constant_cache, closure_ptr, closure_len) = if let PyObjectPayload::Function(pf) = &sget!(frame, func_idx).payload {
                                (Rc::clone(&pf.code), pf.globals.clone(), Rc::clone(&pf.constant_cache),
                                 pf.closure.as_ptr(), pf.closure.len())
                            } else { unreachable!() };
                            // SAFETY: closure ref valid while stack reference held
                            let closure_ref = unsafe { std::slice::from_raw_parts(closure_ptr, closure_len) };
                            let f = Frame::new_closure_from_pool(
                                code, globals, self.builtins.clone(), constant_cache,
                                closure_ref, &mut self.frame_pool,
                            );
                            f
                        } else if call_kind == 1 {
                            // Borrowed path: zero refcount ops for frame creation.
                            // Take func_obj from stack, borrow its Arc fields via ptr::read.
                            unsafe {
                                let func_obj: PyObjectRef = std::ptr::read(frame.stack.as_ptr().add(func_idx));
                                let pf_ptr = match &func_obj.payload {
                                    PyObjectPayload::Function(pf) => &**pf as *const ferrython_core::types::PyFunction,
                                    _ => std::hint::unreachable_unchecked(),
                                };
                                Frame::new_borrowed(&*pf_ptr, func_obj, &self.builtins, &mut self.frame_pool)
                            }
                        } else {
                            // Normal path: clone Arcs from function object
                            let (code, globals, constant_cache) = if let PyObjectPayload::Function(pf) = &sget!(frame, func_idx).payload {
                                (Rc::clone(&pf.code), pf.globals.clone(), Rc::clone(&pf.constant_cache))
                            } else { unreachable!() };
                            let mut f = Frame::new_from_pool(
                                code, globals, self.builtins.clone(), constant_cache,
                                &mut self.frame_pool,
                            );
                            f.scope_kind = crate::frame::ScopeKind::Function;
                            f
                        };
                        // Move args directly from parent stack to new frame locals
                        // SAFETY: we know stack has func + arg_count elements at the end
                        unsafe {
                            let base = frame.stack.as_ptr();
                            for i in 0..arg_count {
                                new_frame.locals[i] = Some(
                                    std::ptr::read(base.add(args_start + i))
                                );
                            }
                            if !borrowed_func {
                                // For non-borrowed paths, take ownership of function object
                                let _func = std::ptr::read(base.add(func_idx));
                            }
                            // For borrowed path (call_kind==1), func was already moved into held_func
                            frame.stack.set_len(func_idx);
                        }
                        // Link cellvars to locals by name (must happen AFTER args are moved)
                        if call_kind == 4 && !new_frame.code.cellvars.is_empty() {
                            for (cell_idx, cell_name) in new_frame.code.cellvars.iter().enumerate() {
                                for (var_idx, var_name) in new_frame.code.varnames.iter().enumerate() {
                                    if cell_name == var_name {
                                        if let Some(val) = new_frame.locals[var_idx].take() {
                                            unsafe { *new_frame.cells[cell_idx].data_ptr() = Some(val) };
                                        }
                                        break;
                                    }
                                }
                            }
                        }
                        self.call_stack.push(new_frame);
                        // Re-derive frame_ptr: push may reallocate Vec
                        rederive_frame!(self, frame_ptr, instr_base, instr_count);
                        if self.call_stack.len() > self.recursion_limit {
                            if let Some(frame) = self.call_stack.pop() {
                                frame.recycle(&mut self.frame_pool);
                            }
                            Err(PyException::recursion_error("maximum recursion depth exceeded"))
                        } else {
                            if has_trace {
                                let frame_obj = self.make_trace_frame();
                                ferrython_stdlib::set_current_frame(Some(frame_obj));
                                self.fire_trace_event("call", PyObject::none());
                            }
                            if has_profile {
                                self.fire_profile_event("call", PyObject::none());
                            }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        } // close the mini-interpreter else (normal frame creation path)
                    } else {
                        // ── Inline Class instantiation for simple classes ──
                        // Avoids execute_one + 2 Vec allocs + double call_object dispatch
                        if let PyObjectPayload::Class(cd) = &sget!(frame, func_idx).payload {
                            // is_simple_class is computed at creation and invalidated on known mutation paths.
                            // Safety check: verify __new__ wasn't added after creation without invalidation.
                            if cd.is_simple_class.get() && !cd.namespace.read().contains_key("__new__") {
                                // Look up __init__: try vtable first (O(1) hash), fall back to namespace
                                let vt = unsafe { &*cd.method_vtable.data_ptr() };
                                let init_fn = if !vt.is_empty() {
                                    vt.get("__init__").cloned()
                                } else {
                                    None
                                }.or_else(|| {
                                    cd.namespace.read().get("__init__").cloned()
                                        .or_else(|| ferrython_core::object::lookup_in_class_mro(
                                            sget!(frame, func_idx), "__init__"))
                                });
                                if let Some(init_fn) = init_fn {
                                    // Check if __init__ is a simple Function we can inline
                                    if let PyObjectPayload::Function(pf) = &init_fn.payload {
                                        if pf.is_simple && pf.code.arg_count as usize == arg_count + 1 {
                                            let cls = sget!(frame, func_idx).clone();
                                            let instance = PyObject::instance(cls);
                                            // Create frame directly for __init__(self, *args)
                                            let mut new_frame = Frame::new_from_pool(
                                                Rc::clone(&pf.code),
                                                pf.globals.clone(),
                                                self.builtins.clone(),
                                                Rc::clone(&pf.constant_cache),
                                                &mut self.frame_pool,
                                            );
                                            new_frame.scope_kind = crate::frame::ScopeKind::Function;
                                            // locals[0] = self (instance)
                                            new_frame.locals[0] = Some(instance.clone());
                                            // Move args from parent stack to locals[1..]
                                            let args_start = func_idx + 1;
                                            unsafe {
                                                let base = frame.stack.as_ptr();
                                                for i in 0..arg_count {
                                                    new_frame.locals[1 + i] = Some(
                                                        std::ptr::read(base.add(args_start + i))
                                                    );
                                                }
                                                // Drop function ref from stack
                                                let _func = std::ptr::read(base.add(func_idx));
                                                frame.stack.set_len(func_idx);
                                            }
                                            // Push instance as return value BEFORE __init__ frame
                                            spush!(frame, instance);
                                            // Mark frame to discard __init__'s return value
                                            new_frame.discard_return = true;
                                            self.call_stack.push(new_frame);
                                            rederive_frame!(self, frame_ptr, instr_base, instr_count);
                                            if self.call_stack.len() > self.recursion_limit {
                                                if let Some(f) = self.call_stack.pop() { f.recycle(&mut self.frame_pool); }
                                                Err(PyException::recursion_error("maximum recursion depth exceeded"))
                                            } else {
                                                hot_ok!(profiling, self.profiler, instr.op)
                                            }
                                        } else {
                                            // __init__ has complex signature — fall back
                                            self.execute_one(instr)
                                        }
                                    } else {
                                        self.execute_one(instr)
                                    }
                                } else {
                                    // No __init__ found — create bare instance
                                    let cls = sget!(frame, func_idx).clone();
                                    let instance = PyObject::instance(cls.clone());
                                    // Set exception `args` for exception subclasses (cached flag)
                                    if cd.is_exception_subclass {
                                        if let PyObjectPayload::Instance(inst) = &instance.payload {
                                            let mut args_vec = Vec::with_capacity(arg_count);
                                            for i in 0..arg_count {
                                                args_vec.push(sget!(frame, func_idx + 1 + i).clone());
                                            }
                                            let mut attrs = inst.attrs.write();
                                            if arg_count == 1 {
                                                attrs.insert(CompactString::from("message"),
                                                    args_vec[0].clone());
                                            }
                                            attrs.insert(CompactString::from("args"),
                                                PyObject::tuple(args_vec));
                                        }
                                    }
                                    unsafe {
                                        let base = frame.stack.as_ptr();
                                        for i in 0..=arg_count {
                                            let _ = std::ptr::read(base.add(func_idx + i));
                                        }
                                        frame.stack.set_len(func_idx);
                                    }
                                    spush!(frame, instance);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                            } else {
                                self.execute_one(instr)
                            }
                        } else {
                        // Fast path for common builtins: len(x), range(n)
                        let builtin_name = if let PyObjectPayload::BuiltinFunction(name) = &sget!(frame, func_idx).payload {
                            Some(name.as_str())
                        } else { None };
                        match (builtin_name, arg_count) {
                            (Some("len"), 1) => {
                                let arg = sget!(frame, stack_len - 1);
                                let fast_len = match &arg.payload {
                                    PyObjectPayload::List(v) => Some(unsafe { &*v.data_ptr() }.len() as i64),
                                    PyObjectPayload::Tuple(v) => Some(v.len() as i64),
                                    PyObjectPayload::Str(s) => Some(s.chars().count() as i64),
                                    PyObjectPayload::Dict(m) => Some(unsafe { &*m.data_ptr() }.len() as i64),
                                    PyObjectPayload::Set(m) => Some(unsafe { &*m.data_ptr() }.len() as i64),
                                    PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Some(b.len() as i64),
                                    _ => None,
                                };
                                if let Some(n) = fast_len {
                                    unsafe { frame.stack.set_len(func_idx); }
                                    spush!(frame, PyObject::int(n));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            (Some("range"), 1) => {
                                let arg = sget!(frame, stack_len - 1);
                                if let PyObjectPayload::Int(PyInt::Small(stop)) = &arg.payload {
                                    let stop = *stop;
                                    unsafe { frame.stack.set_len(func_idx); }
                                    let iter = PyObject::wrap(PyObjectPayload::RangeIter {
                                        current: SyncI64::new(0), stop, step: 1,
                                    });
                                    spush!(frame, iter);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            // Inline isinstance(obj, cls) — single class check
                            (Some("isinstance"), 2) => {
                                let obj = sget!(frame, stack_len - 2);
                                let cls = sget!(frame, stack_len - 1);
                                let fast_result = match &cls.payload {
                                    PyObjectPayload::BuiltinType(bt) => {
                                        let bt_str = bt.as_str();
                                        let matches = match (&obj.payload, bt_str) {
                                            (PyObjectPayload::Int(_), "int") => true,
                                            (PyObjectPayload::Bool(_), "int") => true, // bool is subclass of int
                                            (PyObjectPayload::Bool(_), "bool") => true,
                                            (PyObjectPayload::Float(_), "float") => true,
                                            (PyObjectPayload::Str(_), "str") => true,
                                            (PyObjectPayload::List(_), "list") => true,
                                            (PyObjectPayload::Tuple(_), "tuple") => true,
                                            (PyObjectPayload::Dict(_), "dict") => true,
                                            (PyObjectPayload::InstanceDict(_), "dict") => true,
                                            (PyObjectPayload::MappingProxy(_), "dict") => true,
                                            (PyObjectPayload::Set(_), "set") => true,
                                            (PyObjectPayload::Bytes(_), "bytes") => true,
                                            (PyObjectPayload::ByteArray(_), "bytearray") => true,
                                            (PyObjectPayload::None, "NoneType") => true,
                                            (_, "object") => true, // everything is an instance of object
                                            _ => false,
                                        };
                                        Some(matches)
                                    }
                                    PyObjectPayload::Class(cd) => {
                                        if let PyObjectPayload::Instance(inst) = &obj.payload {
                                            if let PyObjectPayload::Class(obj_cd) = &inst.class.payload {
                                                if obj_cd.name == cd.name { Some(true) }
                                                else if obj_cd.mro.iter().any(|b| {
                                                    matches!(&b.payload, PyObjectPayload::Class(bc) if bc.name == cd.name)
                                                }) { Some(true) }
                                                else { None } // fall through to full isinstance (handles ABC registry, etc.)
                                            } else { None }
                                        } else { None }
                                    }
                                    _ => None,
                                };
                                if let Some(result) = fast_result {
                                    unsafe { frame.stack.set_len(func_idx); }
                                    spush!(frame, PyObject::bool_val(result));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            // Inline type(obj) for builtin types
                            (Some("type"), 1) => {
                                let arg = sget!(frame, stack_len - 1);
                                let type_name = match &arg.payload {
                                    PyObjectPayload::Int(_) => Some("int"),
                                    PyObjectPayload::Float(_) => Some("float"),
                                    PyObjectPayload::Str(_) => Some("str"),
                                    PyObjectPayload::Bool(_) => Some("bool"),
                                    PyObjectPayload::None => Some("NoneType"),
                                    PyObjectPayload::List(_) => Some("list"),
                                    PyObjectPayload::Tuple(_) => Some("tuple"),
                                    PyObjectPayload::Dict(_) => Some("dict"),
                                    PyObjectPayload::Set(_) => Some("set"),
                                    PyObjectPayload::Bytes(_) => Some("bytes"),
                                    PyObjectPayload::ByteArray(_) => Some("bytearray"),
                                    _ => None,
                                };
                                if let Some(name) = type_name {
                                    unsafe { frame.stack.set_len(func_idx); }
                                    spush!(frame, PyObject::wrap(PyObjectPayload::BuiltinType(name.into())));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            // Inline bool(x) — truthiness conversion
                            (Some("bool"), 1) => {
                                let arg = sget!(frame, stack_len - 1);
                                let result = match &arg.payload {
                                    PyObjectPayload::Bool(b) => Some(*b),
                                    PyObjectPayload::Int(PyInt::Small(n)) => Some(*n != 0),
                                    PyObjectPayload::Float(f) => Some(*f != 0.0),
                                    PyObjectPayload::Str(s) => Some(!s.is_empty()),
                                    PyObjectPayload::None => Some(false),
                                    PyObjectPayload::List(v) => Some(!unsafe { &*v.data_ptr() }.is_empty()),
                                    PyObjectPayload::Tuple(v) => Some(!v.is_empty()),
                                    PyObjectPayload::Dict(m) => Some(!unsafe { &*m.data_ptr() }.is_empty()),
                                    _ => None,
                                };
                                if let Some(b) = result {
                                    unsafe { frame.stack.set_len(func_idx); }
                                    spush!(frame, PyObject::bool_val(b));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            // Inline int(x) for common conversions
                            (Some("int"), 1) => {
                                let arg = sget!(frame, stack_len - 1);
                                let result = match &arg.payload {
                                    PyObjectPayload::Int(_) => Some(arg.clone()),
                                    PyObjectPayload::Bool(b) => Some(PyObject::int(if *b { 1 } else { 0 })),
                                    PyObjectPayload::Float(f) => Some(PyObject::int(*f as i64)),
                                    _ => None,
                                };
                                if let Some(v) = result {
                                    unsafe { frame.stack.set_len(func_idx); }
                                    spush!(frame, v);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            // Inline str(x) for common types
                            (Some("str"), 1) => {
                                let arg = sget!(frame, stack_len - 1);
                                let result = match &arg.payload {
                                    PyObjectPayload::Str(_) => Some(arg.clone()),
                                    PyObjectPayload::Int(PyInt::Small(n)) => {
                                        let mut buf = itoa::Buffer::new();
                                        Some(PyObject::str_val(CompactString::from(buf.format(*n))))
                                    }
                                    PyObjectPayload::Float(f) => {
                                        let mut buf = ryu::Buffer::new();
                                        Some(PyObject::str_val(CompactString::from(buf.format(*f))))
                                    }
                                    PyObjectPayload::Bool(b) => Some(PyObject::str_val(CompactString::from(if *b { "True" } else { "False" }))),
                                    PyObjectPayload::None => Some(PyObject::str_val(CompactString::from("None"))),
                                    _ => None,
                                };
                                if let Some(v) = result {
                                    unsafe { frame.stack.set_len(func_idx); }
                                    spush!(frame, v);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            // Inline abs(x) for int/float
                            (Some("abs"), 1) => {
                                let arg = sget!(frame, stack_len - 1);
                                let result = match &arg.payload {
                                    PyObjectPayload::Int(PyInt::Small(n)) => Some(PyObject::int(n.abs())),
                                    PyObjectPayload::Float(f) => Some(PyObject::wrap(PyObjectPayload::Float(f.abs()))),
                                    _ => None,
                                };
                                if let Some(v) = result {
                                    unsafe { frame.stack.set_len(func_idx); }
                                    spush!(frame, v);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            (Some("min"), 2) => {
                                let a = sget!(frame, stack_len - 2);
                                let b = sget!(frame, stack_len - 1);
                                let result = match (&a.payload, &b.payload) {
                                    (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) =>
                                        Some(PyObject::int(std::cmp::min(*x, *y))),
                                    (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) =>
                                        Some(PyObject::float(x.min(*y))),
                                    (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
                                        let xf = *x as f64;
                                        Some(if xf <= *y { PyObject::int(*x) } else { PyObject::float(*y) })
                                    }
                                    (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
                                        let yf = *y as f64;
                                        Some(if *x <= yf { PyObject::float(*x) } else { PyObject::int(*y) })
                                    }
                                    _ => None,
                                };
                                if let Some(v) = result {
                                    unsafe { frame.stack.set_len(func_idx); }
                                    spush!(frame, v);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            (Some("max"), 2) => {
                                let a = sget!(frame, stack_len - 2);
                                let b = sget!(frame, stack_len - 1);
                                let result = match (&a.payload, &b.payload) {
                                    (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) =>
                                        Some(PyObject::int(std::cmp::max(*x, *y))),
                                    (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) =>
                                        Some(PyObject::float(x.max(*y))),
                                    (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
                                        let xf = *x as f64;
                                        Some(if xf >= *y { PyObject::int(*x) } else { PyObject::float(*y) })
                                    }
                                    (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
                                        let yf = *y as f64;
                                        Some(if *x >= yf { PyObject::float(*x) } else { PyObject::int(*y) })
                                    }
                                    _ => None,
                                };
                                if let Some(v) = result {
                                    unsafe { frame.stack.set_len(func_idx); }
                                    spush!(frame, v);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            // Inline hasattr(obj, name) — skip execute_one dispatch
                            (Some("hasattr"), 2) => {
                                let name_arg = sget!(frame, stack_len - 1);
                                if let PyObjectPayload::Str(s) = &name_arg.payload {
                                    let obj = sget!(frame, stack_len - 2);
                                    let result = ferrython_core::object::py_has_attr(obj, s.as_str());
                                    unsafe { frame.stack.set_len(func_idx); }
                                    spush!(frame, PyObject::bool_val(result));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            // Inline getattr(obj, name) — skip execute_one dispatch
                            (Some("getattr"), 2) => {
                                let name_arg = sget!(frame, stack_len - 1);
                                if let PyObjectPayload::Str(s) = &name_arg.payload {
                                    let obj = sget!(frame, stack_len - 2);
                                    if let Some(val) = obj.get_attr(s.as_str()) {
                                        unsafe { frame.stack.set_len(func_idx); }
                                        spush!(frame, val);
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    } else {
                                        self.execute_one(instr)
                                    }
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            // next() — fall through to VM-level dispatch (needs proper exception handling)
                            (Some("next"), 1) | (Some("next"), 2) => {
                                self.execute_one(instr)
                            }
                            // Inline sum(iterable) — native i64 accumulation for list/tuple of ints
                            (Some("sum"), 1) | (Some("sum"), 2) => {
                                let iterable = sget!(frame, func_idx + 1);
                                let items: Option<&[PyObjectRef]> = match &iterable.payload {
                                    PyObjectPayload::List(v) => Some(unsafe { &*v.data_ptr() }),
                                    PyObjectPayload::Tuple(v) => Some(v.as_slice()),
                                    _ => None,
                                };
                                let mut fast_result: Option<i64> = None;
                                if let Some(items) = items {
                                    let start_val: i64 = if arg_count == 2 {
                                        if let PyObjectPayload::Int(PyInt::Small(s)) = &sget!(frame, func_idx + 2).payload {
                                            *s
                                        } else { i64::MIN } // sentinel: fall back
                                    } else { 0 };
                                    if start_val != i64::MIN {
                                        let mut total: i64 = start_val;
                                        let mut ok = true;
                                        for item in items {
                                            if let PyObjectPayload::Int(PyInt::Small(n)) = &item.payload {
                                                if let Some(t) = total.checked_add(*n) {
                                                    total = t;
                                                } else { ok = false; break; }
                                            } else { ok = false; break; }
                                        }
                                        if ok { fast_result = Some(total); }
                                    }
                                }
                                if let Some(total) = fast_result {
                                    unsafe { frame.stack.set_len(func_idx); }
                                    spush!(frame, PyObject::int(total));
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            _ => self.execute_one(instr),
                        }
                    } // close else for BuiltinFunction checks
                    } // close else for Class check
                }
                // Inline LoadGlobal + CallFunction fused: load global, then call
                // arg = (name_idx << 16) | arg_count
                Opcode::LoadGlobalCallFunction => {
                    let name_idx = (instr.arg >> 16) as usize;
                    let arg_count = (instr.arg & 0xFFFF) as usize;
                    let ver = crate::frame::globals_version();
                    // Fast path: cache hit for the global
                    let func_ref = if frame.global_cache_version == ver {
                        if let Some(ref cache) = frame.global_cache {
                            // SAFETY: compiler guarantees name_idx < code.names.len() == cache.len()
                            unsafe { cache.get_unchecked(name_idx) }.as_ref()
                        } else { None }
                    } else { None };
                    if let Some(func_obj) = func_ref {
                        // Check if simple function with matching arg count
                        let call_kind = if let PyObjectPayload::Function(pf) = &func_obj.payload {
                            if pf.is_simple && pf.code.arg_count as usize == arg_count {
                                // Trivial function: body is just `LoadConst X; ReturnValue`
                                // or fused `LoadConstReturnValue X`
                                if (pf.code.instructions.len() == 2
                                    && pf.code.instructions[0].op == Opcode::LoadConst
                                    && pf.code.instructions[1].op == Opcode::ReturnValue)
                                    || (pf.code.instructions.len() == 1
                                        && pf.code.instructions[0].op == Opcode::LoadConstReturnValue)
                                { 3u8 }
                                else if Rc::ptr_eq(&pf.code, &frame.code) { 2u8 } else { 1 }
                            } else { 0 }
                        } else { 0 };
                        if call_kind == 3 {
                            // Trivial function: inline the return constant
                            let ret_val = if let PyObjectPayload::Function(pf) = &func_obj.payload {
                                let ci = pf.code.instructions[0].arg as usize;
                                pf.constant_cache[ci].clone()
                            } else { unreachable!() };
                            // Drop args from stack, push return value
                            let stack_len = frame.stack.len();
                            unsafe {
                                let base = frame.stack.as_ptr();
                                for i in 0..arg_count {
                                    let _ = std::ptr::read(base.add(stack_len - arg_count + i));
                                }
                                frame.stack.set_len(stack_len - arg_count);
                            }
                            spush!(frame, ret_val);
                            hot_ok!(profiling, self.profiler, instr.op)
                        } else if call_kind > 0 {
                            let stack_len = frame.stack.len();
                            let args_start = stack_len - arg_count;
                            // ── Mini-interpreter: inline base-case returns ──
                            // For recursive calls, check if first instr is LoadFastCompareConstJump
                            // and resolve the comparison directly against args on parent stack.
                            let mut mini_result: Option<PyObjectRef> = None;
                            if call_kind == 2 {
                                let instrs = &frame.code.instructions;
                                if instrs.len() > 2
                                    && instrs[0].op == Opcode::LoadFastCompareConstJump
                                {
                                    let packed = instrs[0].arg;
                                    let cmp_op = packed >> 28;
                                    let local_idx = ((packed >> 20) & 0xFF) as usize;
                                    let const_idx = ((packed >> 12) & 0xFF) as usize;
                                    if local_idx < arg_count {
                                        let arg_ref = sget!(frame, args_start + local_idx);
                                        let const_ref = unsafe { frame.constant_cache.get_unchecked(const_idx) };
                                        let cmp_result = match (&arg_ref.payload, &const_ref.payload) {
                                            (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                                match cmp_op {
                                                    0 => Some(*x < *y), 1 => Some(*x <= *y),
                                                    2 => Some(*x == *y), 3 => Some(*x != *y),
                                                    4 => Some(*x > *y), 5 => Some(*x >= *y),
                                                    _ => None,
                                                }
                                            }
                                            _ => None,
                                        };
                                        if let Some(cmp_val) = cmp_result {
                                            if cmp_val {
                                                if instrs[1].op == Opcode::LoadFastReturnValue {
                                                    let ret_local = instrs[1].arg as usize;
                                                    mini_result = Some(if ret_local < arg_count {
                                                        sget!(frame, args_start + ret_local).clone()
                                                    } else { PyObject::none() });
                                                } else if instrs[1].op == Opcode::LoadFast
                                                    && instrs[2].op == Opcode::ReturnValue
                                                {
                                                    let ret_local = instrs[1].arg as usize;
                                                    mini_result = Some(if ret_local < arg_count {
                                                        sget!(frame, args_start + ret_local).clone()
                                                    } else { PyObject::none() });
                                                } else if instrs[1].op == Opcode::LoadConstReturnValue {
                                                    mini_result = Some(unsafe {
                                                        frame.constant_cache.get_unchecked(instrs[1].arg as usize).clone()
                                                    });
                                                }
                                            } else if !cmp_val {
                                                let jt = (packed & 0xFFF) as usize;
                                                if jt < instrs.len() {
                                                    if instrs[jt].op == Opcode::LoadFastReturnValue {
                                                        let ret_local = instrs[jt].arg as usize;
                                                        mini_result = Some(if ret_local < arg_count {
                                                            sget!(frame, args_start + ret_local).clone()
                                                        } else { PyObject::none() });
                                                    } else if instrs[jt].op == Opcode::LoadFast
                                                        && jt + 1 < instrs.len()
                                                        && instrs[jt + 1].op == Opcode::ReturnValue
                                                    {
                                                        let ret_local = instrs[jt].arg as usize;
                                                        mini_result = Some(if ret_local < arg_count {
                                                            sget!(frame, args_start + ret_local).clone()
                                                        } else { PyObject::none() });
                                                    } else if instrs[jt].op == Opcode::LoadConstReturnValue {
                                                        mini_result = Some(unsafe {
                                                            frame.constant_cache.get_unchecked(instrs[jt].arg as usize).clone()
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            // call_kind==1: trivial simple function with args
                            if call_kind == 1 && arg_count > 0 && mini_result.is_none() {
                                if let PyObjectPayload::Function(pf) = &func_obj.payload {
                                    let instrs = &pf.code.instructions;
                                    match instrs.len() {
                                        1 => match instrs[0].op {
                                            Opcode::LoadFastReturnValue => {
                                                let li = instrs[0].arg as usize;
                                                if li < arg_count {
                                                    mini_result = Some(sget!(frame, args_start + li).clone());
                                                }
                                            }
                                            Opcode::LoadConstReturnValue => {
                                                mini_result = Some(pf.constant_cache[instrs[0].arg as usize].clone());
                                            }
                                            _ => {}
                                        }
                                        2 => {
                                            if instrs[0].op == Opcode::LoadFastLoadFastBinaryAdd
                                                && instrs[1].op == Opcode::ReturnValue
                                            {
                                                let ai = (instrs[0].arg >> 16) as usize;
                                                let bi = (instrs[0].arg & 0xFFFF) as usize;
                                                if ai < arg_count && bi < arg_count {
                                                    let a = sget!(frame, args_start + ai);
                                                    let b = sget!(frame, args_start + bi);
                                                    mini_result = match (&a.payload, &b.payload) {
                                                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                                            match x.checked_add(*y) {
                                                                Some(r) => Some(PyObject::int(r)),
                                                                None => {
                                                                    use num_bigint::BigInt;
                                                                    Some(PyObject::big_int(BigInt::from(*x) + BigInt::from(*y)))
                                                                }
                                                            }
                                                        }
                                                        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => Some(PyObject::float(*x + *y)),
                                                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => Some(PyObject::float(*x as f64 + *y)),
                                                        (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => Some(PyObject::float(*x + *y as f64)),
                                                        _ => None,
                                                    };
                                                }
                                            }
                                            else if instrs[0].op == Opcode::LoadFast && instrs[1].op == Opcode::ReturnValue {
                                                let li = instrs[0].arg as usize;
                                                if li < arg_count {
                                                    mini_result = Some(sget!(frame, args_start + li).clone());
                                                }
                                            }
                                        }
                                        3 => {
                                            if instrs[0].op == Opcode::LoadFastLoadFast
                                                && instrs[2].op == Opcode::ReturnValue
                                            {
                                                let ai = (instrs[0].arg >> 16) as usize;
                                                let bi = (instrs[0].arg & 0xFFFF) as usize;
                                                if ai < arg_count && bi < arg_count {
                                                    let a = sget!(frame, args_start + ai);
                                                    let b = sget!(frame, args_start + bi);
                                                    if instrs[1].op == Opcode::BinarySubtract {
                                                        mini_result = match (&a.payload, &b.payload) {
                                                            (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                                                match x.checked_sub(*y) {
                                                                    Some(r) => Some(PyObject::int(r)),
                                                                    None => {
                                                                        use num_bigint::BigInt;
                                                                        Some(PyObject::big_int(BigInt::from(*x) - BigInt::from(*y)))
                                                                    }
                                                                }
                                                            }
                                                            (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => Some(PyObject::float(*x - *y)),
                                                            _ => None,
                                                        };
                                                    } else if instrs[1].op == Opcode::BinaryMultiply {
                                                        mini_result = match (&a.payload, &b.payload) {
                                                            (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                                                match x.checked_mul(*y) {
                                                                    Some(r) => Some(PyObject::int(r)),
                                                                    None => {
                                                                        use num_bigint::BigInt;
                                                                        Some(PyObject::big_int(BigInt::from(*x) * BigInt::from(*y)))
                                                                    }
                                                                }
                                                            }
                                                            (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => Some(PyObject::float(*x * *y)),
                                                            _ => None,
                                                        };
                                                    }
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            if let Some(ret_val) = mini_result {
                                // Base case resolved without frame creation
                                frame.stack.truncate(args_start);
                                spush!(frame, ret_val);
                                hot_ok!(profiling, self.profiler, instr.op)
                            } else {
                            let mut new_frame = if call_kind == 2 {
                                // SAFETY: parent frame outlives child in iterative dispatch
                                unsafe { Frame::new_recursive(frame, &mut self.frame_pool) }
                            } else if call_kind == 1 {
                                // Borrowed path: clone only the Rc<PyObject>, skip Arc clones
                                let func_clone = func_obj.clone();
                                unsafe {
                                    let pf_ptr = match &func_clone.payload {
                                        PyObjectPayload::Function(pf) => &**pf as *const ferrython_core::types::PyFunction,
                                        _ => std::hint::unreachable_unchecked(),
                                    };
                                    Frame::new_borrowed(&*pf_ptr, func_clone, &self.builtins, &mut self.frame_pool)
                                }
                            } else {
                                let (code, globals, constant_cache) = if let PyObjectPayload::Function(pf) = &func_obj.payload {
                                    (Rc::clone(&pf.code), pf.globals.clone(), Rc::clone(&pf.constant_cache))
                                } else { unreachable!() };
                                let mut f = Frame::new_from_pool(
                                    code, globals, self.builtins.clone(), constant_cache,
                                    &mut self.frame_pool,
                                );
                                f.scope_kind = crate::frame::ScopeKind::Function;
                                f
                            };
                            // Move args directly from parent stack to new frame locals
                            unsafe {
                                let base = frame.stack.as_ptr();
                                for i in 0..arg_count {
                                    new_frame.locals[i] = Some(
                                        std::ptr::read(base.add(args_start + i))
                                    );
                                }
                                frame.stack.set_len(args_start);
                            }
                            self.call_stack.push(new_frame);
                            // Re-derive frame_ptr: push may reallocate Vec
                            rederive_frame!(self, frame_ptr, instr_base, instr_count);
                            if self.call_stack.len() > self.recursion_limit {
                                if let Some(frame) = self.call_stack.pop() {
                                    frame.recycle(&mut self.frame_pool);
                                }
                                Err(PyException::recursion_error("maximum recursion depth exceeded"))
                            } else {
                                if has_trace {
                                    let frame_obj = self.make_trace_frame();
                                    ferrython_stdlib::set_current_frame(Some(frame_obj));
                                    self.fire_trace_event("call", PyObject::none());
                                }
                                if has_profile {
                                    self.fire_profile_event("call", PyObject::none());
                                }
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            } // close mini-interpreter else block
                        } else {
                            // Fast path for builtins (len, range) from global cache
                            let builtin_name = if let PyObjectPayload::BuiltinFunction(name) = &func_obj.payload {
                                Some(name.as_str())
                            } else { None };
                            match (builtin_name, arg_count) {
                                (Some("len"), 1) => {
                                    let arg = sget!(frame, frame.stack.len() - 1);
                                    let fast_len = match &arg.payload {
                                        PyObjectPayload::List(v) => Some(unsafe { &*v.data_ptr() }.len() as i64),
                                        PyObjectPayload::Tuple(v) => Some(v.len() as i64),
                                        PyObjectPayload::Str(s) => Some(s.chars().count() as i64),
                                        PyObjectPayload::Dict(m) => Some(unsafe { &*m.data_ptr() }.len() as i64),
                                        PyObjectPayload::Set(m) => Some(unsafe { &*m.data_ptr() }.len() as i64),
                                        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Some(b.len() as i64),
                                        _ => None,
                                    };
                                    if let Some(n) = fast_len {
                                        { let _ = spop!(frame); }
                                        spush!(frame, PyObject::int(n));
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    } else {
                                        spush!(frame, func_obj.clone());
                                        let call_instr = Instruction::new(Opcode::CallFunction, arg_count as u32);
                                        self.execute_one(call_instr)
                                    }
                                }
                                (Some("range"), 1) => {
                                    let arg = sget!(frame, frame.stack.len() - 1);
                                    if let PyObjectPayload::Int(PyInt::Small(stop)) = &arg.payload {
                                        let stop = *stop;
                                        { let _ = spop!(frame); }
                                        let iter = PyObject::wrap(PyObjectPayload::RangeIter {
                                            current: SyncI64::new(0), stop, step: 1,
                                        });
                                        spush!(frame, iter);
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    } else {
                                        spush!(frame, func_obj.clone());
                                        let call_instr = Instruction::new(Opcode::CallFunction, arg_count as u32);
                                        self.execute_one(call_instr)
                                    }
                                }
                                (Some("str"), 1) => {
                                    let arg = sget!(frame, frame.stack.len() - 1);
                                    let result = match &arg.payload {
                                        PyObjectPayload::Str(_) => Some(arg.clone()),
                                        PyObjectPayload::Int(PyInt::Small(n)) => {
                                            let mut buf = itoa::Buffer::new();
                                            Some(PyObject::str_val(CompactString::from(buf.format(*n))))
                                        }
                                        PyObjectPayload::Float(f) => {
                                            let mut buf = ryu::Buffer::new();
                                            Some(PyObject::str_val(CompactString::from(buf.format(*f))))
                                        }
                                        PyObjectPayload::Bool(b) => Some(PyObject::str_val(CompactString::from(if *b { "True" } else { "False" }))),
                                        PyObjectPayload::None => Some(PyObject::str_val(CompactString::from("None"))),
                                        _ => None,
                                    };
                                    if let Some(v) = result {
                                        { let _ = spop!(frame); }
                                        spush!(frame, v);
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    } else {
                                        spush!(frame, func_obj.clone());
                                        let call_instr = Instruction::new(Opcode::CallFunction, arg_count as u32);
                                        self.execute_one(call_instr)
                                    }
                                }
                                (Some("isinstance"), 2) => {
                                    let slen = frame.stack.len();
                                    let obj = sget!(frame, slen - 2);
                                    let cls = sget!(frame, slen - 1);
                                    let result = if let PyObjectPayload::BuiltinType(bt) = &cls.payload {
                                        match bt.as_str() {
                                            "int" => Some(matches!(&obj.payload, PyObjectPayload::Int(_) | PyObjectPayload::Bool(_))),
                                            "float" => Some(matches!(&obj.payload, PyObjectPayload::Float(_))),
                                            "str" => Some(matches!(&obj.payload, PyObjectPayload::Str(_))),
                                            "bool" => Some(matches!(&obj.payload, PyObjectPayload::Bool(_))),
                                            "list" => Some(matches!(&obj.payload, PyObjectPayload::List(_))),
                                            "dict" => Some(matches!(&obj.payload, PyObjectPayload::Dict(_) | PyObjectPayload::InstanceDict(_))),
                                            "tuple" => Some(matches!(&obj.payload, PyObjectPayload::Tuple(_))),
                                            "set" => Some(matches!(&obj.payload, PyObjectPayload::Set(_))),
                                            "bytes" => Some(matches!(&obj.payload, PyObjectPayload::Bytes(_))),
                                            "bytearray" => Some(matches!(&obj.payload, PyObjectPayload::ByteArray(_))),
                                            "object" => Some(true),
                                            _ => None,
                                        }
                                    } else {
                                        None
                                    };
                                    if let Some(matched) = result {
                                        frame.stack.truncate(slen - 2);
                                        spush!(frame, PyObject::bool_val(matched));
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    } else {
                                        spush!(frame, func_obj.clone());
                                        self.execute_one(Instruction::new(Opcode::CallFunction, 2))
                                    }
                                }
                                (Some("type"), 1) => {
                                    let arg = sget!(frame, frame.stack.len() - 1);
                                    let type_obj = match &arg.payload {
                                        PyObjectPayload::Instance(inst) => Some(inst.class.clone()),
                                        _ => None,
                                    };
                                    if let Some(t) = type_obj {
                                        { let _ = spop!(frame); }
                                        spush!(frame, t);
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    } else {
                                        spush!(frame, func_obj.clone());
                                        let call_instr = Instruction::new(Opcode::CallFunction, arg_count as u32);
                                        self.execute_one(call_instr)
                                    }
                                }
                                (Some("int"), 1) => {
                                    let arg = sget!(frame, frame.stack.len() - 1);
                                    let result = match &arg.payload {
                                        PyObjectPayload::Int(_) => Some(arg.clone()),
                                        PyObjectPayload::Bool(b) => Some(PyObject::int(*b as i64)),
                                        PyObjectPayload::Float(f) => Some(PyObject::int(*f as i64)),
                                        _ => None,
                                    };
                                    if let Some(v) = result {
                                        { let _ = spop!(frame); }
                                        spush!(frame, v);
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    } else {
                                        spush!(frame, func_obj.clone());
                                        self.execute_one(Instruction::new(Opcode::CallFunction, 1))
                                    }
                                }
                                (Some("float"), 1) => {
                                    let arg = sget!(frame, frame.stack.len() - 1);
                                    let result = match &arg.payload {
                                        PyObjectPayload::Float(_) => Some(arg.clone()),
                                        PyObjectPayload::Int(PyInt::Small(n)) => Some(PyObject::float(*n as f64)),
                                        PyObjectPayload::Bool(b) => Some(PyObject::float(if *b { 1.0 } else { 0.0 })),
                                        _ => None,
                                    };
                                    if let Some(v) = result {
                                        { let _ = spop!(frame); }
                                        spush!(frame, v);
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    } else {
                                        spush!(frame, func_obj.clone());
                                        self.execute_one(Instruction::new(Opcode::CallFunction, 1))
                                    }
                                }
                                (Some("bool"), 1) => {
                                    let arg = sget!(frame, frame.stack.len() - 1);
                                    let result = match &arg.payload {
                                        PyObjectPayload::Bool(_) => Some(arg.clone()),
                                        PyObjectPayload::Int(PyInt::Small(n)) => Some(PyObject::bool_val(*n != 0)),
                                        PyObjectPayload::Float(f) => Some(PyObject::bool_val(*f != 0.0)),
                                        PyObjectPayload::None => Some(PyObject::bool_val(false)),
                                        PyObjectPayload::Str(s) => Some(PyObject::bool_val(!s.is_empty())),
                                        PyObjectPayload::List(v) => Some(PyObject::bool_val(!unsafe { &*v.data_ptr() }.is_empty())),
                                        PyObjectPayload::Tuple(v) => Some(PyObject::bool_val(!v.is_empty())),
                                        PyObjectPayload::Dict(m) => Some(PyObject::bool_val(!unsafe { &*m.data_ptr() }.is_empty())),
                                        _ => None,
                                    };
                                    if let Some(v) = result {
                                        { let _ = spop!(frame); }
                                        spush!(frame, v);
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    } else {
                                        spush!(frame, func_obj.clone());
                                        self.execute_one(Instruction::new(Opcode::CallFunction, 1))
                                    }
                                }
                                (Some("abs"), 1) => {
                                    let arg = sget!(frame, frame.stack.len() - 1);
                                    let result = match &arg.payload {
                                        PyObjectPayload::Int(PyInt::Small(n)) => Some(PyObject::int(n.abs())),
                                        PyObjectPayload::Float(f) => Some(PyObject::float(f.abs())),
                                        _ => None,
                                    };
                                    if let Some(v) = result {
                                        { let _ = spop!(frame); }
                                        spush!(frame, v);
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    } else {
                                        spush!(frame, func_obj.clone());
                                        self.execute_one(Instruction::new(Opcode::CallFunction, 1))
                                    }
                                }
                                (Some("sum"), 1) => {
                                    let arg = sget!(frame, frame.stack.len() - 1);
                                    let items: Option<&[PyObjectRef]> = match &arg.payload {
                                        PyObjectPayload::List(v) => Some(unsafe { &*v.data_ptr() }),
                                        PyObjectPayload::Tuple(v) => Some(v.as_slice()),
                                        _ => None,
                                    };
                                    if let Some(items) = items {
                                        let mut total: i64 = 0;
                                        let mut ok = true;
                                        for item in items {
                                            if let PyObjectPayload::Int(PyInt::Small(n)) = &item.payload {
                                                if let Some(t) = total.checked_add(*n) {
                                                    total = t;
                                                } else { ok = false; break; }
                                            } else { ok = false; break; }
                                        }
                                        if ok {
                                            { let _ = spop!(frame); }
                                            spush!(frame, PyObject::int(total));
                                            hot_ok!(profiling, self.profiler, instr.op)
                                        } else {
                                            spush!(frame, func_obj.clone());
                                            self.execute_one(Instruction::new(Opcode::CallFunction, 1))
                                        }
                                    } else {
                                        spush!(frame, func_obj.clone());
                                        self.execute_one(Instruction::new(Opcode::CallFunction, 1))
                                    }
                                }
                                (Some("min"), 2) => {
                                    let sl = frame.stack.len();
                                    let a = sget!(frame, sl - 2);
                                    let b = sget!(frame, sl - 1);
                                    let result = match (&a.payload, &b.payload) {
                                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) =>
                                            Some(PyObject::int(std::cmp::min(*x, *y))),
                                        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) =>
                                            Some(PyObject::float(x.min(*y))),
                                        _ => None,
                                    };
                                    if let Some(v) = result {
                                        { let _ = spop!(frame); }
                                        { let _ = spop!(frame); }
                                        spush!(frame, v);
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    } else {
                                        spush!(frame, func_obj.clone());
                                        self.execute_one(Instruction::new(Opcode::CallFunction, 2))
                                    }
                                }
                                (Some("max"), 2) => {
                                    let sl = frame.stack.len();
                                    let a = sget!(frame, sl - 2);
                                    let b = sget!(frame, sl - 1);
                                    let result = match (&a.payload, &b.payload) {
                                        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) =>
                                            Some(PyObject::int(std::cmp::max(*x, *y))),
                                        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) =>
                                            Some(PyObject::float(x.max(*y))),
                                        _ => None,
                                    };
                                    if let Some(v) = result {
                                        { let _ = spop!(frame); }
                                        { let _ = spop!(frame); }
                                        spush!(frame, v);
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    } else {
                                        spush!(frame, func_obj.clone());
                                        self.execute_one(Instruction::new(Opcode::CallFunction, 2))
                                    }
                                }
                                _ => {
                                    // Check for isinstance(obj, cls) where cls is what we just loaded
                                    // Stack layout: [..., isinstance_func, obj, ...existing args...]
                                    // func_obj = the global we loaded (e.g., BuiltinType("int"))
                                    if arg_count == 2 {
                                        let stack_len = frame.stack.len();
                                        // The function (isinstance) should be 2 positions back
                                        if stack_len >= 2 {
                                            let func = sget!(frame, stack_len - 2);
                                            if let PyObjectPayload::BuiltinFunction(ref fn_name) = func.payload {
                                                if fn_name.as_str() == "isinstance" {
                                                    let obj = sget!(frame, stack_len - 1);
                                                    let fast_result = match &func_obj.payload {
                                                        PyObjectPayload::BuiltinType(bt) => {
                                                            match bt.as_str() {
                                                                "int" => Some(matches!(&obj.payload, PyObjectPayload::Int(_) | PyObjectPayload::Bool(_))),
                                                                "float" => Some(matches!(&obj.payload, PyObjectPayload::Float(_))),
                                                                "str" => Some(matches!(&obj.payload, PyObjectPayload::Str(_))),
                                                                "bool" => Some(matches!(&obj.payload, PyObjectPayload::Bool(_))),
                                                                "list" => Some(matches!(&obj.payload, PyObjectPayload::List(_))),
                                                                "dict" => Some(matches!(&obj.payload, PyObjectPayload::Dict(_) | PyObjectPayload::InstanceDict(_))),
                                                                "tuple" => Some(matches!(&obj.payload, PyObjectPayload::Tuple(_))),
                                                                "set" => Some(matches!(&obj.payload, PyObjectPayload::Set(_))),
                                                                "bytes" => Some(matches!(&obj.payload, PyObjectPayload::Bytes(_))),
                                                                "bytearray" => Some(matches!(&obj.payload, PyObjectPayload::ByteArray(_))),
                                                                _ => None,
                                                            }
                                                        }
                                                        PyObjectPayload::Class(cd) => {
                                                            if let PyObjectPayload::Instance(inst) = &obj.payload {
                                                                if let PyObjectPayload::Class(obj_cd) = &inst.class.payload {
                                                                    if obj_cd.name == cd.name { Some(true) }
                                                                    else if obj_cd.mro.iter().any(|b| {
                                                                        matches!(&b.payload, PyObjectPayload::Class(bc) if bc.name == cd.name)
                                                                    }) { Some(true) }
                                                                    else { None } // fall through to full isinstance (handles ABC registry, etc.)
                                                                } else { None }
                                                            } else { None }
                                                        }
                                                        _ => None,
                                                    };
                                                    if let Some(result) = fast_result {
                                                        unsafe {
                                                            let base = frame.stack.as_ptr();
                                                            let _ = std::ptr::read(base.add(stack_len - 2)); // drop isinstance
                                                            let _ = std::ptr::read(base.add(stack_len - 1)); // drop obj
                                                            frame.stack.set_len(stack_len - 2);
                                                        }
                                                        spush!(frame, PyObject::bool_val(result));
                                                        hot_ok!(profiling, self.profiler, instr.op)
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    // Not a simple function — decompose to LoadGlobal + CallFunction
                                    spush!(frame, func_obj.clone());
                                    let call_instr = Instruction::new(Opcode::CallFunction, arg_count as u32);
                                    self.execute_one(call_instr)
                                }
                            }
                        }
                    } else {
                        // Cache miss — decompose to LoadGlobal + CallFunction
                        let load_instr = Instruction::new(Opcode::LoadGlobal, name_idx as u32);
                        let res = self.execute_one(load_instr)?;
                        if res.is_some() { return Ok(res.unwrap()); }
                        let call_instr = Instruction::new(Opcode::CallFunction, arg_count as u32);
                        self.execute_one(call_instr)
                    }
                }
                // Inline LoadMethod fast path for plain instances:
                // Skip execute_one dispatch for the common case of instance.method()
                Opcode::LoadMethod => {
                    let name_idx = instr.arg as usize;
                    let stack_len = frame.stack.len();
                    // 0 = fallback, 1 = bound method (val in fast_val), 2 = instance attr (val in fast_val)
                    // 3 = builtin bound method (method_name in fast_name, receiver on TOS)
                    let mut fast_kind: u8 = 0;
                    let mut fast_val: Option<PyObjectRef> = None;
                    if stack_len > 0 {
                        let obj = sget!(frame, stack_len - 1);
                        match &obj.payload {
                            PyObjectPayload::Instance(inst) => {
                                let skip_ga = inst.class_flags & CLASS_FLAG_HAS_GETATTRIBUTE == 0;
                                if skip_ga && inst.dict_storage.is_none() && !inst.is_special {
                                    let name = &frame.code.names[name_idx];
                                    if name.as_str() != "__class__" && name.as_str() != "__dict__" {
                                        let class = &inst.class;
                                        if let PyObjectPayload::Class(cd) = &class.payload {
                                            // Check inline cache first (avoids vtable lock + hash probe)
                                            let ip = frame.ip as u32;
                                            if let Some(cached) = frame.attr_ic.as_ref().and_then(|ic| ic.lookup(ip, cd.class_version)) {
                                                if matches!(&cached.payload,
                                                    PyObjectPayload::Function(_) | PyObjectPayload::NativeFunction(_)) {
                                                    fast_kind = 1;
                                                    fast_val = Some(cached.clone());
                                                }
                                            }
                                            if fast_kind == 0 {
                                                // Vtable includes own namespace — single lock, single hash probe
                                                let vt = unsafe { &*cd.method_vtable.data_ptr() };
                                                let method_hit = if !vt.is_empty() {
                                                    vt.get(name.as_str()).cloned()
                                                } else {
                                                    unsafe { &*cd.namespace.data_ptr() }.get(name.as_str()).cloned()
                                                };
                                                if let Some(class_val) = method_hit {
                                                    if matches!(&class_val.payload,
                                                        PyObjectPayload::Function(_) | PyObjectPayload::NativeFunction(_)) {
                                                        fast_kind = 1;
                                                        frame.attr_ic.get_or_insert_with(|| Box::new(AttrInlineCache::empty())).insert(ip, cd.class_version, class_val.clone());
                                                        fast_val = Some(class_val);
                                                    }
                                                } else if let Some(v) = unsafe { &*inst.attrs.data_ptr() }.get(name.as_str()).cloned() {
                                                    fast_kind = 2;
                                                    fast_val = Some(v);
                                                } else if unsafe { &*cd.method_vtable.data_ptr() }.is_empty() {
                                                    if let Some(method) = lookup_in_class_mro(class, name.as_str()) {
                                                        if matches!(&method.payload,
                                                            PyObjectPayload::Function(_) | PyObjectPayload::NativeFunction(_)) {
                                                            fast_kind = 1;
                                                            frame.attr_ic.get_or_insert_with(|| Box::new(AttrInlineCache::empty())).insert(ip, cd.class_version, method.clone());
                                                            fast_val = Some(method);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            // Fast path for builtin type method calls (list.append, dict.get, etc.)
                            // Creates BuiltinBoundMethod inline to avoid full get_attr dispatch
                            PyObjectPayload::List(_) | PyObjectPayload::Dict(_)
                            | PyObjectPayload::Str(_) | PyObjectPayload::Tuple(_)
                            | PyObjectPayload::Set(_) | PyObjectPayload::ByteArray(_)
                            | PyObjectPayload::Bytes(_) | PyObjectPayload::InstanceDict(_) => {
                                let name = &frame.code.names[name_idx];
                                if name.as_str() != "__class__" {
                                    fast_kind = 3;
                                }
                            }
                            _ => {}
                        }
                    }
                    match fast_kind {
                        1 => {
                            // Two-item protocol: push method, then receiver
                            // CallMethod will detect method (non-None) at base slot
                            let method = fast_val.unwrap();
                            // TOS is the object — it becomes slot_1 (receiver)
                            // Insert method below it as slot_0
                            let recv = spop!(frame);
                            spush!(frame, method);
                            spush!(frame, recv);
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        2 => {
                            // Two-item protocol slow path: push None sentinel + callable
                            let val = fast_val.unwrap();
                            *frame.stack.last_mut().unwrap() = PyObject::none();
                            spush!(frame, val);
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        3 => {
                            // Builtin type method: use unbound protocol with Str tag
                            // Stack: [name_as_Str, receiver] — CallMethod detects Str in slot_0
                            // Avoids Arc allocation for BuiltinBoundMethod entirely
                            let name_obj = cached_method_name(frame.code.names[name_idx].as_str())
                                .unwrap_or_else(|| PyObject::str_val(frame.code.names[name_idx].clone()));
                            // receiver is already TOS, insert name below it
                            let recv_idx = frame.stack.len() - 1;
                            spush!(frame, name_obj);
                            frame.stack.swap(recv_idx, recv_idx + 1);
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => self.execute_one(instr),
                    }
                }
                // Inline CallMethod super-fast path: two-item protocol + direct frame creation
                Opcode::CallMethod => {
                    let arg_count = instr.arg as usize;
                    let stack_len = frame.stack.len();
                    let base_idx = stack_len - arg_count - 2;
                    let slot_0 = sget!(frame, base_idx);
                    // Fast path: slot_0 is a Python function (unbound method)
                    let is_simple_method = if !matches!(&slot_0.payload, PyObjectPayload::None) {
                        if let PyObjectPayload::Function(pf) = &slot_0.payload {
                            pf.is_simple && pf.code.arg_count as usize == arg_count + 1
                        } else { false }
                    } else { false };
                    if is_simple_method {
                        // Borrowed path: take method object, borrow its Arc fields
                        let method_idx = frame.stack.len() - arg_count - 2;
                        let arg_start = frame.stack.len() - arg_count;
                        let mut new_frame = unsafe {
                            let method_obj: PyObjectRef = std::ptr::read(frame.stack.as_ptr().add(method_idx));
                            let pf_ptr = match &method_obj.payload {
                                PyObjectPayload::Function(pf) => &**pf as *const ferrython_core::types::PyFunction,
                                _ => std::hint::unreachable_unchecked(),
                            };
                            Frame::new_borrowed(&*pf_ptr, method_obj, &self.builtins, &mut self.frame_pool)
                        };
                        // Stack: [..., method, receiver, arg0, ..., argN-1]
                        // Move args + receiver off stack with direct reads
                        unsafe {
                            let base = frame.stack.as_ptr();
                            for i in 0..arg_count {
                                new_frame.locals[i + 1] = Some(
                                    std::ptr::read(base.add(arg_start + i))
                                );
                            }
                            // receiver at arg_start - 1; method already consumed above
                            new_frame.locals[0] = Some(std::ptr::read(base.add(arg_start - 1)));
                            frame.stack.set_len(method_idx);
                        }
                        // Inherit global cache for recursive calls (same code object)
                        if Rc::ptr_eq(&frame.code, &new_frame.code) {
                            if let Some(ref cache) = frame.global_cache {
                                new_frame.global_cache = Some(cache.clone());
                                new_frame.global_cache_version = frame.global_cache_version;
                            }
                        }
                        new_frame.scope_kind = crate::frame::ScopeKind::Function;
                        self.call_stack.push(new_frame);
                        // Re-derive frame_ptr: push may reallocate Vec
                        rederive_frame!(self, frame_ptr, instr_base, instr_count);
                        if self.call_stack.len() > self.recursion_limit {
                            if let Some(f) = self.call_stack.pop() { f.recycle(&mut self.frame_pool); }
                            Err(PyException::recursion_error("maximum recursion depth exceeded"))
                        } else {
                            // Iterative: continue loop with child frame (no recursive call)
                            if has_trace {
                                let frame_obj = self.make_trace_frame();
                                ferrython_stdlib::set_current_frame(Some(frame_obj));
                                self.fire_trace_event("call", PyObject::none());
                            }
                            if has_profile {
                                self.fire_profile_event("call", PyObject::none());
                            }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                    } else {
                        // Fast path for builtin type methods (list.append, dict.get, etc.)
                        // LoadMethod pushes [name_as_Str, receiver] for builtin types
                        let is_builtin_str = matches!(&sget!(frame, base_idx).payload, PyObjectPayload::Str(_));
                        if is_builtin_str {
                            // Check for ultra-fast inline list.append / list.pop
                            let is_list_append = arg_count == 1
                                && matches!((&sget!(frame, base_idx).payload, &sget!(frame, base_idx + 1).payload),
                                    (PyObjectPayload::Str(n), PyObjectPayload::List(_)) if n.as_str() == "append");
                            let is_list_pop = !is_list_append && arg_count == 0
                                && matches!((&sget!(frame, base_idx).payload, &sget!(frame, base_idx + 1).payload),
                                    (PyObjectPayload::Str(n), PyObjectPayload::List(_)) if n.as_str() == "pop");
                            if is_list_append {
                                // Stack: [name, receiver, val] — peek receiver, pop val + truncate
                                let len = frame.stack.len();
                                unsafe {
                                    let val = std::ptr::read(frame.stack.as_ptr().add(len - 1));
                                    let receiver = &*frame.stack.as_ptr().add(len - 2);
                                    if let PyObjectPayload::List(items) = &receiver.payload {
                                        let vec = &mut *items.data_ptr();
                                        vec.push(val);
                                    }
                                    // Drop name + receiver, replace with None
                                    let _receiver = std::ptr::read(frame.stack.as_ptr().add(len - 2));
                                    let _name = std::ptr::read(frame.stack.as_ptr().add(len - 3));
                                    frame.stack.set_len(len - 3);
                                }
                                chain_pop_none!(frame, instr_base, instr_count, profiling, self.profiler, instr.op)
                            } else if is_list_pop {
                                let len = frame.stack.len();
                                unsafe {
                                    let receiver = &*frame.stack.as_ptr().add(len - 1);
                                    if let PyObjectPayload::List(items) = &receiver.payload {
                                        let vec = &mut *items.data_ptr();
                                        match vec.pop() {
                                            Some(val) => {
                                                let _receiver = std::ptr::read(frame.stack.as_ptr().add(len - 1));
                                                let _name = std::ptr::read(frame.stack.as_ptr().add(len - 2));
                                                frame.stack.set_len(len - 2);
                                                spush!(frame, val);
                                                hot_ok!(profiling, self.profiler, instr.op)
                                            }
                                            None => Err(PyException::index_error("pop from empty list")),
                                        }
                                    } else { unreachable!() }
                                }
                            } else if arg_count == 1
                                && matches!((&sget!(frame, base_idx).payload, &sget!(frame, base_idx + 1).payload),
                                    (PyObjectPayload::Str(n), PyObjectPayload::Dict(_)) if n.as_str() == "get")
                            {
                                // Inline dict.get(key) — returns None for missing keys
                                let key_obj = spop!(frame);
                                let receiver = spop!(frame);
                                { let _ = spop!(frame); } // name
                                if let PyObjectPayload::Dict(map) = &receiver.payload {
                                    let r = unsafe { &*map.data_ptr() };
                                    let val = match &key_obj.payload {
                                        PyObjectPayload::Str(s) => r.get(&BorrowedStrKey(s.as_str())).cloned(),
                                        PyObjectPayload::Int(PyInt::Small(n)) => r.get(&BorrowedIntKey(*n)).cloned(),
                                        PyObjectPayload::Bool(b) => r.get(&BorrowedIntKey(*b as i64)).cloned(),
                                        _ => None,
                                    }.unwrap_or_else(PyObject::none);
                                    spush!(frame, val);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else { unreachable!() }
                            } else {
                                // Inline fast paths for common methods — check type+name first, then pop
                                let inline_kind: u8 = {
                                    let name_s = if let PyObjectPayload::Str(n) = &sget!(frame, base_idx).payload {
                                        n.as_str()
                                    } else { "" };
                                    let recv = &sget!(frame, base_idx + 1).payload;
                                    match (name_s, recv, arg_count) {
                                        ("strip", PyObjectPayload::Str(_), 0) => 1,
                                        ("lstrip", PyObjectPayload::Str(_), 0) => 2,
                                        ("rstrip", PyObjectPayload::Str(_), 0) => 3,
                                        ("lower", PyObjectPayload::Str(_), 0) => 4,
                                        ("upper", PyObjectPayload::Str(_), 0) => 5,
                                        ("add", PyObjectPayload::Set(_), 1) => 6,
                                        ("startswith", PyObjectPayload::Str(_), 1) => 7,
                                        ("endswith", PyObjectPayload::Str(_), 1) => 8,
                                        _ => 0,
                                    }
                                };
                                if inline_kind >= 1 && inline_kind <= 5 {
                                    // 0-arg str methods
                                    let receiver = spop!(frame);
                                    { let _ = spop!(frame); } // name
                                    if let PyObjectPayload::Str(s) = &receiver.payload {
                                        let result = match inline_kind {
                                            1 => PyObject::str_val(CompactString::from(s.trim())),
                                            2 => PyObject::str_val(CompactString::from(s.trim_start())),
                                            3 => PyObject::str_val(CompactString::from(s.trim_end())),
                                            4 => PyObject::str_val(CompactString::from(s.to_lowercase())),
                                            _ => PyObject::str_val(CompactString::from(s.to_uppercase())),
                                        };
                                        spush!(frame, result);
                                    }
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else if inline_kind == 6 {
                                    // set.add(item)
                                    let item = spop!(frame);
                                    let receiver = spop!(frame);
                                    { let _ = spop!(frame); } // name
                                    if let PyObjectPayload::Set(set) = &receiver.payload {
                                        let hk = match &item.payload {
                                            PyObjectPayload::Str(s) => Some(HashableKey::str_key(s.clone())),
                                            PyObjectPayload::Int(i) => Some(HashableKey::Int(i.clone())),
                                            PyObjectPayload::Bool(b) => Some(HashableKey::Bool(*b)),
                                            _ => None,
                                        };
                                        if let Some(k) = hk {
                                            set.write().insert(k, item);
                                            chain_pop_none!(frame, instr_base, instr_count, profiling, self.profiler, instr.op)
                                        } else {
                                            // Non-hashable: use general dispatch
                                            match crate::builtins::call_method(&receiver, "add", &[item]) {
                                                Ok(result) => { spush!(frame, result); hot_ok!(profiling, self.profiler, instr.op) }
                                                Err(e) => Err(e)
                                            }
                                        }
                                    } else { unreachable!() }
                                } else if inline_kind == 7 || inline_kind == 8 {
                                    // str.startswith / str.endswith
                                    let arg = spop!(frame);
                                    let receiver = spop!(frame);
                                    { let _ = spop!(frame); } // name
                                    if let (PyObjectPayload::Str(s), PyObjectPayload::Str(prefix)) = (&receiver.payload, &arg.payload) {
                                        let result = if inline_kind == 7 { s.starts_with(prefix.as_str()) } else { s.ends_with(prefix.as_str()) };
                                        spush!(frame, PyObject::bool_val(result));
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    } else {
                                        // Not both strings — use general dispatch
                                        let name = if inline_kind == 7 { "startswith" } else { "endswith" };
                                        match crate::builtins::call_method(&receiver, name, &[arg]) {
                                            Ok(result) => { spush!(frame, result); hot_ok!(profiling, self.profiler, instr.op) }
                                            Err(e) => Err(e)
                                        }
                                    }
                                } else {
                                    // General builtin method dispatch — avoid Vec for 1-2 args
                                    if arg_count == 1 {
                                        let a0 = spop!(frame);
                                        let receiver = spop!(frame);
                                        let name_obj = spop!(frame);
                                        if let PyObjectPayload::Str(ref name) = name_obj.payload {
                                            // str.join with generator/lazy iter: collect via VM first
                                            let a0_result: Result<PyObjectRef, PyException> = if name.as_str() == "join" && matches!(&receiver.payload, PyObjectPayload::Str(_)) {
                                                match &a0.payload {
                                                    PyObjectPayload::Generator(_) => {
                                                        self.collect_iterable(&a0).map(PyObject::list)
                                                    }
                                                    PyObjectPayload::Iterator(iter_data) => {
                                                        let needs_vm = matches!(&*iter_data.read(),
                                                            IteratorData::Enumerate { .. } | IteratorData::Zip { .. }
                                                            | IteratorData::Map { .. } | IteratorData::Filter { .. }
                                                            | IteratorData::Chain { .. } | IteratorData::Starmap { .. }
                                                            | IteratorData::TakeWhile { .. } | IteratorData::DropWhile { .. }
                                                        );
                                                        if needs_vm {
                                                            self.collect_iterable(&a0).map(PyObject::list)
                                                        } else { Ok(a0) }
                                                    }
                                                    _ => Ok(a0),
                                                }
                                            } else { Ok(a0) };
                                            match a0_result.and_then(|a0| crate::builtins::call_method(&receiver, name.as_str(), &[a0])) {
                                                Ok(result) => { spush!(frame, result); hot_ok!(profiling, self.profiler, instr.op) }
                                                Err(e) => Err(e)
                                            }
                                        } else { unreachable!() }
                                    } else if arg_count == 2 {
                                        let a1 = spop!(frame);
                                        let a0 = spop!(frame);
                                        let receiver = spop!(frame);
                                        let name_obj = spop!(frame);
                                        if let PyObjectPayload::Str(ref name) = name_obj.payload {
                                            match crate::builtins::call_method(&receiver, name.as_str(), &[a0, a1]) {
                                                Ok(result) => { spush!(frame, result); hot_ok!(profiling, self.profiler, instr.op) }
                                                Err(e) => Err(e)
                                            }
                                        } else { unreachable!() }
                                    } else if arg_count == 0 {
                                        let receiver = spop!(frame);
                                        let name_obj = spop!(frame);
                                        if let PyObjectPayload::Str(ref name) = name_obj.payload {
                                            match crate::builtins::call_method(&receiver, name.as_str(), &[]) {
                                                Ok(result) => { spush!(frame, result); hot_ok!(profiling, self.profiler, instr.op) }
                                                Err(e) => Err(e)
                                            }
                                        } else { unreachable!() }
                                    } else {
                                        let mut args = Vec::with_capacity(arg_count);
                                        for _ in 0..arg_count {
                                            args.push(spop!(frame));
                                        }
                                        args.reverse();
                                        let receiver = spop!(frame);
                                        let name_obj = spop!(frame);
                                        if let PyObjectPayload::Str(ref name) = name_obj.payload {
                                            match crate::builtins::call_method(&receiver, name.as_str(), &args) {
                                                Ok(result) => { spush!(frame, result); hot_ok!(profiling, self.profiler, instr.op) }
                                                Err(e) => Err(e)
                                            }
                                        } else {
                                            unreachable!()
                                        }
                                    }
                                }
                            }
                        } else {
                            self.execute_one(instr)
                        }
                    }
                }
                // Fused CallMethod + PopTop — discard return value (common for list.append, etc.)
                Opcode::CallMethodPopTop => {
                    let arg_count = instr.arg as usize;
                    let stack_len = frame.stack.len();
                    let base_idx = stack_len - arg_count - 2;
                    // Ultra-fast pointer-identity check for list.append (most common case)
                    // Skips all pattern matching and string comparison when the method name
                    // is the same interned singleton pushed by LoadFastLoadMethod.
                    if arg_count == 1 && is_interned_append(sget!(frame, base_idx)) {
                        if let PyObjectPayload::List(items) = &sget!(frame, base_idx + 1).payload {
                            unsafe {
                                let val = std::ptr::read(frame.stack.as_ptr().add(stack_len - 1));
                                (&mut *items.data_ptr()).push(val);
                                let _receiver = std::ptr::read(frame.stack.as_ptr().add(stack_len - 2));
                                // Name is immortal — drop is no-op, but we must still read it off
                                let _name = std::ptr::read(frame.stack.as_ptr().add(stack_len - 3));
                                frame.stack.set_len(stack_len - 3);
                            }
                            hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                        }
                    }
                    // Ultra-fast pointer-identity check for list.pop
                    if arg_count == 0 && is_interned_pop(sget!(frame, base_idx)) {
                        if let PyObjectPayload::List(items) = &sget!(frame, base_idx + 1).payload {
                            unsafe {
                                let vec = &mut *items.data_ptr();
                                if let Some(_val) = vec.pop() {
                                    let _receiver = std::ptr::read(frame.stack.as_ptr().add(stack_len - 1));
                                    let _name = std::ptr::read(frame.stack.as_ptr().add(stack_len - 2));
                                    frame.stack.set_len(stack_len - 2);
                                    hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                                }
                                // Empty list falls through to existing string-comparison path
                            }
                        }
                    }
                    // Builtin type method dispatch (Str name tag on stack)
                    let is_builtin_str = matches!(&sget!(frame, base_idx).payload, PyObjectPayload::Str(_));
                    if is_builtin_str {
                        // Fallback: string comparison for methods not caught by pointer identity
                        let is_list_append = arg_count == 1
                            && matches!((&sget!(frame, base_idx).payload, &sget!(frame, base_idx + 1).payload),
                                (PyObjectPayload::Str(n), PyObjectPayload::List(_)) if n.as_str() == "append");
                        let is_list_pop = !is_list_append && arg_count == 0
                            && matches!((&sget!(frame, base_idx).payload, &sget!(frame, base_idx + 1).payload),
                                (PyObjectPayload::Str(n), PyObjectPayload::List(_)) if n.as_str() == "pop");
                        if is_list_append {
                            let len = frame.stack.len();
                            unsafe {
                                let val = std::ptr::read(frame.stack.as_ptr().add(len - 1));
                                let receiver = &*frame.stack.as_ptr().add(len - 2);
                                if let PyObjectPayload::List(items) = &receiver.payload {
                                    let vec = &mut *items.data_ptr();
                                    vec.push(val);
                                }
                                let _receiver = std::ptr::read(frame.stack.as_ptr().add(len - 2));
                                let _name = std::ptr::read(frame.stack.as_ptr().add(len - 3));
                                frame.stack.set_len(len - 3);
                            }
                            hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                        } else if is_list_pop {
                            let len = frame.stack.len();
                            unsafe {
                                let receiver = &*frame.stack.as_ptr().add(len - 1);
                                if let PyObjectPayload::List(items) = &receiver.payload {
                                    let vec = &mut *items.data_ptr();
                                    match vec.pop() {
                                        Some(_val) => {
                                            let _receiver = std::ptr::read(frame.stack.as_ptr().add(len - 1));
                                            let _name = std::ptr::read(frame.stack.as_ptr().add(len - 2));
                                            frame.stack.set_len(len - 2);
                                            hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                                        }
                                        None => Err(PyException::index_error("pop from empty list")),
                                    }
                                } else { unreachable!() }
                            }
                        } else {
                            // General builtin method — execute, then discard result
                            let call_result = if arg_count == 1 {
                                let a0 = spop!(frame);
                                let receiver = spop!(frame);
                                let name_obj = spop!(frame);
                                if let PyObjectPayload::Str(ref name) = name_obj.payload {
                                    crate::builtins::call_method(&receiver, name.as_str(), &[a0])
                                } else { Ok(PyObject::none()) }
                            } else if arg_count == 0 {
                                let receiver = spop!(frame);
                                let name_obj = spop!(frame);
                                if let PyObjectPayload::Str(ref name) = name_obj.payload {
                                    crate::builtins::call_method(&receiver, name.as_str(), &[])
                                } else { Ok(PyObject::none()) }
                            } else {
                                let mut args = Vec::with_capacity(arg_count);
                                for _ in 0..arg_count { args.push(spop!(frame)); }
                                args.reverse();
                                let receiver = spop!(frame);
                                let name_obj = spop!(frame);
                                if let PyObjectPayload::Str(ref name) = name_obj.payload {
                                    crate::builtins::call_method(&receiver, name.as_str(), &args)
                                } else { Ok(PyObject::none()) }
                            };
                            match call_result {
                                Ok(_) => { hot_ok!(profiling, self.profiler, instr.op) }
                                Err(e) => Err(e)
                            }
                        }
                    } else {
                        // Python function call or other: delegate to CallMethod, result handler
                        // will detect CallMethodPopTop and discard the return value
                        let cm_instr = ferrython_bytecode::Instruction::new(Opcode::CallMethod, instr.arg);
                        let slot_0 = sget!(frame, base_idx);
                        let fast_data = if !matches!(&slot_0.payload, PyObjectPayload::None) {
                            if let PyObjectPayload::Function(pf) = &slot_0.payload {
                                if pf.is_simple && pf.code.arg_count as usize == arg_count + 1 {
                                    Some((Rc::clone(&pf.code), pf.globals.clone(), Rc::clone(&pf.constant_cache)))
                                } else { None }
                            } else { None }
                        } else { None };
                        if let Some((code, globals, cc)) = fast_data {
                            // Inline frame creation (same as CallMethod)
                            let mut new_frame = Frame::new_from_pool(
                                code, globals, self.builtins.clone(), cc, &mut self.frame_pool,
                            );
                            let arg_start = frame.stack.len() - arg_count;
                            unsafe {
                                let base = frame.stack.as_ptr();
                                for ii in 0..arg_count {
                                    new_frame.locals[ii + 1] = Some(std::ptr::read(base.add(arg_start + ii)));
                                }
                                new_frame.locals[0] = Some(std::ptr::read(base.add(arg_start - 1)));
                                let _method = std::ptr::read(base.add(arg_start - 2));
                                frame.stack.set_len(arg_start - 2);
                            }
                            if Rc::ptr_eq(&frame.code, &new_frame.code) {
                                if let Some(ref cache) = frame.global_cache {
                                    new_frame.global_cache = Some(cache.clone());
                                    new_frame.global_cache_version = frame.global_cache_version;
                                }
                            }
                            new_frame.scope_kind = crate::frame::ScopeKind::Function;
                            self.call_stack.push(new_frame);
                            // Re-derive frame_ptr: push may reallocate Vec
                            rederive_frame!(self, frame_ptr, instr_base, instr_count);
                            if self.call_stack.len() > self.recursion_limit {
                                if let Some(f) = self.call_stack.pop() { f.recycle(&mut self.frame_pool); }
                                Err(PyException::recursion_error("maximum recursion depth exceeded"))
                            } else {
                                // Child frame pushed. When it returns, the Ok(Some(ret)) handler
                                // will check that the calling instruction was CallMethodPopTop
                                // and discard the return value instead of pushing it.
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                        } else {
                            // Non-fast-path: delegate to execute_one, then pop result
                            match self.execute_one(cm_instr) {
                                Ok(res) => {
                                    if res.is_none() {
                                        let frame2 = self.call_stack.last_mut().unwrap();
                                        if !frame2.stack.is_empty() {
                                            drop(spop!(frame2));
                                        }
                                    }
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                Err(e) => Err(e)
                            }
                        }
                    }
                }
                // Inline BinarySubscr for list[int], tuple[int], dict[HashableKey]
                Opcode::BinarySubscr => {
                    let len = frame.stack.len();
                    // SAFETY: well-formed bytecode guarantees stack depth >= 2
                    let obj = sget!(frame, len - 2);
                    let key = sget!(frame, len - 1);
                    match (&obj.payload, &key.payload) {
                            // list[int] — lock-free direct index
                            // SAFETY: single-threaded interpreter
                            (PyObjectPayload::List(items_arc), PyObjectPayload::Int(PyInt::Small(idx))) => {
                                let items = unsafe { &*items_arc.data_ptr() };
                                let i = *idx;
                                let actual = if i < 0 { i + items.len() as i64 } else { i };
                                if actual >= 0 && (actual as usize) < items.len() {
                                    let val = items[actual as usize].clone();
                                    unsafe { frame.binary_op_result(val) };
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            // tuple[int] — direct index
                            (PyObjectPayload::Tuple(items), PyObjectPayload::Int(PyInt::Small(idx))) => {
                                let i = *idx;
                                let actual = if i < 0 { i + items.len() as i64 } else { i };
                                if actual >= 0 && (actual as usize) < items.len() {
                                    let val = items[actual as usize].clone();
                                    unsafe { frame.binary_op_result(val) };
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            // dict[str] — zero-clone hash lookup
                            (PyObjectPayload::Dict(map), PyObjectPayload::Str(s)) => {
                                let val = unsafe { &*map.data_ptr() }.get(&BorrowedStrKey(s.as_str())).cloned();
                                if let Some(v) = val {
                                    unsafe { frame.binary_op_result(v) };
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            // dict[int] — lock-free hash lookup
                            (PyObjectPayload::Dict(map), PyObjectPayload::Int(PyInt::Small(n))) => {
                                let val = unsafe { &*map.data_ptr() }.get(&BorrowedIntKey(*n)).cloned();
                                if let Some(v) = val {
                                    unsafe { frame.binary_op_result(v) };
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            // str[int] — character index
                            (PyObjectPayload::Str(s), PyObjectPayload::Int(PyInt::Small(idx))) => {
                                let chars: Vec<char> = s.chars().collect();
                                let i = *idx;
                                let actual = if i < 0 { i + chars.len() as i64 } else { i };
                                if actual >= 0 && (actual as usize) < chars.len() {
                                    let ch = chars[actual as usize];
                                    let val = PyObject::str_val(CompactString::from(ch.to_string()));
                                    unsafe { frame.binary_op_result(val) };
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            _ => self.execute_one(instr),
                        }
                }
                // Inline StoreSubscr for list[int] and dict[str/int]
                Opcode::StoreSubscr => {
                    let len = frame.stack.len();
                    // SAFETY: well-formed bytecode guarantees stack depth >= 3
                    let key = sget!(frame, len - 1);
                    let obj = sget!(frame, len - 2);
                        match (&obj.payload, &key.payload) {
                            // list[int] = val — lock-free, zero-clone
                            (PyObjectPayload::List(items_arc), PyObjectPayload::Int(PyInt::Small(idx))) => {
                                let items = unsafe { &mut *items_arc.data_ptr() };
                                let i = *idx;
                                let actual = if i < 0 { i + items.len() as i64 } else { i };
                                if actual >= 0 && (actual as usize) < items.len() {
                                    // Move value from stack via ptr::read (no Rc::clone)
                                    let v = unsafe { std::ptr::read(frame.stack.as_ptr().add(len - 3)) };
                                    // Old element drops via assignment
                                    items[actual as usize] = v;
                                    // Drop key and obj, value already moved
                                    unsafe {
                                        std::ptr::drop_in_place(frame.stack.as_mut_ptr().add(len - 1));
                                        std::ptr::drop_in_place(frame.stack.as_mut_ptr().add(len - 2));
                                        frame.stack.set_len(len - 3);
                                    }
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            // dict[str] = val — lock-free, zero-clone value
                            (PyObjectPayload::Dict(map), PyObjectPayload::Str(s)) => {
                                let hk = HashableKey::str_key(s.clone());
                                let map_ptr = map.data_ptr();
                                unsafe {
                                    let v = std::ptr::read(frame.stack.as_ptr().add(len - 3));
                                    (&mut *map_ptr).insert(hk, v);
                                    std::ptr::drop_in_place(frame.stack.as_mut_ptr().add(len - 1));
                                    std::ptr::drop_in_place(frame.stack.as_mut_ptr().add(len - 2));
                                    frame.stack.set_len(len - 3);
                                }
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            // dict[int] = val — lock-free, zero-clone value
                            (PyObjectPayload::Dict(map), PyObjectPayload::Int(PyInt::Small(n))) => {
                                let map_ptr = map.data_ptr();
                                let int_val = *n;
                                unsafe {
                                    let v = std::ptr::read(frame.stack.as_ptr().add(len - 3));
                                    (&mut *map_ptr).insert(HashableKey::Int(PyInt::Small(int_val)), v);
                                    std::ptr::drop_in_place(frame.stack.as_mut_ptr().add(len - 1));
                                    std::ptr::drop_in_place(frame.stack.as_mut_ptr().add(len - 2));
                                    frame.stack.set_len(len - 3);
                                }
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            // dict[bool] = val — lock-free, zero-clone value
                            (PyObjectPayload::Dict(map), PyObjectPayload::Bool(b)) => {
                                let map_ptr = map.data_ptr();
                                let bool_val = *b;
                                unsafe {
                                    let v = std::ptr::read(frame.stack.as_ptr().add(len - 3));
                                    (&mut *map_ptr).insert(HashableKey::Int(PyInt::Small(bool_val as i64)), v);
                                    std::ptr::drop_in_place(frame.stack.as_mut_ptr().add(len - 1));
                                    std::ptr::drop_in_place(frame.stack.as_mut_ptr().add(len - 2));
                                    frame.stack.set_len(len - 3);
                                }
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            _ => self.execute_one(instr),
                        }
                }
                // Inline ListAppend (hot in list comprehensions)
                Opcode::ListAppend => {
                    let item = spop!(frame);
                    let idx = instr.arg as usize;
                    let stack_pos = frame.stack.len() - idx;
                    if let PyObjectPayload::List(items) = &sget!(frame, stack_pos).payload {
                        unsafe { &mut *items.data_ptr() }.push(item);
                    }
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                // Inline MapAdd (hot in dict comprehensions) — avoids clone of dict_obj
                Opcode::MapAdd => {
                    let len = frame.stack.len();
                    let key_ref = sget!(frame, len - 2);
                    let idx = instr.arg as usize;
                    let stack_pos = len - 2 - idx;
                    // Fast path: int key (most common in dict comprehensions)
                    let hk = match &key_ref.payload {
                        PyObjectPayload::Int(PyInt::Small(n)) => {
                            Some(HashableKey::Int(PyInt::Small(*n)))
                        }
                        PyObjectPayload::Str(s) => {
                            Some(HashableKey::str_key(s.clone()))
                        }
                        PyObjectPayload::Bool(b) => {
                            Some(HashableKey::Int(PyInt::Small(*b as i64)))
                        }
                        _ => None,
                    };
                    if let Some(hk) = hk {
                        // Get raw pointer to dict map BEFORE popping (avoids borrow conflict)
                        let map_ptr = if let PyObjectPayload::Dict(m) = &sget!(frame, stack_pos).payload {
                            Some(m.data_ptr())
                        } else { None };
                        if let Some(map_ptr) = map_ptr {
                            let value = spop!(frame);
                            let _key = spop!(frame);
                            unsafe { &mut *map_ptr }.insert(hk, value);
                            hot_ok!(profiling, self.profiler, instr.op)
                        } else {
                            self.execute_one(instr)
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline SetAdd (hot in set comprehensions) — avoids clone of set_obj
                Opcode::SetAdd => {
                    let len = frame.stack.len();
                    let item_ref = sget!(frame, len - 1);
                    let idx = instr.arg as usize;
                    let stack_pos = len - 1 - idx;
                    // Fast path: int/str/bool key
                    let hk = match &item_ref.payload {
                        PyObjectPayload::Int(PyInt::Small(n)) => {
                            Some(HashableKey::Int(PyInt::Small(*n)))
                        }
                        PyObjectPayload::Str(s) => {
                            Some(HashableKey::str_key(s.clone()))
                        }
                        PyObjectPayload::Bool(b) => {
                            Some(HashableKey::Int(PyInt::Small(*b as i64)))
                        }
                        _ => None,
                    };
                    if let Some(hk) = hk {
                        // Get raw pointer to set map BEFORE popping
                        let set_ptr = if let PyObjectPayload::Set(s) = &sget!(frame, stack_pos).payload {
                            Some(s.data_ptr())
                        } else { None };
                        if let Some(set_ptr) = set_ptr {
                            let item = spop!(frame);
                            unsafe { &mut *set_ptr }.insert(hk, item);
                            hot_ok!(profiling, self.profiler, instr.op)
                        } else {
                            self.execute_one(instr)
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline LoadAttr fast path for simple instance attribute reads
                // Fused LoadFast + LoadAttr — common in `x = obj.attr` patterns
                Opcode::LoadFastLoadAttr => {
                    let local_idx = (instr.arg >> 16) as usize;
                    let name_idx = (instr.arg & 0xFFFF) as usize;
                    let name = &frame.code.names[name_idx];
                    let obj = match slocal!(frame, local_idx) {
                        Some(val) => val,
                        None => {
                            return Self::err_unbound_local(&frame.code.varnames, local_idx)
                                .map(|_| PyObject::none());
                        }
                    };
                    // Inline Instance attr fast path
                    let fast_val = if let PyObjectPayload::Instance(inst) = &obj.payload {
                        if inst.class_flags & CLASS_FLAG_HAS_GETATTRIBUTE == 0 {
                            if name.as_str() == "__class__" {
                                Some(inst.class.clone())
                            } else {
                                let attrs = unsafe { &*inst.attrs.data_ptr() };
                                if let Some(v) = attrs.get(name.as_str()) {
                                    match &v.payload {
                                        PyObjectPayload::Function(_)
                                        | PyObjectPayload::Property(_) => None,
                                        _ => Some(v.clone()),
                                    }
                                } else {
                                    drop(attrs);
                                    // Instance dict miss — check vtable for class-level data attrs
                                    if inst.class_flags & CLASS_FLAG_HAS_DESCRIPTORS == 0 {
                                        if let PyObjectPayload::Class(cd) = &inst.class.payload {
                                            let vt = unsafe { &*cd.method_vtable.data_ptr() };
                                            if !vt.is_empty() {
                                                if let Some(class_val) = vt.get(name.as_str()) {
                                                    match &class_val.payload {
                                                        PyObjectPayload::Function(_)
                                                        | PyObjectPayload::NativeFunction(_)
                                                        | PyObjectPayload::NativeClosure { .. }
                                                        | PyObjectPayload::Property(_)
                                                        | PyObjectPayload::ClassMethod(_)
                                                        | PyObjectPayload::StaticMethod(_) => None,
                                                        // cached_property descriptor — must invoke, not return raw
                                                        PyObjectPayload::Instance(cp_inst) if cp_inst.attrs.read().contains_key("__cached_property_func__") => None,
                                                        _ => Some(class_val.clone()),
                                                    }
                                                } else { None }
                                            } else { None }
                                        } else { None }
                                    } else { None }
                                }
                            }
                        } else { None }
                    } else { None };
                    if let Some(val) = fast_val {
                        spush!(frame, val);
                        hot_ok!(profiling, self.profiler, instr.op)
                    } else {
                        // Decompose: push local, then execute LoadAttr
                        spush!(frame, obj.clone());
                        let attr_instr = Instruction::new(Opcode::LoadAttr, name_idx as u32);
                        self.execute_one(attr_instr)
                    }
                }
                // Fused LoadFast + LoadAttr + StoreFast — eliminates one dispatch for `x = obj.attr`
                Opcode::LoadFastLoadAttrStoreFast => {
                    let local_idx = ((instr.arg >> 20) & 0x3FF) as usize;
                    let name_idx = ((instr.arg >> 10) & 0x3FF) as usize;
                    let store_idx = (instr.arg & 0x3FF) as usize;
                    let name = &frame.code.names[name_idx];
                    let obj = match slocal!(frame, local_idx) {
                        Some(val) => val,
                        None => {
                            return Self::err_unbound_local(&frame.code.varnames, local_idx)
                                .map(|_| PyObject::none());
                        }
                    };
                    // Inline Instance attr fast path with IC
                    let fast_val = if let PyObjectPayload::Instance(inst) = &obj.payload {
                        if inst.class_flags & CLASS_FLAG_HAS_GETATTRIBUTE == 0 {
                            if let PyObjectPayload::Class(cd) = &inst.class.payload {
                                // Check inline cache first
                                let ip = frame.ip as u32;
                                if let Some(cached) = frame.attr_ic.as_ref().and_then(|ic| ic.lookup(ip, cd.class_version)) {
                                    Some(cached.clone())
                                } else if name.as_str() == "__class__" {
                                    Some(inst.class.clone())
                                } else {
                                    let attrs = unsafe { &*inst.attrs.data_ptr() };
                                    if let Some(v) = attrs.get(name.as_str()) {
                                        match &v.payload {
                                            PyObjectPayload::Function(_)
                                            | PyObjectPayload::Property(_) => None,
                                            _ => Some(v.clone()),
                                        }
                                    } else {
                                        drop(attrs);
                                        // Instance dict miss — check vtable for class-level attrs
                                        let vt = unsafe { &*cd.method_vtable.data_ptr() };
                                        if !vt.is_empty() {
                                            if let Some(class_val) = vt.get(name.as_str()) {
                                                match &class_val.payload {
                                                    PyObjectPayload::Function(_)
                                                    | PyObjectPayload::NativeFunction(_)
                                                    | PyObjectPayload::NativeClosure { .. }
                                                    | PyObjectPayload::Property(_)
                                                    | PyObjectPayload::ClassMethod(_)
                                                    | PyObjectPayload::StaticMethod(_) => None,
                                                    // cached_property descriptor — must invoke, not return raw
                                                    PyObjectPayload::Instance(cp_inst) if cp_inst.attrs.read().contains_key("__cached_property_func__") => None,
                                                    _ => {
                                                        // Cache class-level non-descriptor attrs
                                                        let val = class_val.clone();
                                                        frame.attr_ic.get_or_insert_with(|| Box::new(AttrInlineCache::empty())).insert(ip, cd.class_version, val.clone());
                                                        Some(val)
                                                    }
                                                }
                                            } else { None }
                                        } else { None }
                                    }
                                }
                            } else { None }
                        } else { None }
                    } else { None };
                    if let Some(val) = fast_val {
                        sset_local!(frame, store_idx, val);
                        hot_ok!(profiling, self.profiler, instr.op)
                    } else {
                        // Decompose: push local, execute LoadAttr, re-acquire frame for store
                        spush!(frame, obj.clone());
                        let attr_instr = Instruction::new(Opcode::LoadAttr, name_idx as u32);
                        let result = self.execute_one(attr_instr);
                        if result.is_ok() {
                            // Re-acquire frame reference (execute_one may have invalidated it)
                            let cs_len = self.call_stack.len();
                            let frame2 = unsafe { self.call_stack.get_unchecked_mut(cs_len - 1) };
                            let val = spop!(frame2);
                            sset_local!(frame2, store_idx, val);
                        }
                        result
                    }
                }
                // Fused LoadFast + LoadMethod — avoids one dispatch per method call
                Opcode::LoadFastLoadMethod => {
                    let local_idx = (instr.arg >> 16) as usize;
                    let name_idx = (instr.arg & 0xFFFF) as usize;
                    let obj = match slocal!(frame, local_idx) {
                        Some(val) => val.clone(),
                        None => {
                            return Self::err_unbound_local(&frame.code.varnames, local_idx)
                                .map(|_| PyObject::none());
                        }
                    };
                    // Determine fast_kind based on object type
                    let mut fast_kind: u8 = 0;
                    let mut fast_val: Option<PyObjectRef> = None;
                    match &obj.payload {
                        PyObjectPayload::Instance(inst) => {
                            let skip_ga = inst.class_flags & CLASS_FLAG_HAS_GETATTRIBUTE == 0;
                            if skip_ga && inst.dict_storage.is_none() && !inst.is_special {
                                let name = &frame.code.names[name_idx];
                                if name.as_str() != "__class__" && name.as_str() != "__dict__" {
                                    let class = &inst.class;
                                    if let PyObjectPayload::Class(cd) = &class.payload {
                                        // Check inline cache first (avoids vtable lock + hash probe)
                                        let ip = frame.ip as u32;
                                        if let Some(cached) = frame.attr_ic.as_ref().and_then(|ic| ic.lookup(ip, cd.class_version)) {
                                            if matches!(&cached.payload,
                                                PyObjectPayload::Function(_) | PyObjectPayload::NativeFunction(_)) {
                                                fast_kind = 1;
                                                fast_val = Some(cached.clone());
                                            }
                                        }
                                        if fast_kind == 0 {
                                            // Use vtable (single lock, single hash probe) instead of namespace.read()
                                            let vt = unsafe { &*cd.method_vtable.data_ptr() };
                                            let method_hit = if !vt.is_empty() {
                                                vt.get(name.as_str()).cloned()
                                            } else {
                                                drop(vt);
                                                unsafe { &*cd.namespace.data_ptr() }.get(name.as_str()).cloned()
                                            };
                                            if let Some(class_val) = method_hit {
                                                if matches!(&class_val.payload,
                                                    PyObjectPayload::Function(_) | PyObjectPayload::NativeFunction(_)) {
                                                    fast_kind = 1;
                                                    // Populate IC for next time
                                                    frame.attr_ic.get_or_insert_with(|| Box::new(AttrInlineCache::empty())).insert(ip, cd.class_version, class_val.clone());
                                                    fast_val = Some(class_val);
                                                }
                                            } else if let Some(v) = unsafe { &*inst.attrs.data_ptr() }.get(name.as_str()).cloned() {
                                                fast_kind = 2;
                                                fast_val = Some(v);
                                            } else if unsafe { &*cd.method_vtable.data_ptr() }.is_empty() {
                                                if let Some(method) = lookup_in_class_mro(class, name.as_str()) {
                                                    if matches!(&method.payload,
                                                        PyObjectPayload::Function(_) | PyObjectPayload::NativeFunction(_)) {
                                                        fast_kind = 1;
                                                        frame.attr_ic.get_or_insert_with(|| Box::new(AttrInlineCache::empty())).insert(ip, cd.class_version, method.clone());
                                                        fast_val = Some(method);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        PyObjectPayload::List(_) | PyObjectPayload::Dict(_)
                        | PyObjectPayload::Str(_) | PyObjectPayload::Tuple(_)
                        | PyObjectPayload::Set(_) | PyObjectPayload::ByteArray(_)
                        | PyObjectPayload::Bytes(_) | PyObjectPayload::InstanceDict(_) => {
                            let name = &frame.code.names[name_idx];
                            if name.as_str() != "__class__" {
                                fast_kind = 3;
                            }
                        }
                        _ => {}
                    }
                    match fast_kind {
                        1 => {
                            // method + receiver protocol
                            spush!(frame, fast_val.unwrap());
                            spush!(frame, obj);
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        2 => {
                            // None sentinel + callable
                            spush!(frame, PyObject::none());
                            spush!(frame, fast_val.unwrap());
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        3 => {
                            // Builtin: Str name tag + receiver (cached to avoid allocation)
                            let name_obj = cached_method_name(frame.code.names[name_idx].as_str())
                                .unwrap_or_else(|| PyObject::str_val(frame.code.names[name_idx].clone()));
                            spush!(frame, name_obj);
                            spush!(frame, obj);
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => {
                            // Fallback: push local to stack, execute LoadMethod
                            spush!(frame, obj);
                            let method_instr = Instruction::new(Opcode::LoadMethod, name_idx as u32);
                            self.execute_one(method_instr)
                        }
                    }
                }
                Opcode::LoadAttr => {
                    let name = &frame.code.names[instr.arg as usize];
                    let obj = sget!(frame, frame.stack.len() - 1);
                    // Fast path: Instance with no __getattribute__ override (cached flag)
                    let fast_val = if let PyObjectPayload::Instance(inst) = &obj.payload {
                        if inst.class_flags & CLASS_FLAG_HAS_GETATTRIBUTE == 0 {
                            // Check instance dict first (most common case — data attrs like p.x)
                            let attrs = unsafe { &*inst.attrs.data_ptr() };
                            if let Some(v) = attrs.get(name.as_str()) {
                                match &v.payload {
                                    PyObjectPayload::Function(_)
                                    | PyObjectPayload::Property(_) => None,
                                    _ => Some(v.clone()),
                                }
                            } else if name.as_str() == "__class__" {
                                // __class__ is a data descriptor — only checked on dict miss
                                Some(inst.class.clone())
                            } else if inst.class_flags & CLASS_FLAG_HAS_DESCRIPTORS == 0 {
                                // Instance dict miss — check vtable for class attrs
                                if let PyObjectPayload::Class(cd) = &inst.class.payload {
                                    let vt = unsafe { &*cd.method_vtable.data_ptr() };
                                    if !vt.is_empty() {
                                        if let Some(class_val) = vt.get(name.as_str()) {
                                            match &class_val.payload {
                                                PyObjectPayload::Function(_)
                                                | PyObjectPayload::NativeFunction(_)
                                                | PyObjectPayload::NativeClosure { .. }
                                                | PyObjectPayload::Property(_)
                                                | PyObjectPayload::ClassMethod(_)
                                                | PyObjectPayload::StaticMethod(_) => None,
                                                // cached_property descriptor — must invoke, not return raw
                                                PyObjectPayload::Instance(cp_inst) if cp_inst.attrs.read().contains_key("__cached_property_func__") => None,
                                                _ => Some(class_val.clone()),
                                            }
                                        } else { None }
                                    } else { None }
                                } else { None }
                            } else { None }
                        } else { None }
                    } else { None };
                    if let Some(val) = fast_val {
                        let len = frame.stack.len();
                        unsafe { *frame.stack.get_unchecked_mut(len - 1) = val };
                        hot_ok!(profiling, self.profiler, instr.op)
                    } else {
                        self.execute_one(instr)
                    }
                }
                // StoreGlobal/StoreName: not in tight loops, delegate to cold path
                Opcode::StoreGlobal | Opcode::StoreName => self.execute_one(instr),
                // Inline StoreAttr fast path for simple instance attribute writes
                Opcode::StoreAttr => {
                    let name = &frame.code.names[instr.arg as usize];
                    let stack_len = frame.stack.len();
                    // Fast path: Instance with no __setattr__, no descriptors, no __slots__ (cached flags)
                    let fast = if stack_len >= 2 {
                        if let PyObjectPayload::Instance(inst) = &sget!(frame, stack_len - 1).payload {
                            inst.class_flags & (CLASS_FLAG_HAS_SETATTR | CLASS_FLAG_HAS_DESCRIPTORS | CLASS_FLAG_HAS_SLOTS) == 0
                        } else { false }
                    } else { false };
                    if fast {
                        let obj = spop!(frame);
                        let value = spop!(frame);
                        if let PyObjectPayload::Instance(inst) = &obj.payload {
                            let map = unsafe { &mut *inst.attrs.data_ptr() };
                            // Fast path: update existing attr without key allocation
                            if let Some(slot) = map.get_mut(name) {
                                *slot = value;
                            } else {
                                map.insert(name.clone(), value);
                            }
                        }
                        hot_ok!(profiling, self.profiler, instr.op)
                    } else {
                        self.execute_one(instr)
                    }
                }
                // CompareOp catch-all: all common cases handled by guarded arms above
                Opcode::CompareOp => self.execute_one(instr),
                // Fused CompareOp + PopJumpIfFalse: avoids intermediate bool allocation
                Opcode::CompareOpPopJumpIfFalse => {
                    let cmp_op = instr.arg >> 24;
                    let jump_target = (instr.arg & 0x00FF_FFFF) as usize;
                    let len = frame.stack.len();
                    if len >= 2 {
                        let a = sget!(frame, len - 2);
                        let b = sget!(frame, len - 1);
                        let fast_result = match (&a.payload, &b.payload) {
                            (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                match cmp_op {
                                    0 => Some(*x < *y),
                                    1 => Some(*x <= *y),
                                    2 => Some(*x == *y),
                                    3 => Some(*x != *y),
                                    4 => Some(*x > *y),
                                    5 => Some(*x >= *y),
                                    _ => None,
                                }
                            }
                            (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                match cmp_op {
                                    0 => Some(*x < *y),
                                    1 => Some(*x <= *y),
                                    2 => Some(*x == *y),
                                    3 => Some(*x != *y),
                                    4 => Some(*x > *y),
                                    5 => Some(*x >= *y),
                                    _ => None,
                                }
                            }
                            _ => None,
                        };
                        if let Some(is_true) = fast_result {
                            // Pop both operands without intermediate Arc operations
                            let len = frame.stack.len();
                            unsafe {
                                let _a = std::ptr::read(frame.stack.as_ptr().add(len - 1));
                                let _b = std::ptr::read(frame.stack.as_ptr().add(len - 2));
                                frame.stack.set_len(len - 2);
                            }
                            if !is_true { frame.ip = jump_target; }
                            hot_ok!(profiling, self.profiler, instr.op)
                        } else {
                            // Fallback: execute CompareOp, then check result
                            let cmp_instr = Instruction::new(Opcode::CompareOp, cmp_op);
                            let result = self.exec_compare_ops(cmp_instr)?;
                            if result.is_none() {
                                let frame = self.call_stack.last_mut().unwrap();
                                let v = spop!(frame);
                                let is_false = match &v.payload {
                                    PyObjectPayload::Bool(b) => !b,
                                    PyObjectPayload::None => true,
                                    PyObjectPayload::Int(PyInt::Small(n)) => *n == 0,
                                    _ => !self.vm_is_truthy(&v)?,
                                };
                                if is_false {
                                    let cs_len = self.call_stack.len();
                                    unsafe { self.call_stack.get_unchecked_mut(cs_len - 1) }.ip = jump_target;
                                }
                            }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                // 4-way superinstruction: LoadFast + LoadConst + CompareOp + PopJumpIfFalse
                // Zero-clone — reads local and constant by reference, no stack ops at all
                Opcode::LoadFastCompareConstJump => {
                    let cmp_op = instr.arg >> 28;
                    let local_idx = ((instr.arg >> 20) & 0xFF) as usize;
                    let const_idx = ((instr.arg >> 12) & 0xFF) as usize;
                    let jump_target = (instr.arg & 0xFFF) as usize;
                    // Read local by reference — no clone
                    match slocal!(frame, local_idx) {
                        Some(local) => {
                            let c = unsafe { frame.constant_cache.get_unchecked(const_idx) };
                            let fast_result = match (&local.payload, &c.payload) {
                                (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                    match cmp_op {
                                        0 => Some(*x < *y),
                                        1 => Some(*x <= *y),
                                        2 => Some(*x == *y),
                                        3 => Some(*x != *y),
                                        4 => Some(*x > *y),
                                        5 => Some(*x >= *y),
                                        _ => None,
                                    }
                                }
                                (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                    match cmp_op {
                                        0 => Some(*x < *y),
                                        1 => Some(*x <= *y),
                                        2 => Some(*x == *y),
                                        3 => Some(*x != *y),
                                        4 => Some(*x > *y),
                                        5 => Some(*x >= *y),
                                        _ => None,
                                    }
                                }
                                _ => None,
                            };
                            if let Some(is_true) = fast_result {
                                if !is_true { frame.ip = jump_target; }
                                hot_ok!(profiling, self.profiler, instr.op)
                            } else {
                                // Fallback: push both, decompose to CompareOp + PopJumpIfFalse
                                spush!(frame, local.clone());
                                spush!(frame, c.clone());
                                let cmp_instr = Instruction::new(Opcode::CompareOp, cmp_op);
                                let result = self.exec_compare_ops(cmp_instr)?;
                                if result.is_none() {
                                    let frame = self.call_stack.last_mut().unwrap();
                                    let v = spop!(frame);
                                    let is_false = match &v.payload {
                                        PyObjectPayload::Bool(b) => !b,
                                        PyObjectPayload::None => true,
                                        PyObjectPayload::Int(PyInt::Small(n)) => *n == 0,
                                        _ => !self.vm_is_truthy(&v)?,
                                    };
                                    if is_false {
                                        let cs_len = self.call_stack.len();
                                        unsafe { self.call_stack.get_unchecked_mut(cs_len - 1) }.ip = jump_target;
                                    }
                                }
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                        }
                        None => Self::err_unbound_local(&frame.code.varnames, local_idx),
                    }
                }
                // 4-way superinstruction: LoadFast + LoadFast + CompareOp + PopJumpIfFalse
                // Zero-clone — reads both locals by reference, no stack ops at all
                Opcode::LoadFastLoadFastCompareJump => {
                    let cmp_op = instr.arg >> 28;
                    let idx1 = ((instr.arg >> 20) & 0xFF) as usize;
                    let idx2 = ((instr.arg >> 12) & 0xFF) as usize;
                    let jump_target = (instr.arg & 0xFFF) as usize;
                    match (slocal!(frame, idx1), slocal!(frame, idx2)) {
                        (Some(a), Some(b)) => {
                            let fast_result = match (&a.payload, &b.payload) {
                                (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                    match cmp_op {
                                        0 => Some(*x < *y),
                                        1 => Some(*x <= *y),
                                        2 => Some(*x == *y),
                                        3 => Some(*x != *y),
                                        4 => Some(*x > *y),
                                        5 => Some(*x >= *y),
                                        _ => None,
                                    }
                                }
                                (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                    match cmp_op {
                                        0 => Some(*x < *y),
                                        1 => Some(*x <= *y),
                                        2 => Some(*x == *y),
                                        3 => Some(*x != *y),
                                        4 => Some(*x > *y),
                                        5 => Some(*x >= *y),
                                        _ => None,
                                    }
                                }
                                (PyObjectPayload::Str(x), PyObjectPayload::Str(y)) => {
                                    match cmp_op {
                                        0 => Some(x < y),
                                        1 => Some(x <= y),
                                        2 => Some(x == y),
                                        3 => Some(x != y),
                                        4 => Some(x > y),
                                        5 => Some(x >= y),
                                        _ => None,
                                    }
                                }
                                _ => None,
                            };
                            if let Some(is_true) = fast_result {
                                if !is_true { frame.ip = jump_target; }
                                hot_ok!(profiling, self.profiler, instr.op)
                            } else {
                                // Fallback: push both, decompose to CompareOp + PopJumpIfFalse
                                spush!(frame, a.clone());
                                spush!(frame, b.clone());
                                let cmp_instr = Instruction::new(Opcode::CompareOp, cmp_op);
                                let result = self.exec_compare_ops(cmp_instr)?;
                                if result.is_none() {
                                    let frame = self.call_stack.last_mut().unwrap();
                                    let v = spop!(frame);
                                    let is_false = match &v.payload {
                                        PyObjectPayload::Bool(b) => !b,
                                        PyObjectPayload::None => true,
                                        PyObjectPayload::Int(PyInt::Small(n)) => *n == 0,
                                        _ => !self.vm_is_truthy(&v)?,
                                    };
                                    if is_false {
                                        let cs_len = self.call_stack.len();
                                        unsafe { self.call_stack.get_unchecked_mut(cs_len - 1) }.ip = jump_target;
                                    }
                                }
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                        }
                        _ => {
                            // One of the locals is unbound
                            if slocal!(frame, idx1).is_none() {
                                Self::err_unbound_local(&frame.code.varnames, idx1)
                            } else {
                                Self::err_unbound_local(&frame.code.varnames, idx2)
                            }
                        }
                    }
                }
                // Fused LoadGlobal + StoreFast: stores global directly to local
                Opcode::LoadGlobalStoreFast => {
                    let name_idx = (instr.arg >> 16) as usize;
                    let store_idx = (instr.arg & 0xFFFF) as usize;
                    let ver = crate::frame::globals_version();
                    if frame.global_cache_version == ver {
                        if let Some(ref cache) = frame.global_cache {
                            if let Some(ref v) = unsafe { cache.get_unchecked(name_idx) } {
                                // Skip clone if destination already holds the same Arc
                                let dest = unsafe { frame.locals.get_unchecked(store_idx) };
                                if let Some(ref existing) = dest {
                                    if PyObjectRef::ptr_eq(existing, v) {
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    }
                                }
                                sset_local!(frame, store_idx, v.clone());
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                        }
                    }
                    // Cache miss: fallback to LoadGlobal + StoreFast
                    let load_instr = Instruction::new(Opcode::LoadGlobal, name_idx as u32);
                    self.execute_one(load_instr)?;
                    let frame = self.call_stack.last_mut().unwrap();
                    let v = spop!(frame);
                    sset_local!(frame, store_idx, v);
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                // Fused PopBlock + Jump: pops exception block and jumps in one dispatch
                Opcode::PopBlockJump => {
                    frame.pop_block();
                    frame.ip = instr.arg as usize;
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                // Fused LoadConst + LoadFast + CompareOp(in/not_in) + StoreFast
                // Zero-Arc: reads constant and local by reference, does containment check,
                // stores bool result to local with in-place mutation.
                Opcode::LoadConstLoadFastContainsStoreFast => {
                    let not_in = (instr.arg >> 31) != 0;
                    let const_idx = ((instr.arg >> 20) & 0x3FF) as usize;
                    let fast_idx = ((instr.arg >> 10) & 0x3FF) as usize;
                    let store_idx = (instr.arg & 0x3FF) as usize;
                    let needle = unsafe { frame.constant_cache.get_unchecked(const_idx) };
                    let haystack_opt = slocal!(frame, fast_idx);
                    if let Some(haystack) = haystack_opt {
                        let found = match (&needle.payload, &haystack.payload) {
                            (PyObjectPayload::Str(s), PyObjectPayload::Dict(map)) => {
                                let r = unsafe { &*map.data_ptr() };
                                Some(r.contains_key(&BorrowedStrKey(s.as_str())))
                            }
                            (PyObjectPayload::Int(PyInt::Small(n)), PyObjectPayload::Dict(map)) => {
                                let r = unsafe { &*map.data_ptr() };
                                Some(r.contains_key(&BorrowedIntKey(*n)))
                            }
                            (PyObjectPayload::Bool(b), PyObjectPayload::Dict(map)) => {
                                let r = unsafe { &*map.data_ptr() };
                                Some(r.contains_key(&BorrowedIntKey(*b as i64)))
                            }
                            (PyObjectPayload::Str(s), PyObjectPayload::Set(items)) => {
                                let r = unsafe { &*items.data_ptr() };
                                Some(r.contains_key(&BorrowedStrKey(s.as_str())))
                            }
                            (PyObjectPayload::Int(PyInt::Small(n)), PyObjectPayload::Set(items)) => {
                                let r = unsafe { &*items.data_ptr() };
                                Some(r.contains_key(&BorrowedIntKey(*n)))
                            }
                            (PyObjectPayload::Str(needle_s), PyObjectPayload::List(items_arc)) => {
                                let items = unsafe { &*items_arc.data_ptr() };
                                Some(items.iter().any(|x| {
                                    if let PyObjectPayload::Str(s) = &x.payload { s == needle_s } else { false }
                                }))
                            }
                            (PyObjectPayload::Int(PyInt::Small(nv)), PyObjectPayload::List(items_arc)) => {
                                let items = unsafe { &*items_arc.data_ptr() };
                                Some(items.iter().any(|x| {
                                    if let PyObjectPayload::Int(PyInt::Small(v)) = &x.payload { v == nv } else { false }
                                }))
                            }
                            (PyObjectPayload::Str(needle_s), PyObjectPayload::Tuple(items)) => {
                                Some(items.iter().any(|x| {
                                    if let PyObjectPayload::Str(s) = &x.payload { s == needle_s } else { false }
                                }))
                            }
                            (PyObjectPayload::Int(PyInt::Small(nv)), PyObjectPayload::Tuple(items)) => {
                                Some(items.iter().any(|x| {
                                    if let PyObjectPayload::Int(PyInt::Small(v)) = &x.payload { v == nv } else { false }
                                }))
                            }
                            (PyObjectPayload::Str(needle_s), PyObjectPayload::Str(haystack_s)) => {
                                Some(haystack_s.contains(needle_s.as_str()))
                            }
                            _ => None,
                        };
                        if let Some(is_in) = found {
                            let result = if not_in { !is_in } else { is_in };
                            // In-place mutation for bool result
                            let dest = unsafe { frame.locals.get_unchecked_mut(store_idx) };
                            if let Some(ref mut arc) = dest {
                                if let Some(obj) = PyObjectRef::get_mut(arc) {
                                    obj.payload = PyObjectPayload::Bool(result);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                            }
                            *dest = Some(PyObject::bool_val(result));
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                    }
                    // Fallback: decompose to individual ops
                    spush!(frame, unsafe { frame.constant_cache.get_unchecked(const_idx) }.clone());
                    if let Some(v) = slocal!(frame, fast_idx) {
                        spush!(frame, v.clone());
                    } else {
                        let _ = spop!(frame);
                        Self::err_unbound_local(&frame.code.varnames, fast_idx)?;
                        unreachable!();
                    }
                    let cmp_arg = if not_in { 7u32 } else { 6u32 };
                    let cmp_instr = Instruction::new(Opcode::CompareOp, cmp_arg);
                    self.execute_one(cmp_instr)?;
                    let frame = self.call_stack.last_mut().unwrap();
                    let v = spop!(frame);
                    sset_local!(frame, store_idx, v);
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                // Fused LoadFast + LoadConst + BinarySubscr + StoreFast
                // Zero-Arc for container/index; clones element with in-place mutation fallback.
                Opcode::LoadFastLoadConstSubscrStoreFast => {
                    let fast_idx = ((instr.arg >> 20) & 0x3FF) as usize;
                    let const_idx = ((instr.arg >> 10) & 0x3FF) as usize;
                    let store_idx = (instr.arg & 0x3FF) as usize;
                    // Use raw pointer to read local without borrowing frame.locals
                    // SAFETY: fast_idx is a valid local index from well-formed bytecode
                    let locals_ptr = frame.locals.as_mut_ptr();
                    let obj_opt = unsafe { &*locals_ptr.add(fast_idx) };
                    let key = unsafe { frame.constant_cache.get_unchecked(const_idx) };
                    if let Some(obj) = obj_opt {
                        match (&obj.payload, &key.payload) {
                            (PyObjectPayload::List(items_arc), PyObjectPayload::Int(PyInt::Small(idx))) => {
                                let items = unsafe { &*items_arc.data_ptr() };
                                let i = *idx;
                                let actual = if i < 0 { i + items.len() as i64 } else { i };
                                if actual >= 0 && (actual as usize) < items.len() {
                                    let elem = &items[actual as usize];
                                    // In-place mutation via raw pointer (bypasses borrow checker)
                                    let dest = unsafe { &mut *locals_ptr.add(store_idx) };
                                    if let Some(ref mut arc) = dest {
                                        if let Some(dest_obj) = PyObjectRef::get_mut(arc) {
                                            match (&elem.payload, &mut dest_obj.payload) {
                                                (PyObjectPayload::Int(src), PyObjectPayload::Int(dst)) => {
                                                    *dst = src.clone();
                                                    hot_ok!(profiling, self.profiler, instr.op)
                                                }
                                                (PyObjectPayload::Float(src), PyObjectPayload::Float(dst)) => {
                                                    *dst = *src;
                                                    hot_ok!(profiling, self.profiler, instr.op)
                                                }
                                                (PyObjectPayload::Bool(src), PyObjectPayload::Bool(dst)) => {
                                                    *dst = *src;
                                                    hot_ok!(profiling, self.profiler, instr.op)
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    *dest = Some(elem.clone());
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                            }
                            (PyObjectPayload::Tuple(items), PyObjectPayload::Int(PyInt::Small(idx))) => {
                                let i = *idx;
                                let actual = if i < 0 { i + items.len() as i64 } else { i };
                                if actual >= 0 && (actual as usize) < items.len() {
                                    let elem = &items[actual as usize];
                                    let dest = unsafe { &mut *locals_ptr.add(store_idx) };
                                    if let Some(ref mut arc) = dest {
                                        if let Some(dest_obj) = PyObjectRef::get_mut(arc) {
                                            match (&elem.payload, &mut dest_obj.payload) {
                                                (PyObjectPayload::Int(src), PyObjectPayload::Int(dst)) => {
                                                    *dst = src.clone();
                                                    hot_ok!(profiling, self.profiler, instr.op)
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    *dest = Some(elem.clone());
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                            }
                            (PyObjectPayload::Dict(map), PyObjectPayload::Str(s)) => {
                                let val = unsafe { &*map.data_ptr() }.get(&BorrowedStrKey(s.as_str())).cloned();
                                if let Some(v) = val {
                                    sset_local!(frame, store_idx, v);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                            }
                            (PyObjectPayload::Dict(map), PyObjectPayload::Int(PyInt::Small(n))) => {
                                let val = unsafe { &*map.data_ptr() }.get(&BorrowedIntKey(*n)).cloned();
                                if let Some(v) = val {
                                    sset_local!(frame, store_idx, v);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                            }
                            _ => {}
                        }
                    }
                    // Fallback: decompose
                    if let Some(v) = slocal!(frame, fast_idx) {
                        spush!(frame, v.clone());
                    } else {
                        Self::err_unbound_local(&frame.code.varnames, fast_idx)?;
                        unreachable!();
                    }
                    spush!(frame, unsafe { frame.constant_cache.get_unchecked(const_idx) }.clone());
                    let subscr_instr = Instruction::new(Opcode::BinarySubscr, 0);
                    self.execute_one(subscr_instr)?;
                    let frame = self.call_stack.last_mut().unwrap();
                    let v = spop!(frame);
                    sset_local!(frame, store_idx, v);
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                // Fused LoadFast + LoadFast + BinarySubscr + StoreFast
                // Zero-Arc for container and key: reads both from locals by reference.
                Opcode::LoadFastLoadFastSubscrStoreFast => {
                    let container_idx = (instr.arg >> 24) as usize;
                    let key_idx = ((instr.arg >> 16) & 0xFF) as usize;
                    let store_idx = ((instr.arg >> 8) & 0xFF) as usize;
                    let locals_ptr = frame.locals.as_mut_ptr();
                    let container = unsafe { &*locals_ptr.add(container_idx) };
                    let key = unsafe { &*locals_ptr.add(key_idx) };
                    if let (Some(ref obj), Some(ref k)) = (container, key) {
                        let result_val = match (&obj.payload, &k.payload) {
                            // dict[int]
                            (PyObjectPayload::Dict(map), PyObjectPayload::Int(PyInt::Small(n))) => {
                                unsafe { &*map.data_ptr() }.get(&BorrowedIntKey(*n)).cloned()
                            }
                            // dict[str]
                            (PyObjectPayload::Dict(map), PyObjectPayload::Str(s)) => {
                                unsafe { &*map.data_ptr() }.get(&BorrowedStrKey(s.as_str())).cloned()
                            }
                            // list[int]
                            (PyObjectPayload::List(items_arc), PyObjectPayload::Int(PyInt::Small(idx))) => {
                                let items = unsafe { &*items_arc.data_ptr() };
                                let i = *idx;
                                let actual = if i < 0 { i + items.len() as i64 } else { i };
                                if actual >= 0 && (actual as usize) < items.len() {
                                    Some(items[actual as usize].clone())
                                } else { None }
                            }
                            // tuple[int]
                            (PyObjectPayload::Tuple(items), PyObjectPayload::Int(PyInt::Small(idx))) => {
                                let i = *idx;
                                let actual = if i < 0 { i + items.len() as i64 } else { i };
                                if actual >= 0 && (actual as usize) < items.len() {
                                    Some(items[actual as usize].clone())
                                } else { None }
                            }
                            _ => None,
                        };
                        if let Some(val) = result_val {
                            // In-place mutation if dest has refcount 1
                            let dest_slot = unsafe { &mut *locals_ptr.add(store_idx) };
                            *dest_slot = Some(val);
                            hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                        }
                    }
                    // Fallback: decompose to individual ops
                    if let Some(ref v) = unsafe { &*frame.locals.as_ptr().add(container_idx) } {
                        spush!(frame, v.clone());
                    } else {
                        Self::err_unbound_local(&frame.code.varnames, container_idx)?;
                        unreachable!();
                    }
                    if let Some(ref v) = unsafe { &*frame.locals.as_ptr().add(key_idx) } {
                        spush!(frame, v.clone());
                    } else {
                        Self::err_unbound_local(&frame.code.varnames, key_idx)?;
                        unreachable!();
                    }
                    let subscr_instr = Instruction::new(Opcode::BinarySubscr, 0);
                    self.execute_one(subscr_instr)?;
                    let frame = self.call_stack.last_mut().unwrap();
                    let v = spop!(frame);
                    sset_local!(frame, store_idx, v);
                    hot_ok!(profiling, self.profiler, instr.op)
                }
                // Fused LoadFast + LoadFast + LoadFast + StoreSubscr
                // Zero-Arc: reads value, container, key from locals by reference.
                Opcode::LoadFastLoadFastLoadFastStoreSubscr => {
                    let val_idx = (instr.arg >> 24) as usize;
                    let container_idx = ((instr.arg >> 16) & 0xFF) as usize;
                    let key_idx = ((instr.arg >> 8) & 0xFF) as usize;
                    let locals_ptr = frame.locals.as_ptr();
                    let val_opt = unsafe { &*locals_ptr.add(val_idx) };
                    let container_opt = unsafe { &*locals_ptr.add(container_idx) };
                    let key_opt = unsafe { &*locals_ptr.add(key_idx) };
                    if let (Some(ref val), Some(ref obj), Some(ref k)) = (val_opt, container_opt, key_opt) {
                        let done = match (&obj.payload, &k.payload) {
                            // dict[int] = val
                            (PyObjectPayload::Dict(map), PyObjectPayload::Int(PyInt::Small(n))) => {
                                let hk = HashableKey::Int(PyInt::Small(*n));
                                unsafe { &mut *map.data_ptr() }.insert(hk, val.clone());
                                true
                            }
                            // dict[str] = val
                            (PyObjectPayload::Dict(map), PyObjectPayload::Str(s)) => {
                                let hk = HashableKey::str_key(s.clone());
                                unsafe { &mut *map.data_ptr() }.insert(hk, val.clone());
                                true
                            }
                            // dict[bool] = val
                            (PyObjectPayload::Dict(map), PyObjectPayload::Bool(b)) => {
                                let hk = HashableKey::Int(PyInt::Small(*b as i64));
                                unsafe { &mut *map.data_ptr() }.insert(hk, val.clone());
                                true
                            }
                            // list[int] = val
                            (PyObjectPayload::List(items_arc), PyObjectPayload::Int(PyInt::Small(idx))) => {
                                let items = unsafe { &mut *items_arc.data_ptr() };
                                let i = *idx;
                                let actual = if i < 0 { i + items.len() as i64 } else { i };
                                if actual >= 0 && (actual as usize) < items.len() {
                                    items[actual as usize] = val.clone();
                                    true
                                } else { false }
                            }
                            _ => false,
                        };
                        if done {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                    }
                    // Fallback: push all 3 locals and execute StoreSubscr
                    for idx in [val_idx, container_idx, key_idx] {
                        if let Some(ref v) = unsafe { &*frame.locals.as_ptr().add(idx) } {
                            spush!(frame, v.clone());
                        } else {
                            Self::err_unbound_local(&frame.code.varnames, idx)?;
                            unreachable!();
                        }
                    }
                    self.execute_one(Instruction::new(Opcode::StoreSubscr, 0))
                }
                Opcode::LoadFastLoadFastContainsStoreFast => {
                    let needle_idx = (instr.arg >> 24) as usize;
                    let haystack_idx = ((instr.arg >> 16) & 0xFF) as usize;
                    let store_idx = ((instr.arg >> 8) & 0xFF) as usize;
                    let negate = (instr.arg & 1) != 0; // 1 = not_in
                    let locals_ptr = frame.locals.as_ptr();
                    let needle_opt = unsafe { &*locals_ptr.add(needle_idx) };
                    let haystack_opt = unsafe { &*locals_ptr.add(haystack_idx) };
                    if let (Some(needle), Some(haystack)) = (needle_opt, haystack_opt) {
                        let found = match &haystack.payload {
                            PyObjectPayload::Dict(map) => {
                                let r = unsafe { &*map.data_ptr() };
                                match &needle.payload {
                                    PyObjectPayload::Int(PyInt::Small(n)) => Some(r.contains_key(&BorrowedIntKey(*n))),
                                    PyObjectPayload::Str(s) => Some(r.contains_key(&BorrowedStrKey(s.as_str()))),
                                    PyObjectPayload::Bool(b) => Some(r.contains_key(&BorrowedIntKey(*b as i64))),
                                    _ => None,
                                }
                            }
                            PyObjectPayload::Set(items) => {
                                let r = unsafe { &*items.data_ptr() };
                                match &needle.payload {
                                    PyObjectPayload::Int(PyInt::Small(n)) => Some(r.contains_key(&BorrowedIntKey(*n))),
                                    PyObjectPayload::Str(s) => Some(r.contains_key(&BorrowedStrKey(s.as_str()))),
                                    PyObjectPayload::Bool(b) => Some(r.contains_key(&BorrowedIntKey(*b as i64))),
                                    _ => None,
                                }
                            }
                            PyObjectPayload::List(items) => {
                                let items = unsafe { &*items.data_ptr() };
                                Some(items.iter().any(|x| {
                                    match (&x.payload, &needle.payload) {
                                        (PyObjectPayload::Int(PyInt::Small(a)), PyObjectPayload::Int(PyInt::Small(b))) => a == b,
                                        (PyObjectPayload::Str(a), PyObjectPayload::Str(b)) => a == b,
                                        _ => PyObjectRef::ptr_eq(x, needle),
                                    }
                                }))
                            }
                            PyObjectPayload::Tuple(items) => {
                                Some(items.iter().any(|x| {
                                    match (&x.payload, &needle.payload) {
                                        (PyObjectPayload::Int(PyInt::Small(a)), PyObjectPayload::Int(PyInt::Small(b))) => a == b,
                                        (PyObjectPayload::Str(a), PyObjectPayload::Str(b)) => a == b,
                                        _ => PyObjectRef::ptr_eq(x, needle),
                                    }
                                }))
                            }
                            PyObjectPayload::Str(haystack_s) => {
                                if let PyObjectPayload::Str(needle_s) = &needle.payload {
                                    Some(haystack_s.contains(needle_s.as_str()))
                                } else { None }
                            }
                            _ => None,
                        };
                        if let Some(is_in) = found {
                            let result = if negate { !is_in } else { is_in };
                            // In-place mutation: if dest already holds a bool, overwrite payload
                            let dest_slot = unsafe { &mut *frame.locals.as_mut_ptr().add(store_idx) };
                            if let Some(ref mut arc) = dest_slot {
                                if let Some(obj) = PyObjectRef::get_mut(arc) {
                                    obj.payload = PyObjectPayload::Bool(result);
                                    hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                                }
                            }
                            *dest_slot = Some(PyObject::bool_val(result));
                            hot_ok_chain!(profiling, self.profiler, instr.op, frame, instr_base, instr_count)
                        }
                    }
                    // Fallback: decompose to individual ops
                    for idx in [needle_idx, haystack_idx] {
                        if let Some(ref v) = unsafe { &*frame.locals.as_ptr().add(idx) } {
                            spush!(frame, v.clone());
                        } else {
                            Self::err_unbound_local(&frame.code.varnames, idx)?;
                            unreachable!();
                        }
                    }
                    let cmp_arg = if negate { 7u32 } else { 6u32 };
                    let r = self.execute_one(Instruction::new(Opcode::CompareOp, cmp_arg));
                    if r.is_ok() {
                        let cs_len2 = self.call_stack.len();
                        let frame2 = unsafe { self.call_stack.get_unchecked_mut(cs_len2 - 1) };
                        if !frame2.stack.is_empty() {
                            let val = frame2.stack.pop().unwrap();
                            unsafe { frame2.set_local_unchecked(store_idx, val) };
                        }
                    }
                    r
                }
                _ => self.execute_one(instr),
            };

            match result {
                Ok(Some(ret)) => {
                    if profiling { self.profiler.end_instruction(instr.op); }
                    // Iterative call/return: if we're deeper than initial_depth,
                    // we're returning from a child frame pushed by inline
                    // CallFunction/CallMethod — pop it and push result to parent.
                    if self.call_stack.len() > initial_depth {
                        if has_trace {
                            self.fire_trace_event("return", ret.clone());
                        }
                        if has_profile {
                            self.fire_profile_event("return", ret.clone());
                        }
                        // SAFETY: call_stack.len() > initial_depth >= 1, so non-empty
                        let child = unsafe {
                            let new_len = self.call_stack.len() - 1;
                            let child = std::ptr::read(self.call_stack.as_ptr().add(new_len));
                            self.call_stack.set_len(new_len);
                            child
                        };
                        let discard = child.discard_return;
                        child.recycle(&mut self.frame_pool);
                        // SAFETY: we verified len > initial_depth >= 1 and popped one
                        let cs_len = self.call_stack.len();
                        let parent = unsafe { self.call_stack.get_unchecked_mut(cs_len - 1) };
                        // Check if the calling instruction was CallMethodPopTop — if so,
                        // discard the return value instead of pushing it to the stack.
                        // Also discard if child was an __init__ frame from inline class instantiation.
                        let caller_op = parent.code.instructions.get(parent.ip.wrapping_sub(1))
                            .map(|i| i.op);
                        if discard || caller_op == Some(Opcode::CallMethodPopTop) {
                            drop(ret);
                        } else {
                            parent.stack.push(ret);
                        }
                        // Re-derive frame_ptr: child frame was popped
                        rederive_frame!(self, frame_ptr, instr_base, instr_count);
                        continue;
                    }
                    // Returning from the initial frame we were called to execute
                    if has_trace {
                        self.fire_trace_event("return", ret.clone());
                    }
                    if has_profile {
                        self.fire_profile_event("return", ret.clone());
                    }
                    return Ok(ret);
                }
                Ok(None) => {
                    if profiling { self.profiler.end_instruction(instr.op); }
                    // Re-derive frame_ptr: execute_one may have modified call_stack
                    rederive_frame!(self, frame_ptr, instr_base, instr_count);
                }
                Err(mut exc) => {
                    // Fire "exception" trace event (cold — only when tracing)
                    if has_trace {
                        let exc_info = PyObject::tuple(vec![
                            PyObject::exception_type(exc.kind),
                            PyObject::str_val(exc.message.clone()),
                            PyObject::none(),
                        ]);
                        self.fire_trace_event("exception", exc_info);
                    }
                    // Implicit chaining: link to active exception (only when present)
                    if exc.context.is_none() {
                        if let Some(ref active) = self.active_exception {
                            exc.context = Some(Box::new(PyException::new(
                                active.kind,
                                active.message.clone(),
                            )));
                        }
                    }
                    // Iterative exception unwind: try current frame, then parents
                    loop {
                        if let Some(handler_ip) = self.unwind_except() {
                            // Extract exc_value and exc_type, reusing original when available
                            let exc_kind = exc.kind;
                            let (exc_value, exc_type) = if let Some(orig) = &exc.original {
                                let cls = if let PyObjectPayload::Instance(inst) = &orig.payload {
                                    inst.class.clone()
                                } else {
                                    PyObject::exception_type(exc_kind)
                                };
                                (orig.clone(), cls)
                            } else {
                                let inst = if let Some(val) = &exc.value {
                                    PyObject::exception_instance_with_args(exc_kind, exc.message.clone(), vec![val.clone()])
                                } else {
                                    PyObject::exception_instance(exc_kind, exc.message.clone())
                                };
                                (inst, PyObject::exception_type(exc_kind))
                            };
                            // Only store attributes that have non-default values
                            // (avoids 4+ hash-map inserts for the common raise/catch case)
                            if let Some(val) = &exc.value {
                                Self::store_exc_attr(&exc_value, "value", val.clone());
                            }
                            if let Some(info) = &exc.os_error_info {
                                Self::store_exc_attr(&exc_value, "errno", PyObject::int(info.errno as i64));
                                Self::store_exc_attr(&exc_value, "strerror", PyObject::str_val(CompactString::from(info.strerror.as_str())));
                                if let Some(fname) = &info.filename {
                                    Self::store_exc_attr(&exc_value, "filename", PyObject::str_val(CompactString::from(fname.as_str())));
                                } else {
                                    Self::store_exc_attr(&exc_value, "filename", PyObject::none());
                                }
                            }
                            if let Some(cause) = &exc.cause {
                                let cause_obj = if let Some(corig) = &cause.original {
                                    corig.clone()
                                } else {
                                    PyObject::exception_instance(cause.kind, cause.message.clone())
                                };
                                Self::store_exc_attr(&exc_value, "__cause__", cause_obj);
                                Self::store_exc_attr(&exc_value, "__suppress_context__", PyObject::bool_val(true));
                            }
                            if let Some(ctx) = &exc.context {
                                let ctx_obj = if let Some(corig) = &ctx.original {
                                    corig.clone()
                                } else {
                                    PyObject::exception_instance(ctx.kind, ctx.message.clone())
                                };
                                Self::store_exc_attr(&exc_value, "__context__", ctx_obj);
                            }
                            // sys.exc_info() reads lazily through active_exception pointer
                            // (registered at run_frame start) — no TLS write needed here
                            let frame = self.call_stack.last_mut().unwrap();
                            frame.push(PyObject::none());         // traceback (lazy)
                            frame.push(exc_value);            // value
                            frame.push(exc_type);             // type
                            frame.ip = handler_ip;
                            // Move exc into active_exception (avoids clone)
                            // Also set thread-local exc info for traceback.format_exc()
                            ferrython_core::error::set_thread_exc_info(
                                exc.kind,
                                exc.message.clone(),
                                exc.traceback.clone(),
                            );
                            self.active_exception = Some(exc);
                            // Re-derive frame_ptr: exception unwind may have popped frames
                            rederive_frame!(self, frame_ptr, instr_base, instr_count);
                            break; // handler found, continue main loop
                        }
                        // No handler in current frame — unwind iteratively
                        if self.call_stack.len() > initial_depth {
                            if let Some(child) = self.call_stack.pop() {
                                child.recycle(&mut self.frame_pool);
                            }
                            continue; // try parent frame's block stack
                        }
                        // Exception escapes — attach traceback now
                        if exc.traceback.is_empty() {
                            self.attach_traceback(&mut exc);
                        }
                        return Err(exc);
                    }
                }
            }
        }
    }

    /// Attach traceback entries from the current call stack to an exception.
    fn attach_traceback(&self, exc: &mut PyException) {
        use ferrython_core::error::TracebackEntry;
        for frame in &self.call_stack {
            let lineno = ferrython_debug::resolve_lineno(
                &frame.code,
                frame.ip.saturating_sub(1),
            );
            exc.traceback.push(TracebackEntry {
                filename: frame.code.filename.to_string(),
                function: frame.code.name.to_string(),
                lineno,
            });
        }
    }

    /// Build a Python-level traceback object chain (CPython-compatible).
    /// The returned object is the outermost frame, with tb_next pointing towards
    /// the innermost frame (matching CPython's `sys.exc_info()[2]` chain order).
    fn build_traceback_object(entries: &[ferrython_core::error::TracebackEntry]) -> PyObjectRef {
        if entries.is_empty() {
            return PyObject::none();
        }
        // entries are ordered [outermost, ..., innermost].
        // CPython chain: outermost -> ... -> innermost -> None
        // Build from innermost to outermost so tb_next links are correct.
        let tb_class = PyObject::builtin_type(CompactString::from("traceback"));
        let frame_class = PyObject::builtin_type(CompactString::from("frame"));
        let mut tb_next = PyObject::none();
        for entry in entries.iter().rev() {
            // Build a minimal frame-like object for tb_frame
            let mut frame_attrs = IndexMap::new();
            frame_attrs.insert(CompactString::from("f_lineno"), PyObject::int(entry.lineno as i64));
            let mut code_attrs = IndexMap::new();
            code_attrs.insert(CompactString::from("co_filename"),
                PyObject::str_val(CompactString::from(&entry.filename)));
            code_attrs.insert(CompactString::from("co_name"),
                PyObject::str_val(CompactString::from(&entry.function)));
            let code_class = PyObject::builtin_type(CompactString::from("code"));
            let code_obj = PyObject::instance_with_attrs(code_class, code_attrs);
            frame_attrs.insert(CompactString::from("f_code"), code_obj);
            frame_attrs.insert(CompactString::from("f_locals"), PyObject::dict(new_fx_hashkey_map()));
            frame_attrs.insert(CompactString::from("f_globals"), PyObject::dict(new_fx_hashkey_map()));
            let frame_obj = PyObject::instance_with_attrs(frame_class.clone(), frame_attrs);

            let mut attrs = IndexMap::new();
            attrs.insert(CompactString::from("tb_lineno"), PyObject::int(entry.lineno as i64));
            attrs.insert(CompactString::from("tb_frame"), frame_obj);
            attrs.insert(CompactString::from("tb_next"), tb_next);
            attrs.insert(CompactString::from("tb_filename"),
                PyObject::str_val(CompactString::from(&entry.filename)));
            attrs.insert(CompactString::from("tb_name"),
                PyObject::str_val(CompactString::from(&entry.function)));
            tb_next = PyObject::instance_with_attrs(tb_class.clone(), attrs);
        }
        tb_next
    }

    /// Store an attribute on an exception value object (works for both Instance and ExceptionInstance).
    fn store_exc_attr(exc_value: &PyObjectRef, name: &str, value: PyObjectRef) {
        match &exc_value.payload {
            PyObjectPayload::Instance(inst) => {
                unsafe { &mut *inst.attrs.data_ptr() }.insert(CompactString::from(name), value);
            }
            PyObjectPayload::ExceptionInstance(ei) => {
                ei.ensure_attrs().write().insert(CompactString::from(name), value);
            }
            _ => {}
        }
    }

    /// Handle a breakpoint hit — print location info and current stack state.
    pub(crate) fn handle_breakpoint_hit(&self) {
        if let Some(frame) = self.call_stack.last() {
            let lineno = ferrython_debug::resolve_lineno(
                &frame.code,
                frame.ip.saturating_sub(1),
            );
            eprintln!(
                "*** Breakpoint hit: File \"{}\", line {}, in {} ***",
                frame.code.filename, lineno, frame.code.name
            );
            // Print local variables if in a function scope
            if frame.scope_kind == crate::frame::ScopeKind::Function {
                let mut locals_info = Vec::new();
                for (i, name) in frame.code.varnames.iter().enumerate() {
                    if let Some(val) = frame.locals.get(i).and_then(|v| v.as_ref()) {
                        locals_info.push(format!("  {} = {}", name, val.py_to_string()));
                    }
                }
                if !locals_info.is_empty() {
                    eprintln!("  Locals:");
                    for info in &locals_info {
                        eprintln!("{}", info);
                    }
                }
            }
        }
    }

    /// Advance a source iterator inline without VM dispatch.
    /// Works for List, Tuple, Range, and RangeIter sources.
    /// Returns `Some(Some(value))` if advanced, `Some(None)` if exhausted,
    /// `None` if the source type requires VM dispatch.
    #[inline(always)]
    fn advance_source_inline(source: &PyObjectRef) -> Option<Option<PyObjectRef>> {
        match &source.payload {
            PyObjectPayload::Iterator(arc) => {
                let mut data = arc.write();
                match &mut *data {
                    IteratorData::List { items, index } => {
                        if *index < items.len() {
                            let v = items[*index].clone();
                            *index += 1;
                            Some(Some(v))
                        } else {
                            Some(None)
                        }
                    }
                    IteratorData::Tuple { items, index } => {
                        if *index < items.len() {
                            let v = items[*index].clone();
                            *index += 1;
                            Some(Some(v))
                        } else {
                            Some(None)
                        }
                    }
                    IteratorData::Range { current, stop, step } => {
                        let done = if *step > 0 { *current >= *stop } else { *current <= *stop };
                        if done {
                            Some(None)
                        } else {
                            let v = PyObject::int(*current);
                            *current += *step;
                            Some(Some(v))
                        }
                    }
                    _ => None,
                }
            }
            PyObjectPayload::RangeIter { current, stop, step } => {
                let cur = current.get();
                let done = if *step > 0 { cur >= *stop } else { cur <= *stop };
                if done {
                    Some(None)
                } else {
                    current.set(cur + *step);
                    Some(Some(PyObject::int(cur)))
                }
            }
            PyObjectPayload::VecIter(data) => {
                let idx = data.index.get();
                if idx < data.items.len() {
                    let v = data.items[idx].clone();
                    data.index.set(idx + 1);
                    Some(Some(v))
                } else {
                    Some(None)
                }
            }
            PyObjectPayload::RefIter { source, index } => {
                let idx = index.get();
                match &source.payload {
                    PyObjectPayload::List(cell) => {
                        let items = unsafe { &*cell.data_ptr() };
                        if idx < items.len() {
                            let v = items[idx].clone();
                            index.set(idx + 1);
                            Some(Some(v))
                        } else {
                            Some(None)
                        }
                    }
                    PyObjectPayload::Tuple(items) => {
                        if idx < items.len() {
                            let v = items[idx].clone();
                            index.set(idx + 1);
                            Some(Some(v))
                        } else {
                            Some(None)
                        }
                    }
                    PyObjectPayload::Dict(cell) | PyObjectPayload::MappingProxy(cell) | PyObjectPayload::DictKeys(cell) => {
                        let map = unsafe { &*cell.data_ptr() };
                        if idx < map.len() {
                            let v = map.get_index(idx).unwrap().0.to_object();
                            index.set(idx + 1);
                            Some(Some(v))
                        } else {
                            Some(None)
                        }
                    }
                    PyObjectPayload::DictValues(cell) => {
                        let map = unsafe { &*cell.data_ptr() };
                        if idx < map.len() {
                            let v = map.get_index(idx).unwrap().1.clone();
                            index.set(idx + 1);
                            Some(Some(v))
                        } else {
                            Some(None)
                        }
                    }
                    PyObjectPayload::DictItems(cell) => {
                        let map = unsafe { &*cell.data_ptr() };
                        if idx < map.len() {
                            let (k, v) = map.get_index(idx).unwrap();
                            let tuple = PyObject::tuple(vec![k.to_object(), v.clone()]);
                            index.set(idx + 1);
                            Some(Some(tuple))
                        } else {
                            Some(None)
                        }
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Find an exception handler on the block stack. Returns handler IP if found.
    pub(crate) fn unwind_except(&mut self) -> Option<usize> {
        let frame = self.call_stack.last_mut()?;
        while let Some(block) = frame.pop_block() {
            match block.kind() {
                BlockKind::Except | BlockKind::Finally => {
                    // Unwind value stack to block level
                    while frame.stack.len() > block.stack_level() {
                        frame.pop();
                    }
                    // Push an ExceptHandler block so PopExcept can find it
                    frame.push_block(BlockKind::ExceptHandler, 0);
                    return Some(block.handler());
                }
                BlockKind::ExceptHandler => {
                    // Clean up a previous except handler (exception in except body)
                    while frame.stack.len() > block.stack_level() {
                        frame.pop();
                    }
                    continue;
                }
                BlockKind::Loop => {
                    while frame.stack.len() > block.stack_level() {
                        frame.pop();
                    }
                    continue;
                }
                BlockKind::With => {
                    // With block exception — jump to cleanup handler which will
                    // call __exit__ with exception info
                    while frame.stack.len() > block.stack_level() {
                        frame.pop();
                    }
                    return Some(block.handler());
                }
            }
        }
        None
    }

    #[cold]
    #[inline(never)]
    fn execute_one(&mut self, instr: ferrython_bytecode::Instruction) -> Result<Option<PyObjectRef>, PyException> {
        use ferrython_bytecode::opcode::Opcode;
        match instr.op {
            Opcode::Nop | Opcode::PopTop | Opcode::PopTopJumpAbsolute
            | Opcode::RotTwo | Opcode::RotThree
            | Opcode::RotFour | Opcode::DupTop | Opcode::DupTopTwo | Opcode::LoadConst
                => self.exec_stack_ops(instr),

            Opcode::LoadName | Opcode::StoreName | Opcode::DeleteName
            | Opcode::LoadFast | Opcode::StoreFast | Opcode::DeleteFast
            | Opcode::LoadDeref | Opcode::StoreDeref | Opcode::DeleteDeref
            | Opcode::LoadClosure | Opcode::LoadClassderef
            | Opcode::LoadGlobal | Opcode::StoreGlobal | Opcode::DeleteGlobal
            | Opcode::LoadFastLoadFast | Opcode::LoadFastLoadConst
            | Opcode::StoreFastLoadFast | Opcode::StoreFastJumpAbsolute
            | Opcode::LoadConstStoreFast | Opcode::LoadGlobalStoreFast
            | Opcode::LoadConstLoadFastContainsStoreFast
            | Opcode::LoadFastLoadConstSubscrStoreFast
            | Opcode::LoadFastLoadFastSubscrStoreFast
            | Opcode::LoadFastLoadFastLoadFastStoreSubscr
            | Opcode::LoadFastLoadFastContainsStoreFast
                => self.exec_name_ops(instr),

            Opcode::LoadAttr | Opcode::StoreAttr | Opcode::DeleteAttr
                => self.exec_attr_ops(instr),

            Opcode::UnaryPositive | Opcode::UnaryNegative
            | Opcode::UnaryNot | Opcode::UnaryInvert
                => self.exec_unary_ops(instr),

            Opcode::BinaryAdd | Opcode::InplaceAdd
            | Opcode::BinarySubtract | Opcode::InplaceSubtract
            | Opcode::BinaryMultiply | Opcode::InplaceMultiply
            | Opcode::BinaryTrueDivide | Opcode::InplaceTrueDivide
            | Opcode::BinaryFloorDivide | Opcode::InplaceFloorDivide
            | Opcode::BinaryModulo | Opcode::InplaceModulo
            | Opcode::BinaryPower | Opcode::InplacePower
            | Opcode::BinaryLshift | Opcode::InplaceLshift
            | Opcode::BinaryRshift | Opcode::InplaceRshift
            | Opcode::BinaryAnd | Opcode::InplaceAnd
            | Opcode::BinaryOr | Opcode::InplaceOr
            | Opcode::BinaryXor | Opcode::InplaceXor
            | Opcode::BinaryMatrixMultiply | Opcode::InplaceMatrixMultiply
            | Opcode::LoadFastLoadConstBinarySub
            | Opcode::LoadFastLoadConstBinaryAdd
            | Opcode::LoadFastLoadFastBinaryAdd
            | Opcode::LoadFastLoadFastBinaryAddStoreFast
            | Opcode::LoadFastLoadConstBinaryAddStoreFast
                => self.exec_binary_ops(instr),

            Opcode::BinarySubscr | Opcode::StoreSubscr | Opcode::DeleteSubscr
                => self.exec_subscript_ops(instr),

            Opcode::CompareOp | Opcode::CompareOpPopJumpIfFalse
            | Opcode::LoadFastCompareConstJump
            | Opcode::LoadFastLoadFastCompareJump => self.exec_compare_ops(instr),

            Opcode::JumpForward | Opcode::JumpAbsolute
            | Opcode::PopJumpIfFalse | Opcode::PopJumpIfTrue
            | Opcode::JumpIfTrueOrPop | Opcode::JumpIfFalseOrPop
            | Opcode::GetIter | Opcode::GetYieldFromIter | Opcode::ForIter
            | Opcode::ForIterStoreFast | Opcode::EndForLoop
            | Opcode::PopBlockJump
                => self.exec_jump_ops(instr),

            Opcode::BuildTuple | Opcode::BuildList | Opcode::BuildSet
            | Opcode::BuildMap | Opcode::BuildConstKeyMap | Opcode::BuildString
            | Opcode::ListAppend | Opcode::SetAdd | Opcode::MapAdd
            | Opcode::DictUpdate | Opcode::DictMerge | Opcode::ListExtend
            | Opcode::SetUpdate | Opcode::ListToTuple | Opcode::BuildSlice
            | Opcode::UnpackSequence | Opcode::UnpackEx
                => self.exec_build_ops(instr),

            Opcode::CallFunction | Opcode::CallFunctionKw | Opcode::CallMethod
            | Opcode::CallMethodPopTop
            | Opcode::CallFunctionEx | Opcode::LoadMethod | Opcode::MakeFunction
            | Opcode::LoadGlobalCallFunction | Opcode::LoadFastLoadAttr
            | Opcode::LoadFastLoadMethod
                => self.exec_call_ops(instr),

            Opcode::ReturnValue | Opcode::LoadFastReturnValue
            | Opcode::LoadConstReturnValue | Opcode::ImportName | Opcode::ImportFrom
            | Opcode::ImportStar
                => self.exec_return_import(instr),

            Opcode::SetupFinally | Opcode::SetupExcept | Opcode::PopBlock
            | Opcode::PopExcept | Opcode::EndFinally | Opcode::BeginFinally
            | Opcode::RaiseVarargs | Opcode::SetupWith | Opcode::SetupAsyncWith
            | Opcode::WithCleanupStart | Opcode::WithCleanupFinish
                => self.exec_exception_ops(instr),

            Opcode::PrintExpr | Opcode::LoadBuildClass | Opcode::SetupAnnotations
            | Opcode::FormatValue | Opcode::ExtendedArg
            | Opcode::YieldValue | Opcode::YieldFrom
            | Opcode::GetAwaitable | Opcode::GetAiter | Opcode::GetAnext
            | Opcode::BeforeAsyncWith | Opcode::EndAsyncFor
                => self.exec_misc_ops(instr),

            #[allow(unreachable_patterns)]
            _ => Err(PyException::runtime_error(format!(
                "unimplemented opcode: {:?}", instr.op
            ))),
        }
    }

    /// Build a minimal frame object for trace/profile callbacks.
    fn make_trace_frame(&self) -> PyObjectRef {
        self.make_trace_frame_at(self.call_stack.len() - 1, 0)
    }

    fn make_trace_frame_at(&self, depth: usize, recurse_depth: usize) -> PyObjectRef {
        let frame = &self.call_stack[depth];
        let ip = if frame.ip > 0 { frame.ip - 1 } else { 0 };
        let lineno = Self::ip_to_line(&frame.code, ip);
        let mut attrs = IndexMap::new();

        // f_code: code object with co_filename, co_name, co_firstlineno, co_varnames, co_argcount
        attrs.insert(CompactString::from("f_code"), {
            let mut code_attrs = IndexMap::new();
            code_attrs.insert(CompactString::from("co_filename"),
                PyObject::str_val(frame.code.filename.clone()));
            code_attrs.insert(CompactString::from("co_name"),
                PyObject::str_val(frame.code.name.clone()));
            code_attrs.insert(CompactString::from("co_firstlineno"),
                PyObject::int(frame.code.first_line_number as i64));
            code_attrs.insert(CompactString::from("co_argcount"),
                PyObject::int(frame.code.arg_count as i64));
            let varnames: Vec<PyObjectRef> = frame.code.varnames.iter()
                .map(|n| PyObject::str_val(n.clone()))
                .collect();
            code_attrs.insert(CompactString::from("co_varnames"),
                PyObject::tuple(varnames));
            PyObject::module_with_attrs(CompactString::from("code"), code_attrs)
        });

        attrs.insert(CompactString::from("f_lineno"), PyObject::int(lineno as i64));
        attrs.insert(CompactString::from("f_lasti"), PyObject::int(ip as i64));

        // f_locals: real local variables from the frame
        let mut local_pairs = Vec::new();
        for (i, name) in frame.code.varnames.iter().enumerate() {
            if let Some(Some(val)) = frame.locals.get(i) {
                local_pairs.push((
                    PyObject::str_val(name.clone()),
                    val.clone(),
                ));
            }
        }
        for (name, val) in frame.local_names_iter() {
            local_pairs.push((
                PyObject::str_val(name.clone()),
                val.clone(),
            ));
        }
        attrs.insert(CompactString::from("f_locals"), PyObject::dict_from_pairs(local_pairs));

        // f_globals: snapshot of globals dict
        let global_pairs: Vec<(PyObjectRef, PyObjectRef)> = frame.globals.read().iter()
            .map(|(k, v)| (PyObject::str_val(k.clone()), v.clone()))
            .collect();
        attrs.insert(CompactString::from("f_globals"), PyObject::dict_from_pairs(global_pairs));

        // f_back: parent frame (limit recursion to 10 levels to avoid stack overflow)
        let f_back = if depth > 0 && recurse_depth < 10 {
            self.make_trace_frame_at(depth - 1, recurse_depth + 1)
        } else {
            PyObject::none()
        };
        attrs.insert(CompactString::from("f_back"), f_back);

        PyObject::module_with_attrs(CompactString::from("frame"), attrs)
    }

    /// Resolve instruction pointer to source line number.
    fn ip_to_line(code: &CodeObject, ip: usize) -> u32 {
        let mut line = code.first_line_number;
        for &(offset, ln) in &code.line_number_table {
            if offset as usize > ip { break; }
            line = ln;
        }
        line
    }

    /// Fire a trace event to the registered sys.settrace function.
    /// Events: "call", "line", "return", "exception"
    fn fire_trace_event(&mut self, event: &str, arg: PyObjectRef) {
        let trace_fn = match ferrython_stdlib::get_trace_func() {
            Some(f) => f,
            None => return,
        };
        let frame_obj = self.make_trace_frame();
        let event_str = PyObject::str_val(CompactString::from(event));
        // Call trace_fn(frame, event, arg) — ignore errors to avoid infinite recursion
        // Temporarily disable trace during callback to prevent re-entrant calls
        ferrython_stdlib::set_trace_func(None);
        let result = self.call_object(trace_fn.clone(), vec![frame_obj, event_str, arg]);
        // If the trace function returns None, tracing is disabled for this scope
        // Otherwise, re-install (could be a different local trace function)
        match result {
            Ok(ref val) if !matches!(&val.payload, PyObjectPayload::None) => {
                ferrython_stdlib::set_trace_func(Some(trace_fn));
            }
            Ok(_) => {
                // Returned None — re-install the global trace function
                ferrython_stdlib::set_trace_func(Some(trace_fn));
            }
            Err(_) => {
                // Error in trace function — disable tracing (CPython behavior)
            }
        }
    }

    /// Fire a profile event to the registered sys.setprofile function.
    /// Events: "call", "return", "c_call", "c_return", "c_exception"
    fn fire_profile_event(&mut self, event: &str, arg: PyObjectRef) {
        let profile_fn = match ferrython_stdlib::get_profile_func() {
            Some(f) => f,
            None => return,
        };
        let frame_obj = self.make_trace_frame();
        let event_str = PyObject::str_val(CompactString::from(event));
        ferrython_stdlib::set_profile_func(None);
        let _ = self.call_object(profile_fn.clone(), vec![frame_obj, event_str, arg]);
        ferrython_stdlib::set_profile_func(Some(profile_fn));
    }

    /// Invoke sys.excepthook if set. Returns true if the hook was called successfully.
    pub fn invoke_excepthook(&mut self, exc: &PyException) -> bool {
        // Look up excepthook from the sys module (user may have reassigned it)
        let hook = if let Some(sys_mod) = self.modules.get("sys") {
            if let Some(h) = sys_mod.get_attr("excepthook") {
                // Check if it's the default (a BuiltinFunction named "sys_excepthook_default")
                // If default, fall through to normal traceback display
                if let PyObjectPayload::BuiltinFunction(name) = &h.payload {
                    if name.contains("excepthook") { return false; }
                }
                Some(h)
            } else {
                None
            }
        } else {
            None
        };
        let hook = match hook {
            Some(h) => h,
            None => return false,
        };
        let exc_type = PyObject::exception_type(exc.kind);
        let exc_value = PyObject::str_val(exc.message.clone());
        let exc_tb = PyObject::none();
        self.call_object(hook, vec![exc_type, exc_value, exc_tb]).is_ok()
    }


    /// Truthiness test that dispatches __bool__/__len__ on instances.
    /// Walk a class hierarchy to find if it inherits from an ExceptionType
    pub(crate) fn find_exception_kind(cls: &PyObjectRef) -> ExceptionKind {
        match &cls.payload {
            PyObjectPayload::ExceptionType(kind) => kind.clone(),
            PyObjectPayload::BuiltinType(name) | PyObjectPayload::BuiltinFunction(name) => {
                ExceptionKind::from_name(name).unwrap_or(ExceptionKind::RuntimeError)
            }
            PyObjectPayload::Class(cd) => {
                // Check if the class name itself maps to a known exception kind
                if let Some(kind) = ExceptionKind::from_name(&cd.name) {
                    return kind;
                }
                for base in &cd.bases {
                    let kind = Self::find_exception_kind(base);
                    if !matches!(kind, ExceptionKind::RuntimeError) {
                        return kind;
                    }
                    // Also check if base IS the exception type
                    if let PyObjectPayload::ExceptionType(k) = &base.payload {
                        return k.clone();
                    }
                }
                // Check MRO
                for base in &cd.mro {
                    if let PyObjectPayload::ExceptionType(k) = &base.payload {
                        return k.clone();
                    }
                }
                ExceptionKind::RuntimeError
            }
            _ => ExceptionKind::RuntimeError,
        }
    }

    /// Check if any exception kind in the class's full MRO matches the expected handler.
    /// Unlike find_exception_kind (which returns the first non-RuntimeError kind),
    /// this checks ALL bases — essential for multiple inheritance like
    /// `BadRequestKeyError(BadRequest, KeyError)` where the second base matters.
    pub(crate) fn any_exception_kind_matches(cls: &PyObjectRef, expected: &ExceptionKind) -> bool {
        match &cls.payload {
            PyObjectPayload::ExceptionType(kind) => exception_kind_matches(kind, expected),
            PyObjectPayload::BuiltinType(name) | PyObjectPayload::BuiltinFunction(name) => {
                if let Some(kind) = ExceptionKind::from_name(name) {
                    exception_kind_matches(&kind, expected)
                } else { false }
            }
            PyObjectPayload::Class(cd) => {
                // Direct name match
                if let Some(kind) = ExceptionKind::from_name(&cd.name) {
                    if exception_kind_matches(&kind, expected) { return true; }
                }
                // Check all bases recursively
                for base in &cd.bases {
                    if Self::any_exception_kind_matches(base, expected) { return true; }
                }
                // Check MRO entries
                for base in &cd.mro {
                    if let PyObjectPayload::ExceptionType(k) = &base.payload {
                        if exception_kind_matches(k, expected) { return true; }
                    }
                    if let PyObjectPayload::Class(bc) = &base.payload {
                        if let Some(kind) = ExceptionKind::from_name(&bc.name) {
                            if exception_kind_matches(&kind, expected) { return true; }
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    pub(crate) fn vm_is_truthy(&mut self, obj: &PyObjectRef) -> PyResult<bool> {
        if let PyObjectPayload::Instance(_) = &obj.payload {
            if let Some(raw_method) = Self::resolve_instance_dunder(obj, "__bool__") {
                let method = self.resolve_descriptor(&raw_method, obj)?;
                let result = self.call_object(method, vec![])?;
                return Ok(result.is_truthy());
            }
            if let Some(raw_method) = Self::resolve_instance_dunder(obj, "__len__") {
                let method = self.resolve_descriptor(&raw_method, obj)?;
                let result = self.call_object(method, vec![])?;
                return Ok(result.is_truthy());
            }
            // Builtin base type subclass: delegate to __builtin_value__
            if let Some(bv) = Self::get_builtin_value(obj) {
                return Ok(bv.is_truthy());
            }
        }
        Ok(obj.is_truthy())
    }

    /// Try to call a dunder method on an instance. Returns None if the object
    /// is not an Instance or doesn't have the named dunder.
    pub(crate) fn try_call_dunder(
        &mut self, obj: &PyObjectRef, dunder: &str, args: Vec<PyObjectRef>,
    ) -> Result<Option<PyObjectRef>, PyException> {
        match &obj.payload {
            PyObjectPayload::Instance(inst) => {
                // Use resolve_instance_dunder to skip BuiltinBoundMethod from builtin type bases
                if let Some(raw_method) = Self::resolve_instance_dunder(obj, dunder) {
                    let method = self.resolve_descriptor(&raw_method, obj)?;
                    return Ok(Some(self.call_object(method, args)?));
                }
                // Fall through: check __builtin_value__ for supported container operations
                if matches!(dunder, "__getitem__" | "__setitem__" | "__delitem__" |
                    "__contains__" | "__iter__" | "__len__" | "__bool__" |
                    "__add__" | "__mul__" | "__eq__" | "__ne__" | "__lt__" | "__le__" | "__gt__" | "__ge__") {
                    if let Some(bv) = Self::get_builtin_value(obj) {
                        return self.try_call_dunder(&bv, dunder, args);
                    }
                }
                // Namedtuple: delegate to builtin instance method dispatch
                if inst.class.get_attr("__namedtuple__").is_some() {
                    if let Ok(result) = builtins::call_method(obj, dunder, &args) {
                        return Ok(Some(result));
                    }
                }
            }
            PyObjectPayload::Module { .. } => {
                if let Some(method) = obj.get_attr(dunder) {
                    // Module methods expect self as first arg (like file objects with _bind_methods)
                    let mut method_args = vec![obj.clone()];
                    method_args.extend(args);
                    return Ok(Some(self.call_object(method, method_args)?));
                }
            }
            _ => {}
        }
        Ok(None)
    }
}

impl Default for VirtualMachine {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if `actual` exception kind matches `expected` (including inheritance).
pub(crate) fn exception_kind_matches(actual: &ExceptionKind, expected: &ExceptionKind) -> bool {
    if std::mem::discriminant(actual) == std::mem::discriminant(expected) {
        return true;
    }
    // Walk the exception hierarchy
    match expected {
        ExceptionKind::BaseException => true, // catches everything
        ExceptionKind::Exception => !matches!(actual,
            ExceptionKind::SystemExit | ExceptionKind::KeyboardInterrupt |
            ExceptionKind::GeneratorExit | ExceptionKind::BaseExceptionGroup
        ),
        ExceptionKind::ArithmeticError => matches!(actual,
            ExceptionKind::ArithmeticError | ExceptionKind::FloatingPointError |
            ExceptionKind::OverflowError | ExceptionKind::ZeroDivisionError
        ),
        ExceptionKind::LookupError => matches!(actual,
            ExceptionKind::LookupError | ExceptionKind::IndexError | ExceptionKind::KeyError
        ),
        ExceptionKind::OSError => matches!(actual,
            ExceptionKind::OSError | ExceptionKind::BlockingIOError |
            ExceptionKind::BrokenPipeError | ExceptionKind::FileExistsError |
            ExceptionKind::FileNotFoundError | ExceptionKind::PermissionError |
            ExceptionKind::TimeoutError | ExceptionKind::IsADirectoryError |
            ExceptionKind::NotADirectoryError | ExceptionKind::ProcessLookupError |
            ExceptionKind::ConnectionError | ExceptionKind::ConnectionResetError |
            ExceptionKind::ConnectionAbortedError | ExceptionKind::ConnectionRefusedError |
            ExceptionKind::InterruptedError | ExceptionKind::ChildProcessError
        ),
        ExceptionKind::ConnectionError => matches!(actual,
            ExceptionKind::ConnectionError | ExceptionKind::ConnectionResetError |
            ExceptionKind::ConnectionAbortedError | ExceptionKind::ConnectionRefusedError
        ),
        ExceptionKind::UnicodeError => matches!(actual,
            ExceptionKind::UnicodeError | ExceptionKind::UnicodeDecodeError |
            ExceptionKind::UnicodeEncodeError
        ),
        ExceptionKind::ValueError => matches!(actual,
            ExceptionKind::ValueError | ExceptionKind::UnicodeError |
            ExceptionKind::UnicodeDecodeError | ExceptionKind::UnicodeEncodeError |
            ExceptionKind::JSONDecodeError
        ),
        ExceptionKind::Warning => matches!(actual,
            ExceptionKind::Warning | ExceptionKind::DeprecationWarning |
            ExceptionKind::RuntimeWarning | ExceptionKind::UserWarning |
            ExceptionKind::SyntaxWarning | ExceptionKind::FutureWarning |
            ExceptionKind::ImportWarning | ExceptionKind::UnicodeWarning |
            ExceptionKind::BytesWarning | ExceptionKind::ResourceWarning |
            ExceptionKind::PendingDeprecationWarning
        ),
        ExceptionKind::ImportError => matches!(actual,
            ExceptionKind::ImportError | ExceptionKind::ModuleNotFoundError
        ),
        ExceptionKind::RuntimeError => matches!(actual,
            ExceptionKind::RuntimeError | ExceptionKind::NotImplementedError |
            ExceptionKind::RecursionError
        ),
        ExceptionKind::NameError => matches!(actual,
            ExceptionKind::NameError | ExceptionKind::UnboundLocalError
        ),
        ExceptionKind::SyntaxError => matches!(actual,
            ExceptionKind::SyntaxError | ExceptionKind::IndentationError |
            ExceptionKind::TabError
        ),
        ExceptionKind::SubprocessError => matches!(actual,
            ExceptionKind::SubprocessError | ExceptionKind::CalledProcessError |
            ExceptionKind::TimeoutExpired
        ),
        ExceptionKind::BaseExceptionGroup => matches!(actual,
            ExceptionKind::BaseExceptionGroup | ExceptionKind::ExceptionGroup
        ),
        _ => false,
    }
}
