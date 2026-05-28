//! Fast binary opcode helpers for the VM dispatch loop.

use crate::frame::Frame;
use compact_str::CompactString;
use ferrython_bytecode::{Instruction, Opcode};
use ferrython_core::object::{PyObject, PyObjectPayload, PyObjectRef};
use ferrython_core::types::PyInt;

pub(crate) enum FastBinaryResult {
    Handled,
    Fallback,
}

#[derive(Clone, Copy)]
pub(crate) enum FastFusedBinaryResult {
    Handled,
    HandledChain,
    Fallback(Opcode),
    UnboundLocal(usize),
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
pub(crate) fn try_fast_fused_binary(
    frame: &mut Frame,
    instr: Instruction,
) -> FastFusedBinaryResult {
    match instr.op {
        Opcode::LoadFastLoadConstBinarySub => {
            let local_idx = (instr.arg >> 16) as usize;
            let const_idx = (instr.arg & 0xFFFF) as usize;
            try_local_const_binary(frame, local_idx, const_idx, Opcode::BinarySubtract)
        }
        Opcode::LoadFastLoadConstBinaryAdd => {
            let local_idx = (instr.arg >> 16) as usize;
            let const_idx = (instr.arg & 0xFFFF) as usize;
            try_local_const_binary(frame, local_idx, const_idx, Opcode::BinaryAdd)
        }
        Opcode::LoadFastLoadConstBinaryMul => {
            let local_idx = (instr.arg >> 16) as usize;
            let const_idx = (instr.arg & 0xFFFF) as usize;
            try_local_const_binary(frame, local_idx, const_idx, Opcode::BinaryMultiply)
        }
        Opcode::LoadFastLoadFastBinaryAdd => {
            let left_idx = (instr.arg >> 16) as usize;
            let right_idx = (instr.arg & 0xFFFF) as usize;
            try_local_local_add(frame, left_idx, right_idx)
        }
        Opcode::LoadFastLoadFastBinaryAddStoreFast => {
            let left_idx = (instr.arg >> 16) as usize;
            let right_idx = ((instr.arg >> 8) & 0xFF) as usize;
            let dest_idx = (instr.arg & 0xFF) as usize;
            try_local_local_add_store(frame, left_idx, right_idx, dest_idx)
        }
        Opcode::LoadFastLoadConstBinaryAddStoreFast => {
            let local_idx = (instr.arg >> 16) as usize;
            let const_idx = ((instr.arg >> 8) & 0xFF) as usize;
            let dest_idx = (instr.arg & 0xFF) as usize;
            try_local_const_store(frame, local_idx, const_idx, dest_idx, Opcode::BinaryAdd)
        }
        Opcode::LoadFastLoadConstBinaryMulStoreFast => {
            let local_idx = (instr.arg >> 16) as usize;
            let const_idx = ((instr.arg >> 8) & 0xFF) as usize;
            let dest_idx = (instr.arg & 0xFF) as usize;
            try_local_const_store(
                frame,
                local_idx,
                const_idx,
                dest_idx,
                Opcode::BinaryMultiply,
            )
        }
        Opcode::LoadFastLoadConstBinarySubStoreFast => {
            let local_idx = (instr.arg >> 16) as usize;
            let const_idx = ((instr.arg >> 8) & 0xFF) as usize;
            let dest_idx = (instr.arg & 0xFF) as usize;
            try_local_const_store(
                frame,
                local_idx,
                const_idx,
                dest_idx,
                Opcode::BinarySubtract,
            )
        }
        Opcode::LoadFastMulModStoreFast => {
            let local_idx = (instr.arg >> 24) as usize;
            let const1_idx = ((instr.arg >> 16) & 0xFF) as usize;
            let const2_idx = ((instr.arg >> 8) & 0xFF) as usize;
            let dest_idx = (instr.arg & 0xFF) as usize;
            try_local_mul_mod_store(frame, local_idx, const1_idx, const2_idx, dest_idx)
        }
        _ => FastFusedBinaryResult::Fallback(instr.op),
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
fn local_ref(frame: &Frame, idx: usize) -> Option<&PyObjectRef> {
    unsafe { frame.locals.get_unchecked(idx).as_ref() }
}

#[inline(always)]
fn const_ref(frame: &Frame, idx: usize) -> &PyObjectRef {
    unsafe { frame.constant_cache.get_unchecked(idx) }
}

#[inline(always)]
fn push(frame: &mut Frame, value: PyObjectRef) {
    unsafe { frame.push_unchecked(value) };
}

#[inline(always)]
fn set_local_mutable(frame: &mut Frame, idx: usize, value: PyObjectRef) {
    let dest_slot = unsafe { frame.locals.get_unchecked_mut(idx) };
    if let Some(ref mut current) = dest_slot {
        if let Some(obj) = PyObjectRef::get_mut(current) {
            match &value.payload {
                PyObjectPayload::Int(PyInt::Small(v)) => {
                    obj.payload = PyObjectPayload::Int(PyInt::Small(*v));
                    return;
                }
                PyObjectPayload::Float(v) => {
                    obj.payload = PyObjectPayload::Float(*v);
                    return;
                }
                PyObjectPayload::Str(v) => {
                    obj.payload = PyObjectPayload::Str(v.clone());
                    return;
                }
                _ => {}
            }
        }
    }
    *dest_slot = Some(value);
}

#[inline(always)]
fn try_local_const_binary(
    frame: &mut Frame,
    local_idx: usize,
    const_idx: usize,
    op: Opcode,
) -> FastFusedBinaryResult {
    let Some(local) = local_ref(frame, local_idx) else {
        return FastFusedBinaryResult::UnboundLocal(local_idx);
    };
    let constant = const_ref(frame, const_idx);
    if let Some(value) = fast_binary_value(local, constant, op) {
        push(frame, value);
        FastFusedBinaryResult::Handled
    } else {
        let local = local.clone();
        let constant = constant.clone();
        push(frame, local);
        push(frame, constant);
        FastFusedBinaryResult::Fallback(op)
    }
}

#[inline(always)]
fn try_local_local_add(
    frame: &mut Frame,
    left_idx: usize,
    right_idx: usize,
) -> FastFusedBinaryResult {
    let Some(left) = local_ref(frame, left_idx) else {
        return FastFusedBinaryResult::UnboundLocal(left_idx);
    };
    let Some(right) = local_ref(frame, right_idx) else {
        return FastFusedBinaryResult::UnboundLocal(right_idx);
    };
    if let Some(value) = fast_add_value(left, right) {
        push(frame, value);
        FastFusedBinaryResult::Handled
    } else {
        let left = left.clone();
        let right = right.clone();
        push(frame, left);
        push(frame, right);
        FastFusedBinaryResult::Fallback(Opcode::BinaryAdd)
    }
}

#[inline(always)]
fn try_local_local_add_store(
    frame: &mut Frame,
    left_idx: usize,
    right_idx: usize,
    dest_idx: usize,
) -> FastFusedBinaryResult {
    let Some(left) = local_ref(frame, left_idx) else {
        return FastFusedBinaryResult::UnboundLocal(left_idx);
    };
    let Some(right) = local_ref(frame, right_idx) else {
        return FastFusedBinaryResult::UnboundLocal(right_idx);
    };
    let Some(value) = fast_add_value(left, right) else {
        let left = left.clone();
        let right = right.clone();
        push(frame, left);
        push(frame, right);
        return FastFusedBinaryResult::Fallback(Opcode::BinaryAdd);
    };
    set_local_mutable(frame, dest_idx, value);
    FastFusedBinaryResult::HandledChain
}

#[inline(always)]
fn try_local_const_store(
    frame: &mut Frame,
    local_idx: usize,
    const_idx: usize,
    dest_idx: usize,
    op: Opcode,
) -> FastFusedBinaryResult {
    let Some(local) = local_ref(frame, local_idx) else {
        return FastFusedBinaryResult::UnboundLocal(local_idx);
    };
    let constant = const_ref(frame, const_idx);
    let Some(value) = fast_binary_value(local, constant, op) else {
        let local = local.clone();
        let constant = constant.clone();
        push(frame, local);
        push(frame, constant);
        return FastFusedBinaryResult::Fallback(op);
    };
    set_local_mutable(frame, dest_idx, value);
    FastFusedBinaryResult::Handled
}

#[inline(always)]
fn try_local_mul_mod_store(
    frame: &mut Frame,
    local_idx: usize,
    const1_idx: usize,
    const2_idx: usize,
    dest_idx: usize,
) -> FastFusedBinaryResult {
    let Some(local) = local_ref(frame, local_idx) else {
        return FastFusedBinaryResult::UnboundLocal(local_idx);
    };
    let first = const_ref(frame, const1_idx);
    let second = const_ref(frame, const2_idx);
    let value = match (&local.payload, &first.payload, &second.payload) {
        (
            PyObjectPayload::Int(PyInt::Small(x)),
            PyObjectPayload::Int(PyInt::Small(m)),
            PyObjectPayload::Int(PyInt::Small(d)),
        ) if *d != 0 => {
            let (x, m, d) = (*x, *m, *d);
            if let Some(product) = x.checked_mul(m) {
                Some(PyObject::int(((product % d) + d) % d))
            } else {
                use num_bigint::BigInt;
                let product = BigInt::from(x) * BigInt::from(m);
                let d_big = BigInt::from(d);
                Some(PyObject::big_int(((&product % &d_big) + &d_big) % &d_big))
            }
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Float(m), PyObjectPayload::Float(d))
            if *d != 0.0 =>
        {
            let product = *x * *m;
            Some(PyObject::float(product - (product / *d).floor() * *d))
        }
        _ => None,
    };
    if let Some(value) = value {
        set_local_mutable(frame, dest_idx, value);
        FastFusedBinaryResult::Handled
    } else {
        let local = local.clone();
        let first = first.clone();
        push(frame, local);
        push(frame, first);
        FastFusedBinaryResult::Fallback(Opcode::BinaryMultiply)
    }
}

#[inline(always)]
fn fast_binary_value(a: &PyObjectRef, b: &PyObjectRef, op: Opcode) -> Option<PyObjectRef> {
    match op {
        Opcode::BinaryAdd => fast_add_value(a, b),
        Opcode::BinarySubtract => fast_subtract_value(a, b),
        Opcode::BinaryMultiply => fast_multiply_value(a, b),
        _ => None,
    }
}

#[inline(always)]
fn fast_add_value(a: &PyObjectRef, b: &PyObjectRef) -> Option<PyObjectRef> {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
            Some(match x.checked_add(*y) {
                Some(r) => PyObject::int(r),
                None => {
                    use num_bigint::BigInt;
                    PyObject::big_int(BigInt::from(*x) + BigInt::from(*y))
                }
            })
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => Some(PyObject::float(*x + *y)),
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
            Some(PyObject::float(*x as f64 + *y))
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
            Some(PyObject::float(*x + *y as f64))
        }
        (PyObjectPayload::Str(x), PyObjectPayload::Str(y)) => {
            let mut s = String::with_capacity(x.len() + y.len());
            s.push_str(x);
            s.push_str(y);
            Some(PyObject::str_val(CompactString::from(s)))
        }
        _ => None,
    }
}

#[inline(always)]
fn fast_subtract_value(a: &PyObjectRef, b: &PyObjectRef) -> Option<PyObjectRef> {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
            Some(match x.checked_sub(*y) {
                Some(r) => PyObject::int(r),
                None => {
                    use num_bigint::BigInt;
                    PyObject::big_int(BigInt::from(*x) - BigInt::from(*y))
                }
            })
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => Some(PyObject::float(*x - *y)),
        _ => None,
    }
}

#[inline(always)]
fn fast_multiply_value(a: &PyObjectRef, b: &PyObjectRef) -> Option<PyObjectRef> {
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
            Some(match x.checked_mul(*y) {
                Some(r) => PyObject::int(r),
                None => {
                    use num_bigint::BigInt;
                    PyObject::big_int(BigInt::from(*x) * BigInt::from(*y))
                }
            })
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => Some(PyObject::float(*x * *y)),
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
            Some(PyObject::float(*x as f64 * *y))
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
            Some(PyObject::float(*x * *y as f64))
        }
        _ => None,
    }
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
