/// Push to frame.stack — grows the stack if capacity is reached.
macro_rules! spush {
    ($frame:expr, $val:expr) => {
        #[allow(unused_unsafe)]
        unsafe {
            let stack = &mut $frame.stack;
            let val = $val;
            if stack.len() < stack.capacity() {
                let len = stack.len();
                std::ptr::write(stack.as_mut_ptr().add(len), val);
                stack.set_len(len + 1);
            } else {
                stack.push(val);
            }
        }
    };
}

/// Pop from frame.stack — panics if empty.
macro_rules! spop {
    ($frame:expr) => {
        $frame.stack.pop().expect("stack underflow")
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
        if $profiling {
            $profiler.end_instruction($op);
        }
        continue;
    }};
}

/// Instruction chaining: if the next instruction is JumpAbsolute, consume it inline.
/// Saves one dispatch cycle per for-loop iteration.
macro_rules! chain_jump {
    ($frame:expr, $instr_base:expr, $instr_count:expr) => {
        let next_ip = $frame.ip;
        if next_ip < $instr_count {
            #[allow(unused_unsafe)]
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
        if $profiling {
            $profiler.end_instruction($op);
        }
        continue;
    }};
}

/// Re-derive frame_ptr, instr_base, instr_count after call_stack modification.
/// SAFETY: call_stack must be non-empty.
macro_rules! rederive_frame {
    ($self_:expr, $frame_ptr:expr, $instr_base:expr, $instr_count:expr) => {
        unsafe {
            $frame_ptr = $self_
                .call_stack
                .as_mut_ptr()
                .add($self_.call_stack.len() - 1);
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
                if $profiling {
                    $profiler.end_instruction($op);
                }
                continue;
            }
        }
        spush!($frame, PyObject::none());
        if $profiling {
            $profiler.end_instruction($op);
        }
        continue;
    }};
}

/// Compare look-ahead: if a CompareOp produces a boolean and the next instruction
/// is PopJumpIfFalse or PopJumpIfTrue, skip the intermediate bool allocation and
/// jump/fall-through directly. Avoids: 1 PyObjectRef alloc, 1 push, 1 dispatch, 1 pop.
/// This is a runtime optimization — the bytecode stream stays standard CPython 3.8.
macro_rules! cmp_jump_lookahead {
    ($result:expr, $frame:expr, $instr_base:expr, $instr_count:expr, $profiling:expr, $profiler:expr, $op:expr) => {{
        let next_ip = $frame.ip;
        if next_ip < $instr_count {
            let ni = unsafe { *$instr_base.add(next_ip) };
            if ni.op == Opcode::PopJumpIfFalse {
                // Drop both operands, skip bool creation, just jump if false
                let len = $frame.stack.len();
                unsafe {
                    let _a = std::ptr::read($frame.stack.as_ptr().add(len - 2));
                    let _b = std::ptr::read($frame.stack.as_ptr().add(len - 1));
                    $frame.stack.set_len(len - 2);
                }
                if !$result {
                    $frame.ip = ni.arg as usize;
                } else {
                    $frame.ip = next_ip + 1;
                }
                if $profiling {
                    $profiler.end_instruction($op);
                }
                continue;
            } else if ni.op == Opcode::PopJumpIfTrue {
                let len = $frame.stack.len();
                unsafe {
                    let _a = std::ptr::read($frame.stack.as_ptr().add(len - 2));
                    let _b = std::ptr::read($frame.stack.as_ptr().add(len - 1));
                    $frame.stack.set_len(len - 2);
                }
                if $result {
                    $frame.ip = ni.arg as usize;
                } else {
                    $frame.ip = next_ip + 1;
                }
                if $profiling {
                    $profiler.end_instruction($op);
                }
                continue;
            }
        }
        // No look-ahead match: fall through to normal bool push
        unsafe { $frame.binary_op_result(PyObject::bool_val($result)) };
        if $profiling {
            $profiler.end_instruction($op);
        }
        continue;
    }};
}
