//! Fast builders, formatting, and sequence-unpack helpers for the VM dispatch loop.

use crate::frame::Frame;
use compact_str::CompactString;
use ferrython_bytecode::code::ConstantValue;
use ferrython_bytecode::{Instruction, Opcode};
use ferrython_core::object::{alloc_tuple_box_empty, PyObject, PyObjectPayload, PyObjectRef};
use ferrython_core::types::PyInt;

pub(crate) enum FastBuildResult {
    Handled,
    Fallback,
    ChainJump,
}

#[inline(always)]
pub(crate) fn try_fast_build(
    frame: &mut Frame,
    instr: Instruction,
    instr_base: *const Instruction,
    instr_count: usize,
) -> FastBuildResult {
    match instr.op {
        Opcode::BuildTuple => build_tuple(frame, instr.arg as usize),
        Opcode::BuildList => build_list(frame, instr.arg as usize),
        Opcode::FormatValue => format_value(frame, instr),
        Opcode::BuildString => build_string(frame, instr.arg as usize),
        Opcode::UnpackSequence => {
            unpack_sequence(frame, instr.arg as usize, instr_base, instr_count)
        }
        _ => FastBuildResult::Fallback,
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
fn build_tuple(frame: &mut Frame, count: usize) -> FastBuildResult {
    match count {
        0 => push(frame, PyObject::tuple(vec![])),
        1 => {
            let a = pop(frame);
            let mut tb = alloc_tuple_box_empty();
            tb.push(a);
            push(frame, PyObject::wrap_leaf(PyObjectPayload::Tuple(tb)));
        }
        2 => {
            let b = pop(frame);
            let a = pop(frame);
            let mut tb = alloc_tuple_box_empty();
            tb.push(a);
            tb.push(b);
            push(frame, PyObject::wrap_leaf(PyObjectPayload::Tuple(tb)));
        }
        3 => {
            let c = pop(frame);
            let b = pop(frame);
            let a = pop(frame);
            let mut tb = alloc_tuple_box_empty();
            tb.push(a);
            tb.push(b);
            tb.push(c);
            push(frame, PyObject::wrap_leaf(PyObjectPayload::Tuple(tb)));
        }
        _ => {
            let start = frame.stack.len() - count;
            let items = frame.stack.split_off(start);
            push(frame, PyObject::tuple(items));
        }
    }
    FastBuildResult::Handled
}

#[inline(always)]
fn build_list(frame: &mut Frame, count: usize) -> FastBuildResult {
    match count {
        0 => push(frame, PyObject::list(vec![])),
        1 => {
            let a = pop(frame);
            push(frame, PyObject::list(vec![a]));
        }
        _ => {
            let start = frame.stack.len() - count;
            let items = frame.stack.split_off(start);
            push(frame, PyObject::list(items));
        }
    }
    FastBuildResult::Handled
}

#[inline(always)]
fn format_value(frame: &mut Frame, instr: Instruction) -> FastBuildResult {
    let has_fmt_spec = instr.arg & 0x04 != 0;
    let conversion = (instr.arg & 0x03) as u8;
    if has_fmt_spec || (conversion != 0 && conversion != 1) {
        return FastBuildResult::Fallback;
    }

    let value = unsafe { frame.stack.get_unchecked(frame.stack.len() - 1) };
    let fast_str = match &value.payload {
        PyObjectPayload::Str(s) => Some(s.to_compact_string()),
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
    let Some(fragment) = fast_str else {
        return FastBuildResult::Fallback;
    };

    let next_ip = frame.ip;
    let instr_len = frame.code.instructions.len();
    if next_ip < instr_len {
        let next = unsafe { *frame.code.instructions.get_unchecked(next_ip) };
        if next.op == Opcode::BuildString && next.arg == 1 {
            replace_stack_top(frame, PyObject::str_val(fragment));
            frame.ip = next_ip + 1;
            return FastBuildResult::Handled;
        }
        if next.op == Opcode::BuildString
            && next.arg == 2
            && fuse_prefix_value(frame, &fragment, next_ip + 1)
        {
            return FastBuildResult::Handled;
        }
        if next_ip + 1 < instr_len && next.op == Opcode::LoadConst {
            let next2 = unsafe { *frame.code.instructions.get_unchecked(next_ip + 1) };
            let suffix_const = &frame.code.constants[next.arg as usize];
            if let ConstantValue::Str(suffix) = suffix_const {
                let suffix = suffix.clone();
                if next2.op == Opcode::BuildString && next2.arg == 2 {
                    let total = fragment.len() + suffix.len();
                    let mut result = String::with_capacity(total);
                    result.push_str(fragment.as_str());
                    result.push_str(suffix.as_str());
                    replace_stack_top(frame, PyObject::str_val(CompactString::from(result)));
                    frame.ip = next_ip + 2;
                    return FastBuildResult::Handled;
                }
                if next2.op == Opcode::BuildString
                    && next2.arg == 3
                    && fuse_prefix_value_suffix(frame, &fragment, &suffix, next_ip + 2)
                {
                    return FastBuildResult::Handled;
                }
            }
        }
    }

    replace_stack_top(frame, PyObject::str_val(fragment));
    FastBuildResult::Handled
}

#[inline(always)]
fn fuse_prefix_value(frame: &mut Frame, fragment: &CompactString, target_ip: usize) -> bool {
    let stack_len = frame.stack.len();
    let prefix_obj = unsafe { frame.stack.get_unchecked(stack_len - 2) };
    let PyObjectPayload::Str(prefix) = &prefix_obj.payload else {
        return false;
    };
    let total = prefix.len() + fragment.len();
    let mut result = String::with_capacity(total);
    result.push_str(prefix.as_str());
    result.push_str(fragment.as_str());
    drop_stack_pair_push_string(frame, stack_len, result);
    frame.ip = target_ip;
    true
}

#[inline(always)]
fn fuse_prefix_value_suffix(
    frame: &mut Frame,
    fragment: &CompactString,
    suffix: &CompactString,
    target_ip: usize,
) -> bool {
    let stack_len = frame.stack.len();
    let prefix_obj = unsafe { frame.stack.get_unchecked(stack_len - 2) };
    let PyObjectPayload::Str(prefix) = &prefix_obj.payload else {
        return false;
    };
    let total = prefix.len() + fragment.len() + suffix.len();
    let mut result = String::with_capacity(total);
    result.push_str(prefix.as_str());
    result.push_str(fragment.as_str());
    result.push_str(suffix.as_str());
    drop_stack_pair_push_string(frame, stack_len, result);
    frame.ip = target_ip;
    true
}

#[inline(always)]
fn drop_stack_pair_push_string(frame: &mut Frame, stack_len: usize, result: String) {
    unsafe {
        std::ptr::drop_in_place(frame.stack.as_mut_ptr().add(stack_len - 2));
        std::ptr::drop_in_place(frame.stack.as_mut_ptr().add(stack_len - 1));
        frame.stack.set_len(stack_len - 2);
    }
    push(frame, PyObject::str_val(CompactString::from(result)));
}

#[inline(always)]
fn replace_stack_top(frame: &mut Frame, value: PyObjectRef) {
    let len = frame.stack.len();
    unsafe { *frame.stack.get_unchecked_mut(len - 1) = value };
}

#[inline(always)]
fn build_string(frame: &mut Frame, count: usize) -> FastBuildResult {
    if count <= 1 {
        if count == 0 {
            push(frame, PyObject::str_val(CompactString::from("")));
        }
        return FastBuildResult::Handled;
    }

    let start = frame.stack.len() - count;
    let mut total_len = 0usize;
    for item in &frame.stack[start..] {
        if let PyObjectPayload::Str(s) = &item.payload {
            total_len += s.len();
        } else {
            return FastBuildResult::Fallback;
        }
    }

    let mut result = String::with_capacity(total_len);
    for item in &frame.stack[start..] {
        if let PyObjectPayload::Str(s) = &item.payload {
            result.push_str(s.as_str());
        }
    }
    frame.stack.truncate(start);
    push(frame, PyObject::str_val(CompactString::from(result)));
    FastBuildResult::Handled
}

#[inline(always)]
fn unpack_sequence(
    frame: &mut Frame,
    count: usize,
    instr_base: *const Instruction,
    instr_count: usize,
) -> FastBuildResult {
    let top = pop(frame);
    match &top.payload {
        PyObjectPayload::Tuple(items) if items.len() == count => {
            unpack_items(frame, items, count, instr_base, instr_count)
        }
        PyObjectPayload::List(cell) => {
            let list = unsafe { &*cell.data_ptr() };
            if list.len() == count {
                unpack_items(frame, list, count, instr_base, instr_count)
            } else {
                push(frame, top);
                FastBuildResult::Fallback
            }
        }
        _ => {
            push(frame, top);
            FastBuildResult::Fallback
        }
    }
}

#[inline(always)]
fn unpack_items(
    frame: &mut Frame,
    items: &[PyObjectRef],
    count: usize,
    instr_base: *const Instruction,
    instr_count: usize,
) -> FastBuildResult {
    let ip = frame.ip;
    if count >= 2 && count <= 8 && ip + count <= instr_count {
        let mut ok = true;
        for i in 0..count - 1 {
            if unsafe { *instr_base.add(ip + i) }.op != Opcode::StoreFast {
                ok = false;
                break;
            }
        }
        if ok {
            let last = unsafe { *instr_base.add(ip + count - 1) };
            let last_info = match last.op {
                Opcode::StoreFast => Some((last.arg as usize, None)),
                Opcode::StoreFastJumpAbsolute => Some((
                    (last.arg >> 16) as usize,
                    Some((last.arg & 0xFFFF) as usize),
                )),
                _ => None,
            };
            if let Some((last_idx, jump)) = last_info {
                for i in 0..count - 1 {
                    let local_idx = unsafe { *instr_base.add(ip + i) }.arg as usize;
                    unsafe { *frame.locals.get_unchecked_mut(local_idx) = Some(items[i].clone()) };
                }
                unsafe {
                    *frame.locals.get_unchecked_mut(last_idx) = Some(items[count - 1].clone())
                };
                frame.ip = jump.unwrap_or(ip + count);
                return FastBuildResult::ChainJump;
            }
        }
    }

    unsafe {
        let stack = &mut frame.stack;
        stack.reserve(count);
        let base = stack.as_mut_ptr().add(stack.len());
        for i in 0..count {
            std::ptr::write(base.add(i), items[count - 1 - i].clone());
        }
        stack.set_len(stack.len() + count);
    }
    FastBuildResult::Handled
}
