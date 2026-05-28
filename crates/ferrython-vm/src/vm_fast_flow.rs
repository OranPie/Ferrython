//! Fast trivial control-flow helpers for the VM dispatch loop.

use crate::frame::Frame;
use ferrython_bytecode::{Instruction, Opcode};
use ferrython_core::object::PyObject;

pub(crate) enum FastFlowResult {
    Handled,
    Fallback,
}

#[inline(always)]
pub(crate) fn try_fast_flow(frame: &mut Frame, instr: Instruction) -> FastFlowResult {
    match instr.op {
        Opcode::Nop => FastFlowResult::Handled,
        Opcode::JumpForward | Opcode::JumpAbsolute => {
            frame.ip = instr.arg as usize;
            FastFlowResult::Handled
        }
        Opcode::BeginFinally => {
            unsafe { frame.push_unchecked(PyObject::none()) };
            FastFlowResult::Handled
        }
        Opcode::PopBlockJump => {
            frame.pop_block();
            frame.ip = instr.arg as usize;
            FastFlowResult::Handled
        }
        _ => FastFlowResult::Fallback,
    }
}
