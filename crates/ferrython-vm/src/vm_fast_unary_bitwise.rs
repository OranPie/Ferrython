//! Fast unary, power, and bitwise opcode helpers for the VM dispatch loop.

use crate::frame::Frame;
use ferrython_bytecode::{Instruction, Opcode};
use ferrython_core::object::{PyObject, PyObjectPayload, PyObjectRef};
use ferrython_core::types::PyInt;

pub(crate) enum FastUnaryBitwiseResult {
    Handled,
    Fallback,
}

#[inline(always)]
pub(crate) fn try_fast_unary_bitwise(
    frame: &mut Frame,
    instr: Instruction,
) -> FastUnaryBitwiseResult {
    match instr.op {
        Opcode::UnaryNot => try_unary_not(frame),
        Opcode::UnaryNegative => try_unary_negative(frame),
        Opcode::BinaryPower | Opcode::InplacePower => try_binary_power(frame),
        Opcode::BinaryAnd | Opcode::InplaceAnd => try_binary_and(frame),
        Opcode::BinaryOr | Opcode::InplaceOr => try_binary_or(frame),
        Opcode::BinaryXor | Opcode::InplaceXor => try_binary_xor(frame),
        Opcode::BinaryLshift | Opcode::InplaceLshift => try_binary_lshift(frame),
        Opcode::BinaryRshift | Opcode::InplaceRshift => try_binary_rshift(frame),
        _ => FastUnaryBitwiseResult::Fallback,
    }
}

#[inline(always)]
fn stack_top(frame: &Frame) -> &PyObjectRef {
    unsafe { frame.stack.get_unchecked(frame.stack.len() - 1) }
}

#[inline(always)]
fn stack_operands(frame: &Frame) -> (&PyObjectRef, &PyObjectRef) {
    let len = frame.stack.len();
    unsafe {
        (
            frame.stack.get_unchecked(len - 2),
            frame.stack.get_unchecked(len - 1),
        )
    }
}

#[inline(always)]
fn store_unary_result(frame: &mut Frame, value: PyObjectRef) {
    let len = frame.stack.len();
    unsafe { *frame.stack.get_unchecked_mut(len - 1) = value };
}

#[inline(always)]
fn store_binary_result(frame: &mut Frame, value: PyObjectRef) {
    unsafe { frame.binary_op_result(value) };
}

#[inline(always)]
fn try_unary_not(frame: &mut Frame) -> FastUnaryBitwiseResult {
    let v = stack_top(frame);
    let result = match &v.payload {
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
    if let Some(value) = result {
        store_unary_result(frame, PyObject::bool_val(value));
        FastUnaryBitwiseResult::Handled
    } else {
        FastUnaryBitwiseResult::Fallback
    }
}

#[inline(always)]
fn try_unary_negative(frame: &mut Frame) -> FastUnaryBitwiseResult {
    let v = stack_top(frame);
    let result = match &v.payload {
        PyObjectPayload::Int(PyInt::Small(n)) => Some(match n.checked_neg() {
            Some(r) => PyObject::int(r),
            None => {
                use num_bigint::BigInt;
                PyObject::big_int(-BigInt::from(*n))
            }
        }),
        PyObjectPayload::Float(f) => Some(PyObject::float(-f)),
        PyObjectPayload::Bool(b) => Some(PyObject::int(if *b { -1 } else { 0 })),
        _ => None,
    };
    if let Some(value) = result {
        store_unary_result(frame, value);
        FastUnaryBitwiseResult::Handled
    } else {
        FastUnaryBitwiseResult::Fallback
    }
}

#[inline(always)]
fn try_binary_power(frame: &mut Frame) -> FastUnaryBitwiseResult {
    let (a, b) = stack_operands(frame);
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y)))
            if *y >= 0 && *y <= 63 =>
        {
            let mut result: i64 = 1;
            let mut overflow = false;
            let base = *x;
            let exp = *y;
            for _ in 0..exp {
                match result.checked_mul(base) {
                    Some(v) => result = v,
                    None => {
                        overflow = true;
                        break;
                    }
                }
            }
            if overflow {
                FastUnaryBitwiseResult::Fallback
            } else {
                store_binary_result(frame, PyObject::int(result));
                FastUnaryBitwiseResult::Handled
            }
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
            store_binary_result(frame, PyObject::float(x.powf(*y)));
            FastUnaryBitwiseResult::Handled
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
            store_binary_result(frame, PyObject::float(x.powi(*y as i32)));
            FastUnaryBitwiseResult::Handled
        }
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
            store_binary_result(frame, PyObject::float((*x as f64).powf(*y)));
            FastUnaryBitwiseResult::Handled
        }
        _ => FastUnaryBitwiseResult::Fallback,
    }
}

#[inline(always)]
fn try_binary_and(frame: &mut Frame) -> FastUnaryBitwiseResult {
    let (a, b) = stack_operands(frame);
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
            store_binary_result(frame, PyObject::int(*x & *y));
            FastUnaryBitwiseResult::Handled
        }
        (PyObjectPayload::Bool(x), PyObjectPayload::Bool(y)) => {
            store_binary_result(frame, PyObject::bool_val(*x && *y));
            FastUnaryBitwiseResult::Handled
        }
        _ => FastUnaryBitwiseResult::Fallback,
    }
}

#[inline(always)]
fn try_binary_or(frame: &mut Frame) -> FastUnaryBitwiseResult {
    let (a, b) = stack_operands(frame);
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
            store_binary_result(frame, PyObject::int(*x | *y));
            FastUnaryBitwiseResult::Handled
        }
        (PyObjectPayload::Bool(x), PyObjectPayload::Bool(y)) => {
            store_binary_result(frame, PyObject::bool_val(*x || *y));
            FastUnaryBitwiseResult::Handled
        }
        _ => FastUnaryBitwiseResult::Fallback,
    }
}

#[inline(always)]
fn try_binary_xor(frame: &mut Frame) -> FastUnaryBitwiseResult {
    let (a, b) = stack_operands(frame);
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
            store_binary_result(frame, PyObject::int(*x ^ *y));
            FastUnaryBitwiseResult::Handled
        }
        (PyObjectPayload::Bool(x), PyObjectPayload::Bool(y)) => {
            store_binary_result(frame, PyObject::bool_val(*x ^ *y));
            FastUnaryBitwiseResult::Handled
        }
        _ => FastUnaryBitwiseResult::Fallback,
    }
}

#[inline(always)]
fn try_binary_lshift(frame: &mut Frame) -> FastUnaryBitwiseResult {
    let (a, b) = stack_operands(frame);
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
            if let Some(result) = PyInt::checked_small_lshift(*x, *y) {
                store_binary_result(frame, PyObject::int(result));
                FastUnaryBitwiseResult::Handled
            } else {
                FastUnaryBitwiseResult::Fallback
            }
        }
        _ => FastUnaryBitwiseResult::Fallback,
    }
}

#[inline(always)]
fn try_binary_rshift(frame: &mut Frame) -> FastUnaryBitwiseResult {
    let (a, b) = stack_operands(frame);
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y)))
            if *y >= 0 && *y < 64 =>
        {
            store_binary_result(frame, PyObject::int(*x >> *y));
            FastUnaryBitwiseResult::Handled
        }
        _ => FastUnaryBitwiseResult::Fallback,
    }
}
