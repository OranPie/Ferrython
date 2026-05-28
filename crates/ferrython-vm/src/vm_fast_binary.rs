//! Fast binary opcode helpers for the VM dispatch loop.

use crate::frame::Frame;
use ferrython_bytecode::{Instruction, Opcode};
use ferrython_core::object::{PyObject, PyObjectPayload, PyObjectRef};
use ferrython_core::types::PyInt;

pub(crate) enum FastBinaryResult {
    Handled,
    Fallback,
}

#[inline(always)]
pub(crate) fn try_fast_binary(frame: &mut Frame, instr: Instruction) -> FastBinaryResult {
    match instr.op {
        Opcode::BinaryAdd | Opcode::InplaceAdd => try_fast_add(frame),
        Opcode::BinarySubtract | Opcode::InplaceSubtract => try_fast_subtract(frame),
        Opcode::BinaryMultiply | Opcode::InplaceMultiply => try_fast_multiply(frame),
        Opcode::BinaryModulo | Opcode::InplaceModulo => try_fast_modulo(frame),
        Opcode::BinaryFloorDivide | Opcode::InplaceFloorDivide => try_fast_floor_divide(frame),
        Opcode::BinaryTrueDivide | Opcode::InplaceTrueDivide => try_fast_true_divide(frame),
        _ => FastBinaryResult::Fallback,
    }
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
fn store_binary_result(frame: &mut Frame, value: PyObjectRef) {
    unsafe { frame.binary_op_result(value) };
}

#[inline(always)]
fn try_fast_add(frame: &mut Frame) -> FastBinaryResult {
    let (a, b) = stack_operands(frame);
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
            let result = match x.checked_add(*y) {
                Some(r) => PyObject::int(r),
                None => {
                    use num_bigint::BigInt;
                    PyObject::big_int(BigInt::from(*x) + BigInt::from(*y))
                }
            };
            store_binary_result(frame, result);
            FastBinaryResult::Handled
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
            store_binary_result(frame, PyObject::float(*x + *y));
            FastBinaryResult::Handled
        }
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
            store_binary_result(frame, PyObject::float(*x as f64 + *y));
            FastBinaryResult::Handled
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
            store_binary_result(frame, PyObject::float(*x + *y as f64));
            FastBinaryResult::Handled
        }
        (PyObjectPayload::Str(_), PyObjectPayload::Str(y)) => {
            let rhs = y.to_compact_string();
            let len = frame.stack.len();
            unsafe {
                let _b_arc = std::ptr::read(frame.stack.as_ptr().add(len - 1));
                let mut a_arc = std::ptr::read(frame.stack.as_ptr().add(len - 2));
                frame.stack.set_len(len - 2);
                drop(_b_arc);
                if PyObjectRef::strong_count(&a_arc) == 2
                    && frame.ip < frame.code.instructions.len()
                {
                    let next = *frame.code.instructions.get_unchecked(frame.ip);
                    let store_idx_opt = match next.op {
                        Opcode::StoreFast => Some(next.arg as usize),
                        Opcode::StoreFastLoadFast | Opcode::StoreFastJumpAbsolute => {
                            Some((next.arg >> 16) as usize)
                        }
                        Opcode::LoadConstStoreFast => Some((next.arg & 0xFFFF) as usize),
                        _ => None,
                    };
                    if let Some(store_idx) = store_idx_opt {
                        let slot = frame.locals.get_unchecked_mut(store_idx);
                        if let Some(ref existing) = slot {
                            if PyObjectRef::ptr_eq(existing, &a_arc) {
                                *slot = None;
                            }
                        }
                    }
                }
                if let Some(obj) = PyObjectRef::get_mut(&mut a_arc) {
                    if let PyObjectPayload::Str(ref mut s) = obj.payload {
                        s.push_str(&rhs);
                        frame.stack.push(a_arc);
                        return FastBinaryResult::Handled;
                    }
                }
                let new_s = if let PyObjectPayload::Str(ref x) = a_arc.payload {
                    let mut s = String::with_capacity(x.len() + rhs.len());
                    s.push_str(x);
                    s.push_str(&rhs);
                    compact_str::CompactString::from(s)
                } else {
                    unreachable!()
                };
                frame.stack.push(PyObject::str_val(new_s));
            }
            FastBinaryResult::Handled
        }
        (PyObjectPayload::List(x), PyObjectPayload::List(y)) => {
            let mut items = unsafe { &*x.data_ptr() }.clone();
            items.extend(unsafe { &*y.data_ptr() }.iter().cloned());
            store_binary_result(frame, PyObject::list(items));
            FastBinaryResult::Handled
        }
        (PyObjectPayload::Tuple(x), PyObjectPayload::Tuple(y)) => {
            let mut items = x.to_vec();
            items.extend(y.iter().cloned());
            store_binary_result(frame, PyObject::tuple(items));
            FastBinaryResult::Handled
        }
        _ => FastBinaryResult::Fallback,
    }
}

#[inline(always)]
fn try_fast_subtract(frame: &mut Frame) -> FastBinaryResult {
    let (a, b) = stack_operands(frame);
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
            let result = match x.checked_sub(*y) {
                Some(r) => PyObject::int(r),
                None => {
                    use num_bigint::BigInt;
                    PyObject::big_int(BigInt::from(*x) - BigInt::from(*y))
                }
            };
            store_binary_result(frame, result);
            FastBinaryResult::Handled
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
            store_binary_result(frame, PyObject::float(*x - *y));
            FastBinaryResult::Handled
        }
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
            store_binary_result(frame, PyObject::float(*x as f64 - *y));
            FastBinaryResult::Handled
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
            store_binary_result(frame, PyObject::float(*x - *y as f64));
            FastBinaryResult::Handled
        }
        _ => FastBinaryResult::Fallback,
    }
}

#[inline(always)]
fn try_fast_multiply(frame: &mut Frame) -> FastBinaryResult {
    let (a, b) = stack_operands(frame);
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
            let result = match x.checked_mul(*y) {
                Some(r) => PyObject::int(r),
                None => {
                    use num_bigint::BigInt;
                    PyObject::big_int(BigInt::from(*x) * BigInt::from(*y))
                }
            };
            store_binary_result(frame, result);
            FastBinaryResult::Handled
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
            store_binary_result(frame, PyObject::float(*x * *y));
            FastBinaryResult::Handled
        }
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
            store_binary_result(frame, PyObject::float(*x as f64 * *y));
            FastBinaryResult::Handled
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
            store_binary_result(frame, PyObject::float(*x * *y as f64));
            FastBinaryResult::Handled
        }
        _ => FastBinaryResult::Fallback,
    }
}

#[inline(always)]
fn try_fast_modulo(frame: &mut Frame) -> FastBinaryResult {
    let (a, b) = stack_operands(frame);
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y)))
            if *y != 0 =>
        {
            let r = if *x >= 0 && *y > 0 {
                *x % *y
            } else {
                ((*x % *y) + *y) % *y
            };
            store_binary_result(frame, PyObject::int(r));
            FastBinaryResult::Handled
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) if *y != 0.0 => {
            let r = *x - (*x / *y).floor() * *y;
            store_binary_result(frame, PyObject::float(r));
            FastBinaryResult::Handled
        }
        _ => FastBinaryResult::Fallback,
    }
}

#[inline(always)]
fn try_fast_floor_divide(frame: &mut Frame) -> FastBinaryResult {
    let (a, b) = stack_operands(frame);
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y)))
            if *y != 0 =>
        {
            let (d, m) = (x.div_euclid(*y), x.rem_euclid(*y));
            let r = if m != 0 && (*x ^ *y) < 0 { d - 1 } else { d };
            store_binary_result(frame, PyObject::int(r));
            FastBinaryResult::Handled
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) if *y != 0.0 => {
            store_binary_result(frame, PyObject::float((*x / *y).floor()));
            FastBinaryResult::Handled
        }
        _ => FastBinaryResult::Fallback,
    }
}

#[inline(always)]
fn try_fast_true_divide(frame: &mut Frame) -> FastBinaryResult {
    let (a, b) = stack_operands(frame);
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y)))
            if *y != 0 =>
        {
            store_binary_result(frame, PyObject::float(*x as f64 / *y as f64));
            FastBinaryResult::Handled
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) if *y != 0.0 => {
            store_binary_result(frame, PyObject::float(*x / *y));
            FastBinaryResult::Handled
        }
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) if *y != 0.0 => {
            store_binary_result(frame, PyObject::float(*x as f64 / *y));
            FastBinaryResult::Handled
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) if *y != 0 => {
            store_binary_result(frame, PyObject::float(*x / *y as f64));
            FastBinaryResult::Handled
        }
        _ => FastBinaryResult::Fallback,
    }
}
