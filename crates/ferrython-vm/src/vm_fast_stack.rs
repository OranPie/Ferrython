//! Fast local/constant/stack opcode helpers for the VM dispatch loop.

use crate::frame::Frame;
use ferrython_bytecode::{Instruction, Opcode};
use ferrython_core::object::{PyObjectPayload, PyObjectRef};

pub(crate) enum FastStackResult {
    Handled,
    Fallback,
    UnboundLocal(usize),
}

#[inline(always)]
pub(crate) fn try_fast_stack(
    frame: &mut Frame,
    instr: Instruction,
    instr_base: *const Instruction,
    instr_count: usize,
) -> FastStackResult {
    match instr.op {
        Opcode::LoadFast => load_fast(frame, instr.arg as usize),
        Opcode::StoreFast => store_fast(frame, instr.arg as usize, instr_base, instr_count),
        Opcode::StoreFastJumpAbsolute => store_fast_jump(frame, instr.arg),
        Opcode::LoadConst => load_const(frame, instr.arg as usize),
        Opcode::LoadFastLoadFast => load_fast_load_fast(frame, instr.arg),
        Opcode::LoadFastLoadConst => load_fast_load_const(frame, instr.arg),
        Opcode::StoreFastLoadFast => store_fast_load_fast(frame, instr.arg),
        Opcode::PopTop => {
            drop(pop(frame));
            FastStackResult::Handled
        }
        Opcode::PopTopJumpAbsolute => {
            drop(pop(frame));
            frame.ip = instr.arg as usize;
            FastStackResult::Handled
        }
        Opcode::DupTop => {
            let v = unsafe { frame.peek_unchecked() }.clone();
            push(frame, v);
            FastStackResult::Handled
        }
        Opcode::RotTwo => {
            let len = frame.stack.len();
            unsafe {
                frame
                    .stack
                    .as_mut_ptr()
                    .add(len - 1)
                    .swap(frame.stack.as_mut_ptr().add(len - 2))
            };
            FastStackResult::Handled
        }
        Opcode::LoadConstStoreFast => load_const_store_fast(frame, instr.arg),
        _ => FastStackResult::Fallback,
    }
}

#[inline(always)]
fn push(frame: &mut Frame, value: PyObjectRef) {
    unsafe { frame.push_unchecked(value) };
}

#[inline(always)]
fn pop(frame: &mut Frame) -> PyObjectRef {
    frame.stack.pop().expect("stack underflow")
}

#[inline(always)]
fn local_ref(frame: &Frame, idx: usize) -> Option<&PyObjectRef> {
    unsafe { frame.locals.get_unchecked(idx).as_ref() }
}

#[inline(always)]
fn set_local(frame: &mut Frame, idx: usize, value: PyObjectRef) {
    frame.set_local(idx, value);
}

#[inline(always)]
fn load_fast(frame: &mut Frame, idx: usize) -> FastStackResult {
    match local_ref(frame, idx) {
        Some(val) => {
            push(frame, val.clone());
            FastStackResult::Handled
        }
        None => FastStackResult::UnboundLocal(idx),
    }
}

#[inline(always)]
fn store_fast(
    frame: &mut Frame,
    idx: usize,
    instr_base: *const Instruction,
    instr_count: usize,
) -> FastStackResult {
    let val = pop(frame);
    set_local(frame, idx, val);
    chain_jump(frame, instr_base, instr_count);
    FastStackResult::Handled
}

#[inline(always)]
fn store_fast_jump(frame: &mut Frame, arg: u32) -> FastStackResult {
    let store_idx = (arg >> 16) as usize;
    let jump_target = (arg & 0xFFFF) as usize;
    let val = pop(frame);
    set_local(frame, store_idx, val);
    frame.ip = jump_target;
    FastStackResult::Handled
}

#[inline(always)]
fn load_const(frame: &mut Frame, idx: usize) -> FastStackResult {
    let obj = unsafe { frame.constant_cache.get_unchecked(idx).clone() };
    push(frame, obj);
    FastStackResult::Handled
}

#[inline(always)]
fn load_fast_load_fast(frame: &mut Frame, arg: u32) -> FastStackResult {
    let idx1 = (arg >> 16) as usize;
    let idx2 = (arg & 0xFFFF) as usize;
    let a = local_ref(frame, idx1).cloned();
    let b = local_ref(frame, idx2).cloned();
    match (a, b) {
        (Some(a), Some(b)) => {
            push(frame, a);
            push(frame, b);
            FastStackResult::Handled
        }
        (None, _) => FastStackResult::UnboundLocal(idx1),
        (_, None) => FastStackResult::UnboundLocal(idx2),
    }
}

#[inline(always)]
fn load_fast_load_const(frame: &mut Frame, arg: u32) -> FastStackResult {
    let fast_idx = (arg >> 16) as usize;
    let const_idx = (arg & 0xFFFF) as usize;
    match local_ref(frame, fast_idx).cloned() {
        Some(val) => {
            push(frame, val);
            let c = unsafe { frame.constant_cache.get_unchecked(const_idx) }.clone();
            push(frame, c);
            FastStackResult::Handled
        }
        None => FastStackResult::UnboundLocal(fast_idx),
    }
}

#[inline(always)]
fn store_fast_load_fast(frame: &mut Frame, arg: u32) -> FastStackResult {
    let store_idx = (arg >> 16) as usize;
    let load_idx = (arg & 0xFFFF) as usize;
    let val = pop(frame);
    set_local(frame, store_idx, val);
    match local_ref(frame, load_idx).cloned() {
        Some(val) => {
            push(frame, val);
            FastStackResult::Handled
        }
        None => FastStackResult::UnboundLocal(load_idx),
    }
}

#[inline(always)]
fn load_const_store_fast(frame: &mut Frame, arg: u32) -> FastStackResult {
    let const_idx = (arg >> 16) as usize;
    let store_idx = (arg & 0xFFFF) as usize;
    let const_ref = unsafe { frame.constant_cache.get_unchecked(const_idx) };
    let stores_cellvar = frame
        .code
        .varnames
        .get(store_idx)
        .map(|name| frame.code.cellvars.iter().any(|cell| cell == name))
        .unwrap_or(false);
    if stores_cellvar {
        frame.set_local(store_idx, const_ref.clone());
        return FastStackResult::Handled;
    }
    let dest_slot = unsafe { frame.locals.get_unchecked_mut(store_idx) };
    if let Some(ref mut arc) = dest_slot {
        if let Some(obj) = PyObjectRef::get_mut(arc) {
            match (&const_ref.payload, &mut obj.payload) {
                (PyObjectPayload::Int(src), PyObjectPayload::Int(dst)) => {
                    *dst = src.clone();
                    return FastStackResult::Handled;
                }
                (PyObjectPayload::Bool(src), PyObjectPayload::Bool(dst)) => {
                    *dst = *src;
                    return FastStackResult::Handled;
                }
                (PyObjectPayload::None, PyObjectPayload::None) => {
                    return FastStackResult::Handled;
                }
                (PyObjectPayload::Float(src), PyObjectPayload::Float(dst)) => {
                    *dst = *src;
                    return FastStackResult::Handled;
                }
                _ => {}
            }
        }
    }
    frame.set_local(store_idx, const_ref.clone());
    FastStackResult::Handled
}

#[inline(always)]
fn chain_jump(frame: &mut Frame, instr_base: *const Instruction, instr_count: usize) {
    let next_ip = frame.ip;
    if next_ip < instr_count {
        let next = unsafe { *instr_base.add(next_ip) };
        if next.op == Opcode::JumpAbsolute {
            frame.ip = next.arg as usize;
        }
    }
}
