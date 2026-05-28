//! Fast return opcode helpers for the VM dispatch loop.

use crate::frame::{BlockKind, Frame};
use ferrython_bytecode::opcode::{Instruction, Opcode};
use ferrython_core::error::PyException;
use ferrython_core::object::{PyObjectPayload, PyObjectRef};

pub(crate) enum FastReturnResult {
    Return(PyObjectRef),
    Fallback,
    UnboundLocal(usize),
    Error(PyException),
}

#[inline(always)]
pub(crate) fn try_fast_return(frame: &mut Frame, instr: Instruction) -> FastReturnResult {
    match instr.op {
        Opcode::ReturnValue => return_value(frame),
        Opcode::LoadFastReturnValue => load_fast_return_value(frame, instr.arg as usize),
        Opcode::LoadConstReturnValue => load_const_return_value(frame, instr.arg as usize),
        _ => FastReturnResult::Fallback,
    }
}

#[inline(always)]
fn return_value(frame: &mut Frame) -> FastReturnResult {
    if frame
        .block_stack
        .iter()
        .any(|b| b.kind() == BlockKind::Finally)
    {
        return FastReturnResult::Fallback;
    }

    let val = frame.stack.pop().expect("stack underflow");
    validate_return(frame, val)
}

#[inline(always)]
fn load_fast_return_value(frame: &Frame, idx: usize) -> FastReturnResult {
    if !frame.block_stack.is_empty() {
        return FastReturnResult::Fallback;
    }

    let Some(val) = (unsafe { frame.locals.get_unchecked(idx).as_ref() }) else {
        return FastReturnResult::UnboundLocal(idx);
    };
    validate_return(frame, val.clone())
}

#[inline(always)]
fn load_const_return_value(frame: &Frame, idx: usize) -> FastReturnResult {
    if !frame.block_stack.is_empty() {
        return FastReturnResult::Fallback;
    }

    let val = unsafe { frame.constant_cache.get_unchecked(idx).clone() };
    validate_return(frame, val)
}

#[inline(always)]
fn validate_return(frame: &Frame, val: PyObjectRef) -> FastReturnResult {
    if frame.discard_return && !matches!(&val.payload, PyObjectPayload::None) {
        FastReturnResult::Error(PyException::type_error(
            "__init__() should return None".to_string(),
        ))
    } else {
        FastReturnResult::Return(val)
    }
}
