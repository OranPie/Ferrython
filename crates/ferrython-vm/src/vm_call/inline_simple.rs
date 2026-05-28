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

    #[inline(always)]
    pub(crate) fn try_inline_simple_function_args(
        pyfunc: &PyFunction,
        args: &[PyObjectRef],
    ) -> Option<PyObjectRef> {
        let arg_count = args.len();
        let instrs = &pyfunc.code.instructions;
        match instrs.len() {
            1 => match instrs[0].op {
                Opcode::LoadFastReturnValue => {
                    let idx = instrs[0].arg as usize;
                    args.get(idx).cloned()
                }
                Opcode::LoadConstReturnValue => {
                    pyfunc.constant_cache.get(instrs[0].arg as usize).cloned()
                }
                _ => None,
            },
            2 => {
                if instrs[1].op == Opcode::ReturnValue {
                    match instrs[0].op {
                        Opcode::LoadFast => {
                            let idx = instrs[0].arg as usize;
                            args.get(idx).cloned()
                        }
                        Opcode::LoadFastLoadFastBinaryAdd => {
                            let left_idx = (instrs[0].arg >> 16) as usize;
                            let right_idx = (instrs[0].arg & 0xFFFF) as usize;
                            if left_idx < arg_count && right_idx < arg_count {
                                Self::fast_binary_result(
                                    Opcode::BinaryAdd,
                                    &args[left_idx],
                                    &args[right_idx],
                                )
                            } else {
                                None
                            }
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            }
            3 => {
                if instrs[0].op == Opcode::LoadFastLoadFast && instrs[2].op == Opcode::ReturnValue {
                    let left_idx = (instrs[0].arg >> 16) as usize;
                    let right_idx = (instrs[0].arg & 0xFFFF) as usize;
                    if left_idx < arg_count && right_idx < arg_count {
                        Self::fast_binary_result(instrs[1].op, &args[left_idx], &args[right_idx])
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    #[inline(always)]
    pub(crate) fn try_inline_closure_return(pyfunc: &PyFunction) -> Option<PyObjectRef> {
        let instrs = &pyfunc.code.instructions;
        if instrs.len() == 2
            && instrs[0].op == Opcode::LoadDeref
            && instrs[1].op == Opcode::ReturnValue
        {
            let cell_idx = instrs[0].arg as usize;
            let n_cell = pyfunc.code.cellvars.len();
            if cell_idx >= n_cell && cell_idx - n_cell < pyfunc.closure.len() {
                let cell = &pyfunc.closure[cell_idx - n_cell];
                unsafe { &*cell.data_ptr() }.clone()
            } else {
                None
            }
        } else if instrs.len() == 1 && instrs[0].op == Opcode::LoadConstReturnValue {
            pyfunc.constant_cache.get(instrs[0].arg as usize).cloned()
        } else {
            None
        }
    }

    #[inline(always)]
    pub(crate) fn try_inline_recursive_base_case(
        instrs: &[ferrython_bytecode::Instruction],
        constants: &[PyObjectRef],
        args: &[PyObjectRef],
    ) -> Option<PyObjectRef> {
        if instrs.len() <= 2 || instrs[0].op != Opcode::LoadFastCompareConstJump {
            return None;
        }
        let packed = instrs[0].arg;
        let cmp_op = packed >> 28;
        let local_idx = ((packed >> 20) & 0xFF) as usize;
        let const_idx = ((packed >> 12) & 0xFF) as usize;
        let arg_ref = args.get(local_idx)?;
        let const_ref = constants.get(const_idx)?;
        let cmp_val = match (&arg_ref.payload, &const_ref.payload) {
            (PyObjectPayload::Int(PyInt::Small(x)), PyObjectPayload::Int(PyInt::Small(y))) => {
                match cmp_op {
                    0 => Some(*x < *y),
                    1 => Some(*x <= *y),
                    2 => Some(*x == *y),
                    3 => Some(*x != *y),
                    4 => Some(*x > *y),
                    5 => Some(*x >= *y),
                    _ => None,
                }
            }
            _ => None,
        }?;
        let ret_ip = if cmp_val {
            1
        } else {
            (packed & 0xFFF) as usize
        };
        inline_return_at(instrs, constants, args, ret_ip)
    }
}

#[inline(always)]
fn inline_return_at(
    instrs: &[ferrython_bytecode::Instruction],
    constants: &[PyObjectRef],
    args: &[PyObjectRef],
    ip: usize,
) -> Option<PyObjectRef> {
    let instr = instrs.get(ip)?;
    match instr.op {
        Opcode::LoadFastReturnValue => {
            let idx = instr.arg as usize;
            Some(args.get(idx).cloned().unwrap_or_else(PyObject::none))
        }
        Opcode::LoadFast if instrs.get(ip + 1).map(|i| i.op) == Some(Opcode::ReturnValue) => {
            let idx = instr.arg as usize;
            Some(args.get(idx).cloned().unwrap_or_else(PyObject::none))
        }
        Opcode::LoadConstReturnValue => constants.get(instr.arg as usize).cloned(),
        _ => None,
    }
}
