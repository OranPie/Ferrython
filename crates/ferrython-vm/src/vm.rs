//! The main virtual machine — executes bytecode instructions.
use crate::frame::{Frame, FramePool, SharedBuiltins};
use compact_str::CompactString;
use ferrython_bytecode::code::CodeFlags;
use ferrython_bytecode::opcode::{Instruction, Opcode};
use ferrython_core::error::{PyException, PyResult};
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_debug::{BreakpointManager, ExecutionProfiler};
use indexmap::IndexMap;
use std::cell::Cell;
use std::rc::Rc;

use crate::vm_fast_paths::{
    fast_deque_native_closure_returns_none, try_fast_builtin_setattr_stack,
    try_fast_global_builtin_call,
};

/// The Ferrython virtual machine.
pub struct VirtualMachine {
    pub(crate) call_stack: Vec<Frame>,
    pub(crate) builtins: SharedBuiltins,
    pub(crate) modules: IndexMap<CompactString, PyObjectRef>,
    /// Currently active exception being handled (for bare `raise` re-raise).
    pub(crate) active_exception: Option<PyException>,
    /// Previous active exceptions for nested except handlers.
    pub(crate) exception_state_stack: Vec<Option<PyException>>,
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
    /// Recursion depth for call dispatch paths that do not create Python frames.
    pub(crate) call_object_depth: Rc<Cell<usize>>,
}

impl VirtualMachine {
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
            // Re-check trace/profile state periodically.
            // When already active, check every 64 opcodes.
            // When inactive, check less frequently (every 256 opcodes) to detect
            // late-set trace functions (e.g. sys.settrace called during execution).
            if has_trace || has_profile {
                if trace_check_counter == 0 {
                    trace_check_counter = 63;
                    has_trace = ferrython_stdlib::is_trace_active();
                    has_profile = ferrython_stdlib::is_profile_active();
                } else {
                    trace_check_counter -= 1;
                }
            } else {
                if trace_check_counter == 0 {
                    trace_check_counter = 255;
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
                if ip >= frame.code.instructions.len() {
                    return Ok(PyObject::none());
                }
                let current_line = Self::ip_to_line(&frame.code, ip);
                let fire_line = current_line != last_line;
                if fire_line {
                    last_line = current_line;
                }
                self.call_stack.last_mut().unwrap().ip = ip + 1;
                if fire_line {
                    self.fire_trace_event("line", PyObject::none());
                }
                // Re-derive all cached pointers: fire_trace_event may call Python code
                rederive_frame!(self, frame_ptr, instr_base, instr_count);
            }

            // SAFETY: frame_ptr is re-derived after any call_stack modification.
            // Hot opcodes `continue` without modifying call_stack, keeping frame_ptr valid.
            let frame = unsafe { &mut *frame_ptr };

            let ip = frame.ip;
            let instr = if !has_trace {
                if ip >= instr_count {
                    return Ok(PyObject::none());
                }
                // SAFETY: bounds check above guarantees ip < instr_count
                let instr = unsafe { *instr_base.add(ip) };
                frame.ip = ip + 1;
                instr
            } else {
                // Tracing path already advanced ip above; read the previous instruction
                let prev_ip = ip.wrapping_sub(1);
                if prev_ip >= instr_count {
                    return Ok(PyObject::none());
                }
                unsafe { *instr_base.add(prev_ip) }
            };

            if profiling {
                self.profiler.start_instruction(instr.op);
            }

            // Inline the hottest opcodes to avoid execute_one dispatch overhead
            let result = match instr.op {
                Opcode::LoadFast
                | Opcode::StoreFast
                | Opcode::StoreFastJumpAbsolute
                | Opcode::LoadConst
                | Opcode::LoadFastLoadFast
                | Opcode::LoadFastLoadConst
                | Opcode::StoreFastLoadFast
                | Opcode::PopTop
                | Opcode::PopTopJumpAbsolute
                | Opcode::DupTop
                | Opcode::RotTwo
                | Opcode::LoadConstStoreFast => {
                    match crate::vm_fast_stack::try_fast_stack(
                        frame,
                        instr,
                        instr_base,
                        instr_count,
                    ) {
                        crate::vm_fast_stack::FastStackResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_stack::FastStackResult::UnboundLocal(idx) => {
                            Self::err_unbound_local(&frame.code.varnames, idx)
                        }
                        crate::vm_fast_stack::FastStackResult::Fallback => unreachable!(),
                    }
                }
                // ── Superinstructions: fused opcode pairs ──
                // 3-way superinstruction: LoadFast + LoadConst + BinarySubtract
                Opcode::LoadFastLoadConstBinarySub => {
                    match crate::vm_fast_binary::try_fast_fused_binary(frame, instr) {
                        crate::vm_fast_binary::FastFusedBinaryResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_binary::FastFusedBinaryResult::Fallback(op) => {
                            self.execute_one(ferrython_bytecode::Instruction::new(op, 0))
                        }
                        crate::vm_fast_binary::FastFusedBinaryResult::UnboundLocal(idx) => {
                            Self::err_unbound_local(&frame.code.varnames, idx)
                        }
                        crate::vm_fast_binary::FastFusedBinaryResult::HandledChain => {
                            self.execute_one(instr)
                        }
                    }
                }
                // 3-way superinstruction: LoadFast + LoadConst + BinaryAdd
                Opcode::LoadFastLoadConstBinaryAdd => {
                    match crate::vm_fast_binary::try_fast_fused_binary(frame, instr) {
                        crate::vm_fast_binary::FastFusedBinaryResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_binary::FastFusedBinaryResult::Fallback(op) => {
                            self.execute_one(ferrython_bytecode::Instruction::new(op, 0))
                        }
                        crate::vm_fast_binary::FastFusedBinaryResult::UnboundLocal(idx) => {
                            Self::err_unbound_local(&frame.code.varnames, idx)
                        }
                        crate::vm_fast_binary::FastFusedBinaryResult::HandledChain => {
                            self.execute_one(instr)
                        }
                    }
                }
                Opcode::LoadFastLoadFastBinaryAdd => {
                    match crate::vm_fast_binary::try_fast_fused_binary(frame, instr) {
                        crate::vm_fast_binary::FastFusedBinaryResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_binary::FastFusedBinaryResult::Fallback(op) => {
                            self.execute_one(ferrython_bytecode::Instruction::new(op, 0))
                        }
                        crate::vm_fast_binary::FastFusedBinaryResult::UnboundLocal(_) => {
                            Err(PyException::name_error(String::from(
                                "local variable referenced before assignment",
                            )))
                        }
                        crate::vm_fast_binary::FastFusedBinaryResult::HandledChain => {
                            self.execute_one(instr)
                        }
                    }
                }
                // 4-way fused: load two locals, add, store result — no stack touch
                Opcode::LoadFastLoadFastBinaryAddStoreFast => {
                    let fast_result = crate::vm_fast_binary::try_fast_fused_binary(frame, instr);
                    match fast_result {
                        crate::vm_fast_binary::FastFusedBinaryResult::HandledChain => {
                            hot_ok_chain!(
                                profiling,
                                self.profiler,
                                instr.op,
                                frame,
                                instr_base,
                                instr_count
                            )
                        }
                        _ => {}
                    }
                    if matches!(
                        fast_result,
                        crate::vm_fast_binary::FastFusedBinaryResult::UnboundLocal(_)
                    ) {
                        Err(PyException::name_error(String::from(
                            "local variable referenced before assignment",
                        )))
                    } else {
                        let dest = (instr.arg & 0xFF) as usize;
                        let r = self.execute_one(ferrython_bytecode::Instruction::new(
                            Opcode::BinaryAdd,
                            0,
                        ));
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
                // 4-way fused: load local + const, add, store — no stack touch
                Opcode::LoadFastLoadConstBinaryAddStoreFast => {
                    let fast_result = crate::vm_fast_binary::try_fast_fused_binary(frame, instr);
                    match fast_result {
                        crate::vm_fast_binary::FastFusedBinaryResult::Handled => {
                            hot_ok_chain!(
                                profiling,
                                self.profiler,
                                instr.op,
                                frame,
                                instr_base,
                                instr_count
                            )
                        }
                        _ => {}
                    }
                    if matches!(
                        fast_result,
                        crate::vm_fast_binary::FastFusedBinaryResult::UnboundLocal(_)
                    ) {
                        Err(PyException::name_error(String::from(
                            "local variable referenced before assignment",
                        )))
                    } else {
                        let dest = (instr.arg & 0xFF) as usize;
                        let r = self.execute_one(ferrython_bytecode::Instruction::new(
                            Opcode::BinaryAdd,
                            0,
                        ));
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
                // Fused LoadFast + LoadConst + BinaryMul + StoreFast (x = x * c)
                Opcode::LoadFastLoadConstBinaryMulStoreFast => {
                    let fast_result = crate::vm_fast_binary::try_fast_fused_binary(frame, instr);
                    match fast_result {
                        crate::vm_fast_binary::FastFusedBinaryResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_binary::FastFusedBinaryResult::UnboundLocal(_) => {
                            Err(PyException::name_error(String::from(
                                "local variable referenced before assignment",
                            )))
                        }
                        _ => {
                            let dest = (instr.arg & 0xFF) as usize;
                            let r = self.execute_one(ferrython_bytecode::Instruction::new(
                                Opcode::BinaryMultiply,
                                0,
                            ));
                            if r.is_ok() {
                                let cs_len2 = self.call_stack.len();
                                let frame2 =
                                    unsafe { self.call_stack.get_unchecked_mut(cs_len2 - 1) };
                                if !frame2.stack.is_empty() {
                                    let val = frame2.stack.pop().unwrap();
                                    unsafe { frame2.set_local_unchecked(dest, val) };
                                }
                            }
                            r
                        }
                    }
                }
                // Fused LoadFast + LoadConst + BinarySub + StoreFast (x = x - 1)
                Opcode::LoadFastLoadConstBinarySubStoreFast => {
                    let fast_result = crate::vm_fast_binary::try_fast_fused_binary(frame, instr);
                    match fast_result {
                        crate::vm_fast_binary::FastFusedBinaryResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_binary::FastFusedBinaryResult::UnboundLocal(_) => {
                            Err(PyException::name_error(String::from(
                                "local variable referenced before assignment",
                            )))
                        }
                        _ => {
                            let dest = (instr.arg & 0xFF) as usize;
                            let r = self.execute_one(ferrython_bytecode::Instruction::new(
                                Opcode::BinarySubtract,
                                0,
                            ));
                            if r.is_ok() {
                                let cs_len2 = self.call_stack.len();
                                let frame2 =
                                    unsafe { self.call_stack.get_unchecked_mut(cs_len2 - 1) };
                                if !frame2.stack.is_empty() {
                                    let val = frame2.stack.pop().unwrap();
                                    unsafe { frame2.set_local_unchecked(dest, val) };
                                }
                            }
                            r
                        }
                    }
                }
                // 6-way fused: x = (x * c1) % c2 — zero stack touch, in-place mutation
                Opcode::LoadFastMulModStoreFast => {
                    let fast_result = crate::vm_fast_binary::try_fast_fused_binary(frame, instr);
                    match fast_result {
                        crate::vm_fast_binary::FastFusedBinaryResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_binary::FastFusedBinaryResult::UnboundLocal(_) => {
                            Err(PyException::name_error(String::from(
                                "local variable referenced before assignment",
                            )))
                        }
                        _ => {
                            let const2_idx = ((instr.arg >> 8) & 0xFF) as usize;
                            let dest = (instr.arg & 0xFF) as usize;
                            let c2 = unsafe { frame.constant_cache.get_unchecked(const2_idx) };
                            let c2_clone = c2.clone();
                            let r = self.execute_one(ferrython_bytecode::Instruction::new(
                                Opcode::BinaryMultiply,
                                0,
                            ));
                            if r.is_ok() {
                                let cs_len2 = self.call_stack.len();
                                let frame2 =
                                    unsafe { self.call_stack.get_unchecked_mut(cs_len2 - 1) };
                                spush!(frame2, c2_clone);
                                let r2 = self.execute_one(ferrython_bytecode::Instruction::new(
                                    Opcode::BinaryModulo,
                                    0,
                                ));
                                if r2.is_ok() {
                                    let cs_len3 = self.call_stack.len();
                                    let frame3 =
                                        unsafe { self.call_stack.get_unchecked_mut(cs_len3 - 1) };
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
                // Fused LoadFast + LoadConst + BinaryMul (pushes result, no store)
                Opcode::LoadFastLoadConstBinaryMul => {
                    match crate::vm_fast_binary::try_fast_fused_binary(frame, instr) {
                        crate::vm_fast_binary::FastFusedBinaryResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_binary::FastFusedBinaryResult::Fallback(op) => {
                            self.execute_one(ferrython_bytecode::Instruction::new(op, 0))
                        }
                        crate::vm_fast_binary::FastFusedBinaryResult::UnboundLocal(_) => {
                            Err(PyException::name_error(String::from(
                                "local variable referenced before assignment",
                            )))
                        }
                        crate::vm_fast_binary::FastFusedBinaryResult::HandledChain => {
                            self.execute_one(instr)
                        }
                    }
                }
                Opcode::Nop
                | Opcode::JumpForward
                | Opcode::JumpAbsolute
                | Opcode::BeginFinally
                | Opcode::PopBlockJump => match crate::vm_fast_flow::try_fast_flow(frame, instr) {
                    crate::vm_fast_flow::FastFlowResult::Handled => {
                        hot_ok!(profiling, self.profiler, instr.op)
                    }
                    crate::vm_fast_flow::FastFlowResult::Fallback => unreachable!(),
                },
                // Fast GetIter for common iterable payloads.
                Opcode::GetIter => match crate::vm_fast_iter::try_fast_get_iter(frame) {
                    crate::vm_fast_iter::FastGetIterResult::Handled => {
                        hot_ok!(profiling, self.profiler, instr.op)
                    }
                    crate::vm_fast_iter::FastGetIterResult::Fallback => self.execute_one(instr),
                },
                // Inline ForIter for Range/List (hot in `for i in range(n)`)
                Opcode::ForIter => {
                    match crate::vm_fast_iter::try_fast_for_iter(
                        frame,
                        instr.arg as usize,
                        instr_base,
                        instr_count,
                    ) {
                        crate::vm_fast_iter::FastForIterResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_iter::FastForIterResult::HandledChain => {
                            hot_ok_chain!(
                                profiling,
                                self.profiler,
                                instr.op,
                                frame,
                                instr_base,
                                instr_count
                            )
                        }
                        crate::vm_fast_iter::FastForIterResult::Generator(gen_arc) => {
                            match self.resume_generator_for_iter(&gen_arc) {
                                Ok(Some(value)) => {
                                    rederive_frame!(self, frame_ptr, instr_base, instr_count);
                                    let frame = unsafe { &mut *frame_ptr };
                                    spush!(frame, value);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                Ok(None) => {
                                    rederive_frame!(self, frame_ptr, instr_base, instr_count);
                                    let frame = unsafe { &mut *frame_ptr };
                                    drop(spop!(frame));
                                    frame.ip = instr.arg as usize;
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                Err(e) => Err(e),
                            }
                        }
                        crate::vm_fast_iter::FastForIterResult::Fallback => self.execute_one(instr),
                        crate::vm_fast_iter::FastForIterResult::Error(error) => Err(error),
                    }
                }
                // ForIter + StoreFast fused: store directly to local, no stack push/pop
                Opcode::ForIterStoreFast => {
                    let jump_target = (instr.arg >> 16) as usize;
                    let store_idx = (instr.arg & 0xFFFF) as usize;
                    match crate::vm_fast_iter::try_fast_for_iter_store(
                        frame,
                        jump_target,
                        store_idx,
                    ) {
                        crate::vm_fast_iter::FastForIterStoreResult::HandledChain => {
                            hot_ok_chain!(
                                profiling,
                                self.profiler,
                                instr.op,
                                frame,
                                instr_base,
                                instr_count
                            )
                        }
                        crate::vm_fast_iter::FastForIterStoreResult::HandledChainDrainFinalizers => {
                            self.drain_pending_finalizers();
                            rederive_frame!(self, frame_ptr, instr_base, instr_count);
                            let frame = unsafe { &mut *frame_ptr };
                            hot_ok_chain!(
                                profiling,
                                self.profiler,
                                instr.op,
                                frame,
                                instr_base,
                                instr_count
                            )
                        }
                        crate::vm_fast_iter::FastForIterStoreResult::Generator(gen_arc) => {
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
                        }
                        crate::vm_fast_iter::FastForIterStoreResult::Fallback => {
                            // Fallback for non-iterator types
                            let for_instr = ferrython_bytecode::Instruction::new(
                                Opcode::ForIter,
                                jump_target as u32,
                            );
                            match self.execute_one(for_instr) {
                                Ok(_) => {
                                    let needs_drain = {
                                        let frame = self.call_stack.last_mut().unwrap();
                                        if frame.ip != jump_target {
                                            let v = spop!(frame);
                                            sset_local!(frame, store_idx, v);
                                            ferrython_core::error::has_pending_finalizers()
                                        } else {
                                            false
                                        }
                                    };
                                    if needs_drain {
                                        self.drain_pending_finalizers();
                                    }
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                Err(e) => Err(e),
                            }
                        }
                    }
                }
                Opcode::ReturnValue
                | Opcode::LoadFastReturnValue
                | Opcode::LoadConstReturnValue => {
                    match crate::vm_return::try_fast_return(frame, instr) {
                        crate::vm_return::FastReturnResult::Return(val) => Ok(Some(val)),
                        crate::vm_return::FastReturnResult::Fallback => self.execute_one(instr),
                        crate::vm_return::FastReturnResult::UnboundLocal(idx) => {
                            Self::err_unbound_local(&frame.code.varnames, idx)
                        }
                        crate::vm_return::FastReturnResult::Error(err) => Err(err),
                    }
                }

                Opcode::BinaryAdd
                | Opcode::InplaceAdd
                | Opcode::BinarySubtract
                | Opcode::InplaceSubtract
                | Opcode::BinaryMultiply
                | Opcode::InplaceMultiply
                | Opcode::BinaryModulo
                | Opcode::InplaceModulo
                | Opcode::BinaryFloorDivide
                | Opcode::InplaceFloorDivide
                | Opcode::BinaryTrueDivide
                | Opcode::InplaceTrueDivide => {
                    match crate::vm_fast_binary::try_fast_binary(frame, instr) {
                        crate::vm_fast_binary::FastBinaryResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_binary::FastBinaryResult::Fallback => {
                            self.execute_one(instr)
                        }
                    }
                }
                Opcode::CompareOp if instr.arg <= 9 => {
                    match crate::vm_fast_compare::try_fast_compare(frame, instr) {
                        crate::vm_fast_compare::FastCompareResult::Bool(result) => {
                            if instr.arg <= 5 {
                                cmp_jump_lookahead!(
                                    result,
                                    frame,
                                    instr_base,
                                    instr_count,
                                    profiling,
                                    self.profiler,
                                    instr.op
                                )
                            } else {
                                crate::vm_fast_compare::store_compare_bool(frame, result);
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                        }
                        crate::vm_fast_compare::FastCompareResult::Fallback => {
                            self.execute_one(instr)
                        }
                    }
                }
                // Inline LoadGlobal: check per-frame cache, then globals, then builtins.
                Opcode::LoadGlobal => {
                    match crate::vm_fast_names::try_fast_load_global(frame, instr.arg as usize) {
                        crate::vm_fast_names::FastNameResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_names::FastNameResult::Fallback => self.execute_one(instr),
                    }
                }
                // Inline PopJumpIfFalse for primitive types (hot in conditionals/loops)
                Opcode::PopJumpIfFalse => match crate::vm_fast_compare::try_fast_pop_jump(frame) {
                    crate::vm_fast_compare::FastPopJumpResult::Bool(truth) => {
                        if !truth {
                            frame.ip = instr.arg as usize;
                        }
                        hot_ok!(profiling, self.profiler, instr.op)
                    }
                    crate::vm_fast_compare::FastPopJumpResult::Fallback(value) => {
                        if !self.vm_is_truthy(&value)? {
                            let cs_len = self.call_stack.len();
                            unsafe { self.call_stack.get_unchecked_mut(cs_len - 1) }.ip =
                                instr.arg as usize;
                        }
                        hot_ok!(profiling, self.profiler, instr.op)
                    }
                },
                Opcode::PopJumpIfTrue => match crate::vm_fast_compare::try_fast_pop_jump(frame) {
                    crate::vm_fast_compare::FastPopJumpResult::Bool(truth) => {
                        if truth {
                            frame.ip = instr.arg as usize;
                        }
                        hot_ok!(profiling, self.profiler, instr.op)
                    }
                    crate::vm_fast_compare::FastPopJumpResult::Fallback(value) => {
                        if self.vm_is_truthy(&value)? {
                            let cs_len = self.call_stack.len();
                            unsafe { self.call_stack.get_unchecked_mut(cs_len - 1) }.ip =
                                instr.arg as usize;
                        }
                        hot_ok!(profiling, self.profiler, instr.op)
                    }
                },
                // Inline try/except block setup/teardown (very cheap, called every iteration in try loops)
                Opcode::SetupExcept
                | Opcode::SetupFinally
                | Opcode::PopBlock
                | Opcode::PopExcept => {
                    match crate::vm_fast_call::try_fast_block_control(frame, instr) {
                        crate::vm_fast_call::FastBlockResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_call::FastBlockResult::PopExcept => {
                            self.restore_previous_exception();
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_call::FastBlockResult::Fallback => unreachable!(),
                    }
                }
                // Inline RaiseVarargs(1) for the common case: raise ExceptionInstance
                Opcode::RaiseVarargs if instr.arg == 1 => {
                    if self.active_exception.is_some() {
                        self.exec_exception_ops(instr)
                    } else {
                        match crate::vm_fast_call::try_fast_raise_varargs_one(frame) {
                            crate::vm_fast_call::FastExceptionFlowResult::Error(err) => Err(err),
                            crate::vm_fast_call::FastExceptionFlowResult::PoppedFinallyNone => {
                                unreachable!()
                            }
                            crate::vm_fast_call::FastExceptionFlowResult::Fallback => {
                                self.exec_exception_ops(instr)
                            }
                        }
                    }
                }
                // EndFinally fast path: TOS is None → no exception, no pending return/jump
                Opcode::EndFinally => match crate::vm_fast_call::try_fast_end_finally_none(frame) {
                    crate::vm_fast_call::FastExceptionFlowResult::PoppedFinallyNone => {
                        hot_ok!(profiling, self.profiler, instr.op)
                    }
                    crate::vm_fast_call::FastExceptionFlowResult::Error(_) => unreachable!(),
                    crate::vm_fast_call::FastExceptionFlowResult::Fallback => {
                        self.execute_one(instr)
                    }
                },
                // Fast unary, power, and bitwise primitive paths.
                Opcode::UnaryNot
                | Opcode::UnaryNegative
                | Opcode::BinaryPower
                | Opcode::InplacePower
                | Opcode::BinaryAnd
                | Opcode::InplaceAnd
                | Opcode::BinaryOr
                | Opcode::InplaceOr
                | Opcode::BinaryXor
                | Opcode::InplaceXor
                | Opcode::BinaryLshift
                | Opcode::InplaceLshift
                | Opcode::BinaryRshift
                | Opcode::InplaceRshift => {
                    match crate::vm_fast_unary_bitwise::try_fast_unary_bitwise(frame, instr) {
                        crate::vm_fast_unary_bitwise::FastUnaryBitwiseResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_unary_bitwise::FastUnaryBitwiseResult::Fallback => {
                            self.execute_one(instr)
                        }
                    }
                }
                // Inline LoadDeref — lock-free closure variable fast path
                // SAFETY: single-threaded interpreter, no concurrent cell writes
                Opcode::LoadDeref => {
                    match crate::vm_fast_call::try_fast_load_deref(frame, instr.arg as usize) {
                        crate::vm_fast_call::FastDerefResult::Loaded => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_call::FastDerefResult::Fallback => self.execute_one(instr),
                        crate::vm_fast_call::FastDerefResult::Stored => unreachable!(),
                    }
                }
                // StoreDeref: lock-free write closure var
                Opcode::StoreDeref => {
                    match crate::vm_fast_call::fast_store_deref(frame, instr.arg as usize) {
                        crate::vm_fast_call::FastDerefResult::Stored => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        _ => unreachable!(),
                    }
                }
                // Fast builders, primitive f-string formatting, and sequence unpack.
                Opcode::BuildTuple
                | Opcode::BuildList
                | Opcode::FormatValue
                | Opcode::BuildString
                | Opcode::UnpackSequence => {
                    match crate::vm_fast_build::try_fast_build(
                        frame,
                        instr,
                        instr_base,
                        instr_count,
                    ) {
                        crate::vm_fast_build::FastBuildResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_build::FastBuildResult::ChainJump => {
                            hot_ok_chain!(
                                profiling,
                                self.profiler,
                                instr.op,
                                frame,
                                instr_base,
                                instr_count
                            )
                        }
                        crate::vm_fast_build::FastBuildResult::Fallback => self.execute_one(instr),
                    }
                }
                // Inline CallFunction fast path for simple Python function calls
                Opcode::CallFunction => {
                    let arg_count = instr.arg as usize;
                    let stack_len = frame.stack.len();
                    if stack_len <= arg_count {
                        // Stack too small — fall through to slow path
                        self.execute_one(instr)
                    } else {
                        let func_idx = stack_len - 1 - arg_count;
                        // Single payload check: determine both is_simple and is_recursive
                        // call_kind: 0=slow, 1=simple, 2=recursive, 3=trivial, 4=closure
                        let call_kind = if let PyObjectPayload::Function(pf) =
                            &sget!(frame, func_idx).payload
                        {
                            if pf.is_simple && pf.code.arg_count as usize == arg_count {
                                // Trivial function: body is just `LoadConst X; ReturnValue`
                                // or fused `LoadConstReturnValue X`
                                // Skip frame creation entirely — just push the constant.
                                if (pf.code.instructions.len() == 2
                                    && pf.code.instructions[0].op == Opcode::LoadConst
                                    && pf.code.instructions[1].op == Opcode::ReturnValue)
                                    || (pf.code.instructions.len() == 1
                                        && pf.code.instructions[0].op
                                            == Opcode::LoadConstReturnValue)
                                {
                                    3u8
                                } else if Rc::ptr_eq(&pf.code, &frame.code) {
                                    2u8
                                } else {
                                    1
                                }
                            } else if pf.code.arg_count as usize == arg_count
                                && pf.code.kwonlyarg_count == 0
                                && !pf.code.flags.contains(CodeFlags::VARARGS)
                                && !pf.code.flags.contains(CodeFlags::VARKEYWORDS)
                                && !pf.code.flags.contains(CodeFlags::GENERATOR)
                                && !pf.code.flags.contains(CodeFlags::COROUTINE)
                            {
                                4u8
                            }
                            // closure or cell function — fast path with cell setup
                            else {
                                0
                            }
                        } else {
                            0
                        };
                        let args_start = func_idx + 1;
                        let trace_active_now = ferrython_stdlib::is_trace_active()
                            || ferrython_stdlib::is_profile_active();
                        // Bound deque methods are stored as NativeClosure objects in locals in
                        // the CPython deque stress tests. These closures do not schedule VM
                        // callbacks, so bypass the generic call_object frame/deferred-call path.
                        if call_kind == 0 && !trace_active_now {
                            if let PyObjectPayload::NativeClosure(nc) =
                                &sget!(frame, func_idx).payload
                            {
                                if let Some(returns_none) = fast_deque_native_closure_returns_none(
                                    nc.name.as_str(),
                                    arg_count,
                                ) {
                                    let args: &[PyObjectRef] = if arg_count == 0 {
                                        &[]
                                    } else {
                                        std::slice::from_ref(sget!(frame, args_start))
                                    };
                                    let result = (nc.func)(args);
                                    unsafe {
                                        let base = frame.stack.as_ptr();
                                        for i in 0..=arg_count {
                                            let _obj = std::ptr::read(base.add(func_idx + i));
                                        }
                                        frame.stack.set_len(func_idx);
                                    }
                                    match result {
                                        Ok(result) => {
                                            if returns_none {
                                                chain_pop_none!(
                                                    frame,
                                                    instr_base,
                                                    instr_count,
                                                    profiling,
                                                    self.profiler,
                                                    instr.op
                                                )
                                            } else {
                                                spush!(frame, result);
                                                hot_ok!(profiling, self.profiler, instr.op)
                                            }
                                        }
                                        Err(e) => return Err(e),
                                    }
                                }
                            }
                        }
                        // Skip all mini-interpreter fast paths when tracing/profiling is active
                        if call_kind == 3 && !trace_active_now {
                            // Trivial function: inline the return constant
                            let const_idx = if let PyObjectPayload::Function(pf) =
                                &sget!(frame, func_idx).payload
                            {
                                pf.code.instructions[0].arg as usize
                            } else {
                                unreachable!()
                            };
                            let ret_val = if let PyObjectPayload::Function(pf) =
                                &sget!(frame, func_idx).payload
                            {
                                pf.constant_cache[const_idx].clone()
                            } else {
                                unreachable!()
                            };
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
                            let args: Vec<PyObjectRef> = frame.stack
                                [args_start..args_start + arg_count]
                                .iter()
                                .cloned()
                                .collect();
                            let mini_result = if let PyObjectPayload::Function(pf) =
                                &sget!(frame, func_idx).payload
                            {
                                match call_kind {
                                    1 if arg_count > 0 => {
                                        Self::try_inline_simple_function_args(pf, &args)
                                    }
                                    2 => Self::try_inline_recursive_base_case(
                                        &frame.code.instructions,
                                        &frame.constant_cache,
                                        &args,
                                    ),
                                    4 => Self::try_inline_closure_return(pf),
                                    _ => None,
                                }
                            } else {
                                None
                            };
                            if let Some(ret_val) = mini_result.filter(|_| !trace_active_now) {
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
                                    let (code, globals, constant_cache, closure_ptr, closure_len) =
                                        if let PyObjectPayload::Function(pf) =
                                            &sget!(frame, func_idx).payload
                                        {
                                            (
                                                Rc::clone(&pf.code),
                                                pf.globals.clone(),
                                                Rc::clone(&pf.constant_cache),
                                                pf.closure.as_ptr(),
                                                pf.closure.len(),
                                            )
                                        } else {
                                            unreachable!()
                                        };
                                    // SAFETY: closure ref valid while stack reference held
                                    let closure_ref = unsafe {
                                        std::slice::from_raw_parts(closure_ptr, closure_len)
                                    };
                                    let f = Frame::new_closure_from_pool(
                                        code,
                                        globals,
                                        self.builtins.clone(),
                                        constant_cache,
                                        closure_ref,
                                        &mut self.frame_pool,
                                    );
                                    f
                                } else if call_kind == 1 {
                                    // Borrowed path: zero refcount ops for frame creation.
                                    // Take func_obj from stack, borrow its Arc fields via ptr::read.
                                    unsafe {
                                        let func_obj: PyObjectRef =
                                            std::ptr::read(frame.stack.as_ptr().add(func_idx));
                                        let pf_ptr = match &func_obj.payload {
                                            PyObjectPayload::Function(pf) => {
                                                &**pf as *const ferrython_core::types::PyFunction
                                            }
                                            _ => std::hint::unreachable_unchecked(),
                                        };
                                        Frame::new_borrowed(
                                            &*pf_ptr,
                                            func_obj,
                                            &self.builtins,
                                            &mut self.frame_pool,
                                        )
                                    }
                                } else {
                                    // Normal path: clone Arcs from function object
                                    let (code, globals, constant_cache) =
                                        if let PyObjectPayload::Function(pf) =
                                            &sget!(frame, func_idx).payload
                                        {
                                            (
                                                Rc::clone(&pf.code),
                                                pf.globals.clone(),
                                                Rc::clone(&pf.constant_cache),
                                            )
                                        } else {
                                            unreachable!()
                                        };
                                    let mut f = Frame::new_from_pool(
                                        code,
                                        globals,
                                        self.builtins.clone(),
                                        constant_cache,
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
                                        new_frame.locals[i] =
                                            Some(std::ptr::read(base.add(args_start + i)));
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
                                    for (cell_idx, cell_name) in
                                        new_frame.code.cellvars.iter().enumerate()
                                    {
                                        for (var_idx, var_name) in
                                            new_frame.code.varnames.iter().enumerate()
                                        {
                                            if cell_name == var_name {
                                                if let Some(val) = new_frame.locals[var_idx].take()
                                                {
                                                    unsafe {
                                                        *new_frame.cells[cell_idx].data_ptr() =
                                                            Some(val)
                                                    };
                                                }
                                                break;
                                            }
                                        }
                                    }
                                }
                                self.call_stack.push(new_frame);
                                // Re-derive frame_ptr: push may reallocate Vec
                                rederive_frame!(self, frame_ptr, instr_base, instr_count);
                                if self.call_stack.len()
                                    > ferrython_stdlib::get_recursion_limit() as usize
                                {
                                    if let Some(frame) = self.call_stack.pop() {
                                        frame.recycle(&mut self.frame_pool);
                                    }
                                    Err(PyException::recursion_error(
                                        "maximum recursion depth exceeded",
                                    ))
                                } else {
                                    // Re-check trace/profile on every call (function calls are already expensive)
                                    has_trace = ferrython_stdlib::is_trace_active();
                                    has_profile = ferrython_stdlib::is_profile_active();
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
                            if !trace_active_now {
                                match crate::vm_fast_call::try_fast_instance_call(
                                    frame,
                                    &self.builtins,
                                    &mut self.frame_pool,
                                    func_idx,
                                    arg_count,
                                ) {
                                    crate::vm_fast_call::FastCallResult::NewFrame(new_frame) => {
                                        self.call_stack.push(new_frame);
                                        rederive_frame!(self, frame_ptr, instr_base, instr_count);
                                        if self.call_stack.len()
                                            > ferrython_stdlib::get_recursion_limit() as usize
                                        {
                                            if let Some(frame) = self.call_stack.pop() {
                                                frame.recycle(&mut self.frame_pool);
                                            }
                                            return Err(PyException::recursion_error(
                                                "maximum recursion depth exceeded",
                                            ));
                                        }
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    }
                                    crate::vm_fast_call::FastCallResult::Pushed => {
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    }
                                    crate::vm_fast_call::FastCallResult::Fallback => {}
                                }
                            }
                            match crate::vm_fast_call::try_fast_simple_class_call(
                                frame,
                                &self.builtins,
                                &mut self.frame_pool,
                                func_idx,
                                arg_count,
                            ) {
                                crate::vm_fast_call::FastCallResult::NewFrame(new_frame) => {
                                    self.call_stack.push(new_frame);
                                    rederive_frame!(self, frame_ptr, instr_base, instr_count);
                                    if self.call_stack.len()
                                        > ferrython_stdlib::get_recursion_limit() as usize
                                    {
                                        if let Some(frame) = self.call_stack.pop() {
                                            frame.recycle(&mut self.frame_pool);
                                        }
                                        Err(PyException::recursion_error(
                                            "maximum recursion depth exceeded",
                                        ))
                                    } else {
                                        hot_ok!(profiling, self.profiler, instr.op)
                                    }
                                }
                                crate::vm_fast_call::FastCallResult::Pushed => {
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                crate::vm_fast_call::FastCallResult::Fallback => {
                                    match crate::vm_fast_call::try_fast_exception_type_call(
                                        frame, func_idx, arg_count,
                                    ) {
                                        crate::vm_fast_call::FastCallResult::Pushed => {
                                            hot_ok!(profiling, self.profiler, instr.op)
                                        }
                                        crate::vm_fast_call::FastCallResult::NewFrame(_) => {
                                            unreachable!()
                                        }
                                        crate::vm_fast_call::FastCallResult::Fallback => {
                                            let func_obj = sget!(frame, func_idx);
                                            let fast_result = if matches!(
                                                (&func_obj.payload, arg_count),
                                                (PyObjectPayload::BuiltinFunction(name), 3)
                                                    if name.as_str() == "setattr"
                                            ) {
                                                if try_fast_builtin_setattr_stack(
                                                    &mut frame.stack,
                                                    func_idx,
                                                ) {
                                                    chain_pop_none!(
                                                        frame,
                                                        instr_base,
                                                        instr_count,
                                                        profiling,
                                                        self.profiler,
                                                        instr.op
                                                    )
                                                } else {
                                                    None
                                                }
                                            } else {
                                                crate::vm_fast_paths::try_fast_callfunction_builtin(
                                                    func_obj,
                                                    &frame.stack,
                                                    arg_count,
                                                )
                                            };
                                            if let Some(result) = fast_result {
                                                frame.stack.truncate(func_idx);
                                                spush!(frame, result);
                                                hot_ok!(profiling, self.profiler, instr.op)
                                            } else {
                                                self.execute_one(instr)
                                            }
                                        }
                                    }
                                }
                            }
                        } // close else for Class check
                    } // close stack guard
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
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(func_obj_ref) = func_ref {
                        let func_obj = func_obj_ref.clone();
                        // Skip all mini-interpreter fast paths when tracing/profiling is active
                        let trace_active_now = ferrython_stdlib::is_trace_active()
                            || ferrython_stdlib::is_profile_active();
                        match crate::vm_fast_call::try_fast_global_function_call(
                            frame,
                            &func_obj,
                            &self.builtins,
                            &mut self.frame_pool,
                            arg_count,
                            trace_active_now,
                            Self::try_inline_simple_function_args,
                            Self::try_inline_recursive_base_case,
                        ) {
                            crate::vm_fast_call::FastGlobalFunctionResult::Pushed => {
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            crate::vm_fast_call::FastGlobalFunctionResult::NewFrame(new_frame) => {
                                self.call_stack.push(new_frame);
                                // Re-derive frame_ptr: push may reallocate Vec
                                rederive_frame!(self, frame_ptr, instr_base, instr_count);
                                if self.call_stack.len()
                                    > ferrython_stdlib::get_recursion_limit() as usize
                                {
                                    if let Some(frame) = self.call_stack.pop() {
                                        frame.recycle(&mut self.frame_pool);
                                    }
                                    Err(PyException::recursion_error(
                                        "maximum recursion depth exceeded",
                                    ))
                                } else {
                                    has_trace = ferrython_stdlib::is_trace_active();
                                    has_profile = ferrython_stdlib::is_profile_active();
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
                            }
                            crate::vm_fast_call::FastGlobalFunctionResult::Fallback => {
                                if let Some(result) =
                                    try_fast_global_builtin_call(&func_obj, &frame.stack, arg_count)
                                {
                                    let stack_len = frame.stack.len();
                                    frame.stack.truncate(stack_len - arg_count);
                                    spush!(frame, result);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                } else {
                                    spush!(frame, func_obj);
                                    let call_instr =
                                        Instruction::new(Opcode::CallFunction, arg_count as u32);
                                    self.execute_one(call_instr)
                                }
                            }
                        }
                    } else {
                        // Cache miss — decompose to LoadGlobal + CallFunction
                        let load_instr = Instruction::new(Opcode::LoadGlobal, name_idx as u32);
                        let res = self.execute_one(load_instr)?;
                        if res.is_some() {
                            return Ok(res.unwrap());
                        }
                        let call_instr = Instruction::new(Opcode::CallFunction, arg_count as u32);
                        self.execute_one(call_instr)
                    }
                }
                Opcode::LoadMethod => match crate::vm_fast_attr::try_fast_attr(frame, instr) {
                    crate::vm_fast_attr::FastAttrResult::Handled => {
                        hot_ok!(profiling, self.profiler, instr.op)
                    }
                    crate::vm_fast_attr::FastAttrResult::Fallback => self.execute_one(instr),
                    crate::vm_fast_attr::FastAttrResult::UnboundLocal(_) => unreachable!(),
                },
                // Inline CallMethod super-fast path: two-item protocol + direct frame creation
                Opcode::CallMethod => {
                    let arg_count = instr.arg as usize;
                    let stack_len = frame.stack.len();
                    let base_idx = stack_len - arg_count - 2;
                    if let Some(new_frame) = crate::vm_fast_method::try_fast_python_method_frame(
                        frame,
                        &self.builtins,
                        &mut self.frame_pool,
                        arg_count,
                    ) {
                        self.call_stack.push(new_frame);
                        // Re-derive frame_ptr: push may reallocate Vec
                        rederive_frame!(self, frame_ptr, instr_base, instr_count);
                        if self.call_stack.len() > ferrython_stdlib::get_recursion_limit() as usize
                        {
                            if let Some(f) = self.call_stack.pop() {
                                f.recycle(&mut self.frame_pool);
                            }
                            Err(PyException::recursion_error(
                                "maximum recursion depth exceeded",
                            ))
                        } else {
                            // Iterative: continue loop with child frame (no recursive call)
                            has_trace = ferrython_stdlib::is_trace_active();
                            has_profile = ferrython_stdlib::is_profile_active();
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
                        let is_builtin_str =
                            matches!(&sget!(frame, base_idx).payload, PyObjectPayload::Str(_));
                        if is_builtin_str {
                            match crate::vm_fast_method::try_fast_builtin_method(frame, arg_count) {
                                crate::vm_fast_method::FastMethodResult::Handled(result) => {
                                    spush!(frame, result);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                crate::vm_fast_method::FastMethodResult::HandledNone => {
                                    chain_pop_none!(
                                        frame,
                                        instr_base,
                                        instr_count,
                                        profiling,
                                        self.profiler,
                                        instr.op
                                    )
                                }
                                crate::vm_fast_method::FastMethodResult::Fallback => {
                                    match self.call_builtin_method_fallback(frame, arg_count) {
                                        Ok(result) => {
                                            spush!(frame, result);
                                            hot_ok!(profiling, self.profiler, instr.op)
                                        }
                                        Err(err) => Err(err),
                                    }
                                }
                                crate::vm_fast_method::FastMethodResult::Error(err) => Err(err),
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
                    let is_builtin_str =
                        matches!(&sget!(frame, base_idx).payload, PyObjectPayload::Str(_));
                    if is_builtin_str {
                        match crate::vm_fast_method::try_fast_builtin_method_poptop(
                            frame, arg_count,
                        ) {
                            crate::vm_fast_method::FastMethodPopTopResult::Handled => {
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                            crate::vm_fast_method::FastMethodPopTopResult::HandledChain => {
                                hot_ok_chain!(
                                    profiling,
                                    self.profiler,
                                    instr.op,
                                    frame,
                                    instr_base,
                                    instr_count
                                )
                            }
                            crate::vm_fast_method::FastMethodPopTopResult::Fallback => {
                                match self.call_builtin_method_fallback(frame, arg_count) {
                                    Ok(_) => hot_ok!(profiling, self.profiler, instr.op),
                                    Err(e) => Err(e),
                                }
                            }
                            crate::vm_fast_method::FastMethodPopTopResult::Error(e) => Err(e),
                        }
                    } else {
                        let cm_instr =
                            ferrython_bytecode::Instruction::new(Opcode::CallMethod, instr.arg);
                        if let Some(new_frame) = crate::vm_fast_method::try_fast_python_method_frame(
                            frame,
                            &self.builtins,
                            &mut self.frame_pool,
                            arg_count,
                        ) {
                            self.call_stack.push(new_frame);
                            rederive_frame!(self, frame_ptr, instr_base, instr_count);
                            if self.call_stack.len()
                                > ferrython_stdlib::get_recursion_limit() as usize
                            {
                                if let Some(f) = self.call_stack.pop() {
                                    f.recycle(&mut self.frame_pool);
                                }
                                Err(PyException::recursion_error(
                                    "maximum recursion depth exceeded",
                                ))
                            } else {
                                hot_ok!(profiling, self.profiler, instr.op)
                            }
                        } else {
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
                                Err(e) => Err(e),
                            }
                        }
                    }
                }
                Opcode::BinarySubscr
                | Opcode::StoreSubscr
                | Opcode::ListAppend
                | Opcode::MapAdd
                | Opcode::SetAdd => {
                    match crate::vm_fast_collections::try_fast_collection(frame, instr) {
                        crate::vm_fast_collections::FastCollectionResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_collections::FastCollectionResult::Fallback => {
                            self.execute_one(instr)
                        }
                    }
                }
                // Fast attribute and method loads for simple instance/builtin paths.
                Opcode::LoadFastLoadAttr
                | Opcode::LoadFastLoadAttrStoreFast
                | Opcode::LoadFastLoadMethod
                | Opcode::LoadAttr => match crate::vm_fast_attr::try_fast_attr(frame, instr) {
                    crate::vm_fast_attr::FastAttrResult::Handled => {
                        hot_ok!(profiling, self.profiler, instr.op)
                    }
                    crate::vm_fast_attr::FastAttrResult::Fallback => {
                        let result = match instr.op {
                            Opcode::LoadFastLoadAttr => {
                                let name_idx = (instr.arg & 0xFFFF) as usize;
                                self.execute_one(Instruction::new(
                                    Opcode::LoadAttr,
                                    name_idx as u32,
                                ))
                            }
                            Opcode::LoadFastLoadAttrStoreFast => {
                                let name_idx = ((instr.arg >> 10) & 0x3FF) as usize;
                                let store_idx = (instr.arg & 0x3FF) as usize;
                                let result = self.execute_one(Instruction::new(
                                    Opcode::LoadAttr,
                                    name_idx as u32,
                                ));
                                if result.is_ok() {
                                    let cs_len = self.call_stack.len();
                                    let frame2 =
                                        unsafe { self.call_stack.get_unchecked_mut(cs_len - 1) };
                                    let value = spop!(frame2);
                                    sset_local!(frame2, store_idx, value);
                                }
                                result
                            }
                            Opcode::LoadFastLoadMethod => {
                                let name_idx = (instr.arg & 0xFFFF) as usize;
                                self.execute_one(Instruction::new(
                                    Opcode::LoadMethod,
                                    name_idx as u32,
                                ))
                            }
                            _ => self.execute_one(instr),
                        };
                        result
                    }
                    crate::vm_fast_attr::FastAttrResult::UnboundLocal(idx) => {
                        Self::err_unbound_local(&frame.code.varnames, idx)
                    }
                },
                // Fast module-scope name access.
                Opcode::LoadName => {
                    match crate::vm_fast_names::try_fast_load_name(frame, instr.arg as usize) {
                        crate::vm_fast_names::FastNameResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_names::FastNameResult::Fallback => self.execute_one(instr),
                    }
                }
                Opcode::StoreName => {
                    match crate::vm_fast_names::try_fast_store_name(frame, instr.arg as usize) {
                        crate::vm_fast_names::FastNameResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_names::FastNameResult::Fallback => self.execute_one(instr),
                    }
                }
                Opcode::StoreAttr => match crate::vm_fast_attr::try_fast_attr(frame, instr) {
                    crate::vm_fast_attr::FastAttrResult::Handled => {
                        hot_ok!(profiling, self.profiler, instr.op)
                    }
                    crate::vm_fast_attr::FastAttrResult::Fallback => self.execute_one(instr),
                    crate::vm_fast_attr::FastAttrResult::UnboundLocal(idx) => {
                        Self::err_unbound_local(&frame.code.varnames, idx)
                    }
                },
                // Compare + jump superinstructions avoid intermediate bool allocation.
                Opcode::CompareOpPopJumpIfFalse => {
                    let cmp_op = instr.arg >> 24;
                    let jump_target = (instr.arg & 0x00FF_FFFF) as usize;
                    match crate::vm_fast_compare::try_fast_compare_jump(frame, instr) {
                        crate::vm_fast_compare::FastCompareJumpResult::Bool(is_true) => {
                            if !is_true {
                                frame.ip = jump_target;
                            }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_compare::FastCompareJumpResult::Fallback => {
                            match self.fallback_compare_jump(cmp_op, jump_target) {
                                Ok(()) => hot_ok!(profiling, self.profiler, instr.op),
                                Err(e) => Err(e),
                            }
                        }
                        crate::vm_fast_compare::FastCompareJumpResult::UnboundLocal(idx) => {
                            Self::err_unbound_local(&frame.code.varnames, idx)
                        }
                    }
                }
                Opcode::LoadFastCompareConstJump => {
                    let cmp_op = instr.arg >> 28;
                    let jump_target = (instr.arg & 0xFFF) as usize;
                    match crate::vm_fast_compare::try_fast_compare_jump(frame, instr) {
                        crate::vm_fast_compare::FastCompareJumpResult::Bool(is_true) => {
                            if !is_true {
                                frame.ip = jump_target;
                            }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_compare::FastCompareJumpResult::Fallback => {
                            match self.fallback_compare_jump(cmp_op, jump_target) {
                                Ok(()) => hot_ok!(profiling, self.profiler, instr.op),
                                Err(e) => Err(e),
                            }
                        }
                        crate::vm_fast_compare::FastCompareJumpResult::UnboundLocal(idx) => {
                            Self::err_unbound_local(&frame.code.varnames, idx)
                        }
                    }
                }
                Opcode::LoadFastLoadFastCompareJump => {
                    let cmp_op = instr.arg >> 28;
                    let jump_target = (instr.arg & 0xFFF) as usize;
                    match crate::vm_fast_compare::try_fast_compare_jump(frame, instr) {
                        crate::vm_fast_compare::FastCompareJumpResult::Bool(is_true) => {
                            if !is_true {
                                frame.ip = jump_target;
                            }
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_compare::FastCompareJumpResult::Fallback => {
                            match self.fallback_compare_jump(cmp_op, jump_target) {
                                Ok(()) => hot_ok!(profiling, self.profiler, instr.op),
                                Err(e) => Err(e),
                            }
                        }
                        crate::vm_fast_compare::FastCompareJumpResult::UnboundLocal(idx) => {
                            Self::err_unbound_local(&frame.code.varnames, idx)
                        }
                    }
                }
                // Fused LoadGlobal + StoreFast: stores global directly to local.
                Opcode::LoadGlobalStoreFast => {
                    match crate::vm_fast_names::try_fast_load_global_store_fast(frame, instr.arg) {
                        crate::vm_fast_names::FastGlobalStoreResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_names::FastGlobalStoreResult::Fallback {
                            name_idx,
                            store_idx,
                        } => {
                            let load_instr = Instruction::new(Opcode::LoadGlobal, name_idx as u32);
                            match self.execute_one(load_instr) {
                                Ok(_) => {
                                    let frame = self.call_stack.last_mut().unwrap();
                                    let value = spop!(frame);
                                    sset_local!(frame, store_idx, value);
                                    hot_ok!(profiling, self.profiler, instr.op)
                                }
                                Err(e) => Err(e),
                            }
                        }
                    }
                }
                Opcode::LoadConstLoadFastContainsStoreFast => {
                    match crate::vm_fast_collections::try_fast_fused_collection(frame, instr) {
                        crate::vm_fast_collections::FastFusedCollectionResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_collections::FastFusedCollectionResult::UnboundLocal(
                            idx,
                        ) => Self::err_unbound_local(&frame.code.varnames, idx),
                        _ => match self.fallback_fused_collection(instr) {
                            Ok(()) => hot_ok!(profiling, self.profiler, instr.op),
                            Err(e) => Err(e),
                        },
                    }
                }
                Opcode::LoadFastLoadConstSubscrStoreFast => {
                    match crate::vm_fast_collections::try_fast_fused_collection(frame, instr) {
                        crate::vm_fast_collections::FastFusedCollectionResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_collections::FastFusedCollectionResult::UnboundLocal(
                            idx,
                        ) => Self::err_unbound_local(&frame.code.varnames, idx),
                        _ => match self.fallback_fused_collection(instr) {
                            Ok(()) => hot_ok!(profiling, self.profiler, instr.op),
                            Err(e) => Err(e),
                        },
                    }
                }
                Opcode::LoadFastLoadFastSubscrStoreFast => {
                    match crate::vm_fast_collections::try_fast_fused_collection(frame, instr) {
                        crate::vm_fast_collections::FastFusedCollectionResult::HandledChain => {
                            hot_ok_chain!(
                                profiling,
                                self.profiler,
                                instr.op,
                                frame,
                                instr_base,
                                instr_count
                            )
                        }
                        crate::vm_fast_collections::FastFusedCollectionResult::UnboundLocal(
                            idx,
                        ) => Self::err_unbound_local(&frame.code.varnames, idx),
                        _ => match self.fallback_fused_collection(instr) {
                            Ok(()) => hot_ok!(profiling, self.profiler, instr.op),
                            Err(e) => Err(e),
                        },
                    }
                }
                Opcode::LoadFastLoadFastLoadFastStoreSubscr => {
                    match crate::vm_fast_collections::try_fast_fused_collection(frame, instr) {
                        crate::vm_fast_collections::FastFusedCollectionResult::Handled => {
                            hot_ok!(profiling, self.profiler, instr.op)
                        }
                        crate::vm_fast_collections::FastFusedCollectionResult::UnboundLocal(
                            idx,
                        ) => Self::err_unbound_local(&frame.code.varnames, idx),
                        _ => match self.fallback_fused_collection(instr) {
                            Ok(()) => hot_ok!(profiling, self.profiler, instr.op),
                            Err(e) => Err(e),
                        },
                    }
                }
                Opcode::LoadFastLoadFastContainsStoreFast => {
                    match crate::vm_fast_collections::try_fast_fused_collection(frame, instr) {
                        crate::vm_fast_collections::FastFusedCollectionResult::HandledChain => {
                            hot_ok_chain!(
                                profiling,
                                self.profiler,
                                instr.op,
                                frame,
                                instr_base,
                                instr_count
                            )
                        }
                        crate::vm_fast_collections::FastFusedCollectionResult::UnboundLocal(
                            idx,
                        ) => Self::err_unbound_local(&frame.code.varnames, idx),
                        _ => match self.fallback_fused_collection(instr) {
                            Ok(()) => hot_ok!(profiling, self.profiler, instr.op),
                            Err(e) => Err(e),
                        },
                    }
                }
                _ => self.execute_one(instr),
            };

            match result {
                Ok(Some(ret)) => {
                    if profiling {
                        self.profiler.end_instruction(instr.op);
                    }
                    // Iterative call/return: if we're deeper than initial_depth,
                    // we're returning from a child frame pushed by inline
                    // CallFunction/CallMethod — pop it and push result to parent.
                    if self.call_stack.len() > initial_depth {
                        // Re-check trace/profile on return (detects late-set functions)
                        has_trace = ferrython_stdlib::is_trace_active();
                        has_profile = ferrython_stdlib::is_profile_active();
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
                        let caller_op = parent
                            .code
                            .instructions
                            .get(parent.ip.wrapping_sub(1))
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
                    if profiling {
                        self.profiler.end_instruction(instr.op);
                    }
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
                            if let Some(ctx_exc) = Self::context_for_raise(active, &exc) {
                                exc.context = Some(Box::new(ctx_exc));
                            }
                        }
                    }
                    // Iterative exception unwind: try current frame, then parents
                    loop {
                        if let Some(handler_ip) = self.unwind_except() {
                            // Attach traceback from call stack if not already present
                            if exc.traceback.is_empty() {
                                self.attach_traceback(&mut exc);
                            } else if let Some(frame) = self.call_stack.last() {
                                let parent_name = frame.code.name.as_str();
                                let first_name =
                                    exc.traceback.first().map(|entry| entry.function.as_str());
                                if first_name != Some(parent_name) {
                                    let mut parent_exc =
                                        PyException::new(exc.kind, exc.message.clone());
                                    Self::push_frame_traceback_entry(&mut parent_exc, frame);
                                    parent_exc.traceback.extend(exc.traceback.drain(..));
                                    exc.traceback = parent_exc.traceback;
                                }
                            }
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
                                    PyObject::exception_instance_with_args(
                                        exc_kind,
                                        exc.message.clone(),
                                        vec![val.clone()],
                                    )
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
                                Self::store_exc_attr(
                                    &exc_value,
                                    "errno",
                                    PyObject::int(info.errno as i64),
                                );
                                Self::store_exc_attr(
                                    &exc_value,
                                    "strerror",
                                    PyObject::str_val(CompactString::from(info.strerror.as_str())),
                                );
                                if let Some(fname) = &info.filename {
                                    Self::store_exc_attr(
                                        &exc_value,
                                        "filename",
                                        PyObject::str_val(CompactString::from(fname.as_str())),
                                    );
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
                                Self::store_exc_attr(
                                    &exc_value,
                                    "__suppress_context__",
                                    PyObject::bool_val(true),
                                );
                            }
                            let existing_context = Self::stored_exc_attr(&exc_value, "__context__");
                            if let Some(ctx) = &exc.context {
                                let ctx_obj = if let Some(corig) = &ctx.original {
                                    corig.clone()
                                } else {
                                    PyObject::exception_instance(ctx.kind, ctx.message.clone())
                                };
                                if let Some(existing) = existing_context.as_ref() {
                                    if !PyObjectRef::ptr_eq(existing, &ctx_obj) {
                                        // Python code such as contextlib.ExitStack may have
                                        // already repaired the visible exception chain before
                                        // re-raising the same object. Preserve that explicit
                                        // object-level chain instead of restoring the VM's stale
                                        // active-exception snapshot.
                                    } else {
                                        Self::store_exc_attr(&exc_value, "__context__", ctx_obj);
                                    }
                                } else {
                                    Self::store_exc_attr(&exc_value, "__context__", ctx_obj);
                                }
                            }
                            let existing_traceback =
                                Self::stored_exc_attr(&exc_value, "__traceback__")
                                    .filter(|tb| !matches!(&tb.payload, PyObjectPayload::None))
                                    .unwrap_or_else(PyObject::none);
                            // Build traceback object and attach to exception value.  A
                            // traceback set via with_traceback() becomes the tail of the
                            // newly raised frame chain.
                            let tb_obj = if !exc.traceback.is_empty() {
                                Self::build_traceback_object_with_tail(
                                    &exc.traceback,
                                    existing_traceback,
                                )
                            } else {
                                existing_traceback
                            };
                            Self::store_exc_attr(&exc_value, "__traceback__", tb_obj.clone());
                            if let Some(frame_obj) = tb_obj.get_attr("tb_frame") {
                                ferrython_stdlib::set_current_frame(Some(frame_obj));
                            }
                            // Ensure exc.original points to the same exc_value so that
                            // sys.exc_info() can retrieve __traceback__ later.
                            exc.original = Some(exc_value.clone());
                            let frame = self.call_stack.last_mut().unwrap();
                            frame.push(tb_obj);
                            frame.push(exc_value); // value
                            frame.push(exc_type); // type
                            frame.ip = handler_ip;
                            self.enter_exception_handler(exc);
                            // Re-derive frame_ptr: exception unwind may have popped frames
                            rederive_frame!(self, frame_ptr, instr_base, instr_count);
                            break; // handler found, continue main loop
                        }
                        // No handler in current frame — unwind iteratively
                        if self.call_stack.len() > initial_depth {
                            if let Some(child) = self.call_stack.pop() {
                                if exc.traceback.is_empty() {
                                    Self::push_frame_traceback_entry(&mut exc, &child);
                                }
                                Self::keep_frame_objects_alive(&mut exc, &child);
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
}

impl Default for VirtualMachine {
    fn default() -> Self {
        Self::new()
    }
}
