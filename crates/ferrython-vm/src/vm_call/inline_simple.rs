use compact_str::CompactString;
use ferrython_bytecode::Opcode;
use ferrython_core::object::{PyObject, PyObjectPayload, PyObjectRef};
use ferrython_core::types::{PyFunction, PyInt};

use crate::VirtualMachine;

impl VirtualMachine {
    #[inline(always)]
    pub(super) fn fast_binary_result(
        op: Opcode,
        left: &PyObjectRef,
        right: &PyObjectRef,
    ) -> Option<PyObjectRef> {
        match op {
            Opcode::BinaryAdd | Opcode::InplaceAdd => match (&left.payload, &right.payload) {
                (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                    match x.checked_add(*y) {
                        Some(r) => Some(PyObject::int(r)),
                        None => {
                            use num_bigint::BigInt;
                            Some(PyObject::big_int(BigInt::from(*x) + BigInt::from(*y)))
                        }
                    }
                }
                (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                    Some(PyObject::float(*x + *y))
                }
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
            },
            Opcode::BinarySubtract | Opcode::InplaceSubtract => {
                match (&left.payload, &right.payload) {
                    (
                        PyObjectPayload::Int(PyInt::Small(x)),
                        PyObjectPayload::Int(PyInt::Small(y)),
                    ) => match x.checked_sub(*y) {
                        Some(r) => Some(PyObject::int(r)),
                        None => {
                            use num_bigint::BigInt;
                            Some(PyObject::big_int(BigInt::from(*x) - BigInt::from(*y)))
                        }
                    },
                    (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                        Some(PyObject::float(*x - *y))
                    }
                    (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
                        Some(PyObject::float(*x as f64 - *y))
                    }
                    (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
                        Some(PyObject::float(*x - *y as f64))
                    }
                    _ => None,
                }
            }
            Opcode::BinaryMultiply | Opcode::InplaceMultiply => {
                match (&left.payload, &right.payload) {
                    (
                        PyObjectPayload::Int(PyInt::Small(x)),
                        PyObjectPayload::Int(PyInt::Small(y)),
                    ) => match x.checked_mul(*y) {
                        Some(r) => Some(PyObject::int(r)),
                        None => {
                            use num_bigint::BigInt;
                            Some(PyObject::big_int(BigInt::from(*x) * BigInt::from(*y)))
                        }
                    },
                    (PyObjectPayload::Float(x), PyObjectPayload::Float(y)) => {
                        Some(PyObject::float(*x * *y))
                    }
                    (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Float(y)) => {
                        Some(PyObject::float(*x as f64 * *y))
                    }
                    (PyObjectPayload::Float(x), PyObjectPayload::Int(PyInt::Small(y))) => {
                        Some(PyObject::float(*x * *y as f64))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    #[inline(always)]
    pub(crate) fn try_inline_simple_function_one_arg(
        pyfunc: &PyFunction,
        arg: &PyObjectRef,
    ) -> Option<PyObjectRef> {
        if pyfunc.code.arg_count != 1 {
            return None;
        }

        let instrs = &pyfunc.code.instructions;
        match instrs.len() {
            1 => match instrs[0].op {
                Opcode::LoadFastReturnValue if instrs[0].arg == 0 => Some(arg.clone()),
                Opcode::LoadConstReturnValue => {
                    pyfunc.constant_cache.get(instrs[0].arg as usize).cloned()
                }
                _ => None,
            },
            2 if instrs[1].op == Opcode::ReturnValue => match instrs[0].op {
                Opcode::LoadFast if instrs[0].arg == 0 => Some(arg.clone()),
                Opcode::LoadConst => pyfunc.constant_cache.get(instrs[0].arg as usize).cloned(),
                Opcode::LoadFastLoadConstBinaryAdd
                | Opcode::LoadFastLoadConstBinarySub
                | Opcode::LoadFastLoadConstBinaryMul => {
                    let local_idx = (instrs[0].arg >> 16) as usize;
                    let const_idx = (instrs[0].arg & 0xFFFF) as usize;
                    if local_idx != 0 {
                        return None;
                    }
                    let op = match instrs[0].op {
                        Opcode::LoadFastLoadConstBinaryAdd => Opcode::BinaryAdd,
                        Opcode::LoadFastLoadConstBinarySub => Opcode::BinarySubtract,
                        Opcode::LoadFastLoadConstBinaryMul => Opcode::BinaryMultiply,
                        _ => unreachable!(),
                    };
                    let rhs = pyfunc.constant_cache.get(const_idx)?;
                    Self::fast_binary_result(op, arg, rhs)
                }
                Opcode::LoadFastLoadFastBinaryAdd => {
                    let left_idx = (instrs[0].arg >> 16) as usize;
                    let right_idx = (instrs[0].arg & 0xFFFF) as usize;
                    if left_idx == 0 && right_idx == 0 {
                        Self::fast_binary_result(Opcode::BinaryAdd, arg, arg)
                    } else {
                        None
                    }
                }
                _ => None,
            },
            3 if instrs[2].op == Opcode::ReturnValue => match instrs[0].op {
                Opcode::LoadFastLoadConst => {
                    let local_idx = (instrs[0].arg >> 16) as usize;
                    let const_idx = (instrs[0].arg & 0xFFFF) as usize;
                    if local_idx != 0 {
                        return None;
                    }
                    let rhs = pyfunc.constant_cache.get(const_idx)?;
                    Self::fast_binary_result(instrs[1].op, arg, rhs)
                }
                Opcode::LoadFastLoadFast => {
                    let left_idx = (instrs[0].arg >> 16) as usize;
                    let right_idx = (instrs[0].arg & 0xFFFF) as usize;
                    if left_idx == 0 && right_idx == 0 {
                        Self::fast_binary_result(instrs[1].op, arg, arg)
                    } else {
                        None
                    }
                }
                _ => None,
            },
            4 if instrs[3].op == Opcode::ReturnValue
                && instrs[0].op == Opcode::LoadFast
                && instrs[0].arg == 0
                && instrs[1].op == Opcode::LoadConst =>
            {
                let rhs = pyfunc.constant_cache.get(instrs[1].arg as usize)?;
                Self::fast_binary_result(instrs[2].op, arg, rhs)
            }
            _ => None,
        }
    }
}
