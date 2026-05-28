//! Fast comparison helpers for the VM dispatch loop.

use crate::frame::Frame;
use ferrython_bytecode::Instruction;
use ferrython_core::object::{PyObject, PyObjectMethods, PyObjectPayload, PyObjectRef};
use ferrython_core::types::{BorrowedIntKey, BorrowedStrKey, HashableKey, PyInt};

pub(crate) enum FastCompareResult {
    Bool(bool),
    Fallback,
}

#[inline(always)]
pub(crate) fn try_fast_compare(frame: &Frame, instr: Instruction) -> FastCompareResult {
    match instr.arg {
        0..=5 => try_ordered_compare(frame, instr.arg),
        6 | 7 => try_contains_compare(frame, instr.arg),
        8 | 9 => try_identity_compare(frame, instr.arg),
        _ => FastCompareResult::Fallback,
    }
}

#[inline(always)]
pub(crate) fn store_compare_bool(frame: &mut Frame, result: bool) {
    unsafe { frame.binary_op_result(PyObject::bool_val(result)) };
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
fn try_ordered_compare(frame: &Frame, op: u32) -> FastCompareResult {
    let (a, b) = stack_operands(frame);
    let compares_weak_ref = |obj: &PyObjectRef| {
        matches!(&obj.payload, PyObjectPayload::Instance(inst)
            if inst.attrs.read().contains_key("__weakref_ref__"))
    };
    if (op == 2 || op == 3) && PyObjectRef::ptr_eq(a, b) && !compares_weak_ref(a) {
        return FastCompareResult::Bool(op == 2);
    }
    match (&a.payload, &b.payload) {
        (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
            FastCompareResult::Bool(match op {
                0 => x < y,
                1 => x <= y,
                2 => x == y,
                3 => x != y,
                4 => x > y,
                _ => x >= y,
            })
        }
        (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
            let (xv, yv) = (*x, *y);
            FastCompareResult::Bool(match op {
                0 => xv < yv,
                1 => xv <= yv,
                2 => xv == yv,
                3 => xv != yv,
                4 => xv > yv,
                _ => xv >= yv,
            })
        }
        (PyObjectPayload::Str(x), PyObjectPayload::Str(y)) if op == 2 || op == 3 => {
            let eq = x == y;
            FastCompareResult::Bool(if op == 2 { eq } else { !eq })
        }
        _ => FastCompareResult::Fallback,
    }
}

#[inline(always)]
fn try_identity_compare(frame: &Frame, op: u32) -> FastCompareResult {
    let (a, b) = stack_operands(frame);
    let same = PyObjectRef::ptr_eq(a, b)
        || matches!((&a.payload, &b.payload),
            (PyObjectPayload::BuiltinType(at), PyObjectPayload::BuiltinType(bt)) if at == bt)
        || matches!((&a.payload, &b.payload),
            (PyObjectPayload::ExceptionType(at), PyObjectPayload::ExceptionType(bt)) if at == bt);
    FastCompareResult::Bool(if op == 8 { same } else { !same })
}

#[inline(always)]
fn try_contains_compare(frame: &Frame, op: u32) -> FastCompareResult {
    let (needle, haystack) = stack_operands(frame);
    let found = match &haystack.payload {
        PyObjectPayload::Dict(map) => {
            let r = unsafe { &*map.data_ptr() };
            match &needle.payload {
                PyObjectPayload::Str(s) => Some(r.contains_key(&BorrowedStrKey(s.as_str()))),
                PyObjectPayload::Int(PyInt::Small(n)) => Some(r.contains_key(&BorrowedIntKey(*n))),
                PyObjectPayload::Bool(b) => Some(r.contains_key(&BorrowedIntKey(*b as i64))),
                _ => None,
            }
        }
        PyObjectPayload::Set(items) => {
            let r = unsafe { &*items.data_ptr() };
            match &needle.payload {
                PyObjectPayload::Str(s) => {
                    Some(r.contains_key(&HashableKey::str_key(s.to_compact_string())))
                }
                PyObjectPayload::Int(PyInt::Small(n)) => {
                    Some(r.contains_key(&HashableKey::Int(PyInt::Small(*n))))
                }
                PyObjectPayload::Bool(b) => {
                    Some(r.contains_key(&HashableKey::Int(PyInt::Small(*b as i64))))
                }
                _ => HashableKey::from_object(needle)
                    .ok()
                    .map(|hk| r.contains_key(&hk)),
            }
        }
        PyObjectPayload::List(items) => {
            let items = unsafe { &*items.data_ptr() };
            Some(items.iter().any(|x| fast_same_or_equal(x, needle)))
        }
        PyObjectPayload::Tuple(items) => Some(items.iter().any(|x| fast_same_or_equal(x, needle))),
        PyObjectPayload::Str(haystack_s) => {
            if let PyObjectPayload::Str(needle_s) = &needle.payload {
                Some(haystack_s.contains(needle_s.as_str()))
            } else {
                None
            }
        }
        _ => None,
    };
    if let Some(is_in) = found {
        FastCompareResult::Bool(if op == 6 { is_in } else { !is_in })
    } else {
        FastCompareResult::Fallback
    }
}

#[inline(always)]
fn fast_same_or_equal(a: &PyObjectRef, b: &PyObjectRef) -> bool {
    PyObjectRef::ptr_eq(a, b)
        || match (&a.payload, &b.payload) {
            (PyObjectPayload::Int(PyInt::Small(a)), PyObjectPayload::Int(PyInt::Small(b))) => {
                a == b
            }
            (PyObjectPayload::Str(a), PyObjectPayload::Str(b)) => a == b,
            (PyObjectPayload::Bool(a), PyObjectPayload::Bool(b)) => a == b,
            (PyObjectPayload::Float(a), PyObjectPayload::Float(b)) => a == b,
            (PyObjectPayload::None, PyObjectPayload::None) => true,
            (PyObjectPayload::Tuple(a), PyObjectPayload::Tuple(b)) => {
                a.len() == b.len()
                    && a.iter().zip(b.iter()).all(|(ai, bi)| {
                        ferrython_core::object::helpers::partial_cmp_objects(ai, bi)
                            == Some(std::cmp::Ordering::Equal)
                    })
            }
            _ => a.is_same(b),
        }
}
