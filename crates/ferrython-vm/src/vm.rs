//! The main virtual machine — executes bytecode instructions.

use crate::builtins;
use crate::frame::{BlockKind, Frame, FramePool, SharedBuiltins};
use compact_str::CompactString;
use ferrython_bytecode::code::{CodeObject, CodeFlags};
use ferrython_bytecode::opcode::{Instruction, Opcode};
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, IteratorData,
    lookup_in_class_mro,
};
use ferrython_core::types::{HashableKey, PyInt, SharedGlobals};
use ferrython_debug::{ExecutionProfiler, BreakpointManager};
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::{Arc, OnceLock};

/// Shared builtins for spawning thread VMs without re-initializing.
static SHARED_BUILTINS: OnceLock<SharedBuiltins> = OnceLock::new();

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
        let builtins = Arc::new(builtins::init_builtins());
        // Register the thread spawn callback so the stdlib can spawn real OS
        // threads for Python function targets.  Uses the shared builtins Arc.
        {
            let shared_bi = Arc::clone(&builtins);
            SHARED_BUILTINS.get_or_init(|| shared_bi);
            ferrython_core::error::register_thread_spawn(spawn_python_thread_impl);
        }
        Self {
            call_stack: Vec::new(),
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
            call_stack: Vec::new(),
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
        Arc::clone(&self.builtins)
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
        Arc::new(RwLock::new(IndexMap::new()))
    }

    /// Execute a code object (module-level).
    pub fn execute(&mut self, code: CodeObject) -> PyResult<PyObjectRef> {
        self.install_hash_eq_dispatch();
        let globals = Arc::new(RwLock::new(IndexMap::new()));
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
                    HashableKey::Str(CompactString::from("__main__")),
                    main_mod,
                );
            }
        }
        self.execute_with_globals(Arc::new(code), globals)
    }

    /// Execute a code object with shared globals (for REPL).
    pub fn execute_with_globals(&mut self, code: Arc<CodeObject>, globals: SharedGlobals) -> PyResult<PyObjectRef> {
        self.install_hash_eq_dispatch();
        let stack_depth = self.call_stack.len();
        let frame = Frame::new(code, globals.clone(), Arc::clone(&self.builtins));
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

    /// Execute a code object as a function call with arguments.
    pub(crate) fn run_frame(&mut self) -> PyResult<PyObjectRef> {
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
            let instr = if has_trace {
                let frame = self.call_stack.last().unwrap();
                let ip = frame.ip;
                if ip >= frame.code.instructions.len() { return Ok(PyObject::none()); }
                let instr = frame.code.instructions[ip];
                let current_line = Self::ip_to_line(&frame.code, ip);
                let fire_line = current_line != last_line;
                if fire_line { last_line = current_line; }
                self.call_stack.last_mut().unwrap().ip = ip + 1;
                if fire_line { self.fire_trace_event("line", PyObject::none()); }
                instr
            } else {
                // SAFETY: call_stack is never empty during execution
                let cs_len = self.call_stack.len();
                let frame = unsafe { self.call_stack.get_unchecked_mut(cs_len - 1) };
                let ip = frame.ip;
                let instructions = &frame.code.instructions;
                if ip >= instructions.len() { return Ok(PyObject::none()); }
                // SAFETY: bounds check above guarantees ip < instructions.len()
                let instr = unsafe { *instructions.get_unchecked(ip) };
                frame.ip = ip + 1;
                instr
            };

            // SAFETY: call_stack is never empty during execution
            let cs_len = self.call_stack.len();
            let frame = unsafe { self.call_stack.get_unchecked_mut(cs_len - 1) };

            if profiling { self.profiler.start_instruction(instr.op); }

            // Inline the hottest opcodes to avoid execute_one dispatch overhead
            let result = match instr.op {
                Opcode::LoadFast => {
                    let idx = instr.arg as usize;
                    // SAFETY: compiler guarantees idx < locals.len()
                    match unsafe { frame.get_local_unchecked(idx) } {
                        Some(val) => { frame.stack.push(val.clone()); Ok(None) }
                        None => Err(PyException::name_error(format!(
                            "local variable '{}' referenced before assignment",
                            frame.code.varnames.get(idx).map(|s| s.as_str()).unwrap_or("?")
                        ))),
                    }
                }
                Opcode::StoreFast => {
                    // SAFETY: stack non-empty (compiler guarantees), idx < locals.len()
                    let val = unsafe { frame.pop_unchecked() };
                    unsafe { frame.set_local_unchecked(instr.arg as usize, val) };
                    Ok(None)
                }
                Opcode::LoadConst => {
                    // SAFETY: compiler guarantees arg < constant_cache.len()
                    let obj = unsafe { frame.constant_cache.get_unchecked(instr.arg as usize).clone() };
                    frame.stack.push(obj);
                    Ok(None)
                }
                // ── Superinstructions: fused opcode pairs ──
                Opcode::LoadFastLoadFast => {
                    let idx1 = (instr.arg >> 16) as usize;
                    let idx2 = (instr.arg & 0xFFFF) as usize;
                    // SAFETY: compiler guarantees indices < locals.len()
                    let a = unsafe { frame.get_local_unchecked(idx1) }.cloned();
                    let b = unsafe { frame.get_local_unchecked(idx2) }.cloned();
                    match (a, b) {
                        (Some(a), Some(b)) => {
                            frame.stack.push(a);
                            frame.stack.push(b);
                            Ok(None)
                        }
                        (None, _) => Err(PyException::name_error(format!(
                            "local variable '{}' referenced before assignment",
                            frame.code.varnames.get(idx1).map(|s| s.as_str()).unwrap_or("?")
                        ))),
                        (_, None) => Err(PyException::name_error(format!(
                            "local variable '{}' referenced before assignment",
                            frame.code.varnames.get(idx2).map(|s| s.as_str()).unwrap_or("?")
                        ))),
                    }
                }
                Opcode::LoadFastLoadConst => {
                    let fast_idx = (instr.arg >> 16) as usize;
                    let const_idx = (instr.arg & 0xFFFF) as usize;
                    // SAFETY: compiler guarantees indices valid
                    match unsafe { frame.get_local_unchecked(fast_idx) } {
                        Some(val) => {
                            frame.stack.push(val.clone());
                            frame.stack.push(unsafe { frame.constant_cache.get_unchecked(const_idx).clone() });
                            Ok(None)
                        }
                        None => Err(PyException::name_error(format!(
                            "local variable '{}' referenced before assignment",
                            frame.code.varnames.get(fast_idx).map(|s| s.as_str()).unwrap_or("?")
                        ))),
                    }
                }
                Opcode::StoreFastLoadFast => {
                    let store_idx = (instr.arg >> 16) as usize;
                    let load_idx = (instr.arg & 0xFFFF) as usize;
                    // SAFETY: stack non-empty, indices < locals.len()
                    let val = unsafe { frame.pop_unchecked() };
                    unsafe { frame.set_local_unchecked(store_idx, val) };
                    match unsafe { frame.get_local_unchecked(load_idx) } {
                        Some(val) => { frame.stack.push(val.clone()); Ok(None) }
                        None => Err(PyException::name_error(format!(
                            "local variable '{}' referenced before assignment",
                            frame.code.varnames.get(load_idx).map(|s| s.as_str()).unwrap_or("?")
                        ))),
                    }
                }
                // 3-way superinstruction: LoadFast + LoadConst + BinarySubtract
                Opcode::LoadFastLoadConstBinarySub => {
                    let fast_idx = (instr.arg >> 16) as usize;
                    let const_idx = (instr.arg & 0xFFFF) as usize;
                    // SAFETY: compiler guarantees indices valid
                    match unsafe { frame.get_local_unchecked(fast_idx) } {
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
                                    frame.stack.push(result);
                                    Ok(None)
                                }
                                (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                    frame.stack.push(PyObject::float(*x - *y));
                                    Ok(None)
                                }
                                _ => {
                                    // Fallback: push both and let execute_one handle BinarySub
                                    frame.stack.push(local.clone());
                                    frame.stack.push(c.clone());
                                    self.execute_one(ferrython_bytecode::Instruction::new(
                                        Opcode::BinarySubtract, 0))
                                }
                            }
                        }
                        None => Err(PyException::name_error(format!(
                            "local variable '{}' referenced before assignment",
                            frame.code.varnames.get(fast_idx).map(|s| s.as_str()).unwrap_or("?")
                        ))),
                    }
                }
                // 3-way superinstruction: LoadFast + LoadConst + BinaryAdd
                Opcode::LoadFastLoadConstBinaryAdd => {
                    let fast_idx = (instr.arg >> 16) as usize;
                    let const_idx = (instr.arg & 0xFFFF) as usize;
                    match unsafe { frame.get_local_unchecked(fast_idx) } {
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
                                    frame.stack.push(result);
                                    Ok(None)
                                }
                                (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                    frame.stack.push(PyObject::float(*x + *y));
                                    Ok(None)
                                }
                                (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
                                    frame.stack.push(PyObject::float(*x as f64 + *y));
                                    Ok(None)
                                }
                                (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
                                    frame.stack.push(PyObject::float(*x + *y as f64));
                                    Ok(None)
                                }
                                _ => {
                                    frame.stack.push(local.clone());
                                    frame.stack.push(c.clone());
                                    self.execute_one(ferrython_bytecode::Instruction::new(
                                        Opcode::BinaryAdd, 0))
                                }
                            }
                        }
                        None => Err(PyException::name_error(format!(
                            "local variable '{}' referenced before assignment",
                            frame.code.varnames.get(fast_idx).map(|s| s.as_str()).unwrap_or("?")
                        ))),
                    }
                }
                Opcode::PopTop => {
                    // SAFETY: stack non-empty for well-formed bytecode
                    drop(unsafe { frame.pop_unchecked() });
                    Ok(None)
                }
                Opcode::DupTop => {
                    // SAFETY: stack non-empty
                    let v = unsafe { frame.peek_unchecked() }.clone();
                    frame.stack.push(v);
                    Ok(None)
                }
                Opcode::RotTwo => {
                    let len = frame.stack.len();
                    frame.stack.swap(len - 1, len - 2);
                    Ok(None)
                }
                Opcode::RotThree => {
                    let len = frame.stack.len();
                    // TOS moves to TOS2, TOS1→TOS, TOS2→TOS1
                    frame.stack.swap(len - 1, len - 3);
                    frame.stack.swap(len - 1, len - 2);
                    Ok(None)
                }
                Opcode::DupTopTwo => {
                    let len = frame.stack.len();
                    let a = frame.stack[len - 2].clone();
                    let b = frame.stack[len - 1].clone();
                    frame.stack.push(a);
                    frame.stack.push(b);
                    Ok(None)
                }
                Opcode::Nop => Ok(None),
                // Inline GetIter for common types
                Opcode::GetIter => {
                    let obj = frame.stack.last().unwrap();
                    match &obj.payload {
                        // Range/list/tuple iterators are already iterators
                        PyObjectPayload::Iterator(_) => Ok(None),
                        _ => self.execute_one(instr),
                    }
                }
                // Inline ForIter for Range/List (hot in `for i in range(n)`)
                Opcode::ForIter => {
                    // SAFETY: stack non-empty (iterator on TOS)
                    let iter = unsafe { frame.peek_unchecked() };
                    if let PyObjectPayload::Iterator(ref iter_data) = iter.payload {
                        let mut data = iter_data.lock();
                        match &mut *data {
                            IteratorData::Range { current, stop, step } => {
                                let done = if *step > 0 { *current >= *stop } else { *current <= *stop };
                                if done {
                                    drop(data);
                                    drop(unsafe { frame.pop_unchecked() });
                                    frame.ip = instr.arg as usize;
                                } else {
                                    let v = PyObject::int(*current);
                                    *current += *step;
                                    drop(data);
                                    frame.stack.push(v);
                                }
                                Ok(None)
                            }
                            IteratorData::List { items, index } => {
                                if *index < items.len() {
                                    let v = items[*index].clone();
                                    *index += 1;
                                    drop(data);
                                    frame.stack.push(v);
                                } else {
                                    drop(data);
                                    drop(unsafe { frame.pop_unchecked() });
                                    frame.ip = instr.arg as usize;
                                }
                                Ok(None)
                            }
                            IteratorData::Tuple { items, index } => {
                                if *index < items.len() {
                                    let v = items[*index].clone();
                                    *index += 1;
                                    drop(data);
                                    frame.stack.push(v);
                                } else {
                                    drop(data);
                                    drop(unsafe { frame.pop_unchecked() });
                                    frame.ip = instr.arg as usize;
                                }
                                Ok(None)
                            }
                            _ => {
                                drop(data);
                                self.execute_one(instr)
                            }
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
                    if let PyObjectPayload::Iterator(ref iter_data) = iter.payload {
                        let mut data = iter_data.lock();
                        match &mut *data {
                            IteratorData::Range { current, stop, step } => {
                                let done = if *step > 0 { *current >= *stop } else { *current <= *stop };
                                if done {
                                    drop(data);
                                    drop(unsafe { frame.pop_unchecked() });
                                    frame.ip = jump_target;
                                } else {
                                    let v = PyObject::int(*current);
                                    *current += *step;
                                    drop(data);
                                    // Store directly to local — no stack push/pop!
                                    unsafe { frame.set_local_unchecked(store_idx, v) };
                                }
                                Ok(None)
                            }
                            IteratorData::List { items, index } => {
                                if *index < items.len() {
                                    let v = items[*index].clone();
                                    *index += 1;
                                    drop(data);
                                    unsafe { frame.set_local_unchecked(store_idx, v) };
                                } else {
                                    drop(data);
                                    drop(unsafe { frame.pop_unchecked() });
                                    frame.ip = jump_target;
                                }
                                Ok(None)
                            }
                            IteratorData::Tuple { items, index } => {
                                if *index < items.len() {
                                    let v = items[*index].clone();
                                    *index += 1;
                                    drop(data);
                                    unsafe { frame.set_local_unchecked(store_idx, v) };
                                } else {
                                    drop(data);
                                    drop(unsafe { frame.pop_unchecked() });
                                    frame.ip = jump_target;
                                }
                                Ok(None)
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
                                    let v = unsafe { frame.pop_unchecked() };
                                    unsafe { frame.set_local_unchecked(store_idx, v) };
                                }
                                Ok(None)
                            }
                        }
                    } else {
                        // Fallback for non-iterator types
                        let for_instr = ferrython_bytecode::Instruction::new(
                            Opcode::ForIter, jump_target as u32);
                        self.execute_one(for_instr)?;
                        let frame = self.call_stack.last_mut().unwrap();
                        if frame.ip != jump_target {
                            let v = unsafe { frame.pop_unchecked() };
                            unsafe { frame.set_local_unchecked(store_idx, v) };
                        }
                        Ok(None)
                    }
                }
                // Inline ReturnValue: fast path when no finally blocks are active
                Opcode::ReturnValue => {
                    if frame.block_stack.is_empty() {
                        // SAFETY: stack non-empty for well-formed bytecode
                        let val = unsafe { frame.pop_unchecked() };
                        Ok(Some(val))
                    } else if frame.block_stack.iter().any(|b| b.kind == BlockKind::Finally) {
                        self.execute_one(instr)
                    } else {
                        // SAFETY: stack non-empty for well-formed bytecode
                        let val = unsafe { frame.pop_unchecked() };
                        Ok(Some(val))
                    }
                }

                // Inline int+int for BinaryAdd (hot in arithmetic loops)
                Opcode::BinaryAdd | Opcode::InplaceAdd => {
                    let len = frame.stack.len();
                    if len >= 2 {
                        let a = &frame.stack[len - 2];
                        let b = &frame.stack[len - 1];
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
                                Ok(None)
                            }
                            (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                let r = *x + *y;
                                unsafe { frame.binary_op_result(PyObject::float(r)) };
                                Ok(None)
                            }
                            (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
                                let r = *x as f64 + *y;
                                unsafe { frame.binary_op_result(PyObject::float(r)) };
                                Ok(None)
                            }
                            (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
                                let r = *x + *y as f64;
                                unsafe { frame.binary_op_result(PyObject::float(r)) };
                                Ok(None)
                            }
                            _ => self.execute_one(instr),
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline int comparisons (hot in for-loop range iteration)
                Opcode::CompareOp if instr.arg <= 5 => {
                    let len = frame.stack.len();
                    if len >= 2 {
                        let a = &frame.stack[len - 2];
                        let b = &frame.stack[len - 1];
                        // Arc pointer equality fast-path: same object → equal
                        if (instr.arg == 2 || instr.arg == 3) && Arc::ptr_eq(a, b) {
                            let result = instr.arg == 2; // Eq=true, Ne=false
                            unsafe { frame.binary_op_result(PyObject::bool_val(result)) };
                            Ok(None)
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
                                Ok(None)
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
                                Ok(None)
                            }
                            // String equality (hot for dict lookups, isinstance checks)
                            (PyObjectPayload::Str(x), PyObjectPayload::Str(y)) if instr.arg == 2 || instr.arg == 3 => {
                                let eq = x == y;
                                let result = if instr.arg == 2 { eq } else { !eq };
                                unsafe { frame.binary_op_result(PyObject::bool_val(result)) };
                                Ok(None)
                            }
                            _ => self.execute_one(instr),
                        }
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline is/is not comparisons (CompareOp arg 8/9)
                Opcode::CompareOp if instr.arg == 8 || instr.arg == 9 => {
                    let len = frame.stack.len();
                    if len >= 2 {
                        let a = &frame.stack[len - 2];
                        let b = &frame.stack[len - 1];
                        let same = Arc::ptr_eq(a, b)
                            || matches!((&a.payload, &b.payload),
                                (PyObjectPayload::BuiltinType(at), PyObjectPayload::BuiltinType(bt)) if at == bt)
                            || matches!((&a.payload, &b.payload),
                                (PyObjectPayload::ExceptionType(at), PyObjectPayload::ExceptionType(bt)) if at == bt);
                        let result = if instr.arg == 8 { same } else { !same };
                        unsafe { frame.binary_op_result(PyObject::bool_val(result)) };
                        Ok(None)
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
                                frame.stack.push(v.clone());
                                Ok(None)
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
                    let v = unsafe { frame.pop_unchecked() };
                    match &v.payload {
                        PyObjectPayload::Bool(b) => {
                            if !b { frame.ip = instr.arg as usize; }
                            Ok(None)
                        }
                        PyObjectPayload::None => {
                            frame.ip = instr.arg as usize;
                            Ok(None)
                        }
                        PyObjectPayload::Int(PyInt::Small(n)) => {
                            if *n == 0 { frame.ip = instr.arg as usize; }
                            Ok(None)
                        }
                        _ => {
                            if !self.vm_is_truthy(&v)? {
                                let cs_len = self.call_stack.len();
                                unsafe { self.call_stack.get_unchecked_mut(cs_len - 1) }.ip = instr.arg as usize;
                            }
                            Ok(None)
                        }
                    }
                }
                Opcode::PopJumpIfTrue => {
                    // SAFETY: stack non-empty for well-formed bytecode
                    let v = unsafe { frame.pop_unchecked() };
                    match &v.payload {
                        PyObjectPayload::Bool(b) => {
                            if *b { frame.ip = instr.arg as usize; }
                            Ok(None)
                        }
                        PyObjectPayload::None => Ok(None),
                        PyObjectPayload::Int(PyInt::Small(n)) => {
                            if *n != 0 { frame.ip = instr.arg as usize; }
                            Ok(None)
                        }
                        _ => {
                            if self.vm_is_truthy(&v)? {
                                let cs_len = self.call_stack.len();
                                unsafe { self.call_stack.get_unchecked_mut(cs_len - 1) }.ip = instr.arg as usize;
                            }
                            Ok(None)
                        }
                    }
                }
                // Inline unconditional jumps (trivial but saves dispatch)
                Opcode::JumpForward | Opcode::JumpAbsolute => {
                    frame.ip = instr.arg as usize;
                    Ok(None)
                }
                // Inline BinarySub int fast path
                Opcode::BinarySubtract | Opcode::InplaceSubtract => {
                    let len = frame.stack.len();
                    if len >= 2 {
                        let a = &frame.stack[len - 2];
                        let b = &frame.stack[len - 1];
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
                                Ok(None)
                            }
                            (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                let r = *x - *y;
                                unsafe { frame.binary_op_result(PyObject::float(r)) };
                                Ok(None)
                            }
                            _ => self.execute_one(instr),
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline BinaryMul int/float fast path
                Opcode::BinaryMultiply | Opcode::InplaceMultiply => {
                    let len = frame.stack.len();
                    if len >= 2 {
                        let a = &frame.stack[len - 2];
                        let b = &frame.stack[len - 1];
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
                                Ok(None)
                            }
                            (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                let r = *x * *y;
                                unsafe { frame.binary_op_result(PyObject::float(r)) };
                                Ok(None)
                            }
                            _ => self.execute_one(instr),
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline BinaryModulo int fast path (hot in fib, loops)
                Opcode::BinaryModulo | Opcode::InplaceModulo => {
                    let len = frame.stack.len();
                    if len >= 2 {
                        let a = &frame.stack[len - 2];
                        let b = &frame.stack[len - 1];
                        match (&a.payload, &b.payload) {
                            (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) if *y != 0 => {
                                // Python modulo: result has same sign as divisor
                                let r = ((*x % *y) + *y) % *y;
                                unsafe { frame.binary_op_result(PyObject::int(r)) };
                                Ok(None)
                            }
                            (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) if *y != 0.0 => {
                                let r = *x % *y;
                                // Python modulo for floats: adjust sign
                                let r = if r != 0.0 && (r < 0.0) != (*y < 0.0) { r + *y } else { r };
                                unsafe { frame.binary_op_result(PyObject::float(r)) };
                                Ok(None)
                            }
                            _ => self.execute_one(instr),
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline BinaryTrueDivide fast path
                Opcode::BinaryTrueDivide | Opcode::InplaceTrueDivide => {
                    let len = frame.stack.len();
                    if len >= 2 {
                        let a = &frame.stack[len - 2];
                        let b = &frame.stack[len - 1];
                        match (&a.payload, &b.payload) {
                            (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) if *y != 0 => {
                                let r = *x as f64 / *y as f64;
                                unsafe { frame.binary_op_result(PyObject::float(r)) };
                                Ok(None)
                            }
                            (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) if *y != 0.0 => {
                                let r = *x / *y;
                                unsafe { frame.binary_op_result(PyObject::float(r)) };
                                Ok(None)
                            }
                            _ => self.execute_one(instr),
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline BinaryFloorDivide fast path
                Opcode::BinaryFloorDivide | Opcode::InplaceFloorDivide => {
                    let len = frame.stack.len();
                    if len >= 2 {
                        let a = &frame.stack[len - 2];
                        let b = &frame.stack[len - 1];
                        match (&a.payload, &b.payload) {
                            (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) if *y != 0 => {
                                // Python floor division: rounds towards negative infinity
                                let r = x.div_euclid(*y);
                                let r = if (*x ^ *y) < 0 && *x % *y != 0 {
                                    r - 1
                                } else {
                                    r
                                };
                                unsafe { frame.binary_op_result(PyObject::int(r)) };
                                Ok(None)
                            }
                            (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) if *y != 0.0 => {
                                let r = (*x / *y).floor();
                                unsafe { frame.binary_op_result(PyObject::float(r)) };
                                Ok(None)
                            }
                            _ => self.execute_one(instr),
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline LoadDeref (closure variable load — common in functional code)
                Opcode::LoadDeref => {
                    let idx = instr.arg as usize;
                    let val = frame.cells[idx].read().clone();
                    match val {
                        Some(v) => { frame.stack.push(v); Ok(None) }
                        None => {
                            let n_cell = frame.code.cellvars.len();
                            let name = if idx < n_cell {
                                frame.code.cellvars[idx].clone()
                            } else {
                                frame.code.freevars[idx - n_cell].clone()
                            };
                            Err(PyException::name_error(format!(
                                "free variable '{}' referenced before assignment in enclosing scope", name
                            )))
                        }
                    }
                }
                // Inline StoreDeref
                Opcode::StoreDeref => {
                    let val = frame.stack.pop().expect("stack underflow");
                    *frame.cells[instr.arg as usize].write() = Some(val);
                    Ok(None)
                }
                // Inline BuildTuple (very common for returns, unpacking)
                Opcode::BuildTuple => {
                    let count = instr.arg as usize;
                    if count == 0 {
                        frame.stack.push(PyObject::tuple(vec![]));
                    } else {
                        let start = frame.stack.len() - count;
                        let items: Vec<PyObjectRef> = frame.stack.drain(start..).collect();
                        frame.stack.push(PyObject::tuple(items));
                    }
                    Ok(None)
                }
                // Inline BuildList
                Opcode::BuildList => {
                    let count = instr.arg as usize;
                    if count == 0 {
                        frame.stack.push(PyObject::list(vec![]));
                    } else {
                        let start = frame.stack.len() - count;
                        let items: Vec<PyObjectRef> = frame.stack.drain(start..).collect();
                        frame.stack.push(PyObject::list(items));
                    }
                    Ok(None)
                }
                // Inline CallFunction fast path for simple Python function calls
                Opcode::CallFunction => {
                    let arg_count = instr.arg as usize;
                    let stack_len = frame.stack.len();
                    let func_idx = stack_len - 1 - arg_count;
                    // Single payload check: determine both is_simple and is_recursive
                    let call_kind = if let PyObjectPayload::Function(pf) = &frame.stack[func_idx].payload {
                        if pf.is_simple && pf.code.arg_count as usize == arg_count {
                            if Arc::ptr_eq(&pf.code, &frame.code) { 2u8 } else { 1 }
                        } else { 0 }
                    } else { 0 };
                    if call_kind > 0 {
                        let args_start = func_idx + 1;
                        let mut new_frame = if call_kind == 2 {
                            // SAFETY: parent frame outlives child in iterative dispatch
                            unsafe { Frame::new_recursive(frame, &mut self.frame_pool) }
                        } else {
                            // Normal path: clone Arcs from function object
                            let (code, globals, constant_cache) = if let PyObjectPayload::Function(pf) = &frame.stack[func_idx].payload {
                                (Arc::clone(&pf.code), pf.globals.clone(), Arc::clone(&pf.constant_cache))
                            } else { unreachable!() };
                            let mut f = Frame::new_from_pool(
                                code, globals, Arc::clone(&self.builtins), constant_cache,
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
                            // Take ownership of function object (dropped at scope end)
                            let _func = std::ptr::read(base.add(func_idx));
                            frame.stack.set_len(func_idx);
                        }
                        self.call_stack.push(new_frame);
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
                            Ok(None)
                        }
                    } else {
                        // Fast path for common builtins: len(x), range(n)
                        let builtin_name = if let PyObjectPayload::BuiltinFunction(name) = &frame.stack[func_idx].payload {
                            Some(name.as_str())
                        } else { None };
                        match (builtin_name, arg_count) {
                            (Some("len"), 1) => {
                                let arg = &frame.stack[stack_len - 1];
                                let fast_len = match &arg.payload {
                                    PyObjectPayload::List(v) => Some(v.read().len() as i64),
                                    PyObjectPayload::Tuple(v) => Some(v.len() as i64),
                                    PyObjectPayload::Str(s) => Some(s.chars().count() as i64),
                                    PyObjectPayload::Dict(m) => Some(m.read().len() as i64),
                                    PyObjectPayload::Set(m) => Some(m.read().len() as i64),
                                    PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Some(b.len() as i64),
                                    _ => None,
                                };
                                if let Some(n) = fast_len {
                                    unsafe { frame.stack.set_len(func_idx); }
                                    frame.stack.push(PyObject::int(n));
                                    Ok(None)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            (Some("range"), 1) => {
                                let arg = &frame.stack[stack_len - 1];
                                if let PyObjectPayload::Int(PyInt::Small(stop)) = &arg.payload {
                                    let stop = *stop;
                                    unsafe { frame.stack.set_len(func_idx); }
                                    let iter = PyObject::wrap(PyObjectPayload::Iterator(
                                        Arc::new(parking_lot::Mutex::new(IteratorData::Range {
                                            current: 0, stop, step: 1,
                                        }))
                                    ));
                                    frame.stack.push(iter);
                                    Ok(None)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            _ => self.execute_one(instr),
                        }
                    }
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
                                if Arc::ptr_eq(&pf.code, &frame.code) { 2u8 } else { 1 }
                            } else { 0 }
                        } else { 0 };
                        if call_kind > 0 {
                            let stack_len = frame.stack.len();
                            let args_start = stack_len - arg_count;
                            let mut new_frame = if call_kind == 2 {
                                // SAFETY: parent frame outlives child in iterative dispatch
                                unsafe { Frame::new_recursive(frame, &mut self.frame_pool) }
                            } else {
                                let (code, globals, constant_cache) = if let PyObjectPayload::Function(pf) = &func_obj.payload {
                                    (Arc::clone(&pf.code), pf.globals.clone(), Arc::clone(&pf.constant_cache))
                                } else { unreachable!() };
                                let mut f = Frame::new_from_pool(
                                    code, globals, Arc::clone(&self.builtins), constant_cache,
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
                                Ok(None)
                            }
                        } else {
                            // Fast path for builtins (len, range) from global cache
                            let builtin_name = if let PyObjectPayload::BuiltinFunction(name) = &func_obj.payload {
                                Some(name.as_str())
                            } else { None };
                            match (builtin_name, arg_count) {
                                (Some("len"), 1) => {
                                    let arg = &frame.stack[frame.stack.len() - 1];
                                    let fast_len = match &arg.payload {
                                        PyObjectPayload::List(v) => Some(v.read().len() as i64),
                                        PyObjectPayload::Tuple(v) => Some(v.len() as i64),
                                        PyObjectPayload::Str(s) => Some(s.chars().count() as i64),
                                        PyObjectPayload::Dict(m) => Some(m.read().len() as i64),
                                        PyObjectPayload::Set(m) => Some(m.read().len() as i64),
                                        PyObjectPayload::Bytes(b) | PyObjectPayload::ByteArray(b) => Some(b.len() as i64),
                                        _ => None,
                                    };
                                    if let Some(n) = fast_len {
                                        frame.stack.pop();
                                        frame.stack.push(PyObject::int(n));
                                        Ok(None)
                                    } else {
                                        frame.stack.push(func_obj.clone());
                                        let call_instr = Instruction::new(Opcode::CallFunction, arg_count as u32);
                                        self.execute_one(call_instr)
                                    }
                                }
                                (Some("range"), 1) => {
                                    let arg = &frame.stack[frame.stack.len() - 1];
                                    if let PyObjectPayload::Int(PyInt::Small(stop)) = &arg.payload {
                                        let stop = *stop;
                                        frame.stack.pop();
                                        let iter = PyObject::wrap(PyObjectPayload::Iterator(
                                            Arc::new(parking_lot::Mutex::new(IteratorData::Range {
                                                current: 0, stop, step: 1,
                                            }))
                                        ));
                                        frame.stack.push(iter);
                                        Ok(None)
                                    } else {
                                        frame.stack.push(func_obj.clone());
                                        let call_instr = Instruction::new(Opcode::CallFunction, arg_count as u32);
                                        self.execute_one(call_instr)
                                    }
                                }
                                _ => {
                                    // Not a simple function — decompose to LoadGlobal + CallFunction
                                    frame.stack.push(func_obj.clone());
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
                // Inline UnpackSequence for tuples and lists
                Opcode::UnpackSequence => {
                    let count = instr.arg as usize;
                    let seq = frame.stack.pop().expect("stack underflow");
                    match &seq.payload {
                        PyObjectPayload::Tuple(items) if items.len() == count => {
                            for item in items.iter().rev() {
                                frame.stack.push(item.clone());
                            }
                            Ok(None)
                        }
                        PyObjectPayload::List(items_arc) => {
                            let items = items_arc.read();
                            if items.len() == count {
                                for item in items.iter().rev() {
                                    frame.stack.push(item.clone());
                                }
                                Ok(None)
                            } else {
                                drop(items);
                                frame.stack.push(seq);
                                self.execute_one(instr)
                            }
                        }
                        _ => {
                            frame.stack.push(seq);
                            self.execute_one(instr)
                        }
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
                        let obj = &frame.stack[stack_len - 1];
                        match &obj.payload {
                            PyObjectPayload::Instance(inst) => {
                                let skip_ga = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                                    !cd.has_getattribute
                                } else { false };
                                if skip_ga && inst.dict_storage.is_none() && !inst.is_special {
                                    let name = &frame.code.names[name_idx];
                                    if name.as_str() != "__class__" && name.as_str() != "__dict__" {
                                        let class = &inst.class;
                                        if let PyObjectPayload::Class(cd) = &class.payload {
                                            if let Some(class_val) = cd.namespace.read().get(name.as_str()).cloned() {
                                                if matches!(&class_val.payload, PyObjectPayload::Function(_)) {
                                                    fast_kind = 1;
                                                    fast_val = Some(class_val);
                                                }
                                            } else if let Some(v) = inst.attrs.read().get(name.as_str()).cloned() {
                                                fast_kind = 2;
                                                fast_val = Some(v);
                                            } else if let Some(method) = lookup_in_class_mro(class, name.as_str()) {
                                                if matches!(&method.payload,
                                                    PyObjectPayload::Function(_) | PyObjectPayload::NativeFunction { .. }) {
                                                    fast_kind = 1;
                                                    fast_val = Some(method);
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
                            let recv = frame.stack.pop().unwrap();
                            frame.stack.push(method);
                            frame.stack.push(recv);
                            Ok(None)
                        }
                        2 => {
                            // Two-item protocol slow path: push None sentinel + callable
                            let val = fast_val.unwrap();
                            *frame.stack.last_mut().unwrap() = PyObject::none();
                            frame.stack.push(val);
                            Ok(None)
                        }
                        3 => {
                            // Builtin type method: use unbound protocol with Str tag
                            // Stack: [name_as_Str, receiver] — CallMethod detects Str in slot_0
                            // Avoids Arc allocation for BuiltinBoundMethod entirely
                            let name = frame.code.names[name_idx].clone();
                            let name_obj = PyObject::str_val(name);
                            // receiver is already TOS, insert name below it
                            let recv_idx = frame.stack.len() - 1;
                            frame.stack.push(name_obj);
                            frame.stack.swap(recv_idx, recv_idx + 1);
                            Ok(None)
                        }
                        _ => self.execute_one(instr),
                    }
                }
                // Inline CallMethod super-fast path: two-item protocol + direct frame creation
                Opcode::CallMethod => {
                    let arg_count = instr.arg as usize;
                    let stack_len = frame.stack.len();
                    let base_idx = stack_len - arg_count - 2;
                    let slot_0 = &frame.stack[base_idx];
                    // Fast path: slot_0 is a Python function (unbound method)
                    let fast_data = if !matches!(&slot_0.payload, PyObjectPayload::None) {
                        if let PyObjectPayload::Function(pf) = &slot_0.payload {
                            if pf.is_simple && pf.code.arg_count as usize == arg_count + 1 {
                                Some((Arc::clone(&pf.code), pf.globals.clone(), Arc::clone(&pf.constant_cache)))
                            } else { None }
                        } else { None }
                    } else { None };
                    if let Some((code, globals, cc)) = fast_data {
                        let mut new_frame = Frame::new_from_pool(
                            code, globals, Arc::clone(&self.builtins), cc, &mut self.frame_pool,
                        );
                        // Stack: [..., method, receiver, arg0, ..., argN-1]
                        // Move args + receiver + method off stack with direct reads
                        let arg_start = frame.stack.len() - arg_count;
                        unsafe {
                            let base = frame.stack.as_ptr();
                            for i in 0..arg_count {
                                new_frame.locals[i + 1] = Some(
                                    std::ptr::read(base.add(arg_start + i))
                                );
                            }
                            // receiver at arg_start - 1, method at arg_start - 2
                            new_frame.locals[0] = Some(std::ptr::read(base.add(arg_start - 1)));
                            let _method = std::ptr::read(base.add(arg_start - 2));
                            frame.stack.set_len(arg_start - 2);
                        }
                        // Inherit global cache for recursive calls (same code object)
                        if Arc::ptr_eq(&frame.code, &new_frame.code) {
                            if let Some(ref cache) = frame.global_cache {
                                new_frame.global_cache = Some(cache.clone());
                                new_frame.global_cache_version = frame.global_cache_version;
                            }
                        }
                        new_frame.scope_kind = crate::frame::ScopeKind::Function;
                        self.call_stack.push(new_frame);
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
                            Ok(None)
                        }
                    } else {
                        // Fast path for builtin type methods (list.append, dict.get, etc.)
                        // LoadMethod pushes [name_as_Str, receiver] for builtin types
                        let is_builtin_str = matches!(&frame.stack[base_idx].payload, PyObjectPayload::Str(_));
                        if is_builtin_str {
                            // Check for ultra-fast inline list.append / list.pop
                            let is_list_append = arg_count == 1
                                && matches!((&frame.stack[base_idx].payload, &frame.stack[base_idx + 1].payload),
                                    (PyObjectPayload::Str(n), PyObjectPayload::List(_)) if n.as_str() == "append");
                            let is_list_pop = !is_list_append && arg_count == 0
                                && matches!((&frame.stack[base_idx].payload, &frame.stack[base_idx + 1].payload),
                                    (PyObjectPayload::Str(n), PyObjectPayload::List(_)) if n.as_str() == "pop");
                            if is_list_append {
                                // Pop val, receiver, name — then push directly
                                let val = frame.stack.pop().unwrap();
                                let receiver = frame.stack.pop().unwrap();
                                frame.stack.pop(); // name
                                if let PyObjectPayload::List(items) = &receiver.payload {
                                    items.write().push(val);
                                }
                                frame.stack.push(PyObject::none());
                                Ok(None)
                            } else if is_list_pop {
                                let receiver = frame.stack.pop().unwrap();
                                frame.stack.pop(); // name
                                if let PyObjectPayload::List(items) = &receiver.payload {
                                    match items.write().pop() {
                                        Some(val) => {
                                            frame.stack.push(val);
                                            Ok(None)
                                        }
                                        None => Err(PyException::index_error("pop from empty list")),
                                    }
                                } else { unreachable!() }
                            } else {
                                // General builtin method dispatch
                                let mut args = Vec::with_capacity(arg_count);
                                for _ in 0..arg_count {
                                    args.push(frame.stack.pop().unwrap());
                                }
                                args.reverse();
                                let receiver = frame.stack.pop().unwrap();
                                let name_obj = frame.stack.pop().unwrap();
                                if let PyObjectPayload::Str(ref name) = name_obj.payload {
                                    let result = crate::builtins::call_method(&receiver, name.as_str(), &args)?;
                                    frame.stack.push(result);
                                    Ok(None)
                                } else {
                                    unreachable!()
                                }
                            }
                        } else {
                            self.execute_one(instr)
                        }
                    }
                }
                // Inline BinarySubscr for list[int], tuple[int], dict[HashableKey]
                Opcode::BinarySubscr => {
                    let len = frame.stack.len();
                    if len >= 2 {
                        let obj = &frame.stack[len - 2];
                        let key = &frame.stack[len - 1];
                        match (&obj.payload, &key.payload) {
                            // list[int] — direct index
                            (PyObjectPayload::List(items_arc), PyObjectPayload::Int(PyInt::Small(idx))) => {
                                let items = items_arc.read();
                                let i = *idx;
                                let actual = if i < 0 { i + items.len() as i64 } else { i };
                                if actual >= 0 && (actual as usize) < items.len() {
                                    let val = items[actual as usize].clone();
                                    drop(items);
                                    unsafe { frame.binary_op_result(val) };
                                    Ok(None)
                                } else {
                                    drop(items);
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
                                    Ok(None)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            // dict[str] — hash lookup (common: kwargs, config dicts)
                            (PyObjectPayload::Dict(map), PyObjectPayload::Str(s)) => {
                                let hk = HashableKey::Str(s.clone());
                                let val = map.read().get(&hk).cloned();
                                if let Some(v) = val {
                                    unsafe { frame.binary_op_result(v) };
                                    Ok(None)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            // dict[int] — hash lookup
                            (PyObjectPayload::Dict(map), PyObjectPayload::Int(pi @ PyInt::Small(_))) => {
                                let hk = HashableKey::Int(pi.clone());
                                let val = map.read().get(&hk).cloned();
                                if let Some(v) = val {
                                    unsafe { frame.binary_op_result(v) };
                                    Ok(None)
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
                                    Ok(None)
                                } else {
                                    self.execute_one(instr)
                                }
                            }
                            _ => self.execute_one(instr),
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline StoreSubscr for list[int] and dict[str/int]
                Opcode::StoreSubscr => {
                    let len = frame.stack.len();
                    if len >= 3 {
                        let key = &frame.stack[len - 1];
                        let obj = &frame.stack[len - 2];
                        let val = &frame.stack[len - 3];
                        match (&obj.payload, &key.payload) {
                            // list[int] = val
                            (PyObjectPayload::List(items_arc), PyObjectPayload::Int(PyInt::Small(idx))) => {
                                let mut items = items_arc.write();
                                let i = *idx;
                                let actual = if i < 0 { i + items.len() as i64 } else { i };
                                if actual >= 0 && (actual as usize) < items.len() {
                                    let v = val.clone();
                                    items[actual as usize] = v;
                                    drop(items);
                                    frame.stack.truncate(len - 3);
                                    Ok(None)
                                } else {
                                    drop(items);
                                    self.execute_one(instr)
                                }
                            }
                            // dict[str] = val
                            (PyObjectPayload::Dict(map), PyObjectPayload::Str(s)) => {
                                let hk = HashableKey::Str(s.clone());
                                let v = val.clone();
                                map.write().insert(hk, v);
                                frame.stack.truncate(len - 3);
                                Ok(None)
                            }
                            // dict[int] = val
                            (PyObjectPayload::Dict(map), PyObjectPayload::Int(pi @ PyInt::Small(_))) => {
                                let hk = HashableKey::Int(pi.clone());
                                let v = val.clone();
                                map.write().insert(hk, v);
                                frame.stack.truncate(len - 3);
                                Ok(None)
                            }
                            _ => self.execute_one(instr),
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline ListAppend (hot in list comprehensions)
                Opcode::ListAppend => {
                    let item = frame.stack.pop().expect("stack underflow");
                    let idx = instr.arg as usize;
                    let stack_pos = frame.stack.len() - idx;
                    if let PyObjectPayload::List(items) = &frame.stack[stack_pos].payload {
                        items.write().push(item);
                    }
                    Ok(None)
                }
                // Inline LoadAttr fast path for simple instance attribute reads
                // Fused LoadFast + LoadAttr — common in `x = obj.attr` patterns
                Opcode::LoadFastLoadAttr => {
                    let local_idx = (instr.arg >> 16) as usize;
                    let name_idx = (instr.arg & 0xFFFF) as usize;
                    let name = &frame.code.names[name_idx];
                    let obj = match unsafe { frame.get_local_unchecked(local_idx) } {
                        Some(val) => val,
                        None => {
                            return Err(PyException::name_error(format!(
                                "local variable referenced before assignment"
                            )));
                        }
                    };
                    // Inline Instance attr fast path
                    let fast_val = if let PyObjectPayload::Instance(inst) = &obj.payload {
                        let has_ga = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                            cd.has_getattribute
                        } else { false };
                        if !has_ga {
                            let attrs = inst.attrs.read();
                            if let Some(v) = attrs.get(name.as_str()) {
                                match &v.payload {
                                    PyObjectPayload::Function(_)
                                    | PyObjectPayload::Property { .. } => None,
                                    _ => Some(v.clone()),
                                }
                            } else { None }
                        } else { None }
                    } else { None };
                    if let Some(val) = fast_val {
                        frame.stack.push(val);
                        Ok(None)
                    } else {
                        // Decompose: push local, then execute LoadAttr
                        frame.stack.push(obj.clone());
                        let attr_instr = Instruction::new(Opcode::LoadAttr, name_idx as u32);
                        self.execute_one(attr_instr)
                    }
                }
                Opcode::LoadAttr => {
                    let name = &frame.code.names[instr.arg as usize];
                    let obj = &frame.stack[frame.stack.len() - 1];
                    // Fast path: Instance with no __getattribute__ override, attr in instance dict,
                    // and attr is not a Function/Property (those need BoundMethod wrapping)
                    let fast_val = if let PyObjectPayload::Instance(inst) = &obj.payload {
                        let has_ga = if let PyObjectPayload::Class(cd) = &inst.class.payload {
                            cd.has_getattribute
                        } else { false };
                        if !has_ga {
                            let attrs = inst.attrs.read();
                            if let Some(v) = attrs.get(name.as_str()) {
                                match &v.payload {
                                    PyObjectPayload::Function(_)
                                    | PyObjectPayload::Property { .. } => None,
                                    _ => Some(v.clone()),
                                }
                            } else { None }
                        } else { None }
                    } else { None };
                    if let Some(val) = fast_val {
                        let len = frame.stack.len();
                        frame.stack[len - 1] = val;
                        Ok(None)
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline StoreGlobal to avoid execute_one dispatch
                Opcode::StoreGlobal => {
                    let name = frame.code.names[instr.arg as usize].clone();
                    let value = frame.stack.pop().expect("stack underflow");
                    frame.globals.write().insert(name, value);
                    crate::frame::bump_globals_version();
                    Ok(None)
                }
                // Inline StoreName for module/class scope
                Opcode::StoreName => {
                    let name = frame.code.names[instr.arg as usize].clone();
                    let value = frame.stack.pop().expect("stack underflow");
                    match frame.scope_kind {
                        crate::frame::ScopeKind::Module => {
                            frame.globals.write().insert(name, value);
                            crate::frame::bump_globals_version();
                        }
                        _ => { frame.local_names.insert(name, value); }
                    }
                    Ok(None)
                }
                // Inline StoreAttr fast path for simple instance attribute writes
                Opcode::StoreAttr => {
                    let name = &frame.code.names[instr.arg as usize];
                    let stack_len = frame.stack.len();
                    // Fast path: Instance with no __setattr__, no descriptors, no __slots__
                    let fast = if stack_len >= 2 {
                        if let PyObjectPayload::Instance(inst) = &frame.stack[stack_len - 1].payload {
                            if let PyObjectPayload::Class(cd) = &inst.class.payload {
                                !cd.has_setattr && !cd.has_descriptors && cd.slots.is_none()
                            } else { false }
                        } else { false }
                    } else { false };
                    if fast {
                        let obj = frame.stack.pop().expect("stack underflow");
                        let value = frame.stack.pop().expect("stack underflow");
                        if let PyObjectPayload::Instance(inst) = &obj.payload {
                            inst.attrs.write().insert(
                                ferrython_core::intern::intern_or_new(name.as_str()),
                                value,
                            );
                        }
                        Ok(None)
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline CompareOp for int/int, float/float, is/is_not
                Opcode::CompareOp => {
                    let len = frame.stack.len();
                    if len >= 2 {
                        let op = instr.arg;
                        let a = &frame.stack[len - 2];
                        let b = &frame.stack[len - 1];
                        let fast_result = match (&a.payload, &b.payload) {
                            (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                                match op {
                                    0 => Some(*x < *y),  // lt
                                    1 => Some(*x <= *y), // le
                                    2 => Some(*x == *y), // eq
                                    3 => Some(*x != *y), // ne
                                    4 => Some(*x > *y),  // gt
                                    5 => Some(*x >= *y), // ge
                                    _ => None,
                                }
                            }
                            (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                match op {
                                    0 => Some(*x < *y),
                                    1 => Some(*x <= *y),
                                    2 => Some(*x == *y),
                                    3 => Some(*x != *y),
                                    4 => Some(*x > *y),
                                    5 => Some(*x >= *y),
                                    _ => None,
                                }
                            }
                            (PyObjectPayload::Bool(x), PyObjectPayload::Bool(y)) if op == 2 || op == 3 => {
                                if op == 2 { Some(*x == *y) } else { Some(*x != *y) }
                            }
                            _ => {
                                // is / is not: pointer identity
                                if op == 8 { Some(std::sync::Arc::ptr_eq(a, b)) }
                                else if op == 9 { Some(!std::sync::Arc::ptr_eq(a, b)) }
                                else { None }
                            }
                        };
                        if let Some(val) = fast_result {
                            unsafe { frame.binary_op_result(PyObject::bool_val(val)) };
                            Ok(None)
                        } else {
                            self.execute_one(instr)
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Fused CompareOp + PopJumpIfFalse: avoids intermediate bool allocation
                Opcode::CompareOpPopJumpIfFalse => {
                    let cmp_op = instr.arg >> 24;
                    let jump_target = (instr.arg & 0x00FF_FFFF) as usize;
                    let len = frame.stack.len();
                    if len >= 2 {
                        let a = &frame.stack[len - 2];
                        let b = &frame.stack[len - 1];
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
                            Ok(None)
                        } else {
                            // Fallback: execute CompareOp, then check result
                            let cmp_instr = Instruction::new(Opcode::CompareOp, cmp_op);
                            let result = self.exec_compare_ops(cmp_instr)?;
                            if result.is_none() {
                                let frame = self.call_stack.last_mut().unwrap();
                                let v = frame.stack.pop().expect("stack underflow");
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
                            Ok(None)
                        }
                    } else {
                        self.execute_one(instr)
                    }
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
                        child.recycle(&mut self.frame_pool);
                        // SAFETY: we verified len > initial_depth >= 1 and popped one
                        let cs_len = self.call_stack.len();
                        unsafe { self.call_stack.get_unchecked_mut(cs_len - 1) }.stack.push(ret);
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
                }
                Err(mut exc) => {
                    // Fire "exception" trace event
                    if has_trace {
                        let exc_info = PyObject::tuple(vec![
                            PyObject::exception_type(exc.kind.clone()),
                            PyObject::str_val(CompactString::from(exc.message.as_str())),
                            PyObject::none(),
                        ]);
                        self.fire_trace_event("exception", exc_info);
                    }
                    // Always attach traceback from the call stack
                    if exc.traceback.is_empty() {
                        self.attach_traceback(&mut exc);
                    }
                    // Implicit chaining: if there's an active exception and the
                    // new one doesn't already have context, set __context__
                    if exc.context.is_none() {
                        if let Some(ref active) = self.active_exception {
                            exc.context = Some(Box::new(active.clone()));
                        }
                    }
                    // Iterative exception unwind: try current frame, then parents
                    loop {
                        if let Some(handler_ip) = self.unwind_except() {
                            // Store active exception for bare `raise` re-raise
                            self.active_exception = Some(exc.clone());
                            // Also update core thread-local (used by ferrython-traceback)
                            ferrython_core::error::set_thread_exc_info(
                                exc.kind.clone(),
                                exc.message.clone(),
                                exc.traceback.clone(),
                            );
                            let frame = self.call_stack.last_mut().unwrap();
                            // CPython pushes (traceback, value, type) — 3 items
                            let (exc_value, exc_type) = if let Some(orig) = &exc.original {
                                let cls = if let PyObjectPayload::Instance(inst) = &orig.payload {
                                    inst.class.clone()
                                } else {
                                    PyObject::exception_type(exc.kind.clone())
                                };
                                (orig.clone(), cls)
                            } else {
                                let inst = if let Some(val) = &exc.value {
                                    PyObject::exception_instance_with_args(exc.kind.clone(), exc.message.clone(), vec![val.clone()])
                                } else {
                                    PyObject::exception_instance(exc.kind.clone(), exc.message.clone())
                                };
                                (
                                    inst,
                                    PyObject::exception_type(exc.kind.clone()),
                                )
                            };
                            // Store StopIteration.value if present
                            if let Some(val) = &exc.value {
                                Self::store_exc_attr(&exc_value, "value", val.clone());
                            }
                            // Store OSError attributes (.errno, .strerror, .filename) if present
                            if let Some(info) = &exc.os_error_info {
                                Self::store_exc_attr(&exc_value, "errno", PyObject::int(info.errno as i64));
                                Self::store_exc_attr(&exc_value, "strerror", PyObject::str_val(CompactString::from(info.strerror.as_str())));
                                if let Some(fname) = &info.filename {
                                    Self::store_exc_attr(&exc_value, "filename", PyObject::str_val(CompactString::from(fname.as_str())));
                                } else {
                                    Self::store_exc_attr(&exc_value, "filename", PyObject::none());
                                }
                            }
                            // Attach __cause__ from exception chaining (raise X from Y)
                            if let Some(cause) = &exc.cause {
                                let cause_obj = if let Some(corig) = &cause.original {
                                    corig.clone()
                                } else {
                                    PyObject::exception_instance(cause.kind.clone(), cause.message.clone())
                                };
                                Self::store_exc_attr(&exc_value, "__cause__", cause_obj);
                            } else {
                                Self::store_exc_attr(&exc_value, "__cause__", PyObject::none());
                            }
                            // Attach __context__ from implicit exception chaining
                            if let Some(ctx) = &exc.context {
                                let ctx_obj = if let Some(corig) = &ctx.original {
                                    corig.clone()
                                } else {
                                    PyObject::exception_instance(ctx.kind.clone(), ctx.message.clone())
                                };
                                Self::store_exc_attr(&exc_value, "__context__", ctx_obj);
                            } else {
                                Self::store_exc_attr(&exc_value, "__context__", PyObject::none());
                            }
                            // Store __suppress_context__ (True when explicit cause is set)
                            if exc.cause.is_some() {
                                Self::store_exc_attr(&exc_value, "__suppress_context__", PyObject::bool_val(true));
                            }
                            // Store __traceback__ on the exception value
                            let tb_obj = Self::build_traceback_object(&exc.traceback);
                            Self::store_exc_attr(&exc_value, "__traceback__", tb_obj.clone());
                            // Update thread-local for sys.exc_info() — after exc_value and __traceback__ are ready
                            ferrython_stdlib::set_exc_info(
                                exc.kind.clone(),
                                exc.message.clone(),
                                Some(exc_value.clone()),
                            );
                            frame.push(tb_obj);               // traceback
                            frame.push(exc_value);            // value
                            frame.push(exc_type);             // type
                            frame.ip = handler_ip;
                            break; // handler found, continue main loop
                        }
                        // No handler in current frame — unwind iteratively
                        if self.call_stack.len() > initial_depth {
                            if let Some(child) = self.call_stack.pop() {
                                child.recycle(&mut self.frame_pool);
                            }
                            continue; // try parent frame's block stack
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
            frame_attrs.insert(CompactString::from("f_locals"), PyObject::dict(IndexMap::new()));
            frame_attrs.insert(CompactString::from("f_globals"), PyObject::dict(IndexMap::new()));
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
                inst.attrs.write().insert(CompactString::from(name), value);
            }
            PyObjectPayload::ExceptionInstance { attrs, .. } => {
                attrs.write().insert(CompactString::from(name), value);
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

    /// Find an exception handler on the block stack. Returns handler IP if found.
    pub(crate) fn unwind_except(&mut self) -> Option<usize> {
        let frame = self.call_stack.last_mut()?;
        while let Some(block) = frame.pop_block() {
            match block.kind {
                BlockKind::Except | BlockKind::Finally => {
                    // Unwind value stack to block level
                    while frame.stack.len() > block.stack_level {
                        frame.pop();
                    }
                    // Push an ExceptHandler block so PopExcept can find it
                    frame.push_block(BlockKind::ExceptHandler, 0);
                    return Some(block.handler);
                }
                BlockKind::ExceptHandler => {
                    // Clean up a previous except handler (exception in except body)
                    while frame.stack.len() > block.stack_level {
                        frame.pop();
                    }
                    continue;
                }
                BlockKind::Loop => {
                    while frame.stack.len() > block.stack_level {
                        frame.pop();
                    }
                    continue;
                }
                BlockKind::With => {
                    // With block exception — jump to cleanup handler which will
                    // call __exit__ with exception info
                    while frame.stack.len() > block.stack_level {
                        frame.pop();
                    }
                    return Some(block.handler);
                }
            }
        }
        None
    }

    #[cold]
    fn execute_one(&mut self, instr: ferrython_bytecode::Instruction) -> Result<Option<PyObjectRef>, PyException> {
        use ferrython_bytecode::opcode::Opcode;
        match instr.op {
            Opcode::Nop | Opcode::PopTop | Opcode::RotTwo | Opcode::RotThree
            | Opcode::RotFour | Opcode::DupTop | Opcode::DupTopTwo | Opcode::LoadConst
                => self.exec_stack_ops(instr),

            Opcode::LoadName | Opcode::StoreName | Opcode::DeleteName
            | Opcode::LoadFast | Opcode::StoreFast | Opcode::DeleteFast
            | Opcode::LoadDeref | Opcode::StoreDeref | Opcode::DeleteDeref
            | Opcode::LoadClosure | Opcode::LoadClassderef
            | Opcode::LoadGlobal | Opcode::StoreGlobal | Opcode::DeleteGlobal
            | Opcode::LoadFastLoadFast | Opcode::LoadFastLoadConst
            | Opcode::StoreFastLoadFast
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
                => self.exec_binary_ops(instr),

            Opcode::BinarySubscr | Opcode::StoreSubscr | Opcode::DeleteSubscr
                => self.exec_subscript_ops(instr),

            Opcode::CompareOp | Opcode::CompareOpPopJumpIfFalse => self.exec_compare_ops(instr),

            Opcode::JumpForward | Opcode::JumpAbsolute
            | Opcode::PopJumpIfFalse | Opcode::PopJumpIfTrue
            | Opcode::JumpIfTrueOrPop | Opcode::JumpIfFalseOrPop
            | Opcode::GetIter | Opcode::GetYieldFromIter | Opcode::ForIter
            | Opcode::ForIterStoreFast | Opcode::EndForLoop
                => self.exec_jump_ops(instr),

            Opcode::BuildTuple | Opcode::BuildList | Opcode::BuildSet
            | Opcode::BuildMap | Opcode::BuildConstKeyMap | Opcode::BuildString
            | Opcode::ListAppend | Opcode::SetAdd | Opcode::MapAdd
            | Opcode::DictUpdate | Opcode::DictMerge | Opcode::ListExtend
            | Opcode::SetUpdate | Opcode::ListToTuple | Opcode::BuildSlice
            | Opcode::UnpackSequence | Opcode::UnpackEx
                => self.exec_build_ops(instr),

            Opcode::CallFunction | Opcode::CallFunctionKw | Opcode::CallMethod
            | Opcode::CallFunctionEx | Opcode::LoadMethod | Opcode::MakeFunction
            | Opcode::LoadGlobalCallFunction | Opcode::LoadFastLoadAttr
                => self.exec_call_ops(instr),

            Opcode::ReturnValue | Opcode::ImportName | Opcode::ImportFrom
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
        for (name, val) in &frame.local_names {
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
        let exc_type = PyObject::exception_type(exc.kind.clone());
        let exc_value = PyObject::str_val(CompactString::from(exc.message.as_str()));
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
