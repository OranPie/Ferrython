//! The main virtual machine — executes bytecode instructions.

use crate::builtins;
use crate::frame::{BlockKind, Frame, FramePool, SharedBuiltins};
use compact_str::CompactString;
use ferrython_bytecode::code::{CodeObject, CodeFlags};
use ferrython_bytecode::opcode::Opcode;
use ferrython_core::error::{ExceptionKind, PyException, PyResult};
use ferrython_core::object::{
    PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef, IteratorData,
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
        let mut has_trace = ferrython_stdlib::get_trace_func().is_some();
        let mut has_profile = ferrython_stdlib::get_profile_func().is_some();

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

        let mut last_line: u32 = 0;
        // Re-check trace/profile periodically (every 64 opcodes) instead of
        // calling thread-local get_trace_func() on every single iteration.
        let mut trace_check_counter: u8 = 0;
        loop {
            if trace_check_counter == 0 {
                trace_check_counter = 63;
                if !has_trace { has_trace = ferrython_stdlib::get_trace_func().is_some(); }
                if !has_profile { has_profile = ferrython_stdlib::get_profile_func().is_some(); }
            } else {
                trace_check_counter -= 1;
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
                let frame = self.call_stack.last_mut().unwrap();
                let ip = frame.ip;
                if ip >= frame.code.instructions.len() { return Ok(PyObject::none()); }
                let instr = frame.code.instructions[ip];
                frame.ip = ip + 1;
                instr
            };

            let frame = self.call_stack.last_mut().unwrap();

            if profiling { self.profiler.start_instruction(instr.op); }

            // Inline the hottest opcodes to avoid execute_one dispatch overhead
            let result = match instr.op {
                Opcode::LoadFast => {
                    let idx = instr.arg as usize;
                    match frame.locals.get(idx).and_then(|v| v.as_ref()) {
                        Some(val) => { frame.stack.push(val.clone()); Ok(None) }
                        None => Err(PyException::name_error(format!(
                            "local variable '{}' referenced before assignment",
                            frame.code.varnames.get(idx).map(|s| s.as_str()).unwrap_or("?")
                        ))),
                    }
                }
                Opcode::StoreFast => {
                    let val = frame.stack.pop().expect("stack underflow");
                    frame.locals[instr.arg as usize] = Some(val);
                    Ok(None)
                }
                Opcode::LoadConst => {
                    let obj = frame.constant_cache[instr.arg as usize].clone();
                    frame.stack.push(obj);
                    Ok(None)
                }
                Opcode::PopTop => {
                    frame.stack.pop();
                    Ok(None)
                }
                Opcode::DupTop => {
                    let v = frame.stack.last().expect("stack underflow").clone();
                    frame.stack.push(v);
                    Ok(None)
                }
                Opcode::RotTwo => {
                    let len = frame.stack.len();
                    frame.stack.swap(len - 1, len - 2);
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
                // Inline ReturnValue: fast path when no finally blocks are active
                Opcode::ReturnValue => {
                    if frame.block_stack.iter().any(|b| b.kind == BlockKind::Finally) {
                        // Must go through full handler for finally unwinding
                        self.execute_one(instr)
                    } else {
                        let val = frame.stack.pop().expect("stack underflow");
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
                                frame.stack.pop();
                                *frame.stack.last_mut().unwrap() = result;
                                Ok(None)
                            }
                            (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                let r = *x + *y;
                                frame.stack.pop();
                                *frame.stack.last_mut().unwrap() = PyObject::float(r);
                                Ok(None)
                            }
                            (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
                                let r = *x as f64 + *y;
                                frame.stack.pop();
                                *frame.stack.last_mut().unwrap() = PyObject::float(r);
                                Ok(None)
                            }
                            (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
                                let r = *x + *y as f64;
                                frame.stack.pop();
                                *frame.stack.last_mut().unwrap() = PyObject::float(r);
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
                            frame.stack.pop();
                            *frame.stack.last_mut().unwrap() = PyObject::bool_val(result);
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
                                frame.stack.pop();
                                *frame.stack.last_mut().unwrap() = PyObject::bool_val(result);
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
                                frame.stack.pop();
                                *frame.stack.last_mut().unwrap() = PyObject::bool_val(result);
                                Ok(None)
                            }
                            // String equality (hot for dict lookups, isinstance checks)
                            (PyObjectPayload::Str(x), PyObjectPayload::Str(y)) if instr.arg == 2 || instr.arg == 3 => {
                                let eq = x == y;
                                let result = if instr.arg == 2 { eq } else { !eq };
                                frame.stack.pop();
                                *frame.stack.last_mut().unwrap() = PyObject::bool_val(result);
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
                        frame.stack.pop();
                        *frame.stack.last_mut().unwrap() = PyObject::bool_val(result);
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
                            if let Some(ref v) = cache[idx] {
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
                    let v = frame.stack.pop().expect("stack underflow");
                    let is_falsy = match &v.payload {
                        PyObjectPayload::Bool(b) => !b,
                        PyObjectPayload::None => true,
                        PyObjectPayload::Int(PyInt::Small(n)) => *n == 0,
                        _ => !self.vm_is_truthy(&v)?,
                    };
                    if is_falsy {
                        self.call_stack.last_mut().unwrap().ip = instr.arg as usize;
                    }
                    Ok(None)
                }
                Opcode::PopJumpIfTrue => {
                    let v = frame.stack.pop().expect("stack underflow");
                    let is_truthy = match &v.payload {
                        PyObjectPayload::Bool(b) => *b,
                        PyObjectPayload::None => false,
                        PyObjectPayload::Int(PyInt::Small(n)) => *n != 0,
                        _ => self.vm_is_truthy(&v)?,
                    };
                    if is_truthy {
                        self.call_stack.last_mut().unwrap().ip = instr.arg as usize;
                    }
                    Ok(None)
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
                                frame.stack.pop();
                                *frame.stack.last_mut().unwrap() = result;
                                Ok(None)
                            }
                            (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                let r = *x - *y;
                                frame.stack.pop();
                                *frame.stack.last_mut().unwrap() = PyObject::float(r);
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
                                frame.stack.pop();
                                *frame.stack.last_mut().unwrap() = result;
                                Ok(None)
                            }
                            (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                                let r = *x * *y;
                                frame.stack.pop();
                                *frame.stack.last_mut().unwrap() = PyObject::float(r);
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
                                frame.stack.pop();
                                *frame.stack.last_mut().unwrap() = PyObject::int(r);
                                Ok(None)
                            }
                            (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) if *y != 0.0 => {
                                let r = *x % *y;
                                // Python modulo for floats: adjust sign
                                let r = if r != 0.0 && (r < 0.0) != (*y < 0.0) { r + *y } else { r };
                                frame.stack.pop();
                                *frame.stack.last_mut().unwrap() = PyObject::float(r);
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
                                frame.stack.pop();
                                *frame.stack.last_mut().unwrap() = PyObject::float(r);
                                Ok(None)
                            }
                            (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) if *y != 0.0 => {
                                let r = *x / *y;
                                frame.stack.pop();
                                *frame.stack.last_mut().unwrap() = PyObject::float(r);
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
                                frame.stack.pop();
                                *frame.stack.last_mut().unwrap() = PyObject::int(r);
                                Ok(None)
                            }
                            (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) if *y != 0.0 => {
                                let r = (*x / *y).floor();
                                frame.stack.pop();
                                *frame.stack.last_mut().unwrap() = PyObject::float(r);
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
                    // Fast path: Python Function with exact positional match, no closures/cells/generators
                    let fast_data = if let PyObjectPayload::Function(pf) = &frame.stack[func_idx].payload {
                        if pf.code.arg_count as usize == arg_count
                            && pf.code.kwonlyarg_count == 0
                            && !pf.code.flags.contains(CodeFlags::VARARGS)
                            && !pf.code.flags.contains(CodeFlags::VARKEYWORDS)
                            && !pf.code.flags.contains(CodeFlags::GENERATOR)
                            && !pf.code.flags.contains(CodeFlags::COROUTINE)
                            && pf.closure.is_empty()
                            && pf.code.cellvars.is_empty()
                            && pf.code.freevars.is_empty()
                        {
                            // Extract Arc-cloned fields directly — avoids cloning the whole func Arc
                            Some((Arc::clone(&pf.code), pf.globals.clone(), Arc::clone(&pf.constant_cache)))
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some((code, globals, constant_cache)) = fast_data {
                        let args_start = func_idx + 1;
                        let mut new_frame = Frame::new_from_pool(
                            code,
                            globals,
                            Arc::clone(&self.builtins),
                            constant_cache,
                            &mut self.frame_pool,
                        );
                        for i in 0..arg_count {
                            new_frame.locals[i] = Some(frame.stack[args_start + i].clone());
                        }
                        new_frame.scope_kind = crate::frame::ScopeKind::Function;
                        frame.stack.truncate(func_idx);
                        self.call_stack.push(new_frame);
                        if self.call_stack.len() > self.recursion_limit {
                            if let Some(frame) = self.call_stack.pop() {
                                frame.recycle(&mut self.frame_pool);
                            }
                            Err(PyException::recursion_error("maximum recursion depth exceeded"))
                        } else {
                            let result = self.run_frame();
                            if let Some(frame) = self.call_stack.pop() {
                                frame.recycle(&mut self.frame_pool);
                            }
                            match result {
                                Ok(val) => {
                                    let val = self.post_call_intercept(val)?;
                                    self.call_stack.last_mut().unwrap().stack.push(val);
                                    Ok(None)
                                }
                                Err(e) => Err(e),
                            }
                        }
                    } else {
                        self.execute_one(instr)
                    }
                }
                // Inline ForIter fast path for simple iterators (Range, List, Tuple)
                Opcode::ForIter => {
                    let stack_len = frame.stack.len();
                    if stack_len > 0 {
                        let iter = &frame.stack[stack_len - 1];
                        if let PyObjectPayload::Iterator(ref iter_data_arc) = iter.payload {
                            let mut data = iter_data_arc.lock();
                            match &mut *data {
                                IteratorData::Range { current, stop, step } => {
                                    let done = if *step > 0 { *current >= *stop } else { *current <= *stop };
                                    if done {
                                        drop(data);
                                        frame.stack.pop();
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
                                        frame.stack.pop();
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
                                        frame.stack.pop();
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
                    } else {
                        self.execute_one(instr)
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
                _ => self.execute_one(instr),
            };

            match result {
                Ok(Some(ret)) => {
                    if profiling { self.profiler.end_instruction(instr.op); }
                    // Fire "return" event
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
                    } else {
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
                => self.exec_binary_ops(instr),

            Opcode::BinarySubscr | Opcode::StoreSubscr | Opcode::DeleteSubscr
                => self.exec_subscript_ops(instr),

            Opcode::CompareOp => self.exec_compare_ops(instr),

            Opcode::JumpForward | Opcode::JumpAbsolute
            | Opcode::PopJumpIfFalse | Opcode::PopJumpIfTrue
            | Opcode::JumpIfTrueOrPop | Opcode::JumpIfFalseOrPop
            | Opcode::GetIter | Opcode::GetYieldFromIter | Opcode::ForIter
            | Opcode::EndForLoop
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
